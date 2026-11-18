// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Decision descriptions carried by every instruction that involves a choice (REQ-DSL-011,
//! SRS §14.4.6). The engine derives pending choice requests from these specs; nothing here is
//! evaluated.
//!
//! JSON (the FP-011 wire contract): a camelCase object with required `chooser` and `kind`, and
//! optional `candidates`, `min`, `max`, `upTo`, `hidden`, `random` and `revealTo`. Optional
//! members are omitted when absent (a present `null` is an error); an explicit `false` is kept
//! as a distinct value.

use serde::ser::{SerializeMap, Serializer};
use serde::{Serialize, ser};
use serde_json::Value;

use crate::expr::json::{
    AsMap, FromJson, JResult, Members, Obj, array_of, bool_of, deserialize_via_from_json, free_str,
    ser_check, single_member, tagged, wire_enum,
};
use crate::expr::{ObjectSelectorAST, PlayerSelectorAST, ValueExpressionAST};

#[cfg(test)]
mod tests;

wire_enum! {
    /// What is being chosen (REQ-DSL-011, SRS §14.4.6).
    ChoiceKind {
        /// `OBJECTS`.
        Objects => "OBJECTS",
        /// `MODE`.
        Mode => "MODE",
        /// `NUMBER`.
        Number => "NUMBER",
        /// `COLOR`.
        Color => "COLOR",
        /// `CREATURE_TYPE`.
        CreatureType => "CREATURE_TYPE",
        /// `CARD_NAME`.
        CardName => "CARD_NAME",
        /// `PLAYER`.
        Player => "PLAYER",
        /// `YES_NO`.
        YesNo => "YES_NO",
        /// `ORDER`.
        Order => "ORDER",
        /// `DIVISION`.
        Division => "DIVISION",
        /// `PILE`.
        Pile => "PILE",
    }
}

/// What a choice picks from (REQ-DSL-011): externally tagged, one key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoiceCandidates {
    /// `{"SELECTOR": ObjectSelectorAST}`.
    Selector(ObjectSelectorAST),
    /// `{"PLAYERS": PlayerSelectorAST}`.
    Players(PlayerSelectorAST),
    /// `{"ENUM": [..]}`: at least one non-empty, distinct string.
    Enum(Vec<String>),
}

/// A decision an instruction requires (REQ-DSL-011, SRS §14.4.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceSpec {
    /// The player who decides.
    pub chooser: PlayerSelectorAST,
    /// What is chosen.
    pub kind: ChoiceKind,
    /// What it is chosen from.
    pub candidates: Option<ChoiceCandidates>,
    /// Least number chosen.
    pub min: Option<ValueExpressionAST>,
    /// Greatest number chosen.
    pub max: Option<ValueExpressionAST>,
    /// "Up to `max`": `Some(true)` requires `max` and forbids `min` (0..=max).
    pub up_to: Option<bool>,
    /// `Some(true)`: the choice made is not disclosed to other players.
    pub hidden: Option<bool>,
    /// `Some(true)`: the state's PRNG chooses instead of a player.
    pub random: Option<bool>,
    /// Players the result is revealed to (JSON member `revealTo`).
    pub reveal_to: Option<PlayerSelectorAST>,
}

/// Validity of an `ENUM` candidate list: non-empty, distinct, non-empty strings.
fn check_enum(items: &[String]) -> JResult<()> {
    if items.is_empty() {
        return Err("ENUM needs at least one candidate".to_owned());
    }
    // Ordered set (R3: no hash-ordered containers): O(n log n) duplicate detection.
    let mut seen = std::collections::BTreeSet::<&str>::new();
    for item in items {
        crate::expr::json::check_free_str(item, "ENUM candidate")?;
        if !seen.insert(item.as_str()) {
            return Err(format!("ENUM candidate {item:?} is listed twice"));
        }
    }
    Ok(())
}

impl FromJson for ChoiceCandidates {
    fn from_json(v: &Value) -> JResult<Self> {
        match single_member(v, "choice candidates")? {
            ("SELECTOR", s) => Ok(ChoiceCandidates::Selector(ObjectSelectorAST::from_json(s)?)),
            ("PLAYERS", p) => Ok(ChoiceCandidates::Players(PlayerSelectorAST::from_json(p)?)),
            ("ENUM", items) => {
                let items = array_of(items, "ENUM")?
                    .iter()
                    .map(|i| free_str(i, "ENUM candidate"))
                    .collect::<JResult<Vec<_>>>()?;
                check_enum(&items)?;
                Ok(ChoiceCandidates::Enum(items))
            }
            (other, _) => Err(format!("unknown choice candidates {other:?}")),
        }
    }
}

impl Serialize for ChoiceCandidates {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ChoiceCandidates::Selector(sel) => tagged(s, "SELECTOR", sel),
            ChoiceCandidates::Players(p) => tagged(s, "PLAYERS", p),
            ChoiceCandidates::Enum(items) => {
                ser_check(check_enum(items))?;
                tagged(s, "ENUM", items)
            }
        }
    }
}

impl ChoiceSpec {
    /// A spec with only `chooser` and `kind` (REQ-DSL-011); every optional member absent.
    #[must_use]
    pub fn new(chooser: PlayerSelectorAST, kind: ChoiceKind) -> ChoiceSpec {
        ChoiceSpec {
            chooser,
            kind,
            candidates: None,
            min: None,
            max: None,
            up_to: None,
            hidden: None,
            random: None,
            reveal_to: None,
        }
    }

    /// Checks the bounds rules of the wire contract (REQ-DSL-011): a `LITERAL` `min` or `max` is
    /// at least 0; when both are `LITERAL`, `min <= max`; `upTo: true` requires `max` and forbids
    /// `min`; an `ENUM` candidate list is non-empty and distinct.
    ///
    /// # Errors
    /// A message naming the broken rule.
    pub fn validate(&self) -> Result<(), String> {
        let literal = |v: &Option<ValueExpressionAST>| match v {
            Some(ValueExpressionAST::Literal(i)) => Some(*i),
            _ => None,
        };
        let (min, max) = (literal(&self.min), literal(&self.max));
        if min.is_some_and(|m| m < 0) {
            return Err("a LITERAL min must be at least 0".to_owned());
        }
        if max.is_some_and(|m| m < 0) {
            return Err("a LITERAL max must be at least 0".to_owned());
        }
        if let (Some(lo), Some(hi)) = (min, max)
            && lo > hi
        {
            return Err(format!("min {lo} is greater than max {hi}"));
        }
        if self.up_to == Some(true) {
            if self.max.is_none() {
                return Err("upTo: true requires max".to_owned());
            }
            if self.min.is_some() {
                return Err("upTo: true forbids min".to_owned());
            }
        }
        if let Some(ChoiceCandidates::Enum(items)) = &self.candidates {
            check_enum(items)?;
        }
        Ok(())
    }
}

impl FromJson for ChoiceSpec {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "choice spec")?;
        let flag = |o: &mut Obj<'_>, key: &'static str| -> JResult<Option<bool>> {
            o.opt(key)?.map(|b| bool_of(b, key)).transpose()
        };
        let value = |o: &mut Obj<'_>, key: &'static str| -> JResult<Option<ValueExpressionAST>> {
            o.opt(key)?.map(ValueExpressionAST::from_json).transpose()
        };
        let spec = ChoiceSpec {
            chooser: PlayerSelectorAST::from_json(o.req("chooser")?)?,
            kind: ChoiceKind::from_json(o.req("kind")?)?,
            candidates: o
                .opt("candidates")?
                .map(ChoiceCandidates::from_json)
                .transpose()?,
            min: value(&mut o, "min")?,
            max: value(&mut o, "max")?,
            up_to: flag(&mut o, "upTo")?,
            hidden: flag(&mut o, "hidden")?,
            random: flag(&mut o, "random")?,
            reveal_to: o
                .opt("revealTo")?
                .map(PlayerSelectorAST::from_json)
                .transpose()?,
        };
        o.finish()?;
        spec.validate()?;
        Ok(spec)
    }
}

impl Members for ChoiceSpec {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        self.validate().map_err(ser::Error::custom)?;
        m.serialize_entry("chooser", &self.chooser)?;
        m.serialize_entry("kind", &self.kind)?;
        if let Some(c) = &self.candidates {
            m.serialize_entry("candidates", c)?;
        }
        if let Some(v) = &self.min {
            m.serialize_entry("min", v)?;
        }
        if let Some(v) = &self.max {
            m.serialize_entry("max", v)?;
        }
        if let Some(b) = &self.up_to {
            m.serialize_entry("upTo", b)?;
        }
        if let Some(b) = &self.hidden {
            m.serialize_entry("hidden", b)?;
        }
        if let Some(b) = &self.random {
            m.serialize_entry("random", b)?;
        }
        if let Some(p) = &self.reveal_to {
            m.serialize_entry("revealTo", p)?;
        }
        Ok(())
    }
}

impl Serialize for ChoiceSpec {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

deserialize_via_from_json!(ChoiceCandidates, ChoiceSpec);
