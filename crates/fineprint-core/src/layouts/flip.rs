// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! FLIP (CR 710): the flipped characteristics apply only to a flipped permanent
//! (REQ-OBJ-002).

use fineprint_dsl::card::{CardDefinitionDSL, FaceKind, LayoutBehavior};
use fineprint_dsl::vocab::RulesVocabulary;

use super::faces::{chars, faces};
use super::types::{Gating, Layout, LayoutError, LayoutQuery};
use crate::objects::{Characteristics, ManaValueContext};
use crate::zones::ZoneKind;

/// FLIP (CR 710).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlipLayout;

const LAYOUT: LayoutBehavior = LayoutBehavior::Flip;

impl Layout for FlipLayout {
    fn behavior(&self) -> LayoutBehavior {
        LAYOUT
    }

    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        let [main, flipped] = faces(LAYOUT, def, [FaceKind::Main, FaceKind::Flipped])?;
        let normal = chars(main, vocab)?;
        // CR 710.2: every zone other than the battlefield, and the battlefield before the
        // permanent flips, has only the normal characteristics; flipped status is lost on
        // leaving the battlefield (CR 710.4, 110.5), so it is ignored elsewhere.
        if query.zone != ZoneKind::Battlefield || !query.status.flipped {
            return Ok(normal);
        }
        let alt = chars(flipped, vocab)?;
        // CR 710.1b, 710.2: a flipped permanent uses the alternative name, text box, type line
        // and power/toughness; CR 710.1c: its mana cost and color do not change.
        Ok(Characteristics {
            names: alt.names,
            mana_cost: normal.mana_cost,
            color_indicator: normal.color_indicator,
            colors: normal.colors,
            supertypes: alt.supertypes,
            card_types: alt.card_types,
            subtypes: alt.subtypes,
            rules_text: alt.rules_text,
            abilities: alt.abilities,
            power: alt.power,
            toughness: alt.toughness,
            loyalty: alt.loyalty,
            defense: alt.defense,
            hand_modifier: alt.hand_modifier,
            life_modifier: alt.life_modifier,
        })
    }

    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        // CR 710.1c: the mana cost, and so the mana value, never changes with flipping; the
        // selected characteristics always carry the Main face's cost.
        Ok(self
            .base_characteristics(def, query, vocab)?
            .mana_value(ctx))
    }

    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
        faces(LAYOUT, def, [FaceKind::Main, FaceKind::Flipped])?;
        Ok(None)
    }
}
