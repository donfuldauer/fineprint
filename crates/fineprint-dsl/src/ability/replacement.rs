// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Replacement pieces (REQ-DSL-009, SRS §14.4.4): what a replacement matches, what it does
//! instead, its 616.1b–d tier hint and its shield.

use std::collections::BTreeMap;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;

use super::common::{EffectInstructionAST, check_non_empty, put_opt, vec_of};
use super::event::{EventKind, EventSubject, FilterValue, check_filters, filters_from};
use crate::choice::ChoiceSpec;
use crate::expr::ValueExpressionAST;
use crate::expr::json::{
    AsMap, FromJson, JResult, Members, Obj, check_free_str, deserialize_via_from_json, free_str,
    ser_check, str_of, u32_of, wire_enum,
};

wire_enum! {
    /// A replacement's 616.1 tier hint (REQ-DSL-009), validated by the toolchain against
    /// 616.1b–d.
    TierHint {
        /// `CONTROL_ETB` (616.1b).
        ControlEtb => "CONTROL_ETB",
        /// `COPY_ETB` (616.1c).
        CopyEtb => "COPY_ETB",
        /// `BACK_FACE_ETB` (616.1d).
        BackFaceEtb => "BACK_FACE_ETB",
    }
}

/// What a replacement watches for (REQ-DSL-009): `{"event","filters"?,"affected"}`; `filters`
/// keys follow rule E5 as in [`super::EventMatcher`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacementMatch {
    /// The event replaced.
    pub event: EventKind,
    /// Field tests (may be empty when present).
    pub filters: Option<BTreeMap<String, FilterValue>>,
    /// Whose event it is.
    pub affected: EventSubject,
}

impl ReplacementMatch {
    /// Checks the filter keys.
    fn check(&self) -> JResult<()> {
        match &self.filters {
            Some(f) => check_filters(f, "filters", true),
            None => Ok(()),
        }
    }
}

impl FromJson for ReplacementMatch {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "replacement match")?;
        let out = ReplacementMatch {
            event: EventKind::from_json(o.req("event")?)?,
            filters: o
                .opt("filters")?
                .map(|f| filters_from(f, "filters", true))
                .transpose()?,
            affected: EventSubject::from_json(o.req("affected")?)?,
        };
        o.finish()?;
        Ok(out)
    }
}

impl Members for ReplacementMatch {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("event", &self.event)?;
        put_opt(m, "filters", self.filters.as_ref())?;
        m.serialize_entry("affected", &self.affected)
    }
}

impl Serialize for ReplacementMatch {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

/// What a replacement does instead (REQ-DSL-009): an object chosen by `op`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplacementOp {
    /// `MODIFY_EVENT{set}`: overwrite event fields (non-empty).
    ModifyEvent {
        /// Field values written into the event.
        set: BTreeMap<String, FilterValue>,
    },
    /// `INSTEAD{instructions}` (non-empty).
    Instead {
        /// What happens instead.
        instructions: Vec<EffectInstructionAST>,
    },
    /// `SKIP`: the event does not happen.
    Skip,
    /// `PREVENT{amount?}`: prevent damage (all of it when `amount` is absent).
    Prevent {
        /// How much is prevented.
        amount: Option<ValueExpressionAST>,
    },
    /// `CHOOSE{choice,store}`: a choice made as the event happens, stored under `store`.
    Choose {
        /// The decision (boxed: it is much larger than the other variants).
        choice: Box<ChoiceSpec>,
        /// Memory key the result is stored under (non-empty).
        store: String,
    },
}

impl ReplacementOp {
    /// The `op` name of this node, e.g. `"MODIFY_EVENT"` (REQ-DSL-009).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            ReplacementOp::ModifyEvent { .. } => "MODIFY_EVENT",
            ReplacementOp::Instead { .. } => "INSTEAD",
            ReplacementOp::Skip => "SKIP",
            ReplacementOp::Prevent { .. } => "PREVENT",
            ReplacementOp::Choose { .. } => "CHOOSE",
        }
    }

    /// Checks the node rules a hand-built value could break.
    fn check(&self) -> JResult<()> {
        match self {
            ReplacementOp::ModifyEvent { set } => {
                if set.is_empty() {
                    return Err("MODIFY_EVENT set must not be empty".to_owned());
                }
                check_filters(set, "set", false)
            }
            ReplacementOp::Instead { instructions } => {
                check_non_empty(instructions, "INSTEAD instructions")
            }
            ReplacementOp::Choose { store, .. } => check_free_str(store, "store"),
            ReplacementOp::Skip | ReplacementOp::Prevent { .. } => Ok(()),
        }
    }
}

impl FromJson for ReplacementOp {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "replacement op")?;
        let out = match str_of(o.req("op")?, "op")? {
            "MODIFY_EVENT" => ReplacementOp::ModifyEvent {
                set: filters_from(o.req("set")?, "set", false)?,
            },
            "INSTEAD" => ReplacementOp::Instead {
                instructions: vec_of(o.req("instructions")?, "INSTEAD instructions", true)?,
            },
            "SKIP" => ReplacementOp::Skip,
            "PREVENT" => ReplacementOp::Prevent {
                amount: o
                    .opt("amount")?
                    .map(ValueExpressionAST::from_json)
                    .transpose()?,
            },
            "CHOOSE" => ReplacementOp::Choose {
                choice: Box::new(ChoiceSpec::from_json(o.req("choice")?)?),
                store: free_str(o.req("store")?, "store")?,
            },
            other => return Err(format!("unknown replacement op {other:?}")),
        };
        o.finish()?;
        out.check()?;
        Ok(out)
    }
}

impl Members for ReplacementOp {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("op", self.name())?;
        match self {
            ReplacementOp::ModifyEvent { set } => m.serialize_entry("set", set),
            ReplacementOp::Instead { instructions } => {
                m.serialize_entry("instructions", instructions)
            }
            ReplacementOp::Skip => Ok(()),
            ReplacementOp::Prevent { amount } => put_opt(m, "amount", amount.as_ref()),
            ReplacementOp::Choose { choice, store } => {
                m.serialize_entry("choice", choice)?;
                m.serialize_entry("store", store)
            }
        }
    }
}

impl Serialize for ReplacementOp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

/// How many times a shield applies (REQ-DSL-009): a count of at least 1, or `"COUNTER_BACKED"`
/// (one use per counter of `counterKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShieldUses {
    /// A fixed number of uses (at least 1).
    Count(u32),
    /// `"COUNTER_BACKED"`.
    CounterBacked,
}

impl FromJson for ShieldUses {
    fn from_json(v: &Value) -> JResult<Self> {
        match v {
            Value::String(s) if s == "COUNTER_BACKED" => Ok(ShieldUses::CounterBacked),
            Value::Number(_) => Ok(ShieldUses::Count(u32_of(v, "shield uses")?)),
            _ => Err("shield uses must be an integer or \"COUNTER_BACKED\"".to_owned()),
        }
    }
}

impl Serialize for ShieldUses {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ShieldUses::Count(n) => s.serialize_u32(*n),
            ShieldUses::CounterBacked => s.serialize_str("COUNTER_BACKED"),
        }
    }
}

/// A replacement that wears off after some uses (REQ-DSL-009): `{"uses","counterKind"?}`;
/// `counterKind` only with `"COUNTER_BACKED"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shield {
    /// How many uses.
    pub uses: ShieldUses,
    /// The counter kind backing the uses (non-empty).
    pub counter_kind: Option<String>,
}

impl Shield {
    /// Checks the shield rules.
    fn check(&self) -> JResult<()> {
        if self.uses == ShieldUses::Count(0) {
            return Err("shield uses must be at least 1".to_owned());
        }
        if let Some(k) = &self.counter_kind {
            if self.uses != ShieldUses::CounterBacked {
                return Err("counterKind is allowed only with uses \"COUNTER_BACKED\"".to_owned());
            }
            check_free_str(k, "counterKind")?;
        }
        Ok(())
    }
}

impl FromJson for Shield {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "shield")?;
        let out = Shield {
            uses: ShieldUses::from_json(o.req("uses")?)?,
            counter_kind: o
                .opt("counterKind")?
                .map(|k| free_str(k, "counterKind"))
                .transpose()?,
        };
        o.finish()?;
        out.check()?;
        Ok(out)
    }
}

impl Members for Shield {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        ser_check(self.check())?;
        m.serialize_entry("uses", &self.uses)?;
        put_opt(m, "counterKind", self.counter_kind.as_ref())
    }
}

impl Serialize for Shield {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

deserialize_via_from_json!(ReplacementMatch, ReplacementOp, ShieldUses, Shield);
