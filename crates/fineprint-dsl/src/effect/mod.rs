// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Effect instructions of the card DSL (REQ-DSL-010, REQ-DSL-015, SRS §14.4.5, §14.8): the
//! closed [`Instruction`] set a resolving spell or ability runs, the [`DurationAST`] of its
//! effects and the [`NativeEffectAST`] escape hatch.
//!
//! The JSON form of every type is the FP-013 wire contract (docs/DECISIONS.md, "#14 FP-013 —
//! Wire catalogue as implemented"), built on the FP-011 rules: camelCase members; required
//! members present; optional members omitted when absent (a present `null` is an error, an
//! explicit `false` is kept); every unknown member is an error; and every `Deserialize` buffers
//! strictly first (duplicate members, non-integer or unsafe numbers, noncharacters and nesting
//! deeper than [`crate::canonical::MAX_DEPTH`] are errors). An instruction is an object whose
//! required `op` member (one of [`Instruction::NAMES`], exactly) selects the variant. Every
//! instruction list is non-empty wherever it appears.
//!
//! `Serialize` writes directly and re-checks the value-level invariants a hand-built value
//! could break, so for every value nested within the depth budget `decode(encode(x)) == x` and
//! the RFC 8785 form of a decoded document is the canonical form of the value. Nesting depth is
//! not re-checked on write: use [`crate::canonical::to_canonical_vec`] for output that must
//! reload.
//!
//! Loop safety: [`Instruction::RepeatWhile`] holds its iteration bound as a
//! [`std::num::NonZeroU32`], so no value and no document without `maxIter` exists.
//!
//! Nothing here is evaluated; the interpreter and the toolchain checks are other units.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;

use crate::ability::{CostAST, EventSubject, StaticEffect, TargetSlotAST, Trigger};
use crate::choice::ChoiceSpec;
use crate::expr::json::{
    AsMap, FromJson, JResult, Members, Obj, check_free_str, deserialize_via_from_json, free_str,
    ser_check, wire_enum,
};
use crate::expr::{ObjectSelectorAST, PlayerSelectorAST, PredicateAST, ValueExpressionAST, Zone};

mod decode;
mod duration;
mod encode;
mod native;
mod util;

#[cfg(test)]
mod tests;

pub use self::duration::DurationAST;
pub use self::native::NativeEffectAST;

use self::util::{check_opaque_map, opaque_map, put_opt};

wire_enum! {
    /// Where in a library an object goes (REQ-DSL-010): `TOP` or `BOTTOM`.
    Position {
        /// `TOP`.
        Top => "TOP",
        /// `BOTTOM`.
        Bottom => "BOTTOM",
    }
}

wire_enum! {
    /// What `TAP_OR_UNTAP` does (REQ-DSL-010): `TAP` or `UNTAP`.
    TapAction {
        /// `TAP`.
        Tap => "TAP",
        /// `UNTAP`.
        Untap => "UNTAP",
    }
}

wire_enum! {
    /// Where `REMEMBER` writes (REQ-DSL-010, SRS §14.4.5): the resolution `MEMORY` or the
    /// `LINKED` store.
    MemoryStore {
        /// `MEMORY`.
        Memory => "MEMORY",
        /// `LINKED`: the Linked Store.
        Linked => "LINKED",
    }
}

// ---------------------------------------------------------------------------------------------
// Auxiliary types

/// What `CREATE_TOKEN` creates (REQ-DSL-010): exactly one of the members `tokenDef` (a token
/// definition id, STR) and `inline` (an opaque object, may be empty). Standalone JSON:
/// `{"tokenDef":"…"}` or `{"inline":{…}}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenSource {
    /// `tokenDef`: a token definition id.
    Def(String),
    /// `inline`: an inline token definition, opaque here.
    Inline(BTreeMap<String, Value>),
}

impl TokenSource {
    /// Checks the member rules (non-empty id; opaque inline object).
    fn check(&self) -> JResult<()> {
        match self {
            TokenSource::Def(d) => check_free_str(d, "tokenDef"),
            TokenSource::Inline(map) => check_opaque_map(map, false, "inline token"),
        }
    }

    /// Consumes exactly one of `tokenDef` and `inline` from `o`.
    fn take_from(o: &mut Obj<'_>) -> JResult<Self> {
        match (o.opt("tokenDef")?, o.opt("inline")?) {
            (Some(d), None) => Ok(TokenSource::Def(free_str(d, "tokenDef")?)),
            (None, Some(i)) => Ok(TokenSource::Inline(opaque_map(i, false, "inline token")?)),
            _ => Err("CREATE_TOKEN needs exactly one of tokenDef and inline".to_owned()),
        }
    }
}

impl FromJson for TokenSource {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "token source")?;
        let out = TokenSource::take_from(&mut o)?;
        o.finish()?;
        Ok(out)
    }
}

impl Members for TokenSource {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        match self {
            TokenSource::Def(d) => m.serialize_entry("tokenDef", d),
            TokenSource::Inline(i) => m.serialize_entry("inline", i),
        }
    }
}

/// What `REMEMBER` stores (REQ-DSL-010): exactly one of the members `value` (a value
/// expression) and `objects` (an object selector). Standalone JSON: `{"value":…}` or
/// `{"objects":…}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RememberSource {
    /// `value`: an integer.
    Value(ValueExpressionAST),
    /// `objects`: a set of objects.
    Objects(ObjectSelectorAST),
}

impl RememberSource {
    /// Consumes exactly one of `value` and `objects` from `o`.
    fn take_from(o: &mut Obj<'_>) -> JResult<Self> {
        match (o.opt("value")?, o.opt("objects")?) {
            (Some(v), None) => Ok(RememberSource::Value(ValueExpressionAST::from_json(v)?)),
            (None, Some(s)) => Ok(RememberSource::Objects(ObjectSelectorAST::from_json(s)?)),
            _ => Err("REMEMBER needs exactly one of value and objects".to_owned()),
        }
    }
}

impl FromJson for RememberSource {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "remember source")?;
        let out = RememberSource::take_from(&mut o)?;
        o.finish()?;
        Ok(out)
    }
}

impl Members for RememberSource {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        match self {
            RememberSource::Value(v) => m.serialize_entry("value", v),
            RememberSource::Objects(s) => m.serialize_entry("objects", s),
        }
    }
}

/// The value `SET_DESIGNATION` sets (REQ-DSL-010): a JSON boolean ([`DesignationValue::Flag`])
/// or a non-empty string ([`DesignationValue::Name`]); the JSON type decides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesignationValue {
    /// A boolean designation (e.g. gained or lost).
    Flag(bool),
    /// A named value (STR).
    Name(String),
}

impl DesignationValue {
    /// Checks the string form.
    fn check(&self) -> JResult<()> {
        match self {
            DesignationValue::Flag(_) => Ok(()),
            DesignationValue::Name(n) => check_free_str(n, "designation value"),
        }
    }
}

impl FromJson for DesignationValue {
    fn from_json(v: &Value) -> JResult<Self> {
        match v {
            Value::Bool(b) => Ok(DesignationValue::Flag(*b)),
            Value::String(_) => Ok(DesignationValue::Name(free_str(v, "designation value")?)),
            _ => Err("designation value must be a boolean or a non-empty string".to_owned()),
        }
    }
}

impl Serialize for DesignationValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ser_check(self.check())?;
        match self {
            DesignationValue::Flag(b) => s.serialize_bool(*b),
            DesignationValue::Name(n) => s.serialize_str(n),
        }
    }
}

/// One mode of `CHOOSE_MODE` (REQ-DSL-010): `{"id":STR,"targets"?:[SLOT,…],"effect":INSTRS}`;
/// `targets` non-empty with distinct slots when present; `effect` non-empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstructionMode {
    /// Mode id (non-empty; distinct within its `CHOOSE_MODE`).
    pub id: String,
    /// The mode's own target slots.
    pub targets: Option<Vec<TargetSlotAST>>,
    /// The mode's instructions (non-empty).
    pub effect: Vec<Instruction>,
}

impl InstructionMode {
    /// Checks the mode rules (id, targets, non-empty effect).
    fn check(&self) -> JResult<()> {
        check_free_str(&self.id, "mode id")?;
        if let Some(t) = &self.targets {
            util::check_non_empty(t, "mode targets")?;
            for (i, slot) in t.iter().enumerate() {
                if t[..i].iter().any(|s| s.slot == slot.slot) {
                    return Err(format!("target slot {} is listed twice", slot.slot));
                }
            }
        }
        util::check_non_empty(&self.effect, "mode effect")
    }
}

impl FromJson for InstructionMode {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "mode")?;
        let out = InstructionMode {
            id: free_str(o.req("id")?, "mode id")?,
            targets: o
                .opt("targets")?
                .map(|t| util::non_empty_vec(t, "mode targets"))
                .transpose()?,
            effect: instr_list(o.req("effect")?, "mode effect")?,
        };
        o.finish()?;
        out.check()?;
        Ok(out)
    }
}

impl Members for InstructionMode {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("id", &self.id)?;
        put_opt(m, "targets", self.targets.as_ref())?;
        m.serialize_entry("effect", &self.effect)
    }
}

/// Decodes a non-empty instruction list (rule A4). A plain loop keeps the per-level stack cost
/// of deep instruction trees small.
fn instr_list(v: &Value, what: &str) -> JResult<Vec<Instruction>> {
    let items = crate::expr::json::array_of(v, what)?;
    if items.is_empty() {
        return Err(format!("{what} must not be empty"));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(Instruction::from_json(item)?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Instruction

/// One effect instruction (REQ-DSL-010, SRS §14.4.5): a closed set of 72 operations, one
/// variant per `op` (see [`Instruction::NAMES`]). The set grows only by a DSL minor version.
/// Every instruction that involves a decision carries an optional `choice` ([`ChoiceSpec`],
/// REQ-DSL-011).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Instruction {
    // --- Zones ---
    /// `DRAW{player, count}`.
    Draw {
        /// Who draws.
        player: PlayerSelectorAST,
        /// How many.
        count: ValueExpressionAST,
    },
    /// `MILL{player, count}`.
    Mill {
        /// Who mills.
        player: PlayerSelectorAST,
        /// How many.
        count: ValueExpressionAST,
    },
    /// `DISCARD{player, count, choice?}`.
    Discard {
        /// Who discards.
        player: PlayerSelectorAST,
        /// How many.
        count: ValueExpressionAST,
        /// The decision.
        choice: Option<ChoiceSpec>,
    },
    /// `MOVE{objects, toZone, position?, faceDown?, tapped?, underControlOf?, transformed?,
    /// choice?}`.
    Move {
        /// What moves.
        objects: ObjectSelectorAST,
        /// Destination zone (JSON `toZone`).
        to_zone: Zone,
        /// Library position.
        position: Option<Position>,
        /// Enters face down (JSON `faceDown`).
        face_down: Option<bool>,
        /// Enters tapped.
        tapped: Option<bool>,
        /// Enters under this player's control (JSON `underControlOf`).
        under_control_of: Option<PlayerSelectorAST>,
        /// Enters transformed.
        transformed: Option<bool>,
        /// The decision.
        choice: Option<ChoiceSpec>,
    },
    /// `SEARCH_LIBRARY{player, selector, count, toZone, position?, choice?}`.
    SearchLibrary {
        /// Who searches.
        player: PlayerSelectorAST,
        /// What may be found.
        selector: ObjectSelectorAST,
        /// How many.
        count: ValueExpressionAST,
        /// Where the found objects go (JSON `toZone`).
        to_zone: Zone,
        /// Library position.
        position: Option<Position>,
        /// The decision.
        choice: Option<ChoiceSpec>,
    },
    /// `SHUFFLE{player}`.
    Shuffle {
        /// Whose library.
        player: PlayerSelectorAST,
    },
    /// `REVEAL{objects, to?, choice?}`.
    Reveal {
        /// What is revealed.
        objects: ObjectSelectorAST,
        /// Revealed to (default: all players).
        to: Option<PlayerSelectorAST>,
        /// The decision.
        choice: Option<ChoiceSpec>,
    },
    /// `LOOK_AT{objects, player}`.
    LookAt {
        /// What is looked at.
        objects: ObjectSelectorAST,
        /// Who looks.
        player: PlayerSelectorAST,
    },
    /// `PUT_ON_TOP_OR_BOTTOM{objects, player, choice?}`.
    PutOnTopOrBottom {
        /// What is put.
        objects: ObjectSelectorAST,
        /// Who decides.
        player: PlayerSelectorAST,
        /// The decision.
        choice: Option<ChoiceSpec>,
    },
    /// `EXILE_UNTIL{objects, duration, faceDown?}`.
    ExileUntil {
        /// What is exiled.
        objects: ObjectSelectorAST,
        /// Until when.
        duration: DurationAST,
        /// Exiled face down (JSON `faceDown`).
        face_down: Option<bool>,
    },
    /// `PLAY_PERMISSION_GRANT{objects, duration, who, alsoAs?}`.
    PlayPermissionGrant {
        /// What may be played.
        objects: ObjectSelectorAST,
        /// For how long.
        duration: DurationAST,
        /// Who may play them.
        who: PlayerSelectorAST,
        /// "As though it were …" (JSON `alsoAs`, STR).
        also_as: Option<String>,
    },

    // --- Damage and life ---
    /// `DEAL_DAMAGE{source, recipients, amount, divided?, choice?}`.
    DealDamage {
        /// The damage source.
        source: ObjectSelectorAST,
        /// Who is dealt damage.
        recipients: EventSubject,
        /// How much.
        amount: ValueExpressionAST,
        /// Divided as the controller chooses.
        divided: Option<bool>,
        /// The decision.
        choice: Option<ChoiceSpec>,
    },
    /// `FIGHT{first, second}`.
    Fight {
        /// One fighter.
        first: ObjectSelectorAST,
        /// The other fighter.
        second: ObjectSelectorAST,
    },
    /// `BITE{source, target}`.
    Bite {
        /// Deals the damage.
        source: ObjectSelectorAST,
        /// Is dealt the damage.
        target: ObjectSelectorAST,
    },
    /// `GAIN_LIFE{player, amount}`.
    GainLife {
        /// Who gains.
        player: PlayerSelectorAST,
        /// How much.
        amount: ValueExpressionAST,
    },
    /// `LOSE_LIFE{player, amount}`.
    LoseLife {
        /// Who loses.
        player: PlayerSelectorAST,
        /// How much.
        amount: ValueExpressionAST,
    },
    /// `SET_LIFE{player, amount}`.
    SetLife {
        /// Whose life total.
        player: PlayerSelectorAST,
        /// The new total.
        amount: ValueExpressionAST,
    },
    /// `PREVENT{duration, source?, recipients?, amount?, combatOnly?}`.
    Prevent {
        /// How long the prevention lasts.
        duration: DurationAST,
        /// Damage from this source only.
        source: Option<ObjectSelectorAST>,
        /// Damage to these only.
        recipients: Option<EventSubject>,
        /// Prevent this much (absent: all).
        amount: Option<ValueExpressionAST>,
        /// Combat damage only (JSON `combatOnly`).
        combat_only: Option<bool>,
    },
    /// `DESTROY{objects, noRegeneration?}`.
    Destroy {
        /// What is destroyed.
        objects: ObjectSelectorAST,
        /// "It can't be regenerated" (JSON `noRegeneration`).
        no_regeneration: Option<bool>,
    },
    /// `SACRIFICE{player, selector, count, choice?}`.
    Sacrifice {
        /// Who sacrifices.
        player: PlayerSelectorAST,
        /// What may be sacrificed.
        selector: ObjectSelectorAST,
        /// How many.
        count: ValueExpressionAST,
        /// The decision.
        choice: Option<ChoiceSpec>,
    },
    /// `REGENERATE{objects}`.
    Regenerate {
        /// What gets a regeneration shield.
        objects: ObjectSelectorAST,
    },
    /// `COUNTER_SPELL{objects, unless?}`.
    CounterSpell {
        /// What is countered.
        objects: ObjectSelectorAST,
        /// "… unless its controller pays …" (non-empty when present).
        unless: Option<Vec<CostAST>>,
    },

    // --- Counters and tokens ---
    /// `PUT_COUNTERS{objects, kind, count}`.
    PutCounters {
        /// Where the counters go.
        objects: ObjectSelectorAST,
        /// Counter kind (STR).
        kind: String,
        /// How many.
        count: ValueExpressionAST,
    },
    /// `REMOVE_COUNTERS{objects, kind, count}`.
    RemoveCounters {
        /// Whose counters.
        objects: ObjectSelectorAST,
        /// Counter kind (STR).
        kind: String,
        /// How many.
        count: ValueExpressionAST,
    },
    /// `MOVE_COUNTERS{from, to, kind, count}`.
    MoveCounters {
        /// Taken from.
        from: ObjectSelectorAST,
        /// Put on.
        to: ObjectSelectorAST,
        /// Counter kind (STR).
        kind: String,
        /// How many.
        count: ValueExpressionAST,
    },
    /// `PROLIFERATE{choice?}`.
    Proliferate {
        /// The decision.
        choice: Option<ChoiceSpec>,
    },
    /// `CREATE_TOKEN{tokenDef | inline, count, modifiers?, controller?}`.
    CreateToken {
        /// Exactly one of `tokenDef` and `inline`.
        token: TokenSource,
        /// How many.
        count: ValueExpressionAST,
        /// Modifiers (a non-empty opaque object).
        modifiers: Option<BTreeMap<String, Value>>,
        /// Created under this player's control.
        controller: Option<PlayerSelectorAST>,
    },
    /// `CREATE_PREDEFINED_TOKEN{kind, count?}`.
    CreatePredefinedToken {
        /// The predefined token kind (STR).
        kind: String,
        /// How many (absent: one).
        count: Option<ValueExpressionAST>,
    },
    /// `CREATE_COPY_TOKEN{of, except?}`.
    CreateCopyToken {
        /// What is copied.
        of: ObjectSelectorAST,
        /// Copy exceptions (a non-empty opaque object).
        except: Option<BTreeMap<String, Value>>,
    },
    /// `POPULATE`.
    Populate,
    /// `AMASS{n}`.
    Amass {
        /// The amount.
        n: ValueExpressionAST,
    },
    /// `INCUBATE{n}`.
    Incubate {
        /// The amount.
        n: ValueExpressionAST,
    },

    // --- Stack ---
    /// `COPY_SPELL_OR_ABILITY{objects, newTargets?}`.
    CopySpellOrAbility {
        /// What is copied.
        objects: ObjectSelectorAST,
        /// "You may choose new targets" (JSON `newTargets`).
        new_targets: Option<bool>,
    },
    /// `CHANGE_TARGETS{objects, choice?}`.
    ChangeTargets {
        /// Whose targets change.
        objects: ObjectSelectorAST,
        /// The decision.
        choice: Option<ChoiceSpec>,
    },
    /// `CAST_FROM{objects, withoutPaying?, altCost?}`.
    CastFrom {
        /// What may be cast.
        objects: ObjectSelectorAST,
        /// Without paying its mana cost (JSON `withoutPaying`).
        without_paying: Option<bool>,
        /// An alternative cost (JSON `altCost`, non-empty when present).
        alt_cost: Option<Vec<CostAST>>,
    },

    // --- State and designations ---
    /// `TAP_OR_UNTAP{objects, action}`.
    TapOrUntap {
        /// What is tapped or untapped.
        objects: ObjectSelectorAST,
        /// Tap or untap.
        action: TapAction,
    },
    /// `TRANSFORM{objects}`.
    Transform {
        /// What transforms.
        objects: ObjectSelectorAST,
    },
    /// `CONVERT{objects}`.
    Convert {
        /// What converts.
        objects: ObjectSelectorAST,
    },
    /// `TURN_FACE_UP{objects, profile?}`.
    TurnFaceUp {
        /// What is turned.
        objects: ObjectSelectorAST,
        /// The face-down profile (STR).
        profile: Option<String>,
    },
    /// `TURN_FACE_DOWN{objects, profile?}`.
    TurnFaceDown {
        /// What is turned.
        objects: ObjectSelectorAST,
        /// The face-down profile (STR).
        profile: Option<String>,
    },
    /// `PHASE_OUT{objects}`.
    PhaseOut {
        /// What phases out.
        objects: ObjectSelectorAST,
    },
    /// `ATTACH{objects, to}`.
    Attach {
        /// What is attached.
        objects: ObjectSelectorAST,
        /// Attached to.
        to: ObjectSelectorAST,
    },
    /// `GAIN_CONTROL{objects, duration, player?}`.
    GainControl {
        /// What changes control.
        objects: ObjectSelectorAST,
        /// For how long.
        duration: DurationAST,
        /// The new controller (absent: the resolving object's controller).
        player: Option<PlayerSelectorAST>,
    },
    /// `EXCHANGE_CONTROL{first, second}`.
    ExchangeControl {
        /// One side.
        first: ObjectSelectorAST,
        /// The other side.
        second: ObjectSelectorAST,
    },
    /// `GOAD{objects}`.
    Goad {
        /// What is goaded.
        objects: ObjectSelectorAST,
    },
    /// `SET_DESIGNATION{kind, value, to?}` (monarch, initiative, day/night, …).
    SetDesignation {
        /// The designation (STR).
        kind: String,
        /// Its value.
        value: DesignationValue,
        /// Who or what receives it.
        to: Option<EventSubject>,
    },
    /// `VENTURE`.
    Venture,
    /// `RING_TEMPTS`.
    RingTempts,
    /// `INCREASE_SPEED{player, amount?}`.
    IncreaseSpeed {
        /// Whose speed.
        player: PlayerSelectorAST,
        /// By how much (absent: one).
        amount: Option<ValueExpressionAST>,
    },
    /// `EMPOWER_JACE{n}`.
    EmpowerJace {
        /// The amount.
        n: ValueExpressionAST,
    },
    /// `MELD{objects}`.
    Meld {
        /// What melds.
        objects: ObjectSelectorAST,
    },
    /// `MUTATE_MERGE{objects, onto}`.
    MutateMerge {
        /// What merges.
        objects: ObjectSelectorAST,
        /// Merged onto.
        onto: ObjectSelectorAST,
    },
    /// `MANIFEST{objects}`.
    Manifest {
        /// What is manifested.
        objects: ObjectSelectorAST,
    },
    /// `CLOAK{objects}`.
    Cloak {
        /// What is cloaked.
        objects: ObjectSelectorAST,
    },
    /// `EXPLORE{objects}`.
    Explore {
        /// What explores.
        objects: ObjectSelectorAST,
    },
    /// `CONNIVE{objects}`.
    Connive {
        /// What connives.
        objects: ObjectSelectorAST,
    },
    /// `DISCOVER{n}`.
    Discover {
        /// The amount.
        n: ValueExpressionAST,
    },
    /// `BLIGHT{n}`.
    Blight {
        /// The amount.
        n: ValueExpressionAST,
    },
    /// `WATERBEND{cost}`.
    Waterbend {
        /// The cost (non-empty).
        cost: Vec<CostAST>,
    },

    // --- Continuous effects ---
    /// `CREATE_CONTINUOUS_EFFECT{effects, duration, lockAffectedSet?}`. An absent
    /// `lockAffectedSet` means true (CR 611.2c); absent, true and false are three distinct
    /// values (see [`Instruction::locks_affected_set`]).
    CreateContinuousEffect {
        /// The effects (non-empty).
        effects: Vec<StaticEffect>,
        /// How long they last.
        duration: DurationAST,
        /// Whether the affected set is fixed when the effect begins (JSON `lockAffectedSet`).
        lock_affected_set: Option<bool>,
    },

    // --- Delayed and reflexive triggers ---
    /// `CREATE_DELAYED_TRIGGER{trigger, effect, captures?, oneShot}` (CR 603.7).
    CreateDelayedTrigger {
        /// When it triggers.
        trigger: Trigger,
        /// What it does (non-empty).
        effect: Vec<Instruction>,
        /// Objects captured now and bound to `$` variables (non-empty when present).
        captures: Option<BTreeMap<String, ObjectSelectorAST>>,
        /// Whether it triggers only once (JSON `oneShot`).
        one_shot: bool,
    },
    /// `REFLEXIVE_TRIGGER{ifActionTaken, effect}` (CR 603.12).
    ReflexiveTrigger {
        /// The action whose performance triggers it (JSON `ifActionTaken`, non-empty).
        if_action_taken: Vec<Instruction>,
        /// What it does (non-empty).
        effect: Vec<Instruction>,
    },

    // --- Control flow ---
    /// `IF{cond, then, else?}`.
    If {
        /// The condition.
        cond: PredicateAST,
        /// Run when it holds (non-empty).
        then: Vec<Instruction>,
        /// Run otherwise (JSON `else`, non-empty when present).
        otherwise: Option<Vec<Instruction>>,
    },
    /// `FOR_EACH{iterator, bind?, body}`.
    ForEach {
        /// What is iterated.
        iterator: EventSubject,
        /// The variable bound to each element (`$name`).
        bind: Option<String>,
        /// Run per element (non-empty).
        body: Vec<Instruction>,
    },
    /// `REPEAT{times, body}`.
    Repeat {
        /// How many times.
        times: ValueExpressionAST,
        /// Run each time (non-empty).
        body: Vec<Instruction>,
    },
    /// `REPEAT_WHILE{cond, maxIter, body}`: exceeding `max_iter` is an engine fault, never a
    /// silent truncation.
    RepeatWhile {
        /// Loop while this holds.
        cond: PredicateAST,
        /// The iteration bound (JSON `maxIter`, 1..=4294967295; required).
        max_iter: NonZeroU32,
        /// Run each time (non-empty).
        body: Vec<Instruction>,
    },
    /// `MAY{player, body}`.
    May {
        /// Who decides.
        player: PlayerSelectorAST,
        /// Run if they do (non-empty).
        body: Vec<Instruction>,
    },
    /// `UNLESS{payer, cost, body}`: "[A] unless [B]", mandatory for loop detection (CR 733.6).
    Unless {
        /// Who may pay.
        payer: PlayerSelectorAST,
        /// The cost (non-empty).
        cost: Vec<CostAST>,
        /// Run unless the cost is paid (non-empty).
        body: Vec<Instruction>,
    },
    /// `CHOOSE_MODE{chooser, min, max, modes}`.
    ChooseMode {
        /// Who chooses.
        chooser: PlayerSelectorAST,
        /// Least number of modes.
        min: u32,
        /// Greatest number of modes (`min <= max <= modes.len()`).
        max: u32,
        /// The modes (non-empty, distinct ids).
        modes: Vec<InstructionMode>,
    },
    /// `VOTE{voters, choice, store}`.
    Vote {
        /// Who votes.
        voters: PlayerSelectorAST,
        /// What each voter chooses.
        choice: ChoiceSpec,
        /// Where the result is stored (STR).
        store: String,
    },
    /// `REMEMBER{key, into, value | objects}`.
    Remember {
        /// Storage key (STR).
        key: String,
        /// Memory or the Linked Store.
        into: MemoryStore,
        /// Exactly one of `value` and `objects`.
        source: RememberSource,
    },

    // --- Native ---
    /// `NATIVE_EFFECT` (REQ-DSL-015): members flat beside `op`.
    NativeEffect(NativeEffectAST),
}

impl Instruction {
    /// Every `op` string, in catalogue order (REQ-DSL-010).
    pub const NAMES: [&'static str; 72] = [
        "DRAW",
        "MILL",
        "DISCARD",
        "MOVE",
        "SEARCH_LIBRARY",
        "SHUFFLE",
        "REVEAL",
        "LOOK_AT",
        "PUT_ON_TOP_OR_BOTTOM",
        "EXILE_UNTIL",
        "PLAY_PERMISSION_GRANT",
        "DEAL_DAMAGE",
        "FIGHT",
        "BITE",
        "GAIN_LIFE",
        "LOSE_LIFE",
        "SET_LIFE",
        "PREVENT",
        "DESTROY",
        "SACRIFICE",
        "REGENERATE",
        "COUNTER_SPELL",
        "PUT_COUNTERS",
        "REMOVE_COUNTERS",
        "MOVE_COUNTERS",
        "PROLIFERATE",
        "CREATE_TOKEN",
        "CREATE_PREDEFINED_TOKEN",
        "CREATE_COPY_TOKEN",
        "POPULATE",
        "AMASS",
        "INCUBATE",
        "COPY_SPELL_OR_ABILITY",
        "CHANGE_TARGETS",
        "CAST_FROM",
        "TAP_OR_UNTAP",
        "TRANSFORM",
        "CONVERT",
        "TURN_FACE_UP",
        "TURN_FACE_DOWN",
        "PHASE_OUT",
        "ATTACH",
        "GAIN_CONTROL",
        "EXCHANGE_CONTROL",
        "GOAD",
        "SET_DESIGNATION",
        "VENTURE",
        "RING_TEMPTS",
        "INCREASE_SPEED",
        "EMPOWER_JACE",
        "MELD",
        "MUTATE_MERGE",
        "MANIFEST",
        "CLOAK",
        "EXPLORE",
        "CONNIVE",
        "DISCOVER",
        "BLIGHT",
        "WATERBEND",
        "CREATE_CONTINUOUS_EFFECT",
        "CREATE_DELAYED_TRIGGER",
        "REFLEXIVE_TRIGGER",
        "IF",
        "FOR_EACH",
        "REPEAT",
        "REPEAT_WHILE",
        "MAY",
        "UNLESS",
        "CHOOSE_MODE",
        "VOTE",
        "REMEMBER",
        "NATIVE_EFFECT",
    ];

    /// The `op` string of this instruction, e.g. `"DEAL_DAMAGE"` (REQ-DSL-010).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Instruction::Draw { .. } => "DRAW",
            Instruction::Mill { .. } => "MILL",
            Instruction::Discard { .. } => "DISCARD",
            Instruction::Move { .. } => "MOVE",
            Instruction::SearchLibrary { .. } => "SEARCH_LIBRARY",
            Instruction::Shuffle { .. } => "SHUFFLE",
            Instruction::Reveal { .. } => "REVEAL",
            Instruction::LookAt { .. } => "LOOK_AT",
            Instruction::PutOnTopOrBottom { .. } => "PUT_ON_TOP_OR_BOTTOM",
            Instruction::ExileUntil { .. } => "EXILE_UNTIL",
            Instruction::PlayPermissionGrant { .. } => "PLAY_PERMISSION_GRANT",
            Instruction::DealDamage { .. } => "DEAL_DAMAGE",
            Instruction::Fight { .. } => "FIGHT",
            Instruction::Bite { .. } => "BITE",
            Instruction::GainLife { .. } => "GAIN_LIFE",
            Instruction::LoseLife { .. } => "LOSE_LIFE",
            Instruction::SetLife { .. } => "SET_LIFE",
            Instruction::Prevent { .. } => "PREVENT",
            Instruction::Destroy { .. } => "DESTROY",
            Instruction::Sacrifice { .. } => "SACRIFICE",
            Instruction::Regenerate { .. } => "REGENERATE",
            Instruction::CounterSpell { .. } => "COUNTER_SPELL",
            Instruction::PutCounters { .. } => "PUT_COUNTERS",
            Instruction::RemoveCounters { .. } => "REMOVE_COUNTERS",
            Instruction::MoveCounters { .. } => "MOVE_COUNTERS",
            Instruction::Proliferate { .. } => "PROLIFERATE",
            Instruction::CreateToken { .. } => "CREATE_TOKEN",
            Instruction::CreatePredefinedToken { .. } => "CREATE_PREDEFINED_TOKEN",
            Instruction::CreateCopyToken { .. } => "CREATE_COPY_TOKEN",
            Instruction::Populate => "POPULATE",
            Instruction::Amass { .. } => "AMASS",
            Instruction::Incubate { .. } => "INCUBATE",
            Instruction::CopySpellOrAbility { .. } => "COPY_SPELL_OR_ABILITY",
            Instruction::ChangeTargets { .. } => "CHANGE_TARGETS",
            Instruction::CastFrom { .. } => "CAST_FROM",
            Instruction::TapOrUntap { .. } => "TAP_OR_UNTAP",
            Instruction::Transform { .. } => "TRANSFORM",
            Instruction::Convert { .. } => "CONVERT",
            Instruction::TurnFaceUp { .. } => "TURN_FACE_UP",
            Instruction::TurnFaceDown { .. } => "TURN_FACE_DOWN",
            Instruction::PhaseOut { .. } => "PHASE_OUT",
            Instruction::Attach { .. } => "ATTACH",
            Instruction::GainControl { .. } => "GAIN_CONTROL",
            Instruction::ExchangeControl { .. } => "EXCHANGE_CONTROL",
            Instruction::Goad { .. } => "GOAD",
            Instruction::SetDesignation { .. } => "SET_DESIGNATION",
            Instruction::Venture => "VENTURE",
            Instruction::RingTempts => "RING_TEMPTS",
            Instruction::IncreaseSpeed { .. } => "INCREASE_SPEED",
            Instruction::EmpowerJace { .. } => "EMPOWER_JACE",
            Instruction::Meld { .. } => "MELD",
            Instruction::MutateMerge { .. } => "MUTATE_MERGE",
            Instruction::Manifest { .. } => "MANIFEST",
            Instruction::Cloak { .. } => "CLOAK",
            Instruction::Explore { .. } => "EXPLORE",
            Instruction::Connive { .. } => "CONNIVE",
            Instruction::Discover { .. } => "DISCOVER",
            Instruction::Blight { .. } => "BLIGHT",
            Instruction::Waterbend { .. } => "WATERBEND",
            Instruction::CreateContinuousEffect { .. } => "CREATE_CONTINUOUS_EFFECT",
            Instruction::CreateDelayedTrigger { .. } => "CREATE_DELAYED_TRIGGER",
            Instruction::ReflexiveTrigger { .. } => "REFLEXIVE_TRIGGER",
            Instruction::If { .. } => "IF",
            Instruction::ForEach { .. } => "FOR_EACH",
            Instruction::Repeat { .. } => "REPEAT",
            Instruction::RepeatWhile { .. } => "REPEAT_WHILE",
            Instruction::May { .. } => "MAY",
            Instruction::Unless { .. } => "UNLESS",
            Instruction::ChooseMode { .. } => "CHOOSE_MODE",
            Instruction::Vote { .. } => "VOTE",
            Instruction::Remember { .. } => "REMEMBER",
            Instruction::NativeEffect(_) => "NATIVE_EFFECT",
        }
    }

    /// Whether this instruction fixes the set of objects its continuous effect affects when the
    /// effect begins (REQ-DSL-010, CR 611.2c): for `CREATE_CONTINUOUS_EFFECT`, `true` when
    /// `lockAffectedSet` is absent or `true` and `false` when it is `false`; every other
    /// instruction creates no continuous effect and returns `false`.
    #[must_use]
    pub const fn locks_affected_set(&self) -> bool {
        match self {
            Instruction::CreateContinuousEffect {
                lock_affected_set, ..
            } => !matches!(lock_affected_set, Some(false)),
            _ => false,
        }
    }
}

impl Serialize for Instruction {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

impl Serialize for TokenSource {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

impl Serialize for RememberSource {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

impl Serialize for InstructionMode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

deserialize_via_from_json!(
    Instruction,
    TokenSource,
    RememberSource,
    DesignationValue,
    InstructionMode,
);
