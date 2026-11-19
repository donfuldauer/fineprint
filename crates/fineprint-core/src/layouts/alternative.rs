// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Cards with an alternative set of characteristics chosen when cast: ADVENTURER (CR 716),
//! OMEN (CR 721) and PROTOTYPE (CR 719) (REQ-OBJ-002).
//!
//! The alternative characteristics are part of the object's copiable values (CR 716.2b, 721.2b,
//! 719.2a): they are plain definition data selected here as base characteristics, never a
//! layer effect.

use fineprint_dsl::card::{CardDefinitionDSL, FaceKind, LayoutBehavior};
use fineprint_dsl::vocab::RulesVocabulary;

use super::faces::{chars, effective_ctx, faces};
use super::types::{CastChoice, Gating, Layout, LayoutError, LayoutQuery};
use crate::objects::{CdaColors, Characteristics, ManaValueContext, base_colors};
use crate::zones::ZoneKind;

/// ADVENTURER (CR 716).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AdventurerLayout;

/// OMEN (CR 721).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OmenLayout;

/// PROTOTYPE (CR 719).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrototypeLayout;

/// Whether the alternative characteristics of an adventurer or omen card apply.
fn stack_alternative(
    layout: LayoutBehavior,
    alternative: CastChoice,
    query: &LayoutQuery,
) -> Result<bool, LayoutError> {
    if query.zone != ZoneKind::Stack {
        // CR 716.4, 721.4: every zone except the stack has only the normal characteristics.
        return Ok(false);
    }
    if query.cast == alternative {
        // CR 716.3b, 721.3b: on the stack as an Adventure / Omen, only the alternative
        // characteristics.
        Ok(true)
    } else if query.cast == CastChoice::None {
        // CR 716.4, 721.4: on the stack not as an Adventure / Omen, the normal ones.
        Ok(false)
    } else {
        Err(LayoutError::InvalidCast {
            layout,
            zone: query.zone,
        })
    }
}

/// Base characteristics of an adventurer or omen card.
fn alternative_chars(
    layout: LayoutBehavior,
    alt_kind: FaceKind,
    alternative: CastChoice,
    def: &CardDefinitionDSL,
    query: &LayoutQuery,
    vocab: &RulesVocabulary,
) -> Result<Characteristics, LayoutError> {
    let [main, alt] = faces(layout, def, [FaceKind::Main, alt_kind])?;
    if stack_alternative(layout, alternative, query)? {
        // CR 716.3b, 721.3b: only the alternative characteristics.
        chars(alt, vocab)
    } else {
        // CR 716.4, 721.4: only the normal characteristics.
        chars(main, vocab)
    }
}

impl Layout for AdventurerLayout {
    fn behavior(&self) -> LayoutBehavior {
        LayoutBehavior::Adventurer
    }

    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        alternative_chars(
            LayoutBehavior::Adventurer,
            FaceKind::Adventure,
            CastChoice::Adventure,
            def,
            query,
            vocab,
        )
    }

    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        // CR 202.3, 716.3b: the selected characteristics' own mana cost.
        Ok(self
            .base_characteristics(def, query, vocab)?
            .mana_value(effective_ctx(query, ctx)))
    }

    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
        faces(
            LayoutBehavior::Adventurer,
            def,
            [FaceKind::Main, FaceKind::Adventure],
        )?;
        Ok(None)
    }
}

impl Layout for OmenLayout {
    fn behavior(&self) -> LayoutBehavior {
        LayoutBehavior::Omen
    }

    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        alternative_chars(
            LayoutBehavior::Omen,
            FaceKind::Omen,
            CastChoice::Omen,
            def,
            query,
            vocab,
        )
    }

    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        // CR 202.3, 721.3b: the selected characteristics' own mana cost.
        Ok(self
            .base_characteristics(def, query, vocab)?
            .mana_value(effective_ctx(query, ctx)))
    }

    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
        faces(LayoutBehavior::Omen, def, [FaceKind::Main, FaceKind::Omen])?;
        Ok(None)
    }
}

/// Whether a prototype card uses its prototype power, toughness and mana cost.
fn prototyped(query: &LayoutQuery) -> Result<bool, LayoutError> {
    match query.zone {
        ZoneKind::Stack | ZoneKind::Battlefield => match query.cast {
            // CR 719.3b: a prototyped spell and the permanent it becomes.
            CastChoice::Prototype => Ok(true),
            // CR 719.4: on the stack or battlefield when not cast prototyped.
            CastChoice::None => Ok(false),
            _ => Err(LayoutError::InvalidCast {
                layout: LayoutBehavior::Prototype,
                zone: query.zone,
            }),
        },
        // CR 719.4: every zone except the stack and the battlefield: normal characteristics.
        _ => Ok(false),
    }
}

impl Layout for PrototypeLayout {
    fn behavior(&self) -> LayoutBehavior {
        LayoutBehavior::Prototype
    }

    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        let [main, proto] = faces(
            LayoutBehavior::Prototype,
            def,
            [FaceKind::Main, FaceKind::Prototype],
        )?;
        let use_proto = prototyped(query)?;
        let normal = chars(main, vocab)?;
        if !use_proto {
            // CR 719.4: only the normal characteristics.
            return Ok(normal);
        }
        let alt = chars(proto, vocab)?;
        // CR 719.3b: the prototype power, toughness and mana cost, and the colors of that cost;
        // CR 719.5: every other characteristic stays the normal one.
        Ok(Characteristics {
            colors: base_colors(
                alt.mana_cost.as_ref(),
                normal.color_indicator,
                CdaColors::None,
            ),
            mana_cost: alt.mana_cost,
            power: alt.power,
            toughness: alt.toughness,
            ..normal
        })
    }

    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        // CR 202.3, 719.3b: the selected mana cost (the prototype cost when prototyped).
        Ok(self
            .base_characteristics(def, query, vocab)?
            .mana_value(effective_ctx(query, ctx)))
    }

    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
        faces(
            LayoutBehavior::Prototype,
            def,
            [FaceKind::Main, FaceKind::Prototype],
        )?;
        Ok(None)
    }
}
