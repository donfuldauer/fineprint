// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The `NATIVE_EFFECT` node (REQ-DSL-015, SRS §14.8): a call to a registered native effect by
//! stable `id@version`, with opaque parameters and the layers and RuleSet kinds it declares it
//! may affect.

use std::collections::BTreeMap;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;

use super::util::{
    check_distinct, check_non_empty, check_opaque_map, non_empty_vec, opaque_map, put_opt,
};
use crate::ability::{Layer, RuleKind};
use crate::expr::json::{
    AsMap, FromJson, JResult, Members, Obj, check_free_str, deserialize_via_from_json, free_str,
    int_of, ser_check, str_of,
};

/// The `op` of a native-effect instruction.
pub(crate) const NATIVE_OP: &str = "NATIVE_EFFECT";

/// A call to a registered native effect (REQ-DSL-015, SRS §14.8). JSON (also its standalone
/// form): `{"op":"NATIVE_EFFECT","id":STR,"version":int,"params":{…},"layers"?:[…],
/// "ruleKinds"?:[…]}`, e.g. `{"op":"NATIVE_EFFECT","id":"native.subgame.start","version":2,
/// "params":{}}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeEffectAST {
    /// Registry id: non-empty, no noncharacter, no `@` (the registry key is `id@version`).
    pub id: String,
    /// Registry version, 1..=4294967295.
    pub version: u32,
    /// Parameters: an opaque object (may be empty).
    pub params: BTreeMap<String, Value>,
    /// Layers the effect may affect (non-empty and distinct when present, order kept); `None`
    /// declares none.
    pub layers: Option<Vec<Layer>>,
    /// RuleSet kinds the effect may affect (JSON `ruleKinds`; non-empty and distinct when
    /// present, order kept); `None` declares none.
    pub rule_kinds: Option<Vec<RuleKind>>,
}

impl NativeEffectAST {
    /// A call with empty `params` that declares no layers and no RuleSet kinds (REQ-DSL-015).
    #[must_use]
    pub fn new(id: impl Into<String>, version: u32) -> NativeEffectAST {
        NativeEffectAST {
            id: id.into(),
            version,
            params: BTreeMap::new(),
            layers: None,
            rule_kinds: None,
        }
    }

    /// The registry key `"{id}@{version}"` serialised into state and replay (REQ-DSL-015,
    /// REQ-API-005).
    #[must_use]
    pub fn registry_key(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }

    /// Checks every rule of the node.
    pub(crate) fn check(&self) -> JResult<()> {
        check_free_str(&self.id, "native effect id")?;
        if self.id.contains('@') {
            return Err(format!(
                "native effect id {:?} must not contain '@'",
                self.id
            ));
        }
        if self.version == 0 {
            return Err("native effect version must be at least 1".to_owned());
        }
        check_opaque_map(&self.params, false, "native effect params")?;
        if let Some(layers) = &self.layers {
            check_non_empty(layers, "layers")?;
            check_distinct(layers, "layers")?;
        }
        if let Some(kinds) = &self.rule_kinds {
            check_non_empty(kinds, "ruleKinds")?;
            check_distinct(kinds, "ruleKinds")?;
        }
        Ok(())
    }

    /// Consumes the members other than `op` from `o`.
    pub(crate) fn take_from(o: &mut Obj<'_>) -> JResult<Self> {
        let id = free_str(o.req("id")?, "native effect id")?;
        let version = int_of(o.req("version")?, "native effect version")?;
        let version = u32::try_from(version)
            .ok()
            .filter(|v| *v != 0)
            .ok_or_else(|| format!("native effect version {version} is outside 1..=4294967295"))?;
        let out = NativeEffectAST {
            id,
            version,
            params: opaque_map(o.req("params")?, false, "native effect params")?,
            layers: o
                .opt("layers")?
                .map(|l| non_empty_vec(l, "layers"))
                .transpose()?,
            rule_kinds: o
                .opt("ruleKinds")?
                .map(|k| non_empty_vec(k, "ruleKinds"))
                .transpose()?,
        };
        out.check()?;
        Ok(out)
    }
}

impl FromJson for NativeEffectAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "native effect")?;
        let op = str_of(o.req("op")?, "op")?;
        if op != NATIVE_OP {
            return Err(format!("expected op {NATIVE_OP:?}, found {op:?}"));
        }
        let out = NativeEffectAST::take_from(&mut o)?;
        o.finish()?;
        Ok(out)
    }
}

impl Members for NativeEffectAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("op", NATIVE_OP)?;
        m.serialize_entry("id", &self.id)?;
        m.serialize_entry("version", &self.version)?;
        m.serialize_entry("params", &self.params)?;
        put_opt(m, "layers", self.layers.as_ref())?;
        put_opt(m, "ruleKinds", self.rule_kinds.as_ref())
    }
}

impl Serialize for NativeEffectAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

deserialize_via_from_json!(NativeEffectAST);
