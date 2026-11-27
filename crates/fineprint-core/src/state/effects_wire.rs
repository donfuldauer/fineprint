// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The wire form (format version 2) of the layer-system components of a game state: the
//! continuous effects with their `SameAsLayer` links, the timestamp ledger and the
//! generating-ability links (REQ-CORE-001, REQ-NFR-006, R12; contracts/CHANGE_REQUESTS.md
//! CR-005).
//!
//! The wire structs mirror the layer types with raw `u64` identifiers; their member and variant
//! names are the wire contract. Decoding treats the bytes as untrusted: it rebuilds the
//! registry through [`ContinuousEffects::register`] and
//! [`ContinuousEffects::register_group`], so a decoded registry passes every check registration
//! makes, rebuilds the ledger once from its checked entries, and then checks
//! what the state boundary checks ([`GameState::register_effect`](super::GameState::register_effect),
//! [`GameState::link_effect_generator`](super::GameState::link_effect_generator)). Every failure
//! is [`StateError::Decode`].
//!
//! The per-effect, ledger and generating-ability checks are shared with
//! [`GameState::serialize`](super::GameState::serialize), which refuses (as
//! [`StateError::Encode`]) a state these checks would refuse on decode.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::expr::ObjectSelectorAST;
use fineprint_dsl::mana::ManaCostDSL;
use fineprint_dsl::text::RewritableCategory;
use serde::{Deserialize, Deserializer, Serialize};

use super::{GeneratingAbility, StateError};
use crate::layers::{
    Affected, ContinuousEffect, ContinuousEffects, CopiedCharacteristic, CopyException,
    CounterHolder, EffectOrigin, LayerEnvironment, LayerModification, MatchContext, ObjectBase,
    TimestampLedger, ValueSource, effect_links, provenance_from_parts, provenance_parts,
};
use crate::objects::Characteristics;

/// A [`StateError::Decode`] with message `msg`.
fn bad(msg: &str) -> StateError {
    StateError::Decode(msg.to_owned())
}

/// Deserializes an `Option` member that must be present; `nil` is `None`.
fn required<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}

/// An `Option` of a JSON-carrying value as an array of zero or one element, so `Some` of a JSON
/// `null` stays distinct from `None`.
mod opt_seq {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

    pub(super) fn serialize<S: Serializer, T: Serialize>(
        value: &Option<T>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        value.as_slice().serialize(s)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        d: D,
    ) -> Result<Option<T>, D::Error> {
        let mut items = Vec::<T>::deserialize(d)?;
        match items.len() {
            0 => Ok(None),
            1 => Ok(items.pop()),
            _ => Err(de::Error::custom(
                "an optional value has at most one element",
            )),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Wire structs
// ---------------------------------------------------------------------------------------------

/// The continuous effects: each effect with its id (strictly ascending) and each `SameAsLayer`
/// part-to-anchor link (strictly ascending part id).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EffectsWire {
    pub(super) effects: Vec<EffectEntryWire>,
    pub(super) links: Vec<LinkWire>,
}

/// One registered effect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EffectEntryWire {
    pub(super) id: u64,
    pub(super) effect: EffectWire,
}

/// One `SameAsLayer` link (CR 613.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LinkWire {
    pub(super) part: u64,
    pub(super) anchor: u64,
}

/// A [`ContinuousEffect`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EffectWire {
    #[serde(deserialize_with = "required")]
    pub(super) source: Option<u64>,
    pub(super) controller: u64,
    pub(super) timestamp: u64,
    pub(super) layer: Layer,
    pub(super) is_cda: bool,
    pub(super) origin: OriginWire,
    pub(super) affected: AffectedWire,
    pub(super) modification: ModificationWire,
}

/// An [`EffectOrigin`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) enum OriginWire {
    Static {
        #[serde(deserialize_with = "required")]
        granted_at: Option<u64>,
    },
    Resolved,
}

/// An [`Affected`]; `Objects` strictly ascending.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum AffectedWire {
    Objects(Vec<u64>),
    Selector(ObjectSelectorAST),
    SameAsLayer(Layer),
}

/// A [`LayerModification`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) enum ModificationWire {
    CopyValues(CharacteristicsWire),
    CopyValuesWithProvenance {
        characteristics: CharacteristicsWire,
        provenance: ProvenanceWire,
    },
    ApplyCopyExceptions(Vec<CopyExceptionWire>),
    FaceDown(CharacteristicsWire),
    SetController(u64),
    SetText(TextTokenStream),
    ReplaceTextWord {
        category: RewritableCategory,
        from: String,
        to: String,
    },
    AddTypes {
        supertypes: BTreeSet<String>,
        card_types: BTreeSet<String>,
        subtypes: BTreeSet<String>,
    },
    SetTypes {
        #[serde(deserialize_with = "required")]
        supertypes: Option<BTreeSet<String>>,
        #[serde(deserialize_with = "required")]
        card_types: Option<BTreeSet<String>>,
        #[serde(deserialize_with = "required")]
        subtypes: Option<BTreeSet<String>>,
    },
    RemoveSubtypes(BTreeSet<String>),
    SetColors(ColorBitmask),
    AddColors(ColorBitmask),
    AddAbility(AbilityAST),
    RemoveAllAbilities,
    RemoveKeyword(String),
    SetPt {
        #[serde(deserialize_with = "required")]
        power: Option<i64>,
        #[serde(deserialize_with = "required")]
        toughness: Option<i64>,
    },
    ModifyPt {
        power: i64,
        toughness: i64,
    },
    SwitchPt,
}

/// [`Characteristics`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CharacteristicsWire {
    pub(super) names: BTreeSet<String>,
    #[serde(deserialize_with = "required")]
    pub(super) mana_cost: Option<ManaCostDSL>,
    #[serde(deserialize_with = "required")]
    pub(super) color_indicator: Option<ColorBitmask>,
    pub(super) colors: ColorBitmask,
    pub(super) supertypes: BTreeSet<String>,
    pub(super) card_types: BTreeSet<String>,
    pub(super) subtypes: BTreeSet<String>,
    pub(super) rules_text: TextTokenStream,
    pub(super) abilities: Vec<AbilityAST>,
    #[serde(with = "opt_seq")]
    pub(super) power: Option<ValueExpressionAST>,
    #[serde(with = "opt_seq")]
    pub(super) toughness: Option<ValueExpressionAST>,
    #[serde(deserialize_with = "required")]
    pub(super) loyalty: Option<i64>,
    #[serde(deserialize_with = "required")]
    pub(super) defense: Option<i64>,
    #[serde(deserialize_with = "required")]
    pub(super) hand_modifier: Option<i64>,
    #[serde(deserialize_with = "required")]
    pub(super) life_modifier: Option<i64>,
}

/// A [`crate::layers::ManaValueProvenance`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProvenanceWire {
    pub(super) source: ValueSourceWire,
    pub(super) zero_when_copied: bool,
    pub(super) colors_pinned: bool,
}

/// A [`ValueSource`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) enum ValueSourceWire {
    Plain,
    Fixed { mana_value: i64 },
    CopyOfBackFaceOrMelded,
}

/// A [`CopyException`].
// The typed `AbilityAST` payload is large; it stays unboxed so the public variants keep the
// shape callers build (docs/DECISIONS.md "#232 FP-164 — Typed card AST positions").
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum CopyExceptionWire {
    GainAbility(AbilityAST),
    SetNames(BTreeSet<String>),
    SetManaCost(#[serde(deserialize_with = "required")] Option<ManaCostDSL>),
    SetColors(ColorBitmask),
    SetSupertypes(BTreeSet<String>),
    SetCardTypes(BTreeSet<String>),
    SetSubtypes(BTreeSet<String>),
    AddSupertypes(BTreeSet<String>),
    AddCardTypes(BTreeSet<String>),
    AddSubtypes(BTreeSet<String>),
    SetPower(ValueExpressionAST),
    SetToughness(ValueExpressionAST),
    NotCopied(CopiedWire),
}

/// A [`CopiedCharacteristic`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum CopiedWire {
    Names,
    ManaCost,
    ColorIndicator,
    Colors,
    Supertypes,
    CardTypes,
    Subtypes,
    RulesText,
    Abilities,
    Power,
    Toughness,
    Loyalty,
    Defense,
    HandModifier,
    LifeModifier,
}

/// The timestamp ledger: each group strictly ascending by key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TimestampsWire {
    pub(super) objects: Vec<ObjectStampWire>,
    pub(super) counters: Vec<CounterStampWire>,
    pub(super) stickers: Vec<StickerStampWire>,
}

/// An object's timestamp (CR 613.7d-j).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ObjectStampWire {
    pub(super) object: u64,
    pub(super) timestamp: u64,
}

/// The timestamp of the counters of one kind on one holder (CR 613.7c).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CounterStampWire {
    pub(super) holder: HolderWire,
    pub(super) kind: String,
    pub(super) timestamp: u64,
}

/// A [`CounterHolder`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum HolderWire {
    Object(u64),
    Player(u64),
}

/// A sticker's timestamp (CR 613.7k).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StickerStampWire {
    pub(super) host: u64,
    pub(super) sticker: u32,
    pub(super) timestamp: u64,
}

/// One generating-ability link (CR 613.8a), strictly ascending effect id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GeneratorWire {
    pub(super) effect: u64,
    pub(super) source: u64,
    pub(super) ability: AbilityAST,
}

// ---------------------------------------------------------------------------------------------
// To the wire
// ---------------------------------------------------------------------------------------------

impl From<&Characteristics> for CharacteristicsWire {
    fn from(c: &Characteristics) -> Self {
        CharacteristicsWire {
            names: c.names.clone(),
            mana_cost: c.mana_cost.clone(),
            color_indicator: c.color_indicator,
            colors: c.colors,
            supertypes: c.supertypes.clone(),
            card_types: c.card_types.clone(),
            subtypes: c.subtypes.clone(),
            rules_text: c.rules_text.clone(),
            abilities: c.abilities.clone(),
            power: c.power.clone(),
            toughness: c.toughness.clone(),
            loyalty: c.loyalty,
            defense: c.defense,
            hand_modifier: c.hand_modifier,
            life_modifier: c.life_modifier,
        }
    }
}

impl From<CharacteristicsWire> for Characteristics {
    fn from(c: CharacteristicsWire) -> Self {
        Characteristics {
            names: c.names,
            mana_cost: c.mana_cost,
            color_indicator: c.color_indicator,
            colors: c.colors,
            supertypes: c.supertypes,
            card_types: c.card_types,
            subtypes: c.subtypes,
            rules_text: c.rules_text,
            abilities: c.abilities,
            power: c.power,
            toughness: c.toughness,
            loyalty: c.loyalty,
            defense: c.defense,
            hand_modifier: c.hand_modifier,
            life_modifier: c.life_modifier,
        }
    }
}

impl From<CopiedCharacteristic> for CopiedWire {
    fn from(c: CopiedCharacteristic) -> Self {
        match c {
            CopiedCharacteristic::Names => CopiedWire::Names,
            CopiedCharacteristic::ManaCost => CopiedWire::ManaCost,
            CopiedCharacteristic::ColorIndicator => CopiedWire::ColorIndicator,
            CopiedCharacteristic::Colors => CopiedWire::Colors,
            CopiedCharacteristic::Supertypes => CopiedWire::Supertypes,
            CopiedCharacteristic::CardTypes => CopiedWire::CardTypes,
            CopiedCharacteristic::Subtypes => CopiedWire::Subtypes,
            CopiedCharacteristic::RulesText => CopiedWire::RulesText,
            CopiedCharacteristic::Abilities => CopiedWire::Abilities,
            CopiedCharacteristic::Power => CopiedWire::Power,
            CopiedCharacteristic::Toughness => CopiedWire::Toughness,
            CopiedCharacteristic::Loyalty => CopiedWire::Loyalty,
            CopiedCharacteristic::Defense => CopiedWire::Defense,
            CopiedCharacteristic::HandModifier => CopiedWire::HandModifier,
            CopiedCharacteristic::LifeModifier => CopiedWire::LifeModifier,
        }
    }
}

impl From<CopiedWire> for CopiedCharacteristic {
    fn from(c: CopiedWire) -> Self {
        match c {
            CopiedWire::Names => CopiedCharacteristic::Names,
            CopiedWire::ManaCost => CopiedCharacteristic::ManaCost,
            CopiedWire::ColorIndicator => CopiedCharacteristic::ColorIndicator,
            CopiedWire::Colors => CopiedCharacteristic::Colors,
            CopiedWire::Supertypes => CopiedCharacteristic::Supertypes,
            CopiedWire::CardTypes => CopiedCharacteristic::CardTypes,
            CopiedWire::Subtypes => CopiedCharacteristic::Subtypes,
            CopiedWire::RulesText => CopiedCharacteristic::RulesText,
            CopiedWire::Abilities => CopiedCharacteristic::Abilities,
            CopiedWire::Power => CopiedCharacteristic::Power,
            CopiedWire::Toughness => CopiedCharacteristic::Toughness,
            CopiedWire::Loyalty => CopiedCharacteristic::Loyalty,
            CopiedWire::Defense => CopiedCharacteristic::Defense,
            CopiedWire::HandModifier => CopiedCharacteristic::HandModifier,
            CopiedWire::LifeModifier => CopiedCharacteristic::LifeModifier,
        }
    }
}

impl From<&CopyException> for CopyExceptionWire {
    fn from(e: &CopyException) -> Self {
        match e {
            CopyException::GainAbility(a) => CopyExceptionWire::GainAbility(a.clone()),
            CopyException::SetNames(s) => CopyExceptionWire::SetNames(s.clone()),
            CopyException::SetManaCost(c) => CopyExceptionWire::SetManaCost(c.clone()),
            CopyException::SetColors(c) => CopyExceptionWire::SetColors(*c),
            CopyException::SetSupertypes(s) => CopyExceptionWire::SetSupertypes(s.clone()),
            CopyException::SetCardTypes(s) => CopyExceptionWire::SetCardTypes(s.clone()),
            CopyException::SetSubtypes(s) => CopyExceptionWire::SetSubtypes(s.clone()),
            CopyException::AddSupertypes(s) => CopyExceptionWire::AddSupertypes(s.clone()),
            CopyException::AddCardTypes(s) => CopyExceptionWire::AddCardTypes(s.clone()),
            CopyException::AddSubtypes(s) => CopyExceptionWire::AddSubtypes(s.clone()),
            CopyException::SetPower(v) => CopyExceptionWire::SetPower(v.clone()),
            CopyException::SetToughness(v) => CopyExceptionWire::SetToughness(v.clone()),
            CopyException::NotCopied(c) => CopyExceptionWire::NotCopied((*c).into()),
        }
    }
}

impl From<CopyExceptionWire> for CopyException {
    fn from(e: CopyExceptionWire) -> Self {
        match e {
            CopyExceptionWire::GainAbility(a) => CopyException::GainAbility(a),
            CopyExceptionWire::SetNames(s) => CopyException::SetNames(s),
            CopyExceptionWire::SetManaCost(c) => CopyException::SetManaCost(c),
            CopyExceptionWire::SetColors(c) => CopyException::SetColors(c),
            CopyExceptionWire::SetSupertypes(s) => CopyException::SetSupertypes(s),
            CopyExceptionWire::SetCardTypes(s) => CopyException::SetCardTypes(s),
            CopyExceptionWire::SetSubtypes(s) => CopyException::SetSubtypes(s),
            CopyExceptionWire::AddSupertypes(s) => CopyException::AddSupertypes(s),
            CopyExceptionWire::AddCardTypes(s) => CopyException::AddCardTypes(s),
            CopyExceptionWire::AddSubtypes(s) => CopyException::AddSubtypes(s),
            CopyExceptionWire::SetPower(v) => CopyException::SetPower(v),
            CopyExceptionWire::SetToughness(v) => CopyException::SetToughness(v),
            CopyExceptionWire::NotCopied(c) => CopyException::NotCopied(c.into()),
        }
    }
}

/// The wire form of `m`.
fn modification_to_wire(m: &LayerModification) -> ModificationWire {
    match m {
        LayerModification::CopyValues(c) => ModificationWire::CopyValues((&**c).into()),
        LayerModification::CopyValuesWithProvenance {
            characteristics,
            provenance,
        } => {
            let (source, zero_when_copied, colors_pinned) = provenance_parts(provenance);
            ModificationWire::CopyValuesWithProvenance {
                characteristics: (&**characteristics).into(),
                provenance: ProvenanceWire {
                    source: match source {
                        ValueSource::Plain => ValueSourceWire::Plain,
                        ValueSource::Fixed { mana_value } => ValueSourceWire::Fixed { mana_value },
                        ValueSource::CopyOfBackFaceOrMelded => {
                            ValueSourceWire::CopyOfBackFaceOrMelded
                        }
                    },
                    zero_when_copied,
                    colors_pinned,
                },
            }
        }
        LayerModification::ApplyCopyExceptions(list) => {
            ModificationWire::ApplyCopyExceptions(list.iter().map(Into::into).collect())
        }
        LayerModification::FaceDown(c) => ModificationWire::FaceDown((&**c).into()),
        LayerModification::SetController(p) => ModificationWire::SetController(p.get()),
        LayerModification::SetText(t) => ModificationWire::SetText(t.clone()),
        LayerModification::ReplaceTextWord { category, from, to } => {
            ModificationWire::ReplaceTextWord {
                category: *category,
                from: from.clone(),
                to: to.clone(),
            }
        }
        LayerModification::AddTypes {
            supertypes,
            card_types,
            subtypes,
        } => ModificationWire::AddTypes {
            supertypes: supertypes.clone(),
            card_types: card_types.clone(),
            subtypes: subtypes.clone(),
        },
        LayerModification::SetTypes {
            supertypes,
            card_types,
            subtypes,
        } => ModificationWire::SetTypes {
            supertypes: supertypes.clone(),
            card_types: card_types.clone(),
            subtypes: subtypes.clone(),
        },
        LayerModification::RemoveSubtypes(s) => ModificationWire::RemoveSubtypes(s.clone()),
        LayerModification::SetColors(c) => ModificationWire::SetColors(*c),
        LayerModification::AddColors(c) => ModificationWire::AddColors(*c),
        LayerModification::AddAbility(a) => ModificationWire::AddAbility(a.clone()),
        LayerModification::RemoveAllAbilities => ModificationWire::RemoveAllAbilities,
        LayerModification::RemoveKeyword(k) => ModificationWire::RemoveKeyword(k.clone()),
        LayerModification::SetPt { power, toughness } => ModificationWire::SetPt {
            power: *power,
            toughness: *toughness,
        },
        LayerModification::ModifyPt { power, toughness } => ModificationWire::ModifyPt {
            power: *power,
            toughness: *toughness,
        },
        LayerModification::SwitchPt => ModificationWire::SwitchPt,
    }
}

/// The wire form of `e`.
fn effect_to_wire(e: &ContinuousEffect) -> EffectWire {
    EffectWire {
        source: e.source.map(ObjectId::get),
        controller: e.controller.get(),
        timestamp: e.timestamp.get(),
        layer: e.layer,
        is_cda: e.is_cda,
        origin: match e.origin {
            EffectOrigin::Static { granted_at } => OriginWire::Static {
                granted_at: granted_at.map(Timestamp::get),
            },
            EffectOrigin::Resolved => OriginWire::Resolved,
        },
        affected: match &e.affected {
            Affected::Objects(set) => AffectedWire::Objects(set.iter().map(|o| o.get()).collect()),
            Affected::Selector(s) => AffectedWire::Selector(s.clone()),
            Affected::SameAsLayer(l) => AffectedWire::SameAsLayer(*l),
        },
        modification: modification_to_wire(&e.modification),
    }
}

impl EffectsWire {
    /// The wire form of `effects`.
    pub(super) fn from_effects(effects: &ContinuousEffects) -> EffectsWire {
        EffectsWire {
            effects: effects
                .iter()
                .map(|(id, e)| EffectEntryWire {
                    id: id.get(),
                    effect: effect_to_wire(e),
                })
                .collect(),
            links: effect_links(effects)
                .iter()
                .map(|(part, anchor)| LinkWire {
                    part: part.get(),
                    anchor: anchor.get(),
                })
                .collect(),
        }
    }
}

impl TimestampsWire {
    /// The wire form of `ledger`.
    pub(super) fn from_ledger(ledger: &TimestampLedger) -> TimestampsWire {
        let entries = ledger.entries();
        TimestampsWire {
            objects: entries
                .objects
                .iter()
                .map(|(o, t)| ObjectStampWire {
                    object: o.get(),
                    timestamp: t.get(),
                })
                .collect(),
            counters: entries
                .counters
                .iter()
                .map(|((holder, kind), t)| CounterStampWire {
                    holder: match holder {
                        CounterHolder::Object(o) => HolderWire::Object(o.get()),
                        CounterHolder::Player(p) => HolderWire::Player(p.get()),
                    },
                    kind: kind.clone(),
                    timestamp: t.get(),
                })
                .collect(),
            stickers: entries
                .stickers
                .iter()
                .map(|((host, sticker), t)| StickerStampWire {
                    host: host.get(),
                    sticker: *sticker,
                    timestamp: t.get(),
                })
                .collect(),
        }
    }
}

/// The wire form of the generating-ability links.
pub(super) fn generators_to_wire(
    generators: &BTreeMap<EffectId, GeneratingAbility>,
) -> Vec<GeneratorWire> {
    generators
        .iter()
        .map(|(id, g)| GeneratorWire {
            effect: id.get(),
            source: g.source.get(),
            ability: g.ability.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// From the wire (untrusted)
// ---------------------------------------------------------------------------------------------

/// The issued ranges a decoded value must stay within: the last issued effect id and timestamp.
#[derive(Clone, Copy)]
pub(super) struct Issued {
    pub(super) effect: u64,
    pub(super) timestamp: u64,
}

impl Issued {
    /// Whether `raw` is a timestamp issued by the state's generator (`1..=last issued`); the
    /// test [`Issued::timestamp`] applies on decode and [`GameState::register_effect`] and
    /// [`GameState::restamp_static_effects`] apply at the state boundary (REQ-CORE-002).
    ///
    /// [`GameState::register_effect`]: super::GameState::register_effect
    /// [`GameState::restamp_static_effects`]: super::GameState::restamp_static_effects
    pub(super) fn has_timestamp(self, raw: u64) -> bool {
        raw != 0 && raw <= self.timestamp
    }

    /// `raw` as a timestamp issued by the state's generator (`1..=last issued`).
    fn timestamp(self, raw: u64) -> Result<Timestamp, StateError> {
        if !self.has_timestamp(raw) {
            return Err(bad("timestamp not issued by the timestamp generator"));
        }
        Ok(Timestamp::new(raw))
    }

    /// `raw` as an effect id issued by the state's generator (`1..=last issued`).
    fn effect(self, raw: u64) -> Result<EffectId, StateError> {
        if raw == 0 || raw > self.effect {
            return Err(bad("effect id not issued by the effect generator"));
        }
        Ok(EffectId::new(raw))
    }
}

/// `Ok` if `keys` is strictly ascending.
fn strictly_ascending<K: Ord>(
    keys: impl IntoIterator<Item = K>,
    what: &str,
) -> Result<(), StateError> {
    let mut previous: Option<K> = None;
    for k in keys {
        if previous.as_ref().is_some_and(|p| *p >= k) {
            return Err(StateError::Decode(format!(
                "{what} not in strictly ascending order"
            )));
        }
        previous = Some(k);
    }
    Ok(())
}

/// The layer modification `m` describes.
fn modification_from_wire(m: ModificationWire) -> Result<LayerModification, StateError> {
    Ok(match m {
        ModificationWire::CopyValues(c) => LayerModification::CopyValues(Box::new(c.into())),
        ModificationWire::CopyValuesWithProvenance {
            characteristics,
            provenance,
        } => {
            let source = match provenance.source {
                ValueSourceWire::Plain => ValueSource::Plain,
                ValueSourceWire::Fixed { mana_value } => ValueSource::Fixed { mana_value },
                ValueSourceWire::CopyOfBackFaceOrMelded => ValueSource::CopyOfBackFaceOrMelded,
            };
            let provenance = provenance_from_parts(
                source,
                provenance.zero_when_copied,
                provenance.colors_pinned,
            )
            .ok_or_else(|| bad("zero_when_copied set for a mana value that is not fixed"))?;
            LayerModification::CopyValuesWithProvenance {
                characteristics: Box::new(characteristics.into()),
                provenance,
            }
        }
        ModificationWire::ApplyCopyExceptions(list) => {
            LayerModification::ApplyCopyExceptions(list.into_iter().map(Into::into).collect())
        }
        ModificationWire::FaceDown(c) => LayerModification::FaceDown(Box::new(c.into())),
        ModificationWire::SetController(p) => LayerModification::SetController(PlayerId::new(p)),
        ModificationWire::SetText(t) => LayerModification::SetText(t),
        ModificationWire::ReplaceTextWord { category, from, to } => {
            LayerModification::ReplaceTextWord { category, from, to }
        }
        ModificationWire::AddTypes {
            supertypes,
            card_types,
            subtypes,
        } => LayerModification::AddTypes {
            supertypes,
            card_types,
            subtypes,
        },
        ModificationWire::SetTypes {
            supertypes,
            card_types,
            subtypes,
        } => LayerModification::SetTypes {
            supertypes,
            card_types,
            subtypes,
        },
        ModificationWire::RemoveSubtypes(s) => LayerModification::RemoveSubtypes(s),
        ModificationWire::SetColors(c) => LayerModification::SetColors(c),
        ModificationWire::AddColors(c) => LayerModification::AddColors(c),
        ModificationWire::AddAbility(a) => LayerModification::AddAbility(a),
        ModificationWire::RemoveAllAbilities => LayerModification::RemoveAllAbilities,
        ModificationWire::RemoveKeyword(k) => LayerModification::RemoveKeyword(k),
        ModificationWire::SetPt { power, toughness } => {
            LayerModification::SetPt { power, toughness }
        }
        ModificationWire::ModifyPt { power, toughness } => {
            LayerModification::ModifyPt { power, toughness }
        }
        ModificationWire::SwitchPt => LayerModification::SwitchPt,
    })
}

/// The checks of one effect's own fields that decoding and [`GameState::serialize`] share:
/// its timestamp and the timestamp of its granting effect (`granted_at`) were issued by the
/// state's generator (REQ-CORE-002), and a layer-7a effect is from a characteristic-defining
/// ability (SRS 6.2 item 1, CR 613.4a: as [`GameState::register_effect`]).
///
/// [`GameState::serialize`]: super::GameState::serialize
/// [`GameState::register_effect`]: super::GameState::register_effect
fn check_effect_fields(effect: &ContinuousEffect, issued: Issued) -> Result<(), StateError> {
    if let EffectOrigin::Static {
        granted_at: Some(t),
    } = effect.origin
    {
        issued.timestamp(t.get())?;
    }
    issued.timestamp(effect.timestamp.get())?;
    // SRS 6.2 item 1, CR 613.4a: as `GameState::register_effect`.
    if effect.layer == Layer::L7a && !effect.is_cda {
        return Err(bad("non-CDA effect in layer 7a"));
    }
    Ok(())
}

/// What decoding checks of every effect of a registry, for [`GameState::serialize`]: its id was
/// issued by the effect generator and its fields pass [`check_effect_fields`].
///
/// # Errors
/// The [`StateError::Decode`] decoding would report.
///
/// [`GameState::serialize`]: super::GameState::serialize
pub(super) fn check_effects(effects: &ContinuousEffects, issued: Issued) -> Result<(), StateError> {
    for (id, effect) in effects.iter() {
        issued.effect(id.get())?;
        check_effect_fields(effect, issued)?;
    }
    Ok(())
}

/// The effect `e` describes, with its fields checked by [`check_effect_fields`].
fn effect_from_wire(e: EffectWire, issued: Issued) -> Result<ContinuousEffect, StateError> {
    let origin = match e.origin {
        OriginWire::Static { granted_at } => EffectOrigin::Static {
            granted_at: granted_at.map(Timestamp::new),
        },
        OriginWire::Resolved => EffectOrigin::Resolved,
    };
    let affected = match e.affected {
        AffectedWire::Objects(list) => {
            strictly_ascending(list.iter().copied(), "affected objects")?;
            Affected::Objects(list.into_iter().map(ObjectId::new).collect())
        }
        AffectedWire::Selector(s) => Affected::Selector(s),
        AffectedWire::SameAsLayer(l) => Affected::SameAsLayer(l),
    };
    let effect = ContinuousEffect {
        source: e.source.map(ObjectId::new),
        controller: PlayerId::new(e.controller),
        timestamp: Timestamp::new(e.timestamp),
        layer: e.layer,
        is_cda: e.is_cda,
        origin,
        affected,
        modification: modification_from_wire(e.modification)?,
    };
    check_effect_fields(&effect, issued)?;
    Ok(effect)
}

/// An environment with no objects: group registration on decode never locks a selector (a
/// resolved part with a selector inside a group is rejected before), so it is never read.
struct NoObjects;

impl LayerEnvironment for NoObjects {
    fn objects(&self) -> Vec<ObjectId> {
        Vec::new()
    }

    fn base(&self, _: ObjectId) -> Option<ObjectBase> {
        None
    }

    fn matches(&self, _: &ObjectSelectorAST, _: &MatchContext<'_>) -> bool {
        false
    }
}

/// A modification that belongs to `layer`, for the stand-in anchor of orphaned `SameAsLayer`
/// parts (removed right after; never stored).
fn stand_in_modification(layer: Layer) -> LayerModification {
    match layer {
        Layer::L1a => LayerModification::ApplyCopyExceptions(Vec::new()),
        Layer::L1b => LayerModification::FaceDown(Box::new(Characteristics {
            names: BTreeSet::new(),
            mana_cost: None,
            color_indicator: None,
            colors: ColorBitmask::NONE,
            supertypes: BTreeSet::new(),
            card_types: BTreeSet::new(),
            subtypes: BTreeSet::new(),
            rules_text: TextTokenStream::default(),
            abilities: Vec::new(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            hand_modifier: None,
            life_modifier: None,
        })),
        Layer::L2 => LayerModification::SetController(PlayerId::new(0)),
        Layer::L3 => LayerModification::SetText(TextTokenStream::default()),
        Layer::L4 => LayerModification::RemoveSubtypes(BTreeSet::new()),
        Layer::L5 => LayerModification::AddColors(ColorBitmask::NONE),
        Layer::L6 => LayerModification::RemoveAllAbilities,
        Layer::L7a | Layer::L7b => LayerModification::SetPt {
            power: None,
            toughness: None,
        },
        Layer::L7c => LayerModification::ModifyPt {
            power: 0,
            toughness: 0,
        },
        Layer::L7d => LayerModification::SwitchPt,
    }
}

/// Rebuilds the registry `wire` describes through [`ContinuousEffects::register`] and
/// [`ContinuousEffects::register_group`] and checks the result is exactly what was described.
pub(super) fn effects_from_wire(
    wire: EffectsWire,
    issued: Issued,
) -> Result<ContinuousEffects, StateError> {
    strictly_ascending(wire.effects.iter().map(|e| e.id), "effect ids")?;
    strictly_ascending(wire.links.iter().map(|l| l.part), "effect links")?;
    let mut described: BTreeMap<EffectId, ContinuousEffect> = BTreeMap::new();
    for entry in wire.effects {
        let id = issued.effect(entry.id)?;
        described.insert(id, effect_from_wire(entry.effect, issued)?);
    }
    let mut links: BTreeMap<EffectId, EffectId> = BTreeMap::new();
    let mut groups: BTreeMap<EffectId, Vec<EffectId>> = BTreeMap::new();
    for link in &wire.links {
        let (part, anchor) = (EffectId::new(link.part), EffectId::new(link.anchor));
        if !described.contains_key(&part) || !described.contains_key(&anchor) {
            return Err(bad("effect link names an unregistered effect"));
        }
        links.insert(part, anchor);
        groups.entry(anchor).or_default().push(part);
    }
    let mut registry = ContinuousEffects::new();
    let refused = |_| bad("effect refused by registration");
    // CR 613.6: each anchor with its linked parts, as one group.
    for (anchor, parts) in &groups {
        let mut group = Vec::with_capacity(parts.len() + 1);
        for id in std::iter::once(anchor).chain(parts) {
            let effect = described[id].clone();
            if effect.origin == EffectOrigin::Resolved
                && matches!(effect.affected, Affected::Selector(_))
            {
                return Err(bad("unlocked resolved selector inside an effect group"));
            }
            group.push((*id, effect));
        }
        registry
            .register_group(&NoObjects, group)
            .map_err(refused)?;
    }
    // SameAsLayer parts whose anchor was removed: registered in a group with a stand-in anchor
    // (id 0, never issued), which is then removed, exactly as removing the real anchor did.
    let mut orphans: BTreeMap<usize, (Layer, Vec<EffectId>)> = BTreeMap::new();
    for (id, effect) in &described {
        if let Affected::SameAsLayer(layer) = effect.affected
            && !links.contains_key(id)
        {
            let key = Layer::ALL.iter().position(|l| *l == layer).unwrap_or(0);
            orphans
                .entry(key)
                .or_insert((layer, Vec::new()))
                .1
                .push(*id);
        }
    }
    for (layer, ids) in orphans.into_values() {
        let stand_in = EffectId::new(0);
        let mut group = vec![(
            stand_in,
            ContinuousEffect {
                source: None,
                controller: PlayerId::new(0),
                timestamp: Timestamp::new(0),
                layer,
                is_cda: false,
                origin: EffectOrigin::Resolved,
                affected: Affected::Objects(BTreeSet::new()),
                modification: stand_in_modification(layer),
            },
        )];
        group.extend(ids.iter().map(|id| (*id, described[id].clone())));
        registry
            .register_group(&NoObjects, group)
            .map_err(refused)?;
        registry.remove(stand_in);
    }
    // Every other effect on its own.
    for (id, effect) in &described {
        if registry.get(*id).is_none() {
            registry.register(*id, effect.clone()).map_err(refused)?;
        }
    }
    let rebuilt: BTreeMap<EffectId, &ContinuousEffect> = registry.iter().collect();
    let same = rebuilt.len() == described.len()
        && described.iter().all(|(id, e)| rebuilt.get(id) == Some(&e));
    if !same || effect_links(&registry) != &links {
        return Err(bad("effects differ from what registration gives"));
    }
    Ok(registry)
}

/// The [`CounterHolder`] `h` describes.
fn holder_from_wire(h: HolderWire) -> CounterHolder {
    match h {
        HolderWire::Object(o) => CounterHolder::Object(ObjectId::new(o)),
        HolderWire::Player(p) => CounterHolder::Player(PlayerId::new(p)),
    }
}

/// The checks of a ledger's wire form that decoding and [`GameState::serialize`] share: each
/// table strictly ascending by key, then every timestamp (objects, counters, stickers, in that
/// order) issued by the state's generator (REQ-CORE-002, CR 613.7).
///
/// # Errors
/// The first failing check, as [`StateError::Decode`].
///
/// [`GameState::serialize`]: super::GameState::serialize
pub(super) fn check_ledger_wire(wire: &TimestampsWire, issued: Issued) -> Result<(), StateError> {
    strictly_ascending(wire.objects.iter().map(|o| o.object), "object timestamps")?;
    strictly_ascending(
        wire.counters
            .iter()
            .map(|c| (holder_from_wire(c.holder), c.kind.as_str())),
        "counter timestamps",
    )?;
    strictly_ascending(
        wire.stickers.iter().map(|s| (s.host, s.sticker)),
        "sticker timestamps",
    )?;
    let stamps = wire.objects.iter().map(|o| o.timestamp);
    let stamps = stamps.chain(wire.counters.iter().map(|c| c.timestamp));
    for t in stamps.chain(wire.stickers.iter().map(|s| s.timestamp)) {
        issued.timestamp(t)?;
    }
    Ok(())
}

/// Rebuilds the ledger `wire` describes once from its validated entries ([`check_ledger_wire`];
/// [`TimestampLedger::from_entries`]): linear in the number of entries (REQ-NFR-006), and equal
/// to the ledger the stamping operations give when they issue those timestamps.
pub(super) fn ledger_from_wire(
    wire: TimestampsWire,
    issued: Issued,
) -> Result<TimestampLedger, StateError> {
    check_ledger_wire(&wire, issued)?;
    let objects = wire
        .objects
        .into_iter()
        .map(|o| (ObjectId::new(o.object), Timestamp::new(o.timestamp)))
        .collect();
    let counters = wire
        .counters
        .into_iter()
        .map(|c| {
            (
                (holder_from_wire(c.holder), c.kind),
                Timestamp::new(c.timestamp),
            )
        })
        .collect();
    let stickers = wire
        .stickers
        .into_iter()
        .map(|s| {
            (
                (ObjectId::new(s.host), s.sticker),
                Timestamp::new(s.timestamp),
            )
        })
        .collect();
    Ok(TimestampLedger::from_entries(objects, counters, stickers))
}

/// Rebuilds the generating-ability links and checks them with [`check_generators`].
pub(super) fn generators_from_wire(
    wire: Vec<GeneratorWire>,
    effects: &ContinuousEffects,
    issued: Issued,
) -> Result<BTreeMap<EffectId, GeneratingAbility>, StateError> {
    strictly_ascending(wire.iter().map(|g| g.effect), "generating-ability links")?;
    let out: BTreeMap<EffectId, GeneratingAbility> = wire
        .into_iter()
        .map(|g| {
            (
                EffectId::new(g.effect),
                GeneratingAbility::new(ObjectId::new(g.source), g.ability),
            )
        })
        .collect();
    check_generators(&out, effects, issued)?;
    Ok(out)
}

/// Checks the generating-ability links against `effects` (CR 613.8a), for decoding and
/// [`GameState::serialize`]: every link names an effect id issued by the effect generator;
/// a link of a registered effect belongs to a static effect with a source, on that source;
/// every static effect with a source has a link. Links are checked in ascending [`EffectId`]
/// order.
///
/// # Errors
/// The first failing check, as [`StateError::Decode`].
///
/// [`GameState::serialize`]: super::GameState::serialize
pub(super) fn check_generators(
    generators: &BTreeMap<EffectId, GeneratingAbility>,
    effects: &ContinuousEffects,
    issued: Issued,
) -> Result<(), StateError> {
    for (&id, g) in generators {
        issued.effect(id.get())?;
        if let Some(effect) = effects.get(id) {
            let static_with_source =
                matches!(effect.origin, EffectOrigin::Static { .. }) && effect.source.is_some();
            if !static_with_source {
                return Err(bad(
                    "generating ability for an effect that is not static with a source",
                ));
            }
            if effect.source != Some(g.source) {
                return Err(bad("generating ability not on the effect's source"));
            }
        }
    }
    for (id, effect) in effects.iter() {
        if matches!(effect.origin, EffectOrigin::Static { .. })
            && effect.source.is_some()
            && !generators.contains_key(&id)
        {
            return Err(bad(
                "static effect with a source lacks its generating ability",
            ));
        }
    }
    Ok(())
}
