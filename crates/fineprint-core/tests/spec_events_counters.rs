// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the AddCounters / RemoveCounters events (REQ-EVENT-001; CR 122.1, 122.3,
//! 704.5q). Black-box: written from the work-item brief only. Counter kinds are CR-defined
//! (`+1/+1`, `-1/-1`) or synthetic (`test.*`).

#[path = "spec_events_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_core::events::{CounterTarget, EventError, GameEvent, Outcome};
use fineprint_core::zones::ZoneKind;
use support::*;

const POISONISH: &str = "test.poisonish";

fn add(target: CounterTarget, kind: &str, amount: i64) -> GameEvent {
    GameEvent::AddCounters {
        target,
        kind: kind.to_string(),
        amount,
    }
}

fn remove(target: CounterTarget, kind: &str, amount: i64) -> GameEvent {
    GameEvent::RemoveCounters {
        target,
        kind: kind.to_string(),
        amount,
    }
}

// REQ-EVENT-001, CR 122.1: counters added to an object are held in ObjectMarkers.
#[test]
fn add_counters_to_object() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    assert_eq!(
        w.ok(add(CounterTarget::Object(a), "test.charge", 2))
            .outcome,
        Outcome::Applied
    );
    w.ok(add(CounterTarget::Object(a), "test.charge", 3));
    w.ok(add(CounterTarget::Object(a), "test.other", 1));
    let m = w.events.markers();
    assert_eq!(m.counter(a, "test.charge"), 5);
    assert_eq!(m.counter(a, "test.other"), 1);
    assert_eq!(m.counter(a, "test.absent"), 0);
    let expected: BTreeMap<String, i64> = [
        ("test.charge".to_string(), 5),
        ("test.other".to_string(), 1),
    ]
    .into();
    assert_eq!(m.counters(a), expected);
}

// REQ-EVENT-001, CR 122.1: an object in any zone can hold counters.
#[test]
fn add_counters_to_object_outside_battlefield() {
    let mut w = World::new();
    let x = w.create(ZoneKind::Exile, P2);
    w.ok(add(CounterTarget::Object(x), "test.time", 4));
    assert_eq!(w.events.markers().counter(x, "test.time"), 4);
}

// REQ-EVENT-001, CR 122.1: counters on a player go through GameState.
#[test]
fn add_and_remove_counters_on_player() {
    let mut w = World::new();
    w.ok(add(CounterTarget::Player(P2), POISONISH, 3));
    assert_eq!(w.game.player(P2).unwrap().counter(POISONISH), 3);
    assert_eq!(w.game.player(P1).unwrap().counter(POISONISH), 0);
    let done = w.ok(remove(CounterTarget::Player(P2), POISONISH, 1));
    assert_eq!(done.outcome, Outcome::CountersRemoved { removed: 1 });
    assert_eq!(w.game.player(P2).unwrap().counter(POISONISH), 2);
    let done = w.ok(remove(CounterTarget::Player(P2), POISONISH, 10));
    assert_eq!(done.outcome, Outcome::CountersRemoved { removed: 2 });
    assert_eq!(w.game.player(P2).unwrap().counter(POISONISH), 0);
    assert!(
        w.game
            .player(P2)
            .unwrap()
            .counters()
            .get(POISONISH)
            .is_none()
    );
}

// REQ-EVENT-001: 0 is a no-op for add and remove.
#[test]
fn zero_counters_is_no_event() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    w.ok(add(CounterTarget::Object(a), "test.charge", 1));
    let before = w.snap();
    for ev in [
        add(CounterTarget::Object(a), "test.charge", 0),
        remove(CounterTarget::Object(a), "test.charge", 0),
        add(CounterTarget::Player(P1), POISONISH, 0),
        remove(CounterTarget::Player(P1), POISONISH, 0),
    ] {
        assert_eq!(w.exec(ev), Ok(None));
    }
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001: a negative amount is InvalidAmount and changes nothing.
#[test]
fn negative_counters_are_invalid() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    w.ok(add(CounterTarget::Object(a), "test.charge", 2));
    let before = w.snap();
    assert_eq!(
        w.exec(add(CounterTarget::Object(a), "test.charge", -1)),
        Err(EventError::InvalidAmount(-1))
    );
    assert_eq!(
        w.exec(remove(CounterTarget::Object(a), "test.charge", -2)),
        Err(EventError::InvalidAmount(-2))
    );
    assert_eq!(
        w.exec(add(CounterTarget::Player(P1), POISONISH, -3)),
        Err(EventError::InvalidAmount(-3))
    );
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 122.1: removing more than held removes what is held and reports it.
#[test]
fn remove_more_than_held_removes_what_is_held() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    w.ok(add(CounterTarget::Object(a), "test.charge", 3));
    let done = w.ok(remove(CounterTarget::Object(a), "test.charge", 2));
    assert_eq!(done.outcome, Outcome::CountersRemoved { removed: 2 });
    assert_eq!(w.events.markers().counter(a, "test.charge"), 1);
    let done = w.ok(remove(CounterTarget::Object(a), "test.charge", 5));
    assert_eq!(done.outcome, Outcome::CountersRemoved { removed: 1 });
    assert_eq!(w.events.markers().counter(a, "test.charge"), 0);
    assert!(w.events.markers().counters(a).is_empty());
}

// REQ-EVENT-001: removing a kind that is not held is a no-op.
#[test]
fn remove_with_nothing_held_is_no_event() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    w.ok(add(CounterTarget::Object(a), "test.charge", 1));
    let before = w.snap();
    assert_eq!(
        w.exec(remove(CounterTarget::Object(a), "test.other", 1)),
        Ok(None)
    );
    assert_eq!(
        w.exec(remove(CounterTarget::Player(P2), POISONISH, 1)),
        Ok(None)
    );
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 122.3, 704.5q: +1/+1 and -1/-1 counters both remain after the events;
// annihilation is a state-based action only.
#[test]
fn plus_and_minus_counters_are_not_annihilated() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    w.ok(add(CounterTarget::Object(a), "+1/+1", 2));
    w.ok(add(CounterTarget::Object(a), "-1/-1", 3));
    assert_eq!(w.events.markers().counter(a, "+1/+1"), 2);
    assert_eq!(w.events.markers().counter(a, "-1/-1"), 3);
    // Both orders in one batch.
    let b = w.create(ZoneKind::Battlefield, P1);
    w.batch(vec![
        add(CounterTarget::Object(b), "-1/-1", 1),
        add(CounterTarget::Object(b), "+1/+1", 1),
    ])
    .expect("batch succeeds");
    assert_eq!(w.events.markers().counter(b, "+1/+1"), 1);
    assert_eq!(w.events.markers().counter(b, "-1/-1"), 1);
}

// REQ-EVENT-001: unknown targets fail with nothing changed.
#[test]
fn unknown_counter_targets_fail_cleanly() {
    let mut w = World::new();
    let before = w.snap();
    assert_eq!(
        w.exec(add(CounterTarget::Object(GHOST), "test.charge", 1)),
        Err(EventError::UnknownObject(GHOST))
    );
    assert_eq!(
        w.exec(remove(CounterTarget::Object(GHOST), "test.charge", 1)),
        Err(EventError::UnknownObject(GHOST))
    );
    assert_eq!(
        w.exec(add(CounterTarget::Player(NOBODY), POISONISH, 1)),
        Err(EventError::UnknownPlayer(NOBODY))
    );
    assert_eq!(
        w.exec(remove(CounterTarget::Player(NOBODY), POISONISH, 1)),
        Err(EventError::UnknownPlayer(NOBODY))
    );
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001: adding past i64::MAX is Overflow with nothing changed.
#[test]
fn counter_overflow_fails_cleanly() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    w.ok(add(CounterTarget::Object(a), "test.charge", i64::MAX));
    w.ok(add(CounterTarget::Player(P1), POISONISH, i64::MAX));
    let before = w.snap();
    assert_eq!(
        w.exec(add(CounterTarget::Object(a), "test.charge", 1)),
        Err(EventError::Overflow)
    );
    assert_eq!(
        w.exec(add(CounterTarget::Player(P1), POISONISH, 1)),
        Err(EventError::Overflow)
    );
    assert_eq!(w.snap(), before);
}
