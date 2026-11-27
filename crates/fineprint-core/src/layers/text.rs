// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Text-changing effects (REQ-DSL-012, SRS §14.5; CR 612, 613.1c): the layer 3
//! `REPLACE_TEXT_WORD` rewrite of an object's text-token table, and the pure helpers that read
//! AST word references through that table.
//!
//! An object's rules text is a token stream ([`fineprint_dsl::text::TextTokenStream`]): literal
//! text, rewritable words and name references. A text-changing effect replaces every rewritable
//! word of one category and value by another value (CR 612.1, 612.2); literal text, name
//! references (`SELF` and `OTHER`, CR 201.5, 612.2) and card-name words are never rewritten, and
//! category `CARD_NAME` is not supported at all (the CR has no word-for-word replacement of card
//! names, CR 612.2). AST nodes reference rewritable words by token id ([`WordRef::TextToken`]),
//! so after the rewrite every node that references the token reads the new value
//! ([`resolve_word_ref`], [`resolve_predicate_words`], [`resolve_selector_words`]).
//!
//! `Characteristics.rules_text` is the real token stream (`fineprint_dsl::card::TextTokenStream`
//! is a re-export of it), so the layer-3 rewrite works on it directly; the former placeholder
//! bridge through MessagePack is gone (docs/DECISIONS.md "#232 FP-164 — Text placeholder bridge
//! removed").
//!
//! **Determinism.** Pure functions over their arguments; no float, no hash-ordered container,
//! no I/O (REQ-CORE-001).

use std::fmt;

use fineprint_dsl::expr::{
    HistoryQuery, ObjectSelectorAST, PlayerAttr, PredicateAST, SelectorFilter, ValueExpressionAST,
    WordRef,
};
use fineprint_dsl::text::{RewritableCategory, TextToken, TextTokenStream};

use super::error::LayerError;
use super::registry::LayerModification;

/// Why a text-token operation failed (REQ-DSL-012).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextWordError {
    /// No token of the stream has this id.
    UnknownToken(String),
    /// The token with this id exists but is literal text or a name reference, not a word.
    NotAWord(String),
    /// A replacement's `from` or `to` word is empty or contains a Unicode noncharacter.
    InvalidWord,
    /// A replacement of category `CARD_NAME`: the CR has no word-for-word replacement of card
    /// names (CR 612.2).
    UnsupportedCategory,
}

impl fmt::Display for TextWordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextWordError::UnknownToken(id) => write!(f, "no text token has id {id:?}"),
            TextWordError::NotAWord(id) => {
                write!(f, "text token {id:?} is not a rewritable word")
            }
            TextWordError::InvalidWord => {
                write!(f, "a replacement word is empty or contains a noncharacter")
            }
            TextWordError::UnsupportedCategory => write!(
                f,
                "card-name words cannot be replaced by a text-changing effect (CR 612.2)"
            ),
        }
    }
}

impl std::error::Error for TextWordError {}

/// Whether `c` is a Unicode noncharacter (U+FDD0-U+FDEF and the last two code points of every
/// plane), which canonical JSON refuses.
fn is_noncharacter(c: char) -> bool {
    let n = u32::from(c);
    (0xFDD0..=0xFDEF).contains(&n) || (n & 0xFFFE) == 0xFFFE
}

/// Whether `word` is a valid replacement word: non-empty and free of noncharacters.
fn valid_word(word: &str) -> bool {
    !word.is_empty() && !word.chars().any(is_noncharacter)
}

/// Checks the arguments of a replacement: category first, then the two words.
fn check_replacement(
    category: RewritableCategory,
    from: &str,
    to: &str,
) -> Result<(), TextWordError> {
    // CR 612.2: no text-changing effect replaces card-name words word for word.
    if category == RewritableCategory::CardName {
        return Err(TextWordError::UnsupportedCategory);
    }
    if !valid_word(from) || !valid_word(to) {
        return Err(TextWordError::InvalidWord);
    }
    Ok(())
}

/// Replaces every word of `category` whose value is exactly `from` (case-sensitive) by `to`
/// (REQ-DSL-012; CR 612.1, 612.2). Pure.
///
/// CR 612.2: only words used in their category's sense change, so literal text, name
/// references (`SELF`, CR 201.5, and `OTHER`), card-name words and words of other categories or
/// values are untouched. Token order, ids and kinds are kept. `from == to` changes nothing.
///
/// # Errors
/// Checked in this order: [`TextWordError::UnsupportedCategory`] for category `CARD_NAME`
/// (CR 612.2); [`TextWordError::InvalidWord`] if `from` or `to` is empty or contains a Unicode
/// noncharacter.
pub fn rewrite_text_words(
    stream: &TextTokenStream,
    category: RewritableCategory,
    from: &str,
    to: &str,
) -> Result<TextTokenStream, TextWordError> {
    check_replacement(category, from, to)?;
    let tokens: Vec<TextToken> = stream
        .tokens()
        .iter()
        .map(|token| match token {
            // CR 612.2: a word used as a word of `category`.
            TextToken::Word {
                id,
                category: c,
                value,
            } if *c == category && value == from => TextToken::Word {
                id: id.clone(),
                category: *c,
                value: to.to_owned(),
            },
            other => other.clone(),
        })
        .collect();
    // Same ids as a valid stream and a valid `to`: the stream stays valid.
    TextTokenStream::new(tokens).map_err(|_| TextWordError::InvalidWord)
}

/// The word a [`WordRef`] names now (REQ-DSL-012, SRS §14.5): a fixed name as given; a token
/// reference as the current value of that word token of `stream`, whatever its category
/// (`CARD_NAME` words are readable too).
///
/// # Errors
/// [`TextWordError::UnknownToken`] if no token has the id; [`TextWordError::NotAWord`] if the
/// token is literal text or a name reference.
pub fn resolve_word_ref(stream: &TextTokenStream, word: &WordRef) -> Result<String, TextWordError> {
    match word {
        WordRef::Name(name) => Ok(name.clone()),
        WordRef::TextToken(id) => token_value(stream, id).map(str::to_owned),
    }
}

/// The current value of the word token `id`.
fn token_value<'a>(stream: &'a TextTokenStream, id: &str) -> Result<&'a str, TextWordError> {
    match stream.find(id) {
        None => Err(TextWordError::UnknownToken(id.to_owned())),
        Some(TextToken::Word { value, .. }) => Ok(value),
        // CR 201.5: name references are not words; literal text is never rewritten.
        Some(_) => Err(TextWordError::NotAWord(id.to_owned())),
    }
}

/// The `TEXT_TOKEN_MATCH` test (REQ-DSL-012, SRS §14.5): whether the word token `token_id` of
/// `stream` currently has the value `value`.
///
/// # Errors
/// As [`resolve_word_ref`].
pub fn text_token_matches(
    stream: &TextTokenStream,
    token_id: &str,
    value: &str,
) -> Result<bool, TextWordError> {
    token_value(stream, token_id).map(|v| v == value)
}

/// A clone of `pred` in which every [`WordRef::TextToken`] reachable through the boolean
/// operators, the `HAS_CARD_TYPE`/`HAS_SUBTYPE`/`HAS_SUPERTYPE`/`HAS_COLOR` leaves, `CONTROLS`
/// selectors, selector predicates and the value expressions reached from those nodes
/// (devotion, mana colour, counts, history conditions; [`resolve_value_words`]) is replaced by [`WordRef::Name`] of its
/// current value in `stream` (REQ-DSL-012, SRS §14.5). [`WordRef::Name`], `TEXT_TOKEN_MATCH`
/// and references (including `{"ref":"SELF"}`, CR 201.5) are unchanged. An `Ok` result holds no
/// [`WordRef::TextToken`].
///
/// # Errors
/// The first unresolvable token, walking children in declaration order: as
/// [`resolve_word_ref`].
pub fn resolve_predicate_words(
    pred: &PredicateAST,
    stream: &TextTokenStream,
) -> Result<PredicateAST, TextWordError> {
    let word = |w: &WordRef| resolve_word_ref(stream, w).map(WordRef::Name);
    let value = |v: &ValueExpressionAST| resolve_value_words(v, stream).map(Box::new);
    Ok(match pred {
        PredicateAST::And(all) => PredicateAST::And(resolve_all(all, stream)?),
        PredicateAST::Or(any) => PredicateAST::Or(resolve_all(any, stream)?),
        PredicateAST::Not(inner) => {
            PredicateAST::Not(Box::new(resolve_predicate_words(inner, stream)?))
        }
        PredicateAST::HasCardType { value: w } => PredicateAST::HasCardType { value: word(w)? },
        PredicateAST::HasSubtype { value: w } => PredicateAST::HasSubtype { value: word(w)? },
        PredicateAST::HasSupertype { value: w } => PredicateAST::HasSupertype { value: word(w)? },
        PredicateAST::HasColor { value: w } => PredicateAST::HasColor { value: word(w)? },
        PredicateAST::HasCounter { kind, compare } => PredicateAST::HasCounter {
            kind: kind.clone(),
            compare: match compare {
                Some((cmp, v)) => Some((*cmp, value(v)?)),
                None => None,
            },
        },
        PredicateAST::ManaValue { cmp, value: v } => PredicateAST::ManaValue {
            cmp: *cmp,
            value: value(v)?,
        },
        PredicateAST::Power { cmp, value: v } => PredicateAST::Power {
            cmp: *cmp,
            value: value(v)?,
        },
        PredicateAST::Toughness { cmp, value: v } => PredicateAST::Toughness {
            cmp: *cmp,
            value: value(v)?,
        },
        PredicateAST::Compare { cmp, lhs, rhs } => PredicateAST::Compare {
            cmp: *cmp,
            lhs: value(lhs)?,
            rhs: value(rhs)?,
        },
        PredicateAST::Controls { selector, player } => PredicateAST::Controls {
            selector: Box::new(resolve_selector_words(selector, stream)?),
            player: player.clone(),
        },
        // Leaves without word references, TEXT_TOKEN_MATCH (read by `text_token_matches`),
        // SAME_AS references and player selectors (which carry no predicates).
        other => other.clone(),
    })
}

/// [`resolve_predicate_words`] on each predicate, in order.
fn resolve_all(
    preds: &[PredicateAST],
    stream: &TextTokenStream,
) -> Result<Vec<PredicateAST>, TextWordError> {
    preds
        .iter()
        .map(|p| resolve_predicate_words(p, stream))
        .collect()
}

/// A clone of `sel` with every [`WordRef::TextToken`] of its count and predicate replaced by
/// the current word, exactly as [`resolve_predicate_words`] (REQ-DSL-012). A reference selector
/// (including `{"ref":"SELF"}`, CR 201.5) is unchanged. An `Ok` result holds no
/// [`WordRef::TextToken`].
///
/// # Errors
/// The first unresolvable token (the count, then the predicate): as [`resolve_word_ref`].
pub fn resolve_selector_words(
    sel: &ObjectSelectorAST,
    stream: &TextTokenStream,
) -> Result<ObjectSelectorAST, TextWordError> {
    match sel {
        ObjectSelectorAST::Ref(r) => Ok(ObjectSelectorAST::Ref(r.clone())),
        ObjectSelectorAST::Filter(f) => Ok(ObjectSelectorAST::Filter(SelectorFilter {
            zones: f.zones.clone(),
            count: match &f.count {
                Some(v) => Some(Box::new(resolve_value_words(v, stream)?)),
                None => None,
            },
            ordering: f.ordering,
            predicate: match &f.predicate {
                Some(p) => Some(Box::new(resolve_predicate_words(p, stream)?)),
                None => None,
            },
        })),
    }
}

/// A clone of the value expression `v` in which every [`WordRef::TextToken`] reachable from it
/// (devotion colours, the mana colour of a history query, counts, and the selectors and
/// predicates it contains, walked as [`resolve_selector_words`] and [`resolve_predicate_words`]
/// do) is replaced by [`WordRef::Name`] of its current value in `stream` (REQ-DSL-012, SRS
/// §14.5). [`WordRef::Name`] and references (including `{"ref":"SELF"}`, CR 201.5) are
/// unchanged. An `Ok` result holds no [`WordRef::TextToken`]. This is the resolver for word
/// references reachable only from a value expression, outside any predicate or selector.
///
/// # Errors
/// The first unresolvable token, walking children in declaration order: as
/// [`resolve_word_ref`].
pub fn resolve_value_words(
    v: &ValueExpressionAST,
    stream: &TextTokenStream,
) -> Result<ValueExpressionAST, TextWordError> {
    let word = |w: &WordRef| resolve_word_ref(stream, w).map(WordRef::Name);
    let pred = |p: &Option<PredicateAST>| match p {
        Some(p) => resolve_predicate_words(p, stream).map(Some),
        None => Ok(None),
    };
    Ok(match v {
        ValueExpressionAST::Count(sel) => {
            ValueExpressionAST::Count(Box::new(resolve_selector_words(sel, stream)?))
        }
        ValueExpressionAST::PlayerAttr {
            reference,
            attr: PlayerAttr::Devotion(words),
        } => ValueExpressionAST::PlayerAttr {
            reference: reference.clone(),
            attr: PlayerAttr::Devotion(words.iter().map(word).collect::<Result<_, _>>()?),
        },
        ValueExpressionAST::Math { op, lhs, rhs } => ValueExpressionAST::Math {
            op: *op,
            lhs: Box::new(resolve_value_words(lhs, stream)?),
            rhs: Box::new(resolve_value_words(rhs, stream)?),
        },
        ValueExpressionAST::Distinct { attr, selector } => ValueExpressionAST::Distinct {
            attr: *attr,
            selector: Box::new(resolve_selector_words(selector, stream)?),
        },
        ValueExpressionAST::History(q) => ValueExpressionAST::History(Box::new(match &**q {
            HistoryQuery::SpellsCastThisTurn { player, condition } => {
                HistoryQuery::SpellsCastThisTurn {
                    player: player.clone(),
                    condition: pred(condition)?,
                }
            }
            HistoryQuery::CreaturesDiedThisTurn { condition } => {
                HistoryQuery::CreaturesDiedThisTurn {
                    condition: pred(condition)?,
                }
            }
            HistoryQuery::ManaSpentToCast { reference, color } => HistoryQuery::ManaSpentToCast {
                reference: reference.clone(),
                color: match color {
                    Some(w) => Some(word(w)?),
                    None => None,
                },
            },
            other => other.clone(),
        })),
        other => other.clone(),
    })
}

/// Registration check of a modification (REQ-DSL-012; CR 612.2): a `ReplaceTextWord` with
/// category `CARD_NAME`, or an empty or noncharacter word, is [`LayerError::Unsupported`]; every
/// other modification passes.
pub(super) fn validate_modification(m: &LayerModification) -> Result<(), LayerError> {
    let LayerModification::ReplaceTextWord { category, from, to } = m else {
        return Ok(());
    };
    check_replacement(*category, from, to).map_err(|e| match e {
        TextWordError::UnsupportedCategory => {
            LayerError::Unsupported("REPLACE_TEXT_WORD of category CARD_NAME (CR 612.2)")
        }
        _ => LayerError::Unsupported("REPLACE_TEXT_WORD with an empty or noncharacter word"),
    })
}

/// Layer 3 application of `REPLACE_TEXT_WORD` to an object's rules text (CR 612.1, 612.2,
/// 613.1c): `Some(new text)` when at least one word changed; `None` (leave the text unchanged)
/// when nothing changed, the text is empty or the replacement is invalid. Never panics (R12).
pub(super) fn replace_in_rules_text(
    rules_text: &TextTokenStream,
    category: RewritableCategory,
    from: &str,
    to: &str,
) -> Option<TextTokenStream> {
    let rewritten = rewrite_text_words(rules_text, category, from, to).ok()?;
    (rewritten != *rules_text).then_some(rewritten)
}

/// The tokens a `REPLACE_TEXT_WORD` of `category` and `from` applies to in `rules_text`
/// (REQ-LAYER-002, REQ-DSL-012; CR 613.8a, 612.2): the `(position, token id)` of every word
/// token whose category is `category` and whose value is exactly `from`, in stream order.
///
/// CR 612.2: only words used in that category's sense count, so literal text, name references
/// (`SELF`, `OTHER`), card-name words and words of other categories or values are never in the
/// set. Category `CARD_NAME` (not supported by the effect, CR 612.2) and empty text give the
/// empty set; never panics (R12). Reads only token data, never a card
/// name or id (REQ-DSL-013). The dependency trial compares this set before and after another
/// effect (docs/DECISIONS.md "#37 FP-036 — Dependency between text effects").
pub(super) fn replace_targets(
    rules_text: &TextTokenStream,
    category: RewritableCategory,
    from: &str,
) -> Vec<(usize, String)> {
    if category == RewritableCategory::CardName {
        return Vec::new();
    }
    rules_text
        .tokens()
        .iter()
        .enumerate()
        .filter_map(|(position, token)| match token {
            // CR 612.2: a word used as a word of `category`.
            TextToken::Word {
                id,
                category: c,
                value,
            } if *c == category && value == from => Some((position, id.clone())),
            _ => None,
        })
        .collect()
}
