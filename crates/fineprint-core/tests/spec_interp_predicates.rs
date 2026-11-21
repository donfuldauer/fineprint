// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `eval_predicate_object`, `eval_predicate_player` and `compare` over
//! layer-evaluated object views (REQ-DSL-003; SRS 14.4.0, 14.5). Black-box: written from the
//! work-item brief only.

#[path = "spec_interp_support.rs"]
mod support;

use fineprint_core::interp::{
    EvalContext, InterpError, ObjectView, compare, eval_predicate_object, eval_predicate_player,
};
use support::*;

fn on_obj(env: &Env, c: &EvalContext, p: &PredicateAST, s: &ObjectView) -> bool {
    eval_predicate_object(env, c, p, s).expect("no error")
}

fn on_player(
    env: &Env,
    c: &EvalContext,
    p: &PredicateAST,
    s: fineprint_base::ids::PlayerId,
) -> bool {
    eval_predicate_player(env, c, p, s).expect("no error")
}

/// A subject with every flag/feature off, and one with the feature the row turns on.
fn plain() -> ObjectView {
    view(20, Zone::Battlefield, P1, &["Artifact"])
}

// REQ-DSL-003: every flag and type predicate: true case, false case, and false on a player.
#[test]
fn req_dsl_003_flag_and_type_predicates() {
    let env = Env::new();
    let c = ctx(P1);
    type Tweak = fn(&mut ObjectView);
    let rows: Vec<(PredicateAST, Tweak)> = vec![
        (PredicateAST::IsCreature, |v| {
            v.characteristics.card_types.insert("Creature".into());
        }),
        (PredicateAST::IsLand, |v| {
            v.characteristics.card_types.insert("Land".into());
        }),
        (PredicateAST::IsToken, |v| v.is_token = true),
        (PredicateAST::FaceDown, |v| v.face_down = true),
        (PredicateAST::Tapped, |v| v.tapped = true),
        (PredicateAST::Attacking, |v| v.attacking = true),
        (PredicateAST::Blocking, |v| v.blocking = true),
        (PredicateAST::EnteredThisTurn, |v| {
            v.entered_this_turn = true
        }),
        (PredicateAST::WasCast, |v| v.was_cast = true),
        (PredicateAST::IsMulticolored, |v| {
            v.characteristics.colors = colors(1 | 16)
        }),
    ];
    for (p, tweak) in rows {
        let off = plain();
        let mut on = plain();
        tweak(&mut on);
        assert!(on_obj(&env, &c, &p, &on), "{p:?} on");
        assert!(!on_obj(&env, &c, &p, &off), "{p:?} off");
        assert!(!on_player(&env, &c, &p, P1), "{p:?} player");
    }
    // Inverse flags: true when the feature is off.
    let inverse: Vec<(PredicateAST, Tweak)> = vec![
        (PredicateAST::FaceUp, |v| v.face_down = true),
        (PredicateAST::Untapped, |v| v.tapped = true),
        (PredicateAST::IsColorless, |v| {
            v.characteristics.colors = colors(8)
        }),
        (PredicateAST::IsPermanent, |v| v.zone = Zone::Graveyard),
    ];
    for (p, tweak) in inverse {
        let base = plain();
        let mut changed = plain();
        tweak(&mut changed);
        assert!(on_obj(&env, &c, &p, &base), "{p:?} base");
        assert!(!on_obj(&env, &c, &p, &changed), "{p:?} changed");
        assert!(!on_player(&env, &c, &p, P1), "{p:?} player");
    }
    // A single colour is not multicoloured.
    let mut mono = plain();
    mono.characteristics.colors = colors(4);
    assert!(!on_obj(&env, &c, &PredicateAST::IsMulticolored, &mono));
    // IS_PERMANENT in every other zone is false.
    for z in Zone::ALL
        .iter()
        .copied()
        .filter(|z| *z != Zone::Battlefield)
    {
        let mut v = plain();
        v.zone = z;
        assert!(!on_obj(&env, &c, &PredicateAST::IsPermanent, &v), "{z:?}");
    }
}

// REQ-DSL-003: IS_PLAYER and IS_CARD_OR_TOKEN.
#[test]
fn req_dsl_003_is_player_and_is_card_or_token() {
    let env = Env::new();
    let c = ctx(P1);
    let mut tok = plain();
    tok.is_token = true;
    assert!(!on_obj(&env, &c, &PredicateAST::IsPlayer, &plain()));
    assert!(on_player(&env, &c, &PredicateAST::IsPlayer, P2));
    assert!(on_obj(&env, &c, &PredicateAST::IsCardOrToken, &plain()));
    assert!(on_obj(&env, &c, &PredicateAST::IsCardOrToken, &tok));
    assert!(!on_player(&env, &c, &PredicateAST::IsCardOrToken, P2));
}

// REQ-DSL-003: AND / OR / NOT on both subject kinds; AND/OR short-circuit left to right; an Err
// from an evaluated child propagates.
#[test]
fn req_dsl_003_boolean_ops() {
    let env = Env::new();
    let c = ctx(P1);
    let t = PredicateAST::IsCardOrToken;
    let f = PredicateAST::IsPlayer;
    let err = PredicateAST::Compare {
        cmp: Cmp::Eq,
        lhs: Box::new(ValueExpressionAST::X),
        rhs: blit(0),
    };
    let s = plain();
    assert!(on_obj(
        &env,
        &c,
        &PredicateAST::And(vec![t.clone(), t.clone()]),
        &s
    ));
    assert!(!on_obj(
        &env,
        &c,
        &PredicateAST::And(vec![t.clone(), f.clone()]),
        &s
    ));
    assert!(on_obj(
        &env,
        &c,
        &PredicateAST::Or(vec![f.clone(), t.clone()]),
        &s
    ));
    assert!(!on_obj(
        &env,
        &c,
        &PredicateAST::Or(vec![f.clone(), f.clone()]),
        &s
    ));
    assert!(on_obj(
        &env,
        &c,
        &PredicateAST::Not(Box::new(f.clone())),
        &s
    ));
    assert!(!on_obj(
        &env,
        &c,
        &PredicateAST::Not(Box::new(t.clone())),
        &s
    ));
    // Player subject: IS_PLAYER true, IS_CARD_OR_TOKEN false.
    assert!(on_player(&env, &c, &PredicateAST::And(vec![f.clone()]), P1));
    assert!(on_player(
        &env,
        &c,
        &PredicateAST::Or(vec![t.clone(), f.clone()]),
        P1
    ));
    assert!(on_player(
        &env,
        &c,
        &PredicateAST::Not(Box::new(t.clone())),
        P1
    ));
    assert!(!on_player(
        &env,
        &c,
        &PredicateAST::Not(Box::new(f.clone())),
        P1
    ));
    // Short-circuit: the erroring child after a deciding child is never evaluated.
    assert!(!on_obj(
        &env,
        &c,
        &PredicateAST::And(vec![f.clone(), err.clone()]),
        &s
    ));
    assert!(on_obj(
        &env,
        &c,
        &PredicateAST::Or(vec![t.clone(), err.clone()]),
        &s
    ));
    assert!(!on_player(
        &env,
        &c,
        &PredicateAST::And(vec![t.clone(), err.clone()]),
        P1
    ));
    assert!(on_player(
        &env,
        &c,
        &PredicateAST::Or(vec![f.clone(), err.clone()]),
        P1
    ));
    // An evaluated erroring child propagates.
    for p in [
        PredicateAST::And(vec![t.clone(), err.clone()]),
        PredicateAST::Or(vec![f.clone(), err.clone()]),
        PredicateAST::Not(Box::new(err.clone())),
        PredicateAST::And(vec![err.clone(), f.clone()]),
    ] {
        assert_eq!(
            eval_predicate_object(&env, &c, &p, &s),
            Err(InterpError::MissingX),
            "{p:?}"
        );
    }
    assert_eq!(
        eval_predicate_player(&env, &c, &PredicateAST::Or(vec![t, err]), P1),
        Err(InterpError::MissingX)
    );
}

// REQ-DSL-003: HAS_CARD_TYPE / HAS_SUBTYPE / HAS_SUPERTYPE are exact, case-sensitive element
// tests on the evaluated sets.
#[test]
fn req_dsl_003_has_type_subtype_supertype() {
    let env = Env::new();
    let c = ctx(P1);
    let mut v = plain();
    v.characteristics.card_types = set(&["Artifact", "Creature"]);
    v.characteristics.subtypes = set(&["Testkin"]);
    v.characteristics.supertypes = set(&["Legendary"]);
    let rows = [
        (
            PredicateAST::HasCardType {
                value: name("Creature"),
            },
            true,
        ),
        (
            PredicateAST::HasCardType {
                value: name("creature"),
            },
            false,
        ),
        (
            PredicateAST::HasCardType {
                value: name("Land"),
            },
            false,
        ),
        (
            PredicateAST::HasSubtype {
                value: name("Testkin"),
            },
            true,
        ),
        (
            PredicateAST::HasSubtype {
                value: name("Testkin "),
            },
            false,
        ),
        (
            PredicateAST::HasSubtype {
                value: name("Creature"),
            },
            false,
        ),
        (
            PredicateAST::HasSupertype {
                value: name("Legendary"),
            },
            true,
        ),
        (
            PredicateAST::HasSupertype {
                value: name("legendary"),
            },
            false,
        ),
        (
            PredicateAST::HasSupertype {
                value: name("Basic"),
            },
            false,
        ),
    ];
    for (p, want) in rows {
        assert_eq!(on_obj(&env, &c, &p, &v), want, "{p:?}");
        assert!(!on_player(&env, &c, &p, P1), "{p:?} player");
    }
}

// REQ-DSL-003: HAS_COLOR maps letters and colour names (ASCII case-insensitive) to bits; an
// unknown word is false, never an error.
#[test]
fn req_dsl_003_has_color() {
    let env = Env::new();
    let c = ctx(P1);
    let mut v = plain();
    v.characteristics.colors = colors(2 | 8); // blue, red
    let rows = [
        ("W", false),
        ("U", true),
        ("B", false),
        ("R", true),
        ("G", false),
        ("u", true),
        ("r", true),
        ("white", false),
        ("blue", true),
        ("Blue", true),
        ("BLACK", false),
        ("red", true),
        ("RED", true),
        ("green", false),
        ("purple", false),
        ("", false),
        ("bluee", false),
    ];
    for (word, want) in rows {
        let p = PredicateAST::HasColor { value: name(word) };
        assert_eq!(
            eval_predicate_object(&env, &c, &p, &v),
            Ok(want),
            "{word:?}"
        );
        assert!(!on_player(&env, &c, &p, P1), "{word:?} player");
    }
    // Every bit on its own.
    for (bit, word) in [
        (1u8, "white"),
        (2, "blue"),
        (4, "black"),
        (8, "red"),
        (16, "green"),
    ] {
        let mut one = plain();
        one.characteristics.colors = colors(bit);
        for (_, other) in [
            (1u8, "white"),
            (2, "blue"),
            (4, "black"),
            (8, "red"),
            (16, "green"),
        ] {
            let p = PredicateAST::HasColor { value: name(other) };
            assert_eq!(
                on_obj(&env, &c, &p, &one),
                other == word,
                "{word} vs {other}"
            );
        }
    }
}

// SRS 14.5, REQ-DSL-003: a TextToken word reads the current value of the token on the source;
// rewriting it changes the result; no source or unknown token is UnknownTextToken.
#[test]
fn req_dsl_003_text_token_words() {
    let mut env = Env::new();
    env.add(view(30, Zone::Battlefield, P1, &["Enchantment"]));
    env.set_token(oid(30), "t1", "red");
    env.set_token(oid(30), "t2", "Testkin");
    let c = ctx_src(P1, oid(30));
    let mut v = plain();
    v.characteristics.colors = colors(8);
    v.characteristics.subtypes = set(&["Testkin"]);
    let col = PredicateAST::HasColor { value: token("t1") };
    let sub = PredicateAST::HasSubtype { value: token("t2") };
    assert!(on_obj(&env, &c, &col, &v));
    assert!(on_obj(&env, &c, &sub, &v));
    // Rewritten word (a text-changing effect).
    env.set_token(oid(30), "t1", "green");
    env.set_token(oid(30), "t2", "Otherkin");
    assert!(!on_obj(&env, &c, &col, &v));
    assert!(!on_obj(&env, &c, &sub, &v));
    v.characteristics.colors = colors(16);
    assert!(on_obj(&env, &c, &col, &v));
    // Unknown token, and no source.
    let bad = PredicateAST::HasCardType { value: token("t9") };
    assert_eq!(
        eval_predicate_object(&env, &c, &bad, &v),
        Err(InterpError::UnknownTextToken("t9".into()))
    );
    assert_eq!(
        eval_predicate_object(&env, &ctx(P1), &col, &v),
        Err(InterpError::UnknownTextToken("t1".into()))
    );
}

// REQ-DSL-003: HAS_KEYWORD and HAS_DESIGNATION.
#[test]
fn req_dsl_003_keyword_and_designation() {
    let env = Env::new();
    let c = ctx(P1);
    let mut v = plain();
    v.keywords = set(&["FLYING"]);
    v.designations = set(&["MONARCH_SOURCE"]);
    let kw = |k: &str| PredicateAST::HasKeyword { keyword: k.into() };
    let ds = |k: &str| PredicateAST::HasDesignation { kind: k.into() };
    assert!(on_obj(&env, &c, &kw("FLYING"), &v));
    assert!(!on_obj(&env, &c, &kw("REACH"), &v));
    assert!(!on_obj(&env, &c, &kw("FLYING"), &plain()));
    assert!(on_obj(&env, &c, &ds("MONARCH_SOURCE"), &v));
    assert!(!on_obj(&env, &c, &ds("OTHER"), &v));
    assert!(!on_player(&env, &c, &kw("FLYING"), P1));
    assert!(!on_player(&env, &c, &ds("MONARCH_SOURCE"), P1));
}

// REQ-DSL-003: HAS_COUNTER without compare is ">= 1"; with compare it compares the count
// (absent kind = 0).
#[test]
fn req_dsl_003_has_counter() {
    let env = Env::new();
    let c = ctx(P1);
    let mut v = plain();
    v.counters.insert("+1/+1".into(), 3);
    let has = |k: &str| PredicateAST::HasCounter {
        kind: k.into(),
        compare: None,
    };
    let cmp = |k: &str, op: Cmp, n: i64| PredicateAST::HasCounter {
        kind: k.into(),
        compare: Some((op, blit(n))),
    };
    assert!(on_obj(&env, &c, &has("+1/+1"), &v));
    assert!(!on_obj(&env, &c, &has("charge"), &v));
    assert!(on_obj(&env, &c, &cmp("+1/+1", Cmp::Eq, 3), &v));
    assert!(on_obj(&env, &c, &cmp("+1/+1", Cmp::Ge, 3), &v));
    assert!(!on_obj(&env, &c, &cmp("+1/+1", Cmp::Gt, 3), &v));
    // Absent kind counts as 0.
    assert!(on_obj(&env, &c, &cmp("charge", Cmp::Eq, 0), &v));
    assert!(!on_obj(&env, &c, &cmp("charge", Cmp::Ge, 1), &v));
    assert!(!on_player(&env, &c, &has("+1/+1"), P1));
    assert!(!on_player(&env, &c, &cmp("charge", Cmp::Eq, 0), P1));
}

// REQ-DSL-003: CONTROLLED_BY / OWNED_BY against the resolved player set.
#[test]
fn req_dsl_003_controlled_and_owned_by() {
    let env = Env::three(Some(P1));
    let c = ctx(P1);
    let mut stolen = view(21, Zone::Battlefield, P2, &["Artifact"]);
    stolen.controller = Some(P1);
    let mut in_gy = view(22, Zone::Graveyard, P3, &["Artifact"]);
    in_gy.controller = None;
    let ctrl = |r: ReferenceAST| PredicateAST::ControlledBy { player: pref(r) };
    let own = |r: ReferenceAST| PredicateAST::OwnedBy { player: pref(r) };
    assert!(on_obj(&env, &c, &ctrl(ReferenceAST::Controller), &stolen));
    assert!(!on_obj(
        &env,
        &c,
        &ctrl(ReferenceAST::EachOpponent),
        &stolen
    ));
    assert!(on_obj(&env, &c, &own(ReferenceAST::EachOpponent), &stolen));
    assert!(!on_obj(&env, &c, &own(ReferenceAST::Controller), &stolen));
    // Controller None: never controlled by anyone.
    let all = PlayerSelectorAST::Players(PlayerGroup::AllPlayers);
    assert!(!on_obj(
        &env,
        &c,
        &PredicateAST::ControlledBy {
            player: all.clone()
        },
        &in_gy
    ));
    assert!(on_obj(
        &env,
        &c,
        &PredicateAST::OwnedBy {
            player: all.clone()
        },
        &in_gy
    ));
    // Nobody resolved: false.
    assert!(!on_obj(&env, &c, &ctrl(ReferenceAST::ThatPlayer), &stolen));
    assert!(!on_player(&env, &c, &ctrl(ReferenceAST::Controller), P1));
    assert!(!on_player(&env, &c, &own(ReferenceAST::Controller), P1));
}

// REQ-DSL-003: MANA_VALUE / POWER / TOUGHNESS compare the view's numbers; CR 208.5 (repo): a
// missing power or toughness is false.
#[test]
fn req_dsl_003_numeric_comparisons() {
    let env = Env::new();
    let c = ctx(P1);
    let mut v = creature(23, Zone::Battlefield, P1, 4, -1);
    v.mana_value = 3;
    let mv = |op: Cmp, n: i64| PredicateAST::ManaValue {
        cmp: op,
        value: blit(n),
    };
    let pw = |op: Cmp, n: i64| PredicateAST::Power {
        cmp: op,
        value: blit(n),
    };
    let th = |op: Cmp, n: i64| PredicateAST::Toughness {
        cmp: op,
        value: blit(n),
    };
    assert!(on_obj(&env, &c, &mv(Cmp::Eq, 3), &v));
    assert!(!on_obj(&env, &c, &mv(Cmp::Lt, 3), &v));
    assert!(on_obj(&env, &c, &pw(Cmp::Ge, 4), &v));
    assert!(!on_obj(&env, &c, &pw(Cmp::Gt, 4), &v));
    assert!(on_obj(&env, &c, &th(Cmp::Lt, 0), &v));
    assert!(!on_obj(&env, &c, &th(Cmp::Ge, 0), &v));
    // No power / toughness: false for every comparison, including "<= anything".
    let nopt = plain();
    for op in Cmp::ALL.iter().copied() {
        assert!(!on_obj(&env, &c, &pw(op, 0), &nopt), "power {op:?}");
        assert!(!on_obj(&env, &c, &th(op, 0), &nopt), "toughness {op:?}");
    }
    // A plain view has mana value 0.
    assert!(on_obj(&env, &c, &mv(Cmp::Eq, 0), &nopt));
    assert!(!on_player(&env, &c, &mv(Cmp::Eq, 0), P1));
    assert!(!on_player(&env, &c, &pw(Cmp::Le, 100), P1));
    assert!(!on_player(&env, &c, &th(Cmp::Le, 100), P1));
}

// REQ-DSL-003: SAME_AS on an object and on a player subject.
#[test]
fn req_dsl_003_same_as() {
    let mut env = Env::new();
    let v = env.add(creature(24, Zone::Battlefield, P1, 1, 1));
    let other = creature(25, Zone::Battlefield, P1, 1, 1);
    env.add(other.clone());
    let mut c = ctx(P1);
    c.targets = vec![Some(obj(24)), Some(pl(P2))];
    let s0 = PredicateAST::SameAs {
        reference: target(0),
    };
    let s1 = PredicateAST::SameAs {
        reference: target(1),
    };
    let view24 = env.objects[&v].clone();
    assert!(on_obj(&env, &c, &s0, &view24));
    assert!(!on_obj(&env, &c, &s0, &other));
    assert!(!on_obj(&env, &c, &s1, &view24));
    assert!(on_player(&env, &c, &s1, P2));
    assert!(!on_player(&env, &c, &s1, P1));
    assert!(!on_player(&env, &c, &s0, P2));
    // Unresolved reference: false, not an error.
    let s9 = PredicateAST::SameAs {
        reference: target(9),
    };
    assert!(!on_obj(&env, &c, &s9, &view24));
    assert!(!on_player(&env, &c, &s9, P2));
}

// REQ-DSL-003: CONTROLS: explicit player, the player subject, else ctx.controller.
#[test]
fn req_dsl_003_controls() {
    let mut env = Env::three(Some(P1));
    env.add(creature(26, Zone::Battlefield, P2, 1, 1));
    env.add(artifact(27, Zone::Battlefield, P1));
    let creatures = Box::new(bf(PredicateAST::IsCreature));
    let none_given = PredicateAST::Controls {
        selector: creatures.clone(),
        player: None,
    };
    let opp = PredicateAST::Controls {
        selector: creatures.clone(),
        player: Some(pref(ReferenceAST::EachOpponent)),
    };
    let s = plain();
    // Object subject, no player: ctx.controller.
    assert!(!on_obj(&env, &ctx(P1), &none_given, &s));
    assert!(on_obj(&env, &ctx(P2), &none_given, &s));
    // Player subject, no player: the subject.
    assert!(on_player(&env, &ctx(P1), &none_given, P2));
    assert!(!on_player(&env, &ctx(P2), &none_given, P3));
    // Explicit player set (independent of the subject).
    assert!(on_obj(&env, &ctx(P1), &opp, &s));
    assert!(!on_obj(&env, &ctx(P2), &opp, &s));
    assert!(on_player(&env, &ctx(P1), &opp, P1));
    assert!(!on_player(&env, &ctx(P2), &opp, P2));
    // Nothing selected.
    let lands = PredicateAST::Controls {
        selector: Box::new(bf(PredicateAST::IsLand)),
        player: None,
    };
    assert!(!on_player(&env, &ctx(P1), &lands, P1));
}

// REQ-DSL-003: PLAYER_HAS_MAX_SPEED: resolved set non-empty and every player has max speed.
#[test]
fn req_dsl_003_player_has_max_speed() {
    let mut env = Env::three(Some(P1));
    env.max_speed.insert(P2);
    let p = |r: ReferenceAST| PredicateAST::PlayerHasMaxSpeed { player: pref(r) };
    let c = ctx(P2);
    assert!(on_obj(&env, &c, &p(ReferenceAST::Controller), &plain()));
    assert!(on_player(&env, &c, &p(ReferenceAST::Controller), P3));
    // P1 and P3 are not at max speed.
    assert!(!on_player(&env, &c, &p(ReferenceAST::EachOpponent), P2));
    env.max_speed.insert(P1);
    assert!(!on_player(&env, &c, &p(ReferenceAST::EachOpponent), P2));
    env.max_speed.insert(P3);
    assert!(on_player(&env, &c, &p(ReferenceAST::EachOpponent), P2));
    // Empty resolved set: false.
    assert!(!on_player(&env, &c, &p(ReferenceAST::ThatPlayer), P2));
    assert!(!on_obj(&env, &c, &p(ReferenceAST::ThatPlayer), &plain()));
}

// REQ-DSL-003: COMPARE is independent of the subject.
#[test]
fn req_dsl_003_compare_predicate() {
    let env = Env::new();
    let c = ctx(P1);
    let p = |op: Cmp, l: i64, r: i64| PredicateAST::Compare {
        cmp: op,
        lhs: blit(l),
        rhs: blit(r),
    };
    assert!(on_obj(&env, &c, &p(Cmp::Lt, 1, 2), &plain()));
    assert!(!on_obj(&env, &c, &p(Cmp::Gt, 1, 2), &plain()));
    assert!(on_player(&env, &c, &p(Cmp::Lt, 1, 2), P2));
    assert!(!on_player(&env, &c, &p(Cmp::Gt, 1, 2), P2));
}

// SRS 14.5, REQ-DSL-003: TEXT_TOKEN_MATCH compares the token's current value.
#[test]
fn req_dsl_003_text_token_match() {
    let mut env = Env::new();
    env.add(view(31, Zone::Battlefield, P1, &["Enchantment"]));
    env.set_token(oid(31), "t1", "Testkin");
    let c = ctx_src(P1, oid(31));
    let p = |tok: &str, val: &str| PredicateAST::TextTokenMatch {
        text_token: tok.into(),
        value: val.into(),
    };
    assert!(on_obj(&env, &c, &p("t1", "Testkin"), &plain()));
    assert!(!on_obj(&env, &c, &p("t1", "Otherkin"), &plain()));
    assert!(on_player(&env, &c, &p("t1", "Testkin"), P2));
    assert!(!on_player(&env, &c, &p("t1", "testkin"), P2));
    env.set_token(oid(31), "t1", "Otherkin");
    assert!(on_obj(&env, &c, &p("t1", "Otherkin"), &plain()));
    // No source: false. Unknown token: error.
    assert!(!on_obj(&env, &ctx(P1), &p("t1", "Otherkin"), &plain()));
    assert!(!on_player(&env, &ctx(P1), &p("t1", "Otherkin"), P1));
    assert_eq!(
        eval_predicate_object(&env, &c, &p("t7", "x"), &plain()),
        Err(InterpError::UnknownTextToken("t7".into()))
    );
    assert_eq!(
        eval_predicate_player(&env, &c, &p("t7", "x"), P1),
        Err(InterpError::UnknownTextToken("t7".into()))
    );
}

// REQ-DSL-003: compare on i64 for every Cmp.
#[test]
fn req_dsl_003_compare_fn() {
    let rows = [
        (Cmp::Eq, [false, true, false]),
        (Cmp::Ne, [true, false, true]),
        (Cmp::Lt, [true, false, false]),
        (Cmp::Le, [true, true, false]),
        (Cmp::Gt, [false, false, true]),
        (Cmp::Ge, [false, true, true]),
    ];
    for (op, want) in rows {
        assert_eq!(compare(op, 1, 2), want[0], "{op:?} 1 2");
        assert_eq!(compare(op, 2, 2), want[1], "{op:?} 2 2");
        assert_eq!(compare(op, 3, 2), want[2], "{op:?} 3 2");
        assert_eq!(compare(op, i64::MIN, i64::MAX), want[0], "{op:?} extremes");
        assert_eq!(
            compare(op, i64::MAX, i64::MIN),
            want[2],
            "{op:?} extremes rev"
        );
    }
}
