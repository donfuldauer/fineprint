// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the Oracle text tokenizer (REQ-ING-004, REQ-DSL-012). All text, names and
//! configuration keys are synthetic (R5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_dsl::card::ColorBitmask;
use fineprint_dsl::text::{NameRefKind, RewritableCategory as Cat, TextToken, TextTokenStream};
use fineprint_dsl::vocab::RulesVocabulary;

use super::*;
use crate::import::normalize::{NormalizedCardRecord, NormalizedFace};

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

fn vocab() -> &'static RulesVocabulary {
    RulesVocabulary::bundled().expect("bundled vocabulary")
}

/// Tokenizes `text` for face `test.face` (no subtypes, empty config) and checks the invariants.
fn tok(text: &str) -> TokenizedFace {
    tok_full(
        "test.key",
        "test.face",
        &[],
        text,
        &TokenizerConfig::default(),
    )
}

fn tok_full(
    key: &str,
    name: &str,
    subtypes: &[&str],
    text: &str,
    config: &TokenizerConfig,
) -> TokenizedFace {
    let subtypes: Vec<String> = subtypes.iter().map(|s| (*s).to_string()).collect();
    let input = TokenizeInput {
        card_key: key,
        face_name: name,
        subtypes: &subtypes,
        text,
    };
    let face = tokenize_face(&input, vocab(), config);
    check_face(&face);
    face
}

/// Every piece of a face in id order: each ability's body, then its modes.
fn pieces(face: &TokenizedFace) -> Vec<&TokenizedText> {
    let mut out = Vec::new();
    for a in &face.abilities {
        out.push(&a.body);
        if let AbilityKind::Modal { modes } = &a.kind {
            out.extend(modes.iter());
        }
    }
    out
}

/// The face-wide invariants (REQ-DSL-012, REQ-ING-004): ids `t1..tn` in text order and unique;
/// every stream re-validates; spans are contiguous and cover the text; the concatenated token
/// text reproduces the reminder-stripped text with SelfRef rendered as `SELF_REF_PLACEHOLDER`;
/// no empty token; no two adjacent literals; no NUMBER or CARD_NAME words; SelfRef carries no
/// name; section indexes are in range.
fn check_face(face: &TokenizedFace) {
    let mut n = 0u64;
    let mut seen = BTreeSet::new();
    for p in pieces(face) {
        let copy = TextTokenStream::new(p.tokens.tokens().to_vec());
        assert!(copy.is_ok(), "stream fails validation: {copy:?}");
        assert_eq!(p.tokens.len(), p.spans.len());
        assert!(!p.text.is_empty());
        let mut pos = 0;
        let mut expected = String::new();
        let mut prev_literal = false;
        for (t, &(s, e)) in p.tokens.tokens().iter().zip(&p.spans) {
            n += 1;
            assert_eq!(t.id(), format!("t{n}"));
            assert!(seen.insert(t.id().to_string()));
            assert_eq!(s, pos, "spans must be contiguous");
            assert!(e > s, "empty token");
            pos = e;
            let src = &p.text[s..e];
            match t {
                TextToken::Literal { text, .. } => {
                    assert_eq!(text, src);
                    assert!(!prev_literal, "adjacent literals must merge");
                    expected.push_str(src);
                }
                TextToken::Word {
                    category, value, ..
                } => {
                    assert_eq!(value, src);
                    assert!(!matches!(category, Cat::Number | Cat::CardName));
                    expected.push_str(src);
                }
                TextToken::NameRef {
                    kind: NameRefKind::SelfRef,
                    name,
                    ..
                } => {
                    assert!(name.is_none());
                    expected.push_str(SELF_REF_PLACEHOLDER);
                }
                TextToken::NameRef {
                    kind: NameRefKind::Other,
                    name,
                    ..
                } => {
                    assert_eq!(name.as_deref(), Some(src));
                    expected.push_str(src);
                }
            }
            prev_literal = matches!(t, TextToken::Literal { .. });
        }
        assert_eq!(pos, p.text.len(), "spans must cover the text");
        assert_eq!(render_tokens(&p.tokens, SELF_REF_PLACEHOLDER), expected);
    }
    for a in &face.abilities {
        if let Some(s) = a.section {
            assert!(s < face.sections.len());
        }
    }
}

fn words(p: &TokenizedText) -> Vec<(Cat, String)> {
    p.tokens
        .tokens()
        .iter()
        .filter_map(|t| match t {
            TextToken::Word {
                category, value, ..
            } => Some((*category, value.clone())),
            _ => None,
        })
        .collect()
}

fn self_count(p: &TokenizedText) -> usize {
    p.tokens
        .tokens()
        .iter()
        .filter(|t| {
            matches!(
                t,
                TextToken::NameRef {
                    kind: NameRefKind::SelfRef,
                    ..
                }
            )
        })
        .count()
}

fn texts(face: &TokenizedFace) -> Vec<&str> {
    face.abilities
        .iter()
        .map(|a| a.body.text.as_str())
        .collect()
}

fn sections_of(face: &TokenizedFace) -> Vec<(&str, Option<usize>)> {
    face.abilities
        .iter()
        .map(|a| (a.body.text.as_str(), a.section))
        .collect()
}

fn config_json(json: &str) -> TokenizerConfig {
    TokenizerConfig::from_json(json.as_bytes()).expect("valid config")
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

// ---------------------------------------------------------------------------------------------
// 1. Normalization
// ---------------------------------------------------------------------------------------------

/// REQ-ING-004(e): each typographic variant maps to its ASCII form; the em dash stays; a BOM is
/// stripped; CRLF and CR become LF; trailing spaces go; noncharacters become U+FFFD.
#[test]
fn normalize_variants() {
    let rows: [(&str, &str); 14] = [
        ("\u{2018}a\u{2019}", "'a'"),
        ("\u{201C}b\u{201D}", "\"b\""),
        ("1\u{2013}2", "1-2"),
        ("1\u{2012}2", "1-2"),
        ("\u{2212}1", "-1"),
        ("a\u{A0}b", "a b"),
        ("a\u{2009}b\u{202F}c", "a b c"),
        ("a\r\nb\rc", "a\nb\nc"),
        ("\u{FEFF}x", "x"),
        ("\u{FEFF}\u{FEFF}x", "x"),
        ("a \u{2014} b", "a \u{2014} b"),
        ("a  \nb \r\n", "a\nb\n"),
        ("x\u{FDD0}y\u{FFFF}", "x\u{FFFD}y\u{FFFD}"),
        ("a\tb", "a b"),
    ];
    for (input, want) in rows {
        assert_eq!(normalize_text(input), want, "input {input:?}");
    }
}

/// REQ-ING-004(e): normalizing twice equals normalizing once.
#[test]
fn normalize_idempotent() {
    let samples = [
        "\u{FEFF}\u{FEFF}A\u{2019}s \u{2013}1\r\n\u{A0} \r",
        "x \u{2014} y\u{FDD0}\r\r\n",
        "",
        "\u{2018}\u{201C}\u{2212}\u{2011}",
    ];
    for s in samples {
        let once = normalize_text(s);
        assert_eq!(normalize_text(&once), once);
        assert_eq!(normalize_text(s), once);
    }
}

/// REQ-ING-004(e), CR 711.2: the tokenizer normalizes first, so a curly apostrophe in the text
/// and the face name still match, and an en dash in a level band is accepted.
#[test]
fn normalize_feeds_tokenizer() {
    let f = tok_full(
        "test.key",
        "Test\u{2019}s Face",
        &[],
        "LEVEL 1\u{2013}3\r\nTest's Face does test thing.",
        &TokenizerConfig::default(),
    );
    assert_eq!(f.display, "LEVEL 1-3\nTest's Face does test thing.");
    assert_eq!(
        f.sections[0].kind,
        SectionKind::LevelBand {
            min: 1,
            max: Some(3)
        }
    );
    assert_eq!(self_count(&f.abilities[0].body), 1);
}

// ---------------------------------------------------------------------------------------------
// 2. Reminder text
// ---------------------------------------------------------------------------------------------

/// REQ-ING-004(a), CR 207.2a: an inline span is stripped and the original stays for display.
#[test]
fn reminder_inline_stripped_original_kept() {
    let f = tok("Flying (Test reminder about flying.)\nDo test thing.");
    assert_eq!(texts(&f), ["Flying", "Do test thing."]);
    assert_eq!(
        f.abilities[0].original,
        "Flying (Test reminder about flying.)"
    );
    assert_eq!(f.abilities[0].body.original, f.abilities[0].original);
    assert_eq!(
        f.display,
        "Flying (Test reminder about flying.)\nDo test thing."
    );
}

/// REQ-ING-004(a), CR 207.2a: reminder text on its own line is dropped from the abilities but
/// stays in the display original.
#[test]
fn reminder_own_line_dropped() {
    let f = tok("Do test thing.\n(Test reminder on its own line.)\n  (more) (still more)  ");
    assert_eq!(texts(&f), ["Do test thing."]);
    assert!(f.display.contains("(Test reminder on its own line.)"));
}

/// REQ-ING-004(a), CR 207.2a: nested parentheses go as one span; whitespace and punctuation are
/// tidy afterwards; a line without a removal is left alone.
#[test]
fn reminder_nested_and_tidy() {
    let keep = BTreeSet::new();
    let rows = [
        ("Do (a (b) c) thing.", "Do thing."),
        ("Do thing (x).", "Do thing."),
        ("A (x), B (y); C (z): D", "A, B; C: D"),
        ("(x) lead", "lead"),
        ("A  (x)  B", "A B"),
        ("no parens  here", "no parens  here"),
    ];
    for (input, want) in rows {
        assert_eq!(strip_reminder_text(input, &keep), want, "input {input:?}");
    }
}

/// REQ-ING-004(a), R12: unbalanced parentheses stay literal and never panic.
#[test]
fn reminder_unbalanced_kept() {
    let keep = BTreeSet::new();
    assert_eq!(strip_reminder_text("Do (thing", &keep), "Do (thing");
    assert_eq!(strip_reminder_text("Do thing)", &keep), "Do thing)");
    assert_eq!(strip_reminder_text("Do (a (b) thing", &keep), "Do (a thing");
    assert_eq!(strip_reminder_text(")(", &keep), ")(");
    let f = tok("Do (test thing\nDo test) thing");
    assert_eq!(texts(&f), ["Do (test thing", "Do test) thing"]);
}

/// REQ-ING-004(a): a configured span is kept for that card only; the same span on a card
/// without an entry is stripped.
#[test]
fn reminder_config_exception() {
    let cfg = config_json(
        r#"{"formatVersion":1,"cards":{"test.paren_card":{"keepParentheses":["(This is rules text.)"]}}}"#,
    );
    let text = "Do test thing. (This is rules text.) (Not kept.)";
    let kept = tok_full("test.paren_card", "test.face", &[], text, &cfg);
    assert_eq!(texts(&kept), ["Do test thing. (This is rules text.)"]);
    let other = tok_full("test.other_card", "test.face", &[], text, &cfg);
    assert_eq!(texts(&other), ["Do test thing."]);
    let keep = set(&["(k)"]);
    assert_eq!(strip_reminder_text("A (k) (j)", &keep), "A (k)");
}

/// REQ-ING-004(a), R12: the config loader accepts a valid document and normalizes its strings.
#[test]
fn config_loader_accepts_valid() {
    let cfg = config_json(
        r#"{"formatVersion":1,"cards":{
            "test.a":{"keepParentheses":["(x)"],"shortNames":["Shorty"],"otherNames":["test.other"]},
            "test.b":{},
            "test.c":{"shortNames":["Quote’s"]}}}"#,
    );
    assert_eq!(cfg.cards.len(), 3);
    let a = &cfg.cards["test.a"];
    assert_eq!(a.keep_parentheses, set(&["(x)"]));
    assert_eq!(a.short_names, set(&["Shorty"]));
    assert_eq!(a.other_names, set(&["test.other"]));
    assert_eq!(cfg.cards["test.b"], CardTextConfig::default());
    assert_eq!(cfg.cards["test.c"].short_names, set(&["Quote's"]));
    assert_eq!(
        TokenizerConfig::from_json(br#"{"formatVersion":1,"cards":{}}"#),
        Ok(TokenizerConfig::default())
    );
}

/// REQ-ING-004(a), R12: malformed, oversized, deeply nested and wrong-typed documents are
/// refused with a clean error.
#[test]
fn config_loader_rejects_bad() {
    let shape_errors = [
        "[]",
        "{}",
        r#"{"formatVersion":2,"cards":{}}"#,
        r#"{"formatVersion":"1","cards":{}}"#,
        r#"{"formatVersion":1}"#,
        r#"{"formatVersion":1,"cards":[]}"#,
        r#"{"formatVersion":1,"cards":{},"extra":1}"#,
        r#"{"formatVersion":1,"cards":{"k":[]}}"#,
        r#"{"formatVersion":1,"cards":{"k":{"unknown":[]}}}"#,
        r#"{"formatVersion":1,"cards":{"k":{"shortNames":"x"}}}"#,
        r#"{"formatVersion":1,"cards":{"k":{"shortNames":[1]}}}"#,
        r#"{"formatVersion":1,"cards":{"k":{"shortNames":["  "]}}}"#,
        r#"{"formatVersion":1,"cards":{"k":{"shortNames":["a\nb"]}}}"#,
        r#"{"formatVersion":1,"cards":{"k":{"keepParentheses":["no parens"]}}}"#,
        r#"{"formatVersion":1,"cards":{"":{}}}"#,
    ];
    for doc in shape_errors {
        let r = TokenizerConfig::from_json(doc.as_bytes());
        assert!(matches!(r, Err(ConfigError::Shape(_))), "{doc}: {r:?}");
        assert!(!r.unwrap_err().to_string().is_empty());
    }
    for doc in ["", "{", "nul", "{\"formatVersion\":1,}", "\u{0}"] {
        let r = TokenizerConfig::from_json(doc.as_bytes());
        assert!(matches!(r, Err(ConfigError::Json(_))), "{doc}: {r:?}");
        assert!(r.unwrap_err().to_string().contains("JSON"));
    }
    let deep = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    assert!(matches!(
        TokenizerConfig::from_json(deep.as_bytes()),
        Err(ConfigError::Json(_))
    ));
    let big = vec![b' '; MAX_CONFIG_BYTES + 1];
    let r = TokenizerConfig::from_json(&big);
    assert_eq!(
        r,
        Err(ConfigError::TooLarge {
            len: MAX_CONFIG_BYTES + 1
        })
    );
    assert!(r.unwrap_err().to_string().contains("limit"));
    let long = format!(
        r#"{{"formatVersion":1,"cards":{{"k":{{"shortNames":["{}"]}}}}}}"#,
        "a".repeat(MAX_CONFIG_STRING_BYTES + 1)
    );
    assert!(matches!(
        TokenizerConfig::from_json(long.as_bytes()),
        Err(ConfigError::Shape(_))
    ));
    let many: Vec<String> = (0..=MAX_CONFIG_LIST_LEN)
        .map(|i| format!("\"n{i}\""))
        .collect();
    let many = format!(
        r#"{{"formatVersion":1,"cards":{{"k":{{"otherNames":[{}]}}}}}}"#,
        many.join(",")
    );
    assert!(matches!(
        TokenizerConfig::from_json(many.as_bytes()),
        Err(ConfigError::Shape(_))
    ));
    assert_eq!(CONFIG_FORMAT_VERSION, 1);
}

// ---------------------------------------------------------------------------------------------
// 3. Self-references
// ---------------------------------------------------------------------------------------------

/// REQ-ING-004(b), CR 201.5: the face's own name (case-sensitive, whole word), `~`, and the
/// "this <noun>" phrases (any case, every vocabulary card type, the face's own subtypes) become
/// NameRef{SELF} with no name.
#[test]
fn self_reference_forms() {
    let f = tok_full(
        "test.key",
        "Test Flier",
        &["Goblin", "Equipment"],
        "Test Flier does test thing.\n~ does test thing.\nThis creature does test thing.\n\
         this spell does test thing.\nTHIS ARTIFACT and This Land and this enchantment.\n\
         this Planeswalker, this permanent, this card, this token.\n\
         This Goblin and this equipment.",
        &TokenizerConfig::default(),
    );
    let counts: Vec<usize> = f.abilities.iter().map(|a| self_count(&a.body)).collect();
    assert_eq!(counts, [1, 1, 1, 1, 3, 4, 2]);
    // "Goblin" inside "This Goblin" is part of the self-reference, not a word.
    assert!(words(&f.abilities[6].body).is_empty());
    for t in &vocab().card_types {
        let g = tok(&format!("Exile this {t}."));
        assert_eq!(self_count(&g.abilities[0].body), 1, "{t}");
    }
}

/// REQ-ING-004(b), CR 201.5: the name matches case-sensitively and only as a whole word; a
/// "this" phrase needs a known noun; an empty name never matches.
#[test]
fn self_reference_boundaries() {
    let f = tok_full(
        "test.key",
        "Zorb",
        &[],
        "Zorbs and zorb and xZorb and Zorb_x and Zorb's.\nthis thing and thistle creature.",
        &TokenizerConfig::default(),
    );
    assert_eq!(self_count(&f.abilities[0].body), 1);
    assert_eq!(f.abilities[0].body.spans.len(), 3);
    assert_eq!(self_count(&f.abilities[1].body), 0);
    let g = tok_full("k", "", &[], "Do test thing.", &TokenizerConfig::default());
    assert_eq!(self_count(&g.abilities[0].body), 0);
}

/// REQ-ING-004(b), CR 201.5c: configured shortened names are self-references; configured other
/// names are NameRef{OTHER} carrying the name; a card without an entry gets neither.
#[test]
fn self_reference_config_aliases() {
    let cfg = config_json(
        r#"{"formatVersion":1,"cards":{"test.alias_card":
            {"shortNames":["Zed"],"otherNames":["test.partner card"]}}}"#,
    );
    let text = "Zed, the Test, and test.partner card do test thing.";
    let f = tok_full("test.alias_card", "Zed, the Test", &[], text, &cfg);
    let toks = f.abilities[0].body.tokens.tokens();
    assert!(matches!(
        &toks[0],
        TextToken::NameRef {
            kind: NameRefKind::SelfRef,
            name: None,
            ..
        }
    ));
    let other: Vec<String> = toks
        .iter()
        .filter_map(|t| match t {
            TextToken::NameRef {
                kind: NameRefKind::Other,
                name,
                ..
            } => name.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(other, ["test.partner card"]);
    let alias = tok_full(
        "test.alias_card",
        "test.full",
        &[],
        "Zed does test thing.",
        &cfg,
    );
    assert_eq!(self_count(&alias.abilities[0].body), 1);
    let none = tok_full("test.none", "test.full", &[], text, &cfg);
    assert_eq!(self_count(&none.abilities[0].body), 0);
}

// ---------------------------------------------------------------------------------------------
// 4. Rewritable words
// ---------------------------------------------------------------------------------------------

/// REQ-ING-004(c), REQ-DSL-012, CR 105.1, CR 305.6, CR 612.2: the five color words, the five basic
/// land types and creature types (one multi-word) are words of the right category in every case
/// style; the value is the matched source text.
#[test]
fn rewritable_words_categories_and_cases() {
    let styles: [fn(&str) -> String; 3] = [str::to_lowercase, str::to_uppercase, str::to_string];
    for style in styles {
        let colors = ["white", "Blue", "black", "Red", "green"].map(style);
        let lands = ["Plains", "island", "Swamp", "MOUNTAIN", "Forest"].map(style);
        let creatures = ["Goblin", "elf", "Time Lord", "Assembly-Worker"].map(style);
        let text = format!(
            "{} and {} do. {}; Do test thing.",
            colors.join(" "),
            lands.join(", "),
            creatures.join(" or "),
        );
        let f = tok(&text);
        let got = words(&f.abilities[0].body);
        let mut want: Vec<(Cat, String)> = Vec::new();
        want.extend(colors.iter().map(|c| (Cat::ColorWord, c.clone())));
        want.extend(lands.iter().map(|c| (Cat::BasicLandType, c.clone())));
        want.extend(creatures.iter().map(|c| (Cat::CreatureType, c.clone())));
        assert_eq!(got, want, "text {text:?}");
    }
}

/// REQ-ING-004(c), CR 612.2: whole words only; a match inside a longer word is not marked; a
/// multi-word creature type matches whole or not at all.
#[test]
fn rewritable_words_boundaries() {
    let f = tok("Redness, whitewash, Islands, Goblins, xblue, red_x, Time Lords, Time.");
    assert!(words(&f.abilities[0].body).is_empty());
    let g = tok("non-Goblin, red-hot, (gone) Lord Time Lord.");
    assert_eq!(
        words(&g.abilities[0].body),
        [
            (Cat::CreatureType, "Goblin".to_string()),
            (Cat::ColorWord, "red".to_string()),
            (Cat::CreatureType, "Time Lord".to_string()),
        ]
    );
}

/// REQ-ING-004(c), CR 612.2: words inside the card's own name or a configured name are never
/// marked; the same words elsewhere are.
#[test]
fn rewritable_words_not_inside_names() {
    let cfg = config_json(
        r#"{"formatVersion":1,"cards":{"test.k":{"shortNames":["Blue Elf"],"otherNames":["Green Forest Goblin"]}}}"#,
    );
    let f = tok_full(
        "test.k",
        "Red Goblin of the Swamp",
        &[],
        "Red Goblin of the Swamp, Blue Elf and Green Forest Goblin see a red Goblin in a Swamp.",
        &cfg,
    );
    let body = &f.abilities[0].body;
    assert_eq!(self_count(body), 2);
    assert_eq!(
        words(body),
        [
            (Cat::ColorWord, "red".to_string()),
            (Cat::CreatureType, "Goblin".to_string()),
            (Cat::BasicLandType, "Swamp".to_string()),
        ]
    );
}

/// REQ-ING-004(c), REQ-DSL-012: numbers stay literal (no NUMBER or CARD_NAME words); ids are
/// face-wide `t1..`; adjacent literal text is one token.
#[test]
fn rewritable_words_ids_and_literals() {
    let f = tok("Draw 3 cards.\nBlue and red.\nDo 10 test things.");
    let all: Vec<&TextToken> = pieces(&f)
        .into_iter()
        .flat_map(|p| p.tokens.tokens())
        .collect();
    let ids: Vec<&str> = all.iter().map(|t| t.id()).collect();
    assert_eq!(ids, ["t1", "t2", "t3", "t4", "t5", "t6"]);
    assert!(matches!(all[0], TextToken::Literal { text, .. } if text == "Draw 3 cards."));
    assert!(matches!(all[5], TextToken::Literal { text, .. } if text == "Do 10 test things."));
    assert_eq!(
        f.abilities[1].body.spans,
        [(0, 4), (4, 9), (9, 12), (12, 13)]
    );
}

// ---------------------------------------------------------------------------------------------
// 5. Line structure
// ---------------------------------------------------------------------------------------------

/// REQ-ING-004(d), CR 113.2c: each paragraph break is a new ability; a comma or semicolon list
/// stays one ability; blank lines are skipped.
#[test]
fn structure_paragraphs() {
    let f = tok("Flying, vigilance, trample\n\nDo test thing; do other thing.\n");
    assert_eq!(
        texts(&f),
        [
            "Flying, vigilance, trample",
            "Do test thing; do other thing."
        ]
    );
    assert!(f.abilities.iter().all(|a| a.kind == AbilityKind::Plain));
    assert!(f.abilities.iter().all(|a| a.section.is_none()));
    assert!(f.sections.is_empty());
    assert!(tok("").abilities.is_empty());
}

/// REQ-ING-004(d), CR 207.2c, CR 207.2d: a leading anchor word is recorded and removed from the
/// marked text; malformed prefixes fall back to Plain.
#[test]
fn structure_anchor() {
    let f = tok(
        "Test Word \u{2014} Do test thing.\nTest's Odd-Word \u{2014} Do it.\n\
         lower \u{2014} Do it.\nWith 2 \u{2014} Do it.\nTest\u{2014}Do it.\nTest Word \u{2014} \n\
         One Two Three Four Five Six Seven \u{2014} Do it.\nIIII \u{2014} Do it.",
    );
    assert_eq!(f.abilities.len(), 8);
    assert_eq!(f.abilities[0].kind, AbilityKind::Anchored);
    assert_eq!(f.abilities[0].anchor.as_deref(), Some("Test Word"));
    assert_eq!(f.abilities[0].body.text, "Do test thing.");
    assert_eq!(f.abilities[1].anchor.as_deref(), Some("Test's Odd-Word"));
    for a in &f.abilities[2..] {
        assert_eq!(a.kind, AbilityKind::Plain, "{}", a.original);
        assert!(a.anchor.is_none());
        assert_eq!(a.body.text, a.original.trim());
    }
}

/// REQ-ING-004(d), CR 700.2: a "choose" header ending in an em dash takes the following bullet
/// lines as its modes, for any count wording.
#[test]
fn structure_modal_headers() {
    for header in [
        "Choose one \u{2014}",
        "Choose two \u{2014}",
        "Choose one or more \u{2014}",
        "When this creature enters, choose one or both \u{2014}",
    ] {
        let text = format!("{header}\n\u{2022} Do test thing.\n\u{2022} Do red thing.\nAfter.");
        let f = tok(&text);
        assert_eq!(f.abilities.len(), 2, "{header}");
        let a = &f.abilities[0];
        assert_eq!(a.body.text, header);
        let AbilityKind::Modal { modes } = &a.kind else {
            panic!("expected modal for {header}");
        };
        let m: Vec<&str> = modes.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(m, ["Do test thing.", "Do red thing."]);
        assert_eq!(modes[1].original, "\u{2022} Do red thing.");
        assert_eq!(words(&modes[1]), [(Cat::ColorWord, "red".to_string())]);
        assert_eq!(a.original.lines().count(), 3);
        assert_eq!(f.abilities[1].body.text, "After.");
        assert_eq!(f.abilities[1].kind, AbilityKind::Plain);
    }
}

/// REQ-ING-004(d), CR 700.2h, CR 700.2i: pawprint options are modes (the pawprints stay in the
/// mode text), a cost before the effect stays in the mode text, and reminder text is stripped.
#[test]
fn structure_modal_pawprint_and_cost() {
    let f = tok(
        "Choose up to three {P} worth of modes. (Test reminder.)\n{P} \u{2014} Do test thing.\n\
         {P}{P} \u{2014} Do other thing.",
    );
    let AbilityKind::Modal { modes } = &f.abilities[0].kind else {
        panic!("expected modal");
    };
    let m: Vec<&str> = modes.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(
        m,
        [
            "{P} \u{2014} Do test thing.",
            "{P}{P} \u{2014} Do other thing."
        ]
    );
    let g = tok("Choose one \u{2014}\n\u{2022} {1} \u{2014} Do test thing.\n\u{2022} {2}: Draw.");
    let AbilityKind::Modal { modes } = &g.abilities[0].kind else {
        panic!("expected modal");
    };
    assert_eq!(modes[0].text, "{1} \u{2014} Do test thing.");
    assert_eq!(modes[1].text, "{2}: Draw.");
}

/// REQ-ING-004(d), CR 700.2: bullets with no header, a header with no bullets, bullets after a
/// header without the em dash, empty bullets and a bare pawprint fall back to plain lines.
#[test]
fn structure_modal_fallbacks() {
    let f = tok(
        "\u{2022} Stray bullet.\nChoose one \u{2014}\nDo test thing.\n\
         Choose a thing.\n\u{2022} Not a mode.\nChoose one \u{2014}\n\u{2022}\n{P}",
    );
    assert_eq!(f.abilities.len(), 8);
    assert!(
        f.abilities.iter().all(|a| a.kind == AbilityKind::Plain),
        "{:?}",
        f.abilities
    );
}

/// REQ-ING-004(d), CR 722.2, CR 722.4: station symbol lines `N+ | text`, `N+|text` and a bare
/// `N+ |` open sections; abilities before the first symbol are ordinary; a P/T line is kept as
/// the striation's text; more than six digits is plain.
#[test]
fn structure_station() {
    let f = tok(
        "Station (Test reminder.)\nDo test thing.\n3+ | Flying\nDo red thing.\n8+|Trample\n\
         12+ |\nDo blue thing.\n5/5\n1234567+ | Too long.\n3 | No plus.\n+3 | Wrong.",
    );
    assert_eq!(
        f.sections
            .iter()
            .map(|s| s.kind.clone())
            .collect::<Vec<_>>(),
        [
            SectionKind::Station { threshold: 3 },
            SectionKind::Station { threshold: 8 },
            SectionKind::Station { threshold: 12 },
        ]
    );
    assert_eq!(
        sections_of(&f),
        [
            ("Station", None),
            ("Do test thing.", None),
            ("Flying", Some(0)),
            ("Do red thing.", Some(0)),
            ("Trample", Some(1)),
            ("Do blue thing.", Some(2)),
            ("1234567+ | Too long.", Some(2)),
            ("3 | No plus.", Some(2)),
            ("+3 | Wrong.", Some(2)),
        ]
    );
    assert_eq!(f.sections[2].pt.as_deref(), Some("5/5"));
    assert_eq!(f.sections[2].header.as_deref(), Some("12+ |"));
    assert_eq!(f.sections[0].pt, None);
    assert_eq!(f.abilities[2].original, "3+ | Flying");
    assert!(f.abilities.iter().all(|a| a.kind == AbilityKind::Plain));
    let six = tok("999999+ | Max.");
    assert_eq!(
        six.sections[0].kind,
        SectionKind::Station { threshold: 999_999 }
    );
}

/// REQ-ING-004(d), CR 711.2, CR 711.4: level bands `LEVEL a-b` and `LEVEL a+` (any case) carry
/// their bounds; ability and P/T lines belong to the band; lines before the first band are
/// ordinary; malformed headers are plain.
#[test]
fn structure_level_bands() {
    let f = tok(
        "Level up {2} (Test reminder.)\nLEVEL 1-3\n2/3\nFlying\nlevel 4+\n4/5\nDo test thing.\n\
         Trample\nLEVEL 5-2\nLEVEL x\nLEVEL 1234567+\n3/3",
    );
    assert_eq!(
        f.sections
            .iter()
            .map(|s| s.kind.clone())
            .collect::<Vec<_>>(),
        [
            SectionKind::LevelBand {
                min: 1,
                max: Some(3)
            },
            SectionKind::LevelBand { min: 4, max: None },
        ]
    );
    assert_eq!(f.sections[0].pt.as_deref(), Some("2/3"));
    assert_eq!(f.sections[1].pt.as_deref(), Some("4/5"));
    assert_eq!(f.sections[1].header.as_deref(), Some("level 4+"));
    assert_eq!(
        sections_of(&f),
        [
            ("Level up {2}", None),
            ("Flying", Some(0)),
            ("Do test thing.", Some(1)),
            ("Trample", Some(1)),
            ("LEVEL 5-2", Some(1)),
            ("LEVEL x", Some(1)),
            ("LEVEL 1234567+", Some(1)),
            ("3/3", Some(1)),
        ]
    );
    // A P/T line outside any striation is a plain line.
    assert_eq!(texts(&tok("2/3")), ["2/3"]);
}

/// REQ-ING-004(d), CR 715.2b, CR 715.2c: single chapters and numeral lists; non-canonical,
/// too-large and out-of-order numerals, the hyphen form and ordinary capital-I lines are not
/// chapters.
#[test]
fn structure_chapters() {
    let f = tok(
        "(Test reminder.)\nI \u{2014} Do test thing.\nII, III \u{2014} Do red thing.\n\
         IV \u{2014} Do it.\nXCIX \u{2014} Do it.\nIIII \u{2014} Do it.\nIC \u{2014} Do it.\n\
         I - Do it.\nIt does test thing.\nInstant \u{2014} Do it.\nII, I \u{2014} Do it.\n\
         I \u{2014} \nC \u{2014} Do it.\nI, , II \u{2014} Do it.",
    );
    assert_eq!(f.abilities.len(), 13);
    let kinds: Vec<&AbilityKind> = f.abilities.iter().map(|a| &a.kind).collect();
    assert_eq!(kinds[0], &AbilityKind::Chapter { chapters: vec![1] });
    assert_eq!(
        kinds[1],
        &AbilityKind::Chapter {
            chapters: vec![2, 3]
        }
    );
    assert_eq!(kinds[2], &AbilityKind::Chapter { chapters: vec![4] });
    assert_eq!(kinds[3], &AbilityKind::Chapter { chapters: vec![99] });
    assert_eq!(f.abilities[0].body.text, "Do test thing.");
    assert_eq!(
        words(&f.abilities[1].body),
        [(Cat::ColorWord, "red".into())]
    );
    for (i, a) in f.abilities.iter().enumerate().skip(4) {
        let want = if i == 8 {
            // "Instant — Do it." has a word prefix: an anchor, not a chapter.
            AbilityKind::Anchored
        } else {
            AbilityKind::Plain
        };
        assert_eq!(a.kind, want, "{}", a.original);
    }
}

/// REQ-ING-004(d), CR 717.2a, CR 717.3: class level headers carry the level and cost; the text
/// after the em dash is the level's first ability; lines group under their level; the top
/// section is level 1; malformed headers are plain.
#[test]
fn structure_class_levels() {
    let f = tok(
        "(Test reminder.)\nDo test thing.\n{1}{W}: Level 2 \u{2014} Flying\nDo red thing.\n\
         {3}: level 3 \u{2014}\nTrample\n{X}: Level two \u{2014} Plain.\n: Level 4 \u{2014} Plain.\n\
         {1}: Level 0 \u{2014} Plain.\n{1}: Level 5 no dash",
    );
    assert_eq!(
        f.sections
            .iter()
            .map(|s| s.kind.clone())
            .collect::<Vec<_>>(),
        [
            SectionKind::ClassLevel {
                level: 1,
                cost: None
            },
            SectionKind::ClassLevel {
                level: 2,
                cost: Some("{1}{W}".into())
            },
            SectionKind::ClassLevel {
                level: 3,
                cost: Some("{3}".into())
            },
        ]
    );
    assert_eq!(f.sections[0].header, None);
    assert_eq!(
        sections_of(&f),
        [
            ("Do test thing.", Some(0)),
            ("Flying", Some(1)),
            ("Do red thing.", Some(1)),
            ("Trample", Some(2)),
            ("{X}: Level two \u{2014} Plain.", Some(2)),
            (": Level 4 \u{2014} Plain.", Some(2)),
            ("{1}: Level 0 \u{2014} Plain.", Some(2)),
            ("{1}: Level 5 no dash", Some(2)),
        ]
    );
    assert!(f.abilities.iter().all(|a| a.kind == AbilityKind::Plain));
    // No header at all: no implicit level 1.
    assert!(tok("Do test thing.").sections.is_empty());
    // A station line before a class header: no implicit level 1 either.
    assert_eq!(tok("1+ | A\n{1}: Level 2 \u{2014} B").sections.len(), 2);
}

/// REQ-ING-004(a)-(d), CR 207.2a, CR 207.2c, CR 700.2, CR 722.2: a combined face with reminder
/// text, an anchor, a modal group inside a station striation, self-references and words, end
/// to end.
#[test]
fn structure_combined_face() {
    let f = tok_full(
        "test.key",
        "Test Ship",
        &["Spacecraft"],
        "Station (Test reminder.)\nTest Word \u{2014} When Test Ship enters, do test thing.\n\
         (Reminder line.)\n5+ | Flying (Test reminder.)\nChoose one \u{2014}\n\u{2022} This Spacecraft \
         deals 1 to a Goblin.\n\u{2022} Add white.\n6/6\nDo test thing.",
        &TokenizerConfig::default(),
    );
    assert_eq!(f.abilities.len(), 5);
    assert_eq!(f.abilities[1].kind, AbilityKind::Anchored);
    assert_eq!(self_count(&f.abilities[1].body), 1);
    assert_eq!(f.abilities[2].body.text, "Flying");
    assert_eq!(f.abilities[2].section, Some(0));
    let AbilityKind::Modal { modes } = &f.abilities[3].kind else {
        panic!("expected modal");
    };
    assert_eq!(f.abilities[3].section, Some(0));
    assert_eq!(self_count(&modes[0]), 1);
    assert_eq!(words(&modes[0]), [(Cat::CreatureType, "Goblin".into())]);
    assert_eq!(words(&modes[1]), [(Cat::ColorWord, "white".into())]);
    assert_eq!(f.sections[0].pt.as_deref(), Some("6/6"));
    assert_eq!(f.abilities[4].section, Some(0));
    assert!(f.display.contains("(Reminder line.)"));
}

// ---------------------------------------------------------------------------------------------
// Record helper, rendering and limits
// ---------------------------------------------------------------------------------------------

fn face(name: &str, subtypes: &[&str], text: Option<&str>) -> NormalizedFace {
    NormalizedFace {
        name: name.to_string(),
        mana_cost: None,
        mana_cost_text: None,
        supertypes: Vec::new(),
        card_types: vec!["Creature".to_string()],
        subtypes: subtypes.iter().map(|s| (*s).to_string()).collect(),
        oracle_text: text.map(str::to_string),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        colors: None,
        color_indicator: None,
        computed_colors: ColorBitmask::NONE,
    }
}

/// REQ-ING-004: `tokenize_record` tokenizes every face with the record's `oracle_id` as the
/// config key; `TokenizeInput::from_face` treats absent text as empty.
#[test]
fn record_helper() {
    let record = NormalizedCardRecord {
        card_def_id: "test.record".to_string(),
        name: "test.alpha // test.beta".to_string(),
        layout: "split".to_string(),
        faces: vec![
            face(
                "test.alpha",
                &["Elf"],
                Some("This Elf does test thing. Zip."),
            ),
            face("test.beta", &[], None),
        ],
        color_identity: ColorBitmask::NONE,
        source_color_identity: None,
        mana_value: 0,
        keywords: Vec::new(),
        all_parts: Vec::new(),
        legalities: BTreeMap::new(),
        game_changer: false,
        digital_only: false,
        oracle_text_hash: String::new(),
        diagnostics: Vec::new(),
    };
    let cfg = config_json(r#"{"formatVersion":1,"cards":{"test.record":{"shortNames":["Zip"]}}}"#);
    let faces = tokenize_record(&record, vocab(), &cfg);
    assert_eq!(faces.len(), 2);
    check_face(&faces[0]);
    assert_eq!(self_count(&faces[0].abilities[0].body), 2);
    assert!(faces[1].abilities.is_empty());
    assert_eq!(faces[1].display, "");
    let input = TokenizeInput::from_face("test.record", &record.faces[1]);
    assert_eq!(input.text, "");
    assert_eq!(input.face_name, "test.beta");
    assert_eq!(input.card_key, "test.record");
}

/// REQ-DSL-012: `render_tokens` renders literals, words and other names as text and SelfRef as
/// the given placeholder.
#[test]
fn render_tokens_placeholder() {
    let cfg = config_json(r#"{"formatVersion":1,"cards":{"k":{"otherNames":["test.o"]}}}"#);
    let f = tok_full(
        "k",
        "test.self",
        &[],
        "test.self sees test.o and a red Elf.",
        &cfg,
    );
    let body = &f.abilities[0].body;
    assert_eq!(
        render_tokens(&body.tokens, SELF_REF_PLACEHOLDER),
        "~ sees test.o and a red Elf."
    );
    assert_eq!(
        render_tokens(&body.tokens, "<self>"),
        "<self> sees test.o and a red Elf."
    );
}

/// REQ-ING-004, R12: text above `MAX_TEXT_BYTES` is cut on a char boundary and flagged.
#[test]
fn limits_truncation() {
    let text = "\u{00E9}".repeat(MAX_TEXT_BYTES);
    let f = tok(&text);
    assert!(f.truncated);
    assert_eq!(f.display.len(), MAX_TEXT_BYTES);
    let odd = format!("a{}", "\u{00E9}".repeat(MAX_TEXT_BYTES));
    let g = tok(&odd);
    assert!(g.truncated);
    assert_eq!(g.display.len(), MAX_TEXT_BYTES - 1);
    assert!(!tok("short").truncated);
}

/// REQ-ING-004, REQ-DSL-012, R12: `\0`, noncharacters, emoji and accented letters never make a
/// stream invalid; a letter glued to a word keeps it from matching.
#[test]
fn unicode_safety() {
    let f = tok("\0 red \u{FDD0}\u{10FFFF} Goblin\u{00E9} \u{1F600} ~~ \u{FFFE}");
    assert!(!f.display.chars().any(normalize::is_noncharacter));
    assert_eq!(self_count(&f.abilities[0].body), 2);
    assert_eq!(
        words(&f.abilities[0].body),
        [(Cat::ColorWord, "red".into())]
    );
}

// ---------------------------------------------------------------------------------------------
// 6. Determinism
// ---------------------------------------------------------------------------------------------

/// R3, REQ-ING-004: the same input gives identical output ten times, and the order of config
/// entries and list items changes neither the configuration nor the output.
#[test]
fn determinism() {
    let text = "Test Word \u{2014} red Goblin.\n3+ | Zed and test.o.\n(x) Choose one \u{2014}\n\
                \u{2022} Swamp.\n\u{2022} (k) Blue.";
    let a = config_json(
        r#"{"formatVersion":1,"cards":{"k":{"shortNames":["Zed","Zee"],"otherNames":["test.o","test.p"],"keepParentheses":["(k)","(j)"]},"m":{}}}"#,
    );
    let b = config_json(
        r#"{"cards":{"m":{},"k":{"keepParentheses":["(j)","(k)"],"otherNames":["test.p","test.o"],"shortNames":["Zee","Zed"]}},"formatVersion":1}"#,
    );
    assert_eq!(a, b);
    let first = tok_full("k", "test.face", &["Goblin"], text, &a);
    let AbilityKind::Modal { modes } = &first.abilities[2].kind else {
        panic!("expected modal");
    };
    assert_eq!(modes[1].text, "(k) Blue.");
    let debug = format!("{first:?}");
    for _ in 0..10 {
        let again = tok_full("k", "test.face", &["Goblin"], text, &a);
        assert_eq!(again, first);
        let other = tok_full("k", "test.face", &["Goblin"], text, &b);
        assert_eq!(format!("{other:?}"), debug);
    }
}

// ---------------------------------------------------------------------------------------------
// 7. Property and pathological inputs
// ---------------------------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// Pieces biased towards the structural characters of Oracle text.
const ALPHABET: [&str; 40] = [
    "(",
    ")",
    "\u{2022}",
    "\u{2014}",
    "\u{2013}",
    "-",
    "|",
    "+",
    "0",
    "3",
    "12",
    "I",
    "V",
    "X",
    "II, III",
    "\n",
    "\r\n",
    " ",
    ":",
    "{P}",
    "{2}",
    "LEVEL ",
    "Level ",
    "Choose one ",
    "red",
    "Goblin",
    "Time Lord",
    "this creature",
    "~",
    "test.self",
    "\u{00E9}",
    "\u{1F600}",
    "\u{FDD0}",
    "\0",
    "/",
    "\u{FEFF}",
    "\u{A0}",
    "'",
    "Swamp",
    "Word",
];

fn random_text(rng: &mut Rng) -> String {
    let len = rng.below(4096) as usize;
    let mut s = String::new();
    while s.len() < len {
        if rng.below(4) == 0 {
            // Any scalar value, including noncharacters and controls.
            let c = char::from_u32(rng.below(0x11_0000) as u32).unwrap_or('\u{FFFD}');
            s.push(c);
        } else {
            s.push_str(ALPHABET[rng.below(ALPHABET.len() as u64) as usize]);
        }
    }
    s
}

/// R12, REQ-ING-004, REQ-DSL-012: 2,500 fixed-seed random texts never panic, every stream
/// validates, ids are unique face-wide, spans cover the text, and the render invariant holds
/// (all via `check_face`); normalization is idempotent on them.
#[test]
fn property_random_text() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let cfg = config_json(
        r#"{"formatVersion":1,"cards":{"k":{"keepParentheses":["(I)"],"shortNames":["Word"],"otherNames":["Goblin"]}}}"#,
    );
    let empty = TokenizerConfig::default();
    for i in 0..2_500u32 {
        let text = random_text(&mut rng);
        let once = normalize_text(&text);
        assert_eq!(normalize_text(&once), once);
        let config = if i % 2 == 0 { &cfg } else { &empty };
        let f = tok_full("k", "test.self", &["Goblin", "Time Lord"], &text, config);
        assert!(!f.truncated);
    }
}

/// R12, REQ-ING-004(a): the config loader survives bounded random bytes (raw and lossily
/// decoded) behind JSON-looking prefixes.
#[test]
fn property_config_random_bytes() {
    let mut rng = Rng(0x0123_4567_89ab_cdef);
    let prefixes: [&[u8]; 4] = [
        b"",
        b"{",
        b"{\"formatVersion\":1,\"cards\":{\"k\":{\"shortNames\":[",
        b"[",
    ];
    for i in 0..2_500usize {
        let len = rng.below(4096) as usize;
        let mut b: Vec<u8> = prefixes[i % 4].to_vec();
        b.extend((0..len).map(|_| (rng.next() & 0xff) as u8));
        let s = String::from_utf8_lossy(&b);
        let _ = TokenizerConfig::from_json(s.as_bytes());
        let _ = TokenizerConfig::from_json(&b);
    }
}

/// R12, REQ-ING-004: pathological inputs finish without panic: 10,000 open parentheses, deep
/// balanced nesting, a 1 MiB single line, 100k empty lines and long separator runs.
#[test]
fn pathological_inputs() {
    let parens = "(".repeat(10_000);
    let f = tok(&parens);
    assert_eq!(texts(&f), [parens.as_str()]);
    let closed = format!("x {}{} y", "(".repeat(10_000), ")".repeat(10_000));
    assert_eq!(texts(&tok(&closed)), ["x y"]);
    let line = "red Goblin ".repeat((1 << 20) / 11 + 1);
    assert!(line.len() >= 1 << 20);
    assert!(tok(&line).truncated);
    let empty = "\n".repeat(100_000);
    assert!(tok(&empty).abilities.is_empty());
    let _ = tok(&"\u{2014}".repeat(30_000));
    let bullets = format!("Choose one \u{2014}\n{}", "\u{2022} x\n".repeat(10_000));
    let k = tok(&bullets);
    assert!(matches!(&k.abilities[0].kind, AbilityKind::Modal { modes } if modes.len() == 10_000));
    let _ = tok(&"~".repeat(70_000));
    let _ = tok(&"this ".repeat(20_000));
}
