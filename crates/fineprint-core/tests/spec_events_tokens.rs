// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the CreateToken event and token zone changes (REQ-EVENT-001, REQ-DSL-003;
//! CR 111.2, 111.8, 701.7a, 616.1g). Black-box: written from the work-item brief only.

#[path = "spec_events_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::events::{
    EventError, GameEvent, HistoryQuery, HistoryScope, HistoryStat, Outcome, ZoneChangeCause,
};
use fineprint_core::zones::{MoveRequest, ZoneError, ZoneId, ZoneKind};
use support::*;

fn token_object(outcome: &Outcome) -> ObjectId {
    match outcome {
        Outcome::TokenCreated { object } => *object,
        other => panic!("expected TokenCreated, got {other:?}"),
    }
}

fn zc(object: ObjectId, to: ZoneKind) -> GameEvent {
    GameEvent::ZoneChange {
        object,
        request: MoveRequest::new(to),
        cause: ZoneChangeCause::Other,
    }
}

// REQ-EVENT-001, CR 111.2, 701.7a: the token enters the battlefield owned and controlled by its
// creator; permanent state exists; the card is marked a token; no LKI.
#[test]
fn create_token_puts_a_permanent_onto_the_battlefield() {
    let mut w = World::new();
    w.game.set_turn_number(3);
    let card = w.fresh_card();
    assert!(!w.events.markers().is_token(card));
    let lki_before = bytes(&w.lki);
    let done = w.ok(GameEvent::CreateToken {
        owner: P1,
        card,
        controller: None,
    });
    assert!(!done.lki_captured);
    let t = token_object(&done.outcome);
    let rec = w.zones.object(t).expect("token in a zone");
    assert_eq!(rec.zone, ZoneId::Battlefield);
    assert_eq!(rec.owner, P1);
    assert_eq!(rec.controller, P1);
    assert_eq!(rec.card, card);
    let perm = w.permanents.get(t).expect("permanent state");
    assert_eq!(perm.controller(), P1);
    assert_eq!(perm.controlled_since_turn(), 3);
    assert!(w.events.markers().is_token(card));
    assert_eq!(bytes(&w.lki), lki_before);
}

// REQ-EVENT-001, CR 111.2: `controller: Some(other)` gives the other player control.
#[test]
fn create_token_under_another_controller() {
    let mut w = World::new();
    let card = w.fresh_card();
    let t = token_object(
        &w.ok(GameEvent::CreateToken {
            owner: P1,
            card,
            controller: Some(P2),
        })
        .outcome,
    );
    let rec = w.zones.object(t).unwrap();
    assert_eq!(rec.owner, P1);
    assert_eq!(rec.controller, P2);
    assert_eq!(w.permanents.get(t).unwrap().controller(), P2);
}

// REQ-EVENT-001: a duplicate CardId surfaces as EventError::Zone with nothing changed.
#[test]
fn create_token_with_duplicate_card_fails_cleanly() {
    let mut w = World::new();
    let existing = w.create(ZoneKind::Hand, P1);
    let card = w.zones.object(existing).unwrap().card;
    let before = w.snap();
    assert_eq!(
        w.exec(GameEvent::CreateToken {
            owner: P2,
            card,
            controller: None,
        }),
        Err(EventError::Zone(ZoneError::CardAlreadyPresent(card)))
    );
    assert_eq!(w.snap(), before);
    assert!(!w.events.markers().is_token(card));
}

// REQ-EVENT-001: unknown owner or controller is UnknownPlayer with nothing changed.
#[test]
fn create_token_unknown_player_fails_cleanly() {
    let mut w = World::new();
    let card = w.fresh_card();
    let before = w.snap();
    assert_eq!(
        w.exec(GameEvent::CreateToken {
            owner: NOBODY,
            card,
            controller: None,
        }),
        Err(EventError::UnknownPlayer(NOBODY))
    );
    assert_eq!(
        w.exec(GameEvent::CreateToken {
            owner: P1,
            card,
            controller: Some(NOBODY),
        }),
        Err(EventError::UnknownPlayer(NOBODY))
    );
    assert_eq!(w.snap(), before);
}

// REQ-EVENT-001, CR 111.8, 603.10a: a token leaves the battlefield once (its LKI is captured);
// afterwards it cannot change zones: a second ZoneChange is no event and it stays put.
#[test]
fn token_that_left_the_battlefield_cannot_move_again() {
    let mut w = World::new();
    let card = w.fresh_card();
    let t = token_object(
        &w.ok(GameEvent::CreateToken {
            owner: P1,
            card,
            controller: None,
        })
        .outcome,
    );
    let done = w.ok(zc(t, ZoneKind::Graveyard));
    assert!(done.lki_captured);
    assert!(w.lki.get(t).is_some());
    let gy = match done.outcome {
        Outcome::ZoneChange(m) => m.new,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(w.zones.object(gy).unwrap().zone, ZoneId::Graveyard(P1));
    assert!(w.events.markers().is_token(card));

    let before = w.snap();
    for to in [
        ZoneKind::Exile,
        ZoneKind::Battlefield,
        ZoneKind::Hand,
        ZoneKind::Graveyard,
    ] {
        assert_eq!(w.exec(zc(gy, to)), Ok(None), "to {to:?}");
    }
    assert_eq!(w.snap(), before);
    assert_eq!(w.zones.object(gy).unwrap().zone, ZoneId::Graveyard(P1));
}

// REQ-DSL-003: TokensCreated counts executed creations for the owner only.
#[test]
fn tokens_created_ledger() {
    let mut w = World::new();
    for _ in 0..2 {
        let card = w.fresh_card();
        w.ok(GameEvent::CreateToken {
            owner: P2,
            card,
            controller: Some(P1),
        });
    }
    let q = |p| {
        w.events.history().query(&HistoryQuery {
            stat: HistoryStat::TokensCreated,
            scope: HistoryScope::Game,
            player: Some(p),
        })
    };
    assert_eq!(q(P2), 2);
    assert_eq!(q(P1), 0);
}
