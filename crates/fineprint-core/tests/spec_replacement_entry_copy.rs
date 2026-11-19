// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the copy source of an enters-as-copy entry (REQ-EVENT-001, REQ-CORE-003;
//! CR 707.5, 608.2h, SRS 2.3; review round 2 finding G1 with round-1 finding F6). The object
//! becomes a copy as it enters (CR 707.5): the copy is part of the entry batch, with no
//! `BecomeCopy` event. The copied object may leave its zone in the same batch as the entry, in
//! either submission order: the copy then uses its last known information (CR 608.2h) and still
//! succeeds. A standalone `BecomeCopy` whose copied object left the battlefield in an earlier
//! call still fails, and nothing changes. Black-box: written from the work-item brief only.

#[path = "spec_replacement_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::{EventBatch, GameEvent, Outcome, ZoneChangeCause};
use fineprint_core::replacement::{
    Action, BusError, BusEventKind, EventMatcher, Modification, ReplacementEffect, Tier,
};
use fineprint_core::zones::ZoneKind;
use support::*;

/// The copy-ETB effect (id 1) making `entering` enter as a copy of `original`.
fn copy_on_entry(entering: ObjectId, original: ObjectId) -> ReplacementEffect {
    fx(
        1,
        EventMatcher {
            to: Some(ZoneKind::Battlefield),
            ..m_zc_object(entering)
        },
        Action::Modify(Modification::EntersAsCopyOf(original)),
    )
    .with_tier(Tier::CopyEtb)
}

/// The copy effect (id 1) making every created token a copy of `original`.
fn copy_on_token(original: ObjectId) -> ReplacementEffect {
    fx(
        1,
        EventMatcher::new(BusEventKind::CreateToken),
        Action::Modify(Modification::EntersAsCopyOf(original)),
    )
}

/// A Skip effect (id 2) on every BecomeCopy.
fn skip_become_copy() -> ReplacementEffect {
    fx(2, EventMatcher::new(BusEventKind::BecomeCopy), Action::Skip)
}

/// `original` is destroyed (to its owner's graveyard).
fn destroy(original: ObjectId) -> GameEvent {
    zone_change(original, ZoneKind::Graveyard, ZoneChangeCause::Destroy)
}

/// Asserts the CR 707.5 entry shape: the call executed exactly one non-empty batch, the log gained
/// exactly one batch, and no BecomeCopy event was executed or logged. Returns the executed events.
fn assert_one_batch_no_become_copy(r: &Run) -> Vec<fineprint_core::events::ExecutedEvent> {
    assert_eq!(r.nonempty_batches().len(), 1, "one executed batch");
    let log = r.new_log();
    assert_eq!(log.len(), 1, "the log gains one batch, not two");
    let no_copy_event = |e: &fineprint_core::events::ExecutedEvent| {
        !matches!(e.instance.event(), GameEvent::BecomeCopy { .. })
    };
    assert!(r.executed().iter().all(no_copy_event), "no BecomeCopy");
    assert!(
        log[0].events.iter().all(no_copy_event),
        "no BecomeCopy logged"
    );
    r.executed()
}

/// The new id of the entered object (the executed event whose outcome is not `original`'s move).
fn entered_id(events: &[fineprint_core::events::ExecutedEvent], original: ObjectId) -> ObjectId {
    let ids: Vec<ObjectId> = events
        .iter()
        .filter(|e| match e.instance.event() {
            GameEvent::ZoneChange { object, .. } => *object != original,
            GameEvent::CreateToken { .. } => true,
            _ => false,
        })
        .map(|e| new_id(&e.outcome))
        .collect();
    assert_eq!(ids.len(), 1, "exactly one entering event: {events:?}");
    ids[0]
}

/// The zone `original` was moved to by the call (its executed ZoneChange).
fn original_moved_to(r: &Run, original: ObjectId) -> Option<ZoneKind> {
    let moves: Vec<ObjectId> = r
        .executed()
        .iter()
        .filter(|e| {
            matches!(e.instance.event(), GameEvent::ZoneChange { object, .. } if *object == original)
        })
        .map(|e| match e.outcome {
            Outcome::ZoneChange(m) => m.new,
            ref other => panic!("expected a ZoneChange outcome, got {other:?}"),
        })
        .collect();
    assert_eq!(moves.len(), 1, "the original moved exactly once");
    r.world().zone_of(moves[0])
}

// REQ-EVENT-001, REQ-CORE-003, CR 707.5, 608.2h, 400.7 (G1): a card enters as a copy of an
// original that is destroyed in the SAME batch, in both submission orders: the call succeeds, the
// new object is a copy of the original, the original ends in the graveyard, and the copy is part
// of the one entry batch (no BecomeCopy event). A Skip effect on BecomeCopy is not applied.
#[test]
fn req_event_001_r2_g1_copy_source_leaves_in_same_batch_zone_change() {
    for entering_first in [true, false] {
        let mut w = World::new();
        let card = w.create(ZoneKind::Hand, P1);
        let original = w.create(ZoneKind::Battlefield, P2);
        let enter = zone_change(card, ZoneKind::Battlefield, ZoneChangeCause::Other);
        let events = if entering_first {
            vec![enter, destroy(original)]
        } else {
            vec![destroy(original), enter]
        };
        let r = run_batch(
            Bus::new(w, vec![copy_on_entry(card, original), skip_become_copy()]),
            events,
            false,
            &[],
        );
        let ctx = format!("entering first: {entering_first}");
        assert!(r.result.is_ok(), "{ctx}: {:?}", r.result);
        assert_eq!(r.applications(), eids(&[1]), "{ctx}");
        let executed = assert_one_batch_no_become_copy(&r);
        assert_eq!(executed.len(), 2, "{ctx}");
        let entered = entered_id(&executed, original);
        assert_ne!(entered, card, "{ctx}");
        assert_eq!(r.world().zone_of(entered), Some(ZoneKind::Battlefield));
        assert_eq!(
            r.world().events.markers().copy_of(entered),
            Some(original),
            "{ctx}"
        );
        assert_eq!(
            original_moved_to(&r, original),
            Some(ZoneKind::Graveyard),
            "{ctx}"
        );
    }
}

// REQ-EVENT-001, REQ-CORE-003, CR 707.5, 608.2h (G1): the same for a token created as a copy of
// an original destroyed in the same batch, in both submission orders.
#[test]
fn req_event_001_r2_g1_copy_source_leaves_in_same_batch_token() {
    for token_first in [true, false] {
        let mut w = World::new();
        let original = w.create(ZoneKind::Battlefield, P2);
        let card = w.fresh_card();
        let create = GameEvent::CreateToken {
            owner: P1,
            card,
            controller: None,
        };
        let events = if token_first {
            vec![create, destroy(original)]
        } else {
            vec![destroy(original), create]
        };
        let r = run_batch(
            Bus::new(w, vec![copy_on_token(original), skip_become_copy()]),
            events,
            false,
            &[],
        );
        let ctx = format!("token first: {token_first}");
        assert!(r.result.is_ok(), "{ctx}: {:?}", r.result);
        assert_eq!(r.applications(), eids(&[1]), "{ctx}");
        let executed = assert_one_batch_no_become_copy(&r);
        assert_eq!(executed.len(), 2, "{ctx}");
        let token = entered_id(&executed, original);
        assert_eq!(r.world().zone_of(token), Some(ZoneKind::Battlefield));
        assert_eq!(
            r.world().events.markers().copy_of(token),
            Some(original),
            "{ctx}"
        );
        assert_eq!(
            original_moved_to(&r, original),
            Some(ZoneKind::Graveyard),
            "{ctx}"
        );
    }
}

// REQ-EVENT-001, REQ-CORE-003, CR 707.5, 608.2h (G1): the copy source stays on the battlefield
// (current information): a zone-change entry and a token entry each succeed with one entry
// batch, no BecomeCopy event, and the copy link set; the original is untouched.
#[test]
fn req_event_001_r2_g1_copy_source_stays() {
    // Zone change.
    let mut w = World::new();
    let card = w.create(ZoneKind::Hand, P1);
    let original = w.create(ZoneKind::Battlefield, P2);
    let r = run_event(
        Bus::new(w, vec![copy_on_entry(card, original)]),
        zone_change(card, ZoneKind::Battlefield, ZoneChangeCause::Other),
        &[],
    );
    let executed = assert_one_batch_no_become_copy(&r);
    assert_eq!(executed.len(), 1);
    let entered = new_id(&executed[0].outcome);
    assert_eq!(r.world().events.markers().copy_of(entered), Some(original));
    assert_eq!(r.world().zone_of(original), Some(ZoneKind::Battlefield));

    // Token.
    let mut w = World::new();
    let original = w.create(ZoneKind::Battlefield, P2);
    let card = w.fresh_card();
    let r = run_event(
        Bus::new(w, vec![copy_on_token(original)]),
        GameEvent::CreateToken {
            owner: P1,
            card,
            controller: None,
        },
        &[],
    );
    let executed = assert_one_batch_no_become_copy(&r);
    assert_eq!(executed.len(), 1);
    let token = new_id(&executed[0].outcome);
    assert_eq!(r.world().events.markers().copy_of(token), Some(original));
    assert_eq!(r.world().zone_of(original), Some(ZoneKind::Battlefield));
}

// REQ-EVENT-001, REQ-CORE-003, CR 707.5, 608.2h (G1): the lenient source check is only for the
// entry copy: a standalone BecomeCopy whose copied object left the battlefield in an EARLIER
// call fails with `BusError::Event(_)` and changes nothing.
#[test]
fn req_event_001_r2_g1_standalone_become_copy_of_departed_source_fails() {
    let mut w = World::new();
    let obj = w.create(ZoneKind::Battlefield, P1);
    let original = w.create(ZoneKind::Battlefield, P2);
    // The earlier call: the original is destroyed through the bus.
    let first = run_event(Bus::new(w, Vec::new()), destroy(original), &[]);
    assert!(first.result.is_ok(), "{:?}", first.result);
    assert_eq!(first.world().zone_of(original), None);
    // The later call: a standalone BecomeCopy of the departed original.
    let r = run_event(
        first.after.clone(),
        GameEvent::BecomeCopy {
            object: obj,
            copied: original,
        },
        &[],
    );
    assert!(matches!(r.error(), BusError::Event(_)), "{:?}", r.error());
    assert_unchanged(&r);
    assert_eq!(r.world().events.markers().copy_of(obj), None);
}

// REQ-EVENT-001, REQ-CORE-003, CR 707.5 (G1), REQ-CORE-001: the same-batch departure case is
// deterministic: a re-run of either submission order gives identical output, state and hash.
#[test]
fn req_event_001_r2_g1_same_batch_departure_is_deterministic() {
    for entering_first in [true, false] {
        let mut w = World::new();
        let card = w.create(ZoneKind::Hand, P1);
        let original = w.create(ZoneKind::Battlefield, P2);
        let enter = zone_change(card, ZoneKind::Battlefield, ZoneChangeCause::Other);
        let events = if entering_first {
            vec![enter, destroy(original)]
        } else {
            vec![destroy(original), enter]
        };
        let base = Bus::new(w, vec![copy_on_entry(card, original)]);
        let call = Call::Batch(EventBatch::from_events(events), false);
        let a = run(base.clone(), call.clone(), &[]);
        let b = run(base, call, &[]);
        assert_eq!(a.result, b.result);
        assert_eq!(a.after.canonical(), b.after.canonical());
        assert_eq!(a.hash, b.hash);
    }
}
