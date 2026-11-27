// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the mana pool, mana units with restrictions and riders, `plan_payment` and
//! `spend` (REQ-CAST-001 steps 7-8; CR 106.3, 106.4, 106.6, 106.6a, 107.4a-c, 107.4h, 118.3,
//! 118.3a, 118.5, 500.5, 601.2h, 703.4q). Black-box: written from the work-item brief only.
//! Every name is invented and neutral (R5). Randomness comes only from a seeded generator
//! written here (R3).

use std::collections::BTreeSet;

use fineprint_base::ids::ObjectId;
use fineprint_core::casting::ManaComponent;
use fineprint_core::mana::{
    ManaPool, ManaRestriction, ManaRider, ManaType, ManaUnit, PaymentPlan, SpendPurpose,
    plan_payment, spend,
};
use fineprint_dsl::mana::Color;

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

const ALL_TYPES: [ManaType; 6] = [
    ManaType::White,
    ManaType::Blue,
    ManaType::Black,
    ManaType::Red,
    ManaType::Green,
    ManaType::Colorless,
];

fn src(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn unit(t: ManaType) -> ManaUnit {
    ManaUnit::new(t, src(1))
}

fn pool_of(units: &[ManaUnit]) -> ManaPool {
    let mut p = ManaPool::default();
    for u in units {
        p.add(u.clone());
    }
    p
}

fn cost(f: impl FnOnce(&mut ManaComponent)) -> ManaComponent {
    let mut m = ManaComponent::default();
    f(&mut m);
    m
}

fn any_purpose() -> SpendPurpose {
    SpendPurpose::new(Vec::<&str>::new())
}

fn purpose(tags: &[&str]) -> SpendPurpose {
    SpendPurpose::new(tags.iter().copied())
}

fn rider(id: &str) -> ManaRider {
    ManaRider { id: id.to_owned() }
}

/// The units the plan spends, in plan order.
fn planned_units(pool: &ManaPool, plan: &PaymentPlan) -> Vec<ManaUnit> {
    plan.spent
        .iter()
        .map(|&i| pool.units()[i].clone())
        .collect()
}

/// One cost symbol, for the brute-force reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sym {
    Colored(ManaType),
    Colorless,
    Snow,
    Generic,
}

fn symbols(c: &ManaComponent) -> Vec<Sym> {
    let mut v = Vec::new();
    let colored = [
        (c.white, ManaType::White),
        (c.blue, ManaType::Blue),
        (c.black, ManaType::Black),
        (c.red, ManaType::Red),
        (c.green, ManaType::Green),
    ];
    for (n, t) in colored {
        for _ in 0..n {
            v.push(Sym::Colored(t));
        }
    }
    for _ in 0..c.colorless {
        v.push(Sym::Colorless);
    }
    for _ in 0..c.snow {
        v.push(Sym::Snow);
    }
    for _ in 0..c.generic {
        v.push(Sym::Generic);
    }
    v
}

/// CR 107.4a-c, 107.4h: whether `u` can pay symbol `s`.
fn pays(u: &ManaUnit, s: Sym) -> bool {
    match s {
        Sym::Colored(t) => u.mana_type == t,
        Sym::Colorless => u.mana_type == ManaType::Colorless,
        Sym::Snow => u.source_is_snow,
        Sym::Generic => true,
    }
}

/// The brief's restriction semantics, written independently: a unit is allowed iff each of its
/// restrictions shares a tag with the purpose. The tags of a restriction are not public, so the
/// reference takes them from the generator's record (`tags_of`).
fn allowed_ref(tags_of: &[Vec<Vec<String>>], i: usize, p: &BTreeSet<String>) -> bool {
    tags_of[i]
        .iter()
        .all(|restriction| restriction.iter().any(|t| p.contains(t)))
}

/// Brute force: can the symbols be assigned to distinct units among `candidates`?
fn assignable(units: &[ManaUnit], candidates: &[usize], syms: &[Sym]) -> bool {
    fn go(units: &[ManaUnit], cands: &[usize], syms: &[Sym], used: &mut Vec<bool>) -> bool {
        let Some((&s, rest)) = syms.split_first() else {
            return true;
        };
        for (k, &i) in cands.iter().enumerate() {
            if !used[k] && pays(&units[i], s) {
                used[k] = true;
                if go(units, cands, rest, used) {
                    return true;
                }
                used[k] = false;
            }
        }
        false
    }
    if syms.len() > candidates.len() {
        return false;
    }
    let mut used = vec![false; candidates.len()];
    go(units, candidates, syms, &mut used)
}

/// A seeded xorshift generator (R3: literal seeds only).
struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const TAGS: [&str; 3] = ["spell", "ability", "test.tag_x"];

/// A random pool of at most 6 units, with the restriction tags of each unit recorded.
fn random_pool(g: &mut Gen) -> (ManaPool, Vec<Vec<Vec<String>>>) {
    let n = g.below(7) as usize;
    let mut pool = ManaPool::default();
    let mut tags_of = Vec::new();
    for k in 0..n {
        let t = ALL_TYPES[g.below(6) as usize];
        let mut u = ManaUnit::new(t, src(1 + g.below(3)));
        if g.below(3) == 0 {
            u = u.snow();
        }
        let mut rs = Vec::new();
        for _ in 0..g.below(3) {
            if g.below(2) == 0 {
                continue;
            }
            let tags: Vec<String> = TAGS
                .iter()
                .filter(|_| g.below(2) == 0)
                .map(|s| (*s).to_owned())
                .collect();
            u = u.restricted(ManaRestriction::only_for(tags.iter().map(String::as_str)));
            rs.push(tags);
        }
        if g.below(3) == 0 {
            u = u.with_rider(rider(&format!("test.rider_{k}")));
        }
        pool.add(u);
        tags_of.push(rs);
    }
    (pool, tags_of)
}

fn random_cost(g: &mut Gen) -> ManaComponent {
    let mut c = ManaComponent::default();
    for _ in 0..g.below(6) {
        match g.below(9) {
            0 => c.white += 1,
            1 => c.blue += 1,
            2 => c.black += 1,
            3 => c.red += 1,
            4 => c.green += 1,
            5 => c.colorless += 1,
            6 => c.snow += 1,
            _ => c.generic += 1,
        }
    }
    c
}

fn random_purpose(g: &mut Gen) -> BTreeSet<String> {
    TAGS.iter()
        .filter(|_| g.below(2) == 0)
        .map(|s| (*s).to_owned())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// A. The pool
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: units are kept in insertion order and counted by type (CR 106.4).
#[test]
fn req_cast_001_pool_add_keeps_order_and_counts() {
    // CR 106.4
    let a = ManaUnit::new(ManaType::Red, src(1));
    let b = ManaUnit::new(ManaType::Green, src(2));
    let c = ManaUnit::new(ManaType::Red, src(3)).snow();
    let mut p = ManaPool::default();
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);
    p.add(a.clone());
    p.add(b.clone());
    p.add(c.clone());
    assert_eq!(p.units(), &[a, b, c]);
    assert_eq!(p.len(), 3);
    assert!(!p.is_empty());
    assert_eq!(p.count(ManaType::Red), 2);
    assert_eq!(p.count(ManaType::Green), 1);
    assert_eq!(p.count(ManaType::Blue), 0);
    assert_eq!(p.count(ManaType::Colorless), 0);
}

/// REQ-CAST-001: `empty()` returns every unit (lost) and leaves the pool empty (CR 500.5,
/// 703.4q, 106.4).
#[test]
fn req_cast_001_pool_empty_returns_every_unit() {
    // CR 500.5, CR 703.4q, CR 106.4
    let units = vec![
        ManaUnit::new(ManaType::White, src(1)),
        ManaUnit::new(ManaType::Colorless, src(2)).with_rider(rider("test.rider_a")),
        ManaUnit::new(ManaType::Black, src(3)).restricted(ManaRestriction::only_for(["spell"])),
    ];
    let mut p = pool_of(&units);
    let lost = p.empty();
    assert_eq!(lost, units);
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);
    assert_eq!(p, ManaPool::default());
    assert!(p.empty().is_empty(), "emptying an empty pool loses nothing");
}

/// REQ-CAST-001: each unit keeps its own restrictions and riders, even when type and source are
/// equal (CR 106.6, 106.6a).
#[test]
fn req_cast_001_units_keep_their_own_restrictions_and_riders() {
    // CR 106.6, CR 106.6a
    let plain = ManaUnit::new(ManaType::Green, src(7));
    let restricted =
        ManaUnit::new(ManaType::Green, src(7)).restricted(ManaRestriction::only_for(["ability"]));
    let ridden = ManaUnit::new(ManaType::Green, src(7)).with_rider(rider("test.rider_a"));
    assert_ne!(plain, restricted);
    assert_ne!(plain, ridden);
    let p = pool_of(&[plain.clone(), restricted.clone(), ridden.clone()]);
    assert_eq!(p.units()[0].restrictions.len(), 0);
    assert_eq!(p.units()[0].riders.len(), 0);
    assert_eq!(p.units()[1].restrictions.len(), 1);
    assert!(p.units()[1].riders.is_empty());
    assert!(p.units()[2].restrictions.is_empty());
    assert_eq!(p.units()[2].riders, vec![rider("test.rider_a")]);
    assert_eq!(p.count(ManaType::Green), 3);
    // Builders set the public fields.
    let s = ManaUnit::new(ManaType::Blue, src(9)).snow();
    assert!(s.source_is_snow);
    assert_eq!(s.source, src(9));
    assert_eq!(s.mana_type, ManaType::Blue);
    assert!(!ManaUnit::new(ManaType::Blue, src(9)).source_is_snow);
}

/// REQ-CAST-001: `ManaType::from_color` maps each color to its mana type (CR 106.3, 107.4a).
#[test]
fn req_cast_001_mana_type_from_color() {
    // CR 106.3, CR 107.4a
    assert_eq!(ManaType::from_color(Color::White), ManaType::White);
    assert_eq!(ManaType::from_color(Color::Blue), ManaType::Blue);
    assert_eq!(ManaType::from_color(Color::Black), ManaType::Black);
    assert_eq!(ManaType::from_color(Color::Red), ManaType::Red);
    assert_eq!(ManaType::from_color(Color::Green), ManaType::Green);
}

// ---------------------------------------------------------------------------------------------
// B. plan_payment
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: a colored symbol needs exactly that color (CR 107.4a).
#[test]
fn req_cast_001_colored_symbol_needs_exact_color() {
    // CR 107.4a
    let c = cost(|m| m.red = 1);
    for t in ALL_TYPES {
        let p = pool_of(&[unit(t)]);
        let plan = plan_payment(&p, &c, &any_purpose());
        assert_eq!(plan.is_some(), t == ManaType::Red, "{t:?}");
        if let Some(plan) = plan {
            assert_eq!(plan.spent, vec![0]);
        }
    }
}

/// REQ-CAST-001: `{C}` needs colorless mana only (CR 107.4c).
#[test]
fn req_cast_001_colorless_symbol_needs_colorless() {
    // CR 107.4c
    let c = cost(|m| m.colorless = 1);
    for t in ALL_TYPES {
        let p = pool_of(&[unit(t), unit(t).snow()]);
        let plan = plan_payment(&p, &c, &any_purpose());
        assert_eq!(plan.is_some(), t == ManaType::Colorless, "{t:?}");
    }
}

/// REQ-CAST-001: `{S}` needs mana from a snow source, of any type; a colorless non-snow unit
/// does not pay it (CR 107.4h, 106.3).
#[test]
fn req_cast_001_snow_symbol_needs_snow_source() {
    // CR 107.4h, CR 106.3
    let c = cost(|m| m.snow = 1);
    let plain = pool_of(&[unit(ManaType::Colorless), unit(ManaType::Red)]);
    assert!(plan_payment(&plain, &c, &any_purpose()).is_none());
    for t in ALL_TYPES {
        let p = pool_of(&[unit(ManaType::Colorless), unit(t).snow()]);
        let plan = plan_payment(&p, &c, &any_purpose()).expect("a snow unit pays {S}");
        assert_eq!(plan.spent, vec![1], "{t:?}");
    }
}

/// REQ-CAST-001: generic mana is paid with any type (CR 107.4b).
#[test]
fn req_cast_001_generic_accepts_any_type() {
    // CR 107.4b
    let c = cost(|m| m.generic = 1);
    for t in ALL_TYPES {
        let p = pool_of(&[unit(t)]);
        assert!(plan_payment(&p, &c, &any_purpose()).is_some(), "{t:?}");
        let s = pool_of(&[unit(t).snow()]);
        assert!(plan_payment(&s, &c, &any_purpose()).is_some(), "{t:?} snow");
    }
}

/// REQ-CAST-001: the planner is complete: a greedy spend of a flexible unit must not block a
/// strict symbol, in either pool order (CR 601.2h, 107.4a-c, 107.4h).
#[test]
fn req_cast_001_planner_is_complete_against_greedy_traps() {
    // CR 601.2h, CR 107.4a, CR 107.4b, CR 107.4c, CR 107.4h
    struct Case {
        cost: ManaComponent,
        units: Vec<ManaUnit>,
    }
    let cases = vec![
        // {W}{C} with one white and one colorless unit.
        Case {
            cost: cost(|m| {
                m.white = 1;
                m.colorless = 1;
            }),
            units: vec![unit(ManaType::White), unit(ManaType::Colorless)],
        },
        // {1}{C}: generic must not take the only colorless unit.
        Case {
            cost: cost(|m| {
                m.generic = 1;
                m.colorless = 1;
            }),
            units: vec![unit(ManaType::Colorless), unit(ManaType::Green)],
        },
        // {W}{W}{1}.
        Case {
            cost: cost(|m| {
                m.white = 2;
                m.generic = 1;
            }),
            units: vec![
                unit(ManaType::White),
                unit(ManaType::Blue),
                unit(ManaType::White),
            ],
        },
        // {S}{R}: the snow unit is red, the other red is not snow.
        Case {
            cost: cost(|m| {
                m.snow = 1;
                m.red = 1;
            }),
            units: vec![unit(ManaType::Red).snow(), unit(ManaType::Red)],
        },
        // {S}{1}: generic must not take the only snow unit.
        Case {
            cost: cost(|m| {
                m.snow = 1;
                m.generic = 1;
            }),
            units: vec![unit(ManaType::Blue).snow(), unit(ManaType::Blue)],
        },
        // {S}{C}: snow colorless and plain colorless.
        Case {
            cost: cost(|m| {
                m.snow = 1;
                m.colorless = 1;
            }),
            units: vec![
                unit(ManaType::Colorless).snow(),
                unit(ManaType::Green).snow(),
                unit(ManaType::Colorless),
            ],
        },
    ];
    for case in cases {
        for order in [false, true] {
            let mut units = case.units.clone();
            if order {
                units.reverse();
            }
            let p = pool_of(&units);
            let plan = plan_payment(&p, &case.cost, &any_purpose())
                .unwrap_or_else(|| panic!("payable: {:?} with {units:?}", case.cost));
            let idx: Vec<usize> = (0..units.len()).collect();
            let spent = planned_units(&p, &plan);
            assert_eq!(spent.len() as u64, case.cost.total());
            assert!(
                assignable(&units, &plan.spent, &symbols(&case.cost)),
                "plan {:?} does not pay {:?}",
                plan.spent,
                case.cost
            );
            assert!(assignable(&units, &idx, &symbols(&case.cost)));
        }
    }
}

/// REQ-CAST-001: one short means no plan; no partial payment (CR 601.2h, 118.3).
#[test]
fn req_cast_001_short_by_one_is_none() {
    // CR 601.2h "Partial payments are not allowed.", CR 118.3
    let c = cost(|m| {
        m.red = 2;
        m.generic = 1;
    });
    let short = pool_of(&[unit(ManaType::Red), unit(ManaType::Red)]);
    assert!(plan_payment(&short, &c, &any_purpose()).is_none());
    let wrong = pool_of(&[
        unit(ManaType::Red),
        unit(ManaType::Green),
        unit(ManaType::Green),
    ]);
    assert!(plan_payment(&wrong, &c, &any_purpose()).is_none());
    let enough = pool_of(&[
        unit(ManaType::Red),
        unit(ManaType::Green),
        unit(ManaType::Red),
    ]);
    let plan = plan_payment(&enough, &c, &any_purpose()).expect("payable");
    assert_eq!(plan.spent, vec![0, 1, 2]);
    assert!(plan_payment(&ManaPool::default(), &c, &any_purpose()).is_none());
}

/// REQ-CAST-001: `{0}` is paid with an empty plan, even from an empty pool (CR 118.3a, 118.5).
#[test]
fn req_cast_001_zero_cost_empty_plan() {
    // CR 118.3a "Players can always pay 0 mana.", CR 118.5
    let zero = ManaComponent::default();
    let plan = plan_payment(&ManaPool::default(), &zero, &any_purpose()).expect("{0} payable");
    assert!(plan.spent.is_empty());
    assert!(plan.riders_fired.is_empty());
    let p = pool_of(&[unit(ManaType::Red).with_rider(rider("test.rider_a"))]);
    let plan = plan_payment(&p, &zero, &any_purpose()).expect("{0} payable");
    assert!(plan.spent.is_empty());
    assert!(plan.riders_fired.is_empty());
}

/// REQ-CAST-001: restricted mana is spendable only for a matching purpose; the same pool pays
/// under an allowed purpose and fails under a disallowed one (CR 106.6).
#[test]
fn req_cast_001_restricted_mana_only_where_allowed() {
    // CR 106.6
    let c = cost(|m| m.generic = 1);
    let p = pool_of(&[unit(ManaType::Red).restricted(ManaRestriction::only_for(["ability"]))]);
    assert!(plan_payment(&p, &c, &purpose(&["spell"])).is_none());
    assert!(plan_payment(&p, &c, &any_purpose()).is_none());
    assert!(plan_payment(&p, &c, &purpose(&["ability"])).is_some());
    assert!(plan_payment(&p, &c, &purpose(&["spell", "ability"])).is_some());
    // An unrestricted unit is allowed everywhere; the restricted one stays unused under "spell".
    let both = pool_of(&[
        unit(ManaType::Red).restricted(ManaRestriction::only_for(["ability"])),
        unit(ManaType::Red),
    ]);
    let plan = plan_payment(&both, &cost(|m| m.red = 1), &purpose(&["spell"])).expect("payable");
    assert_eq!(plan.spent, vec![1]);
    assert!(plan_payment(&both, &cost(|m| m.red = 2), &purpose(&["spell"])).is_none());
    assert!(plan_payment(&both, &cost(|m| m.red = 2), &purpose(&["ability"])).is_some());
}

/// REQ-CAST-001: a unit with two restrictions needs both satisfied (CR 106.6).
#[test]
fn req_cast_001_two_restrictions_both_required() {
    // CR 106.6
    let u = unit(ManaType::Green)
        .restricted(ManaRestriction::only_for(["spell"]))
        .restricted(ManaRestriction::only_for(["test.tag_x"]));
    let p = pool_of(&[u]);
    let c = cost(|m| m.green = 1);
    assert!(plan_payment(&p, &c, &purpose(&["spell"])).is_none());
    assert!(plan_payment(&p, &c, &purpose(&["test.tag_x"])).is_none());
    assert!(plan_payment(&p, &c, &purpose(&["spell", "test.tag_x"])).is_some());
    // A restriction with several tags needs only one of them.
    let multi = pool_of(&[
        unit(ManaType::Green).restricted(ManaRestriction::only_for(["spell", "ability"]))
    ]);
    assert!(plan_payment(&multi, &c, &purpose(&["ability"])).is_some());
    assert!(plan_payment(&multi, &c, &purpose(&["test.tag_x"])).is_none());
}

/// REQ-CAST-001: riders are reported only for spent units and once per spent unit (CR 106.6,
/// 106.6a).
#[test]
fn req_cast_001_riders_fire_only_for_spent_units() {
    // CR 106.6, CR 106.6a
    let p = pool_of(&[
        unit(ManaType::Blue).with_rider(rider("test.rider_unspent")),
        unit(ManaType::Red)
            .with_rider(rider("test.rider_a"))
            .with_rider(rider("test.rider_b")),
        unit(ManaType::Red).with_rider(rider("test.rider_a")),
        unit(ManaType::Green),
    ]);
    let plan = plan_payment(
        &p,
        &cost(|m| {
            m.red = 2;
            m.green = 1;
        }),
        &any_purpose(),
    )
    .expect("payable");
    assert_eq!(plan.spent, vec![1, 2, 3]);
    let mut seen = BTreeSet::new();
    for (i, riders) in &plan.riders_fired {
        assert!(plan.spent.contains(i), "rider of unspent unit {i}");
        assert!(seen.insert(*i), "unit {i} reported twice");
        assert_eq!(riders, &p.units()[*i].riders);
    }
    for &i in &plan.spent {
        if !p.units()[i].riders.is_empty() {
            assert!(seen.contains(&i), "rider of spent unit {i} missing");
        }
    }
    assert!(!seen.contains(&0));
    let all: Vec<&ManaRider> = plan.riders_fired.iter().flat_map(|(_, r)| r).collect();
    assert_eq!(
        all,
        vec![
            &rider("test.rider_a"),
            &rider("test.rider_b"),
            &rider("test.rider_a")
        ]
    );
}

/// REQ-CAST-001: `spend` removes exactly the planned units, returns them, and leaves the excess
/// in the pool in order (CR 118.3a).
#[test]
fn req_cast_001_spend_leaves_excess() {
    // CR 118.3a
    let units = vec![
        unit(ManaType::Green),
        unit(ManaType::Red),
        unit(ManaType::White),
        unit(ManaType::Red).with_rider(rider("test.rider_a")),
    ];
    let mut p = pool_of(&units);
    let plan = plan_payment(&p, &cost(|m| m.red = 2), &any_purpose()).expect("payable");
    assert_eq!(plan.spent, vec![1, 3]);
    let spent = spend(&mut p, &plan);
    assert_eq!(spent, vec![units[1].clone(), units[3].clone()]);
    assert_eq!(p.units(), &[units[0].clone(), units[2].clone()]);
    // Spending an empty plan changes nothing.
    let before = p.clone();
    let zero = plan_payment(&p, &ManaComponent::default(), &any_purpose()).expect("{0}");
    assert!(spend(&mut p, &zero).is_empty());
    assert_eq!(p, before);
}

/// REQ-CAST-001: planning is deterministic: repeated calls and equal pools built in the same
/// order give the same plan (R3; CR 601.2h).
#[test]
fn req_cast_001_plan_is_deterministic() {
    // CR 601.2h
    let build = || {
        pool_of(&[
            unit(ManaType::Red),
            unit(ManaType::Green).snow(),
            unit(ManaType::Red).with_rider(rider("test.rider_a")),
            unit(ManaType::Colorless),
            unit(ManaType::Blue).restricted(ManaRestriction::only_for(["spell"])),
        ])
    };
    let c = cost(|m| {
        m.red = 1;
        m.generic = 2;
    });
    let first = plan_payment(&build(), &c, &purpose(&["spell"])).expect("payable");
    for _ in 0..10 {
        let again = plan_payment(&build(), &c, &purpose(&["spell"])).expect("payable");
        assert_eq!(again.spent, first.spent);
        assert_eq!(again.riders_fired, first.riders_fired);
    }
}

/// REQ-CAST-001 (R12): empty, very long and unknown tags and rider ids never panic; unknown
/// tags match nothing; a restriction with no tags allows nothing (CR 106.6).
#[test]
fn req_cast_001_untrusted_tags_and_riders() {
    // CR 106.6
    let long = "t".repeat(100_000);
    let c = cost(|m| m.generic = 1);
    let empty_tag = pool_of(&[unit(ManaType::Red).restricted(ManaRestriction::only_for([""]))]);
    assert!(plan_payment(&empty_tag, &c, &purpose(&["spell"])).is_none());
    assert!(plan_payment(&empty_tag, &c, &purpose(&[""])).is_some());
    let long_tag =
        pool_of(&[unit(ManaType::Red).restricted(ManaRestriction::only_for([long.as_str()]))]);
    assert!(plan_payment(&long_tag, &c, &purpose(&["spell", "ability"])).is_none());
    assert!(plan_payment(&long_tag, &c, &purpose(&[long.as_str()])).is_some());
    let no_tags =
        pool_of(&[unit(ManaType::Red).restricted(ManaRestriction::only_for(Vec::<&str>::new()))]);
    assert!(plan_payment(&no_tags, &c, &purpose(&["spell", "ability", ""])).is_none());
    // Unknown purpose tags do not unlock restricted mana.
    let restricted =
        pool_of(&[unit(ManaType::Red).restricted(ManaRestriction::only_for(["ability"]))]);
    assert!(plan_payment(&restricted, &c, &purpose(&["test.unknown", long.as_str()])).is_none());
    // Odd rider ids are carried through.
    let mut ridden = pool_of(&[
        unit(ManaType::Red).with_rider(rider("")),
        unit(ManaType::Red).with_rider(rider(&long)),
    ]);
    let plan = plan_payment(&ridden, &cost(|m| m.red = 2), &any_purpose()).expect("payable");
    let fired: Vec<&ManaRider> = plan.riders_fired.iter().flat_map(|(_, r)| r).collect();
    assert_eq!(fired, vec![&rider(""), &rider(&long)]);
    assert_eq!(spend(&mut ridden, &plan).len(), 2);
    assert!(ridden.is_empty());
}

// ---------------------------------------------------------------------------------------------
// C. Properties (seeded generator, R3)
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: on random pools of at most 6 units, `plan_payment` is Some iff a brute-force
/// search finds an assignment; a plan uses only allowed units, each at most once, and exactly
/// pays the cost; riders are reported only for spent units; `spend` removes exactly the plan
/// (CR 601.2h, 106.6, 106.6a, 107.4a-c, 107.4h, 118.3a).
#[test]
fn req_cast_001_property_plan_matches_brute_force() {
    // CR 601.2h, CR 106.6, CR 106.6a, CR 107.4a, CR 107.4b, CR 107.4c, CR 107.4h, CR 118.3a
    let mut g = Gen(0x005E_ED00_52A1_1CE5_u64);
    let mut somes = 0u32;
    let mut nones = 0u32;
    for _ in 0..4000 {
        let (pool, tags_of) = random_pool(&mut g);
        let c = random_cost(&mut g);
        let ptags = random_purpose(&mut g);
        let p = SpendPurpose::new(ptags.iter().map(String::as_str));
        assert_eq!(p.tags, ptags);
        let units = pool.units().to_vec();
        let allowed: Vec<usize> = (0..units.len())
            .filter(|&i| allowed_ref(&tags_of, i, &ptags))
            .collect();
        let syms = symbols(&c);
        let expected = assignable(&units, &allowed, &syms);
        let plan = plan_payment(&pool, &c, &p);
        // (2) completeness and soundness against brute force.
        assert_eq!(
            plan.is_some(),
            expected,
            "cost {c:?} pool {units:?} purpose {ptags:?}"
        );
        let Some(plan) = plan else {
            nones += 1;
            continue;
        };
        somes += 1;
        // (4) each unit at most once, ascending, in range.
        assert!(
            plan.spent.windows(2).all(|w| w[0] < w[1]),
            "{:?}",
            plan.spent
        );
        assert!(plan.spent.iter().all(|&i| i < units.len()));
        // (1) never a disallowed unit.
        for &i in &plan.spent {
            assert!(allowed.contains(&i), "disallowed unit {i} used");
        }
        // The plan pays exactly the cost.
        assert_eq!(plan.spent.len() as u64, c.total());
        assert!(assignable(&units, &plan.spent, &syms));
        // Riders only for spent units, once each, with the unit's own riders.
        let mut seen = BTreeSet::new();
        for (i, r) in &plan.riders_fired {
            assert!(plan.spent.contains(i));
            assert!(seen.insert(*i));
            assert_eq!(r, &units[*i].riders);
        }
        for &i in &plan.spent {
            if !units[i].riders.is_empty() {
                assert!(seen.contains(&i));
            }
        }
        // (3) spend removes exactly the plan; the rest stays in order.
        let mut after = pool.clone();
        let spent = spend(&mut after, &plan);
        assert_eq!(after.len(), pool.len() - plan.spent.len());
        let kept: Vec<ManaUnit> = (0..units.len())
            .filter(|i| !plan.spent.contains(i))
            .map(|i| units[i].clone())
            .collect();
        assert_eq!(after.units(), kept.as_slice());
        let expected_spent: Vec<ManaUnit> = plan.spent.iter().map(|&i| units[i].clone()).collect();
        assert_eq!(spent, expected_spent);
        // Determinism.
        let again = plan_payment(&pool, &c, &p).expect("same answer");
        assert_eq!(again.spent, plan.spent);
        assert_eq!(again.riders_fired, plan.riders_fired);
    }
    assert!(
        somes > 200 && nones > 200,
        "coverage: {somes} some, {nones} none"
    );
}
