// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of trigger normalization for the state-based destruction cause (#209 FP-155):
// REQ-SBA-001, REQ-EVENT-001, REQ-EVENT-002. A move with `ZoneChangeCause::StateBasedDestroy`
// (CR 704.5g, CR 704.5h) is a destruction (CR 701.8b): it maps exactly like `Destroy`, so a
// "whenever a creature is destroyed" listener fires for it; a plain `StateBasedAction` move
// (CR 704.5f, CR 704.5i) is not destroyed. Synthetic ids and neutral ability ids only.

use super::*;

/// The kinds of a single move of object 1 (new id 2) from `from` to `to` with `cause`.
fn move_kinds(from: ZoneId, to: ZoneId, cause: ZoneChangeCause) -> Vec<EventKind> {
    let mut got = one(zone_change_exec(1, 2, from, to, cause));
    assert_eq!(got.len(), 1);
    got.remove(0).kinds
}

// REQ-SBA-001, REQ-EVENT-001: CR 701.8b, CR 603.6c, CR 603.10a. A battlefield-to-graveyard
// move with `StateBasedDestroy` yields `Destroyed` plus exactly the kinds of the same move with
// `Destroy` (ZONE_CHANGE, LEAVES_BATTLEFIELD, DIES, PUT_INTO_GRAVEYARD); with `StateBasedAction`
// it yields the same kinds without `Destroyed`.
#[test]
fn req_sba_001_cr_701_8b_state_based_destroy_maps_like_destroy() {
    let (from, to) = (ZoneId::Battlefield, ZoneId::Graveyard(p(1)));
    let sbd = move_kinds(from, to, ZoneChangeCause::StateBasedDestroy);
    let destroy = move_kinds(from, to, ZoneChangeCause::Destroy);
    let plain = move_kinds(from, to, ZoneChangeCause::StateBasedAction);
    assert_eq!(sbd, destroy);
    assert_eq!(
        sbd.iter().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            EventKind::ZoneChange,
            EventKind::LeavesBattlefield,
            EventKind::Dies,
            EventKind::PutIntoGraveyard,
            EventKind::Destroyed,
        ])
    );
    assert!(!plain.contains(&EventKind::Destroyed));
    let mut without: Vec<EventKind> = sbd
        .iter()
        .copied()
        .filter(|&k| k != EventKind::Destroyed)
        .collect();
    without.sort();
    let mut plain_sorted = plain.clone();
    plain_sorted.sort();
    assert_eq!(without, plain_sorted);
}

// REQ-SBA-001, REQ-EVENT-001: CR 701.8b. A `StateBasedDestroy` move between any two zones maps
// to exactly the kinds of the `Destroy` move between the same zones (the bus never submits such
// a move off the battlefield, but the mapping does not depend on it).
#[test]
fn req_sba_001_cr_701_8b_state_based_destroy_maps_like_destroy_in_every_zone_pair() {
    for from in ZoneKind::ALL {
        for to in ZoneKind::ALL {
            let (f, t) = (ZoneId::of(from, p(1)), ZoneId::of(to, p(1)));
            let sbd = one(zone_change_exec(
                1,
                2,
                f,
                t,
                ZoneChangeCause::StateBasedDestroy,
            ));
            let destroy = one(zone_change_exec(1, 2, f, t, ZoneChangeCause::Destroy));
            assert_eq!(sbd, destroy, "{from:?} -> {to:?}");
        }
    }
}

/// One SBA batch: object 1 destroyed by 704.5g, 2 by 704.5h (both `StateBasedDestroy`), 3 put
/// into the graveyard by 704.5f and 4 by 704.5i (both `StateBasedAction`).
fn sba_batch() -> ExecutedBatch {
    let gy = ZoneId::Graveyard(p(1));
    batch_of(vec![
        zone_change_exec(
            1,
            11,
            ZoneId::Battlefield,
            gy,
            ZoneChangeCause::StateBasedDestroy,
        ),
        zone_change_exec(
            2,
            12,
            ZoneId::Battlefield,
            gy,
            ZoneChangeCause::StateBasedDestroy,
        ),
        zone_change_exec(
            3,
            13,
            ZoneId::Battlefield,
            gy,
            ZoneChangeCause::StateBasedAction,
        ),
        zone_change_exec(
            4,
            14,
            ZoneId::Battlefield,
            gy,
            ZoneChangeCause::StateBasedAction,
        ),
    ])
}

// REQ-SBA-001, REQ-EVENT-002: CR 701.8b, CR 603.10a. A synthetic "whenever a creature is
// destroyed" listener (a DESTROYED matcher on a creature subject, binding the object) fires for
// the 704.5g and 704.5h deaths (`StateBasedDestroy`), once each, looking back, and not for the
// 704.5f or 704.5i deaths (`StateBasedAction`), although all four were creatures before the
// event. A "whenever a creature dies" control listener fires for all four.
#[test]
fn req_sba_001_cr_701_8b_destroyed_listener_fires_for_state_based_destroy_only() {
    let events = events_from_batch(&sba_batch());
    let destroyed = with_bind(
        with_subject(
            matcher(EventKind::Destroyed),
            EventSubject::Object(creature_sel()),
        ),
        "object",
        "it",
    );
    let mut env = FakeEnv::new().pre(reg(50, 1, "test.on_destroyed", destroyed));
    for id in 1..=4 {
        env = env.creature(StateView::PreEvent, id);
    }
    let got = run(&env, &events);
    assert_eq!(got.len(), 2);
    for (t, (index, id)) in got.iter().zip([(0, 1), (1, 2)]) {
        assert_eq!(t.event_indices, vec![index]);
        assert_eq!(t.source, o(50));
        assert_eq!(t.look_back, Some(LookBackKind::LeavesBattlefield));
        assert_eq!(t.bindings.get("it"), Some(&EventField::Object(o(id))));
    }
    // Control: every one of them dies.
    let dies = with_subject(
        matcher(EventKind::Dies),
        EventSubject::Object(creature_sel()),
    );
    let mut env = FakeEnv::new().pre(reg(50, 1, "test.on_dies", dies));
    for id in 1..=4 {
        env = env.creature(StateView::PreEvent, id);
    }
    assert_eq!(run(&env, &events).len(), 4);
}
