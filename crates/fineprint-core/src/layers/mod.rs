// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The layer system (REQ-LAYER-001, REQ-CORE-002, SRS 6.1): the continuous-effect registry,
//! per-object evaluation through layers 1a-7d, timestamps and the LKI hook.
//!
//! **Registry.** [`ContinuousEffects`] holds every continuous effect under its [`EffectId`]
//! (fineprint_base) with its source, controller, timestamp, layer, origin, affected objects and
//! [`LayerModification`]. A modification must belong to its declared layer
//! ([`LayerModification::belongs_to`]).
//!
//! **Evaluation.** [`evaluate`] starts from the object's base characteristics (CR 613.1) as the
//! caller's [`LayerEnvironment`] supplies them and applies, in order, layers 1a, 1b, 2, 3, 4, 5,
//! 6, 7a, 7b, 7c and 7d (CR 613.1-613.4); within each, steps apply in timestamp order (CR 613.7),
//! ties broken effects before counters, then by ascending [`EffectId`], then by counter kind.
//! Selector effects are matched against the object as evaluated so far (CR 611.3a, 613.5);
//! [`Affected::Objects`] is a locked set (CR 611.2c). Keyword counters apply in layer 6
//! (CR 122.1b, 613.1f) and P/T counters in layer 7c (CR 122.1a, 613.4c). Layer 7 is the last
//! layer. Evaluation takes shared references only: the base is never mutated.
//!
//! **Timestamps.** [`TimestampLedger`] and the free functions assign timestamps per
//! CR 613.7a-n from the caller's single monotonic `IdGenerator<Timestamp>`.
//!
//! **LKI.** [`LayeredProvider`] implements [`crate::memory::CharacteristicsProvider`] with the
//! evaluated characteristics (SRS 2.3).
//!
//! **Scope.** This module is standalone: it is not wired into the game state, its Merkle layout
//! or the state hash (the placeholder `state::EffectRegistry` stays). **Out of scope** (later
//! units): dependency ordering (CR 613.8); characteristic-defining abilities first (CR 613.3,
//! 613.4a; `is_cda` is stored only); lock-in of an effect's set across layers (CR 613.6,
//! `SameAsLayer` is rejected as unsupported); the cached versus full-recompute mode; the passes
//! after the layers (CR 613.10, 613.11); wiring into the game state and the state hash;
//! switching the call sites in events, triggers, replacement and zones; effect expiry and
//! durations (the caller removes ended effects).
//!
//! **Determinism.** Only `Vec`, `BTreeMap` and `BTreeSet`; no floats, clock or I/O; the order
//! of registration never changes a result (REQ-CORE-001).

mod error;
mod pipeline;
mod provider;
mod registry;
mod timestamps;

#[cfg(test)]
mod tests;

pub use error::LayerError;
pub use pipeline::{
    Evaluated, KEYWORD_COUNTER_KINDS, LayerEnvironment, MatchContext, ObjectBase, evaluate,
    evaluate_all, keyword_counter_ability, literal, literal_of, pt_counter_delta,
};
pub use provider::LayeredProvider;
pub use registry::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerModification,
};
pub use timestamps::{
    CounterHolder, SimultaneousStamp, StampReason, TimestampLedger, restamp_static_effects,
    stamp_resolved_effect, stamp_static_and_resolved, static_effect_timestamp,
};
