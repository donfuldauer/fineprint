// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Knowledge across zone changes: `on_move` rules (a)-(d), `on_player_leaves` and
//! `on_game_end` (REQ-CORE-004, REQ-ZONE-001; CR 400.2, 400.7, 402.3, 708.5, 708.9).
//! Black-box: written from the work-item brief only.

#[path = "spec_knowledge_support.rs"]
mod support;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::knowledge::KnowledgeError;
use fineprint_core::zones::{MoveRequest, ObjectRecord, Position, ZoneId, ZoneKind};
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

/// REQ-CORE-004, CR 402.3 (hidden-source identity rule, design decision of the brief): an
/// opponent who knew the card forgets it when the hand-to-library move is unobserved, and keeps
/// it when the move is observed.
#[test]
fn hand_to_library_opponent_knowledge_needs_observation() {
    for observed in [false, true] {
        let mut w = KWorld::three();
        let h = w.create(ZoneKind::Hand, P1);
        let card = w.card(h);
        w.k.add_known(&w.zones, h, P2).expect("add");
        let req = KWorld::req_at(ZoneKind::Library, Position::Bottom);
        let moved = if observed {
            w.mv_obs(h, req, &[P2])
        } else {
            w.mv(h, req)
        };
        assert_eq!(w.can(P2, moved.new), observed, "observed {observed}");
        let expected = if observed { set(&[card]) } else { set(&[]) };
        assert_eq!(lib_known(&w.view(P2), P1), expected);
        assert!(w.can(P1, moved.new), "hand owner (iii)");
        assert!(!w.can(P3, moved.new));
    }
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
/// knowledge follows the card to its new id when P2 watched the draw (hidden-source identity
/// rule (i): the caller passes P2 in `observers` because the selected position, the top, is
/// public). The same draw without observers makes P2 forget: holding a position entry on the
/// drawn card does not make P2 an observer.
#[test]
fn drawn_card_loses_position_entries() {
    let build = || -> (KWorld, Vec<ObjectId>) {
        let mut w = KWorld::two();
        let lib: Vec<_> = (0..3)
            .map(|_| w.create_bottom(ZoneKind::Library, P1))
            .collect();
        w.k.set_position_known(&w.zones, P2, P1, 0, lib[0])
            .expect("pos");
        w.k.set_position_known(&w.zones, P2, P1, 2, lib[2])
            .expect("pos");
        (w, lib)
    };
    // observed draw
    let (mut w, lib) = build();
    let moved = w.mv_obs(lib[0], MoveRequest::new(ZoneKind::Hand), &[P2]);
    assert_ne!(moved.old, moved.new);
    let pos = w.positions(P2, P1);
    assert!(!pos.values().any(|id| *id == lib[0] || *id == moved.new));
    assert!(
        w.can(P2, moved.new),
        "P2 watched the draw: memory follows it"
    );
    assert!(w.known_to(moved.new).contains(&P2));
    assert!(
        w.can(P2, lib[2]),
        "observed: P2 keeps the rest of its knowledge"
    );
    // put the card back on top with a fresh id: the old entry stays gone
    let back = w.mv(moved.new, KWorld::req_at(ZoneKind::Library, Position::Top));
    assert_eq!(w.library(P1)[0], back.new);
    assert!(!w.positions(P2, P1).contains_key(&0));
    // that hand-to-library move was hidden from P2: P2 forgot the card
    assert!(!w.known_to(back.new).contains(&P2));
    assert!(!w.can(P2, back.new));
    // contrast: the same draw with no observers; the position entry alone is not observation
    let (mut u, lib) = build();
    let moved = u.mv_to(lib[0], ZoneKind::Hand);
    assert!(
        !u.can(P2, moved.new),
        "unobserved draw: P2 forgets the card"
    );
    assert!(!u.known_to(moved.new).contains(&P2));
    assert!(!u.can(P2, lib[2]), "non-observer forgets the library cards");
    assert!(!u.known_to(lib[2]).contains(&P2));
    assert!(
        u.positions(P2, P1).is_empty(),
        "non-observer loses every position entry of that library"
    );
}

/// REQ-CORE-004, CR 400.7: object ids change on a zone move, but stored knowledge follows the
/// card to the new id across several moves when the viewer watched each hidden-source move
/// (`on_move_observed`; the hidden-source identity rule is a design decision of the brief).
#[test]
fn knowledge_follows_the_card_across_moves() {
    let mut w = KWorld::three();
    let l = w.create(ZoneKind::Library, P1);
    let card = w.card(l);
    w.k.add_known(&w.zones, l, P3).expect("add");
    let m1 = w.mv_obs(l, MoveRequest::new(ZoneKind::Hand), &[P3]);
    let m2 = w.mv_obs(m1.new, fd_req(ZoneKind::Exile), &[P3]);
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

/// REQ-CORE-004 (hidden-source identity rule, design decision of the brief): the same moves
/// unobserved by P3 make P3 forget the card: library to hand (step 0, P3 neither observer,
/// nor able to see the hand, nor the hand's owner), and the card stays forgotten.
#[test]
fn knowledge_lost_across_unobserved_moves() {
    let mut w = KWorld::three();
    let l = w.create(ZoneKind::Library, P1);
    w.k.add_known(&w.zones, l, P3).expect("add");
    let m1 = w.mv_to(l, ZoneKind::Hand);
    assert!(!w.known_to(m1.new).contains(&P3), "unobserved draw");
    assert!(!w.can(P3, m1.new));
    let m2 = w.mv(m1.new, fd_req(ZoneKind::Exile));
    let m3 = w.mv(m2.new, fd_req(ZoneKind::Exile));
    assert_eq!(w.known_to(m3.new), set(&[P1]), "CR 402.3: only the owner");
    assert!(!w.can(P3, m3.new));
    // observed draw, then an unobserved hand-to-exile move: P3 forgets at the second move
    let mut w = KWorld::three();
    let l = w.create(ZoneKind::Library, P1);
    w.k.add_known(&w.zones, l, P3).expect("add");
    let m1 = w.mv_obs(l, MoveRequest::new(ZoneKind::Hand), &[P3]);
    assert!(w.can(P3, m1.new));
    let m2 = w.mv(m1.new, fd_req(ZoneKind::Exile));
    assert!(!w.known_to(m2.new).contains(&P3));
    assert!(!w.can(P3, m2.new));
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

// ---------------------------------------------------------------------------------------------
// hidden-source moves (the identity rule of the brief, step 0; a design decision, not
// CR-derived). Twin worlds differ only in which hidden card moved.
// ---------------------------------------------------------------------------------------------

const CARD_A: u64 = 10;
const CARD_B: u64 = 11;
const CARD_X: u64 = 20;
const CARD_Y: u64 = 21;

/// P1's library, top first, built with fixed synthetic cards; P2 knows A and B. Object ids are
/// assigned in library order, so the top object has the same id in both twins.
fn draw_twin(top_first: &[u64]) -> KWorld {
    let mut w = KWorld::two();
    for c in top_first {
        let id = w.create_card(
            cid(*c),
            P1,
            KWorld::req_at(ZoneKind::Library, Position::Bottom),
        );
        if *c == CARD_A || *c == CARD_B {
            w.k.add_known(&w.zones, id, P2).expect("add");
        }
    }
    w
}

/// World 1: A on top. World 2: the unknown X on top. Same cards, same KnownTo for P2.
fn draw_twins() -> (KWorld, KWorld) {
    let w1 = draw_twin(&[CARD_A, CARD_X, CARD_B, CARD_Y]);
    let w2 = draw_twin(&[CARD_X, CARD_A, CARD_B, CARD_Y]);
    assert_eq!(
        w1.view(P2),
        w2.view(P2),
        "precondition: P2 cannot tell them apart"
    );
    assert_eq!(
        lib_known(&w1.view(P2), P1),
        set(&[cid(CARD_A), cid(CARD_B)])
    );
    (w1, w2)
}

/// REQ-CORE-004, CR 401.2, 402.3 (hidden-source identity rule): the same unrevealed draw in
/// two worlds that differ only in the hidden top card gives P2 equal views and equal positions;
/// P2 can no longer identify A or B anywhere. P1 (who sees the drawn card) has different views.
#[test]
fn twin_draw_unobserved_views_equal_and_candidates_forgotten() {
    let (mut w1, mut w2) = draw_twins();
    let top1 = w1.library(P1)[0];
    let top2 = w2.library(P1)[0];
    assert_eq!(top1, top2, "same object id, different card");
    let m1 = w1.mv_to(top1, ZoneKind::Hand);
    let m2 = w2.mv_to(top2, ZoneKind::Hand);
    assert_eq!(m1.new, m2.new);
    assert_eq!(w1.view(P2), w2.view(P2), "no leak of which card was drawn");
    assert_eq!(w1.positions(P2, P1), w2.positions(P2, P1));
    for w in [&w1, &w2] {
        let v = w.view(P2);
        assert!(lib_known(&v, P1).is_empty(), "library candidates forgotten");
        assert!(
            opp_hand_known(&v, P1).is_empty(),
            "drawn card not identified"
        );
        for id in w.zones.all_object_ids() {
            assert!(!w.can(P2, id), "P2 identifies nothing: {:?}", w.rec(id));
            assert!(!w.known_to(id).contains(&P2));
        }
    }
    // control: the drawer sees the drawn card in their own hand
    assert_ne!(w1.view(P1), w2.view(P1));
    assert!(w1.can(P1, m1.new));
}

/// REQ-CORE-004 (hidden-source identity rule (i)): when P2 watched the draw
/// (`on_move_observed` with P2), P2 keeps its knowledge, so the twins' views differ for P2 and
/// P2 identifies the drawn A.
#[test]
fn twin_draw_observed_views_differ() {
    let (mut w1, mut w2) = draw_twins();
    let top = w1.library(P1)[0];
    let m1 = w1.mv_obs(top, MoveRequest::new(ZoneKind::Hand), &[P2]);
    w2.mv_obs(top, MoveRequest::new(ZoneKind::Hand), &[P2]);
    assert_ne!(w1.view(P2), w2.view(P2));
    assert!(w1.can(P2, m1.new));
    assert_eq!(opp_hand_known(&w1.view(P2), P1), set(&[cid(CARD_A)]));
    assert_eq!(lib_known(&w1.view(P2), P1), set(&[cid(CARD_B)]));
    assert!(opp_hand_known(&w2.view(P2), P1).is_empty());
    assert_eq!(
        lib_known(&w2.view(P2), P1),
        set(&[cid(CARD_A), cid(CARD_B)])
    );
}

/// REQ-CORE-004, CR 400.2 (hidden-source identity rule (ii)): a draw to a public face-up zone
/// (library to graveyard) is seen by everyone; P2 identifies the card and keeps B.
#[test]
fn draw_to_public_destination_is_observed() {
    let (mut w1, _) = draw_twins();
    let top = w1.library(P1)[0];
    let moved = w1.mv_to(top, ZoneKind::Graveyard);
    assert!(w1.can(P2, moved.new));
    assert_eq!(
        find_view(&w1.view(P2), moved.new).expect("graveyard").card,
        Some(cid(CARD_A))
    );
    assert_eq!(lib_known(&w1.view(P2), P1), set(&[cid(CARD_B)]), "B kept");
}

/// REQ-CORE-004, CR 401.2 (hidden-source identity rule (i)): P2 holds a position entry on the
/// top card. A position entry alone does not make P2 an observer: an unobserved draw of the
/// top card makes P2 forget the drawn card, B and its positions. When the selected position is
/// public (a draw of the top card) the caller passes P2 in `observers`, and P2 keeps the drawn
/// card and B.
#[test]
fn draw_with_position_entry_needs_explicit_observer() {
    // observed: P2 passed in observers
    let (mut w1, _) = draw_twins();
    let top = w1.library(P1)[0];
    w1.k.set_position_known(&w1.zones, P2, P1, 0, top)
        .expect("pos");
    let moved = w1.mv_obs(top, MoveRequest::new(ZoneKind::Hand), &[P2]);
    assert!(w1.can(P2, moved.new));
    assert_eq!(opp_hand_known(&w1.view(P2), P1), set(&[cid(CARD_A)]));
    assert_eq!(lib_known(&w1.view(P2), P1), set(&[cid(CARD_B)]));
    // unobserved: the same draw with no observers, through on_move and on_move_observed(&[])
    for plain in [true, false] {
        let (mut w, _) = draw_twins();
        let top = w.library(P1)[0];
        w.k.set_position_known(&w.zones, P2, P1, 0, top)
            .expect("pos");
        let moved = if plain {
            w.mv_to(top, ZoneKind::Hand)
        } else {
            w.mv_obs(top, MoveRequest::new(ZoneKind::Hand), &[])
        };
        assert!(!w.can(P2, moved.new), "plain {plain}");
        let v = w.view(P2);
        assert!(opp_hand_known(&v, P1).is_empty(), "plain {plain}");
        assert!(lib_known(&v, P1).is_empty(), "plain {plain}");
        assert!(w.positions(P2, P1).is_empty(), "plain {plain}");
    }
}

/// REQ-CORE-004, CR 401.2 (hidden-source identity rule): an unobserved draw drops all of the
/// non-observer's position entries for that library; an observer's other entries are kept.
#[test]
fn unobserved_draw_drops_all_positions_of_that_library() {
    let (mut w, _) = draw_twins();
    let lib = w.library(P1);
    w.k.set_position_known(&w.zones, P2, P1, 2, lib[2])
        .expect("pos");
    let mut obs = w.clone();
    w.mv_to(lib[0], ZoneKind::Hand);
    assert!(w.positions(P2, P1).is_empty());
    assert!(!w.can(P2, lib[2]), "B forgotten too");
    // observed: the entry for lib[2] is now stale (it moved up to index 1), but P2 still knows
    // the card
    obs.mv_obs(lib[0], MoveRequest::new(ZoneKind::Hand), &[P2]);
    assert!(obs.can(P2, lib[2]));
    assert_eq!(obs.library(P1)[1], lib[2]);
}

const CARD_Z: u64 = 22;

/// The undisclosed-selection setup (PR #174 review findings): P1's library, top first, is
/// X, A, B, Y, Z (fixed synthetic cards, ids in that order). P2 knows the position of A
/// (index 1) and knows B as a member of the set; X, Y and Z are unknown to P2. With three
/// players, P3 knows nothing. Returns the world and the library ids, top first.
fn selection_world(players: &[PlayerId]) -> (KWorld, Vec<ObjectId>) {
    let (w, lib, _, _) = selection_world_at(players, false);
    (w, lib)
}

/// The undisclosed-selection setup with A on top (`a_top`: A, X, B, Y, Z; P2 knows A at index
/// 0) or second (X, A, B, Y, Z; P2 knows A at index 1). P2 knows B as a member of the set.
/// Returns the world, the library ids top first, and the ids of A and X.
fn selection_world_at(
    players: &[PlayerId],
    a_top: bool,
) -> (KWorld, Vec<ObjectId>, ObjectId, ObjectId) {
    let mut w = KWorld::with_players(players);
    let order = if a_top {
        [CARD_A, CARD_X, CARD_B, CARD_Y, CARD_Z]
    } else {
        [CARD_X, CARD_A, CARD_B, CARD_Y, CARD_Z]
    };
    for c in order {
        w.create_card(
            cid(c),
            P1,
            KWorld::req_at(ZoneKind::Library, Position::Bottom),
        );
    }
    let lib = w.library(P1);
    let (ai, xi) = if a_top { (0, 1) } else { (1, 0) };
    assert_eq!(w.card(lib[ai]), cid(CARD_A));
    assert_eq!(w.card(lib[xi]), cid(CARD_X));
    assert_eq!(w.card(lib[2]), cid(CARD_B));
    w.k.set_position_known(&w.zones, P2, P1, ai, lib[ai])
        .expect("pos");
    w.k.add_known(&w.zones, lib[2], P2).expect("add");
    assert_eq!(
        lib_known(&w.view(P2), P1),
        set(&[cid(CARD_A), cid(CARD_B)]),
        "precondition: P2 knows A (and its position) and B"
    );
    assert_eq!(w.positions(P2, P1), [(ai, lib[ai])].into_iter().collect());
    let (a, x) = (lib[ai], lib[xi]);
    (w, lib, a, x)
}

/// The destinations of a secret selection from P1's library: P1's hand, face-down exile and a
/// face-down permanent controlled by P1 (PR #174 review finding F1: library to face-down exile).
fn selection_destinations() -> [MoveRequest; 3] {
    [
        MoveRequest::new(ZoneKind::Hand),
        fd_req(ZoneKind::Exile),
        MoveRequest {
            to: ZoneKind::Battlefield,
            position: Position::Top,
            face_down: true,
            controller: Some(P1),
        },
    ]
}

/// REQ-CORE-004, CR 401.2, 402.3, 406.3, 708.5 (hidden-source identity rule, design decision
/// of the brief; PR #174 review findings, including F1 4625229503): an UNDISCLOSED SELECTION.
/// P1 secretly takes a library card (`on_move`, no observers) into its hand, into face-down
/// exile, or onto the battlefield face down under its control: world 1 takes A, whose position
/// P2 knows (index 0 or index 1); world 2 takes the unknown X with the same request. Knowing
/// A's position does not tell P2 which position was selected, so P2 is not an observer in
/// either world: P2's whole view and `position_known` are equal across the worlds, P2 can
/// identify neither A nor B anywhere (in particular not the new exile or battlefield object),
/// and has no position entries left. Two- and three-player variants (P3 knows nothing; its
/// views are equal too).
#[test]
fn undisclosed_selection_twin_no_leak() {
    for players in [vec![P1, P2], vec![P1, P2, P3]] {
        for a_top in [false, true] {
            for req in selection_destinations() {
                let ctx = format!("{players:?} a_top {a_top} {req:?}");
                let (mut w1, lib, a, x) = selection_world_at(&players, a_top);
                let (mut w2, lib2, _, _) = selection_world_at(&players, a_top);
                assert_eq!(lib, lib2, "identical public structure");
                for p in &players {
                    assert_eq!(w1.view(*p), w2.view(*p), "precondition {p:?}");
                }
                let m1 = w1.mv(a, req);
                let m2 = w2.mv(x, req);
                assert_eq!(m1.new, m2.new, "same new id");
                assert_eq!((m1.from, m1.to), (m2.from, m2.to));
                assert_eq!(w1.card(m1.new), cid(CARD_A));
                assert_eq!(w2.card(m2.new), cid(CARD_X));
                assert_eq!(
                    w1.view(P2),
                    w2.view(P2),
                    "{ctx}: P2 cannot tell which card was taken"
                );
                assert_eq!(w1.positions(P2, P1), w2.positions(P2, P1));
                assert!(w1.positions(P2, P1).is_empty(), "{ctx}");
                for w in [&w1, &w2] {
                    let v = w.view(P2);
                    assert!(lib_known(&v, P1).is_empty(), "{ctx}: A and B forgotten");
                    assert!(opp_hand_known(&v, P1).is_empty(), "{ctx}: hand unknown");
                    assert!(!view_cards(&v).contains(&cid(CARD_A)), "{ctx}");
                    assert!(!view_cards(&v).contains(&cid(CARD_B)), "{ctx}");
                    assert!(!w.can(P2, m1.new), "{ctx}: new object unidentified");
                    if req.to != ZoneKind::Hand {
                        let ov = find_view(&v, m1.new).expect("public destination");
                        assert_eq!(ov.card, None, "{ctx}");
                        assert!(ov.face_down, "{ctx}");
                    }
                    for id in w.zones.all_object_ids() {
                        assert!(!w.can(P2, id), "{ctx}: P2 identifies {:?}", w.rec(id));
                        assert!(!w.known_to(id).contains(&P2), "{ctx}");
                    }
                }
                if players.contains(&P3) {
                    assert_eq!(w1.view(P3), w2.view(P3), "{ctx}: P3 knows nothing");
                    assert_eq!(w1.positions(P3, P1), w2.positions(P3, P1));
                }
                if req.to != ZoneKind::Exile {
                    // control: the selecting player sees which card arrived in their hand
                    // (CR 402.3) or which face-down permanent they control (CR 708.5)
                    assert_ne!(w1.view(P1), w2.view(P1), "{ctx}");
                    assert!(w1.can(P1, m1.new), "{ctx}");
                    assert!(w2.can(P1, m2.new), "{ctx}");
                }
            }
        }
    }
}

/// REQ-CORE-004, CR 406.3 (PR #174 review finding F1 4625229503, control): the face-down exile
/// selection with P2 passed in `observers` in both worlds: P2 identifies the exile object in
/// world 1 (it is A, which P2 knew) and not in world 2 (it is X); P2's views differ.
#[test]
fn undisclosed_selection_to_face_down_exile_observed_control() {
    for a_top in [false, true] {
        let (mut w1, _, a, _) = selection_world_at(&[P1, P2], a_top);
        let (mut w2, _, _, x) = selection_world_at(&[P1, P2], a_top);
        let m1 = w1.mv_obs(a, fd_req(ZoneKind::Exile), &[P2]);
        let m2 = w2.mv_obs(x, fd_req(ZoneKind::Exile), &[P2]);
        assert_eq!(m1.new, m2.new);
        assert!(w1.can(P2, m1.new), "a_top {a_top}: observer identifies A");
        assert!(!w2.can(P2, m2.new), "a_top {a_top}: X was never known");
        assert_eq!(
            find_view(&w1.view(P2), m1.new).expect("exile").card,
            Some(cid(CARD_A))
        );
        assert_ne!(w1.view(P2), w2.view(P2), "a_top {a_top}");
    }
}

/// REQ-CORE-004, CR 401.2, 402.3 (hidden-source identity rule (i)): controls for the
/// undisclosed selection. (1) The same pair of worlds with P2 passed in `observers` gives P2
/// different views: an observer sees which card was taken. (2) A draw of the top card where P2
/// knows the top card's position: with observers [P2] P2 identifies the drawn card; with no
/// observers P2 forgets it (position knowledge alone is not observation). (3) The hand to
/// library direction is unchanged: the hand's owner observes (iii).
#[test]
fn undisclosed_selection_controls() {
    // (1) observed selection
    for players in [vec![P1, P2], vec![P1, P2, P3]] {
        let (mut w1, lib) = selection_world(&players);
        let (mut w2, _) = selection_world(&players);
        let req = MoveRequest::new(ZoneKind::Hand);
        let m1 = w1.mv_obs(lib[1], req, &[P2]);
        w2.mv_obs(lib[0], req, &[P2]);
        assert_ne!(
            w1.view(P2),
            w2.view(P2),
            "{players:?}: the observer can tell"
        );
        assert!(w1.can(P2, m1.new));
        assert_eq!(opp_hand_known(&w1.view(P2), P1), set(&[cid(CARD_A)]));
        assert_eq!(lib_known(&w1.view(P2), P1), set(&[cid(CARD_B)]));
        assert!(opp_hand_known(&w2.view(P2), P1).is_empty());
        assert_eq!(
            lib_known(&w2.view(P2), P1),
            set(&[cid(CARD_A), cid(CARD_B)])
        );
        if players.contains(&P3) {
            assert_eq!(w1.view(P3), w2.view(P3), "P3 did not observe");
        }
    }
    // (2) top-card draw where P2 knows the top card's position
    for observers in [vec![P2], vec![]] {
        let (mut w, lib) = selection_world(&[P1, P2]);
        w.k.set_position_known(&w.zones, P2, P1, 0, lib[0])
            .expect("pos");
        assert!(w.can(P2, lib[0]));
        let moved = w.mv_obs(lib[0], MoveRequest::new(ZoneKind::Hand), &observers);
        let watched = observers.contains(&P2);
        assert_eq!(w.can(P2, moved.new), watched, "{observers:?}");
        let expected_hand = if watched {
            set(&[cid(CARD_X)])
        } else {
            set(&[])
        };
        assert_eq!(opp_hand_known(&w.view(P2), P1), expected_hand);
        assert_eq!(w.can(P2, lib[1]), watched, "A kept only by an observer");
        assert_eq!(w.can(P2, lib[2]), watched, "B kept only by an observer");
        if !watched {
            assert!(w.positions(P2, P1).is_empty());
        }
    }
    // (3) hand to library: the hand's owner observes (iii); P2 does not
    let (mut w, lib) = selection_world(&[P1, P2]);
    let h = w.mv(lib[3], MoveRequest::new(ZoneKind::Hand)); // Y, unobserved by P2
    let back = w.mv(h.new, KWorld::req_at(ZoneKind::Library, Position::Top));
    assert!(w.can(P1, back.new), "hand owner keeps the card (iii)");
    assert!(w.known_to(back.new).contains(&P1));
    assert!(!w.can(P2, back.new));
}

/// Hand of P1: A, B, X (fixed cards, ids in that order); P2 knows A and B.
fn hand_twin() -> (KWorld, Vec<ObjectId>) {
    let mut w = KWorld::two();
    w.create_card(cid(CARD_Y), P1, MoveRequest::new(ZoneKind::Library));
    let ids: Vec<ObjectId> = [CARD_A, CARD_B, CARD_X]
        .iter()
        .map(|c| w.create_card(cid(*c), P1, MoveRequest::new(ZoneKind::Hand)))
        .collect();
    w.k.add_known(&w.zones, ids[0], P2).expect("add");
    w.k.add_known(&w.zones, ids[1], P2).expect("add");
    (w, ids)
}

/// REQ-CORE-004, CR 402.3 (hidden-source identity rule): P1 puts a card from hand on top of
/// the library unobserved; world 1 moves A, world 2 moves the unknown X. P2's views are equal
/// and both known hand cards are forgotten. Controls: observed by P2 the views differ; the
/// hand owner P1 keeps knowledge of the moved card (iii).
#[test]
fn twin_hand_to_library_unobserved() {
    let (mut w1, ids) = hand_twin();
    let (mut w2, _) = hand_twin();
    let top = KWorld::req_at(ZoneKind::Library, Position::Top);
    let m1 = w1.mv(ids[0], top);
    let m2 = w2.mv(ids[2], top);
    assert_eq!(m1.new, m2.new);
    assert_eq!(w1.view(P2), w2.view(P2));
    for w in [&w1, &w2] {
        let v = w.view(P2);
        assert!(
            opp_hand_known(&v, P1).is_empty(),
            "both hand cards forgotten"
        );
        assert!(lib_known(&v, P1).is_empty());
        assert!(!w.can(P2, ids[1]));
    }
    // the hand owner keeps knowledge of the card put on the library
    assert!(w1.can(P1, m1.new));
    assert_eq!(lib_known(&w1.view(P1), P1), set(&[cid(CARD_A)]));
    assert_eq!(lib_known(&w2.view(P1), P1), set(&[cid(CARD_X)]));
    // control: observed by P2
    let (mut o1, _) = hand_twin();
    let (mut o2, _) = hand_twin();
    o1.mv_obs(ids[0], top, &[P2]);
    o2.mv_obs(ids[2], top, &[P2]);
    assert_ne!(o1.view(P2), o2.view(P2));
    assert_eq!(lib_known(&o1.view(P2), P1), set(&[cid(CARD_A)]));
    assert_eq!(opp_hand_known(&o1.view(P2), P1), set(&[cid(CARD_B)]));
    assert!(lib_known(&o2.view(P2), P1).is_empty());
    assert_eq!(
        opp_hand_known(&o2.view(P2), P1),
        set(&[cid(CARD_A), cid(CARD_B)])
    );
}

/// REQ-CORE-004, CR 406.3, 708.5 (hidden-source identity rule (ii)): library to face-down
/// exile: nobody can see the new object, so every non-observer forgets the moved card and the
/// other library cards; an observer keeps them.
#[test]
fn library_to_face_down_exile_forgotten_unless_observed() {
    for observers in [vec![], vec![P3]] {
        let mut w = KWorld::three();
        let l = w.create(ZoneKind::Library, P1);
        let m = w.create(ZoneKind::Library, P1);
        for p in [P2, P3] {
            w.k.add_known(&w.zones, l, p).expect("add");
            w.k.add_known(&w.zones, m, p).expect("add");
        }
        let moved = w.mv_obs(l, fd_req(ZoneKind::Exile), &observers);
        let p3_keeps = observers.contains(&P3);
        assert!(!w.can(P2, moved.new), "{observers:?}");
        assert!(!w.can(P2, m));
        assert_eq!(w.can(P3, moved.new), p3_keeps, "{observers:?}");
        assert_eq!(w.can(P3, m), p3_keeps);
        assert!(!w.can(P1, moved.new), "owner did not see it either");
    }
}

/// REQ-CORE-004, CR 708.5 (hidden-source identity rule (ii)): library to a face-down
/// permanent controlled by P2: P2 can see it and keeps its knowledge; P3 forgets. Both
/// directions: with P3 as controller P3 keeps and P2 forgets.
#[test]
fn library_to_face_down_battlefield_controller_keeps() {
    for (ctl, other) in [(P2, P3), (P3, P2)] {
        let mut w = KWorld::three();
        let l = w.create(ZoneKind::Library, P1);
        let m = w.create(ZoneKind::Library, P1);
        for p in [P2, P3] {
            w.k.add_known(&w.zones, l, p).expect("add");
            w.k.add_known(&w.zones, m, p).expect("add");
        }
        let req = MoveRequest {
            to: ZoneKind::Battlefield,
            position: Position::Top,
            face_down: true,
            controller: Some(ctl),
        };
        let moved = w.mv(l, req);
        assert!(w.can(ctl, moved.new), "CR 708.5 controller {ctl:?}");
        assert!(w.known_to(moved.new).contains(&ctl), "memory kept");
        assert!(w.can(ctl, m), "controller observed: keeps the library card");
        assert!(!w.can(other, moved.new), "{other:?} forgets");
        assert!(!w.known_to(moved.new).contains(&other));
        assert!(!w.can(other, m));
    }
}

/// REQ-CORE-004, CR 401.2, 701.24a, SRS 2.4: same-zone moves (from == to) skip step 0 (library
/// and hand repositioning keep everyone's identity knowledge), and a shuffle never touches
/// KnownTo.
#[test]
fn same_zone_reposition_and_shuffle_keep_identity_knowledge() {
    let (mut w, _) = draw_twins();
    let lib = w.library(P1);
    let before: Vec<_> = lib.iter().map(|id| w.known_to(*id)).collect();
    let moved = w.mv(lib[0], KWorld::req_at(ZoneKind::Library, Position::Bottom));
    assert_eq!(moved.old, moved.new);
    let after: Vec<_> = lib.iter().map(|id| w.known_to(*id)).collect();
    assert_eq!(before, after);
    assert_eq!(lib_known(&w.view(P2), P1), set(&[cid(CARD_A), cid(CARD_B)]));
    w.k.shuffle_library(&w.zones, P1, &[]).expect("shuffle");
    let shuffled: Vec<_> = lib.iter().map(|id| w.known_to(*id)).collect();
    assert_eq!(before, shuffled, "SRS 2.4: shuffle keeps set knowledge");
    // hand reposition
    let (mut h, ids) = hand_twin();
    h.mv(ids[0], MoveRequest::new(ZoneKind::Hand));
    assert!(h.can(P2, ids[0]));
    assert!(h.can(P2, ids[1]));
}

/// REQ-CORE-004, R12: `on_move_observed` errors: an unknown moved object is `UnknownObject`
/// (checked first), an unknown observer is `UnknownPlayer` (the first one); nothing changes.
/// `on_move` and `on_move_observed` with no observers give equal state.
#[test]
fn on_move_observed_errors_and_equivalence() {
    let (mut w, _) = draw_twins();
    let top = w.library(P1)[0];
    let before = w.rec(top);
    let moved = w.raw_mv(top, MoveRequest::new(ZoneKind::Hand));
    let snapshot = w.k.clone();
    assert_eq!(
        w.k.on_move_observed(&w.zones, &before, &moved, &[P2, NOBODY, pid(77)]),
        Err(KnowledgeError::UnknownPlayer(NOBODY))
    );
    assert_eq!(
        w.k.on_move_observed(&w.zones, &before, &moved, &[pid(u64::MAX)]),
        Err(KnowledgeError::UnknownPlayer(pid(u64::MAX)))
    );
    let fake = fineprint_core::zones::Moved {
        new: oid(987_654),
        ..moved
    };
    assert_eq!(
        w.k.on_move_observed(&w.zones, &before, &fake, &[]),
        Err(KnowledgeError::UnknownObject(oid(987_654)))
    );
    assert_eq!(
        w.k.on_move_observed(&w.zones, &before, &fake, &[NOBODY]),
        Err(KnowledgeError::UnknownObject(oid(987_654))),
        "UnknownObject first"
    );
    assert_eq!(
        w.k.on_move(&w.zones, &before, &fake),
        Err(KnowledgeError::UnknownObject(oid(987_654)))
    );
    assert_eq!(w.k, snapshot, "rejected calls change nothing");
    let mut a = w.clone();
    let mut b = w.clone();
    a.k.on_move(&a.zones, &before, &moved).expect("on_move");
    b.k.on_move_observed(&b.zones, &before, &moved, &[])
        .expect("on_move_observed");
    assert_eq!(a.k, b.k);
    assert_ne!(a.k, snapshot, "the draw did change the knowledge");
}

// ---------------------------------------------------------------------------------------------
// removal from a hidden zone: `Zones::send_outside` + `on_removed_from_hidden_zone`
// (PR #174 review finding F2 4625229511; the forgetting rule is a design decision of the brief)
// ---------------------------------------------------------------------------------------------

/// P1's library, top first: A, X, B, Y (fixed synthetic cards, ids in that order). P2 knows the
/// SET {A, B} (no positions); X and Y are unknown to P2. Returns the world and the library ids.
fn removal_world(players: &[PlayerId]) -> (KWorld, Vec<ObjectId>) {
    let mut w = KWorld::with_players(players);
    for c in [CARD_A, CARD_X, CARD_B, CARD_Y] {
        w.create_card(
            cid(c),
            P1,
            KWorld::req_at(ZoneKind::Library, Position::Bottom),
        );
    }
    let lib = w.library(P1);
    w.k.add_known(&w.zones, lib[0], P2).expect("add");
    w.k.add_known(&w.zones, lib[2], P2).expect("add");
    assert_eq!(
        lib_known(&w.view(P2), P1),
        set(&[cid(CARD_A), cid(CARD_B)]),
        "precondition: P2 knows the set {{A, B}}"
    );
    assert!(w.positions(P2, P1).is_empty());
    (w, lib)
}

/// REQ-CORE-004, CR 401.2, 401.3 (F2, hidden-source identity rule applied to a removal): P2
/// knows the set {A, B} of P1's library. World 1 removes A from the game zones
/// (`send_outside` + hook, no observers), world 2 removes the unknown X. P2 cannot tell which
/// card left: P2's whole view and `position_known` are equal across the worlds and P2's
/// library `known_cards` is empty in both. Three-player variant: P3, who knows nothing, has
/// equal views too.
#[test]
fn removal_twin_unobserved_no_leak() {
    for players in [vec![P1, P2], vec![P1, P2, P3]] {
        let (mut w1, lib) = removal_world(&players);
        let (mut w2, _) = removal_world(&players);
        let (b1, c1) = w1.send_out(lib[0], &[]); // A
        let (b2, c2) = w2.send_out(lib[1], &[]); // X
        assert_eq!((c1, c2), (cid(CARD_A), cid(CARD_X)));
        assert_eq!(b1.zone, b2.zone);
        assert_eq!(w1.library(P1).len(), w2.library(P1).len(), "CR 401.3 count");
        assert_eq!(
            w1.view(P2),
            w2.view(P2),
            "{players:?}: no leak of which card left"
        );
        assert_eq!(w1.positions(P2, P1), w2.positions(P2, P1));
        for w in [&w1, &w2] {
            assert!(
                lib_known(&w.view(P2), P1).is_empty(),
                "{players:?}: forgotten"
            );
            for id in w.library(P1) {
                assert!(!w.known_to(id).contains(&P2), "{players:?}");
                assert!(!w.can(P2, id));
            }
        }
        if players.contains(&P3) {
            assert_eq!(w1.view(P3), w2.view(P3));
            assert_eq!(w1.positions(P3, P1), w2.positions(P3, P1));
        }
    }
}

/// REQ-CORE-004, CR 401.2 (F2 control, observer (i)): the same twin removals with P2 passed in
/// `observers`: P2 keeps its knowledge, so its views differ (world 1 shows {B}, world 2 shows
/// {A, B}).
#[test]
fn removal_twin_observed_views_differ() {
    for players in [vec![P1, P2], vec![P1, P2, P3]] {
        let (mut w1, lib) = removal_world(&players);
        let (mut w2, _) = removal_world(&players);
        w1.send_out(lib[0], &[P2]);
        w2.send_out(lib[1], &[P2]);
        assert_ne!(w1.view(P2), w2.view(P2), "{players:?}");
        assert_eq!(lib_known(&w1.view(P2), P1), set(&[cid(CARD_B)]));
        assert_eq!(
            lib_known(&w2.view(P2), P1),
            set(&[cid(CARD_A), cid(CARD_B)])
        );
    }
}

/// P1's hand: A, B, X (fixed synthetic cards); P2 knows A and B (`add_known`).
fn removal_hand_world() -> (KWorld, Vec<ObjectId>) {
    let mut w = KWorld::two();
    w.create_card(cid(CARD_Y), P1, MoveRequest::new(ZoneKind::Library));
    let ids: Vec<ObjectId> = [CARD_A, CARD_B, CARD_X]
        .iter()
        .map(|c| w.create_card(cid(*c), P1, MoveRequest::new(ZoneKind::Hand)))
        .collect();
    w.k.add_known(&w.zones, ids[0], P2).expect("add");
    w.k.add_known(&w.zones, ids[1], P2).expect("add");
    assert_eq!(
        opp_hand_known(&w.view(P2), P1),
        set(&[cid(CARD_A), cid(CARD_B)])
    );
    (w, ids)
}

/// REQ-CORE-004, CR 402.3 (hidden-source identity rule applied to a removal from a hand): P2
/// knows A and B in P1's hand. Unobserved removal (world 1 removes A, world 2 the unknown X):
/// P2 forgets both, its views are equal across the twins. P1, the hand's owner, observes (iii)
/// and keeps its own entitlement (its own hand view shows every remaining card). With
/// `observers = [P2]`, P2 keeps its knowledge of the remaining card.
#[test]
fn removal_from_hand() {
    let (mut w1, ids) = removal_hand_world();
    let (mut w2, _) = removal_hand_world();
    w1.send_out(ids[0], &[]);
    w2.send_out(ids[2], &[]);
    assert_eq!(w1.view(P2), w2.view(P2), "no leak of which hand card left");
    for w in [&w1, &w2] {
        let v = w.view(P2);
        assert!(opp_hand_known(&v, P1).is_empty(), "both forgotten");
        assert!(!w.can(P2, ids[1]));
        assert!(!w.known_to(ids[1]).contains(&P2));
        // the hand owner keeps its own entitlement (CR 402.3)
        let hand = w.zones.contents(ZoneId::Hand(P1));
        assert_eq!(hand.len(), 2);
        for id in &hand {
            assert!(w.can(P1, *id));
            assert_eq!(
                find_view(&w.view(P1), *id).expect("own hand").card,
                Some(w.card(*id))
            );
        }
    }
    // control: the hand owner sees which card left its hand
    assert_ne!(w1.view(P1), w2.view(P1));
    // observed by P2: P2 keeps B
    let (mut o, ids) = removal_hand_world();
    o.send_out(ids[0], &[P2]);
    assert!(o.can(P2, ids[1]));
    assert!(o.known_to(ids[1]).contains(&P2));
    assert_eq!(opp_hand_known(&o.view(P2), P1), set(&[cid(CARD_B)]));
}

/// REQ-CORE-004, CR 401.2 (removal and position knowledge): P2 knows the positions of indexes
/// 0 and 2 of P1's library; P1 knows indexes 0 and 1. Removing index 1 watched by P1 and P2
/// keeps the entries at index 0, drops P2's shifted entry (old index 2) and P1's entry on the
/// removed object. Without observers every entry of P1 and P2 for that library is gone. P2
/// keeps identity knowledge of both cards it knew only as an observer.
#[test]
fn removal_and_position_entries() {
    for observers in [vec![P1, P2], vec![]] {
        let mut w = KWorld::two();
        let lib: Vec<ObjectId> = (0..4)
            .map(|_| w.create_bottom(ZoneKind::Library, P1))
            .collect();
        for i in [0, 2] {
            w.k.set_position_known(&w.zones, P2, P1, i, lib[i])
                .expect("pos");
        }
        for i in [0, 1] {
            w.k.set_position_known(&w.zones, P1, P1, i, lib[i])
                .expect("pos");
        }
        w.send_out(lib[1], &observers);
        assert_eq!(w.library(P1), vec![lib[0], lib[2], lib[3]]);
        let watched = !observers.is_empty();
        if watched {
            let only_top = [(0, lib[0])].into_iter().collect();
            assert_eq!(w.positions(P2, P1), only_top, "shifted entry dropped");
            assert_eq!(w.positions(P1, P1), only_top, "removed object's entry gone");
        } else {
            assert!(w.positions(P2, P1).is_empty());
            assert!(w.positions(P1, P1).is_empty());
        }
        for id in [lib[0], lib[2]] {
            assert_eq!(w.can(P2, id), watched, "{observers:?}");
        }
        for (_, id) in w.positions(P1, P1).into_iter().chain(w.positions(P2, P1)) {
            assert_ne!(id, lib[1], "the removed object is never reported");
        }
    }
}

/// REQ-CORE-004, R12 (`on_removed_from_hidden_zone` errors and no-ops): a removal from a public
/// zone (battlefield, graveyard, exile; CR 400.2) is `Ok` and changes nothing; a source library
/// or hand of an unknown player is `UnknownPlayer(that player)`; an unknown observer is
/// `UnknownPlayer` of the first unknown observer; the owner error comes before the observer
/// error; every rejected call changes nothing.
#[test]
fn removal_hook_errors_and_public_no_op() {
    // public sources: no-op
    for (kind, fd) in [
        (ZoneKind::Battlefield, false),
        (ZoneKind::Battlefield, true),
        (ZoneKind::Graveyard, false),
        (ZoneKind::Exile, false),
        (ZoneKind::Exile, true),
    ] {
        let (mut w, lib) = removal_world(&[P1, P2, P3]);
        w.k.set_position_known(&w.zones, P2, P1, 1, lib[1])
            .expect("pos");
        let id = if fd {
            w.create_fd(kind, P1)
        } else {
            w.create(kind, P1)
        };
        w.k.add_known(&w.zones, id, P3).expect("add");
        let before = w.rec(id);
        w.zones
            .send_outside(id, fineprint_core::zones::OutsidePool::Sideboard)
            .expect("out");
        let snapshot = w.k.clone();
        for observers in [vec![], vec![P2]] {
            assert_eq!(
                w.k.on_removed_from_hidden_zone(&w.zones, &before, &observers),
                Ok(()),
                "{kind:?} fd {fd}"
            );
            assert_eq!(
                w.k, snapshot,
                "{kind:?} fd {fd}: public removal changes nothing"
            );
        }
        // the position entry on X also made P2 know X (CR 401.2)
        assert_eq!(
            lib_known(&w.view(P2), P1),
            set(&[cid(CARD_A), cid(CARD_X), cid(CARD_B)])
        );
        assert_eq!(w.positions(P2, P1), [(1, lib[1])].into_iter().collect());
    }
    // errors
    let (mut w, lib) = removal_world(&[P1, P2]);
    w.k.set_position_known(&w.zones, P2, P1, 1, lib[1])
        .expect("pos");
    let before = w.rec(lib[0]);
    w.zones
        .send_outside(lib[0], fineprint_core::zones::OutsidePool::Sideboard)
        .expect("out");
    let snapshot = w.k.clone();
    let with_zone = |zone: ZoneId| ObjectRecord { zone, ..before };
    let cases: Vec<(ObjectRecord, Vec<PlayerId>, PlayerId)> = vec![
        (with_zone(ZoneId::Library(NOBODY)), vec![], NOBODY),
        (with_zone(ZoneId::Hand(NOBODY)), vec![], NOBODY),
        (with_zone(ZoneId::Library(pid(55))), vec![NOBODY], pid(55)),
        (with_zone(ZoneId::Hand(pid(55))), vec![P2, NOBODY], pid(55)),
        (before, vec![P2, NOBODY, pid(77)], NOBODY),
        (before, vec![pid(u64::MAX)], pid(u64::MAX)),
        (with_zone(ZoneId::Hand(P2)), vec![pid(77), NOBODY], pid(77)),
    ];
    for (b, observers, bad) in cases {
        assert_eq!(
            w.k.on_removed_from_hidden_zone(&w.zones, &b, &observers),
            Err(KnowledgeError::UnknownPlayer(bad)),
            "{b:?} {observers:?}"
        );
        assert_eq!(w.k, snapshot, "rejected call changes nothing");
    }
    // the accepted call afterwards does change the knowledge (P2 did not watch)
    w.k.on_removed_from_hidden_zone(&w.zones, &before, &[])
        .expect("hook");
    assert_ne!(w.k, snapshot);
    assert!(lib_known(&w.view(P2), P1).is_empty());
}
