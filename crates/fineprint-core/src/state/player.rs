// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Per-player state: life, counters and designations (REQ-CORE-001, REQ-CORE-002).

use std::collections::BTreeMap;
use std::ops::Deref;

use fineprint_base::encode::{CanonicalEncode, put_bytes, put_variant};
use fineprint_base::ids::PlayerId;

/// The value of a player designation (REQ-CORE-001).
///
/// Designations are an open, data-defined map keyed by a free string (SRS §3.1: new sets add
/// designations without engine changes); this type only carries the value.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DesignationValue {
    /// The designation is present and carries no value.
    Flag,
    /// An integer value.
    Int(i64),
    /// A text value.
    Text(String),
}

impl CanonicalEncode for DesignationValue {
    /// `Flag`: `0x00`; `Int(i)`: `0x01` then `i` as 8 bytes little-endian; `Text(s)`:
    /// `0x02` then [`put_bytes`] of the UTF-8 bytes.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            DesignationValue::Flag => put_variant(out, 0x00),
            DesignationValue::Int(i) => {
                put_variant(out, 0x01);
                i.encode_canonical(out);
            }
            DesignationValue::Text(s) => {
                put_variant(out, 0x02);
                put_bytes(out, s.as_bytes());
            }
        }
    }
}

/// The state of one player (REQ-CORE-001, REQ-CORE-002).
///
/// Counter kinds and designation keys are open-ended strings (SRS §10.2): no kind has built-in
/// rules here. The counter map never holds an amount below 1.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlayerState {
    id: PlayerId,
    life: i64,
    counters: BTreeMap<String, i64>,
    designations: BTreeMap<String, DesignationValue>,
}

impl PlayerState {
    /// A player with id `id`, life total `life`, no counters and no designations.
    #[must_use]
    pub fn new(id: PlayerId, life: i64) -> PlayerState {
        PlayerState {
            id,
            life,
            counters: BTreeMap::new(),
            designations: BTreeMap::new(),
        }
    }

    /// The player's id.
    #[must_use]
    pub fn id(&self) -> PlayerId {
        self.id
    }

    /// The player's life total.
    #[must_use]
    pub fn life(&self) -> i64 {
        self.life
    }

    /// Sets the player's life total.
    pub fn set_life(&mut self, life: i64) {
        self.life = life;
    }

    /// The number of counters of `kind` on the player (0 if none; never negative).
    #[must_use]
    pub fn counter(&self, kind: &str) -> i64 {
        self.counters.get(kind).copied().unwrap_or(0)
    }

    /// Sets the number of counters of `kind` to `amount` (REQ-CORE-006: signed 64-bit).
    ///
    /// An amount of 0 or below removes the entry: a count is never negative, so the map only
    /// ever holds amounts in `1..=i64::MAX`.
    pub fn set_counter(&mut self, kind: &str, amount: i64) {
        if amount <= 0 {
            self.counters.remove(kind);
        } else {
            self.counters.insert(kind.to_owned(), amount);
        }
    }

    /// Every counter kind with an amount of at least 1, in ascending kind order.
    #[must_use]
    pub fn counters(&self) -> &BTreeMap<String, i64> {
        &self.counters
    }

    /// The value of designation `key`, if the player has it.
    #[must_use]
    pub fn designation(&self, key: &str) -> Option<&DesignationValue> {
        self.designations.get(key)
    }

    /// Gives the player designation `key` with `value`, replacing any previous value.
    pub fn set_designation(&mut self, key: &str, value: DesignationValue) {
        self.designations.insert(key.to_owned(), value);
    }

    /// Removes designation `key`, returning its previous value.
    pub fn clear_designation(&mut self, key: &str) -> Option<DesignationValue> {
        self.designations.remove(key)
    }

    /// Every designation, in ascending key order.
    #[must_use]
    pub fn designations(&self) -> &BTreeMap<String, DesignationValue> {
        &self.designations
    }

    /// Rebuilds a player from validated wire parts (the caller checked that every amount is at
    /// least 1).
    pub(super) fn from_parts(
        id: PlayerId,
        life: i64,
        counters: BTreeMap<String, i64>,
        designations: BTreeMap<String, DesignationValue>,
    ) -> PlayerState {
        PlayerState {
            id,
            life,
            counters,
            designations,
        }
    }
}

/// Write access to one stored player of a `GameState` (REQ-CORE-001, REQ-CORE-002).
///
/// Returned by `GameState::player_mut`. It reads the player through [`Deref`] and changes it
/// only through the setters below, so the player's id can never be changed or the whole value
/// replaced: the stored player's [`PlayerState::id`] always equals its key in the state.
#[derive(Debug)]
pub struct PlayerMut<'a> {
    player: &'a mut PlayerState,
}

impl<'a> PlayerMut<'a> {
    /// Wraps the (already un-shared) stored player.
    pub(super) fn new(player: &'a mut PlayerState) -> PlayerMut<'a> {
        PlayerMut { player }
    }

    /// Sets the player's life total (see [`PlayerState::set_life`]).
    pub fn set_life(&mut self, life: i64) {
        self.player.set_life(life);
    }

    /// Sets the number of counters of `kind`; 0 or below removes the entry (see
    /// [`PlayerState::set_counter`]).
    pub fn set_counter(&mut self, kind: &str, amount: i64) {
        self.player.set_counter(kind, amount);
    }

    /// Gives the player designation `key` with `value` (see [`PlayerState::set_designation`]).
    pub fn set_designation(&mut self, key: &str, value: DesignationValue) {
        self.player.set_designation(key, value);
    }

    /// Removes designation `key`, returning its previous value (see
    /// [`PlayerState::clear_designation`]).
    pub fn clear_designation(&mut self, key: &str) -> Option<DesignationValue> {
        self.player.clear_designation(key)
    }
}

impl Deref for PlayerMut<'_> {
    type Target = PlayerState;

    /// Read-only access to the player.
    fn deref(&self) -> &PlayerState {
        self.player
    }
}
