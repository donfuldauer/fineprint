// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the mana pool and the payment planner: REQ-CAST-001 (per-unit restrictions and
// riders, CR 106.6, 106.6a; payment, CR 107.4a-c, 107.4h, 118.3a, 118.5, 601.2h; pool emptying,
// CR 500.5, 703.4q). Synthetic sources, neutral tags and rider ids only (R5).

use fineprint_base::encode::encode_to_vec;
use fineprint_base::ids::ObjectId;
use fineprint_dsl::mana::Color;

use super::*;

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn unit(t: ManaType) -> ManaUnit {
    ManaUnit::new(t, o(1))
}

fn pool_of(units: Vec<ManaUnit>) -> ManaPool {
    let mut p = ManaPool::new();
    for u in units {
        p.add(u);
    }
    p
}

fn cost(f: impl FnOnce(&mut ManaComponent)) -> ManaComponent {
    let mut c = ManaComponent::default();
    f(&mut c);
    c
}

fn any() -> SpendPurpose {
    SpendPurpose::new(["spell"])
}

// ---------------------------------------------------------------------------------------------
// Types, units and the pool
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, CR 106.1b: the six mana types, their colors and history identifiers.
#[test]
fn req_cast_001_mana_types() {
    for c in Color::ALL {
        let t = ManaType::from_color(c);
        assert_eq!(t.color(), Some(c));
        assert_eq!(ManaType::ALL.get(t.index()), Some(&t));
    }
    assert_eq!(ManaType::Colorless.color(), None);
    assert_eq!(
        ManaType::ALL.get(ManaType::Colorless.index()),
        Some(&ManaType::Colorless)
    );
    let ids: Vec<&str> = ManaType::ALL.iter().map(|t| t.as_str()).collect();
    assert_eq!(
        ids,
        vec!["white", "blue", "black", "red", "green", "colorless"]
    );
    let tags: Vec<Vec<u8>> = ManaType::ALL.iter().map(encode_to_vec).collect();
    assert_eq!(
        tags,
        vec![vec![0], vec![1], vec![2], vec![3], vec![4], vec![5]]
    );
}

/// REQ-CAST-001, CR 106.4, 500.5, 703.4q: add keeps insertion order; count; empty returns the
/// lost units and leaves the pool empty.
#[test]
fn req_cast_001_pool_add_units_count_empty() {
    let mut p = ManaPool::new();
    assert!(p.is_empty());
    p.add(unit(ManaType::Red));
    p.add(unit(ManaType::Green).snow());
    p.add(unit(ManaType::Red));
    assert_eq!(p.len(), 3);
    assert_eq!(p.count(ManaType::Red), 2);
    assert_eq!(p.count(ManaType::Green), 1);
    assert_eq!(p.count(ManaType::White), 0);
    assert_eq!(p.units().get(1), Some(&unit(ManaType::Green).snow()));
    let lost = p.empty();
    assert_eq!(
        lost,
        vec![
            unit(ManaType::Red),
            unit(ManaType::Green).snow(),
            unit(ManaType::Red)
        ]
    );
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);
    assert!(p.empty().is_empty());
}

/// REQ-CAST-001, CR 106.6, 106.6a: restrictions and riders stay on each unit individually, even
/// for units of the same type and source.
#[test]
fn req_cast_001_per_unit_restrictions_and_riders() {
    let r = ManaRestriction::only_for(["spell:artifact"]);
    let mut p = ManaPool::new();
    p.add(
        unit(ManaType::Blue)
            .restricted(r.clone())
            .with_rider(ManaRider::new("rider_a")),
    );
    p.add(unit(ManaType::Blue));
    p.add(
        unit(ManaType::Blue)
            .restricted(r.clone())
            .with_rider(ManaRider::new("rider_a")),
    );
    let units = p.units();
    assert_eq!(units.len(), 3);
    assert_eq!(units[0].restrictions, vec![r.clone()]);
    assert_eq!(units[0].riders, vec![ManaRider::new("rider_a")]);
    assert!(units[1].restrictions.is_empty());
    assert!(units[1].riders.is_empty());
    assert_eq!(units[2].riders, vec![ManaRider::new("rider_a")]);
    assert_eq!(r.tags().len(), 1);
    // Canonical encodings distinguish restricted and plain units.
    assert_ne!(encode_to_vec(&units[0]), encode_to_vec(&units[1]));
    assert_ne!(
        encode_to_vec(&p),
        encode_to_vec(&pool_of(vec![unit(ManaType::Blue)]))
    );
}

/// REQ-CAST-001, CR 106.6: a unit is allowed iff every restriction has a tag in the purpose;
/// an empty restriction allows nothing; unknown tags match nothing (R12).
#[test]
fn req_cast_001_allowed_for() {
    let artifact = ManaRestriction::only_for(["spell:artifact", "ability"]);
    let creature = ManaRestriction::only_for(["spell:creature"]);
    let u = unit(ManaType::Red).restricted(artifact.clone());
    assert!(u.allowed_for(&SpendPurpose::new(["spell", "spell:artifact"])));
    assert!(u.allowed_for(&SpendPurpose::new(["ability"])));
    assert!(!u.allowed_for(&SpendPurpose::new(["spell"])));
    assert!(!u.allowed_for(&SpendPurpose::new(["SPELL:ARTIFACT"])));
    let both = u.clone().restricted(creature);
    assert!(!both.allowed_for(&SpendPurpose::new(["spell:artifact"])));
    assert!(both.allowed_for(&SpendPurpose::new(["spell:artifact", "spell:creature"])));
    let none = unit(ManaType::Red).restricted(ManaRestriction::only_for([]));
    assert!(!none.allowed_for(&SpendPurpose::new(["spell"])));
    assert!(unit(ManaType::Red).allowed_for(&SpendPurpose::default()));
    assert_eq!(unit(ManaType::Red).source_kind(), "nonsnow");
    assert_eq!(unit(ManaType::Red).snow().source_kind(), "snow");
}

// ---------------------------------------------------------------------------------------------
// plan_payment
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, CR 107.4a: a colored symbol needs exactly that color.
#[test]
fn req_cast_001_plan_color_exact() {
    let p = pool_of(vec![unit(ManaType::Blue), unit(ManaType::Colorless)]);
    assert_eq!(plan_payment(&p, &cost(|c| c.white = 1), &any()), None);
    let plan = plan_payment(&p, &cost(|c| c.blue = 1), &any()).expect("plan");
    assert_eq!(plan.spent, vec![0]);
}

/// REQ-CAST-001, CR 107.4c: {C} needs colorless mana; colored mana can't pay it.
#[test]
fn req_cast_001_plan_colorless_symbol() {
    let p = pool_of(vec![unit(ManaType::Red), unit(ManaType::Green)]);
    assert_eq!(plan_payment(&p, &cost(|c| c.colorless = 1), &any()), None);
    let p = pool_of(vec![unit(ManaType::Red), unit(ManaType::Colorless)]);
    let plan = plan_payment(&p, &cost(|c| c.colorless = 1), &any()).expect("plan");
    assert_eq!(plan.spent, vec![1]);
}

/// REQ-CAST-001, CR 107.4h: {S} needs a unit from a snow source, of any type.
#[test]
fn req_cast_001_plan_snow_symbol() {
    let p = pool_of(vec![unit(ManaType::Red), unit(ManaType::Colorless)]);
    assert_eq!(plan_payment(&p, &cost(|c| c.snow = 1), &any()), None);
    let p = pool_of(vec![unit(ManaType::Red), unit(ManaType::Black).snow()]);
    let plan = plan_payment(&p, &cost(|c| c.snow = 1), &any()).expect("plan");
    assert_eq!(plan.spent, vec![1]);
    // The snow unit must be kept for {S} even when it is the lower index.
    let p = pool_of(vec![unit(ManaType::Red).snow(), unit(ManaType::Red)]);
    let plan = plan_payment(
        &p,
        &cost(|c| {
            c.snow = 1;
            c.red = 1;
        }),
        &any(),
    )
    .expect("plan");
    assert_eq!(plan.spent, vec![0, 1]);
}

/// REQ-CAST-001, CR 107.4b: generic mana is paid by any type.
#[test]
fn req_cast_001_plan_generic_any_type() {
    for t in ManaType::ALL {
        let p = pool_of(vec![unit(t)]);
        let plan = plan_payment(&p, &cost(|c| c.generic = 1), &any()).expect("plan");
        assert_eq!(plan.spent, vec![0]);
    }
}

/// REQ-CAST-001, CR 601.2h: completeness, a flexible unit never starves a symbol, whatever the
/// pool order ({W}{C} from a colorless and a white unit; {1}{W} with W first).
#[test]
fn req_cast_001_plan_complete_in_any_order() {
    let wc = cost(|c| {
        c.white = 1;
        c.colorless = 1;
    });
    for p in [
        pool_of(vec![unit(ManaType::Colorless), unit(ManaType::White)]),
        pool_of(vec![unit(ManaType::White), unit(ManaType::Colorless)]),
    ] {
        assert_eq!(
            plan_payment(&p, &wc, &any()).expect("plan").spent,
            vec![0, 1]
        );
    }
    let one_w = cost(|c| {
        c.white = 1;
        c.generic = 1;
    });
    for p in [
        pool_of(vec![unit(ManaType::White), unit(ManaType::Green)]),
        pool_of(vec![unit(ManaType::Green), unit(ManaType::White)]),
    ] {
        assert_eq!(
            plan_payment(&p, &one_w, &any()).expect("plan").spent,
            vec![0, 1]
        );
    }
    // A snow colored unit is needed for its color while another snow unit pays {S}.
    let p = pool_of(vec![
        unit(ManaType::Green).snow(),
        unit(ManaType::Blue).snow(),
        unit(ManaType::Blue),
    ]);
    let c = cost(|c| {
        c.green = 1;
        c.snow = 1;
        c.blue = 1;
    });
    assert_eq!(
        plan_payment(&p, &c, &any()).expect("plan").spent,
        vec![0, 1, 2]
    );
}

/// REQ-CAST-001, CR 106.6: restricted mana is spent only where allowed (an allowed and a
/// disallowed purpose).
#[test]
fn req_cast_001_plan_restricted_only_where_allowed() {
    let r = ManaRestriction::only_for(["spell:artifact"]);
    let p = pool_of(vec![unit(ManaType::Colorless).restricted(r)]);
    let one = cost(|c| c.generic = 1);
    assert_eq!(plan_payment(&p, &one, &SpendPurpose::new(["spell"])), None);
    let plan =
        plan_payment(&p, &one, &SpendPurpose::new(["spell", "spell:artifact"])).expect("allowed");
    assert_eq!(plan.spent, vec![0]);
}

/// REQ-CAST-001, CR 106.6: every restriction on one unit must be satisfied.
#[test]
fn req_cast_001_plan_several_restrictions() {
    let u = unit(ManaType::Red)
        .restricted(ManaRestriction::only_for(["spell"]))
        .restricted(ManaRestriction::only_for(["x_cost"]));
    let p = pool_of(vec![u]);
    let c = cost(|c| c.red = 1);
    assert_eq!(plan_payment(&p, &c, &SpendPurpose::new(["spell"])), None);
    assert_eq!(plan_payment(&p, &c, &SpendPurpose::new(["x_cost"])), None);
    assert!(plan_payment(&p, &c, &SpendPurpose::new(["spell", "x_cost"])).is_some());
}

/// REQ-CAST-001: the tie-break spends the most restricted allowed units first, then the lowest
/// index; riders of spent units are reported per unit (CR 106.6a).
#[test]
fn req_cast_001_plan_tie_break_and_riders() {
    let r = ManaRestriction::only_for(["spell"]);
    let p = pool_of(vec![
        unit(ManaType::Red),
        unit(ManaType::Red).restricted(r.clone()),
        unit(ManaType::Red)
            .restricted(r.clone())
            .with_rider(ManaRider::new("rider_b")),
        unit(ManaType::Red),
    ]);
    let plan = plan_payment(&p, &cost(|c| c.generic = 2), &any()).expect("plan");
    assert_eq!(plan.spent, vec![1, 2]);
    assert_eq!(
        plan.riders_fired,
        vec![(2, vec![ManaRider::new("rider_b")])]
    );
    let plan = plan_payment(&p, &cost(|c| c.generic = 3), &any()).expect("plan");
    assert_eq!(plan.spent, vec![0, 1, 2]);
    // Disallowed restricted units are skipped: the plain units pay.
    let plan = plan_payment(
        &p,
        &cost(|c| c.generic = 2),
        &SpendPurpose::new(["ability"]),
    )
    .expect("p");
    assert_eq!(plan.spent, vec![0, 3]);
}

/// REQ-CAST-001, CR 601.2h: no partial payment, and {0} is the empty plan (CR 118.5).
#[test]
fn req_cast_001_plan_no_partial_and_zero() {
    let p = pool_of(vec![unit(ManaType::Red), unit(ManaType::Red)]);
    assert_eq!(plan_payment(&p, &cost(|c| c.generic = 3), &any()), None);
    assert_eq!(
        plan_payment(
            &p,
            &cost(|c| {
                c.red = 2;
                c.green = 1;
            }),
            &any()
        ),
        None
    );
    let zero = plan_payment(&p, &ManaComponent::default(), &any()).expect("zero");
    assert_eq!(zero, PaymentPlan::default());
    assert_eq!(
        plan_payment(&ManaPool::new(), &ManaComponent::default(), &any()),
        Some(PaymentPlan::default())
    );
    // Huge amounts never panic.
    assert_eq!(
        plan_payment(&p, &cost(|c| c.generic = u64::MAX), &any()),
        None
    );
    let max = ManaComponent {
        generic: u64::MAX,
        white: u64::MAX,
        blue: u64::MAX,
        black: u64::MAX,
        red: u64::MAX,
        green: u64::MAX,
        colorless: u64::MAX,
        snow: u64::MAX,
    };
    assert_eq!(plan_payment(&p, &max, &any()), None);
}

/// REQ-CAST-001, R3: the same pool gives the same plan ten times, and equal pools built
/// separately give equal plans; a permuted pool still pays with four units and still spends the
/// restricted unit first (tie-break).
#[test]
fn req_cast_001_plan_deterministic() {
    let build = |order: &[usize]| {
        let all = [
            unit(ManaType::Green).snow(),
            unit(ManaType::Red).restricted(ManaRestriction::only_for(["spell"])),
            unit(ManaType::Colorless),
            unit(ManaType::Green),
            unit(ManaType::White).with_rider(ManaRider::new("rider_c")),
        ];
        pool_of(order.iter().map(|&i| all[i].clone()).collect())
    };
    let c = cost(|c| {
        c.green = 1;
        c.snow = 1;
        c.generic = 2;
    });
    let first = plan_payment(&build(&[0, 1, 2, 3, 4]), &c, &any());
    assert!(first.is_some());
    for _ in 0..10 {
        assert_eq!(plan_payment(&build(&[0, 1, 2, 3, 4]), &c, &any()), first);
    }
    let spent_of = |order: &[usize]| {
        let mut p = build(order);
        let plan = plan_payment(&p, &c, &any()).expect("plan");
        spend(&mut p, &plan)
    };
    for order in [
        [0, 1, 2, 3, 4],
        [4, 3, 2, 1, 0],
        [2, 0, 4, 1, 3],
        [1, 3, 0, 4, 2],
    ] {
        let s = spent_of(&order);
        assert_eq!(s.len(), 4);
        assert!(s.iter().any(|u| !u.restrictions.is_empty()));
        assert!(s.iter().any(|u| u.source_is_snow));
    }
}

/// REQ-CAST-001, CR 118.3a, 106.4a: spend removes exactly the planned units and keeps the rest;
/// out-of-range and repeated indices are ignored.
#[test]
fn req_cast_001_spend_exact() {
    let mut p = pool_of(vec![
        unit(ManaType::White),
        unit(ManaType::Blue),
        unit(ManaType::Black),
    ]);
    let plan = plan_payment(
        &p,
        &cost(|c| {
            c.white = 1;
            c.black = 1;
        }),
        &any(),
    )
    .expect("plan");
    let spent = spend(&mut p, &plan);
    assert_eq!(spent, vec![unit(ManaType::White), unit(ManaType::Black)]);
    assert_eq!(p.units(), &[unit(ManaType::Blue)]);
    let odd = PaymentPlan {
        spent: vec![5, 0, 0],
        riders_fired: Vec::new(),
    };
    assert_eq!(spend(&mut p, &odd), vec![unit(ManaType::Blue)]);
    assert!(p.is_empty());
}

/// REQ-CAST-001: payment-method offers and purposes have canonical encodings that distinguish
/// their fields.
#[test]
fn req_cast_001_offer_and_purpose_encoding() {
    let a = PaymentMethodOffer {
        label: "method_a".to_owned(),
        eligible: vec![o(1)],
    };
    let b = PaymentMethodOffer {
        label: "method_a".to_owned(),
        eligible: vec![o(2)],
    };
    assert_ne!(encode_to_vec(&a), encode_to_vec(&b));
    assert_ne!(
        encode_to_vec(&SpendPurpose::new(["spell"])),
        encode_to_vec(&SpendPurpose::new(["ability"]))
    );
    assert_ne!(
        encode_to_vec(&ManaRestriction::only_for(["spell"])),
        encode_to_vec(&ManaRestriction::only_for([]))
    );
}

// ---------------------------------------------------------------------------------------------
// Property tests (REQ-CAST-001)
// ---------------------------------------------------------------------------------------------

/// A small deterministic generator (LCG, literal seeds only).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

const TAGS: [&str; 3] = ["spell", "ability", "spell:artifact"];

fn random_unit(g: &mut Lcg) -> ManaUnit {
    let t = ManaType::ALL[usize::try_from(g.below(6)).unwrap()];
    let mut u = ManaUnit::new(t, o(g.below(4)));
    if g.below(3) == 0 {
        u = u.snow();
    }
    for _ in 0..g.below(3) {
        let a = TAGS[usize::try_from(g.below(3)).unwrap()];
        let b = TAGS[usize::try_from(g.below(3)).unwrap()];
        u = u.restricted(ManaRestriction::only_for([a, b]));
    }
    if g.below(4) == 0 {
        u = u.with_rider(ManaRider::new("rider_p"));
    }
    u
}

fn random_cost(g: &mut Lcg) -> ManaComponent {
    ManaComponent {
        generic: g.below(3),
        white: g.below(2),
        blue: g.below(2),
        black: 0,
        red: g.below(2),
        green: g.below(2),
        colorless: g.below(2),
        snow: g.below(2),
    }
}

fn random_purpose(g: &mut Lcg) -> SpendPurpose {
    let n = usize::try_from(g.below(3)).unwrap();
    SpendPurpose::new(TAGS.iter().copied().skip(n))
}

/// Demand slots as in the planner: W U B R G C, S, generic.
fn demand(c: &ManaComponent) -> [u64; 8] {
    [
        c.white,
        c.blue,
        c.black,
        c.red,
        c.green,
        c.colorless,
        c.snow,
        c.generic,
    ]
}

/// Brute force: can `units` pay `d` (each unit used at most once; `exact`: every unit used)?
fn brute(units: &[&ManaUnit], d: &mut [u64; 8], exact: bool) -> bool {
    let Some((first, rest)) = units.split_first() else {
        return d.iter().all(|x| *x == 0);
    };
    if !exact && brute(rest, d, exact) {
        return true;
    }
    let mut slots = vec![first.mana_type.index(), 7];
    if first.source_is_snow {
        slots.push(6);
    }
    for s in slots {
        if d[s] > 0 {
            d[s] -= 1;
            let ok = brute(rest, d, exact);
            d[s] += 1;
            if ok {
                return true;
            }
        }
    }
    false
}

/// REQ-CAST-001, CR 106.6, 601.2h, 118.3a: on seeded random pools, the plan is Some iff a
/// brute-force assignment exists; it never spends a disallowed unit; its units pay the cost
/// exactly; spend removes exactly the planned units.
#[test]
fn property_plan_matches_brute_force() {
    let mut g = Lcg(0x5eed_0054);
    let mut found = 0;
    for _ in 0..3000 {
        let n = g.below(7);
        let pool = pool_of((0..n).map(|_| random_unit(&mut g)).collect());
        let c = random_cost(&mut g);
        let purpose = random_purpose(&mut g);
        let allowed: Vec<&ManaUnit> = pool
            .units()
            .iter()
            .filter(|u| u.allowed_for(&purpose))
            .collect();
        let exists = brute(&allowed, &mut demand(&c), false);
        let plan = plan_payment(&pool, &c, &purpose);
        assert_eq!(plan.is_some(), exists, "pool {pool:?} cost {c:?}");
        let Some(plan) = plan else { continue };
        found += 1;
        // Ascending, distinct, in range, allowed.
        assert!(plan.spent.windows(2).all(|w| w[0] < w[1]));
        let spent_units: Vec<&ManaUnit> = plan
            .spent
            .iter()
            .map(|&i| pool.units().get(i).expect("in range"))
            .collect();
        assert!(spent_units.iter().all(|u| u.allowed_for(&purpose)));
        assert_eq!(u64::try_from(spent_units.len()).unwrap(), c.total());
        assert!(brute(&spent_units, &mut demand(&c), true));
        // Riders: exactly the spent units with riders.
        let expected: Vec<(usize, Vec<ManaRider>)> = plan
            .spent
            .iter()
            .filter(|&&i| !pool.units()[i].riders.is_empty())
            .map(|&i| (i, pool.units()[i].riders.clone()))
            .collect();
        assert_eq!(plan.riders_fired, expected);
        // spend removes exactly those units.
        let mut after = pool.clone();
        let removed = spend(&mut after, &plan);
        let owned: Vec<ManaUnit> = spent_units.into_iter().cloned().collect();
        assert_eq!(removed, owned);
        let kept: Vec<ManaUnit> = pool
            .units()
            .iter()
            .enumerate()
            .filter(|(i, _)| !plan.spent.contains(i))
            .map(|(_, u)| u.clone())
            .collect();
        assert_eq!(after.units(), kept.as_slice());
    }
    // The run exercises both outcomes.
    assert!(found > 100, "only {found} payable cases");
}

/// REQ-CAST-001, R3: the plan is a pure function of its inputs (repeated runs agree).
#[test]
fn property_plan_repeatable() {
    let mut g = Lcg(0x0054_0001);
    for _ in 0..500 {
        let n = g.below(9);
        let pool = pool_of((0..n).map(|_| random_unit(&mut g)).collect());
        let c = random_cost(&mut g);
        let purpose = random_purpose(&mut g);
        let first = plan_payment(&pool, &c, &purpose);
        assert_eq!(plan_payment(&pool.clone(), &c, &purpose), first);
    }
}
