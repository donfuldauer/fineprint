// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Double-faced cards, DFC_NONMODAL and DFC_MODAL (CR 712): only the face that is up
//! (REQ-OBJ-002).

use fineprint_dsl::card::{CardDefinitionDSL, CardFaceDSL, FaceKind, LayoutBehavior};
use fineprint_dsl::vocab::RulesVocabulary;

use super::faces::{chars, effective_ctx, faces};
use super::types::{CastChoice, FaceUp, Gating, Layout, LayoutError, LayoutQuery};
use crate::objects::{Characteristics, ManaValueContext, ManaValueSubject, mana_value};
use crate::zones::ZoneKind;

/// DFC_NONMODAL (CR 712): transforming double-faced cards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DfcNonmodalLayout;

/// DFC_MODAL (CR 712): modal double-faced cards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DfcModalLayout;

/// The front and back faces of a `layout` definition.
fn dfc_faces(
    layout: LayoutBehavior,
    def: &CardDefinitionDSL,
) -> Result<[&CardFaceDSL; 2], LayoutError> {
    faces(layout, def, [FaceKind::Main, FaceKind::Back])
}

/// Which face is up for a nonmodal double-faced object.
fn nonmodal_face(query: &LayoutQuery) -> Result<FaceUp, LayoutError> {
    match query.zone {
        ZoneKind::Stack => match query.cast {
            // CR 712.6b: normally the front face is up on the stack.
            CastChoice::None => Ok(FaceUp::Front),
            // CR 712.6b, 712.8a: cast "transformed" or "converted", back face up.
            CastChoice::Transformed => Ok(FaceUp::Back),
            _ => Err(LayoutError::InvalidCast {
                layout: LayoutBehavior::DfcNonmodal,
                zone: query.zone,
            }),
        },
        // CR 712.6c, 712.6d: the face that is up.
        ZoneKind::Battlefield => Ok(query.status.face_up),
        // CR 712.6a: in every other zone only the front face.
        _ => Ok(FaceUp::Front),
    }
}

/// Which face is up for a modal double-faced object.
fn modal_face(query: &LayoutQuery) -> Result<FaceUp, LayoutError> {
    match query.zone {
        ZoneKind::Stack => match query.cast {
            // CR 712.8b, 712.6e: the face chosen before it is put on the stack.
            CastChoice::ModalFace(face) => Ok(face),
            _ => Err(LayoutError::InvalidCast {
                layout: LayoutBehavior::DfcModal,
                zone: query.zone,
            }),
        },
        // CR 712.6e: the face that is up (CR 712.9, 712.10, 712.11, 712.11a: how it entered).
        ZoneKind::Battlefield => Ok(query.status.face_up),
        // CR 712.6a: in every other zone only the front face.
        _ => Ok(FaceUp::Front),
    }
}

/// The characteristics of the face `up`.
fn face_chars(
    [front, back]: [&CardFaceDSL; 2],
    up: FaceUp,
    vocab: &RulesVocabulary,
) -> Result<Characteristics, LayoutError> {
    match up {
        FaceUp::Front => chars(front, vocab),
        FaceUp::Back => chars(back, vocab),
    }
}

impl Layout for DfcNonmodalLayout {
    fn behavior(&self) -> LayoutBehavior {
        LayoutBehavior::DfcNonmodal
    }

    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        let faces = dfc_faces(LayoutBehavior::DfcNonmodal, def)?;
        let up = nonmodal_face(query)?;
        // CR 712.6a-712.6d: only the characteristics of the face that is up.
        face_chars(faces, up, vocab)
    }

    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        let selected = self.base_characteristics(def, query, vocab)?;
        let [front, _] = dfc_faces(LayoutBehavior::DfcNonmodal, def)?;
        let copy_in_play = matches!(query.zone, ZoneKind::Stack | ZoneKind::Battlefield);
        let subject = if query.status.copying_back_face && copy_in_play {
            // CR 202.3b, 712.6d: a permanent or spell that is a copy of the back face of a
            // nonmodal double-faced object has mana value 0. In every other zone the flag is
            // ignored and the front face's cost applies (CR 712.6a).
            ManaValueSubject::CopyOfNonmodalBackFace
        } else {
            match nonmodal_face(query)? {
                // CR 202.3: the front face's own cost.
                FaceUp::Front => ManaValueSubject::Plain {
                    mana_cost: selected.mana_cost.as_ref(),
                },
                // CR 202.3b, 712.6b, 712.6d: the back face uses the front face's cost.
                FaceUp::Back => ManaValueSubject::NonmodalBackFace {
                    front_mana_cost: front.mana_cost.as_ref(),
                },
            }
        };
        Ok(mana_value(&subject, effective_ctx(query, ctx)))
    }

    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
        dfc_faces(LayoutBehavior::DfcNonmodal, def)?;
        Ok(None)
    }
}

impl Layout for DfcModalLayout {
    fn behavior(&self) -> LayoutBehavior {
        LayoutBehavior::DfcModal
    }

    fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        let faces = dfc_faces(LayoutBehavior::DfcModal, def)?;
        let up = modal_face(query)?;
        // CR 712.6a, 712.6e: only the characteristics of the face that is up.
        face_chars(faces, up, vocab)
    }

    fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &LayoutQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        // CR 712.6e, 202.3a: the selected face's own cost; a modal face with no mana cost has
        // mana value 0 (the front-cost rule of CR 202.3b is for nonmodal back faces only).
        Ok(self
            .base_characteristics(def, query, vocab)?
            .mana_value(effective_ctx(query, ctx)))
    }

    fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
        dfc_faces(LayoutBehavior::DfcModal, def)?;
        Ok(None)
    }
}
