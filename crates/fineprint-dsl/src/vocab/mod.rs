// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The `RulesVocabulary` contract (REQ-OBJ-001, REQ-SUB-001, REQ-DSL-014; owner #15 FP-014).
//!
//! A [`RulesVocabulary`] is the rules vocabulary the Comprehensive Rules define, as data: card
//! types, supertypes, subtypes grouped by card type, keyword abilities and keyword actions,
//! counter kinds that carry rules and the predefined-token table. It holds names, identifiers,
//! rule references and token characteristics only, never rules prose (REQ-NFR-004).
//!
//! The vocabulary is generated offline by `fineprint-ingest vocab` from the cached CR text and
//! committed as `crates/fineprint-dsl/data/vocab.json`; [`RulesVocabulary::bundled`] embeds that
//! file at compile time, so loading needs no I/O. The wire form is versioned
//! ([`VOCABULARY_VERSION`]) and self-contained so a card database artifact can embed it
//! unchanged (REQ-ING-014).
//!
//! # Wire form
//!
//! A JSON object with exactly the members `vocabularyVersion`, `crEffectiveDate`, `cardTypes`,
//! `supertypes`, `subtypeGroups`, `keywordAbilities`, `keywordActions`, `counterKinds` and
//! `predefinedTokens` (see the field documentation of [`RulesVocabulary`]). Every list keeps CR
//! order. The file bytes are [`RulesVocabulary::to_canonical_bytes`]: the RFC 8785 canonical JSON
//! followed by exactly one `\n` (the trailing-newline convention of every committed text file).
//!
//! # Validation
//!
//! [`RulesVocabulary::from_slice`] treats its input as untrusted (R12): duplicate JSON keys,
//! unknown members, a wrong `vocabularyVersion`, malformed entries, cross-references to unknown
//! card types, supertypes or subtypes, and duplicate names or ids within a list are errors.
//!
//! Determinism (R3): every container is a `Vec` in CR order and every lookup is a linear or
//! ordered scan; there is no float, no hash-ordered container and no runtime I/O.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::canonical::{self, CanonicalError};

#[cfg(test)]
mod tests;

/// The only `vocabularyVersion` this loader accepts.
pub const VOCABULARY_VERSION: u32 = 1;

/// The color words a token face may carry, in canonical (WUBRG) order.
pub const COLOR_WORDS: [&str; 5] = ["white", "blue", "black", "red", "green"];

/// The committed vocabulary file, embedded at compile time.
const BUNDLED_JSON: &[u8] = include_bytes!("../../data/vocab.json");

/// The rules vocabulary (see the module documentation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesVocabulary {
    /// Wire version; always [`VOCABULARY_VERSION`].
    #[serde(rename = "vocabularyVersion")]
    pub vocabulary_version: u32,
    /// The effective date of the CR release the vocabulary was generated from (`YYYY-MM-DD`).
    #[serde(rename = "crEffectiveDate")]
    pub cr_effective_date: String,
    /// Card types with a leading capital, in CR order (CR 205.2a).
    #[serde(rename = "cardTypes")]
    pub card_types: Vec<String>,
    /// Supertypes with a leading capital, in CR order (CR 205.4a).
    #[serde(rename = "supertypes")]
    pub supertypes: Vec<String>,
    /// Subtype lists by card type, one group per CR 205.3 list rule, in CR order (CR 205.3).
    #[serde(rename = "subtypeGroups")]
    pub subtype_groups: Vec<SubtypeGroup>,
    /// Keyword abilities, one per CR 702 heading, in CR order (CR 702).
    #[serde(rename = "keywordAbilities")]
    pub keyword_abilities: Vec<Keyword>,
    /// Keyword actions, one per CR 701 heading, in CR order (CR 701).
    #[serde(rename = "keywordActions")]
    pub keyword_actions: Vec<Keyword>,
    /// Counter kinds that carry rules, in CR order (CR 122.1).
    #[serde(rename = "counterKinds")]
    pub counter_kinds: Vec<CounterKind>,
    /// The predefined tokens, in CR order (CR 111.10).
    #[serde(rename = "predefinedTokens")]
    pub predefined_tokens: Vec<PredefinedToken>,
}

/// One subtype list (CR 205.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubtypeGroup {
    /// The lower-case word the CR uses for the list (`artifact` for "artifact types").
    #[serde(rename = "group")]
    pub group: String,
    /// The card types the list belongs to (capitalised, from [`RulesVocabulary::card_types`]).
    #[serde(rename = "cardTypes")]
    pub card_types: Vec<String>,
    /// The rule that holds the list.
    #[serde(rename = "ruleRef")]
    pub rule_ref: String,
    /// The subtypes, in CR order and spelling.
    #[serde(rename = "subtypes")]
    pub subtypes: Vec<String>,
}

/// A keyword ability or keyword action (CR 701, CR 702).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyword {
    /// Upper-case snake identifier (`DOUBLE_STRIKE`), unique across both keyword lists.
    #[serde(rename = "id")]
    pub id: String,
    /// The heading name as the CR spells it.
    #[serde(rename = "name")]
    pub name: String,
    /// The heading rule (`702.9`).
    #[serde(rename = "ruleRef")]
    pub rule_ref: String,
}

/// A counter kind with built-in rules (CR 122.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CounterKind {
    /// The counter's kind as the CR spells it (`shield`, `first strike`, `+X/+Y`).
    #[serde(rename = "kind")]
    pub kind: String,
    /// The rules class: `KEYWORD` for keyword counters, otherwise an upper-case identifier.
    #[serde(rename = "class")]
    pub class: String,
    /// The rule that gives the counter its rules.
    #[serde(rename = "ruleRef")]
    pub rule_ref: String,
}

/// A predefined token (CR 111.10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PredefinedToken {
    /// The rule's letter suffix (`a` for 111.10a): one lower-case ASCII letter.
    #[serde(rename = "letter")]
    pub letter: String,
    /// The rule that defines the token.
    #[serde(rename = "ruleRef")]
    pub rule_ref: String,
    /// The token's name.
    #[serde(rename = "name")]
    pub name: String,
    /// One face, or two for a double-faced token.
    #[serde(rename = "faces")]
    pub faces: Vec<TokenFace>,
}

/// The characteristics of one face of a predefined token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenFace {
    /// The face's name.
    #[serde(rename = "name")]
    pub name: String,
    /// Color words from [`COLOR_WORDS`], in that order; `[]` for colorless.
    #[serde(rename = "colors")]
    pub colors: Vec<String>,
    /// Supertypes (from [`RulesVocabulary::supertypes`]).
    #[serde(rename = "supertypes")]
    pub supertypes: Vec<String>,
    /// Card types (from [`RulesVocabulary::card_types`]); never empty.
    #[serde(rename = "cardTypes")]
    pub card_types: Vec<String>,
    /// Subtypes, each a subtype of one of the face's card types.
    #[serde(rename = "subtypes")]
    pub subtypes: Vec<String>,
    /// Printed power, `null` when absent. The member is required on the wire (an explicit
    /// `null`, never an omitted member).
    #[serde(rename = "power", deserialize_with = "required_nullable")]
    pub power: Option<i64>,
    /// Printed toughness, `null` when absent. Required on the wire, as `power`.
    #[serde(rename = "toughness", deserialize_with = "required_nullable")]
    pub toughness: Option<i64>,
    /// The face's abilities in order: keyword phrases and quoted ability texts without quotes.
    #[serde(rename = "abilities")]
    pub abilities: Vec<String>,
}

/// Deserialises a nullable integer member. Used through `deserialize_with` (and without
/// `#[serde(default)]`), it makes the member required: serde treats an `Option` field as optional
/// only when it has no `deserialize_with`, so an omitted `power` or `toughness` is a missing
/// field while an explicit `null` is `None`.
fn required_nullable<'de, D>(d: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<i64>::deserialize(d)
}

/// Why a vocabulary could not be loaded or serialised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VocabError {
    /// The bytes are not acceptable JSON (syntax, duplicate key, unsafe integer, ...).
    Json(CanonicalError),
    /// `vocabularyVersion` is missing, not an integer, or not [`VOCABULARY_VERSION`].
    Version(String),
    /// The JSON does not have the wire shape (unknown member, wrong type, missing member).
    Shape(String),
    /// The shape is right but a value breaks a rule (path and reason).
    Invalid {
        /// Where the value is (`predefinedTokens[3].letter`).
        path: String,
        /// What is wrong with it.
        reason: String,
    },
}

impl fmt::Display for VocabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VocabError::Json(e) => write!(f, "vocabulary JSON: {e}"),
            VocabError::Version(v) => write!(
                f,
                "vocabularyVersion {v} is not supported (expected {VOCABULARY_VERSION})"
            ),
            VocabError::Shape(msg) => write!(f, "vocabulary shape: {msg}"),
            VocabError::Invalid { path, reason } => write!(f, "vocabulary {path}: {reason}"),
        }
    }
}

impl std::error::Error for VocabError {}

impl From<CanonicalError> for VocabError {
    fn from(e: CanonicalError) -> Self {
        VocabError::Json(e)
    }
}

fn invalid(path: impl Into<String>, reason: impl Into<String>) -> VocabError {
    VocabError::Invalid {
        path: path.into(),
        reason: reason.into(),
    }
}

impl RulesVocabulary {
    /// Loads and validates a vocabulary from untrusted JSON bytes.
    ///
    /// # Errors
    /// [`VocabError::Json`] for invalid JSON, duplicate keys or unsafe integers;
    /// [`VocabError::Version`] for a wrong `vocabularyVersion`; [`VocabError::Shape`] for unknown
    /// or missing members and wrong value types; [`VocabError::Invalid`] when [`Self::validate`]
    /// fails.
    pub fn from_slice(bytes: &[u8]) -> Result<RulesVocabulary, VocabError> {
        let value = canonical::parse_checked(bytes)?;
        let Value::Object(map) = &value else {
            return Err(VocabError::Shape(
                "the document is not a JSON object".to_owned(),
            ));
        };
        match map.get("vocabularyVersion") {
            Some(Value::Number(n)) if n.as_u64() == Some(u64::from(VOCABULARY_VERSION)) => {}
            Some(v) => return Err(VocabError::Version(v.to_string())),
            None => return Err(VocabError::Version("(missing)".to_owned())),
        }
        let vocab: RulesVocabulary =
            serde_json::from_value(value).map_err(|e| VocabError::Shape(e.to_string()))?;
        vocab.validate()?;
        Ok(vocab)
    }

    /// [`Self::from_slice`] on the UTF-8 bytes of `s`.
    ///
    /// # Errors
    /// As [`Self::from_slice`].
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<RulesVocabulary, VocabError> {
        Self::from_slice(s.as_bytes())
    }

    /// The vocabulary committed as `crates/fineprint-dsl/data/vocab.json`, embedded at compile
    /// time. It is parsed once and memoised; the memo is write-once and its value depends only on
    /// the embedded bytes, so it cannot make results differ (R3).
    ///
    /// # Errors
    /// The loader's error if the committed file is invalid (the test suite guards against it).
    pub fn bundled() -> Result<&'static RulesVocabulary, VocabError> {
        static BUNDLED: OnceLock<Result<RulesVocabulary, VocabError>> = OnceLock::new();
        BUNDLED
            .get_or_init(|| RulesVocabulary::from_slice(BUNDLED_JSON))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The embedded bytes of the committed vocabulary file.
    #[must_use]
    pub fn bundled_bytes() -> &'static [u8] {
        BUNDLED_JSON
    }

    /// The file form: RFC 8785 canonical JSON followed by exactly one `\n`. The committed file
    /// and the generator output are produced by this function.
    ///
    /// # Errors
    /// [`VocabError::Json`] if a value cannot be represented (an integer beyond the safe range).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, VocabError> {
        let mut bytes = canonical::to_canonical_vec(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Checks every rule of the wire contract (see the module documentation).
    ///
    /// # Errors
    /// [`VocabError::Version`] or [`VocabError::Invalid`] naming the first offending value.
    pub fn validate(&self) -> Result<(), VocabError> {
        if self.vocabulary_version != VOCABULARY_VERSION {
            return Err(VocabError::Version(self.vocabulary_version.to_string()));
        }
        check_date(&self.cr_effective_date)?;
        check_name_list(&self.card_types, "cardTypes")?;
        if self.card_types.is_empty() {
            return Err(invalid("cardTypes", "must not be empty"));
        }
        check_name_list(&self.supertypes, "supertypes")?;
        self.check_subtype_groups()?;
        let mut ids = BTreeSet::new();
        check_keywords(&self.keyword_actions, "keywordActions", &mut ids)?;
        check_keywords(&self.keyword_abilities, "keywordAbilities", &mut ids)?;
        self.check_counters()?;
        self.check_tokens()
    }

    fn check_subtype_groups(&self) -> Result<(), VocabError> {
        let mut groups = BTreeSet::new();
        for (i, g) in self.subtype_groups.iter().enumerate() {
            let p = format!("subtypeGroups[{i}]");
            check_name(&g.group, &format!("{p}.group"))?;
            if !groups.insert(g.group.as_str()) {
                return Err(invalid(format!("{p}.group"), "duplicate group"));
            }
            check_rule_ref(&g.rule_ref, &format!("{p}.ruleRef"))?;
            check_name_list(&g.card_types, &format!("{p}.cardTypes"))?;
            if g.card_types.is_empty() {
                return Err(invalid(format!("{p}.cardTypes"), "must not be empty"));
            }
            for ct in &g.card_types {
                if !self.is_card_type(ct) {
                    return Err(invalid(format!("{p}.cardTypes"), "unknown card type"));
                }
            }
            check_name_list(&g.subtypes, &format!("{p}.subtypes"))?;
            if g.subtypes.is_empty() {
                return Err(invalid(format!("{p}.subtypes"), "must not be empty"));
            }
        }
        Ok(())
    }

    fn check_counters(&self) -> Result<(), VocabError> {
        let mut kinds = BTreeSet::new();
        for (i, c) in self.counter_kinds.iter().enumerate() {
            let p = format!("counterKinds[{i}]");
            check_name(&c.kind, &format!("{p}.kind"))?;
            if !kinds.insert(c.kind.as_str()) {
                return Err(invalid(format!("{p}.kind"), "duplicate kind"));
            }
            if !is_upper_snake(&c.class) {
                return Err(invalid(
                    format!("{p}.class"),
                    "must be an upper-case identifier",
                ));
            }
            check_rule_ref(&c.rule_ref, &format!("{p}.ruleRef"))?;
        }
        Ok(())
    }

    fn check_tokens(&self) -> Result<(), VocabError> {
        let mut letters = BTreeSet::new();
        let mut names = BTreeSet::new();
        for (i, t) in self.predefined_tokens.iter().enumerate() {
            let p = format!("predefinedTokens[{i}]");
            let b = t.letter.as_bytes();
            if b.len() != 1 || !b[0].is_ascii_lowercase() {
                return Err(invalid(
                    format!("{p}.letter"),
                    "must be one lower-case ASCII letter",
                ));
            }
            if !letters.insert(t.letter.as_str()) {
                return Err(invalid(format!("{p}.letter"), "duplicate letter"));
            }
            check_rule_ref(&t.rule_ref, &format!("{p}.ruleRef"))?;
            check_name(&t.name, &format!("{p}.name"))?;
            if !names.insert(t.name.as_str()) {
                return Err(invalid(format!("{p}.name"), "duplicate token name"));
            }
            if t.faces.is_empty() || t.faces.len() > 2 {
                return Err(invalid(format!("{p}.faces"), "must have one or two faces"));
            }
            for (j, f) in t.faces.iter().enumerate() {
                self.check_face(f, &format!("{p}.faces[{j}]"))?;
            }
        }
        Ok(())
    }

    fn check_face(&self, f: &TokenFace, p: &str) -> Result<(), VocabError> {
        check_name(&f.name, &format!("{p}.name"))?;
        let mut last = None;
        for c in &f.colors {
            let Some(pos) = COLOR_WORDS.iter().position(|w| w == c) else {
                return Err(invalid(format!("{p}.colors"), "unknown color word"));
            };
            if last.is_some_and(|l| l >= pos) {
                return Err(invalid(
                    format!("{p}.colors"),
                    "colors must be distinct and in white, blue, black, red, green order",
                ));
            }
            last = Some(pos);
        }
        check_name_list(&f.supertypes, &format!("{p}.supertypes"))?;
        if let Some(s) = f.supertypes.iter().find(|s| !self.is_supertype(s)) {
            return Err(invalid(
                format!("{p}.supertypes"),
                format!("unknown supertype {s:?}"),
            ));
        }
        check_name_list(&f.card_types, &format!("{p}.cardTypes"))?;
        if f.card_types.is_empty() {
            return Err(invalid(format!("{p}.cardTypes"), "must not be empty"));
        }
        if let Some(c) = f.card_types.iter().find(|c| !self.is_card_type(c)) {
            return Err(invalid(
                format!("{p}.cardTypes"),
                format!("unknown card type {c:?}"),
            ));
        }
        check_name_list(&f.subtypes, &format!("{p}.subtypes"))?;
        for s in &f.subtypes {
            if !f.card_types.iter().any(|c| self.is_subtype_of(c, s)) {
                return Err(invalid(
                    format!("{p}.subtypes"),
                    format!("{s:?} is not a subtype of the face's card types"),
                ));
            }
        }
        if f.power.is_some() != f.toughness.is_some() {
            return Err(invalid(
                p.to_owned(),
                "power and toughness must both be present or both null",
            ));
        }
        for v in [f.power, f.toughness].into_iter().flatten() {
            if v.unsigned_abs() > canonical::MAX_SAFE_INTEGER.unsigned_abs() {
                return Err(invalid(p.to_owned(), "power/toughness out of range"));
            }
        }
        for (k, a) in f.abilities.iter().enumerate() {
            check_name(a, &format!("{p}.abilities[{k}]"))?;
        }
        Ok(())
    }

    /// True when `name` is a card type (exact, capitalised spelling).
    #[must_use]
    pub fn is_card_type(&self, name: &str) -> bool {
        self.card_types.iter().any(|c| c == name)
    }

    /// True when `name` is a supertype (exact, capitalised spelling).
    #[must_use]
    pub fn is_supertype(&self, name: &str) -> bool {
        self.supertypes.iter().any(|s| s == name)
    }

    /// The subtypes of `card_type` (CR 205.3): the union of every group that lists the card
    /// type, in group order then CR order, without repeats. Empty for an unknown card type or a
    /// card type with no subtypes.
    pub fn subtypes_of<'a>(&'a self, card_type: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        let mut seen = BTreeSet::new();
        self.subtype_groups
            .iter()
            .filter(move |g| g.card_types.iter().any(|c| c == card_type))
            .flat_map(|g| g.subtypes.iter().map(String::as_str))
            .filter(move |s| seen.insert(*s))
    }

    /// True when `name` is a subtype of `card_type` (exact spelling).
    #[must_use]
    pub fn is_subtype_of(&self, card_type: &str, name: &str) -> bool {
        self.subtypes_of(card_type).any(|s| s == name)
    }

    /// The keyword ability with identifier `id` (CR 702).
    #[must_use]
    pub fn keyword_ability(&self, id: &str) -> Option<&Keyword> {
        self.keyword_abilities.iter().find(|k| k.id == id)
    }

    /// The keyword action with identifier `id` (CR 701).
    #[must_use]
    pub fn keyword_action(&self, id: &str) -> Option<&Keyword> {
        self.keyword_actions.iter().find(|k| k.id == id)
    }

    /// The predefined token with letter `key` (one lower-case letter) or, failing that, the
    /// token named `key` (CR 111.10).
    #[must_use]
    pub fn token(&self, key: &str) -> Option<&PredefinedToken> {
        self.predefined_tokens
            .iter()
            .find(|t| t.letter == key)
            .or_else(|| self.predefined_tokens.iter().find(|t| t.name == key))
    }

    /// The counter kind `kind` if it carries rules (CR 122.1).
    #[must_use]
    pub fn counter_kind(&self, kind: &str) -> Option<&CounterKind> {
        self.counter_kinds.iter().find(|c| c.kind == kind)
    }
}

impl std::str::FromStr for RulesVocabulary {
    type Err = VocabError;

    fn from_str(s: &str) -> Result<RulesVocabulary, VocabError> {
        RulesVocabulary::from_slice(s.as_bytes())
    }
}

/// A name: non-empty, no surrounding whitespace, no control characters.
fn check_name(s: &str, path: &str) -> Result<(), VocabError> {
    if s.is_empty() {
        return Err(invalid(path, "must not be empty"));
    }
    if s.trim() != s {
        return Err(invalid(path, "must not have surrounding whitespace"));
    }
    if s.chars().any(char::is_control) {
        return Err(invalid(path, "must not contain control characters"));
    }
    Ok(())
}

/// A list of distinct names.
fn check_name_list(list: &[String], path: &str) -> Result<(), VocabError> {
    let mut seen = BTreeSet::new();
    for (i, s) in list.iter().enumerate() {
        check_name(s, &format!("{path}[{i}]"))?;
        if !seen.insert(s.as_str()) {
            return Err(invalid(format!("{path}[{i}]"), format!("duplicate {s:?}")));
        }
    }
    Ok(())
}

fn check_keywords(
    list: &[Keyword],
    path: &str,
    ids: &mut BTreeSet<String>,
) -> Result<(), VocabError> {
    let mut names = BTreeSet::new();
    for (i, k) in list.iter().enumerate() {
        let p = format!("{path}[{i}]");
        if !is_upper_snake(&k.id) {
            return Err(invalid(
                format!("{p}.id"),
                "must be an upper-case snake identifier",
            ));
        }
        if !ids.insert(k.id.clone()) {
            return Err(invalid(
                format!("{p}.id"),
                "duplicate keyword id (ids are unique across abilities and actions)",
            ));
        }
        check_name(&k.name, &format!("{p}.name"))?;
        if !names.insert(k.name.as_str()) {
            return Err(invalid(format!("{p}.name"), "duplicate keyword name"));
        }
        check_rule_ref(&k.rule_ref, &format!("{p}.ruleRef"))?;
    }
    Ok(())
}

/// `[A-Z0-9]+(_[A-Z0-9]+)*`.
#[must_use]
pub fn is_upper_snake(s: &str) -> bool {
    !s.is_empty()
        && s.split('_').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        })
}

/// True for a dotted CR rule number: `ddd.n` with up to two lower-case letters.
#[must_use]
pub fn is_rule_ref(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 5 || !b[..3].iter().all(u8::is_ascii_digit) || b[3] != b'.' {
        return false;
    }
    let rest = &b[4..];
    let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
    let letters = &rest[digits..];
    digits > 0 && letters.len() <= 2 && letters.iter().all(u8::is_ascii_lowercase)
}

fn check_rule_ref(s: &str, path: &str) -> Result<(), VocabError> {
    if is_rule_ref(s) {
        Ok(())
    } else {
        Err(invalid(path, "must be a CR rule number such as 205.3g"))
    }
}

/// `YYYY-MM-DD` naming a real calendar day.
fn check_date(s: &str) -> Result<(), VocabError> {
    let bad = || invalid("crEffectiveDate", "must be a calendar date YYYY-MM-DD");
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return Err(bad());
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = &b[r];
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        part.iter()
            .try_fold(0u32, |acc, d| Some(acc * 10 + u32::from(d - b'0')))
    };
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else {
        return Err(bad());
    };
    if d == 0 || d > days_in_month(y, m) {
        return Err(bad());
    }
    Ok(())
}

/// Days in month `m` of year `y` (0 for a month outside 1..=12).
#[must_use]
pub fn days_in_month(y: u32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400) => 29,
        2 => 28,
        _ => 0,
    }
}
