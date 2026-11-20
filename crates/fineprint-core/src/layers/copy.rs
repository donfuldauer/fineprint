// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Copiable values and layer-1a effects (REQ-OBJ-002, REQ-LAYER-001; CR 613.2a, 613.2c, 707,
//! 731.2a).
//!
//! [`CopiableValues`] are an object's characteristics at the end of layer 1 (CR 613.2c, 707.2)
//! together with how its mana value is derived ([`ValueSource`], CR 202.3a-c, 713.5c). The
//! layout alternative characteristics (prototype, Room halves, Adventure, Omen, preparation)
//! are part of whatever [`Characteristics`] the layout selected and are carried unchanged
//! (CR 719.2a, 709.5b).
//!
//! [`CopiableValues::copy_with`] is what a copy effect gives: the copied values, then each
//! [`CopyException`] in order (CR 707.2, 707.3, 707.9a-d). [`CopiableValues::modified`] is a
//! copiable modification that is not a copy, such as an "as ... enters" ability setting power
//! and toughness (CR 613.2a, 707.2). [`copiable_effect`] wraps either in a layer-1a
//! [`ContinuousEffect`] that replaces all characteristics and carries the mana-value provenance
//! ([`ManaValueProvenance`]); [`modification_effect`] is the layer-1a effect of a non-copy
//! modification that changes only the fields it sets; [`merge_effect`] is the layer-1a effect
//! of a merge, stamped at the merge (CR 731.2a). The values are a snapshot taken when the effect
//! is built: changing the original afterwards changes no built effect (CR 707.2b, 707.2c). Merge and copy effects share
//! layer 1a and apply in timestamp order (CR 613.2); nothing in the evaluator is special-cased.
//!
//! [`copiable_values`] reads an object's copiable values, with their mana-value provenance, and
//! [`copiable_characteristics`] its copiable characteristics: its base characteristics after
//! layers 1a and 1b only, so a copy of a copy, of a face-down object (CR 707.2 example,
//! 708.2) and of a prototyped permanent (CR 719.3d) take the right values. Counters and the
//! effects of layers 2-7 are not copied (CR 707.2).
//!
//! **Not implemented here:** the exceptions of CR 707.9e, 707.9f and 707.9g (replacement-effect
//! and conditional exceptions); copies of spells (CR 707.10); tokens that copy double-faced
//! objects (CR 707.8a, 707.10g); face-down merged components (layer 1b, #39); the face-down
//! status itself (no `FaceDown` effect is created by this module: a copy is never face down).
//!
//! **Determinism.** Only `Vec`, `BTreeSet`; no floats, clock or I/O (REQ-CORE-001).

use std::collections::BTreeSet;

use fineprint_base::ids::{ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, LayoutBehavior, ValueExpressionAST};
use fineprint_dsl::mana::ManaCostDSL;
use fineprint_dsl::vocab::RulesVocabulary;

use super::error::LayerError;
use super::lockin::LockRecord;
use super::ordering::{Keyed, Scratch, run_layer};
use super::pipeline::{Evaluated, LayerEnvironment, Step};
pub use super::registry::ValueSource;
use super::registry::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerModification,
    ManaValueProvenance,
};
use crate::layouts::{FaceUp, LayoutError, MeldedPair, MergedPermanent};
use crate::objects::{
    CdaColors, Characteristics, ManaValueContext, ManaValueSubject, base_colors, mana_value,
};

/// An object's copiable values: its characteristics at the end of layer 1 (CR 613.2c) and the
/// source of its mana value.
///
/// A private flag records whether a [`ValueSource::Fixed`] source is a back face of a nonmodal
/// DFC or a melded permanent (a copy of it has mana value 0, CR 202.3b, 202.3c) or not (a
/// merged permanent whose top component is an ordinary object). It is meaningful only for a
/// `Fixed` source: [`CopiableValues::provenance`] exports it as `false` for any other source,
/// whatever its raw value (`==` still compares the raw flag).
///
/// A second private flag records whether the colors were set or kept by a copy exception
/// (CR 707.9b; [`ManaValueProvenance::colors_pinned`]), so a later
/// [`CopyException::SetManaCost`] leaves them; it is `false` for the constructors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopiableValues {
    /// The characteristics (CR 707.2).
    pub characteristics: Characteristics,
    /// How the mana value is derived.
    pub source: ValueSource,
    /// `Fixed` from a nonmodal back face or melded permanent: a copy has mana value 0. Ignored
    /// for any other source.
    zero_when_copied: bool,
    /// The colors were set or kept by a copy exception (CR 707.9b).
    colors_pinned: bool,
}

/// One "except ..." clause of a copy effect, applied in slice order (CR 707.9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyException {
    /// CR 707.9a: the copy gains the ability (appended to the abilities); it is part of the
    /// copy's copiable values.
    GainAbility(AbilityAST),
    /// CR 707.9b: the names become exactly these.
    SetNames(BTreeSet<String>),
    /// CR 707.9b: the mana cost becomes this. The colors are re-derived from the new cost and
    /// the color indicator only if they are not pinned (set or kept by an earlier exception of
    /// the same effect, [`CopyException::SetColors`] or [`CopyException::NotCopied`] of
    /// [`CopiedCharacteristic::Colors`], or by an earlier copy or modification whose values
    /// carry the pin, [`ManaValueProvenance::colors_pinned`]) and they equalled that derivation
    /// of the old cost; otherwise they are left (docs/DECISIONS.md "#38 FP-037 — SetManaCost and
    /// colors", "#38 FP-037 — CodeRabbit round 1: colors pinned by SetColors and
    /// NotCopied(Colors)" and "#38 FP-037 — CodeRabbit round 2: explicit colors carried with the
    /// copiable values").
    SetManaCost(Option<ManaCostDSL>),
    /// CR 707.9b, 707.9d: the colors become exactly these and the color indicator is not
    /// copied (cleared).
    SetColors(ColorBitmask),
    /// CR 707.9b: the supertypes become exactly these.
    SetSupertypes(BTreeSet<String>),
    /// CR 707.9b: the card types become exactly these.
    SetCardTypes(BTreeSet<String>),
    /// CR 707.9b: the subtypes become exactly these.
    SetSubtypes(BTreeSet<String>),
    /// CR 707.9d last sentence: these supertypes "in addition to its other types" (union).
    AddSupertypes(BTreeSet<String>),
    /// CR 707.9d last sentence: these card types "in addition to its other types" (union).
    AddCardTypes(BTreeSet<String>),
    /// CR 707.9d last sentence: these subtypes "in addition to its other types" (union).
    AddSubtypes(BTreeSet<String>),
    /// CR 707.9b, 707.9d: the power becomes this expression (a copied CDA expression is
    /// replaced).
    SetPower(ValueExpressionAST),
    /// CR 707.9b, 707.9d: the toughness becomes this expression (a copied CDA expression is
    /// replaced).
    SetToughness(ValueExpressionAST),
    /// CR 707.9c: the characteristic is not copied; it keeps the copier's own value. Ignored
    /// (no change, no pin) by a modification that is not a copy ([`CopiableValues::modified`],
    /// [`modification_effect`]).
    NotCopied(CopiedCharacteristic),
}

/// A characteristic a [`CopyException::NotCopied`] names (CR 707.9c).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CopiedCharacteristic {
    /// The names.
    Names,
    /// The mana cost.
    ManaCost,
    /// The color indicator.
    ColorIndicator,
    /// The colors; the color indicator is kept with them (CR 707.9d).
    Colors,
    /// The supertypes.
    Supertypes,
    /// The card types.
    CardTypes,
    /// The subtypes.
    Subtypes,
    /// The rules text.
    RulesText,
    /// The abilities.
    Abilities,
    /// The power.
    Power,
    /// The toughness.
    Toughness,
    /// The loyalty.
    Loyalty,
    /// The defense.
    Defense,
    /// The hand modifier.
    HandModifier,
    /// The life modifier.
    LifeModifier,
}

/// A characteristic a characteristic-defining ability can define, for CR 707.9d.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CdaTarget {
    /// The type line (layer 4).
    Types,
    /// The colors (layer 5).
    Colors,
    /// Power and toughness (layer 7a).
    PowerToughness,
}

impl CopyException {
    /// The characteristic whose copied CDA this exception stops from being copied (CR 707.9d):
    /// a set value or a value not copied. `Add*` types keep their CDAs (707.9d last sentence).
    ///
    /// Any type-line exception drops any layer-4 CDA: the placeholder ability data does not say
    /// which part of the type line a layer-4 CDA defines; per-part targets are tracked in #198.
    fn cda_target(&self) -> Option<CdaTarget> {
        match self {
            CopyException::SetColors(_)
            | CopyException::NotCopied(CopiedCharacteristic::Colors) => Some(CdaTarget::Colors),
            CopyException::SetSupertypes(_)
            | CopyException::SetCardTypes(_)
            | CopyException::SetSubtypes(_)
            | CopyException::NotCopied(
                CopiedCharacteristic::Supertypes
                | CopiedCharacteristic::CardTypes
                | CopiedCharacteristic::Subtypes,
            ) => {
                // Any type-line exception drops any layer-4 CDA: the placeholder ability data
                // does not say which part of the type line a layer-4 CDA defines; per-part
                // targets are tracked in #198.
                Some(CdaTarget::Types)
            }
            CopyException::SetPower(_)
            | CopyException::SetToughness(_)
            | CopyException::NotCopied(
                CopiedCharacteristic::Power | CopiedCharacteristic::Toughness,
            ) => Some(CdaTarget::PowerToughness),
            _ => None,
        }
    }
}

/// The characteristics a characteristic-defining ability defines, read from the placeholder
/// ability JSON: a `"STATIC"` ability with a `"CHAR"` effect marked `"isCDA": true` defines the
/// characteristic of its layer (4: types, 5: colors, 7a: power and toughness). Reads ability
/// data only, never card identity (R4).
fn cda_targets(ability: &AbilityAST) -> BTreeSet<CdaTarget> {
    let mut out = BTreeSet::new();
    let Some(object) = ability.0.as_object() else {
        return out;
    };
    if object.get("kind").and_then(|k| k.as_str()) != Some("STATIC") {
        return out;
    }
    let Some(effects) = object.get("effects").and_then(|e| e.as_array()) else {
        return out;
    };
    for effect in effects {
        let is_cda_char = effect.get("type").and_then(|t| t.as_str()) == Some("CHAR")
            && effect.get("isCDA").and_then(|b| b.as_bool()) == Some(true);
        if !is_cda_char {
            continue;
        }
        match effect.get("layer").and_then(|l| l.as_str()) {
            Some("4") => out.insert(CdaTarget::Types),
            Some("5") => out.insert(CdaTarget::Colors),
            Some("7a") => out.insert(CdaTarget::PowerToughness),
            _ => false,
        };
    }
    out
}

/// Applies `exceptions` in slice order to `c` (CR 707.9a-d) and returns the final colors pin.
///
/// `own` is the copier's values for [`CopyException::NotCopied`] (CR 707.9c) in a copy; `None`
/// for a modification that is not a copy, where `NotCopied` is skipped (no field changes, no
/// pin). `colors_pinned` is the pin of the values `c` holds: colors set or kept by an exception
/// (here or in an earlier copy or modification) are a copiable value of their own (CR 707.9b),
/// and a [`CopyException::SetManaCost`] does not re-derive them. `SetColors` and, in a copy,
/// `NotCopied` of the colors set the pin; nothing clears it.
pub(super) fn apply_exceptions(
    c: &mut Characteristics,
    own: Option<&Characteristics>,
    exceptions: &[CopyException],
    colors_pinned: bool,
) -> bool {
    // CR 707.9b, 707.9c, 707.9d: colors an exception set or kept are final.
    let mut colors_pinned = colors_pinned;
    for exception in exceptions {
        match exception {
            // CR 707.9a: the gained ability becomes part of the copiable values.
            CopyException::GainAbility(a) => c.abilities.push(a.clone()),
            // CR 707.9b: the final set of values becomes part of the copiable values.
            CopyException::SetNames(n) => c.names.clone_from(n),
            CopyException::SetManaCost(cost) => {
                let derived_old =
                    base_colors(c.mana_cost.as_ref(), c.color_indicator, CdaColors::None);
                if !colors_pinned && c.colors == derived_old {
                    c.colors = base_colors(cost.as_ref(), c.color_indicator, CdaColors::None);
                }
                c.mana_cost.clone_from(cost);
            }
            // CR 707.9b, 707.9d: a set color; the color indicator is not copied.
            CopyException::SetColors(m) => {
                c.colors = *m;
                c.color_indicator = None;
                colors_pinned = true;
            }
            CopyException::SetSupertypes(s) => c.supertypes.clone_from(s),
            CopyException::SetCardTypes(s) => c.card_types.clone_from(s),
            CopyException::SetSubtypes(s) => c.subtypes.clone_from(s),
            // CR 707.9d last sentence: "in addition to its other types".
            CopyException::AddSupertypes(s) => c.supertypes.extend(s.iter().cloned()),
            CopyException::AddCardTypes(s) => c.card_types.extend(s.iter().cloned()),
            CopyException::AddSubtypes(s) => c.subtypes.extend(s.iter().cloned()),
            // CR 707.9b, 707.9d: the expression replaces any copied CDA expression.
            CopyException::SetPower(v) => c.power = Some(v.clone()),
            CopyException::SetToughness(v) => c.toughness = Some(v.clone()),
            // CR 707.9c: the copier keeps its original value; a modification that is not a
            // copy has no original to keep and ignores it.
            CopyException::NotCopied(field) => {
                if let Some(own) = own {
                    keep_own(c, own, *field);
                    if *field == CopiedCharacteristic::Colors {
                        colors_pinned = true;
                    }
                }
            }
        }
    }
    colors_pinned
}

/// CR 707.9c, 707.9d: `field` of `c` takes `own`'s value (colors with the color indicator).
fn keep_own(c: &mut Characteristics, own: &Characteristics, field: CopiedCharacteristic) {
    match field {
        CopiedCharacteristic::Names => c.names.clone_from(&own.names),
        CopiedCharacteristic::ManaCost => c.mana_cost.clone_from(&own.mana_cost),
        CopiedCharacteristic::ColorIndicator => c.color_indicator = own.color_indicator,
        // CR 707.9d: if that characteristic is color, the color indicator is not copied either.
        CopiedCharacteristic::Colors => {
            c.colors = own.colors;
            c.color_indicator = own.color_indicator;
        }
        CopiedCharacteristic::Supertypes => c.supertypes.clone_from(&own.supertypes),
        CopiedCharacteristic::CardTypes => c.card_types.clone_from(&own.card_types),
        CopiedCharacteristic::Subtypes => c.subtypes.clone_from(&own.subtypes),
        CopiedCharacteristic::RulesText => c.rules_text.clone_from(&own.rules_text),
        CopiedCharacteristic::Abilities => c.abilities.clone_from(&own.abilities),
        CopiedCharacteristic::Power => c.power.clone_from(&own.power),
        CopiedCharacteristic::Toughness => c.toughness.clone_from(&own.toughness),
        CopiedCharacteristic::Loyalty => c.loyalty = own.loyalty,
        CopiedCharacteristic::Defense => c.defense = own.defense,
        CopiedCharacteristic::HandModifier => c.hand_modifier = own.hand_modifier,
        CopiedCharacteristic::LifeModifier => c.life_modifier = own.life_modifier,
    }
}

impl CopiableValues {
    /// Copiable values whose mana value is the characteristics' own ([`ValueSource::Plain`]).
    #[must_use]
    pub fn plain(characteristics: Characteristics) -> CopiableValues {
        CopiableValues {
            characteristics,
            source: ValueSource::Plain,
            zero_when_copied: false,
            colors_pinned: false,
        }
    }

    /// The back face of a nonmodal double-faced permanent: its mana value is that of the front
    /// face's cost, X counting 0 (CR 202.3b, 202.3e); a copy of it has mana value 0.
    #[must_use]
    pub fn nonmodal_back_face(
        characteristics: Characteristics,
        front_mana_cost: Option<ManaCostDSL>,
    ) -> CopiableValues {
        // CR 202.3b: calculated as though it had the front face's mana cost.
        let mv = mana_value(
            &ManaValueSubject::NonmodalBackFace {
                front_mana_cost: front_mana_cost.as_ref(),
            },
            ManaValueContext::OffStack,
        );
        CopiableValues {
            characteristics,
            source: ValueSource::Fixed { mana_value: mv },
            zero_when_copied: true,
            colors_pinned: false,
        }
    }

    /// A melded permanent: the combined back face's characteristics and the sum of the front
    /// faces' mana values (CR 713.5c, 202.3c); a copy of it has mana value 0.
    ///
    /// # Errors
    /// The [`LayoutError`] of [`MeldedPair::characteristics`].
    pub fn melded(
        pair: &MeldedPair<'_>,
        vocab: &RulesVocabulary,
    ) -> Result<CopiableValues, LayoutError> {
        let characteristics = pair.characteristics(vocab)?;
        // CR 202.3c, 713.5c: the combined cost of the front faces.
        let mv = pair.mana_value(false, vocab)?;
        Ok(CopiableValues {
            characteristics,
            source: ValueSource::Fixed { mana_value: mv },
            zero_when_copied: true,
            colors_pinned: false,
        })
    }

    /// A merged permanent: the topmost component's characteristics, plus every component's
    /// abilities when mutated ([`MergedPermanent::base_characteristics`], CR 731.2a, 702.140e),
    /// and the topmost component's mana value. When the topmost component is a nonmodal
    /// double-faced card with its back face up or copying a back face, the source is always
    /// [`ValueSource::Fixed`] and the value copies as a back face (mana value 0, CR 202.3b), even
    /// when the numbers match. Otherwise the source is [`ValueSource::Plain`] when that mana
    /// value equals the characteristics' own off the stack, else `Fixed`, and it copies as
    /// `Plain`.
    ///
    /// # Errors
    /// The [`LayoutError`] of [`MergedPermanent::base_characteristics`] or
    /// [`MergedPermanent::mana_value`].
    pub fn merged(
        merged: &MergedPermanent<'_>,
        vocab: &RulesVocabulary,
    ) -> Result<CopiableValues, LayoutError> {
        // CR 731.2a, 702.140e.
        let characteristics = merged.base_characteristics(vocab)?;
        let mv = merged.mana_value(vocab)?;
        let own = characteristics.mana_value(ManaValueContext::OffStack);
        let top = &merged.components()[0];
        // CR 202.3b: a back-face top keeps its classification even when the numbers match.
        let back_face_top = top.def.layout == LayoutBehavior::DfcNonmodal
            && (top.query.status.face_up == FaceUp::Back || top.query.status.copying_back_face);
        if mv == own && !back_face_top {
            return Ok(CopiableValues::plain(characteristics));
        }
        Ok(CopiableValues {
            characteristics,
            source: ValueSource::Fixed { mana_value: mv },
            zero_when_copied: back_face_top,
            colors_pinned: false,
        })
    }

    /// The mana value (CR 202.3): [`ValueSource::Plain`] the characteristics' own in `ctx`
    /// (X counts 0 off the stack and the chosen number on it, CR 202.3e); `Fixed` the stored
    /// value; [`ValueSource::CopyOfBackFaceOrMelded`] 0 (CR 202.3b, 202.3c).
    #[must_use]
    pub fn mana_value(&self, ctx: ManaValueContext) -> i64 {
        // CR 202.3, 202.3b, 202.3c, 202.3e.
        self.provenance().mana_value(&self.characteristics, ctx)
    }

    /// The provenance of the mana value: the source, whether a copy has mana value 0
    /// (CR 202.3b, 202.3c), and whether the colors are pinned (CR 707.9b). The zero flag is
    /// exported only for a [`ValueSource::Fixed`] source and is `false` for any other, so values
    /// whose `source` was reassigned export a consistent provenance.
    #[must_use]
    pub fn provenance(&self) -> ManaValueProvenance {
        ManaValueProvenance {
            source: self.source.clone(),
            // CR 202.3b, 202.3c: the flag only qualifies a fixed mana value.
            zero_when_copied: matches!(self.source, ValueSource::Fixed { .. })
                && self.zero_when_copied,
            colors_pinned: self.colors_pinned,
        }
    }

    /// Copiable values from characteristics and a mana-value provenance, for example those of
    /// an [`Evaluated`] ([`Evaluated::mana_value_provenance`]); [`copiable_values`] builds them
    /// this way. The inverse of [`CopiableValues::provenance`].
    #[must_use]
    pub fn with_provenance(
        characteristics: Characteristics,
        provenance: ManaValueProvenance,
    ) -> CopiableValues {
        CopiableValues {
            characteristics,
            source: provenance.source,
            zero_when_copied: provenance.zero_when_copied,
            colors_pinned: provenance.colors_pinned,
        }
    }

    /// What a copy effect gives (CR 707.2, 707.3, 707.9): this object's copiable values, then
    /// each exception in the order given. `own` is the copier's values before copying (for
    /// [`CopyException::NotCopied`], CR 707.9c).
    ///
    /// Only the characteristics are copied: no status, counter or non-copy effect (CR 707.2).
    /// When an exception sets, keeps or does not copy the types, the colors or power and
    /// toughness, a copied characteristic-defining ability that defines it is not copied
    /// (CR 707.9d; recognised from the ability data, see `cda_targets`); `Add*` type exceptions
    /// keep those abilities. Gained abilities are never dropped.
    ///
    /// The result's source is [`ValueSource::CopyOfBackFaceOrMelded`] if this one is
    /// `CopyOfBackFaceOrMelded` or a `Fixed` from a nonmodal back face or melded permanent
    /// (CR 202.3b, 202.3c), whatever the exceptions; otherwise [`ValueSource::Plain`]. The
    /// result's colors are pinned (CR 707.9b) if this one's are or an exception sets or keeps
    /// them ([`CopyException::SetColors`], [`CopyException::NotCopied`] of the colors); a
    /// [`CopyException::SetManaCost`] re-derives only unpinned colors.
    #[must_use]
    pub fn copy_with(&self, own: &Characteristics, exceptions: &[CopyException]) -> CopiableValues {
        // CR 707.2: the copiable values only.
        let mut c = self.characteristics.clone();
        // CR 707.9d: a CDA defining a characteristic the effect sets, keeps or does not copy
        // is not copied.
        let dropped: BTreeSet<CdaTarget> = exceptions
            .iter()
            .filter_map(CopyException::cda_target)
            .collect();
        if !dropped.is_empty() {
            c.abilities
                .retain(|a| cda_targets(a).intersection(&dropped).next().is_none());
        }
        // CR 707.9a-c: the exceptions, in order; CR 707.9b: pinned colors stay pinned.
        let pinned = apply_exceptions(&mut c, Some(own), exceptions, self.colors_pinned);
        let zero = self.provenance().copies_as_zero();
        CopiableValues {
            characteristics: c,
            // CR 202.3b, 202.3c: a copy of a back face or melded permanent has mana value 0.
            source: if zero {
                ValueSource::CopyOfBackFaceOrMelded
            } else {
                ValueSource::Plain
            },
            zero_when_copied: false,
            colors_pinned: self.colors_pinned || pinned,
        }
    }

    /// A copiable (layer 1a) modification that is not a copy ("as ... enters" setting power and
    /// toughness, CR 613.2a, 707.2): these values with the exceptions applied in order, source
    /// and zero flag unchanged. A [`CopyException::NotCopied`] is ignored (no change, no pin),
    /// and no ability is dropped. The colors are pinned (CR 707.9b) if these are or a
    /// [`CopyException::SetColors`] sets them.
    ///
    /// The result is a complete snapshot: [`copiable_effect`] of it replaces all characteristics,
    /// copied ones included. The layer-1a effect that carries only the fields the modification
    /// sets is [`modification_effect`].
    ///
    /// An "as ... enters" or "as ... is turned face up" power and toughness setting
    /// (CR 208.2b; replacement effects, CR 614.1c, 614.1e) built from these values must be
    /// registered with [`EffectOrigin::Resolved`]: once applied it lasts (CR 611.2a) and the
    /// evaluator must not drop it when the permanent loses the ability, as it drops a
    /// static-ability effect (CR 611.3b).
    #[must_use]
    pub fn modified(&self, exceptions: &[CopyException]) -> CopiableValues {
        let mut c = self.characteristics.clone();
        // CR 613.2a, 707.2: not a copy, so `NotCopied` has nothing to keep.
        let pinned = apply_exceptions(&mut c, None, exceptions, self.colors_pinned);
        CopiableValues {
            characteristics: c,
            source: self.source.clone(),
            zero_when_copied: self.zero_when_copied,
            colors_pinned: self.colors_pinned || pinned,
        }
    }
}

/// The copiable characteristics of `id`: the object's characteristics after layers 1a and 1b
/// only (CR 613.2c, 707.2). Counters and effects of layers 2-7 do not apply, and power and
/// toughness stay as expressions (a characteristic-defining expression is not evaluated).
///
/// The two sublayers are run exactly as [`crate::layers::evaluate`] runs them (same ordering,
/// affected sets, lock-in and generating-ability checks), so layer 1a applies before 1b
/// (CR 613.2a, 613.2b): a face-down permanent that becomes a copy stays face down (CR 708.10)
/// and a copy of a face-down permanent copies its face-down values (CR 708.2).
///
/// This gives the characteristics only: wrapping them with [`CopiableValues::plain`] loses a
/// fixed or zero mana value (CR 202.3b, 202.3c). [`copiable_values`] is the same run with the
/// mana-value provenance, and is what a copy of `id` should copy.
///
/// # Errors
/// [`LayerError::UnknownObject`] if the environment has no base for `id`.
pub fn copiable_characteristics(
    effects: &ContinuousEffects,
    env: &dyn LayerEnvironment,
    id: ObjectId,
) -> Result<Characteristics, LayerError> {
    layer_one(effects, env, id).map(|state| state.characteristics)
}

/// The copiable values of `id`: the characteristics of [`copiable_characteristics`] (the same
/// layer 1a and 1b run, CR 613.2c, 707.2) with the mana-value provenance of that run
/// ([`Evaluated::mana_value_provenance`]): plain for the base, set by each layer-1a effect that
/// replaces all characteristics, kept by a [`modification_effect`] (which may only pin the
/// colors, CR 707.9b), plain again after a face-down effect (CR 708.2, 202.3a). For any values
/// `V`, registering [`copiable_effect`] of `V` on an object and reading `copiable_values` gives
/// `V`'s characteristics and provenance (colors pin included) back when no other applicable
/// effect changes the characteristics or the provenance before the end of layer 1: a layer-1b
/// `FaceDown` effect, a later layer-1a copy or modification, or an earlier one that is not
/// replaced.
///
/// # Errors
/// [`LayerError::UnknownObject`] if the environment has no base for `id`.
pub fn copiable_values(
    effects: &ContinuousEffects,
    env: &dyn LayerEnvironment,
    id: ObjectId,
) -> Result<CopiableValues, LayerError> {
    let state = layer_one(effects, env, id)?;
    Ok(CopiableValues::with_provenance(
        state.characteristics,
        state.mana_value_provenance,
    ))
}

/// `id` after layers 1a and 1b only (CR 613.2, 613.2c).
fn layer_one(
    effects: &ContinuousEffects,
    env: &dyn LayerEnvironment,
    id: ObjectId,
) -> Result<Evaluated, LayerError> {
    // CR 613.1: start with the actual object.
    let base = env.base(id).ok_or(LayerError::UnknownObject(id))?;
    let mut scratch = Scratch {
        state: Evaluated {
            characteristics: base.characteristics,
            controller: base.controller,
            mana_value_provenance: ManaValueProvenance::plain(),
        },
        pt: None,
    };
    let mut locks = LockRecord::new(effects);
    let mut record = Vec::new();
    // CR 613.2, 613.2c: layer 1 only; its result is the copiable values.
    for layer in [Layer::L1a, Layer::L1b] {
        let steps: Vec<Keyed<'_>> = effects
            .iter()
            .filter(|(_, e)| e.layer == layer)
            .map(|(eid, e)| (e.timestamp, Step::Effect(eid, e)))
            .collect();
        run_layer(layer, steps, env, id, &mut scratch, &mut locks, &mut record);
    }
    Ok(scratch.state)
}

/// A layer-1a continuous effect (CR 613.2a) carrying these copiable values, with the given
/// timestamp (CR 613.7b): [`Layer::L1a`], `is_cda` false, [`Affected::Objects`]`(objects)`.
/// The modification is [`LayerModification::CopyValues`] when the values' provenance is plain,
/// otherwise [`LayerModification::CopyValuesWithProvenance`], so the evaluated object and
/// [`copiable_values`] keep a fixed or zero mana value (CR 202.3b, 202.3c) and pinned colors
/// (CR 707.9b). Either replaces all characteristics. The values are a snapshot (CR 707.2b,
/// 707.2c); for a modification that changes only the fields it sets, use
/// [`modification_effect`].
///
/// An "as ... enters" or "as ... is turned face up" power and toughness setting (CR 208.2b;
/// replacement effects, CR 614.1c, 614.1e) must be registered with [`EffectOrigin::Resolved`]:
/// once applied it lasts (CR 611.2a) and the evaluator must not drop it when the permanent loses
/// the ability, as it drops a static-ability effect (CR 611.3b).
#[must_use]
pub fn copiable_effect(
    values: &CopiableValues,
    source: Option<ObjectId>,
    controller: PlayerId,
    origin: EffectOrigin,
    timestamp: Timestamp,
    objects: BTreeSet<ObjectId>,
) -> ContinuousEffect {
    ContinuousEffect {
        source,
        controller,
        timestamp,
        layer: Layer::L1a,
        is_cda: false,
        origin,
        affected: Affected::Objects(objects),
        modification: copy_modification(values),
    }
}

/// The layer-1a modification of `values`: plain values as [`LayerModification::CopyValues`],
/// any other provenance as [`LayerModification::CopyValuesWithProvenance`].
fn copy_modification(values: &CopiableValues) -> LayerModification {
    let characteristics = Box::new(values.characteristics.clone());
    let provenance = values.provenance();
    if provenance.is_plain() {
        LayerModification::CopyValues(characteristics)
    } else {
        LayerModification::CopyValuesWithProvenance {
            characteristics,
            provenance,
        }
    }
}

/// A layer-1a continuous effect (CR 613.2a, 707.2) for a copiable modification that is not a
/// copy, such as an "as ... enters" ability setting power and toughness: the `exceptions`, in
/// order, applied to the object's characteristics as layer 1a has them at this effect's
/// timestamp ([`LayerModification::ApplyCopyExceptions`]). Only the fields the exceptions set
/// change, so removing an earlier copy effect also removes what it copied while the set values
/// stay; a later copy effect replaces all characteristics, these included (CR 613.2, timestamp
/// order). A [`CopyException::NotCopied`] is ignored (no change, no pin); no ability is
/// dropped; the mana-value source and zero flag are kept, and a [`CopyException::SetColors`]
/// pins the colors (CR 707.9b, [`ManaValueProvenance::colors_pinned`]). [`Layer::L1a`],
/// `is_cda` false, [`Affected::Objects`]`(objects)`.
///
/// An "as ... enters" or "as ... is turned face up" power and toughness setting (CR 208.2b;
/// replacement effects, CR 614.1c, 614.1e) must be registered with [`EffectOrigin::Resolved`]:
/// once applied it lasts (CR 611.2a) and the evaluator must not drop it when the permanent loses
/// the ability, as it drops a static-ability effect (CR 611.3b).
#[must_use]
pub fn modification_effect(
    exceptions: &[CopyException],
    source: Option<ObjectId>,
    controller: PlayerId,
    origin: EffectOrigin,
    timestamp: Timestamp,
    objects: BTreeSet<ObjectId>,
) -> ContinuousEffect {
    ContinuousEffect {
        source,
        controller,
        timestamp,
        layer: Layer::L1a,
        is_cda: false,
        origin,
        affected: Affected::Objects(objects),
        modification: LayerModification::ApplyCopyExceptions(exceptions.to_vec()),
    }
}

/// The layer-1a effect of a merge (CR 731.2a, 613.2a): [`copiable_effect`] of
/// [`CopiableValues::merged`] for the one merged `object` (so a fixed mana value, or a back-face
/// top whose copy has mana value 0, is carried with it), no source, origin
/// [`EffectOrigin::Resolved`], stamped at the merge (`timestamp`, which the caller takes from
/// its ledger). A later merge is a new effect with a later timestamp and supersedes this one by
/// timestamp order (CR 613.2); the caller may remove the old one.
///
/// # Errors
/// The [`LayoutError`] of [`CopiableValues::merged`].
pub fn merge_effect(
    merged: &MergedPermanent<'_>,
    vocab: &RulesVocabulary,
    object: ObjectId,
    controller: PlayerId,
    timestamp: Timestamp,
) -> Result<ContinuousEffect, LayoutError> {
    let values = CopiableValues::merged(merged, vocab)?;
    Ok(copiable_effect(
        &values,
        None,
        controller,
        EffectOrigin::Resolved,
        timestamp,
        BTreeSet::from([object]),
    ))
}
