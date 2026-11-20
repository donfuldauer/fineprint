// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The LKI hook: evaluated characteristics for [`CharacteristicsProvider`] (REQ-CORE-003,
//! REQ-LAYER-001).

use std::collections::BTreeMap;

use fineprint_base::ids::ObjectId;

use super::pipeline::{LayerEnvironment, evaluate};
use super::registry::ContinuousEffects;
use crate::memory::CharacteristicsProvider;
use crate::objects::Characteristics;

/// A [`CharacteristicsProvider`] whose characteristics are the layer-evaluated ones (SRS 2.3:
/// LKI holds the characteristics after layers 1-7, CR 608.2h).
#[derive(Clone, Copy)]
pub struct LayeredProvider<'a> {
    effects: &'a ContinuousEffects,
    env: &'a dyn LayerEnvironment,
}

impl<'a> LayeredProvider<'a> {
    /// A provider evaluating `effects` over `env`.
    #[must_use]
    pub fn new(effects: &'a ContinuousEffects, env: &'a dyn LayerEnvironment) -> Self {
        LayeredProvider { effects, env }
    }
}

impl CharacteristicsProvider for LayeredProvider<'_> {
    /// The evaluated characteristics of `id`; `None` when the object is unknown.
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        evaluate(self.effects, self.env, id)
            .ok()
            .map(|e| e.characteristics)
    }

    /// The environment's counters on `id` with an amount of at least 1.
    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.env
            .counters(id)
            .into_iter()
            .filter(|&(_, n)| n >= 1)
            .collect()
    }

    /// The environment's objects attached to `id`.
    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.env.attached_here(id)
    }
}
