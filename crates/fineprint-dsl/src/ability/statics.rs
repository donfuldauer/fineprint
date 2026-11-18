// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Static effects (REQ-DSL-004, REQ-DSL-008, SRS §14.4.3). Each effect is exactly one of a
//! [`CharacteristicEffect`] (applied in a layer), a [`PlayerEffect`] (pass A, 613.10) or a
//! [`RuleModification`] (pass B, 613.11, one kind from the closed [`RuleKind`] set), so every
//! static effect declares its layer or RuleSet kind, its selector and its modification.

use std::collections::BTreeMap;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;

use super::common::{
    check_params, check_upper_name, object_of, opt_bool, put_opt, rest_params, write_params,
};
use super::cost::{CostAST, costs_from};
use crate::expr::json::{
    AsMap, FromJson, JResult, Members, Obj, check_int, deserialize_via_from_json, int_of,
    ser_check, str_of, tagged, wire_enum,
};
use crate::expr::{ObjectSelectorAST, PlayerSelectorAST, ValueExpressionAST};

wire_enum! {
    /// A layer of the layer system (REQ-DSL-008): 1a, 1b, 2–6, 7a–7d.
    Layer {
        /// `"1a"`.
        L1a => "1a",
        /// `"1b"`.
        L1b => "1b",
        /// `"2"`.
        L2 => "2",
        /// `"3"`.
        L3 => "3",
        /// `"4"`.
        L4 => "4",
        /// `"5"`.
        L5 => "5",
        /// `"6"`.
        L6 => "6",
        /// `"7a"`.
        L7a => "7a",
        /// `"7b"`.
        L7b => "7b",
        /// `"7c"`.
        L7c => "7c",
        /// `"7d"`.
        L7d => "7d",
    }
}

wire_enum! {
    /// The closed set of rule-modification kinds applied in pass B (REQ-DSL-008, SRS §14.4.3,
    /// 613.11). Each kind is consulted by exactly one engine procedure (SRS §14.6).
    RuleKind {
        /// `COST_INCREASE` (601.2f calculator).
        CostIncrease => "COST_INCREASE",
        /// `COST_REDUCTION` (601.2f calculator).
        CostReduction => "COST_REDUCTION",
        /// `COST_MINIMUM` (601.2f calculator).
        CostMinimum => "COST_MINIMUM",
        /// `COST_SET` (601.2f calculator).
        CostSet => "COST_SET",
        /// `ALT_COST_GRANT` (601.2b).
        AltCostGrant => "ALT_COST_GRANT",
        /// `ADDITIONAL_COST_REQUIRE` (601.2b).
        AdditionalCostRequire => "ADDITIONAL_COST_REQUIRE",
        /// `PAYMENT_METHOD` (601.2h).
        PaymentMethod => "PAYMENT_METHOD",
        /// `CAST_PERMISSION` (priority legal-action generator).
        CastPermission => "CAST_PERMISSION",
        /// `PLAY_PERMISSION` (priority legal-action generator).
        PlayPermission => "PLAY_PERMISSION",
        /// `TIMING_PERMISSION`.
        TimingPermission => "TIMING_PERMISSION",
        /// `PROHIBITION`.
        Prohibition => "PROHIBITION",
        /// `ADDITIONAL_LAND_PLAY` (special action 116.2a).
        AdditionalLandPlay => "ADDITIONAL_LAND_PLAY",
        /// `MAX_HAND_SIZE` (cleanup 514.1).
        MaxHandSize => "MAX_HAND_SIZE",
        /// `ATTACK_RESTRICTION` (declare-attackers solver).
        AttackRestriction => "ATTACK_RESTRICTION",
        /// `ATTACK_REQUIREMENT` (declare-attackers solver).
        AttackRequirement => "ATTACK_REQUIREMENT",
        /// `ATTACK_COST` (declare-attackers solver).
        AttackCost => "ATTACK_COST",
        /// `BLOCK_RESTRICTION` (declare-blockers solver).
        BlockRestriction => "BLOCK_RESTRICTION",
        /// `BLOCK_REQUIREMENT` (declare-blockers solver).
        BlockRequirement => "BLOCK_REQUIREMENT",
        /// `BLOCK_COST` (declare-blockers solver).
        BlockCost => "BLOCK_COST",
        /// `BLOCK_CAPACITY` (declare-blockers solver).
        BlockCapacity => "BLOCK_CAPACITY",
        /// `EVASION`.
        Evasion => "EVASION",
        /// `HASTE_AS_THOUGH`.
        HasteAsThough => "HASTE_AS_THOUGH",
        /// `ATTACK_AS_THOUGH`.
        AttackAsThough => "ATTACK_AS_THOUGH",
        /// `DAMAGE_ASSIGNMENT` (510.1 assigner).
        DamageAssignment => "DAMAGE_ASSIGNMENT",
        /// `TARGETING_RULE` (601.2c / 115).
        TargetingRule => "TARGETING_RULE",
        /// `SKIP` (turn sequencer).
        Skip => "SKIP",
        /// `UNTAP_RULE` (502.3).
        UntapRule => "UNTAP_RULE",
        /// `MANA_RULE` (500.5, 106).
        ManaRule => "MANA_RULE",
        /// `LIFE_RULE`.
        LifeRule => "LIFE_RULE",
        /// `DRAW_RULE`.
        DrawRule => "DRAW_RULE",
        /// `ZONE_RULE`.
        ZoneRule => "ZONE_RULE",
        /// `WIN_LOSE_RULE` (104 / SBA).
        WinLoseRule => "WIN_LOSE_RULE",
        /// `SBA_RULE` (§9 registry).
        SbaRule => "SBA_RULE",
        /// `STATION_SUBSTITUTE` (702.184c).
        StationSubstitute => "STATION_SUBSTITUTE",
    }
}

/// What a characteristic effect applies to (REQ-DSL-008): a selector, or
/// `{"sameAsLayer": L}` to declare 613.6 lock-in explicitly (the set of objects fixed when the
/// effect of the same ability in layer `L` first applied).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Affected {
    /// The objects a selector picks.
    Selector(ObjectSelectorAST),
    /// `{"sameAsLayer": L}` (613.6).
    SameAsLayer(Layer),
}

impl FromJson for Affected {
    fn from_json(v: &Value) -> JResult<Self> {
        if object_of(v, "affected")?.contains_key("sameAsLayer") {
            let mut o = Obj::new(v, "affected")?;
            let layer = Layer::from_json(o.req("sameAsLayer")?)?;
            o.finish()?;
            Ok(Affected::SameAsLayer(layer))
        } else {
            Ok(Affected::Selector(ObjectSelectorAST::from_json(v)?))
        }
    }
}

impl Serialize for Affected {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Affected::Selector(sel) => sel.serialize(s),
            Affected::SameAsLayer(l) => tagged(s, "sameAsLayer", l),
        }
    }
}

/// A power or toughness of `SET_BASE_PT`/`MODIFY_PT` (REQ-DSL-008): a JSON integer or a
/// value-expression object (so `"X"`, a bare string, is not one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PtValue {
    /// A safe integer, e.g. `1`.
    Int(i64),
    /// A value expression written as an object, e.g. `{"LITERAL":1}`.
    Expr(ValueExpressionAST),
}

impl PtValue {
    /// Checks the hand-buildable invariants.
    fn check(&self, what: &str) -> JResult<()> {
        match self {
            PtValue::Int(i) => check_int(*i, what),
            PtValue::Expr(ValueExpressionAST::X) => Err(format!(
                "{what} must be an integer or a value-expression object, not \"X\""
            )),
            PtValue::Expr(_) => Ok(()),
        }
    }
}

impl FromJson for PtValue {
    fn from_json(v: &Value) -> JResult<Self> {
        match v {
            Value::Number(_) => Ok(PtValue::Int(int_of(v, "power/toughness")?)),
            Value::Object(_) => Ok(PtValue::Expr(ValueExpressionAST::from_json(v)?)),
            _ => Err("power/toughness must be an integer or a value-expression object".to_owned()),
        }
    }
}

impl Serialize for PtValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ser_check(self.check("power/toughness"))?;
        match self {
            PtValue::Int(i) => s.serialize_i64(*i),
            PtValue::Expr(e) => e.serialize(s),
        }
    }
}

/// The ops whose `power` and `toughness` are typed.
const PT_OPS: [&str; 2] = ["SET_BASE_PT", "MODIFY_PT"];

/// What an effect does (REQ-DSL-008): `{"op": NAME, …flat params}`. For `SET_BASE_PT` and
/// `MODIFY_PT`, `power` and `toughness` are required [`PtValue`]s held in their own fields;
/// every other member (and, for other ops, `power`/`toughness` too) is an opaque parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Modification {
    /// The operation name (`[A-Z][A-Z0-9_]*`).
    pub op: String,
    /// Power, present exactly for `SET_BASE_PT` and `MODIFY_PT`.
    pub power: Option<PtValue>,
    /// Toughness, present exactly for `SET_BASE_PT` and `MODIFY_PT`.
    pub toughness: Option<PtValue>,
    /// Every other member (non-empty names, not `op`, opaque values).
    pub params: BTreeMap<String, Value>,
}

impl Modification {
    /// A modification with no parameters (REQ-DSL-008); for `SET_BASE_PT`/`MODIFY_PT` set
    /// `power` and `toughness` before writing it.
    #[must_use]
    pub fn new(op: impl Into<String>) -> Modification {
        Modification {
            op: op.into(),
            power: None,
            toughness: None,
            params: BTreeMap::new(),
        }
    }

    /// Whether `op` has typed power and toughness.
    fn is_pt(&self) -> bool {
        PT_OPS.contains(&self.op.as_str())
    }

    /// Checks every rule of the modification model.
    fn check(&self) -> JResult<()> {
        check_upper_name(&self.op, "modification op")?;
        if self.is_pt() {
            match (&self.power, &self.toughness) {
                (Some(p), Some(t)) => {
                    p.check("power")?;
                    t.check("toughness")?;
                }
                _ => return Err(format!("{} requires power and toughness", self.op)),
            }
            check_params(&self.params, &["op", "power", "toughness"], "modification")
        } else {
            if self.power.is_some() || self.toughness.is_some() {
                return Err(format!(
                    "typed power/toughness belong to SET_BASE_PT and MODIFY_PT, not {}",
                    self.op
                ));
            }
            check_params(&self.params, &["op"], "modification")
        }
    }
}

impl FromJson for Modification {
    fn from_json(v: &Value) -> JResult<Self> {
        let map = object_of(v, "modification")?;
        let op = str_of(
            map.get("op")
                .ok_or("modification is missing member \"op\"")?,
            "op",
        )?;
        let out = if PT_OPS.contains(&op) {
            let pt = |key: &str| -> JResult<PtValue> {
                PtValue::from_json(
                    map.get(key)
                        .ok_or_else(|| format!("{op} is missing member {key:?}"))?,
                )
            };
            Modification {
                op: op.to_owned(),
                power: Some(pt("power")?),
                toughness: Some(pt("toughness")?),
                params: rest_params(map, &["op", "power", "toughness"], "modification")?,
            }
        } else {
            Modification {
                op: op.to_owned(),
                power: None,
                toughness: None,
                params: rest_params(map, &["op"], "modification")?,
            }
        };
        out.check()?;
        Ok(out)
    }
}

impl Members for Modification {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("op", &self.op)?;
        put_opt(m, "power", self.power.as_ref())?;
        put_opt(m, "toughness", self.toughness.as_ref())?;
        write_params(&self.params, m)
    }
}

impl Serialize for Modification {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

/// An effect applied in a layer (REQ-DSL-008): `{"type":"CHAR","layer","affected",
/// "modification","isCDA"?}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacteristicEffect {
    /// The layer it applies in.
    pub layer: Layer,
    /// What it applies to.
    pub affected: Affected,
    /// What it does.
    pub modification: Modification,
    /// Whether it is a characteristic-defining ability (JSON `isCDA`).
    pub is_cda: Option<bool>,
}

/// An effect on players applied in pass A (REQ-DSL-008, 613.10): `{"type":"PLAYER","affected",
/// "modification"}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerEffect {
    /// The players affected.
    pub affected: PlayerSelectorAST,
    /// What it does.
    pub modification: Modification,
}

/// A rule modification applied in pass B (REQ-DSL-008, 613.11): `{"type":"RULE","kind",
/// "cost"?, …flat params}`. A `cost` member is a non-empty [`CostAST`] array; other members
/// are opaque (no per-kind checks yet).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleModification {
    /// The kind; it selects the one engine procedure that consults this effect.
    pub kind: RuleKind,
    /// The `cost` member (non-empty when present).
    pub cost: Option<Vec<CostAST>>,
    /// Every other member (non-empty names, not `type`/`kind`/`cost`, opaque values).
    pub params: BTreeMap<String, Value>,
}

impl RuleModification {
    /// A rule modification with no parameters (REQ-DSL-008).
    #[must_use]
    pub fn new(kind: RuleKind) -> RuleModification {
        RuleModification {
            kind,
            cost: None,
            params: BTreeMap::new(),
        }
    }

    /// Checks the parameter model.
    fn check(&self) -> JResult<()> {
        if self.cost.as_ref().is_some_and(Vec::is_empty) {
            return Err("RULE cost must not be empty".to_owned());
        }
        check_params(&self.params, RULE_KNOWN, "rule modification")
    }
}

/// Members of a `RULE` object that are not flat params.
const RULE_KNOWN: &[&str] = &["type", "kind", "cost"];

/// One effect of a static ability (REQ-DSL-004, REQ-DSL-008): chosen by the `type` member
/// (`"CHAR"`, `"PLAYER"` or `"RULE"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StaticEffect {
    /// `"CHAR"`.
    Char(CharacteristicEffect),
    /// `"PLAYER"`.
    Player(PlayerEffect),
    /// `"RULE"`.
    Rule(RuleModification),
}

impl StaticEffect {
    /// Checks the effect-level rules.
    fn check(&self) -> JResult<()> {
        match self {
            StaticEffect::Char(c) => c.modification.check(),
            StaticEffect::Player(p) => p.modification.check(),
            StaticEffect::Rule(r) => r.check(),
        }
    }
}

impl FromJson for StaticEffect {
    fn from_json(v: &Value) -> JResult<Self> {
        let map = object_of(v, "static effect")?;
        let ty = map
            .get("type")
            .ok_or("static effect is missing member \"type\"")?;
        let out = match str_of(ty, "static effect type")? {
            "CHAR" => {
                let mut o = Obj::new(v, "CHAR effect")?;
                o.req("type")?;
                let c = CharacteristicEffect {
                    layer: Layer::from_json(o.req("layer")?)?,
                    affected: Affected::from_json(o.req("affected")?)?,
                    modification: Modification::from_json(o.req("modification")?)?,
                    is_cda: opt_bool(&mut o, "isCDA")?,
                };
                o.finish()?;
                StaticEffect::Char(c)
            }
            "PLAYER" => {
                let mut o = Obj::new(v, "PLAYER effect")?;
                o.req("type")?;
                let p = PlayerEffect {
                    affected: PlayerSelectorAST::from_json(o.req("affected")?)?,
                    modification: Modification::from_json(o.req("modification")?)?,
                };
                o.finish()?;
                StaticEffect::Player(p)
            }
            "RULE" => {
                let kind = RuleKind::from_json(
                    map.get("kind")
                        .ok_or("RULE effect is missing member \"kind\"")?,
                )?;
                let cost = match map.get("cost") {
                    None => None,
                    Some(Value::Null) => {
                        return Err("RULE member \"cost\" must be omitted rather than null".into());
                    }
                    Some(c) => Some(costs_from(c, "RULE cost")?),
                };
                StaticEffect::Rule(RuleModification {
                    kind,
                    cost,
                    params: rest_params(map, RULE_KNOWN, "rule modification")?,
                })
            }
            other => return Err(format!("unknown static effect type {other:?}")),
        };
        out.check()?;
        Ok(out)
    }
}

impl Members for CharacteristicEffect {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.modification.check())?;
        m.serialize_entry("type", "CHAR")?;
        m.serialize_entry("layer", &self.layer)?;
        m.serialize_entry("affected", &self.affected)?;
        m.serialize_entry("modification", &self.modification)?;
        put_opt(m, "isCDA", self.is_cda.as_ref())
    }
}

impl Members for PlayerEffect {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.modification.check())?;
        m.serialize_entry("type", "PLAYER")?;
        m.serialize_entry("affected", &self.affected)?;
        m.serialize_entry("modification", &self.modification)
    }
}

impl Members for RuleModification {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("type", "RULE")?;
        m.serialize_entry("kind", &self.kind)?;
        put_opt(m, "cost", self.cost.as_ref())?;
        write_params(&self.params, m)
    }
}

impl Members for StaticEffect {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        match self {
            StaticEffect::Char(c) => c.members(m),
            StaticEffect::Player(p) => p.members(m),
            StaticEffect::Rule(r) => r.members(m),
        }
    }
}

/// `Serialize` through [`Members`] and `FromJson` through [`StaticEffect`] (the standalone JSON
/// of each effect struct is the static-effect object with its own `type`).
macro_rules! effect_struct_json {
    ($($ty:ident => $variant:ident, $tag:literal);+ $(;)?) => {
        $(
            impl Serialize for $ty {
                fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                    AsMap(self).serialize(s)
                }
            }

            impl FromJson for $ty {
                fn from_json(v: &Value) -> JResult<Self> {
                    match StaticEffect::from_json(v)? {
                        StaticEffect::$variant(e) => Ok(e),
                        _ => Err(format!("expected a static effect of type {:?}", $tag)),
                    }
                }
            }
        )+
    };
}

effect_struct_json! {
    CharacteristicEffect => Char, "CHAR";
    PlayerEffect => Player, "PLAYER";
    RuleModification => Rule, "RULE";
}

impl Serialize for StaticEffect {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

/// The 613.6 lock-in cross-check of one static ability (REQ-DSL-008, REQ-DSL-004): every CHAR
/// effect with `{"sameAsLayer": L}` needs another CHAR effect of the same ability in layer `L`
/// whose `affected` is a selector, so lock-in is never dangling, self-referential or chained.
pub(crate) fn check_lock_in(effects: &[StaticEffect]) -> JResult<()> {
    for (i, e) in effects.iter().enumerate() {
        let StaticEffect::Char(CharacteristicEffect {
            affected: Affected::SameAsLayer(l),
            ..
        }) = e
        else {
            continue;
        };
        let anchored = effects.iter().enumerate().any(|(j, other)| {
            j != i
                && matches!(other, StaticEffect::Char(CharacteristicEffect {
                    layer,
                    affected: Affected::Selector(_),
                    ..
                }) if layer == l)
        });
        if !anchored {
            return Err(format!(
                "sameAsLayer {l} needs another CHAR effect of this ability in layer {l} with a \
                 selector (613.6 lock-in)"
            ));
        }
    }
    Ok(())
}

deserialize_via_from_json!(
    Affected,
    PtValue,
    Modification,
    StaticEffect,
    CharacteristicEffect,
    PlayerEffect,
    RuleModification,
);
