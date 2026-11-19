// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for the events module (REQ-CORE-006, REQ-EVENT-001, REQ-DSL-003): random
//! scripts of events (fixed seeds, local deterministic generator) run on two identical worlds
//! give equal results, observable state and EventState bytes; every error leaves the world equal
//! to its pre-call snapshot; the log and the ledger stay consistent with what executed.
//! Black-box: written from the work-item brief only.

#[path = "spec_events_support.rs"]
mod support;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::events::{
    CounterTarget, DamageTarget, GameEvent, HistoryQuery, HistoryScope, HistoryStat, Phase, Step,
    ZoneChangeCause,
};
use fineprint_core::permanent::AttachTarget;
use fineprint_core::zones::{MoveRequest, Position, ZoneKind};
use support::*;

const SEEDS: [u64; 8] = [1, 2, 3, 0x27, 0xFEED, 0xDEAD_BEEF, 77, 2026];
const STEPS: usize = 120;

const KINDS: [ZoneKind; 5] = [
    ZoneKind::Library,
    ZoneKind::Hand,
    ZoneKind::Battlefield,
    ZoneKind::Graveyard,
    ZoneKind::Exile,
];
const CAUSES: [ZoneChangeCause; 4] = [
    ZoneChangeCause::Other,
    ZoneChangeCause::Destroy,
    ZoneChangeCause::Sacrifice,
    ZoneChangeCause::StateBasedAction,
];
const STEPS_ALL: [Step; 11] = [
    Step::Untap,
    Step::Upkeep,
    Step::Draw,
    Step::BeginningOfCombat,
    Step::DeclareAttackers,
    Step::DeclareBlockers,
    Step::FirstStrikeDamage,
    Step::CombatDamage,
    Step::EndOfCombat,
    Step::End,
    Step::Cleanup,
];
const PHASES: [Phase; 5] = [
    Phase::Beginning,
    Phase::PrecombatMain,
    Phase::Combat,
    Phase::PostcombatMain,
    Phase::Ending,
];
const COUNTER_KINDS: [&str; 3] = ["+1/+1", "-1/-1", "test.charge"];

/// A world with cards in every zone of both players.
fn seeded_world() -> World {
    let mut w = World::new();
    for p in [P1, P2] {
        for _ in 0..4 {
            w.create(ZoneKind::Library, p);
        }
        for kind in [
            ZoneKind::Hand,
            ZoneKind::Battlefield,
            ZoneKind::Battlefield,
            ZoneKind::Graveyard,
            ZoneKind::Exile,
        ] {
            w.create(kind, p);
        }
    }
    w
}

/// An object id: usually one that exists now, sometimes a stale or unknown one.
fn pick_object(rng: &mut Lcg, w: &World) -> ObjectId {
    let ids = w.zones.all_object_ids();
    if ids.is_empty() || rng.below(10) == 0 {
        return oid(rng.below(60) + 1);
    }
    rng.pick(&ids)
}

/// A battlefield permanent when there is one.
fn pick_permanent(rng: &mut Lcg, w: &World) -> ObjectId {
    let ids = w.permanents.ids();
    if ids.is_empty() {
        return pick_object(rng, w);
    }
    rng.pick(&ids)
}

fn pick_player(rng: &mut Lcg) -> PlayerId {
    if rng.below(20) == 0 {
        NOBODY
    } else {
        rng.pick(&[P1, P2])
    }
}

/// A small signed amount, mostly positive, sometimes 0 or negative.
fn amount(rng: &mut Lcg) -> i64 {
    i64::try_from(rng.below(8)).expect("small") - 1
}

fn random_event(rng: &mut Lcg, w: &mut World) -> GameEvent {
    match rng.below(17) {
        0 | 1 => {
            let mut request = MoveRequest::new(rng.pick(&KINDS));
            if rng.below(3) == 0 {
                request.position = Position::Bottom;
            }
            if request.to == ZoneKind::Battlefield && rng.below(3) == 0 {
                request.controller = Some(rng.pick(&[P1, P2]));
            }
            // Face-down entries onto the battlefield (REQ-EVENT-001, CR 701.27b, 708.8).
            if request.to == ZoneKind::Battlefield && rng.below(4) == 0 {
                request.face_down = true;
            }
            GameEvent::ZoneChange {
                object: pick_object(rng, w),
                request,
                cause: rng.pick(&CAUSES),
            }
        }
        2 => GameEvent::Draw {
            player: pick_player(rng),
        },
        3 => GameEvent::Damage {
            source: if rng.below(2) == 0 {
                None
            } else {
                Some(pick_object(rng, w))
            },
            source_controller: if rng.below(2) == 0 {
                None
            } else {
                Some(pick_player(rng))
            },
            target: if rng.below(2) == 0 {
                DamageTarget::Player(pick_player(rng))
            } else {
                DamageTarget::Object(pick_permanent(rng, w))
            },
            amount: amount(rng),
            combat: rng.below(2) == 0,
        },
        4 => GameEvent::LifeChange {
            player: pick_player(rng),
            delta: amount(rng) - 3,
        },
        5 | 6 => {
            let target = if rng.below(3) == 0 {
                CounterTarget::Player(pick_player(rng))
            } else {
                CounterTarget::Object(pick_object(rng, w))
            };
            let kind = rng.pick(&COUNTER_KINDS).to_string();
            if rng.below(2) == 0 {
                GameEvent::AddCounters {
                    target,
                    kind,
                    amount: amount(rng),
                }
            } else {
                GameEvent::RemoveCounters {
                    target,
                    kind,
                    amount: amount(rng),
                }
            }
        }
        7 => GameEvent::Tap {
            object: pick_permanent(rng, w),
        },
        8 => GameEvent::Untap {
            object: pick_permanent(rng, w),
        },
        9 => {
            // Sometimes a card id that already exists (a Zone error).
            let card = if rng.below(10) == 0 {
                cid(1)
            } else {
                w.fresh_card()
            };
            GameEvent::CreateToken {
                owner: pick_player(rng),
                card,
                controller: if rng.below(3) == 0 {
                    Some(pick_player(rng))
                } else {
                    None
                },
            }
        }
        10 => GameEvent::TurnBegins {
            player: pick_player(rng),
        },
        11 => GameEvent::PhaseBegins {
            phase: rng.pick(&PHASES),
        },
        12 => GameEvent::StepBegins {
            step: rng.pick(&STEPS_ALL),
        },
        13 => GameEvent::Attach {
            attachment: pick_permanent(rng, w),
            target: if rng.below(4) == 0 {
                AttachTarget::Player(pick_player(rng))
            } else {
                AttachTarget::Permanent(pick_permanent(rng, w))
            },
        },
        14 => GameEvent::Unattach {
            attachment: pick_permanent(rng, w),
        },
        15 => GameEvent::BecomeCopy {
            object: pick_permanent(rng, w),
            copied: pick_object(rng, w),
        },
        _ => {
            let object = pick_permanent(rng, w);
            if rng.below(2) == 0 {
                GameEvent::TurnFaceUp { object }
            } else {
                GameEvent::TurnFaceDown { object }
            }
        }
    }
}

fn game_query(w: &World, stat: HistoryStat, p: PlayerId) -> i64 {
    w.events.history().query(&HistoryQuery {
        stat,
        scope: HistoryScope::Game,
        player: Some(p),
    })
}

// REQ-CORE-006, REQ-EVENT-001, REQ-DSL-003: determinism, all-or-nothing errors, log and ledger
// consistency over random scripts.
#[test]
fn random_scripts_are_deterministic_and_errors_change_nothing() {
    for seed in SEEDS {
        let mut rng = Lcg::new(seed);
        let mut w1 = seeded_world();
        let mut w2 = seeded_world();
        let mut logged = 0usize;
        let mut draws = [0i64; 2];
        let mut ok_events = 0usize;
        let mut errors = 0usize;

        for step in 0..STEPS {
            let n = 1 + rng.below(3) as usize;
            let mut events = Vec::with_capacity(n);
            for _ in 0..n {
                events.push(random_event(&mut rng, &mut w1));
            }
            let before = w1.snap();
            let r1 = w1.batch(events.clone());
            let r2 = w2.batch(events.clone());
            assert_eq!(r1, r2, "seed {seed} step {step}: results differ");
            assert_eq!(
                w1.snap(),
                w2.snap(),
                "seed {seed} step {step}: state differs"
            );
            assert_eq!(bytes(&w1.events), bytes(&w2.events));

            match r1 {
                Err(e) => {
                    errors += 1;
                    assert_eq!(
                        w1.snap(),
                        before,
                        "seed {seed} step {step}: error {e:?} changed the world ({events:?})"
                    );
                }
                Ok(batch) => {
                    assert!(batch.events.len() <= events.len());
                    if batch.events.is_empty() {
                        assert_eq!(w1.events.log().len(), logged);
                    } else {
                        assert_eq!(batch.index, logged as u64);
                        logged += 1;
                        assert_eq!(w1.events.log().len(), logged);
                        assert_eq!(w1.events.log().batches().last(), Some(&batch));
                    }
                    assert_eq!(batch.turn, w1.turn());
                    for e in &batch.events {
                        ok_events += 1;
                        if let GameEvent::Draw { player } = e.instance.event() {
                            draws[usize::from(*player == P2)] += 1;
                        }
                    }
                }
            }

            // Log indices are 0..len.
            let idx: Vec<u64> = w1.events.log().batches().iter().map(|b| b.index).collect();
            assert_eq!(idx, (0..logged as u64).collect::<Vec<_>>());

            // Ledger (CR 119.2, 119.3, 120.3a): life = start + gained - lost; cards drawn =
            // executed draws.
            for (i, p) in [P1, P2].into_iter().enumerate() {
                let gained = game_query(&w1, HistoryStat::LifeGained, p);
                let lost = game_query(&w1, HistoryStat::LifeLost, p);
                assert_eq!(
                    w1.life(p),
                    STARTING_LIFE + gained - lost,
                    "seed {seed} step {step}: life of {p:?}"
                );
                assert_eq!(game_query(&w1, HistoryStat::CardsDrawn, p), draws[i]);
            }
            // CR 110.1, CR 400.7: every permanent is on the battlefield; every battlefield
            // object is a permanent.
            let bf = w1
                .zones
                .contents(fineprint_core::zones::ZoneId::Battlefield);
            assert_eq!(w1.permanents.ids(), {
                let mut v = bf.clone();
                v.sort();
                v
            });
            // REQ-EVENT-001, REQ-CORE-004, CR 701.27b: the zone record's face-down flag and the
            // permanent's status agree for every battlefield object.
            for &id in &bf {
                assert_eq!(
                    w1.zones.object(id).unwrap().face_down,
                    w1.permanents.get(id).unwrap().status().face_down,
                    "seed {seed} step {step}: face-down disagreement for {id:?}"
                );
            }
            // CR 122.1, 122.2: markers never hold counters below 1.
            for id in w1.zones.all_object_ids() {
                assert!(w1.events.markers().counters(id).values().all(|&n| n >= 1));
                assert!(w1.events.markers().damage(id) >= 0);
            }
        }
        assert!(ok_events > 0, "seed {seed}: some events occurred");
        assert!(errors > 0, "seed {seed}: some errors were exercised");
    }
}

// REQ-CORE-006: a cloned world replays a script to the same bytes (clone is a faithful,
// independent snapshot).
#[test]
fn clone_is_an_independent_snapshot() {
    let mut rng = Lcg::new(0xC10E);
    let mut w = seeded_world();
    for _ in 0..30 {
        let ev = random_event(&mut rng, &mut w);
        let _ = w.exec(ev);
    }
    let snapshot = w.clone();
    let frozen = snapshot.snap();
    let mut a = w.clone();
    let mut b = w.clone();
    for _ in 0..30 {
        let ev = random_event(&mut rng, &mut a);
        let ra = a.exec(ev.clone());
        let rb = b.exec(ev);
        assert_eq!(ra, rb);
    }
    assert_eq!(a.snap(), b.snap());
    assert_eq!(
        snapshot.snap(),
        frozen,
        "the snapshot did not follow later changes"
    );
}
