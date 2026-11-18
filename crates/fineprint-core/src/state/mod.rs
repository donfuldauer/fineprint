// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The root game-state value `S_t` (REQ-CORE-001, REQ-CORE-002, REQ-NFR-010).
//!
//! A [`GameState`](crate::state::GameState) holds the players (N players from the start), the
//! turn position and turn order, the PRNG, one monotonic generator per identifier kind, and
//! placeholders for the effect registry and history ledger. All randomness comes from the
//! state's PRNG.
//!
//! **Snapshots.** Large components are held behind `Arc` and written copy-on-write with
//! `Arc::make_mut`, so `clone()` copies only small handle structures and a write to a clone
//! un-shares only the component it touches. Later units add their components (zones, objects)
//! the same way: an `Arc`-shared field, a `_mut` accessor through `Arc::make_mut`, a leaf
//! group in `GameState::merkle_tree` and a member of the wire struct.
//!
//! **Hash.** [`GameState::merkle_tree`](crate::state::GameState::merkle_tree) lays out one
//! Merkle leaf per component (a frozen contract, see the method). **Transport.**
//! [`GameState::serialize`](crate::state::GameState::serialize) writes MessagePack, which is
//! never a hash input (REQ-NFR-006).

mod error;
mod hash;
mod placeholders;
mod player;
mod turn;
mod wire;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::sync::Arc;

use fineprint_base::ids::{
    AbilityInstanceId, CardId, EffectId, IdGenerator, IdGenerators, LinkId, ObjectId, PlayerId,
    Timestamp,
};
use fineprint_base::prng::Pcg64Dxsm;

pub use error::StateError;
pub use placeholders::{EffectRegistry, HistoryLedger};
pub use player::{DesignationValue, PlayerState};
pub use turn::TurnInfo;

/// The largest number of players a game state holds (REQ-CORE-001, REQ-NFR-010).
pub const MAX_PLAYERS: usize = 256;

/// The version of the serialized state format written by [`GameState::serialize`].
pub const STATE_FORMAT_VERSION: u32 = 1;

/// The whole match state at one tick (REQ-CORE-001).
///
/// Cloning is a cheap snapshot with structural sharing: every [`PlayerState`] stays shared until
/// it is written through [`GameState::player_mut`]. Mutating one copy never affects another.
/// Equality compares values, never pointers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameState {
    prng: Pcg64Dxsm,
    ids: IdGenerators,
    turn: TurnInfo,
    turn_order: Arc<Vec<PlayerId>>,
    players: BTreeMap<PlayerId, Arc<PlayerState>>,
    effects: EffectRegistry,
    history: HistoryLedger,
}

impl GameState {
    /// A new pre-game state with `player_count` players, each at `starting_life`.
    ///
    /// The PRNG is `Pcg64Dxsm::from_u64_seed(seed)`. Players are issued from the state's own
    /// player generator, so their ids are `1..=player_count`, and the turn order is the seat
    /// order `[1, 2, ..., player_count]`.
    ///
    /// # Errors
    /// [`StateError::NoPlayers`] if `player_count` is 0, [`StateError::TooManyPlayers`] if it
    /// exceeds [`MAX_PLAYERS`].
    pub fn new(
        seed: u64,
        player_count: usize,
        starting_life: i64,
    ) -> Result<GameState, StateError> {
        if player_count == 0 {
            return Err(StateError::NoPlayers);
        }
        if player_count > MAX_PLAYERS {
            return Err(StateError::TooManyPlayers);
        }
        let mut state = GameState {
            prng: Pcg64Dxsm::from_u64_seed(seed),
            ids: IdGenerators::new(),
            turn: TurnInfo::default(),
            turn_order: Arc::new(Vec::new()),
            players: BTreeMap::new(),
            effects: EffectRegistry::default(),
            history: HistoryLedger::default(),
        };
        for _ in 0..player_count {
            state.add_player(starting_life)?;
        }
        Ok(state)
    }

    /// Adds a player at `starting_life`: issues the next [`PlayerId`] from the state's player
    /// generator and appends it to the end of the turn order.
    ///
    /// # Errors
    /// [`StateError::TooManyPlayers`] if the state already holds [`MAX_PLAYERS`] players,
    /// [`StateError::IdExhausted`] if the player generator is exhausted. The state is then
    /// unchanged.
    pub fn add_player(&mut self, starting_life: i64) -> Result<PlayerId, StateError> {
        if self.players.len() >= MAX_PLAYERS {
            return Err(StateError::TooManyPlayers);
        }
        let id = self
            .ids
            .player
            .issue()
            .map_err(|_| StateError::IdExhausted)?;
        self.players
            .insert(id, Arc::new(PlayerState::new(id, starting_life)));
        Arc::make_mut(&mut self.turn_order).push(id);
        Ok(id)
    }

    /// The number of players.
    #[must_use]
    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    /// The player with id `id`, if any.
    #[must_use]
    pub fn player(&self, id: PlayerId) -> Option<&PlayerState> {
        self.players.get(&id).map(|p| &**p)
    }

    /// Mutable access to the player with id `id`, if any. Copy-on-write: if the player's
    /// storage is shared with another snapshot, only this player is copied first.
    pub fn player_mut(&mut self, id: PlayerId) -> Option<&mut PlayerState> {
        self.players.get_mut(&id).map(Arc::make_mut)
    }

    /// Every player in ascending [`PlayerId`] order.
    pub fn players(&self) -> impl Iterator<Item = &PlayerState> + '_ {
        self.players.values().map(|p| &**p)
    }

    /// The turn order (REQ-NFR-010): every player id exactly once, in turn sequence.
    #[must_use]
    pub fn turn_order(&self) -> &[PlayerId] {
        &self.turn_order
    }

    /// Replaces the turn order (REQ-NFR-010).
    ///
    /// # Errors
    /// [`StateError::InvalidTurnOrder`] unless `order` is a permutation of exactly the current
    /// player ids (no duplicate, none missing, none unknown); the state is then unchanged.
    pub fn set_turn_order(&mut self, order: Vec<PlayerId>) -> Result<(), StateError> {
        if !is_permutation_of_keys(&order, &self.players) {
            return Err(StateError::InvalidTurnOrder);
        }
        self.turn_order = Arc::new(order);
        Ok(())
    }

    /// The turn position.
    #[must_use]
    pub fn turn(&self) -> &TurnInfo {
        &self.turn
    }

    /// Sets the turn number.
    pub fn set_turn_number(&mut self, n: u64) {
        self.turn.set_turn_number(n);
    }

    /// Sets the active player (`None` for none).
    ///
    /// # Errors
    /// [`StateError::UnknownPlayer`] if `player` names no player; the state is then unchanged.
    pub fn set_active_player(&mut self, player: Option<PlayerId>) -> Result<(), StateError> {
        self.check_player(player)?;
        self.turn.set_active_player(player);
        Ok(())
    }

    /// Sets the player who holds priority (`None` for none).
    ///
    /// # Errors
    /// [`StateError::UnknownPlayer`] if `player` names no player; the state is then unchanged.
    pub fn set_priority_player(&mut self, player: Option<PlayerId>) -> Result<(), StateError> {
        self.check_player(player)?;
        self.turn.set_priority_player(player);
        Ok(())
    }

    /// The state's PRNG (REQ-CORE-001).
    #[must_use]
    pub fn prng(&self) -> &Pcg64Dxsm {
        &self.prng
    }

    /// Mutable access to the state's PRNG; all randomness comes from it (REQ-CORE-001).
    pub fn prng_mut(&mut self) -> &mut Pcg64Dxsm {
        &mut self.prng
    }

    /// The identifier generators (read-only; REQ-CORE-002).
    #[must_use]
    pub fn ids(&self) -> &IdGenerators {
        &self.ids
    }

    /// Issues the next [`CardId`] (REQ-CORE-002).
    ///
    /// # Errors
    /// [`StateError::IdExhausted`] on exhaustion; the state is then unchanged.
    pub fn issue_card_id(&mut self) -> Result<CardId, StateError> {
        issue(&mut self.ids.card)
    }

    /// Issues the next [`ObjectId`] (REQ-CORE-002).
    ///
    /// # Errors
    /// [`StateError::IdExhausted`] on exhaustion; the state is then unchanged.
    pub fn issue_object_id(&mut self) -> Result<ObjectId, StateError> {
        issue(&mut self.ids.object)
    }

    /// Issues the next [`AbilityInstanceId`] (REQ-CORE-002).
    ///
    /// # Errors
    /// [`StateError::IdExhausted`] on exhaustion; the state is then unchanged.
    pub fn issue_ability_instance_id(&mut self) -> Result<AbilityInstanceId, StateError> {
        issue(&mut self.ids.ability_instance)
    }

    /// Issues the next [`EffectId`] (REQ-CORE-002).
    ///
    /// # Errors
    /// [`StateError::IdExhausted`] on exhaustion; the state is then unchanged.
    pub fn issue_effect_id(&mut self) -> Result<EffectId, StateError> {
        issue(&mut self.ids.effect)
    }

    /// Issues the next [`Timestamp`] from the single monotonic counter (REQ-CORE-002).
    ///
    /// # Errors
    /// [`StateError::IdExhausted`] on exhaustion; the state is then unchanged.
    pub fn issue_timestamp(&mut self) -> Result<Timestamp, StateError> {
        issue(&mut self.ids.timestamp)
    }

    /// Issues the next [`LinkId`] (REQ-CORE-002).
    ///
    /// # Errors
    /// [`StateError::IdExhausted`] on exhaustion; the state is then unchanged.
    pub fn issue_link_id(&mut self) -> Result<LinkId, StateError> {
        issue(&mut self.ids.link)
    }

    /// The effect registry (placeholder).
    #[must_use]
    pub fn effects(&self) -> &EffectRegistry {
        &self.effects
    }

    /// The history ledger (placeholder).
    #[must_use]
    pub fn history(&self) -> &HistoryLedger {
        &self.history
    }

    /// `true` iff both states hold player `id` and share its storage (the same `Arc`).
    /// This is how structural sharing between snapshots is observed.
    #[must_use]
    pub fn shares_player_with(&self, other: &GameState, id: PlayerId) -> bool {
        match (self.players.get(&id), other.players.get(&id)) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// `Ok` if `player` is `None` or names a player of this state.
    fn check_player(&self, player: Option<PlayerId>) -> Result<(), StateError> {
        match player {
            Some(id) if !self.players.contains_key(&id) => Err(StateError::UnknownPlayer(id)),
            _ => Ok(()),
        }
    }
}

/// Issues from `generator`, mapping exhaustion to [`StateError::IdExhausted`] (the generator
/// is unchanged on error).
fn issue<I: fineprint_base::ids::Id>(generator: &mut IdGenerator<I>) -> Result<I, StateError> {
    generator.issue().map_err(|_| StateError::IdExhausted)
}

/// `true` iff `order` holds every key of `players` exactly once and nothing else.
fn is_permutation_of_keys<V>(order: &[PlayerId], players: &BTreeMap<PlayerId, V>) -> bool {
    if order.len() != players.len() {
        return false;
    }
    let mut sorted = order.to_vec();
    sorted.sort_unstable();
    sorted.iter().eq(players.keys())
}
