// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared checks: layout match and face sets, and the mana-value context (REQ-OBJ-002).

use fineprint_dsl::card::{CardDefinitionDSL, CardFaceDSL, FaceKind, LayoutBehavior};
use fineprint_dsl::vocab::RulesVocabulary;

use super::types::{LayoutError, LayoutQuery};
use crate::objects::{Characteristics, ManaValueContext};
use crate::zones::ZoneKind;

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

/// The mana-value context that applies to an object in `query.zone`: the caller's `ctx` on the
/// stack, [`ManaValueContext::OffStack`] in every other zone whatever `ctx` says.
pub(super) fn effective_ctx(query: &LayoutQuery, ctx: ManaValueContext) -> ManaValueContext {
    if query.zone == ZoneKind::Stack {
        // CR 202.3e: on the stack X is the number chosen for it (CR 107.3p: Y likewise).
        ctx
    } else {
        // CR 202.3e, 107.3g: off the stack X is 0, even on the battlefield (CR 107.3m); Y
        // follows the same rules (CR 107.3p) and Z is treated the same way.
        ManaValueContext::OffStack
    }
}
