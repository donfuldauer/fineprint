// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The game state as the layer system reads it (REQ-LAYER-001, REQ-LAYER-002, SRS 6.1-6.2).
//!
//! The game state holds no objects, zones or permanents yet, so a [`GameEnvironment`] joins a
//! [`GameState`] (the effects, the timestamp ledger and the generating-ability links) with a
//! caller-supplied [`ObjectWorld`] (the objects). It implements [`LayerEnvironment`]:
//!
//! - objects, bases, base mana-value provenance, counters, attachments, selector matching and
//!   characteristic-defining values come from the [`ObjectWorld`];
//! - counter timestamps (CR 613.7c) come from the state's [`TimestampLedger`]
//!   ([`GameState::timestamps`]);
//! - [`LayerEnvironment::generating_ability_exists`] (CR 613.8a, 611.3, 113.6) is answered from
//!   the state's links ([`GameState::effect_generator`]): an effect without a link is not
//!   modelled and exists; a linked effect exists iff its source has an ability equal to the
//!   linked one ([`GeneratingAbility`], identity by value). The source's abilities are those of
//!   `source_view` when the layer system gives it (the source is the object being evaluated, or
//!   the entering permanent's projection). Otherwise they are those of the source as evaluated
//!   through the layers ([`crate::layers::evaluate`] of the state's effects without the effect
//!   asked about, which never depends on itself), one level deep: inside that evaluation the
//!   existence of an effect whose source is yet another object is judged on the world's base of
//!   that object. Full cross-object (layer-major) evaluation is
//!   #186 (docs/DECISIONS.md "#183 FP-144 — Ability identity by value" and "#183 FP-144 —
//!   Existence on another source"). A source the world does not know has no abilities.
//!
//! **Layer cache invalidation** (REQ-LAYER-005, D7). The state holds no [`LayerCache`]; a caller
//! that caches evaluations through a [`GameEnvironment`] declares, after each mutation of the
//! state, the [`Invalidation`] below (every invalidation drops every entry):
//!
//! | `GameState` mutation | Invalidation |
//! |---|---|
//! | [`GameState::register_effect`], [`GameState::register_effect_group`], [`GameState::effects_mut`] writes, [`GameState::restamp_static_effects`] | none needed: the cache detects registry changes itself ([`Invalidation::EffectRegistration`]) |
//! | [`GameState::remove_effect`] of a continuous effect | none needed (detected); [`Invalidation::EffectExpiry`] may be declared |
//! | [`GameState::remove_effect`] or [`GameState::link_effect_generator`] of an ETB effect id | none: not read by layer evaluation |
//! | [`GameState::stamp_counters`], [`GameState::clear_counter_stamp`] | [`Invalidation::Timestamp`] (counter timestamps, CR 613.7c) |
//! | [`GameState::stamp_object`], [`GameState::stamp_objects_simultaneously`], [`GameState::stamp_sticker`], [`GameState::forget_object_stamps`], [`GameState::timestamps_mut`] writes | [`Invalidation::Timestamp`] (the ledger; the effects' own timestamps change only through the registry) |
//! | [`GameState::stamp_resolved_effect`], [`GameState::stamp_static_and_resolved`], `issue_*` ids, the PRNG | none: no layer input changes |
//! | [`GameState::player_mut`] writes, [`GameState::add_player`], [`GameState::set_turn_order`], [`GameState::set_active_player`], [`GameState::set_priority_player`], [`GameState::set_turn_number`] | [`Invalidation::PlayerValue`] (what an [`ObjectWorld`] may read for selectors and characteristic-defining values) |
//!
//! Changes to the [`ObjectWorld`] (zones, control, counters, attachments, status, designations)
//! are declared by the world's owner with [`Invalidation::ZoneChange`],
//! [`Invalidation::ControlChange`], [`Invalidation::CounterChange`],
//! [`Invalidation::Attachment`], [`Invalidation::Status`] and [`Invalidation::Designation`];
//! those mutation points live outside the game state (a follow-up unit).
//!
//! [`LayerCache`]: crate::layers::LayerCache
//! [`Invalidation`]: crate::layers::Invalidation
//! [`Invalidation::EffectRegistration`]: crate::layers::Invalidation::EffectRegistration
//! [`Invalidation::EffectExpiry`]: crate::layers::Invalidation::EffectExpiry
//! [`Invalidation::Timestamp`]: crate::layers::Invalidation::Timestamp
//! [`Invalidation::PlayerValue`]: crate::layers::Invalidation::PlayerValue
//! [`Invalidation::ZoneChange`]: crate::layers::Invalidation::ZoneChange
//! [`Invalidation::ControlChange`]: crate::layers::Invalidation::ControlChange
//! [`Invalidation::CounterChange`]: crate::layers::Invalidation::CounterChange
//! [`Invalidation::Attachment`]: crate::layers::Invalidation::Attachment
//! [`Invalidation::Status`]: crate::layers::Invalidation::Status
//! [`Invalidation::Designation`]: crate::layers::Invalidation::Designation
//! [`TimestampLedger`]: crate::layers::TimestampLedger

use std::collections::BTreeMap;

use fineprint_base::ids::{EffectId, ObjectId, Timestamp};
use fineprint_dsl::card::ValueExpressionAST;
use fineprint_dsl::expr::ObjectSelectorAST;

use super::{GameState, GeneratingAbility};
use crate::layers::{
    CounterHolder, Evaluated, LayerEnvironment, ManaValueProvenance, MatchContext, ObjectBase,
    evaluate, literal_of,
};

/// The object-facing half of [`LayerEnvironment`]: the objects the layer system evaluates,
/// supplied by the caller until the game state holds objects (REQ-LAYER-001). Every method must
/// be a pure function of the caller's state (REQ-CORE-001). The defaults match
/// [`LayerEnvironment`]'s.
pub trait ObjectWorld {
    /// Every object to evaluate, ascending [`ObjectId`].
    fn objects(&self) -> Vec<ObjectId>;

    /// The base of `id` (CR 613.1); `None` means unknown.
    fn base(&self, id: ObjectId) -> Option<ObjectBase>;

    /// The provenance of the base mana value of `id`
    /// ([`LayerEnvironment::base_mana_value_provenance`]). Default: plain.
    fn base_mana_value_provenance(&self, id: ObjectId) -> ManaValueProvenance {
        let _ = id;
        ManaValueProvenance::plain()
    }

    /// The counters on `id`: kind to amount (at least 1). Default: none.
    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        let _ = id;
        BTreeMap::new()
    }

    /// The objects attached to `id`. Default: none.
    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        let _ = id;
        Vec::new()
    }

    /// Whether the candidate in `ctx` matches `selector`, judged on `ctx.candidate_view`
    /// (CR 611.3a, 613.5).
    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool;

    /// The value of a non-literal base power or toughness of `object` (CR 604.3). Default: a
    /// literal gives its number ([`literal_of`]), anything else `None`.
    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        let _ = object;
        literal_of(expr)
    }
}

/// A [`GameState`] with a caller-supplied [`ObjectWorld`], read by the layer system
/// (REQ-LAYER-001, REQ-LAYER-002). See the module documentation for what each method reads.
#[derive(Clone, Copy)]
pub struct GameEnvironment<'a> {
    state: &'a GameState,
    world: &'a dyn ObjectWorld,
}

impl<'a> GameEnvironment<'a> {
    /// The environment of `state` with the objects of `world`.
    #[must_use]
    pub fn new(state: &'a GameState, world: &'a dyn ObjectWorld) -> GameEnvironment<'a> {
        GameEnvironment { state, world }
    }

    /// The game state.
    #[must_use]
    pub fn state(&self) -> &'a GameState {
        self.state
    }

    /// The object world.
    #[must_use]
    pub fn world(&self) -> &'a dyn ObjectWorld {
        self.world
    }
}

/// Whether `link`'s ability is among `abilities` (identity by value; duplicates count once).
fn has_ability(link: &GeneratingAbility, abilities: &[fineprint_dsl::card::AbilityAST]) -> bool {
    abilities.contains(&link.ability)
}

impl GameEnvironment<'_> {
    /// The link of `effect` if it must be judged: `Ok(link)`; otherwise the answer: `true`
    /// without a link (not modelled), `false` for a link to another source.
    fn link_for(&self, effect: EffectId, source: ObjectId) -> Result<&GeneratingAbility, bool> {
        match self.state.effect_generator(effect) {
            None => Err(true),
            Some(link) if link.source != source => Err(false),
            Some(link) => Ok(link),
        }
    }

    /// The existence rule without evaluation: the view's abilities when given, else the
    /// world's base abilities of `source`.
    fn exists_on_view_or_base(
        &self,
        effect: EffectId,
        source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        let link = match self.link_for(effect, source) {
            Ok(link) => link,
            Err(answer) => return answer,
        };
        match source_view {
            Some(view) => has_ability(link, &view.characteristics.abilities),
            None => self
                .world
                .base(source)
                .is_some_and(|base| has_ability(link, &base.characteristics.abilities)),
        }
    }
}

/// The environment used to evaluate another source for the existence question: the game
/// environment, with the existence of effects judged on the view or the base only, so the
/// evaluation is one level deep and never recurses.
struct SourceEnvironment<'e, 'a>(&'e GameEnvironment<'a>);

impl LayerEnvironment for SourceEnvironment<'_, '_> {
    fn objects(&self) -> Vec<ObjectId> {
        self.0.objects()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.0.base(id)
    }

    fn base_mana_value_provenance(&self, id: ObjectId) -> ManaValueProvenance {
        self.0.base_mana_value_provenance(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.0.counters(id)
    }

    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        self.0.counter_timestamp(id, kind)
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.0.attached_here(id)
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        self.0.matches(selector, ctx)
    }

    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        self.0.eval_value(expr, object)
    }

    /// CR 613.8a: as the game environment, but another source is judged on its base.
    fn generating_ability_exists(
        &self,
        effect: EffectId,
        source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        self.0.exists_on_view_or_base(effect, source, source_view)
    }
}

impl LayerEnvironment for GameEnvironment<'_> {
    fn objects(&self) -> Vec<ObjectId> {
        self.world.objects()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.world.base(id)
    }

    fn base_mana_value_provenance(&self, id: ObjectId) -> ManaValueProvenance {
        self.world.base_mana_value_provenance(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.world.counters(id)
    }

    /// CR 613.7c: the state's ledger timestamp of the counters of `kind` on object `id`.
    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        self.state
            .timestamps()
            .counter_timestamp(CounterHolder::Object(id), kind)
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.world.attached_here(id)
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        self.world.matches(selector, ctx)
    }

    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        self.world.eval_value(expr, object)
    }

    /// CR 613.8a, 611.3, 113.6: the effect exists while its source has its generating ability.
    /// No link: not modelled, `true`. A link whose source is not `source`: `false` (the ability
    /// is not on that object). Otherwise the ability exists iff it is among the abilities of
    /// `source_view` when given; else among those of `source` evaluated through the layers
    /// without `effect` (one level deep, see the module documentation; `false` if the world
    /// does not know `source`).
    fn generating_ability_exists(
        &self,
        effect: EffectId,
        source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        let link = match self.link_for(effect, source) {
            Ok(link) => link,
            Err(answer) => return answer,
        };
        if let Some(view) = source_view {
            return has_ability(link, &view.characteristics.abilities);
        }
        // CR 611.3b, 613.8a: the source's abilities as the layers give them, so an ability
        // another effect removed from it no longer generates its effect. The effect itself is
        // left out: an effect never depends on itself (CR 613.8a names "the other" effect), so
        // an effect that also removes its own source's abilities keeps applying elsewhere.
        let mut others = self.state.effects().clone();
        others.remove(effect);
        evaluate(&others, &SourceEnvironment(self), source)
            .is_ok_and(|evaluated| has_ability(link, &evaluated.characteristics.abilities))
    }
}
