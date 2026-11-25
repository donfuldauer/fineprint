// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests for lost attachment links in executed events (#180 FP-143): REQ-EVENT-001 (the
// executed log reports every link an event removes), REQ-EVENT-002 (what trigger detection
// reads) and REQ-CORE-006 (canonical encoding of `UnattachRecord`, `ExecutedEvent::unattached`
// and `GameEvent::UnattachFrom`). CR 701.3b, CR 701.3c, CR 701.3d. Synthetic ids only.

use fineprint_base::encode::{CanonicalEncode, put_len, put_variant};

use super::*;

fn rec(attachment: ObjectId, host: AttachTarget) -> UnattachRecord {
    UnattachRecord { attachment, host }
}

fn on(id: ObjectId) -> AttachTarget {
    AttachTarget::Permanent(id)
}

fn attach(attachment: ObjectId, target: AttachTarget) -> GameEvent {
    GameEvent::Attach { attachment, target }
}

/// A world with `n` permanents of player 1 on the battlefield, cards 1..=n.
fn world_with(n: u64) -> (World, Vec<ObjectId>) {
    let mut w = World::new();
    let ids = (1..=n)
        .map(|card| w.create(card, 1, ZoneKind::Battlefield))
        .collect();
    (w, ids)
}

/// Executes `event`, which must occur, and returns its executed record.
fn occurs(w: &mut World, event: GameEvent) -> ExecutedEvent {
    w.exec(event).unwrap().expect("the event occurs")
}

// REQ-EVENT-001, REQ-EVENT-002: CR 701.3a, CR 701.3c, CR 701.3d. Attaching to a different
// permanent or player host reports the previous host; the attach itself is unchanged (new
// timestamp each time).
#[test]
fn req_event_001_cr_701_3d_attach_to_new_host_reports_previous() {
    let (mut w, ids) = world_with(3);
    let (a, b, eq) = (ids[0], ids[1], ids[2]);
    let first = occurs(&mut w, attach(eq, on(a)));
    let Outcome::Attached { timestamp: t1 } = first.outcome else {
        panic!("expected attached");
    };
    // A first attach ends no link.
    assert!(first.unattached.is_empty());

    let second = occurs(&mut w, attach(eq, on(b)));
    let Outcome::Attached { timestamp: t2 } = second.outcome else {
        panic!("expected attached");
    };
    assert!(t2 > t1, "CR 701.3c: a new timestamp");
    assert_eq!(second.unattached, vec![rec(eq, on(a))]);
    assert_eq!(w.permanents.attached_to(eq), Some(on(b)));

    // Permanent to player, then player to another player, then player to permanent.
    let third = occurs(&mut w, attach(eq, AttachTarget::Player(p(1))));
    assert_eq!(third.unattached, vec![rec(eq, on(b))]);
    let fourth = occurs(&mut w, attach(eq, AttachTarget::Player(p(2))));
    assert_eq!(fourth.unattached, vec![rec(eq, AttachTarget::Player(p(1)))]);
    let fifth = occurs(&mut w, attach(eq, on(a)));
    assert_eq!(fifth.unattached, vec![rec(eq, AttachTarget::Player(p(2)))]);

    // The log holds the same records.
    let logged: Vec<_> = w
        .events
        .log()
        .batches()
        .iter()
        .flat_map(|b| b.events.iter().map(|e| e.unattached.clone()))
        .collect();
    assert_eq!(
        logged,
        vec![
            first.unattached,
            second.unattached,
            third.unattached,
            fourth.unattached,
            fifth.unattached,
        ]
    );
}

// REQ-EVENT-001: CR 701.3b. Re-attaching to the current host does nothing: no event, no
// record, nothing changed (permanent and player hosts).
#[test]
fn req_event_001_cr_701_3b_same_host_attach_records_nothing() {
    let (mut w, ids) = world_with(2);
    let (a, eq) = (ids[0], ids[1]);
    occurs(&mut w, attach(eq, on(a)));
    assert_noop(&mut w, attach(eq, on(a)));
    occurs(&mut w, attach(eq, AttachTarget::Player(p(2))));
    assert_noop(&mut w, attach(eq, AttachTarget::Player(p(2))));
    // In a batch, a same-host attach is simply not in the executed list.
    let out = w
        .batch(vec![attach(eq, AttachTarget::Player(p(2)))])
        .unwrap();
    assert!(out.events.is_empty());
}

// REQ-EVENT-001, REQ-EVENT-002: CR 701.3d. A host leaving the battlefield ends the link of its
// single attachment; the record names the attachment's (unchanged) id and the host's old id.
#[test]
fn req_event_001_cr_701_3d_host_leaves_single() {
    let (mut w, ids) = world_with(2);
    let (host, eq) = (ids[0], ids[1]);
    occurs(&mut w, attach(eq, on(host)));
    let ex = occurs(&mut w, destroy(host));
    let m = moved_of(&ex);
    assert_eq!(m.old, host);
    assert_ne!(m.new, host);
    assert_eq!(ex.unattached, vec![rec(eq, on(host))]);
    assert_eq!(w.permanents.attached_to(eq), None);
    // A later explicit Unattach finds nothing.
    assert_noop(&mut w, GameEvent::Unattach { attachment: eq });
}

// REQ-EVENT-001: CR 701.3d. A host leaving with several attachments reports one record per
// attachment in ascending id order, whatever order they were attached in; attachments to
// other hosts are untouched.
#[test]
fn req_event_001_cr_701_3d_host_leaves_several_ascending() {
    let (mut w, ids) = world_with(5);
    let (host, other, x, y, z) = (ids[0], ids[1], ids[2], ids[3], ids[4]);
    occurs(&mut w, attach(z, on(host)));
    occurs(&mut w, attach(x, on(host)));
    occurs(&mut w, attach(y, on(other)));
    let ex = occurs(&mut w, zc(host, ZoneKind::Exile));
    assert_eq!(ex.unattached, vec![rec(x, on(host)), rec(z, on(host))]);
    assert_eq!(w.permanents.attached_to(y), Some(on(other)));
}

// REQ-EVENT-001: CR 701.3d. An attachment leaving the battlefield reports its own link under
// its old id (CR 400.7: it becomes a new object).
#[test]
fn req_event_001_cr_701_3d_attachment_leaves() {
    let (mut w, ids) = world_with(2);
    let (host, eq) = (ids[0], ids[1]);
    occurs(&mut w, attach(eq, on(host)));
    let ex = occurs(&mut w, zc(eq, ZoneKind::Hand));
    let m = moved_of(&ex);
    assert_eq!(m.old, eq);
    assert_eq!(ex.unattached, vec![rec(eq, on(host))]);
    // Attached to a player: the player is the host reported.
    let (mut w, ids) = world_with(1);
    let eq = ids[0];
    occurs(&mut w, attach(eq, AttachTarget::Player(p(2))));
    let ex = occurs(&mut w, destroy(eq));
    assert_eq!(ex.unattached, vec![rec(eq, AttachTarget::Player(p(2)))]);
}

// REQ-EVENT-001: CR 701.3d. An attachment that has its own attachments leaves: first its own
// link, then each permanent attached to it, ascending id.
#[test]
fn req_event_001_cr_701_3d_attachment_with_attachments_leaves() {
    let (mut w, ids) = world_with(4);
    let (host, mid, top2, top1) = (ids[0], ids[1], ids[2], ids[3]);
    occurs(&mut w, attach(mid, on(host)));
    occurs(&mut w, attach(top1, on(mid)));
    occurs(&mut w, attach(top2, on(mid)));
    let ex = occurs(&mut w, destroy(mid));
    assert_eq!(
        ex.unattached,
        vec![rec(mid, on(host)), rec(top2, on(mid)), rec(top1, on(mid))]
    );
    assert_eq!(w.permanents.attached_to(top1), None);
    assert_eq!(w.permanents.attached_to(top2), None);
    assert!(w.permanents.attachments_of(on(host)).is_empty());
}

// REQ-EVENT-001: CR 400.7. Zone changes that end no link record nothing: an unattached
// permanent leaving, a card moving between other zones, a battlefield reposition of a host and
// of an attachment, and a permanent entering the battlefield.
#[test]
fn req_event_001_cr_400_7_moves_without_links_record_nothing() {
    let (mut w, ids) = world_with(3);
    let (host, eq, loner) = (ids[0], ids[1], ids[2]);
    occurs(&mut w, attach(eq, on(host)));
    let ex = occurs(&mut w, destroy(loner));
    assert!(ex.unattached.is_empty());
    let card = w.create(9, 1, ZoneKind::Hand);
    let ex = occurs(&mut w, zc(card, ZoneKind::Graveyard));
    assert!(ex.unattached.is_empty());
    let ex = occurs(&mut w, zc(moved_of(&ex).new, ZoneKind::Battlefield));
    assert!(ex.unattached.is_empty());
    for id in [host, eq] {
        let ex = occurs(&mut w, zc(id, ZoneKind::Battlefield));
        assert_eq!(moved_of(&ex).old, moved_of(&ex).new);
        assert!(ex.unattached.is_empty());
    }
    assert_eq!(w.permanents.attached_to(eq), Some(on(host)));
}

// REQ-EVENT-001: CR 701.3d. Explicit Unattach reports the link it cleared (permanent and
// player hosts); an unattached permanent, a card off the battlefield and an unknown id behave
// as before (no-op, no-op, UnknownObject).
#[test]
fn req_event_001_cr_701_3d_unattach_reports_cleared_link() {
    let (mut w, ids) = world_with(2);
    let (host, eq) = (ids[0], ids[1]);
    occurs(&mut w, attach(eq, on(host)));
    let ex = occurs(&mut w, GameEvent::Unattach { attachment: eq });
    assert_eq!(ex.outcome, Outcome::Applied);
    assert_eq!(ex.unattached, vec![rec(eq, on(host))]);
    occurs(&mut w, attach(eq, AttachTarget::Player(p(1))));
    let ex = occurs(&mut w, GameEvent::Unattach { attachment: eq });
    assert_eq!(ex.unattached, vec![rec(eq, AttachTarget::Player(p(1)))]);
    assert_noop(&mut w, GameEvent::Unattach { attachment: eq });
    let card = w.create(7, 1, ZoneKind::Hand);
    assert_noop(&mut w, GameEvent::Unattach { attachment: card });
    assert_fails(
        &mut w,
        GameEvent::Unattach {
            attachment: ObjectId::new(99),
        },
        EventError::UnknownObject(ObjectId::new(99)),
    );
}

// REQ-EVENT-001: CR 701.3d. UnattachFrom clears the link only when it is to exactly the named
// host: matching (permanent and player) reports one record; another host, no host, a card off
// the battlefield are no-ops; an unknown id is UnknownObject.
#[test]
fn req_event_001_cr_701_3d_unattach_from() {
    let (mut w, ids) = world_with(3);
    let (host, other, eq) = (ids[0], ids[1], ids[2]);
    let from = |host| GameEvent::UnattachFrom {
        attachment: eq,
        host,
    };
    occurs(&mut w, attach(eq, on(host)));
    // Non-matching hosts: nothing happens.
    assert_noop(&mut w, from(on(other)));
    assert_noop(&mut w, from(AttachTarget::Player(p(1))));
    assert_eq!(w.permanents.attached_to(eq), Some(on(host)));
    // Matching.
    let ex = occurs(&mut w, from(on(host)));
    assert_eq!(ex.outcome, Outcome::Applied);
    assert_eq!(ex.unattached, vec![rec(eq, on(host))]);
    assert_eq!(w.permanents.attached_to(eq), None);
    // Unattached.
    assert_noop(&mut w, from(on(host)));
    // Player host.
    occurs(&mut w, attach(eq, AttachTarget::Player(p(2))));
    assert_noop(&mut w, from(AttachTarget::Player(p(1))));
    let ex = occurs(&mut w, from(AttachTarget::Player(p(2))));
    assert_eq!(ex.unattached, vec![rec(eq, AttachTarget::Player(p(2)))]);
    // Off the battlefield and unknown.
    let card = w.create(7, 1, ZoneKind::Hand);
    assert_noop(
        &mut w,
        GameEvent::UnattachFrom {
            attachment: card,
            host: on(host),
        },
    );
    assert_fails(
        &mut w,
        GameEvent::UnattachFrom {
            attachment: ObjectId::new(99),
            host: on(host),
        },
        EventError::UnknownObject(ObjectId::new(99)),
    );
}

// REQ-EVENT-001: CR 701.3d, CR 704.5n, CR 704.5p. No link is reported twice: an Unattach
// earlier in the batch clears the link, so the host's (or the attachment's) later zone change
// finds nothing. Exactly one record in the whole batch.
#[test]
fn req_event_001_cr_701_3d_unattach_then_zone_change_one_record() {
    for leaving_host in [true, false] {
        let (mut w, ids) = world_with(2);
        let (host, eq) = (ids[0], ids[1]);
        occurs(&mut w, attach(eq, on(host)));
        let leaving = if leaving_host { host } else { eq };
        let out = w
            .batch(vec![
                GameEvent::Unattach { attachment: eq },
                destroy(leaving),
            ])
            .unwrap();
        assert_eq!(out.events.len(), 2);
        assert_eq!(out.events[0].unattached, vec![rec(eq, on(host))]);
        assert!(out.events[1].unattached.is_empty());
        let total: usize = out.events.iter().map(|e| e.unattached.len()).sum();
        assert_eq!(total, 1);
    }
    // Same with UnattachFrom.
    let (mut w, ids) = world_with(2);
    let (host, eq) = (ids[0], ids[1]);
    occurs(&mut w, attach(eq, on(host)));
    let out = w
        .batch(vec![
            GameEvent::UnattachFrom {
                attachment: eq,
                host: on(host),
            },
            destroy(host),
        ])
        .unwrap();
    let all: Vec<_> = out
        .events
        .iter()
        .flat_map(|e| e.unattached.clone())
        .collect();
    assert_eq!(all, vec![rec(eq, on(host))]);
}

// REQ-EVENT-001: CR 701.3d, CR 704.3. Simultaneous departures of a host and its attachment:
// the first move reports the link; the second finds it already ended.
#[test]
fn req_event_001_cr_701_3d_host_and_attachment_leave_together() {
    let (mut w, ids) = world_with(2);
    let (host, eq) = (ids[0], ids[1]);
    occurs(&mut w, attach(eq, on(host)));
    let out = w.batch(vec![destroy(host), destroy(eq)]).unwrap();
    assert_eq!(out.events[0].unattached, vec![rec(eq, on(host))]);
    assert!(out.events[1].unattached.is_empty());
    let (mut w, ids) = world_with(2);
    let (host, eq) = (ids[0], ids[1]);
    occurs(&mut w, attach(eq, on(host)));
    let out = w.batch(vec![destroy(eq), destroy(host)]).unwrap();
    assert_eq!(out.events[0].unattached, vec![rec(eq, on(host))]);
    assert!(out.events[1].unattached.is_empty());
}

// REQ-EVENT-001: a failing batch that would have produced records changes and logs nothing;
// the successful retry logs exactly the returned records (records come from the real run, not
// doubled by the planning run).
#[test]
fn req_event_001_unattach_records_roll_back() {
    let (mut w, ids) = world_with(4);
    let (a, b, eq, aura) = (ids[0], ids[1], ids[2], ids[3]);
    occurs(&mut w, attach(eq, on(a)));
    occurs(&mut w, attach(aura, on(a)));
    let good = vec![
        attach(eq, on(b)),
        destroy(a),
        GameEvent::Unattach { attachment: eq },
    ];
    let mut bad = good.clone();
    bad.push(GameEvent::LifeChange {
        player: p(9),
        delta: 1,
    });
    let before = w.parts();
    let log_len = w.events.log().batches().len();
    assert_eq!(w.batch(bad), Err(EventError::UnknownPlayer(p(9))));
    assert!(w.parts() == before, "a failed batch changed state");
    assert_eq!(w.events.log().batches().len(), log_len);

    let out = w.batch(good).unwrap();
    assert_eq!(out.events[0].unattached, vec![rec(eq, on(a))]);
    assert_eq!(out.events[1].unattached, vec![rec(aura, on(a))]);
    assert_eq!(out.events[2].unattached, vec![rec(eq, on(b))]);
    assert_eq!(w.events.log().batches().last(), Some(&out));
}

// REQ-CORE-006: UnattachRecord encodes as attachment then host; ExecutedEvent ends with the
// length-prefixed record sequence; UnattachFrom is tag 17 then attachment then host.
#[test]
fn req_core_006_unattach_encoding() {
    let att = ObjectId::new(5);
    let host = on(ObjectId::new(3));
    let r = rec(att, host);
    let mut want = Vec::new();
    att.encode_canonical(&mut want);
    host.encode_canonical(&mut want);
    assert_eq!(encode_to_vec(&r), want);
    let player_rec = rec(att, AttachTarget::Player(p(1)));
    assert_ne!(encode_to_vec(&player_rec), encode_to_vec(&r));
    assert_ne!(
        encode_to_vec(&rec(ObjectId::new(3), on(ObjectId::new(5)))),
        encode_to_vec(&r)
    );

    let ex = ExecutedEvent {
        instance: EventInstance::new(GameEvent::Unattach { attachment: att }),
        outcome: Outcome::Applied,
        lki_captured: false,
        unattached: vec![],
    };
    let mut want = Vec::new();
    ex.instance.encode_canonical(&mut want);
    ex.outcome.encode_canonical(&mut want);
    ex.lki_captured.encode_canonical(&mut want);
    let prefix = want.clone();
    put_len(&mut want, 0);
    assert_eq!(encode_to_vec(&ex), want);

    let with = ExecutedEvent {
        unattached: vec![r, player_rec],
        ..ex.clone()
    };
    let mut want = prefix;
    put_len(&mut want, 2);
    r.encode_canonical(&mut want);
    player_rec.encode_canonical(&mut want);
    assert_eq!(encode_to_vec(&with), want);
    let swapped = ExecutedEvent {
        unattached: vec![player_rec, r],
        ..ex.clone()
    };
    assert_ne!(encode_to_vec(&swapped), encode_to_vec(&with));

    let ev = GameEvent::UnattachFrom {
        attachment: att,
        host,
    };
    let mut want = Vec::new();
    put_variant(&mut want, 17);
    att.encode_canonical(&mut want);
    host.encode_canonical(&mut want);
    assert_eq!(encode_to_vec(&ev), want);
    assert_ne!(
        encode_to_vec(&ev),
        encode_to_vec(&GameEvent::Unattach { attachment: att })
    );
}
