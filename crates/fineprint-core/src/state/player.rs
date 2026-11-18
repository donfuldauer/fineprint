// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Per-player state: life, counters and designations (REQ-CORE-001, REQ-CORE-002).

use std::collections::BTreeMap;

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
/// rules here. The counter map never holds a zero amount.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlayerState {
    id: PlayerId,
    life: i64,
    counters: BTreeMap<String, u64>,
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

    /// The number of counters of `kind` on the player (0 if none).
    #[must_use]
    pub fn counter(&self, kind: &str) -> u64 {
        self.counters.get(kind).copied().unwrap_or(0)
    }

    /// Sets the number of counters of `kind` to `amount`; an amount of 0 removes the entry.
    pub fn set_counter(&mut self, kind: &str, amount: u64) {
        if amount == 0 {
            self.counters.remove(kind);
        } else {
            self.counters.insert(kind.to_owned(), amount);
        }
    }

    /// Every counter kind with a non-zero amount, in ascending kind order.
    #[must_use]
    pub fn counters(&self) -> &BTreeMap<String, u64> {
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

    /// Rebuilds a player from validated wire parts (the caller checked that no amount is 0).
    pub(super) fn from_parts(
        id: PlayerId,
        life: i64,
        counters: BTreeMap<String, u64>,
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
