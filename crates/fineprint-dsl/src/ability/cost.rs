// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Costs, activation timing and activation limits (REQ-DSL-007, SRS §14.4.2). Mana-payment
//! modifiers (convoke, delve, improvise, assist) are keyword macros and never change a cost.

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;

use super::common::{check_non_empty, put_opt, vec_of};
use crate::expr::json::{
    AsMap, FromJson, JResult, Members, Obj, bool_of, check_free_str, deserialize_via_from_json,
    free_str, ser_check, str_of, u32_of, wire_enum,
};
use crate::expr::{ObjectSelectorAST, ValueExpressionAST, Zone};
use crate::mana::ManaCostDSL;

wire_enum! {
    /// When an activated ability may be activated (REQ-DSL-007). Absent means `INSTANT`.
    ActivationTiming {
        /// `INSTANT`: any time the controller has priority.
        Instant => "INSTANT",
        /// `SORCERY`: only when a sorcery could be cast.
        Sorcery => "SORCERY",
        /// `YOUR_TURN`: only during the controller's turn.
        YourTurn => "YOUR_TURN",
        /// `OPPONENT_TURN`: only during an opponent's turn.
        OpponentTurn => "OPPONENT_TURN",
        /// `COMBAT_ONLY`: only during combat.
        CombatOnly => "COMBAT_ONLY",
    }
}

/// How often an ability may be activated or may trigger (REQ-DSL-007, REQ-DSL-006):
/// `{"perTurn"?: n, "perGame"?: n}` with `n >= 1` and at least one member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivationLimit {
    /// At most this many times each turn.
    pub per_turn: Option<u32>,
    /// At most this many times each game.
    pub per_game: Option<u32>,
}

impl ActivationLimit {
    /// Checks the limit rules.
    fn check(self) -> JResult<()> {
        if self.per_turn.is_none() && self.per_game.is_none() {
            return Err("an activation limit needs perTurn or perGame".to_owned());
        }
        if self.per_turn == Some(0) || self.per_game == Some(0) {
            return Err("an activation limit must be at least 1".to_owned());
        }
        Ok(())
    }
}

impl FromJson for ActivationLimit {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "activation limit")?;
        let out = ActivationLimit {
            per_turn: o
                .opt("perTurn")?
                .map(|n| u32_of(n, "perTurn"))
                .transpose()?,
            per_game: o
                .opt("perGame")?
                .map(|n| u32_of(n, "perGame"))
                .transpose()?,
        };
        o.finish()?;
        out.check()?;
        Ok(out)
    }
}

impl Members for ActivationLimit {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        put_opt(m, "perTurn", self.per_turn.as_ref())?;
        put_opt(m, "perGame", self.per_game.as_ref())
    }
}

impl Serialize for ActivationLimit {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

/// One cost component (REQ-DSL-007, SRS §14.4.2): an object whose `cost` member names the
/// node, plus that node's members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CostAST {
    /// `PAY_MANA{mana}`.
    PayMana {
        /// The mana paid.
        mana: ManaCostDSL,
    },
    /// `PAY_LIFE{amount}`.
    PayLife {
        /// Life paid.
        amount: ValueExpressionAST,
    },
    /// `TAP_SOURCE`.
    TapSource,
    /// `UNTAP_SOURCE`.
    UntapSource,
    /// `TAP_OBJECTS{selector,count}`.
    TapObjects {
        /// What is tapped.
        selector: ObjectSelectorAST,
        /// How many.
        count: ValueExpressionAST,
    },
    /// `SACRIFICE{selector,count}`.
    Sacrifice {
        /// What is sacrificed.
        selector: ObjectSelectorAST,
        /// How many.
        count: ValueExpressionAST,
    },
    /// `DISCARD{selector,count,random?}`.
    Discard {
        /// What is discarded.
        selector: ObjectSelectorAST,
        /// How many.
        count: ValueExpressionAST,
        /// `Some(true)`: chosen at random.
        random: Option<bool>,
    },
    /// `EXILE_FROM_ZONE{zone,selector}` plus exactly one of `count` and `minTotalMV`.
    ExileFromZone {
        /// The zone exiled from.
        zone: Zone,
        /// What is exiled.
        selector: ObjectSelectorAST,
        /// How many (exclusive with `min_total_mv`).
        count: Option<ValueExpressionAST>,
        /// Least total mana value exiled (JSON `minTotalMV`; exclusive with `count`).
        min_total_mv: Option<ValueExpressionAST>,
    },
    /// `REMOVE_COUNTERS{from,kind,amount}`.
    RemoveCounters {
        /// Whose counters.
        from: ObjectSelectorAST,
        /// Counter kind (non-empty).
        kind: String,
        /// How many.
        amount: ValueExpressionAST,
    },
    /// `PUT_COUNTERS{on,kind,amount}`.
    PutCounters {
        /// Where the counters go.
        on: ObjectSelectorAST,
        /// Counter kind (non-empty).
        kind: String,
        /// How many.
        amount: ValueExpressionAST,
    },
    /// `RETURN_TO_HAND{selector}`.
    ReturnToHand {
        /// What is returned.
        selector: ObjectSelectorAST,
    },
    /// `REVEAL{selector}`.
    Reveal {
        /// What is revealed.
        selector: ObjectSelectorAST,
    },
    /// `PAY_ENERGY{amount}`.
    PayEnergy {
        /// Energy paid.
        amount: ValueExpressionAST,
    },
    /// `COLLECT_EVIDENCE{n}`.
    CollectEvidence {
        /// Evidence total.
        n: ValueExpressionAST,
    },
    /// `FORAGE`.
    Forage,
    /// `WATERBEND{mana}`.
    Waterbend {
        /// The mana part.
        mana: ManaCostDSL,
    },
    /// `LOYALTY{delta}`.
    Loyalty {
        /// Loyalty change.
        delta: ValueExpressionAST,
    },
    /// `COMPOSITE{costs}` (non-empty).
    Composite {
        /// The parts, paid together.
        costs: Vec<CostAST>,
    },
}

impl CostAST {
    /// Every cost node name, in declaration order (REQ-DSL-007).
    pub const NAMES: [&'static str; 18] = [
        "PAY_MANA",
        "PAY_LIFE",
        "TAP_SOURCE",
        "UNTAP_SOURCE",
        "TAP_OBJECTS",
        "SACRIFICE",
        "DISCARD",
        "EXILE_FROM_ZONE",
        "REMOVE_COUNTERS",
        "PUT_COUNTERS",
        "RETURN_TO_HAND",
        "REVEAL",
        "PAY_ENERGY",
        "COLLECT_EVIDENCE",
        "FORAGE",
        "WATERBEND",
        "LOYALTY",
        "COMPOSITE",
    ];

    /// The `cost` name of this node, e.g. `"PAY_MANA"` (REQ-DSL-007).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            CostAST::PayMana { .. } => "PAY_MANA",
            CostAST::PayLife { .. } => "PAY_LIFE",
            CostAST::TapSource => "TAP_SOURCE",
            CostAST::UntapSource => "UNTAP_SOURCE",
            CostAST::TapObjects { .. } => "TAP_OBJECTS",
            CostAST::Sacrifice { .. } => "SACRIFICE",
            CostAST::Discard { .. } => "DISCARD",
            CostAST::ExileFromZone { .. } => "EXILE_FROM_ZONE",
            CostAST::RemoveCounters { .. } => "REMOVE_COUNTERS",
            CostAST::PutCounters { .. } => "PUT_COUNTERS",
            CostAST::ReturnToHand { .. } => "RETURN_TO_HAND",
            CostAST::Reveal { .. } => "REVEAL",
            CostAST::PayEnergy { .. } => "PAY_ENERGY",
            CostAST::CollectEvidence { .. } => "COLLECT_EVIDENCE",
            CostAST::Forage => "FORAGE",
            CostAST::Waterbend { .. } => "WATERBEND",
            CostAST::Loyalty { .. } => "LOYALTY",
            CostAST::Composite { .. } => "COMPOSITE",
        }
    }

    /// Checks the node-level rules a hand-built value could break (non-shallow parts check
    /// themselves when written).
    fn check(&self) -> JResult<()> {
        match self {
            CostAST::ExileFromZone {
                count,
                min_total_mv,
                ..
            } => match (count, min_total_mv) {
                (Some(_), None) | (None, Some(_)) => Ok(()),
                _ => Err("EXILE_FROM_ZONE needs exactly one of count and minTotalMV".to_owned()),
            },
            CostAST::RemoveCounters { kind, .. } | CostAST::PutCounters { kind, .. } => {
                check_free_str(kind, "counter kind")
            }
            CostAST::Composite { costs } => check_non_empty(costs, "COMPOSITE costs"),
            _ => Ok(()),
        }
    }
}

/// Decodes a mana cost symbol string.
fn mana_of(v: &Value) -> JResult<ManaCostDSL> {
    ManaCostDSL::parse(str_of(v, "mana")?).map_err(|e| format!("mana: {e}"))
}

/// Decodes a required value-expression member.
fn value_req(o: &mut Obj<'_>, key: &str) -> JResult<ValueExpressionAST> {
    ValueExpressionAST::from_json(o.req(key)?)
}

/// Decodes a required selector member.
fn sel_req(o: &mut Obj<'_>, key: &str) -> JResult<ObjectSelectorAST> {
    ObjectSelectorAST::from_json(o.req(key)?)
}

impl FromJson for CostAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "cost")?;
        let out = match str_of(o.req("cost")?, "cost")? {
            "PAY_MANA" => CostAST::PayMana {
                mana: mana_of(o.req("mana")?)?,
            },
            "PAY_LIFE" => CostAST::PayLife {
                amount: value_req(&mut o, "amount")?,
            },
            "TAP_SOURCE" => CostAST::TapSource,
            "UNTAP_SOURCE" => CostAST::UntapSource,
            "TAP_OBJECTS" => CostAST::TapObjects {
                selector: sel_req(&mut o, "selector")?,
                count: value_req(&mut o, "count")?,
            },
            "SACRIFICE" => CostAST::Sacrifice {
                selector: sel_req(&mut o, "selector")?,
                count: value_req(&mut o, "count")?,
            },
            "DISCARD" => CostAST::Discard {
                selector: sel_req(&mut o, "selector")?,
                count: value_req(&mut o, "count")?,
                random: o.opt("random")?.map(|b| bool_of(b, "random")).transpose()?,
            },
            "EXILE_FROM_ZONE" => CostAST::ExileFromZone {
                zone: Zone::from_json(o.req("zone")?)?,
                selector: sel_req(&mut o, "selector")?,
                count: o
                    .opt("count")?
                    .map(ValueExpressionAST::from_json)
                    .transpose()?,
                min_total_mv: o
                    .opt("minTotalMV")?
                    .map(ValueExpressionAST::from_json)
                    .transpose()?,
            },
            "REMOVE_COUNTERS" => CostAST::RemoveCounters {
                from: sel_req(&mut o, "from")?,
                kind: free_str(o.req("kind")?, "counter kind")?,
                amount: value_req(&mut o, "amount")?,
            },
            "PUT_COUNTERS" => CostAST::PutCounters {
                on: sel_req(&mut o, "on")?,
                kind: free_str(o.req("kind")?, "counter kind")?,
                amount: value_req(&mut o, "amount")?,
            },
            "RETURN_TO_HAND" => CostAST::ReturnToHand {
                selector: sel_req(&mut o, "selector")?,
            },
            "REVEAL" => CostAST::Reveal {
                selector: sel_req(&mut o, "selector")?,
            },
            "PAY_ENERGY" => CostAST::PayEnergy {
                amount: value_req(&mut o, "amount")?,
            },
            "COLLECT_EVIDENCE" => CostAST::CollectEvidence {
                n: value_req(&mut o, "n")?,
            },
            "FORAGE" => CostAST::Forage,
            "WATERBEND" => CostAST::Waterbend {
                mana: mana_of(o.req("mana")?)?,
            },
            "LOYALTY" => CostAST::Loyalty {
                delta: value_req(&mut o, "delta")?,
            },
            "COMPOSITE" => CostAST::Composite {
                costs: vec_of(o.req("costs")?, "COMPOSITE costs", true)?,
            },
            other => return Err(format!("unknown cost {other:?}")),
        };
        o.finish()?;
        out.check()?;
        Ok(out)
    }
}

impl Members for CostAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("cost", self.name())?;
        match self {
            CostAST::PayMana { mana } | CostAST::Waterbend { mana } => {
                m.serialize_entry("mana", mana)
            }
            CostAST::PayLife { amount } | CostAST::PayEnergy { amount } => {
                m.serialize_entry("amount", amount)
            }
            CostAST::TapSource | CostAST::UntapSource | CostAST::Forage => Ok(()),
            CostAST::TapObjects { selector, count } | CostAST::Sacrifice { selector, count } => {
                m.serialize_entry("selector", selector)?;
                m.serialize_entry("count", count)
            }
            CostAST::Discard {
                selector,
                count,
                random,
            } => {
                m.serialize_entry("selector", selector)?;
                m.serialize_entry("count", count)?;
                put_opt(m, "random", random.as_ref())
            }
            CostAST::ExileFromZone {
                zone,
                selector,
                count,
                min_total_mv,
            } => {
                m.serialize_entry("zone", zone)?;
                m.serialize_entry("selector", selector)?;
                put_opt(m, "count", count.as_ref())?;
                put_opt(m, "minTotalMV", min_total_mv.as_ref())
            }
            CostAST::RemoveCounters { from, kind, amount } => {
                m.serialize_entry("from", from)?;
                m.serialize_entry("kind", kind)?;
                m.serialize_entry("amount", amount)
            }
            CostAST::PutCounters { on, kind, amount } => {
                m.serialize_entry("on", on)?;
                m.serialize_entry("kind", kind)?;
                m.serialize_entry("amount", amount)
            }
            CostAST::ReturnToHand { selector } | CostAST::Reveal { selector } => {
                m.serialize_entry("selector", selector)
            }
            CostAST::CollectEvidence { n } => m.serialize_entry("n", n),
            CostAST::Loyalty { delta } => m.serialize_entry("delta", delta),
            CostAST::Composite { costs } => m.serialize_entry("costs", costs),
        }
    }
}

impl Serialize for CostAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

/// Decodes a non-empty `CostAST` array (the `cost` member of an activated ability and of a
/// `RULE` static effect).
pub(crate) fn costs_from(v: &Value, what: &str) -> JResult<Vec<CostAST>> {
    vec_of(v, what, true)
}

deserialize_via_from_json!(ActivationLimit, CostAST);
