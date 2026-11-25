// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Oracle text tokenizer (SRS §16 pipeline step [3], REQ-ING-004, REQ-DSL-012).
//!
//! [`tokenize_face`] turns one face's raw Oracle text into ability-sized pieces, each with a
//! validated [`TextTokenStream`]. The pipeline runs in this order:
//!
//! 1. **Normalize** (REQ-ING-004(e)): [`normalize_text`] strips a BOM, turns CRLF and CR into LF,
//!    Unicode space separators and tabs into an ASCII space, curly quotes into ASCII quotes, and
//!    the hyphen, figure dash, en dash and minus sign into `-`; the em dash stays. Noncharacters
//!    become U+FFFD so every token validates. Trailing spaces of each line are trimmed. The result
//!    is the display original ([`TokenizedFace::display`]).
//! 2. **Strip** (REQ-ING-004(a), CR 207.2a): [`strip_reminder_text`] removes balanced
//!    parenthesized spans of each line, except the spans the client configuration declares
//!    rules text ([`TokenizerConfig`]). A line left empty is dropped.
//! 3. **Split** (REQ-ING-004(d), CR 113.2c): each remaining line is an ability, except that mode
//!    bullets join their header (CR 700.2) and station symbols (CR 722.2), level bands
//!    (CR 711.2) and class levels (CR 717.2a) open [`Section`]s the following abilities belong
//!    to. Chapters (CR 715.2) and anchor words (CR 207.2c) are recorded on the ability.
//! 4. **Mark** (REQ-ING-004(b)/(c), CR 612.2): self-references become `NameRef{SELF}`, configured
//!    other names `NameRef{OTHER}`, and color words, basic land types and creature types become
//!    `Word` tokens; everything else is literal text. Ids `t1`, `t2`, ... run face-wide in text
//!    order.
//!
//! The module is pure: no I/O, clock or environment, ordered containers only, and the output is
//! a function of the text, the face name and subtypes, the vocabulary and the configuration
//! (R3). It never panics on any `&str` input (R12). Only the raw Oracle text is capped by
//! [`MAX_TEXT_BYTES`]. The configuration has its own limits ([`MAX_CONFIG_BYTES`],
//! [`MAX_CONFIG_STRING_BYTES`] and [`MAX_CONFIG_LIST_LEN`], enforced when it is loaded), and the
//! face name and subtypes come from import records that were already size-capped. It builds no
//! AST.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fineprint_dsl::text::{NameRefKind, TextToken, TextTokenStream};
use fineprint_dsl::vocab::RulesVocabulary;

use crate::import::normalize::{NormalizedCardRecord, NormalizedFace};

mod config;
mod lines;
mod mark;
mod normalize;

#[cfg(test)]
mod tests;

pub use config::{
    CONFIG_FORMAT_VERSION, MAX_CONFIG_BYTES, MAX_CONFIG_LIST_LEN, MAX_CONFIG_STRING_BYTES,
};

use mark::{Lexicon, NamePattern};

/// Largest Oracle text, in bytes, a face is tokenized from; longer text is cut at the last char
/// boundary within the limit and [`TokenizedFace::truncated`] is set.
pub const MAX_TEXT_BYTES: usize = 65_536;

/// The text [`render_tokens`] uses for a `NameRef{SELF}` token when rebuilding text.
pub const SELF_REF_PLACEHOLDER: &str = "~";

/// The client configuration of one card (REQ-ING-004(a)/(b)). The repository holds no card list:
/// a client supplies this data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CardTextConfig {
    /// Exact parenthesized spans (normalized, parentheses included) that are rules text and are
    /// kept when reminder text is stripped.
    pub keep_parentheses: BTreeSet<String>,
    /// Shortened names that refer to the card itself (CR 201.5c); they become `NameRef{SELF}`.
    pub short_names: BTreeSet<String>,
    /// Other names the text refers to; they become `NameRef{OTHER}` carrying the name.
    pub other_names: BTreeSet<String>,
}

/// The tokenizer's client configuration (REQ-ING-004(a)/(b)): per-card entries keyed by the
/// record's `oracle_id` ([`NormalizedCardRecord::card_def_id`]). Load it with
/// [`TokenizerConfig::from_json`]; the default is empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenizerConfig {
    /// Card key → entry, in key order.
    pub cards: BTreeMap<String, CardTextConfig>,
}

/// Why a configuration document was refused (R12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// The document is larger than [`MAX_CONFIG_BYTES`].
    TooLarge {
        /// The document length in bytes.
        len: usize,
    },
    /// The document is not well-formed JSON (the `serde_json` message).
    Json(String),
    /// The document is JSON of the wrong shape.
    Shape(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::TooLarge { len } => write!(
                f,
                "tokenizer config is {len} bytes, above the {MAX_CONFIG_BYTES}-byte limit"
            ),
            ConfigError::Json(m) => write!(f, "tokenizer config is not valid JSON: {m}"),
            ConfigError::Shape(m) => write!(f, "tokenizer config has the wrong shape: {m}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// The input of [`tokenize_face`]: one face's raw Oracle text and its context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenizeInput<'a> {
    /// The key of the card's [`TokenizerConfig`] entry (the `oracle_id`).
    pub card_key: &'a str,
    /// The face name; whole-word, case-sensitive occurrences are self-references (CR 201.5).
    pub face_name: &'a str,
    /// The face's own subtypes; "this <subtype>" is a self-reference.
    pub subtypes: &'a [String],
    /// The raw Oracle text.
    pub text: &'a str,
}

impl<'a> TokenizeInput<'a> {
    /// The input for `face` of the card keyed `card_key`; absent Oracle text is empty text.
    #[must_use]
    pub fn from_face(card_key: &'a str, face: &'a NormalizedFace) -> TokenizeInput<'a> {
        TokenizeInput {
            card_key,
            face_name: &face.name,
            subtypes: &face.subtypes,
            text: face.oracle_text.as_deref().unwrap_or(""),
        }
    }
}

/// One tokenized piece of text: an ability body, a modal header or a mode (REQ-DSL-012).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenizedText {
    /// The normalized source line this piece comes from, reminder text included (for display).
    pub original: String,
    /// The text that was marked: reminder text stripped, structural prefixes (station symbol,
    /// class level, chapter numerals, anchor, bullet) removed, trimmed.
    pub text: String,
    /// The validated tokens of `text`.
    pub tokens: TextTokenStream,
    /// The byte range of `text` each token covers, parallel to `tokens`; the ranges are
    /// contiguous and cover `text` exactly.
    pub spans: Vec<(usize, usize)>,
}

/// The structural kind of an ability (REQ-ING-004(d)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbilityKind {
    /// An ordinary ability line (CR 113.2c).
    Plain,
    /// An ability with a leading ability word or flavor word (CR 207.2c, 207.2d), held in
    /// [`TokenizedAbility::anchor`] and not interpreted.
    Anchored,
    /// A modal ability (CR 700.2): the header is [`TokenizedAbility::body`], the modes follow in
    /// order. Bullet modes (700.2) and pawprint modes (700.2i) are both modes; a cost before the
    /// effect (700.2h) stays in the mode text.
    Modal {
        /// The modes, in text order.
        modes: Vec<TokenizedText>,
    },
    /// A chapter ability (CR 715.2b); a numeral list means the same as separate chapters
    /// (CR 715.2c).
    Chapter {
        /// The chapter numbers, strictly increasing.
        chapters: Vec<u32>,
    },
}

/// One ability of a face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenizedAbility {
    /// The normalized source line(s), reminder text included (a modal ability joins its header
    /// and mode lines with `\n`).
    pub original: String,
    /// The structural kind.
    pub kind: AbilityKind,
    /// The anchor (ability word or flavor word) text, if the ability had one.
    pub anchor: Option<String>,
    /// The index in [`TokenizedFace::sections`] of the section this ability belongs to, or `None`
    /// for an ability outside every section (CR 722.4, 711.4).
    pub section: Option<usize>,
    /// The ability text and its tokens (the header for a modal ability).
    pub body: TokenizedText,
}

/// The kind of a [`Section`] (REQ-ING-004(d)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SectionKind {
    /// A station symbol `N+` striation (CR 722.2).
    Station {
        /// The threshold `N`.
        threshold: u32,
    },
    /// A level band striation `LEVEL min-max` or `LEVEL min+` (CR 711.2a, 711.2b).
    LevelBand {
        /// The lower bound.
        min: u32,
        /// The upper bound, `None` for `min+`.
        max: Option<u32>,
    },
    /// A class level (CR 717.2a); level 1 is the section above the first header (CR 717.3).
    ClassLevel {
        /// The level.
        level: u32,
        /// The cost text before the colon; `None` for the implicit level 1.
        cost: Option<String>,
    },
}

/// A group of abilities under one station symbol, level band or class level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// What the section is.
    pub kind: SectionKind,
    /// The normalized header line, or `None` for the implicit class level 1.
    pub header: Option<String>,
    /// A bare power/toughness line of a station or level band striation, as source text
    /// (CR 711.2, 722.2); never converted to numbers.
    pub pt: Option<String>,
}

/// The tokenizer output for one face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenizedFace {
    /// The normalized Oracle text, reminder text included, for display.
    pub display: String,
    /// True when the raw text exceeded [`MAX_TEXT_BYTES`] and was cut.
    pub truncated: bool,
    /// The abilities in text order.
    pub abilities: Vec<TokenizedAbility>,
    /// The sections in text order; [`TokenizedAbility::section`] indexes this list.
    pub sections: Vec<Section>,
}

/// Normalizes typographic variants of Oracle text (REQ-ING-004(e)); see the module docs for the
/// mapping. Idempotent.
#[must_use]
pub fn normalize_text(text: &str) -> String {
    normalize::normalize(text)
}

/// Strips reminder text from one normalized line (REQ-ING-004(a), CR 207.2a): balanced top-level
/// parenthesized spans are removed unless their exact text is in `keep`; unbalanced parentheses
/// stay. After a removal, spaces collapse, no space precedes `.`, `,`, `;` or `:`, and the line
/// is trimmed.
#[must_use]
pub fn strip_reminder_text(line: &str, keep: &BTreeSet<String>) -> String {
    normalize::strip_reminder(line, keep)
}

/// Rebuilds text from tokens: literal text, word values and other names as they are, and
/// `NameRef{SELF}` as `self_placeholder` (REQ-DSL-012).
#[must_use]
pub fn render_tokens(tokens: &TextTokenStream, self_placeholder: &str) -> String {
    let mut out = String::new();
    for t in tokens.tokens() {
        match t {
            TextToken::Literal { text, .. } => out.push_str(text),
            TextToken::Word { value, .. } => out.push_str(value),
            TextToken::NameRef {
                kind: NameRefKind::Other,
                name: Some(name),
                ..
            } => out.push_str(name),
            TextToken::NameRef { .. } => out.push_str(self_placeholder),
        }
    }
    out
}

/// Tokenizes every face of `record`, keyed by its `oracle_id` (REQ-ING-004).
#[must_use]
pub fn tokenize_record(
    record: &NormalizedCardRecord,
    vocab: &RulesVocabulary,
    config: &TokenizerConfig,
) -> Vec<TokenizedFace> {
    record
        .faces
        .iter()
        .map(|f| {
            tokenize_face(
                &TokenizeInput::from_face(&record.card_def_id, f),
                vocab,
                config,
            )
        })
        .collect()
}

/// One non-empty line after stripping.
struct Line {
    /// The normalized line, reminder text included.
    original: String,
    /// The stripped, trimmed line.
    text: String,
}

/// Cuts `text` to at most [`MAX_TEXT_BYTES`] on a char boundary.
fn truncate(text: &str) -> (&str, bool) {
    if text.len() <= MAX_TEXT_BYTES {
        return (text, false);
    }
    let mut end = MAX_TEXT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

/// Builds the tokenized pieces with one face-wide id counter.
struct Marker<'a> {
    /// The face lexicon.
    lex: &'a Lexicon,
    /// The name patterns.
    names: &'a [NamePattern],
    /// The last id number used.
    counter: u64,
}

impl Marker<'_> {
    /// Marks `text` (REQ-ING-004(b)/(c)).
    fn piece(&mut self, original: &str, text: &str) -> TokenizedText {
        let (tokens, spans) = mark::mark(text, self.lex, self.names, &mut self.counter);
        // The tokens are non-empty, uniquely numbered and free of noncharacters (normalization
        // replaced them), so validation succeeds; the fallback keeps the function total.
        let (tokens, spans) = match TextTokenStream::new(tokens) {
            Ok(t) => (t, spans),
            Err(_) => (TextTokenStream::default(), Vec::new()),
        };
        TokenizedText {
            original: original.to_string(),
            text: text.to_string(),
            tokens,
            spans,
        }
    }
}

/// The self and other name patterns of a face: the face name and shortened names
/// (CR 201.5, 201.5c), `~`, and the configured other names.
fn name_patterns(face_name: &str, card: &CardTextConfig) -> Vec<NamePattern> {
    let mut out = BTreeSet::new();
    let own = normalize::normalize(face_name);
    for n in std::iter::once(own.trim())
        .chain(card.short_names.iter().map(String::as_str))
        .chain(std::iter::once("~"))
    {
        if !n.is_empty() {
            out.insert(NamePattern {
                text: n.to_string(),
                other: None,
            });
        }
    }
    for n in &card.other_names {
        if !n.is_empty() {
            out.insert(NamePattern {
                text: n.clone(),
                other: Some(n.clone()),
            });
        }
    }
    out.into_iter().collect()
}

/// True when the first section header line of the face is a class level header.
fn starts_with_class(lines: &[Line]) -> bool {
    for l in lines {
        if lines::station(&l.text).is_some() || lines::level_band(&l.text).is_some() {
            return false;
        }
        if lines::class_level(&l.text).is_some() {
            return true;
        }
    }
    false
}

/// Tokenizes one face (REQ-ING-004, REQ-DSL-012): normalize, strip, split, mark. Total and
/// deterministic (R3, R12).
#[must_use]
pub fn tokenize_face(
    input: &TokenizeInput<'_>,
    vocab: &RulesVocabulary,
    config: &TokenizerConfig,
) -> TokenizedFace {
    let (raw, truncated) = truncate(input.text);
    let display = normalize::normalize(raw);
    let empty = CardTextConfig::default();
    let card = config.cards.get(input.card_key).unwrap_or(&empty);

    // CR 207.2a: reminder text, which may stand on its own line.
    let lines: Vec<Line> = display
        .split('\n')
        .filter_map(|l| {
            let stripped = normalize::strip_reminder(l, &card.keep_parentheses);
            let t = stripped.trim();
            (!t.is_empty()).then(|| Line {
                original: l.to_string(),
                text: t.to_string(),
            })
        })
        .collect();

    let subtypes: Vec<String> = input
        .subtypes
        .iter()
        .map(|s| normalize::normalize(s))
        .collect();
    let lex = Lexicon::new(vocab, &subtypes);
    let names = name_patterns(input.face_name, card);
    let mut marker = Marker {
        lex: &lex,
        names: &names,
        counter: 0,
    };

    let mut sections: Vec<Section> = Vec::new();
    let mut current = None;
    if starts_with_class(&lines) {
        // CR 717.3: the section above the first class level bar is level 1.
        sections.push(Section {
            kind: SectionKind::ClassLevel {
                level: 1,
                cost: None,
            },
            header: None,
            pt: None,
        });
        current = Some(0);
    }
    let mut abilities = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        let t = line.text.as_str();
        let mut open = |kind: SectionKind| {
            sections.push(Section {
                kind,
                header: Some(line.original.clone()),
                pt: None,
            });
            sections.len() - 1
        };
        let body = if let Some((threshold, rest)) = lines::station(t) {
            // CR 722.2: a station symbol opens a striation.
            current = Some(open(SectionKind::Station { threshold }));
            rest
        } else if let Some((min, max)) = lines::level_band(t) {
            // CR 711.2: a level symbol opens a striation.
            current = Some(open(SectionKind::LevelBand { min, max }));
            ""
        } else if let Some((level, cost, rest)) = lines::class_level(t) {
            // CR 717.2a: a class level bar.
            current = Some(open(SectionKind::ClassLevel {
                level,
                cost: Some(cost.to_string()),
            }));
            rest
        } else {
            t
        };
        if let Some(s) = current.and_then(|s| sections.get_mut(s))
            && body == t
            && s.pt.is_none()
            && matches!(
                s.kind,
                SectionKind::Station { .. } | SectionKind::LevelBand { .. }
            )
            && lines::is_pt_line(t)
        {
            // CR 711.2, 722.2: the striation's power/toughness box, kept as text.
            s.pt = Some(t.to_string());
            i += 1;
            continue;
        }
        if body.is_empty() {
            i += 1;
            continue;
        }
        i = classify(body, &lines, i, current, &mut marker, &mut abilities);
    }

    TokenizedFace {
        display,
        truncated,
        abilities,
        sections,
    }
}

/// Classifies the body of line `i` and pushes its ability; returns the next line index.
fn classify(
    body: &str,
    lines: &[Line],
    i: usize,
    section: Option<usize>,
    marker: &mut Marker<'_>,
    out: &mut Vec<TokenizedAbility>,
) -> usize {
    let original = lines[i].original.as_str();
    // CR 715.2b, 715.2c: chapter numerals.
    if let Some((chapters, effect)) = lines::chapter(body) {
        out.push(TokenizedAbility {
            original: original.to_string(),
            kind: AbilityKind::Chapter { chapters },
            anchor: None,
            section,
            body: marker.piece(original, effect),
        });
        return i + 1;
    }
    // CR 207.2c, 207.2d: an anchor word has no rules meaning; it is kept as text.
    let (anchor, rest) = match lines::anchor(body) {
        Some((a, r)) => (Some(a.to_string()), r),
        None => (None, body),
    };
    // CR 700.2, 700.2i: a "choose" header followed by option lines.
    if lines::is_modal_header(rest) {
        let bullets_ok = lines::ends_with_dash(rest);
        let mut j = i + 1;
        while j < lines.len() && lines::option_text(&lines[j].text, bullets_ok).is_some() {
            j += 1;
        }
        if j > i + 1 {
            let header = marker.piece(original, rest);
            let modes = lines[i + 1..j]
                .iter()
                .filter_map(|l| {
                    lines::option_text(&l.text, bullets_ok).map(|m| (l.original.as_str(), m))
                })
                .map(|(o, m)| marker.piece(o, m))
                .collect();
            let joined: Vec<&str> = lines[i..j].iter().map(|l| l.original.as_str()).collect();
            out.push(TokenizedAbility {
                original: joined.join("\n"),
                kind: AbilityKind::Modal { modes },
                anchor,
                section,
                body: header,
            });
            return j;
        }
    }
    let kind = if anchor.is_some() {
        AbilityKind::Anchored
    } else {
        AbilityKind::Plain
    };
    out.push(TokenizedAbility {
        original: original.to_string(),
        kind,
        anchor,
        section,
        body: marker.piece(original, rest),
    });
    i + 1
}
