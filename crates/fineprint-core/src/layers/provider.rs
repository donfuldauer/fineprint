// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The LKI hook: evaluated characteristics for [`CharacteristicsProvider`] (REQ-CORE-003,
//! REQ-LAYER-001).

use std::collections::BTreeMap;

use fineprint_base::ids::{ObjectId, PlayerId};

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

    /// The controller of `id` after layer 2 (CR 613.1b, REQ-LAYER-001); `None` when the object
    /// is unknown, cannot be evaluated, or the evaluation names no controller.
    fn controller(&self, id: ObjectId) -> Option<PlayerId> {
        evaluate(self.effects, self.env, id)
            .ok()
            .and_then(|e| e.controller)
    }
}

#[cfg(test)]
mod tests {
    // Unit tests of `LayeredProvider::controller`: REQ-LAYER-001 (CR 613.1b, CR 613.7).
    // Synthetic ids and synthetic characteristics only.

    use std::collections::BTreeSet;

    use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
    use fineprint_dsl::ability::Layer;
    use fineprint_dsl::card::{ColorBitmask, TextTokenStream};
    use fineprint_dsl::expr::ObjectSelectorAST;

    use super::*;
    use crate::layers::{
        Affected, ContinuousEffect, EffectOrigin, LayerModification, MatchContext, ObjectBase,
    };

    fn p(n: u64) -> PlayerId {
        PlayerId::new(n)
    }

    fn o(n: u64) -> ObjectId {
        ObjectId::new(n)
    }

    fn chars() -> Characteristics {
        Characteristics {
            names: BTreeSet::from(["test.vanilla_2_2".to_owned()]),
            mana_cost: None,
            color_indicator: None,
            colors: ColorBitmask::NONE,
            supertypes: BTreeSet::new(),
            card_types: BTreeSet::from(["Creature".to_owned()]),
            subtypes: BTreeSet::new(),
            rules_text: TextTokenStream::default(),
            abilities: Vec::new(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            hand_modifier: None,
            life_modifier: None,
        }
    }

    /// Object 1 with base controller `base`.
    struct Env {
        base: Option<PlayerId>,
    }

    impl LayerEnvironment for Env {
        fn objects(&self) -> Vec<ObjectId> {
            vec![o(1)]
        }

        fn base(&self, id: ObjectId) -> Option<ObjectBase> {
            (id == o(1)).then(|| ObjectBase {
                characteristics: chars(),
                controller: self.base,
                on_battlefield: true,
            })
        }

        fn matches(&self, _selector: &ObjectSelectorAST, _ctx: &MatchContext<'_>) -> bool {
            false
        }
    }

    /// A resolved layer-2 control effect on object 1 giving it to `to` at timestamp `t`.
    fn control(to: u64, t: u64) -> ContinuousEffect {
        ContinuousEffect {
            source: None,
            controller: p(to),
            timestamp: Timestamp::new(t),
            layer: Layer::L2,
            is_cda: false,
            origin: EffectOrigin::Resolved,
            affected: Affected::Objects([o(1)].into_iter().collect()),
            modification: LayerModification::SetController(p(to)),
        }
    }

    fn effects(list: &[(u64, ContinuousEffect)]) -> ContinuousEffects {
        let mut r = ContinuousEffects::new();
        for (id, effect) in list {
            r.register(EffectId::new(*id), effect.clone()).unwrap();
        }
        r
    }

    /// REQ-LAYER-001; CR 613.1b: a layer-2 control effect sets the provider's controller.
    #[test]
    fn req_layer_001_controller_after_layer_2() {
        let env = Env { base: Some(p(1)) };
        let fx = effects(&[(1, control(2, 1))]);
        assert_eq!(LayeredProvider::new(&fx, &env).controller(o(1)), Some(p(2)));
    }

    /// REQ-LAYER-001; CR 613.1b: with no control effect the base controller is returned; a
    /// base naming none, or an unknown object, gives `None` (the caller falls back).
    #[test]
    fn req_layer_001_controller_without_effect_or_unknown() {
        let none = ContinuousEffects::new();
        let env = Env { base: Some(p(1)) };
        assert_eq!(
            LayeredProvider::new(&none, &env).controller(o(1)),
            Some(p(1))
        );
        let env = Env { base: None };
        assert_eq!(LayeredProvider::new(&none, &env).controller(o(1)), None);
        assert_eq!(LayeredProvider::new(&none, &env).controller(o(9)), None);
        let fx = effects(&[(1, control(2, 1))]);
        assert_eq!(LayeredProvider::new(&fx, &env).controller(o(9)), None);
    }

    /// REQ-LAYER-001; CR 613.1b, CR 613.7: of two layer-2 control effects the later timestamp
    /// wins, in both registration (effect id) orders and both timestamp orders.
    #[test]
    fn req_layer_001_controller_later_timestamp_wins() {
        let env = Env { base: Some(p(1)) };
        // P2 at timestamp 1, P3 at timestamp 2: P3 wins whichever is registered first.
        for list in [
            [(1, control(2, 1)), (2, control(3, 2))],
            [(1, control(3, 2)), (2, control(2, 1))],
        ] {
            let fx = effects(&list);
            assert_eq!(LayeredProvider::new(&fx, &env).controller(o(1)), Some(p(3)));
        }
        // Timestamps swapped: P2 now later, P2 wins in both registration orders.
        for list in [
            [(1, control(2, 2)), (2, control(3, 1))],
            [(1, control(3, 1)), (2, control(2, 2))],
        ] {
            let fx = effects(&list);
            assert_eq!(LayeredProvider::new(&fx, &env).controller(o(1)), Some(p(2)));
        }
    }
}
