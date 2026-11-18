// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Events and triggers (REQ-DSL-006, SRS §14.4.1): the closed [`EventKind`] set, the
//! [`EventMatcher`] (which has no look-back member: the engine derives the flag, 603.10a–g), the
//! [`Trigger`] modes and [`StepId`].

use std::collections::BTreeMap;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;

use super::common::{check_no_lookback, object_of, put_opt};
use crate::expr::json::{
    AsMap, FromJson, JResult, Members, Obj, check_bound_var, check_free_str, check_int,
    deserialize_via_from_json, int_of, ser_check, str_of, tagged, wire_enum,
};
use crate::expr::{ObjectSelectorAST, PlayerGroup, PlayerSelectorAST, PredicateAST, Zone};

wire_enum! {
    /// The closed set of game events a trigger or replacement can match (REQ-DSL-006,
    /// SRS §14.4.1). It grows only by a DSL minor version.
    EventKind {
        /// `ZONE_CHANGE` (filters `from`, `to`).
        ZoneChange => "ZONE_CHANGE",
        /// `ENTERS`.
        Enters => "ENTERS",
        /// `DIES`.
        Dies => "DIES",
        /// `LEAVES_BATTLEFIELD`.
        LeavesBattlefield => "LEAVES_BATTLEFIELD",
        /// `PUT_INTO_GRAVEYARD`.
        PutIntoGraveyard => "PUT_INTO_GRAVEYARD",
        /// `LEAVES_GRAVEYARD`.
        LeavesGraveyard => "LEAVES_GRAVEYARD",
        /// `EXILED`.
        Exiled => "EXILED",
        /// `RETURNED_TO_HAND`.
        ReturnedToHand => "RETURNED_TO_HAND",
        /// `DRAWN`.
        Drawn => "DRAWN",
        /// `DISCARDED`.
        Discarded => "DISCARDED",
        /// `MILLED`.
        Milled => "MILLED",
        /// `CYCLED`.
        Cycled => "CYCLED",
        /// `SACRIFICED`.
        Sacrificed => "SACRIFICED",
        /// `DESTROYED`.
        Destroyed => "DESTROYED",
        /// `CAST`.
        Cast => "CAST",
        /// `COPIED`.
        Copied => "COPIED",
        /// `ACTIVATED`.
        Activated => "ACTIVATED",
        /// `TRIGGERED`.
        Triggered => "TRIGGERED",
        /// `RESOLVED`.
        Resolved => "RESOLVED",
        /// `COUNTERED`.
        Countered => "COUNTERED",
        /// `BECAME_TARGET`.
        BecameTarget => "BECAME_TARGET",
        /// `ATTACKS`.
        Attacks => "ATTACKS",
        /// `ATTACKS_ALONE`.
        AttacksAlone => "ATTACKS_ALONE",
        /// `BLOCKS`.
        Blocks => "BLOCKS",
        /// `BECOMES_BLOCKED`.
        BecomesBlocked => "BECOMES_BLOCKED",
        /// `ATTACKER_UNBLOCKED`.
        AttackerUnblocked => "ATTACKER_UNBLOCKED",
        /// `DAMAGE_DEALT` (filter `combat`).
        DamageDealt => "DAMAGE_DEALT",
        /// `DAMAGE_DEALT_BATCH`.
        DamageDealtBatch => "DAMAGE_DEALT_BATCH",
        /// `LIFE_GAINED`.
        LifeGained => "LIFE_GAINED",
        /// `LIFE_LOST`.
        LifeLost => "LIFE_LOST",
        /// `COUNTERS_PUT` (filter `kind`).
        CountersPut => "COUNTERS_PUT",
        /// `COUNTERS_REMOVED` (filter `kind`).
        CountersRemoved => "COUNTERS_REMOVED",
        /// `TAPPED`.
        Tapped => "TAPPED",
        /// `UNTAPPED`.
        Untapped => "UNTAPPED",
        /// `TRANSFORMED`.
        Transformed => "TRANSFORMED",
        /// `CONVERTED`.
        Converted => "CONVERTED",
        /// `TURNED_FACE_UP`.
        TurnedFaceUp => "TURNED_FACE_UP",
        /// `PHASED_OUT`.
        PhasedOut => "PHASED_OUT",
        /// `PHASED_IN`.
        PhasedIn => "PHASED_IN",
        /// `ATTACHED`.
        Attached => "ATTACHED",
        /// `UNATTACHED`.
        Unattached => "UNATTACHED",
        /// `CONTROL_CHANGED`.
        ControlChanged => "CONTROL_CHANGED",
        /// `TOKEN_CREATED`.
        TokenCreated => "TOKEN_CREATED",
        /// `SEARCHED_LIBRARY`.
        SearchedLibrary => "SEARCHED_LIBRARY",
        /// `SHUFFLED`.
        Shuffled => "SHUFFLED",
        /// `SCRY`.
        Scry => "SCRY",
        /// `SURVEIL`.
        Surveil => "SURVEIL",
        /// `EXPLORE`.
        Explore => "EXPLORE",
        /// `VENTURE`.
        Venture => "VENTURE",
        /// `BLIGHT`.
        Blight => "BLIGHT",
        /// `WATERBEND`.
        Waterbend => "WATERBEND",
        /// `DESIGNATION_GAINED` (filter `kind`).
        DesignationGained => "DESIGNATION_GAINED",
        /// `ROLLED_DIE`.
        RolledDie => "ROLLED_DIE",
        /// `FLIPPED_COIN`.
        FlippedCoin => "FLIPPED_COIN",
        /// `PLAYER_LOST`.
        PlayerLost => "PLAYER_LOST",
        /// `PLANESWALKED`.
        Planeswalked => "PLANESWALKED",
        /// `SPECIAL_ACTION_TAKEN` (filter `kind`).
        SpecialActionTaken => "SPECIAL_ACTION_TAKEN",
    }
}

wire_enum! {
    /// How simultaneous events are grouped (REQ-DSL-006): "Whenever a creature …" (`EACH`) vs
    /// "Whenever one or more creatures …" (`ONE_OR_MORE`).
    EventBatch {
        /// `EACH`: one trigger per event.
        Each => "EACH",
        /// `ONE_OR_MORE`: one trigger per batch.
        OneOrMore => "ONE_OR_MORE",
    }
}

wire_enum! {
    /// A step or main phase named by a `PHASE_STEP` trigger (REQ-DSL-006): "At the beginning
    /// of …".
    StepId {
        /// `UNTAP`.
        Untap => "UNTAP",
        /// `UPKEEP`.
        Upkeep => "UPKEEP",
        /// `DRAW`.
        Draw => "DRAW",
        /// `PRECOMBAT_MAIN`.
        PrecombatMain => "PRECOMBAT_MAIN",
        /// `BEGINNING_OF_COMBAT`.
        BeginningOfCombat => "BEGINNING_OF_COMBAT",
        /// `DECLARE_ATTACKERS`.
        DeclareAttackers => "DECLARE_ATTACKERS",
        /// `DECLARE_BLOCKERS`.
        DeclareBlockers => "DECLARE_BLOCKERS",
        /// `FIRST_STRIKE_DAMAGE`.
        FirstStrikeDamage => "FIRST_STRIKE_DAMAGE",
        /// `COMBAT_DAMAGE`.
        CombatDamage => "COMBAT_DAMAGE",
        /// `END_OF_COMBAT`.
        EndOfCombat => "END_OF_COMBAT",
        /// `POSTCOMBAT_MAIN`.
        PostcombatMain => "POSTCOMBAT_MAIN",
        /// `END`.
        End => "END",
        /// `CLEANUP`.
        Cleanup => "CLEANUP",
    }
}

/// Whose event an [`EventMatcher`] or replacement looks at (REQ-DSL-006): any object selector
/// (references included, so `{"ref":"CONTROLLER"}` is `Object`), or a player group
/// (`{"players":"ALL_PLAYERS"}`). Each value has exactly one spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventSubject {
    /// An object selector or a reference.
    Object(ObjectSelectorAST),
    /// `{"players": PlayerGroup}`.
    Players(PlayerGroup),
}

impl FromJson for EventSubject {
    fn from_json(v: &Value) -> JResult<Self> {
        if object_of(v, "event subject")?.contains_key("players") {
            let mut o = Obj::new(v, "event subject")?;
            let g = PlayerGroup::from_json(o.req("players")?)?;
            o.finish()?;
            Ok(EventSubject::Players(g))
        } else {
            Ok(EventSubject::Object(ObjectSelectorAST::from_json(v)?))
        }
    }
}

impl Serialize for EventSubject {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            EventSubject::Object(sel) => sel.serialize(s),
            EventSubject::Players(g) => tagged(s, "players", g),
        }
    }
}

impl From<PlayerGroup> for EventSubject {
    fn from(g: PlayerGroup) -> Self {
        EventSubject::Players(g)
    }
}

/// The value of one event filter or `MODIFY_EVENT` field (REQ-DSL-006, REQ-DSL-009). The JSON
/// type decides the variant: `true`/`false` → [`FilterValue::Bool`]; an integer →
/// [`FilterValue::Int`]; `"ANY"` → [`FilterValue::Any`]; a zone name → [`FilterValue::Zone`];
/// any other non-empty string → [`FilterValue::Name`]; an object → [`FilterValue::Predicate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterValue {
    /// A boolean, e.g. `"combat": true`.
    Bool(bool),
    /// A safe integer.
    Int(i64),
    /// The string `"ANY"`.
    Any,
    /// A zone name, e.g. `"to": "GRAVEYARD"`.
    Zone(Zone),
    /// Any other non-empty string (never `"ANY"` nor a zone name: see [`FilterValue::name`]).
    Name(String),
    /// A predicate, e.g. `"recipient": {"pred":"IS_PLAYER"}`.
    Predicate(PredicateAST),
}

impl FilterValue {
    /// A `Name` value (REQ-DSL-006). `"ANY"` and zone names have their own variants and an empty
    /// string or one with a noncharacter is not a name, so those are refused.
    ///
    /// # Errors
    /// A message naming the broken rule.
    pub fn name(text: impl Into<String>) -> Result<FilterValue, String> {
        let text = text.into();
        check_name(&text)?;
        Ok(FilterValue::Name(text))
    }
}

/// Validity of a `Name` filter value.
fn check_name(s: &str) -> JResult<()> {
    check_free_str(s, "filter name")?;
    if s == "ANY" || Zone::from_wire(s).is_some() {
        return Err(format!(
            "filter name {s:?} is spelled as {} instead",
            if s == "ANY" { "Any" } else { "Zone" }
        ));
    }
    Ok(())
}

impl FromJson for FilterValue {
    fn from_json(v: &Value) -> JResult<Self> {
        match v {
            Value::Bool(b) => Ok(FilterValue::Bool(*b)),
            Value::Number(_) => Ok(FilterValue::Int(int_of(v, "filter value")?)),
            Value::String(s) if s == "ANY" => Ok(FilterValue::Any),
            Value::String(s) => match Zone::from_wire(s) {
                Some(z) => Ok(FilterValue::Zone(z)),
                None => FilterValue::name(s.as_str()),
            },
            Value::Object(_) => Ok(FilterValue::Predicate(PredicateAST::from_json(v)?)),
            Value::Null | Value::Array(_) => Err(
                "a filter value must be a boolean, integer, string or predicate object".to_owned(),
            ),
        }
    }
}

impl Serialize for FilterValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            FilterValue::Bool(b) => s.serialize_bool(*b),
            FilterValue::Int(i) => {
                ser_check(check_int(*i, "filter value"))?;
                s.serialize_i64(*i)
            }
            FilterValue::Any => s.serialize_str("ANY"),
            FilterValue::Zone(z) => z.serialize(s),
            FilterValue::Name(n) => {
                ser_check(check_name(n))?;
                s.serialize_str(n)
            }
            FilterValue::Predicate(p) => p.serialize(s),
        }
    }
}

/// Validity of a `filters` key: non-empty and not a look-back name (E5).
fn check_filter_key(k: &str, what: &str) -> JResult<()> {
    check_free_str(k, what)?;
    check_no_lookback(k, what)
}

/// Decodes a `filters` (or `set`) object; `lookback` applies rule E5 to its keys.
pub(crate) fn filters_from(
    v: &Value,
    what: &str,
    lookback: bool,
) -> JResult<BTreeMap<String, FilterValue>> {
    let map = object_of(v, what)?;
    let mut out = BTreeMap::new();
    for (k, item) in map {
        if lookback {
            check_filter_key(k, what)?;
        } else {
            check_free_str(k, what)?;
        }
        out.insert(k.clone(), FilterValue::from_json(item)?);
    }
    Ok(out)
}

/// Validity of a decoded or hand-built filter map (keys only; values check themselves).
pub(crate) fn check_filters(
    filters: &BTreeMap<String, FilterValue>,
    what: &str,
    lookback: bool,
) -> JResult<()> {
    filters.keys().try_for_each(|k| {
        if lookback {
            check_filter_key(k, what)
        } else {
            check_free_str(k, what)
        }
    })
}

/// Validity of a `bind` map: keys as `filters` keys (E5), values valid `BOUND` variables.
fn check_bind(bind: &BTreeMap<String, String>) -> JResult<()> {
    bind.iter().try_for_each(|(k, var)| {
        check_filter_key(k, "bind")?;
        check_bound_var(var)
    })
}

/// What a trigger listens for (REQ-DSL-006, SRS §14.4.1). There is deliberately no look-back
/// member: the engine derives the flag from `kind` and `filters` (603.10a–g), and rule E5
/// refuses any `filters` or `bind` key that would spell one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventMatcher {
    /// The event.
    pub kind: EventKind,
    /// Whose event it is.
    pub subject: Option<EventSubject>,
    /// Field tests, by event field name (may be empty when present).
    pub filters: Option<BTreeMap<String, FilterValue>>,
    /// Event fields captured into `$vars` for the effect (may be empty when present).
    pub bind: Option<BTreeMap<String, String>>,
    /// Per-event or per-batch triggering.
    pub batch: Option<EventBatch>,
}

impl EventMatcher {
    /// A matcher with only `kind` (REQ-DSL-006); every optional member absent.
    #[must_use]
    pub fn new(kind: EventKind) -> EventMatcher {
        EventMatcher {
            kind,
            subject: None,
            filters: None,
            bind: None,
            batch: None,
        }
    }

    /// Checks E5 and the `bind` variable shape.
    fn check(&self) -> JResult<()> {
        if let Some(f) = &self.filters {
            check_filters(f, "filters", true)?;
        }
        if let Some(b) = &self.bind {
            check_bind(b)?;
        }
        Ok(())
    }
}

impl FromJson for EventMatcher {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "event matcher")?;
        let kind = EventKind::from_json(o.req("kind")?)?;
        let subject = o.opt("subject")?.map(EventSubject::from_json).transpose()?;
        let filters = o
            .opt("filters")?
            .map(|f| filters_from(f, "filters", true))
            .transpose()?;
        let bind = o
            .opt("bind")?
            .map(|b| {
                object_of(b, "bind")?
                    .iter()
                    .map(|(k, var)| Ok((k.clone(), str_of(var, "bind variable")?.to_owned())))
                    .collect::<JResult<BTreeMap<_, _>>>()
            })
            .transpose()?;
        let batch = o.opt("batch")?.map(EventBatch::from_json).transpose()?;
        o.finish()?;
        let out = EventMatcher {
            kind,
            subject,
            filters,
            bind,
            batch,
        };
        out.check()?;
        Ok(out)
    }
}

impl Members for EventMatcher {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("kind", &self.kind)?;
        put_opt(m, "subject", self.subject.as_ref())?;
        put_opt(m, "filters", self.filters.as_ref())?;
        put_opt(m, "bind", self.bind.as_ref())?;
        put_opt(m, "batch", self.batch.as_ref())
    }
}

impl Serialize for EventMatcher {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

/// When a triggered ability triggers (REQ-DSL-006, SRS §14.4.1): `{"mode":"EVENT","event"}`,
/// `{"mode":"PHASE_STEP","step","whose"}` or `{"mode":"STATE","state"}` (CR 603.8 state
/// trigger).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// "When/Whenever …".
    Event(EventMatcher),
    /// "At the beginning of …".
    PhaseStep {
        /// The step or main phase.
        step: StepId,
        /// Whose step.
        whose: PlayerSelectorAST,
    },
    /// A state trigger (CR 603.8).
    State(PredicateAST),
}

impl FromJson for Trigger {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "trigger")?;
        let out = match str_of(o.req("mode")?, "trigger mode")? {
            "EVENT" => Trigger::Event(EventMatcher::from_json(o.req("event")?)?),
            "PHASE_STEP" => Trigger::PhaseStep {
                step: StepId::from_json(o.req("step")?)?,
                whose: PlayerSelectorAST::from_json(o.req("whose")?)?,
            },
            "STATE" => Trigger::State(PredicateAST::from_json(o.req("state")?)?),
            other => return Err(format!("unknown trigger mode {other:?}")),
        };
        o.finish()?;
        Ok(out)
    }
}

impl Members for Trigger {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        match self {
            Trigger::Event(e) => {
                m.serialize_entry("mode", "EVENT")?;
                m.serialize_entry("event", e)
            }
            Trigger::PhaseStep { step, whose } => {
                m.serialize_entry("mode", "PHASE_STEP")?;
                m.serialize_entry("step", step)?;
                m.serialize_entry("whose", whose)
            }
            Trigger::State(p) => {
                m.serialize_entry("mode", "STATE")?;
                m.serialize_entry("state", p)
            }
        }
    }
}

impl Serialize for Trigger {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

deserialize_via_from_json!(EventSubject, FilterValue, EventMatcher, Trigger);
