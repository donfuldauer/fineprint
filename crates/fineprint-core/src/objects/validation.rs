// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Type-line validation against the rules vocabulary (REQ-OBJ-001, CR 205).

use fineprint_dsl::vocab::RulesVocabulary;

use super::error::ObjectError;

/// Checks a type line against `vocab` (CR 205.2a, 205.3c, 205.3d, 205.4a, 205.4b).
///
/// Checks run in this order and the first failure is returned: every card type must be known
/// ([`ObjectError::UnknownCardType`]), every supertype must be known
/// ([`ObjectError::UnknownSupertype`]), and every subtype must be a subtype of at least one of
/// the card types ([`ObjectError::SubtypeNotCorrelated`]; always so when `card_types` is empty).
/// A supertype is never rejected for the card type it appears with (CR 205.4b). Matching is
/// exact and case-sensitive.
///
/// # Errors
/// The first failing check, as above.
pub fn validate_types(
    supertypes: &[&str],
    card_types: &[&str],
    subtypes: &[&str],
    vocab: &RulesVocabulary,
) -> Result<(), ObjectError> {
    // CR 205.2a: the card types come from the vocabulary.
    if let Some(bad) = card_types.iter().find(|c| !vocab.is_card_type(c)) {
        return Err(ObjectError::UnknownCardType((*bad).to_owned()));
    }
    // CR 205.4a: the supertypes come from the vocabulary; CR 205.4b: any card type is allowed.
    if let Some(bad) = supertypes.iter().find(|s| !vocab.is_supertype(s)) {
        return Err(ObjectError::UnknownSupertype((*bad).to_owned()));
    }
    // CR 205.3c: each subtype is correlated to one of the object's card types.
    if let Some(bad) = subtypes
        .iter()
        .find(|s| !can_gain_subtype(card_types, s, vocab))
    {
        return Err(ObjectError::SubtypeNotCorrelated {
            subtype: (*bad).to_owned(),
        });
    }
    Ok(())
}

/// Whether an object with `card_types` can have `subtype`: true iff it is a subtype of at least
/// one of them (CR 205.3d; shared lists per CR 205.3k, 205.3m; none per CR 205.3r).
#[must_use]
pub fn can_gain_subtype(card_types: &[&str], subtype: &str, vocab: &RulesVocabulary) -> bool {
    // CR 205.3d: an object can't gain a subtype that doesn't correspond to one of its types.
    card_types.iter().any(|c| vocab.is_subtype_of(c, subtype))
}
