// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of text-changing effects: REQ-DSL-012 (REPLACE_TEXT_WORD over token tables, word
// references by token id; CR 612.1, 612.2, 612.3, 613.1c, 201.5), REQ-LAYER-001 and
// REQ-LAYER-002 (layer 3 application and ordering; CR 613.7, 613.8a-c), REQ-LAYER-005 (cache
// agrees with full recompute), REQ-CORE-001 (determinism). Synthetic objects only (R5): object
// names are `test.*`; the word values are rules vocabulary (basic land types, colour words,
// creature types).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{self, AbilityAST, ColorBitmask};
use fineprint_dsl::expr::{
    Cmp, DistinctAttr, HistoryQuery, MathOp, ObjectSelectorAST, PlayerAttr, PlayerGroup,
    PlayerSelectorAST, PredicateAST, ReferenceAST, SelectorFilter, ValueExpressionAST, WordRef,
};
use fineprint_dsl::text::{NameRefKind, RewritableCategory, TextToken, TextTokenStream};

use super::*;
use crate::objects::Characteristics;

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

const BLT: RewritableCategory = RewritableCategory::BasicLandType;
const SUPPORTED: [RewritableCategory; 4] = [
    RewritableCategory::ColorWord,
    RewritableCategory::BasicLandType,
    RewritableCategory::CreatureType,
    RewritableCategory::Number,
];

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

fn ts(n: u64) -> Timestamp {
    Timestamp::new(n)
}

fn lit(id: &str, text: &str) -> TextToken {
    TextToken::Literal {
        id: id.to_owned(),
        text: text.to_owned(),
    }
}

fn word(id: &str, category: RewritableCategory, value: &str) -> TextToken {
    TextToken::Word {
        id: id.to_owned(),
        category,
        value: value.to_owned(),
    }
}

fn self_ref(id: &str) -> TextToken {
    TextToken::NameRef {
        id: id.to_owned(),
        kind: NameRefKind::SelfRef,
        name: None,
    }
}

fn other_ref(id: &str, name: &str) -> TextToken {
    TextToken::NameRef {
        id: id.to_owned(),
        kind: NameRefKind::Other,
        name: Some(name.to_owned()),
    }
}

fn stream(tokens: Vec<TextToken>) -> TextTokenStream {
    TextTokenStream::new(tokens).unwrap()
}

fn tref(id: &str) -> WordRef {
    WordRef::TextToken(id.to_owned())
}

fn name(n: &str) -> WordRef {
    WordRef::Name(n.to_owned())
}

/// A stream with a card-name word, a literal and an OTHER name reference whose text equals the
/// replaced word, a lowercase variant and the same value in another category.
fn sample() -> TextTokenStream {
    stream(vec![
        lit("t1", "Swamp"),
        word("t2", BLT, "Swamp"),
        word("t3", RewritableCategory::ColorWord, "black"),
        self_ref("t4"),
        other_ref("t5", "Swamp"),
        word("t6", RewritableCategory::CardName, "Swamp"),
        word("t7", RewritableCategory::CreatureType, "Goblin"),
        word("t8", RewritableCategory::Number, "2"),
        word("t9", BLT, "swamp"),
        word("t10", RewritableCategory::ColorWord, "Swamp"),
    ])
}

fn value_of(s: &TextTokenStream, id: &str) -> String {
    resolve_word_ref(s, &tref(id)).unwrap()
}

fn filter(pred: PredicateAST) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    })
}

fn has_token_ref<T: std::fmt::Debug>(ast: &T) -> bool {
    format!("{ast:?}").contains("TextToken(")
}

// ---------------------------------------------------------------------------------------------
// rewrite_text_words
// ---------------------------------------------------------------------------------------------

// REQ-DSL-012; CR 612.1: every word of the category with the value is replaced.
#[test]
fn rewrite_replaces_matching_word() {
    let s = stream(vec![
        lit("a", "Destroy target "),
        word("b", BLT, "Swamp"),
        lit("c", " or "),
        word("d", BLT, "Swamp"),
    ]);
    let r = rewrite_text_words(&s, BLT, "Swamp", "Island").unwrap();
    assert_eq!(value_of(&r, "b"), "Island");
    assert_eq!(value_of(&r, "d"), "Island");
    assert_eq!(r.tokens()[0], s.tokens()[0]);
    assert_eq!(r.tokens()[2], s.tokens()[2]);
}

// REQ-DSL-012; CR 612.2: an exact, case-sensitive match; only the matching category; literal
// text, name references (SELF, OTHER) and card-name words are never rewritten.
#[test]
fn rewrite_is_exact_and_category_bound() {
    let s = sample();
    let r = rewrite_text_words(&s, BLT, "Swamp", "Island").unwrap();
    let mut expected = s.tokens().to_vec();
    expected[1] = word("t2", BLT, "Island");
    assert_eq!(r.tokens(), expected.as_slice());
    // The lowercase word t9 and the colour-category "Swamp" t10 stay.
    assert_eq!(value_of(&r, "t9"), "swamp");
    assert_eq!(value_of(&r, "t10"), "Swamp");
    assert_eq!(value_of(&r, "t6"), "Swamp");
}

// REQ-DSL-012; CR 612.2, 201.5: for each supported category, literal text, SELF and OTHER
// references and CARD_NAME words equal to `from` stay; only the word of that category changes.
#[test]
fn rewrite_never_touches_literals_names_or_card_names_in_any_category() {
    for category in SUPPORTED {
        let s = stream(vec![
            lit("l", "Elf"),
            self_ref("s"),
            other_ref("o", "Elf"),
            word("n", RewritableCategory::CardName, "Elf"),
            word("w", category, "Elf"),
        ]);
        let r = rewrite_text_words(&s, category, "Elf", "Merfolk").unwrap();
        let mut expected = s.tokens().to_vec();
        expected[4] = word("w", category, "Merfolk");
        assert_eq!(r.tokens(), expected.as_slice(), "{category:?}");
    }
}

// REQ-DSL-012; CR 612.2: no match gives an equal stream; from == to gives an equal stream (an
// ordinary replacement that changes nothing).
#[test]
fn rewrite_without_match_or_with_same_word_is_identity() {
    let s = sample();
    assert_eq!(rewrite_text_words(&s, BLT, "Forest", "Island").unwrap(), s);
    assert_eq!(rewrite_text_words(&s, BLT, "Swamp", "Swamp").unwrap(), s);
    let empty = TextTokenStream::default();
    assert_eq!(
        rewrite_text_words(&empty, BLT, "Swamp", "Island").unwrap(),
        empty
    );
}

// REQ-DSL-012: empty or noncharacter words are InvalidWord.
#[test]
fn rewrite_rejects_invalid_words() {
    let s = sample();
    for (from, to) in [
        ("", "Island"),
        ("Swamp", ""),
        ("Sw\u{FDD0}amp", "Island"),
        ("Swamp", "Isl\u{FFFE}and"),
        ("Swamp", "\u{1FFFF}"),
        ("\u{10FFFF}", "Island"),
    ] {
        assert_eq!(
            rewrite_text_words(&s, BLT, from, to),
            Err(TextWordError::InvalidWord),
            "{from:?} -> {to:?}"
        );
    }
}

// REQ-DSL-012; CR 612.2: CARD_NAME is unsupported, checked before the words.
#[test]
fn rewrite_rejects_card_name_category_first() {
    let s = sample();
    let cn = RewritableCategory::CardName;
    assert_eq!(
        rewrite_text_words(&s, cn, "Swamp", "Island"),
        Err(TextWordError::UnsupportedCategory)
    );
    assert_eq!(
        rewrite_text_words(&s, cn, "", ""),
        Err(TextWordError::UnsupportedCategory)
    );
}

// REQ-DSL-012: every error displays a non-empty message and is a std error.
#[test]
fn errors_display() {
    let all = [
        TextWordError::UnknownToken("t1".to_owned()),
        TextWordError::NotAWord("t1".to_owned()),
        TextWordError::InvalidRulesText("x".to_owned()),
        TextWordError::InvalidWord,
        TextWordError::UnsupportedCategory,
    ];
    for err in all {
        let as_error: &dyn std::error::Error = &err;
        assert!(!as_error.to_string().is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// decode / encode
// ---------------------------------------------------------------------------------------------

// REQ-DSL-012: decode(encode(s)) == s; the empty stream and the empty placeholder correspond.
#[test]
fn encode_decode_round_trip() {
    let s = sample();
    assert_eq!(decode_rules_text(&encode_rules_text(&s)).unwrap(), s);
    let empty = TextTokenStream::default();
    assert_eq!(encode_rules_text(&empty), card::TextTokenStream(Vec::new()));
    assert_eq!(
        decode_rules_text(&card::TextTokenStream(Vec::new())).unwrap(),
        empty
    );
}

/// A placeholder element: a JSON object with these string members, in this order.
fn element<V: FromIterator<(String, V)> + From<String>>(members: &[(&str, &str)]) -> V {
    members
        .iter()
        .map(|(k, v)| ((*k).to_owned(), V::from((*v).to_owned())))
        .collect()
}

// REQ-DSL-012, R12: placeholder content that is not a valid stream is InvalidRulesText.
#[test]
fn decode_rejects_placeholder_garbage() {
    let garbage = [
        card::TextTokenStream(vec!["test.word".into()]),
        card::TextTokenStream(vec![7_i64.into()]),
        card::TextTokenStream(vec![element(&[
            ("id", "t1"),
            ("kind", "WORD"),
            ("category", "BASIC_LAND_TYPE"),
        ])]),
        // A duplicate id.
        card::TextTokenStream(vec![
            element(&[("id", "t1"), ("kind", "LITERAL"), ("text", "x")]),
            element(&[("id", "t1"), ("kind", "LITERAL"), ("text", "y")]),
        ]),
    ];
    for g in garbage {
        assert!(
            matches!(decode_rules_text(&g), Err(TextWordError::InvalidRulesText(m)) if !m.is_empty()),
            "{g:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// resolve_word_ref / text_token_matches
// ---------------------------------------------------------------------------------------------

// REQ-DSL-012; CR 201.5: names resolve to themselves, words to their current value (CARD_NAME
// included); unknown ids and non-words are errors.
#[test]
fn resolve_word_ref_cases() {
    let s = sample();
    assert_eq!(resolve_word_ref(&s, &name("Elf")).unwrap(), "Elf");
    assert_eq!(resolve_word_ref(&s, &tref("t2")).unwrap(), "Swamp");
    assert_eq!(resolve_word_ref(&s, &tref("t6")).unwrap(), "Swamp");
    assert_eq!(
        resolve_word_ref(&s, &tref("nope")),
        Err(TextWordError::UnknownToken("nope".to_owned()))
    );
    for id in ["t1", "t4", "t5"] {
        assert_eq!(
            resolve_word_ref(&s, &tref(id)),
            Err(TextWordError::NotAWord(id.to_owned()))
        );
    }
    let r = rewrite_text_words(&s, BLT, "Swamp", "Island").unwrap();
    assert_eq!(resolve_word_ref(&r, &tref("t2")).unwrap(), "Island");
}

// REQ-DSL-012: TEXT_TOKEN_MATCH reads the current value.
#[test]
fn text_token_matches_cases() {
    let s = sample();
    assert_eq!(text_token_matches(&s, "t2", "Swamp"), Ok(true));
    assert_eq!(text_token_matches(&s, "t2", "Island"), Ok(false));
    assert_eq!(text_token_matches(&s, "t6", "Swamp"), Ok(true));
    let r = rewrite_text_words(&s, BLT, "Swamp", "Island").unwrap();
    assert_eq!(text_token_matches(&r, "t2", "Island"), Ok(true));
    assert_eq!(
        text_token_matches(&s, "zz", "Swamp"),
        Err(TextWordError::UnknownToken("zz".to_owned()))
    );
    assert_eq!(
        text_token_matches(&s, "t4", "Swamp"),
        Err(TextWordError::NotAWord("t4".to_owned()))
    );
}

// ---------------------------------------------------------------------------------------------
// resolve_predicate_words / resolve_selector_words
// ---------------------------------------------------------------------------------------------

fn devotion(words: Vec<WordRef>) -> Box<ValueExpressionAST> {
    Box::new(ValueExpressionAST::PlayerAttr {
        reference: ReferenceAST::Controller,
        attr: PlayerAttr::Devotion(words),
    })
}

fn lit_v(n: i64) -> Box<ValueExpressionAST> {
    Box::new(ValueExpressionAST::Literal(n))
}

// REQ-DSL-012; CR 201.5: nested AND/OR/NOT and the four HAS_* leaves resolve; names,
// TEXT_TOKEN_MATCH and SAME_AS SELF stay.
#[test]
fn resolve_predicate_nested_boolean_and_leaves() {
    let s = sample();
    let pred = PredicateAST::And(vec![
        PredicateAST::Or(vec![
            PredicateAST::HasSubtype { value: tref("t7") },
            PredicateAST::Not(Box::new(PredicateAST::HasColor { value: tref("t3") })),
        ]),
        PredicateAST::HasCardType {
            value: name("Creature"),
        },
        PredicateAST::HasSupertype { value: tref("t8") },
        PredicateAST::TextTokenMatch {
            text_token: "t2".to_owned(),
            value: "Swamp".to_owned(),
        },
        PredicateAST::SameAs {
            reference: ReferenceAST::SelfRef,
        },
        PredicateAST::IsCreature,
    ]);
    let expected = PredicateAST::And(vec![
        PredicateAST::Or(vec![
            PredicateAST::HasSubtype {
                value: name("Goblin"),
            },
            PredicateAST::Not(Box::new(PredicateAST::HasColor {
                value: name("black"),
            })),
        ]),
        PredicateAST::HasCardType {
            value: name("Creature"),
        },
        PredicateAST::HasSupertype { value: name("2") },
        PredicateAST::TextTokenMatch {
            text_token: "t2".to_owned(),
            value: "Swamp".to_owned(),
        },
        PredicateAST::SameAs {
            reference: ReferenceAST::SelfRef,
        },
        PredicateAST::IsCreature,
    ]);
    let got = resolve_predicate_words(&pred, &s).unwrap();
    assert_eq!(got, expected);
    assert!(!has_token_ref(&got));
}

/// A filter whose count is a devotion to `w` and whose predicate is HAS_SUBTYPE `w`.
fn inner(w: &WordRef) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: Some(devotion(vec![w.clone()])),
        ordering: None,
        predicate: Some(Box::new(PredicateAST::HasSubtype { value: w.clone() })),
    })
}

/// A value expression reaching `w` through MATH and the three history queries with words.
fn history(w: &WordRef) -> Box<ValueExpressionAST> {
    Box::new(ValueExpressionAST::Math {
        op: MathOp::Add,
        lhs: Box::new(ValueExpressionAST::History(Box::new(
            HistoryQuery::SpellsCastThisTurn {
                player: None,
                condition: Some(PredicateAST::HasColor { value: w.clone() }),
            },
        ))),
        rhs: Box::new(ValueExpressionAST::Math {
            op: MathOp::Max,
            lhs: Box::new(ValueExpressionAST::History(Box::new(
                HistoryQuery::CreaturesDiedThisTurn {
                    condition: Some(PredicateAST::HasSubtype { value: w.clone() }),
                },
            ))),
            rhs: Box::new(ValueExpressionAST::History(Box::new(
                HistoryQuery::ManaSpentToCast {
                    reference: ReferenceAST::SelfRef,
                    color: Some(w.clone()),
                },
            ))),
        }),
    })
}

/// A predicate reaching `w` through CONTROLS, selector counts and predicates, and value
/// expressions (devotion, count, distinct, history, counters).
fn deep_predicate(w: &WordRef) -> PredicateAST {
    PredicateAST::And(vec![
        PredicateAST::Controls {
            selector: Box::new(inner(w)),
            player: Some(PlayerSelectorAST::Players(PlayerGroup::AllPlayers)),
        },
        PredicateAST::Compare {
            cmp: Cmp::Ge,
            lhs: devotion(vec![name("blue"), w.clone()]),
            rhs: Box::new(ValueExpressionAST::Count(Box::new(inner(w)))),
        },
        PredicateAST::ManaValue {
            cmp: Cmp::Eq,
            value: Box::new(ValueExpressionAST::Distinct {
                attr: DistinctAttr::Subtype,
                selector: Box::new(inner(w)),
            }),
        },
        PredicateAST::Power {
            cmp: Cmp::Lt,
            value: history(w),
        },
        PredicateAST::Toughness {
            cmp: Cmp::Gt,
            value: devotion(vec![w.clone()]),
        },
        PredicateAST::HasCounter {
            kind: "test.counter".to_owned(),
            compare: Some((Cmp::Ne, devotion(vec![w.clone()]))),
        },
        PredicateAST::HasCounter {
            kind: "test.counter".to_owned(),
            compare: None,
        },
        PredicateAST::Compare {
            cmp: Cmp::Le,
            lhs: lit_v(1),
            rhs: Box::new(ValueExpressionAST::History(Box::new(
                HistoryQuery::ManaSpentToCast {
                    reference: ReferenceAST::SelfRef,
                    color: None,
                },
            ))),
        },
        PredicateAST::Compare {
            cmp: Cmp::Le,
            lhs: Box::new(ValueExpressionAST::History(Box::new(
                HistoryQuery::LifeLostThisTurn {
                    player: PlayerSelectorAST::Ref(ReferenceAST::Controller),
                },
            ))),
            rhs: Box::new(ValueExpressionAST::Math {
                op: MathOp::Sub,
                lhs: lit_v(3),
                rhs: Box::new(ValueExpressionAST::X),
            }),
        },
        PredicateAST::ControlledBy {
            player: PlayerSelectorAST::Ref(ReferenceAST::Controller),
        },
    ])
}

// REQ-DSL-012: CONTROLS selectors, selector predicates and counts, and value expressions
// (devotion, counts, math, distinct, history, counters, comparisons) resolve.
#[test]
fn resolve_predicate_reaches_selectors_and_values() {
    let s = sample();
    let got = resolve_predicate_words(&deep_predicate(&tref("t7")), &s).unwrap();
    assert_eq!(got, deep_predicate(&name("Goblin")));
    assert!(!has_token_ref(&got));
    // An unknown id anywhere along these paths is an error.
    assert_eq!(
        resolve_predicate_words(&deep_predicate(&tref("missing")), &s),
        Err(TextWordError::UnknownToken("missing".to_owned()))
    );
}

// REQ-DSL-012: each nested path reports its own unresolvable token.
#[test]
fn resolve_predicate_each_path_errors() {
    let s = sample();
    let bad = || tref("t1"); // a literal: NotAWord
    let err = Err(TextWordError::NotAWord("t1".to_owned()));
    let cases = vec![
        PredicateAST::Not(Box::new(PredicateAST::HasCardType { value: bad() })),
        PredicateAST::Or(vec![PredicateAST::HasSupertype { value: bad() }]),
        PredicateAST::Power {
            cmp: Cmp::Eq,
            value: devotion(vec![bad()]),
        },
        PredicateAST::HasCounter {
            kind: "k".to_owned(),
            compare: Some((Cmp::Eq, devotion(vec![bad()]))),
        },
        PredicateAST::ManaValue {
            cmp: Cmp::Eq,
            value: Box::new(ValueExpressionAST::History(Box::new(
                HistoryQuery::ManaSpentToCast {
                    reference: ReferenceAST::SelfRef,
                    color: Some(bad()),
                },
            ))),
        },
        PredicateAST::Toughness {
            cmp: Cmp::Eq,
            value: Box::new(ValueExpressionAST::History(Box::new(
                HistoryQuery::SpellsCastThisTurn {
                    player: None,
                    condition: Some(PredicateAST::HasColor { value: bad() }),
                },
            ))),
        },
        PredicateAST::Compare {
            cmp: Cmp::Eq,
            lhs: lit_v(0),
            rhs: Box::new(ValueExpressionAST::History(Box::new(
                HistoryQuery::CreaturesDiedThisTurn {
                    condition: Some(PredicateAST::HasColor { value: bad() }),
                },
            ))),
        },
        PredicateAST::Compare {
            cmp: Cmp::Eq,
            lhs: Box::new(ValueExpressionAST::Math {
                op: MathOp::Mul,
                lhs: lit_v(0),
                rhs: Box::new(ValueExpressionAST::Distinct {
                    attr: DistinctAttr::Color,
                    selector: Box::new(filter(PredicateAST::HasColor { value: bad() })),
                }),
            }),
            rhs: lit_v(0),
        },
        PredicateAST::Controls {
            selector: Box::new(ObjectSelectorAST::Filter(SelectorFilter {
                zones: None,
                count: Some(devotion(vec![bad()])),
                ordering: None,
                predicate: None,
            })),
            player: None,
        },
    ];
    for c in cases {
        assert_eq!(resolve_predicate_words(&c, &s), err, "{c:?}");
    }
}

// REQ-DSL-012: the first unresolvable token in walk order is reported.
#[test]
fn resolve_reports_first_unknown_token() {
    let s = sample();
    let pred = PredicateAST::And(vec![
        PredicateAST::HasSubtype {
            value: tref("first"),
        },
        PredicateAST::HasColor {
            value: tref("second"),
        },
    ]);
    assert_eq!(
        resolve_predicate_words(&pred, &s),
        Err(TextWordError::UnknownToken("first".to_owned()))
    );
}

// REQ-DSL-012; CR 201.5: a reference selector ({"ref":"SELF"}) is never touched; a filter's
// predicate and count resolve; a filter without either stays.
#[test]
fn resolve_selector_cases() {
    let s = sample();
    let self_sel = ObjectSelectorAST::Ref(ReferenceAST::SelfRef);
    assert_eq!(resolve_selector_words(&self_sel, &s).unwrap(), self_sel);
    let sel = filter(PredicateAST::HasSubtype { value: tref("t7") });
    let got = resolve_selector_words(&sel, &s).unwrap();
    assert_eq!(
        got,
        filter(PredicateAST::HasSubtype {
            value: name("Goblin")
        })
    );
    assert!(!has_token_ref(&got));
    let bare = ObjectSelectorAST::Filter(SelectorFilter {
        zones: Some(vec![fineprint_dsl::expr::Zone::Battlefield]),
        count: None,
        ordering: None,
        predicate: None,
    });
    assert_eq!(resolve_selector_words(&bare, &s).unwrap(), bare);
    assert_eq!(
        resolve_selector_words(&filter(PredicateAST::HasSubtype { value: tref("t4") }), &s),
        Err(TextWordError::NotAWord("t4".to_owned()))
    );
}

// ---------------------------------------------------------------------------------------------
// Layer pipeline
// ---------------------------------------------------------------------------------------------

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A synthetic creature `name` with `subtypes` and the given rules text.
fn object(name: &str, subtypes: &[&str], text: card::TextTokenStream) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::BLACK,
        supertypes: BTreeSet::new(),
        card_types: set(&["Creature"]),
        subtypes: set(subtypes),
        rules_text: text,
        abilities: Vec::new(),
        power: Some(literal(2)),
        toughness: Some(literal(2)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// The rules text "Destroy target [Swamp word t2]. [SELF] ..." used by the pipeline tests.
fn swamp_text() -> TextTokenStream {
    stream(vec![
        lit("t1", "Destroy target "),
        word("t2", BLT, "Swamp"),
        self_ref("t3"),
        other_ref("t4", "Swamp"),
        word("t5", RewritableCategory::CardName, "Swamp"),
    ])
}

#[derive(Clone, Default)]
struct Env {
    bases: BTreeMap<ObjectId, ObjectBase>,
}

impl Env {
    fn with(mut self, id: ObjectId, c: Characteristics) -> Env {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(PlayerId::new(1)),
                on_battlefield: true,
            },
        );
        self
    }
}

/// Whether `pred` holds for `c`, for word references already resolved to names.
fn holds(pred: &PredicateAST, c: &Characteristics) -> bool {
    match pred {
        PredicateAST::And(all) => all.iter().all(|p| holds(p, c)),
        PredicateAST::IsCreature => c.card_types.contains("Creature"),
        PredicateAST::HasSubtype {
            value: WordRef::Name(t),
        } => c.subtypes.contains(t),
        _ => false,
    }
}

impl LayerEnvironment for Env {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        match selector {
            ObjectSelectorAST::Ref(ReferenceAST::SelfRef) => ctx.source == Some(ctx.candidate),
            ObjectSelectorAST::Filter(f) => f
                .predicate
                .as_ref()
                .is_some_and(|p| holds(p, &ctx.candidate_view.characteristics)),
            ObjectSelectorAST::Ref(_) => false,
        }
    }
}

fn replace(category: RewritableCategory, from: &str, to: &str) -> LayerModification {
    LayerModification::ReplaceTextWord {
        category,
        from: from.to_owned(),
        to: to.to_owned(),
    }
}

fn only(ids: &[u64]) -> Affected {
    Affected::Objects(ids.iter().map(|&n| o(n)).collect())
}

fn eff(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: PlayerId::new(1),
        timestamp: ts(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected,
        modification: m,
    }
}

fn l3(t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    eff(Layer::L3, t, affected, m)
}

fn registry(list: Vec<(u64, ContinuousEffect)>) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    for (id, effect) in list {
        r.register(e(id), effect).unwrap();
    }
    r
}

fn text_of(r: &ContinuousEffects, env: &Env, id: ObjectId) -> TextTokenStream {
    decode_rules_text(&evaluate(r, env, id).unwrap().characteristics.rules_text).unwrap()
}

fn two_objects() -> Env {
    Env::default()
        .with(
            o(1),
            object("test.bear_1", &["Bear"], encode_rules_text(&swamp_text())),
        )
        .with(
            o(2),
            object("test.wall_2", &["Wall"], encode_rules_text(&swamp_text())),
        )
}

// REQ-DSL-012, REQ-LAYER-001; CR 612.1, 612.2, 613.1c, 201.5: the evaluated table is
// rewritten; self-references, OTHER name references, literals and card-name words are not.
#[test]
fn layer3_rewrites_the_table() {
    let env = two_objects();
    let r = registry(vec![(
        1,
        l3(1, only(&[1]), replace(BLT, "Swamp", "Island")),
    )]);
    let mut expected = swamp_text().tokens().to_vec();
    expected[1] = word("t2", BLT, "Island");
    assert_eq!(text_of(&r, &env, o(1)).tokens(), expected.as_slice());
    // The base is never mutated.
    assert_eq!(
        env.bases[&o(1)].characteristics.rules_text,
        encode_rules_text(&swamp_text())
    );
}

// REQ-DSL-012, REQ-LAYER-001; CR 611.2c: the effect leaves objects outside its locked set
// unchanged.
#[test]
fn layer3_respects_locked_set() {
    let env = two_objects();
    let r = registry(vec![(
        1,
        l3(1, only(&[1]), replace(BLT, "Swamp", "Island")),
    )]);
    assert_eq!(
        evaluate(&r, &env, o(2)).unwrap(),
        Evaluated {
            characteristics: env.bases[&o(2)].characteristics.clone(),
            controller: Some(PlayerId::new(1)),
        }
    );
}

// REQ-DSL-012; CR 611.3a, 613.1c: a selector-affected text effect changes matching objects
// only.
#[test]
fn layer3_selector_affected() {
    let env = two_objects();
    let bears = filter(PredicateAST::HasSubtype {
        value: name("Bear"),
    });
    let r = registry(vec![(
        1,
        ContinuousEffect {
            origin: EffectOrigin::Static { granted_at: None },
            ..l3(
                1,
                Affected::Selector(bears),
                replace(BLT, "Swamp", "Forest"),
            )
        },
    )]);
    assert_eq!(value_of(&text_of(&r, &env, o(1)), "t2"), "Forest");
    assert_eq!(value_of(&text_of(&r, &env, o(2)), "t2"), "Swamp");
}

// REQ-DSL-012, R12; CR 612.1: undecodable and empty rules text are no-ops and evaluation is Ok.
#[test]
fn layer3_undecodable_or_empty_text_is_noop() {
    for text in [
        card::TextTokenStream(vec!["test.word".into()]),
        card::TextTokenStream(Vec::new()),
    ] {
        let env = Env::default().with(o(1), object("test.bear_1", &["Bear"], text.clone()));
        let r = registry(vec![(
            1,
            l3(1, only(&[1]), replace(BLT, "Swamp", "Island")),
        )]);
        let got = evaluate(&r, &env, o(1)).unwrap();
        assert_eq!(got.characteristics.rules_text, text);
    }
}

// REQ-DSL-012; CR 612.2: with no matching word (and with from == to) the text is left
// byte-identical, not re-encoded (members deliberately not in wire order).
#[test]
fn layer3_no_change_leaves_text_identical() {
    let text = card::TextTokenStream(vec![element(&[
        ("value", "Swamp"),
        ("kind", "WORD"),
        ("id", "t1"),
        ("category", "BASIC_LAND_TYPE"),
    ])]);
    assert!(decode_rules_text(&text).is_ok());
    let env = Env::default().with(o(1), object("test.bear_1", &["Bear"], text.clone()));
    for m in [
        replace(BLT, "Forest", "Island"),
        replace(BLT, "Swamp", "Swamp"),
        replace(RewritableCategory::ColorWord, "Swamp", "Island"),
    ] {
        let r = registry(vec![(1, l3(1, only(&[1]), m))]);
        assert_eq!(
            evaluate(&r, &env, o(1)).unwrap().characteristics.rules_text,
            text
        );
    }
}

/// A placeholder ability whose content mentions the replaced word.
fn ability_mentioning(word_value: &str) -> AbilityAST {
    AbilityAST(element(&[
        ("kind", "test.ability"),
        ("category", "BASIC_LAND_TYPE"),
        ("value", word_value),
    ]))
}

// REQ-DSL-012; CR 612.3: the effect never changes abilities, printed or granted in layer 6.
#[test]
fn layer3_never_touches_abilities() {
    let mut c = object("test.bear_1", &["Bear"], encode_rules_text(&swamp_text()));
    c.abilities = vec![ability_mentioning("Swamp")];
    let env = Env::default().with(o(1), c);
    let r = registry(vec![
        (
            1,
            eff(
                Layer::L6,
                1,
                only(&[1]),
                LayerModification::AddAbility(ability_mentioning("Swamp")),
            ),
        ),
        (2, l3(2, only(&[1]), replace(BLT, "Swamp", "Island"))),
    ]);
    let got = evaluate(&r, &env, o(1)).unwrap().characteristics;
    assert_eq!(
        got.abilities,
        vec![ability_mentioning("Swamp"), ability_mentioning("Swamp")]
    );
    assert_eq!(
        value_of(&decode_rules_text(&got.rules_text).unwrap(), "t2"),
        "Island"
    );
}

// REQ-DSL-012, REQ-LAYER-001; CR 613.1c: the modification belongs to layer 3 only.
#[test]
fn register_outside_layer3_is_mismatch() {
    for layer in [Layer::L4, Layer::L1a, Layer::L6] {
        let mut r = ContinuousEffects::new();
        assert_eq!(
            r.register(
                e(1),
                eff(layer, 1, only(&[1]), replace(BLT, "Swamp", "Island"))
            ),
            Err(LayerError::LayerMismatch { layer })
        );
        assert!(r.is_empty());
    }
}

fn invalid_replacements() -> Vec<LayerModification> {
    vec![
        replace(BLT, "", "Island"),
        replace(BLT, "Swamp", ""),
        replace(BLT, "Sw\u{FDEF}amp", "Island"),
        replace(BLT, "Swamp", "\u{FFFF}"),
        replace(RewritableCategory::CardName, "Swamp", "Island"),
    ]
}

/// A registry with one valid effect, to show that a failed registration leaves it unchanged.
fn seeded() -> ContinuousEffects {
    registry(vec![(
        9,
        l3(1, only(&[1]), replace(BLT, "Swamp", "Forest")),
    )])
}

// REQ-DSL-012; CR 612.2: register rejects empty or noncharacter words and CARD_NAME with
// Unsupported, registry unchanged.
#[test]
fn register_rejects_invalid_replacements() {
    for m in invalid_replacements() {
        let mut r = seeded();
        let got = r.register(e(1), l3(2, only(&[1]), m.clone()));
        assert!(
            matches!(got, Err(LayerError::Unsupported(msg)) if !msg.is_empty()),
            "{m:?}"
        );
        assert_eq!(r, seeded());
    }
}

// REQ-DSL-012; CR 612.2: register_group and register_group_for_as_long_as reject the same
// effects, all or nothing.
#[test]
fn register_group_rejects_invalid_replacements() {
    let env = two_objects();
    let window = ForAsLongAsWindow {
        starts: true,
        ended_before_first_application: false,
        begins_again: false,
    };
    for m in invalid_replacements() {
        let parts = vec![
            (e(1), l3(2, only(&[1]), replace(BLT, "Swamp", "Island"))),
            (e(2), l3(2, only(&[1]), m.clone())),
        ];
        let mut r = seeded();
        assert!(
            matches!(
                r.register_group(&env, parts.clone()),
                Err(LayerError::Unsupported(_))
            ),
            "{m:?}"
        );
        assert_eq!(r, seeded());
        assert!(
            matches!(
                r.register_group_for_as_long_as(&env, window, parts),
                Err(LayerError::Unsupported(_))
            ),
            "{m:?}"
        );
        assert_eq!(r, seeded());
    }
}

// REQ-DSL-012; CR 612.2, 612.2a: the four supported categories are accepted with locked sets
// and selectors, through every registration path.
#[test]
fn register_accepts_supported_categories() {
    let env = two_objects();
    let window = ForAsLongAsWindow {
        starts: true,
        ended_before_first_application: false,
        begins_again: false,
    };
    let sel = filter(PredicateAST::IsCreature);
    for category in SUPPORTED {
        let m = replace(category, "Swamp", "Island");
        let mut r = ContinuousEffects::new();
        r.register(e(1), l3(1, only(&[1]), m.clone())).unwrap();
        r.register(e(2), l3(1, Affected::Selector(sel.clone()), m.clone()))
            .unwrap();
        r.register_group(
            &env,
            vec![(e(3), l3(1, Affected::Selector(sel.clone()), m.clone()))],
        )
        .unwrap();
        assert_eq!(
            r.register_group_for_as_long_as(&env, window, vec![(e(4), l3(1, only(&[2]), m))]),
            Ok(true)
        );
        assert_eq!(r.len(), 4);
        // A resolved group part with a selector was locked to the matching creatures.
        assert_eq!(r.get(e(3)).unwrap().affected, only(&[1, 2]));
    }
}

/// A shared scene: three objects and several text effects.
fn scene() -> (Vec<(u64, ContinuousEffect)>, Env) {
    let env = two_objects().with(
        o(3),
        object("test.elf_3", &["Elf"], encode_rules_text(&swamp_text())),
    );
    let list = vec![
        (1, l3(5, only(&[1, 3]), replace(BLT, "Swamp", "Island"))),
        (2, l3(3, only(&[2]), replace(BLT, "Swamp", "Mountain"))),
        (3, l3(7, only(&[1, 2]), replace(BLT, "Island", "Plains"))),
        (
            4,
            eff(
                Layer::L7c,
                1,
                only(&[3]),
                LayerModification::ModifyPt {
                    power: 1,
                    toughness: 1,
                },
            ),
        ),
    ];
    (list, env)
}

/// A small deterministic linear congruential generator (no rand crate, no clock).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % (n as u64)).unwrap()
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

// REQ-CORE-001, REQ-LAYER-001, REQ-DSL-012: two evaluations are equal, and registration order
// does not change the result (CR 613.7: timestamp order, then EffectId).
#[test]
fn deterministic_and_registration_order_irrelevant() {
    let (list, env) = scene();
    let reference = evaluate_all(&registry(list.clone()), &env).unwrap();
    assert_eq!(
        evaluate_all(&registry(list.clone()), &env).unwrap(),
        reference
    );
    let r = registry(list.clone());
    assert_eq!(value_of(&text_of(&r, &env, o(1)), "t2"), "Plains");
    assert_eq!(value_of(&text_of(&r, &env, o(2)), "t2"), "Mountain");
    assert_eq!(value_of(&text_of(&r, &env, o(3)), "t2"), "Island");
    let mut rng = Lcg(37);
    for _ in 0..50 {
        let mut shuffled = list.clone();
        for i in (1..shuffled.len()).rev() {
            let j = rng.below(i + 1);
            shuffled.swap(i, j);
        }
        assert_eq!(evaluate_all(&registry(shuffled), &env).unwrap(), reference);
    }
}

// REQ-LAYER-005, REQ-DSL-012: the cache gives exactly the uncached evaluation, and the digests
// agree.
#[test]
fn cache_equals_uncached() {
    let (list, env) = scene();
    let r = registry(list);
    let full = evaluate_all(&r, &env).unwrap();
    let mut cache = LayerCache::new();
    let first = cache.evaluate_all(&r, &env).unwrap();
    let second = cache.evaluate_all(&r, &env).unwrap();
    assert_eq!(first, full);
    assert_eq!(second, full);
    assert_eq!(evaluated_digest(&second), evaluated_digest(&full));
    for id in [o(1), o(2), o(3)] {
        assert_eq!(cache.evaluate(&r, &env, id).unwrap(), full[&id]);
    }
}

// REQ-DSL-012, REQ-LAYER-002; CR 613.8a, 613.8b: on text with a Swamp word and no Island word,
// B = Island->Forest depends on A = Swamp->Island (applying A changes what B applies to), so
// the Swamp word ends as Forest in both timestamp orders.
#[test]
fn chained_effects_follow_dependency_in_both_orders() {
    let env = Env::default().with(
        o(1),
        object("test.bear_1", &["Bear"], encode_rules_text(&swamp_text())),
    );
    for (ta, tb) in [(1, 2), (2, 1)] {
        let r = registry(vec![
            (1, l3(ta, only(&[1]), replace(BLT, "Swamp", "Island"))),
            (2, l3(tb, only(&[1]), replace(BLT, "Island", "Forest"))),
        ]);
        assert_eq!(
            value_of(&text_of(&r, &env, o(1)), "t2"),
            "Forest",
            "A at {ta}"
        );
    }
}

// REQ-DSL-012, REQ-LAYER-002; CR 613.8b, 613.7: A = Swamp->Island and B = Swamp->Forest depend
// on each other (each one applied first makes the other do nothing): a loop applied in
// timestamp order, so the earlier timestamp's `to` wins, in both orders.
#[test]
fn mutually_dependent_effects_apply_in_timestamp_order() {
    let env = Env::default().with(
        o(1),
        object("test.bear_1", &["Bear"], encode_rules_text(&swamp_text())),
    );
    for (ta, tb, expected) in [(1, 2, "Island"), (2, 1, "Forest")] {
        let r = registry(vec![
            (1, l3(ta, only(&[1]), replace(BLT, "Swamp", "Island"))),
            (2, l3(tb, only(&[1]), replace(BLT, "Swamp", "Forest"))),
        ]);
        assert_eq!(value_of(&text_of(&r, &env, o(1)), "t2"), expected);
    }
}

// REQ-DSL-012, REQ-LAYER-002; CR 613.7, 613.1c: SetText earlier then ReplaceTextWord rewrites
// the new text; ReplaceTextWord earlier then SetText (whose text has no Swamp word) leaves the
// SetText text exactly, although the replacement changed the base's Swamp word. (A later
// SetText whose text holds the replaced word is a dependency question, CR 613.8a: see
// docs/DECISIONS.md "#37 FP-036 — Dependency between text effects".)
#[test]
fn set_text_and_replace_follow_timestamps() {
    let env = Env::default().with(
        o(1),
        object(
            "test.bear_1",
            &["Bear"],
            encode_rules_text(&stream(vec![word("x1", BLT, "Swamp")])),
        ),
    );
    // SetText at 1, replacement at 2: the new text is rewritten.
    let with_swamp = encode_rules_text(&swamp_text());
    let r = registry(vec![
        (1, l3(1, only(&[1]), LayerModification::SetText(with_swamp))),
        (2, l3(2, only(&[1]), replace(BLT, "Swamp", "Island"))),
    ]);
    let mut expected = swamp_text().tokens().to_vec();
    expected[1] = word("t2", BLT, "Island");
    assert_eq!(text_of(&r, &env, o(1)).tokens(), expected.as_slice());
    // Replacement at 1, SetText at 2: the SetText text wins.
    let forest = encode_rules_text(&stream(vec![word("t2", BLT, "Forest")]));
    let r = registry(vec![
        (1, l3(1, only(&[1]), replace(BLT, "Swamp", "Island"))),
        (
            2,
            l3(2, only(&[1]), LayerModification::SetText(forest.clone())),
        ),
    ]);
    assert_eq!(
        evaluate(&r, &env, o(1)).unwrap().characteristics.rules_text,
        forest
    );
    // Without the SetText, the replacement does change the base word.
    let r = registry(vec![(
        1,
        l3(1, only(&[1]), replace(BLT, "Swamp", "Island")),
    )]);
    assert_eq!(value_of(&text_of(&r, &env, o(1)), "x1"), "Island");
}

// ---------------------------------------------------------------------------------------------
// Acceptance: a dependent ability applies to different objects after a text change
// ---------------------------------------------------------------------------------------------

/// The objects the source's dependent selector picks: the selector is resolved against the
/// source's evaluated table, then matched against every evaluated object.
fn selected(
    r: &ContinuousEffects,
    env: &Env,
    source: ObjectId,
    sel: &ObjectSelectorAST,
) -> Vec<ObjectId> {
    let table = text_of(r, env, source);
    let resolved = resolve_selector_words(sel, &table).unwrap();
    let ObjectSelectorAST::Filter(f) = &resolved else {
        panic!("filter expected");
    };
    let pred = f.predicate.as_ref().unwrap();
    evaluate_all(r, env)
        .unwrap()
        .into_iter()
        .filter(|(_, v)| holds(pred, &v.characteristics))
        .map(|(id, _)| id)
        .collect()
}

// REQ-DSL-012; CR 612.1, 613.1c, 201.5: "creatures of type [Goblin word] other than [SELF]"
// selects the Goblin before the text change and the Elf after Goblin->Elf; the source's
// self-reference is not rewritten.
#[test]
fn acceptance_text_change_changes_what_dependent_ability_applies_to() {
    let source_text = stream(vec![
        lit("a1", "Creatures of type "),
        word("a2", RewritableCategory::CreatureType, "Goblin"),
        lit("a3", " other than "),
        self_ref("a4"),
        lit("a5", " get +1/+1."),
    ]);
    let env = Env::default()
        .with(
            o(1),
            object("test.lord_1", &["Wizard"], encode_rules_text(&source_text)),
        )
        .with(
            o(2),
            object(
                "test.goblin_2",
                &["Goblin"],
                card::TextTokenStream(Vec::new()),
            ),
        )
        .with(
            o(3),
            object("test.elf_3", &["Elf"], card::TextTokenStream(Vec::new())),
        );
    let dependent = filter(PredicateAST::And(vec![
        PredicateAST::IsCreature,
        PredicateAST::HasSubtype { value: tref("a2") },
    ]));
    let before = ContinuousEffects::new();
    assert_eq!(selected(&before, &env, o(1), &dependent), vec![o(2)]);
    let after = registry(vec![(
        1,
        l3(
            1,
            only(&[1]),
            replace(RewritableCategory::CreatureType, "Goblin", "Elf"),
        ),
    )]);
    assert_eq!(selected(&after, &env, o(1), &dependent), vec![o(3)]);
    let table = text_of(&after, &env, o(1));
    assert_eq!(table.tokens()[3], self_ref("a4"));
    assert_eq!(text_token_matches(&table, "a2", "Elf"), Ok(true));
}

// ---------------------------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------------------------

const WORDS: [&str; 6] = ["Swamp", "Island", "Goblin", "Elf", "black", "2"];

fn random_stream(rng: &mut Lcg) -> TextTokenStream {
    let n = rng.below(8);
    let tokens = (0..n)
        .map(|i| {
            let id = format!("t{i}");
            let w = *rng.pick(&WORDS);
            match rng.below(4) {
                0 => lit(&id, w),
                1 => self_ref(&id),
                2 => other_ref(&id, w),
                _ => word(&id, *rng.pick(RewritableCategory::ALL), w),
            }
        })
        .collect();
    stream(tokens)
}

// REQ-DSL-012; CR 612.2: over random streams, a rewrite keeps ids, order and kinds, changes
// exactly the matching words of the category, and decode(encode(s)) == s.
#[test]
fn property_rewrite_and_round_trip() {
    let mut rng = Lcg(612);
    for _ in 0..500 {
        let s = random_stream(&mut rng);
        assert_eq!(decode_rules_text(&encode_rules_text(&s)).unwrap(), s);
        let category = *rng.pick(&SUPPORTED);
        let from = *rng.pick(&WORDS);
        let to = *rng.pick(&WORDS);
        let r = rewrite_text_words(&s, category, from, to).unwrap();
        assert_eq!(r.len(), s.len());
        for (a, b) in s.tokens().iter().zip(r.tokens()) {
            assert_eq!(a.id(), b.id());
            match (a, b) {
                (
                    TextToken::Word {
                        category: ca,
                        value: va,
                        ..
                    },
                    TextToken::Word {
                        category: cb,
                        value: vb,
                        ..
                    },
                ) => {
                    assert_eq!(ca, cb);
                    if *ca == category && va == from {
                        assert_eq!(vb, to);
                    } else {
                        assert_eq!(va, vb);
                    }
                }
                _ => assert_eq!(a, b),
            }
        }
    }
}

fn random_pred(rng: &mut Lcg, ids: &[String], depth: u32) -> PredicateAST {
    let w = |rng: &mut Lcg| {
        if ids.is_empty() || rng.below(3) == 0 {
            name("Elf")
        } else {
            tref(rng.pick(ids))
        }
    };
    let choice = if depth == 0 {
        rng.below(4)
    } else {
        rng.below(8)
    };
    match choice {
        0 => PredicateAST::HasSubtype { value: w(rng) },
        1 => PredicateAST::HasColor { value: w(rng) },
        2 => PredicateAST::HasCardType { value: w(rng) },
        3 => PredicateAST::HasSupertype { value: w(rng) },
        4 => PredicateAST::And(vec![
            random_pred(rng, ids, depth - 1),
            random_pred(rng, ids, depth - 1),
        ]),
        5 => PredicateAST::Not(Box::new(random_pred(rng, ids, depth - 1))),
        6 => PredicateAST::Controls {
            selector: Box::new(filter(random_pred(rng, ids, depth - 1))),
            player: None,
        },
        _ => PredicateAST::Compare {
            cmp: Cmp::Ge,
            lhs: devotion(vec![w(rng)]),
            rhs: Box::new(ValueExpressionAST::Count(Box::new(filter(random_pred(
                rng,
                ids,
                depth - 1,
            ))))),
        },
    }
}

// REQ-DSL-012: resolving a random predicate whose token refs all name words leaves no
// TextToken reference, in the predicate and in a selector built from it.
#[test]
fn property_resolution_leaves_no_token_refs() {
    let mut rng = Lcg(201);
    for _ in 0..300 {
        let s = random_stream(&mut rng);
        let ids: Vec<String> = s
            .tokens()
            .iter()
            .filter(|t| t.is_rewritable())
            .map(|t| t.id().to_owned())
            .collect();
        let pred = random_pred(&mut rng, &ids, 4);
        let got = resolve_predicate_words(&pred, &s).unwrap();
        assert!(!has_token_ref(&got), "{got:?}");
        let sel = resolve_selector_words(&filter(pred), &s).unwrap();
        assert!(!has_token_ref(&sel));
    }
}

/// A bounded random JSON-like value, built through `From`/`FromIterator` so no JSON crate is
/// named here.
fn random_value<V>(rng: &mut Lcg, depth: u32) -> V
where
    V: From<i64> + From<bool> + From<String> + From<()> + From<Vec<V>> + FromIterator<(String, V)>,
{
    const KEYS: [&str; 8] = [
        "id", "kind", "category", "value", "text", "ref", "name", "x",
    ];
    const STRS: [&str; 12] = [
        "t1",
        "t2",
        "WORD",
        "LITERAL",
        "NAME_REF",
        "SELF",
        "OTHER",
        "BASIC_LAND_TYPE",
        "CARD_NAME",
        "Swamp",
        "",
        "\u{FDD0}",
    ];
    let top = if depth == 0 { 4 } else { 6 };
    match rng.below(top) {
        0 => V::from(()),
        1 => V::from(rng.below(2) == 0),
        2 => V::from(i64::try_from(rng.next()).unwrap() - (1 << 30)),
        3 => V::from((*rng.pick(&STRS)).to_owned()),
        4 => {
            let n = rng.below(4);
            V::from(
                (0..n)
                    .map(|_| random_value(rng, depth - 1))
                    .collect::<Vec<V>>(),
            )
        }
        _ => {
            let n = rng.below(6);
            (0..n)
                .map(|_| ((*rng.pick(&KEYS)).to_owned(), random_value(rng, depth - 1)))
                .collect()
        }
    }
}

/// A random placeholder biased towards near-valid tokens.
fn random_placeholder(rng: &mut Lcg) -> card::TextTokenStream {
    let n = rng.below(4);
    let mut text = card::TextTokenStream(Vec::new());
    for _ in 0..n {
        let v = if rng.below(2) == 0 {
            random_value(rng, 3)
        } else {
            // An object whose members are all from the token vocabulary.
            random_value(rng, 1)
        };
        text.0.push(v);
    }
    text
}

// REQ-DSL-012, R12: decode_rules_text and layer 3 application never panic on bounded random
// placeholder content, and evaluation stays Ok; undecodable text is left unchanged.
#[test]
fn property_random_placeholders_never_panic() {
    let mut rng = Lcg(1206);
    let r = registry(vec![(
        1,
        l3(1, only(&[1]), replace(BLT, "Swamp", "Island")),
    )]);
    for i in 0..1500 {
        let text = random_placeholder(&mut rng);
        let decoded = decode_rules_text(&text);
        let env = Env::default().with(o(1), object("test.bear_1", &["Bear"], text.clone()));
        let got = evaluate(&r, &env, o(1)).unwrap().characteristics.rules_text;
        if decoded.is_err() {
            assert_eq!(got, text, "case {i}");
        }
    }
}

// REQ-DSL-012, R12: random bytes decoded as a MessagePack placeholder never make
// decode_rules_text or layer 3 panic.
#[test]
fn property_random_bytes_never_panic() {
    let mut rng = Lcg(42);
    let r = registry(vec![(
        1,
        l3(1, only(&[1]), replace(BLT, "Swamp", "Island")),
    )]);
    for _ in 0..2000 {
        let len = rng.below(48);
        let mut bytes: Vec<u8> = (0..len)
            .map(|_| u8::try_from(rng.next() & 0xFF).unwrap())
            .collect();
        // Half the runs start as a one-element MessagePack array so more inputs reach the
        // token decoder.
        if rng.below(2) == 0 {
            bytes.insert(0, 0x91);
        }
        let Ok(text) = rmp_serde::from_slice::<card::TextTokenStream>(&bytes) else {
            continue;
        };
        let _ = decode_rules_text(&text);
        let env = Env::default().with(o(1), object("test.bear_1", &["Bear"], text));
        assert!(evaluate(&r, &env, o(1)).is_ok());
    }
}
