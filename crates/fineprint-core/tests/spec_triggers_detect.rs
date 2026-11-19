// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for trigger detection (REQ-EVENT-002; CR 603.2c, 603.2g, 603.4, 603.10): batching,
//! output order, filters, subjects, bindings, intervening-if, limits kept in Memory, non-event
//! triggers, the waiting-trigger queue and hand-rolled property loops. Black-box: written from
//! the work-item brief only.

#[path = "spec_triggers_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_core::memory::MemoryTable;
use fineprint_core::triggers::{
    EventField, LookBackKind, StateView, TriggerEvent, TriggerRegistration, WaitingTriggers,
    detect, look_back_kind,
};
use fineprint_dsl::ability::{EventBatch as DslBatch, EventKind, StepId, Trigger};
use fineprint_dsl::expr::{PlayerGroup, PlayerSelectorAST, PredicateAST, Zone};
use support::*;

use EventKind as K;
use StateView::{PostEvent, PreEvent};

/// A post-event `TAPPED` event about object `n` (same id before and after).
fn tapped(n: u64) -> TriggerEvent {
    ev_obj(&[K::Tapped], Some(n), Some(n))
}

fn s(x: &str) -> String {
    x.to_string()
}

// =============================================================================================
// E. batching and output order
// =============================================================================================

// REQ-EVENT-002, CR 603.2c: EACH (explicit or absent) triggers once per matching event.
#[test]
fn spec_triggers_detect_each_one_per_event() {
    for b in [None, Some(DslBatch::Each)] {
        let r = reg(1, P1, ability("a1", batch(m(K::Tapped), b)));
        let env = FakeEnv::new().with_post(r);
        let events = vec![
            tapped(5),
            ev_obj(&[K::Untapped], Some(6), Some(6)),
            tapped(7),
        ];
        let out = run(&env, &events);
        assert_eq!(
            summary(&out),
            vec![(1, s("a1"), vec![0]), (1, s("a1"), vec![2])],
            "{b:?}"
        );
        for t in &out {
            assert_eq!(t.look_back, None);
        }
    }
}

// REQ-EVENT-002, CR 603.2c: ONE_OR_MORE triggers once for all matching events, indices
// ascending; no matching event gives no trigger.
#[test]
fn spec_triggers_detect_one_or_more() {
    let r = reg(
        1,
        P1,
        ability("a1", batch(m(K::Tapped), Some(DslBatch::OneOrMore))),
    );
    let env = FakeEnv::new().with_post(r);
    let events = vec![
        tapped(5),
        ev_obj(&[K::Untapped], Some(6), Some(6)),
        tapped(7),
        tapped(8),
    ];
    let out = run(&env, &events);
    assert_eq!(summary(&out), vec![(1, s("a1"), vec![0, 2, 3])]);

    let none = vec![ev_obj(&[K::Untapped], Some(6), Some(6))];
    assert!(run(&env, &none).is_empty());
    assert!(run(&env, &[]).is_empty());
}

// REQ-EVENT-002, CR 603.2c: output is ordered by (first event index, source id, ability id).
#[test]
fn spec_triggers_detect_output_order() {
    let r9 = reg(9, P1, ability("a1", m(K::Tapped)));
    let r3 = reg(3, P2, ability("a1", m(K::Tapped)));
    let r3b = reg(3, P2, ability("a2", m(K::Tapped)));
    let one_or_more = reg(
        2,
        P1,
        ability("z", batch(m(K::Untapped), Some(DslBatch::OneOrMore))),
    );
    let env = FakeEnv::new()
        .with_post(r9)
        .with_post(r3b)
        .with_post(one_or_more)
        .with_post(r3);
    let events = vec![
        ev_obj(&[K::Untapped], Some(6), Some(6)),
        tapped(5),
        ev_obj(&[K::Untapped], Some(7), Some(7)),
    ];
    let out = run(&env, &events);
    assert_eq!(
        summary(&out),
        vec![
            (2, s("z"), vec![0, 2]),
            (3, s("a1"), vec![1]),
            (3, s("a2"), vec![1]),
            (9, s("a1"), vec![1]),
        ]
    );
    assert_eq!(out[1].controller, P2);
    assert_eq!(out[3].controller, P1);
}

// REQ-EVENT-002, CR 603.2c: one event carrying several kinds the matcher could match still
// triggers an EACH ability once (it is one occurrence).
#[test]
fn spec_triggers_detect_single_trigger_per_event() {
    let r = reg(1, P1, ability("a1", m(K::Dies)));
    let env = FakeEnv::new().with_pre(r);
    let out = run(&env, &[dies_event(5, 6)]);
    assert_eq!(summary(&out), vec![(1, s("a1"), vec![0])]);
}

// =============================================================================================
// F. filters
// =============================================================================================

// REQ-EVENT-002, CR 603.2: value filters (Bool, Int, Zone, Name) need an equal Value field;
// a mismatch or a missing field gives no trigger.
#[test]
fn spec_triggers_detect_value_filters() {
    let cases: Vec<(
        K,
        &str,
        fineprint_dsl::ability::FilterValue,
        EventField,
        EventField,
    )> = vec![
        (
            K::DamageDealt,
            "combat",
            fineprint_dsl::ability::FilterValue::Bool(true),
            vbool(true),
            vbool(false),
        ),
        (
            K::LifeGained,
            "amount",
            fineprint_dsl::ability::FilterValue::Int(3),
            vint(3),
            vint(2),
        ),
        (
            K::ZoneChange,
            "to",
            zone(Zone::Graveyard),
            vzone(Zone::Graveyard),
            vzone(Zone::Exile),
        ),
        (
            K::CountersPut,
            "kind",
            name("plus_one"),
            EventField::Value(name("plus_one")),
            EventField::Value(name("minus_one")),
        ),
    ];
    for (kind, key, fv, good, bad) in cases {
        let r = reg(1, P1, ability("a1", filt(m(kind), key, fv.clone())));
        let env = FakeEnv::new().with_post(r);
        let base = TriggerEvent::new(vec![kind]);
        let ok = run(&env, &[base.clone().with_field(key, good)]);
        assert_eq!(ok.len(), 1, "{kind:?} {key} equal");
        let no = run(&env, &[base.clone().with_field(key, bad)]);
        assert!(no.is_empty(), "{kind:?} {key} mismatch");
        let missing = run(&env, std::slice::from_ref(&base));
        assert!(missing.is_empty(), "{kind:?} {key} missing");
        // A non-Value field under the key never equals a value filter.
        let other = run(&env, &[base.with_field(key, EventField::Player(P1))]);
        assert!(other.is_empty(), "{kind:?} {key} non-value field");
    }
}

// REQ-EVENT-002, CR 603.2: every filter must hold (conjunction).
#[test]
fn spec_triggers_detect_filters_all_must_hold() {
    let mm = filt(
        filt(m(K::ZoneChange), "from", zone(Zone::Hand)),
        "to",
        zone(Zone::Graveyard),
    );
    let env = FakeEnv::new().with_post(reg(1, P1, ability("a1", mm)));
    let e = |from: Zone, to: Zone| {
        TriggerEvent::new(vec![K::ZoneChange])
            .with_field("from", vzone(from))
            .with_field("to", vzone(to))
    };
    assert_eq!(run(&env, &[e(Zone::Hand, Zone::Graveyard)]).len(), 1);
    assert!(run(&env, &[e(Zone::Hand, Zone::Exile)]).is_empty());
    assert!(run(&env, &[e(Zone::Library, Zone::Graveyard)]).is_empty());
}

// REQ-EVENT-002, CR 603.2: ANY matches whether or not the field is present.
#[test]
fn spec_triggers_detect_any_filter() {
    let r = reg(
        1,
        P1,
        ability(
            "a1",
            filt(
                m(K::CountersPut),
                "kind",
                fineprint_dsl::ability::FilterValue::Any,
            ),
        ),
    );
    let env = FakeEnv::new().with_post(r);
    let base = TriggerEvent::new(vec![K::CountersPut]);
    assert_eq!(run(&env, std::slice::from_ref(&base)).len(), 1);
    assert_eq!(
        run(
            &env,
            &[base.with_field("kind", EventField::Value(name("plus_one")))]
        )
        .len(),
        1
    );
}

// REQ-EVENT-002, CR 603.2: a Predicate filter asks the env, passing Some(field) when present and
// None when absent.
#[test]
fn spec_triggers_detect_predicate_filter() {
    let r = reg(
        1,
        P1,
        ability(
            "a1",
            filt(
                m(K::DamageDealt),
                "recipient",
                fineprint_dsl::ability::FilterValue::Predicate(tag_pred("p_ok")),
            ),
        ),
    );
    let base = TriggerEvent::new(vec![K::DamageDealt]);
    let with = base.clone().with_field("recipient", EventField::Player(P2));

    let yes = FakeEnv::new()
        .with_post(r.clone())
        .pred_true(PostEvent, "p_ok");
    assert_eq!(run(&yes, std::slice::from_ref(&with)).len(), 1);
    assert_eq!(
        *yes.pred_calls.borrow(),
        vec![(PostEvent, s("p_ok"), Some(EventField::Player(P2)))]
    );
    yes.clear_calls();
    let _ = run(&yes, std::slice::from_ref(&base));
    assert_eq!(*yes.pred_calls.borrow(), vec![(PostEvent, s("p_ok"), None)]);

    let no = FakeEnv::new().with_post(r);
    assert!(run(&no, &[with]).is_empty());
    assert_eq!(no.pred_calls.borrow().len(), 1);
}

// REQ-EVENT-002, CR 603.10: a Predicate filter on a look-back matcher is asked in the PreEvent
// view.
#[test]
fn spec_triggers_detect_predicate_filter_look_back_view() {
    let mm = filt(
        m(K::Dies),
        "to",
        fineprint_dsl::ability::FilterValue::Predicate(tag_pred("p_pre")),
    );
    let env = FakeEnv::new()
        .with_pre(reg(1, P1, ability("a1", mm)))
        .pred_true(PreEvent, "p_pre");
    assert_eq!(run(&env, &[dies_event(5, 6)]).len(), 1);
    assert_eq!(
        *env.pred_calls.borrow(),
        vec![(PreEvent, s("p_pre"), Some(vzone(Zone::Graveyard)))]
    );
}

// REQ-EVENT-002, CR 603.2: the matcher's kind must be among the event's kinds.
#[test]
fn spec_triggers_detect_kind_must_match() {
    let env = FakeEnv::new().with_post(reg(1, P1, ability("a1", m(K::Untapped))));
    assert!(run(&env, &[tapped(5)]).is_empty());
}

// =============================================================================================
// G. subjects
// =============================================================================================

// REQ-EVENT-002, CR 603.10: an object subject is checked with subject_post in the PostEvent view
// and subject_pre in the PreEvent view.
#[test]
fn spec_triggers_detect_object_subject_ids() {
    let event = ev_obj(&[K::ZoneChange, K::Dies, K::Enters], Some(10), Some(11));
    let enters = reg(1, P1, ability("a1", subj_obj(m(K::Enters))));
    let env = FakeEnv::new().accept(PostEvent, &[11]).with_post(enters);
    assert_eq!(run(&env, std::slice::from_ref(&event)).len(), 1);
    assert_eq!(*env.object_calls.borrow(), vec![(PostEvent, oid(11))]);

    let dies = reg(1, P1, ability("a1", subj_obj(m(K::Dies))));
    let env = FakeEnv::new().accept(PreEvent, &[10]).with_pre(dies);
    assert_eq!(run(&env, &[event]).len(), 1);
    assert_eq!(*env.object_calls.borrow(), vec![(PreEvent, oid(10))]);
}

// REQ-EVENT-002, CR 603.10: a missing id for the view gives no trigger.
#[test]
fn spec_triggers_detect_object_subject_missing_id() {
    let enters = reg(1, P1, ability("a1", subj_obj(m(K::Enters))));
    let env = FakeEnv::new()
        .accept(PostEvent, &[10])
        .accept(PreEvent, &[10])
        .with_both(enters);
    assert!(run(&env, &[ev_obj(&[K::Enters], Some(10), None)]).is_empty());
    assert!(run(&env, &[TriggerEvent::new(vec![K::Enters])]).is_empty());

    let dies = reg(1, P1, ability("a1", subj_obj(m(K::Dies))));
    let env = FakeEnv::new()
        .accept(PostEvent, &[10])
        .accept(PreEvent, &[10])
        .with_both(dies);
    assert!(run(&env, &[ev_obj(&[K::Dies], None, Some(10))]).is_empty());
}

// REQ-EVENT-002, CR 603.2: an object the env rejects gives no trigger.
#[test]
fn spec_triggers_detect_object_subject_rejected() {
    let r = reg(1, P1, ability("a1", subj_obj(m(K::Tapped))));
    let env = FakeEnv::new().accept(PostEvent, &[6]).with_post(r);
    assert!(run(&env, &[tapped(5)]).is_empty());
    assert_eq!(run(&env, &[tapped(6)]).len(), 1);
}

// REQ-EVENT-002, CR 603.2: a players subject needs subject_player and the env's agreement.
#[test]
fn spec_triggers_detect_player_subject() {
    let r = reg(1, P1, ability("a1", subj_players(m(K::LifeGained))));
    let env = FakeEnv::new()
        .accept_player(PostEvent, P2)
        .with_post(r.clone());
    assert_eq!(run(&env, &[ev_player(&[K::LifeGained], P2)]).len(), 1);
    assert_eq!(*env.player_calls.borrow(), vec![(PostEvent, P2)]);
    assert!(run(&env, &[ev_player(&[K::LifeGained], P1)]).is_empty());
    assert!(run(&env, &[TriggerEvent::new(vec![K::LifeGained])]).is_empty());
    // An event about an object only still lacks a player.
    assert!(run(&env, &[ev_obj(&[K::LifeGained], Some(2), Some(2))]).is_empty());
}

// REQ-EVENT-002, CR 603.2: no subject means unconstrained.
#[test]
fn spec_triggers_detect_no_subject_unconstrained() {
    let r = reg(1, P1, ability("a1", m(K::LifeGained)));
    let env = FakeEnv::new().with_post(r);
    assert_eq!(
        run(&env, &[TriggerEvent::new(vec![K::LifeGained])]).len(),
        1
    );
    assert_eq!(run(&env, &[ev_player(&[K::LifeGained], P2)]).len(), 1);
    assert!(env.object_calls.borrow().is_empty());
    assert!(env.player_calls.borrow().is_empty());
}

// =============================================================================================
// H. bindings
// =============================================================================================

// REQ-EVENT-002, CR 603.10: "object" binds the post id without look-back and the pre id with
// it; "player" binds the player; any other key binds the field's value.
#[test]
fn spec_triggers_detect_bindings() {
    let mm = bind(
        bind(bind(m(K::Dies), "object", "$X"), "player", "$P"),
        "amount",
        "$N",
    );
    let event = dies_event(10, 11)
        .with_player(P2)
        .with_field("amount", vint(4));
    let env = FakeEnv::new().with_pre(reg(1, P1, ability("a1", mm.clone())));
    let out = run(&env, std::slice::from_ref(&event));
    assert_eq!(out.len(), 1);
    let mut expected = BTreeMap::new();
    expected.insert(s("$X"), EventField::Object(oid(10)));
    expected.insert(s("$P"), EventField::Player(P2));
    expected.insert(s("$N"), vint(4));
    assert_eq!(out[0].bindings, expected);

    let mut post = mm;
    post.kind = K::PutIntoGraveyard;
    let env = FakeEnv::new().with_post(reg(1, P1, ability("a1", post)));
    let out = run(&env, &[event]);
    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0].bindings.get("$X"),
        Some(&EventField::Object(oid(11)))
    );
}

// REQ-EVENT-002: a bound field the event lacks leaves its variable absent (the trigger still
// happens).
#[test]
fn spec_triggers_detect_binding_absent_field() {
    let mm = bind(bind(m(K::LifeGained), "amount", "$N"), "source", "$S");
    let env = FakeEnv::new().with_post(reg(1, P1, ability("a1", mm)));
    let out = run(
        &env,
        &[ev_player(&[K::LifeGained], P1).with_field("amount", vint(2))],
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].bindings.get("$N"), Some(&vint(2)));
    assert!(!out[0].bindings.contains_key("$S"));
    assert_eq!(out[0].bindings.len(), 1);
}

// REQ-EVENT-002, CR 603.2c: a ONE_OR_MORE trigger binds from its first matching event.
#[test]
fn spec_triggers_detect_bindings_first_matching_event() {
    let mm = batch(
        bind(subj_obj(m(K::Tapped)), "amount", "$N"),
        Some(DslBatch::OneOrMore),
    );
    let env = FakeEnv::new()
        .accept(PostEvent, &[6, 7])
        .with_post(reg(1, P1, ability("a1", mm)));
    let events = vec![
        tapped(5).with_field("amount", vint(5)),
        ev_obj(&[K::Untapped], Some(6), Some(6)).with_field("amount", vint(9)),
        tapped(6).with_field("amount", vint(1)),
        tapped(7).with_field("amount", vint(2)),
    ];
    let out = run(&env, &events);
    assert_eq!(summary(&out), vec![(1, s("a1"), vec![2, 3])]);
    assert_eq!(out[0].bindings.get("$N"), Some(&vint(1)));
}

// =============================================================================================
// I. intervening-if
// =============================================================================================

// REQ-EVENT-002, CR 603.4: a false intervening-if gives no trigger; true triggers.
#[test]
fn spec_triggers_detect_intervening_if() {
    let r = reg(1, P1, ability("a1", m(K::Tapped)));
    let r = TriggerRegistration {
        ability: with_if(r.ability, "if_ok"),
        ..r
    };
    let no = FakeEnv::new().with_post(r.clone());
    assert!(run(&no, &[tapped(5)]).is_empty());
    assert_eq!(no.cond_calls.borrow().len(), 1);
    let yes = FakeEnv::new().with_post(r).cond_true(PostEvent, "if_ok");
    assert_eq!(run(&yes, &[tapped(5)]).len(), 1);
}

// REQ-EVENT-002, CR 603.4: a failed intervening-if consumes no limit: a later event whose
// condition is true still triggers in the same turn.
#[test]
fn spec_triggers_detect_intervening_if_consumes_no_limit() {
    let a = with_limit(
        with_if(ability("a1", bind(m(K::Tapped), "object", "$X")), "bound"),
        Some(1),
        None,
    );
    let mut env = FakeEnv::new().with_post(reg(1, P1, a));
    env.cond_objects.insert(oid(6));
    // One call: event 0 fails the condition, event 1 passes.
    let mut mem = MemoryTable::new();
    let out = run_in(&env, &mut mem, 1, &[tapped(5), tapped(6)]);
    assert_eq!(summary(&out), vec![(1, s("a1"), vec![1])]);

    // Two calls: the first (condition false) leaves the limit untouched.
    let mut mem = MemoryTable::new();
    assert!(run_in(&env, &mut mem, 1, &[tapped(5)]).is_empty());
    assert_eq!(run_in(&env, &mut mem, 1, &[tapped(6)]).len(), 1);
    assert!(run_in(&env, &mut mem, 1, &[tapped(6)]).is_empty());
}

// REQ-EVENT-002, CR 603.4, 603.10: the condition receives the bindings and the matcher's view
// (PreEvent under look-back).
#[test]
fn spec_triggers_detect_intervening_if_view_and_bindings() {
    let a = with_if(ability("a1", bind(m(K::Dies), "object", "$X")), "c");
    let env = FakeEnv::new()
        .with_pre(reg(1, P1, a))
        .cond_true(PreEvent, "c");
    assert_eq!(run(&env, &[dies_event(10, 11)]).len(), 1);
    let mut b = BTreeMap::new();
    b.insert(s("$X"), EventField::Object(oid(10)));
    assert_eq!(*env.cond_calls.borrow(), vec![(PreEvent, s("c"), b)]);

    let a = with_if(ability("a1", bind(m(K::Tapped), "object", "$X")), "c");
    let env = FakeEnv::new()
        .with_post(reg(1, P1, a))
        .cond_true(PostEvent, "c");
    assert_eq!(run(&env, &[tapped(7)]).len(), 1);
    let mut b = BTreeMap::new();
    b.insert(s("$X"), EventField::Object(oid(7)));
    assert_eq!(*env.cond_calls.borrow(), vec![(PostEvent, s("c"), b)]);
}

// =============================================================================================
// J. limits kept in Memory
// =============================================================================================

fn limited(per_turn: Option<u32>, per_game: Option<u32>) -> FakeEnv {
    FakeEnv::new().with_post(reg(
        1,
        P1,
        with_limit(ability("a1", m(K::Tapped)), per_turn, per_game),
    ))
}

// REQ-EVENT-002: perTurn=1 lets only the first matching event trigger, resets next turn.
#[test]
fn spec_triggers_detect_limit_per_turn_one() {
    let env = limited(Some(1), None);
    let mut mem = MemoryTable::new();
    let out = run_in(&env, &mut mem, 1, &[tapped(5), tapped(6)]);
    assert_eq!(summary(&out), vec![(1, s("a1"), vec![0])]);
    assert!(run_in(&env, &mut mem, 1, &[tapped(7)]).is_empty());
    assert_eq!(run_in(&env, &mut mem, 2, &[tapped(7), tapped(8)]).len(), 1);
    assert!(run_in(&env, &mut mem, 2, &[tapped(9)]).is_empty());
    // A fresh table resets.
    let mut fresh = MemoryTable::new();
    assert_eq!(run_in(&env, &mut fresh, 2, &[tapped(9)]).len(), 1);
}

// REQ-EVENT-002: perTurn=2.
#[test]
fn spec_triggers_detect_limit_per_turn_two() {
    let env = limited(Some(2), None);
    let mut mem = MemoryTable::new();
    let out = run_in(&env, &mut mem, 1, &[tapped(5), tapped(6), tapped(7)]);
    assert_eq!(
        summary(&out),
        vec![(1, s("a1"), vec![0]), (1, s("a1"), vec![1])]
    );
    assert!(run_in(&env, &mut mem, 1, &[tapped(8)]).is_empty());
    let mut mem = MemoryTable::new();
    assert_eq!(run_in(&env, &mut mem, 1, &[tapped(5)]).len(), 1);
    assert_eq!(run_in(&env, &mut mem, 1, &[tapped(5)]).len(), 1);
    assert!(run_in(&env, &mut mem, 1, &[tapped(5)]).is_empty());
    assert_eq!(run_in(&env, &mut mem, 2, &[tapped(5)]).len(), 1);
}

// REQ-EVENT-002: perGame=1 never triggers again, even in later turns.
#[test]
fn spec_triggers_detect_limit_per_game() {
    let env = limited(None, Some(1));
    let mut mem = MemoryTable::new();
    assert_eq!(run_in(&env, &mut mem, 1, &[tapped(5), tapped(6)]).len(), 1);
    for turn in 1..6 {
        assert!(
            run_in(&env, &mut mem, turn, &[tapped(5)]).is_empty(),
            "{turn}"
        );
    }
}

// REQ-EVENT-002: perTurn and perGame together.
#[test]
fn spec_triggers_detect_limit_both() {
    let env = limited(Some(1), Some(2));
    let mut mem = MemoryTable::new();
    assert_eq!(run_in(&env, &mut mem, 1, &[tapped(5), tapped(6)]).len(), 1);
    assert!(run_in(&env, &mut mem, 1, &[tapped(5)]).is_empty());
    assert_eq!(run_in(&env, &mut mem, 2, &[tapped(5), tapped(6)]).len(), 1);
    assert!(run_in(&env, &mut mem, 3, &[tapped(5)]).is_empty());
    assert!(run_in(&env, &mut mem, 4, &[tapped(5)]).is_empty());
}

// REQ-EVENT-002: a ONE_OR_MORE trigger counts once against a limit.
#[test]
fn spec_triggers_detect_limit_one_or_more_counts_once() {
    let env = FakeEnv::new().with_post(reg(
        1,
        P1,
        with_limit(
            ability("a1", batch(m(K::Tapped), Some(DslBatch::OneOrMore))),
            Some(2),
            None,
        ),
    ));
    let mut mem = MemoryTable::new();
    assert_eq!(
        summary(&run_in(&env, &mut mem, 1, &[tapped(5), tapped(6)])),
        vec![(1, s("a1"), vec![0, 1])]
    );
    assert_eq!(run_in(&env, &mut mem, 1, &[tapped(5), tapped(6)]).len(), 1);
    assert!(run_in(&env, &mut mem, 1, &[tapped(5)]).is_empty());
}

// REQ-EVENT-002: limits are per ability instance: sources do not share a counter, nor do two
// abilities of one source.
#[test]
fn spec_triggers_detect_limit_per_instance() {
    let lim = |id: &str| with_limit(ability(id, m(K::Tapped)), Some(1), None);
    let env = FakeEnv::new()
        .with_post(reg(1, P1, lim("a1")))
        .with_post(reg(2, P1, lim("a1")))
        .with_post(reg(1, P1, lim("a2")));
    let mut mem = MemoryTable::new();
    let out = run_in(&env, &mut mem, 1, &[tapped(5), tapped(6)]);
    assert_eq!(
        summary(&out),
        vec![
            (1, s("a1"), vec![0]),
            (1, s("a2"), vec![0]),
            (2, s("a1"), vec![0]),
        ]
    );
    assert!(run_in(&env, &mut mem, 1, &[tapped(5)]).is_empty());

    // Source 2's counter is not consumed by source 1.
    let only1 = FakeEnv::new().with_post(reg(1, P1, lim("a1")));
    let mut mem = MemoryTable::new();
    assert_eq!(run_in(&only1, &mut mem, 1, &[tapped(5)]).len(), 1);
    let only2 = FakeEnv::new()
        .with_post(reg(2, P1, lim("a1")))
        .with_post(reg(1, P1, lim("a2")));
    assert_eq!(run_in(&only2, &mut mem, 1, &[tapped(5)]).len(), 2);
}

// REQ-EVENT-002, REQ-CORE-001: limit state lives only in the MemoryTable passed in: a clone
// taken before a call reproduces the call's result.
#[test]
fn spec_triggers_detect_limit_state_in_memory() {
    let env = limited(Some(1), None);
    let mut mem = MemoryTable::new();
    let before = mem.clone();
    let first = run_in(&env, &mut mem, 1, &[tapped(5)]);
    assert_eq!(first.len(), 1);
    assert_ne!(mem, before, "the limit was recorded in the table");
    let mut again = before.clone();
    assert_eq!(run_in(&env, &mut again, 1, &[tapped(5)]), first);
    assert_eq!(again, mem);
    assert!(run_in(&env, &mut mem, 1, &[tapped(5)]).is_empty());
}

// REQ-EVENT-002: an ability with no limit triggers on every matching event, every call.
#[test]
fn spec_triggers_detect_no_limit_unbounded() {
    let env = FakeEnv::new().with_post(reg(1, P1, ability("a1", m(K::Tapped))));
    let mut mem = MemoryTable::new();
    for _ in 0..3 {
        assert_eq!(run_in(&env, &mut mem, 1, &[tapped(5), tapped(6)]).len(), 2);
    }
}

// =============================================================================================
// K. non-event triggers and degenerate inputs
// =============================================================================================

// REQ-EVENT-002, CR 603.2: PHASE_STEP and STATE triggers are not detected from events.
#[test]
fn spec_triggers_detect_non_event_triggers_ignored() {
    let phase = reg(
        1,
        P1,
        ability_with(
            "p",
            Trigger::PhaseStep {
                step: StepId::Upkeep,
                whose: PlayerSelectorAST::Players(PlayerGroup::AllPlayers),
            },
        ),
    );
    let state = reg(
        2,
        P1,
        ability_with("s", Trigger::State(PredicateAST::IsCreature)),
    );
    let env = FakeEnv::new().with_both(phase).with_both(state);
    let events: Vec<TriggerEvent> = EventKind::ALL
        .iter()
        .map(|k| ev_obj(&[*k], Some(5), Some(5)).with_player(P1))
        .collect();
    assert!(run(&env, &events).is_empty());
}

// REQ-EVENT-002, R12: empty inputs, empty kinds and missing subjects never panic.
#[test]
fn spec_triggers_detect_degenerate_inputs() {
    let env = FakeEnv::new()
        .with_both(reg(1, P1, ability("a1", subj_obj(m(K::Dies)))))
        .with_both(reg(2, P1, ability("a2", m(K::Tapped))));
    assert!(run(&env, &[]).is_empty());
    assert!(run(&FakeEnv::new(), &[tapped(5), dies_event(1, 2)]).is_empty());
    let odd = vec![
        TriggerEvent::new(Vec::new()),
        TriggerEvent::new(Vec::new()).with_object(Some(oid(1)), None),
        TriggerEvent::new(vec![K::Dies]),
        TriggerEvent::new(vec![K::Dies, K::Dies]).with_object(None, None),
    ];
    assert!(run(&env, &odd).is_empty());
    // Duplicate kinds in one event are still one occurrence.
    let out = run(&env, &[TriggerEvent::new(vec![K::Tapped, K::Tapped])]);
    assert_eq!(summary(&out), vec![(2, s("a2"), vec![0])]);
}

// =============================================================================================
// L. the waiting-trigger queue
// =============================================================================================

// REQ-EVENT-002, CR 603.3: push_all appends in order; iter and take_all yield creation order;
// take_all drains.
#[test]
fn spec_triggers_detect_queue_fifo() {
    let mut q = WaitingTriggers::new();
    assert!(q.is_empty());
    assert_eq!(q.len(), 0);
    assert_eq!(q, WaitingTriggers::default());
    let a = wt(9, P1, "a", 0);
    let b = wt(1, P2, "b", 1);
    let c = wt(5, P1, "c", 2);
    q.push_all(vec![a.clone(), b.clone()]);
    q.push_all(Vec::new());
    q.push_all(vec![c.clone()]);
    assert_eq!(q.len(), 3);
    assert!(!q.is_empty());
    assert_eq!(
        q.iter().cloned().collect::<Vec<_>>(),
        vec![a.clone(), b.clone(), c.clone()]
    );
    let snapshot = q.clone();
    assert_eq!(q.take_all(), vec![a, b, c]);
    assert!(q.is_empty());
    assert_eq!(q.len(), 0);
    assert!(q.take_all().is_empty());
    assert_ne!(q, snapshot);
}

// REQ-EVENT-002, CR 603.3: take_for_controller removes only that player's triggers, in creation
// order, and leaves the rest in their original order.
#[test]
fn spec_triggers_detect_queue_take_for_controller() {
    let mut q = WaitingTriggers::new();
    let t = [
        wt(1, P1, "a", 0),
        wt(2, P2, "b", 0),
        wt(3, P1, "c", 1),
        wt(4, P3, "d", 1),
        wt(5, P2, "e", 2),
        wt(6, P1, "f", 2),
    ];
    q.push_all(t.to_vec());
    assert_eq!(
        q.take_for_controller(P1),
        vec![t[0].clone(), t[2].clone(), t[5].clone()]
    );
    assert_eq!(q.len(), 3);
    assert_eq!(
        q.iter().cloned().collect::<Vec<_>>(),
        vec![t[1].clone(), t[3].clone(), t[4].clone()]
    );
    assert!(q.take_for_controller(P1).is_empty());
    assert_eq!(q.take_for_controller(P2), vec![t[1].clone(), t[4].clone()]);
    assert_eq!(q.take_all(), vec![t[3].clone()]);
    assert!(q.is_empty());
}

// REQ-EVENT-002: detect output feeds the queue in output order.
#[test]
fn spec_triggers_detect_queue_from_detect() {
    let env = FakeEnv::new()
        .with_post(reg(9, P2, ability("a1", m(K::Tapped))))
        .with_post(reg(3, P1, ability("a1", m(K::Tapped))));
    let out = run(&env, &[tapped(5)]);
    let mut q = WaitingTriggers::new();
    q.push_all(out.clone());
    assert_eq!(q.iter().cloned().collect::<Vec<_>>(), out);
    assert_eq!(q.take_for_controller(P2), vec![out[1].clone()]);
}

// =============================================================================================
// property loops (seeded LCG; no wall clock, no OS randomness)
// =============================================================================================

const KIND_POOL: [K; 12] = [
    K::ZoneChange,
    K::Dies,
    K::LeavesBattlefield,
    K::PutIntoGraveyard,
    K::Enters,
    K::Tapped,
    K::Sacrificed,
    K::PhasedOut,
    K::ControlChanged,
    K::LifeGained,
    K::PlayerLost,
    K::ReturnedToHand,
];
const ZONES: [Zone; 5] = [
    Zone::Battlefield,
    Zone::Graveyard,
    Zone::Hand,
    Zone::Library,
    Zone::Exile,
];
const PLAYERS: [fineprint_base::ids::PlayerId; 3] = [P1, P2, P3];

struct Scenario {
    env: FakeEnv,
    regs: Vec<TriggerRegistration>,
    events: Vec<TriggerEvent>,
}

fn gen_scenario(rng: &mut Lcg) -> Scenario {
    let mut events = Vec::new();
    for _ in 0..rng.below(6) {
        let mut kinds = Vec::new();
        for _ in 0..rng.below(4) {
            kinds.push(rng.pick(&KIND_POOL));
        }
        let mut e = TriggerEvent::new(kinds);
        let pre = 1 + rng.below(8);
        let post = if rng.coin() { pre } else { pre + 50 };
        e = e.with_object(
            if rng.below(5) == 0 {
                None
            } else {
                Some(oid(pre))
            },
            if rng.below(5) == 0 {
                None
            } else {
                Some(oid(post))
            },
        );
        if rng.coin() {
            e = e.with_player(rng.pick(&PLAYERS));
        }
        if rng.coin() {
            e = e.with_field("from", vzone(rng.pick(&ZONES)));
        }
        if rng.coin() {
            e = e.with_field("to", vzone(rng.pick(&ZONES)));
        }
        if rng.coin() {
            let n = i64::try_from(rng.below(3)).expect("small");
            e = e.with_field("amount", vint(n));
        }
        events.push(e);
    }

    let mut env = FakeEnv::new();
    for id in 1..60 {
        if rng.coin() {
            env.objects.insert((PreEvent, oid(id)));
        }
        if rng.coin() {
            env.objects.insert((PostEvent, oid(id)));
        }
        // Every object is visible (the invisible ToHandOrLibrary case is excluded here).
        env.visible.insert(oid(id));
    }
    for p in PLAYERS {
        if rng.coin() {
            env.players.insert((PreEvent, p));
        }
        if rng.coin() {
            env.players.insert((PostEvent, p));
        }
    }
    if rng.coin() {
        env.conds.insert((PreEvent, s("c")));
    }
    if rng.coin() {
        env.conds.insert((PostEvent, s("c")));
    }

    let mut regs = Vec::new();
    for i in 0..(1 + rng.below(5)) {
        let mut mm = m(rng.pick(&KIND_POOL));
        match rng.below(4) {
            0 => mm = subj_obj(mm),
            1 => mm = subj_players(mm),
            _ => {}
        }
        if rng.coin() {
            mm = filt(mm, "from", zone(rng.pick(&ZONES)));
        }
        if rng.below(3) == 0 {
            mm = filt(mm, "to", zone(rng.pick(&ZONES)));
        }
        if rng.coin() {
            mm = bind(bind(mm, "object", "$X"), "amount", "$N");
        }
        mm = batch(
            mm,
            rng.pick(&[None, Some(DslBatch::Each), Some(DslBatch::OneOrMore)]),
        );
        let mut a = ability(&format!("a{i}"), mm);
        if rng.below(3) == 0 {
            a = with_if(a, "c");
        }
        if rng.below(3) == 0 {
            a = with_limit(a, Some(1), None);
        }
        let r = reg(100 + rng.below(4), rng.pick(&PLAYERS), a);
        match rng.below(3) {
            0 => env.pre.push(r.clone()),
            1 => env.post.push(r.clone()),
            _ => {
                env.pre.push(r.clone());
                env.post.push(r.clone());
            }
        }
        regs.push(r);
    }
    Scenario { env, regs, events }
}

// REQ-EVENT-002, REQ-CORE-001: detect is a function of its inputs: identical inputs give
// identical output and identical memory.
#[test]
fn spec_triggers_detect_property_deterministic() {
    let mut rng = Lcg::new(0x0029_0001);
    for _ in 0..400 {
        let sc = gen_scenario(&mut rng);
        let mut m1 = MemoryTable::new();
        let mut m2 = MemoryTable::new();
        let turn = 1 + rng.below(3);
        let a = detect(&sc.env, &mut m1, turn, &sc.events);
        let b = detect(&sc.env, &mut m2, turn, &sc.events);
        assert_eq!(a, b);
        assert_eq!(m1, m2);
        // Output order: ascending (first event index, source, ability id); indices in range and
        // ascending.
        let keys: Vec<(usize, u64, String)> = a
            .iter()
            .map(|t| (t.event_indices[0], t.source.get(), t.ability_id.clone()))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        for t in &a {
            assert!(!t.event_indices.is_empty());
            assert!(t.event_indices.windows(2).all(|w| w[0] < w[1]));
            assert!(t.event_indices.iter().all(|&i| i < sc.events.len()));
        }
    }
}

// REQ-EVENT-002, CR 603.3 (order independence of detection): the order in which the env lists
// registrations does not change the output.
#[test]
fn spec_triggers_detect_property_registration_order() {
    let mut rng = Lcg::new(0x0029_0002);
    for _ in 0..400 {
        let mut sc = gen_scenario(&mut rng);
        // gen_scenario gives every registration a distinct (source, ability id).
        let base = detect(&sc.env, &mut MemoryTable::new(), 1, &sc.events);
        for _ in 0..3 {
            rng.shuffle(&mut sc.env.pre);
            rng.shuffle(&mut sc.env.post);
            let again = detect(&sc.env, &mut MemoryTable::new(), 1, &sc.events);
            assert_eq!(again, base);
        }
    }
}

// REQ-EVENT-002, CR 603.10: a trigger has look_back Some iff its matcher's look_back_kind is
// Some, and then equal to it (every object visible).
#[test]
fn spec_triggers_detect_property_look_back_flag() {
    let mut rng = Lcg::new(0x0029_0003);
    let mut seen_some = 0;
    let mut seen_none = 0;
    for _ in 0..600 {
        let sc = gen_scenario(&mut rng);
        let out = detect(&sc.env, &mut MemoryTable::new(), 1, &sc.events);
        for t in &out {
            let r = sc
                .regs
                .iter()
                .find(|r| r.source == t.source && r.ability.base.ability_id == t.ability_id)
                .expect("trigger comes from a registration");
            let Trigger::Event(mm) = &r.ability.trigger else {
                panic!("only event triggers are detected");
            };
            let derived = look_back_kind(mm);
            // Barring the invisible ToHandOrLibrary case: with no pre-event id there is nothing
            // to ask is_visible_to_all about, so such triggers are not judged here.
            if derived == Some(LookBackKind::ToHandOrLibrary)
                && t.event_indices
                    .iter()
                    .any(|&i| sc.events[i].subject_pre.is_none())
            {
                continue;
            }
            assert_eq!(t.look_back, derived, "{t:?}");
            assert_eq!(t.controller, r.controller);
            if t.look_back.is_some() {
                seen_some += 1;
                assert!(
                    sc.env.pre.contains(r),
                    "look-back uses a pre-event registration"
                );
            } else {
                seen_none += 1;
                assert!(
                    sc.env.post.contains(r),
                    "others use a post-event registration"
                );
            }
        }
    }
    assert!(seen_some > 0 && seen_none > 0, "{seen_some} {seen_none}");
}
