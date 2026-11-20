// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Leak tests and reveal rules for face-down objects (REQ-OBJ-003, REQ-CORE-004; CR 708.5,
//! 708.9, 708.4, 708.2; acceptance criterion "Redaction: opponents see only the profile (leak
//! test)"). The opponent's whole redacted view is searched, not one field (R12). Black-box:
//! written from the work-item brief only.
//!
//! A phased-out face-down permanent (CR 708.5) is not tested here: the zone container and the
//! knowledge projection have no phasing input, so the existing API cannot express it.

#[path = "spec_layers_facedown_support.rs"]
mod support;

#[path = "spec_knowledge_support.rs"]
mod ksup;

use fineprint_base::ids::{CardId, ObjectId};
use fineprint_core::layers::{ContinuousEffects, evaluate};
use fineprint_core::layouts::{FaceDownProfile, must_reveal_on_leave};
use fineprint_core::objects::Characteristics;
use fineprint_core::zones::{MoveRequest, Position, ZoneId, ZoneKind};
use ksup::KWorld;

/// A card number no other object of these worlds uses; its decimal text is searched for in
/// the opponents' views.
const SECRET_CARD: u64 = 987_654_321;

fn profiles() -> [(FaceDownProfile, Characteristics); 2] {
    [
        (FaceDownProfile::default_2_2(), support::expected_default()),
        (FaceDownProfile::with_ward_2(), support::expected_ward()),
    ]
}

/// Creates the secret card face down in `kind` (battlefield or stack), owned and controlled by
/// `owner`, plus some face-up noise objects.
fn world_with_secret(
    players: &[fineprint_base::ids::PlayerId],
    kind: ZoneKind,
) -> (KWorld, ObjectId) {
    let mut w = KWorld::with_players(players);
    let owner = players[0];
    w.create(ZoneKind::Battlefield, players[1]);
    w.create(ZoneKind::Graveyard, owner);
    let req = MoveRequest {
        to: kind,
        position: Position::Top,
        face_down: true,
        controller: Some(owner),
    };
    let id = w.create_card(ksup::cid(SECRET_CARD), owner, req);
    w.create(ZoneKind::Hand, owner);
    (w, id)
}

/// Asserts the redaction for every player: the controller identifies the card, every other
/// player sees `face_down: true, card: None` and the card id appears nowhere in their view.
fn assert_redacted(w: &KWorld, id: ObjectId, controller: fineprint_base::ids::PlayerId) {
    let secret = ksup::cid(SECRET_CARD);
    for viewer in w.players() {
        let view = w.k.project(&w.zones, viewer).expect("project");
        let ov = ksup::find_view(&view, id).expect("the object is visible as an object");
        assert!(ov.face_down, "viewer {viewer:?}");
        if viewer == controller {
            assert_eq!(ov.card, Some(secret), "CR 708.5: the controller may look");
        } else {
            assert_eq!(ov.card, None, "viewer {viewer:?}");
            assert_eq!(ov.owner, controller);
            let debug = format!("{view:?}");
            assert!(
                !debug.contains(&SECRET_CARD.to_string()),
                "viewer {viewer:?}: card id leaked in {debug}"
            );
            assert!(!ksup::view_cards(&view).contains(&secret));
            assert!(
                !w.k.can_identify(&w.zones, viewer, id).unwrap(),
                "viewer {viewer:?}"
            );
        }
    }
}

/// The evaluated characteristics of a face-down object with the secret face-up base: exactly
/// the profile, and none of the face-up markers anywhere in their Debug text.
fn assert_evaluated_profile(on_battlefield: bool) {
    for (p, want) in profiles() {
        let mut env = support::TestEnv::new();
        let id = support::oid(1);
        env.put_full(
            id,
            support::secret_face_up(),
            Some(support::P1),
            on_battlefield,
        );
        let mut effects = ContinuousEffects::new();
        effects
            .register(support::eid(1), support::fd_eff(&p, 3, id))
            .unwrap();
        let ev = evaluate(&effects, &env, id).unwrap();
        assert_eq!(support::norm(&ev.characteristics), want);
        support::assert_no_secret(&format!("{:?}", ev.characteristics), "evaluated");
        support::assert_no_secret(&format!("{ev:?}"), "evaluated whole");
        // Control: without the effect the markers are there (the search is meaningful).
        let face_up = evaluate(&ContinuousEffects::new(), &env, id).unwrap();
        let debug = format!("{:?}", face_up.characteristics);
        for m in support::secret_markers() {
            assert!(debug.contains(&m), "control marker {m:?} in {debug}");
        }
    }
}

/// REQ-OBJ-003, REQ-CORE-004, CR 708.5, 708.2 (leak test): a face-down permanent controlled by
/// P1 is `face_down: true, card: None` for every opponent, its card id is nowhere in their
/// whole views, and its controller identifies it; the evaluated characteristics anyone may see
/// are the profile with no trace of the face-up card.
#[test]
fn req_obj_003_req_core_004_face_down_permanent_leak_test() {
    for players in [vec![ksup::P1, ksup::P2], vec![ksup::P1, ksup::P2, ksup::P3]] {
        let (w, id) = world_with_secret(&players, ZoneKind::Battlefield);
        assert_redacted(&w, id, ksup::P1);
    }
    assert_evaluated_profile(true);
}

/// REQ-OBJ-003, REQ-CORE-004, CR 708.4, 708.5 (leak test): the same for a face-down spell on
/// the stack.
#[test]
fn req_obj_003_req_core_004_face_down_spell_leak_test() {
    for players in [vec![ksup::P1, ksup::P2], vec![ksup::P1, ksup::P2, ksup::P3]] {
        let (w, id) = world_with_secret(&players, ZoneKind::Stack);
        assert_redacted(&w, id, ksup::P1);
    }
    assert_evaluated_profile(false);
}

/// REQ-OBJ-003, REQ-CORE-004, CR 708.5: a face-down permanent whose controller is not its owner
/// is identified by its controller only.
#[test]
fn req_obj_003_req_core_004_controller_not_owner_looks() {
    let mut w = KWorld::three();
    let req = MoveRequest {
        to: ZoneKind::Battlefield,
        position: Position::Top,
        face_down: true,
        controller: Some(ksup::P2),
    };
    let id = w.create_card(ksup::cid(SECRET_CARD), ksup::P1, req);
    for viewer in w.players() {
        let view = w.k.project(&w.zones, viewer).unwrap();
        let ov = ksup::find_view(&view, id).unwrap();
        if viewer == ksup::P2 {
            assert_eq!(ov.card, Some(ksup::cid(SECRET_CARD)));
        } else {
            assert_eq!(ov.card, None, "viewer {viewer:?}");
            assert!(!format!("{view:?}").contains(&SECRET_CARD.to_string()));
        }
    }
}

/// REQ-CORE-004, SRS 2.4 (existing behavior): a face-down card in exile is hidden from its
/// owner too.
#[test]
fn req_core_004_face_down_exile_hidden_from_owner() {
    let mut w = KWorld::two();
    let id = w.create_fd(ZoneKind::Exile, ksup::P1);
    for viewer in w.players() {
        assert!(
            !w.k.can_identify(&w.zones, viewer, id).unwrap(),
            "{viewer:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// reveal rules (CR 708.9)
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-003, CR 708.9: `must_reveal_on_leave` is true exactly for a face-down object moving
/// from the battlefield to another zone, or from the stack to a zone other than the
/// battlefield; same-zone moves and face-up objects are false.
#[test]
fn req_obj_003_708_9_must_reveal_truth_table() {
    let p1 = ksup::P1;
    let p2 = ksup::P2;
    let zones = [
        ZoneId::Battlefield,
        ZoneId::Stack,
        ZoneId::Hand(p1),
        ZoneId::Hand(p2),
        ZoneId::Graveyard(p1),
        ZoneId::Exile,
        ZoneId::Library(p1),
        ZoneId::Command,
    ];
    let mut trues = 0;
    for face_down in [false, true] {
        for from in zones {
            for to in zones {
                // Same-zone moves are false (the brief).
                let want = face_down
                    && from != to
                    && ((from == ZoneId::Battlefield && to != ZoneId::Battlefield)
                        || (from == ZoneId::Stack && to != ZoneId::Battlefield));
                assert_eq!(
                    must_reveal_on_leave(face_down, from, to),
                    want,
                    "face_down {face_down} {from:?} -> {to:?}"
                );
                if want {
                    trues += 1;
                }
            }
        }
    }
    // Battlefield -> 7 others, Stack -> 6 others (not Battlefield, not itself).
    assert_eq!(trues, 13);
    assert!(!must_reveal_on_leave(
        true,
        ZoneId::Battlefield,
        ZoneId::Battlefield
    ));
    assert!(!must_reveal_on_leave(true, ZoneId::Stack, ZoneId::Stack));
    assert!(!must_reveal_on_leave(
        true,
        ZoneId::Stack,
        ZoneId::Battlefield
    ));
    assert!(!must_reveal_on_leave(
        true,
        ZoneId::Exile,
        ZoneId::Graveyard(p1)
    ));
}

/// REQ-OBJ-003, REQ-CORE-004, CR 708.9: a face-down permanent that moves to the graveyard or to
/// exile (through the zone move and `Knowledge::on_move`) is known to every player afterwards,
/// as `must_reveal_on_leave` says.
#[test]
fn req_obj_003_708_9_leaving_the_battlefield_reveals_to_all() {
    for to in [
        ZoneKind::Graveyard,
        ZoneKind::Exile,
        ZoneKind::Hand,
        ZoneKind::Library,
    ] {
        for start in [ZoneKind::Battlefield, ZoneKind::Stack] {
            let (mut w, id) = world_with_secret(&[ksup::P1, ksup::P2, ksup::P3], start);
            let before = w.rec(id);
            let moved = w.mv(id, MoveRequest::new(to));
            assert!(must_reveal_on_leave(before.face_down, moved.from, moved.to));
            let card: CardId = w.card(moved.new);
            assert_eq!(card, ksup::cid(SECRET_CARD));
            for viewer in w.players() {
                let known = w.k.known_to(&w.zones, moved.new).unwrap();
                assert!(known.contains(&viewer), "{start:?} -> {to:?}: {viewer:?}");
            }
            if matches!(to, ZoneKind::Graveyard | ZoneKind::Exile) {
                for viewer in w.players() {
                    assert!(
                        w.k.can_identify(&w.zones, viewer, moved.new).unwrap(),
                        "{start:?} -> {to:?}: {viewer:?}"
                    );
                }
            }
        }
    }
}

/// REQ-OBJ-003, REQ-CORE-004, CR 708.9, 708.5: a face-down spell that resolves onto the
/// battlefield face down is not revealed: opponents still cannot identify it.
#[test]
fn req_obj_003_708_9_stack_to_battlefield_is_not_revealed() {
    let (mut w, id) = world_with_secret(&[ksup::P1, ksup::P2], ZoneKind::Stack);
    let before = w.rec(id);
    let req = MoveRequest {
        to: ZoneKind::Battlefield,
        position: Position::Top,
        face_down: true,
        controller: Some(ksup::P1),
    };
    let moved = w.mv(id, req);
    assert!(!must_reveal_on_leave(
        before.face_down,
        moved.from,
        moved.to
    ));
    assert_redacted(&w, moved.new, ksup::P1);
}

/// REQ-OBJ-003, REQ-CORE-004, CR 708.9: at the end of the game every face-down permanent and
/// spell is revealed to all players.
#[test]
fn req_obj_003_708_9_game_end_reveals_all() {
    let mut w = KWorld::three();
    let a = w.create_fd_ctl(ZoneKind::Battlefield, ksup::P1, ksup::P1);
    let b = w.create_fd_ctl(ZoneKind::Stack, ksup::P2, ksup::P2);
    let c = w.create_fd_ctl(ZoneKind::Battlefield, ksup::P3, ksup::P3);
    assert!(!w.k.can_identify(&w.zones, ksup::P2, a).unwrap());
    w.k.on_game_end(&w.zones);
    for id in [a, b, c] {
        for viewer in w.players() {
            assert!(
                w.k.can_identify(&w.zones, viewer, id).unwrap(),
                "{id:?} {viewer:?}"
            );
        }
    }
}

/// REQ-OBJ-003, REQ-CORE-004, CR 708.9: when a player leaves, the face-down permanents and
/// spells they own are revealed to all; other players' face-down objects stay hidden.
#[test]
fn req_obj_003_708_9_player_leaves_reveals_their_face_down_objects() {
    let mut w = KWorld::three();
    let mine_bf = w.create_fd_ctl(ZoneKind::Battlefield, ksup::P1, ksup::P1);
    let mine_stack = w.create_fd_ctl(ZoneKind::Stack, ksup::P1, ksup::P1);
    let theirs = w.create_fd_ctl(ZoneKind::Battlefield, ksup::P2, ksup::P2);
    w.k.on_player_leaves(&w.zones, ksup::P1).unwrap();
    for viewer in w.players() {
        assert!(
            w.k.can_identify(&w.zones, viewer, mine_bf).unwrap(),
            "{viewer:?}"
        );
        assert!(
            w.k.can_identify(&w.zones, viewer, mine_stack).unwrap(),
            "{viewer:?}"
        );
    }
    assert!(!w.k.can_identify(&w.zones, ksup::P1, theirs).unwrap());
    assert!(!w.k.can_identify(&w.zones, ksup::P3, theirs).unwrap());
    assert!(w.k.can_identify(&w.zones, ksup::P2, theirs).unwrap());
}
