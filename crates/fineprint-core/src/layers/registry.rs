// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The continuous-effect registry (REQ-LAYER-001, CR 611, CR 613.1).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::ObjectSelectorAST;
use fineprint_dsl::text::RewritableCategory;

use super::copy::CopyException;
use super::error::LayerError;
use super::lockin::{ForAsLongAsWindow, PreparedGroup, prepare_group};
use super::pipeline::LayerEnvironment;
use super::text::validate_modification;
use crate::objects::{Characteristics, ManaValueContext};

/// Where a continuous effect comes from; decides its timestamp (CR 613.7a, 613.7b).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectOrigin {
    /// A static ability (CR 611.3). `granted_at: None`: the ability is printed on or belongs to
    /// the source object; `granted_at: Some(t)`: the ability was granted to the object by an
    /// effect created at `t` (CR 613.7a).
    Static {
        /// The timestamp of the effect that granted the ability, if it was granted.
        granted_at: Option<Timestamp>,
    },
    /// The resolution of a spell or ability (CR 611.2, CR 613.7b).
    Resolved,
}

/// How an object's mana value is derived (CR 202.3a-c, 713.5c).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueSource {
    /// Its own mana cost (also a modal DFC face, a token, a face-down object).
    Plain,
    /// The mana value is fixed from outside the characteristics: the back face of a nonmodal
    /// DFC (front cost), a melded permanent (sum of fronts), a merged permanent (top component).
    Fixed {
        /// The mana value.
        mana_value: i64,
    },
    /// A copy of the back face of a nonmodal DFC or of a melded permanent: mana value 0
    /// (CR 202.3b, 202.3c). Copying a `CopyOfBackFaceOrMelded` keeps it.
    CopyOfBackFaceOrMelded,
}

/// The provenance of an object's mana value through layer 1 (CR 202.3a-c, 613.2c): its
/// [`ValueSource`] and whether a copy of it has mana value 0 (a [`ValueSource::Fixed`] value
/// from a nonmodal back face or a melded permanent, CR 202.3b, 202.3c).
///
/// Built only from a [`crate::layers::CopiableValues`] ([`crate::layers::CopiableValues::provenance`])
/// or as [`ManaValueProvenance::plain`], so the copy classification is always consistent with
/// the source. Carried by [`LayerModification::CopyValuesWithProvenance`] and by
/// [`crate::layers::Evaluated::mana_value_provenance`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManaValueProvenance {
    /// How the mana value is derived.
    pub(super) source: ValueSource,
    /// `Fixed` from a nonmodal back face or melded permanent: a copy has mana value 0.
    pub(super) zero_when_copied: bool,
}

impl ManaValueProvenance {
    /// The mana value is the characteristics' own ([`ValueSource::Plain`]); a copy of it is
    /// plain too.
    #[must_use]
    pub const fn plain() -> ManaValueProvenance {
        ManaValueProvenance {
            source: ValueSource::Plain,
            zero_when_copied: false,
        }
    }

    /// How the mana value is derived.
    #[must_use]
    pub fn source(&self) -> &ValueSource {
        &self.source
    }

    /// Whether a copy of an object with this provenance has mana value 0 (CR 202.3b, 202.3c):
    /// the source is [`ValueSource::CopyOfBackFaceOrMelded`], or a [`ValueSource::Fixed`] from a
    /// nonmodal back face or a melded permanent.
    #[must_use]
    pub fn copies_as_zero(&self) -> bool {
        match self.source {
            ValueSource::CopyOfBackFaceOrMelded => true,
            ValueSource::Fixed { .. } => self.zero_when_copied,
            ValueSource::Plain => false,
        }
    }

    /// Whether this is [`ManaValueProvenance::plain`].
    #[must_use]
    pub fn is_plain(&self) -> bool {
        *self == ManaValueProvenance::plain()
    }

    /// The mana value of an object with characteristics `c` and this provenance (CR 202.3):
    /// [`ValueSource::Plain`] `c`'s own in `ctx` (X counts 0 off the stack and the chosen
    /// number on it, CR 202.3e); `Fixed` the stored value in every context;
    /// [`ValueSource::CopyOfBackFaceOrMelded`] 0 (CR 202.3b, 202.3c).
    #[must_use]
    pub fn mana_value(&self, c: &Characteristics, ctx: ManaValueContext) -> i64 {
        match self.source {
            ValueSource::Plain => c.mana_value(ctx),
            ValueSource::Fixed { mana_value } => mana_value,
            // CR 202.3b, 202.3c: a copy of a nonmodal back face or of a melded permanent is 0.
            ValueSource::CopyOfBackFaceOrMelded => 0,
        }
    }
}

impl Default for ManaValueProvenance {
    fn default() -> ManaValueProvenance {
        ManaValueProvenance::plain()
    }
}

/// The objects a continuous effect applies to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Affected {
    /// A locked set, fixed when the effect began (CR 611.2c).
    Objects(BTreeSet<ObjectId>),
    /// A selector matched again at every evaluation against the object as evaluated so far
    /// (CR 611.3a, CR 613.5).
    Selector(ObjectSelectorAST),
    /// The set of the same effect in an earlier layer or sublayer (CR 613.6 lock-in; the DSL's
    /// `sameAsLayer`): the part applies to an object exactly when its anchor, the part of the
    /// same group in that layer, applied to it earlier in the same evaluation. Only
    /// [`ContinuousEffects::register_group`] accepts it; [`ContinuousEffects::register`] rejects
    /// it with [`LayerError::Unsupported`], because a link needs a group.
    SameAsLayer(Layer),
}

/// What a continuous effect does to an object; each variant belongs to the layers named in its
/// documentation (CR 613.1, 613.2, 613.4). [`LayerModification::belongs_to`] is the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerModification {
    /// Layer 1a (CR 613.2a): replaces all characteristics with the given copiable values. The
    /// mana value of the result is the characteristics' own ([`ManaValueProvenance::plain`]).
    CopyValues(Box<Characteristics>),
    /// Layer 1a (CR 613.2a, 202.3b, 202.3c): replaces all characteristics with the given
    /// copiable values, exactly as [`LayerModification::CopyValues`], and sets the mana-value
    /// provenance of the result to `provenance` (a fixed mana value, or a copy of a back face or
    /// melded permanent with mana value 0). Built by [`crate::layers::copiable_effect`] for
    /// values whose provenance is not plain.
    CopyValuesWithProvenance {
        /// The copiable values.
        characteristics: Box<Characteristics>,
        /// How the mana value of the result is derived.
        provenance: ManaValueProvenance,
    },
    /// Layer 1a (CR 613.2a, 707.2): a copiable modification that is not a copy (an "as ...
    /// enters" ability setting power and toughness): the exceptions, in order, applied to the
    /// object's current characteristics, which are also the "own" values of
    /// [`CopyException::NotCopied`] (so it changes nothing). Only the fields the exceptions set
    /// change; no characteristic-defining ability is dropped; the mana-value provenance is
    /// unchanged. Built by [`crate::layers::modification_effect`].
    ApplyCopyExceptions(Vec<CopyException>),
    /// Layer 1b (CR 613.2b): replaces all characteristics with the given face-down values.
    FaceDown(Box<Characteristics>),
    /// Layer 2 (CR 613.1b): the object's controller becomes the player.
    SetController(PlayerId),
    /// Layer 3 (CR 613.1c): replaces the rules text.
    SetText(TextTokenStream),
    /// Layer 3 (CR 612.1, 612.2, 613.1c): the text-changing effect `REPLACE_TEXT_WORD`. Every
    /// rewritable word of the rules text with this `category` and exactly the value `from`
    /// becomes `to`; literal text, name references and card-name words are never rewritten, and
    /// granted abilities are not text (CR 612.3). Category `CARD_NAME` and empty or
    /// noncharacter words are rejected at registration ([`LayerError::Unsupported`]). Rules text
    /// that is empty or does not decode as a token stream is left unchanged
    /// ([`crate::layers::rewrite_text_words`], [`crate::layers::decode_rules_text`]).
    ReplaceTextWord {
        /// The word category (`COLOR_WORD`, `BASIC_LAND_TYPE`, `CREATURE_TYPE` or `NUMBER`).
        category: RewritableCategory,
        /// The word replaced (exact, case-sensitive).
        from: String,
        /// The word it becomes.
        to: String,
    },
    /// Layer 4 (CR 613.1d): adds the given supertypes, card types and subtypes (union).
    AddTypes {
        /// Supertypes to add.
        supertypes: BTreeSet<String>,
        /// Card types to add.
        card_types: BTreeSet<String>,
        /// Subtypes to add.
        subtypes: BTreeSet<String>,
    },
    /// Layer 4 (CR 613.1d): `Some(x)` replaces that part of the type line with `x`; `None`
    /// leaves it.
    SetTypes {
        /// New supertypes, if set.
        supertypes: Option<BTreeSet<String>>,
        /// New card types, if set.
        card_types: Option<BTreeSet<String>>,
        /// New subtypes, if set.
        subtypes: Option<BTreeSet<String>>,
    },
    /// Layer 4 (CR 613.1d): removes the given subtypes.
    RemoveSubtypes(BTreeSet<String>),
    /// Layer 5 (CR 613.1e): the colors become exactly these (the color indicator is untouched).
    SetColors(ColorBitmask),
    /// Layer 5 (CR 613.1e): adds these colors.
    AddColors(ColorBitmask),
    /// Layer 6 (CR 613.1f): adds the ability.
    AddAbility(AbilityAST),
    /// Layer 6 (CR 613.1f): removes every ability.
    RemoveAllAbilities,
    /// Layer 6 (CR 613.1f): removes every keyword ability whose `keyword` is this string, and,
    /// for hexproof, every "hexproof from [quality]" variant (CR 702.11e).
    RemoveKeyword(String),
    /// Layer 7a or 7b (CR 613.4a, 613.4b): `Some(n)` sets that value to `n`; `None` leaves it.
    SetPt {
        /// New power, if set.
        power: Option<i64>,
        /// New toughness, if set.
        toughness: Option<i64>,
    },
    /// Layer 7c (CR 613.4c): adds to power and toughness (negative values subtract).
    ModifyPt {
        /// Added to power.
        power: i64,
        /// Added to toughness.
        toughness: i64,
    },
    /// Layer 7d (CR 613.4d): switches power and toughness.
    SwitchPt,
}

impl LayerModification {
    /// Whether this modification belongs to `layer` (the table of CR 613.1-613.4 as used by this
    /// unit; `SetPt` belongs to 7a and 7b).
    #[must_use]
    pub fn belongs_to(&self, layer: Layer) -> bool {
        let allowed: &[Layer] = match self {
            LayerModification::CopyValues(_)
            | LayerModification::CopyValuesWithProvenance { .. }
            | LayerModification::ApplyCopyExceptions(_) => &[Layer::L1a],
            LayerModification::FaceDown(_) => &[Layer::L1b],
            LayerModification::SetController(_) => &[Layer::L2],
            LayerModification::SetText(_) | LayerModification::ReplaceTextWord { .. } => {
                &[Layer::L3]
            }
            LayerModification::AddTypes { .. }
            | LayerModification::SetTypes { .. }
            | LayerModification::RemoveSubtypes(_) => &[Layer::L4],
            LayerModification::SetColors(_) | LayerModification::AddColors(_) => &[Layer::L5],
            LayerModification::AddAbility(_)
            | LayerModification::RemoveAllAbilities
            | LayerModification::RemoveKeyword(_) => &[Layer::L6],
            LayerModification::SetPt { .. } => &[Layer::L7a, Layer::L7b],
            LayerModification::ModifyPt { .. } => &[Layer::L7c],
            LayerModification::SwitchPt => &[Layer::L7d],
        };
        allowed.contains(&layer)
    }
}

/// One continuous effect (CR 611): its source, controller, timestamp (CR 613.7), layer, origin,
/// affected objects and modification. `is_cda` orders characteristic-defining effects first in
/// layers 2-6 (CR 613.3) and decides dependency condition (c) (CR 613.8a).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinuousEffect {
    /// The object whose ability generated the effect, if any.
    pub source: Option<ObjectId>,
    /// The effect's controller.
    pub controller: PlayerId,
    /// The effect's timestamp (CR 613.7).
    pub timestamp: Timestamp,
    /// The layer or sublayer it applies in (CR 613.1).
    pub layer: Layer,
    /// Whether it comes from a characteristic-defining ability (CR 604.3); applied first in
    /// layers 2-6 (CR 613.3) and used by dependency condition (c) (CR 613.8a).
    pub is_cda: bool,
    /// Static ability or resolved spell or ability.
    pub origin: EffectOrigin,
    /// The objects it applies to.
    pub affected: Affected,
    /// What it does.
    pub modification: LayerModification,
}

/// Every registered continuous effect, keyed and iterated by ascending [`EffectId`]
/// (REQ-LAYER-001). Whether an effect is currently in force (its source on the battlefield,
/// its duration) is the caller's job: the caller removes effects that ended.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ContinuousEffects {
    effects: BTreeMap<EffectId, ContinuousEffect>,
    /// Each `SameAsLayer` part of a registered group to its anchor (CR 613.6).
    links: BTreeMap<EffectId, EffectId>,
}

impl ContinuousEffects {
    /// An empty registry.
    #[must_use]
    pub fn new() -> ContinuousEffects {
        ContinuousEffects::default()
    }

    /// Registers `effect` under `id`.
    ///
    /// # Errors
    /// [`LayerError::DuplicateEffect`] if `id` is registered; [`LayerError::LayerMismatch`] if
    /// the modification does not belong to the effect's layer; [`LayerError::Unsupported`] for
    /// a [`LayerModification::ReplaceTextWord`] of category `CARD_NAME` or with an empty or
    /// noncharacter word (CR 612.2), or if the affected set is [`Affected::SameAsLayer`] (a link
    /// needs a group: [`ContinuousEffects::register_group`]). The registry is then unchanged. A
    /// selector is stored as given, never locked.
    pub fn register(&mut self, id: EffectId, effect: ContinuousEffect) -> Result<(), LayerError> {
        if self.effects.contains_key(&id) {
            return Err(LayerError::DuplicateEffect(id));
        }
        if !effect.modification.belongs_to(effect.layer) {
            return Err(LayerError::LayerMismatch {
                layer: effect.layer,
            });
        }
        // CR 612.2: no word replacement of card names; the words must be valid.
        validate_modification(&effect.modification)?;
        if matches!(effect.affected, Affected::SameAsLayer(_)) {
            return Err(LayerError::Unsupported(
                "sameAsLayer affected sets (CR 613.6 lock-in)",
            ));
        }
        self.effects.insert(id, effect);
        Ok(())
    }

    /// Registers one multi-layer effect (CR 613.6) whose `parts` are one [`ContinuousEffect`]
    /// per layer or sublayer. All or nothing: on any error the registry is unchanged.
    ///
    /// A part with [`Affected::SameAsLayer`]`(l)` is linked to its anchor: the part of the group
    /// whose layer is `l` and whose affected set is not `SameAsLayer` (the lowest [`EffectId`]
    /// if several); `l` must be the group's first layer (the earliest layer, in [`Layer::ALL`]
    /// order, of the parts that are not `SameAsLayer`) and strictly earlier than the part's own
    /// layer. The part then applies to an object exactly when the anchor applied to it earlier
    /// in the same evaluation, even if the generating ability has been removed in between
    /// (CR 613.6). Other parts are independent of each other (CR 611.2c). A part from a
    /// resolving spell or ability ([`EffectOrigin::Resolved`]) with a selector is stored with the
    /// concrete set [`crate::layers::resolve_affected_set`] gives against the registry as it is
    /// before this call (CR 611.2c); a static part keeps its selector (CR 611.3a); a locked set
    /// stays as given.
    ///
    /// # Errors
    /// [`LayerError::Unsupported`] for an empty group; [`LayerError::DuplicateEffect`] for an
    /// id given twice or already registered; [`LayerError::LayerMismatch`] for a modification
    /// outside its part's layer (duplicates are reported before mismatches, each in ascending
    /// id order); [`LayerError::Unsupported`] for an invalid
    /// [`LayerModification::ReplaceTextWord`] (as [`ContinuousEffects::register`], after the
    /// mismatches); [`LayerError::BadLink`] for a `SameAsLayer` part without a valid anchor in the
    /// group's first layer; the first error of evaluating an object while locking a selector.
    pub fn register_group(
        &mut self,
        env: &dyn LayerEnvironment,
        parts: Vec<(EffectId, ContinuousEffect)>,
    ) -> Result<(), LayerError> {
        let group = prepare_group(self, env, parts)?;
        self.store(group);
        Ok(())
    }

    /// [`ContinuousEffects::register_group`] for an effect with a "for as long as" duration
    /// (CR 611.2b). The group is validated (and its resolved selectors locked) exactly as by
    /// `register_group`, with the same errors and nothing registered on error. Then, if the
    /// effect does nothing ([`ForAsLongAsWindow::effect_does_something`] is `false`), nothing
    /// is registered and the result is `Ok(false)`; otherwise the group is registered and the
    /// result is `Ok(true)`.
    ///
    /// # Errors
    /// As [`ContinuousEffects::register_group`].
    pub fn register_group_for_as_long_as(
        &mut self,
        env: &dyn LayerEnvironment,
        window: ForAsLongAsWindow,
        parts: Vec<(EffectId, ContinuousEffect)>,
    ) -> Result<bool, LayerError> {
        let group = prepare_group(self, env, parts)?;
        // CR 611.2b: a duration that never starts, or ends before the first application without
        // beginning again, makes the effect do nothing.
        if !window.effect_does_something() {
            return Ok(false);
        }
        self.store(group);
        Ok(true)
    }

    /// Stores a validated group.
    fn store(&mut self, group: PreparedGroup) {
        self.effects.extend(group.parts);
        self.links.extend(group.links);
    }

    /// The links of `SameAsLayer` parts to their anchors (CR 613.6).
    pub(super) fn links(&self) -> &BTreeMap<EffectId, EffectId> {
        &self.links
    }

    /// Removes the effect `id`; whether it was registered. Removing an anchor leaves its linked
    /// parts registered but applying to nothing; removing a linked part removes its link.
    pub fn remove(&mut self, id: EffectId) -> bool {
        self.links.remove(&id);
        self.links.retain(|_, anchor| *anchor != id);
        self.effects.remove(&id).is_some()
    }

    /// The effect `id`, if registered.
    #[must_use]
    pub fn get(&self, id: EffectId) -> Option<&ContinuousEffect> {
        self.effects.get(&id)
    }

    /// Gives the effect `id` the timestamp `ts`.
    ///
    /// # Errors
    /// [`LayerError::UnknownEffect`] if `id` is not registered.
    pub fn set_timestamp(&mut self, id: EffectId, ts: Timestamp) -> Result<(), LayerError> {
        let effect = self
            .effects
            .get_mut(&id)
            .ok_or(LayerError::UnknownEffect(id))?;
        effect.timestamp = ts;
        Ok(())
    }

    /// The number of registered effects.
    #[must_use]
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Whether no effect is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// Every effect with its id, ascending [`EffectId`].
    pub fn iter(&self) -> impl Iterator<Item = (EffectId, &ContinuousEffect)> {
        self.effects.iter().map(|(&id, e)| (id, e))
    }
}
