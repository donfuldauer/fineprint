// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Step 4 of the tokenizer: self-reference and rewritable-word marking (REQ-ING-004(b)/(c),
//! REQ-DSL-012).

use std::collections::BTreeMap;

use fineprint_dsl::text::{NameRefKind, RewritableCategory, TextToken};
use fineprint_dsl::vocab::RulesVocabulary;

use super::normalize::normalize;

/// The CR 105.1 color words.
const COLOR_WORDS: [&str; 5] = ["white", "blue", "black", "red", "green"];

/// The CR 305.6 basic land types.
const BASIC_LAND_TYPES: [&str; 5] = ["Plains", "Island", "Swamp", "Mountain", "Forest"];

/// Nouns that, after "this", refer to the object itself besides the card types and the face's
/// own subtypes (SRS §14.5 convention, not CR text; decision "#89 FP-087 — Self-reference
/// phrase set").
pub(super) const SELF_NOUNS: [&str; 4] = ["spell", "permanent", "card", "token"];

/// True for characters that continue a word: letters, digits and `_`.
pub(super) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// What a lexicon phrase marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    /// A "this <noun>" self-reference.
    SelfRef,
    /// A rewritable word of this category.
    Word(RewritableCategory),
}

/// The case-insensitive phrases of one face: color words, basic land types, creature types and
/// the "this <noun>" self-references, keyed by their lower-case form.
pub(super) struct Lexicon {
    /// Lower-case phrase → entry.
    map: BTreeMap<String, Entry>,
    /// The largest phrase length in characters.
    max_chars: usize,
}

impl Lexicon {
    /// Builds the lexicon from the vocabulary and the face's own subtypes. Later inserts win, so
    /// the priority on a collision is self-reference, color word, basic land type, creature type.
    pub(super) fn new(vocab: &RulesVocabulary, own_subtypes: &[String]) -> Lexicon {
        let mut map = BTreeMap::new();
        // CR 612.2: creature-type words used as creature types are rewritable.
        for t in vocab.subtypes_of("Creature") {
            insert(&mut map, t, Entry::Word(RewritableCategory::CreatureType));
        }
        // CR 305.6: the basic land types.
        for t in BASIC_LAND_TYPES {
            insert(&mut map, t, Entry::Word(RewritableCategory::BasicLandType));
        }
        // CR 105.1: the five colors.
        for t in COLOR_WORDS {
            insert(&mut map, t, Entry::Word(RewritableCategory::ColorWord));
        }
        for t in vocab
            .card_types
            .iter()
            .map(String::as_str)
            .chain(own_subtypes.iter().map(String::as_str))
            .chain(SELF_NOUNS)
        {
            if !t.trim().is_empty() {
                insert(&mut map, &format!("this {}", t.trim()), Entry::SelfRef);
            }
        }
        let max_chars = map.keys().map(|k| k.chars().count()).max().unwrap_or(0);
        Lexicon { map, max_chars }
    }
}

/// Inserts the normalized, lower-cased form of `phrase`.
fn insert(map: &mut BTreeMap<String, Entry>, phrase: &str, entry: Entry) {
    let key = normalize(phrase).to_lowercase();
    if !key.is_empty() {
        map.insert(key, entry);
    }
}

/// One marked span.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Mark {
    /// `NameRef{SelfRef}`.
    SelfRef,
    /// `NameRef{Other}` with this name.
    Other(String),
    /// A rewritable word.
    Word(RewritableCategory),
}

/// A case-sensitive name pattern: the face name, a shortened name (CR 201.5, 201.5c), `~`, or a
/// configured other name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NamePattern {
    /// The exact text to find.
    pub(super) text: String,
    /// `None` for a self-reference, `Some(name)` for another name.
    pub(super) other: Option<String>,
}

/// The character before byte `i`, if any.
fn prev_char(text: &str, i: usize) -> Option<char> {
    text[..i].chars().next_back()
}

/// True when `text[start..end]` is not part of a longer word on either side.
fn whole_word(text: &str, start: usize, end: usize) -> bool {
    let inner = &text[start..end];
    let first_ok = match inner.chars().next() {
        Some(c) if is_word_char(c) => !prev_char(text, start).is_some_and(is_word_char),
        _ => true,
    };
    let last_ok = match inner.chars().next_back() {
        Some(c) if is_word_char(c) => !text[end..].chars().next().is_some_and(is_word_char),
        _ => true,
    };
    first_ok && last_ok
}

/// Finds the name matches: leftmost, then longest, then self before other; non-overlapping.
fn name_marks(text: &str, names: &[NamePattern]) -> Vec<(usize, usize, Mark)> {
    let mut all = Vec::new();
    for p in names {
        if p.text.is_empty() {
            continue;
        }
        for (i, _) in text.match_indices(p.text.as_str()) {
            let end = i + p.text.len();
            if whole_word(text, i, end) {
                let mark = p.other.clone().map_or(Mark::SelfRef, Mark::Other);
                all.push((i, end, mark));
            }
        }
    }
    all.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.cmp(&a.1))
            .then(matches!(b.2, Mark::SelfRef).cmp(&matches!(a.2, Mark::SelfRef)))
            .then(a.2.cmp_key().cmp(&b.2.cmp_key()))
    });
    let mut out: Vec<(usize, usize, Mark)> = Vec::new();
    let mut pos = 0;
    for m in all {
        if m.0 >= pos {
            pos = m.1;
            out.push(m);
        }
    }
    out
}

impl Mark {
    /// A total-order key for ties (deterministic, R3).
    fn cmp_key(&self) -> (u8, &str) {
        match self {
            Mark::SelfRef => (0, ""),
            Mark::Other(n) => (1, n.as_str()),
            Mark::Word(c) => (2, c.as_str()),
        }
    }
}

/// Finds the lexicon matches in `text[a..b]`: at each word start, the longest phrase that ends
/// at a word boundary within the gap.
fn lexicon_marks(
    text: &str,
    a: usize,
    b: usize,
    lex: &Lexicon,
    out: &mut Vec<(usize, usize, Mark)>,
) {
    let mut pos = a;
    while pos < b {
        let Some(c) = text[pos..].chars().next() else {
            break;
        };
        let starts_word = is_word_char(c) && !prev_char(text, pos).is_some_and(is_word_char);
        if starts_word && let Some((end, entry)) = longest_at(text, pos, b, lex) {
            let mark = match entry {
                Entry::SelfRef => Mark::SelfRef,
                Entry::Word(cat) => Mark::Word(cat),
            };
            out.push((pos, end, mark));
            pos = end;
            continue;
        }
        pos += c.len_utf8();
    }
}

/// The longest lexicon phrase starting at `start` and ending at a word boundary at or before `b`.
fn longest_at(text: &str, start: usize, b: usize, lex: &Lexicon) -> Option<(usize, Entry)> {
    let mut ends = Vec::new();
    let mut prev_word = false;
    for (n, (off, c)) in text[start..b].char_indices().enumerate() {
        let j = start + off;
        if prev_word && !is_word_char(c) {
            ends.push(j);
        }
        if n >= lex.max_chars {
            break;
        }
        prev_word = is_word_char(c);
        let next = j + c.len_utf8();
        if next == b && prev_word && !text[b..].chars().next().is_some_and(is_word_char) {
            ends.push(b);
        }
    }
    ends.iter().rev().find_map(|&end| {
        let key = text[start..end].to_lowercase();
        lex.map.get(&key).map(|e| (end, *e))
    })
}

/// Marks `text` and returns its tokens and their byte spans. Ids continue from `counter` (`t1`,
/// `t2`, ... face-wide). Adjacent literal text forms one token; no token is empty.
pub(super) fn mark(
    text: &str,
    lex: &Lexicon,
    names: &[NamePattern],
    counter: &mut u64,
) -> (Vec<TextToken>, Vec<(usize, usize)>) {
    let named = name_marks(text, names);
    let mut marks = Vec::new();
    let mut gap = 0;
    for m in named {
        lexicon_marks(text, gap, m.0, lex, &mut marks);
        gap = m.1;
        marks.push(m);
    }
    lexicon_marks(text, gap, text.len(), lex, &mut marks);
    marks.sort_by_key(|m| m.0);

    let mut tokens = Vec::new();
    let mut spans = Vec::new();
    let mut next_id = || {
        *counter += 1;
        format!("t{counter}")
    };
    let mut pos = 0;
    for (s, e, mark) in marks {
        if s > pos {
            tokens.push(TextToken::Literal {
                id: next_id(),
                text: text[pos..s].to_string(),
            });
            spans.push((pos, s));
        }
        let id = next_id();
        tokens.push(match mark {
            Mark::SelfRef => TextToken::NameRef {
                id,
                kind: NameRefKind::SelfRef,
                name: None,
            },
            Mark::Other(name) => TextToken::NameRef {
                id,
                kind: NameRefKind::Other,
                name: Some(name),
            },
            Mark::Word(category) => TextToken::Word {
                id,
                category,
                value: text[s..e].to_string(),
            },
        });
        spans.push((s, e));
        pos = e;
    }
    if pos < text.len() {
        tokens.push(TextToken::Literal {
            id: next_id(),
            text: text[pos..].to_string(),
        });
        spans.push((pos, text.len()));
    }
    (tokens, spans)
}
