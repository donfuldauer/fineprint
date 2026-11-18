// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared pieces of the ability schema (REQ-DSL-005, REQ-DSL-010 placeholder): opaque JSON
//! values, flat parameter maps, the placeholder [`EffectInstructionAST`], target slots and mode
//! sets, plus the small validity helpers the other submodules reuse.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::{Map, Value};

use crate::canonical::MAX_DEPTH;
use crate::expr::ObjectSelectorAST;
use crate::expr::json::{
    AsMap, FromJson, JResult, Members, Obj, array_of, check_free_str, check_int,
    deserialize_via_from_json, free_str, is_noncharacter, ser_check, str_of, u32_of,
};

/// Validity of an opaque JSON value (the brief's OPAQUE rule): no `null` anywhere, integers only
/// and within ±(2^53 − 1), no noncharacter in any string or member name, and nesting within
/// [`MAX_DEPTH`] counting from `depth` (the depth of the container holding `v`).
pub(crate) fn check_opaque(v: &Value, depth: usize, what: &str) -> JResult<()> {
    match v {
        Value::Null => Err(format!("{what} must not contain null")),
        Value::Bool(_) => Ok(()),
        Value::Number(n) => match n.as_i64() {
            Some(i) => check_int(i, what),
            None => Err(format!("{what} holds a non-integer or unsafe number {n}")),
        },
        Value::String(s) => check_chars(s, what),
        Value::Array(items) => {
            check_depth(depth, what)?;
            items
                .iter()
                .try_for_each(|i| check_opaque(i, depth + 1, what))
        }
        Value::Object(map) => {
            check_depth(depth, what)?;
            map.iter().try_for_each(|(k, i)| {
                check_chars(k, what)?;
                check_opaque(i, depth + 1, what)
            })
        }
    }
}

/// Fails when a container at `depth` would exceed [`MAX_DEPTH`].
fn check_depth(depth: usize, what: &str) -> JResult<()> {
    if depth > MAX_DEPTH {
        Err(format!(
            "{what} is nested deeper than {MAX_DEPTH} arrays and objects"
        ))
    } else {
        Ok(())
    }
}

/// Rejects a string holding a noncharacter.
fn check_chars(s: &str, what: &str) -> JResult<()> {
    if s.chars().any(is_noncharacter) {
        Err(format!(
            "{what}: string {s:?} contains a Unicode noncharacter"
        ))
    } else {
        Ok(())
    }
}

/// Rule E5 (REQ-DSL-006, SRS §14.4.1 "Look-back flag (§8.2)"): the engine derives the look-back
/// flag from kind and filters (603.10a–g), so no definition may name one. A key is refused when
/// its lower-cased form with every `_` and `-` removed starts with `lookback`.
pub(crate) fn check_no_lookback(key: &str, what: &str) -> JResult<()> {
    let folded: String = key
        .to_lowercase()
        .chars()
        .filter(|c| *c != '_' && *c != '-')
        .collect();
    if folded.starts_with("lookback") {
        Err(format!(
            "{what} key {key:?} would set a look-back flag, which the engine derives \
             (603.10a-g) and a definition must not set"
        ))
    } else {
        Ok(())
    }
}

/// Validity of an operation or keyword name: `[A-Z][A-Z0-9_]*`.
pub(crate) fn check_upper_name(s: &str, what: &str) -> JResult<()> {
    let mut bytes = s.bytes();
    let ok = bytes.next().is_some_and(|b| b.is_ascii_uppercase())
        && bytes.all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(format!("{what} {s:?} must match [A-Z][A-Z0-9_]*"))
    }
}

/// Validity of one flat parameter: a non-empty name that is not reserved, and an opaque value.
/// Rule E5 does not apply here: flat parameters are opaque by contract (docs/DECISIONS.md,
/// "#13 FP-012 — Rule E5 covers filters and bind keys only").
pub(crate) fn check_param(key: &str, value: &Value, reserved: &[&str], what: &str) -> JResult<()> {
    check_free_str(key, what)?;
    if reserved.contains(&key) {
        return Err(format!("{what} parameter may not be named {key:?}"));
    }
    check_opaque(value, 2, what)
}

/// Validity of a parameter map (see [`check_param`]).
pub(crate) fn check_params(
    params: &BTreeMap<String, Value>,
    reserved: &[&str],
    what: &str,
) -> JResult<()> {
    params
        .iter()
        .try_for_each(|(k, v)| check_param(k, v, reserved, what))
}

/// Collects every member of `map` whose name is not in `known` as a flat parameter, checking
/// each with [`check_param`] (`known` names are also the reserved ones).
pub(crate) fn rest_params(
    map: &Map<String, Value>,
    known: &[&str],
    what: &str,
) -> JResult<BTreeMap<String, Value>> {
    let mut out = BTreeMap::new();
    for (k, v) in map {
        if known.contains(&k.as_str()) {
            continue;
        }
        if v.is_null() {
            return Err(format!(
                "{what} member {k:?} must be omitted rather than null"
            ));
        }
        check_param(k, v, known, what)?;
        out.insert(k.clone(), v.clone());
    }
    Ok(out)
}

/// The object map of `v`.
pub(crate) fn object_of<'a>(v: &'a Value, what: &str) -> JResult<&'a Map<String, Value>> {
    v.as_object()
        .ok_or_else(|| format!("{what} must be an object"))
}

/// Writes every parameter as a member.
pub(crate) fn write_params<M: SerializeMap>(
    params: &BTreeMap<String, Value>,
    m: &mut M,
) -> Result<(), M::Error> {
    for (k, v) in params {
        m.serialize_entry(k, v)?;
    }
    Ok(())
}

/// Validity of a list of strings that must be non-empty, distinct and non-empty each.
pub(crate) fn check_distinct_strs(items: &[String], what: &str) -> JResult<()> {
    if items.is_empty() {
        return Err(format!("{what} must not be empty"));
    }
    let mut seen = BTreeSet::new();
    for item in items {
        check_free_str(item, what)?;
        if !seen.insert(item.as_str()) {
            return Err(format!("{what} {item:?} is listed twice"));
        }
    }
    Ok(())
}

/// Decodes an array whose elements are converted with `T::from_json`; `non_empty` requires at
/// least one element.
pub(crate) fn vec_of<T: FromJson>(v: &Value, what: &str, non_empty: bool) -> JResult<Vec<T>> {
    let items = array_of(v, what)?;
    if non_empty && items.is_empty() {
        return Err(format!("{what} must not be empty"));
    }
    items.iter().map(T::from_json).collect()
}

/// Fails when `non_empty` holds and `items` is empty.
pub(crate) fn check_non_empty<T>(items: &[T], what: &str) -> JResult<()> {
    if items.is_empty() {
        Err(format!("{what} must not be empty"))
    } else {
        Ok(())
    }
}

/// Decodes the optional boolean member `key`.
pub(crate) fn opt_bool(o: &mut Obj<'_>, key: &'static str) -> JResult<Option<bool>> {
    o.opt(key)?
        .map(|b| crate::expr::json::bool_of(b, key))
        .transpose()
}

/// Writes `key: value` when `value` is present.
pub(crate) fn put_opt<M: SerializeMap, T: Serialize + ?Sized>(
    m: &mut M,
    key: &'static str,
    value: Option<&T>,
) -> Result<(), M::Error> {
    match value {
        Some(v) => m.serialize_entry(key, v),
        None => Ok(()),
    }
}

// ---------------------------------------------------------------------------------------------
// EffectInstructionAST (placeholder)

/// One effect instruction (REQ-DSL-010 placeholder; docs/DECISIONS.md "#13 FP-012 —
/// EffectInstructionAST is a placeholder"): an object with a required `op` matching
/// `[A-Z][A-Z0-9_]*` and any other members, each an opaque JSON value (no `null` anywhere),
/// stored and written back unchanged. FP-013 replaces it with the real instruction type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectInstructionAST {
    /// The instruction name.
    pub op: String,
    /// Every other member, by name (non-empty, not `op`).
    pub params: BTreeMap<String, Value>,
}

impl EffectInstructionAST {
    /// An instruction with no parameters (REQ-DSL-010 placeholder).
    #[must_use]
    pub fn new(op: impl Into<String>) -> EffectInstructionAST {
        EffectInstructionAST {
            op: op.into(),
            params: BTreeMap::new(),
        }
    }

    /// Checks the placeholder rules.
    fn check(&self) -> JResult<()> {
        check_upper_name(&self.op, "effect op")?;
        check_params(&self.params, &["op"], "effect instruction")
    }
}

impl FromJson for EffectInstructionAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let map = object_of(v, "effect instruction")?;
        let op = str_of(
            map.get("op")
                .ok_or("effect instruction is missing member \"op\"")?,
            "op",
        )?
        .to_owned();
        let out = EffectInstructionAST {
            op,
            params: rest_params(map, &["op"], "effect instruction")?,
        };
        out.check()?;
        Ok(out)
    }
}

impl Members for EffectInstructionAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("op", &self.op)?;
        write_params(&self.params, m)
    }
}

impl Serialize for EffectInstructionAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

// ---------------------------------------------------------------------------------------------
// TargetSlotAST, ModeSetAST, ModeAST

/// One target slot of a spell or ability (REQ-DSL-005): `{"slot","selector","min","max"}`, all
/// required, `min <= max`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetSlotAST {
    /// Slot index, referenced by `{"ref":"TARGET","slot":n}`.
    pub slot: u32,
    /// What may be targeted.
    pub selector: ObjectSelectorAST,
    /// Least number of targets.
    pub min: u32,
    /// Greatest number of targets.
    pub max: u32,
}

impl TargetSlotAST {
    /// Checks `min <= max`.
    fn check(&self) -> JResult<()> {
        if self.min > self.max {
            Err(format!(
                "target slot {}: min {} is greater than max {}",
                self.slot, self.min, self.max
            ))
        } else {
            Ok(())
        }
    }
}

impl FromJson for TargetSlotAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "target slot")?;
        let out = TargetSlotAST {
            slot: u32_of(o.req("slot")?, "slot")?,
            selector: ObjectSelectorAST::from_json(o.req("selector")?)?,
            min: u32_of(o.req("min")?, "min")?,
            max: u32_of(o.req("max")?, "max")?,
        };
        o.finish()?;
        out.check()?;
        Ok(out)
    }
}

impl Members for TargetSlotAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("slot", &self.slot)?;
        m.serialize_entry("selector", &self.selector)?;
        m.serialize_entry("min", &self.min)?;
        m.serialize_entry("max", &self.max)
    }
}

impl Serialize for TargetSlotAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

/// Validity of a `targets` array: non-empty, slots distinct, each slot valid.
pub(crate) fn check_targets(targets: &[TargetSlotAST]) -> JResult<()> {
    check_non_empty(targets, "targets")?;
    let mut seen = BTreeSet::new();
    for t in targets {
        t.check()?;
        if !seen.insert(t.slot) {
            return Err(format!("target slot {} is listed twice", t.slot));
        }
    }
    Ok(())
}

/// Decodes the optional `targets` member.
pub(crate) fn opt_targets(o: &mut Obj<'_>) -> JResult<Option<Vec<TargetSlotAST>>> {
    o.opt("targets")?
        .map(|t| {
            let targets = vec_of::<TargetSlotAST>(t, "targets", true)?;
            check_targets(&targets)?;
            Ok(targets)
        })
        .transpose()
}

/// Writes the optional `targets` member after checking it.
pub(crate) fn put_targets<M: SerializeMap>(
    m: &mut M,
    targets: Option<&Vec<TargetSlotAST>>,
) -> Result<(), M::Error> {
    if let Some(t) = targets {
        ser_check(check_targets(t))?;
        m.serialize_entry("targets", t)?;
    }
    Ok(())
}

/// One mode of a modal spell or ability (REQ-DSL-005): `{"id","targets"?,"effect"}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeAST {
    /// Mode id (non-empty, distinct within the mode set).
    pub id: String,
    /// The mode's own target slots (non-empty when present, slots distinct).
    pub targets: Option<Vec<TargetSlotAST>>,
    /// The mode's instructions (may be empty).
    pub effect: Vec<EffectInstructionAST>,
}

impl FromJson for ModeAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "mode")?;
        let out = ModeAST {
            id: free_str(o.req("id")?, "mode id")?,
            targets: opt_targets(&mut o)?,
            effect: vec_of(o.req("effect")?, "effect", false)?,
        };
        o.finish()?;
        Ok(out)
    }
}

impl Members for ModeAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(check_free_str(&self.id, "mode id"))?;
        m.serialize_entry("id", &self.id)?;
        put_targets(m, self.targets.as_ref())?;
        m.serialize_entry("effect", &self.effect)
    }
}

impl Serialize for ModeAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

/// The modes of a modal spell or ability (REQ-DSL-005): `{"min","max","modes"}`, all required;
/// modes non-empty with distinct ids; `min <= max <= modes.len()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeSetAST {
    /// Least number of modes chosen.
    pub min: u32,
    /// Greatest number of modes chosen.
    pub max: u32,
    /// The modes, in printed order.
    pub modes: Vec<ModeAST>,
}

impl ModeSetAST {
    /// Checks the mode-set rules.
    fn check(&self) -> JResult<()> {
        check_non_empty(&self.modes, "modes")?;
        let mut seen = BTreeSet::new();
        for mode in &self.modes {
            check_free_str(&mode.id, "mode id")?;
            if !seen.insert(mode.id.as_str()) {
                return Err(format!("mode id {:?} is listed twice", mode.id));
            }
        }
        if self.min > self.max {
            return Err(format!(
                "mode set: min {} is greater than max {}",
                self.min, self.max
            ));
        }
        if u64::from(self.max) > self.modes.len() as u64 {
            return Err(format!(
                "mode set: max {} exceeds the {} modes",
                self.max,
                self.modes.len()
            ));
        }
        Ok(())
    }
}

impl FromJson for ModeSetAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "mode set")?;
        let out = ModeSetAST {
            min: u32_of(o.req("min")?, "min")?,
            max: u32_of(o.req("max")?, "max")?,
            modes: vec_of(o.req("modes")?, "modes", true)?,
        };
        o.finish()?;
        out.check()?;
        Ok(out)
    }
}

impl Members for ModeSetAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("min", &self.min)?;
        m.serialize_entry("max", &self.max)?;
        m.serialize_entry("modes", &self.modes)
    }
}

impl Serialize for ModeSetAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

deserialize_via_from_json!(EffectInstructionAST, TargetSlotAST, ModeAST, ModeSetAST);
