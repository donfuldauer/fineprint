// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Identity knowledge: `add_known`, `reveal_to_all`, `known_to`, `can_identify`, `grant_look`
//! and `shuffle_face_down_pile` (REQ-CORE-004, REQ-ZONE-001; CR 400.2, 402.3, 708.5, 406.3,
//! 406.3b, 701.20a/b, 701.24a). Black-box: written from the work-item brief only.

#[path = "spec_knowledge_support.rs"]
mod support;

use fineprint_core::knowledge::{Knowledge, KnowledgeError};
use fineprint_core::zones::{MoveRequest, ZoneId, ZoneKind};
use support::*;

const HIDDEN: [ZoneKind; 2] = [ZoneKind::Library, ZoneKind::Hand];
const PUBLIC: [ZoneKind; 5] = [
    ZoneKind::Graveyard,
    ZoneKind::Battlefield,
    ZoneKind::Stack,
    ZoneKind::Exile,
    ZoneKind::Command,
];

/// REQ-CORE-004: a fresh knowledge stores nothing; `Default` equals `new`.
#[test]
fn new_knowledge_stores_nothing() {
    let mut w = KWorld::two();
    assert_eq!(Knowledge::default(), Knowledge::new());
    for kind in ZoneKind::ALL {
        let id = w.create(kind, P1);
        assert!(
            w.known_to(id).is_empty(),
            "{kind:?}: stored set starts empty"
        );
    }
}

/// REQ-CORE-004, CR 400.2: rule (1) public face-up objects are identifiable by every player,
/// yet `known_to` reports the stored set only (no implicit entitlements).
#[test]
fn rule1_public_face_up_identifiable_by_all_without_stored_entry() {
    let mut w = KWorld::three();
    for kind in PUBLIC {
        let id = w.create(kind, P2);
        for p in [P1, P2, P3] {
            assert!(w.can(p, id), "{kind:?} visible to {p:?}");
        }
        assert!(w.known_to(id).is_empty(), "{kind:?}: no implicit entry");
    }
}

/// REQ-CORE-004, CR 402.3: rule (2) a player identifies the cards in their own hand; an
/// opponent cannot (negative: opponent hand).
#[test]
fn rule2_own_hand_identifiable_opponent_hand_not() {
    let mut w = KWorld::three();
    let h = w.create(ZoneKind::Hand, P1);
    assert!(w.can(P1, h));
    assert!(!w.can(P2, h));
    assert!(!w.can(P3, h));
    assert!(
        w.known_to(h).is_empty(),
        "own-hand right is implicit, not stored"
    );
}

/// REQ-CORE-004, CR 401.2: negative, nobody (not even the owner) identifies a library card
/// with no stored knowledge.
#[test]
fn library_card_unidentifiable_by_everyone() {
    let mut w = KWorld::three();
    let l = w.create(ZoneKind::Library, P1);
    for p in [P1, P2, P3] {
        assert!(!w.can(p, l), "{p:?}");
    }
}

/// REQ-CORE-004, CR 708.5: rule (3) the controller identifies a face-down permanent or spell
/// they control; an opponent (even the owner when not controlling it) cannot.
#[test]
fn rule3_face_down_permanent_and_spell_controller_only() {
    let mut w = KWorld::three();
    for kind in [ZoneKind::Battlefield, ZoneKind::Stack] {
        let own = w.create_fd(kind, P1);
        assert!(w.can(P1, own), "{kind:?}: controller");
        assert!(!w.can(P2, own), "{kind:?}: opponent");
        assert!(!w.can(P3, own), "{kind:?}: opponent");
        // owned by P2, controlled by P1: the owner has no right under CR 708.5
        let stolen = w.create_fd_ctl(kind, P2, P1);
        assert!(w.can(P1, stolen), "{kind:?}: controller, not owner");
        assert!(!w.can(P2, stolen), "{kind:?}: owner but not controller");
        assert!(w.known_to(own).is_empty(), "controller right is implicit");
    }
}

/// REQ-CORE-004, CR 406.3, 708.5: negative, the owner of a face-down card in exile,
/// graveyard or command zone has no implicit right to identify it.
#[test]
fn face_down_outside_battlefield_and_stack_owner_has_no_right() {
    let mut w = KWorld::two();
    for kind in [ZoneKind::Exile, ZoneKind::Graveyard, ZoneKind::Command] {
        let id = w.create_fd(kind, P1);
        assert!(!w.can(P1, id), "{kind:?}: owner");
        assert!(!w.can(P2, id), "{kind:?}: opponent");
    }
}

/// REQ-CORE-004: rule (4) a stored KnownTo entry lets exactly that player identify a card in
/// every zone where they could not otherwise.
#[test]
fn rule4_add_known_grants_that_player_only() {
    let mut w = KWorld::three();
    let cases = [
        (w.create(ZoneKind::Library, P1), P2),
        (w.create(ZoneKind::Hand, P1), P3),
        (w.create_fd(ZoneKind::Battlefield, P1), P2),
        (w.create_fd(ZoneKind::Stack, P2), P3),
        (w.create_fd(ZoneKind::Exile, P1), P1),
        (w.create_fd(ZoneKind::Graveyard, P3), P2),
        (w.create_fd(ZoneKind::Command, P2), P1),
    ];
    for (id, p) in cases {
        let others: Vec<_> = [P1, P2, P3]
            .into_iter()
            .filter(|q| *q != p && !w.can(*q, id))
            .collect();
        assert!(!w.can(p, id), "precondition");
        w.k.add_known(&w.zones, id, p).expect("add_known");
        assert_eq!(w.known_to(id), set(&[p]));
        assert!(w.can(p, id), "{p:?} now identifies");
        for q in others {
            assert!(!w.can(q, id), "{q:?} still does not");
        }
        // idempotent
        w.k.add_known(&w.zones, id, p).expect("add_known again");
        assert_eq!(w.known_to(id), set(&[p]));
    }
}

/// REQ-CORE-004, CR 701.20a: `reveal_to_all` stores every player of the game.
#[test]
fn reveal_to_all_stores_every_player() {
    for players in [vec![P1, P2], vec![P1, P2, P3]] {
        let mut w = KWorld::with_players(&players);
        for kind in HIDDEN {
            let id = w.create(kind, P2);
            w.k.reveal_to_all(&w.zones, id).expect("reveal");
            assert_eq!(w.known_to(id), set(&players));
            for p in &players {
                assert!(w.can(*p, id));
            }
        }
    }
}

/// REQ-CORE-004, CR 701.20b: revealing does not move the object (same id, zone, position).
#[test]
fn reveal_does_not_move_the_object() {
    let mut w = KWorld::two();
    let a = w.create(ZoneKind::Library, P1);
    let b = w.create(ZoneKind::Library, P1);
    let h = w.create(ZoneKind::Hand, P2);
    let zones_before = w.zones.clone();
    let ids_before = w.ids.clone();
    w.k.reveal_to_all(&w.zones, a).expect("reveal");
    w.k.reveal_to_all(&w.zones, h).expect("reveal");
    w.k.add_known(&w.zones, b, P2).expect("add");
    assert_eq!(
        w.zones, zones_before,
        "Zones untouched (the API takes &Zones)"
    );
    assert_eq!(w.ids, ids_before);
    assert_eq!(w.library(P1), vec![b, a]);
    assert_eq!(w.rec(h).zone, ZoneId::Hand(P2));
}

/// REQ-CORE-004, R12: unknown objects and players are errors, never panics, and leave the
/// knowledge unchanged.
#[test]
fn error_paths_unknown_object_and_player() {
    let mut w = KWorld::two();
    let id = w.create(ZoneKind::Library, P1);
    let fd = w.create_fd(ZoneKind::Exile, P1);
    let snapshot = w.k.clone();
    for bad in [oid(0), oid(12_345), oid(u64::MAX)] {
        assert_eq!(
            w.k.known_to(&w.zones, bad),
            Err(KnowledgeError::UnknownObject(bad))
        );
        assert_eq!(
            w.k.add_known(&w.zones, bad, P1),
            Err(KnowledgeError::UnknownObject(bad))
        );
        assert_eq!(
            w.k.reveal_to_all(&w.zones, bad),
            Err(KnowledgeError::UnknownObject(bad))
        );
        assert_eq!(
            w.k.can_identify(&w.zones, P1, bad),
            Err(KnowledgeError::UnknownObject(bad))
        );
        assert_eq!(
            w.k.grant_look(&w.zones, bad, P1),
            Err(KnowledgeError::UnknownObject(bad))
        );
        assert_eq!(
            w.k.shuffle_face_down_pile(&w.zones, &[bad]),
            Err(KnowledgeError::UnknownObject(bad))
        );
        assert_eq!(
            w.k.shuffle_face_down_pile(&w.zones, &[fd, bad]),
            Err(KnowledgeError::UnknownObject(bad))
        );
    }
    for bad in [NOBODY, pid(0), pid(u64::MAX)] {
        assert_eq!(
            w.k.add_known(&w.zones, id, bad),
            Err(KnowledgeError::UnknownPlayer(bad))
        );
        assert_eq!(
            w.k.grant_look(&w.zones, fd, bad),
            Err(KnowledgeError::UnknownPlayer(bad))
        );
        // can_identify for a non-player is an error, never Ok(false)
        assert_eq!(
            w.k.can_identify(&w.zones, bad, id),
            Err(KnowledgeError::UnknownPlayer(bad))
        );
        assert_eq!(
            w.k.can_identify(&w.zones, bad, fd),
            Err(KnowledgeError::UnknownPlayer(bad))
        );
        // UnknownObject is checked first
        assert_eq!(
            w.k.can_identify(&w.zones, bad, oid(54_321)),
            Err(KnowledgeError::UnknownObject(oid(54_321)))
        );
    }
    assert_eq!(w.k, snapshot, "failed calls change nothing");
    assert!(w.known_to(id).is_empty());
}

/// REQ-CORE-004, CR 406.3, 406.3b: `grant_look` on a face-down exiled card lets exactly that
/// player identify it (it is stored in KnownTo).
#[test]
fn grant_look_on_face_down_exile() {
    let mut w = KWorld::three();
    let x = w.create_fd(ZoneKind::Exile, P1);
    w.k.grant_look(&w.zones, x, P2).expect("grant");
    assert!(w.can(P2, x));
    assert!(!w.can(P1, x), "owner still has no right");
    assert!(!w.can(P3, x));
    assert!(w.known_to(x).contains(&P2));
    assert!(!w.known_to(x).contains(&P1));
    assert!(!w.known_to(x).contains(&P3));
}

/// REQ-CORE-004, CR 406.3: `grant_look` requires face down AND in exile; a face-up exiled
/// card and a face-down permanent give `NotFaceDown` and change nothing.
#[test]
fn grant_look_not_face_down_errors() {
    let mut w = KWorld::two();
    let up = w.create(ZoneKind::Exile, P1);
    let fd_bf = w.create_fd(ZoneKind::Battlefield, P1);
    let fd_stack = w.create_fd(ZoneKind::Stack, P1);
    let lib = w.create(ZoneKind::Library, P1);
    let hand = w.create(ZoneKind::Hand, P1);
    let snapshot = w.k.clone();
    for id in [up, fd_bf, fd_stack, lib, hand] {
        assert_eq!(
            w.k.grant_look(&w.zones, id, P2),
            Err(KnowledgeError::NotFaceDown(id)),
            "{:?}",
            w.rec(id).zone
        );
    }
    assert_eq!(w.k, snapshot);
    assert!(!w.can(P2, fd_bf));
}

/// REQ-CORE-004, CR 406.3: a look permission persists through unrelated events (other moves,
/// reveals, shuffles of a library) until the card leaves exile or is shuffled.
#[test]
fn grant_persists_after_unrelated_events() {
    let mut w = KWorld::two();
    let x = w.create_fd(ZoneKind::Exile, P1);
    let other_exile = w.create_fd(ZoneKind::Exile, P2);
    w.k.grant_look(&w.zones, x, P2).expect("grant");
    let l1 = w.create(ZoneKind::Library, P1);
    let _l2 = w.create(ZoneKind::Library, P1);
    let h = w.create(ZoneKind::Hand, P2);
    w.mv_to(h, ZoneKind::Graveyard);
    w.k.reveal_to_all(&w.zones, l1).expect("reveal");
    w.k.shuffle_library(&w.zones, P1, &[]).expect("shuffle");
    w.k.grant_look(&w.zones, other_exile, P1)
        .expect("grant other");
    w.mv(other_exile, MoveRequest::new(ZoneKind::Graveyard));
    w.k.shuffle_face_down_pile(&w.zones, &[])
        .expect("empty pile");
    assert!(w.can(P2, x), "grant persists");
    assert!(w.known_to(x).contains(&P2));
    assert!(!w.can(P1, x));
}

/// REQ-CORE-004, CR 406.3, 701.24a: shuffling a face-down pile clears the stored KnownTo of
/// its cards (including grants), but not of cards outside the pile.
#[test]
fn shuffle_face_down_pile_clears_known_to() {
    let mut w = KWorld::three();
    let a = w.create_fd(ZoneKind::Exile, P1);
    let b = w.create_fd(ZoneKind::Exile, P2);
    let outside = w.create_fd(ZoneKind::Exile, P3);
    w.k.grant_look(&w.zones, a, P2).expect("grant");
    w.k.grant_look(&w.zones, b, P3).expect("grant");
    w.k.reveal_to_all(&w.zones, b).expect("reveal");
    w.k.grant_look(&w.zones, outside, P1).expect("grant");
    w.k.shuffle_face_down_pile(&w.zones, &[a, b])
        .expect("shuffle");
    assert!(w.known_to(a).is_empty());
    assert!(w.known_to(b).is_empty());
    for p in [P1, P2, P3] {
        assert!(!w.can(p, a));
        assert!(!w.can(p, b));
    }
    assert_eq!(w.known_to(outside), set(&[P1]));
    assert!(w.can(P1, outside));
}

/// REQ-CORE-004, CR 701.24a: a face-up id in the pile gives `NotFaceDown` and the call
/// changes nothing (the face-down card keeps its grant).
#[test]
fn shuffle_face_down_pile_rejects_face_up() {
    let mut w = KWorld::two();
    let a = w.create_fd(ZoneKind::Exile, P1);
    let up = w.create(ZoneKind::Exile, P1);
    w.k.grant_look(&w.zones, a, P2).expect("grant");
    let snapshot = w.k.clone();
    assert_eq!(
        w.k.shuffle_face_down_pile(&w.zones, &[a, up]),
        Err(KnowledgeError::NotFaceDown(up))
    );
    assert_eq!(w.k, snapshot, "rejected call changes nothing");
    assert!(w.can(P2, a));
}

/// REQ-CORE-004, CR 406.3, 701.24a: the pile must be face down AND in exile; a face-down
/// object in a library, on the battlefield, on the stack, in a graveyard or in the command zone
/// gives `NotFaceDown` and changes nothing (the library card stays in `known_cards`).
#[test]
fn shuffle_face_down_pile_rejects_face_down_outside_exile() {
    let mut w = KWorld::two();
    let lib = w.create_fd(ZoneKind::Library, P1);
    let bf = w.create_fd(ZoneKind::Battlefield, P1);
    let st = w.create_fd(ZoneKind::Stack, P1);
    let gy = w.create_fd(ZoneKind::Graveyard, P1);
    let cm = w.create_fd(ZoneKind::Command, P1);
    let hand = w.create(ZoneKind::Hand, P1);
    for id in [lib, bf, st, gy, cm, hand] {
        w.k.add_known(&w.zones, id, P2).expect("add");
    }
    let snapshot = w.k.clone();
    for id in [lib, bf, st, gy, cm, hand] {
        assert_eq!(
            w.k.shuffle_face_down_pile(&w.zones, &[id]),
            Err(KnowledgeError::NotFaceDown(id)),
            "{:?}",
            w.rec(id)
        );
    }
    assert_eq!(w.k, snapshot, "rejected calls change nothing");
    assert_eq!(w.known_to(lib), set(&[P2]));
    assert_eq!(lib_known(&w.view(P2), P1), set(&[w.card(lib)]));
    assert!(w.can(P2, bf));
}

/// REQ-CORE-004, CR 406.3, 701.24a: a pile mixing valid exiled cards with an invalid id is
/// rejected as a whole; all ids are validated before any change, so nothing is cleared.
#[test]
fn shuffle_face_down_pile_mixed_valid_and_invalid_clears_nothing() {
    let mut w = KWorld::three();
    let a = w.create_fd(ZoneKind::Exile, P1);
    let b = w.create_fd(ZoneKind::Exile, P2);
    let bf = w.create_fd(ZoneKind::Battlefield, P1);
    let lib = w.create_fd(ZoneKind::Library, P1);
    w.k.grant_look(&w.zones, a, P2).expect("grant");
    w.k.grant_look(&w.zones, b, P3).expect("grant");
    let snapshot = w.k.clone();
    assert_eq!(
        w.k.shuffle_face_down_pile(&w.zones, &[a, b, bf]),
        Err(KnowledgeError::NotFaceDown(bf))
    );
    assert_eq!(
        w.k.shuffle_face_down_pile(&w.zones, &[a, lib, b]),
        Err(KnowledgeError::NotFaceDown(lib))
    );
    assert_eq!(
        w.k.shuffle_face_down_pile(&w.zones, &[a, oid(77_777)]),
        Err(KnowledgeError::UnknownObject(oid(77_777)))
    );
    assert_eq!(w.k, snapshot, "nothing cleared");
    assert!(w.can(P2, a));
    assert!(w.can(P3, b));
    // the valid exile pile alone still clears
    w.k.shuffle_face_down_pile(&w.zones, &[a, b]).expect("pile");
    assert!(w.known_to(a).is_empty());
    assert!(w.known_to(b).is_empty());
}

/// REQ-CORE-004: KnownTo is player memory stored per card; it follows the card to its new
/// object id after a zone change (design decision in the brief, CR 400.7 notwithstanding) when
/// the viewer watched the move (`on_move_observed`).
#[test]
fn known_to_follows_the_card_to_the_new_id() {
    let mut w = KWorld::two();
    let l = w.create(ZoneKind::Library, P1);
    w.k.add_known(&w.zones, l, P2).expect("add");
    let moved = w.mv_obs(l, MoveRequest::new(ZoneKind::Hand), &[P2]);
    assert_ne!(moved.old, moved.new, "CR 400.7 new object");
    assert!(w.known_to(moved.new).contains(&P2));
    assert!(w.can(P2, moved.new));
    assert_eq!(
        w.k.known_to(&w.zones, moved.old),
        Err(KnowledgeError::UnknownObject(moved.old)),
        "the old id is gone"
    );
}

/// REQ-CORE-004 (hidden-source identity rule, a design decision of the brief): the same move
/// unobserved by the viewer (`on_move`, the card goes to the owner's hand) makes the viewer
/// forget the card; the owner sees it in their own hand (CR 402.3).
#[test]
fn known_to_forgotten_when_the_viewer_did_not_watch_the_move() {
    let mut w = KWorld::two();
    let l = w.create(ZoneKind::Library, P1);
    w.k.add_known(&w.zones, l, P2).expect("add");
    let moved = w.mv_to(l, ZoneKind::Hand);
    assert!(!w.known_to(moved.new).contains(&P2));
    assert!(!w.can(P2, moved.new));
    assert!(opp_hand_known(&w.view(P2), P1).is_empty());
    assert!(w.can(P1, moved.new));
}

/// REQ-CORE-004: `Display` and `Error` are implemented for every error variant.
#[test]
fn error_display_and_error_trait() {
    let errs = [
        KnowledgeError::UnknownObject(oid(7)),
        KnowledgeError::UnknownPlayer(NOBODY),
        KnowledgeError::NotInLibrary {
            owner: P1,
            object: oid(3),
        },
        KnowledgeError::IndexMismatch {
            owner: P1,
            index: 4,
            object: oid(3),
        },
        KnowledgeError::NotFaceDown(oid(9)),
    ];
    for e in errs {
        assert!(!e.to_string().is_empty());
        let dynerr: &dyn std::error::Error = &e;
        assert!(!dynerr.to_string().is_empty());
    }
}
