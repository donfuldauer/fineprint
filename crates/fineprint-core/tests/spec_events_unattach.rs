// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the lost-attachment-link records of executed events (REQ-EVENT-001,
//! REQ-EVENT-002, REQ-CORE-006; CR 701.3a, 701.3b, 701.3c, 701.3d, 702.26i, 702.26j): every
//! link an event ends is reported exactly once in `ExecutedEvent::unattached`.
//! Black-box: written from the work-item brief only.

#[path = "spec_events_support.rs"]
mod support;

use std::collections::BTreeSet;

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::ids::{ObjectId, Timestamp};
use fineprint_core::events::{
    EventError, EventInstance, ExecutedBatch, ExecutedEvent, GameEvent, Outcome, UnattachRecord,
    ZoneChangeCause,
};
use fineprint_core::permanent::AttachTarget;
use fineprint_core::zones::{MoveRequest, Moved, Position, ZoneKind};
use support::*;

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

fn unattach(attachment: ObjectId) -> GameEvent {
    GameEvent::Unattach { attachment }
}

fn unattach_from(attachment: ObjectId, host: AttachTarget) -> GameEvent {
    GameEvent::UnattachFrom { attachment, host }
}

fn zc(object: ObjectId, to: ZoneKind) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause: ZoneChangeCause::Other,
    }
}

fn moved_of(e: &ExecutedEvent) -> Moved {
    match &e.outcome {
        Outcome::ZoneChange(m) => *m,
        other => panic!("expected a ZoneChange outcome, got {other:?}"),
    }
}

fn timestamp_of(e: &ExecutedEvent) -> Timestamp {
    match &e.outcome {
        Outcome::Attached { timestamp } => *timestamp,
        other => panic!("expected an Attached outcome, got {other:?}"),
    }
}

fn bf(w: &mut World) -> ObjectId {
    w.create(ZoneKind::Battlefield, P1)
}

fn total_records(b: &ExecutedBatch) -> Vec<UnattachRecord> {
    b.events
        .iter()
        .flat_map(|e| e.unattached.iter().copied())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// 1. attach
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, CR 701.3a, 701.3c, 701.3d: moving to a different host ends the old link (one
// record with the previous host) and issues a strictly later timestamp.
#[test]
fn req_event_001_attach_to_different_host_records_previous_host() {
    let mut w = World::new();
    let (a, b, c) = (bf(&mut w), bf(&mut w), bf(&mut w));
    let first = w.ok(attach(a, on(b)));
    assert!(first.unattached.is_empty(), "first attach loses no link");
    let second = w.ok(attach(a, on(c)));
    assert_eq!(second.unattached, vec![rec(a, on(b))]);
    assert!(timestamp_of(&second) > timestamp_of(&first));
    assert_eq!(w.permanents.attached_to(a), Some(on(c)));
}

// REQ-EVENT-001, CR 701.3a: a first attach records nothing.
#[test]
fn req_event_001_first_attach_records_nothing() {
    let mut w = World::new();
    let (a, b) = (bf(&mut w), bf(&mut w));
    let done = w.ok(attach(a, on(b)));
    assert_eq!(done.unattached, Vec::<UnattachRecord>::new());
    let c = bf(&mut w);
    let p = w.ok(attach(c, AttachTarget::Player(P2)));
    assert!(p.unattached.is_empty());
}

// REQ-EVENT-001, CR 701.3b: attaching to the current host is no event; nothing is logged.
#[test]
fn req_event_001_same_host_attach_is_no_event() {
    let mut w = World::new();
    let (a, b) = (bf(&mut w), bf(&mut w));
    w.ok(attach(a, on(b)));
    let before = w.snap();
    assert_eq!(w.exec(attach(a, on(b))), Ok(None));
    assert_eq!(w.snap(), before, "nothing logged, nothing changed");
    let p = bf(&mut w);
    w.ok(attach(p, AttachTarget::Player(P2)));
    let before = w.snap();
    assert_eq!(w.exec(attach(p, AttachTarget::Player(P2))), Ok(None));
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 701.3a, 701.3d: player hosts: player to permanent, permanent to player.
#[test]
fn req_event_001_player_host_transitions() {
    let mut w = World::new();
    let (a, b) = (bf(&mut w), bf(&mut w));
    assert!(
        w.ok(attach(a, AttachTarget::Player(P1)))
            .unattached
            .is_empty()
    );
    let to_perm = w.ok(attach(a, on(b)));
    assert_eq!(to_perm.unattached, vec![rec(a, AttachTarget::Player(P1))]);
    let to_player = w.ok(attach(a, AttachTarget::Player(P2)));
    assert_eq!(to_player.unattached, vec![rec(a, on(b))]);
    let player_to_player = w.ok(attach(a, AttachTarget::Player(P1)));
    assert_eq!(
        player_to_player.unattached,
        vec![rec(a, AttachTarget::Player(P2))]
    );
}

// ---------------------------------------------------------------------------------------------
// 2. leaving the battlefield
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, CR 701.3d: a host leaving yields one record per attachment, ascending id,
// whatever order the attachments were attached in (both orders).
#[test]
fn req_event_001_host_leaves_one_record_per_attachment_ascending() {
    for reversed in [false, true] {
        let mut w = World::new();
        let host = bf(&mut w);
        let (a1, a2, a3) = (bf(&mut w), bf(&mut w), bf(&mut w));
        let mut order = vec![a1, a2, a3];
        if reversed {
            order.reverse();
        }
        for att in order {
            w.ok(attach(att, on(host)));
        }
        let done = w.ok(zc(host, ZoneKind::Graveyard));
        assert_eq!(
            done.unattached,
            vec![rec(a1, on(host)), rec(a2, on(host)), rec(a3, on(host))],
            "reversed={reversed}"
        );
        for att in [a1, a2, a3] {
            assert_eq!(w.permanents.attached_to(att), None);
        }
    }
}

// REQ-EVENT-001, CR 701.3d: a host with its own host records its own link first, then those
// attached to it, even when an attachment has a lower id.
#[test]
fn req_event_001_leaving_host_that_is_itself_attached_records_own_link_first() {
    let mut w = World::new();
    let low = bf(&mut w);
    let g = bf(&mut w);
    let h = bf(&mut w);
    assert!(low < h);
    w.ok(attach(h, on(g)));
    w.ok(attach(low, on(h)));
    let done = w.ok(zc(h, ZoneKind::Graveyard));
    assert_eq!(done.unattached, vec![rec(h, on(g)), rec(low, on(h))]);
}

// REQ-EVENT-001, CR 701.3d: an attachment leaving the battlefield records its OLD id.
#[test]
fn req_event_001_attachment_leaves_records_old_id() {
    let mut w = World::new();
    let (host, att) = (bf(&mut w), bf(&mut w));
    w.ok(attach(att, on(host)));
    let done = w.ok(zc(att, ZoneKind::Graveyard));
    let m = moved_of(&done);
    assert_eq!(m.old, att);
    assert_ne!(m.new, m.old, "the object gets a new id");
    assert_eq!(done.unattached, vec![rec(m.old, on(host))]);
}

// REQ-EVENT-001, CR 701.3d: a player-hosted attachment leaving records the player host.
#[test]
fn req_event_001_attachment_on_player_leaves() {
    let mut w = World::new();
    let att = bf(&mut w);
    w.ok(attach(att, AttachTarget::Player(P2)));
    let done = w.ok(zc(att, ZoneKind::Exile));
    assert_eq!(done.unattached, vec![rec(att, AttachTarget::Player(P2))]);
}

// REQ-EVENT-001, CR 701.3d: a leaving object that is both attached and a host records the host
// link, then each attachment, with the old ids.
#[test]
fn req_event_001_leaving_middle_of_a_chain() {
    let mut w = World::new();
    let (g, h, x) = (bf(&mut w), bf(&mut w), bf(&mut w));
    w.ok(attach(h, on(g)));
    w.ok(attach(x, on(h)));
    let done = w.ok(zc(h, ZoneKind::Hand));
    assert_eq!(done.unattached, vec![rec(h, on(g)), rec(x, on(h))]);
    assert_eq!(w.permanents.attached_to(x), None);
}

// REQ-EVENT-001, CR 701.3d: events that do not leave the battlefield record nothing.
#[test]
fn req_event_001_zone_changes_not_leaving_battlefield_record_nothing() {
    let mut w = World::new();
    let hand = w.create(ZoneKind::Hand, P1);
    let gy = w.create(ZoneKind::Graveyard, P1);
    let lib = w.create(ZoneKind::Library, P1);
    let _lib2 = w.create(ZoneKind::Library, P1);
    // leaving a non-battlefield zone
    assert!(w.ok(zc(hand, ZoneKind::Graveyard)).unattached.is_empty());
    // entering the battlefield
    assert!(w.ok(zc(gy, ZoneKind::Battlefield)).unattached.is_empty());
    // same-zone reposition
    let mut bottom = MoveRequest::new(ZoneKind::Library);
    bottom.position = Position::Bottom;
    let done = w.ok(GameEvent::ZoneChange {
        object: lib,
        request: bottom,
        cause: ZoneChangeCause::Other,
    });
    assert!(done.unattached.is_empty());
    // a draw
    let drawn = w.ok(GameEvent::Draw { player: P1 });
    assert!(drawn.unattached.is_empty());
}

// REQ-EVENT-001, CR 702.26i, 702.26j, 701.26: tapping and untapping end no link.
#[test]
fn req_event_001_tap_untap_record_nothing() {
    let mut w = World::new();
    let (host, att) = (bf(&mut w), bf(&mut w));
    w.ok(attach(att, on(host)));
    assert!(w.ok(GameEvent::Tap { object: host }).unattached.is_empty());
    assert!(w.ok(GameEvent::Tap { object: att }).unattached.is_empty());
    assert!(
        w.ok(GameEvent::Untap { object: host })
            .unattached
            .is_empty()
    );
    assert_eq!(w.permanents.attached_to(att), Some(on(host)));
}

// ---------------------------------------------------------------------------------------------
// 3. explicit Unattach and UnattachFrom
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, CR 701.3d: Unattach clears whatever host is current and logs one record.
#[test]
fn req_event_001_unattach_logs_one_record() {
    let mut w = World::new();
    let (host, att) = (bf(&mut w), bf(&mut w));
    w.ok(attach(att, on(host)));
    let done = w.ok(unattach(att));
    assert_eq!(done.unattached, vec![rec(att, on(host))]);
    assert_eq!(w.permanents.attached_to(att), None);
    // nothing left to clear: no event
    assert_eq!(w.exec(unattach(att)), Ok(None));
}

// REQ-EVENT-001, CR 701.3d: UnattachFrom with the matching host clears the link, one record.
#[test]
fn req_event_001_unattach_from_matching_host() {
    let mut w = World::new();
    let (host, att) = (bf(&mut w), bf(&mut w));
    w.ok(attach(att, on(host)));
    let done = w.ok(unattach_from(att, on(host)));
    assert_eq!(done.unattached, vec![rec(att, on(host))]);
    assert_eq!(w.permanents.attached_to(att), None);

    w.ok(attach(att, AttachTarget::Player(P2)));
    let done = w.ok(unattach_from(att, AttachTarget::Player(P2)));
    assert_eq!(done.unattached, vec![rec(att, AttachTarget::Player(P2))]);
}

// REQ-EVENT-001, CR 701.3d: UnattachFrom with a different host is no event and leaves the link.
#[test]
fn req_event_001_unattach_from_wrong_host_is_no_event() {
    let mut w = World::new();
    let (host, other, att) = (bf(&mut w), bf(&mut w), bf(&mut w));
    w.ok(attach(att, on(host)));
    let before = w.snap();
    assert_eq!(w.exec(unattach_from(att, on(other))), Ok(None));
    assert_eq!(
        w.exec(unattach_from(att, AttachTarget::Player(P1))),
        Ok(None)
    );
    assert_eq!(w.snap(), before);
    // the link is intact: a later Unattach still logs it
    let done = w.ok(unattach(att));
    assert_eq!(done.unattached, vec![rec(att, on(host))]);
}

// REQ-EVENT-001, CR 701.3d: UnattachFrom of an unattached object is no event.
#[test]
fn req_event_001_unattach_from_unattached_is_no_event() {
    let mut w = World::new();
    let (host, att) = (bf(&mut w), bf(&mut w));
    let before = w.snap();
    assert_eq!(w.exec(unattach_from(att, on(host))), Ok(None));
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001: an unknown attachment is an error for both forms.
#[test]
fn req_event_001_unattach_unknown_object_errors() {
    let mut w = World::new();
    let host = bf(&mut w);
    let before = w.snap();
    assert_eq!(
        w.exec(unattach_from(GHOST, on(host))),
        Err(EventError::UnknownObject(GHOST))
    );
    assert_eq!(
        w.exec(unattach(GHOST)),
        Err(EventError::UnknownObject(GHOST))
    );
    assert_eq!(w.snap(), before);
}

// ---------------------------------------------------------------------------------------------
// 4. no duplicates, rollback
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, REQ-EVENT-002, CR 701.3d: Unattach then the host leaving reports the link once,
// on the Unattach event.
#[test]
fn req_event_001_unattach_then_host_leaves_reports_once() {
    let mut w = World::new();
    let (host, att) = (bf(&mut w), bf(&mut w));
    w.ok(attach(att, on(host)));
    let b = w
        .batch(vec![unattach(att), zc(host, ZoneKind::Graveyard)])
        .expect("batch");
    assert_eq!(b.events[0].unattached, vec![rec(att, on(host))]);
    assert!(b.events[1].unattached.is_empty());
    assert_eq!(total_records(&b).len(), 1);
    assert_eq!(w.permanents.attached_to(att), None);
}

// REQ-EVENT-001, REQ-EVENT-002, CR 701.3d: Unattach then the attachment itself leaving.
#[test]
fn req_event_001_unattach_then_attachment_leaves_reports_once() {
    let mut w = World::new();
    let (host, att) = (bf(&mut w), bf(&mut w));
    w.ok(attach(att, on(host)));
    let b = w
        .batch(vec![unattach(att), zc(att, ZoneKind::Graveyard)])
        .expect("batch");
    assert_eq!(b.events[0].unattached, vec![rec(att, on(host))]);
    assert!(b.events[1].unattached.is_empty());
    assert_eq!(total_records(&b).len(), 1);
    assert_eq!(w.permanents.attached_to(att), None);
}

// REQ-EVENT-001, CR 701.3d: two attachments of one host plus the host leaving: each link once,
// in both attach orders and with an Unattach for just one of them first.
#[test]
fn req_event_001_two_attachments_each_link_once() {
    for reversed in [false, true] {
        let mut w = World::new();
        let host = bf(&mut w);
        let (a, b) = (bf(&mut w), bf(&mut w));
        let mut order = vec![a, b];
        if reversed {
            order.reverse();
        }
        for att in order {
            w.ok(attach(att, on(host)));
        }
        let mut w2 = w.clone();
        let done = w.batch(vec![zc(host, ZoneKind::Graveyard)]).expect("batch");
        assert_eq!(
            total_records(&done),
            vec![rec(a, on(host)), rec(b, on(host))]
        );

        let done = w2
            .batch(vec![unattach(b), zc(host, ZoneKind::Graveyard)])
            .expect("batch");
        assert_eq!(done.events[0].unattached, vec![rec(b, on(host))]);
        assert_eq!(done.events[1].unattached, vec![rec(a, on(host))]);
        let all = total_records(&done);
        let unique: BTreeSet<_> = all.iter().map(|r| (r.attachment, r.host)).collect();
        assert_eq!(unique.len(), all.len(), "no record repeats");
    }
}

// REQ-EVENT-001, REQ-CORE-006: a batch whose last event fails leaves the link and the log alone.
#[test]
fn req_event_001_failing_batch_rolls_back_links_and_log() {
    let mut w = World::new();
    let (host, att) = (bf(&mut w), bf(&mut w));
    w.ok(attach(att, on(host)));
    let before = w.snap();
    let result = w.batch(vec![
        unattach(att),
        zc(host, ZoneKind::Graveyard),
        tap_ghost(),
    ]);
    assert_eq!(result, Err(EventError::UnknownObject(GHOST)));
    assert_eq!(w.snap(), before);
    assert_eq!(w.permanents.attached_to(att), Some(on(host)));
    // the link is still reportable afterwards
    let done = w.ok(unattach(att));
    assert_eq!(done.unattached, vec![rec(att, on(host))]);
}

fn tap_ghost() -> GameEvent {
    GameEvent::Tap { object: GHOST }
}

// ---------------------------------------------------------------------------------------------
// 5. encoding
// ---------------------------------------------------------------------------------------------

fn base_event(unattached: Vec<UnattachRecord>) -> ExecutedEvent {
    ExecutedEvent {
        instance: EventInstance::new(GameEvent::Draw { player: P1 }),
        outcome: Outcome::Applied,
        lki_captured: false,
        unattached,
    }
}

fn record_bytes(r: &UnattachRecord) -> Vec<u8> {
    let mut out = bytes(&r.attachment);
    out.extend(bytes(&r.host));
    out
}

// REQ-CORE-006: an empty-records event is the previous encoding plus an empty sequence.
#[test]
fn req_core_006_executed_event_encodes_records_last() {
    let empty = base_event(vec![]);
    let mut previous = bytes(&empty.instance);
    previous.extend(bytes(&empty.outcome));
    previous.extend(bytes(&empty.lki_captured));
    let mut expected_empty = previous.clone();
    expected_empty.extend(bytes(&Vec::<u8>::new()));
    assert_eq!(bytes(&empty), expected_empty);

    let records = vec![
        rec(oid(3), on(oid(4))),
        rec(oid(1), AttachTarget::Player(P2)),
    ];
    let full = base_event(records.clone());
    let mut expected = previous;
    expected.extend(2u32.to_le_bytes());
    for r in &records {
        expected.extend(record_bytes(r));
    }
    assert_eq!(bytes(&full), expected);
    assert_eq!(bytes(&records[0]), record_bytes(&records[0]));
}

// REQ-CORE-006: record order and each record field are significant.
#[test]
fn req_core_006_distinct_records_distinct_encodings() {
    let r1 = rec(oid(1), on(oid(2)));
    let r2 = rec(oid(2), on(oid(1)));
    let r3 = rec(oid(1), AttachTarget::Player(P1));
    let r4 = rec(oid(1), on(oid(3)));
    let variants = [
        base_event(vec![]),
        base_event(vec![r1]),
        base_event(vec![r2]),
        base_event(vec![r3]),
        base_event(vec![r4]),
        base_event(vec![r1, r2]),
        base_event(vec![r2, r1]),
        base_event(vec![r1, r1]),
    ];
    let encoded: Vec<Vec<u8>> = variants.iter().map(bytes).collect();
    for i in 0..encoded.len() {
        for j in (i + 1)..encoded.len() {
            assert_ne!(encoded[i], encoded[j], "{i} vs {j}");
        }
    }
    let plain = [r1, r2, r3, r4];
    for (i, a) in plain.iter().enumerate() {
        for b in &plain[i + 1..] {
            assert_ne!(bytes(a), bytes(b));
        }
    }
}

// REQ-CORE-006, REQ-EVENT-001: GameEvent::UnattachFrom is tag 17, attachment, host.
#[test]
fn req_core_006_unattach_from_encoding() {
    let ev = unattach_from(oid(5), on(oid(6)));
    let mut expected = vec![17u8];
    expected.extend(bytes(&oid(5)));
    expected.extend(bytes(&on(oid(6))));
    assert_eq!(bytes(&ev), expected);

    assert_ne!(bytes(&ev), bytes(&unattach(oid(5))));
    assert_ne!(bytes(&ev), bytes(&unattach_from(oid(5), on(oid(7)))));
    assert_ne!(
        bytes(&ev),
        bytes(&unattach_from(oid(5), AttachTarget::Player(P1)))
    );
    assert_ne!(bytes(&ev), bytes(&unattach_from(oid(4), on(oid(6)))));
    // existing tags are unchanged
    assert_eq!(bytes(&unattach(oid(5)))[0], 13);
    assert_eq!(bytes(&attach(oid(5), on(oid(6))))[0], 12);
}

// REQ-CORE-006: a real log is byte-identical when the same script runs twice.
#[test]
fn req_core_006_same_script_same_log_bytes() {
    let run = || {
        let mut w = World::new();
        let (h, a, b) = (bf(&mut w), bf(&mut w), bf(&mut w));
        let batch = w
            .batch(vec![
                attach(a, on(h)),
                attach(b, on(h)),
                attach(a, AttachTarget::Player(P1)),
                zc(h, ZoneKind::Graveyard),
            ])
            .expect("batch");
        bytes(&batch)
    };
    assert_eq!(run(), run());
}

// ---------------------------------------------------------------------------------------------
// 8. property test (fixed seeds, local deterministic generator as spec_events_property.rs)
// ---------------------------------------------------------------------------------------------

type Links = BTreeSet<(ObjectId, AttachTarget)>;

fn links_of(w: &World) -> Links {
    let mut out = BTreeSet::new();
    for id in w.permanents.ids() {
        if let Some(host) = w.permanents.attached_to(id) {
            out.insert((id, host));
        }
    }
    out
}

fn pick_perm(rng: &mut Lcg, w: &World) -> ObjectId {
    let ids = w.permanents.ids();
    if ids.is_empty() || rng.below(25) == 0 {
        return oid(rng.below(40) + 1);
    }
    rng.pick(&ids)
}

fn random_batch(rng: &mut Lcg, w: &World) -> Vec<GameEvent> {
    let n = rng.below(3) + 1;
    (0..n)
        .map(|_| match rng.below(7) {
            0 | 1 => attach(pick_perm(rng, w), on(pick_perm(rng, w))),
            2 => attach(pick_perm(rng, w), AttachTarget::Player(rng.pick(&[P1, P2]))),
            3 => unattach(pick_perm(rng, w)),
            4 => {
                let host = if rng.below(4) == 0 {
                    AttachTarget::Player(rng.pick(&[P1, P2]))
                } else {
                    on(pick_perm(rng, w))
                };
                unattach_from(pick_perm(rng, w), host)
            }
            5 => zc(
                pick_perm(rng, w),
                rng.pick(&[ZoneKind::Graveyard, ZoneKind::Hand, ZoneKind::Exile]),
            ),
            _ => {
                let ids = w.zones.all_object_ids();
                let obj = if ids.is_empty() {
                    oid(1)
                } else {
                    rng.pick(&ids)
                };
                zc(obj, ZoneKind::Battlefield)
            }
        })
        .collect()
}

fn fresh_world() -> World {
    let mut w = World::new();
    for p in [P1, P2] {
        for _ in 0..4 {
            w.create(ZoneKind::Battlefield, p);
        }
        w.create(ZoneKind::Hand, p);
        w.create(ZoneKind::Graveyard, p);
    }
    w
}

/// Runs one seed; returns the canonical bytes of every executed batch.
fn run_seed(seed: u64, steps: usize) -> (Vec<u8>, usize) {
    let mut rng = Lcg::new(seed);
    let mut w = fresh_world();
    let mut log = Vec::new();
    let mut count = 0;
    for _ in 0..steps {
        let events = random_batch(&mut rng, &w);
        let has_attach = events.iter().any(|e| matches!(e, GameEvent::Attach { .. }));
        let before_snap = w.snap();
        let mut links = links_of(&w);
        match w.batch(events) {
            Err(_) => assert_eq!(w.snap(), before_snap, "a failed batch changes nothing"),
            Ok(done) => {
                let mut seen = BTreeSet::new();
                for e in &done.events {
                    for r in &e.unattached {
                        count += 1;
                        let pair = (r.attachment, r.host);
                        assert!(
                            links.remove(&pair),
                            "seed {seed}: record {r:?} is not a live link ({e:?})"
                        );
                        if !has_attach {
                            assert!(seen.insert(pair), "seed {seed}: repeated record {r:?}");
                        }
                    }
                    if let (GameEvent::Attach { attachment, target }, Outcome::Attached { .. }) =
                        (e.instance.event(), &e.outcome)
                    {
                        links.insert((*attachment, *target));
                    }
                }
                assert_eq!(links, links_of(&w), "seed {seed}: link set bookkeeping");
                done.encode_canonical(&mut log);
            }
        }
    }
    (log, count)
}

// REQ-EVENT-001, REQ-CORE-006, CR 701.3d: before - records + created == after at every step;
// no record repeats within an attach-free batch; the same seed gives byte-identical logs.
#[test]
fn req_event_001_property_records_account_for_every_lost_link() {
    let mut total = 0;
    for seed in [1u64, 2, 3, 0x27, 0xFEED, 0xDEAD_BEEF, 77, 2026, 31337, 4242] {
        let (first, n) = run_seed(seed, 150);
        let (second, _) = run_seed(seed, 150);
        assert_eq!(first, second, "seed {seed}: replay is byte-identical");
        total += n;
    }
    assert!(total > 0, "the generator must exercise lost links");
}
