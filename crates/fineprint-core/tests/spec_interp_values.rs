// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for integer value expressions: LITERAL, X, COUNT, ATTR, PLAYER_ATTR, MATH,
//! DISTINCT and MEMORY (REQ-DSL-003, REQ-CORE-006; SRS 14.4.0). Black-box: written from the
//! work-item brief only. No floating point anywhere (clippy `disallowed-types`).

#[path = "spec_interp_support.rs"]
mod support;

use fineprint_core::interp::{EvalContext, InterpError, PlayerStat, eval_value};
use fineprint_core::memory::ChoiceValue;
use support::*;

fn val(env: &Env, c: &EvalContext, v: &ValueExpressionAST) -> i64 {
    eval_value(env, c, v).expect("no error")
}

// REQ-DSL-003: LITERAL and X.
#[test]
fn req_dsl_003_literal_and_x() {
    let env = Env::new();
    let mut c = ctx(P1);
    for n in [0, 1, -1, 42, i64::MIN, i64::MAX] {
        assert_eq!(val(&env, &c, &lit(n)), n);
    }
    assert_eq!(
        eval_value(&env, &c, &ValueExpressionAST::X),
        Err(InterpError::MissingX)
    );
    c.x = Some(7);
    assert_eq!(val(&env, &c, &ValueExpressionAST::X), 7);
    c.x = Some(0);
    assert_eq!(val(&env, &c, &ValueExpressionAST::X), 0);
}

// REQ-DSL-003: COUNT is the number of selected objects, capped by the selector's count.
#[test]
fn req_dsl_003_count() {
    let mut env = Env::new();
    env.add(creature(1, Zone::Battlefield, P1, 1, 1));
    env.add(creature(2, Zone::Battlefield, P2, 1, 1));
    env.add(land(3, Zone::Battlefield, P1));
    env.add(creature(4, Zone::Graveyard, P1, 1, 1));
    let c = ctx(P1);
    assert_eq!(val(&env, &c, &count(bf(PredicateAST::IsCreature))), 2);
    assert_eq!(val(&env, &c, &count(bf(PredicateAST::IsToken))), 0);
    assert_eq!(
        val(
            &env,
            &c,
            &count(in_zones(&[Zone::Graveyard, Zone::Battlefield], None))
        ),
        4
    );
    let capped = filter(None, None, None, Some(lit(1)));
    assert_eq!(val(&env, &c, &count(capped)), 1);
    let src = ctx_src(P1, oid(1));
    assert_eq!(
        val(
            &env,
            &src,
            &count(ObjectSelectorAST::Ref(ReferenceAST::SelfRef))
        ),
        1
    );
}

// REQ-DSL-003: ATTR for every ObjectAttr; None counts as 0 (CR 208.5 repo convention).
#[test]
fn req_dsl_003_attr_every_attribute() {
    let mut env = Env::new();
    let mut v = creature(1, Zone::Battlefield, P1, 5, -2);
    v.mana_value = 4;
    v.loyalty = Some(3);
    v.defense = Some(6);
    v.characteristics.colors = colors(1 | 4 | 16);
    v.counters.insert("charge".into(), 2);
    env.add(v);
    env.add(artifact(2, Zone::Battlefield, P1)); // nothing set
    let rows = [
        (ObjectAttr::Power, 5, 0),
        (ObjectAttr::Toughness, -2, 0),
        (ObjectAttr::ManaValue, 4, 0),
        (ObjectAttr::Loyalty, 3, 0),
        (ObjectAttr::Defense, 6, 0),
        (ObjectAttr::ColorCount, 3, 0),
        (ObjectAttr::Counters("charge".into()), 2, 0),
        (ObjectAttr::Counters("other".into()), 0, 0),
    ];
    for (a, full, empty) in rows {
        assert_eq!(
            val(
                &env,
                &ctx_src(P1, oid(1)),
                &attr(ReferenceAST::SelfRef, a.clone())
            ),
            full,
            "{a:?}"
        );
        assert_eq!(
            val(
                &env,
                &ctx_src(P1, oid(2)),
                &attr(ReferenceAST::SelfRef, a.clone())
            ),
            empty,
            "{a:?} empty"
        );
        // No items: 0, never an error.
        assert_eq!(
            val(&env, &ctx(P1), &attr(ReferenceAST::SelfRef, a.clone())),
            0
        );
    }
}

// REQ-DSL-003: ATTR sums over every object item; player items contribute 0.
#[test]
fn req_dsl_003_attr_sums_over_items() {
    let mut env = Env::new();
    env.add(creature(1, Zone::Battlefield, P1, 2, 3));
    env.add(creature(2, Zone::Battlefield, P2, 4, 1));
    let mut c = ctx(P1);
    c.bindings
        .insert("$all".into(), vec![obj(1), pl(P2), obj(2)]);
    assert_eq!(val(&env, &c, &attr(bound("$all"), ObjectAttr::Power)), 6);
    assert_eq!(
        val(&env, &c, &attr(bound("$all"), ObjectAttr::Toughness)),
        4
    );
    assert_eq!(
        val(&env, &c, &attr(ReferenceAST::Controller, ObjectAttr::Power)),
        0
    );
    // An unbound var is an error.
    assert_eq!(
        eval_value(&env, &c, &attr(bound("$x"), ObjectAttr::Power)),
        Err(InterpError::UnboundVariable("$x".into()))
    );
}

// REQ-DSL-003, CR 608.2h / 113.7a: ATTR of a source that left uses its last known information.
#[test]
fn req_dsl_003_attr_source_lki() {
    let mut env = Env::new();
    env.add(creature(1, Zone::Battlefield, P1, 3, 3));
    let c = ctx_src(P1, oid(1));
    env.vanish_with_lki(oid(1));
    assert_eq!(
        val(&env, &c, &attr(ReferenceAST::SourceLki, ObjectAttr::Power)),
        3
    );
    assert_eq!(
        val(&env, &c, &attr(ReferenceAST::SelfRef, ObjectAttr::Power)),
        3
    );
    env.vanish(oid(1));
    assert_eq!(
        val(&env, &c, &attr(ReferenceAST::SourceLki, ObjectAttr::Power)),
        0
    );
}

// REQ-DSL-003: PLAYER_ATTR maps every attribute one to one to PlayerStat and sums over players.
#[test]
fn req_dsl_003_player_attr_every_attribute() {
    let mut env = Env::three(Some(P1));
    let stats = [
        (PlayerAttr::Life, PlayerStat::Life),
        (PlayerAttr::Speed, PlayerStat::Speed),
        (PlayerAttr::HandSize, PlayerStat::HandSize),
        (PlayerAttr::LibrarySize, PlayerStat::LibrarySize),
        (PlayerAttr::GraveyardSize, PlayerStat::GraveyardSize),
        (
            PlayerAttr::Counters("poison".into()),
            PlayerStat::Counters("poison".into()),
        ),
    ];
    // Distinct values per (player, stat) so a wrong mapping is visible.
    for (i, (_, s)) in stats.iter().enumerate() {
        let base = (i as i64 + 1) * 100;
        env.set_stat(P1, s, base + 1);
        env.set_stat(P2, s, base + 2);
        env.set_stat(P3, s, base + 3);
    }
    for (i, (a, _)) in stats.iter().enumerate() {
        let base = (i as i64 + 1) * 100;
        let c = ctx(P2);
        assert_eq!(
            val(&env, &c, &pattr(ReferenceAST::Controller, a.clone())),
            base + 2,
            "{a:?}"
        );
        assert_eq!(
            val(&env, &c, &pattr(ReferenceAST::EachOpponent, a.clone())),
            2 * base + 4,
            "{a:?} opponents"
        );
        // Nobody resolved: 0.
        assert_eq!(
            val(&env, &c, &pattr(ReferenceAST::ThatPlayer, a.clone())),
            0,
            "{a:?} none"
        );
    }
    // Unknown counters kind: 0.
    assert_eq!(
        val(
            &env,
            &ctx(P1),
            &pattr(ReferenceAST::Controller, PlayerAttr::Counters("x".into()))
        ),
        0
    );
    // An object referent contributes 0.
    env.add(creature(5, Zone::Battlefield, P1, 9, 9));
    assert_eq!(
        val(
            &env,
            &ctx_src(P1, oid(5)),
            &pattr(ReferenceAST::SelfRef, PlayerAttr::Life)
        ),
        0
    );
}

// REQ-DSL-003: devotion with fixed and text-token colour words (SRS 14.5): a rewritten word
// changes the colours passed to the environment.
#[test]
fn req_dsl_003_player_attr_devotion() {
    let mut env = Env::new();
    env.add(view(40, Zone::Battlefield, P1, &["Enchantment"]));
    env.set_token(oid(40), "t1", "red");
    env.devotion_table.insert((P1, "red".into()), 3);
    env.devotion_table.insert((P1, "green".into()), 5);
    env.devotion_table.insert((P1, "blue".into()), 1);
    env.devotion_table.insert((P2, "red".into()), 4);
    let c = ctx_src(P1, oid(40));
    let dev = |words: Vec<WordRef>| PlayerAttr::Devotion(words);
    assert_eq!(
        val(
            &env,
            &c,
            &pattr(ReferenceAST::Controller, dev(vec![token("t1")]))
        ),
        3
    );
    assert_eq!(
        env.devotion_calls.borrow().last().cloned(),
        Some((P1, vec!["red".to_string()]))
    );
    assert_eq!(
        val(
            &env,
            &c,
            &pattr(
                ReferenceAST::Controller,
                dev(vec![name("blue"), token("t1")])
            )
        ),
        4
    );
    assert_eq!(
        env.devotion_calls.borrow().last().cloned(),
        Some((P1, vec!["blue".to_string(), "red".to_string()]))
    );
    // Rewritten token.
    env.set_token(oid(40), "t1", "green");
    assert_eq!(
        val(
            &env,
            &c,
            &pattr(ReferenceAST::Controller, dev(vec![token("t1")]))
        ),
        5
    );
    assert_eq!(
        env.devotion_calls.borrow().last().cloned(),
        Some((P1, vec!["green".to_string()]))
    );
    // Summed over players.
    env.set_token(oid(40), "t1", "red");
    let mut both = c.clone();
    both.bindings.insert("$ps".into(), vec![pl(P1), pl(P2)]);
    assert_eq!(
        val(&env, &both, &pattr(bound("$ps"), dev(vec![token("t1")]))),
        7
    );
}

/// The MATH table: (op, lhs, rhs, expected).
fn math_rows() -> Vec<(MathOp, i64, i64, Result<i64, InterpError>)> {
    use MathOp::*;
    vec![
        (Add, 2, 3, Ok(5)),
        (Add, -2, -3, Ok(-5)),
        (Add, i64::MAX, 1, Ok(i64::MAX)),
        (Add, i64::MIN, -1, Ok(i64::MIN)),
        (Sub, 2, 3, Ok(-1)),
        (Sub, i64::MIN, 1, Ok(i64::MIN)),
        (Sub, i64::MAX, -1, Ok(i64::MAX)),
        (Sub, 0, i64::MIN, Ok(i64::MAX)),
        (Mul, 4, -3, Ok(-12)),
        (Mul, i64::MAX, 2, Ok(i64::MAX)),
        (Mul, i64::MIN, 2, Ok(i64::MIN)),
        (Mul, i64::MIN, -1, Ok(i64::MAX)),
        (Mul, i64::MAX, -2, Ok(i64::MIN)),
        (DivFloor, 7, 2, Ok(3)),
        (DivFloor, -7, 2, Ok(-4)),
        (DivFloor, 7, -2, Ok(-4)),
        (DivFloor, -7, -2, Ok(3)),
        (DivFloor, 6, 3, Ok(2)),
        (DivFloor, -6, 3, Ok(-2)),
        (DivFloor, 0, 5, Ok(0)),
        (DivFloor, 1, 0, Err(InterpError::DivisionByZero)),
        (DivFloor, 0, 0, Err(InterpError::DivisionByZero)),
        (DivFloor, i64::MIN, -1, Ok(i64::MAX)),
        (DivFloor, i64::MIN, 1, Ok(i64::MIN)),
        (DivCeil, 7, 2, Ok(4)),
        (DivCeil, -7, 2, Ok(-3)),
        (DivCeil, 7, -2, Ok(-3)),
        (DivCeil, -7, -2, Ok(4)),
        (DivCeil, 6, 3, Ok(2)),
        (DivCeil, -6, 3, Ok(-2)),
        (DivCeil, 1, 0, Err(InterpError::DivisionByZero)),
        (DivCeil, i64::MIN, -1, Ok(i64::MAX)),
        (DivCeil, i64::MAX, 2, Ok(i64::MAX / 2 + 1)),
        (Min, 3, -4, Ok(-4)),
        (Min, i64::MIN, i64::MAX, Ok(i64::MIN)),
        (Max, 3, -4, Ok(3)),
        (Max, i64::MIN, i64::MAX, Ok(i64::MAX)),
    ]
}

// REQ-DSL-003, REQ-CORE-006: MATH with every operator; saturation; floor/ceil rounding;
// division by zero is an error.
#[test]
fn req_dsl_003_req_core_006_math_table() {
    let env = Env::new();
    let c = ctx(P1);
    for (op, l, r, want) in math_rows() {
        assert_eq!(
            eval_value(&env, &c, &math(op, lit(l), lit(r))),
            want,
            "{op:?}({l}, {r})"
        );
    }
}

// REQ-DSL-003: MATH evaluates lhs first; an lhs error wins over an rhs error.
#[test]
fn req_dsl_003_math_lhs_error_wins() {
    let env = Env::new();
    let c = ctx(P1);
    let div0 = math(MathOp::DivFloor, lit(1), lit(0));
    assert_eq!(
        eval_value(
            &env,
            &c,
            &math(MathOp::Add, ValueExpressionAST::X, div0.clone())
        ),
        Err(InterpError::MissingX)
    );
    assert_eq!(
        eval_value(&env, &c, &math(MathOp::Add, div0, ValueExpressionAST::X)),
        Err(InterpError::DivisionByZero)
    );
    assert_eq!(
        eval_value(&env, &c, &math(MathOp::Max, lit(1), ValueExpressionAST::X)),
        Err(InterpError::MissingX)
    );
    // Nested.
    let nested = math(
        MathOp::Mul,
        math(MathOp::Add, lit(2), lit(3)),
        math(MathOp::DivCeil, lit(7), lit(2)),
    );
    assert_eq!(val(&env, &c, &nested), 20);
}

/// Graveyard objects for DISTINCT and the card-type threshold scenario.
fn graveyard_env() -> Env {
    let mut env = Env::new();
    let mut a = creature(1, Zone::Graveyard, P1, 2, 2);
    a.characteristics.card_types = set(&["Artifact", "Creature"]);
    a.characteristics.subtypes = set(&["Testkin", "Golemoid"]);
    a.characteristics.supertypes = set(&["Legendary"]);
    a.characteristics.colors = colors(1 | 2);
    a.characteristics.names = set(&["test.walker_2_2"]);
    a.mana_value = 3;
    let mut b = instant(2, Zone::Graveyard, P2);
    b.characteristics.colors = colors(2 | 8);
    b.characteristics.names = set(&["test.spark"]);
    b.mana_value = 3;
    let mut d = land(3, Zone::Graveyard, P1);
    d.characteristics.subtypes = set(&["Testkin"]);
    d.characteristics.names = set(&["test.walker_2_2"]);
    d.mana_value = 0;
    let mut e = creature(4, Zone::Graveyard, P2, 2, 5);
    e.characteristics.names = set(&["test.split_a", "test.split_b"]);
    e.mana_value = 1;
    env.add_all([a, b, d, e]);
    // A battlefield object that must not be counted.
    let mut f = view(5, Zone::Battlefield, P1, &["Enchantment"]);
    f.characteristics.colors = colors(16);
    env.add(f);
    env
}

// REQ-DSL-003: DISTINCT for every attribute.
#[test]
fn req_dsl_003_distinct_every_attribute() {
    let env = graveyard_env();
    let c = ctx(P1);
    let gy = || in_zones(&[Zone::Graveyard], None);
    let rows = [
        (DistinctAttr::CardType, 4),  // Artifact, Creature, Instant, Land
        (DistinctAttr::Subtype, 2),   // Testkin, Golemoid
        (DistinctAttr::Supertype, 1), // Legendary
        (DistinctAttr::Color, 3),     // W, U, R
        (DistinctAttr::Name, 4),      // walker, spark, split_a, split_b
        (DistinctAttr::ManaValue, 3), // 3, 0, 1
        (DistinctAttr::Power, 1),     // 2 (None skipped)
        (DistinctAttr::Toughness, 2), // 2, 5
    ];
    for (a, want) in rows {
        assert_eq!(val(&env, &c, &distinct(a, gy())), want, "{a:?}");
        // Nothing selected: 0.
        assert_eq!(
            val(&env, &c, &distinct(a, in_zones(&[Zone::Exile], None))),
            0,
            "{a:?} empty"
        );
    }
    // All five colours.
    let mut env5 = Env::new();
    env5.add({
        let mut v = view(1, Zone::Battlefield, P1, &["Artifact"]);
        v.characteristics.colors = colors(31);
        v
    });
    assert_eq!(
        val(
            &env5,
            &c,
            &distinct(DistinctAttr::Color, filter(None, None, None, None))
        ),
        5
    );
}

// REQ-DSL-003: "four or more card types among cards in graveyards" threshold as COMPARE of a
// DISTINCT count (a card-type-count ability word pattern).
#[test]
fn req_dsl_003_card_type_threshold_scenario() {
    let mut env = graveyard_env();
    let c = ctx(P1);
    let threshold = PredicateAST::Compare {
        cmp: Cmp::Ge,
        lhs: Box::new(distinct(
            DistinctAttr::CardType,
            in_zones(&[Zone::Graveyard], None),
        )),
        rhs: blit(4),
    };
    let s = view(9, Zone::Battlefield, P1, &["Creature"]);
    assert_eq!(
        fineprint_core::interp::eval_predicate_object(&env, &c, &threshold, &s),
        Ok(true)
    );
    // Only P1's graveyard: Artifact, Creature, Land = 3.
    let mine = PredicateAST::Compare {
        cmp: Cmp::Ge,
        lhs: Box::new(distinct(
            DistinctAttr::CardType,
            filter(
                Some(vec![Zone::Graveyard]),
                Some(PredicateAST::OwnedBy {
                    player: pref(ReferenceAST::Controller),
                }),
                None,
                None,
            ),
        )),
        rhs: blit(4),
    };
    assert_eq!(
        fineprint_core::interp::eval_predicate_player(&env, &c, &mine, P1),
        Ok(false)
    );
    // Removing the instant drops the shared count to 3.
    env.vanish(oid(2));
    assert_eq!(
        fineprint_core::interp::eval_predicate_object(&env, &c, &threshold, &s),
        Ok(false)
    );
}

// REQ-DSL-003: MEMORY reads Number and Mode; every other variant, an absent key, no memory and no
// source read 0.
#[test]
fn req_dsl_003_memory() {
    let mut env = Env::new();
    env.add(view(1, Zone::Battlefield, P1, &["Artifact"]));
    env.add(view(2, Zone::Battlefield, P1, &["Artifact"]));
    let m = env.memory_mut(oid(1));
    m.set_choice("n", ChoiceValue::Number(-6));
    m.set_choice("mode", ChoiceValue::Mode(2));
    m.set_choice("color", ChoiceValue::Color("red".into()));
    m.set_choice("type", ChoiceValue::CreatureType("Testkin".into()));
    m.set_choice("cname", ChoiceValue::CardName("test.walker_2_2".into()));
    m.set_choice("who", ChoiceValue::Player(P2));
    m.set_choice("anchor", ChoiceValue::Anchor("first".into()));
    let c = ctx_src(P1, oid(1));
    let mem = |k: &str| ValueExpressionAST::Memory(k.into());
    assert_eq!(val(&env, &c, &mem("n")), -6);
    assert_eq!(val(&env, &c, &mem("mode")), 2);
    for k in ["color", "type", "cname", "who", "anchor", "absent"] {
        assert_eq!(val(&env, &c, &mem(k)), 0, "{k}");
    }
    // Source without memory, and no source.
    assert_eq!(val(&env, &ctx_src(P1, oid(2)), &mem("n")), 0);
    assert_eq!(val(&env, &ctx(P1), &mem("n")), 0);
    // Last choice wins (one value per key).
    env.memory_mut(oid(1))
        .set_choice("n", ChoiceValue::Number(11));
    assert_eq!(val(&env, &c, &mem("n")), 11);
    // Mode at the top of its range.
    env.memory_mut(oid(1))
        .set_choice("mode", ChoiceValue::Mode(u32::MAX));
    assert_eq!(val(&env, &c, &mem("mode")), i64::from(u32::MAX));
}

// REQ-CORE-006: large operands give exact integer results (no float rounding anywhere).
#[test]
fn req_core_006_large_operands_integer_only() {
    let env = Env::new();
    let c = ctx(P1);
    let big = (1_i64 << 53) - 1; // 2^53 - 1: the largest literal the wire form allows
    // 2^53 + 1 is not representable in a 64-bit float; an integer engine gets it exactly.
    assert_eq!(val(&env, &c, &math(MathOp::Add, lit(big), lit(2))), big + 2);
    assert_eq!(
        val(&env, &c, &math(MathOp::Mul, lit(big), lit(1023))),
        big * 1023
    );
    assert_eq!(
        val(&env, &c, &math(MathOp::Mul, lit(big), lit(big))),
        i64::MAX
    );
    assert_eq!(
        val(&env, &c, &math(MathOp::DivFloor, lit(i64::MAX), lit(3))),
        i64::MAX / 3
    );
    assert_eq!(
        val(&env, &c, &math(MathOp::DivCeil, lit(i64::MAX), lit(3))),
        i64::MAX / 3 + 1
    );
    assert_eq!(
        val(&env, &c, &math(MathOp::DivFloor, lit(-big), lit(2))),
        -(big / 2) - 1
    );
    assert_eq!(
        val(
            &env,
            &c,
            &math(MathOp::Sub, lit(i64::MAX), lit(i64::MAX - 1))
        ),
        1
    );
    assert_eq!(
        val(
            &env,
            &c,
            &math(MathOp::Sub, lit(i64::MAX - 1), lit(i64::MAX))
        ),
        -1
    );
}
