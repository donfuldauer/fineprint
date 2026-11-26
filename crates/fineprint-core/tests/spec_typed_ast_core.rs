// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Black-box spec tests for #232 FP-164 in fineprint-core: every place that held a card AST
//! wrapper holds the real typed AST (contract C6), text effects operate on the typed stream, and
//! the state hash of a state holding typed ASTs is deterministic and content-sensitive (C7).
//! Covers REQ-DSL-012 (layer 3 text tokens; CR 612.1, 612.2, 201.5), REQ-DSL-003 (value
//! expressions), REQ-NFR-006 (canonical encoding and state hash) and REQ-CORE-001
//! (determinism). Written from the work-item brief only, through the public API and the helpers
//! the existing core spec tests use. Synthetic data only (R5); fixed seeds only (R3, R7).

#[path = "spec_layers_support.rs"]
mod support;

#[path = "spec_layers_state_support.rs"]
mod state_support;

use fineprint_core::layers::{
    CopyException, LayerError, LayerModification, StampReason, evaluate, rewrite_text_words,
};
use fineprint_core::objects::Characteristics;
use fineprint_core::state::GameState;
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::text::{NameRefKind, RewritableCategory, TextToken};
use serde_json::{Value, json};
use support::{TestEnv, eff, enchantment, oid, registry};

use RewritableCategory::{BasicLandType, CardName, ColorWord};

// ---------------------------------------------------------------------------------------------
// typed fixtures
// ---------------------------------------------------------------------------------------------

fn lit_t(id: &str, text: &str) -> TextToken {
    TextToken::Literal {
        id: id.into(),
        text: text.into(),
    }
}

fn word(id: &str, category: RewritableCategory, value: &str) -> TextToken {
    TextToken::Word {
        id: id.into(),
        category,
        value: value.into(),
    }
}

fn self_ref(id: &str) -> TextToken {
    TextToken::NameRef {
        id: id.into(),
        kind: NameRefKind::SelfRef,
        name: None,
    }
}

fn other_ref(id: &str, name: &str) -> TextToken {
    TextToken::NameRef {
        id: id.into(),
        kind: NameRefKind::Other,
        name: Some(name.into()),
    }
}

fn stream(tokens: Vec<TextToken>) -> TextTokenStream {
    TextTokenStream::new(tokens).expect("valid synthetic stream")
}

/// Text with every token kind: literal text, rewritable words and both name references, some
/// holding the same value as the word a text effect looks for.
fn mixed_text() -> TextTokenStream {
    stream(vec![
        lit_t("l0", "Swamp"),
        word("t1", BasicLandType, "Swamp"),
        self_ref("t2"),
        other_ref("t3", "Swamp"),
        word("t4", ColorWord, "Swamp"),
        word("t5", BasicLandType, "Plains"),
    ])
}

fn ability(v: Value) -> AbilityAST {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{v}: {e}"))
}

fn value(v: Value) -> ValueExpressionAST {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{v}: {e}"))
}

/// One valid ability of every kind (effect lists use real instructions).
fn every_kind_abilities() -> Vec<AbilityAST> {
    vec![
        ability(json!({"abilityId": "a0", "kind": "SPELL", "effect": [{"op": "POPULATE"}]})),
        ability(json!({"abilityId": "a1", "kind": "ACTIVATED",
            "cost": [{"cost": "TAP_SOURCE"}], "effect": []})),
        ability(json!({"abilityId": "a2", "kind": "TRIGGERED",
            "trigger": {"mode": "STATE", "state": {"pred": "IS_CREATURE"}}, "effect": []})),
        ability(json!({"abilityId": "a3", "kind": "STATIC",
            "effects": [{"type": "RULE", "kind": "SKIP"}]})),
        ability(json!({"abilityId": "a4", "kind": "REPLACEMENT",
            "match": {"event": "DIES", "affected": {"ref": "SELF"}},
            "replaceWith": [{"op": "SKIP"}]})),
        ability(
            json!({"abilityId": "a5", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
            "params": {"n": 2}}),
        ),
    ]
}

/// A creature whose characteristics hold typed values in every AST position.
fn typed_creature(power: i64) -> Characteristics {
    Characteristics {
        rules_text: mixed_text(),
        abilities: every_kind_abilities(),
        power: Some(ValueExpressionAST::Literal(power)),
        toughness: Some(value(json!({"LITERAL": 4}))),
        ..support::creature("test.typed_creature", 0, 0)
    }
}

// ---------------------------------------------------------------------------------------------
// (a) Characteristics keep typed values
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-003, REQ-DSL-012, REQ-NFR-006 (C6): `Characteristics` built from typed values keeps
/// them exactly (equal inputs give equal values, each field reads back as given and serialises
/// to its wire form), and evaluation without effects returns the typed text and abilities
/// unchanged and literal power/toughness as `ValueExpressionAST::Literal` (`{"LITERAL": n}`).
#[test]
fn req_dsl_003_012_characteristics_keep_typed_values() {
    let a = typed_creature(3);
    let b = typed_creature(3);
    assert_eq!(a, b);
    assert_eq!(a.clone(), a);
    assert_ne!(a, typed_creature(5));
    assert_eq!(a.rules_text, mixed_text());
    assert_eq!(a.abilities, every_kind_abilities());
    assert_eq!(a.power, Some(ValueExpressionAST::Literal(3)));
    assert_eq!(
        serde_json::to_value(&a.toughness).unwrap(),
        json!({"LITERAL": 4})
    );
    assert_eq!(
        serde_json::to_value(&a.rules_text).unwrap()[3],
        json!({"id": "t3", "kind": "NAME_REF", "ref": "OTHER", "name": "Swamp"})
    );
    let kinds: Vec<&str> = a.abilities.iter().map(AbilityAST::kind).collect();
    assert_eq!(
        kinds,
        [
            "SPELL",
            "ACTIVATED",
            "TRIGGERED",
            "STATIC",
            "REPLACEMENT",
            "KEYWORD"
        ]
    );

    let mut env = TestEnv::new();
    let id = env.put(oid(1), a.clone());
    let ev = evaluate(&registry(&[]), &env, id).expect("evaluate");
    assert_eq!(ev.characteristics.rules_text, a.rules_text);
    assert_eq!(ev.characteristics.abilities, a.abilities);
    assert_eq!(
        ev.characteristics.power,
        Some(ValueExpressionAST::Literal(3))
    );
    assert_eq!(
        serde_json::to_value(&ev.characteristics.power).unwrap(),
        json!({"LITERAL": 3})
    );
    assert_eq!(
        ev.characteristics.toughness,
        Some(ValueExpressionAST::Literal(4))
    );
}

// ---------------------------------------------------------------------------------------------
// (b) layer 3 text effects over the typed stream
// ---------------------------------------------------------------------------------------------

fn replace(category: RewritableCategory, from: &str, to: &str) -> LayerModification {
    LayerModification::ReplaceTextWord {
        category,
        from: from.into(),
        to: to.into(),
    }
}

fn eval_text(effects: &[(u64, LayerModification)]) -> TextTokenStream {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(1),
        Characteristics {
            rules_text: mixed_text(),
            ..enchantment("test.text_target")
        },
    );
    let list: Vec<_> = effects
        .iter()
        .enumerate()
        .map(|(i, (t, m))| {
            (
                u64::try_from(i).unwrap() + 1,
                eff(Layer::L3, *t, support::objs(&[a]), m.clone()),
            )
        })
        .collect();
    evaluate(&registry(&list), &env, a)
        .expect("evaluate")
        .characteristics
        .rules_text
}

/// REQ-DSL-012, CR 612.1, 612.2, 201.5 (C6): a layer 3 REPLACE_TEXT_WORD over typed rules text
/// rewrites exactly the matching rewritable word: literal text, both name references (the
/// self-reference is never rewritten) and words of another category holding the same value
/// stay untouched, and every token keeps its id and position.
#[test]
fn req_dsl_012_replace_text_word_rewrites_only_matching_words() {
    // CR 612.1
    let got = eval_text(&[(5, replace(BasicLandType, "Swamp", "Island"))]);
    let want = stream(vec![
        lit_t("l0", "Swamp"),
        word("t1", BasicLandType, "Island"),
        self_ref("t2"),
        other_ref("t3", "Swamp"),
        word("t4", ColorWord, "Swamp"),
        word("t5", BasicLandType, "Plains"),
    ]);
    assert_eq!(got, want);
    assert_eq!(
        rewrite_text_words(&mixed_text(), BasicLandType, "Swamp", "Island"),
        Ok(want)
    );
}

/// REQ-DSL-012, REQ-LAYER-002; CR 613.7, 613.8a, 613.8b, 613.8c (C6): two overlapping text
/// effects over typed text, each pair in both timestamp orders and both registration orders,
/// with the outcomes the existing layer-3 suite establishes (`spec_layers_text.rs`,
/// `chained_effects_dependency_both_orders` and `chained_effects_loop_timestamp_order`):
/// A = Swamp→Island and B = Island→Forest (B depends on A) end as Forest in both orders;
/// A = Swamp→Island and C = Swamp→Forest (a dependency loop) end as the earlier timestamp's
/// `to`. Only the matching word changes in every case.
#[test]
fn req_dsl_012_overlapping_text_effects_both_timestamp_orders() {
    // CR 613.8a
    let a = replace(BasicLandType, "Swamp", "Island");
    let b = replace(BasicLandType, "Island", "Forest");
    let c = replace(BasicLandType, "Swamp", "Forest");
    let expect = |v: &str| {
        stream(vec![
            lit_t("l0", "Swamp"),
            word("t1", BasicLandType, v),
            self_ref("t2"),
            other_ref("t3", "Swamp"),
            word("t4", ColorWord, "Swamp"),
            word("t5", BasicLandType, "Plains"),
        ])
    };
    for (ta, tb) in [(5, 6), (6, 5)] {
        // Chained: dependency decides, whatever the timestamps or registration order.
        assert_eq!(
            eval_text(&[(ta, a.clone()), (tb, b.clone())]),
            expect("Forest"),
            "chain ts(A)={ta} ts(B)={tb}"
        );
        assert_eq!(
            eval_text(&[(tb, b.clone()), (ta, a.clone())]),
            expect("Forest"),
            "chain registered B first, ts(A)={ta} ts(B)={tb}"
        );
        // Loop: timestamp order decides, the earlier effect's `to` wins (CR 613.8b).
        let want = if ta < tb { "Island" } else { "Forest" };
        assert_eq!(
            eval_text(&[(ta, a.clone()), (tb, c.clone())]),
            expect(want),
            "loop ts(A)={ta} ts(C)={tb}"
        );
        assert_eq!(
            eval_text(&[(tb, c.clone()), (ta, a.clone())]),
            expect(want),
            "loop registered C first, ts(A)={ta} ts(C)={tb}"
        );
    }
}

/// REQ-DSL-012, REQ-NFR-006 (C6): an effect that matches no rewritable word leaves the typed
/// text byte-identical (equal value and equal serialised bytes), whether the value occurs in
/// literal text and name references only, in another category, or nowhere.
#[test]
fn req_dsl_012_non_matching_effect_leaves_text_byte_identical() {
    let base = serde_json::to_vec(&mixed_text()).unwrap();
    for m in [
        replace(BasicLandType, "Forest", "Island"),
        replace(RewritableCategory::CreatureType, "Swamp", "Elf"),
        replace(RewritableCategory::Number, "Swamp", "3"),
        replace(ColorWord, "Red", "Blue"),
    ] {
        let got = eval_text(&[(5, m.clone())]);
        assert_eq!(got, mixed_text(), "{m:?}");
        assert_eq!(serde_json::to_vec(&got).unwrap(), base, "{m:?}: bytes");
    }
}

/// REQ-DSL-012 (C6): CARD_NAME is unsupported: registering such an effect fails with
/// `LayerError::Unsupported`, and the pure helper reports the unsupported category.
#[test]
fn req_dsl_012_card_name_category_is_unsupported() {
    let mut r = registry(&[]);
    let res = r.register(
        fineprint_base::ids::EffectId::new(1),
        eff(
            Layer::L3,
            5,
            support::objs(&[oid(1)]),
            replace(CardName, "Swamp", "Island"),
        ),
    );
    assert!(matches!(res, Err(LayerError::Unsupported(_))), "{res:?}");
    assert_eq!(r.len(), 0);
    assert_eq!(
        rewrite_text_words(&mixed_text(), CardName, "Swamp", "Island"),
        Err(fineprint_core::layers::TextWordError::UnsupportedCategory)
    );
}

// ---------------------------------------------------------------------------------------------
// (c), (d) state hash of a state holding typed ASTs
// ---------------------------------------------------------------------------------------------

/// The typed contents a state is built from; one field at a time is varied by (d).
#[derive(Clone)]
struct Contents {
    literal: i64,
    token: &'static str,
    ability_id: &'static str,
}

const BASE: Contents = Contents {
    literal: 2,
    token: "test.word_a",
    ability_id: "test.typed_grant",
};

fn chars_with(c: &Contents) -> Characteristics {
    Characteristics {
        rules_text: stream(vec![
            lit_t("w0", c.token),
            word("w1", ColorWord, "Red"),
            self_ref("w2"),
        ]),
        abilities: vec![ability(
            json!({"abilityId": c.ability_id, "kind": "KEYWORD",
            "keyword": "TEST_KEYWORD"}),
        )],
        power: Some(ValueExpressionAST::Literal(c.literal)),
        toughness: Some(value(json!({"MATH": {"op": "ADD", "lhs": "X",
            "rhs": {"LITERAL": 1}}}))),
        ..support::creature("test.typed_state", 0, 0)
    }
}

/// A state holding typed ASTs in every place of C6: Characteristics inside a layer-1a copy and
/// a layer-1b face-down modification, a copy exception setting power to a typed value and
/// gaining a typed ability, a layer-3 `SetText` and a layer-6 `AddAbility`.
fn typed_state(c: &Contents) -> GameState {
    let mut s = state_support::base();
    let t0 = s.stamp_object(oid(1), StampReason::EntersZone).unwrap();
    let _ = t0;
    let grant = ability(json!({"abilityId": c.ability_id, "kind": "STATIC",
        "effects": [{"type": "RULE", "kind": "SKIP"}]}));
    let mods: Vec<(Layer, LayerModification)> = vec![
        (
            Layer::L1a,
            LayerModification::CopyValues(Box::new(chars_with(c))),
        ),
        (
            Layer::L1a,
            LayerModification::ApplyCopyExceptions(vec![
                CopyException::SetPower(ValueExpressionAST::Literal(c.literal)),
                CopyException::GainAbility(grant.clone()),
            ]),
        ),
        (
            Layer::L1b,
            LayerModification::FaceDown(Box::new(chars_with(c))),
        ),
        (
            Layer::L3,
            LayerModification::SetText(stream(vec![
                lit_t("s0", c.token),
                other_ref("s1", "Test Other"),
            ])),
        ),
        (Layer::L6, LayerModification::AddAbility(grant)),
    ];
    for (layer, m) in mods {
        let t = s.stamp_resolved_effect().unwrap();
        s.register_effect(state_support::resolved(t, layer, &[1, 2], m), None)
            .unwrap_or_else(|e| panic!("{layer:?}: {e:?}"));
    }
    s
}

/// The state hash of `typed_state(&BASE)`. It was computed by this test's own path in the
/// default (cached) build and is asserted unchanged in the `full-recompute` build (C7: the layer
/// mode cannot change a state hash).
const GOLDEN_TYPED_STATE: &str = "9879316ad2f5773b38edc06eca295cfd4f8e7c5413a30f7fac6bee6bdee9f179";

/// REQ-NFR-006, REQ-CORE-001 (C7): the state hash of a state holding typed ASTs is the same over
/// 10 repeated constructions, survives a serialise/deserialise round trip of the whole state
/// (effects included) with equal effects and equal bytes, equals the pinned value in both layer
/// modes, and evaluating through the cache does not change it.
#[test]
fn req_nfr_006_typed_state_hash_is_deterministic() {
    let first = typed_state(&BASE);
    let h = first.state_hash();
    for _ in 0..10 {
        let again = typed_state(&BASE);
        assert_eq!(again.state_hash(), h);
        assert_eq!(again.serialize().unwrap(), first.serialize().unwrap());
    }
    let bytes = first.serialize().unwrap();
    let back = GameState::deserialize(&bytes).expect("round trip");
    assert_eq!(back, first);
    assert_eq!(back.effects(), first.effects());
    assert_eq!(back.state_hash(), h);
    assert_eq!(back.serialize().unwrap(), bytes);
    assert_eq!(
        h.to_hex(),
        GOLDEN_TYPED_STATE,
        "cached and full-recompute agree"
    );
    // Evaluating the state's effects (cached and uncached) leaves the hash alone.
    let w = state_support::world();
    let env = fineprint_core::state::GameEnvironment::new(&first, &w);
    let mut cache = fineprint_core::layers::LayerCache::new();
    let cached = cache.evaluate_all(first.effects(), &env).expect("cached");
    let plain = fineprint_core::layers::evaluate_all(first.effects(), &env).expect("uncached");
    assert_eq!(cached, plain);
    assert_eq!(first.state_hash(), h);
}

/// REQ-NFR-006, REQ-CORE-001 (C7): two states that differ only in one typed AST (a LITERAL
/// value, a text token, one ability) have different state hashes, and each is deterministic.
#[test]
fn req_nfr_006_one_typed_ast_changes_the_state_hash() {
    let h = typed_state(&BASE).state_hash();
    let variants = [
        Contents {
            literal: 3,
            ..BASE.clone()
        },
        Contents {
            token: "test.word_b",
            ..BASE.clone()
        },
        Contents {
            ability_id: "test.typed_grant_other",
            ..BASE.clone()
        },
    ];
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(h.to_hex());
    for v in &variants {
        let hv = typed_state(v).state_hash();
        assert_eq!(typed_state(v).state_hash(), hv);
        assert!(
            seen.insert(hv.to_hex()),
            "a variant hashes like another state"
        );
    }
}

/// REQ-NFR-006, REQ-CORE-001 (T5 property, fixed seeds): for random typed contents (literals in
/// the safe range, token texts, ability ids) the state hash is a function of the contents:
/// building twice gives equal hashes and equal bytes, and contents that differ give different
/// hashes.
#[test]
fn req_nfr_006_property_state_hash_is_a_function_of_typed_contents() {
    let mut rng = state_support::Rng::new(0x0232_F164);
    let tokens = ["test.word_a", "test.word_b", "test.word_c", "test.word_d"];
    let ids = ["test.grant_a", "test.grant_b", "test.grant_c"];
    let mut by_hash: std::collections::BTreeMap<String, (i64, &str, &str)> =
        std::collections::BTreeMap::new();
    for _ in 0..40 {
        let c = Contents {
            literal: i64::try_from(rng.below(7)).unwrap() - 3,
            token: tokens[usize::try_from(rng.below(4)).unwrap()],
            ability_id: ids[usize::try_from(rng.below(3)).unwrap()],
        };
        let a = typed_state(&c);
        let b = typed_state(&c);
        assert_eq!(a.state_hash(), b.state_hash());
        assert_eq!(a.serialize().unwrap(), b.serialize().unwrap());
        let key = (c.literal, c.token, c.ability_id);
        let prev = by_hash.insert(a.state_hash().to_hex(), key);
        assert!(prev.is_none() || prev == Some(key), "{prev:?} vs {key:?}");
    }
}
