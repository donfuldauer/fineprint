// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The compile step from the DSL text-changing effect to the engine's layer modification
//! (REQ-DSL-012, SRS §14.5; CR 612.1, 612.2, 613.1c).
//!
//! A DSL layer 3 [`CharacteristicEffect`] whose modification is
//! `{"op":"REPLACE_TEXT_WORD","category","from","to"}` ([`Modification::as_replace_text_word`])
//! compiles into [`LayerModification::ReplaceTextWord`]. Category `CARD_NAME` and empty or
//! noncharacter words are refused with exactly the [`LayerError::Unsupported`] that
//! registration gives (CR 612.2), so every `Ok` result passes
//! [`super::ContinuousEffects::register`] validation. The category and words are the literal
//! values chosen on resolution: the resolving interpreter builds the modification with them
//! ([`Modification::replace_text_word`]) and calls this step.
//!
//! **Determinism.** Pure functions of their argument; they never panic, read no card name or
//! id (REQ-DSL-013) and use no float, hash-ordered container or I/O (REQ-CORE-001).

use fineprint_dsl::ability::{CharacteristicEffect, Layer, Modification};

use super::error::LayerError;
use super::registry::LayerModification;
use super::text::validate_modification;

/// Compiles a DSL `REPLACE_TEXT_WORD` modification into
/// [`LayerModification::ReplaceTextWord`] (REQ-DSL-012, SRS §14.5; CR 612.1, 612.2). The
/// result always passes registration validation. `from == to` compiles: it is an ordinary
/// replacement that changes nothing.
///
/// # Errors
/// [`LayerError::Unsupported`]: `"not a text-changing modification"` for any other op;
/// `"REPLACE_TEXT_WORD with malformed members"` when the typed view
/// ([`Modification::as_replace_text_word`]) refuses the members;
/// `"REPLACE_TEXT_WORD of category CARD_NAME (CR 612.2)"` for category `CARD_NAME`; and the
/// registration error for an empty or noncharacter `from` or `to`.
pub fn compile_text_modification(m: &Modification) -> Result<LayerModification, LayerError> {
    let (category, from, to) = match m.as_replace_text_word() {
        Ok(Some(parts)) => parts,
        Ok(None) => return Err(LayerError::Unsupported("not a text-changing modification")),
        Err(_) => {
            return Err(LayerError::Unsupported(
                "REPLACE_TEXT_WORD with malformed members",
            ));
        }
    };
    let out = LayerModification::ReplaceTextWord { category, from, to };
    // CR 612.2: the same checks, in the same order and with the same errors, as registration
    // (category CARD_NAME first, then the words).
    validate_modification(&out)?;
    Ok(out)
}

/// Compiles a DSL characteristic effect carrying `REPLACE_TEXT_WORD` (REQ-DSL-012, SRS
/// §14.5): as [`compile_text_modification`], and the effect must declare layer 3, the layer of
/// text-changing effects (CR 613.1c). Its affected set and CDA flag are the caller's to
/// register; only the modification is compiled.
///
/// # Errors
/// Every error of [`compile_text_modification`] (checked first); then
/// `LayerError::Unsupported("REPLACE_TEXT_WORD outside layer 3 (CR 613.1c)")` for any other
/// layer.
pub fn compile_text_effect(e: &CharacteristicEffect) -> Result<LayerModification, LayerError> {
    let out = compile_text_modification(&e.modification)?;
    // CR 613.1c: text-changing effects apply in layer 3.
    if e.layer != Layer::L3 {
        return Err(LayerError::Unsupported(
            "REPLACE_TEXT_WORD outside layer 3 (CR 613.1c)",
        ));
    }
    Ok(out)
}
