// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `fineprint-ingest vocab`: generates the `RulesVocabulary` (REQ-OBJ-001, REQ-SUB-001,
//! REQ-DSL-014; #15 FP-014) from the cached Comprehensive Rules text.
//!
//! The output is [`fineprint_dsl::vocab::RulesVocabulary::to_canonical_bytes`] of the parsed
//! vocabulary: the same CR bytes give the same output bytes on every platform (CRLF, CR and LF
//! input are equivalent, a BOM is ignored, nothing from the environment enters the output).
//!
//! # Fixed structural knowledge
//!
//! Only *where* each list lives is fixed; the lists themselves, their lengths, the token
//! letters and the number of rules all come from the text, so a new CR release flows through:
//!
//! - **Effective date**: the first line containing `effective as of <Month D, YYYY>`.
//! - **Card types**: CR 205.2a, the first sentence starting `The ` that contains ` are `; the
//!   items after ` are ` (capitalised).
//! - **Supertypes**: CR 205.4a, same sentence rule (capitalised).
//! - **Subtype groups**: every lettered sub-rule of CR 205.3 whose first clause names a list with
//!   `called [a|an] <group> type[s]`. The card types are the card-type words (singular or
//!   plural) in the first clause (up to the first `,` or `;`); the subtypes are the items of
//!   every sentence containing `<group> types are ` or `<group> type is ` (after a `:` when the
//!   sentence has one). Parentheticals are removed first. Other 205.3 sub-rules are ignored.
//! - **Keyword actions / abilities**: every unlettered rule `701.N` / `702.N` with N >= 2 is a
//!   heading; 701.1 and 702.1 are introductory and skipped. A heading must be heading-shaped (one
//!   line, at most 8 words and 64 characters, no `.` at its end or `. ` inside); anything else
//!   is an input error rather than a keyword. Ids follow [`keyword_id`].
//! - **Counter kinds**: the lettered sub-rules of CR 122.1. CR 122.1b is the keyword-counter
//!   list (the first sentence starting `The ` with ` are `; class `KEYWORD`); every other
//!   sub-rule contributes the word before its first ` counter`/` counters` (class
//!   `POWER_TOUGHNESS` for a `+X/+Y` shape, otherwise [`keyword_id`] of the kind).
//! - **Predefined tokens**: every lettered sub-rule of CR 111.10 (see [`tokens`]).
//!
//! Lists: items are separated by `,` (or a lone ` and `), the final `and`/`or` is removed,
//! trailing `as well as ...` qualifiers are dropped, and every item must look like a name.

use std::fmt;

use fineprint_dsl::vocab::{
    CounterKind, Keyword, RulesVocabulary, SubtypeGroup, VOCABULARY_VERSION,
};

pub mod cli;
pub mod cr;
pub mod tokens;

#[cfg(test)]
mod tests;

use cr::{CrDoc, Rule, capitalize, sentences, split_list, strip_parentheticals};

/// Why the vocabulary could not be generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenError {
    /// The CR text is unusable (encoding, markers, missing or unparseable rules): exit 2.
    Input(String),
    /// Two keywords map to the same id: exit 1.
    Collision(String),
}

impl GenError {
    /// The CLI exit code: 1 for a collision, 2 for an input error.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            GenError::Input(_) => 2,
            GenError::Collision(_) => 1,
        }
    }
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GenError::Input(m) => write!(f, "{m}"),
            GenError::Collision(m) => write!(f, "keyword id collision: {m}"),
        }
    }
}

impl std::error::Error for GenError {}

fn input(msg: impl Into<String>) -> GenError {
    GenError::Input(msg.into())
}

/// Parses the CR bytes into a validated vocabulary.
///
/// # Errors
/// [`GenError::Input`] for unusable input, [`GenError::Collision`] for a keyword id collision.
pub fn generate(cr_bytes: &[u8]) -> Result<RulesVocabulary, GenError> {
    let doc = cr::parse_doc(cr_bytes)?;
    let cr_effective_date = doc.effective_date()?;
    let card_types = capitalised_list(doc.require("205.2a")?)?;
    let supertypes = capitalised_list(doc.require("205.4a")?)?;
    let subtype_groups = subtype_groups(&doc, &card_types)?;
    let keyword_actions = keywords(&doc, "701")?;
    let keyword_abilities = keywords(&doc, "702")?;
    check_collisions(&keyword_actions, &keyword_abilities)?;
    let counter_kinds = counter_kinds(&doc)?;
    let mut vocab = RulesVocabulary {
        vocabulary_version: VOCABULARY_VERSION,
        cr_effective_date,
        card_types,
        supertypes,
        subtype_groups,
        keyword_abilities,
        keyword_actions,
        counter_kinds,
        predefined_tokens: Vec::new(),
    };
    vocab.predefined_tokens = tokens::predefined_tokens(&doc, &vocab)?;
    vocab
        .validate()
        .map_err(|e| input(format!("generated vocabulary is invalid: {e}")))?;
    Ok(vocab)
}

/// [`generate`] followed by [`RulesVocabulary::to_canonical_bytes`].
///
/// # Errors
/// As [`generate`].
pub fn generate_bytes(cr_bytes: &[u8]) -> Result<Vec<u8>, GenError> {
    generate(cr_bytes)?
        .to_canonical_bytes()
        .map_err(|e| input(e.to_string()))
}

/// The rule's text with parentheticals removed, split into sentences.
fn plain_sentences(rule: &Rule) -> Result<Vec<String>, GenError> {
    let text = strip_parentheticals(&rule.text)
        .ok_or_else(|| input(format!("rule {} has unbalanced parentheses", rule.id())))?;
    sentences(&text)
        .ok_or_else(|| input(format!("rule {} has unbalanced quotation marks", rule.id())))
}

/// The list of the first sentence starting `The ` that contains ` are `.
fn the_list(rule: &Rule) -> Result<Vec<String>, GenError> {
    let sentence = plain_sentences(rule)?
        .into_iter()
        .find(|s| s.starts_with("The ") && s.contains(" are "))
        .ok_or_else(|| input(format!("rule {} has no `The ... are ...` list", rule.id())))?;
    let list = sentence.split_once(" are ").map_or("", |(_, l)| l);
    split_list(list).ok_or_else(|| input(format!("rule {} has an unparseable list", rule.id())))
}

// CR 205.2a, CR 205.4a: card types and supertypes, capitalised (SRS §4.1 spelling).
fn capitalised_list(rule: &Rule) -> Result<Vec<String>, GenError> {
    Ok(the_list(rule)?.iter().map(|s| capitalize(s)).collect())
}

/// True when `word` (lower-case) is `card_type` (lower-case) or a plural of it.
fn names_card_type(word: &str, card_type: &str) -> bool {
    word == card_type
        || word.strip_suffix('s') == Some(card_type)
        || word.strip_suffix("es") == Some(card_type)
        || card_type
            .strip_suffix('y')
            .is_some_and(|stem| word.strip_suffix("ies") == Some(stem))
}

// CR 205.3: one group per lettered 205.3 sub-rule that names a subtype list (CR 205.3g..205.3q in
// the baseline).
fn subtype_groups(doc: &CrDoc, card_types: &[String]) -> Result<Vec<SubtypeGroup>, GenError> {
    let mut out = Vec::new();
    for rule in doc.children("205.3")? {
        let sents = plain_sentences(rule)?;
        let Some(first) = sents.first() else {
            continue;
        };
        let Some(group) = called_group(first) else {
            continue;
        };
        let clause = first.split([',', ';']).next().unwrap_or("");
        let mut group_types: Vec<String> = Vec::new();
        for word in clause.split(|c: char| !c.is_alphabetic()) {
            let word = word.to_lowercase();
            for ct in card_types {
                if names_card_type(&word, &ct.to_lowercase()) && !group_types.contains(ct) {
                    group_types.push(ct.clone());
                }
            }
        }
        if group_types.is_empty() {
            return Err(input(format!(
                "rule {} names no card type for its subtype list",
                rule.id()
            )));
        }
        let plural = format!("{group} types are ");
        let singular = format!("{group} type is ");
        let mut subtypes = Vec::new();
        for s in &sents[1..] {
            let Some((_, rest)) = s.split_once(&plural).or_else(|| s.split_once(&singular)) else {
                continue;
            };
            let list = rest.split_once(": ").map_or(rest, |(_, l)| l);
            let items = split_list(list)
                .ok_or_else(|| input(format!("rule {} has an unparseable list", rule.id())))?;
            subtypes.extend(items);
        }
        if subtypes.is_empty() {
            return Err(input(format!(
                "rule {} names {group} types but lists none",
                rule.id()
            )));
        }
        out.push(SubtypeGroup {
            group,
            card_types: group_types,
            rule_ref: rule.id(),
            subtypes,
        });
    }
    if out.is_empty() {
        return Err(input(
            "missing required rule 205.3g..: no CR 205.3 sub-rule lists subtypes",
        ));
    }
    Ok(out)
}

/// `... called [a|an] <group> type[s] ...` in the first clause -> `<group>`.
fn called_group(sentence: &str) -> Option<String> {
    let (_, rest) = sentence.split_once("called ")?;
    let rest = rest
        .strip_prefix("a ")
        .or_else(|| rest.strip_prefix("an "))
        .unwrap_or(rest);
    let mut words = rest.split_whitespace();
    let group = words.next()?;
    let next = words.next()?;
    let next = next.trim_end_matches([',', ';']);
    if (next == "type" || next == "types") && group.chars().all(char::is_alphabetic) {
        Some(group.to_lowercase())
    } else {
        None
    }
}

/// The keyword id of a heading name: apostrophes are removed, Latin-1 letters with diacritics
/// fold to their base letters, every other character that is not an ASCII letter or digit is a
/// separator, separator runs collapse to one `_`, leading and trailing separators are dropped,
/// and letters are upper-cased (`Double Strike` -> `DOUBLE_STRIKE`, `For Mirrodin!` ->
/// `FOR_MIRRODIN`). `None` when nothing remains.
#[must_use]
pub fn keyword_id(name: &str) -> Option<String> {
    let mut out = String::new();
    let mut pending_sep = false;
    for ch in name.chars() {
        if matches!(ch, '\'' | '\u{2019}' | '\u{2018}' | '\u{02bc}') {
            continue;
        }
        let folded = fold_latin1(ch);
        if folded.is_empty() {
            pending_sep = true;
            continue;
        }
        for c in folded.chars() {
            if pending_sep && !out.is_empty() {
                out.push('_');
            }
            pending_sep = false;
            out.push(c.to_ascii_uppercase());
        }
    }
    (!out.is_empty()).then_some(out)
}

/// ASCII alphanumerics map to themselves; Latin-1 letters with diacritics to their base
/// letters; everything else to "" (a separator).
fn fold_latin1(ch: char) -> &'static str {
    const ASCII: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    if ch.is_ascii_alphanumeric() {
        let i = ASCII.find(ch).unwrap_or(0);
        return &ASCII[i..=i];
    }
    match ch {
        'À'..='Å' | 'à'..='å' => "A",
        'Æ' | 'æ' => "AE",
        'Ç' | 'ç' => "C",
        'È'..='Ë' | 'è'..='ë' => "E",
        'Ì'..='Ï' | 'ì'..='ï' => "I",
        'Ð' | 'ð' => "D",
        'Ñ' | 'ñ' => "N",
        'Ò'..='Ö' | 'Ø' | 'ò'..='ö' | 'ø' => "O",
        'Ù'..='Ü' | 'ù'..='ü' => "U",
        'Ý' | 'ý' | 'ÿ' => "Y",
        'Þ' | 'þ' => "TH",
        'ß' => "SS",
        _ => "",
    }
}

/// A heading-shaped rule text (see the module documentation).
fn is_heading(rule: &Rule) -> bool {
    let t = rule.first_line.as_str();
    rule.continuation_lines == 0
        && !t.is_empty()
        && !t.ends_with('.')
        && !t.contains(". ")
        && t.chars().count() <= 64
        && t.split_whitespace().count() <= 8
}

// CR 701, CR 702: one keyword per unlettered `701.N` / `702.N` heading with N >= 2.
fn keywords(doc: &CrDoc, section: &str) -> Result<Vec<Keyword>, GenError> {
    let mut out: Vec<Keyword> = Vec::new();
    for rule in doc
        .rules
        .iter()
        .filter(|r| r.section == section && r.letters.is_empty())
    {
        if rule.number.trim_start_matches('0') == "1" {
            continue; // CR 701.1, CR 702.1: introductory paragraphs
        }
        if !is_heading(rule) {
            return Err(input(format!(
                "rule {} should be a keyword heading but is prose",
                rule.id()
            )));
        }
        let name = rule.first_line.clone();
        let id = keyword_id(&name)
            .ok_or_else(|| input(format!("rule {} heading gives an empty id", rule.id())))?;
        if let Some(prev) = out.iter().find(|k| k.id == id) {
            return Err(GenError::Collision(format!(
                "{} and {} both map to {id}",
                prev.rule_ref,
                rule.id()
            )));
        }
        if out.iter().any(|k| k.rule_ref == rule.id()) {
            return Err(input(format!(
                "rule {} appears more than once in the CR text",
                rule.id()
            )));
        }
        out.push(Keyword {
            id,
            name,
            rule_ref: rule.id(),
        });
    }
    if out.is_empty() {
        return Err(input(format!(
            "missing required rules {section}.2..: no keyword headings in section {section}"
        )));
    }
    Ok(out)
}

fn check_collisions(actions: &[Keyword], abilities: &[Keyword]) -> Result<(), GenError> {
    for a in abilities {
        if let Some(b) = actions.iter().find(|b| b.id == a.id) {
            return Err(GenError::Collision(format!(
                "{} and {} both map to {}",
                b.rule_ref, a.rule_ref, a.id
            )));
        }
    }
    Ok(())
}

/// True for a `+X/+Y` shaped kind (two signed one-letter or numeric parts around `/`).
fn is_pt_shape(kind: &str) -> bool {
    let Some((p, t)) = kind.split_once('/') else {
        return false;
    };
    let part = |s: &str| {
        let s = s.strip_prefix(['+', '-']).unwrap_or(s);
        !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric())
    };
    part(p) && part(t)
}

// CR 122.1: counter kinds with built-in rules; CR 122.1b is the keyword-counter list.
fn counter_kinds(doc: &CrDoc) -> Result<Vec<CounterKind>, GenError> {
    let children = doc.children("122.1")?;
    if !children.iter().any(|r| r.letters == "b") {
        return Err(input("missing required rule 122.1b in the CR text"));
    }
    let mut out: Vec<CounterKind> = Vec::new();
    for rule in children {
        if rule.letters == "b" {
            for kind in the_list(rule)? {
                out.push(CounterKind {
                    kind,
                    class: "KEYWORD".to_owned(),
                    rule_ref: rule.id(),
                });
            }
            continue;
        }
        let text = strip_parentheticals(&rule.text)
            .ok_or_else(|| input(format!("rule {} has unbalanced parentheses", rule.id())))?;
        let kind = counter_word(&text)
            .ok_or_else(|| input(format!("rule {} names no counter kind", rule.id())))?;
        let class = if is_pt_shape(&kind) {
            "POWER_TOUGHNESS".to_owned()
        } else {
            keyword_id(&kind)
                .ok_or_else(|| input(format!("rule {} counter kind gives no class", rule.id())))?
        };
        out.push(CounterKind {
            kind,
            class,
            rule_ref: rule.id(),
        });
    }
    Ok(out)
}

/// The word before the first ` counter` / ` counters` (a whole word) in `text`.
fn counter_word(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let idx = words.iter().position(|w| {
        let w = w.trim_end_matches(|c: char| !c.is_alphanumeric());
        w == "counter" || w == "counters"
    })?;
    let word = words.get(idx.checked_sub(1)?)?;
    let lower = word.to_lowercase();
    if matches!(
        lower.as_str(),
        "a" | "an" | "the" | "one" | "more" | "of" | "no" | "each" | "keyword"
    ) || word.contains([',', '.', ';', ':', '\u{201c}', '\u{201d}'])
    {
        return None;
    }
    Some((*word).to_owned())
}
