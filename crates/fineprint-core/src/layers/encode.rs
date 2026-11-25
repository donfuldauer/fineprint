// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Canonical (Merkle-leaf) encodings of the layer-system values the game state stores
//! (REQ-NFR-006, REQ-CORE-006; contracts/CHANGE_REQUESTS.md CR-005): [`ContinuousEffects`] (the
//! payload of the state leaf `/effects`) and [`TimestampLedger`] (the payload of
//! `/timestamps`), with every value they hold. Integration glue for the game state (#183
//! FP-144); no layer behaviour depends on it.
//!
//! **Layout.** Every collection is [`put_len`] of its entry count, then the entries in ascending
//! key order; integers and identifiers are fixed-width little-endian; `String`, `Option`,
//! `BTreeSet` and `BTreeMap` use their `fineprint_base` encodings. Every enum is a one-byte tag
//! written by an explicit `match` (never a discriminant cast, never `Debug`), then the payload
//! of the variant in declaration order:
//!
//! | Value | Tags |
//! |---|---|
//! | `Layer` | 1a `0`, 1b `1`, 2 `2`, 3 `3`, 4 `4`, 5 `5`, 6 `6`, 7a `7`, 7b `8`, 7c `9`, 7d `10` |
//! | [`EffectOrigin`] | `Static` `0` + `Option<Timestamp>`; `Resolved` `1` |
//! | [`Affected`] | `Objects` `0` + `BTreeSet<ObjectId>`; `Selector` `1` + DSL value; `SameAsLayer` `2` + layer tag |
//! | [`LayerModification`] | `CopyValues` `0`, `CopyValuesWithProvenance` `1`, `ApplyCopyExceptions` `2`, `FaceDown` `3`, `SetController` `4`, `SetText` `5`, `ReplaceTextWord` `6`, `AddTypes` `7`, `SetTypes` `8`, `RemoveSubtypes` `9`, `SetColors` `10`, `AddColors` `11`, `AddAbility` `12`, `RemoveAllAbilities` `13`, `RemoveKeyword` `14`, `SetPt` `15`, `ModifyPt` `16`, `SwitchPt` `17` |
//! | [`ValueSource`] | `Plain` `0`; `Fixed` `1` + `i64`; `CopyOfBackFaceOrMelded` `2` |
//! | [`CopyException`] | `GainAbility` `0` ... `NotCopied` `12`, in declaration order |
//! | [`CopiedCharacteristic`] | `Names` `0` ... `LifeModifier` `14`, in declaration order |
//! | `RewritableCategory` | `COLOR_WORD` `0`, `BASIC_LAND_TYPE` `1`, `CREATURE_TYPE` `2`, `NUMBER` `3`, `CARD_NAME` `4` |
//! | [`CounterHolder`] | `Object` `0` + `ObjectId`; `Player` `1` + `PlayerId` |
//!
//! **DSL values** (`AbilityAST`, `ObjectSelectorAST`, `ValueExpressionAST`, `TextTokenStream`)
//! are tag `0` then [`put_bytes`] of their RFC 8785 canonical JSON
//! ([`fineprint_dsl::canonical::to_canonical_vec`]). A value that has no canonical JSON (an
//! integer beyond ±2^53−1, a noncharacter, nesting deeper than the JCS budget) is tag `1` then a
//! structural encoding of its JSON tree: `null` `0`; `bool` `1` + byte; number `2` + its decimal
//! JSON text; string `3` + string; array `4` + count + items; object `5` + count + (key, value)
//! in ascending key order. A value that cannot be represented as JSON at all is tag `2` alone.
//! MessagePack is never written into a hash input; it is used only to turn a typed selector into
//! its JSON tree for the structural form. Equal values always give equal bytes.
//!
//! **Characteristics** (inside copy and face-down modifications): names, mana cost (its canonical
//! text, or none), color indicator bits (or none), color bits, supertypes, card types, subtypes,
//! rules text (DSL value), abilities (count, then each as a DSL value), power and toughness
//! (each an `Option` of a DSL value), loyalty, defense, hand modifier, life modifier.
//!
//! **[`ContinuousEffect`]**: source, controller, timestamp, layer tag, `is_cda`, origin,
//! affected, modification. **[`ContinuousEffects`]**: [`put_len`] of the effect count, then per
//! effect in ascending [`EffectId`] order the id, the effect and its `SameAsLayer` anchor link
//! (`Option<EffectId>`, CR 613.6). **[`ManaValueProvenance`]**: source, zero-when-copied flag,
//! colors-pinned flag. **[`TimestampLedger`]**: [`put_len`] of the entry count, then the object
//! entries (tag `0`), the counter entries (tag `1`) and the sticker entries (tag `2`), each
//! group in ascending key order. Each empty collection is `put_len(0)`, 4 zero bytes.
//! [`StampReason`](super::StampReason) is not stored in the ledger and is not hashed.

use std::collections::BTreeMap;

use fineprint_base::encode::{CanonicalEncode, put_bytes, put_len, put_variant};
use fineprint_base::ids::EffectId;
use fineprint_dsl::ability::Layer;
use fineprint_dsl::canonical::to_canonical_vec;
use fineprint_dsl::card::{AbilityAST, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::expr::ObjectSelectorAST;
use fineprint_dsl::text::RewritableCategory;
use serde::Serialize;

use super::copy::{CopiedCharacteristic, CopyException};
use super::registry::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerModification,
    ManaValueProvenance, ValueSource,
};
use super::timestamps::{CounterHolder, TimestampLedger};
use crate::objects::Characteristics;

// ---------------------------------------------------------------------------------------------
// DSL values
// ---------------------------------------------------------------------------------------------

/// Appends the canonical encoding of a JSON tree carried as an [`AbilityAST`]: tag 0 and its
/// RFC 8785 canonical JSON, else tag 1 and its structural encoding (module documentation).
pub(crate) fn put_json(out: &mut Vec<u8>, value: &AbilityAST) {
    if let Ok(bytes) = to_canonical_vec(value) {
        put_variant(out, 0);
        put_bytes(out, &bytes);
    } else {
        put_variant(out, 1);
        put_structural(out, value);
    }
}

/// The structural encoding of a JSON tree (module documentation), used only when the value has
/// no RFC 8785 form. Object members are sorted by key, so the result never depends on the
/// map's iteration order.
fn put_structural(out: &mut Vec<u8>, value: &AbilityAST) {
    let v = &value.0;
    if v.is_null() {
        put_variant(out, 0);
    } else if let Some(b) = v.as_bool() {
        put_variant(out, 1);
        b.encode_canonical(out);
    } else if v.is_number() {
        put_variant(out, 2);
        put_bytes(out, v.to_string().as_bytes());
    } else if let Some(s) = v.as_str() {
        put_variant(out, 3);
        s.encode_canonical(out);
    } else if let Some(items) = v.as_array() {
        put_variant(out, 4);
        put_len(out, items.len());
        for item in items {
            put_structural(out, &AbilityAST(item.clone()));
        }
    } else if let Some(members) = v.as_object() {
        put_variant(out, 5);
        let mut entries: Vec<_> = members.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        put_len(out, entries.len());
        for (key, item) in entries {
            key.encode_canonical(out);
            put_structural(out, &AbilityAST(item.clone()));
        }
    }
}

/// Appends the canonical encoding of a typed DSL value (module documentation): its RFC 8785
/// canonical JSON (tag 0); else the structural encoding of its JSON tree (tag 1); else tag 2.
pub(super) fn put_typed<T: Serialize>(out: &mut Vec<u8>, value: &T) {
    if let Ok(bytes) = to_canonical_vec(value) {
        put_variant(out, 0);
        put_bytes(out, &bytes);
        return;
    }
    // Only to obtain the JSON tree; these bytes are never hashed.
    let tree = rmp_serde::to_vec_named(value)
        .ok()
        .and_then(|bytes| rmp_serde::from_slice::<AbilityAST>(&bytes).ok());
    match tree {
        Some(tree) => {
            put_variant(out, 1);
            put_structural(out, &tree);
        }
        None => put_variant(out, 2),
    }
}

/// A [`ValueExpressionAST`] as a DSL value.
fn put_value_expr(out: &mut Vec<u8>, value: &ValueExpressionAST) {
    put_json(out, &AbilityAST(value.0.clone()));
}

/// A [`TextTokenStream`] as a DSL value (its JSON array of tokens).
fn put_text(out: &mut Vec<u8>, text: &TextTokenStream) {
    put_json(out, &AbilityAST(text.0.clone().into()));
}

/// An `Option` of a [`ValueExpressionAST`]: `0x00`, or `0x01` and the value.
fn put_opt_value_expr(out: &mut Vec<u8>, value: Option<&ValueExpressionAST>) {
    match value {
        None => put_variant(out, 0),
        Some(v) => {
            put_variant(out, 1);
            put_value_expr(out, v);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Enum tags
// ---------------------------------------------------------------------------------------------

/// The stable one-byte tag of `layer` (module documentation).
pub(crate) fn layer_tag(layer: Layer) -> u8 {
    match layer {
        Layer::L1a => 0,
        Layer::L1b => 1,
        Layer::L2 => 2,
        Layer::L3 => 3,
        Layer::L4 => 4,
        Layer::L5 => 5,
        Layer::L6 => 6,
        Layer::L7a => 7,
        Layer::L7b => 8,
        Layer::L7c => 9,
        Layer::L7d => 10,
    }
}

/// The stable one-byte tag of a rewritable-word category (module documentation).
fn category_tag(category: RewritableCategory) -> u8 {
    match category {
        RewritableCategory::ColorWord => 0,
        RewritableCategory::BasicLandType => 1,
        RewritableCategory::CreatureType => 2,
        RewritableCategory::Number => 3,
        RewritableCategory::CardName => 4,
    }
}

// ---------------------------------------------------------------------------------------------
// Characteristics and provenance
// ---------------------------------------------------------------------------------------------

/// Appends the canonical encoding of `c`, field by field (module documentation).
fn put_characteristics(out: &mut Vec<u8>, c: &Characteristics) {
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
    put_text(out, &c.rules_text);
    put_len(out, c.abilities.len());
    for ability in &c.abilities {
        put_json(out, ability);
    }
    put_opt_value_expr(out, c.power.as_ref());
    put_opt_value_expr(out, c.toughness.as_ref());
    c.loyalty.encode_canonical(out);
    c.defense.encode_canonical(out);
    c.hand_modifier.encode_canonical(out);
    c.life_modifier.encode_canonical(out);
}

impl CanonicalEncode for ValueSource {
    /// `Plain` `0`; `Fixed` `1` + mana value; `CopyOfBackFaceOrMelded` `2`.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            ValueSource::Plain => put_variant(out, 0),
            ValueSource::Fixed { mana_value } => {
                put_variant(out, 1);
                mana_value.encode_canonical(out);
            }
            ValueSource::CopyOfBackFaceOrMelded => put_variant(out, 2),
        }
    }
}

impl CanonicalEncode for ManaValueProvenance {
    /// Source, zero-when-copied flag, colors-pinned flag.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.source.encode_canonical(out);
        self.zero_when_copied.encode_canonical(out);
        self.colors_pinned.encode_canonical(out);
    }
}

/// The parts of a provenance: its source, whether a copy of it has mana value 0 (meaningful for
/// a [`ValueSource::Fixed`] source only) and whether its colors are pinned. For the wire form of
/// the game state (crate-internal glue).
pub(crate) fn provenance_parts(p: &ManaValueProvenance) -> (ValueSource, bool, bool) {
    (p.source.clone(), p.zero_when_copied, p.colors_pinned)
}

/// The provenance with these parts; `None` when `zero_when_copied` is set for a source that is
/// not [`ValueSource::Fixed`] (CR 202.3b, 202.3c: the flag only qualifies a fixed mana value;
/// no layer operation builds such a value). The inverse of [`provenance_parts`].
pub(crate) fn provenance_from_parts(
    source: ValueSource,
    zero_when_copied: bool,
    colors_pinned: bool,
) -> Option<ManaValueProvenance> {
    if zero_when_copied && !matches!(source, ValueSource::Fixed { .. }) {
        return None;
    }
    Some(ManaValueProvenance {
        source,
        zero_when_copied,
        colors_pinned,
    })
}

impl CanonicalEncode for CopiedCharacteristic {
    /// One tag byte, in declaration order (module documentation).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        let tag = match self {
            CopiedCharacteristic::Names => 0,
            CopiedCharacteristic::ManaCost => 1,
            CopiedCharacteristic::ColorIndicator => 2,
            CopiedCharacteristic::Colors => 3,
            CopiedCharacteristic::Supertypes => 4,
            CopiedCharacteristic::CardTypes => 5,
            CopiedCharacteristic::Subtypes => 6,
            CopiedCharacteristic::RulesText => 7,
            CopiedCharacteristic::Abilities => 8,
            CopiedCharacteristic::Power => 9,
            CopiedCharacteristic::Toughness => 10,
            CopiedCharacteristic::Loyalty => 11,
            CopiedCharacteristic::Defense => 12,
            CopiedCharacteristic::HandModifier => 13,
            CopiedCharacteristic::LifeModifier => 14,
        };
        put_variant(out, tag);
    }
}

impl CanonicalEncode for CopyException {
    /// One tag byte in declaration order, then the payload (module documentation).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            CopyException::GainAbility(a) => {
                put_variant(out, 0);
                put_json(out, a);
            }
            CopyException::SetNames(names) => {
                put_variant(out, 1);
                names.encode_canonical(out);
            }
            CopyException::SetManaCost(cost) => {
                put_variant(out, 2);
                cost.as_ref().map(ToString::to_string).encode_canonical(out);
            }
            CopyException::SetColors(colors) => {
                put_variant(out, 3);
                colors.bits().encode_canonical(out);
            }
            CopyException::SetSupertypes(s) => {
                put_variant(out, 4);
                s.encode_canonical(out);
            }
            CopyException::SetCardTypes(s) => {
                put_variant(out, 5);
                s.encode_canonical(out);
            }
            CopyException::SetSubtypes(s) => {
                put_variant(out, 6);
                s.encode_canonical(out);
            }
            CopyException::AddSupertypes(s) => {
                put_variant(out, 7);
                s.encode_canonical(out);
            }
            CopyException::AddCardTypes(s) => {
                put_variant(out, 8);
                s.encode_canonical(out);
            }
            CopyException::AddSubtypes(s) => {
                put_variant(out, 9);
                s.encode_canonical(out);
            }
            CopyException::SetPower(v) => {
                put_variant(out, 10);
                put_value_expr(out, v);
            }
            CopyException::SetToughness(v) => {
                put_variant(out, 11);
                put_value_expr(out, v);
            }
            CopyException::NotCopied(c) => {
                put_variant(out, 12);
                c.encode_canonical(out);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Effects
// ---------------------------------------------------------------------------------------------

impl CanonicalEncode for EffectOrigin {
    /// `Static` `0` + the granting timestamp (`Option`); `Resolved` `1`.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            EffectOrigin::Static { granted_at } => {
                put_variant(out, 0);
                granted_at.encode_canonical(out);
            }
            EffectOrigin::Resolved => put_variant(out, 1),
        }
    }
}

impl CanonicalEncode for Affected {
    /// `Objects` `0` + the set; `Selector` `1` + the selector (DSL value); `SameAsLayer` `2` +
    /// the layer tag.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Affected::Objects(set) => {
                put_variant(out, 0);
                set.encode_canonical(out);
            }
            Affected::Selector(selector) => {
                put_variant(out, 1);
                put_typed::<ObjectSelectorAST>(out, selector);
            }
            Affected::SameAsLayer(layer) => {
                put_variant(out, 2);
                put_variant(out, layer_tag(*layer));
            }
        }
    }
}

impl CanonicalEncode for LayerModification {
    /// One tag byte (module documentation), then every payload field in declaration order.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            LayerModification::CopyValues(c) => {
                put_variant(out, 0);
                put_characteristics(out, c);
            }
            LayerModification::CopyValuesWithProvenance {
                characteristics,
                provenance,
            } => {
                put_variant(out, 1);
                put_characteristics(out, characteristics);
                provenance.encode_canonical(out);
            }
            LayerModification::ApplyCopyExceptions(list) => {
                put_variant(out, 2);
                list.encode_canonical(out);
            }
            LayerModification::FaceDown(c) => {
                put_variant(out, 3);
                put_characteristics(out, c);
            }
            LayerModification::SetController(p) => {
                put_variant(out, 4);
                p.encode_canonical(out);
            }
            LayerModification::SetText(text) => {
                put_variant(out, 5);
                put_text(out, text);
            }
            LayerModification::ReplaceTextWord { category, from, to } => {
                put_variant(out, 6);
                put_variant(out, category_tag(*category));
                from.encode_canonical(out);
                to.encode_canonical(out);
            }
            LayerModification::AddTypes {
                supertypes,
                card_types,
                subtypes,
            } => {
                put_variant(out, 7);
                supertypes.encode_canonical(out);
                card_types.encode_canonical(out);
                subtypes.encode_canonical(out);
            }
            LayerModification::SetTypes {
                supertypes,
                card_types,
                subtypes,
            } => {
                put_variant(out, 8);
                supertypes.encode_canonical(out);
                card_types.encode_canonical(out);
                subtypes.encode_canonical(out);
            }
            LayerModification::RemoveSubtypes(s) => {
                put_variant(out, 9);
                s.encode_canonical(out);
            }
            LayerModification::SetColors(c) => {
                put_variant(out, 10);
                c.bits().encode_canonical(out);
            }
            LayerModification::AddColors(c) => {
                put_variant(out, 11);
                c.bits().encode_canonical(out);
            }
            LayerModification::AddAbility(a) => {
                put_variant(out, 12);
                put_json(out, a);
            }
            LayerModification::RemoveAllAbilities => put_variant(out, 13),
            LayerModification::RemoveKeyword(k) => {
                put_variant(out, 14);
                k.encode_canonical(out);
            }
            LayerModification::SetPt { power, toughness } => {
                put_variant(out, 15);
                power.encode_canonical(out);
                toughness.encode_canonical(out);
            }
            LayerModification::ModifyPt { power, toughness } => {
                put_variant(out, 16);
                power.encode_canonical(out);
                toughness.encode_canonical(out);
            }
            LayerModification::SwitchPt => put_variant(out, 17),
        }
    }
}

impl CanonicalEncode for ContinuousEffect {
    /// Source, controller, timestamp, layer tag, `is_cda`, origin, affected, modification.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.source.encode_canonical(out);
        self.controller.encode_canonical(out);
        self.timestamp.encode_canonical(out);
        put_variant(out, layer_tag(self.layer));
        self.is_cda.encode_canonical(out);
        self.origin.encode_canonical(out);
        self.affected.encode_canonical(out);
        self.modification.encode_canonical(out);
    }
}

impl CanonicalEncode for ContinuousEffects {
    /// [`put_len`] of the effect count, then per effect in ascending [`EffectId`] order: the
    /// id, the effect, and its `SameAsLayer` anchor link as an `Option<EffectId>` (CR 613.6;
    /// every link belongs to a registered part). An empty registry is `put_len(0)` (4 zero
    /// bytes).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_len(out, self.len());
        let links = self.links();
        for (id, effect) in self.iter() {
            id.encode_canonical(out);
            effect.encode_canonical(out);
            links.get(&id).copied().encode_canonical(out);
        }
    }
}

/// The `SameAsLayer` part-to-anchor links of `effects` (CR 613.6), for the wire form of the
/// game state (crate-internal glue).
pub(crate) fn effect_links(effects: &ContinuousEffects) -> &BTreeMap<EffectId, EffectId> {
    effects.links()
}

// ---------------------------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------------------------

impl CanonicalEncode for CounterHolder {
    /// `Object` `0` + the object id; `Player` `1` + the player id.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            CounterHolder::Object(id) => {
                put_variant(out, 0);
                id.encode_canonical(out);
            }
            CounterHolder::Player(id) => {
                put_variant(out, 1);
                id.encode_canonical(out);
            }
        }
    }
}

impl CanonicalEncode for TimestampLedger {
    /// [`put_len`] of the number of entries (objects, counters and stickers together), then the
    /// object entries (tag `0`, [`ObjectId`](fineprint_base::ids::ObjectId), timestamp), the
    /// counter entries (tag `1`, [`CounterHolder`], kind, timestamp) and the sticker entries
    /// (tag `2`, host, sticker number `u32`, timestamp), each group in ascending key order
    /// (CR 613.7c-k). An empty ledger is `put_len(0)` (4 zero bytes).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        let entries = self.entries();
        put_len(
            out,
            entries.objects.len() + entries.counters.len() + entries.stickers.len(),
        );
        for (id, ts) in entries.objects {
            put_variant(out, 0);
            id.encode_canonical(out);
            ts.encode_canonical(out);
        }
        for ((holder, kind), ts) in entries.counters {
            put_variant(out, 1);
            holder.encode_canonical(out);
            kind.encode_canonical(out);
            ts.encode_canonical(out);
        }
        for ((host, sticker), ts) in entries.stickers {
            put_variant(out, 2);
            host.encode_canonical(out);
            sticker.encode_canonical(out);
            ts.encode_canonical(out);
        }
    }
}
