// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::zones`. Black-box: written from the work-item brief only.
//!
//! Zone model: zone kinds and identifiers, visibility, unordered zones, owner and controller
//! bookkeeping, outside-the-game card pools and no-panic behaviour on odd arguments
//! (REQ-ZONE-001 items 2, 5, 6, 9; REQ-CORE-002).

use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};
use fineprint_core::zones::{
    MoveRequest, OutsidePool, Position, ZoneError, ZoneId, ZoneKind, Zones, ZonesConfig,
};

fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn cid(n: u64) -> CardId {
    CardId::new(n)
}

fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn req(kind: ZoneKind) -> MoveRequest {
    MoveRequest::new(kind)
}

fn req_pos(kind: ZoneKind, position: Position) -> MoveRequest {
    let mut r = MoveRequest::new(kind);
    r.position = position;
    r
}

fn req_face_down(kind: ZoneKind) -> MoveRequest {
    let mut r = MoveRequest::new(kind);
    r.face_down = true;
    r
}

fn req_ctrl(kind: ZoneKind, controller: PlayerId) -> MoveRequest {
    let mut r = MoveRequest::new(kind);
    r.controller = Some(controller);
    r
}

fn two() -> Zones {
    Zones::new(&[pid(1), pid(2)], ZonesConfig::default())
}

const ALL_POOLS: [OutsidePool; 6] = [
    OutsidePool::Sideboard,
    OutsidePool::Companion,
    OutsidePool::Lesson,
    OutsidePool::AttractionDeck,
    OutsidePool::ContraptionDeck,
    OutsidePool::Spellbook,
];

/// Every zone identifier for the given players: the per-player zones of each player, then the
/// shared zones.
fn zone_ids(players: &[PlayerId]) -> Vec<ZoneId> {
    let mut out = Vec::new();
    for &p in players {
        out.push(ZoneId::Library(p));
        out.push(ZoneId::Hand(p));
        out.push(ZoneId::Graveyard(p));
    }
    out.extend([
        ZoneId::Battlefield,
        ZoneId::Stack,
        ZoneId::Exile,
        ZoneId::Command,
    ]);
    out
}

fn appears_anywhere(z: &Zones, id: ObjectId, players: &[PlayerId]) -> bool {
    z.all_object_ids().contains(&id)
        || zone_ids(players)
            .into_iter()
            .any(|zone| z.contents(zone).contains(&id))
}

// ---------------------------------------------------------------------------------------------
// Item 6: zone kinds and zone identifiers.
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001, CR 400.2: the seven zone kinds and their per-player / public / ordered flags.
#[test]
fn zone_kind_flags_match_the_specified_table() {
    use ZoneKind::*;
    assert_eq!(
        ZoneKind::ALL,
        [Library, Hand, Battlefield, Graveyard, Stack, Exile, Command]
    );
    // (kind, per_player, public, ordered)
    let table = [
        (Library, true, false, true),
        (Hand, true, false, false),
        (Battlefield, false, true, false),
        (Graveyard, true, true, true),
        (Stack, false, true, true),
        (Exile, false, true, false),
        (Command, false, true, false),
    ];
    for (kind, per_player, public, ordered) in table {
        assert_eq!(kind.is_per_player(), per_player, "{kind:?} per-player");
        assert_eq!(kind.is_public(), public, "{kind:?} public");
        assert_eq!(kind.is_ordered(), ordered, "{kind:?} ordered");
    }
}

// REQ-ZONE-001, CR 400.11: outside the game is not a zone; there are exactly seven kinds, all
// distinct and listed in ascending order.
#[test]
fn zone_kind_all_has_exactly_seven_distinct_kinds() {
    assert_eq!(ZoneKind::ALL.len(), 7);
    for (i, a) in ZoneKind::ALL.iter().enumerate() {
        for b in ZoneKind::ALL.iter().skip(i + 1) {
            assert_ne!(a, b);
        }
    }
}

// REQ-ZONE-001, CR 400.2: ZoneId::kind / owner / of are consistent; shared ids ignore the owner.
#[test]
fn zone_id_kind_owner_and_of_are_consistent() {
    for p in [pid(1), pid(2), pid(0), pid(u64::MAX)] {
        assert_eq!(ZoneId::of(ZoneKind::Library, p), ZoneId::Library(p));
        assert_eq!(ZoneId::of(ZoneKind::Hand, p), ZoneId::Hand(p));
        assert_eq!(ZoneId::of(ZoneKind::Graveyard, p), ZoneId::Graveyard(p));
        assert_eq!(ZoneId::of(ZoneKind::Battlefield, p), ZoneId::Battlefield);
        assert_eq!(ZoneId::of(ZoneKind::Stack, p), ZoneId::Stack);
        assert_eq!(ZoneId::of(ZoneKind::Exile, p), ZoneId::Exile);
        assert_eq!(ZoneId::of(ZoneKind::Command, p), ZoneId::Command);
        for kind in ZoneKind::ALL {
            let z = ZoneId::of(kind, p);
            assert_eq!(z.kind(), kind);
            if kind.is_per_player() {
                assert_eq!(z.owner(), Some(p));
            } else {
                assert_eq!(z.owner(), None);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Item 6: visibility.
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001 items 1, 2, 4, 5, 6, CR 400.2, 401.2, 402.3, 406.3: public information is
// "public zone and not face down"; library and hand are hidden even face up.
#[test]
fn is_public_info_follows_zone_visibility_and_face_down() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let mut card = 0;
    for kind in ZoneKind::ALL {
        for face_down in [false, true] {
            card += 1;
            let mut r = req(kind);
            r.face_down = face_down;
            let id = z.create_object(&mut g, cid(card), pid(1), r).unwrap();
            assert_eq!(z.object(id).unwrap().face_down, face_down);
            let expected = kind.is_public() && !face_down;
            assert_eq!(
                z.is_public_info(id),
                Some(expected),
                "{kind:?} face_down={face_down}"
            );
        }
    }
}

// REQ-ZONE-001 items 5, 6, CR 400.7, 406.3: face_down comes only from each move's request; a
// face-down object moved with a default request is face up afterwards.
#[test]
fn face_down_comes_only_from_the_move_request() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let a = z
        .create_object(&mut g, cid(1), pid(1), req_face_down(ZoneKind::Exile))
        .unwrap();
    assert_eq!(z.is_public_info(a), Some(false));
    let b = z.move_object(&mut g, a, req(ZoneKind::Exile)).unwrap().new;
    assert!(!z.object(b).unwrap().face_down);
    assert_eq!(z.is_public_info(b), Some(true));
    let c = z
        .move_object(&mut g, b, req_face_down(ZoneKind::Battlefield))
        .unwrap()
        .new;
    assert!(z.object(c).unwrap().face_down);
    assert_eq!(z.is_public_info(c), Some(false));
    let d = z
        .move_object(&mut g, c, req(ZoneKind::Battlefield))
        .unwrap()
        .new;
    assert!(!z.object(d).unwrap().face_down);
    assert_eq!(z.is_public_info(d), Some(true));
    let e = z
        .move_object(&mut g, d, req_face_down(ZoneKind::Stack))
        .unwrap()
        .new;
    assert_eq!(z.is_public_info(e), Some(false));
    let f = z
        .move_object(&mut g, e, req(ZoneKind::Command))
        .unwrap()
        .new;
    assert_eq!(z.is_public_info(f), Some(true));
}

// REQ-ZONE-001, REQ-CORE-002, CR 400.7: unknown ids give None, including ids of objects that
// have since moved.
#[test]
fn is_public_info_unknown_id_is_none() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    assert_eq!(z.is_public_info(oid(1)), None);
    let a = z
        .create_object(&mut g, cid(1), pid(1), req(ZoneKind::Exile))
        .unwrap();
    z.move_object(&mut g, a, req(ZoneKind::Exile)).unwrap();
    assert_eq!(z.is_public_info(a), None);
    assert_eq!(z.is_public_info(oid(0)), None);
    assert_eq!(z.is_public_info(oid(u64::MAX)), None);
}

// ---------------------------------------------------------------------------------------------
// Item 5: unordered zones and counting.
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001 items 2, 5, 6, CR 402.3, 406.3, 400.7, 400.8, 406.7, 400.10: hand, battlefield,
// exile and command list ascending ObjectId whatever Position is requested. Two generators are
// used so that a larger id enters before a smaller one (both start below every id issued by the
// other, so neither collides with a live object).
#[test]
fn unordered_zones_list_ascending_object_ids_whatever_the_position() {
    for kind in [
        ZoneKind::Hand,
        ZoneKind::Battlefield,
        ZoneKind::Exile,
        ZoneKind::Command,
    ] {
        let mut z = two();
        let mut high = IdGenerator::<ObjectId>::from_last_issued(1000);
        let mut low = IdGenerator::<ObjectId>::new();
        let a = z
            .create_object(&mut high, cid(1), pid(1), req_pos(kind, Position::Top))
            .unwrap();
        let b = z
            .create_object(&mut low, cid(2), pid(1), req_pos(kind, Position::Bottom))
            .unwrap();
        let c = z
            .create_object(&mut high, cid(3), pid(1), req_pos(kind, Position::Index(0)))
            .unwrap();
        let d = z
            .create_object(&mut low, cid(4), pid(1), req_pos(kind, Position::Top))
            .unwrap();
        assert_eq!((a, b, c, d), (oid(1001), oid(1), oid(1002), oid(2)));
        let zone = ZoneId::of(kind, pid(1));
        assert_eq!(z.contents(zone), vec![oid(1), oid(2), oid(1001), oid(1002)]);
        // Moving within the zone keeps the listing ascending. R10 correction for finding F1
        // (CodeRabbit comment 4621941423): in the hand and on the battlefield this is not a zone
        // change and keeps the id; in exile and the command zone it is a new object.
        let e = z
            .move_object(&mut low, c, req_pos(kind, Position::Index(1)))
            .unwrap()
            .new;
        if matches!(kind, ZoneKind::Exile | ZoneKind::Command) {
            assert_eq!(e, oid(3));
            assert_eq!(z.contents(zone), vec![oid(1), oid(2), oid(3), oid(1001)]);
        } else {
            assert_eq!(e, c);
            assert_eq!(low.last_issued(), 2);
            assert_eq!(z.contents(zone), vec![oid(1), oid(2), oid(1001), oid(1002)]);
        }
    }
}

// REQ-ZONE-001 items 1, 2, CR 401.3, 402.3: len / is_empty agree with contents for every zone,
// and any player's library and hand can be counted.
#[test]
fn len_and_is_empty_agree_with_contents() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let players = [pid(1), pid(2)];
    for zone in zone_ids(&players) {
        assert_eq!(z.len(zone), 0);
        assert!(z.is_empty(zone));
    }
    let mut card = 0;
    for (i, kind) in ZoneKind::ALL.into_iter().enumerate() {
        for _ in 0..=i {
            card += 1;
            z.create_object(&mut g, cid(card), pid(2), req(kind))
                .unwrap();
        }
    }
    for zone in zone_ids(&players) {
        let n = z.contents(zone).len();
        assert_eq!(z.len(zone), n, "{zone:?}");
        assert_eq!(z.is_empty(zone), n == 0, "{zone:?}");
    }
    assert_eq!(z.len(ZoneId::Library(pid(2))), 1);
    assert_eq!(z.len(ZoneId::Hand(pid(2))), 2);
    assert_eq!(z.len(ZoneId::Library(pid(1))), 0);
    assert_eq!(z.len(ZoneId::Battlefield), 3);
    assert_eq!(z.len(ZoneId::Command), 7);
    // Unknown zones are empty.
    assert_eq!(z.len(ZoneId::Library(pid(9))), 0);
    assert!(z.is_empty(ZoneId::Hand(pid(9))));
    assert!(z.contents(ZoneId::Graveyard(pid(9))).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Item 7: owner and controller bookkeeping.
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002, CR 400.7, 109.4, 108.4a: the owner never changes across moves; the stored
// controller defaults to the owner (the CR 108.4a fallback outside the battlefield and the
// stack); the CardId persists.
#[test]
fn owner_never_changes_and_controller_defaults_to_owner() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let mut id = z
        .create_object(&mut g, cid(7), pid(2), req(ZoneKind::Library))
        .unwrap();
    let rec = z.object(id).unwrap();
    assert_eq!(
        (rec.id, rec.card, rec.owner, rec.controller),
        (id, cid(7), pid(2), pid(2))
    );
    assert_eq!(rec.zone, ZoneId::Library(pid(2)));
    for kind in [
        ZoneKind::Hand,
        ZoneKind::Stack,
        ZoneKind::Battlefield,
        ZoneKind::Graveyard,
        ZoneKind::Exile,
        ZoneKind::Command,
        ZoneKind::Library,
    ] {
        id = z.move_object(&mut g, id, req(kind)).unwrap().new;
        let rec = z.object(id).unwrap();
        assert_eq!(rec.id, id);
        assert_eq!(rec.card, cid(7));
        assert_eq!(rec.owner, pid(2));
        assert_eq!(rec.controller, pid(2));
        assert_eq!(rec.zone, ZoneId::of(kind, pid(2)));
    }
}

// REQ-ZONE-001, CR 109.4, 110.2, 110.2a, 405.4: an explicit controller is honoured on the
// battlefield and the stack.
#[test]
fn explicit_controller_allowed_on_battlefield_and_stack() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let a = z
        .create_object(
            &mut g,
            cid(1),
            pid(1),
            req_ctrl(ZoneKind::Battlefield, pid(2)),
        )
        .unwrap();
    assert_eq!(z.object(a).unwrap().controller, pid(2));
    assert_eq!(z.object(a).unwrap().owner, pid(1));
    let b = z
        .move_object(&mut g, a, req_ctrl(ZoneKind::Stack, pid(2)))
        .unwrap()
        .new;
    assert_eq!(z.object(b).unwrap().controller, pid(2));
    let c = z
        .move_object(&mut g, b, req_ctrl(ZoneKind::Battlefield, pid(1)))
        .unwrap()
        .new;
    assert_eq!(z.object(c).unwrap().controller, pid(1));
    let d = z
        .create_object(&mut g, cid(2), pid(2), req_ctrl(ZoneKind::Stack, pid(1)))
        .unwrap();
    assert_eq!(z.object(d).unwrap().controller, pid(1));
    assert_eq!(z.object(d).unwrap().owner, pid(2));
}

// REQ-ZONE-001, CR 400.7, 109.4, 110.2a, 108.4a: after leaving the battlefield (or the stack)
// the stored controller is the owner again (the CR 108.4a fallback); a battlefield-to-battlefield
// move with a default request also takes the owner as controller from the request.
#[test]
fn controller_reverts_to_owner_after_leaving_battlefield() {
    for to in ZoneKind::ALL {
        let mut z = two();
        let mut g = IdGenerator::<ObjectId>::new();
        let a = z
            .create_object(
                &mut g,
                cid(1),
                pid(1),
                req_ctrl(ZoneKind::Battlefield, pid(2)),
            )
            .unwrap();
        let b = z.move_object(&mut g, a, req(to)).unwrap().new;
        let rec = z.object(b).unwrap();
        assert_eq!(rec.controller, pid(1), "to {to:?}");
        assert_eq!(rec.owner, pid(1));
        let s = z
            .create_object(&mut g, cid(2), pid(1), req_ctrl(ZoneKind::Stack, pid(2)))
            .unwrap();
        let t = z.move_object(&mut g, s, req(to)).unwrap().new;
        assert_eq!(z.object(t).unwrap().controller, pid(1), "stack to {to:?}");
    }
}

// REQ-ZONE-001, CR 109.4: a controller in the request is rejected outside the battlefield and
// the stack (even when it names the owner); state and generator are unchanged on the error. This
// is a container rule, not a CR prohibition: CR 109.4c gives emblems in the command zone a
// controller, but the container does not model emblems or variant cards yet.
#[test]
fn controller_in_request_rejected_outside_battlefield_and_stack() {
    for kind in [
        ZoneKind::Library,
        ZoneKind::Hand,
        ZoneKind::Graveyard,
        ZoneKind::Exile,
        ZoneKind::Command,
    ] {
        for q in [pid(2), pid(1)] {
            let mut z = two();
            let mut g = IdGenerator::<ObjectId>::new();
            let a = z
                .create_object(&mut g, cid(1), pid(1), req(ZoneKind::Battlefield))
                .unwrap();
            let before = z.clone();
            let last = g.last_issued();
            assert_eq!(
                z.create_object(&mut g, cid(2), pid(1), req_ctrl(kind, q)),
                Err(ZoneError::ControllerNotApplicable)
            );
            assert_eq!(z, before);
            assert_eq!(g.last_issued(), last);
            assert_eq!(
                z.move_object(&mut g, a, req_ctrl(kind, q)).map(|m| m.new),
                Err(ZoneError::ControllerNotApplicable)
            );
            assert_eq!(z, before);
            assert_eq!(g.last_issued(), last);
            assert!(z.object(a).is_some());
        }
    }
}

// REQ-ZONE-001, CR 109.4, 110.2a, 405.4: an unknown controller on the battlefield or stack is
// UnknownPlayer; state and generator unchanged.
#[test]
fn unknown_controller_on_battlefield_or_stack_is_unknown_player() {
    for kind in [ZoneKind::Battlefield, ZoneKind::Stack] {
        let mut z = two();
        let mut g = IdGenerator::<ObjectId>::new();
        let a = z
            .create_object(&mut g, cid(1), pid(1), req(ZoneKind::Hand))
            .unwrap();
        let before = z.clone();
        let last = g.last_issued();
        assert_eq!(
            z.create_object(&mut g, cid(2), pid(1), req_ctrl(kind, pid(3))),
            Err(ZoneError::UnknownPlayer(pid(3)))
        );
        assert_eq!(
            z.move_object(&mut g, a, req_ctrl(kind, pid(3)))
                .map(|m| m.new),
            Err(ZoneError::UnknownPlayer(pid(3)))
        );
        assert_eq!(z, before);
        assert_eq!(g.last_issued(), last);
    }
}

// REQ-ZONE-001, REQ-CORE-002, CR 400.7, 109.4, 110.2: set_controller changes control in place
// (no new object) on the battlefield and stack.
#[test]
fn set_controller_works_on_battlefield_and_stack() {
    for kind in [ZoneKind::Battlefield, ZoneKind::Stack] {
        let mut z = two();
        let mut g = IdGenerator::<ObjectId>::new();
        let a = z.create_object(&mut g, cid(1), pid(1), req(kind)).unwrap();
        let last = g.last_issued();
        z.set_controller(a, pid(2)).unwrap();
        let rec = z.object(a).unwrap();
        assert_eq!(rec.id, a);
        assert_eq!(rec.controller, pid(2));
        assert_eq!(rec.owner, pid(1));
        assert_eq!(z.all_object_ids(), vec![a]);
        assert_eq!(g.last_issued(), last);
        z.set_controller(a, pid(1)).unwrap();
        assert_eq!(z.object(a).unwrap().controller, pid(1));
    }
}

// REQ-ZONE-001, CR 109.4, 400.7: set_controller errors: other zones (a container rule; emblems,
// CR 109.4c, and variant cards are not modelled yet), unknown object, unknown player; state
// unchanged on each.
#[test]
fn set_controller_errors_leave_state_unchanged() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let mut card = 0;
    for kind in [
        ZoneKind::Library,
        ZoneKind::Hand,
        ZoneKind::Graveyard,
        ZoneKind::Exile,
        ZoneKind::Command,
    ] {
        card += 1;
        let a = z
            .create_object(&mut g, cid(card), pid(1), req(kind))
            .unwrap();
        let before = z.clone();
        assert_eq!(
            z.set_controller(a, pid(2)),
            Err(ZoneError::ControllerNotApplicable),
            "{kind:?}"
        );
        assert_eq!(z, before);
    }
    let bf = z
        .create_object(&mut g, cid(99), pid(1), req(ZoneKind::Battlefield))
        .unwrap();
    let before = z.clone();
    assert_eq!(
        z.set_controller(bf, pid(5)),
        Err(ZoneError::UnknownPlayer(pid(5)))
    );
    assert_eq!(
        z.set_controller(oid(5000), pid(2)),
        Err(ZoneError::UnknownObject(oid(5000)))
    );
    assert_eq!(z, before);
    // R10 correction for finding F1 (CodeRabbit comment 4621941423): a battlefield-to-battlefield
    // move is not a zone change (CR 400.7) and keeps the id.
    let same = z
        .move_object(&mut g, bf, req(ZoneKind::Battlefield))
        .unwrap()
        .new;
    assert_eq!(same, bf);
    // A moved object's old id is unknown after a real zone change.
    let nb = z.move_object(&mut g, bf, req(ZoneKind::Stack)).unwrap().new;
    assert_ne!(nb, bf);
    assert_eq!(
        z.set_controller(bf, pid(2)),
        Err(ZoneError::UnknownObject(bf))
    );
    assert_eq!(z.object(nb).unwrap().controller, pid(1));
}

// REQ-ZONE-001, REQ-CORE-002, CR 109.4, 108.4a: the rules-level controller is Some only on the
// battlefield and the stack; in the library, hand, graveyard, exile and command zone it is None
// while the stored `controller` field keeps the owner fallback (finding F4, comment 4621941395).
#[test]
fn rules_controller_is_none_outside_battlefield_and_stack() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let mut card = 0;
    for kind in ZoneKind::ALL {
        card += 1;
        let on_bf_or_stack = matches!(kind, ZoneKind::Battlefield | ZoneKind::Stack);
        let r = if on_bf_or_stack {
            req_ctrl(kind, pid(2))
        } else {
            req(kind)
        };
        let a = z.create_object(&mut g, cid(card), pid(1), r).unwrap();
        let rec = *z.object(a).unwrap();
        if on_bf_or_stack {
            assert_eq!(rec.rules_controller(), Some(pid(2)), "{kind:?}");
            assert_eq!(rec.controller, pid(2));
            // Leaving for the owner's hand: no rules controller, owner fallback stored.
            let m = z.move_object(&mut g, a, req(ZoneKind::Hand)).unwrap();
            let moved = *z.object(m.new).unwrap();
            assert_eq!(moved.rules_controller(), None);
            assert_eq!(moved.controller, pid(1));
        } else {
            assert_eq!(rec.rules_controller(), None, "{kind:?}");
            assert_eq!(rec.controller, pid(1), "{kind:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Item 8: outside the game.
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001 item 9, CR 400.11: outside cards are listed ascending per owner and per pool;
// unknown players have empty pools.
#[test]
fn outside_cards_are_per_owner_per_pool_ascending() {
    let mut z = two();
    let mut card = 100;
    for (i, pool) in ALL_POOLS.into_iter().enumerate() {
        for p in [pid(1), pid(2)] {
            // Add in descending order to check the ascending listing.
            for k in (0..=i as u64).rev() {
                let c = card + k;
                z.add_outside_card(p, pool, cid(c)).unwrap();
            }
            card += 10;
        }
    }
    let mut card = 100;
    for (i, pool) in ALL_POOLS.into_iter().enumerate() {
        for p in [pid(1), pid(2)] {
            let expected: Vec<CardId> = (0..=i as u64).map(|k| cid(card + k)).collect();
            assert_eq!(z.outside_cards(p, pool), expected, "{p:?} {pool:?}");
            card += 10;
        }
    }
    for pool in ALL_POOLS {
        assert!(z.outside_cards(pid(3), pool).is_empty());
        assert!(z.outside_cards(pid(0), pool).is_empty());
    }
    // Outside cards are not objects.
    assert!(z.all_object_ids().is_empty());
    assert_eq!(z.object_of_card(cid(100)), None);
}

// REQ-ZONE-001 item 9, REQ-CORE-002, CR 400.11: a CardId exists once: in one pool or in one zone.
#[test]
fn card_already_present_in_any_pool_or_zone() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    z.add_outside_card(pid(1), OutsidePool::Sideboard, cid(1))
        .unwrap();
    let a = z
        .create_object(&mut g, cid(2), pid(1), req(ZoneKind::Hand))
        .unwrap();
    let before = z.clone();
    let last = g.last_issued();
    for pool in ALL_POOLS {
        for p in [pid(1), pid(2)] {
            assert_eq!(
                z.add_outside_card(p, pool, cid(1)),
                Err(ZoneError::CardAlreadyPresent(cid(1)))
            );
            assert_eq!(
                z.add_outside_card(p, pool, cid(2)),
                Err(ZoneError::CardAlreadyPresent(cid(2)))
            );
        }
    }
    for kind in ZoneKind::ALL {
        for p in [pid(1), pid(2)] {
            assert_eq!(
                z.create_object(&mut g, cid(1), p, req(kind)),
                Err(ZoneError::CardAlreadyPresent(cid(1)))
            );
            assert_eq!(
                z.create_object(&mut g, cid(2), p, req(kind)),
                Err(ZoneError::CardAlreadyPresent(cid(2)))
            );
        }
    }
    assert_eq!(z, before);
    assert_eq!(g.last_issued(), last);
    assert_eq!(z.object_of_card(cid(2)), Some(a));
}

// REQ-ZONE-001 item 9, REQ-CORE-002, CR 400.7, 400.11: entering from outside creates a new
// object with a fresh id in the requested zone and removes the card from the pool.
#[test]
fn enter_from_outside_creates_fresh_object_and_empties_pool_entry() {
    for kind in ZoneKind::ALL {
        let mut z = two();
        let mut g = IdGenerator::<ObjectId>::from_last_issued(40);
        z.add_outside_card(pid(2), OutsidePool::Lesson, cid(5))
            .unwrap();
        z.add_outside_card(pid(2), OutsidePool::Lesson, cid(6))
            .unwrap();
        let id = z
            .enter_from_outside(&mut g, pid(2), cid(5), OutsidePool::Lesson, req(kind))
            .unwrap();
        assert_eq!(id, oid(41));
        assert_eq!(g.last_issued(), 41);
        let rec = z.object(id).unwrap();
        assert_eq!(rec.card, cid(5));
        assert_eq!(rec.owner, pid(2));
        assert_eq!(rec.controller, pid(2));
        assert_eq!(rec.zone, ZoneId::of(kind, pid(2)));
        assert_eq!(z.contents(ZoneId::of(kind, pid(2))), vec![id]);
        assert_eq!(z.object_of_card(cid(5)), Some(id));
        assert_eq!(z.outside_cards(pid(2), OutsidePool::Lesson), vec![cid(6)]);
    }
}

// REQ-ZONE-001 item 9, CR 400.11: entering a card that is not in that owner's pool fails with
// NoSuchOutsideCard and changes nothing.
#[test]
fn enter_from_outside_requires_the_card_in_that_pool() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    z.add_outside_card(pid(1), OutsidePool::Sideboard, cid(1))
        .unwrap();
    let a = z
        .create_object(&mut g, cid(2), pid(1), req(ZoneKind::Hand))
        .unwrap();
    let before = z.clone();
    let last = g.last_issued();
    let attempts = [
        (pid(1), cid(1), OutsidePool::Companion),
        (pid(1), cid(1), OutsidePool::Spellbook),
        (pid(2), cid(1), OutsidePool::Sideboard),
        (pid(1), cid(9), OutsidePool::Sideboard),
        (pid(1), cid(2), OutsidePool::Sideboard),
    ];
    for (p, c, pool) in attempts {
        assert_eq!(
            z.enter_from_outside(&mut g, p, c, pool, req(ZoneKind::Battlefield)),
            Err(ZoneError::NoSuchOutsideCard(c)),
            "{p:?} {c:?} {pool:?}"
        );
    }
    assert_eq!(z, before);
    assert_eq!(g.last_issued(), last);
    assert_eq!(z.object_of_card(cid(2)), Some(a));
}

// REQ-ZONE-001 item 9, REQ-CORE-002, CR 400.7, 400.11: sending an object outside removes it
// entirely and puts its card in the owner's pool, even when another player controls it.
#[test]
fn send_outside_removes_object_and_fills_owners_pool() {
    let players = [pid(1), pid(2)];
    for kind in ZoneKind::ALL {
        for pool in ALL_POOLS {
            let mut z = two();
            let mut g = IdGenerator::<ObjectId>::new();
            let mut r = req(kind);
            if matches!(kind, ZoneKind::Battlefield | ZoneKind::Stack) {
                r.controller = Some(pid(2));
            }
            let a = z.create_object(&mut g, cid(3), pid(1), r).unwrap();
            let other = z.create_object(&mut g, cid(4), pid(1), req(kind)).unwrap();
            let last = g.last_issued();
            assert_eq!(z.send_outside(a, pool), Ok(cid(3)));
            assert_eq!(g.last_issued(), last);
            assert_eq!(z.object(a), None);
            assert_eq!(z.object_of_card(cid(3)), None);
            assert_eq!(z.is_public_info(a), None);
            assert!(!appears_anywhere(&z, a, &players));
            assert_eq!(z.all_object_ids(), vec![other]);
            assert_eq!(z.outside_cards(pid(1), pool), vec![cid(3)]);
            assert!(z.outside_cards(pid(2), pool).is_empty());
            for other_pool in ALL_POOLS {
                if other_pool != pool {
                    assert!(z.outside_cards(pid(1), other_pool).is_empty());
                }
            }
            // A second send of the same (now gone) id fails without change.
            let before = z.clone();
            assert_eq!(z.send_outside(a, pool), Err(ZoneError::UnknownObject(a)));
            assert_eq!(z, before);
        }
    }
}

// REQ-ZONE-001 item 9, REQ-CORE-002, CR 400.7, 400.11: each round trip through outside the game
// yields a different ObjectId.
#[test]
fn outside_round_trip_yields_new_object_id_each_time() {
    let players = [pid(1), pid(2)];
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let mut id = z
        .create_object(&mut g, cid(8), pid(2), req(ZoneKind::Battlefield))
        .unwrap();
    let mut seen = vec![id];
    for (i, pool) in ALL_POOLS.into_iter().cycle().take(18).enumerate() {
        assert_eq!(z.send_outside(id, pool), Ok(cid(8)));
        assert_eq!(z.outside_cards(pid(2), pool), vec![cid(8)]);
        let kind = ZoneKind::ALL[i % 7];
        let next = z
            .enter_from_outside(&mut g, pid(2), cid(8), pool, req(kind))
            .unwrap();
        assert!(z.outside_cards(pid(2), pool).is_empty());
        for old in &seen {
            assert_ne!(*old, next);
            assert!(!appears_anywhere(&z, *old, &players));
            assert_eq!(z.object(*old), None);
        }
        assert!(next > *seen.last().unwrap());
        assert_eq!(z.object_of_card(cid(8)), Some(next));
        seen.push(next);
        id = next;
    }
}

// REQ-ZONE-001 item 9, CR 400.11: outside-pool calls with an unknown owner fail without panic.
#[test]
fn outside_calls_with_unknown_owner_fail_cleanly() {
    let mut z = two();
    let mut g = IdGenerator::<ObjectId>::new();
    let before = z.clone();
    assert_eq!(
        z.add_outside_card(pid(3), OutsidePool::Sideboard, cid(1)),
        Err(ZoneError::UnknownPlayer(pid(3)))
    );
    assert!(z.outside_cards(pid(3), OutsidePool::Sideboard).is_empty());
    assert!(
        z.enter_from_outside(
            &mut g,
            pid(3),
            cid(1),
            OutsidePool::Sideboard,
            req(ZoneKind::Hand)
        )
        .is_err()
    );
    assert_eq!(z, before);
    assert_eq!(g.last_issued(), 0);
}

// ---------------------------------------------------------------------------------------------
// Item 13: construction and no-panic behaviour.
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001, REQ-CORE-002: construction with no players and with duplicates.
#[test]
fn construction_dedups_and_sorts_players() {
    let z = Zones::new(&[], ZonesConfig::default());
    assert!(z.players().is_empty());
    assert!(z.all_object_ids().is_empty());
    assert!(!z.config().free_graveyard_reorder);
    let z = Zones::new(
        &[pid(3), pid(1), pid(3), pid(2), pid(1)],
        ZonesConfig {
            free_graveyard_reorder: true,
        },
    );
    assert_eq!(z.players(), vec![pid(1), pid(2), pid(3)]);
    assert!(z.config().free_graveyard_reorder);
    for zone in zone_ids(&[pid(1), pid(2), pid(3)]) {
        assert!(z.contents(zone).is_empty());
        assert!(z.is_empty(zone));
    }
    assert!(!ZonesConfig::default().free_graveyard_reorder);
    let d = MoveRequest::new(ZoneKind::Exile);
    assert_eq!(d.to, ZoneKind::Exile);
    assert!(matches!(d.position, Position::Top));
    assert!(!d.face_down);
    assert_eq!(d.controller, None);
}

// REQ-ZONE-001, CR 400.3: with no players, every call naming a player fails cleanly.
#[test]
fn empty_player_list_rejects_owners_without_panic() {
    let mut z = Zones::new(&[], ZonesConfig::default());
    let mut g = IdGenerator::<ObjectId>::new();
    for kind in ZoneKind::ALL {
        assert_eq!(
            z.create_object(&mut g, cid(1), pid(1), req(kind)),
            Err(ZoneError::UnknownPlayer(pid(1)))
        );
    }
    assert_eq!(
        z.add_outside_card(pid(1), OutsidePool::Companion, cid(1)),
        Err(ZoneError::UnknownPlayer(pid(1)))
    );
    assert_eq!(g.last_issued(), 0);
    assert_eq!(z, Zones::new(&[], ZonesConfig::default()));
}

// REQ-ZONE-001, REQ-CORE-002: unknown ids (including 0 and u64::MAX) give errors / None /
// empty, never a panic.
#[test]
fn odd_arguments_never_panic() {
    let mut z = Zones::new(
        &[pid(1), pid(2)],
        ZonesConfig {
            free_graveyard_reorder: true,
        },
    );
    let mut g = IdGenerator::<ObjectId>::new();
    let a = z
        .create_object(&mut g, cid(1), pid(1), req(ZoneKind::Graveyard))
        .unwrap();
    let before = z.clone();
    for raw in [0, 3, 1000, u64::MAX] {
        let o = oid(raw);
        assert_eq!(z.object(o), None);
        assert_eq!(z.is_public_info(o), None);
        assert_eq!(
            z.move_object(&mut g, o, req(ZoneKind::Hand)).map(|m| m.new),
            Err(ZoneError::UnknownObject(o))
        );
        assert_eq!(
            z.send_outside(o, OutsidePool::Sideboard),
            Err(ZoneError::UnknownObject(o))
        );
        assert_eq!(
            z.set_controller(o, pid(1)),
            Err(ZoneError::UnknownObject(o))
        );
        assert_eq!(z.object_of_card(cid(raw)), None);
        let p = pid(raw);
        assert_eq!(
            z.reorder_graveyard(p, Vec::new()),
            Err(ZoneError::UnknownPlayer(p))
        );
        assert!(z.contents(ZoneId::Library(p)).is_empty());
        assert_eq!(z.len(ZoneId::Hand(p)), 0);
        assert!(z.is_empty(ZoneId::Graveyard(p)));
        for pool in ALL_POOLS {
            assert!(z.outside_cards(p, pool).is_empty());
        }
    }
    // Empty and odd reorder lists on a real graveyard.
    assert_eq!(
        z.reorder_graveyard(pid(1), Vec::new()),
        Err(ZoneError::InvalidOrder)
    );
    assert_eq!(
        z.reorder_graveyard(pid(1), vec![oid(0), oid(u64::MAX)]),
        Err(ZoneError::InvalidOrder)
    );
    assert_eq!(z, before);
    assert_eq!(g.last_issued(), 1);
    assert_eq!(z.object_of_card(cid(1)), Some(a));
    // Extreme positions and card ids. Both calls must succeed (finding F6, comment
    // 4621941418); an index beyond the length goes to the bottom (CR 401.7), so the second
    // object is below the first.
    let mut made = Vec::new();
    for (i, raw) in [0, u64::MAX].into_iter().enumerate() {
        let r = req_pos(ZoneKind::Library, Position::Index(usize::MAX));
        let id = z
            .create_object(&mut g, cid(raw), pid(2), r)
            .expect("Index(usize::MAX) is a valid position: the bottom");
        assert_eq!(z.object(id).unwrap().card, cid(raw));
        assert_eq!(z.object_of_card(cid(raw)), Some(id));
        assert_eq!(z.len(ZoneId::Library(pid(2))), i + 1);
        made.push(id);
    }
    assert_eq!(made, vec![oid(2), oid(3)]);
    assert_eq!(z.contents(ZoneId::Library(pid(2))), made);
}

// REQ-ZONE-001, REQ-CORE-002: ZoneError values display non-empty text and are std errors.
#[test]
fn zone_error_display_and_error_trait() {
    let errors = [
        ZoneError::UnknownPlayer(pid(1)),
        ZoneError::UnknownObject(oid(2)),
        ZoneError::CardAlreadyPresent(cid(3)),
        ZoneError::IdExhausted,
        ZoneError::ControllerNotApplicable,
        ZoneError::ReorderNotAllowed,
        ZoneError::InvalidOrder,
        ZoneError::NoSuchOutsideCard(cid(4)),
        ZoneError::IdCollision(oid(5)),
    ];
    for e in errors {
        assert!(!e.to_string().is_empty(), "{e:?}");
        let as_error: &dyn std::error::Error = &e;
        assert!(!as_error.to_string().is_empty());
    }
}
