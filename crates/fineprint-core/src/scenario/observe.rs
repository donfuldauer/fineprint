// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! What a [`ScenarioHost`](super::ScenarioHost) reports: plain, serde-able data (REQ-TEST-001).
//!
//! An [`Observed`] is the omniscient test view of a game (it includes hidden zones). It exists
//! for scenario assertions only and must never be handed to a client: client views are the
//! redacted per-player views (REQ-CORE-004, R12).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A pending choice as plain data (REQ-TEST-001).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ObservedChoice {
    /// The scenario id of the chooser.
    pub chooser: String,
    /// The SRS 15.3 choice type (`PRIORITY_ACTION`, ...).
    pub choice_type: String,
    /// The answers on offer, as names (for `PRIORITY_ACTION`: `PASS_PRIORITY` and the response
    /// name of every legal action).
    pub options: Vec<String>,
}

/// One observed event: a type name and its fields (REQ-TEST-001).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ObservedEvent {
    /// The event type name (`ZONE_CHANGE`, `PRIORITY_PASSED`, ...); `type` in JSON.
    #[serde(rename = "type")]
    pub event_type: String,
    /// The fields, by name; players and objects appear as their scenario ids.
    pub fields: BTreeMap<String, serde_json::Value>,
}

/// Evaluated characteristics of one object (REQ-TEST-001).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ObservedCharacteristics {
    /// Power, if the object has one.
    pub power: Option<i64>,
    /// Toughness, if the object has one.
    pub toughness: Option<i64>,
    /// Card types, sorted.
    pub types: Vec<String>,
    /// Subtypes, sorted.
    pub subtypes: Vec<String>,
    /// Supertypes, sorted.
    pub supertypes: Vec<String>,
    /// Colors as `W`, `U`, `B`, `R`, `G`, in that order.
    pub colors: Vec<String>,
    /// Ability keys: a keyword ability's keyword id (`FLYING`), any other ability its kind
    /// (`ACTIVATED`, `TRIGGERED`, `STATIC`, `SPELL`, `REPLACEMENT`); sorted, without
    /// duplicates.
    pub abilities: Vec<String>,
    /// The controller's scenario id, if the object has a controller.
    pub controller: Option<String>,
    /// The mana value.
    pub mana_value: Option<i64>,
}

/// The game result (REQ-TEST-001).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ObservedResult {
    /// `IN_PROGRESS`, `WON` or `DRAWN`.
    pub status: String,
    /// The winners' scenario ids.
    pub winners: Vec<String>,
    /// The losers' scenario ids.
    pub losers: Vec<String>,
}

impl Default for ObservedResult {
    fn default() -> ObservedResult {
        ObservedResult {
            status: "IN_PROGRESS".to_owned(),
            winners: Vec::new(),
            losers: Vec::new(),
        }
    }
}

/// The full observable state of a host plus every event emitted since `build` (REQ-TEST-001).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Observed {
    /// Evaluated characteristics by object id.
    pub characteristics: BTreeMap<String, ObservedCharacteristics>,
    /// Zone contents: key `<owner>/<zone>` for the per-player zones (for example `p1/hand`,
    /// `p1/exile`), or `battlefield`, `stack`, `sharedExile`; a host may add `command` for the
    /// whole command zone. Orders follow the setup conventions (library top first, graveyard and
    /// stack bottom first).
    pub zones: BTreeMap<String, Vec<String>>,
    /// Life totals by player id.
    pub life: BTreeMap<String, i64>,
    /// Counters by player or object id: kind to amount.
    pub counters: BTreeMap<String, BTreeMap<String, i64>>,
    /// Designations by player or object id: key to value (`true` for a flag, an integer or a
    /// string).
    pub designations: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    /// Every event since `build`, in order.
    pub events: Vec<ObservedEvent>,
    /// The pending choice, if any.
    pub pending: Option<ObservedChoice>,
    /// The game result.
    pub result: ObservedResult,
}
