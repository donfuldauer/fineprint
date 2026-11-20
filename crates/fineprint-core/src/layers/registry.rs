// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The continuous-effect registry (REQ-LAYER-001, CR 611, CR 613.1).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::ObjectSelectorAST;

use super::error::LayerError;
use crate::objects::Characteristics;

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

/// The objects a continuous effect applies to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Affected {
    /// A locked set, fixed when the effect began (CR 611.2c).
    Objects(BTreeSet<ObjectId>),
    /// A selector matched again at every evaluation against the object as evaluated so far
    /// (CR 611.3a, CR 613.5).
    Selector(ObjectSelectorAST),
    /// The set of the same effect in another layer (CR 613.6 lock-in); registration rejects it
    /// with [`LayerError::Unsupported`] in this unit.
    SameAsLayer(Layer),
}

/// What a continuous effect does to an object; each variant belongs to the layers named in its
/// documentation (CR 613.1, 613.2, 613.4). [`LayerModification::belongs_to`] is the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerModification {
    /// Layer 1a (CR 613.2a): replaces all characteristics with the given copiable values.
    CopyValues(Box<Characteristics>),
    /// Layer 1b (CR 613.2b): replaces all characteristics with the given face-down values.
    FaceDown(Box<Characteristics>),
    /// Layer 2 (CR 613.1b): the object's controller becomes the player.
    SetController(PlayerId),
    /// Layer 3 (CR 613.1c): replaces the rules text.
    SetText(TextTokenStream),
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
            LayerModification::CopyValues(_) => &[Layer::L1a],
            LayerModification::FaceDown(_) => &[Layer::L1b],
            LayerModification::SetController(_) => &[Layer::L2],
            LayerModification::SetText(_) => &[Layer::L3],
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
    /// the modification does not belong to the effect's layer; [`LayerError::Unsupported`] if
    /// the affected set is [`Affected::SameAsLayer`] (CR 613.6 lock-in is a later unit). The
    /// registry is then unchanged.
    pub fn register(&mut self, id: EffectId, effect: ContinuousEffect) -> Result<(), LayerError> {
        if self.effects.contains_key(&id) {
            return Err(LayerError::DuplicateEffect(id));
        }
        if !effect.modification.belongs_to(effect.layer) {
            return Err(LayerError::LayerMismatch {
                layer: effect.layer,
            });
        }
        if matches!(effect.affected, Affected::SameAsLayer(_)) {
            return Err(LayerError::Unsupported(
                "sameAsLayer affected sets (CR 613.6 lock-in)",
            ));
        }
        self.effects.insert(id, effect);
        Ok(())
    }

    /// Removes the effect `id`; whether it was registered.
    pub fn remove(&mut self, id: EffectId) -> bool {
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
