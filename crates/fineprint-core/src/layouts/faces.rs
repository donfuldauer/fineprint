// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared checks: layout match and face sets (REQ-OBJ-002).

use fineprint_dsl::card::{CardDefinitionDSL, CardFaceDSL, FaceKind, LayoutBehavior};
use fineprint_dsl::vocab::RulesVocabulary;

use super::types::LayoutError;
use crate::objects::Characteristics;

/// Checks that `def` is a `layout` definition whose faces are exactly the face kinds in
/// `required`, each once, in any order; returns the faces in the order of `required`.
///
/// Faces are looked up by kind, never by index.
///
/// # Errors
/// [`LayoutError::LayoutMismatch`] when `def.layout != layout`, then [`LayoutError::BadFaces`].
pub(super) fn faces<'a, const N: usize>(
    layout: LayoutBehavior,
    def: &'a CardDefinitionDSL,
    required: [FaceKind; N],
) -> Result<[&'a CardFaceDSL; N], LayoutError> {
    if def.layout != layout {
        return Err(LayoutError::LayoutMismatch {
            expected: layout,
            found: def.layout,
        });
    }
    let bad = LayoutError::BadFaces { layout };
    // `required` holds distinct kinds; with exactly N faces and each required kind present
    // exactly once, the face set is exactly `required`.
    if def.faces.len() != N {
        return Err(bad);
    }
    let mut selected: Vec<&'a CardFaceDSL> = Vec::with_capacity(N);
    for kind in required {
        let mut found = def.faces.iter().filter(|f| f.face_kind == kind);
        match (found.next(), found.next()) {
            (Some(face), None) => selected.push(face),
            _ => return Err(bad),
        }
    }
    selected
        .try_into()
        .map_err(|_| LayoutError::BadFaces { layout })
}

/// The printed characteristics of `face`, wrapping a type-line error.
///
/// # Errors
/// [`LayoutError::Object`].
pub(super) fn chars(
    face: &CardFaceDSL,
    vocab: &RulesVocabulary,
) -> Result<Characteristics, LayoutError> {
    Ok(Characteristics::from_face(face, vocab)?)
}
