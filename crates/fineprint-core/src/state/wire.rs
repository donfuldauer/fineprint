// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! MessagePack transport of a game state (REQ-CORE-001, REQ-NFR-006, REQ-CORE-006).
//!
//! The wire structs below are private; their member names are the wire contract. MessagePack
//! is transport only and never a hash input. Deserialization treats the bytes as untrusted
//! (R12): every failure is [`StateError::Decode`], nothing panics, and no length field read
//! from the input is used to preallocate.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;

use fineprint_base::ids::{IdGenerator, IdGenerators, PlayerId};
use fineprint_base::prng::{ALGORITHM_ID, Pcg64Dxsm};
use serde::{Deserialize, Serialize};

use super::effects_wire::{
    EffectsWire, GeneratorWire, Issued, TimestampsWire, effects_from_wire, generators_from_wire,
    generators_to_wire, ledger_from_wire,
};
use super::{
    DesignationValue, GameState, HistoryLedger, MAX_PLAYERS, PlayerState, STATE_FORMAT_VERSION,
    StateError, TurnInfo, is_permutation_of_keys,
};

/// The serialized game state (format version 2; version 1 is not read, CR-005).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StateWire {
    pub(super) format_version: u32,
    pub(super) prng_algorithm: String,
    pub(super) prng: PrngWire,
    pub(super) ids: IdsWire,
    pub(super) turn: TurnWire,
    pub(super) players: Vec<PlayerWire>,
    pub(super) effects: EffectsWire,
    pub(super) history: EmptyWire,
    pub(super) timestamps: TimestampsWire,
    pub(super) effect_generators: Vec<GeneratorWire>,
}

/// The PRNG's two `u128` words, each as `[hi, lo]` (rmp-serde's `u128` support is not
/// guaranteed across clients).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PrngWire {
    pub(super) state: [u64; 2],
    pub(super) increment: [u64; 2],
}

/// Each generator's last issued raw value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdsWire {
    pub(super) card: u64,
    pub(super) object: u64,
    pub(super) ability_instance: u64,
    pub(super) effect: u64,
    pub(super) timestamp: u64,
    pub(super) link: u64,
    pub(super) player: u64,
}

/// The turn position and turn order, player ids as raw `u64`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TurnWire {
    pub(super) number: u64,
    #[serde(deserialize_with = "required_option")]
    pub(super) active_player: Option<u64>,
    #[serde(deserialize_with = "required_option")]
    pub(super) priority_player: Option<u64>,
    pub(super) order: Vec<u64>,
}

/// Deserializes an `Option<u64>` member that must be present (serde would otherwise read a
/// missing `Option` member as `None`); `nil` is `None`.
fn required_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Option::<u64>::deserialize(d)
}

/// One player.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlayerWire {
    pub(super) id: u64,
    pub(super) life: i64,
    pub(super) counters: BTreeMap<String, i64>,
    pub(super) designations: BTreeMap<String, DesignationWire>,
}

/// A designation value (externally tagged: `"Flag"`, `{"Int": i}`, `{"Text": s}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum DesignationWire {
    Flag,
    Int(i64),
    Text(String),
}

/// An empty map: the placeholder history ledger.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmptyWire {}

/// Splits a `u128` into `[hi, lo]`.
fn split(v: u128) -> [u64; 2] {
    // Truncation is the intent: the high and low 64 bits.
    [(v >> 64) as u64, v as u64]
}

/// Joins `[hi, lo]` into a `u128`.
fn join(parts: [u64; 2]) -> u128 {
    (u128::from(parts[0]) << 64) | u128::from(parts[1])
}

/// A [`StateError::Decode`] with message `msg`.
fn bad(msg: &str) -> StateError {
    StateError::Decode(msg.to_owned())
}

impl From<&DesignationValue> for DesignationWire {
    fn from(v: &DesignationValue) -> Self {
        match v {
            DesignationValue::Flag => DesignationWire::Flag,
            DesignationValue::Int(i) => DesignationWire::Int(*i),
            DesignationValue::Text(s) => DesignationWire::Text(s.clone()),
        }
    }
}

impl From<DesignationWire> for DesignationValue {
    fn from(v: DesignationWire) -> Self {
        match v {
            DesignationWire::Flag => DesignationValue::Flag,
            DesignationWire::Int(i) => DesignationValue::Int(i),
            DesignationWire::Text(s) => DesignationValue::Text(s),
        }
    }
}

impl StateWire {
    /// The wire form of `state`.
    pub(super) fn from_state(state: &GameState) -> StateWire {
        let (prng_state, prng_increment) = state.prng.raw();
        let ids = &state.ids;
        StateWire {
            format_version: STATE_FORMAT_VERSION,
            prng_algorithm: ALGORITHM_ID.to_owned(),
            prng: PrngWire {
                state: split(prng_state),
                increment: split(prng_increment),
            },
            ids: IdsWire {
                card: ids.card.last_issued(),
                object: ids.object.last_issued(),
                ability_instance: ids.ability_instance.last_issued(),
                effect: ids.effect.last_issued(),
                timestamp: ids.timestamp.last_issued(),
                link: ids.link.last_issued(),
                player: ids.player.last_issued(),
            },
            turn: TurnWire {
                number: state.turn.turn_number(),
                active_player: state.turn.active_player().map(PlayerId::get),
                priority_player: state.turn.priority_player().map(PlayerId::get),
                order: state.turn_order.iter().map(|p| p.get()).collect(),
            },
            players: state
                .players()
                .map(|p| PlayerWire {
                    id: p.id().get(),
                    life: p.life(),
                    counters: p.counters().clone(),
                    designations: p
                        .designations()
                        .iter()
                        .map(|(k, v)| (k.clone(), DesignationWire::from(v)))
                        .collect(),
                })
                .collect(),
            effects: EffectsWire::from_effects(&state.effects),
            history: EmptyWire {},
            timestamps: TimestampsWire::from_ledger(&state.timestamps),
            effect_generators: generators_to_wire(&state.generators),
        }
    }

    /// Validates the wire value and builds the state it describes.
    ///
    /// # Errors
    /// [`StateError::Decode`] for any value that does not fully validate.
    pub(super) fn into_state(self) -> Result<GameState, StateError> {
        if self.format_version != STATE_FORMAT_VERSION {
            return Err(bad("unsupported format_version"));
        }
        if self.prng_algorithm != ALGORITHM_ID {
            return Err(bad("unsupported prng_algorithm"));
        }
        let prng = Pcg64Dxsm::from_raw(join(self.prng.state), join(self.prng.increment))
            .ok_or_else(|| bad("prng increment must be odd"))?;
        if self.players.is_empty() {
            return Err(bad("no players"));
        }
        if self.players.len() > MAX_PLAYERS {
            return Err(bad("too many players"));
        }
        let mut players: BTreeMap<PlayerId, Arc<PlayerState>> = BTreeMap::new();
        let mut previous = 0_u64;
        for p in self.players {
            if p.id == 0 {
                return Err(bad("player id 0"));
            }
            if p.id > self.ids.player {
                return Err(bad("player id beyond the player generator"));
            }
            if p.id <= previous {
                return Err(bad("players not in strictly ascending id order"));
            }
            previous = p.id;
            if p.counters.values().any(|&n| n <= 0) {
                return Err(bad("counter amount below 1"));
            }
            let id = PlayerId::new(p.id);
            let designations = p
                .designations
                .into_iter()
                .map(|(k, v)| (k, DesignationValue::from(v)))
                .collect();
            let player = PlayerState::from_parts(id, p.life, p.counters, designations);
            players.insert(id, Arc::new(player));
        }
        let order: Vec<PlayerId> = self.turn.order.into_iter().map(PlayerId::new).collect();
        if !is_permutation_of_keys(&order, &players) {
            return Err(bad("turn order is not a permutation of the player ids"));
        }
        let active_player = self.turn.active_player.map(PlayerId::new);
        let priority_player = self.turn.priority_player.map(PlayerId::new);
        for p in [active_player, priority_player].into_iter().flatten() {
            if !players.contains_key(&p) {
                return Err(bad("turn names an unknown player"));
            }
        }
        let ids = IdGenerators {
            card: IdGenerator::from_last_issued(self.ids.card),
            object: IdGenerator::from_last_issued(self.ids.object),
            ability_instance: IdGenerator::from_last_issued(self.ids.ability_instance),
            effect: IdGenerator::from_last_issued(self.ids.effect),
            timestamp: IdGenerator::from_last_issued(self.ids.timestamp),
            link: IdGenerator::from_last_issued(self.ids.link),
            player: IdGenerator::from_last_issued(self.ids.player),
        };
        let issued = Issued {
            effect: self.ids.effect,
            timestamp: self.ids.timestamp,
        };
        let effects = effects_from_wire(self.effects, issued)?;
        let timestamps = ledger_from_wire(self.timestamps, issued)?;
        let generators = generators_from_wire(self.effect_generators, &effects, issued)?;
        Ok(GameState {
            prng,
            ids,
            turn: TurnInfo::from_parts(self.turn.number, active_player, priority_player),
            turn_order: Arc::new(order),
            players,
            effects: Arc::new(effects),
            timestamps: Arc::new(timestamps),
            generators: Arc::new(generators),
            history: HistoryLedger::default(),
        })
    }

    /// MessagePack bytes of this wire value (structs as maps keyed by member name).
    pub(super) fn to_bytes(&self) -> Result<Vec<u8>, StateError> {
        rmp_serde::to_vec_named(self).map_err(|e| StateError::Encode(e.to_string()))
    }

    /// Decodes exactly one MessagePack value spanning all of `bytes`.
    pub(super) fn from_bytes(bytes: &[u8]) -> Result<StateWire, StateError> {
        let mut de = rmp_serde::Deserializer::new(Cursor::new(bytes));
        let wire =
            StateWire::deserialize(&mut de).map_err(|e| StateError::Decode(e.to_string()))?;
        let consumed = de.position();
        if usize::try_from(consumed).ok() != Some(bytes.len()) {
            return Err(bad("trailing bytes after the state"));
        }
        Ok(wire)
    }
}

impl GameState {
    /// The MessagePack transport form of this state (REQ-CORE-001, REQ-NFR-006): the same
    /// state always gives the same bytes. Never a hash input.
    ///
    /// # Errors
    /// [`StateError::Encode`] if the encoder fails.
    pub fn serialize(&self) -> Result<Vec<u8>, StateError> {
        StateWire::from_state(self).to_bytes()
    }

    /// Rebuilds a state from untrusted MessagePack bytes, validating everything (R12).
    ///
    /// # Errors
    /// [`StateError::Decode`] for malformed bytes, trailing bytes, an unsupported format
    /// version or PRNG algorithm, an even PRNG increment, 0 or more than [`MAX_PLAYERS`]
    /// players, a player id that is 0, repeated, out of ascending order or beyond the player
    /// generator, a turn order that is not a permutation of the player ids, an active or
    /// priority player that is not a player, a counter amount below 1 or above `i64::MAX`,
    /// unknown members, or wrong types; and for the layer components (format version 2,
    /// CR-005): effect ids out of ascending order or not issued by the effect generator,
    /// timestamps (of effects, granting effects, objects, counters and stickers) that are 0 or
    /// above the timestamp generator, an effect or group that registration refuses (a layer
    /// mismatch, an invalid text change, a bad `SameAsLayer` link, a resolved selector left
    /// unlocked in a group), a non-CDA effect in layer 7a (CR 613.4a), a static effect with a
    /// source without its generating ability, a generating ability for any other registered
    /// effect or on another source, and duplicate or unordered ledger or link entries.
    pub fn deserialize(bytes: &[u8]) -> Result<GameState, StateError> {
        StateWire::from_bytes(bytes)?.into_state()
    }
}
