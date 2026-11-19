// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for the replacement bus over generated effect sets on Damage and ZoneChange
//! events (REQ-EVENT-001; CR 614.5, 615, 616.1, 616.1f). A fixed LCG seeded per case drives
//! the generator (R3: no system randomness, no new dependencies). Black-box: written from the
//! work-item brief only.

#[path = "spec_replacement_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::{EffectId, ObjectId, PlayerId};
use fineprint_core::events::{GameEvent, ZoneChangeCause};
use fineprint_core::replacement::{
    Action, AmountOp, BusEventKind, EventMatcher, Modification, ReplacementEffect, Shield, Tier,
};
use fineprint_core::zones::ZoneKind;
use support::*;

const CASES: u64 = 250;

const TIERS: [Tier; 5] = [
    Tier::SelfReplacement,
    Tier::ControlEtb,
    Tier::CopyEtb,
    Tier::BackFaceEtb,
    Tier::Other,
];

const DESTS: [ZoneKind; 4] = [
    ZoneKind::Graveyard,
    ZoneKind::Exile,
    ZoneKind::Hand,
    ZoneKind::Library,
];

/// One generated case.
struct Case {
    world: World,
    effects: Vec<ReplacementEffect>,
    events: Vec<GameEvent>,
    /// Every effect is a prevention effect on Damage and every event is Damage.
    prevention_only: bool,
}

fn player(rng: &mut Lcg) -> PlayerId {
    rng.pick(&[P1, P2])
}

fn tier(rng: &mut Lcg) -> Tier {
    rng.pick(&TIERS)
}

/// A prevention effect with an optional shield or pool.
fn gen_prevention(rng: &mut Lcg, n: u64) -> ReplacementEffect {
    let matcher = if rng.chance(1, 2) {
        m_dmg_player(player(rng))
    } else {
        EventMatcher::new(BusEventKind::Damage)
    };
    let then = if rng.chance(1, 4) {
        vec![life_change(player(rng), 1)]
    } else {
        Vec::new()
    };
    let e = fx(n, matcher, Action::Prevent { then });
    let e = e.with_tier(tier(rng)).with_contained(rng.chance(1, 5));
    match rng.below(3) {
        0 => e,
        1 => e.with_shield(Shield::Uses(1 + rng.below(3) as u32)),
        _ => e.with_shield(Shield::Pool(1 + rng.below(4) as i64)),
    }
}

/// Any damage effect: prevention, amount change (possibly an override bounded by
/// `amount_below`) or skip.
fn gen_damage(rng: &mut Lcg, n: u64) -> ReplacementEffect {
    match rng.below(4) {
        0 => gen_prevention(rng, n),
        1 => {
            let below = 2 + rng.below(7) as i64;
            let e = fx(
                n,
                EventMatcher {
                    amount_below: Some(below),
                    ..EventMatcher::new(BusEventKind::Damage)
                },
                Action::Modify(Modification::Amount(AmountOp::Add(1 + rng.below(3) as i64))),
            );
            e.with_overrides_614_5(rng.chance(1, 2))
                .with_tier(tier(rng))
        }
        2 => {
            let op = match rng.below(3) {
                0 => AmountOp::Add(rng.below(4) as i64),
                1 => AmountOp::Mul(rng.below(4) as i64),
                _ => AmountOp::Set(rng.below(6) as i64),
            };
            let e = fx(
                n,
                m_dmg_player(player(rng)),
                Action::Modify(Modification::Amount(op)),
            )
            .with_tier(tier(rng))
            .with_contained(rng.chance(1, 5));
            if rng.chance(1, 3) {
                e.with_shield(Shield::Uses(1 + rng.below(2) as u32))
            } else {
                e
            }
        }
        _ => fx(n, m_dmg_player(player(rng)), Action::Skip).with_tier(tier(rng)),
    }
}

/// A zone-change effect: a destination change or a skip.
fn gen_zone(rng: &mut Lcg, n: u64, objs: &[ObjectId]) -> ReplacementEffect {
    let mut matcher = EventMatcher::new(BusEventKind::ZoneChange);
    if rng.chance(1, 2) {
        matcher.to = Some(rng.pick(&DESTS));
    }
    if rng.chance(1, 3) {
        matcher.affected_object = Some(rng.pick(objs));
    }
    if rng.chance(1, 4) {
        matcher.from = Some(ZoneKind::Battlefield);
    }
    let action = if rng.chance(1, 5) {
        Action::Skip
    } else {
        Action::Modify(Modification::SetDestination(rng.pick(&DESTS)))
    };
    let e = fx(n, matcher, action)
        .with_tier(tier(rng))
        .with_contained(rng.chance(1, 6));
    if rng.chance(1, 4) {
        e.with_shield(Shield::Uses(1 + rng.below(2) as u32))
    } else {
        e
    }
}

fn gen_case(seed: u64) -> Case {
    let mut rng = Lcg::new(seed);
    let mut world = World::new();
    let objs: Vec<ObjectId> = (0..3)
        .map(|_| {
            let owner = player(&mut rng);
            let controller = player(&mut rng);
            world.create_controlled(ZoneKind::Battlefield, owner, Some(controller))
        })
        .collect();
    let prevention_only = rng.chance(1, 3);

    // Distinct effect ids in 1..=20, in generation (insertion) order.
    let count = 1 + rng.below(5);
    let mut ids: Vec<u64> = Vec::new();
    while (ids.len() as u64) < count {
        let n = 1 + rng.below(20);
        if !ids.contains(&n) {
            ids.push(n);
        }
    }
    let effects = ids
        .iter()
        .map(|&n| {
            if prevention_only {
                gen_prevention(&mut rng, n)
            } else if rng.chance(1, 2) {
                gen_damage(&mut rng, n)
            } else {
                gen_zone(&mut rng, n, &objs)
            }
        })
        .collect();

    // 1..=3 events; each object moves at most once.
    let mut events = Vec::new();
    let mut unmoved = objs.clone();
    for _ in 0..1 + rng.below(3) {
        if prevention_only || unmoved.is_empty() || rng.chance(1, 2) {
            events.push(dmg_player(player(&mut rng), 1 + rng.below(6) as i64));
        } else {
            let i = usize::try_from(rng.below(unmoved.len() as u64)).expect("fits");
            let obj = unmoved.remove(i);
            events.push(zone_change(
                obj,
                rng.pick(&DESTS),
                rng.pick(&[ZoneChangeCause::Destroy, ZoneChangeCause::Other]),
            ));
        }
    }
    Case {
        world,
        effects,
        events,
        prevention_only,
    }
}

#[derive(Clone, Copy, Debug)]
enum Strategy {
    First,
    Last,
    Seeded(u64),
}

fn run_case(case: &Case, effects: Vec<ReplacementEffect>, strategy: Strategy) -> Run {
    let bus = Bus::new(case.world.clone(), effects);
    let mut rng = Lcg::new(match strategy {
        Strategy::Seeded(s) => s,
        _ => 0,
    });
    drive(bus, Call::batch(case.events.clone(), false), |_, req| {
        let n = option_count(req) as u64;
        assert!(n >= 2, "a request offers at least two effects: {req:?}");
        let i = match strategy {
            Strategy::First => 0,
            Strategy::Last => n - 1,
            Strategy::Seeded(_) => rng.below(n),
        };
        u32::try_from(i).expect("fits")
    })
}

fn submitted_damage(events: &[GameEvent]) -> i64 {
    events
        .iter()
        .map(|e| match e {
            GameEvent::Damage { amount, .. } => *amount,
            _ => 0,
        })
        .sum()
}

fn check_invariants(case: &Case, r: &Run, seed: u64, strategy: Strategy) {
    let ctx = format!("seed {seed}, {strategy:?}");
    let report = match &r.result {
        Ok(rep) => rep,
        Err(e) => panic!("{ctx}: unexpected bus error {e:?}"),
    };
    let by_id: BTreeMap<EffectId, &ReplacementEffect> =
        case.effects.iter().map(|e| (e.id, e)).collect();

    // (1) CR 614.5: applied sets hold distinct ids; a non-override effect applies at most once
    // per event (at most once per submitted event overall; children here match nothing).
    for e in r.executed() {
        let ids = applied_of(&e);
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ctx}: {ids:?}");
        for id in &ids {
            assert!(by_id.contains_key(id), "{ctx}: unknown applied {id:?}");
        }
    }
    let mut counts: BTreeMap<EffectId, usize> = BTreeMap::new();
    for id in &report.applications {
        *counts.entry(*id).or_default() += 1;
    }
    for (id, &n) in &counts {
        let effect = by_id
            .get(id)
            .unwrap_or_else(|| panic!("{ctx}: unknown {id:?}"));
        if !effect.overrides_614_5 {
            assert!(
                n <= case.events.len(),
                "{ctx}: {id:?} applied {n} times to {} events",
                case.events.len()
            );
        }
    }

    // (4) CR 615: with only prevention effects, dealt + prevented == submitted.
    if case.prevention_only {
        let dealt: i64 = r
            .executed()
            .iter()
            .map(|e| match e.instance.event() {
                GameEvent::Damage { amount, .. } => *amount,
                _ => 0,
            })
            .sum();
        assert_eq!(
            dealt + report.prevented,
            submitted_damage(&case.events),
            "{ctx}"
        );
        assert!(report.prevented >= 0, "{ctx}");
    }

    // (5) CR 615: shields never go below zero; an exhausted effect is absent.
    for id in r.after.registry.ids() {
        assert!(by_id.contains_key(&id), "{ctx}: registry gained {id:?}");
    }
    for effect in &case.effects {
        let n = counts.get(&effect.id).copied().unwrap_or(0);
        let present = r.after.registry.get(effect.id).is_some();
        match effect.shield {
            None => assert!(present, "{ctx}: unshielded {:?} removed", effect.id),
            Some(Shield::Uses(u)) => {
                let u = u as usize;
                assert!(n <= u, "{ctx}: {:?} used {n} times of {u}", effect.id);
                assert_eq!(present, n < u, "{ctx}: {:?} used {n} of {u}", effect.id);
            }
            Some(Shield::Pool(_)) => {
                // A pool is removed only once drained, which takes at least one application.
                if !present {
                    assert!(n >= 1, "{ctx}: unused pool {:?} removed", effect.id);
                }
            }
        }
    }
}

// REQ-EVENT-001, CR 614.5, 615, 616.1, 616.1f: for every generated case and every answer
// strategy (first, last, seeded) the bus terminates, and invariants (1), (4) and (5) hold.
#[test]
fn req_event_001_property_terminates_and_invariants_hold() {
    for seed in 0..CASES {
        let case = gen_case(seed);
        for strategy in [
            Strategy::First,
            Strategy::Last,
            Strategy::Seeded(seed.wrapping_mul(31).wrapping_add(7)),
        ] {
            let r = run_case(&case, case.effects.clone(), strategy);
            check_invariants(&case, &r, seed, strategy);
        }
    }
}

// REQ-EVENT-001, CR 616.1 (options ascending by EffectId): results do not depend on the
// registry insertion order (2), and identical re-runs are identical (3).
#[test]
fn req_event_001_property_order_independent_and_deterministic() {
    for seed in 0..CASES {
        let case = gen_case(seed);
        for strategy in [
            Strategy::First,
            Strategy::Last,
            Strategy::Seeded(seed ^ 0xA5A5),
        ] {
            let a = run_case(&case, case.effects.clone(), strategy);
            let mut reversed = case.effects.clone();
            reversed.reverse();
            let b = run_case(&case, reversed, strategy);
            let c = run_case(&case, case.effects.clone(), strategy);
            for (name, other) in [("reversed", &b), ("rerun", &c)] {
                assert_eq!(a.result, other.result, "seed {seed}, {strategy:?}, {name}");
                assert_eq!(
                    bytes(&a.after.registry),
                    bytes(&other.after.registry),
                    "seed {seed}, {strategy:?}, {name}"
                );
                assert_eq!(
                    a.after.snap(),
                    other.after.snap(),
                    "seed {seed}, {strategy:?}, {name}"
                );
                assert_eq!(a.hash, other.hash, "seed {seed}, {strategy:?}, {name}");
                assert_eq!(
                    a.requests, other.requests,
                    "seed {seed}, {strategy:?}, {name}"
                );
            }
        }
    }
}

// REQ-EVENT-001: the generator itself is deterministic and covers both modes.
#[test]
fn req_event_001_property_generator_is_deterministic() {
    let mut modes = [0usize; 2];
    for seed in 0..CASES {
        let a = gen_case(seed);
        let b = gen_case(seed);
        assert_eq!(a.effects, b.effects);
        assert_eq!(a.events, b.events);
        modes[usize::from(a.prevention_only)] += 1;
    }
    assert!(modes[0] > 0 && modes[1] > 0);
}
