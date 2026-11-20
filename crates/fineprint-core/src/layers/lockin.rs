// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Lock-in of affected sets (REQ-LAYER-003, SRS 6.3; CR 613.6, 611.2b, 611.2c, 611.3a).
//!
//! A multi-layer effect is registered as a group ([`ContinuousEffects::register_group`]): one
//! [`ContinuousEffect`] per layer or sublayer. A part whose affected set is
//! [`Affected::SameAsLayer`] is linked to its anchor (the part of the named, strictly earlier
//! layer) and applies to an object exactly when the anchor applied to it earlier in the same
//! evaluation (CR 613.6). A resolved part with a selector is replaced by the concrete set of
//! objects matching it when the group is registered (CR 611.2c); a static part keeps its selector
//! and is matched again on every evaluation (CR 611.3a). A "for as long as" window that never
//! starts, or ends before the first application without beginning again, makes the effect do
//! nothing ([`ForAsLongAsWindow`], CR 611.2b).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::expr::ObjectSelectorAST;

use super::error::LayerError;
use super::pipeline::{LayerEnvironment, MatchContext, evaluate};
use super::registry::{Affected, ContinuousEffect, ContinuousEffects, EffectOrigin};

/// The set of objects a selector picks now, for an effect of `source` controlled by
/// `controller` (CR 611.2c: the set is determined when the effect begins).
///
/// For each object of `env.objects()` in ascending [`ObjectId`] order, the object is evaluated
/// fully with `effects` ([`evaluate`]) and included iff `env.matches(selector, ..)` holds for
/// that evaluated view. Deterministic: the result is a [`BTreeSet`] (REQ-CORE-001).
///
/// # Errors
/// The first [`LayerError`] of [`evaluate`] (for example [`LayerError::UnknownObject`]).
pub fn resolve_affected_set(
    effects: &ContinuousEffects,
    env: &dyn LayerEnvironment,
    source: Option<ObjectId>,
    controller: PlayerId,
    selector: &ObjectSelectorAST,
) -> Result<BTreeSet<ObjectId>, LayerError> {
    let mut ids = env.objects();
    ids.sort_unstable();
    ids.dedup();
    let mut out = BTreeSet::new();
    for id in ids {
        // CR 611.2c: matched against the object's current characteristics.
        let evaluated = evaluate(effects, env, id)?;
        let ctx = MatchContext {
            source,
            effect_controller: controller,
            candidate: id,
            candidate_view: &evaluated,
        };
        if env.matches(selector, &ctx) {
            out.insert(id);
        }
    }
    Ok(out)
}

/// What the caller knows about a "for as long as" duration of an effect created by a resolving
/// spell or ability (CR 611.2b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForAsLongAsWindow {
    /// Whether the duration ever starts.
    pub starts: bool,
    /// Whether the duration ended before the moment the effect would first be applied.
    pub ended_before_first_application: bool,
    /// Whether the duration begins again during that spell or ability's resolution.
    pub begins_again: bool,
}

impl ForAsLongAsWindow {
    /// Whether the effect does something (CR 611.2b): `false` if the duration never starts;
    /// `false` if it ended before the effect would first be applied and does not begin again
    /// during the resolution; `true` otherwise.
    #[must_use]
    pub fn effect_does_something(self) -> bool {
        // CR 611.2b: "If the 'for as long as' duration never starts, the effect does nothing."
        if !self.starts {
            return false;
        }
        // CR 611.2b: ended before the first application "and doesn't begin again during that
        // spell or ability's resolution": the effect does nothing.
        !(self.ended_before_first_application && !self.begins_again)
    }
}

/// The position of `layer` in [`Layer::ALL`] (CR 613.1 order, sublayers included).
fn layer_index(layer: Layer) -> usize {
    Layer::ALL
        .iter()
        .position(|&l| l == layer)
        .unwrap_or(usize::MAX)
}

/// A validated group ready to be stored: its parts (resolved selectors locked) and the link of
/// every `SameAsLayer` part to its anchor.
pub(super) struct PreparedGroup {
    /// The parts, ascending [`EffectId`].
    pub parts: Vec<(EffectId, ContinuousEffect)>,
    /// `SameAsLayer` part to anchor.
    pub links: BTreeMap<EffectId, EffectId>,
}

/// Validates a group against `registry` and locks its resolved selector parts (CR 613.6,
/// 611.2c). Nothing is modified; every part locks against the registry as it is now.
///
/// # Errors
/// [`LayerError::Unsupported`] for an empty group; [`LayerError::DuplicateEffect`] for an id
/// used twice in the group or already registered; [`LayerError::LayerMismatch`] for a
/// modification outside its part's layer; [`LayerError::BadLink`] for a `SameAsLayer` part
/// without an anchor in a strictly earlier layer; the first error of [`resolve_affected_set`].
pub(super) fn prepare_group(
    registry: &ContinuousEffects,
    env: &dyn LayerEnvironment,
    mut parts: Vec<(EffectId, ContinuousEffect)>,
) -> Result<PreparedGroup, LayerError> {
    if parts.is_empty() {
        return Err(LayerError::Unsupported("empty effect group"));
    }
    // Ascending EffectId: the order the caller listed the parts in never changes a result or
    // which error is reported (REQ-CORE-001).
    parts.sort_by_key(|(id, _)| *id);
    for (i, (id, part)) in parts.iter().enumerate() {
        if registry.get(*id).is_some() || (i > 0 && parts[i - 1].0 == *id) {
            return Err(LayerError::DuplicateEffect(*id));
        }
        if !part.modification.belongs_to(part.layer) {
            return Err(LayerError::LayerMismatch { layer: part.layer });
        }
    }
    // CR 613.6: each SameAsLayer part continues the set its anchor started with in an earlier
    // layer or sublayer. The anchor is the lowest-id non-linked part of the named layer.
    let mut links = BTreeMap::new();
    for (id, part) in &parts {
        let Affected::SameAsLayer(anchor_layer) = part.affected else {
            continue;
        };
        let anchor = parts
            .iter()
            .find(|(_, a)| {
                a.layer == anchor_layer && !matches!(a.affected, Affected::SameAsLayer(_))
            })
            .map(|(aid, _)| *aid);
        match anchor {
            Some(aid) if layer_index(anchor_layer) < layer_index(part.layer) => {
                links.insert(*id, aid);
            }
            _ => return Err(LayerError::BadLink(*id)),
        }
    }
    // CR 611.2c: a resolved part's set is determined when the effect begins, each part
    // independently; CR 611.3a: a static part keeps its selector.
    let mut locked = Vec::with_capacity(parts.len());
    for (id, mut part) in parts {
        if part.origin == EffectOrigin::Resolved
            && let Affected::Selector(selector) = &part.affected
        {
            let set = resolve_affected_set(registry, env, part.source, part.controller, selector)?;
            part.affected = Affected::Objects(set);
        }
        locked.push((id, part));
    }
    Ok(PreparedGroup {
        parts: locked,
        links,
    })
}

/// The lock-in record of one per-object evaluation (CR 613.6): the effects that applied to the
/// object so far, and the links of `SameAsLayer` parts to their anchors. Lives only for that
/// evaluation, so static selectors are matched again on every computation (CR 611.3a).
pub(super) struct LockRecord<'a> {
    /// `SameAsLayer` part to anchor, from the registry.
    links: &'a BTreeMap<EffectId, EffectId>,
    /// The effects that applied to the object earlier in this evaluation.
    applied: BTreeSet<EffectId>,
}

impl<'a> LockRecord<'a> {
    /// An empty record for one evaluation with `effects`.
    pub(super) fn new(effects: &'a ContinuousEffects) -> LockRecord<'a> {
        LockRecord {
            links: effects.links(),
            applied: BTreeSet::new(),
        }
    }

    /// Records that `effect` applied to the object.
    pub(super) fn record(&mut self, effect: EffectId) {
        self.applied.insert(effect);
    }

    /// Whether the linked part `effect` applies (CR 613.6): its anchor applied to the object
    /// earlier in this evaluation. A part without a (registered) anchor applies to nothing.
    pub(super) fn linked_applies(&self, effect: EffectId) -> bool {
        self.links
            .get(&effect)
            .is_some_and(|anchor| self.applied.contains(anchor))
    }
}
