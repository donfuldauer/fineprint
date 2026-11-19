// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the executed-event log (REQ-EVENT-001; CR 603.2c, 603.10, 500.6). Black-box:
//! written from the work-item brief only.

#[path = "spec_events_support.rs"]
mod support;

use fineprint_core::events::{
    EventError, ExecutedBatch, GameEvent, Outcome, Phase, ZoneChangeCause,
};
use fineprint_core::zones::{MoveRequest, ZoneId, ZoneKind};
use support::*;

fn indices(batches: &[ExecutedBatch]) -> Vec<u64> {
    batches.iter().map(|b| b.index).collect()
}

// REQ-EVENT-001: indices count logged batches from 0; failed and empty batches are not logged.
#[test]
fn log_indices_count_only_logged_batches() {
    let mut w = World::new();
    assert_eq!(w.events.log().len(), 0);
    assert!(w.events.log().batches().is_empty());

    let b0 = w.ok(GameEvent::LifeChange {
        player: P1,
        delta: 1,
    });
    assert_eq!(
        w.exec(GameEvent::LifeChange {
            player: P1,
            delta: 0
        }),
        Ok(None)
    );
    assert_eq!(
        w.batch(vec![
            GameEvent::LifeChange {
                player: P1,
                delta: 1
            },
            GameEvent::Draw { player: NOBODY },
        ]),
        Err(EventError::UnknownPlayer(NOBODY))
    );
    let r1 = w
        .batch(vec![GameEvent::LifeChange {
            player: P2,
            delta: 2,
        }])
        .unwrap();
    let r2 = w
        .batch(vec![GameEvent::PhaseBegins {
            phase: Phase::Combat,
        }])
        .unwrap();
    assert_eq!(r1.index, 1);
    assert_eq!(r2.index, 2);
    let log = w.events.log();
    assert_eq!(log.len(), 3);
    assert_eq!(indices(log.batches()), vec![0, 1, 2]);
    assert_eq!(log.batches()[0].events, vec![b0]);
    assert_eq!(log.batches()[1], r1);
    assert_eq!(log.batches()[2], r2);
}

// REQ-EVENT-001: `turn` is the turn number when the batch finished executing.
#[test]
fn log_turn_is_turn_at_end_of_batch() {
    let mut w = World::new();
    let first = w
        .batch(vec![GameEvent::LifeChange {
            player: P1,
            delta: 1,
        }])
        .unwrap();
    assert_eq!(first.turn, 1);
    let rollover = w
        .batch(vec![
            GameEvent::LifeChange {
                player: P1,
                delta: 1,
            },
            GameEvent::TurnBegins { player: P2 },
        ])
        .unwrap();
    assert_eq!(rollover.turn, 2);
    let after = w
        .batch(vec![GameEvent::LifeChange {
            player: P2,
            delta: 1,
        }])
        .unwrap();
    assert_eq!(after.turn, 2);
    let turns: Vec<u64> = w.events.log().batches().iter().map(|b| b.turn).collect();
    assert_eq!(turns, vec![1, 2, 2]);
}

// REQ-EVENT-001: batches_since returns batches with index >= the argument, empty if none.
#[test]
fn batches_since() {
    let mut w = World::new();
    for d in 1..=4 {
        w.ok(GameEvent::LifeChange {
            player: P1,
            delta: d,
        });
    }
    let log = w.events.log();
    assert_eq!(indices(log.batches_since(0)), vec![0, 1, 2, 3]);
    assert_eq!(indices(log.batches_since(2)), vec![2, 3]);
    assert_eq!(indices(log.batches_since(3)), vec![3]);
    assert!(log.batches_since(4).is_empty());
    assert!(log.batches_since(u64::MAX).is_empty());
    assert!(World::new().events.log().batches_since(0).is_empty());
}

// REQ-EVENT-001, CR 603.2c: the events list holds only those that occurred, in submission
// order, with their outcomes (old/new ids from Moved).
#[test]
fn log_lists_occurred_events_in_order_with_outcomes() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Battlefield, P2);
    let submitted = vec![
        GameEvent::LifeChange {
            player: P1,
            delta: 0,
        },
        GameEvent::Tap { object: a },
        GameEvent::LifeChange {
            player: P2,
            delta: 2,
        },
        GameEvent::Tap { object: a },
        GameEvent::ZoneChange {
            object: b,
            request: MoveRequest::new(ZoneKind::Graveyard),
            cause: ZoneChangeCause::Sacrifice,
        },
        GameEvent::Untap { object: a },
    ];
    let done = w.batch(submitted.clone()).unwrap();
    let kept: Vec<&GameEvent> = done.events.iter().map(|e| e.instance.event()).collect();
    assert_eq!(
        kept,
        vec![&submitted[1], &submitted[2], &submitted[4], &submitted[5]]
    );
    assert_eq!(done.events[0].outcome, Outcome::Applied);
    assert_eq!(done.events[1].outcome, Outcome::Applied);
    let new_b = w.zones.contents(ZoneId::Graveyard(P2))[0];
    match done.events[2].outcome {
        Outcome::ZoneChange(m) => {
            assert_eq!(m.old, b);
            assert_eq!(m.new, new_b);
            assert_eq!(m.from, ZoneId::Battlefield);
            assert_eq!(m.to, ZoneId::Graveyard(P2));
        }
        ref other => panic!("unexpected {other:?}"),
    }
    assert!(done.events[2].lki_captured);
    assert!(!done.events[0].lki_captured);
    assert_eq!(w.events.log().batches(), &[done]);
}

// REQ-EVENT-001: every Outcome kind appears in the log as returned.
#[test]
fn log_records_each_outcome_kind() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let host = w.create(ZoneKind::Battlefield, P2);
    let card = w.fresh_card();
    let results = vec![
        w.ok(GameEvent::CreateToken {
            owner: P1,
            card,
            controller: None,
        }),
        w.ok(GameEvent::AddCounters {
            target: fineprint_core::events::CounterTarget::Object(a),
            kind: "test.charge".into(),
            amount: 2,
        }),
        w.ok(GameEvent::RemoveCounters {
            target: fineprint_core::events::CounterTarget::Object(a),
            kind: "test.charge".into(),
            amount: 1,
        }),
        w.ok(GameEvent::Attach {
            attachment: a,
            target: fineprint_core::permanent::AttachTarget::Permanent(host),
        }),
        w.ok(GameEvent::TurnBegins { player: P2 }),
    ];
    assert!(matches!(results[0].outcome, Outcome::TokenCreated { .. }));
    assert_eq!(results[2].outcome, Outcome::CountersRemoved { removed: 1 });
    assert!(matches!(results[3].outcome, Outcome::Attached { .. }));
    assert_eq!(results[4].outcome, Outcome::TurnBegan { turn: 2 });
    let logged: Vec<_> = w
        .events
        .log()
        .batches()
        .iter()
        .flat_map(|b| b.events.clone())
        .collect();
    assert_eq!(logged, results);
}
