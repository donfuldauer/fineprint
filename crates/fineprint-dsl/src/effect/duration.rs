// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! How long an instruction's effect lasts (REQ-DSL-010, SRS §14.4.5): [`DurationAST`].

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;

use super::util::req;
use crate::ability::StepId;
use crate::expr::json::{
    AsMap, FromJson, JResult, Members, Obj, deserialize_via_from_json, str_of,
};
use crate::expr::{PlayerSelectorAST, PredicateAST};

/// The duration of an effect (REQ-DSL-010, SRS §14.4.5): an object with a required `kind` and
/// the members that kind has, e.g. `{"kind":"UNTIL_END_OF_TURN"}` or
/// `{"kind":"UNTIL","step":"END","whose":{"ref":"CONTROLLER"}}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DurationAST {
    /// `UNTIL_END_OF_TURN`.
    UntilEndOfTurn,
    /// `UNTIL_YOUR_NEXT_TURN`.
    UntilYourNextTurn,
    /// `UNTIL_END_OF_COMBAT`.
    UntilEndOfCombat,
    /// `UNTIL{step, whose}`: until the given step of the given player's turn.
    Until {
        /// The step or main phase.
        step: StepId,
        /// Whose step.
        whose: PlayerSelectorAST,
    },
    /// `FOR_AS_LONG_AS{predicate}` (CR 611.2b).
    ForAsLongAs {
        /// The condition that keeps the effect alive.
        predicate: PredicateAST,
    },
    /// `WHILE_SOURCE_ON_BATTLEFIELD`.
    WhileSourceOnBattlefield,
    /// `WHILE_CONTROLLED_BY{player}`.
    WhileControlledBy {
        /// The controlling player.
        player: PlayerSelectorAST,
    },
    /// `PERMANENT`: for the rest of the game.
    Permanent,
    /// `PERPETUAL`: an extension duration (not a CR term), accepted on the wire.
    Perpetual,
}

impl DurationAST {
    /// Every `kind` string, in declaration order (REQ-DSL-010).
    pub const KINDS: [&'static str; 9] = [
        "UNTIL_END_OF_TURN",
        "UNTIL_YOUR_NEXT_TURN",
        "UNTIL_END_OF_COMBAT",
        "UNTIL",
        "FOR_AS_LONG_AS",
        "WHILE_SOURCE_ON_BATTLEFIELD",
        "WHILE_CONTROLLED_BY",
        "PERMANENT",
        "PERPETUAL",
    ];

    /// The `kind` string of this duration, e.g. `"UNTIL_END_OF_TURN"` (REQ-DSL-010).
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            DurationAST::UntilEndOfTurn => "UNTIL_END_OF_TURN",
            DurationAST::UntilYourNextTurn => "UNTIL_YOUR_NEXT_TURN",
            DurationAST::UntilEndOfCombat => "UNTIL_END_OF_COMBAT",
            DurationAST::Until { .. } => "UNTIL",
            DurationAST::ForAsLongAs { .. } => "FOR_AS_LONG_AS",
            DurationAST::WhileSourceOnBattlefield => "WHILE_SOURCE_ON_BATTLEFIELD",
            DurationAST::WhileControlledBy { .. } => "WHILE_CONTROLLED_BY",
            DurationAST::Permanent => "PERMANENT",
            DurationAST::Perpetual => "PERPETUAL",
        }
    }
}

impl FromJson for DurationAST {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "duration")?;
        let out = match str_of(o.req("kind")?, "duration kind")? {
            "UNTIL_END_OF_TURN" => DurationAST::UntilEndOfTurn,
            "UNTIL_YOUR_NEXT_TURN" => DurationAST::UntilYourNextTurn,
            "UNTIL_END_OF_COMBAT" => DurationAST::UntilEndOfCombat,
            "UNTIL" => DurationAST::Until {
                step: req(&mut o, "step")?,
                whose: req(&mut o, "whose")?,
            },
            // CR 611.2b: "for as long as" durations.
            "FOR_AS_LONG_AS" => DurationAST::ForAsLongAs {
                predicate: req(&mut o, "predicate")?,
            },
            "WHILE_SOURCE_ON_BATTLEFIELD" => DurationAST::WhileSourceOnBattlefield,
            "WHILE_CONTROLLED_BY" => DurationAST::WhileControlledBy {
                player: req(&mut o, "player")?,
            },
            "PERMANENT" => DurationAST::Permanent,
            "PERPETUAL" => DurationAST::Perpetual,
            other => return Err(format!("unknown duration kind {other:?}")),
        };
        o.finish()?;
        Ok(out)
    }
}

impl Members for DurationAST {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        m.serialize_entry("kind", self.kind())?;
        match self {
            DurationAST::Until { step, whose } => {
                m.serialize_entry("step", step)?;
                m.serialize_entry("whose", whose)
            }
            DurationAST::ForAsLongAs { predicate } => m.serialize_entry("predicate", predicate),
            DurationAST::WhileControlledBy { player } => m.serialize_entry("player", player),
            DurationAST::UntilEndOfTurn
            | DurationAST::UntilYourNextTurn
            | DurationAST::UntilEndOfCombat
            | DurationAST::WhileSourceOnBattlefield
            | DurationAST::Permanent
            | DurationAST::Perpetual => Ok(()),
        }
    }
}

impl Serialize for DurationAST {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AsMap(self).serialize(s)
    }
}

deserialize_via_from_json!(DurationAST);
