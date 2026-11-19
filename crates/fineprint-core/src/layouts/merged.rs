// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! MUTATE and merged permanents (CR 702.140, 731): one object represented by several
//! components, whose topmost component supplies the characteristics (REQ-OBJ-002).
//!
//! The merge is a copiable effect applied in layer 1a with the timestamp of the merge
//! (CR 731.2a, 613.2a). There is no layer system yet, so this module only selects the base
//! characteristics that effect yields; applying it in layer 1a and its timestamp are later
//! work. Casting a mutating spell and performing the merge (CR 702.140c) are actions and are
//! not implemented.

use fineprint_dsl::card::{CardDefinitionDSL, FaceKind, LayoutBehavior};
use fineprint_dsl::vocab::RulesVocabulary;

use super::faces::{chars, effective_ctx, faces};
use super::layout_for;
use super::meld::MeldLayout;
use super::preparation::PreparationLayout;
use super::station::StationLayout;
use super::types::{Gating, Layout, LayoutError, LayoutQuery};
use crate::objects::{Characteristics, ManaValueContext};
use crate::zones::ZoneKind;

/// MUTATE (CR 702.140): a card with mutate, as an individual card: its printed
/// characteristics in every zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MutateLayout;

const LAYOUT: LayoutBehavior = LayoutBehavior::Mutate;

impl Layout for MutateLayout {
    fn behavior(&self) -> LayoutBehavior {
        LAYOUT
    }

    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        _query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        let [main] = faces(LAYOUT, def, [FaceKind::Main])?;
        chars(main, vocab)
    }

    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        // CR 202.3: the printed mana cost.
        Ok(self
            .base_characteristics(def, query, vocab)?
            .mana_value(effective_ctx(query, ctx)))
    }

    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
        faces(LAYOUT, def, [FaceKind::Main])?;
        Ok(None)
    }
}

/// Where a merging object is placed: on top of the permanent or under it (CR 702.140c, 731.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeSide {
    /// On top: it becomes the topmost component.
    Over,
    /// Under: it becomes the bottommost component.
    Under,
}

/// One card or token representing a merged permanent.
///
/// A token component's characteristics come from a definition of any supported layout used as
/// a stand-in: TOKEN definitions are unsupported until a later issue (DECISIONS.md #23).
#[derive(Clone, Copy, Debug)]
pub struct MergedComponent<'a> {
    /// The component's definition.
    pub def: &'a CardDefinitionDSL,
    /// Its cast choice and status; `query.zone` is ignored (the battlefield is always used).
    pub query: LayoutQuery,
    /// The component is a token.
    pub is_token: bool,
    /// The component is face up.
    pub face_up: bool,
}

/// The layout of a component: the five composite layouts' structs for MUTATE, MELD, STATION
/// and PREPARATION, [`layout_for`] for every other behavior.
fn component_layout(behavior: LayoutBehavior) -> Result<&'static dyn Layout, LayoutError> {
    Ok(match behavior {
        LayoutBehavior::Mutate => &MutateLayout,
        LayoutBehavior::Meld => &MeldLayout,
        LayoutBehavior::Station => &StationLayout,
        LayoutBehavior::Preparation => &PreparationLayout,
        other => return layout_for(other),
    })
}

impl MergedComponent<'_> {
    /// This component's query, on the battlefield.
    fn battlefield_query(&self) -> LayoutQuery {
        LayoutQuery {
            zone: ZoneKind::Battlefield,
            ..self.query
        }
    }

    /// This component's base characteristics on the battlefield, with its own layout.
    fn characteristics(&self, vocab: &RulesVocabulary) -> Result<Characteristics, LayoutError> {
        component_layout(self.def.layout)?.base_characteristics(
            self.def,
            &self.battlefield_query(),
            vocab,
        )
    }
}

/// A merged permanent: one object represented by two or more components, top to bottom
/// (CR 731.2).
///
/// Neither [`MergedPermanent::new`] nor [`MergedPermanent::merge`] models entering the
/// battlefield: the merged permanent is the same object it was, has not just entered and keeps
/// the effects that applied to it (CR 731.2b, 731.2c).
#[derive(Clone, Debug)]
pub struct MergedPermanent<'a> {
    components: Vec<MergedComponent<'a>>,
    mutated: bool,
}

impl<'a> MergedPermanent<'a> {
    /// A merged permanent with these components, topmost first; `mutated` is true for a
    /// mutated permanent (CR 702.140c), which has the abilities of every component.
    ///
    /// Only the count is checked here; characteristics are validated by
    /// [`MergedPermanent::base_characteristics`] and [`MergedPermanent::mana_value`].
    ///
    /// # Errors
    /// [`LayoutError::TooFewComponents`] with fewer than two components.
    pub fn new(
        components_top_to_bottom: Vec<MergedComponent<'a>>,
        mutated: bool,
    ) -> Result<MergedPermanent<'a>, LayoutError> {
        if components_top_to_bottom.len() < 2 {
            return Err(LayoutError::TooFewComponents {
                count: components_top_to_bottom.len(),
            });
        }
        Ok(MergedPermanent {
            components: components_top_to_bottom,
            mutated,
        })
    }

    /// This permanent with `component` merged on top ([`MergeSide::Over`]) or underneath
    /// ([`MergeSide::Under`]) (CR 702.140c, 731.2); `self` is unchanged.
    #[must_use]
    pub fn merge(&self, component: MergedComponent<'a>, side: MergeSide) -> MergedPermanent<'a> {
        let mut components = Vec::with_capacity(self.components.len() + 1);
        match side {
            MergeSide::Over => {
                components.push(component);
                components.extend(self.components.iter().copied());
            }
            MergeSide::Under => {
                components.extend(self.components.iter().copied());
                components.push(component);
            }
        }
        MergedPermanent {
            components,
            mutated: self.mutated,
        }
    }

    /// The components, topmost first.
    #[must_use]
    pub fn components(&self) -> &[MergedComponent<'a>] {
        &self.components
    }

    /// The topmost component. `new` guarantees at least two components.
    fn top(&self) -> &MergedComponent<'a> {
        &self.components[0]
    }

    /// The base characteristics of the merged permanent.
    ///
    /// The topmost component's, selected with its own layout on the battlefield; nothing from
    /// the other components (CR 731.2a, 707.2). A mutated permanent has the abilities of every
    /// component, top to bottom, without deduplication; every other characteristic, rules text
    /// included, stays the topmost component's (CR 702.140e).
    ///
    /// # Errors
    /// The topmost component's [`LayoutError`] ([`LayoutError::UnsupportedLayout`] for a
    /// layout no component may use), then, when mutated, the other components' errors in top
    /// to bottom order.
    pub fn base_characteristics(
        &self,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        // CR 731.2a: only the characteristics of the topmost component.
        let mut out = self.top().characteristics(vocab)?;
        if self.mutated {
            // CR 702.140e: all abilities of each card and token that represents it.
            for component in &self.components[1..] {
                out.abilities
                    .extend(component.characteristics(vocab)?.abilities);
            }
        }
        Ok(out)
    }

    /// The mana value of the merged permanent: the topmost component's, on the battlefield
    /// (X, Y and Z count 0) (CR 731.2a, 202.3).
    ///
    /// # Errors
    /// As [`MergedPermanent::base_characteristics`]: the same selection runs first.
    pub fn mana_value(&self, vocab: &RulesVocabulary) -> Result<i64, LayoutError> {
        self.base_characteristics(vocab)?;
        let top = self.top();
        component_layout(top.def.layout)?.mana_value(
            top.def,
            &top.battlefield_query(),
            ManaValueContext::OffStack,
            vocab,
        )
    }

    /// Whether the merged permanent is a token: only if its topmost component is (CR 731.2d).
    #[must_use]
    pub fn is_token(&self) -> bool {
        self.top().is_token
    }

    /// Whether the merged permanent is face up: its topmost component's status (CR 731.2e).
    #[must_use]
    pub fn face_up(&self) -> bool {
        self.top().face_up
    }

    /// The number of objects the merged permanent is: 1 (CR 702.140c, 731.2).
    #[must_use]
    pub fn object_count(&self) -> usize {
        1
    }

    /// The components put into the new zone when the merged permanent leaves the battlefield,
    /// top to bottom: one permanent leaves, each component moves (CR 731.3).
    #[must_use]
    pub fn components_leaving(&self) -> Vec<MergedComponent<'a>> {
        self.components.clone()
    }
}
