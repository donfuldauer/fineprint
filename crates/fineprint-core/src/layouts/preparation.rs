// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! PREPARATION (CR 723): normal characteristics in every zone; the prepare spell defines the
//! characteristics of the copy a prepared permanent creates (REQ-OBJ-002).
//!
//! The prepare spell stays in the definition in every zone, so its existence and values remain
//! part of the object's copiable values (CR 723.2a, 723.2b). The card itself can never be cast
//! as its prepare spell (CR 723.3): [`PreparationLayout`] never selects it.
//!
//! Not implemented here (actions): creating the copy in exile and casting it, the copy's
//! exception to CR 704.5e, who may cast it, becoming unprepared (CR 723.3b) and losing the
//! prepared designation as the copy becomes cast (CR 723.3c, 601.2i).

use fineprint_dsl::card::{CardDefinitionDSL, FaceKind, LayoutBehavior};
use fineprint_dsl::vocab::RulesVocabulary;

use super::faces::{chars, effective_ctx, faces};
use super::types::{Gating, Layout, LayoutError, LayoutQuery};
use crate::objects::{Characteristics, ManaValueContext};
use crate::zones::ZoneKind;

/// PREPARATION (CR 723).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreparationLayout;

const LAYOUT: LayoutBehavior = LayoutBehavior::Preparation;

/// The key of the prepared designation in a permanent's designations map (CR 723.3a); its
/// value is `DesignationValue::Flag`.
pub const PREPARED_DESIGNATION: &str = "prepared";

/// The face set of a preparation card: exactly one Main face and one PrepareSpell face.
const FACES: [FaceKind; 2] = [FaceKind::Main, FaceKind::PrepareSpell];

impl Layout for PreparationLayout {
    fn behavior(&self) -> LayoutBehavior {
        LAYOUT
    }

    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        _query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        let [main, _] = faces(LAYOUT, def, FACES)?;
        // CR 723.4: in every zone, the stack and battlefield included, only the normal
        // characteristics; CR 723.3: the card can't be cast as its prepare spell.
        chars(main, vocab)
    }

    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        // CR 202.3: the normal mana cost.
        Ok(self
            .base_characteristics(def, query, vocab)?
            .mana_value(effective_ctx(query, ctx)))
    }

    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
        faces(LAYOUT, def, FACES)?;
        Ok(None)
    }
}

/// The characteristics of the prepare spell of `def`: the normal characteristics of the copy
/// created in exile, which has only the prepare spell's characteristics (CR 723.3c).
///
/// # Errors
/// [`LayoutError::LayoutMismatch`], [`LayoutError::BadFaces`], then [`LayoutError::Object`].
pub fn prepare_spell_characteristics(
    def: &CardDefinitionDSL,
    vocab: &RulesVocabulary,
) -> Result<Characteristics, LayoutError> {
    let [_, prepare] = faces(LAYOUT, def, FACES)?;
    // CR 723.3c: "has only the characteristics of that permanent's prepare spell".
    chars(prepare, vocab)
}

/// Checks that a permanent with definition `def` in `zone` can become prepared and returns
/// the characteristics of the copy it creates ([`prepare_spell_characteristics`]).
///
/// The caller then gives the permanent the [`PREPARED_DESIGNATION`] designation. Creating and
/// casting the copy, who may cast it, becoming unprepared and losing the designation when the
/// copy becomes cast (CR 723.3b, 723.3c, 601.2i) are actions and are not implemented.
///
/// # Errors
/// In this order: [`LayoutError::LayoutMismatch`] and [`LayoutError::BadFaces`] (only a
/// permanent with a prepare spell can become prepared, CR 723.3a);
/// [`LayoutError::InvalidCast`] unless `zone` is the battlefield (prepared is a permanent's
/// designation); [`LayoutError::AlreadyPrepared`] if `already_prepared` (CR 723.3a); then
/// [`LayoutError::Object`].
pub fn become_prepared(
    def: &CardDefinitionDSL,
    zone: ZoneKind,
    already_prepared: bool,
    vocab: &RulesVocabulary,
) -> Result<Characteristics, LayoutError> {
    // CR 723.3a: a permanent can't gain the designation unless it has a prepare spell.
    faces(LAYOUT, def, FACES)?;
    if zone != ZoneKind::Battlefield {
        return Err(LayoutError::InvalidCast {
            layout: LAYOUT,
            zone,
        });
    }
    // CR 723.3a: a permanent can't gain the designation if it already has it.
    if already_prepared {
        return Err(LayoutError::AlreadyPrepared);
    }
    // CR 723.3c: the copy has only the prepare spell's characteristics.
    prepare_spell_characteristics(def, vocab)
}
