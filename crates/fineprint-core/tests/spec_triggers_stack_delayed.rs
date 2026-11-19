// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for delayed and reflexive triggered abilities (REQ-EVENT-002; CR 603.7, 603.7b,
//! 603.7d-g, 603.12, 603.12a): `DelayedTriggers` (registration ids, captured bindings, one-shot
//! versus stated duration, fire, cancel, detection end to end) and `reflexive_triggers`.
//! Black-box: written from the work-item brief only.

#[path = "spec_triggers_stack_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_core::memory::MemoryTable;
use fineprint_core::triggers::{
    DelayedTriggers, EventField, PendingTrigger, TriggerEvent, TriggerOrigin, TriggerRegistration,
    detect, reflexive_triggers,
};
use fineprint_core::triggers::{LookBackKind, StateView};
use fineprint_dsl::ability::{EventKind, FilterValue, Trigger, TriggeredAbilityAST};
use support::*;

fn vint(n: i64) -> EventField {
    EventField::Value(FilterValue::Int(n))
}

fn binds(pairs: &[(&str, EventField)]) -> BTreeMap<String, EventField> {
    pairs.iter().map(|(k, v)| (s(k), v.clone())).collect()
}

/// `a` with its ability id replaced (what `registrations()` must list).
fn renamed(mut a: TriggeredAbilityAST, id: &str) -> TriggeredAbilityAST {
    a.base.ability_id = id.to_string();
    a
}

fn delayed_ability() -> TriggeredAbilityAST {
    ev_ability("test.trigger_delayed", EventKind::Dies)
}

// =============================================================================================
// DelayedTriggers (CR 603.7)
// =============================================================================================

/// REQ-EVENT-002; CR 603.7, 603.7d-g: ids are 0, 1, 2, ... and never reused after a cancel;
/// `registrations()` lists them by id with the stored source and controller and the ability id
/// replaced by `delayed#<n>`.
#[test]
fn req_event_002_delayed_ids_and_registrations() {
    let mut dt = DelayedTriggers::new();
    assert!(dt.is_empty());
    assert_eq!(dt.len(), 0);
    assert_eq!(dt, DelayedTriggers::default());
    let a = delayed_ability();
    let b = ev_ability("test.trigger_delayed_b", EventKind::Drawn);
    let i0 = dt.register(oid(9), P2, a.clone(), BTreeMap::new(), true);
    let i1 = dt.register(oid(3), P1, b.clone(), BTreeMap::new(), false);
    let i2 = dt.register(oid(9), P1, a.clone(), BTreeMap::new(), true);
    assert_eq!((i0.get(), i1.get(), i2.get()), (0, 1, 2));
    assert!(i0 < i1 && i1 < i2);
    assert_eq!(dt.len(), 3);
    assert!(!dt.is_empty());
    assert_eq!(
        dt.registrations(),
        vec![
            TriggerRegistration {
                source: oid(9),
                controller: P2,
                ability: renamed(a.clone(), "delayed#0"),
            },
            TriggerRegistration {
                source: oid(3),
                controller: P1,
                ability: renamed(b.clone(), "delayed#1"),
            },
            TriggerRegistration {
                source: oid(9),
                controller: P1,
                ability: renamed(a.clone(), "delayed#2"),
            },
        ]
    );
    assert!(dt.cancel(i1));
    assert!(!dt.cancel(i1));
    assert_eq!(dt.len(), 2);
    let i3 = dt.register(oid(4), P2, b, BTreeMap::new(), false);
    assert_eq!(i3.get(), 3);
    let ids: Vec<String> = dt
        .registrations()
        .iter()
        .map(|r| r.ability.base.ability_id.clone())
        .collect();
    assert_eq!(ids, vec![s("delayed#0"), s("delayed#2"), s("delayed#3")]);
}

/// REQ-EVENT-002; CR 603.7, 603.7c: the bindings captured at creation survive and win over the
/// event's on a name clash; the pending trigger has origin Delayed, the delayed#<n> id and the
/// registered source and controller.
#[test]
fn req_event_002_delayed_captured_bindings_win() {
    let mut dt = DelayedTriggers::new();
    let captured = binds(&[
        ("$X", EventField::Object(oid(5))),
        ("$P", EventField::Player(P1)),
    ]);
    dt.register(oid(9), P2, delayed_ability(), captured, false);
    let waiting = vec![wt(
        9,
        P2,
        "delayed#0",
        binds(&[("$P", EventField::Player(P3)), ("$N", vint(4))]),
    )];
    let out = dt.take_triggered(&CondEnv::new(), &waiting);
    assert_eq!(
        out,
        vec![PendingTrigger {
            origin: TriggerOrigin::Delayed,
            controller: P2,
            source: Some(oid(9)),
            ability_id: s("delayed#0"),
            bindings: binds(&[
                ("$X", EventField::Object(oid(5))),
                ("$P", EventField::Player(P1)),
                ("$N", vint(4)),
            ]),
            optional: false,
            responds_to_trigger: false,
            targets: vec![],
            modes: None,
        }]
    );
}

/// REQ-EVENT-002; CR 603.7b: a delayed trigger without a stated duration triggers only once: at
/// most one pending trigger (the first matching entry) however many match, and it is removed.
#[test]
fn req_event_002_delayed_one_shot_triggers_once() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(oid(9), P2, delayed_ability(), BTreeMap::new(), true);
    let waiting = vec![
        wt(9, P2, "delayed#0", binds(&[("$N", vint(1))])),
        wt(9, P2, "delayed#0", binds(&[("$N", vint(2))])),
    ];
    let out = dt.take_triggered(&CondEnv::new(), &waiting);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].bindings, binds(&[("$N", vint(1))]));
    assert!(dt.is_empty());
    assert!(dt.registrations().is_empty());
    assert!(dt.take_triggered(&CondEnv::new(), &waiting).is_empty());
    assert_eq!(dt.fire(id), None);
    assert!(!dt.cancel(id));
}

/// REQ-EVENT-002; CR 603.7b: a delayed trigger with a stated duration yields one pending trigger
/// per matching entry and stays registered.
#[test]
fn req_event_002_delayed_with_duration_triggers_each_time() {
    let mut dt = DelayedTriggers::new();
    dt.register(oid(9), P2, delayed_ability(), BTreeMap::new(), false);
    let waiting = vec![
        wt(9, P2, "delayed#0", binds(&[("$N", vint(1))])),
        wt(9, P2, "delayed#0", binds(&[("$N", vint(2))])),
    ];
    for _ in 0..2 {
        let out = dt.take_triggered(&CondEnv::new(), &waiting);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].bindings, binds(&[("$N", vint(1))]));
        assert_eq!(out[1].bindings, binds(&[("$N", vint(2))]));
        assert_eq!(dt.len(), 1);
    }
}

/// REQ-EVENT-002; CR 603.7: entries that are not delayed registrations (another ability id, or
/// the right id on another source) are ignored and consume nothing; output follows input order.
#[test]
fn req_event_002_delayed_ignores_others_and_keeps_input_order() {
    let mut dt = DelayedTriggers::new();
    dt.register(oid(9), P2, delayed_ability(), BTreeMap::new(), true);
    dt.register(oid(7), P1, delayed_ability(), BTreeMap::new(), true);
    let others = vec![
        wt(9, P2, "test.trigger_other", BTreeMap::new()),
        wt(8, P2, "delayed#0", BTreeMap::new()),
        wt(9, P2, "delayed#5", BTreeMap::new()),
    ];
    assert!(dt.take_triggered(&CondEnv::new(), &others).is_empty());
    assert_eq!(dt.len(), 2);
    let waiting = vec![
        wt(9, P2, "test.trigger_other", BTreeMap::new()),
        wt(7, P1, "delayed#1", BTreeMap::new()),
        wt(9, P2, "delayed#0", BTreeMap::new()),
    ];
    let out = dt.take_triggered(&CondEnv::new(), &waiting);
    let ids: Vec<(Option<u64>, String)> = out
        .iter()
        .map(|p| (p.source.map(|o| o.get()), p.ability_id.clone()))
        .collect();
    assert_eq!(
        ids,
        vec![(Some(7), s("delayed#1")), (Some(9), s("delayed#0"))]
    );
    assert_eq!(out[0].controller, P1);
    assert_eq!(out[1].controller, P2);
    assert!(dt.is_empty());
}

/// REQ-EVENT-002; CR 603.7, 603.7b: `fire` (phase/step delayed triggers) yields the captured
/// bindings; a one-shot is removed, one with a duration stays; a cancelled one does not fire.
#[test]
fn req_event_002_delayed_fire_and_cancel() {
    let mut dt = DelayedTriggers::new();
    let captured = binds(&[("$X", EventField::Object(oid(5)))]);
    let once = dt.register(oid(9), P2, delayed_ability(), captured.clone(), true);
    let lasting = dt.register(oid(8), P1, delayed_ability(), BTreeMap::new(), false);
    let gone = dt.register(oid(7), P1, delayed_ability(), BTreeMap::new(), false);
    assert!(dt.cancel(gone));
    assert_eq!(dt.fire(gone), None);

    let p = dt.fire(once).expect("fires");
    assert_eq!(p.origin, TriggerOrigin::Delayed);
    assert_eq!(p.source, Some(oid(9)));
    assert_eq!(p.controller, P2);
    assert_eq!(p.ability_id, s("delayed#0"));
    assert_eq!(p.bindings, captured);
    assert_eq!(dt.fire(once), None);

    for _ in 0..2 {
        let p = dt.fire(lasting).expect("fires");
        assert_eq!(p.ability_id, s("delayed#1"));
        assert_eq!(p.controller, P1);
        assert!(p.bindings.is_empty());
    }
    assert_eq!(dt.len(), 1);
}

/// REQ-EVENT-002; CR 603.7, 603.3b, 603.5: optional, targets, modes and responds_to_trigger come
/// from the ability as in `from_waiting`.
#[test]
fn req_event_002_delayed_copies_ability_fields() {
    let mut a = ability_with(
        "test.trigger_delayed_t",
        Trigger::Event(m(EventKind::Triggered)),
    );
    a.optional = Some(true);
    a.targets = Some(vec![slot(0, 1, 1)]);
    let ms = modeset(1, 1, vec![mode("x", vec![])]);
    a.modes = Some(ms.clone());
    let mut dt = DelayedTriggers::new();
    let id = dt.register(oid(9), P2, a, BTreeMap::new(), false);
    for p in [
        dt.fire(id).expect("fires"),
        dt.take_triggered(&CondEnv::new(), &[wt(9, P2, "delayed#0", BTreeMap::new())])
            .pop()
            .expect("triggers"),
    ] {
        assert!(p.optional);
        assert!(p.responds_to_trigger);
        assert_eq!(p.targets, vec![slot(0, 1, 1)]);
        assert_eq!(p.modes, Some(ms.clone()));
    }
}

/// REQ-EVENT-002; CR 603.7, 603.7a: end to end: `detect` over an environment whose registrations
/// are `DelayedTriggers::registrations()` finds the delayed trigger, and `take_triggered` turns it
/// into a pending trigger with the captured bindings winning.
#[test]
fn req_event_002_delayed_end_to_end_with_detect() {
    let mut dt = DelayedTriggers::new();
    let gain = ability_with(
        "test.trigger_delayed_gain",
        Trigger::Event(bind(
            bind(m(EventKind::LifeGained), "amount", "$N"),
            "player",
            "$P",
        )),
    );
    let captured = binds(&[
        ("$P", EventField::Player(P3)),
        ("$X", EventField::Object(oid(5))),
    ]);
    dt.register(oid(9), P2, gain, captured, true);
    dt.register(
        oid(9),
        P2,
        ev_ability("test.trigger_delayed_dies", EventKind::Dies),
        BTreeMap::new(),
        true,
    );
    let mut env = CondEnv::new();
    env.post = dt.registrations();
    env.pre = dt.registrations();
    let events = vec![
        TriggerEvent::new(vec![EventKind::LifeGained])
            .with_player(P1)
            .with_field("amount", vint(2)),
    ];
    let waiting = detect(&env, &mut MemoryTable::new(), 1, &events);
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].ability_id, s("delayed#0"));
    let out = dt.take_triggered(&env, &waiting);
    assert_eq!(out.len(), 1);
    let p = &out[0];
    assert_eq!(p.origin, TriggerOrigin::Delayed);
    assert_eq!(p.source, Some(oid(9)));
    assert_eq!(p.controller, P2);
    assert_eq!(p.ability_id, s("delayed#0"));
    assert_eq!(
        p.bindings,
        binds(&[
            ("$N", vint(2)),
            ("$P", EventField::Player(P3)),
            ("$X", EventField::Object(oid(5))),
        ])
    );
    // The one-shot is gone; the other delayed trigger is still waiting for its event.
    let left: Vec<String> = dt
        .registrations()
        .iter()
        .map(|r| r.ability.base.ability_id.clone())
        .collect();
    assert_eq!(left, vec![s("delayed#1")]);
}

// =============================================================================================
// intervening "if" of delayed triggers (CR 603.4, 603.7, 603.7b)
// =============================================================================================

fn if_ability() -> TriggeredAbilityAST {
    with_if(
        ev_ability("test.trigger_delayed_if", EventKind::Dies),
        "test.cond",
    )
}

/// REQ-EVENT-002; CR 603.4, 603.7c: the intervening "if" sees the event bindings overlaid by the
/// captured ones (captured wins): a condition true only for the captured value lets it fire, and
/// the pending trigger carries the captured value.
#[test]
fn req_event_002_delayed_if_sees_captured_bindings() {
    let mut dt = DelayedTriggers::new();
    let captured = binds(&[("x", EventField::Object(oid(1)))]);
    dt.register(oid(9), P2, if_ability(), captured, true);
    let env = BindEnv::new().requiring("test.cond", "x", EventField::Object(oid(1)));
    let waiting = vec![wt(
        9,
        P2,
        "delayed#0",
        binds(&[("x", EventField::Object(oid(2)))]),
    )];
    let out = dt.take_triggered(&env, &waiting);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].bindings, binds(&[("x", EventField::Object(oid(1)))]));
    assert_eq!(out[0].ability_id, s("delayed#0"));
    let calls = env.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, s("test.cond"));
    assert_eq!(calls[0].2, binds(&[("x", EventField::Object(oid(1)))]));
    assert!(dt.is_empty());
}

/// REQ-EVENT-002; CR 603.4, 603.7b: the mirror: a condition true only for the event's value
/// fails (the captured value wins), nothing triggers and the one-shot entry is kept.
#[test]
fn req_event_002_delayed_if_false_keeps_one_shot() {
    let mut dt = DelayedTriggers::new();
    let captured = binds(&[("x", EventField::Object(oid(1)))]);
    dt.register(oid(9), P2, if_ability(), captured, true);
    let env = BindEnv::new().requiring("test.cond", "x", EventField::Object(oid(2)));
    let waiting = vec![wt(
        9,
        P2,
        "delayed#0",
        binds(&[("x", EventField::Object(oid(2)))]),
    )];
    assert!(dt.take_triggered(&env, &waiting).is_empty());
    assert_eq!(dt.len(), 1);
    assert!(!dt.is_empty());
}

/// REQ-EVENT-002; CR 603.4, 603.7b: a one-shot with two matching elements, the first failing its
/// condition: exactly one pending trigger, built from the second element, entry removed.
#[test]
fn req_event_002_delayed_if_one_shot_second_element_fires() {
    let mut dt = DelayedTriggers::new();
    dt.register(oid(9), P2, if_ability(), BTreeMap::new(), true);
    let env = BindEnv::new().requiring("test.cond", "n", vint(2));
    let waiting = vec![
        wt(9, P2, "delayed#0", binds(&[("n", vint(1))])),
        wt(9, P2, "delayed#0", binds(&[("n", vint(2))])),
    ];
    let out = dt.take_triggered(&env, &waiting);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].bindings, binds(&[("n", vint(2))]));
    assert!(dt.is_empty());
    assert!(dt.take_triggered(&env, &waiting).is_empty());
}

/// REQ-EVENT-002; CR 603.4, 603.7b: with a stated duration, one pending trigger per passing
/// element (failing ones are dropped) and the registration stays.
#[test]
fn req_event_002_delayed_if_duration_one_per_passing_element() {
    let mut dt = DelayedTriggers::new();
    dt.register(oid(9), P2, if_ability(), BTreeMap::new(), false);
    let env = BindEnv::new().requiring("test.cond", "n", vint(2));
    let waiting = vec![
        wt(9, P2, "delayed#0", binds(&[("n", vint(2))])),
        wt(9, P2, "delayed#0", binds(&[("n", vint(1))])),
        wt(9, P2, "delayed#0", binds(&[("n", vint(2)), ("k", vint(7))])),
    ];
    for _ in 0..2 {
        let out = dt.take_triggered(&env, &waiting);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].bindings, binds(&[("n", vint(2))]));
        assert_eq!(out[1].bindings, binds(&[("n", vint(2)), ("k", vint(7))]));
        assert_eq!(dt.len(), 1);
    }
}

/// REQ-EVENT-002; CR 603.4, 603.7: `registrations()` no longer carries the intervening "if"
/// (it is checked in `take_triggered`); the ability id is still `delayed#<n>`.
#[test]
fn req_event_002_delayed_registrations_drop_intervening_if() {
    let mut dt = DelayedTriggers::new();
    let a = if_ability();
    assert!(a.intervening_if.is_some());
    dt.register(oid(9), P2, a.clone(), BTreeMap::new(), true);
    let regs = dt.registrations();
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].ability.intervening_if, None);
    assert_eq!(regs[0].ability.base.ability_id, s("delayed#0"));
    assert_eq!(regs[0].source, oid(9));
    assert_eq!(regs[0].controller, P2);
}

/// REQ-EVENT-002; CR 603.4, 603.7: the condition is evaluated in the pre-event view when the
/// waiting element has a look-back kind, otherwise in the post-event view.
#[test]
fn req_event_002_delayed_if_view_follows_look_back() {
    let mut dt = DelayedTriggers::new();
    dt.register(oid(9), P2, if_ability(), BTreeMap::new(), false);
    let env = BindEnv::new();
    let mut looked = wt(9, P2, "delayed#0", BTreeMap::new());
    looked.look_back = Some(LookBackKind::LeavesBattlefield);
    let plain = wt(9, P2, "delayed#0", BTreeMap::new());
    let out = dt.take_triggered(&env, &[looked, plain]);
    assert_eq!(out.len(), 2);
    let views: Vec<StateView> = env.calls.borrow().iter().map(|c| c.0).collect();
    assert_eq!(views, vec![StateView::PreEvent, StateView::PostEvent]);
}

// =============================================================================================
// reflexive triggers (CR 603.12, 603.12a)
// =============================================================================================

/// REQ-EVENT-002; CR 603.12: no occurrence earlier in the resolution, no reflexive trigger.
#[test]
fn req_event_002_reflexive_zero_occurrences() {
    let a = ev_ability("test.trigger_reflexive", EventKind::Sacrificed);
    for repeated in [false, true] {
        assert!(
            reflexive_triggers(oid(4), P1, &a, BTreeMap::new(), 0, repeated).is_empty(),
            "repeated_payment={repeated}"
        );
    }
}

/// REQ-EVENT-002; CR 603.12a: n occurrences give n reflexive triggers with origin Reflexive, the
/// ability id unchanged and the bindings copied.
#[test]
fn req_event_002_reflexive_once_per_occurrence() {
    let mut a = ev_ability("test.trigger_reflexive", EventKind::Sacrificed);
    a.targets = Some(vec![slot(0, 1, 1)]);
    a.optional = Some(true);
    let b = binds(&[("$X", EventField::Object(oid(6)))]);
    for n in [1u32, 2, 3] {
        let out = reflexive_triggers(oid(4), P2, &a, b.clone(), n, false);
        assert_eq!(out.len(), n as usize);
        for p in &out {
            assert_eq!(
                p,
                &PendingTrigger {
                    origin: TriggerOrigin::Reflexive,
                    controller: P2,
                    source: Some(oid(4)),
                    ability_id: s("test.trigger_reflexive"),
                    bindings: b.clone(),
                    optional: true,
                    responds_to_trigger: false,
                    targets: vec![slot(0, 1, 1)],
                    modes: None,
                }
            );
        }
    }
}

/// REQ-EVENT-002; CR 603.12a: a cost paid one or more times triggers the reflexive ability only
/// once.
#[test]
fn req_event_002_reflexive_repeated_payment_once() {
    let a = ev_ability("test.trigger_reflexive", EventKind::Sacrificed);
    for n in [1u32, 2, 5] {
        let out = reflexive_triggers(oid(4), P1, &a, BTreeMap::new(), n, true);
        assert_eq!(out.len(), 1, "occurrences={n}");
        assert_eq!(out[0].origin, TriggerOrigin::Reflexive);
    }
}
