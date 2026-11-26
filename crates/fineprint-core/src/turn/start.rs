// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Game start: the starting player, shuffles, starting life, opening hands, the London mulligan
//! and the pregame actions (REQ-FMT-001, REQ-NET-003; CR 103.1, CR 103.3-103.6).
//!
//! [`run_game_start`] performs everything from "the players have decks" to "the first turn can
//! begin", written against the [`GameStartHost`] seam (the game itself arrives in later units,
//! docs/DECISIONS.md "#48 FP-046 — GameStartHost seam"). The order of PRNG calls is part of the
//! contract: one `next_bounded` for the starting player (none when
//! [`StartOptions::starting_player`] is set), then one shuffle per player in stored turn order,
//! then one shuffle per mulligan taken, in declaration order within each round.

use std::collections::BTreeMap;

use fineprint_base::ids::{ObjectId, PlayerId};

use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOptions, ChoicePayload, ChoiceReason, ChoiceResponse,
    ChoiceType, Interrupt, PendingChoiceRequest, ValidatorContract, ValidatorRule,
};
use crate::format::FormatConfig;
use crate::state::GameState;

/// Options of one game start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartOptions {
    /// `Some(p)`: `p` plays first and NO random draw is made (a match's later game, a house
    /// rule; CR 103.1).
    pub starting_player: Option<PlayerId>,
}

/// What a game start did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartReport {
    /// The player who plays first (CR 103.1).
    pub starting_player: PlayerId,
    /// Mulligans TAKEN per player (free ones included), in the host's stored turn order.
    pub mulligans: Vec<(PlayerId, u64)>,
    /// Each player's opening hand (ascending ids), in the host's stored turn order.
    pub opening_hands: Vec<(PlayerId, Vec<ObjectId>)>,
}

/// The game-side operations the game start needs (the seam of this module).
pub trait GameStartHost {
    /// The game state (the PRNG, the turn order and the players live here).
    fn game(&self) -> &GameState;
    /// The game state, mutably.
    fn game_mut(&mut self) -> &mut GameState;
    /// The format (starting life, hand size, mulligan rules).
    fn format(&self) -> &FormatConfig;
    /// CR 103.5: some effects modify a player's starting hand size. Default: the format's.
    fn starting_hand_size(&self, _player: PlayerId) -> u32 {
        self.format().starting_hand_size
    }
    /// The library, TOP FIRST (index 0 is the top).
    fn library(&self, player: PlayerId) -> Vec<ObjectId>;
    /// Replaces the library order; `order` is a permutation of the current library.
    ///
    /// # Errors
    /// Any interrupt of the host.
    fn set_library(&mut self, player: PlayerId, order: Vec<ObjectId>) -> Result<(), Interrupt>;
    /// The hand, ascending ids.
    fn hand(&self, player: PlayerId) -> Vec<ObjectId>;
    /// Moves the top `count` library cards to the hand (a Draw event each), top card first.
    ///
    /// # Errors
    /// Any interrupt of the host.
    fn draw_cards(
        &mut self,
        player: PlayerId,
        count: u32,
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt>;
    /// Moves EVERY hand card to the library: new library = old library followed by the hand
    /// cards in ascending id order (the caller shuffles right after).
    ///
    /// # Errors
    /// Any interrupt of the host.
    fn return_hand_to_library(&mut self, player: PlayerId) -> Result<(), Interrupt>;
    /// Moves hand cards to the library bottom: new library = old library followed by `cards` in
    /// the order given (the LAST listed card is the bottom-most).
    ///
    /// # Errors
    /// Any interrupt of the host.
    fn put_on_library_bottom(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
    ) -> Result<(), Interrupt>;
    /// Records who plays first (this is what `TurnHost::starting_player()` reports).
    ///
    /// # Errors
    /// Any interrupt of the host.
    fn set_starting_player(&mut self, player: PlayerId) -> Result<(), Interrupt>;
    /// Sets a player's life total.
    ///
    /// # Errors
    /// Any interrupt of the host.
    fn set_player_life(&mut self, player: PlayerId, life: i64) -> Result<(), Interrupt>;
    /// Opening-hand cards that allow a pregame action (CR 103.6); ascending ids; empty: none.
    fn pregame_eligible(&self, player: PlayerId) -> Vec<ObjectId>;
    /// Performs the pregame actions of exactly `cards`, in the order given (CR 103.6a,
    /// CR 103.6b).
    ///
    /// # Errors
    /// Any interrupt of the host.
    fn perform_pregame_actions(
        &mut self,
        player: PlayerId,
        cards: &[ObjectId],
        choices: &mut ChoiceContext<'_>,
    ) -> Result<(), Interrupt>;
}

/// CR 103.5c: the number of mulligans that count toward the cards to put on the bottom (and
/// toward the mulligan limit) after `taken` mulligans. In a multiplayer game (more than two
/// players) with `free_first` set, the first mulligan does not count.
#[must_use]
pub fn mulligan_to_bottom(taken: u64, player_count: usize, free_first: bool) -> u64 {
    if free_first && player_count > 2 && taken >= 1 {
        taken.saturating_sub(1)
    } else {
        taken
    }
}

/// The error of a protocol misuse or an untrusted answer.
fn procedure_error(message: &str) -> Interrupt {
    Interrupt::Error(ChoiceError::Procedure(format!("game start: {message}")))
}

/// Whether `objects` are distinct and all in `pool`.
fn distinct_subset(objects: &[ObjectId], pool: &[ObjectId]) -> bool {
    let mut seen: Vec<ObjectId> = Vec::with_capacity(objects.len());
    for o in objects {
        if !pool.contains(o) || seen.contains(o) {
            return false;
        }
        seen.push(*o);
    }
    true
}

/// CR 103.3: shuffles `player`'s library with the state's PRNG.
fn shuffle_library<H: GameStartHost + ?Sized>(
    host: &mut H,
    player: PlayerId,
) -> Result<(), Interrupt> {
    let mut lib = host.library(player);
    host.game_mut().prng_mut().shuffle(&mut lib);
    host.set_library(player, lib)
}

/// CR 103.5: the one `MULLIGAN_DECISION` request of `player`; `Ok(Some(bottom))` keeps the hand,
/// `Ok(None)` takes a mulligan.
fn ask_mulligan<H: GameStartHost + ?Sized>(
    host: &H,
    player: PlayerId,
    taken: u64,
    to_bottom: u64,
    choices: &mut ChoiceContext<'_>,
) -> Result<Option<Vec<ObjectId>>, Interrupt> {
    let hand = host.hand(player);
    let default_bottom: Vec<ObjectId> = hand
        .iter()
        .take(usize::try_from(to_bottom).unwrap_or(usize::MAX))
        .copied()
        .collect();
    let request = PendingChoiceRequest {
        request_id: choices.next_request_id(),
        chooser: player,
        choice_type: ChoiceType::MulliganDecision,
        reason: ChoiceReason {
            cr: "103.5".to_owned(),
            source: None,
        },
        min: to_bottom,
        max: to_bottom,
        hidden: true,
        reveal_to: vec![player],
        default_on_timeout: ChoiceResponse::KeepHand {
            bottom: default_bottom,
        },
        options: ChoiceOptions::Validator(ValidatorContract {
            min: to_bottom,
            max: to_bottom,
            rule: ValidatorRule::Payload,
        }),
        payload: ChoicePayload::MulliganDecision {
            hand: hand.clone(),
            mulligans_taken: taken,
            to_bottom,
        },
    };
    match choices.ask(request)? {
        ChoiceResponse::KeepHand { bottom } => {
            // R12: the session validated the answer; never trust it blindly anyway.
            let count = u64::try_from(bottom.len()).unwrap_or(u64::MAX);
            if count != to_bottom || !distinct_subset(&bottom, &hand) {
                return Err(procedure_error(
                    "the KEEP_HAND answer is not a valid bottom",
                ));
            }
            Ok(Some(bottom))
        }
        ChoiceResponse::TakeMulligan => Ok(None),
        _ => Err(procedure_error(
            "the mulligan answer is neither KEEP_HAND nor TAKE_MULLIGAN",
        )),
    }
}

/// CR 103.6: the one `PREGAME_ACTION` request of `player`.
fn ask_pregame<H: GameStartHost + ?Sized>(
    host: &mut H,
    player: PlayerId,
    eligible: Vec<ObjectId>,
    choices: &mut ChoiceContext<'_>,
) -> Result<(), Interrupt> {
    let max = u64::try_from(eligible.len()).unwrap_or(u64::MAX);
    let request = PendingChoiceRequest {
        request_id: choices.next_request_id(),
        chooser: player,
        choice_type: ChoiceType::PregameAction,
        reason: ChoiceReason {
            cr: "103.6".to_owned(),
            source: None,
        },
        min: 0,
        max,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SubmitPregame {
            objects: Vec::new(),
        },
        options: ChoiceOptions::Validator(ValidatorContract {
            min: 0,
            max,
            rule: ValidatorRule::Subset {
                candidates: eligible.clone(),
            },
        }),
        payload: ChoicePayload::PregameAction {
            eligible: eligible.clone(),
        },
    };
    let ChoiceResponse::SubmitPregame { objects } = choices.ask(request)? else {
        return Err(procedure_error("the pregame answer is not SUBMIT_PREGAME"));
    };
    if !distinct_subset(&objects, &eligible) {
        return Err(procedure_error(
            "the pregame answer is not a set of eligible cards",
        ));
    }
    if objects.is_empty() {
        return Ok(());
    }
    host.perform_pregame_actions(player, &objects, choices)
}

/// Runs the game start (REQ-FMT-001, REQ-NET-003): CR 103.1 the starting player, CR 103.3 the
/// shuffles, CR 103.4 starting life, CR 103.5 the opening hands and the London mulligan, CR
/// 103.6 the pregame actions.
///
/// # Errors
/// [`ChoiceError::Procedure`] for a game without players, an unknown
/// [`StartOptions::starting_player`] or an invalid answer; `Interrupt::Pending` while a request
/// waits for its answer; any interrupt of the host.
pub fn run_game_start<H: GameStartHost + ?Sized>(
    host: &mut H,
    options: &StartOptions,
    choices: &mut ChoiceContext<'_>,
) -> Result<StartReport, Interrupt> {
    let stored: Vec<PlayerId> = host.game().turn_order().to_vec();
    if stored.is_empty() {
        return Err(procedure_error("the game has no players"));
    }
    let player_count = stored.len();

    // CR 103.1: the starting player; a seeded draw (one call) unless the option names one.
    let starting = match options.starting_player {
        Some(p) => {
            if !stored.contains(&p) {
                return Err(procedure_error("the starting player is not a player"));
            }
            p
        }
        None => {
            let bound = u64::try_from(player_count).unwrap_or(u64::MAX);
            let drawn = host.game_mut().prng_mut().next_bounded(bound);
            let index = drawn.and_then(|i| usize::try_from(i).ok());
            match index.and_then(|i| stored.get(i)) {
                Some(p) => *p,
                None => return Err(procedure_error("the starting player draw failed")),
            }
        }
    };
    host.set_starting_player(starting)?;

    // CR 103.3: each player shuffles their deck, in stored turn order.
    for &p in &stored {
        shuffle_library(host, p)?;
    }

    // CR 103.4: starting life.
    let life = host.format().starting_life;
    for &p in &stored {
        host.set_player_life(p, life)?;
    }

    // CR 103.5: declaration order: the starting player, then the others in turn order.
    let start_index = stored.iter().position(|p| *p == starting).unwrap_or(0);
    let mut order: Vec<PlayerId> = stored[start_index..].to_vec();
    order.extend_from_slice(&stored[..start_index]);

    for &p in &order {
        let size = host.starting_hand_size(p);
        host.draw_cards(p, size, choices)?;
    }

    // CR 103.5: mulligan rounds.
    let free_first = host.format().mulligan.free_first_mulligan_multiplayer;
    let mut taken: BTreeMap<PlayerId, u64> = stored.iter().map(|p| (*p, 0)).collect();
    let mut deciding: Vec<PlayerId> = order.clone();
    while !deciding.is_empty() {
        let mut mulliganing: Vec<PlayerId> = Vec::new();
        let mut still: Vec<PlayerId> = Vec::new();
        for &p in &deciding {
            let taken_p = taken.get(&p).copied().unwrap_or(0);
            let c = mulligan_to_bottom(taken_p, player_count, free_first);
            let size = u64::from(host.starting_hand_size(p));
            let hand = host.hand(p);
            let hand_len = u64::try_from(hand.len()).unwrap_or(u64::MAX);
            if c >= size || c >= hand_len {
                // CR 103.5: the opening hand would be zero cards: no further mulligan; the
                // player keeps, bottoming the whole hand (ascending ids).
                host.put_on_library_bottom(p, &hand)?;
                continue;
            }
            match ask_mulligan(host, p, taken_p, c, choices)? {
                Some(bottom) => host.put_on_library_bottom(p, &bottom)?,
                None => {
                    mulliganing.push(p);
                    still.push(p);
                }
            }
        }
        // CR 103.5: all who decided to take a mulligan do so at the same time.
        for &p in &mulliganing {
            if let Some(t) = taken.get_mut(&p) {
                *t += 1;
            }
            host.return_hand_to_library(p)?;
            shuffle_library(host, p)?;
            let size = host.starting_hand_size(p);
            host.draw_cards(p, size, choices)?;
        }
        deciding = still;
    }

    // CR 103.6: pregame actions, starting player first.
    for &p in &order {
        let eligible = host.pregame_eligible(p);
        if !eligible.is_empty() {
            ask_pregame(host, p, eligible, choices)?;
        }
    }

    Ok(StartReport {
        starting_player: starting,
        mulligans: stored
            .iter()
            .map(|p| (*p, taken.get(p).copied().unwrap_or(0)))
            .collect(),
        opening_hands: stored.iter().map(|p| (*p, host.hand(*p))).collect(),
    })
}
