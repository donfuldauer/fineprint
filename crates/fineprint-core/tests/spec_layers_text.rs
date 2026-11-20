// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for layer 3 `REPLACE_TEXT_WORD` through the layer pipeline (REQ-DSL-012,
//! REQ-LAYER-001, REQ-LAYER-002, REQ-LAYER-005, REQ-CORE-001; CR 612.1, 612.2, 612.3, 613.1c,
//! 613.7, 613.8a, 613.8b, 613.8c, 201.5): application, category scoping, self-references and
//! card names, granted abilities, affected sets, chained effects ordered by dependency,
//! registration checks, no-op rules, purity and cache consistency, and the acceptance scenario
//! "synthetic text-change scenario changes what a dependent ability applies to".
//!
//! Black-box: written from the FP-036 brief only. Synthetic objects named `test.*`; word values
//! are rules vocabulary only (R5). The text mixing a Swamp word and an Island word (former gap
//! G6, fixed in PR #196 review round 1) is tested in section 7 by the tokens each replacement
//! applies to (CR 613.8a).

#[path = "spec_layers_support.rs"]
mod support;

use std::collections::BTreeSet;
use std::rc::Rc;

use fineprint_base::ids::ObjectId;
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, ForAsLongAsWindow, LayerCache, LayerError,
    LayerModification, MatchContext, OrderItem, OrderRecord, decode_rules_text, encode_rules_text,
    evaluate, evaluate_all, evaluate_with_order, evaluated_digest, resolve_selector_words,
};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card;
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST, SelectorFilter, WordRef};
use fineprint_dsl::text::{NameRefKind, RewritableCategory, TextToken, TextTokenStream as Stream};
use support::*;

use RewritableCategory::{BasicLandType, CardName, ColorWord, CreatureType, Number};

// ---------------------------------------------------------------------------------------------
// builders
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

fn stream(tokens: Vec<TextToken>) -> Stream {
    Stream::new(tokens).expect("valid stream")
}

fn enc(tokens: Vec<TextToken>) -> card::TextTokenStream {
    encode_rules_text(&stream(tokens))
}

fn dec(t: &card::TextTokenStream) -> Stream {
    decode_rules_text(t).expect("evaluated rules text decodes")
}

fn replace(category: RewritableCategory, from: &str, to: &str) -> LayerModification {
    LayerModification::ReplaceTextWord {
        category,
        from: from.into(),
        to: to.into(),
    }
}

/// A resolved layer-3 text effect on a locked set.
fn text_eff(
    t: u64,
    ids: &[ObjectId],
    category: RewritableCategory,
    from: &str,
    to: &str,
) -> ContinuousEffect {
    eff(Layer::L3, t, objs(ids), replace(category, from, to))
}

/// A synthetic enchantment with the given rules text.
fn with_text(name: &str, tokens: Vec<TextToken>) -> Characteristics {
    Characteristics {
        rules_text: enc(tokens),
        ..enchantment(name)
    }
}

/// The token list of requirement 1.
fn req1_tokens() -> Vec<TextToken> {
    vec![
        lit_t("l0", "Gain"),
        word("t1", BasicLandType, "Swamp"),
        word("t2", ColorWord, "Red"),
        self_ref("t3"),
        other_ref("t4", "Swamp"),
        word("t5", CardName, "Swamp"),
    ]
}

/// A token list with a word of every category plus the non-word tokens.
fn mixed_tokens() -> Vec<TextToken> {
    vec![
        lit_t("l0", "Swamp"),
        word("t1", BasicLandType, "Swamp"),
        word("t2", ColorWord, "Red"),
        word("t3", CreatureType, "Goblin"),
        word("t4", Number, "3"),
        word("t5", CardName, "Goblin"),
        self_ref("t6"),
        other_ref("t7", "Red"),
        word("t8", CardName, "Red"),
        word("t9", ColorWord, "red"),
    ]
}

fn eval_text(r: &ContinuousEffects, env: &TestEnv, id: ObjectId) -> Stream {
    dec(&evaluate(r, env, id)
        .expect("evaluate")
        .characteristics
        .rules_text)
}

fn token_value(s: &Stream, id: &str) -> String {
    match s.find(id).expect("token present") {
        TextToken::Word { value, .. } => value.clone(),
        other => panic!("not a word: {other:?}"),
    }
}

/// The layer-3 steps of an order record, in order.
fn l3_steps(rec: &OrderRecord) -> Vec<OrderItem> {
    rec.steps
        .iter()
        .filter(|s| s.layer == Layer::L3)
        .map(|s| s.item.clone())
        .collect()
}

const A: u64 = 1;
const B: u64 = 2;

// ---------------------------------------------------------------------------------------------
// 1. basic rewrite
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012; CR 612.1, 612.2, 613.1c, 201.5: the BASIC_LAND_TYPE word is rewritten in layer
/// 3; the literal, the COLOR_WORD word, SELF, OTHER "Swamp" and the CARD_NAME "Swamp" word are
/// unchanged; ids, order and kinds are kept; the environment's base is not mutated.
#[test]
fn basic_rewrite_through_pipeline() {
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
    let base_before = env.base_chars(a).clone();
    let r = registry(&[(1, text_eff(5, &[a], BasicLandType, "Swamp", "Island"))]);
    let e = evaluate(&r, &env, a).expect("evaluate");
    let got = dec(&e.characteristics.rules_text);
    let mut want = req1_tokens();
    want[1] = word("t1", BasicLandType, "Island");
    assert_eq!(got.tokens(), want.as_slice());
    assert_eq!(env.base_chars(a), &base_before);
    // Every other characteristic is untouched.
    assert_eq!(
        Characteristics {
            rules_text: base_before.rules_text.clone(),
            ..e.characteristics.clone()
        },
        norm(&base_before)
    );
}

// ---------------------------------------------------------------------------------------------
// 2. category scoping
// ---------------------------------------------------------------------------------------------

fn single_effect_result(m: LayerModification) -> (Vec<TextToken>, Stream) {
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", mixed_tokens()));
    let r = registry(&[(1, eff(Layer::L3, 5, objs(&[a]), m))]);
    (mixed_tokens(), eval_text(&r, &env, a))
}

fn assert_only_changed(before: &[TextToken], after: &Stream, changed: &[(&str, &str)]) {
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(after.tokens()) {
        assert_eq!(b.id(), a.id());
        match changed.iter().find(|(id, _)| *id == b.id()) {
            Some((_, v)) => match (b, a) {
                (
                    TextToken::Word { category: c1, .. },
                    TextToken::Word {
                        category: c2,
                        value,
                        ..
                    },
                ) => {
                    assert_eq!(c1, c2);
                    assert_eq!(value, v);
                }
                _ => panic!("changed token {} is not a word", b.id()),
            },
            None => assert_eq!(b, a, "token {} must be unchanged", b.id()),
        }
    }
}

/// REQ-DSL-012; CR 612.2: an exact, case-sensitive COLOR_WORD rewrite changes only the
/// COLOR_WORD "Red" (not "red", not the CARD_NAME "Red", not the OTHER "Red").
#[test]
fn category_scoping_color_word() {
    let (before, after) = single_effect_result(replace(ColorWord, "Red", "Blue"));
    assert_only_changed(&before, &after, &[("t2", "Blue")]);
}

/// REQ-DSL-012; CR 612.2: case-sensitive: "swamp" does not match the "Swamp" word.
#[test]
fn category_scoping_case_sensitive() {
    let (before, after) = single_effect_result(replace(BasicLandType, "swamp", "Island"));
    assert_only_changed(&before, &after, &[]);
    let (before, after) = single_effect_result(replace(ColorWord, "red", "Blue"));
    assert_only_changed(&before, &after, &[("t9", "Blue")]);
}

/// REQ-DSL-012; CR 612.2, 612.2a: a CREATURE_TYPE rewrite changes the CREATURE_TYPE word only;
/// the CARD_NAME word with the same value stays.
#[test]
fn category_scoping_creature_type() {
    let (before, after) = single_effect_result(replace(CreatureType, "Goblin", "Elf"));
    assert_only_changed(&before, &after, &[("t3", "Elf")]);
}

/// REQ-DSL-012; CR 612.1: a NUMBER rewrite changes the NUMBER word and nothing else.
#[test]
fn category_scoping_number() {
    let (before, after) = single_effect_result(replace(Number, "3", "4"));
    assert_only_changed(&before, &after, &[("t4", "4")]);
}

/// REQ-DSL-012; CR 612.2: from == to leaves `rules_text` byte-identical.
#[test]
fn from_equals_to_is_byte_identical() {
    for (cat, w) in [
        (ColorWord, "Red"),
        (BasicLandType, "Swamp"),
        (CreatureType, "Goblin"),
        (Number, "3"),
    ] {
        let mut env = TestEnv::new();
        let a = env.put(oid(A), with_text("test.text_a", mixed_tokens()));
        let r = registry(&[(1, text_eff(5, &[a], cat, w, w))]);
        let e = evaluate(&r, &env, a).expect("evaluate");
        assert_eq!(
            e.characteristics.rules_text,
            env.base_chars(a).rules_text,
            "{cat:?}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 3. self-references
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012; CR 201.5, 612.2: for every supported category, with `from` equal to the OTHER
/// name or to a literal text, the SELF and OTHER NameRef tokens and the Literal tokens are
/// unchanged after evaluation.
#[test]
fn self_references_never_rewritten() {
    let tokens = vec![
        lit_t("l0", "Swamp"),
        lit_t("l1", "Goblin"),
        self_ref("s0"),
        other_ref("o0", "Swamp"),
        other_ref("o1", "Goblin"),
        other_ref("o2", "3"),
        other_ref("o3", "Red"),
        lit_t("l2", "3"),
        lit_t("l3", "Red"),
        word("w0", BasicLandType, "Swamp"),
        word("w1", CreatureType, "Goblin"),
        word("w2", Number, "3"),
        word("w3", ColorWord, "Red"),
    ];
    let non_words: Vec<TextToken> = tokens
        .iter()
        .filter(|t| !matches!(t, TextToken::Word { .. }))
        .cloned()
        .collect();
    for cat in [ColorWord, BasicLandType, CreatureType, Number] {
        for from in ["Swamp", "Goblin", "3", "Red"] {
            let mut env = TestEnv::new();
            let a = env.put(oid(A), with_text("test.self_ref", tokens.clone()));
            let r = registry(&[(1, text_eff(5, &[a], cat, from, "Forest"))]);
            let got = eval_text(&r, &env, a);
            let got_non_words: Vec<TextToken> = got
                .tokens()
                .iter()
                .filter(|t| !matches!(t, TextToken::Word { .. }))
                .cloned()
                .collect();
            assert_eq!(got_non_words, non_words, "{cat:?} {from}");
            assert_eq!(got.len(), tokens.len());
        }
    }
}

/// REQ-DSL-012; CR 201.5: after evaluation, `resolve_word_ref` on a NameRef or a Literal id is
/// NotAWord, and a `{"ref":"SELF"}` reference selector is unchanged by
/// `resolve_selector_words`.
#[test]
fn self_references_read_through_helpers() {
    use fineprint_core::layers::{TextWordError, resolve_word_ref};
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
    let r = registry(&[(1, text_eff(5, &[a], BasicLandType, "Swamp", "Island"))]);
    let got = eval_text(&r, &env, a);
    for id in ["l0", "t3", "t4"] {
        assert_eq!(
            resolve_word_ref(&got, &WordRef::TextToken(id.into())),
            Err(TextWordError::NotAWord(id.into()))
        );
    }
    let self_sel = ObjectSelectorAST::Ref(fineprint_dsl::expr::ReferenceAST::SelfRef);
    assert_eq!(
        resolve_selector_words(&self_sel, &got),
        Ok(self_sel.clone())
    );
}

// ---------------------------------------------------------------------------------------------
// 4. card names
// ---------------------------------------------------------------------------------------------

fn register_all_three(
    r: &mut ContinuousEffects,
    env: &TestEnv,
    id: u64,
    e: ContinuousEffect,
) -> [Result<(), LayerError>; 3] {
    let before = r.clone();
    let one = r.register(eid(id), e.clone());
    assert_eq!(
        *r, before,
        "register must leave the registry unchanged on error"
    );
    let two = r.register_group(env, vec![(eid(id), e.clone())]);
    assert_eq!(
        *r, before,
        "register_group must leave the registry unchanged on error"
    );
    let three = r
        .register_group_for_as_long_as(
            env,
            ForAsLongAsWindow {
                starts: true,
                ended_before_first_application: false,
                begins_again: false,
            },
            vec![(eid(id), e)],
        )
        .map(|_| ());
    assert_eq!(*r, before);
    [one, two, three]
}

/// REQ-DSL-012, REQ-LAYER-001; CR 612.2: a CARD_NAME replacement is rejected with
/// `Unsupported` by all three entry points, registry length unchanged.
#[test]
fn card_name_replacement_rejected() {
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
    let mut r = registry(&[(1, text_eff(5, &[a], BasicLandType, "Swamp", "Island"))]);
    for (from, to) in [("Swamp", "Island"), ("", ""), ("\u{FFFF}", "x")] {
        for affected in [objs(&[a]), by_sel(SEL_ALL)] {
            let e = eff(Layer::L3, 6, affected, replace(CardName, from, to));
            let res = register_all_three(&mut r, &env, 2, e);
            for (i, x) in res.iter().enumerate() {
                assert!(
                    matches!(x, Err(LayerError::Unsupported(_))),
                    "entry point {i}: {x:?}"
                );
            }
            assert_eq!(r.len(), 1);
        }
    }
}

/// REQ-DSL-012; CR 612.2: a CARD_NAME word is still readable by token id after evaluation and
/// is never rewritten by a replacement of another category with the same value.
#[test]
fn card_name_word_readable_and_unchanged() {
    use fineprint_core::layers::{resolve_word_ref, text_token_matches};
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
    let r = registry(&[
        (1, text_eff(5, &[a], BasicLandType, "Swamp", "Island")),
        (2, text_eff(6, &[a], CreatureType, "Swamp", "Elf")),
        (3, text_eff(7, &[a], ColorWord, "Swamp", "Blue")),
    ]);
    let got = eval_text(&r, &env, a);
    assert_eq!(got.find("t5"), Some(&word("t5", CardName, "Swamp")));
    assert_eq!(
        resolve_word_ref(&got, &WordRef::TextToken("t5".into())),
        Ok("Swamp".into())
    );
    assert_eq!(text_token_matches(&got, "t5", "Swamp"), Ok(true));
}

// ---------------------------------------------------------------------------------------------
// 5. granted abilities
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012; CR 612.3: the base ability and an ability granted in layer 6 are byte-identical
/// with and without a layer-3 text effect that rewrites the object's text.
#[test]
fn granted_abilities_not_rewritten() {
    let mut env = TestEnv::new();
    let base = Characteristics {
        abilities: vec![static_ability("test.text_a.static")],
        ..creature("test.text_a", 2, 2)
    };
    let a = env.put(
        oid(A),
        Characteristics {
            rules_text: enc(req1_tokens()),
            ..base
        },
    );
    let grant = eff(
        Layer::L6,
        3,
        objs(&[a]),
        LayerModification::AddAbility(kw_ability("test.granted", "flying")),
    );
    let without = registry(&[(1, grant.clone())]);
    let with = registry(&[
        (1, grant),
        (2, text_eff(5, &[a], BasicLandType, "Swamp", "Island")),
    ]);
    let e0 = evaluate(&without, &env, a).expect("evaluate");
    let e1 = evaluate(&with, &env, a).expect("evaluate");
    assert_eq!(e0.characteristics.abilities.len(), 2);
    assert_eq!(e1.characteristics.abilities, e0.characteristics.abilities);
    assert_eq!(
        e1.characteristics.abilities[0],
        static_ability("test.text_a.static")
    );
    assert_eq!(
        e1.characteristics.abilities[1],
        kw_ability("test.granted", "flying")
    );
    assert_ne!(e1.characteristics.rules_text, e0.characteristics.rules_text);
    assert_eq!(
        token_value(&dec(&e1.characteristics.rules_text), "t1"),
        "Island"
    );
    // Everything but the text is the same.
    assert_eq!(
        Characteristics {
            rules_text: e0.characteristics.rules_text.clone(),
            ..e1.characteristics.clone()
        },
        e0.characteristics
    );
}

// ---------------------------------------------------------------------------------------------
// 6. affected set and timestamps
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012; CR 613.7: a locked set {A} does not change B with identical text.
#[test]
fn locked_set_changes_only_its_objects() {
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
    let b = env.put(oid(B), with_text("test.text_b", req1_tokens()));
    let r = registry(&[(1, text_eff(5, &[a], BasicLandType, "Swamp", "Island"))]);
    assert_eq!(token_value(&eval_text(&r, &env, a), "t1"), "Island");
    let eb = evaluate(&r, &env, b).expect("evaluate");
    assert_eq!(eb.characteristics.rules_text, env.base_chars(b).rules_text);
}

/// REQ-DSL-012; CR 613.7: a selector effect changes exactly the objects the selector matches.
#[test]
fn selector_changes_exactly_matching_objects() {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(A),
        Characteristics {
            rules_text: enc(req1_tokens()),
            ..creature("test.text_a", 1, 1)
        },
    );
    let b = env.put(oid(B), with_text("test.text_b", req1_tokens()));
    let c = env.put(
        oid(3),
        Characteristics {
            rules_text: enc(req1_tokens()),
            ..creature("test.text_c", 1, 1)
        },
    );
    let r = registry(&[(
        1,
        eff(
            Layer::L3,
            5,
            by_sel(SEL_CREATURE),
            replace(BasicLandType, "Swamp", "Island"),
        ),
    )]);
    let all = evaluate_all(&r, &env).expect("evaluate_all");
    assert_eq!(
        token_value(&dec(&all[&a].characteristics.rules_text), "t1"),
        "Island"
    );
    assert_eq!(
        token_value(&dec(&all[&c].characteristics.rules_text), "t1"),
        "Island"
    );
    assert_eq!(
        all[&b].characteristics.rules_text,
        env.base_chars(b).rules_text
    );
}

/// REQ-DSL-012; CR 613.7: two disjoint text effects give the same result in both timestamp
/// orders.
#[test]
fn disjoint_effects_commute() {
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
    let mut results = Vec::new();
    for (t1, t2) in [(5, 6), (6, 5)] {
        let r = registry(&[
            (1, text_eff(t1, &[a], BasicLandType, "Swamp", "Island")),
            (2, text_eff(t2, &[a], ColorWord, "Red", "Blue")),
        ]);
        results.push(eval_text(&r, &env, a));
    }
    assert_eq!(results[0], results[1]);
    let mut want = req1_tokens();
    want[1] = word("t1", BasicLandType, "Island");
    want[2] = word("t2", ColorWord, "Blue");
    assert_eq!(results[0].tokens(), want.as_slice());
}

// ---------------------------------------------------------------------------------------------
// 7. chained effects (dependency)
// ---------------------------------------------------------------------------------------------

/// Text with a single BASIC_LAND_TYPE "Swamp" word and no Island word (the mixed Swamp+Island
/// text of former gap G6 is [`swamp_island`]).
fn swamp_only() -> Vec<TextToken> {
    vec![lit_t("l0", "test.text"), word("w", BasicLandType, "Swamp")]
}

const EFF_A: u64 = 1;
const EFF_B: u64 = 2;

/// REQ-DSL-012, REQ-LAYER-002; CR 613.8a, 613.8b, 613.8c: A = Swamp->Island, B = Island->Forest:
/// B depends on A, so in both timestamp orders the word ends as Forest and the order record
/// lists A then B in layer 3.
#[test]
fn chained_effects_dependency_both_orders() {
    for (ta, tb) in [(5, 6), (6, 5)] {
        let mut env = TestEnv::new();
        let x = env.put(oid(A), with_text("test.chain", swamp_only()));
        let r = registry(&[
            (EFF_A, text_eff(ta, &[x], BasicLandType, "Swamp", "Island")),
            (EFF_B, text_eff(tb, &[x], BasicLandType, "Island", "Forest")),
        ]);
        let (e, rec) = evaluate_with_order(&r, &env, x).expect("evaluate");
        assert_eq!(
            token_value(&dec(&e.characteristics.rules_text), "w"),
            "Forest",
            "ts(A)={ta} ts(B)={tb}"
        );
        assert_eq!(
            l3_steps(&rec),
            vec![OrderItem::Effect(eid(EFF_A)), OrderItem::Effect(eid(EFF_B))],
            "ts(A)={ta} ts(B)={tb}"
        );
        assert_eq!(evaluate(&r, &env, x).expect("evaluate"), e);
    }
}

/// REQ-DSL-012, REQ-LAYER-002; CR 613.8a, 613.8b: A = Swamp->Island, B = Swamp->Forest form a
/// dependency loop applied in timestamp order: the earlier timestamp's `to` wins in both
/// orders, and the order record lists the earlier effect first.
#[test]
fn chained_effects_loop_timestamp_order() {
    for (ta, tb, want, first, second) in [
        (5, 6, "Island", EFF_A, EFF_B),
        (6, 5, "Forest", EFF_B, EFF_A),
    ] {
        let mut env = TestEnv::new();
        let x = env.put(oid(A), with_text("test.chain", swamp_only()));
        let r = registry(&[
            (EFF_A, text_eff(ta, &[x], BasicLandType, "Swamp", "Island")),
            (EFF_B, text_eff(tb, &[x], BasicLandType, "Swamp", "Forest")),
        ]);
        let (e, rec) = evaluate_with_order(&r, &env, x).expect("evaluate");
        assert_eq!(
            token_value(&dec(&e.characteristics.rules_text), "w"),
            want,
            "ts(A)={ta} ts(B)={tb}"
        );
        assert_eq!(
            l3_steps(&rec),
            vec![
                OrderItem::Effect(eid(first)),
                OrderItem::Effect(eid(second))
            ],
            "ts(A)={ta} ts(B)={tb}"
        );
    }
}

/// Text with a BASIC_LAND_TYPE "Swamp" word `w1` and a BASIC_LAND_TYPE "Island" word `w2`
/// (former gap G6).
fn swamp_island() -> Vec<TextToken> {
    vec![
        lit_t("l0", "test.text"),
        word("w1", BasicLandType, "Swamp"),
        word("w2", BasicLandType, "Island"),
    ]
}

/// The evaluated text, the layer-3 order and the cached evaluation of `swamp_island()` under
/// A = Swamp->`a_to` at `ta` and B = `b_from`->`b_to` at `tb`; asserts that `evaluate` and a
/// `LayerCache` (twice) give exactly the `evaluate_with_order` result (REQ-LAYER-005).
fn run_mixed(ta: u64, tb: u64, a_to: &str, b_from: &str, b_to: &str) -> (Stream, Vec<OrderItem>) {
    let mut env = TestEnv::new();
    let x = env.put(oid(A), with_text("test.mixed", swamp_island()));
    let r = registry(&[
        (EFF_A, text_eff(ta, &[x], BasicLandType, "Swamp", a_to)),
        (EFF_B, text_eff(tb, &[x], BasicLandType, b_from, b_to)),
    ]);
    let (e, rec) = evaluate_with_order(&r, &env, x).expect("evaluate");
    assert_eq!(evaluate(&r, &env, x).expect("evaluate"), e);
    let mut cache = LayerCache::new();
    assert_eq!(cache.evaluate(&r, &env, x).expect("cached"), e);
    assert_eq!(cache.evaluate(&r, &env, x).expect("cached again"), e);
    let all = evaluate_all(&r, &env).expect("evaluate_all");
    let cached_all = cache.evaluate_all(&r, &env).expect("cached all");
    assert_eq!(cached_all, all);
    assert_eq!(evaluated_digest(&cached_all), evaluated_digest(&all));
    (dec(&e.characteristics.rules_text), l3_steps(&rec))
}

/// REQ-DSL-012, REQ-LAYER-002; CR 613.8a, 613.8b, 613.8c (B1, former gap G6, CodeRabbit
/// finding 4640489029): on text [Swamp w1, Island w2], A = Swamp->Island applies only to
/// Swamp words, which B = Island->Forest never changes, so A is independent of B; applying A
/// creates an Island word, so B depends on A. Both words end as Forest in every timestamp
/// order, and the order record lists A then B in layer 3.
#[test]
fn mixed_text_chain_dependency_all_orders() {
    for (ta, tb) in [(5, 6), (6, 5), (20, 10), (10, 20)] {
        let (text, order) = run_mixed(ta, tb, "Island", "Island", "Forest");
        assert_eq!(token_value(&text, "w1"), "Forest", "ts(A)={ta} ts(B)={tb}");
        assert_eq!(token_value(&text, "w2"), "Forest", "ts(A)={ta} ts(B)={tb}");
        assert_eq!(
            text.find("l0"),
            Some(&lit_t("l0", "test.text")),
            "ts(A)={ta} ts(B)={tb}"
        );
        assert_eq!(
            order,
            vec![OrderItem::Effect(eid(EFF_A)), OrderItem::Effect(eid(EFF_B))],
            "ts(A)={ta} ts(B)={tb}"
        );
    }
}

/// REQ-DSL-012, REQ-LAYER-002; CR 613.8a, 613.8b (B2): on text [Swamp w1, Island w2],
/// A = Swamp->Island and B = Island->Swamp each change what the other applies to, so they form
/// a dependency loop applied in timestamp order: A earlier gives [Swamp, Swamp], B earlier
/// gives [Island, Island]; the order record lists the earlier effect first.
#[test]
fn mixed_text_swap_loop_timestamp_order() {
    for (ta, tb, want, first, second) in [
        (5, 6, "Swamp", EFF_A, EFF_B),
        (6, 5, "Island", EFF_B, EFF_A),
        (10, 20, "Swamp", EFF_A, EFF_B),
        (20, 10, "Island", EFF_B, EFF_A),
    ] {
        let (text, order) = run_mixed(ta, tb, "Island", "Island", "Swamp");
        assert_eq!(token_value(&text, "w1"), want, "ts(A)={ta} ts(B)={tb}");
        assert_eq!(token_value(&text, "w2"), want, "ts(A)={ta} ts(B)={tb}");
        assert_eq!(
            order,
            vec![
                OrderItem::Effect(eid(first)),
                OrderItem::Effect(eid(second))
            ],
            "ts(A)={ta} ts(B)={tb}"
        );
    }
}

/// REQ-LAYER-005, REQ-CORE-001 (B3): for the B1 and B2 inputs, a `LayerCache` gives exactly
/// the uncached evaluation, repeatedly, and two independent runs agree.
#[test]
fn mixed_text_cache_equals_uncached() {
    for (ta, tb) in [(5, 6), (6, 5)] {
        for b_to in ["Forest", "Swamp"] {
            // run_mixed asserts cached == uncached; a second run must be identical.
            assert_eq!(
                run_mixed(ta, tb, "Island", "Island", b_to),
                run_mixed(ta, tb, "Island", "Island", b_to),
                "ts(A)={ta} ts(B)={tb} B->{b_to}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 8. acceptance scenario
// ---------------------------------------------------------------------------------------------

const S_ID: u64 = 10;
const X_ID: u64 = 11;
const Y_ID: u64 = 12;
const ANTHEM: u64 = 50;
const TEXT_CHANGE: u64 = 51;

/// The dependent ability's selector: `{"pred":"HAS_SUBTYPE","value":{"textToken":"t14"}}`.
fn goblin_selector() -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(PredicateAST::HasSubtype {
            value: WordRef::TextToken("t14".into()),
        })),
    })
}

/// The scenario's objects: S (the source, text [WORD t14 CREATURE_TYPE "Goblin"]), X (a
/// Goblin), Y (an Elf).
fn scenario_env(source_text: &Stream) -> TestEnv {
    let mut env = TestEnv::new();
    env.put(
        oid(S_ID),
        Characteristics {
            rules_text: enc(vec![word("t14", CreatureType, "Goblin")]),
            ..enchantment("test.source")
        },
    );
    env.put(
        oid(X_ID),
        Characteristics {
            subtypes: set(&["Goblin"]),
            ..creature("test.x_goblin", 2, 2)
        },
    );
    env.put(
        oid(Y_ID),
        Characteristics {
            subtypes: set(&["Elf"]),
            ..creature("test.y_elf", 2, 2)
        },
    );
    // `matches` resolves the selector through the source's text, then tests the candidate's
    // subtypes against the resolved Name.
    let sel = goblin_selector();
    let text = source_text.clone();
    env.add_selector(
        sel.clone(),
        Rc::new(move |ctx: &MatchContext<'_>| {
            let resolved = resolve_selector_words(&sel, &text).expect("selector resolves");
            match resolved {
                ObjectSelectorAST::Filter(f) => match f.predicate.as_deref() {
                    Some(PredicateAST::HasSubtype {
                        value: WordRef::Name(n),
                    }) => ctx.candidate_view.characteristics.subtypes.contains(n),
                    other => panic!("unexpected resolved predicate {other:?}"),
                },
                other => panic!("unexpected resolved selector {other:?}"),
            }
        }),
    );
    env
}

fn scenario_registry(text_change: bool) -> ContinuousEffects {
    let mut v = vec![(
        ANTHEM,
        static_eff(
            oid(S_ID),
            None,
            Layer::L7c,
            3,
            Affected::Selector(goblin_selector()),
            modify_pt(1, 1),
        ),
    )];
    if text_change {
        v.push((
            TEXT_CHANGE,
            text_eff(9, &[oid(S_ID)], CreatureType, "Goblin", "Elf"),
        ));
    }
    registry(&v)
}

/// An evaluated power and toughness.
type Pt = (Option<i64>, Option<i64>);

/// Two-stage evaluation (cross-object evaluation is gap #186): stage 1 evaluates S, stage 2
/// evaluates X and Y with the selector resolved through S's evaluated text.
fn run_scenario(text_change: bool) -> (Pt, Pt) {
    let r = scenario_registry(text_change);
    let base_text = stream(vec![word("t14", CreatureType, "Goblin")]);
    let stage1 = scenario_env(&base_text);
    let s_text = eval_text(&r, &stage1, oid(S_ID));
    let stage2 = scenario_env(&s_text);
    let x = evaluate(&r, &stage2, oid(X_ID)).expect("evaluate X");
    let y = evaluate(&r, &stage2, oid(Y_ID)).expect("evaluate Y");
    // The registered selector still holds the token id, not the resolved word.
    assert_eq!(
        r.get(eid(ANTHEM)).expect("registered").affected,
        Affected::Selector(goblin_selector())
    );
    (pt(&x), pt(&y))
}

/// REQ-DSL-012 (acceptance: "synthetic text-change scenario changes what a dependent ability
/// applies to"); CR 612.1, 612.2a, 613.1c: without a text effect the Goblin gets +1/+1 and the
/// Elf does not; with CREATURE_TYPE Goblin->Elf on the source, the Elf gets +1/+1 and the Goblin
/// does not.
#[test]
fn acceptance_text_change_retargets_dependent_ability() {
    let (x, y) = run_scenario(false);
    assert_eq!(x, spt(3, 3));
    assert_eq!(y, spt(2, 2));
    let (x, y) = run_scenario(true);
    assert_eq!(x, spt(2, 2));
    assert_eq!(y, spt(3, 3));
}

/// REQ-DSL-012; CR 612.1: stage 1 alone: the source's t14 word reads "Elf" after the text
/// effect, and the registered selector is unchanged.
#[test]
fn acceptance_source_text_rewritten() {
    let r = scenario_registry(true);
    let env = scenario_env(&stream(vec![word("t14", CreatureType, "Goblin")]));
    let s = eval_text(&r, &env, oid(S_ID));
    assert_eq!(s, stream(vec![word("t14", CreatureType, "Elf")]));
    match &r.get(eid(ANTHEM)).expect("registered").affected {
        Affected::Selector(ObjectSelectorAST::Filter(f)) => assert_eq!(
            f.predicate.as_deref(),
            Some(&PredicateAST::HasSubtype {
                value: WordRef::TextToken("t14".into())
            })
        ),
        other => panic!("unexpected {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// 10. registration
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012, REQ-LAYER-001; CR 613.1c: ReplaceTextWord outside layer 3 is LayerMismatch.
#[test]
fn registration_layer_mismatch() {
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
    for layer in [
        Layer::L1a,
        Layer::L1b,
        Layer::L2,
        Layer::L4,
        Layer::L5,
        Layer::L6,
        Layer::L7a,
        Layer::L7b,
        Layer::L7c,
        Layer::L7d,
    ] {
        let mut r = ContinuousEffects::new();
        let e = eff(
            layer,
            5,
            objs(&[a]),
            replace(BasicLandType, "Swamp", "Island"),
        );
        assert_eq!(
            r.register(eid(1), e.clone()),
            Err(LayerError::LayerMismatch { layer })
        );
        assert_eq!(
            r.register_group(&env, vec![(eid(1), e)]),
            Err(LayerError::LayerMismatch { layer })
        );
        assert!(r.is_empty());
    }
}

/// REQ-DSL-012, REQ-LAYER-001; CR 613.1c: layer 3 accepts the four supported categories with
/// both a locked set and a selector, through all three entry points.
#[test]
fn registration_accepts_supported_categories() {
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
    let mut n = 0u64;
    for cat in [ColorWord, BasicLandType, CreatureType, Number] {
        for affected in [objs(&[a]), by_sel(SEL_ALL)] {
            let e = eff(Layer::L3, 5, affected, replace(cat, "3", "4"));
            let mut r = ContinuousEffects::new();
            n += 1;
            assert_eq!(r.register(eid(n), e.clone()), Ok(()));
            n += 1;
            assert_eq!(r.register_group(&env, vec![(eid(n), e.clone())]), Ok(()));
            n += 1;
            assert_eq!(
                r.register_group_for_as_long_as(
                    &env,
                    ForAsLongAsWindow {
                        starts: true,
                        ended_before_first_application: false,
                        begins_again: false,
                    },
                    vec![(eid(n), e)],
                ),
                Ok(true)
            );
            assert_eq!(r.len(), 3);
        }
    }
}

/// REQ-DSL-012, REQ-LAYER-001: an empty or noncharacter `from` or `to` is Unsupported through
/// all three entry points, registry unchanged.
#[test]
fn registration_rejects_invalid_words() {
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
    let mut r = registry(&[(1, text_eff(5, &[a], BasicLandType, "Swamp", "Island"))]);
    for cat in [ColorWord, BasicLandType, CreatureType, Number] {
        for (from, to) in [
            ("", "Island"),
            ("Swamp", ""),
            ("Sw\u{FFFF}amp", "Island"),
            ("Swamp", "\u{FFFF}"),
        ] {
            let e = eff(Layer::L3, 6, objs(&[a]), replace(cat, from, to));
            for (i, x) in register_all_three(&mut r, &env, 2, e).iter().enumerate() {
                assert!(
                    matches!(x, Err(LayerError::Unsupported(_))),
                    "{cat:?} {from:?} {to:?} entry {i}: {x:?}"
                );
            }
            assert_eq!(r.len(), 1);
        }
    }
}

/// REQ-DSL-012, REQ-LAYER-001; CR 613.1c: a group with a valid part and an invalid
/// ReplaceTextWord part registers nothing (all or nothing).
#[test]
fn registration_group_all_or_nothing() {
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
    let mut r = ContinuousEffects::new();
    let parts = vec![
        (
            eid(1),
            eff(Layer::L4, 5, objs(&[a]), add_card_types(&["Artifact"])),
        ),
        (
            eid(2),
            eff(Layer::L3, 5, objs(&[a]), replace(CardName, "x", "y")),
        ),
    ];
    assert!(matches!(
        r.register_group(&env, parts),
        Err(LayerError::Unsupported(_))
    ));
    assert!(r.is_empty());
}

/// REQ-DSL-012, REQ-LAYER-001; CR 613.1c: a multi-part group (L3 text part plus L4 AddTypes
/// part on one locked set) registers and both parts apply.
#[test]
fn registration_multi_part_group_applies() {
    let mut env = TestEnv::new();
    let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
    let mut r = ContinuousEffects::new();
    r.register_group(
        &env,
        vec![
            (
                eid(1),
                eff(
                    Layer::L3,
                    5,
                    objs(&[a]),
                    replace(BasicLandType, "Swamp", "Island"),
                ),
            ),
            (
                eid(2),
                eff(Layer::L4, 5, objs(&[a]), add_card_types(&["Artifact"])),
            ),
        ],
    )
    .expect("group registers");
    assert_eq!(r.len(), 2);
    let e = evaluate(&r, &env, a).expect("evaluate");
    assert!(e.characteristics.card_types.contains("Artifact"));
    assert_eq!(
        token_value(&dec(&e.characteristics.rules_text), "t1"),
        "Island"
    );
}

/// REQ-DSL-012, REQ-LAYER-002; CR 613.1c, 613.7, 613.8a, 613.8b: the word replacement depends
/// on the whole-text SetText (applying the SetText changes what the replacement applies to,
/// CR 613.8a) and the SetText does not depend on the replacement, so the SetText applies first
/// in BOTH timestamp orders (CR 613.8b) and its new text has the land-type word rewritten.
/// Orchestrator correction of the FP-036 brief (requirement 10), backed by a rules-researcher
/// answer: the brief line "ReplaceTextWord earlier and SetText later, the SetText text wins
/// unchanged" is superseded.
#[test]
fn set_text_and_replace_both_orders() {
    use fineprint_core::layers::Selection;
    const SET: u64 = 1;
    const REP: u64 = 2;
    let replacement = vec![lit_t("r0", "test.new"), word("r1", BasicLandType, "Swamp")];
    let want = stream(vec![
        lit_t("r0", "test.new"),
        word("r1", BasicLandType, "Island"),
    ]);
    for (t_set, t_rep) in [(5, 6), (6, 5)] {
        let mut env = TestEnv::new();
        let a = env.put(oid(A), with_text("test.text_a", req1_tokens()));
        let r = registry(&[
            (
                SET,
                eff(
                    Layer::L3,
                    t_set,
                    objs(&[a]),
                    LayerModification::SetText(enc(replacement.clone())),
                ),
            ),
            (REP, text_eff(t_rep, &[a], BasicLandType, "Swamp", "Island")),
        ]);
        let (e, rec) = evaluate_with_order(&r, &env, a).expect("evaluate");
        let ctx = format!("SetText ts {t_set}, replace ts {t_rep}");
        assert_eq!(dec(&e.characteristics.rules_text), want, "{ctx}");
        assert_eq!(e, evaluate(&r, &env, a).expect("evaluate"), "{ctx}");
        let l3: Vec<_> = rec.steps.iter().filter(|s| s.layer == Layer::L3).collect();
        assert_eq!(
            l3.iter().map(|s| s.item.clone()).collect::<Vec<_>>(),
            vec![OrderItem::Effect(eid(SET)), OrderItem::Effect(eid(REP))],
            "{ctx}"
        );
        assert!(l3.iter().all(|s| s.applied), "{ctx}");
        if t_set > t_rep {
            // CR 613.8b: the later SetText is released first by dependency.
            assert_eq!(l3[0].selection, Selection::Dependency, "{ctx}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 11. no-op rules
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-012, REQ-CORE-001, REQ-LAYER-005: placeholder garbage and empty text evaluate Ok and
/// unchanged; an effect matching no token leaves the text equal to the base.
#[test]
fn no_op_rules() {
    let garbage = card::TextTokenStream(vec!["test.word".into()]);
    let empty = card::TextTokenStream(Vec::new());
    let unmatched = enc(req1_tokens());
    for base_text in [garbage, empty, unmatched] {
        let mut env = TestEnv::new();
        let a = env.put(
            oid(A),
            Characteristics {
                rules_text: base_text.clone(),
                ..enchantment("test.noop")
            },
        );
        let r = registry(&[
            (1, text_eff(5, &[a], BasicLandType, "Plains", "Island")),
            (2, text_eff(6, &[a], ColorWord, "Green", "Blue")),
        ]);
        let e = evaluate(&r, &env, a).expect("evaluate is Ok");
        assert_eq!(e.characteristics.rules_text.0, base_text.0);
        assert_eq!(e.characteristics, norm(env.base_chars(a)));
    }
}

/// REQ-DSL-012, REQ-CORE-001: garbage placeholder text under a matching-looking replacement
/// stays unchanged too (the effect is a no-op, never an error).
#[test]
fn no_op_on_undecodable_text_with_effect() {
    let garbage = card::TextTokenStream(vec!["Swamp".into()]);
    let mut env = TestEnv::new();
    let a = env.put(
        oid(A),
        Characteristics {
            rules_text: garbage.clone(),
            ..enchantment("test.noop")
        },
    );
    let r = registry(&[(1, text_eff(5, &[a], BasicLandType, "Swamp", "Island"))]);
    let e = evaluate(&r, &env, a).expect("Ok");
    assert_eq!(e.characteristics.rules_text, garbage);
}

// ---------------------------------------------------------------------------------------------
// 12. purity and determinism
// ---------------------------------------------------------------------------------------------

fn purity_world() -> (TestEnv, Vec<(u64, ContinuousEffect)>) {
    let mut env = TestEnv::new();
    let a = env.put(
        oid(A),
        Characteristics {
            rules_text: enc(req1_tokens()),
            ..creature("test.text_a", 1, 1)
        },
    );
    let b = env.put(oid(B), with_text("test.text_b", mixed_tokens()));
    let c = env.put(oid(3), with_text("test.text_c", swamp_only()));
    let effects = vec![
        (1, text_eff(5, &[a, b], BasicLandType, "Swamp", "Island")),
        (2, text_eff(6, &[b, c], BasicLandType, "Island", "Forest")),
        (
            3,
            eff(
                Layer::L3,
                7,
                by_sel(SEL_CREATURE),
                replace(ColorWord, "Red", "Blue"),
            ),
        ),
        (4, text_eff(8, &[b], CreatureType, "Goblin", "Elf")),
        (
            5,
            eff(
                Layer::L2,
                9,
                objs(&[a, b, c]),
                LayerModification::SetController(P2),
            ),
        ),
        (
            6,
            eff(Layer::L4, 4, objs(&[a, c]), add_card_types(&["Artifact"])),
        ),
        (7, text_eff(3, &[c], Number, "3", "4")),
    ];
    (env, effects)
}

/// REQ-LAYER-001, REQ-LAYER-005, REQ-CORE-001: evaluate twice equal; evaluate_all equals
/// per-object evaluate; fixed shuffled registration orders agree; the cache agrees with the
/// uncached evaluation and the digests are equal.
#[test]
fn purity_and_determinism() {
    let (env, effects) = purity_world();
    let r = registry(&effects);
    let all = evaluate_all(&r, &env).expect("evaluate_all");
    for id in env.objects_ids() {
        let e1 = evaluate(&r, &env, id).expect("evaluate");
        let e2 = evaluate(&r, &env, id).expect("evaluate");
        assert_eq!(e1, e2);
        assert_eq!(all[&id], e1);
    }
    let mut rng = Lcg::new(0x37);
    for _ in 0..8 {
        let mut order = effects.clone();
        rng.shuffle(&mut order);
        let r2 = registry(&order);
        assert_eq!(evaluate_all(&r2, &env).expect("evaluate_all"), all);
    }
    let mut cache = LayerCache::new();
    let cached = cache.evaluate_all(&r, &env).expect("cached");
    assert_eq!(cached, all);
    assert_eq!(evaluated_digest(&cached), evaluated_digest(&all));
    let cached_again = cache.evaluate_all(&r, &env).expect("cached again");
    assert_eq!(cached_again, all);
    assert_eq!(evaluated_digest(&cached_again), evaluated_digest(&all));
    for id in env.objects_ids() {
        assert_eq!(cache.evaluate(&r, &env, id).expect("cached"), all[&id]);
    }
    // The base is untouched.
    let (fresh, _) = purity_world();
    assert_eq!(env.bases, fresh.bases);
}

/// REQ-LAYER-001, REQ-CORE-001; CR 613.1c: in the order record a ReplaceTextWord step comes
/// after every layer-2 step and before every layer-4 step.
#[test]
fn order_record_places_text_steps_in_layer_three() {
    let (env, effects) = purity_world();
    let r = registry(&effects);
    let text_ids: BTreeSet<u64> = [1, 2, 3, 4, 7].into_iter().collect();
    for id in env.objects_ids() {
        let (e, rec) = evaluate_with_order(&r, &env, id).expect("evaluate");
        assert_eq!(e, evaluate(&r, &env, id).expect("evaluate"));
        let pos = |pred: &dyn Fn(&fineprint_core::layers::OrderStep) -> bool| -> Vec<usize> {
            rec.steps
                .iter()
                .enumerate()
                .filter(|(_, s)| pred(s))
                .map(|(i, _)| i)
                .collect()
        };
        let text_pos =
            pos(&|s| matches!(&s.item, OrderItem::Effect(x) if text_ids.contains(&x.get())));
        assert!(!text_pos.is_empty(), "object {id:?} has text steps");
        let l2 = pos(&|s| s.layer == Layer::L2);
        let l4 = pos(&|s| s.layer == Layer::L4);
        for t in &text_pos {
            assert_eq!(rec.steps[*t].layer, Layer::L3);
            assert!(l2.iter().all(|p| p < t));
            assert!(l4.iter().all(|p| p > t));
        }
    }
}

trait ObjectIds {
    fn objects_ids(&self) -> Vec<ObjectId>;
}

impl ObjectIds for TestEnv {
    fn objects_ids(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }
}
