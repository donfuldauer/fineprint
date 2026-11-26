// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The layer system inside the game state (REQ-LAYER-001, REQ-LAYER-002, REQ-CORE-002): the
//! continuous effects ([`ContinuousEffects`]), the timestamp ledger ([`TimestampLedger`], CR
//! 613.7) and the link from each static-ability effect (and each ETB replacement effect) to the
//! ability on its source that generates it ([`GeneratingAbility`], CR 613.8a "existence").
//!
//! **Effect ids.** Every [`EffectId`] comes from the state's own effect generator. Continuous
//! effects and ETB replacement effects share that id space; an ETB effect id is linked with
//! [`GameState::link_effect_generator`] (it is not a continuous effect).
//!
//! **Timestamps.** Every timestamp comes from the state's single monotonic timestamp generator
//! (REQ-CORE-002) through the wrappers below, which call the [`TimestampLedger`] and the free
//! functions of [`crate::layers`]. Every wrapper is all or nothing: on an error the state,
//! generators included, is unchanged.
//!
//! **Validation at this boundary.** [`GameState::register_effect`] enforces what
//! [`ContinuousEffects::register`] enforces plus: an effect of a static ability with a source
//! has the link to its generating ability, and only such an effect has one; a layer-7a effect
//! is from a characteristic-defining ability (SRS 6.2 item 1, CR 613.4a).
//! [`GameState::effects_mut`] gives direct, unvalidated access to the registry (a state made
//! inconsistent through it serializes, but [`GameState::deserialize`] rejects it).

use std::collections::BTreeMap;
use std::sync::Arc;

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::AbilityAST;

use super::{GameState, StateError};
use crate::layers::{
    ContinuousEffect, ContinuousEffects, CounterHolder, EffectOrigin, LayerEnvironment, LayerError,
    SimultaneousStamp, StampReason, TimestampLedger, put_typed, restamp_static_effects,
    stamp_resolved_effect, stamp_static_and_resolved,
};

/// The ability on `source` that generates an effect (REQ-LAYER-002, CR 613.8a, 611.3, 113.6).
///
/// An ability is identified on its source by value: by its [`AbilityAST`] (docs/DECISIONS.md
/// "#183 FP-144 — Ability identity by value"). Two identical abilities on one source are one
/// identity: the effect exists while the source has at least one of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratingAbility {
    /// The object the ability is on.
    pub source: ObjectId,
    /// The ability itself.
    pub ability: AbilityAST,
}

impl GeneratingAbility {
    /// The ability `ability` on `source`.
    #[must_use]
    pub fn new(source: ObjectId, ability: AbilityAST) -> GeneratingAbility {
        GeneratingAbility { source, ability }
    }
}

impl CanonicalEncode for GeneratingAbility {
    /// The source [`ObjectId`], then the ability as a DSL value (tag `0` and its RFC 8785
    /// canonical JSON, or the structural fallback; `crate::layers` encoding documentation).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.source.encode_canonical(out);
        put_typed(out, &self.ability);
    }
}

/// Maps a layer-system error to a state error: exhaustion of the timestamp generator is
/// [`StateError::IdExhausted`]; anything else is [`StateError::Layer`].
fn from_layer(e: LayerError) -> StateError {
    match e {
        LayerError::IdExhausted => StateError::IdExhausted,
        other => StateError::Layer(other),
    }
}

/// The boundary checks of [`GameState::register_effect`] that do not need the state.
fn check_effect(
    effect: &ContinuousEffect,
    generator: Option<&GeneratingAbility>,
) -> Result<(), StateError> {
    // SRS 6.2 item 1, CR 613.4a: layer 7a consists only of characteristic-defining abilities.
    if effect.layer == Layer::L7a && !effect.is_cda {
        return Err(StateError::NonCdaInLayer7a);
    }
    // CR 613.8a, 611.3: a static ability's effect exists only while its ability does, so it
    // carries the link to that ability; a resolved effect (CR 611.2) has none.
    match (&effect.origin, effect.source, generator) {
        (EffectOrigin::Static { .. }, Some(_), None) => Err(StateError::MissingGenerator),
        (EffectOrigin::Static { .. }, Some(source), Some(g)) if g.source != source => {
            Err(StateError::GeneratorSourceMismatch)
        }
        (EffectOrigin::Static { .. }, Some(_), Some(_)) | (_, _, None) => Ok(()),
        (EffectOrigin::Static { .. }, None, Some(_)) | (EffectOrigin::Resolved, _, Some(_)) => {
            Err(StateError::UnexpectedGenerator)
        }
    }
}

impl GameState {
    /// The continuous effects in force (REQ-LAYER-001).
    #[must_use]
    pub fn effects(&self) -> &ContinuousEffects {
        &self.effects
    }

    /// Direct write access to the continuous effects, copy-on-write (only the registry is
    /// un-shared). Nothing written here is validated by the state: prefer
    /// [`GameState::register_effect`] and [`GameState::remove_effect`]. A static effect with a
    /// source registered here has no generating-ability link and is reported to exist always;
    /// such a state serializes, but [`GameState::deserialize`] rejects it.
    pub fn effects_mut(&mut self) -> &mut ContinuousEffects {
        Arc::make_mut(&mut self.effects)
    }

    /// The timestamp ledger (REQ-CORE-002, CR 613.7).
    #[must_use]
    pub fn timestamps(&self) -> &TimestampLedger {
        &self.timestamps
    }

    /// Direct write access to the timestamp ledger, copy-on-write. Timestamps written here
    /// must come from this state's generator ([`GameState::issue_timestamp`]); prefer the
    /// stamping wrappers ([`GameState::stamp_object`] and the others).
    pub fn timestamps_mut(&mut self) -> &mut TimestampLedger {
        Arc::make_mut(&mut self.timestamps)
    }

    /// The links from effect ids to their generating abilities, ascending [`EffectId`].
    #[must_use]
    pub fn effect_generators(&self) -> &BTreeMap<EffectId, GeneratingAbility> {
        &self.generators
    }

    /// The generating ability linked to `effect`, if any (CR 613.8a).
    #[must_use]
    pub fn effect_generator(&self, effect: EffectId) -> Option<&GeneratingAbility> {
        self.generators.get(&effect)
    }

    /// Registers `effect` under the next [`EffectId`] of the state's effect generator and
    /// records the link to its generating ability (REQ-LAYER-001, REQ-LAYER-002). All or
    /// nothing: on any error the state, the effect generator included, is unchanged.
    ///
    /// # Errors
    /// - [`StateError::NonCdaInLayer7a`]: `effect.layer` is 7a and `effect.is_cda` is `false`
    ///   (SRS 6.2 item 1, CR 613.4a);
    /// - [`StateError::MissingGenerator`]: a static effect (`EffectOrigin::Static`) with a
    ///   source and no `generator` (CR 613.8a);
    /// - [`StateError::GeneratorSourceMismatch`]: `generator.source` is not `effect.source`;
    /// - [`StateError::UnexpectedGenerator`]: a `generator` for a resolved effect or a static
    ///   effect without a source;
    /// - [`StateError::IdExhausted`]: the effect generator is exhausted;
    /// - [`StateError::Layer`]: the error of [`ContinuousEffects::register`] (a layer mismatch,
    ///   an invalid text change, a `SameAsLayer` affected set, an id already registered).
    pub fn register_effect(
        &mut self,
        effect: ContinuousEffect,
        generator: Option<GeneratingAbility>,
    ) -> Result<EffectId, StateError> {
        check_effect(&effect, generator.as_ref())?;
        let mut ids = self.ids.effect.clone();
        let id = ids.issue().map_err(|_| StateError::IdExhausted)?;
        if self.effects.get(id).is_some() {
            return Err(StateError::Layer(LayerError::DuplicateEffect(id)));
        }
        // Validated on an empty registry first, so a refusal never un-shares the registry.
        ContinuousEffects::new()
            .register(id, effect.clone())
            .map_err(StateError::Layer)?;
        Arc::make_mut(&mut self.effects)
            .register(id, effect)
            .map_err(StateError::Layer)?;
        self.ids.effect = ids;
        if let Some(g) = generator {
            Arc::make_mut(&mut self.generators).insert(id, g);
        }
        Ok(id)
    }

    /// Registers one multi-layer effect (CR 613.6) through
    /// [`ContinuousEffects::register_group`]: each part gets the next [`EffectId`] in the order
    /// given and its generating-ability link, with the checks of
    /// [`GameState::register_effect`] applied to every part. Returns the parts' ids in that
    /// order. All or nothing.
    ///
    /// # Errors
    /// The errors of [`GameState::register_effect`] for any part, and the error of
    /// [`ContinuousEffects::register_group`] as [`StateError::Layer`].
    pub fn register_effect_group(
        &mut self,
        env: &dyn LayerEnvironment,
        parts: Vec<(ContinuousEffect, Option<GeneratingAbility>)>,
    ) -> Result<Vec<EffectId>, StateError> {
        let mut ids = self.ids.effect.clone();
        let mut group = Vec::with_capacity(parts.len());
        let mut links = Vec::new();
        for (effect, generator) in parts {
            check_effect(&effect, generator.as_ref())?;
            let id = ids.issue().map_err(|_| StateError::IdExhausted)?;
            if let Some(g) = generator {
                links.push((id, g));
            }
            group.push((id, effect));
        }
        let out: Vec<EffectId> = group.iter().map(|(id, _)| *id).collect();
        let mut next = (*self.effects).clone();
        next.register_group(env, group).map_err(StateError::Layer)?;
        self.effects = Arc::new(next);
        self.ids.effect = ids;
        if !links.is_empty() {
            Arc::make_mut(&mut self.generators).extend(links);
        }
        Ok(out)
    }

    /// Links the effect id `effect`, which is not a continuous effect, to its generating ability:
    /// for an ETB replacement effect of a permanent's own ability (CR 614.12; ETB effect ids
    /// share the effect id space, docs/DECISIONS.md "#40 FP-039"). The state is unchanged on
    /// error.
    ///
    /// # Errors
    /// [`StateError::EffectIdNotIssued`] if `effect` was not issued by this state's effect
    /// generator (0, or above its last issued id); [`StateError::AlreadyLinked`] if `effect`
    /// already has a link; [`StateError::EffectRegistered`] if `effect` is a registered
    /// continuous effect (its link is made by [`GameState::register_effect`]).
    pub fn link_effect_generator(
        &mut self,
        effect: EffectId,
        generator: GeneratingAbility,
    ) -> Result<(), StateError> {
        if effect.get() == 0 || effect.get() > self.ids.effect.last_issued() {
            return Err(StateError::EffectIdNotIssued(effect));
        }
        if self.generators.contains_key(&effect) {
            return Err(StateError::AlreadyLinked(effect));
        }
        if self.effects.get(effect).is_some() {
            return Err(StateError::EffectRegistered(effect));
        }
        Arc::make_mut(&mut self.generators).insert(effect, generator);
        Ok(())
    }

    /// Removes the effect `id`: the continuous effect (as [`ContinuousEffects::remove`]) and its
    /// generating-ability link, so no link is left dangling. Also drops the link of an ETB
    /// effect id. Whether anything (an effect or a link) was removed.
    pub fn remove_effect(&mut self, id: EffectId) -> bool {
        let registered = self.effects.get(id).is_some();
        if registered {
            Arc::make_mut(&mut self.effects).remove(id);
        }
        let linked = self.generators.contains_key(&id);
        if linked {
            Arc::make_mut(&mut self.generators).remove(&id);
        }
        registered || linked
    }

    /// The APNAP order (CR 101.4, 613.7m): the active player, then each other player in turn
    /// order starting after the active player (the turn order rotated to the active player).
    /// With no active player (or one missing from the turn order) it is the turn order as is.
    #[must_use]
    pub fn apnap_order(&self) -> Vec<PlayerId> {
        let order = self.turn_order();
        match self
            .turn
            .active_player()
            .and_then(|a| order.iter().position(|&p| p == a))
        {
            Some(at) => order[at..].iter().chain(&order[..at]).copied().collect(),
            None => order.to_vec(),
        }
    }

    /// Runs `f` on copies of the timestamp generator and the ledger and commits both only on
    /// success, so a failure changes nothing and never un-shares the ledger.
    fn with_ledger<T>(
        &mut self,
        f: impl FnOnce(
            &mut TimestampLedger,
            &mut fineprint_base::ids::IdGenerator<Timestamp>,
        ) -> Result<T, LayerError>,
    ) -> Result<T, StateError> {
        let mut ids = self.ids.timestamp.clone();
        let mut ledger = (*self.timestamps).clone();
        let out = f(&mut ledger, &mut ids).map_err(from_layer)?;
        self.ids.timestamp = ids;
        self.timestamps = Arc::new(ledger);
        Ok(out)
    }

    /// Gives object `id` a new timestamp, then its stickers new ones right after it (CR 613.7d
    /// zone entry, 613.7e attach, 613.7f face up or down, 613.7g transform or convert,
    /// 613.7h-j special cards, 613.7k stickers), from the state's timestamp generator.
    ///
    /// # Errors
    /// [`StateError::IdExhausted`]; the state is then unchanged.
    pub fn stamp_object(
        &mut self,
        id: ObjectId,
        reason: StampReason,
    ) -> Result<Timestamp, StateError> {
        self.with_ledger(|ledger, ids| ledger.stamp_object(ids, id, reason))
    }

    /// Gives every object of `batch` a timestamp at the same moment (CR 613.7m): in APNAP order
    /// ([`GameState::apnap_order`]: the active player first, then the turn order from there;
    /// the plain turn order when no active player is set), each player's objects in that
    /// player's `choice` order, ties by [`ObjectId`]. Returns the objects with their timestamps
    /// in that order.
    ///
    /// # Errors
    /// [`StateError::Layer`] with [`LayerError::UnknownPlayer`] (a batch player is not a
    /// player of the turn order) or [`LayerError::DuplicateObject`];
    /// [`StateError::IdExhausted`]. The state is then unchanged.
    pub fn stamp_objects_simultaneously(
        &mut self,
        batch: &[SimultaneousStamp],
        reason: StampReason,
    ) -> Result<Vec<(ObjectId, Timestamp)>, StateError> {
        let apnap = self.apnap_order();
        self.with_ledger(|ledger, ids| {
            ledger.stamp_objects_simultaneously(ids, &apnap, batch, reason)
        })
    }

    /// A counter of `kind` is put on `holder`: a new timestamp shared by every counter of that
    /// kind on it (CR 613.7c).
    ///
    /// # Errors
    /// [`StateError::IdExhausted`]; the state is then unchanged.
    pub fn stamp_counters(
        &mut self,
        holder: CounterHolder,
        kind: &str,
    ) -> Result<Timestamp, StateError> {
        self.with_ledger(|ledger, ids| ledger.put_counters(ids, holder, kind))
    }

    /// Forgets the timestamp of the counters of `kind` on `holder` (the last one left).
    pub fn clear_counter_stamp(&mut self, holder: CounterHolder, kind: &str) {
        if self.timestamps.counter_timestamp(holder, kind).is_some() {
            Arc::make_mut(&mut self.timestamps).clear_counters(holder, kind);
        }
    }

    /// A sticker is put on `host`: a new timestamp (CR 613.7k).
    ///
    /// # Errors
    /// [`StateError::IdExhausted`]; the state is then unchanged.
    pub fn stamp_sticker(&mut self, host: ObjectId, sticker: u32) -> Result<Timestamp, StateError> {
        self.with_ledger(|ledger, ids| ledger.put_sticker(ids, host, sticker))
    }

    /// Forgets object `id`'s timestamp, its stickers' and the counters on it.
    pub fn forget_object_stamps(&mut self, id: ObjectId) {
        let mut ledger = (*self.timestamps).clone();
        ledger.forget_object(id);
        if ledger != *self.timestamps {
            self.timestamps = Arc::new(ledger);
        }
    }

    /// The timestamp of a continuous effect created by a resolving spell or ability: a new one
    /// (CR 613.7b).
    ///
    /// # Errors
    /// [`StateError::IdExhausted`]; the state is then unchanged.
    pub fn stamp_resolved_effect(&mut self) -> Result<Timestamp, StateError> {
        stamp_resolved_effect(&mut self.ids.timestamp).map_err(from_layer)
    }

    /// Timestamps for an object's own static-ability effect and a resolved effect applying to it
    /// that would be stamped simultaneously: `(static, resolved)`, the static one earlier
    /// (CR 613.7n).
    ///
    /// # Errors
    /// [`StateError::IdExhausted`]; the state is then unchanged.
    pub fn stamp_static_and_resolved(&mut self) -> Result<(Timestamp, Timestamp), StateError> {
        stamp_static_and_resolved(&mut self.ids.timestamp).map_err(from_layer)
    }

    /// Re-stamps the static-ability effects of `object` after it moved from timestamp
    /// `old_object_ts` to `new_object_ts` (CR 613.7a), drawing any new timestamp from the
    /// state's generator ([`crate::layers::restamp_static_effects`]).
    ///
    /// # Errors
    /// [`StateError::IdExhausted`]; the state is then unchanged.
    pub fn restamp_static_effects(
        &mut self,
        object: ObjectId,
        old_object_ts: Timestamp,
        new_object_ts: Timestamp,
    ) -> Result<(), StateError> {
        let mut ids = self.ids.timestamp.clone();
        let mut effects = (*self.effects).clone();
        restamp_static_effects(&mut effects, &mut ids, object, old_object_ts, new_object_ts)
            .map_err(from_layer)?;
        self.ids.timestamp = ids;
        if effects != *self.effects {
            self.effects = Arc::new(effects);
        }
        Ok(())
    }
}
