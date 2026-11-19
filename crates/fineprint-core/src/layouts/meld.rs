// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! MELD (CR 713, 701.42): an unmelded meld card has only its front face; a melded pair is one
//! object with the combined back face (REQ-OBJ-002).
//!
//! Meld cards are not double-faced cards (CR 713.1) and cannot be transformed or converted
//! (CR 713.1c), so no status or cast choice changes an unmelded card's characteristics. The
//! meld action itself (CR 701.42a, 713.6) is not implemented: [`MeldedPair`] describes a pair
//! the caller has already put onto the battlefield melded.

use fineprint_dsl::card::{CardDefinitionDSL, CardFaceDSL, FaceKind, LayoutBehavior};
use fineprint_dsl::vocab::RulesVocabulary;

use super::faces::{chars, effective_ctx};
use super::types::{Gating, Layout, LayoutError, LayoutQuery};
use crate::objects::{Characteristics, ManaValueContext, ManaValueSubject, mana_value};

/// MELD (CR 713): an unmelded meld card, in every zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeldLayout;

const LAYOUT: LayoutBehavior = LayoutBehavior::Meld;

/// The Main face and the optional MeldResult face of a MELD definition.
///
/// # Errors
/// [`LayoutError::LayoutMismatch`], then [`LayoutError::BadFaces`] unless the faces are exactly
/// one Main face and at most one MeldResult face.
fn meld_faces(
    def: &CardDefinitionDSL,
) -> Result<(&CardFaceDSL, Option<&CardFaceDSL>), LayoutError> {
    if def.layout != LAYOUT {
        return Err(LayoutError::LayoutMismatch {
            expected: LAYOUT,
            found: def.layout,
        });
    }
    let mut main = None;
    let mut result = None;
    for face in &def.faces {
        let slot = match face.face_kind {
            FaceKind::Main => &mut main,
            FaceKind::MeldResult => &mut result,
            _ => return Err(LayoutError::BadFaces { layout: LAYOUT }),
        };
        if slot.replace(face).is_some() {
            return Err(LayoutError::BadFaces { layout: LAYOUT });
        }
    }
    match main {
        Some(main) => Ok((main, result)),
        None => Err(LayoutError::BadFaces { layout: LAYOUT }),
    }
}

impl Layout for MeldLayout {
    fn behavior(&self) -> LayoutBehavior {
        LAYOUT
    }

    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        _query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        let (main, _) = meld_faces(def)?;
        // CR 713.5a, 713.5b: an unmelded meld card has only its front face's characteristics
        // in every zone; CR 713.1b: the back face never determines them outside a melded
        // permanent.
        chars(main, vocab)
    }

    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        // CR 202.3: the front face's own mana cost.
        Ok(self
            .base_characteristics(def, query, vocab)?
            .mana_value(effective_ctx(query, ctx)))
    }

    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
        meld_faces(def)?;
        Ok(None)
    }
}

/// A melded permanent: one object represented by the two cards of a meld pair (CR 701.42a,
/// 713.1a), with the characteristics of the combined back face (CR 713.5c).
#[derive(Clone, Copy, Debug)]
pub struct MeldedPair<'a> {
    first: &'a CardDefinitionDSL,
    second: &'a CardDefinitionDSL,
    fronts: [&'a CardFaceDSL; 2],
    combined: &'a CardFaceDSL,
}

impl<'a> MeldedPair<'a> {
    /// The melded permanent formed by `first` and `second`.
    ///
    /// The pairing compares each definition's `meld_partner` with the other's `card_def_id`
    /// (data of the two definitions, never a literal).
    ///
    /// # Errors
    /// In this order: [`LayoutError::LayoutMismatch`] unless both are MELD definitions;
    /// [`LayoutError::BadFaces`] unless both have valid face sets; [`LayoutError::NotMeldPair`]
    /// unless each names the other as its meld partner and they are different cards
    /// (CR 701.42b); [`LayoutError::BadFaces`] unless exactly one carries the MeldResult face,
    /// the combined back face (CR 713.5c).
    pub fn new(
        first: &'a CardDefinitionDSL,
        second: &'a CardDefinitionDSL,
    ) -> Result<MeldedPair<'a>, LayoutError> {
        for def in [first, second] {
            if def.layout != LAYOUT {
                return Err(LayoutError::LayoutMismatch {
                    expected: LAYOUT,
                    found: def.layout,
                });
            }
        }
        let (front_a, result_a) = meld_faces(first)?;
        let (front_b, result_b) = meld_faces(second)?;
        // CR 701.42b: only the two cards of the same meld pair can be melded.
        let names = |def: &CardDefinitionDSL, other: &CardDefinitionDSL| {
            def.meld_partner.as_deref() == Some(other.card_def_id.as_str())
        };
        if !names(first, second) || !names(second, first) || first.card_def_id == second.card_def_id
        {
            return Err(LayoutError::NotMeldPair);
        }
        // CR 713.5c: the pair's back faces form one combined face, carried by one definition.
        let combined = match (result_a, result_b) {
            (Some(face), None) | (None, Some(face)) => face,
            _ => return Err(LayoutError::BadFaces { layout: LAYOUT }),
        };
        Ok(MeldedPair {
            first,
            second,
            fronts: [front_a, front_b],
            combined,
        })
    }

    /// The characteristics of the melded permanent: only the combined back face's (CR 713.5c).
    ///
    /// # Errors
    /// [`LayoutError::Object`] if the combined face's type line is rejected.
    pub fn characteristics(&self, vocab: &RulesVocabulary) -> Result<Characteristics, LayoutError> {
        // CR 713.5c: only the combined back face; nothing from either front face.
        chars(self.combined, vocab)
    }

    /// The mana value of the melded permanent: the sum of the two front faces' mana values, or
    /// 0 for a copy of a melded permanent (CR 202.3c, 713.5c).
    ///
    /// # Errors
    /// As [`MeldedPair::characteristics`]: the combined face is validated first.
    pub fn mana_value(
        &self,
        is_copy_of_melded: bool,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        self.characteristics(vocab)?;
        // CR 202.3c, 713.5c: the combined cost of both front faces; a copy has mana value 0.
        let subject = if is_copy_of_melded {
            ManaValueSubject::CopyOfMelded
        } else {
            ManaValueSubject::Melded {
                front_mana_costs: [
                    self.fronts[0].mana_cost.as_ref(),
                    self.fronts[1].mana_cost.as_ref(),
                ],
            }
        };
        Ok(mana_value(&subject, ManaValueContext::OffStack))
    }

    /// The number of objects the melded permanent is: 1 (CR 713.8e).
    #[must_use]
    pub fn object_count(&self) -> usize {
        1
    }

    /// The number of cards that represent it: 2 (CR 713.8e).
    #[must_use]
    pub fn card_count(&self) -> usize {
        2
    }

    /// The cards put into the new zone when the melded permanent leaves the battlefield, in
    /// the order `[first, second]`: one permanent leaves, two cards move (CR 713.8).
    #[must_use]
    pub fn cards_on_leaving(&self) -> [&'a CardDefinitionDSL; 2] {
        [self.first, self.second]
    }
}
