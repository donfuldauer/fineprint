// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `UNATTACHED` trigger events built from executed attachment-link records
//! (REQ-EVENT-002, REQ-EVENT-001; CR 701.3a, 701.3c, 701.3d, 603.10c, 702.26i, 702.26j).
//! Black-box: written from the work-item brief only.

#[path = "spec_triggers_support.rs"]
mod support;

#[path = "spec_events_support.rs"]
mod esupport;

use fineprint_base::ids::{ObjectId, Timestamp};
use fineprint_core::events::{
    EventInstance, ExecutedBatch, ExecutedEvent, GameEvent, Outcome, UnattachRecord,
    ZoneChangeCause,
};
use fineprint_core::permanent::AttachTarget;
use fineprint_core::triggers::{
    StateView, TriggerEvent, events_from_batch, events_from_batch_with_lki,
};
use fineprint_core::zones::{MoveRequest, Moved, ZoneId, ZoneKind};
use fineprint_dsl::ability::EventKind;

use esupport::World;
use support::{FakeEnv, P1, ability, m, reg, run, summary};

use EventKind as K;

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn rec(attachment: ObjectId, host: AttachTarget) -> UnattachRecord {
    UnattachRecord { attachment, host }
}

fn on(host: ObjectId) -> AttachTarget {
    AttachTarget::Permanent(host)
}

fn attach(attachment: ObjectId, target: AttachTarget) -> GameEvent {
    GameEvent::Attach { attachment, target }
}

fn zc(object: ObjectId, to: ZoneKind) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause: ZoneChangeCause::Other,
    }
}

fn bf(w: &mut World) -> ObjectId {
    w.create(ZoneKind::Battlefield, P1)
}

fn is_unattached(t: &TriggerEvent) -> bool {
    t.kinds.contains(&K::Unattached)
}

fn is_attached(t: &TriggerEvent) -> bool {
    t.kinds.contains(&K::Attached)
}

fn same_object(t: &TriggerEvent, id: ObjectId) -> bool {
    t.subject_pre == Some(id) && t.subject_post == Some(id)
}

/// Executes `events` as one batch and maps it with and without the look-back store; both must
/// agree (CR 603.10c).
fn mapped(w: &mut World, events: Vec<GameEvent>) -> (ExecutedBatch, Vec<TriggerEvent>) {
    let batch = w.batch(events).expect("batch executes");
    let plain = events_from_batch(&batch);
    let with_lki = events_from_batch_with_lki(&batch, &w.lki);
    assert_eq!(plain.len(), with_lki.len());
    for (a, b) in plain.iter().zip(&with_lki) {
        assert_eq!(a.kinds, b.kinds);
        assert_eq!(
            (a.subject_pre, a.subject_post),
            (b.subject_pre, b.subject_post)
        );
    }
    (batch, with_lki)
}

fn hand_event(
    event: GameEvent,
    outcome: Outcome,
    unattached: Vec<UnattachRecord>,
) -> ExecutedEvent {
    ExecutedEvent {
        instance: EventInstance::new(event),
        outcome,
        lki_captured: false,
        unattached,
    }
}

fn hand_batch(events: Vec<ExecutedEvent>) -> ExecutedBatch {
    ExecutedBatch {
        index: 0,
        turn: 1,
        events,
    }
}

fn unattached_subjects(out: &[TriggerEvent]) -> Vec<Option<ObjectId>> {
    out.iter()
        .filter(|t| is_unattached(t))
        .map(|t| t.subject_pre)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// real executed logs
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002, CR 701.3d, 603.10c: moving equipment from A to B gives ATTACHED then UNATTACHED
// for the equipment.
#[test]
fn req_event_002_equipment_moves_between_hosts() {
    let mut w = World::new();
    let (eq, a, b) = (bf(&mut w), bf(&mut w), bf(&mut w));
    let (_, first) = mapped(&mut w, vec![attach(eq, on(a))]);
    assert_eq!(first.len(), 1);
    assert!(is_attached(&first[0]) && !is_unattached(&first[0]));

    let (_, out) = mapped(&mut w, vec![attach(eq, on(b))]);
    assert_eq!(out.len(), 2);
    assert!(is_attached(&out[0]) && !is_unattached(&out[0]));
    assert!(is_unattached(&out[1]) && !is_attached(&out[1]));
    assert!(same_object(&out[0], eq));
    assert!(same_object(&out[1], eq));
}

// REQ-EVENT-002, CR 701.3a: an initial attach emits only ATTACHED; same-host emits nothing.
#[test]
fn req_event_002_initial_attach_only_attached_same_host_nothing() {
    let mut w = World::new();
    let (eq, a) = (bf(&mut w), bf(&mut w));
    let (_, out) = mapped(&mut w, vec![attach(eq, on(a))]);
    assert_eq!(out.len(), 1);
    assert!(is_attached(&out[0]));
    assert!(!out.iter().any(is_unattached));

    let (batch, out) = mapped(&mut w, vec![attach(eq, on(a))]);
    assert!(batch.events.is_empty(), "no event occurred");
    assert!(out.is_empty());
}

// REQ-EVENT-002, CR 701.3d, 603.10c: a dying host gives one UNATTACHED per attachment after the
// host's own occurrences, in ascending attachment id (both attach orders).
#[test]
fn req_event_002_host_dies_one_unattached_per_attachment() {
    for reversed in [false, true] {
        let mut w = World::new();
        let host = bf(&mut w);
        let (a1, a2) = (bf(&mut w), bf(&mut w));
        let order = if reversed { [a2, a1] } else { [a1, a2] };
        for att in order {
            w.ok(attach(att, on(host)));
        }
        let (batch, out) = mapped(&mut w, vec![zc(host, ZoneKind::Graveyard)]);
        let moved = match batch.events[0].outcome {
            Outcome::ZoneChange(mv) => mv,
            ref o => panic!("{o:?}"),
        };
        let first_unattached = out.iter().position(is_unattached).expect("some UNATTACHED");
        assert!(first_unattached >= 1);
        assert!(
            out[..first_unattached]
                .iter()
                .all(|t| t.kinds.contains(&K::ZoneChange) && t.subject_pre == Some(moved.old))
        );
        assert!(out[first_unattached..].iter().all(is_unattached));
        assert_eq!(
            unattached_subjects(&out),
            vec![Some(a1), Some(a2)],
            "reversed={reversed}"
        );
        assert!(
            out[first_unattached..]
                .iter()
                .all(|t| t.subject_pre == t.subject_post)
        );
    }
}

// REQ-EVENT-002, CR 701.3d, 603.10c: a dying attachment gives one UNATTACHED with its OLD id.
#[test]
fn req_event_002_attachment_dies_uses_old_id() {
    let mut w = World::new();
    let (host, att) = (bf(&mut w), bf(&mut w));
    w.ok(attach(att, on(host)));
    let (batch, out) = mapped(&mut w, vec![zc(att, ZoneKind::Graveyard)]);
    let moved = match batch.events[0].outcome {
        Outcome::ZoneChange(mv) => mv,
        ref o => panic!("{o:?}"),
    };
    assert_ne!(moved.old, moved.new);
    assert!(w.lki.get(moved.old).is_some(), "look-back data captured");
    assert_eq!(unattached_subjects(&out), vec![Some(moved.old)]);
    let last = out.last().expect("occurrences");
    assert!(is_unattached(last));
    assert!(same_object(last, moved.old));
}

// REQ-EVENT-002, CR 701.3d: explicit Unattach and UnattachFrom give exactly one UNATTACHED each.
#[test]
fn req_event_002_explicit_unattach_variants_one_occurrence() {
    let mut w = World::new();
    let (host, att) = (bf(&mut w), bf(&mut w));
    w.ok(attach(att, on(host)));
    let (_, out) = mapped(&mut w, vec![GameEvent::Unattach { attachment: att }]);
    assert_eq!(out.len(), 1);
    assert!(is_unattached(&out[0]) && same_object(&out[0], att));

    w.ok(attach(att, on(host)));
    let (_, out) = mapped(
        &mut w,
        vec![GameEvent::UnattachFrom {
            attachment: att,
            host: on(host),
        }],
    );
    assert_eq!(out.len(), 1);
    assert!(is_unattached(&out[0]) && same_object(&out[0], att));
}

// REQ-EVENT-002, CR 701.3d: Unattach then the host leaving gives exactly one UNATTACHED for the
// attachment.
#[test]
fn req_event_002_unattach_then_host_leaves_one_unattached() {
    let mut w = World::new();
    let (host, att) = (bf(&mut w), bf(&mut w));
    w.ok(attach(att, on(host)));
    let (_, out) = mapped(
        &mut w,
        vec![
            GameEvent::Unattach { attachment: att },
            zc(host, ZoneKind::Graveyard),
        ],
    );
    assert_eq!(unattached_subjects(&out), vec![Some(att)]);
}

// ---------------------------------------------------------------------------------------------
// hand-built logs
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-002, CR 701.3d: an explicit Unattach with empty records still yields one UNATTACHED;
// records it carries are ignored (one occurrence from the event itself).
#[test]
fn req_event_002_hand_built_unattach_events() {
    let att = ObjectId::new(5);
    let host = ObjectId::new(6);
    let variants = [
        (GameEvent::Unattach { attachment: att }, vec![]),
        (
            GameEvent::Unattach { attachment: att },
            vec![rec(att, on(host))],
        ),
        (
            GameEvent::UnattachFrom {
                attachment: att,
                host: on(host),
            },
            vec![],
        ),
        (
            GameEvent::UnattachFrom {
                attachment: att,
                host: on(host),
            },
            vec![rec(att, on(host))],
        ),
    ];
    for (ev, records) in variants {
        let batch = hand_batch(vec![hand_event(ev.clone(), Outcome::Applied, records)]);
        let out = events_from_batch(&batch);
        assert_eq!(out.len(), 1, "{ev:?}");
        assert!(is_unattached(&out[0]) && same_object(&out[0], att));
    }
}

// REQ-EVENT-002, CR 701.3d: a hand-built Attach with two records gives ATTACHED then two
// UNATTACHED in record order.
#[test]
fn req_event_002_hand_built_attach_with_two_records() {
    let (att, new_host) = (ObjectId::new(5), ObjectId::new(9));
    let (r1, r2) = (ObjectId::new(8), ObjectId::new(7));
    let batch = hand_batch(vec![hand_event(
        attach(att, on(new_host)),
        Outcome::Attached {
            timestamp: Timestamp::new(3),
        },
        vec![rec(r1, on(ObjectId::new(1))), rec(r2, on(ObjectId::new(2)))],
    )]);
    let out = events_from_batch(&batch);
    assert_eq!(out.len(), 3);
    assert!(is_attached(&out[0]) && !is_unattached(&out[0]));
    assert!(same_object(&out[0], att));
    assert!(is_unattached(&out[1]) && same_object(&out[1], r1));
    assert!(is_unattached(&out[2]) && same_object(&out[2], r2));
}

// REQ-EVENT-002, CR 701.3d: a hand-built zone change with records yields its own occurrences,
// then the UNATTACHED ones in record order.
#[test]
fn req_event_002_hand_built_zone_change_with_records() {
    let (old, new) = (ObjectId::new(4), ObjectId::new(40));
    let moved = Moved {
        old,
        new,
        from: ZoneId::of(ZoneKind::Battlefield, P1),
        to: ZoneId::of(ZoneKind::Graveyard, P1),
    };
    let build = |records: Vec<UnattachRecord>| {
        let mut e = hand_event(
            zc(old, ZoneKind::Graveyard),
            Outcome::ZoneChange(moved),
            records,
        );
        e.lki_captured = true;
        hand_batch(vec![e])
    };
    let bare = events_from_batch(&build(vec![]));
    assert!(!bare.is_empty());
    assert!(!bare.iter().any(is_unattached));

    let (x, y) = (ObjectId::new(10), ObjectId::new(11));
    let out = events_from_batch(&build(vec![rec(x, on(old)), rec(y, on(old))]));
    assert_eq!(out.len(), bare.len() + 2);
    for (a, b) in bare.iter().zip(&out) {
        assert_eq!(a.kinds, b.kinds);
        assert_eq!(
            (a.subject_pre, a.subject_post),
            (b.subject_pre, b.subject_post)
        );
    }
    assert!(is_unattached(&out[bare.len()]) && same_object(&out[bare.len()], x));
    assert!(is_unattached(&out[bare.len() + 1]) && same_object(&out[bare.len() + 1], y));
}

// ---------------------------------------------------------------------------------------------
// 7. detection end to end
// ---------------------------------------------------------------------------------------------

fn unattached_env() -> FakeEnv {
    FakeEnv::new().with_pre(reg(1, P1, ability("u", m(K::Unattached))))
}

// REQ-EVENT-002, CR 603.10c: an UNATTACHED matcher triggers when equipment moves from A to B.
#[test]
fn req_event_002_matcher_triggers_when_equipment_moves() {
    let mut w = World::new();
    let (eq, a, b) = (bf(&mut w), bf(&mut w), bf(&mut w));
    w.ok(attach(eq, on(a)));
    let (_, out) = mapped(&mut w, vec![attach(eq, on(b))]);
    let idx = out.iter().position(is_unattached).expect("UNATTACHED");
    assert_eq!(
        summary(&run(&unattached_env(), &out)),
        vec![(1, "u".to_string(), vec![idx])]
    );
}

// REQ-EVENT-002, CR 603.10c, 701.3d: it triggers when the host leaves the battlefield.
#[test]
fn req_event_002_matcher_triggers_when_host_leaves() {
    let mut w = World::new();
    let (eq, host) = (bf(&mut w), bf(&mut w));
    w.ok(attach(eq, on(host)));
    let (_, out) = mapped(&mut w, vec![zc(host, ZoneKind::Graveyard)]);
    let idx = out.iter().position(is_unattached).expect("UNATTACHED");
    assert_eq!(
        summary(&run(&unattached_env(), &out)),
        vec![(1, "u".to_string(), vec![idx])]
    );
    let _ = StateView::PreEvent;
}

// REQ-EVENT-002, CR 701.3a, 701.3b: it does not trigger on an initial attach or a same-host
// attach.
#[test]
fn req_event_002_matcher_silent_on_initial_and_same_host_attach() {
    let mut w = World::new();
    let (eq, a) = (bf(&mut w), bf(&mut w));
    let (_, out) = mapped(&mut w, vec![attach(eq, on(a))]);
    assert!(run(&unattached_env(), &out).is_empty());
    let (_, out) = mapped(&mut w, vec![attach(eq, on(a))]);
    assert!(run(&unattached_env(), &out).is_empty());
}
