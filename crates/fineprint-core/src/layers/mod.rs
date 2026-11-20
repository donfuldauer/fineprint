// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The layer system (REQ-LAYER-001, REQ-LAYER-002, REQ-LAYER-003, REQ-CORE-002, SRS 6.1-6.3):
//! the continuous-effect registry, per-object evaluation through layers 1a-7d, the ordering
//! within each layer, lock-in of affected sets, timestamps and the LKI hook; and the passes after
//! the layers (REQ-LAYER-004, SRS 6.4).
//!
//! **Registry.** [`ContinuousEffects`] holds every continuous effect under its [`EffectId`]
//! (fineprint_base) with its source, controller, timestamp, layer, origin, affected objects and
//! [`LayerModification`]. A modification must belong to its declared layer
//! ([`LayerModification::belongs_to`]).
//!
//! **Evaluation.** [`evaluate`] starts from the object's base characteristics (CR 613.1) as the
//! caller's [`LayerEnvironment`] supplies them and applies, in order, layers 1a, 1b, 2, 3, 4, 5,
//! 6, 7a, 7b, 7c and 7d (CR 613.1-613.4). Selector effects are matched against the object as
//! evaluated so far (CR 611.3a, 613.5); [`Affected::Objects`] is a locked set (CR 611.2c).
//! Keyword counters apply in layer 6 (CR 122.1b, 613.1f) and P/T counters in layer 7c
//! (CR 122.1a, 613.4c). Layer 7 is the last layer. Evaluation takes shared references only: the
//! base is never mutated.
//!
//! **Ordering within a layer** (REQ-LAYER-002, SRS 6.2). In layers 2-6, effects from
//! characteristic-defining abilities apply first (CR 613.3); layer 7a holds only those
//! (CR 613.4a). Then, repeatedly, the unapplied step that depends on no other unapplied step and
//! has the earliest ordering key applies (CR 613.8b): timestamp (CR 613.7, counters at their
//! counter timestamp, CR 613.7c), then effects before counters, then ascending [`EffectId`], then
//! counter kind. Dependency (CR 613.8a) is found generically by trial application on a scratch
//! copy of the partially evaluated object, comparing whether the other step applies and what it
//! does with and without the first; a CDA and a non-CDA never depend on each other. A dependency
//! loop (a strongly connected component of the dependency graph whose members depend on nothing
//! outside it) is applied in timestamp order, one member at a time, and the relation is
//! recomputed after every application (CR 613.8b, 613.8c). [`evaluate_with_order`] also returns
//! the [`OrderRecord`] of the order used, ready for a rules-trace event.
//!
//! **Lock-in** (REQ-LAYER-003, SRS 6.3). A multi-layer effect is registered as a group of
//! parts, one per layer or sublayer ([`ContinuousEffects::register_group`]). A part whose
//! affected set is [`Affected::SameAsLayer`] follows its anchor, the part of the group's first
//! layer (the earliest layer of its parts that are not `SameAsLayer`): within one evaluation of an object it applies exactly when the anchor applied to that
//! object, even if the object stopped matching the selector or the generating ability was
//! removed in between (CR 613.6). An anchor that applied to nothing locks its parts to nothing.
//! A resolved part with a selector is locked to the concrete set
//! [`resolve_affected_set`] gives at registration, each part independently (CR 611.2c); a static
//! selector is matched again on every evaluation, so a static group's lock holds within one
//! evaluation only (CR 611.3a). The record lives in the single per-object evaluation: there is
//! no cache and no cross-object state. [`ContinuousEffects::register_group_for_as_long_as`]
//! registers nothing when its [`ForAsLongAsWindow`] makes the effect do nothing (CR 611.2b).
//! [`ContinuousEffects::register`] never locks a selector and rejects `SameAsLayer`.
//!
//! **Timestamps.** [`TimestampLedger`] and the free functions assign timestamps per
//! CR 613.7a-n from the caller's single monotonic `IdGenerator<Timestamp>`.
//!
//! **LKI.** [`LayeredProvider`] implements [`crate::memory::CharacteristicsProvider`] with the
//! evaluated characteristics (SRS 2.3).
//!
//! **Passes after the layers** (REQ-LAYER-004, SRS 6.4). [`PostLayerEffects`] holds the effects
//! that affect players and those that modify the rules of the game. Pass A
//! ([`compute_player_properties`]) applies the player effects after the determination of
//! objects' characteristics (CR 613.10); pass B ([`build_rule_set`]) builds the [`RuleSet`], the
//! rule modifications grouped by kind, after all other continuous effects (CR 613.11). Both use
//! timestamp order, then ascending [`EffectId`] (CR 613.7), with an optional dependency hook
//! ([`PostLayerEnvironment::depends_on`], CR 613.8b). In the rule set prohibitions are evaluated
//! last, so an applicable "can't" beats any "can" ([`RuleSet::query`], CR 101.2); it gives the
//! maximum hand size (CR 402.2), the number of land plays (CR 305.2) and the cost effects in the
//! stages of CR 601.2f ([`CostStages`]). A resolved player effect applies to a fixed set of
//! players; a rule effect is never locked and applies to whatever matches when the rule set is
//! built (CR 611.2c).
//!
//! **Scope.** This module is standalone: it is not wired into the game state, its Merkle layout
//! or the state hash (the placeholder `state::EffectRegistry` stays). **Out of scope** (later
//! units): the cached versus full-recompute mode; wiring into the game state and the state hash
//! (#183 FP-144); switching the call sites in events, triggers, replacement and zones;
//! layer-major evaluation across objects (#186 FP-146; dependency is judged on the object under
//! evaluation only); effect expiry and durations (the caller removes ended effects; a "for as
//! long as" window is judged by the caller); typed player properties derived from pass A and the
//! cost calculation itself (the casting procedure).
//!
//! **Determinism.** Only `Vec`, `BTreeMap` and `BTreeSet`; no floats, clock or I/O; the order
//! of registration never changes a result (REQ-CORE-001).

mod error;
mod lockin;
mod ordering;
mod pipeline;
mod post;
mod provider;
mod registry;
mod timestamps;

#[cfg(test)]
mod lockin_tests;
#[cfg(test)]
mod ordering_tests;
#[cfg(test)]
mod tests;

pub use error::LayerError;
pub use lockin::{ForAsLongAsWindow, resolve_affected_set};
pub use ordering::{OrderItem, OrderRecord, OrderStep, Selection};
pub use pipeline::{
    Evaluated, KEYWORD_COUNTER_KINDS, LayerEnvironment, MatchContext, ObjectBase, evaluate,
    evaluate_all, evaluate_with_order, keyword_counter_ability, keyword_counter_keyword, literal,
    literal_of, pt_counter_delta,
};
pub use post::{
    AppliedPlayerEffect, CostStages, DEFAULT_LAND_PLAYS, DEFAULT_MAX_HAND_SIZE, PlayerAffected,
    PlayerPassEffect, PlayerProperties, PostLayerEffects, PostLayerEnvironment, RuleEffect,
    RuleEntry, RuleSet, Verdict, build_rule_set, compute_player_properties,
};
pub use provider::LayeredProvider;
pub use registry::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerModification,
};
pub use timestamps::{
    CounterHolder, SimultaneousStamp, StampReason, TimestampLedger, restamp_static_effects,
    stamp_resolved_effect, stamp_static_and_resolved, static_effect_timestamp,
};
