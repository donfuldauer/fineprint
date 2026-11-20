// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The CR 601.2f hook: the cost effects of the [`super::RuleSet`] that apply to one player, in
//! the stages the casting procedure applies them in (REQ-LAYER-004, CR 613.11).

use std::collections::BTreeSet;

use fineprint_base::ids::EffectId;

use super::rules::RuleEntry;
use crate::layers::LayerError;

/// The cost effects applying to one player, split by the stage of CR 601.2b and 601.2f they
/// belong to. Each list is in application order. Nothing here computes a cost: the casting
/// procedure does (CR 601.2f).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CostStages {
    /// `ALT_COST_GRANT` and `ADDITIONAL_COST_REQUIRE` entries: alternative and additional costs
    /// announced in CR 601.2b.
    pub grants: Vec<RuleEntry>,
    /// `COST_INCREASE` entries: added to the total cost (CR 601.2f).
    pub increases: Vec<RuleEntry>,
    /// `COST_REDUCTION` entries, in timestamp order by default; the player may apply them in
    /// any order (CR 601.2f, [`CostStages::reorder_reductions`]).
    pub reductions: Vec<RuleEntry>,
    /// The `COST_MINIMUM` entries, then the `COST_SET` entries, each kind in timestamp order:
    /// the effects that directly affect the total cost, applied after the reductions and before
    /// the total cost is locked in (CR 601.2f).
    pub total_effects: Vec<RuleEntry>,
}

impl CostStages {
    /// Puts the reductions in the order `order`, chosen by the player (CR 601.2f: "If multiple
    /// cost reductions apply, the player may apply them in any order.").
    ///
    /// # Errors
    /// [`LayerError::Unsupported`] if `order` is not a permutation of the ids of the current
    /// reductions (a missing, extra, unknown or repeated id); the reductions are then unchanged.
    pub fn reorder_reductions(&mut self, order: &[EffectId]) -> Result<(), LayerError> {
        let current: BTreeSet<EffectId> = self.reductions.iter().map(|e| e.id).collect();
        let wanted: BTreeSet<EffectId> = order.iter().copied().collect();
        if order.len() != self.reductions.len() || wanted.len() != order.len() || wanted != current
        {
            return Err(LayerError::Unsupported(
                "the reduction order is not a permutation of the current reductions (CR 601.2f)",
            ));
        }
        let mut reordered = Vec::with_capacity(order.len());
        for id in order {
            if let Some(entry) = self.reductions.iter().find(|e| e.id == *id) {
                reordered.push(entry.clone());
            }
        }
        self.reductions = reordered;
        Ok(())
    }
}
