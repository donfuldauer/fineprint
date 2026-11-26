// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The layer system (REQ-LAYER-001, REQ-LAYER-002, REQ-LAYER-003, REQ-CORE-002, SRS 6.1-6.3):
//! the continuous-effect registry, layer-major evaluation through layers 1a-7d, the ordering
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
//! **Layer-major evaluation across objects** (REQ-LAYER-001, CR 613.5). [`evaluate`],
//! [`evaluate_with_order`], [`evaluate_all`] and [`copiable_values`] run one board pass: every
//! object of the universe advances through each layer together. The universe is
//! `env.objects()` sorted ascending and de-duplicated, with the object asked for added, keeping
//! only objects with a base (an object without one is absent; it is an error only for the object
//! asked for, or for any listed object in [`evaluate_all`], the first by ascending id). Objects
//! are always processed by ascending [`ObjectId`], each with its own scratch state and lock-in
//! record, and read each other only through start-of-layer snapshots, so results never depend on
//! the order or duplicates of `env.objects()` and `evaluate(id) == evaluate_all()[id]`. At the
//! start of layer 7a every object after layers 1-6 is frozen into an [`EvaluatedView`] handed to
//! [`LayerEnvironment::eval_value_with_view`] for each non-literal power or toughness (CR 604.3,
//! 613.4a): it sees each object's controller after layer 2 (CR 613.1b) and its types, colors and
//! abilities after layers 3-6; power and toughness in it are those after layer 6 (a CDA never
//! reads another CDA's result: no recursion). A literal power or toughness never reaches the
//! environment. [`LayerEnvironment::generating_ability_exists`] gets `Some` source view for
//! every source in the universe: the object under evaluation its state so far; another object
//! its state through the end of the effect's own layer without the effect (CR 613.8a), one level
//! deep (inside, other sources are judged on their start-of-layer state). The dependency trial
//! (CR 613.8a) is still judged on the object under evaluation only, and a selector
//! ([`LayerEnvironment::matches`]) sees only [`MatchContext::candidate_view`].
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
//! **Text changes** (REQ-DSL-012, SRS 14.5). [`LayerModification::ReplaceTextWord`] is the
//! layer 3 text-changing effect `REPLACE_TEXT_WORD` (CR 612.1, 613.1c): it decodes the object's
//! rules text into a token stream ([`decode_rules_text`]), replaces every rewritable word of its
//! category whose value is exactly `from` by `to` ([`rewrite_text_words`]) and writes the text
//! back only if a word changed ([`encode_rules_text`]). Literal text, name references (CR 201.5,
//! 612.2) and card-name words are never rewritten; category `CARD_NAME` and empty or
//! noncharacter words are rejected at registration (CR 612.2). Text that is empty or does not
//! decode is left unchanged. Only the object's own text changes: abilities granted by effects
//! are not rewritten (CR 612.3). Chained text effects are ordered by the generic dependency
//! trial above (CR 613.8). AST nodes read rewritable words by token id: [`resolve_word_ref`],
//! [`text_token_matches`], [`resolve_predicate_words`] and [`resolve_selector_words`] read them
//! through the current table, so a dependent ability applies to different objects after a
//! change.
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
//! evaluation only (CR 611.3a). The record belongs to one object's evaluation and is
//! never cached or shared across objects; the [`LayerCache`] stores only finished [`Evaluated`]
//! values. [`ContinuousEffects::register_group_for_as_long_as`]
//! registers nothing when its [`ForAsLongAsWindow`] makes the effect do nothing (CR 611.2b).
//! [`ContinuousEffects::register`] never locks a selector and rejects `SameAsLayer`.
//!
//! **Layer 1a: copiable values** (REQ-OBJ-002, CR 613.2a, 613.2c, 707, 731.2a).
//! [`CopiableValues`] are an object's characteristics at the end of layer 1 with the source of
//! its mana value ([`ValueSource`], CR 202.3b, 202.3c). [`CopiableValues::copy_with`] gives what
//! a copy effect copies, with its [`CopyException`]s (CR 707.9a-d);
//! [`CopiableValues::modified`] applies a non-copy modification to values (CR 613.2a).
//! [`copiable_effect`] wraps either as a layer-1a effect that replaces all characteristics, a
//! snapshot taken when it is built (CR 707.2b, 707.2c): [`LayerModification::CopyValues`] for
//! plain values, [`LayerModification::CopyValuesWithProvenance`] for a fixed or zero mana value
//! or colors pinned by a copy exception (CR 707.9b, [`ManaValueProvenance::colors_pinned`]),
//! which [`Evaluated::mana_value_provenance`] and [`Evaluated::mana_value`] then report
//! (CR 202.3b, 202.3c; [`ManaValueProvenance`]). [`modification_effect`] is the layer-1a effect
//! of a non-copy modification ([`LayerModification::ApplyCopyExceptions`]): it changes only the
//! fields it sets (a `NotCopied` exception is ignored), so removing an earlier copy removes what
//! was copied. An "as ... enters" power and toughness setting is a [`modification_effect`]
//! registered with [`EffectOrigin::Resolved`] so it outlasts the ability (CR 208.2b, 611.2a), not
//! a [`copiable_effect`] snapshot, which would keep an earlier copy's characteristics after that
//! copy ends. [`merge_effect`] is the
//! layer-1a effect of a merged permanent stamped at the merge (CR 731.2a). Merge and copy
//! effects order by timestamp within layer 1a like any other effects; the evaluator has no
//! special case. [`copiable_values`] reads an object's copiable values with their mana-value
//! provenance, [`copiable_characteristics`] the characteristics only: layers 1a and 1b only, no
//! counters (CR 707.2). Both [`evaluate`] and [`copiable_values`] start from the base's
//! provenance, [`LayerEnvironment::base_mana_value_provenance`] (plain by default; the
//! environment supplies it for a nonmodal back face or melded permanent, CR 202.3b, 202.3c,
//! 713.5c). Reading `copiable_values` after registering [`copiable_effect`] of `V` gives `V`'s
//! characteristics and exported provenance (not necessarily a value `==` to `V`) when the effect
//! applies to the object and nothing else changes them before the end of layer 1.
//!
//! **Layer 1b: face-down** (REQ-OBJ-003, CR 613.2b, 613.2c, 708). [`face_down_effect`] is the
//! layer-1b effect giving an object the characteristics of a
//! [`crate::layouts::FaceDownProfile`] ([`LayerModification::FaceDown`], CR 708.2): it replaces
//! all characteristics, resets the mana-value provenance to plain, and applies after layer 1a
//! whatever the timestamps, so the profile is the object's copiable values (CR 613.2c, 707.2,
//! 708.10). The same builder serves entering the battlefield face down, being cast face down and
//! turning face down (CR 708.2a, 708.3, 708.4). [`turn_face_down`] refuses, changing nothing, a
//! face-down permanent (CR 708.2b), a double-faced permanent (CR 712.13) and a merged permanent
//! with a double-faced component (CR 731.2j) ([`FaceDownTarget`], [`FaceDownRefusal`]); otherwise
//! it stamps the object (CR 613.7f) and registers the effect. [`turn_face_up`] stamps the object
//! (CR 613.7f) and removes the effect, so the copiable values revert (CR 708.8); it refuses an
//! instant or sorcery (CR 701.40g, 701.58g, 731.2g, [`FaceUpRefusal`]). The turn-face-up special
//! action and its costs are later work.
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
//! **Cache** (REQ-LAYER-005, SRS 6.5). [`LayerCache`] caches [`Evaluated`] values keyed by
//! `(`[`StateVersion`]`, ObjectId)`; it detects registry changes itself and takes every other
//! change as an explicit [`LayerCache::invalidate`] with an [`Invalidation`] cause, which drops
//! every entry. [`LayerCache::evaluate_all`] answers all its misses from one board pass. The
//! cargo feature `full-recompute` ([`FULL_RECOMPUTE`]) turns storing off; CI compares both modes
//! through [`evaluated_digest`]. The evaluation cost, its profile and the benchmark figures are
//! in the [`LayerCache`] module documentation.
//!
//! **Scope.** The game state (`crate::state::GameState`) stores the [`ContinuousEffects`] and the
//! [`TimestampLedger`], hashes them (their canonical encodings, CR-005) and evaluates them
//! through `crate::state::GameEnvironment`; the cache keeps its own [`StateVersion`] counter and
//! is not part of the state. **Out of scope** (later units): calling [`LayerCache::invalidate`]
//! from the zone, control, counter, attachment and status mutation points; switching the call
//! sites in events, triggers, replacement and zones; forwarding the [`EvaluatedView`] from the
//! game state's environment (`crate::state::GameEnvironment` and the entering-permanent
//! projection still use the default [`LayerEnvironment::eval_value_with_view`]); effect expiry and durations (the caller removes ended effects; a "for as
//! long as" window is judged by the caller); typed player properties derived from pass A and the
//! cost calculation itself (the casting procedure).
//!
//! **Determinism.** Only `Vec`, `BTreeMap` and `BTreeSet`; no floats, clock or I/O; the order
//! of registration never changes a result (REQ-CORE-001).

mod board;
mod cache;
mod copy;
mod encode;
mod error;
mod facedown;
mod lockin;
mod ordering;
mod pipeline;
mod post;
mod provider;
mod registry;
mod text;
mod timestamps;

#[cfg(test)]
mod cache_tests;
#[cfg(test)]
mod copy_tests;
#[cfg(test)]
mod cross_object_tests;
#[cfg(test)]
mod encode_tests;
#[cfg(test)]
mod facedown_tests;
#[cfg(test)]
mod lockin_tests;
#[cfg(test)]
mod ordering_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod text_tests;

pub use board::EvaluatedView;
pub use cache::{
    CacheStats, FULL_RECOMPUTE, Invalidation, LayerCache, StateVersion, evaluated_digest,
};
pub use copy::{
    CopiableValues, CopiedCharacteristic, CopyException, ValueSource, copiable_characteristics,
    copiable_effect, copiable_values, merge_effect, modification_effect,
};
pub(crate) use encode::{effect_links, provenance_from_parts, provenance_parts, put_json};
pub use error::LayerError;
pub use facedown::{
    FaceDownRefusal, FaceDownTarget, FaceUpRefusal, face_down_effect, turn_face_down, turn_face_up,
};
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
    ManaValueProvenance,
};
pub use text::{
    TextWordError, decode_rules_text, encode_rules_text, resolve_predicate_words,
    resolve_selector_words, resolve_word_ref, rewrite_text_words, text_token_matches,
};
pub use timestamps::{
    CounterHolder, SimultaneousStamp, StampReason, TimestampLedger, restamp_static_effects,
    stamp_resolved_effect, stamp_static_and_resolved, static_effect_timestamp,
};
