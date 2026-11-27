// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property and bounded random-input tests for the layer 3 text-token helpers (REQ-DSL-012,
//! REQ-CORE-001; CR 612.1, 612.2, 201.5): `rewrite_text_words` preserves the stream's shape and
//! changes exactly the matching words; results stay valid streams; the typed stream's wire form
//! round-trips; an absent `from` is a no-op; resolved predicates hold no token references; and
//! decoding arbitrary placeholder content as the typed stream never panics, nor does `evaluate`
//! on whatever decodes (R12). #232 FP-164: `decode_rules_text`/`encode_rules_text` were removed
//! (the rules text of `Characteristics` is the typed stream), so their properties are stated on
//! the typed stream's serde.
//!
//! Black-box: written from the FP-036 brief only. Deterministic: a seeded LCG with fixed seeds,
//! no clock and no OS randomness (R3, R7). Synthetic values only (R5).

#[path = "spec_layers_support.rs"]
mod support;

use fineprint_core::layers::{
    LayerModification, evaluate, resolve_predicate_words, resolve_word_ref, rewrite_text_words,
};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card;
use fineprint_dsl::expr::{PredicateAST, WordRef};
use fineprint_dsl::text::{NameRefKind, RewritableCategory, TextToken, TextTokenStream as Stream};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use support::*;

use RewritableCategory::{BasicLandType, CardName, ColorWord, CreatureType, Number};

const SEEDS: [u64; 4] = [1, 37, 4242, 0xFEED_F00D];
/// Cases per seed; 4 seeds give 256 cases per property (at least 200).
const CASES: usize = 64;

const ALL_CATS: [RewritableCategory; 5] =
    [ColorWord, BasicLandType, CreatureType, Number, CardName];
const SUPPORTED: [RewritableCategory; 4] = [ColorWord, BasicLandType, CreatureType, Number];
const VALUES: [&str; 12] = [
    "Swamp",
    "Island",
    "Forest",
    "Red",
    "Blue",
    "Goblin",
    "Elf",
    "3",
    "4",
    "swamp",
    "red",
    "test.word",
];

fn pick<'a, T>(rng: &mut Lcg, v: &'a [T]) -> &'a T {
    &v[usize::try_from(rng.below(v.len() as u64)).expect("fits")]
}

fn gen_token(rng: &mut Lcg, i: usize) -> TextToken {
    let id = format!("t{i}");
    match rng.below(5) {
        0 => TextToken::Literal {
            id,
            text: (*pick(rng, &VALUES)).into(),
        },
        1 => TextToken::NameRef {
            id,
            kind: NameRefKind::SelfRef,
            name: None,
        },
        2 => TextToken::NameRef {
            id,
            kind: NameRefKind::Other,
            name: Some((*pick(rng, &VALUES)).into()),
        },
        _ => TextToken::Word {
            id,
            category: *pick(rng, &ALL_CATS),
            value: (*pick(rng, &VALUES)).into(),
        },
    }
}

fn gen_stream(rng: &mut Lcg) -> Stream {
    let n = usize::try_from(rng.below(10)).expect("fits");
    let mut tokens: Vec<TextToken> = (0..n).map(|i| gen_token(rng, i)).collect();
    rng.shuffle(&mut tokens); // ids stay unique, order varies
    Stream::new(tokens).expect("generated stream is valid")
}

fn for_cases(mut f: impl FnMut(&mut Lcg)) {
    for seed in SEEDS {
        let mut rng = Lcg::new(seed);
        for _ in 0..CASES {
            f(&mut rng);
        }
    }
}

fn kind_of(t: &TextToken) -> u8 {
    match t {
        TextToken::Literal { .. } => 0,
        TextToken::Word { .. } => 1,
        TextToken::NameRef { .. } => 2,
    }
}

/// REQ-DSL-012; CR 612.1, 612.2, 201.5: (a) count, ids, order and kinds are kept; non-words,
/// CARD_NAME words and non-matching words are unchanged; every changed token was a matching
/// word and now holds `to`; every matching word changed (when from != to). (b) the result is
/// always a valid stream.
#[test]
fn prop_rewrite_shape_and_scope() {
    for_cases(|rng| {
        let s = gen_stream(rng);
        let cat = *pick(rng, &SUPPORTED);
        let from = *pick(rng, &VALUES);
        let to = *pick(rng, &VALUES);
        let out = rewrite_text_words(&s, cat, from, to).expect("valid arguments");
        assert_eq!(out.len(), s.len());
        for (a, b) in s.tokens().iter().zip(out.tokens()) {
            assert_eq!(a.id(), b.id());
            assert_eq!(kind_of(a), kind_of(b));
            match a {
                TextToken::Word {
                    category, value, ..
                } if *category == cat && value == from => match b {
                    TextToken::Word {
                        category: c2,
                        value: v2,
                        ..
                    } => {
                        assert_eq!(c2, category);
                        assert_eq!(v2, to);
                    }
                    _ => unreachable!(),
                },
                _ => assert_eq!(a, b, "{cat:?} {from} -> {to}"),
            }
        }
        // (b)
        assert!(Stream::new(out.tokens().to_vec()).is_ok());
    });
}

/// REQ-DSL-012; CR 612.2: CARD_NAME is always UnsupportedCategory, never a rewrite.
#[test]
fn prop_card_name_never_rewritten() {
    for_cases(|rng| {
        let s = gen_stream(rng);
        let from = *pick(rng, &VALUES);
        let to = *pick(rng, &VALUES);
        assert_eq!(
            rewrite_text_words(&s, CardName, from, to),
            Err(fineprint_core::layers::TextWordError::UnsupportedCategory)
        );
    });
}

/// REQ-DSL-012: (c) decode(encode(s)) == s for generated valid streams, through JSON and
/// through MessagePack, as the typed rules-text position `card::TextTokenStream`.
#[test]
fn prop_encode_decode_round_trip() {
    for_cases(|rng| {
        let s = gen_stream(rng);
        let j: card::TextTokenStream =
            serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(j, s);
        let m: card::TextTokenStream = via_msgpack(&s);
        assert_eq!(m, s);
    });
}

/// REQ-DSL-012: (d) with `from` absent from the stream the result equals the input.
#[test]
fn prop_absent_from_is_identity() {
    for_cases(|rng| {
        let s = gen_stream(rng);
        let cat = *pick(rng, &SUPPORTED);
        let to = *pick(rng, &VALUES);
        assert_eq!(
            rewrite_text_words(&s, cat, "test.absent", to),
            Ok(s.clone())
        );
        // from == to is the identity too
        let from = *pick(rng, &VALUES);
        assert_eq!(rewrite_text_words(&s, cat, from, from), Ok(s));
    });
}

// ---------------------------------------------------------------------------------------------
// (e) predicates
// ---------------------------------------------------------------------------------------------

/// A stream whose ids `w0..w{n}` are all words (so every reference resolves).
fn gen_word_stream(rng: &mut Lcg, n: usize) -> Stream {
    let mut tokens: Vec<TextToken> = (0..n)
        .map(|i| TextToken::Word {
            id: format!("w{i}"),
            category: *pick(rng, &ALL_CATS),
            value: (*pick(rng, &VALUES)).into(),
        })
        .collect();
    tokens.push(TextToken::NameRef {
        id: "self".into(),
        kind: NameRefKind::SelfRef,
        name: None,
    });
    rng.shuffle(&mut tokens);
    Stream::new(tokens).expect("valid")
}

fn gen_ref(rng: &mut Lcg, n: usize) -> WordRef {
    if rng.below(3) == 0 {
        WordRef::Name((*pick(rng, &VALUES)).into())
    } else {
        WordRef::TextToken(format!("w{}", rng.below(n as u64)))
    }
}

fn gen_pred(rng: &mut Lcg, n: usize, depth: u32) -> PredicateAST {
    let pick_kind = if depth == 0 {
        3 + rng.below(4)
    } else {
        rng.below(7)
    };
    match pick_kind {
        0 => PredicateAST::And(
            (0..1 + rng.below(3))
                .map(|_| gen_pred(rng, n, depth - 1))
                .collect(),
        ),
        1 => PredicateAST::Or(
            (0..1 + rng.below(3))
                .map(|_| gen_pred(rng, n, depth - 1))
                .collect(),
        ),
        2 => PredicateAST::Not(Box::new(gen_pred(rng, n, depth - 1))),
        3 => PredicateAST::HasCardType {
            value: gen_ref(rng, n),
        },
        4 => PredicateAST::HasSubtype {
            value: gen_ref(rng, n),
        },
        5 => PredicateAST::HasSupertype {
            value: gen_ref(rng, n),
        },
        _ => PredicateAST::HasColor {
            value: gen_ref(rng, n),
        },
    }
}

/// Checks `out` against `input`: same shape; every TextToken(id) became Name(resolve(id));
/// every Name is unchanged; no TextToken is left.
fn check_resolved(input: &PredicateAST, out: &PredicateAST, s: &Stream) {
    let leaf = |a: &WordRef, b: &WordRef| match a {
        WordRef::TextToken(_) => {
            assert_eq!(b, &WordRef::Name(resolve_word_ref(s, a).expect("resolves")));
        }
        WordRef::Name(_) => assert_eq!(a, b),
    };
    match (input, out) {
        (PredicateAST::And(x), PredicateAST::And(y))
        | (PredicateAST::Or(x), PredicateAST::Or(y)) => {
            assert_eq!(x.len(), y.len());
            for (p, q) in x.iter().zip(y) {
                check_resolved(p, q, s);
            }
        }
        (PredicateAST::Not(p), PredicateAST::Not(q)) => check_resolved(p, q, s),
        (PredicateAST::HasCardType { value: a }, PredicateAST::HasCardType { value: b })
        | (PredicateAST::HasSubtype { value: a }, PredicateAST::HasSubtype { value: b })
        | (PredicateAST::HasSupertype { value: a }, PredicateAST::HasSupertype { value: b })
        | (PredicateAST::HasColor { value: a }, PredicateAST::HasColor { value: b }) => leaf(a, b),
        (a, b) => panic!("shape changed: {a:?} -> {b:?}"),
    }
}

/// REQ-DSL-012 (SRS §14.5): (e) for generated And/Or/Not trees over the four HAS_* leaves,
/// resolution replaces every TextToken(id) by Name(current value) and keeps Names; after a
/// rewrite, the same tree reads the rewritten values.
#[test]
fn prop_resolve_predicate_words() {
    for_cases(|rng| {
        let n = 1 + usize::try_from(rng.below(6)).expect("fits");
        let s = gen_word_stream(rng, n);
        let p = gen_pred(rng, n, 3);
        let copy = p.clone();
        let out = resolve_predicate_words(&p, &s).expect("every id resolves");
        check_resolved(&p, &out, &s);
        assert_eq!(p, copy);
        // through a rewrite
        let cat = *pick(rng, &SUPPORTED);
        let s2 =
            rewrite_text_words(&s, cat, pick(rng, &VALUES), pick(rng, &VALUES)).expect("valid");
        let out2 = resolve_predicate_words(&p, &s2).expect("every id resolves");
        check_resolved(&p, &out2, &s2);
    });
}

// ---------------------------------------------------------------------------------------------
// (f) bounded random placeholder content
// ---------------------------------------------------------------------------------------------

/// A JSON-like value serialised through the MessagePack bridge (no floats, R3).
#[derive(Clone, Debug)]
enum J {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl Serialize for J {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            J::Null => s.serialize_unit(),
            J::Bool(b) => s.serialize_bool(*b),
            J::Int(i) => s.serialize_i64(*i),
            J::Str(x) => s.serialize_str(x),
            J::Arr(v) => {
                let mut q = s.serialize_seq(Some(v.len()))?;
                for x in v {
                    q.serialize_element(x)?;
                }
                q.end()
            }
            J::Obj(v) => {
                let mut m = s.serialize_map(Some(v.len()))?;
                for (k, x) in v {
                    m.serialize_entry(k, x)?;
                }
                m.end()
            }
        }
    }
}

const KEYS: [&str; 8] = [
    "id", "kind", "text", "category", "value", "ref", "name", "test.k",
];
const STRS: [&str; 16] = [
    "",
    "a",
    "b",
    "LITERAL",
    "WORD",
    "NAME_REF",
    "SELF",
    "OTHER",
    "COLOR_WORD",
    "BASIC_LAND_TYPE",
    "CREATURE_TYPE",
    "NUMBER",
    "CARD_NAME",
    "Swamp",
    "\u{FFFF}",
    "test.word",
];

fn gen_str(rng: &mut Lcg) -> String {
    (*pick(rng, &STRS)).to_string()
}

fn gen_j(rng: &mut Lcg, depth: u32) -> J {
    let k = if depth == 0 {
        rng.below(4)
    } else {
        rng.below(7)
    };
    match k {
        0 => J::Null,
        1 => J::Bool(rng.below(2) == 0),
        2 => J::Int(i64::try_from(rng.below(2000)).expect("fits") - 1000),
        3 => J::Str(gen_str(rng)),
        4 => J::Arr((0..rng.below(4)).map(|_| gen_j(rng, depth - 1)).collect()),
        5 => J::Obj(Vec::new()),
        _ => J::Obj(
            (0..rng.below(5))
                .map(|_| ((*pick(rng, &KEYS)).to_string(), gen_j(rng, depth - 1)))
                .collect(),
        ),
    }
}

/// A token-shaped object: the right keys, often with right values, sometimes wrong types.
fn gen_token_like(rng: &mut Lcg) -> J {
    let mut fields = Vec::new();
    for key in KEYS {
        match rng.below(4) {
            0 => {}
            1 => fields.push((key.to_string(), gen_j(rng, 1))),
            _ => fields.push((key.to_string(), J::Str(gen_str(rng)))),
        }
    }
    J::Obj(fields)
}

/// Arbitrary placeholder-like content decoded as the typed stream through MessagePack, if it
/// decodes (#232 FP-164: formerly every value was accepted as a placeholder).
fn gen_placeholder(rng: &mut Lcg) -> Option<card::TextTokenStream> {
    let n = rng.below(5);
    let elems: Vec<J> = (0..n)
        .map(|_| {
            if rng.below(2) == 0 {
                gen_token_like(rng)
            } else {
                gen_j(rng, 3)
            }
        })
        .collect();
    let bytes = rmp_serde::to_vec_named(&elems).ok()?;
    rmp_serde::from_slice(&bytes).ok()
}

/// REQ-DSL-012, REQ-CORE-001 (R12): decoding arbitrary JSON-like placeholder content as the
/// typed stream never panics (Ok or Err), and an Ok result re-encodes and decodes to itself.
#[test]
fn prop_decode_never_panics() {
    for seed in SEEDS {
        let mut rng = Lcg::new(seed ^ 0x5EED);
        for _ in 0..(CASES * 4) {
            if let Some(s) = gen_placeholder(&mut rng) {
                let back: card::TextTokenStream = via_msgpack(&s);
                assert_eq!(back, s);
            }
        }
    }
}

/// REQ-DSL-012, REQ-CORE-001 (R12): random MessagePack bytes never make the typed stream's
/// decoder panic, and whatever decodes round-trips.
#[test]
fn prop_decode_random_bytes_never_panics() {
    for seed in SEEDS {
        let mut rng = Lcg::new(seed ^ 0xB17E5);
        for _ in 0..(CASES * 8) {
            let len = usize::try_from(rng.below(48)).expect("fits");
            let mut bytes: Vec<u8> = (0..len)
                .map(|_| u8::try_from(rng.below(256)).expect("fits"))
                .collect();
            // Start with an array marker most of the time so more inputs reach the decoder.
            if !bytes.is_empty() && rng.below(4) != 0 {
                bytes[0] = 0x90 | u8::try_from(rng.below(16)).expect("fits");
            }
            if let Ok(p) = rmp_serde::from_slice::<card::TextTokenStream>(&bytes) {
                let back: card::TextTokenStream = via_msgpack(&p);
                assert_eq!(back, p);
            }
        }
    }
}

/// REQ-DSL-012, REQ-CORE-001 (R12): full `evaluate` under a ReplaceTextWord effect never panics
/// and always returns Ok on arbitrary placeholder text that decodes as the typed stream, with the
/// result `rewrite_text_words` gives. #232 FP-164: undecodable text can no longer be held by
/// `Characteristics` (formerly "left unchanged").
#[test]
fn prop_evaluate_never_panics_on_placeholder_text() {
    for seed in SEEDS {
        let mut rng = Lcg::new(seed ^ 0xE7A1);
        for _ in 0..CASES {
            let Some(p) = gen_placeholder(&mut rng) else {
                continue;
            };
            let mut env = TestEnv::new();
            let a = env.put(
                oid(1),
                Characteristics {
                    rules_text: p.clone(),
                    ..enchantment("test.random_text")
                },
            );
            let cat = *pick(&mut rng, &SUPPORTED);
            let from = *pick(&mut rng, &VALUES);
            let to = *pick(&mut rng, &VALUES);
            let r = registry(&[(
                1,
                eff(
                    Layer::L3,
                    5,
                    objs(&[a]),
                    LayerModification::ReplaceTextWord {
                        category: cat,
                        from: from.into(),
                        to: to.into(),
                    },
                ),
            )]);
            let e = evaluate(&r, &env, a).expect("evaluate is Ok");
            if let Ok(want) = rewrite_text_words(&p, cat, from, to) {
                assert_eq!(e.characteristics.rules_text, want);
            }
        }
    }
}

/// REQ-DSL-012, REQ-LAYER-001; CR 612.1: on generated valid text, the pipeline result equals
/// `rewrite_text_words` on the decoded base (or the base itself when nothing changed).
#[test]
fn prop_pipeline_matches_helper() {
    for_cases(|rng| {
        let s = gen_stream(rng);
        let base = s.clone();
        let mut env = TestEnv::new();
        let a = env.put(
            oid(1),
            Characteristics {
                rules_text: base.clone(),
                ..enchantment("test.random_text")
            },
        );
        let cat = *pick(rng, &SUPPORTED);
        let from = *pick(rng, &VALUES);
        let to = *pick(rng, &VALUES);
        let r = registry(&[(
            1,
            eff(
                Layer::L3,
                5,
                objs(&[a]),
                LayerModification::ReplaceTextWord {
                    category: cat,
                    from: from.into(),
                    to: to.into(),
                },
            ),
        )]);
        let e = evaluate(&r, &env, a).expect("evaluate");
        let want = rewrite_text_words(&s, cat, from, to).expect("valid");
        if want == s {
            assert_eq!(e.characteristics.rules_text, base);
        } else {
            assert_eq!(e.characteristics.rules_text, want);
        }
    });
}
