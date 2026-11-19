// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Knowledge across zone changes: `on_move` rules (a)-(d), `on_player_leaves` and
//! `on_game_end` (REQ-CORE-004, REQ-ZONE-001; CR 400.2, 400.7, 402.3, 708.5, 708.9).
//! Black-box: written from the work-item brief only.

#[path = "spec_knowledge_support.rs"]
mod support;

use fineprint_core::knowledge::KnowledgeError;
use fineprint_core::zones::{MoveRequest, Position, ZoneId, ZoneKind};
use support::*;

const PUBLIC: [ZoneKind; 5] = [
    ZoneKind::Graveyard,
    ZoneKind::Battlefield,
    ZoneKind::Stack,
    ZoneKind::Exile,
    ZoneKind::Command,
];

fn fd_req(kind: ZoneKind) -> MoveRequest {
    let mut r = MoveRequest::new(kind);
    r.face_down = true;
    r
}

/// REQ-CORE-004, CR 400.2: rule (a) a face-up object leaving a public zone for a hidden zone
/// was seen by everyone; every player keeps knowing it.
#[test]
fn public_face_up_to_hidden_zone_everyone_knows() {
    for dest in [ZoneKind::Hand, ZoneKind::Library] {
        for from in PUBLIC {
            let mut w = KWorld::three();
            let id = w.create(from, P2);
            let card = w.card(id);
            let moved = w.mv_to(id, dest);
            assert_eq!(moved.to, ZoneId::of(dest, P2));
            assert_eq!(
                w.known_to(moved.new),
                set(&[P1, P2, P3]),
                "{from:?}->{dest:?}"
            );
            for p in [P1, P2, P3] {
                assert!(w.can(p, moved.new));
            }
            let v = w.view(P1);
            if dest == ZoneKind::Hand {
                assert_eq!(opp_hand_known(&v, P2), set(&[card]));
            } else {
                assert_eq!(lib_known(&v, P2), set(&[card]));
            }
        }
    }
}

/// REQ-CORE-004, CR 400.2: rule (a) also applies to a face-up public object turned face down
/// in a public zone (exile to exile face down): everyone saw it.
#[test]
fn public_face_up_to_face_down_exile_everyone_knows() {
    let mut w = KWorld::two();
    let id = w.create(ZoneKind::Battlefield, P1);
    let moved = w.mv(id, fd_req(ZoneKind::Exile));
    assert_eq!(w.known_to(moved.new), set(&[P1, P2]));
    let v = w.view(P2);
    let ov = find_view(&v, moved.new).expect("in exile view");
    assert!(ov.face_down);
    assert_eq!(ov.card, Some(w.card(moved.new)));
}

/// REQ-CORE-004, CR 402.3: rule (b) a card leaving its owner's hand is known to the owner
/// (hand to library); the opponent does not learn it.
#[test]
fn hand_to_library_owner_knows_opponent_does_not() {
    let mut w = KWorld::three();
    let h = w.create(ZoneKind::Hand, P1);
    let card = w.card(h);
    let moved = w.mv(h, KWorld::req_at(ZoneKind::Library, Position::Bottom));
    assert_eq!(w.known_to(moved.new), set(&[P1]));
    assert!(w.can(P1, moved.new));
    assert!(!w.can(P2, moved.new));
    assert!(!w.can(P3, moved.new));
    assert_eq!(lib_known(&w.view(P1), P1), set(&[card]));
    assert!(lib_known(&w.view(P2), P1).is_empty());
}

/// REQ-CORE-004, CR 402.3: rule (b) a card exiled face down from a hand stays known to its
/// owner only.
#[test]
fn hand_to_face_down_exile_owner_knows() {
    let mut w = KWorld::two();
    let h = w.create(ZoneKind::Hand, P2);
    let moved = w.mv(h, fd_req(ZoneKind::Exile));
    assert_eq!(w.known_to(moved.new), set(&[P2]));
    assert!(w.can(P2, moved.new));
    assert!(!w.can(P1, moved.new));
    let v = w.view(P1);
    assert_eq!(find_view(&v, moved.new).expect("exile").card, None);
}

/// REQ-CORE-004, CR 708.9: a face-down permanent leaving the battlefield for a hand, library,
/// graveyard or exile is revealed to all; before the move the opponent could not identify it.
#[test]
fn face_down_permanent_leaving_battlefield_is_revealed() {
    let dests = [
        MoveRequest::new(ZoneKind::Hand),
        MoveRequest::new(ZoneKind::Library),
        MoveRequest::new(ZoneKind::Graveyard),
        MoveRequest::new(ZoneKind::Exile),
        fd_req(ZoneKind::Exile),
        fd_req(ZoneKind::Graveyard),
    ];
    for req in dests {
        let mut w = KWorld::three();
        let id = w.create_fd(ZoneKind::Battlefield, P1);
        assert!(!w.can(P2, id), "CR 708.5: hidden before");
        assert!(!w.can(P3, id));
        let moved = w.mv(id, req);
        assert_eq!(w.known_to(moved.new), set(&[P1, P2, P3]), "{req:?}");
        for p in [P1, P2, P3] {
            assert!(w.can(p, moved.new), "{req:?} {p:?}");
        }
    }
}

/// REQ-CORE-004, CR 708.9: a face-down permanent controlled by a non-owner leaving the
/// battlefield is revealed to all, including its owner.
#[test]
fn face_down_permanent_of_other_controller_revealed_on_leaving() {
    let mut w = KWorld::two();
    let id = w.create_fd_ctl(ZoneKind::Battlefield, P2, P1);
    assert!(!w.can(P2, id));
    let moved = w.mv_to(id, ZoneKind::Hand);
    assert_eq!(moved.to, ZoneId::Hand(P2));
    assert_eq!(w.known_to(moved.new), set(&[P1, P2]));
}

/// REQ-CORE-004, CR 708.5, 708.9: a face-down spell resolving onto the battlefield is not
/// revealed; its controller still knows it (by memory and by CR 708.5).
#[test]
fn face_down_spell_to_battlefield_not_revealed() {
    let mut w = KWorld::three();
    let s = w.create_fd(ZoneKind::Stack, P1);
    let moved = w.mv(s, fd_req(ZoneKind::Battlefield));
    assert_eq!(
        w.known_to(moved.new),
        set(&[P1]),
        "controller added, no reveal"
    );
    assert!(w.can(P1, moved.new));
    assert!(!w.can(P2, moved.new));
    assert!(!w.can(P3, moved.new));
    let v = w.view(P2);
    let ov = find_view(&v, moved.new).expect("battlefield");
    assert!(ov.face_down);
    assert_eq!(ov.card, None);
}

/// REQ-CORE-004, CR 708.5: the controller's memory survives a later change of control: after
/// the face-down permanent is controlled by another player, the old controller still knows it.
#[test]
fn face_down_controller_memory_survives_control_change() {
    let mut w = KWorld::two();
    let s = w.create_fd(ZoneKind::Stack, P1);
    let moved = w.mv(s, fd_req(ZoneKind::Battlefield));
    w.zones.set_controller(moved.new, P2).expect("control");
    assert!(w.can(P1, moved.new), "P1 remembers it");
    assert!(w.can(P2, moved.new), "P2 controls it");
}

/// REQ-CORE-004, CR 708.9: a face-down spell moving from the stack to a graveyard (or exile, a
/// hand) is revealed to all.
#[test]
fn face_down_spell_off_stack_is_revealed() {
    for req in [
        MoveRequest::new(ZoneKind::Graveyard),
        fd_req(ZoneKind::Exile),
        MoveRequest::new(ZoneKind::Hand),
    ] {
        let mut w = KWorld::three();
        let s = w.create_fd(ZoneKind::Stack, P2);
        assert!(!w.can(P1, s));
        let moved = w.mv(s, req);
        assert_eq!(w.known_to(moved.new), set(&[P1, P2, P3]), "{req:?}");
    }
}

/// REQ-CORE-004, CR 708.9: a face-down permanent exiled face down is revealed to all as it
/// moves; afterwards everyone identifies it by memory, though it is shown face down.
#[test]
fn face_down_permanent_to_face_down_exile_revealed_yet_face_down() {
    let mut w = KWorld::two();
    let id = w.create_fd(ZoneKind::Battlefield, P1);
    let card = w.card(id);
    let moved = w.mv(id, fd_req(ZoneKind::Exile));
    assert!(w.rec(moved.new).face_down);
    assert_eq!(w.known_to(moved.new), set(&[P1, P2]));
    for p in [P1, P2] {
        let ov = find_view(&w.view(p), moved.new).expect("exile");
        assert!(ov.face_down, "{p:?}");
        assert_eq!(ov.card, Some(card), "{p:?}");
    }
}

/// REQ-CORE-004, CR 708.5, 708.9: a same-zone reposition of a face-down permanent (from ==
/// to) is no reveal; only its controller is stored.
#[test]
fn face_down_same_zone_move_is_not_a_reveal() {
    let mut w = KWorld::two();
    let id = w.create_fd(ZoneKind::Battlefield, P1);
    let moved = w.mv(id, fd_req(ZoneKind::Battlefield));
    assert_eq!(moved.old, moved.new);
    assert_eq!(w.known_to(moved.new), set(&[P1]));
    assert!(!w.can(P2, moved.new));
}

/// REQ-CORE-004, CR 406.3: a face-down card leaving a graveyard or exile (not a permanent or a
/// spell) is not revealed by the move.
#[test]
fn face_down_non_permanent_moving_is_not_revealed() {
    for from in [ZoneKind::Graveyard, ZoneKind::Exile, ZoneKind::Command] {
        let mut w = KWorld::two();
        let id = w.create_fd(from, P1);
        let moved = w.mv_to(id, ZoneKind::Hand);
        assert!(w.known_to(moved.new).is_empty(), "{from:?}");
        assert!(!w.can(P2, moved.new), "{from:?}");
        assert!(w.can(P1, moved.new), "own hand");
    }
}

/// REQ-CORE-004, CR 401.2: rule (d) a same-zone reposition in a library keeps the card's
/// knowledge and id, drops every entry naming the moved card (even at the same index) and
/// keeps the others.
#[test]
fn same_zone_library_reposition_drops_entries_of_moved_card() {
    let mut w = KWorld::three();
    let lib: Vec<_> = (0..3)
        .map(|_| w.create_bottom(ZoneKind::Library, P1))
        .collect();
    for (i, id) in lib.iter().enumerate() {
        w.k.set_position_known(&w.zones, P1, P1, i, *id)
            .expect("pos");
        w.k.set_position_known(&w.zones, P2, P1, i, *id)
            .expect("pos");
    }
    w.k.add_known(&w.zones, lib[1], P3).expect("add");
    let moved = w.mv(
        lib[1],
        KWorld::req_at(ZoneKind::Library, Position::Index(1)),
    );
    assert_eq!(moved.old, moved.new);
    assert_eq!(w.library(P1), lib, "same order");
    assert_eq!(w.known_to(lib[1]), set(&[P1, P2, P3]), "knowledge kept");
    for v in [P1, P2] {
        let pos = w.positions(v, P1);
        assert_eq!(
            pos,
            [(0, lib[0]), (2, lib[2])].into_iter().collect(),
            "{v:?}"
        );
    }
    // move the top card to the bottom: its entries dropped, the others now stale
    w.mv(lib[0], KWorld::req_at(ZoneKind::Library, Position::Bottom));
    assert_eq!(w.library(P1), vec![lib[1], lib[2], lib[0]]);
    for v in [P1, P2] {
        assert!(w.positions(v, P1).is_empty(), "{v:?}");
    }
    // moving lib[0] back to the top must not resurrect its dropped entry
    w.raw_mv(lib[0], KWorld::req_at(ZoneKind::Library, Position::Top));
    assert_eq!(w.library(P1), lib);
    for v in [P1, P2] {
        let pos = w.positions(v, P1);
        assert!(!pos.values().any(|id| *id == lib[0]), "{v:?}");
        assert!(!pos.values().any(|id| *id == lib[1]), "{v:?}");
    }
}

/// REQ-CORE-004, CR 401.2, 400.7: a drawn card (library to hand) loses its position entries;
/// knowledge follows the card to its new id.
#[test]
fn drawn_card_loses_position_entries() {
    let mut w = KWorld::two();
    let lib: Vec<_> = (0..3)
        .map(|_| w.create_bottom(ZoneKind::Library, P1))
        .collect();
    w.k.set_position_known(&w.zones, P2, P1, 0, lib[0])
        .expect("pos");
    w.k.set_position_known(&w.zones, P2, P1, 2, lib[2])
        .expect("pos");
    let moved = w.mv_to(lib[0], ZoneKind::Hand);
    assert_ne!(moved.old, moved.new);
    let pos = w.positions(P2, P1);
    assert!(!pos.values().any(|id| *id == lib[0] || *id == moved.new));
    assert!(w.can(P2, moved.new), "P2 knew the card: memory follows it");
    assert!(w.known_to(moved.new).contains(&P2));
    // put the card back on top with a fresh id: the old entry stays gone
    let back = w.mv(moved.new, KWorld::req_at(ZoneKind::Library, Position::Top));
    assert_eq!(w.library(P1)[0], back.new);
    assert!(!w.positions(P2, P1).contains_key(&0));
}

/// REQ-CORE-004, CR 400.7: object ids change on a zone move, but stored knowledge follows the
/// card to the new id across several moves.
#[test]
fn knowledge_follows_the_card_across_moves() {
    let mut w = KWorld::three();
    let l = w.create(ZoneKind::Library, P1);
    let card = w.card(l);
    w.k.add_known(&w.zones, l, P3).expect("add");
    let m1 = w.mv_to(l, ZoneKind::Hand);
    let m2 = w.mv(m1.new, fd_req(ZoneKind::Exile));
    let m3 = w.mv(m2.new, fd_req(ZoneKind::Exile));
    assert_ne!(m2.new, m3.new, "same-zone exile gives a new id");
    assert_eq!(w.card(m3.new), card);
    assert!(w.known_to(m3.new).contains(&P3));
    assert!(
        w.known_to(m3.new).contains(&P1),
        "owner saw it leave the hand"
    );
    assert!(!w.known_to(m3.new).contains(&P2));
    assert!(w.can(P3, m3.new));
    assert!(!w.can(P2, m3.new));
}

/// REQ-CORE-004, CR 708.9: when a player leaves, only the face-down permanents and spells that
/// player owns are revealed to all; their face-down exiled cards, hand and library are not, and
/// other players' face-down permanents stay hidden.
#[test]
fn player_leaves_reveals_only_their_face_down_permanents_and_spells() {
    let mut w = KWorld::three();
    let bf = w.create_fd(ZoneKind::Battlefield, P2);
    let st = w.create_fd(ZoneKind::Stack, P2);
    let owned_ctl_other = w.create_fd_ctl(ZoneKind::Battlefield, P2, P1);
    let ex = w.create_fd(ZoneKind::Exile, P2);
    let hand = w.create(ZoneKind::Hand, P2);
    let lib = w.create(ZoneKind::Library, P2);
    let other_bf = w.create_fd(ZoneKind::Battlefield, P1);
    let other_st = w.create_fd(ZoneKind::Stack, P3);
    w.k.on_player_leaves(&w.zones, P2).expect("leave");
    for id in [bf, st, owned_ctl_other] {
        assert_eq!(w.known_to(id), set(&[P1, P2, P3]), "{:?}", w.rec(id));
    }
    for id in [ex, hand, lib, other_bf, other_st] {
        assert!(w.known_to(id).is_empty(), "{:?}", w.rec(id));
    }
    assert!(!w.can(P1, ex));
    assert!(!w.can(P1, hand));
    assert!(!w.can(P2, other_bf));
    assert!(!w.can(P1, other_st));
}

/// REQ-CORE-004, R12: `on_player_leaves` for a player not in the game is `UnknownPlayer`.
#[test]
fn player_leaves_unknown_player() {
    let mut w = KWorld::two();
    w.create_fd(ZoneKind::Battlefield, P1);
    let snapshot = w.k.clone();
    for bad in [NOBODY, pid(u64::MAX)] {
        assert_eq!(
            w.k.on_player_leaves(&w.zones, bad),
            Err(KnowledgeError::UnknownPlayer(bad))
        );
    }
    assert_eq!(w.k, snapshot);
}

/// REQ-CORE-004, CR 708.9: at the end of the game all face-down permanents and spells are
/// revealed to all players; hidden zones are not revealed.
#[test]
fn game_end_reveals_all_face_down_permanents_and_spells() {
    let mut w = KWorld::three();
    let a = w.create_fd(ZoneKind::Battlefield, P1);
    let b = w.create_fd_ctl(ZoneKind::Battlefield, P2, P3);
    let c = w.create_fd(ZoneKind::Stack, P3);
    let hand = w.create(ZoneKind::Hand, P1);
    let lib = w.create(ZoneKind::Library, P2);
    w.k.on_game_end(&w.zones);
    for id in [a, b, c] {
        assert_eq!(w.known_to(id), set(&[P1, P2, P3]));
        for p in [P1, P2, P3] {
            assert!(w.can(p, id));
        }
    }
    assert!(w.known_to(hand).is_empty());
    assert!(w.known_to(lib).is_empty());
    assert!(!w.can(P2, hand));
    assert!(!w.can(P1, lib));
}
