// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the Draw event (REQ-EVENT-001, REQ-DSL-003; CR 121.1, 121.2, 121.4,
//! 704.5b). Black-box: written from the work-item brief only.

#[path = "spec_events_support.rs"]
mod support;

use fineprint_core::events::ZoneChangeCause;
use fineprint_core::events::{
    EventError, GameEvent, HistoryQuery, HistoryScope, HistoryStat, Outcome,
};
use fineprint_core::zones::{MoveRequest, Position, ZoneId, ZoneKind};
use support::*;

fn drawn(w: &World, p: fineprint_base::ids::PlayerId) -> i64 {
    w.events.history().query(&HistoryQuery {
        stat: HistoryStat::CardsDrawn,
        scope: HistoryScope::Game,
        player: Some(p),
    })
}

// REQ-EVENT-001, CR 121.1: a draw puts exactly the top card of the library into the hand.
#[test]
fn draw_moves_the_top_card_to_hand() {
    let mut w = World::new();
    for _ in 0..3 {
        w.create(ZoneKind::Library, P1);
    }
    let lib = w.zones.contents(ZoneId::Library(P1));
    let top = lib[0];
    let card = w.zones.object(top).unwrap().card;

    let done = w.ok(GameEvent::Draw { player: P1 });
    let m = match done.outcome {
        Outcome::ZoneChange(m) => m,
        other => panic!("expected ZoneChange outcome, got {other:?}"),
    };
    assert_eq!(m.old, top);
    assert_eq!(m.from, ZoneId::Library(P1));
    assert_eq!(m.to, ZoneId::Hand(P1));
    assert_eq!(w.zones.contents(ZoneId::Hand(P1)), vec![m.new]);
    assert_eq!(w.zones.object(m.new).unwrap().card, card);
    assert_eq!(w.zones.contents(ZoneId::Library(P1)), lib[1..].to_vec());
    assert!(w.zones.is_empty(ZoneId::Hand(P2)));
}

// REQ-EVENT-001, CR 121.2: two Draw events in one batch draw two cards, one at a time, in
// library order.
#[test]
fn two_draws_in_one_batch_draw_two_cards_in_order() {
    let mut w = World::new();
    for _ in 0..3 {
        w.create(ZoneKind::Library, P1);
    }
    let lib = w.zones.contents(ZoneId::Library(P1));
    let batch = w
        .batch(vec![
            GameEvent::Draw { player: P1 },
            GameEvent::Draw { player: P1 },
        ])
        .expect("both draws succeed");
    assert_eq!(batch.events.len(), 2);
    let olds: Vec<_> = batch
        .events
        .iter()
        .map(|e| match e.outcome {
            Outcome::ZoneChange(m) => m.old,
            ref other => panic!("unexpected outcome {other:?}"),
        })
        .collect();
    assert_eq!(olds, vec![lib[0], lib[1]]);
    assert_eq!(w.zones.len(ZoneId::Hand(P1)), 2);
    assert_eq!(w.zones.contents(ZoneId::Library(P1)), vec![lib[2]]);
    assert_eq!(drawn(&w, P1), 2);
}

// REQ-EVENT-001, CR 121.4, 704.5b: a draw from an empty library is no event, changes no zone,
// and records the player once (even after two attempts); take returns then clears.
#[test]
fn empty_library_draw_is_flagged_once() {
    let mut w = World::new();
    w.create(ZoneKind::Library, P2);
    assert!(w.events.empty_library_draws().is_empty());
    let zones_before = bytes(&w.zones);
    let log_len = w.events.log().len();

    assert_eq!(w.exec(GameEvent::Draw { player: P1 }), Ok(None));
    assert_eq!(w.exec(GameEvent::Draw { player: P1 }), Ok(None));
    assert_eq!(bytes(&w.zones), zones_before);
    assert_eq!(w.events.log().len(), log_len);
    assert_eq!(drawn(&w, P1), 0);
    assert_eq!(w.events.empty_library_draws(), vec![P1]);
    // Reading does not clear.
    assert_eq!(w.events.empty_library_draws(), vec![P1]);

    assert_eq!(w.events.take_empty_library_draws(), vec![P1]);
    assert!(w.events.empty_library_draws().is_empty());
    assert!(w.events.take_empty_library_draws().is_empty());
}

// REQ-EVENT-001, CR 704.5b: the set is ascending with no duplicates.
#[test]
fn empty_library_draws_are_ascending_without_duplicates() {
    let mut w = World::new();
    for p in [P2, P1, P2, P1] {
        assert_eq!(w.exec(GameEvent::Draw { player: p }), Ok(None));
    }
    assert_eq!(w.events.empty_library_draws(), vec![P1, P2]);
    assert_eq!(w.events.take_empty_library_draws(), vec![P1, P2]);
    assert!(w.events.empty_library_draws().is_empty());
}

// REQ-EVENT-001: an unknown player is UnknownPlayer and nothing changes.
#[test]
fn draw_by_unknown_player_fails_cleanly() {
    let mut w = World::new();
    w.create(ZoneKind::Library, P1);
    let before = w.snap();
    assert_eq!(
        w.exec(GameEvent::Draw { player: NOBODY }),
        Err(EventError::UnknownPlayer(NOBODY))
    );
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 603.10a: the drawn card's LKI is captured like any zone exit.
#[test]
fn draw_captures_lki_of_the_drawn_card() {
    let mut w = World::new();
    w.create(ZoneKind::Library, P1);
    let top = w.zones.contents(ZoneId::Library(P1))[0];
    w.provider.set_chars(top, chars("test.drawn_2_2", 2, 2));
    let done = w.ok(GameEvent::Draw { player: P1 });
    assert!(done.lki_captured);
    let rec = w.lki.get(top).expect("captured");
    assert_eq!(rec.zone(), ZoneId::Library(P1));
    assert_eq!(rec.characteristics(), &chars("test.drawn_2_2", 2, 2));
}

// REQ-DSL-003, CR 121.2, 121.4: CardsDrawn counts only executed draws (not empty-library
// attempts, not draws of a rolled-back batch).
#[test]
fn cards_drawn_counts_only_executed_draws() {
    let mut w = World::new();
    for _ in 0..2 {
        w.create(ZoneKind::Library, P1);
    }
    w.ok(GameEvent::Draw { player: P1 });
    assert_eq!(drawn(&w, P1), 1);
    // A rolled-back batch draws nothing.
    let r = w.batch(vec![
        GameEvent::Draw { player: P1 },
        GameEvent::Draw { player: NOBODY },
    ]);
    assert_eq!(r, Err(EventError::UnknownPlayer(NOBODY)));
    assert_eq!(drawn(&w, P1), 1);
    w.ok(GameEvent::Draw { player: P1 });
    assert_eq!(w.exec(GameEvent::Draw { player: P1 }), Ok(None));
    assert_eq!(drawn(&w, P1), 2);
    assert_eq!(drawn(&w, P2), 0);
}

// REQ-EVENT-001, CR 400.7: a same-zone reposition captures no LKI (the object does not leave its
// zone); the Draw after it in the same batch takes the card now on top and does capture LKI.
#[test]
fn same_zone_reposition_then_draw_in_one_batch() {
    let mut w = World::new();
    for _ in 0..3 {
        w.create(ZoneKind::Library, P1);
    }
    let lib = w.zones.contents(ZoneId::Library(P1));
    let (t0, t1) = (lib[0], lib[1]);
    let log_len = w.events.log().len();
    let done = w
        .batch(vec![
            GameEvent::ZoneChange {
                object: t0,
                request: MoveRequest {
                    to: ZoneKind::Library,
                    position: Position::Bottom,
                    face_down: false,
                    controller: None,
                },
                cause: ZoneChangeCause::Other,
            },
            GameEvent::Draw { player: P1 },
        ])
        .expect("batch succeeds");
    assert_eq!(done.events.len(), 2);
    assert!(!done.events[0].lki_captured);
    match &done.events[0].outcome {
        Outcome::ZoneChange(m) => {
            assert_eq!(m.old, m.new);
            assert_eq!(m.from, m.to);
        }
        other => panic!("expected ZoneChange outcome, got {other:?}"),
    }
    assert!(done.events[1].lki_captured);
    let moved = match &done.events[1].outcome {
        Outcome::ZoneChange(m) => *m,
        other => panic!("expected ZoneChange outcome, got {other:?}"),
    };
    assert_eq!(moved.old, t1, "the card now on top was drawn");
    assert_eq!(moved.to, ZoneId::Hand(P1));
    assert!(w.lki.get(moved.old).is_some());
    assert!(
        w.lki.get(t0).is_none(),
        "the repositioned card left nothing behind"
    );
    assert_eq!(w.events.log().len(), log_len + 1);
}

// REQ-EVENT-001, CR 111.8, 121.1: a token that left the battlefield to the library cannot be
// drawn: Draw is a no-op (no move, nothing counted, no empty-library flag change, nothing
// logged), also when another card lies beneath it.
#[test]
fn draw_of_a_token_in_the_library_is_a_no_op() {
    for cards_beneath in [0usize, 1] {
        let mut w = World::new();
        for _ in 0..cards_beneath {
            w.create(ZoneKind::Library, P1);
        }
        let card = w.fresh_card();
        let created = w.ok(GameEvent::CreateToken {
            owner: P1,
            card,
            controller: None,
        });
        let token = match created.outcome {
            Outcome::TokenCreated { object } => object,
            other => panic!("expected TokenCreated, got {other:?}"),
        };
        let moved = match w
            .ok(GameEvent::ZoneChange {
                object: token,
                request: MoveRequest::new(ZoneKind::Library),
                cause: ZoneChangeCause::Other,
            })
            .outcome
        {
            Outcome::ZoneChange(m) => m,
            other => panic!("expected ZoneChange outcome, got {other:?}"),
        };
        let new = moved.new;
        assert_eq!(w.zones.contents(ZoneId::Library(P1))[0], new);
        let flags = w.events.empty_library_draws();
        let log_len = w.events.log().len();
        let before = w.snap();

        assert_eq!(w.exec(GameEvent::Draw { player: P1 }), Ok(None));
        assert_eq!(w.zones.object(new).unwrap().zone, ZoneId::Library(P1));
        assert_eq!(drawn(&w, P1), 0);
        assert_eq!(w.events.empty_library_draws(), flags);
        assert_eq!(w.events.log().len(), log_len);
        assert_eq!(w.snap(), before);
    }
}

// REQ-EVENT-001, CR 400.7: with a one-card library the Draw takes the very card that was just
// repositioned in place: the reposition still reports no LKI, the Draw does.
#[test]
fn same_zone_reposition_then_draw_of_the_same_card() {
    let mut w = World::new();
    let only = w.create(ZoneKind::Library, P1);
    let done = w
        .batch(vec![
            GameEvent::ZoneChange {
                object: only,
                request: MoveRequest {
                    to: ZoneKind::Library,
                    position: Position::Bottom,
                    face_down: false,
                    controller: None,
                },
                cause: ZoneChangeCause::Other,
            },
            GameEvent::Draw { player: P1 },
        ])
        .expect("batch succeeds");
    assert_eq!(done.events.len(), 2);
    assert!(!done.events[0].lki_captured);
    assert!(done.events[1].lki_captured);
    assert!(w.lki.get(only).is_some());
}
