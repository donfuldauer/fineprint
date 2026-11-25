// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Checks of the AST content of a card definition against the real DSL types (REQ-DSL-003,
//! REQ-DSL-010, REQ-DSL-012, REQ-NFR-006).
//!
//! The card wrappers ([`ValueExpressionAST`], [`TextTokenStream`], [`AbilityAST`],
//! [`PartnerAbilityDSL`]) keep their permissive `serde_json::Value` content and plain serde
//! (docs/DECISIONS.md, "#159 FP-140 — Card wrappers validated against the real AST types").
//! Each offers a typed view `to_real` that decodes the content with the strict decoder of the
//! real type ([`crate::expr::ValueExpressionAST`], [`crate::text::TextTokenStream`],
//! [`crate::ability::AbilityAST`]) and succeeds only if the content is also the single canonical
//! spelling of the decoded value: re-encoding the real value (through the checked serializer of
//! [`crate::canonical`]) must give exactly the original JSON. Validation therefore never changes
//! the canonical JSON or the hash of a valid definition.
//!
//! Decoding reads the content through a borrowed `&Value`; the real decoders buffer it with a
//! depth check before recursing, so even a hand-built value nested far beyond
//! [`crate::canonical::MAX_DEPTH`] is an error, never a stack overflow, and nothing is cloned
//! before that check. Every function here is total: errors are returned, never panics (R12).

use serde::de::value::SeqDeserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    AbilityAST, CardDefinitionDSL, CardDefinitionError, PartnerAbilityDSL, TextTokenStream,
    ValueExpressionAST,
};
use crate::{ability, canonical, expr, text};

/// The `kind` every partner ability must have (docs/DECISIONS.md, "#159 FP-140").
const PARTNER_KIND: &str = "KEYWORD";

/// Fails unless `real` re-encodes to exactly `original` (one JSON spelling per valid AST).
pub(super) fn single_spelling<T: Serialize + ?Sized>(
    real: &T,
    original: &Value,
) -> Result<(), String> {
    let encoded = canonical::to_checked_value(real).map_err(|e| e.to_string())?;
    if encoded == *original {
        Ok(())
    } else {
        Err("the content is not the canonical JSON form of the decoded AST".to_owned())
    }
}

/// Decodes `value` strictly as `T` from the borrowed value and checks its single spelling.
fn decode<'a, T: Deserialize<'a> + Serialize>(value: &'a Value) -> Result<T, String> {
    let real = T::deserialize(value).map_err(|e| e.to_string())?;
    single_spelling(&real, value)?;
    Ok(real)
}

impl ValueExpressionAST {
    /// Typed view: the content decoded as a real [`expr::ValueExpressionAST`] (REQ-DSL-003).
    ///
    /// # Errors
    /// The decoder's message (a `String`) if the content is not a valid value expression, or if
    /// it is not the expression's canonical JSON spelling. A present `null` is an error.
    pub fn to_real(&self) -> Result<expr::ValueExpressionAST, String> {
        decode(&self.0)
    }
}

impl TextTokenStream {
    /// Typed view: the tokens decoded as a real [`text::TextTokenStream`] (REQ-DSL-012).
    ///
    /// # Errors
    /// The decoder's message (a `String`) if a token is invalid, ids repeat, or the content is
    /// not the stream's canonical JSON spelling.
    pub fn to_real(&self) -> Result<text::TextTokenStream, String> {
        let tokens = SeqDeserializer::<_, serde_json::Error>::new(self.0.iter());
        let real = text::TextTokenStream::deserialize(tokens).map_err(|e| e.to_string())?;
        match canonical::to_checked_value(&real).map_err(|e| e.to_string())? {
            Value::Array(items) if items == self.0 => Ok(real),
            _ => Err(
                "the content is not the canonical JSON form of the decoded token stream".to_owned(),
            ),
        }
    }
}

impl AbilityAST {
    /// Typed view: the content decoded as a real [`ability::AbilityAST`] (REQ-DSL-010).
    ///
    /// Effect lists are checked only as far as [`ability::AbilityAST`]'s own decoder checks them
    /// (docs/DECISIONS.md, "#159 FP-140").
    ///
    /// # Errors
    /// The decoder's message (a `String`) if the content is not a valid ability, or if it is not
    /// the ability's canonical JSON spelling.
    pub fn to_real(&self) -> Result<ability::AbilityAST, String> {
        decode(&self.0)
    }
}

impl PartnerAbilityDSL {
    /// Typed view: the content decoded as a real [`ability::AbilityAST`] whose `kind` is
    /// `KEYWORD` (REQ-DSL-010; partner variants are keyword abilities, docs/DECISIONS.md
    /// "#159 FP-140").
    ///
    /// # Errors
    /// The decoder's message (a `String`) if the content is not a valid ability or not its
    /// canonical JSON spelling, or a message naming the kind if it is not a `KEYWORD` ability.
    pub fn to_real(&self) -> Result<ability::AbilityAST, String> {
        let real: ability::AbilityAST = decode(&self.0)?;
        if real.kind() == PARTNER_KIND {
            Ok(real)
        } else {
            Err(format!(
                "a partner ability must be a {PARTNER_KIND} ability, found kind {}",
                real.kind()
            ))
        }
    }
}

impl CardDefinitionDSL {
    /// Checks every AST position against the real DSL types (REQ-DSL-003, REQ-DSL-010,
    /// REQ-DSL-012): each face's `power` and `toughness` as a value expression, its `text` as a
    /// token stream, each of its `abilities` as an ability, and each
    /// `commanderEligibility.partnerAbilities` entry as a `KEYWORD` ability. Each position must
    /// decode and be the canonical spelling of the decoded value (see the `to_real` views), so a
    /// passing check never changes the canonical JSON or the hash (REQ-NFR-006).
    ///
    /// [`CardDefinitionDSL::seal`] and [`CardDefinitionDSL::from_json`] run this check;
    /// [`CardDefinitionDSL::to_canonical_json`], [`CardDefinitionDSL::compute_hash`],
    /// [`CardDefinitionDSL::verify_hash`] and plain serde do not.
    ///
    /// # Errors
    /// [`CardDefinitionError::Schema`] for the first invalid position in document order, with a
    /// message that names it, for example `faces[1].abilities[0]` or
    /// `commanderEligibility.partnerAbilities[0]`.
    pub fn validate_asts(&self) -> Result<(), CardDefinitionError> {
        self.check_asts().map_err(CardDefinitionError::Schema)
    }

    /// Body of [`CardDefinitionDSL::validate_asts`]: the first failure as a located message.
    pub(super) fn check_asts(&self) -> Result<(), String> {
        let at =
            |location: String, message: String| format!("invalid AST at {location}: {message}");
        for (i, face) in self.faces.iter().enumerate() {
            if let Some(power) = &face.power {
                power
                    .to_real()
                    .map_err(|e| at(format!("faces[{i}].power"), e))?;
            }
            if let Some(toughness) = &face.toughness {
                toughness
                    .to_real()
                    .map_err(|e| at(format!("faces[{i}].toughness"), e))?;
            }
            face.text
                .to_real()
                .map_err(|e| at(format!("faces[{i}].text"), e))?;
            for (j, ability) in face.abilities.iter().enumerate() {
                ability
                    .to_real()
                    .map_err(|e| at(format!("faces[{i}].abilities[{j}]"), e))?;
            }
        }
        if let Some(eligibility) = &self.commander_eligibility {
            for (j, partner) in eligibility.partner_abilities.iter().enumerate() {
                partner
                    .to_real()
                    .map_err(|e| at(format!("commanderEligibility.partnerAbilities[{j}]"), e))?;
            }
        }
        Ok(())
    }
}
