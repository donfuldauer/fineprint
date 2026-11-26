// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The layer cache (REQ-LAYER-005, SRS 6.5): evaluated characteristics cached per
//! `(stateVersion, ObjectId)`, the full-recompute debug mode, and the digest the differential
//! test compares between the two modes.
//!
//! The cache is an optimisation only: a value it returns is always equal to a fresh
//! [`evaluate`](super::evaluate) of the same registry and environment, provided the caller
//! declares every environment change with [`LayerCache::invalidate`]. It is not a rules
//! procedure and changes no rules behaviour.
//!
//! **What is cached.** Finished [`Evaluated`] values (characteristics and controller after
//! layer 7d), one per object, keyed by the cache's current [`StateVersion`] and the
//! [`ObjectId`]. The per-evaluation lock-in record and the order record are not cached; they
//! stay inside the single per-object evaluation.
//!
//! **Invalidation.** Coarse by design: any change drops every entry and bumps the version by
//! one. An object's evaluation can depend on other objects (selector matching, the values of
//! characteristic-defining expressions, the dependency trial), so a change to one object cannot
//! safely be confined to that object's entry. Changes to the [`ContinuousEffects`] registry are
//! detected by the cache itself: it keeps a snapshot of the registry it last evaluated against
//! and invalidates with [`Invalidation::EffectRegistration`] when the registry passed in
//! differs. Everything else the cache cannot observe through the pure
//! [`LayerEnvironment`] methods, and the caller declares it with [`LayerCache::invalidate`]
//! (see [`Invalidation`] for the SRS 6.5 inputs each cause covers).
//!
//! **Expected evaluation cost.** Let `E` be the number of registered effects, `N` the number
//! of objects, and `k` the number of steps (effects of the layer and counters) one object has
//! in one layer.
//! - A board pass ([`super::board`]) runs every object through the eleven layers and
//!   sublayers. Per layer it lists the layer's effects once (`O(E)`), computes one
//!   foreign-source view per static effect of the layer whose source is on the board (one
//!   single-object run of the layer each, so at most `E` such runs per pass), and runs the
//!   layer for each of the `N` objects. One object's layer builds its `k` steps and orders them
//!   (REQ-LAYER-002): each selection asks the dependency relation lazily, so finding the
//!   earliest independent step usually costs `O(k)` trial applications (a copy of the object's
//!   characteristics and one application each), `O(k^2)` per layer; only when no step is
//!   independent (a dependency loop) is the whole relation, `O(k^2)` trials, computed. Each
//!   trial of a selector effect also asks [`LayerEnvironment::matches`]. A pass is therefore
//!   about `O(N * sum of k^2 over the layers)` trial applications plus `O(E * k)` for the
//!   foreign views, and copies each object's state once (at the start of layer 7a, moved, not
//!   cloned).
//! - A hit costs one comparison of the registry with the snapshot (`O(E)`, done once per
//!   [`LayerCache::evaluate_all`] call and once per [`LayerCache::evaluate`] call), one ordered
//!   map lookup (`O(log N)`) and one clone of the stored [`Evaluated`].
//! - A miss of [`LayerCache::evaluate`] costs the same comparison plus one board pass of
//!   [`evaluate`](super::evaluate) and the insertion; its result is the asked object's.
//! - [`LayerCache::evaluate_all`] answers all its misses from at most one board pass, run at
//!   the first miss: a call with `m` misses costs one pass plus `O(m log N)` insertions (no pass
//!   when every object is a hit).
//! - An invalidation costs dropping every entry (`O(N)`); a registry change also clones the
//!   registry once (`O(E)`) into the snapshot.
//! - A full-board recompute ([`LayerCache::evaluate_all`] after an invalidation) is therefore
//!   one board pass and `N` insertions; before #190 FP-147 it was `N` misses of one board pass
//!   each.
//!
//! Where the time goes (callgrind on one [`evaluate_all`](super::evaluate_all) of the
//! `layers_recompute` board: 200 permanents, 4 players, 21 effects in layers 4-7c with a
//! layer-4 dependency, locked sets, counters and characteristic-defining P/T). At commit
//! 1360900 a pass took about 1.24 G instructions: 97% in the ordering within layers, of which
//! 60% compared ability lists in the dependency trial (counting copies of every ability with
//! JSON equality, for every pair of steps), 14% copied the object's characteristics for trial
//! applications and 7% dropped them; copying every object's state and lock-in record at the
//! start of every layer took about 3%. After #190 FP-147 a pass takes about 166 M instructions
//! (7.4 times fewer): about 61% in the lazy dependency relation, most of it copying and
//! dropping the characteristics of trial applications (the `Characteristics` value with its
//! string sets and JSON abilities), 6% applying steps and 6% in the set and ability-list
//! comparisons; no per-layer board copy remains. Further gains need a cheaper copy of an
//! object's characteristics (outside this module).
//!
//! Benchmark medians of `cargo bench --locked -p fineprint-core --bench layers_recompute --
//! --noplot` (criterion `median`, same 4-core development machine and command for both, not the
//! CI reference runner, with other work running, so figures vary by about 15% between runs;
//! the CI `bench (ubuntu-latest)` artifacts are the trend of record, REQ-NFR-009). The cold and
//! after-invalidation figures at 1360900 are 200 board passes each:
//!
//! | benchmark | 1360900 | #190 FP-147 |
//! |---|---|---|
//! | `full_recompute_200` | 93.0 ms | 13.0 ms |
//! | `cached_cold_200` | 18.33 s | 16.8 ms |
//! | `cached_warm_200` | 421 µs | 380 µs |
//! | `cached_after_invalidation_200` | 18.44 s | 16.0 ms |
//!
//! The SRS target is at most 2 ms per full-board recompute for a 4-player board with 200
//! permanents on the reference runner (GitHub's standard hosted `ubuntu-latest`, REQ-NFR-009).
//! It is a target, not a correctness requirement: the `layers_recompute` benchmark measures it
//! and CI uploads the results; no test asserts it (R7).
//!
//! **Full-recompute mode.** With the cargo feature `full-recompute` ([`FULL_RECOMPUTE`]),
//! [`LayerCache::evaluate`] and [`LayerCache::evaluate_all`] always call the uncached
//! functions and store nothing; every call is a miss. Versions, invalidations and registry-change
//! detection behave exactly as in cached mode. Results are identical in both modes; CI compares
//! them with [`evaluated_digest`] (the `differential` job).
//!
//! **Determinism** (REQ-CORE-001). Only `BTreeMap` and `Option`; no floats, clock, I/O, threads,
//! global state or environment reads. The mode is chosen at compile time with `cfg!`.

use std::collections::BTreeMap;

use fineprint_base::encode::{CanonicalEncode, put_bytes, put_len, put_variant};
use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::ObjectId;
use serde::Serialize;

use fineprint_dsl::ability::Layer;

use super::board::{run_board, universe_known};
use super::error::LayerError;
use super::ordering::OrderStep;
use super::pipeline::{self, Evaluated, LayerEnvironment};
use super::registry::{ContinuousEffects, ManaValueProvenance, ValueSource};
use crate::objects::Characteristics;

/// Whether this build runs in full-recompute mode (the `full-recompute` cargo feature): the
/// cache stores nothing and every evaluation is computed afresh (REQ-LAYER-005). Fixed at
/// compile time; no environment variable is read.
pub const FULL_RECOMPUTE: bool = cfg!(feature = "full-recompute");

/// The version of the state the cache's entries belong to (REQ-LAYER-005). It starts at 0 and
/// grows by exactly 1 per invalidation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct StateVersion(u64);

impl StateVersion {
    /// The version number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Why the cache is invalidated: one variant per kind of change SRS 6.5 lists as able to
/// affect an effect's existence, selector or output (REQ-LAYER-005). Every variant drops every
/// entry; the variant documents the cause and keeps call sites readable.
///
/// The SRS 6.5 inputs map to variants as follows: zone changes to [`Invalidation::ZoneChange`];
/// control changes to [`Invalidation::ControlChange`]; counter changes to
/// [`Invalidation::CounterChange`]; effect registration to [`Invalidation::EffectRegistration`]
/// (detected by the cache itself); effect expiry to [`Invalidation::EffectExpiry`]; timestamps
/// to [`Invalidation::Timestamp`]; designations to [`Invalidation::Designation`]; attachment
/// changes to [`Invalidation::Attachment`]; status changes to [`Invalidation::Status`]; a change
/// to a player value a CDA or condition reads to [`Invalidation::PlayerValue`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Invalidation {
    /// An object entered or left the battlefield or another zone: a change to
    /// `ObjectBase::on_battlefield`, to an object's base (its base mana-value provenance,
    /// `LayerEnvironment::base_mana_value_provenance`, included), or to the list `objects()`
    /// returns.
    ZoneChange,
    /// An object's controller before layer 2 changed (`ObjectBase::controller`).
    ControlChange,
    /// Counters were put on or removed from an object (`LayerEnvironment::counters`).
    CounterChange,
    /// An effect was registered, removed, re-timestamped or the registry was replaced. The cache
    /// detects this itself by comparing the registry with its snapshot; a caller may also
    /// declare it.
    EffectRegistration,
    /// An effect ended (its duration expired or its source left). The caller removes the effect
    /// from the registry; declaring the expiry is also allowed.
    EffectExpiry,
    /// A timestamp changed: an effect's or a counter's timestamp
    /// (`LayerEnvironment::counter_timestamp`) or the ledger's.
    Timestamp,
    /// A designation was gained or lost (anything `LayerEnvironment::matches` may read).
    Designation,
    /// An object became attached or unattached (`LayerEnvironment::attached_here`).
    Attachment,
    /// A status changed: tapped, flipped, face up or down, phased in or out (anything
    /// `LayerEnvironment::matches` or `generating_ability_exists` may read).
    Status,
    /// A player value changed that a characteristic-defining expression
    /// (`LayerEnvironment::eval_value`) or a selector condition reads: life total, hand size,
    /// counts of objects, and the like.
    PlayerValue,
}

impl Invalidation {
    /// Every variant, in declaration order.
    pub const ALL: &'static [Invalidation] = &[
        Invalidation::ZoneChange,
        Invalidation::ControlChange,
        Invalidation::CounterChange,
        Invalidation::EffectRegistration,
        Invalidation::EffectExpiry,
        Invalidation::Timestamp,
        Invalidation::Designation,
        Invalidation::Attachment,
        Invalidation::Status,
        Invalidation::PlayerValue,
    ];
}

/// Counters of the cache's activity since it was created.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CacheStats {
    /// Evaluations answered from a stored entry.
    pub hits: u64,
    /// Evaluations computed afresh, failed ones included.
    pub misses: u64,
    /// Invalidations, declared or detected registry changes.
    pub invalidations: u64,
}

/// A cache of evaluated characteristics keyed by `(StateVersion, ObjectId)` (REQ-LAYER-005,
/// SRS 6.5). See the module documentation for the invalidation contract and the costs.
#[derive(Clone, Debug, Default)]
pub struct LayerCache {
    version: StateVersion,
    entries: BTreeMap<(StateVersion, ObjectId), Evaluated>,
    /// The registry of the last evaluation; `None` before the first.
    snapshot: Option<ContinuousEffects>,
    stats: CacheStats,
}

impl LayerCache {
    /// An empty cache at version 0.
    #[must_use]
    pub fn new() -> LayerCache {
        LayerCache::default()
    }

    /// The current state version.
    #[must_use]
    pub fn version(&self) -> StateVersion {
        self.version
    }

    /// The hit, miss and invalidation counts so far.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// The number of entries stored now (always 0 in full-recompute mode).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no entry is stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Declares a change of `cause`: bumps the version by exactly 1, drops every entry, counts
    /// one invalidation and returns the new version (REQ-LAYER-005).
    pub fn invalidate(&mut self, cause: Invalidation) -> StateVersion {
        // Coarse by design: `cause` does not narrow what is dropped (module documentation).
        let _ = cause;
        self.version = StateVersion(self.version.0 + 1);
        self.entries.clear();
        self.stats.invalidations += 1;
        self.version
    }

    /// The evaluation of `id` against `effects` and `env`, equal to
    /// [`evaluate`](super::evaluate). A registry that differs from the one of the last call
    /// first invalidates with [`Invalidation::EffectRegistration`] (never on the first call).
    /// A hit returns a clone of the stored entry; a miss evaluates and stores the result.
    ///
    /// # Errors
    /// The error of [`evaluate`](super::evaluate) (for example
    /// [`LayerError::UnknownObject`]); it counts as a miss and is not stored.
    pub fn evaluate(
        &mut self,
        effects: &ContinuousEffects,
        env: &dyn LayerEnvironment,
        id: ObjectId,
    ) -> Result<Evaluated, LayerError> {
        self.sync_registry(effects);
        self.lookup_or_evaluate(effects, env, id)
    }

    /// Every object of `env.objects()` by ascending [`ObjectId`] (a listed duplicate is looked up
    /// again, as before), equal to [`evaluate_all`](super::evaluate_all); each object is a hit or
    /// a miss as for [`LayerCache::evaluate`]. The registry is compared once per call.
    ///
    /// Every miss of the call is answered from at most one layer-major board pass over the
    /// objects of `env.objects()` that have a base, run at the first miss (REQ-LAYER-005); for
    /// each of those objects it gives exactly what [`evaluate`](super::evaluate) gives. In
    /// cached mode each miss stores its object's result as it is visited; in full-recompute mode
    /// nothing is stored.
    ///
    /// # Errors
    /// The first error of [`evaluate`](super::evaluate) by ascending [`ObjectId`], as
    /// [`evaluate_all`](super::evaluate_all): [`LayerError::UnknownObject`] for the first object
    /// without a base; entries computed before it stay stored, and the results of the objects
    /// after it are not stored.
    pub fn evaluate_all(
        &mut self,
        effects: &ContinuousEffects,
        env: &dyn LayerEnvironment,
    ) -> Result<BTreeMap<ObjectId, Evaluated>, LayerError> {
        self.sync_registry(effects);
        // REQ-CORE-001: ascending ids, never the environment's order, so the first error is the
        // one of the uncached `evaluate_all`.
        let mut ids = env.objects();
        ids.sort_unstable();
        let mut board: Option<BTreeMap<ObjectId, (Evaluated, Vec<OrderStep>)>> = None;
        let mut out = BTreeMap::new();
        for id in ids {
            let key = (self.version, id);
            if !FULL_RECOMPUTE && let Some(hit) = self.entries.get(&key) {
                self.stats.hits += 1;
                out.insert(id, hit.clone());
                continue;
            }
            // A failed lookup counts as a miss and is never stored.
            self.stats.misses += 1;
            // REQ-LAYER-005: one board pass for every miss of this call, run at the first.
            let board = board
                .get_or_insert_with(|| run_board(effects, env, universe_known(env), Layer::ALL));
            // Each object's result is taken once; a listed duplicate (a miss again only in
            // full-recompute mode) reuses the result already returned for it.
            let evaluated = match board.remove(&id) {
                Some((evaluated, _)) => evaluated,
                None => out.get(&id).cloned().ok_or(LayerError::UnknownObject(id))?,
            };
            if !FULL_RECOMPUTE {
                self.entries.insert(key, evaluated.clone());
            }
            out.insert(id, evaluated);
        }
        Ok(out)
    }

    /// Invalidates for a registry that differs from the snapshot and takes a new snapshot.
    fn sync_registry(&mut self, effects: &ContinuousEffects) {
        match &self.snapshot {
            Some(snapshot) if snapshot == effects => {}
            Some(_) => {
                self.invalidate(Invalidation::EffectRegistration);
                self.snapshot = Some(effects.clone());
            }
            None => self.snapshot = Some(effects.clone()),
        }
    }

    /// One object: the stored entry, or a fresh evaluation stored on success.
    fn lookup_or_evaluate(
        &mut self,
        effects: &ContinuousEffects,
        env: &dyn LayerEnvironment,
        id: ObjectId,
    ) -> Result<Evaluated, LayerError> {
        let key = (self.version, id);
        if !FULL_RECOMPUTE && let Some(hit) = self.entries.get(&key) {
            self.stats.hits += 1;
            return Ok(hit.clone());
        }
        self.stats.misses += 1;
        let evaluated = pipeline::evaluate(effects, env, id)?;
        if !FULL_RECOMPUTE {
            self.entries.insert(key, evaluated.clone());
        }
        Ok(evaluated)
    }
}

/// The SHA-256 digest (fineprint_base) of every `(ObjectId, Evaluated)` of `map`, used by the
/// differential test to compare cached and full-recompute results (REQ-LAYER-005).
///
/// The preimage is canonical and deterministic: the ASCII tag `fineprint.layers.evaluated.v2`,
/// the number of entries, then for each entry in ascending [`ObjectId`] order the object id,
/// the characteristics, the controller (`Option`) and the mana-value provenance
/// ([`Evaluated::mana_value_provenance`]: a variant tag, 0 plain, 1 fixed followed by the mana
/// value as `i64`, 2 a copy of a back face or melded permanent; then the flag that a fixed value
/// comes from a back face or melded permanent, as `bool`; then the colors pin
/// ([`ManaValueProvenance::colors_pinned`], CR 707.9b), as `bool`). The characteristics are encoded field by
/// field in declaration order exactly as LKI records encode them (decision "#24 FP-023 —
/// Characteristics encoding"): sets and integers through their canonical encoding, the mana
/// cost as its canonical text, color masks as their bits, and the DSL trees (rules text,
/// abilities, power, toughness) as tag 0 and their RFC 8785 canonical JSON (tag 1 and
/// MessagePack if canonical JSON is impossible, tag 2 if neither exists). Equal maps give equal
/// digests; a change to any characteristic, the controller or the provenance changes the
/// preimage.
#[must_use]
pub fn evaluated_digest(map: &BTreeMap<ObjectId, Evaluated>) -> Digest {
    let mut out = Vec::new();
    put_bytes(&mut out, b"fineprint.layers.evaluated.v2");
    put_len(&mut out, map.len());
    for (id, evaluated) in map {
        id.encode_canonical(&mut out);
        encode_characteristics(&evaluated.characteristics, &mut out);
        evaluated.controller.encode_canonical(&mut out);
        encode_provenance(&evaluated.mana_value_provenance, &mut out);
    }
    sha256(&out)
}

/// Appends the encoding of a mana-value provenance: the source's variant tag (and its mana
/// value for `Fixed`), then its back-face-or-melded flag, then its colors pin.
fn encode_provenance(p: &ManaValueProvenance, out: &mut Vec<u8>) {
    match &p.source {
        ValueSource::Plain => put_variant(out, 0),
        ValueSource::Fixed { mana_value } => {
            put_variant(out, 1);
            mana_value.encode_canonical(out);
        }
        ValueSource::CopyOfBackFaceOrMelded => put_variant(out, 2),
    }
    p.zero_when_copied.encode_canonical(out);
    p.colors_pinned.encode_canonical(out);
}

/// Appends a deterministic encoding of a serde value: tag 0 and its RFC 8785 canonical JSON;
/// otherwise tag 1 and its MessagePack form; tag 2 if neither exists. Equal values give equal
/// bytes.
fn put_serde<T: Serialize>(out: &mut Vec<u8>, value: &T) {
    if let Ok(bytes) = fineprint_dsl::canonical::to_canonical_vec(value) {
        put_variant(out, 0);
        put_bytes(out, &bytes);
    } else if let Ok(bytes) = rmp_serde::to_vec(value) {
        put_variant(out, 1);
        put_bytes(out, &bytes);
    } else {
        put_variant(out, 2);
    }
}

/// Appends the canonical encoding of `c`, field by field in declaration order (the layout of
/// the "#24 FP-023 — Characteristics encoding" decision).
fn encode_characteristics(c: &Characteristics, out: &mut Vec<u8>) {
    c.names.encode_canonical(out);
    c.mana_cost
        .as_ref()
        .map(ToString::to_string)
        .encode_canonical(out);
    c.color_indicator.map(|m| m.bits()).encode_canonical(out);
    c.colors.bits().encode_canonical(out);
    c.supertypes.encode_canonical(out);
    c.card_types.encode_canonical(out);
    c.subtypes.encode_canonical(out);
    put_serde(out, &c.rules_text);
    put_serde(out, &c.abilities);
    put_serde(out, &c.power);
    put_serde(out, &c.toughness);
    c.loyalty.encode_canonical(out);
    c.defense.encode_canonical(out);
    c.hand_modifier.encode_canonical(out);
    c.life_modifier.encode_canonical(out);
}
