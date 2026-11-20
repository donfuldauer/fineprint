// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Layer 1b: face-down spells and permanents (REQ-OBJ-003, REQ-LAYER-001; CR 613.2b, 613.2c,
//! 708).
//!
//! [`face_down_effect`] is the layer-1b effect that gives an object the characteristics of a
//! [`FaceDownProfile`] (CR 613.2b, 708.2). It replaces all characteristics and resets the
//! mana-value provenance to plain (a face-down object has no mana cost, CR 708.2a). Layer 1b runs
//! after layer 1a whatever the timestamps, so a face-down object that becomes a copy keeps the
//! profile (CR 708.10), and the result is the object's copiable values (CR 613.2c, 707.2): a copy
//! of a face-down object copies the profile and is itself face up (CR 707.3). Effects of layers
//! 2-7 and counters still apply to the face-down object.
//!
//! The same builder serves an object that enters the battlefield face down, is cast face down,
//! or is turned face down (CR 708.2a, 708.3, 708.4): registered before the object is evaluated as
//! a permanent or spell, it hides the face-up characteristics, so enters-the-battlefield abilities
//! of the face-up card neither trigger nor apply (CR 708.3) and casting checks see only the
//! profile (CR 708.4). A double-faced card may enter or be cast face down (CR 712.12).
//!
//! [`turn_face_down`] turns an existing permanent face down: it refuses, changing nothing, a
//! permanent that is already face down (CR 708.2b), a double-faced permanent (CR 712.13) and a
//! face-up merged permanent with a double-faced component (CR 731.2j); otherwise it stamps the
//! object (CR 613.7f) and registers the effect. [`turn_face_up`] removes the effect so the
//! copiable values revert (CR 708.8) (a shared effect is narrowed in place, keeping its links),
//! stamping the object again (CR 613.7f); it refuses an
//! instant or sorcery (CR 701.40g, 701.58g, 731.2g). Everything else registered on the object
//! stays (CR 708.8, 708.10).
//!
//! **Merged permanents** (CR 731.2a, 731.2e, 731.2f). A merged permanent has only its topmost
//! component's characteristics (layer 1a, [`crate::layers::merge_effect`]); a face-down effect on
//! the merged object applies after it in layer 1b. A face-down lower component contributes
//! nothing. The status of the whole permanent is the caller's, from the topmost component.
//!
//! **Not implemented here** (later units): the turn-face-up special action (CR 116.2b) and its
//! costs, "as ... is turned face up" power and toughness settings applied at turn-up
//! (CR 708.11; register them with [`crate::layers::modification_effect`]), turn-face-up
//! triggers, updating the zone record's `face_down` flag (the `TurnFaceUp`/`TurnFaceDown`
//! events do that) and the reveal-on-leave wiring ([`crate::layouts::must_reveal_on_leave`]).
//!
//! **Determinism.** No floats, clock or I/O; every failing call leaves its inputs unchanged
//! (REQ-CORE-006).

use std::collections::BTreeSet;
use std::fmt;

use fineprint_base::ids::{EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;

use super::error::LayerError;
use super::registry::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerModification,
};
use super::timestamps::{StampReason, TimestampLedger};
use crate::layouts::FaceDownProfile;

/// The layer-1b effect giving `object` the characteristics of `profile` (CR 613.2b, 708.2):
/// [`Layer::L1b`], `is_cda` false, [`Affected::Objects`]`({object})`,
/// [`LayerModification::FaceDown`] of the profile's characteristics, with `timestamp`.
///
/// One builder for every way to be face down: entering the battlefield face down (CR 708.3),
/// being cast face down (CR 708.4) and turning face down (CR 708.2a). Register it before the
/// object is evaluated as a permanent or spell. A double-faced card may enter or be cast face
/// down (CR 712.12); this builder does not refuse it.
#[must_use]
pub fn face_down_effect(
    profile: &FaceDownProfile,
    source: Option<ObjectId>,
    controller: PlayerId,
    origin: EffectOrigin,
    timestamp: Timestamp,
    object: ObjectId,
) -> ContinuousEffect {
    ContinuousEffect {
        source,
        controller,
        timestamp,
        // CR 613.2b: layer 1b.
        layer: Layer::L1b,
        is_cda: false,
        origin,
        affected: Affected::Objects(BTreeSet::from([object])),
        // CR 708.2: only the listed characteristics.
        modification: LayerModification::FaceDown(Box::new(profile.characteristics().clone())),
    }
}

/// What the caller knows about a permanent it would turn face down (CR 708.2b, 712.13, 731.2j).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FaceDownTarget {
    /// The permanent is already face down (CR 708.2b).
    pub already_face_down: bool,
    /// The permanent is double-faced (CR 712.13).
    pub double_faced_permanent: bool,
    /// The permanent is a face-up merged permanent with a double-faced component (CR 731.2j).
    pub merged_contains_double_faced: bool,
}

/// Why [`turn_face_down`] did nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FaceDownRefusal {
    /// A face-down permanent can't be turned face down (CR 708.2b).
    AlreadyFaceDown,
    /// A double-faced permanent can't be turned face down (CR 712.13).
    DoubleFacedPermanent,
    /// A face-up merged permanent with a double-faced component can't be turned face down
    /// (CR 731.2j).
    MergedContainsDoubleFaced,
    /// Stamping or registering failed.
    Layer(LayerError),
}

impl fmt::Display for FaceDownRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FaceDownRefusal::AlreadyFaceDown => f.write_str("the permanent is already face down"),
            FaceDownRefusal::DoubleFacedPermanent => {
                f.write_str("a double-faced permanent can't be turned face down")
            }
            FaceDownRefusal::MergedContainsDoubleFaced => f.write_str(
                "a merged permanent with a double-faced component can't be turned face down",
            ),
            FaceDownRefusal::Layer(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FaceDownRefusal {}

/// Why [`turn_face_up`] did nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FaceUpRefusal {
    /// The effect id is unknown, or its effect is not a [`LayerModification::FaceDown`] effect
    /// whose locked affected set contains the object.
    NotFaceDown,
    /// The permanent is represented by an instant or sorcery card: its controller reveals it and
    /// it stays face down; no timestamp, no trigger (CR 701.40g, 701.58g, 731.2g).
    InstantOrSorcery,
    /// Stamping or registering failed.
    Layer(LayerError),
}

impl fmt::Display for FaceUpRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FaceUpRefusal::NotFaceDown => f.write_str("no face-down effect on the object"),
            FaceUpRefusal::InstantOrSorcery => f.write_str("an instant or sorcery stays face down"),
            FaceUpRefusal::Layer(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FaceUpRefusal {}

/// Turns the existing permanent `object` face down with `profile` (CR 708.2a).
///
/// Refusals, checked in this order, change nothing: [`FaceDownRefusal::AlreadyFaceDown`]
/// (CR 708.2b), [`FaceDownRefusal::DoubleFacedPermanent`] (CR 712.13),
/// [`FaceDownRefusal::MergedContainsDoubleFaced`] (CR 731.2j). Otherwise the object receives a
/// new timestamp (CR 613.7f, [`StampReason::TurnedFaceUpOrDown`]), [`face_down_effect`] with that
/// timestamp is registered under `effect_id`, and the new timestamp is returned.
///
/// # Errors
/// A refusal above, or [`FaceDownRefusal::Layer`] with the error of stamping
/// ([`LayerError::IdExhausted`]) or of registering ([`LayerError::DuplicateEffect`]). On any
/// error `effects`, `ledger` and `ids` are unchanged.
#[allow(clippy::too_many_arguments)]
pub fn turn_face_down(
    effects: &mut ContinuousEffects,
    effect_id: EffectId,
    profile: &FaceDownProfile,
    target: &FaceDownTarget,
    source: Option<ObjectId>,
    controller: PlayerId,
    origin: EffectOrigin,
    object: ObjectId,
    ledger: &mut TimestampLedger,
    ids: &mut IdGenerator<Timestamp>,
) -> Result<Timestamp, FaceDownRefusal> {
    // CR 708.2b: nothing happens to a face-down permanent.
    if target.already_face_down {
        return Err(FaceDownRefusal::AlreadyFaceDown);
    }
    // CR 712.13: nothing happens to a double-faced permanent.
    if target.double_faced_permanent {
        return Err(FaceDownRefusal::DoubleFacedPermanent);
    }
    // CR 731.2j: nothing happens to a face-up merged permanent with a double-faced component.
    if target.merged_contains_double_faced {
        return Err(FaceDownRefusal::MergedContainsDoubleFaced);
    }
    // Work on copies and commit only after everything succeeded.
    let mut next_ledger = ledger.clone();
    let mut next_ids = ids.clone();
    // CR 613.7f: a permanent receives a new timestamp each time it turns face down.
    let ts = next_ledger
        .stamp_object(&mut next_ids, object, StampReason::TurnedFaceUpOrDown)
        .map_err(FaceDownRefusal::Layer)?;
    let effect = face_down_effect(profile, source, controller, origin, ts, object);
    // `register` leaves the registry unchanged on error.
    effects
        .register(effect_id, effect)
        .map_err(FaceDownRefusal::Layer)?;
    *ledger = next_ledger;
    *ids = next_ids;
    Ok(ts)
}

/// Turns the face-down permanent `object` face up by ending its face-down effect `effect_id`
/// (CR 708.8): its copiable values revert to its normal copiable values, which are the copied
/// values if it became a copy while face down (CR 708.10). Every other effect stays, since it is
/// the same object (CR 708.8). The object receives a new timestamp (CR 613.7f), returned.
///
/// If the effect's locked set holds other objects too, only `object` is taken out of it, in
/// place ([`ContinuousEffects::narrow_objects`]): the effect keeps its id and timestamp, and the
/// links of its same-as-layer parts are kept, so the others stay face down (CR 613.6, 708.8).
/// An effect left with no object is removed together with its links.
///
/// # Errors
/// [`FaceUpRefusal::NotFaceDown`] if `effect_id` is unknown, is not a
/// [`LayerModification::FaceDown`] effect, or its affected set is not a locked set containing
/// `object`; then [`FaceUpRefusal::InstantOrSorcery`] if `instant_or_sorcery` (CR 701.40g,
/// 701.58g, 731.2g); [`FaceUpRefusal::Layer`] with [`LayerError::IdExhausted`] if no timestamp is
/// left. On any error `effects`, `ledger` and `ids` are unchanged.
pub fn turn_face_up(
    effects: &mut ContinuousEffects,
    effect_id: EffectId,
    object: ObjectId,
    instant_or_sorcery: bool,
    ledger: &mut TimestampLedger,
    ids: &mut IdGenerator<Timestamp>,
) -> Result<Timestamp, FaceUpRefusal> {
    let effect = effects.get(effect_id).ok_or(FaceUpRefusal::NotFaceDown)?;
    if !matches!(effect.modification, LayerModification::FaceDown(_)) {
        return Err(FaceUpRefusal::NotFaceDown);
    }
    let remaining: BTreeSet<ObjectId> = match &effect.affected {
        Affected::Objects(set) if set.contains(&object) => {
            set.iter().copied().filter(|&o| o != object).collect()
        }
        _ => return Err(FaceUpRefusal::NotFaceDown),
    };
    // CR 701.40g, 701.58g, 731.2g: an instant or sorcery is revealed and stays face down.
    if instant_or_sorcery {
        return Err(FaceUpRefusal::InstantOrSorcery);
    }
    let mut next_ledger = ledger.clone();
    let mut next_ids = ids.clone();
    // CR 613.7f: a permanent receives a new timestamp each time it turns face up.
    let ts = next_ledger
        .stamp_object(&mut next_ids, object, StampReason::TurnedFaceUpOrDown)
        .map_err(FaceUpRefusal::Layer)?;
    // CR 708.8: the face-down characteristics end; the copiable values revert.
    if remaining.is_empty() {
        effects.remove(effect_id);
    } else {
        // CR 613.6, 708.8: narrow the anchor in place so the other objects stay face down and
        // every same-as-layer link to the anchor keeps applying to them.
        let mut next_effects = effects.clone();
        next_effects
            .narrow_objects(effect_id, remaining)
            .map_err(FaceUpRefusal::Layer)?;
        *effects = next_effects;
    }
    *ledger = next_ledger;
    *ids = next_ids;
    Ok(ts)
}
