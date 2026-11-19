// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! STATION (CR 722, 702.184): printed characteristics, no power or toughness off the
//! battlefield, and station symbols gated on charge counters (REQ-OBJ-002).
//!
//! The station ability (CR 702.184a, 702.184c) and putting charge counters on a permanent are
//! actions and are not implemented. Station symbols are supplied by the caller as
//! [`StationSymbol`] values: the DSL's ability AST is an opaque placeholder, so they cannot be
//! read from the definition yet (DECISIONS.md #23).

use fineprint_dsl::card::{
    AbilityAST, CardDefinitionDSL, FaceKind, LayoutBehavior, ValueExpressionAST,
};
use fineprint_dsl::vocab::RulesVocabulary;

use super::faces::{chars, effective_ctx, faces};
use super::types::{Gating, Layout, LayoutError, LayoutQuery};
use crate::objects::{Characteristics, ManaValueContext};
use crate::zones::ZoneKind;

/// STATION (CR 722).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StationLayout;

const LAYOUT: LayoutBehavior = LayoutBehavior::Station;

impl Layout for StationLayout {
    fn behavior(&self) -> LayoutBehavior {
        LAYOUT
    }

    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        let [main] = faces(LAYOUT, def, [FaceKind::Main])?;
        let mut out = chars(main, vocab)?;
        // CR 722.2c: in any zone other than the battlefield, a station card has no power or
        // toughness (the stack included; DECISIONS.md #23). CR 722.4: the abilities not
        // preceded by a station symbol are the printed abilities, present in every zone.
        if query.zone != ZoneKind::Battlefield {
            out.power = None;
            out.toughness = None;
        }
        Ok(out)
    }

    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        // CR 202.3: the printed mana cost.
        Ok(self
            .base_characteristics(def, query, vocab)?
            .mana_value(effective_ctx(query, ctx)))
    }

    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
        faces(LAYOUT, def, [FaceKind::Main])?;
        Ok(None)
    }
}

/// One station symbol `{N+}` and the abilities and power/toughness box printed in its
/// striation (CR 722.2, 722.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StationSymbol {
    /// N: the number of charge counters needed.
    pub threshold: i64,
    /// The abilities in the symbol's striation.
    pub abilities: Vec<AbilityAST>,
    /// The power/toughness box in the symbol's striation, if any.
    pub power_toughness: Option<(ValueExpressionAST, ValueExpressionAST)>,
}

impl StationSymbol {
    /// Whether this symbol's static ability makes the permanent a creature with base power and
    /// toughness: it has a power/toughness box (CR 722.2b).
    #[must_use]
    pub fn grants_creature(&self) -> bool {
        self.power_toughness.is_some()
    }
}

/// The station symbols whose static abilities apply to an object in `zone` with
/// `charge_counters` charge counters, in printed order.
///
/// Off the battlefield none applies (a station symbol is a static ability of a permanent
/// gated on its charge counters). On the battlefield, each symbol with `threshold` at most
/// `charge_counters` applies (CR 722.2a, 722.2b: N or more); a negative counter count
/// activates none. When two symbols apply, both are returned: choosing between their
/// power/toughness boxes is the layer system's job (DECISIONS.md #23).
///
/// # Errors
/// [`LayoutError::InvalidStationSymbols`] unless there are one or two symbols (CR 722.1).
pub fn active_station_symbols(
    symbols: &[StationSymbol],
    zone: ZoneKind,
    charge_counters: i64,
) -> Result<Vec<&StationSymbol>, LayoutError> {
    // CR 722.1: a station card has one or two station symbols.
    if !(1..=2).contains(&symbols.len()) {
        return Err(LayoutError::InvalidStationSymbols {
            count: symbols.len(),
        });
    }
    if zone != ZoneKind::Battlefield || charge_counters < 0 {
        return Ok(Vec::new());
    }
    // CR 722.2a, 722.2b: "as long as this permanent has N or more charge counters on it".
    Ok(symbols
        .iter()
        .filter(|s| s.threshold <= charge_counters)
        .collect())
}
