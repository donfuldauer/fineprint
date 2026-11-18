// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The ability envelope and the six ability kinds of the card DSL (REQ-DSL-004..REQ-DSL-009,
//! SRS §14.3–§14.4.4): spell, activated, triggered, static, replacement and keyword abilities,
//! with events, costs, static effects and replacement operations.
//!
//! The JSON form of every type is the FP-012 wire contract (docs/DECISIONS.md, "#13 FP-012 —
//! Wire catalogue as implemented"), built on the FP-011 rules: camelCase members; required
//! members present; optional members omitted when absent (a present `null` is an error, an
//! explicit `false` is kept); every unknown member is an error; and every `Deserialize` buffers
//! strictly first (duplicate members, non-integer or unsafe numbers, noncharacters and nesting
//! deeper than [`crate::canonical::MAX_DEPTH`] are errors). `Serialize` writes directly and
//! re-checks the value-level invariants a hand-built value could break, so for every value
//! nested within the depth budget `decode(encode(x)) == x` and the RFC 8785 form of a decoded
//! document is the canonical form of the value. As in [`crate::expr`], nesting depth is not
//! re-checked on write: use [`crate::canonical::to_canonical_vec`] for output that must reload.
//!
//! There is no way to write a look-back flag on a trigger (SRS §14.4.1; the engine derives it,
//! 603.10a–g): no ability, trigger, matcher or typed effect has such a member, and rule E5
//! refuses any `filters`/`bind` key whose lower-cased form without `_` and `-` starts with
//! `lookback`. The flat parameters of the opaque placeholder types (effect instructions,
//! modifications, RULE effects, keyword params) are opaque by contract and not checked.
//!
//! Nothing here is evaluated; the interpreter and the toolchain checks are other units.

use std::collections::BTreeMap;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;

use crate::expr::json::{
    AsMap, FromJson, JResult, Members, Obj, array_of, check_free_str, deserialize_via_from_json,
    free_str, ser_check, str_of, u32_of,
};
use crate::expr::{PredicateAST, Zone};

mod common;
mod cost;
mod event;
mod replacement;
mod statics;

#[cfg(test)]
mod tests;

pub use self::common::{EffectInstructionAST, ModeAST, ModeSetAST, TargetSlotAST};
pub use self::cost::{ActivationLimit, ActivationTiming, CostAST};
pub use self::event::{
    EventBatch, EventKind, EventMatcher, EventSubject, FilterValue, StepId, Trigger,
};
pub use self::replacement::{ReplacementMatch, ReplacementOp, Shield, ShieldUses, TierHint};
pub use self::statics::{
    Affected, CharacteristicEffect, Layer, Modification, PlayerEffect, PtValue, RuleKind,
    RuleModification, StaticEffect,
};

use self::common::{
    check_distinct_strs, check_non_empty, check_params, check_upper_name, object_of, opt_bool,
    opt_targets, put_opt, put_targets, vec_of,
};
use self::cost::costs_from;
use self::statics::check_lock_in;

// ---------------------------------------------------------------------------------------------
// AbilityBase

/// The members every ability carries (REQ-DSL-005, SRS §14.3), written flat in the ability
/// object. The `kind` member selects the [`AbilityAST`] variant and is not stored here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbilityBase {
    /// Stable id within the definition, e.g. `"a2"` (JSON `abilityId`, non-empty).
    pub ability_id: String,
    /// Zones the ability functions in (CR 113.6; JSON `functionsIn`; non-empty, distinct).
    /// `None` means the toolchain default.
    pub functions_in: Option<Vec<Zone>>,
    /// The `[start, end]` token span of `face.text` it was compiled from (JSON `textSpan`,
    /// `start <= end`).
    pub text_span: Option<(u32, u32)>,
    /// CR 607 linked-pair ids (JSON `linkIds`; non-empty, distinct, each non-empty).
    pub link_ids: Option<Vec<String>>,
    /// An ability word (no rules meaning; JSON `abilityWord`, non-empty).
    pub ability_word: Option<String>,
    /// "As long as …" gating or "activate only if …" condition.
    pub condition: Option<PredicateAST>,
    /// Whether it is a mana ability (CR 605.1; JSON `isManaAbility`).
    pub is_mana_ability: Option<bool>,
}

impl AbilityBase {
    /// A base with only `abilityId` (REQ-DSL-005); every optional member absent.
    #[must_use]
    pub fn new(ability_id: impl Into<String>) -> AbilityBase {
        AbilityBase {
            ability_id: ability_id.into(),
            functions_in: None,
            text_span: None,
            link_ids: None,
            ability_word: None,
            condition: None,
            is_mana_ability: None,
        }
    }

    /// Checks the base rules (non-empty strings, distinct zones and link ids, span order).
    fn check(&self) -> JResult<()> {
        check_free_str(&self.ability_id, "abilityId")?;
        if let Some(zones) = &self.functions_in {
            check_non_empty(zones, "functionsIn")?;
            for (i, z) in zones.iter().enumerate() {
                if zones[..i].contains(z) {
                    return Err(format!("functionsIn lists zone {z} twice"));
                }
            }
        }
        if let Some((start, end)) = self.text_span
            && start > end
        {
            return Err(format!("textSpan start {start} is after end {end}"));
        }
        if let Some(ids) = &self.link_ids {
            check_distinct_strs(ids, "linkIds")?;
        }
        if let Some(w) = &self.ability_word {
            check_free_str(w, "abilityWord")?;
        }
        Ok(())
    }

    /// Consumes the base members from `o`.
    fn take_from(o: &mut Obj<'_>) -> JResult<Self> {
        let base = AbilityBase {
            ability_id: free_str(o.req("abilityId")?, "abilityId")?,
            functions_in: o
                .opt("functionsIn")?
                .map(|z| vec_of::<Zone>(z, "functionsIn", true))
                .transpose()?,
            text_span: o.opt("textSpan")?.map(span_from).transpose()?,
            link_ids: o
                .opt("linkIds")?
                .map(|ids| {
                    array_of(ids, "linkIds")?
                        .iter()
                        .map(|i| str_of(i, "linkId").map(str::to_owned))
                        .collect::<JResult<Vec<_>>>()
                })
                .transpose()?,
            ability_word: o
                .opt("abilityWord")?
                .map(|w| free_str(w, "abilityWord"))
                .transpose()?,
            condition: o
                .opt("condition")?
                .map(PredicateAST::from_json)
                .transpose()?,
            is_mana_ability: opt_bool(o, "isManaAbility")?,
        };
        base.check()?;
        Ok(base)
    }
}

/// Decodes `textSpan`: exactly two `u32`.
fn span_from(v: &Value) -> JResult<(u32, u32)> {
    match array_of(v, "textSpan")? {
        [a, b] => Ok((u32_of(a, "textSpan")?, u32_of(b, "textSpan")?)),
        _ => Err("textSpan must be exactly [start, end]".to_owned()),
    }
}

impl FromJson for AbilityBase {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "ability base")?;
        let out = AbilityBase::take_from(&mut o)?;
        o.finish()?;
        Ok(out)
    }
}

impl Members for AbilityBase {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("abilityId", &self.ability_id)?;
        put_opt(m, "functionsIn", self.functions_in.as_ref())?;
        if let Some((start, end)) = self.text_span {
            m.serialize_entry("textSpan", &[start, end])?;
        }
        put_opt(m, "linkIds", self.link_ids.as_ref())?;
        put_opt(m, "abilityWord", self.ability_word.as_ref())?;
        put_opt(m, "condition", self.condition.as_ref())?;
        put_opt(m, "isManaAbility", self.is_mana_ability.as_ref())
    }
}

impl Serialize for AbilityBase {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

// ---------------------------------------------------------------------------------------------
// The six kinds

/// A spell ability (REQ-DSL-005): `kind` `"SPELL"`, `targets?`, `modes?`, `effect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpellAbilityAST {
    /// The envelope.
    pub base: AbilityBase,
    /// Target slots (non-empty when present, slots distinct).
    pub targets: Option<Vec<TargetSlotAST>>,
    /// Modes.
    pub modes: Option<ModeSetAST>,
    /// Instructions (may be empty).
    pub effect: Vec<EffectInstructionAST>,
}

/// An activated ability (REQ-DSL-007, SRS §14.4.2): `kind` `"ACTIVATED"`, `cost` (non-empty),
/// `timing?`, `limits?`, `activateOnlyIf?`, `isLoyalty?`, `targets?`, `modes?`, `effect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivatedAbilityAST {
    /// The envelope.
    pub base: AbilityBase,
    /// The cost (non-empty).
    pub cost: Vec<CostAST>,
    /// Timing; `None` means `INSTANT`.
    pub timing: Option<ActivationTiming>,
    /// Activation limits (non-empty when present).
    pub limits: Option<Vec<ActivationLimit>>,
    /// "Activate only if …" (JSON `activateOnlyIf`).
    pub activate_only_if: Option<PredicateAST>,
    /// Whether it is a loyalty ability (JSON `isLoyalty`).
    pub is_loyalty: Option<bool>,
    /// Target slots.
    pub targets: Option<Vec<TargetSlotAST>>,
    /// Modes.
    pub modes: Option<ModeSetAST>,
    /// Instructions (may be empty).
    pub effect: Vec<EffectInstructionAST>,
}

/// A triggered ability (REQ-DSL-006, SRS §14.4.1): `kind` `"TRIGGERED"`, `trigger`,
/// `interveningIf?` (CR 603.4), `optional?`, `limit?`, `targets?`, `modes?`, `effect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriggeredAbilityAST {
    /// The envelope.
    pub base: AbilityBase,
    /// When it triggers.
    pub trigger: Trigger,
    /// Intervening "if" clause (CR 603.4; JSON `interveningIf`).
    pub intervening_if: Option<PredicateAST>,
    /// "You may …".
    pub optional: Option<bool>,
    /// "This ability triggers only once each turn".
    pub limit: Option<ActivationLimit>,
    /// Target slots.
    pub targets: Option<Vec<TargetSlotAST>>,
    /// Modes.
    pub modes: Option<ModeSetAST>,
    /// Instructions (may be empty).
    pub effect: Vec<EffectInstructionAST>,
}

/// A static ability (REQ-DSL-004, REQ-DSL-008, SRS §14.4.3): `kind` `"STATIC"`, `effects`
/// (non-empty), with the 613.6 lock-in cross-check across its CHAR effects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticAbilityAST {
    /// The envelope (its `condition` is the "as long as …" gate).
    pub base: AbilityBase,
    /// The effects (non-empty).
    pub effects: Vec<StaticEffect>,
}

/// A replacement ability (REQ-DSL-009, SRS §14.4.4): `kind` `"REPLACEMENT"`,
/// `isSelfReplacement?` (CR 614.15, tier 616.1a), `tierHint?`, `match`, `replaceWith`
/// (non-empty), `optional?`, `overrides614_5?` (903.9b-style repeated application), `shield?`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacementAbilityAST {
    /// The envelope.
    pub base: AbilityBase,
    /// Self-replacement (CR 614.15; JSON `isSelfReplacement`).
    pub is_self_replacement: Option<bool>,
    /// 616.1b–d tier hint (JSON `tierHint`).
    pub tier_hint: Option<TierHint>,
    /// What it replaces (JSON `match`).
    pub matcher: ReplacementMatch,
    /// What happens instead (JSON `replaceWith`, non-empty).
    pub replace_with: Vec<ReplacementOp>,
    /// "You may … instead".
    pub optional: Option<bool>,
    /// JSON `overrides614_5`.
    pub overrides_614_5: Option<bool>,
    /// A use-limited shield.
    pub shield: Option<Shield>,
}

/// A keyword instance (REQ-DSL-005): `kind` `"KEYWORD"`, `keyword` (`[A-Z][A-Z0-9_]*`) and
/// `params?` (an object of opaque values; the keyword macro gives them meaning).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeywordInstanceAST {
    /// The envelope.
    pub base: AbilityBase,
    /// The keyword name.
    pub keyword: String,
    /// Keyword parameters (non-empty names, opaque values; may be empty when present).
    pub params: Option<BTreeMap<String, Value>>,
}

/// Any ability (REQ-DSL-005, SRS §14.3): one of six kinds, chosen by the `kind` member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbilityAST {
    /// `"SPELL"`.
    Spell(SpellAbilityAST),
    /// `"ACTIVATED"`.
    Activated(ActivatedAbilityAST),
    /// `"TRIGGERED"`.
    Triggered(TriggeredAbilityAST),
    /// `"STATIC"`.
    Static(StaticAbilityAST),
    /// `"REPLACEMENT"`.
    Replacement(ReplacementAbilityAST),
    /// `"KEYWORD"`.
    Keyword(KeywordInstanceAST),
}

impl AbilityAST {
    /// Every `kind` string, in declaration order (REQ-DSL-005).
    pub const KINDS: [&'static str; 6] = [
        "SPELL",
        "ACTIVATED",
        "TRIGGERED",
        "STATIC",
        "REPLACEMENT",
        "KEYWORD",
    ];

    /// The `kind` string of this ability, e.g. `"TRIGGERED"` (REQ-DSL-005).
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            AbilityAST::Spell(_) => "SPELL",
            AbilityAST::Activated(_) => "ACTIVATED",
            AbilityAST::Triggered(_) => "TRIGGERED",
            AbilityAST::Static(_) => "STATIC",
            AbilityAST::Replacement(_) => "REPLACEMENT",
            AbilityAST::Keyword(_) => "KEYWORD",
        }
    }

    /// The envelope of this ability (REQ-DSL-005).
    #[must_use]
    pub const fn base(&self) -> &AbilityBase {
        match self {
            AbilityAST::Spell(a) => &a.base,
            AbilityAST::Activated(a) => &a.base,
            AbilityAST::Triggered(a) => &a.base,
            AbilityAST::Static(a) => &a.base,
            AbilityAST::Replacement(a) => &a.base,
            AbilityAST::Keyword(a) => &a.base,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Decoding

/// Decodes the optional `modes` member.
fn opt_modes(o: &mut Obj<'_>) -> JResult<Option<ModeSetAST>> {
    o.opt("modes")?.map(ModeSetAST::from_json).transpose()
}

/// Decodes the required `effect` member (may be empty).
fn effect_req(o: &mut Obj<'_>) -> JResult<Vec<EffectInstructionAST>> {
    vec_of(o.req("effect")?, "effect", false)
}

/// Decodes the optional predicate member `key`.
fn opt_pred(o: &mut Obj<'_>, key: &str) -> JResult<Option<PredicateAST>> {
    o.opt(key)?.map(PredicateAST::from_json).transpose()
}

impl SpellAbilityAST {
    /// Consumes the SPELL members.
    fn take_from(o: &mut Obj<'_>, base: AbilityBase) -> JResult<Self> {
        Ok(SpellAbilityAST {
            base,
            targets: opt_targets(o)?,
            modes: opt_modes(o)?,
            effect: effect_req(o)?,
        })
    }
}

impl ActivatedAbilityAST {
    /// Consumes the ACTIVATED members.
    fn take_from(o: &mut Obj<'_>, base: AbilityBase) -> JResult<Self> {
        Ok(ActivatedAbilityAST {
            base,
            cost: costs_from(o.req("cost")?, "cost")?,
            timing: o
                .opt("timing")?
                .map(ActivationTiming::from_json)
                .transpose()?,
            limits: o
                .opt("limits")?
                .map(|l| vec_of(l, "limits", true))
                .transpose()?,
            activate_only_if: opt_pred(o, "activateOnlyIf")?,
            is_loyalty: opt_bool(o, "isLoyalty")?,
            targets: opt_targets(o)?,
            modes: opt_modes(o)?,
            effect: effect_req(o)?,
        })
    }

    /// Checks the non-empty arrays.
    fn check(&self) -> JResult<()> {
        check_non_empty(&self.cost, "cost")?;
        if let Some(l) = &self.limits {
            check_non_empty(l, "limits")?;
        }
        Ok(())
    }
}

impl TriggeredAbilityAST {
    /// Consumes the TRIGGERED members.
    fn take_from(o: &mut Obj<'_>, base: AbilityBase) -> JResult<Self> {
        Ok(TriggeredAbilityAST {
            base,
            trigger: Trigger::from_json(o.req("trigger")?)?,
            intervening_if: opt_pred(o, "interveningIf")?,
            optional: opt_bool(o, "optional")?,
            limit: o
                .opt("limit")?
                .map(ActivationLimit::from_json)
                .transpose()?,
            targets: opt_targets(o)?,
            modes: opt_modes(o)?,
            effect: effect_req(o)?,
        })
    }
}

impl StaticAbilityAST {
    /// Consumes the STATIC members.
    fn take_from(o: &mut Obj<'_>, base: AbilityBase) -> JResult<Self> {
        let out = StaticAbilityAST {
            base,
            effects: vec_of(o.req("effects")?, "effects", true)?,
        };
        out.check()?;
        Ok(out)
    }

    /// Checks non-emptiness and the 613.6 lock-in rule.
    fn check(&self) -> JResult<()> {
        check_non_empty(&self.effects, "effects")?;
        check_lock_in(&self.effects)
    }
}

impl ReplacementAbilityAST {
    /// Consumes the REPLACEMENT members.
    fn take_from(o: &mut Obj<'_>, base: AbilityBase) -> JResult<Self> {
        Ok(ReplacementAbilityAST {
            base,
            is_self_replacement: opt_bool(o, "isSelfReplacement")?,
            tier_hint: o.opt("tierHint")?.map(TierHint::from_json).transpose()?,
            matcher: ReplacementMatch::from_json(o.req("match")?)?,
            replace_with: vec_of(o.req("replaceWith")?, "replaceWith", true)?,
            optional: opt_bool(o, "optional")?,
            overrides_614_5: opt_bool(o, "overrides614_5")?,
            shield: o.opt("shield")?.map(Shield::from_json).transpose()?,
        })
    }

    /// Checks non-emptiness.
    fn check(&self) -> JResult<()> {
        check_non_empty(&self.replace_with, "replaceWith")
    }
}

impl KeywordInstanceAST {
    /// A keyword instance with no parameters (REQ-DSL-005).
    #[must_use]
    pub fn new(base: AbilityBase, keyword: impl Into<String>) -> KeywordInstanceAST {
        KeywordInstanceAST {
            base,
            keyword: keyword.into(),
            params: None,
        }
    }

    /// Consumes the KEYWORD members.
    fn take_from(o: &mut Obj<'_>, base: AbilityBase) -> JResult<Self> {
        let keyword = str_of(o.req("keyword")?, "keyword")?.to_owned();
        let params = o
            .opt("params")?
            .map(|p| -> JResult<BTreeMap<String, Value>> {
                Ok(object_of(p, "params")?
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect())
            })
            .transpose()?;
        let out = KeywordInstanceAST {
            base,
            keyword,
            params,
        };
        out.check()?;
        Ok(out)
    }

    /// Checks the keyword name and the parameters.
    fn check(&self) -> JResult<()> {
        check_upper_name(&self.keyword, "keyword")?;
        match &self.params {
            Some(p) => check_params(p, &[], "keyword params"),
            None => Ok(()),
        }
    }
}

/// Decodes the kind-specific members of an ability whose `kind` is `kind`.
fn take_kind(o: &mut Obj<'_>, kind: &str, base: AbilityBase) -> JResult<AbilityAST> {
    Ok(match kind {
        "SPELL" => AbilityAST::Spell(SpellAbilityAST::take_from(o, base)?),
        "ACTIVATED" => AbilityAST::Activated(ActivatedAbilityAST::take_from(o, base)?),
        "TRIGGERED" => AbilityAST::Triggered(TriggeredAbilityAST::take_from(o, base)?),
        "STATIC" => AbilityAST::Static(StaticAbilityAST::take_from(o, base)?),
        "REPLACEMENT" => AbilityAST::Replacement(ReplacementAbilityAST::take_from(o, base)?),
        "KEYWORD" => AbilityAST::Keyword(KeywordInstanceAST::take_from(o, base)?),
        other => return Err(format!("unknown ability kind {other:?}")),
    })
}

impl FromJson for AbilityAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "ability")?;
        let kind = str_of(o.req("kind")?, "ability kind")?;
        let base = AbilityBase::take_from(&mut o)?;
        let out = take_kind(&mut o, kind, base)?;
        o.finish()?;
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------------------
// Encoding

/// Writes `kind` and the base members.
fn put_head<M: SerializeMap>(m: &mut M, kind: &str, base: &AbilityBase) -> Result<(), M::Error> {
    m.serialize_entry("kind", kind)?;
    base.members(m)
}

impl Members for SpellAbilityAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        put_head(m, "SPELL", &self.base)?;
        put_targets(m, self.targets.as_ref())?;
        put_opt(m, "modes", self.modes.as_ref())?;
        m.serialize_entry("effect", &self.effect)
    }
}

impl Members for ActivatedAbilityAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        put_head(m, "ACTIVATED", &self.base)?;
        m.serialize_entry("cost", &self.cost)?;
        put_opt(m, "timing", self.timing.as_ref())?;
        put_opt(m, "limits", self.limits.as_ref())?;
        put_opt(m, "activateOnlyIf", self.activate_only_if.as_ref())?;
        put_opt(m, "isLoyalty", self.is_loyalty.as_ref())?;
        put_targets(m, self.targets.as_ref())?;
        put_opt(m, "modes", self.modes.as_ref())?;
        m.serialize_entry("effect", &self.effect)
    }
}

impl Members for TriggeredAbilityAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        put_head(m, "TRIGGERED", &self.base)?;
        m.serialize_entry("trigger", &self.trigger)?;
        put_opt(m, "interveningIf", self.intervening_if.as_ref())?;
        put_opt(m, "optional", self.optional.as_ref())?;
        put_opt(m, "limit", self.limit.as_ref())?;
        put_targets(m, self.targets.as_ref())?;
        put_opt(m, "modes", self.modes.as_ref())?;
        m.serialize_entry("effect", &self.effect)
    }
}

impl Members for StaticAbilityAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        put_head(m, "STATIC", &self.base)?;
        m.serialize_entry("effects", &self.effects)
    }
}

impl Members for ReplacementAbilityAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        put_head(m, "REPLACEMENT", &self.base)?;
        put_opt(m, "isSelfReplacement", self.is_self_replacement.as_ref())?;
        put_opt(m, "tierHint", self.tier_hint.as_ref())?;
        m.serialize_entry("match", &self.matcher)?;
        m.serialize_entry("replaceWith", &self.replace_with)?;
        put_opt(m, "optional", self.optional.as_ref())?;
        put_opt(m, "overrides614_5", self.overrides_614_5.as_ref())?;
        put_opt(m, "shield", self.shield.as_ref())
    }
}

impl Members for KeywordInstanceAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        put_head(m, "KEYWORD", &self.base)?;
        m.serialize_entry("keyword", &self.keyword)?;
        put_opt(m, "params", self.params.as_ref())
    }
}

impl Members for AbilityAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        match self {
            AbilityAST::Spell(a) => a.members(m),
            AbilityAST::Activated(a) => a.members(m),
            AbilityAST::Triggered(a) => a.members(m),
            AbilityAST::Static(a) => a.members(m),
            AbilityAST::Replacement(a) => a.members(m),
            AbilityAST::Keyword(a) => a.members(m),
        }
    }
}

/// `Serialize` through [`Members`], and `FromJson` for a variant struct: the JSON is the whole
/// ability object, whose `kind` must be the struct's own.
macro_rules! ability_struct_json {
    ($($ty:ident => $variant:ident, $kind:literal);+ $(;)?) => {
        $(
            impl Serialize for $ty {
                fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                    AsMap(self).serialize(s)
                }
            }

            impl FromJson for $ty {
                fn from_json(v: &Value) -> JResult<Self> {
                    match AbilityAST::from_json(v)? {
                        AbilityAST::$variant(a) => Ok(a),
                        other => Err(format!(
                            "expected an ability of kind {:?}, found {:?}",
                            $kind,
                            other.kind()
                        )),
                    }
                }
            }
        )+
    };
}

ability_struct_json! {
    SpellAbilityAST => Spell, "SPELL";
    ActivatedAbilityAST => Activated, "ACTIVATED";
    TriggeredAbilityAST => Triggered, "TRIGGERED";
    StaticAbilityAST => Static, "STATIC";
    ReplacementAbilityAST => Replacement, "REPLACEMENT";
    KeywordInstanceAST => Keyword, "KEYWORD";
}

impl Serialize for AbilityAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

deserialize_via_from_json!(
    AbilityBase,
    AbilityAST,
    SpellAbilityAST,
    ActivatedAbilityAST,
    TriggeredAbilityAST,
    StaticAbilityAST,
    ReplacementAbilityAST,
    KeywordInstanceAST,
);
