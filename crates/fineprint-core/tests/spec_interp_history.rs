// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `HISTORY(query)` read from the history ledger (REQ-DSL-003). The ledger is fed
//! only by executed events plus `record_spell_cast` (docs/DECISIONS.md "#27 FP-026"). Black-box:
//! written from the work-item brief only.

#[path = "spec_interp_support.rs"]
mod support;

use fineprint_base::ids::PlayerId;
use fineprint_core::interp::{EvalContext, eval_value};
use support::*;

fn val(env: &Env, c: &EvalContext, v: &ValueExpressionAST) -> i64 {
    eval_value(env, c, v).expect("no error")
}

/// A three-player ledger over turns 1..=3 (each feed at its turn).
fn scripted() -> Env {
    let script = [
        // turn 1
        (1, Feed::Spell(P1)),
        (1, Feed::Spell(P2)),
        (1, Feed::Lose(P2, 4)),
        (1, Feed::Gain(P1, 2)),
        (1, Feed::Draw(P1)),
        // turn 2
        (2, Feed::Spell(P1)),
        (2, Feed::Spell(P1)),
        (2, Feed::Spell(P3)),
        (2, Feed::Lose(P1, 3)),
        (2, Feed::Lose(P3, 5)),
        (2, Feed::Lose(P1, 1)),
        (2, Feed::Gain(P2, 6)),
        (2, Feed::Gain(P3, 1)),
        (2, Feed::Draw(P2)),
        (2, Feed::Draw(P2)),
        (2, Feed::Draw(P3)),
        // turn 3
        (3, Feed::Spell(P2)),
        (3, Feed::Lose(P2, 7)),
        (3, Feed::Draw(P1)),
    ];
    let mut env = Env::three(Some(P2));
    env.history = ledger(3, &script);
    env.turn = 2;
    env
}

fn spells(player: Option<PlayerSelectorAST>) -> ValueExpressionAST {
    history(HistoryQuery::SpellsCastThisTurn {
        player,
        condition: None,
    })
}

// REQ-DSL-003: SPELLS_CAST_THIS_TURN without a condition: all players, or summed over the
// resolved players; other turns never count; turn_number() decides "this turn".
#[test]
fn req_dsl_003_spells_cast_this_turn_ledger() {
    let mut env = scripted();
    let c = ctx(P1);
    assert_eq!(val(&env, &c, &spells(None)), 3);
    assert_eq!(
        val(&env, &c, &spells(Some(pref(ReferenceAST::Controller)))),
        2
    );
    assert_eq!(
        val(&env, &c, &spells(Some(pref(ReferenceAST::EachOpponent)))),
        1
    );
    assert_eq!(
        val(
            &env,
            &c,
            &spells(Some(PlayerSelectorAST::Players(PlayerGroup::AllPlayers)))
        ),
        3
    );
    // Nobody resolved: 0.
    assert_eq!(
        val(&env, &c, &spells(Some(pref(ReferenceAST::ThatPlayer)))),
        0
    );
    // Turn 1 and turn 3 views of the same ledger.
    env.turn = 1;
    assert_eq!(val(&env, &c, &spells(None)), 2);
    assert_eq!(
        val(&env, &c, &spells(Some(pref(ReferenceAST::Controller)))),
        1
    );
    env.turn = 3;
    assert_eq!(val(&env, &c, &spells(None)), 1);
    assert_eq!(
        val(&env, &c, &spells(Some(pref(ReferenceAST::Controller)))),
        0
    );
    env.turn = 4;
    assert_eq!(val(&env, &c, &spells(None)), 0);
}

// REQ-DSL-003: LIFE_LOST / LIFE_GAINED / CARDS_DRAWN this turn, per player and summed.
#[test]
fn req_dsl_003_life_and_draw_queries() {
    let mut env = scripted();
    let lost = |r: ReferenceAST| history(HistoryQuery::LifeLostThisTurn { player: pref(r) });
    let gained = |r: ReferenceAST| history(HistoryQuery::LifeGainedThisTurn { player: pref(r) });
    let drawn = |r: ReferenceAST| history(HistoryQuery::CardsDrawnThisTurn { player: pref(r) });
    let all = PlayerSelectorAST::Players(PlayerGroup::AllPlayers);
    let c1 = ctx(P1);
    let c2 = ctx(P2);
    // turn 2
    assert_eq!(val(&env, &c1, &lost(ReferenceAST::Controller)), 4);
    assert_eq!(val(&env, &c2, &lost(ReferenceAST::Controller)), 0);
    assert_eq!(val(&env, &c2, &lost(ReferenceAST::EachOpponent)), 9);
    assert_eq!(
        val(
            &env,
            &c1,
            &history(HistoryQuery::LifeLostThisTurn {
                player: all.clone()
            })
        ),
        9
    );
    assert_eq!(val(&env, &c2, &gained(ReferenceAST::Controller)), 6);
    assert_eq!(val(&env, &c2, &gained(ReferenceAST::EachOpponent)), 1);
    assert_eq!(val(&env, &c1, &gained(ReferenceAST::Controller)), 0);
    assert_eq!(val(&env, &c2, &drawn(ReferenceAST::Controller)), 2);
    assert_eq!(val(&env, &c2, &drawn(ReferenceAST::EachOpponent)), 1);
    assert_eq!(
        val(
            &env,
            &c1,
            &history(HistoryQuery::CardsDrawnThisTurn {
                player: all.clone()
            })
        ),
        3
    );
    assert_eq!(val(&env, &c1, &lost(ReferenceAST::ThatPlayer)), 0);
    // turn 1
    env.turn = 1;
    assert_eq!(val(&env, &c2, &lost(ReferenceAST::Controller)), 4);
    assert_eq!(val(&env, &c1, &gained(ReferenceAST::Controller)), 2);
    assert_eq!(val(&env, &c1, &drawn(ReferenceAST::Controller)), 1);
    assert_eq!(val(&env, &c2, &drawn(ReferenceAST::Controller)), 0);
    // turn 3
    env.turn = 3;
    assert_eq!(val(&env, &c2, &lost(ReferenceAST::Controller)), 7);
    assert_eq!(val(&env, &c1, &lost(ReferenceAST::Controller)), 0);
    assert_eq!(val(&env, &c1, &drawn(ReferenceAST::Controller)), 1);
    assert_eq!(
        val(
            &env,
            &c1,
            &history(HistoryQuery::LifeGainedThisTurn { player: all })
        ),
        0
    );
}

// REQ-DSL-003: SPELLS_CAST_THIS_TURN with a condition counts env.spells_cast_this_turn() entries
// whose caster is in the player set and whose view passes the predicate.
#[test]
fn req_dsl_003_spells_cast_with_condition() {
    let mut env = Env::three(Some(P1));
    let mut red_creature = creature(1, Zone::Stack, P1, 2, 2);
    red_creature.characteristics.colors = colors(8);
    let blue_instant = {
        let mut v = instant(2, Zone::Stack, P2);
        v.characteristics.colors = colors(2);
        v
    };
    let p3_creature = creature(3, Zone::Stack, P3, 1, 1);
    // As cast: a red Instant on the stack. Currently: a creature of another colour in the
    // graveyard (the same id). The condition must read the as-cast view (F9).
    let mut p1_instant = instant(4, Zone::Stack, P1);
    p1_instant.characteristics.colors = colors(8);
    let mut now = creature(4, Zone::Graveyard, P1, 1, 1);
    now.characteristics.colors = colors(2);
    env.add(now);
    env.spells = vec![
        (P1, red_creature),
        (P2, blue_instant),
        (P3, p3_creature),
        (P1, p1_instant),
    ];
    // The ledger disagrees on purpose: the conditioned query must not read it.
    env.history = ledger(3, &[(1, Feed::Spell(P1))]);
    let q = |player: Option<PlayerSelectorAST>, cond: PredicateAST| {
        history(HistoryQuery::SpellsCastThisTurn {
            player,
            condition: Some(cond),
        })
    };
    let c = ctx(P1);
    assert_eq!(val(&env, &c, &q(None, PredicateAST::IsCreature)), 2);
    assert_eq!(
        val(
            &env,
            &c,
            &q(
                Some(pref(ReferenceAST::Controller)),
                PredicateAST::IsCreature
            )
        ),
        1
    );
    assert_eq!(
        val(
            &env,
            &c,
            &q(
                Some(pref(ReferenceAST::Controller)),
                PredicateAST::IsCardOrToken
            )
        ),
        2
    );
    assert_eq!(
        val(
            &env,
            &c,
            &q(
                Some(pref(ReferenceAST::EachOpponent)),
                PredicateAST::IsCreature
            )
        ),
        1
    );
    assert_eq!(
        val(
            &env,
            &c,
            &q(
                None,
                PredicateAST::HasColor {
                    value: name("blue")
                }
            )
        ),
        1
    );
    assert_eq!(val(&env, &c, &q(None, PredicateAST::IsLand)), 0);
    assert_eq!(
        val(
            &env,
            &c,
            &q(
                Some(pref(ReferenceAST::ThatPlayer)),
                PredicateAST::IsCardOrToken
            )
        ),
        0
    );
}

// REQ-DSL-003: the condition is evaluated on the AS-CAST view recorded in the entry, never on
// the object's current view (object 4 was an Instant on the stack and is now a creature in the
// graveyard).
#[test]
fn req_dsl_003_spells_cast_condition_uses_as_cast_view() {
    let mut env = Env::new();
    let mut as_cast = instant(4, Zone::Stack, P1);
    as_cast.characteristics.colors = colors(8);
    let mut now = creature(4, Zone::Graveyard, P1, 1, 1);
    now.characteristics.colors = colors(2);
    env.add(now);
    env.spells = vec![(P1, as_cast)];
    let c = ctx(P1);
    let q = |cond: PredicateAST| {
        history(HistoryQuery::SpellsCastThisTurn {
            player: None,
            condition: Some(cond),
        })
    };
    let card_type = |t: &str| PredicateAST::HasCardType { value: name(t) };
    let color = |t: &str| PredicateAST::HasColor { value: name(t) };
    // As cast: an Instant, red, not a creature.
    assert_eq!(val(&env, &c, &q(card_type("Instant"))), 1);
    assert_eq!(val(&env, &c, &q(card_type("Creature"))), 0);
    assert_eq!(val(&env, &c, &q(PredicateAST::IsCreature)), 0);
    assert_eq!(val(&env, &c, &q(color("red"))), 1);
    assert_eq!(val(&env, &c, &q(color("blue"))), 0);
    // The object is on the stack as cast, so it is not a permanent.
    assert_eq!(val(&env, &c, &q(PredicateAST::IsPermanent)), 0);
}

// REQ-DSL-003: with no player selector the condition counts every entry, including a caster who
// is not seated; with a selector only casters in the resolved player set count.
#[test]
fn req_dsl_003_spells_cast_player_none_counts_unseated_casters() {
    let mut env = Env::new(); // seated: P1, P2
    env.spells = vec![
        (P1, creature(1, Zone::Stack, P1, 1, 1)),
        (P2, creature(2, Zone::Stack, P2, 1, 1)),
        (NOBODY, creature(3, Zone::Stack, NOBODY, 1, 1)),
        (NOBODY, instant(5, Zone::Stack, NOBODY)),
    ];
    let c = ctx(P1);
    let q = |player: Option<PlayerSelectorAST>| {
        history(HistoryQuery::SpellsCastThisTurn {
            player,
            condition: Some(PredicateAST::IsCreature),
        })
    };
    assert_eq!(val(&env, &c, &q(None)), 3);
    assert_eq!(
        val(
            &env,
            &c,
            &q(Some(PlayerSelectorAST::Players(PlayerGroup::AllPlayers)))
        ),
        2
    );
    assert_eq!(val(&env, &c, &q(Some(pref(ReferenceAST::Controller)))), 1);
    assert_eq!(
        val(&env, &ctx(NOBODY), &q(Some(pref(ReferenceAST::Controller)))),
        0
    );
}

// REQ-DSL-003: CREATURES_DIED_THIS_TURN with and without a condition.
#[test]
fn req_dsl_003_creatures_died_this_turn() {
    let mut env = Env::new();
    let c = ctx(P1);
    let q = |cond: Option<PredicateAST>| {
        history(HistoryQuery::CreaturesDiedThisTurn { condition: cond })
    };
    assert_eq!(val(&env, &c, &q(None)), 0);
    let mut mine = creature(1, Zone::Battlefield, P1, 2, 2);
    mine.is_token = true;
    let theirs = creature(2, Zone::Battlefield, P2, 3, 3);
    env.died = vec![mine, theirs];
    assert_eq!(val(&env, &c, &q(None)), 2);
    assert_eq!(val(&env, &c, &q(Some(PredicateAST::IsToken))), 1);
    assert_eq!(
        val(
            &env,
            &c,
            &q(Some(PredicateAST::ControlledBy {
                player: pref(ReferenceAST::EachOpponent)
            }))
        ),
        1
    );
    assert_eq!(val(&env, &c, &q(Some(PredicateAST::IsLand))), 0);
}

// REQ-DSL-003: MANA_SPENT_TO_CAST sums over the reference's objects; the colour word is resolved
// (a text token reads its current value); None stays None.
#[test]
fn req_dsl_003_mana_spent_to_cast() {
    let mut env = Env::new();
    env.add(instant(1, Zone::Stack, P1));
    env.add(instant(2, Zone::Stack, P1));
    env.mana_spent.insert((oid(1), None), 5);
    env.mana_spent.insert((oid(1), Some("red".into())), 2);
    env.mana_spent.insert((oid(1), Some("green".into())), 3);
    env.mana_spent.insert((oid(2), None), 4);
    env.set_token(oid(1), "t1", "red");
    let c = ctx_src(P1, oid(1));
    let q = |r: ReferenceAST, color: Option<WordRef>| {
        history(HistoryQuery::ManaSpentToCast {
            reference: r,
            color,
        })
    };
    assert_eq!(val(&env, &c, &q(ReferenceAST::SelfRef, None)), 5);
    assert_eq!(
        env.mana_calls.borrow().last().cloned(),
        Some((oid(1), None))
    );
    assert_eq!(
        val(&env, &c, &q(ReferenceAST::SelfRef, Some(name("green")))),
        3
    );
    assert_eq!(
        env.mana_calls.borrow().last().cloned(),
        Some((oid(1), Some("green".to_string())))
    );
    assert_eq!(
        val(&env, &c, &q(ReferenceAST::SelfRef, Some(token("t1")))),
        2
    );
    env.set_token(oid(1), "t1", "green");
    assert_eq!(
        val(&env, &c, &q(ReferenceAST::SelfRef, Some(token("t1")))),
        3
    );
    // Summed over several objects; players contribute nothing.
    let mut both = c.clone();
    both.bindings
        .insert("$s".into(), vec![obj(1), pl(P2), obj(2)]);
    assert_eq!(val(&env, &both, &q(bound("$s"), None)), 9);
    // No items: 0.
    assert_eq!(val(&env, &ctx(P1), &q(ReferenceAST::SelfRef, None)), 0);
}

fn dup_env(script: &[(u64, Feed)]) -> Env {
    let mut env = Env::new();
    env.history = ledger(2, script);
    env
}

fn with_list(list: &[PlayerId]) -> EvalContext {
    let mut c = ctx(P1);
    c.bindings
        .insert("$v".into(), list.iter().map(|p| pl(*p)).collect());
    c
}

// REQ-DSL-003 (modelling choice): a player selector in a HISTORY query is a set of distinct
// players, so a duplicate in the bound list is counted once.
#[test]
fn req_dsl_003_history_distinct_players_ledger() {
    let env = dup_env(&[
        (1, Feed::Spell(P1)),
        (1, Feed::Spell(P1)),
        (1, Feed::Spell(P2)),
    ]);
    let q = spells(Some(pref(bound("$v"))));
    assert_eq!(val(&env, &with_list(&[P1, P1, P2]), &q), 3);
    // No duplicates: unchanged.
    assert_eq!(val(&env, &with_list(&[P1, P2]), &q), 3);
}

// REQ-DSL-003 (modelling choice): the conditioned SPELLS_CAST_THIS_TURN counts each as-cast entry
// once however often its caster is listed.
#[test]
fn req_dsl_003_history_distinct_players_condition() {
    let mut env = Env::new();
    let mut a = instant(1, Zone::Stack, P1);
    a.characteristics.colors = colors(8);
    env.spells = vec![
        (P1, a),
        (P1, instant(2, Zone::Stack, P1)),
        (P2, instant(3, Zone::Stack, P2)),
    ];
    let c = with_list(&[P1, P1, P2]);
    let q = |cond: PredicateAST| {
        history(HistoryQuery::SpellsCastThisTurn {
            player: Some(pref(bound("$v"))),
            condition: Some(cond),
        })
    };
    assert_eq!(
        val(
            &env,
            &c,
            &q(PredicateAST::HasCardType {
                value: name("Instant")
            })
        ),
        3
    );
    // Only spell A (the coloured one) passes.
    assert_eq!(
        val(&env, &c, &q(PredicateAST::HasColor { value: name("red") })),
        1
    );
}

// REQ-DSL-003 (modelling choice): LIFE_LOST and CARDS_DRAWN sum over distinct players.
#[test]
fn req_dsl_003_history_distinct_players_life_and_draw() {
    let env = dup_env(&[
        (1, Feed::Lose(P1, 4)),
        (1, Feed::Draw(P1)),
        (1, Feed::Draw(P1)),
        (1, Feed::Draw(P2)),
    ]);
    let lost = history(HistoryQuery::LifeLostThisTurn {
        player: pref(bound("$v")),
    });
    assert_eq!(val(&env, &with_list(&[P1, P1]), &lost), 4);
    let drawn = history(HistoryQuery::CardsDrawnThisTurn {
        player: pref(bound("$v")),
    });
    assert_eq!(val(&env, &with_list(&[P1, P1, P2]), &drawn), 3);
    assert_eq!(val(&env, &with_list(&[P1, P2]), &drawn), 3);
}

// REQ-DSL-003, CR 608.2h: MANA_SPENT_TO_CAST with a source absent from the env but captured in
// LKI: SELF and SOURCE_LKI read it; TARGET of an absent object is gone and contributes 0.
#[test]
fn req_dsl_003_mana_spent_absent_source_uses_lki() {
    let mut env = Env::new();
    env.lki.insert(oid(5), instant(5, Zone::Stack, P1));
    env.mana_spent.insert((oid(5), None), 3);
    env.mana_spent.insert((oid(5), Some("red".into())), 1);
    let q = |r: ReferenceAST, color: Option<WordRef>| {
        history(HistoryQuery::ManaSpentToCast {
            reference: r,
            color,
        })
    };
    let c = ctx_src(P1, oid(5));
    assert_eq!(val(&env, &c, &q(ReferenceAST::SelfRef, None)), 3);
    assert_eq!(
        val(&env, &c, &q(ReferenceAST::SelfRef, Some(name("red")))),
        1
    );
    assert_eq!(val(&env, &c, &q(ReferenceAST::SourceLki, None)), 3);
    let mut t = ctx(P1);
    t.targets = vec![Some(obj(5))];
    assert_eq!(val(&env, &t, &q(target(0), None)), 0);
}
