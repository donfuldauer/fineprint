// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The redacted projection V(S, P) (REQ-CORE-004, REQ-ZONE-001; CR 400.2, 401.2, 401.3,
//! 402.3, 406.3, 708.5). Exact expected views for hand-built scenarios, determinism,
//! non-mutation, clone independence and canonical encoding. Black-box: written from the
//! work-item brief only.

#[path = "spec_knowledge_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::{CardId, ObjectId, PlayerId};
use fineprint_core::knowledge::{
    HandView, Knowledge, KnowledgeError, LibraryView, ObjectView, PlayerZonesView, ZonesView,
};
use fineprint_core::zones::{MoveRequest, ZoneKind};
use support::*;

fn ov(w: &KWorld, id: ObjectId, visible: bool) -> ObjectView {
    let r = w.rec(id);
    ObjectView {
        id,
        owner: r.owner,
        controller: r.controller,
        face_down: r.face_down,
        card: visible.then_some(r.card),
    }
}

fn lv(count: usize, cards: &[CardId]) -> LibraryView {
    LibraryView {
        count,
        known_cards: set(cards),
    }
}

/// The two-player scenario: every zone populated, a mix of face-down, revealed and granted.
struct Two {
    w: KWorld,
    l1a: ObjectId,
    l1b: ObjectId,
    h1: ObjectId,
    g1: ObjectId,
    l2: ObjectId,
    h2a: ObjectId,
    h2b: ObjectId,
    g2fd: ObjectId,
    bf1: ObjectId,
    bffd: ObjectId,
    bfsteal: ObjectId,
    s1fd: ObjectId,
    s2: ObjectId,
    exfd: ObjectId,
    exu: ObjectId,
    cm: ObjectId,
}

fn two() -> Two {
    let mut w = KWorld::two();
    let l1a = w.create_bottom(ZoneKind::Library, P1);
    let l1b = w.create_bottom(ZoneKind::Library, P1);
    let h1 = w.create(ZoneKind::Hand, P1);
    let g1 = w.create(ZoneKind::Graveyard, P1);
    let l2 = w.create(ZoneKind::Library, P2);
    let h2a = w.create(ZoneKind::Hand, P2);
    let h2b = w.create(ZoneKind::Hand, P2);
    let g2fd = w.create_fd(ZoneKind::Graveyard, P2);
    let bf1 = w.create(ZoneKind::Battlefield, P1);
    let bffd = w.create_fd(ZoneKind::Battlefield, P2);
    let bfsteal = w.create_req(
        P1,
        MoveRequest {
            controller: Some(P2),
            ..MoveRequest::new(ZoneKind::Battlefield)
        },
    );
    let s1fd = w.create_fd(ZoneKind::Stack, P1);
    let s2 = w.create(ZoneKind::Stack, P2);
    let exfd = w.create_fd(ZoneKind::Exile, P1);
    let exu = w.create(ZoneKind::Exile, P2);
    let cm = w.create(ZoneKind::Command, P1);
    w.k.add_known(&w.zones, l1a, P2).expect("add");
    w.k.reveal_to_all(&w.zones, h2b).expect("reveal");
    w.k.grant_look(&w.zones, exfd, P2).expect("grant");
    w.k.set_position_known(&w.zones, P1, P1, 1, l1b)
        .expect("pos");
    Two {
        w,
        l1a,
        l1b,
        h1,
        g1,
        l2,
        h2a,
        h2b,
        g2fd,
        bf1,
        bffd,
        bfsteal,
        s1fd,
        s2,
        exfd,
        exu,
        cm,
    }
}

/// REQ-CORE-004, CR 400.2, 401.2, 401.3, 402.3, 406.3, 708.5: exact view for P1 in the
/// two-player scenario.
#[test]
fn exact_view_two_players_p1() {
    let t = two();
    let w = &t.w;
    let c = |id| w.card(id);
    let expected = ZonesView {
        viewer: P1,
        players: BTreeMap::from([
            (
                P1,
                PlayerZonesView {
                    library: lv(2, &[c(t.l1b)]),
                    hand: HandView::Own(vec![ov(w, t.h1, true)]),
                    graveyard: vec![ov(w, t.g1, true)],
                },
            ),
            (
                P2,
                PlayerZonesView {
                    library: lv(1, &[]),
                    hand: HandView::Opponent {
                        count: 2,
                        known_cards: set(&[c(t.h2b)]),
                    },
                    graveyard: vec![ov(w, t.g2fd, false)],
                },
            ),
        ]),
        battlefield: vec![
            ov(w, t.bf1, true),
            ov(w, t.bffd, false),
            ov(w, t.bfsteal, true),
        ],
        stack: vec![ov(w, t.s2, true), ov(w, t.s1fd, true)],
        exile: vec![ov(w, t.exfd, false), ov(w, t.exu, true)],
        command: vec![ov(w, t.cm, true)],
    };
    assert_eq!(w.view(P1), expected);
    assert_eq!(consistency_project(&w.zones, &w.k, P1), expected);
}

/// REQ-CORE-004, CR 400.2, 401.2, 402.3, 406.3, 708.5: exact view for P2 in the two-player
/// scenario (granted face-down exile card visible, opponent's face-down spell hidden).
#[test]
fn exact_view_two_players_p2() {
    let t = two();
    let w = &t.w;
    let c = |id| w.card(id);
    let expected = ZonesView {
        viewer: P2,
        players: BTreeMap::from([
            (
                P1,
                PlayerZonesView {
                    library: lv(2, &[c(t.l1a)]),
                    hand: HandView::Opponent {
                        count: 1,
                        known_cards: set(&[]),
                    },
                    graveyard: vec![ov(w, t.g1, true)],
                },
            ),
            (
                P2,
                PlayerZonesView {
                    library: lv(1, &[]),
                    hand: HandView::Own(vec![ov(w, t.h2a, true), ov(w, t.h2b, true)]),
                    graveyard: vec![ov(w, t.g2fd, false)],
                },
            ),
        ]),
        battlefield: vec![
            ov(w, t.bf1, true),
            ov(w, t.bffd, true),
            ov(w, t.bfsteal, true),
        ],
        stack: vec![ov(w, t.s2, true), ov(w, t.s1fd, false)],
        exile: vec![ov(w, t.exfd, true), ov(w, t.exu, true)],
        command: vec![ov(w, t.cm, true)],
    };
    assert_eq!(w.view(P2), expected);
    assert_eq!(consistency_project(&w.zones, &w.k, P2), expected);
}

/// REQ-CORE-004, CR 406.3, 406.3b: three players; a face-down exiled card with a grant for one
/// viewer only is identified by that viewer only (not even by its owner).
#[test]
fn three_players_grant_for_one_viewer_only() {
    let mut w = KWorld::three();
    let x = w.create_fd(ZoneKind::Exile, P1);
    let h3 = w.create(ZoneKind::Hand, P3);
    let l2 = w.create(ZoneKind::Library, P2);
    w.k.grant_look(&w.zones, x, P2).expect("grant");
    w.k.add_known(&w.zones, h3, P1).expect("add");
    w.k.add_known(&w.zones, l2, P3).expect("add");
    let card = w.card(x);
    for (p, sees) in [(P1, false), (P2, true), (P3, false)] {
        let v = w.view(p);
        assert_eq!(v.exile, vec![ov(&w, x, sees)], "{p:?}");
        assert_eq!(v.exile[0].card.is_some(), sees);
        if sees {
            assert_eq!(v.exile[0].card, Some(card));
        }
        assert_eq!(v, consistency_project(&w.zones, &w.k, p), "{p:?}");
        assert_eq!(v.players.len(), 3, "an entry for every player");
        assert_eq!(v.viewer, p);
    }
    assert_eq!(opp_hand_known(&w.view(P1), P3), set(&[w.card(h3)]));
    assert!(opp_hand_known(&w.view(P2), P3).is_empty());
    assert_eq!(lib_known(&w.view(P3), P2), set(&[w.card(l2)]));
    assert!(
        lib_known(&w.view(P2), P2).is_empty(),
        "own library is hidden too"
    );
    assert!(lib_known(&w.view(P1), P2).is_empty());
}

/// REQ-CORE-004, CR 401.3, 402.3: library and hand counts are public to every viewer.
#[test]
fn counts_are_public() {
    let mut w = KWorld::three();
    for _ in 0..4 {
        w.create(ZoneKind::Library, P2);
    }
    for _ in 0..3 {
        w.create(ZoneKind::Hand, P2);
    }
    w.create(ZoneKind::Library, P3);
    for p in [P1, P2, P3] {
        let v = w.view(p);
        assert_eq!(v.players[&P2].library.count, 4);
        assert_eq!(v.players[&P3].library.count, 1);
        assert_eq!(v.players[&P1].library.count, 0);
        match &v.players[&P2].hand {
            HandView::Own(objs) => {
                assert_eq!(p, P2);
                assert_eq!(objs.len(), 3);
            }
            HandView::Opponent { count, known_cards } => {
                assert_ne!(p, P2);
                assert_eq!(*count, 3);
                assert!(known_cards.is_empty());
            }
        }
    }
}

/// REQ-CORE-004, CR 402.3: the own hand lists every object in ascending ObjectId with its card.
#[test]
fn own_hand_ascending_with_cards() {
    let mut w = KWorld::two();
    let ids: Vec<_> = (0..4).map(|_| w.create(ZoneKind::Hand, P1)).collect();
    let v = w.view(P1);
    match &v.players[&P1].hand {
        HandView::Own(objs) => {
            let got: Vec<_> = objs.iter().map(|o| o.id).collect();
            assert_eq!(got, ids);
            assert!(objs.iter().all(|o| o.card.is_some()));
        }
        HandView::Opponent { .. } => panic!("own hand expected"),
    }
}

/// REQ-CORE-004, CR 400.2: ordered public zones keep the container order (stack and graveyard
/// top first).
#[test]
fn public_zone_order_follows_contents() {
    let mut w = KWorld::two();
    let g: Vec<_> = (0..3).map(|_| w.create(ZoneKind::Graveyard, P1)).collect();
    let s: Vec<_> = (0..3).map(|_| w.create(ZoneKind::Stack, P2)).collect();
    let v = w.view(P2);
    let gv: Vec<_> = v.players[&P1].graveyard.iter().map(|o| o.id).collect();
    let sv: Vec<_> = v.stack.iter().map(|o| o.id).collect();
    assert_eq!(gv, vec![g[2], g[1], g[0]]);
    assert_eq!(sv, vec![s[2], s[1], s[0]]);
}

/// REQ-CORE-004, R12: a viewer not in the game is `UnknownPlayer`.
#[test]
fn project_unknown_viewer() {
    let t = two();
    for bad in [NOBODY, pid(0), pid(u64::MAX)] {
        assert_eq!(
            t.w.k.project(&t.w.zones, bad),
            Err(KnowledgeError::UnknownPlayer(bad))
        );
    }
}

/// REQ-CORE-004, REQ-CORE-001: projection is deterministic and does not mutate Zones or the
/// Knowledge.
#[test]
fn project_deterministic_and_non_mutating() {
    let t = two();
    let zones = t.w.zones.clone();
    let k = t.w.k.clone();
    let enc = bytes(&t.w.k);
    for p in [P1, P2] {
        let a = t.w.k.project(&t.w.zones, p);
        let b = t.w.k.project(&t.w.zones, p);
        assert_eq!(a, b);
        assert_eq!(a, k.project(&zones, p), "a clone projects the same");
    }
    assert_eq!(t.w.zones, zones);
    assert_eq!(t.w.k, k);
    assert_eq!(bytes(&t.w.k), enc);
    // queries do not mutate either
    for id in t.w.zones.all_object_ids() {
        let _ = t.w.k.can_identify(&t.w.zones, P2, id);
        let _ = t.w.k.known_to(&t.w.zones, id);
    }
    let _ = t.w.k.position_known(&t.w.zones, P1, P1);
    assert_eq!(t.w.k, k);
}

/// REQ-CORE-004: a clone is an independent snapshot.
#[test]
fn clone_independence() {
    let t = two();
    let original = t.w.k.clone();
    let mut copy = t.w.k.clone();
    copy.add_known(&t.w.zones, t.h1, P2).expect("add");
    copy.reveal_to_all(&t.w.zones, t.l2).expect("reveal");
    copy.shuffle_library(&t.w.zones, P1, &[]).expect("shuffle");
    assert_ne!(copy, original);
    assert_eq!(t.w.k, original);
    assert!(!t.w.k.can_identify(&t.w.zones, P2, t.h1).expect("ok"));
    assert!(copy.can_identify(&t.w.zones, P2, t.h1).expect("ok"));
    assert_eq!(
        t.w.k.position_known(&t.w.zones, P1, P1),
        BTreeMap::from([(1, t.l1b)])
    );
    assert!(copy.position_known(&t.w.zones, P1, P1).is_empty());
}

/// REQ-CORE-004, REQ-CORE-001: canonical encoding is deterministic, appends to a pre-filled
/// buffer, equal state gives equal bytes, different knowledge gives different bytes.
#[test]
fn canonical_encoding() {
    let t = two();
    assert_appends(&t.w.k);
    assert_appends(&Knowledge::new());
    assert_eq!(bytes(&t.w.k), bytes(&t.w.k.clone()));
    // equal state reached in a different call order
    let mut a = KWorld::three();
    let x = a.create(ZoneKind::Library, P1);
    let y = a.create(ZoneKind::Hand, P2);
    let mut b = a.clone();
    a.k.add_known(&a.zones, x, P2).expect("add");
    a.k.add_known(&a.zones, x, P3).expect("add");
    a.k.add_known(&a.zones, y, P1).expect("add");
    b.k.add_known(&b.zones, y, P1).expect("add");
    b.k.add_known(&b.zones, x, P3).expect("add");
    b.k.add_known(&b.zones, x, P2).expect("add");
    assert_eq!(a.k, b.k);
    assert_eq!(bytes(&a.k), bytes(&b.k));
    // different identity knowledge
    let mut c = a.clone();
    c.k.add_known(&c.zones, y, P3).expect("add");
    assert_ne!(bytes(&a.k), bytes(&c.k));
    // different position knowledge only (KnownTo equal)
    let mut d = a.clone();
    d.k.set_position_known(&d.zones, P2, P1, 0, x).expect("pos");
    assert_eq!(d.known_to(x), a.known_to(x));
    assert_ne!(d.k, a.k);
    assert_ne!(bytes(&a.k), bytes(&d.k));
    // empty differs from non-empty
    assert_ne!(bytes(&Knowledge::new()), bytes(&a.k));
}

/// REQ-CORE-004: hidden information never changes the view: P1's view is the same whatever
/// the CardIds in P2's hand and library, and whatever P2 knows.
#[test]
fn hidden_changes_do_not_change_the_view() {
    let build = |offset: u64, p2_knows: bool| -> KWorld {
        let mut w = KWorld::two();
        let mk = |w: &mut KWorld, card: CardId, kind: ZoneKind, owner: PlayerId| {
            w.zones
                .create_object(&mut w.ids, card, owner, MoveRequest::new(kind))
                .expect("create")
        };
        mk(&mut w, cid(1), ZoneKind::Battlefield, P1);
        let h = mk(&mut w, cid(100 + offset), ZoneKind::Hand, P2);
        let l = mk(&mut w, cid(200 + offset), ZoneKind::Library, P2);
        let ml = mk(&mut w, cid(300 + offset), ZoneKind::Library, P1);
        if p2_knows {
            w.k.add_known(&w.zones, ml, P2).expect("add");
            w.k.set_position_known(&w.zones, P2, P2, 0, l).expect("pos");
            w.k.add_known(&w.zones, h, P2).expect("add");
        }
        w
    };
    let base = build(0, false);
    let other = build(7, true);
    assert_eq!(base.view(P1), other.view(P1));
    assert_ne!(base.view(P2), other.view(P2), "P2's own hand differs");
}
