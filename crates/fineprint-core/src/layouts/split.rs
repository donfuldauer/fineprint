// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! SPLIT (CR 709): both halves combined except for one half cast on the stack (REQ-OBJ-002).

use std::collections::BTreeSet;

use fineprint_dsl::card::{
    CardDefinitionDSL, CardFaceDSL, ColorBitmask, FaceKind, LayoutBehavior, TextTokenStream,
};
use fineprint_dsl::mana::{ManaCostDSL, ManaSymbol};
use fineprint_dsl::vocab::RulesVocabulary;

use super::faces::{chars, effective_ctx, faces};
use super::types::{CastChoice, Gating, Layout, LayoutError, LayoutQuery};
use crate::objects::{
    Characteristics, ManaValueContext, ManaValueSubject, SplitHalf, SplitState, mana_value,
    union_colors,
};
use crate::zones::ZoneKind;

/// SPLIT (CR 709).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SplitLayout;

const LAYOUT: LayoutBehavior = LayoutBehavior::Split;

/// The left and right halves and the split state the query selects.
fn select<'a>(
    def: &'a CardDefinitionDSL,
    query: &LayoutQuery,
) -> Result<(&'a CardFaceDSL, &'a CardFaceDSL, SplitState), LayoutError> {
    let [left, right] = faces(LAYOUT, def, [FaceKind::LeftHalf, FaceKind::RightHalf])?;
    let state = if query.zone == ZoneKind::Stack {
        match query.cast {
            // CR 709.3b: on the stack only the half being cast exists.
            CastChoice::SplitHalf(half) => SplitState::OnStackHalf(half),
            // CR 709.4d, 702.102b: a fused split spell has both halves combined.
            CastChoice::Fused => SplitState::OnStackFused,
            _ => {
                return Err(LayoutError::InvalidCast {
                    layout: LAYOUT,
                    zone: query.zone,
                });
            }
        }
    } else {
        // CR 709.4: every zone except the stack has both halves combined; the battlefield is
        // included although no split instant or sorcery can enter it (CR 304.4, 307.4).
        SplitState::NotOnStack
    };
    Ok((left, right, state))
}

/// The characteristics of the two halves combined (CR 709.4, 709.4a, 709.4b, 709.4c).
pub(super) fn combined(left: Characteristics, right: Characteristics) -> Characteristics {
    // CR 709.4b: the combined mana cost keeps the separate symbols, left then right; a half
    // with no mana cost contributes nothing, and neither having one is no mana cost.
    let symbols: Vec<ManaSymbol> = [&left.mana_cost, &right.mana_cost]
        .into_iter()
        .flatten()
        .flat_map(|c| c.symbols().iter().copied())
        .collect();
    // Every symbol comes from a valid cost, so this is `None` only when `symbols` is empty.
    let mana_cost = ManaCostDSL::new(symbols).ok();
    let color_indicator = match (left.color_indicator, right.color_indicator) {
        (None, None) => None,
        (l, r) => Some(union_colors(
            l.unwrap_or(ColorBitmask::NONE),
            r.unwrap_or(ColorBitmask::NONE),
        )),
    };
    let mut rules_text = left.rules_text.0;
    rules_text.extend(right.rules_text.0);
    let mut abilities = left.abilities;
    abilities.extend(right.abilities);
    Characteristics {
        // CR 709.4a: the split card has both names.
        names: left.names.into_iter().chain(right.names).collect(),
        mana_cost,
        color_indicator,
        // CR 709.4b: colors from the combined cost (and the indicators).
        colors: union_colors(left.colors, right.colors),
        // CR 709.4c: each card type on either half.
        supertypes: union(left.supertypes, right.supertypes),
        card_types: union(left.card_types, right.card_types),
        subtypes: union(left.subtypes, right.subtypes),
        // CR 709.4c: each ability in the text box of each half.
        rules_text: TextTokenStream(rules_text),
        abilities,
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

pub(super) fn union(mut a: BTreeSet<String>, b: BTreeSet<String>) -> BTreeSet<String> {
    a.extend(b);
    a
}

impl Layout for SplitLayout {
    fn behavior(&self) -> LayoutBehavior {
        LAYOUT
    }

    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        let (left, right, state) = select(def, query)?;
        Ok(match state {
            // CR 709.3b: only the half being cast exists on the stack.
            SplitState::OnStackHalf(SplitHalf::Left) => chars(left, vocab)?,
            SplitState::OnStackHalf(SplitHalf::Right) => chars(right, vocab)?,
            // CR 709.4, 709.4d: off the stack, or fused, both halves combined.
            SplitState::NotOnStack | SplitState::OnStackFused => {
                combined(chars(left, vocab)?, chars(right, vocab)?)
            }
        })
    }

    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        let (left, right, state) = select(def, query)?;
        // Validate the type lines of the halves that exist, as `base_characteristics` does
        // (same errors, same order), without building the combined record.
        match state {
            // CR 709.3b: only the half being cast exists on the stack.
            SplitState::OnStackHalf(SplitHalf::Left) => {
                chars(left, vocab)?;
            }
            SplitState::OnStackHalf(SplitHalf::Right) => {
                chars(right, vocab)?;
            }
            // CR 709.4, 709.4d: off the stack, or fused, both halves, left then right.
            SplitState::NotOnStack | SplitState::OnStackFused => {
                chars(left, vocab)?;
                chars(right, vocab)?;
            }
        }
        // CR 202.3d: combined costs off the stack or fused; the cast half's cost otherwise.
        Ok(mana_value(
            &ManaValueSubject::Split {
                left: left.mana_cost.as_ref(),
                right: right.mana_cost.as_ref(),
                state,
            },
            effective_ctx(query, ctx),
        ))
    }

    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
        faces(LAYOUT, def, [FaceKind::LeftHalf, FaceKind::RightHalf])?;
        Ok(None)
    }
}
