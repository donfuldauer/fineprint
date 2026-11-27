// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Card DSL types and runtime for the Fine Print rules engine (rules crate).

#![forbid(unsafe_code)]

/// RFC 8785 canonical JSON (REQ-NFR-006, REQ-DSL-001): duplicate-key and safe-integer checks
/// plus the JCS serialisation used for every content hash.
pub mod canonical;

/// Card definition top-level types (REQ-DSL-001, REQ-DSL-002): `CardDefinitionDSL`,
/// `CardFaceDSL`, layout and support enums, provenance, `dslVersion` checks and
/// `definitionHash`.
pub mod card;

/// Structured mana costs (REQ-OBJ-001, SRS §4.1): `ManaSymbol`, `ManaCostDSL` and the
/// canonical symbol text grammar.
pub mod mana;

/// References, selectors, predicates and integer value expressions (REQ-DSL-003,
/// REQ-CORE-006, SRS §14.4.0).
pub mod expr;

/// Rules text as a stream of text tokens with rewritable words and name references
/// (REQ-DSL-012, SRS §14.5).
pub mod text;

/// Decision descriptions: `ChoiceSpec` (REQ-DSL-011, SRS §14.4.6).
pub mod choice;

/// Ability envelope and the six ability kinds: triggers and events, activated abilities and
/// costs, static effects and replacements (REQ-DSL-004..REQ-DSL-009, SRS §14.3–§14.4.4).
pub mod ability;

/// Effect instructions a resolving spell or ability runs, their durations and the
/// `NATIVE_EFFECT` node (REQ-DSL-010, REQ-DSL-015, SRS §14.4.5, §14.8).
pub mod effect;

/// The `RulesVocabulary` contract (REQ-OBJ-001, REQ-SUB-001, REQ-DSL-014): card types,
/// supertypes, subtypes, keywords, counter kinds and predefined tokens extracted from the CR,
/// bundled from `data/vocab.json`.
pub mod vocab;

/// Static validation lints over card definitions (REQ-ING-009).
pub mod lint;
