// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `select_objects`: zones, predicate trees, ordering and count (REQ-DSL-003,
//! REQ-ARCH-002; CR 101.3). Black-box: written from the work-item brief only.

#[path = "spec_interp_support.rs"]
mod support;

use fineprint_base::ids::ObjectId;
use fineprint_core::interp::{EvalContext, Ignored, InterpError, Resolution, select_objects};
use support::*;

fn select(env: &Env, c: &EvalContext, s: &ObjectSelectorAST) -> Vec<ObjectId> {
    items(select_objects(env, c, s))
}

fn ids(ns: &[u64]) -> Vec<ObjectId> {
    ns.iter().map(|n| oid(*n)).collect()
}

/// Objects spread over several zones, inserted in a scrambled id order.
fn mixed_env() -> Env {
    let mut env = Env::new();
    env.add(creature(5, Zone::Battlefield, P1, 2, 2));
    env.add(land(2, Zone::Battlefield, P2));
    env.add(creature(9, Zone::Graveyard, P2, 1, 1));
    env.add(creature(1, Zone::Battlefield, P2, 3, 3));
    env.add(artifact(7, Zone::Exile, P1));
    env.add(instant(4, Zone::Graveyard, P1));
    env.add(artifact(3, Zone::Battlefield, P1));
    env
}

// REQ-DSL-003: no zones means the battlefield; no ordering means ObjectId ascending.
#[test]
fn req_dsl_003_default_zone_is_battlefield() {
    let env = mixed_env();
    let all = filter(None, Some(PredicateAST::IsCardOrToken), None, None);
    assert_eq!(select(&env, &ctx(P1), &all), ids(&[1, 2, 3, 5]));
    let creatures = bf(PredicateAST::IsCreature);
    assert_eq!(select(&env, &ctx(P1), &creatures), ids(&[1, 5]));
    // Nothing matches: empty.
    let tokens = bf(PredicateAST::IsToken);
    assert!(select(&env, &ctx(P1), &tokens).is_empty());
}

// REQ-DSL-003: listed zones only; with no predicate every candidate is kept; ties (no ordering)
// break by ObjectId ascending across zones.
#[test]
fn req_dsl_003_listed_zones_no_predicate() {
    let env = mixed_env();
    let gy = in_zones(&[Zone::Graveyard], None);
    assert_eq!(select(&env, &ctx(P1), &gy), ids(&[4, 9]));
    let two = in_zones(&[Zone::Exile, Zone::Graveyard], None);
    assert_eq!(select(&env, &ctx(P1), &two), ids(&[4, 7, 9]));
    let none = in_zones(&[Zone::Library], None);
    assert!(select(&env, &ctx(P1), &none).is_empty());
}

// REQ-DSL-003: candidates are de-duplicated, and ids the environment lists but cannot describe
// (object() is None) are skipped.
#[test]
fn req_dsl_003_candidates_deduplicated_and_stale_skipped() {
    let mut env = mixed_env();
    // A duplicate id in the same zone list, an id listed under a second zone, and a stale id.
    env.extra_listed
        .insert(Zone::Battlefield, vec![oid(5), oid(500)]);
    env.extra_listed.insert(Zone::Graveyard, vec![oid(1)]);
    let sel = in_zones(&[Zone::Battlefield, Zone::Graveyard], None);
    assert_eq!(select(&env, &ctx(P1), &sel), ids(&[1, 2, 3, 4, 5, 9]));
}

// REQ-DSL-003: a predicate tree (AND / OR / NOT) filters candidates.
#[test]
fn req_dsl_003_predicate_tree_filters() {
    let env = mixed_env();
    let c = ctx(P1);
    let zones = vec![Zone::Battlefield, Zone::Graveyard, Zone::Exile];
    // Creature AND controlled by P2 (only battlefield objects have a controller).
    let p = PredicateAST::And(vec![
        PredicateAST::IsCreature,
        PredicateAST::ControlledBy {
            player: PlayerSelectorAST::Ref(ReferenceAST::EachOpponent),
        },
    ]);
    assert_eq!(
        select(&env, &c, &filter(Some(zones.clone()), Some(p), None, None)),
        ids(&[1])
    );
    // Land OR artifact.
    let p = PredicateAST::Or(vec![
        PredicateAST::IsLand,
        PredicateAST::HasCardType {
            value: name("Artifact"),
        },
    ]);
    assert_eq!(
        select(&env, &c, &filter(Some(zones.clone()), Some(p), None, None)),
        ids(&[2, 3, 7])
    );
    // NOT permanent.
    let p = PredicateAST::Not(Box::new(PredicateAST::IsPermanent));
    assert_eq!(
        select(&env, &c, &filter(Some(zones), Some(p), None, None)),
        ids(&[4, 7, 9])
    );
}

// REQ-DSL-003: "creatures you control": COUNT of IS_CREATURE AND CONTROLLED_BY(CONTROLLER).
#[test]
fn req_dsl_003_creatures_you_control() {
    let mut env = mixed_env();
    env.add(creature(6, Zone::Battlefield, P1, 1, 1));
    let you = PredicateAST::And(vec![
        PredicateAST::IsCreature,
        PredicateAST::ControlledBy {
            player: pref(ReferenceAST::Controller),
        },
    ]);
    let sel = bf(you);
    assert_eq!(select(&env, &ctx(P1), &sel), ids(&[5, 6]));
    assert_eq!(select(&env, &ctx(P2), &sel), ids(&[1]));
    assert_eq!(
        fineprint_core::interp::eval_value(&env, &ctx(P1), &count(sel.clone())),
        Ok(2)
    );
    assert_eq!(
        fineprint_core::interp::eval_value(&env, &ctx(P2), &count(sel)),
        Ok(1)
    );
}

// REQ-DSL-003: an Err from the predicate propagates out of the selector.
#[test]
fn req_dsl_003_predicate_error_propagates() {
    let env = mixed_env();
    let p = PredicateAST::Power {
        cmp: Cmp::Ge,
        value: Box::new(ValueExpressionAST::X),
    };
    assert_eq!(
        select_objects(&env, &ctx(P1), &bf(p)),
        Err(InterpError::MissingX)
    );
}

/// One keyed row: (id, timestamp, mana value, power, toughness, zone position).
type KeyRow = (u64, u64, i64, Option<i64>, Option<i64>, u32);

/// Battlefield creatures whose sort keys are given per id; the env lists them in reverse.
fn keyed_env(rows: &[KeyRow]) -> Env {
    let mut env = Env::new();
    for &(id, t, mv, p, tough, pos) in rows {
        let mut v = view(id, Zone::Battlefield, P1, &["Creature"]);
        v.timestamp = ts(t);
        v.mana_value = mv;
        v.power = p;
        v.toughness = tough;
        v.zone_position = pos;
        env.add(v);
    }
    env
}

fn ordered(env: &Env, by: OrderKey, dir: Direction) -> Vec<ObjectId> {
    select(
        env,
        &ctx(P1),
        &filter(None, None, Some(order(by, dir)), None),
    )
}

// REQ-DSL-003: every OrderKey in Asc and Desc; equal keys break by ObjectId ascending, also
// under Desc; None power/toughness sorts before any number.
#[test]
fn req_dsl_003_ordering_every_key_both_directions() {
    // (id, timestamp, mana value, power, toughness, zone position)
    let env = keyed_env(&[
        (4, 30, 2, Some(1), None, 7),
        (1, 10, 5, Some(-1), Some(4), 2),
        (3, 20, 2, None, Some(4), 7),
        (2, 40, 0, Some(1), Some(0), 0),
        (6, 25, 5, Some(1), Some(-2), 3),
    ]);
    let table: [(OrderKey, &[u64], &[u64]); 5] = [
        (OrderKey::Timestamp, &[1, 3, 6, 4, 2], &[2, 4, 6, 3, 1]),
        // mv: 2->0, 3->2, 4->2, 1->5, 6->5
        (OrderKey::ManaValue, &[2, 3, 4, 1, 6], &[1, 6, 3, 4, 2]),
        // power: 3->None, 1->-1, 2/4/6->1
        (OrderKey::Power, &[3, 1, 2, 4, 6], &[2, 4, 6, 1, 3]),
        // toughness: 4->None, 6->-2, 2->0, 1/3->4
        (OrderKey::Toughness, &[4, 6, 2, 1, 3], &[1, 3, 2, 6, 4]),
        // position: 2->0, 1->2, 6->3, 3/4->7
        (OrderKey::ZonePosition, &[2, 1, 6, 3, 4], &[3, 4, 6, 1, 2]),
    ];
    for (key, asc, desc) in table {
        assert_eq!(ordered(&env, key, Direction::Asc), ids(asc), "{key:?} Asc");
        assert_eq!(
            ordered(&env, key, Direction::Desc),
            ids(desc),
            "{key:?} Desc"
        );
    }
}

// REQ-DSL-003: Timestamp ordering with both timestamp orders (A older than B, B older than A).
#[test]
fn req_dsl_003_ordering_timestamp_both_orders() {
    // A = 1, B = 2.
    let a_older = keyed_env(&[(1, 5, 0, None, None, 0), (2, 9, 0, None, None, 0)]);
    let b_older = keyed_env(&[(1, 9, 0, None, None, 0), (2, 5, 0, None, None, 0)]);
    assert_eq!(
        ordered(&a_older, OrderKey::Timestamp, Direction::Asc),
        ids(&[1, 2])
    );
    assert_eq!(
        ordered(&a_older, OrderKey::Timestamp, Direction::Desc),
        ids(&[2, 1])
    );
    assert_eq!(
        ordered(&b_older, OrderKey::Timestamp, Direction::Asc),
        ids(&[2, 1])
    );
    assert_eq!(
        ordered(&b_older, OrderKey::Timestamp, Direction::Desc),
        ids(&[1, 2])
    );
    // Oldest one: count 1 after ordering.
    let oldest = filter(
        None,
        None,
        Some(order(OrderKey::Timestamp, Direction::Asc)),
        Some(lit(1)),
    );
    assert_eq!(select(&a_older, &ctx(P1), &oldest), ids(&[1]));
    assert_eq!(select(&b_older, &ctx(P1), &oldest), ids(&[2]));
}

// REQ-DSL-003: the result does not depend on the order objects_in returns (R3).
#[test]
fn req_dsl_003_result_independent_of_listing_order() {
    let base = keyed_env(&[
        (4, 30, 2, Some(1), None, 7),
        (1, 10, 5, Some(-1), Some(4), 2),
        (3, 20, 2, None, Some(4), 7),
        (2, 40, 0, Some(1), Some(0), 0),
    ]);
    let sels = [
        filter(None, None, None, None),
        filter(
            None,
            None,
            Some(order(OrderKey::ManaValue, Direction::Desc)),
            None,
        ),
        filter(
            None,
            None,
            Some(order(OrderKey::Power, Direction::Asc)),
            Some(lit(2)),
        ),
    ];
    for sel in &sels {
        let want = {
            let mut e = base.clone();
            e.list_order = ListOrder::Insertion;
            select(&e, &ctx(P1), sel)
        };
        for lo in [
            ListOrder::Reverse,
            ListOrder::Shuffled(1),
            ListOrder::Shuffled(77),
        ] {
            let mut e = base.clone();
            e.list_order = lo;
            assert_eq!(select(&e, &ctx(P1), sel), want, "{lo:?} {sel:?}");
        }
    }
}

// REQ-DSL-003: count 0, negative, exactly the length, larger than the length, and as X.
#[test]
fn req_dsl_003_count_cases() {
    let env = keyed_env(&[
        (4, 30, 0, None, None, 0),
        (1, 10, 0, None, None, 0),
        (3, 20, 0, None, None, 0),
    ]);
    let by_ts = Some(order(OrderKey::Timestamp, Direction::Desc));
    let with = |n: ValueExpressionAST| filter(None, None, by_ts, Some(n));
    let c = ctx(P1);
    assert!(select(&env, &c, &with(lit(0))).is_empty());
    assert!(select(&env, &c, &with(lit(-1))).is_empty());
    assert!(select(&env, &c, &with(lit(i64::MIN))).is_empty());
    assert_eq!(select(&env, &c, &with(lit(1))), ids(&[4]));
    assert_eq!(select(&env, &c, &with(lit(2))), ids(&[4, 3]));
    assert_eq!(select(&env, &c, &with(lit(3))), ids(&[4, 3, 1]));
    assert_eq!(select(&env, &c, &with(lit(4))), ids(&[4, 3, 1]));
    assert_eq!(select(&env, &c, &with(lit(i64::MAX))), ids(&[4, 3, 1]));
    // Count as X.
    let mut cx = ctx(P1);
    cx.x = Some(2);
    assert_eq!(
        select(&env, &cx, &with(ValueExpressionAST::X)),
        ids(&[4, 3])
    );
    assert_eq!(
        select_objects(&env, &c, &with(ValueExpressionAST::X)),
        Err(InterpError::MissingX)
    );
    // Count without ordering: first n by ObjectId.
    let first2 = filter(None, None, None, Some(lit(2)));
    assert_eq!(select(&env, &c, &first2), ids(&[1, 3]));
}

// REQ-DSL-003: reference form: objects kept; a player referent is unresolved (CR 101.3).
#[test]
fn req_dsl_003_reference_form() {
    let env = mixed_env();
    let mut c = ctx_src(P1, oid(5));
    c.targets = vec![Some(obj(2)), Some(pl(P2))];
    assert_eq!(
        select_objects(&env, &c, &ObjectSelectorAST::Ref(ReferenceAST::SelfRef)),
        Ok(Resolution {
            items: ids(&[5]),
            ignored: Vec::new()
        })
    );
    assert_eq!(
        select(&env, &c, &ObjectSelectorAST::Ref(target(0))),
        ids(&[2])
    );
    assert_eq!(
        select_objects(&env, &c, &ObjectSelectorAST::Ref(target(1))),
        Ok(Resolution {
            items: Vec::new(),
            ignored: vec![Ignored::Unresolved(target(1))]
        })
    );
}
