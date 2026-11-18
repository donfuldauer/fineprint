// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The predefined-token table (CR 111.10; REQ-SUB-001).
//!
//! Every lettered sub-rule of CR 111.10 defines one token; its letter is the rule's letter
//! suffix (exactly one lower-case letter). The rule's sentences (quote-aware, see
//! [`super::cr::sentences`]) are read as follows:
//!
//! - The first sentence is the framing `A|An <Name> token is a|an <description>`.
//! - When the framing description is exactly `double-faced token`, the token is double-faced
//!   and needs exactly two later sentences `Its front face is a|an <face>` and
//!   `Its back face is a|an <face>`, in that order (front, then back). A missing, repeated or
//!   reversed face sentence or any other label is an input error.
//!   Otherwise the framing description is the single face, and any `Its <label> face is ...`
//!   sentence is an input error.
//! - Sentences starting `See ` are cross-references and are ignored. Any other sentence is an
//!   input error (never silently dropped).
//!
//! A face description is `[P/T] <colors> <supertypes/subtypes/card types> [token]
//! [named <Name>] [with <abilities>]`. Colors are `colorless` or color words joined by `and`
//! (stored in white, blue, black, red, green order). Type words are matched against the
//! vocabulary's card types and supertypes (case-insensitively) and against the subtypes of the
//! face's card types (longest match first, so two-word subtypes stay whole); an unknown word is
//! an input error. `abilities` lists, in order, every quoted text without its quotation marks
//! (and without a trailing `,` the quoting style puts inside the marks) and every unquoted
//! phrase between `,` and `and` separators. An unquoted phrase must be a keyword ability
//! phrase: a CR 702 keyword name (case-insensitive), alone or followed by one parameter (a card
//! type as in `enchant creature`, a number or `X`, or a `{...}` mana cost); anything else is an
//! input error.
//!
//! The token's name is its first face's name; a face's name is its `named <Name>` if present,
//! else the framing `<Name>`.

use fineprint_dsl::vocab::{COLOR_WORDS, PredefinedToken, RulesVocabulary, TokenFace};

use super::cr::{CrDoc, Rule, sentences};
use super::{GenError, input};

/// Parses every CR 111.10 sub-rule; `vocab` supplies the card types, supertypes and subtypes.
///
/// # Errors
/// [`GenError::Input`] when no token rule exists or one does not parse.
pub fn predefined_tokens(
    doc: &CrDoc,
    vocab: &RulesVocabulary,
) -> Result<Vec<PredefinedToken>, GenError> {
    let rules = doc.children("111.10")?;
    if rules.is_empty() {
        return Err(input(
            "missing required rules 111.10a..: no predefined tokens in the CR text",
        ));
    }
    rules.into_iter().map(|r| token(r, vocab)).collect()
}

// CR 111.10: one predefined token per lettered sub-rule.
fn token(rule: &Rule, vocab: &RulesVocabulary) -> Result<PredefinedToken, GenError> {
    let id = rule.id();
    let err = |what: &str| input(format!("rule {id}: {what}"));
    if rule.letters.len() != 1 {
        return Err(err("a predefined-token rule must have a one-letter suffix"));
    }
    let sents = sentences(&rule.text).ok_or_else(|| err("unbalanced quotation marks"))?;
    let (framing, rest) = sents.split_first().ok_or_else(|| err("empty rule"))?;
    let after_article = framing
        .strip_prefix("A ")
        .or_else(|| framing.strip_prefix("An "))
        .ok_or_else(|| err("the first sentence is not `A|An <Name> token is ...`"))?;
    let (framing_name, description) = after_article
        .split_once(" token is ")
        .ok_or_else(|| err("the first sentence is not `A|An <Name> token is ...`"))?;
    let description =
        strip_article(description).ok_or_else(|| err("the token description has no article"))?;
    // CR 111.10i (the double-faced shape): the framing declares the layout; a double-faced
    // token has exactly one `Its front face is ...` and one `Its back face is ...` sentence, in
    // that order. A single-faced token has no face sentence.
    let double_faced = description == DOUBLE_FACED;
    let (mut front, mut back) = (None, None);
    for s in rest {
        if s.starts_with("See ") {
            continue;
        }
        let (label, face) = s
            .strip_prefix("Its ")
            .and_then(|x| x.split_once(" face is "))
            .and_then(|(l, d)| Some((l, strip_article(d)?)))
            .ok_or_else(|| err("unrecognised sentence in the token definition"))?;
        if !double_faced {
            return Err(err(
                "a face sentence in a token that is not declared double-faced",
            ));
        }
        let slot = match label {
            "front" if back.is_some() => {
                return Err(err(
                    "the back face sentence precedes the front face sentence",
                ));
            }
            "front" => &mut front,
            "back" => &mut back,
            _ => return Err(err("a face label other than `front` or `back`")),
        };
        if slot.replace(face).is_some() {
            return Err(err("a face label repeats"));
        }
    }
    let face_texts = if double_faced {
        match (front, back) {
            (Some(f), Some(b)) => vec![f, b],
            _ => {
                return Err(err(
                    "a double-faced token needs one front and one back face sentence",
                ));
            }
        }
    } else {
        vec![description]
    };
    let faces = face_texts
        .into_iter()
        .map(|d| face(d, framing_name, vocab).map_err(|m| err(&m)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PredefinedToken {
        letter: rule.letters.clone(),
        rule_ref: id.clone(),
        name: faces[0].name.clone(),
        faces,
    })
}

/// The framing description of a double-faced token (`An X token is a double-faced token.`).
const DOUBLE_FACED: &str = "double-faced token";

fn strip_article(s: &str) -> Option<&str> {
    s.strip_prefix("a ").or_else(|| s.strip_prefix("an "))
}

/// Splits `s` at the first ` <word> ` outside quotation marks.
fn split_top_level<'a>(s: &'a str, word: &str) -> Option<(&'a str, &'a str)> {
    let needle = format!(" {word} ");
    let mut depth = 0usize;
    let mut straight = false;
    for (i, ch) in s.char_indices() {
        match ch {
            '\u{201c}' => depth += 1,
            '\u{201d}' => depth = depth.saturating_sub(1),
            '"' => straight = !straight,
            ' ' if depth == 0 && !straight && s[i..].starts_with(&needle) => {
                return Some((&s[..i], &s[i + needle.len()..]));
            }
            _ => {}
        }
    }
    None
}

fn face(desc: &str, framing_name: &str, vocab: &RulesVocabulary) -> Result<TokenFace, String> {
    let (head, abilities) = match split_top_level(desc, "with") {
        Some((h, a)) => (h, abilities_of(a, vocab)?),
        None => (desc, Vec::new()),
    };
    let (types_part, name) = match split_top_level(head, "named") {
        Some((t, n)) => (t, n.trim().to_owned()),
        None => (head, framing_name.trim().to_owned()),
    };
    if name.is_empty() || !super::cr::looks_like_name(&name) {
        return Err("the token name does not parse".to_owned());
    }
    let mut words: Vec<&str> = types_part.split_whitespace().collect();
    if words.last() == Some(&"token") {
        words.pop();
    }
    let mut i = 0;
    let (mut power, mut toughness) = (None, None);
    if let Some((p, t)) = words.first().and_then(|w| w.split_once('/')) {
        power = Some(small_int(p).ok_or("unparseable power")?);
        toughness = Some(small_int(t).ok_or("unparseable toughness")?);
        i = 1;
    }
    let mut colorless = false;
    let mut colors = Vec::new();
    let mut saw_color_word = false;
    while let Some(w) = words.get(i) {
        match *w {
            "colorless" => colorless = true,
            "and" if saw_color_word => {}
            c if COLOR_WORDS.contains(&c) => colors.push(c),
            _ => break,
        }
        saw_color_word = true;
        i += 1;
    }
    if !saw_color_word || (colorless && !colors.is_empty()) {
        return Err("the token's color does not parse".to_owned());
    }
    let colors: Vec<String> = COLOR_WORDS
        .iter()
        .filter(|c| colors.contains(c))
        .map(|c| (*c).to_owned())
        .collect();
    let mut supertypes = Vec::new();
    let mut card_types = Vec::new();
    let mut subtype_words = Vec::new();
    for w in &words[i..] {
        if let Some(ct) = vocab.card_types.iter().find(|c| c.eq_ignore_ascii_case(w)) {
            card_types.push(ct.clone());
        } else if let Some(st) = vocab.supertypes.iter().find(|s| s.eq_ignore_ascii_case(w)) {
            supertypes.push(st.clone());
        } else if card_types.is_empty() {
            subtype_words.push(*w);
        } else {
            return Err("a type word follows the card types".to_owned());
        }
    }
    if card_types.is_empty() {
        return Err("the token has no card type".to_owned());
    }
    let subtypes = match_subtypes(&subtype_words, &card_types, vocab)?;
    Ok(TokenFace {
        name,
        colors,
        supertypes,
        card_types,
        subtypes,
        power,
        toughness,
        abilities,
    })
}

fn small_int(s: &str) -> Option<i64> {
    if s.is_empty() || s.len() > 6 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Greedy longest-match of `words` against the subtypes of `card_types`.
fn match_subtypes(
    words: &[&str],
    card_types: &[String],
    vocab: &RulesVocabulary,
) -> Result<Vec<String>, String> {
    let known: Vec<&str> = card_types
        .iter()
        .flat_map(|c| vocab.subtypes_of(c).collect::<Vec<_>>())
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let mut matched = None;
        for len in (1..=(words.len() - i).min(4)).rev() {
            let candidate = words[i..i + len].join(" ");
            if known.contains(&candidate.as_str()) {
                matched = Some((candidate, len));
                break;
            }
        }
        let (name, len) = matched.ok_or("a token type word is not a known subtype")?;
        if out.contains(&name) {
            return Err("a token subtype repeats".to_owned());
        }
        out.push(name);
        i += len;
    }
    Ok(out)
}

/// The ordered ability list of a `with ...` phrase. Quoted texts are kept as written; every
/// unquoted fragment must be a keyword phrase ([`is_keyword_phrase`]).
fn abilities_of(s: &str, vocab: &RulesVocabulary) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut quoted = String::new();
    let mut depth = 0usize;
    let mut straight = false;
    let flush_plain = |plain: &mut String, out: &mut Vec<String>| -> Result<(), String> {
        for piece in plain.split(',') {
            for part in split_and(piece) {
                let part = part.trim().trim_end_matches('.').trim();
                if part.is_empty() {
                    continue;
                }
                if !is_keyword_phrase(part, vocab) {
                    return Err(format!(
                        "the unquoted ability `{part}` is not a keyword ability phrase"
                    ));
                }
                out.push(part.to_owned());
            }
        }
        plain.clear();
        Ok(())
    };
    let flush_quoted = |quoted: &mut String, out: &mut Vec<String>| -> Result<(), String> {
        let text = quoted.trim().trim_end_matches(',').trim();
        if text.is_empty() {
            return Err("an empty quoted ability".to_owned());
        }
        out.push(text.to_owned());
        quoted.clear();
        Ok(())
    };
    for ch in s.chars() {
        match ch {
            '\u{201c}' if !straight => {
                if depth == 0 {
                    flush_plain(&mut plain, &mut out)?;
                } else {
                    quoted.push(ch);
                }
                depth += 1;
            }
            '\u{201d}' if !straight => {
                depth = depth
                    .checked_sub(1)
                    .ok_or("unbalanced quotation marks in abilities")?;
                if depth == 0 {
                    flush_quoted(&mut quoted, &mut out)?;
                } else {
                    quoted.push(ch);
                }
            }
            '"' if depth == 0 => {
                if straight {
                    flush_quoted(&mut quoted, &mut out)?;
                } else {
                    flush_plain(&mut plain, &mut out)?;
                }
                straight = !straight;
            }
            _ if depth > 0 || straight => quoted.push(ch),
            _ => plain.push(ch),
        }
    }
    if depth != 0 || straight {
        return Err("unbalanced quotation marks in abilities".to_owned());
    }
    flush_plain(&mut plain, &mut out)?;
    Ok(out)
}

// CR 702: an unquoted token ability is a keyword ability. The grammar (#15 FP-014 CR round 1,
// docs/DECISIONS.md) accepts, case-insensitively, a keyword-ability name from the vocabulary
// (the CR 702 headings) alone, or followed by one space and one parameter: a card type
// (`enchant creature`), a number or `X`, or a mana cost made of `{...}` symbols.
fn is_keyword_phrase(fragment: &str, vocab: &RulesVocabulary) -> bool {
    let lower = fragment.to_lowercase();
    vocab.keyword_abilities.iter().any(|k| {
        let name = k.name.to_lowercase();
        if lower == name {
            return true;
        }
        lower
            .strip_prefix(name.as_str())
            .and_then(|r| r.strip_prefix(' '))
            .is_some_and(|p| is_keyword_parameter(p, vocab))
    })
}

/// A keyword parameter: a card type, a number (at most 6 digits) or `x`, or a mana cost
/// (`{1}{w}`: one or more brace groups of ASCII letters, digits and `/`). `p` is lower-case.
fn is_keyword_parameter(p: &str, vocab: &RulesVocabulary) -> bool {
    if vocab.card_types.iter().any(|c| c.to_lowercase() == p) {
        return true;
    }
    if p == "x" || small_int(p).is_some() {
        return true;
    }
    let Some(inner) = p.strip_prefix('{').and_then(|r| r.strip_suffix('}')) else {
        return false;
    };
    inner
        .split("}{")
        .all(|sym| !sym.is_empty() && sym.chars().all(|c| c.is_ascii_alphanumeric() || c == '/'))
}

/// Splits on the word `and`.
fn split_and(s: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    for w in s.split_whitespace() {
        if w == "and" {
            parts.push(String::new());
        } else if let Some(last) = parts.last_mut() {
            if !last.is_empty() {
                last.push(' ');
            }
            last.push_str(w);
        }
    }
    parts
}
