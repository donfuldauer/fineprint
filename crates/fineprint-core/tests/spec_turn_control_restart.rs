// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for restarting the game (REQ-TURN-003; CR 728.1, 728.1a, 728.2, 728.4, 728.5).
//! Black-box: written from the work-item brief only. A "restarting effect" is any spell or
//! ability that restarts the game; no card is named (R5).

#[path = "spec_turn_control_support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::{ChoiceError, ChoiceResponse, PendingChoiceRequest};
use fineprint_core::turn::control::{RestartReport, restart_game};
use fineprint_core::turn::{GameStartHost, RestartHost, StartOptions};
use support::start::{Policy, card, drive};
use support::*;

fn keep(req: &PendingChoiceRequest) -> ChoiceResponse {
    Policy::keep_all().answer(req)
}

fn restart(h: RFake, restarter: PlayerId) -> Result<(RFake, RestartReport), ChoiceError> {
    run_session(h, move |h, c| restart_game(h, restarter, c), keep)
}

/// A two-player game in progress: P1 started it, some cards are in hands and elsewhere (P1's
/// card 3 is controlled by P2, P2's card 4 by P1).
fn mid_game() -> RFake {
    let mut h = RFake::new(2, 10);
    h.m.starting = Some(P1);
    for p in [1u64, 2] {
        let lib: Vec<ObjectId> = (5..=10).map(|i| card(p, i)).collect();
        h.m.libs.insert(pid(p), lib);
        h.m.hands.insert(pid(p), vec![card(p, 1), card(p, 2)]);
    }
    h.elsewhere.insert(card(1, 3), P2);
    h.elsewhere.insert(card(1, 4), P1);
    h.elsewhere.insert(card(2, 3), P2);
    h.elsewhere.insert(card(2, 4), P1);
    h
}

/// Every card of the game, by owner.
fn all_cards() -> BTreeMap<ObjectId, PlayerId> {
    let mut m = BTreeMap::new();
    for p in [1u64, 2] {
        for i in 1..=10 {
            m.insert(card(p, i), pid(p));
        }
    }
    m
}

/// REQ-TURN-003, CR 728.1, CR 728.1a, CR 728.4: the call order: the game ends without a
/// result, the exempt cards are read, the game is reset, the new game starts with the
/// restarter as starting player, and the restarting effect finishes resolving last.
#[test]
fn restart_call_order() {
    let mut h = mid_game();
    h.exempt = [card(1, 3)].into_iter().collect();
    let (h, rep) = restart(h, P2).expect("restart");
    let calls = h.calls();
    assert_eq!(calls[0], "end_game");
    assert_eq!(calls[1], "exempt_cards");
    assert_eq!(calls[2], format!("reset:{:?}", vec![card(1, 3).get()]));
    // CR 728.1a: no random starting-player draw; the restarter starts.
    assert_eq!(calls[3], "start:2");
    assert_eq!(calls.last().map(String::as_str), Some("finish"));
    assert_eq!(calls.iter().filter(|c| *c == "finish").count(), 1);
    assert_eq!(calls.iter().filter(|c| *c == "end_game").count(), 1);
    assert_eq!(calls.iter().filter(|c| c.starts_with("start:")).count(), 1);
    assert!(calls.iter().any(|c| c.starts_with("setlib:")));
    assert!(calls.iter().any(|c| c.starts_with("life:")));
    assert!(calls.iter().any(|c| c.starts_with("draw:")));
    assert_eq!(rep.start.starting_player, P2);
    assert_eq!(h.m.starting, Some(P2));
}

/// REQ-TURN-003, CR 728.1a: the starting player is the restarter, whether or not that is the
/// previous starting player (both cases, both players).
#[test]
fn restarter_starts_the_new_game() {
    for previous in [P1, P2] {
        for restarter in [P1, P2] {
            let mut h = mid_game();
            h.m.starting = Some(previous);
            let (h, rep) = restart(h, restarter).expect("restart");
            assert_eq!(rep.start.starting_player, restarter);
            assert_eq!(h.m.starting, Some(restarter));
            assert!(h.calls().contains(&format!("start:{}", restarter.get())));
        }
    }
}

/// REQ-TURN-003, CR 728.1, CR 728.1a: the game start is exactly `run_game_start` with the
/// restarter given, over the reset game: same report, same libraries and hands, same PRNG state
/// (so no random starting-player draw was made).
#[test]
fn restart_equals_game_start_with_restarter() {
    for restarter in [P1, P2] {
        let mut h = mid_game();
        h.exempt = [card(2, 4), card(1, 1)].into_iter().collect();
        // Reference: reset by hand, then the plain game start with the restarter given.
        let mut reference = h.clone();
        let exempt = reference.exempt.clone();
        reference.reset_for_new_game(&exempt).expect("reset");
        let expected = drive(
            reference.m.clone(),
            StartOptions {
                starting_player: Some(restarter),
            },
            &Policy::keep_all(),
        );
        let (h, rep) = restart(h, restarter).expect("restart");
        assert_eq!(rep.start, expected.report);
        assert_eq!(h.m.raw(), expected.host.raw());
        assert_eq!(h.m.libs, expected.host.libs);
        assert_eq!(h.m.hands, expected.host.hands);
        // And not the random draw: running without the option from the same state would make
        // one extra PRNG call first.
        let random = drive(
            reference.m.clone(),
            StartOptions::default(),
            &Policy::keep_all(),
        );
        assert_ne!(random.host.raw(), h.m.raw());
    }
}

/// REQ-TURN-003, CR 728.5: the exempt set reaches `reset_for_new_game` and the report, and
/// exempt cards are not in their owner's deck as the new game begins.
#[test]
fn exempt_cards() {
    for exempt in [
        BTreeSet::new(),
        [card(1, 3)].into_iter().collect::<BTreeSet<_>>(),
        [card(1, 1), card(2, 4), card(2, 9)].into_iter().collect(),
    ] {
        let mut h = mid_game();
        h.exempt = exempt.clone();
        let (h, rep) = restart(h, P1).expect("restart");
        assert_eq!(rep.exempt, exempt);
        assert_eq!(h.reset_got.as_ref(), Some(&exempt));
        assert_eq!(h.left_out, exempt);
        let deck = h.deck_cards();
        for c in &exempt {
            assert!(!deck.contains_key(c), "{c:?} is exempt");
        }
    }
}

/// REQ-TURN-003, CR 728.2: every card of the game (wherever it was, whoever controlled it)
/// is in the new game in its OWNER's library or opening hand; ownership does not change.
#[test]
fn cards_return_to_owners() {
    let (h, rep) = restart(mid_game(), P2).expect("restart");
    let deck = h.deck_cards();
    // Each card occurs exactly once: the raw entry count (before the map deduplicates) equals
    // the number of distinct cards of the game.
    let raw: usize = h.m.libs.values().map(Vec::len).sum::<usize>()
        + h.m.hands.values().map(Vec::len).sum::<usize>();
    assert_eq!(raw, all_cards().len());
    assert_eq!(deck.len(), raw);
    assert_eq!(deck, all_cards());
    for (p, hand) in &rep.start.opening_hands {
        for c in hand {
            assert_eq!(owner_of(*c), *p);
        }
        assert_eq!(hand.len(), 7);
    }
    assert!(h.elsewhere.is_empty());
}

/// REQ-TURN-003, CR 728.1: no player wins, loses or draws the restarted game; every player is
/// in the new game.
#[test]
fn no_result_and_everyone_plays_again() {
    let (h, _) = restart(mid_game(), P1).expect("restart");
    let game = GameStartHost::game(&h);
    assert_eq!(game.turn_order(), &[P1, P2]);
    for p in [P1, P2] {
        let state = game.player(p).expect("still a player");
        assert!(
            state.designations().is_empty(),
            "{p:?} has a result: {:?}",
            state.designations()
        );
        assert_eq!(state.life(), 20);
    }
    let calls = h.calls();
    for p in [1, 2] {
        assert!(calls.contains(&format!("life:{p}:20")));
    }
}

/// REQ-TURN-003, CR 728.1a: a restarter who is not a player of the game is a procedure error.
#[test]
fn restarter_not_a_player_is_an_error() {
    for bad in [PlayerId::new(3), PlayerId::new(99)] {
        match restart(mid_game(), bad) {
            Err(ChoiceError::Procedure(_)) => {}
            other => panic!("expected a procedure error for {bad:?}, got {other:?}"),
        }
    }
}

/// REQ-TURN-003, CR 728.1a: three players: any of them can be the restarter.
#[test]
fn three_player_restart() {
    for restarter in [P1, P2, P3] {
        let mut h = RFake::new(3, 12);
        h.m.starting = Some(P1);
        let (h, rep) = restart(h, restarter).expect("restart");
        assert_eq!(rep.start.starting_player, restarter);
        let calls = h.calls();
        let start_at = calls
            .iter()
            .position(|c| *c == format!("start:{}", restarter.get()))
            .expect("starting player set");
        let reset_at = calls
            .iter()
            .position(|c| c.starts_with("reset:"))
            .expect("reset");
        assert!(reset_at < start_at);
        assert_eq!(calls.last().map(String::as_str), Some("finish"));
    }
}

/// REQ-TURN-003: a restart is deterministic.
#[test]
fn restart_is_deterministic() {
    let a = restart(mid_game(), P2).expect("restart");
    let b = restart(mid_game(), P2).expect("restart");
    assert_eq!(a.1, b.1);
    assert_eq!(a.0.calls(), b.0.calls());
    assert_eq!(a.0.m.raw(), b.0.m.raw());
}
