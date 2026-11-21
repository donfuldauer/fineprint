// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for impossible-instruction semantics (REQ-ARCH-002, REQ-DSL-003; CR 101.3): the
//! impossible parts are reported in `Resolution::ignored` and the rest is still returned and
//! still used. Black-box: written from the work-item brief only.

#[path = "spec_interp_support.rs"]
mod support;

use std::cell::RefCell;

use fineprint_core::interp::{
    EvalContext, Ignored, Partition, Resolution, eval_value, partition_possible, resolve_players,
    resolve_reference, select_objects,
};
use support::*;

fn two_targets() -> (Env, EvalContext) {
    let mut env = Env::new();
    env.add(creature(1, Zone::Battlefield, P2, 3, 4));
    env.add(creature(2, Zone::Battlefield, P2, 5, 6));
    let mut c = ctx_src(P1, oid(50));
    env.add(view(50, Zone::Stack, P1, &["Instant"]));
    c.targets = vec![Some(obj(1)), Some(obj(2))];
    c.bindings.insert("$targets".into(), vec![obj(1), obj(2)]);
    (env, c)
}

// REQ-ARCH-002, CR 101.3: a two-target selector with one target gone selects the survivor (count
// 1), reports the gone one, and ATTR sums over the survivor only.
#[test]
fn req_arch_002_two_targets_one_gone() {
    let (mut env, c) = two_targets();
    let sel = ObjectSelectorAST::Ref(bound("$targets"));
    assert_eq!(val(&env, &c, &count(sel.clone())), 2);
    assert_eq!(
        val(&env, &c, &attr(bound("$targets"), ObjectAttr::Power)),
        8
    );
    // Target 1 leaves the battlefield (LKI captured: still gone for a target, CR 608.2b).
    env.vanish_with_lki(oid(1));
    assert_eq!(
        select_objects(&env, &c, &sel),
        Ok(Resolution {
            items: vec![oid(2)],
            ignored: vec![Ignored::Gone(obj(1))],
        })
    );
    assert_eq!(val(&env, &c, &count(sel)), 1);
    assert_eq!(
        val(&env, &c, &attr(bound("$targets"), ObjectAttr::Power)),
        5
    );
    assert_eq!(
        val(&env, &c, &attr(bound("$targets"), ObjectAttr::Toughness)),
        6
    );
    // The same via the two slots: the gone slot contributes 0, the other still counts.
    let sum = math(
        MathOp::Add,
        attr(target(0), ObjectAttr::Power),
        attr(target(1), ObjectAttr::Power),
    );
    assert_eq!(val(&env, &c, &sum), 5);
    assert_eq!(
        resolve_reference(&env, &c, &target(0)),
        Ok(Resolution {
            items: Vec::new(),
            ignored: vec![Ignored::Gone(obj(1))],
        })
    );
    assert_eq!(
        resolve_reference(&env, &c, &target(1)),
        Ok(Resolution {
            items: vec![obj(2)],
            ignored: Vec::new(),
        })
    );
    // Both gone: nothing left, still no error.
    env.vanish(oid(2));
    assert_eq!(
        select_objects(&env, &c, &ObjectSelectorAST::Ref(bound("$targets"))),
        Ok(Resolution {
            items: Vec::new(),
            ignored: vec![Ignored::Gone(obj(1)), Ignored::Gone(obj(2))],
        })
    );
    assert_eq!(val(&env, &c, &sum), 0);
}

fn val(env: &Env, c: &EvalContext, v: &ValueExpressionAST) -> i64 {
    eval_value(env, c, v).expect("ignored parts never produce an error")
}

// REQ-ARCH-002, CR 101.3: a mixed binding keeps the possible items in order and reports each
// impossible one (gone object, unknown player) in order.
#[test]
fn req_arch_002_mixed_binding_keeps_order() {
    let (mut env, mut c) = two_targets();
    c.bindings.insert(
        "$mix".into(),
        vec![obj(2), pl(NOBODY), obj(1), pl(P2), obj(77)],
    );
    env.vanish(oid(1));
    assert_eq!(
        resolve_reference(&env, &c, &bound("$mix")),
        Ok(Resolution {
            items: vec![obj(2), pl(P2)],
            ignored: vec![
                Ignored::Gone(pl(NOBODY)),
                Ignored::Gone(obj(1)),
                Ignored::Gone(obj(77)),
            ],
        })
    );
    // Through resolve_players: the surviving player only.
    let got = resolve_players(&env, &c, &pref(bound("$mix"))).expect("no error");
    assert_eq!(got.items, vec![P2]);
}

// REQ-ARCH-002, CR 101.3: out-of-range slots, empty slots and unknown players are ignored and
// contribute 0 inside COUNT / ATTR / PLAYER_ATTR; never an Err.
#[test]
fn req_arch_002_ignored_references_inside_values_give_zero() {
    let (env, mut c) = two_targets();
    c.targets.push(None);
    c.targets.push(Some(pl(NOBODY)));
    let refs = [target(2), target(3), target(99)];
    for r in refs {
        assert_eq!(
            val(&env, &c, &count(ObjectSelectorAST::Ref(r.clone()))),
            0,
            "{r:?}"
        );
        assert_eq!(
            val(&env, &c, &attr(r.clone(), ObjectAttr::Power)),
            0,
            "{r:?}"
        );
        assert_eq!(
            val(&env, &c, &pattr(r.clone(), PlayerAttr::Life)),
            0,
            "{r:?}"
        );
    }
    for r in [
        ReferenceAST::ThatPlayer,
        ReferenceAST::DefendingPlayer,
        ReferenceAST::Enchanted,
        ReferenceAST::Remembered { key: "k".into() },
        ReferenceAST::Chosen { key: "k".into() },
        ReferenceAST::Linked {
            link_id: "l".into(),
        },
    ] {
        assert_eq!(
            val(&env, &c, &count(ObjectSelectorAST::Ref(r.clone()))),
            0,
            "{r:?}"
        );
        assert_eq!(
            val(&env, &c, &attr(r.clone(), ObjectAttr::Power)),
            0,
            "{r:?}"
        );
        assert_eq!(
            val(&env, &c, &pattr(r.clone(), PlayerAttr::Life)),
            0,
            "{r:?}"
        );
    }
    // The rest of a MATH still happens.
    let mut env = env;
    env.set_stat(P1, &fineprint_core::interp::PlayerStat::Life, 20);
    let v = math(
        MathOp::Add,
        pattr(target(3), PlayerAttr::Life),
        pattr(ReferenceAST::Controller, PlayerAttr::Life),
    );
    assert_eq!(val(&env, &c, &v), 20);
}

// REQ-ARCH-002, CR 101.3 / 608.2h: a source that left with LKI is still usable by SELF and
// SOURCE_LKI; with no information both are unresolved and values read 0.
#[test]
fn req_arch_002_source_gone_lki_vs_no_info() {
    let mut env = Env::new();
    let mut v = creature(60, Zone::Battlefield, P1, 4, 4);
    v.mana_value = 2;
    env.add(v);
    let c = ctx_src(P1, oid(60));
    env.vanish_with_lki(oid(60));
    for r in [ReferenceAST::SelfRef, ReferenceAST::SourceLki] {
        assert_eq!(
            resolve_reference(&env, &c, &r),
            Ok(Resolution {
                items: vec![obj(60)],
                ignored: Vec::new(),
            })
        );
        assert_eq!(val(&env, &c, &attr(r.clone(), ObjectAttr::ManaValue)), 2);
    }
    assert_eq!(
        resolve_reference(&env, &c, &ReferenceAST::Owner),
        Ok(Resolution {
            items: vec![pl(P1)],
            ignored: Vec::new(),
        })
    );
    env.vanish(oid(60));
    for r in [
        ReferenceAST::SelfRef,
        ReferenceAST::SourceLki,
        ReferenceAST::Owner,
    ] {
        assert_eq!(
            resolve_reference(&env, &c, &r),
            Ok(Resolution {
                items: Vec::new(),
                ignored: vec![Ignored::Unresolved(r.clone())],
            })
        );
        assert_eq!(val(&env, &c, &attr(r.clone(), ObjectAttr::ManaValue)), 0);
    }
}

// REQ-ARCH-002, CR 101.3: partition_possible calls is_possible once per part, in order, and keeps
// input order in both halves.
#[test]
fn req_arch_002_partition_possible() {
    let calls = RefCell::new(Vec::new());
    let parts = vec![5, 2, 8, 1, 9, 4, 6];
    let got = partition_possible(parts.clone(), |n: &i32| {
        calls.borrow_mut().push(*n);
        n % 2 == 0
    });
    assert_eq!(
        got,
        Partition {
            performed: vec![2, 8, 4, 6],
            ignored: vec![5, 1, 9],
        }
    );
    assert_eq!(calls.into_inner(), parts);
    // All possible, none possible, empty.
    let all = partition_possible(vec!['a', 'b'], |_| true);
    assert_eq!(all.performed, vec!['a', 'b']);
    assert!(all.ignored.is_empty());
    let none = partition_possible(vec!['a', 'b'], |_| false);
    assert!(none.performed.is_empty());
    assert_eq!(none.ignored, vec!['a', 'b']);
    let empty = partition_possible(Vec::<u8>::new(), |_| panic!("never called"));
    assert!(empty.performed.is_empty() && empty.ignored.is_empty());
    // Works on referents straight from a resolution.
    let (env, _) = two_targets();
    let parts = vec![obj(1), obj(2)];
    let split = partition_possible(parts, |r| match r {
        fineprint_core::interp::Referent::Object(id) => {
            fineprint_core::interp::InterpEnv::object(&env, *id)
                .is_some_and(|v| v.power.unwrap_or(0) > 3)
        }
        fineprint_core::interp::Referent::Player(_) => false,
    });
    assert_eq!(split.performed, vec![obj(2)]);
    assert_eq!(split.ignored, vec![obj(1)]);
}
