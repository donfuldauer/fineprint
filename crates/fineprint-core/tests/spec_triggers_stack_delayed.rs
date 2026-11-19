// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for delayed and reflexive triggered abilities (REQ-EVENT-002; CR 603.7, 603.7b,
//! 603.7d-g, 603.12, 603.12a): `DelayedTriggers` (registration ids, captured bindings, one-shot
//! versus stated duration, fire, cancel, detection end to end) and `reflexive_triggers`.
//! Black-box: written from the work-item brief only.

#[path = "spec_triggers_stack_support.rs"]
mod support;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::memory::MemoryTable;
use fineprint_core::triggers::{
    DelayedTriggers, EventField, PendingTrigger, TriggerEnv, TriggerEvent, TriggerOrigin,
    TriggerRegistration, detect, reflexive_triggers,
};
use fineprint_core::triggers::{LookBackKind, StateView};
use fineprint_dsl::ability::{
    ActivationLimit, EventKind, FilterValue, Trigger, TriggeredAbilityAST,
};
use fineprint_dsl::expr::{ObjectSelectorAST, PlayerGroup, PredicateAST};
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
    let out = dt.take_triggered(&CondEnv::new(), 1, &waiting);
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
    let out = dt.take_triggered(&CondEnv::new(), 1, &waiting);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].bindings, binds(&[("$N", vint(1))]));
    assert!(dt.is_empty());
    assert!(dt.registrations().is_empty());
    assert!(dt.take_triggered(&CondEnv::new(), 1, &waiting).is_empty());
    assert_eq!(dt.fire(&FireEnv::new(true), 1, id), None);
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
        let out = dt.take_triggered(&CondEnv::new(), 1, &waiting);
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
    assert!(dt.take_triggered(&CondEnv::new(), 1, &others).is_empty());
    assert_eq!(dt.len(), 2);
    let waiting = vec![
        wt(9, P2, "test.trigger_other", BTreeMap::new()),
        wt(7, P1, "delayed#1", BTreeMap::new()),
        wt(9, P2, "delayed#0", BTreeMap::new()),
    ];
    let out = dt.take_triggered(&CondEnv::new(), 1, &waiting);
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
    assert_eq!(dt.fire(&FireEnv::new(true), 1, gone), None);

    let p = dt.fire(&FireEnv::new(true), 1, once).expect("fires");
    assert_eq!(p.origin, TriggerOrigin::Delayed);
    assert_eq!(p.source, Some(oid(9)));
    assert_eq!(p.controller, P2);
    assert_eq!(p.ability_id, s("delayed#0"));
    assert_eq!(p.bindings, captured);
    assert_eq!(dt.fire(&FireEnv::new(true), 1, once), None);

    for _ in 0..2 {
        let p = dt.fire(&FireEnv::new(true), 1, lasting).expect("fires");
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
        dt.fire(&FireEnv::new(true), 1, id).expect("fires"),
        dt.take_triggered(
            &CondEnv::new(),
            1,
            &[wt(9, P2, "delayed#0", BTreeMap::new())],
        )
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
    let out = dt.take_triggered(&env, 1, &waiting);
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
    let out = dt.take_triggered(&env, 1, &waiting);
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
    assert!(dt.take_triggered(&env, 1, &waiting).is_empty());
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
    let out = dt.take_triggered(&env, 1, &waiting);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].bindings, binds(&[("n", vint(2))]));
    assert!(dt.is_empty());
    assert!(dt.take_triggered(&env, 1, &waiting).is_empty());
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
        let out = dt.take_triggered(&env, 1, &waiting);
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
    let out = dt.take_triggered(&env, 1, &[looked, plain]);
    assert_eq!(out.len(), 2);
    let views: Vec<StateView> = env.calls.borrow().iter().map(|c| c.0).collect();
    assert_eq!(views, vec![StateView::PreEvent, StateView::PostEvent]);
}

// =============================================================================================
// delayed triggers: intervening "if" and limit are applied by `take_triggered` (CR 603.2h,
// 603.4, 603.7b)
// =============================================================================================

/// An environment listing the registrations of a `DelayedTriggers` whose `condition_holds` reads
/// the bindings map: a condition holds only when the binding `n` equals 2.
struct LimitEnv {
    regs: Vec<TriggerRegistration>,
}

impl LimitEnv {
    fn of(dt: &DelayedTriggers) -> LimitEnv {
        LimitEnv {
            regs: dt.registrations(),
        }
    }
}

impl TriggerEnv for LimitEnv {
    fn registrations(&self, _view: StateView) -> Vec<TriggerRegistration> {
        self.regs.clone()
    }

    fn object_matches(
        &self,
        _view: StateView,
        _reg: &TriggerRegistration,
        _object: ObjectId,
        _selector: &ObjectSelectorAST,
    ) -> bool {
        true
    }

    fn player_matches(
        &self,
        _view: StateView,
        _reg: &TriggerRegistration,
        _player: PlayerId,
        _group: &PlayerGroup,
    ) -> bool {
        true
    }

    fn predicate_holds(
        &self,
        _view: StateView,
        _reg: &TriggerRegistration,
        _pred: &PredicateAST,
        _field: Option<&EventField>,
    ) -> bool {
        false
    }

    fn condition_holds(
        &self,
        _view: StateView,
        _reg: &TriggerRegistration,
        _pred: &PredicateAST,
        bindings: &BTreeMap<String, EventField>,
    ) -> bool {
        bindings.get("n") == Some(&vint(2))
    }

    fn is_visible_to_all(&self, _object: ObjectId) -> bool {
        true
    }
}

/// A delayed ability on a life-gain event binding the amount to `n`, with the intervening "if"
/// `test.cond` and the given limit.
fn limited_ability(per_turn: Option<u32>, per_game: Option<u32>) -> TriggeredAbilityAST {
    let mut a = with_if(
        ability_with(
            "test.trigger_delayed_limited",
            Trigger::Event(bind(m(EventKind::LifeGained), "amount", "n")),
        ),
        "test.cond",
    );
    a.limit = Some(ActivationLimit { per_turn, per_game });
    a
}

fn gain(n: i64) -> TriggerEvent {
    TriggerEvent::new(vec![EventKind::LifeGained])
        .with_player(P1)
        .with_field("amount", vint(n))
}

/// One `detect` call over `events` (fresh Memory) in `turn`, then `take_triggered`.
fn detect_and_take(
    dt: &mut DelayedTriggers,
    turn: u64,
    events: &[TriggerEvent],
) -> Vec<PendingTrigger> {
    let env = LimitEnv::of(dt);
    let waiting = detect(&env, &mut MemoryTable::new(), turn, events);
    dt.take_triggered(&env, turn, &waiting)
}

fn amounts(out: &[PendingTrigger]) -> Vec<EventField> {
    out.iter().map(|p| p.bindings["n"].clone()).collect()
}

/// REQ-EVENT-002; CR 603.2h, 603.4, 603.7b: one batch, limit once per turn, the "if" holds only
/// for the second of two events: `detect` must not spend the limit on the failing first event,
/// so exactly one pending trigger, for the second event.
#[test]
fn req_event_002_delayed_limit_one_batch_failing_first_does_not_consume() {
    let mut dt = DelayedTriggers::new();
    dt.register(
        oid(9),
        P2,
        limited_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    let out = detect_and_take(&mut dt, 5, &[gain(1), gain(2)]);
    assert_eq!(out.len(), 1);
    assert_eq!(amounts(&out), vec![vint(2)]);
    assert_eq!(dt.len(), 1);
}

/// REQ-EVENT-002; CR 603.2h, 603.4: two calls in the same turn: a failing event first leaves the
/// limit unspent, so a passing event in the second call triggers.
#[test]
fn req_event_002_delayed_limit_two_calls_failing_then_passing() {
    let mut dt = DelayedTriggers::new();
    dt.register(
        oid(9),
        P2,
        limited_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    assert!(detect_and_take(&mut dt, 5, &[gain(1)]).is_empty());
    let out = detect_and_take(&mut dt, 5, &[gain(2)]);
    assert_eq!(out.len(), 1);
    assert_eq!(amounts(&out), vec![vint(2)]);
}

/// REQ-EVENT-002; CR 603.2h, 603.7b: the per-turn limit is still enforced within a call and
/// across calls of one turn, and a new turn resets it.
#[test]
fn req_event_002_delayed_limit_per_turn_enforced_and_reset() {
    let mut dt = DelayedTriggers::new();
    dt.register(
        oid(9),
        P2,
        limited_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    let out = detect_and_take(&mut dt, 5, &[gain(2), gain(2)]);
    assert_eq!(out.len(), 1);
    assert!(detect_and_take(&mut dt, 5, &[gain(2)]).is_empty());
    assert_eq!(detect_and_take(&mut dt, 6, &[gain(2)]).len(), 1);
    assert!(detect_and_take(&mut dt, 6, &[gain(2)]).is_empty());
}

/// REQ-EVENT-002; CR 603.2h, 603.7b: the per-game limit is never reset by a new turn.
#[test]
fn req_event_002_delayed_limit_per_game_never_reset() {
    let mut dt = DelayedTriggers::new();
    dt.register(
        oid(9),
        P2,
        limited_ability(None, Some(1)),
        BTreeMap::new(),
        false,
    );
    assert_eq!(detect_and_take(&mut dt, 5, &[gain(2)]).len(), 1);
    assert!(detect_and_take(&mut dt, 6, &[gain(2)]).is_empty());
    assert!(detect_and_take(&mut dt, 7, &[gain(2)]).is_empty());
}

/// REQ-EVENT-002; CR 603.2h, 603.4: `detect` leaves the Memory untouched for a delayed ability
/// with a limit: no `trigger.limit.*` entry is written.
#[test]
fn req_event_002_delayed_detect_writes_no_limit_memory() {
    let mut dt = DelayedTriggers::new();
    dt.register(
        oid(9),
        P2,
        limited_ability(Some(1), Some(3)),
        BTreeMap::new(),
        false,
    );
    let env = LimitEnv::of(&dt);
    let mut memory = MemoryTable::new();
    let before = memory.clone();
    let waiting = detect(&env, &mut memory, 5, &[gain(1), gain(2), gain(2)]);
    assert_eq!(waiting.len(), 3);
    assert_eq!(memory, before);
    assert!(memory.get(oid(9)).is_none());
}

/// REQ-EVENT-002; CR 603.2h, 603.4: `registrations()` lists the ability without its intervening
/// "if" and without its limit (both are applied by `take_triggered`).
#[test]
fn req_event_002_delayed_registrations_drop_if_and_limit() {
    let mut dt = DelayedTriggers::new();
    let a = limited_ability(Some(1), Some(2));
    assert!(a.intervening_if.is_some() && a.limit.is_some());
    dt.register(oid(9), P2, a, BTreeMap::new(), false);
    let regs = dt.registrations();
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].ability.intervening_if, None);
    assert_eq!(regs[0].ability.limit, None);
    assert_eq!(regs[0].ability.base.ability_id, s("delayed#0"));
}

/// REQ-EVENT-002; CR 603.4, 603.7b: a one-shot registration with a limit whose only element
/// fails its condition stays registered and fires on a later passing element.
#[test]
fn req_event_002_delayed_one_shot_with_limit_survives_failing_element() {
    let mut dt = DelayedTriggers::new();
    dt.register(
        oid(9),
        P2,
        limited_ability(Some(1), None),
        BTreeMap::new(),
        true,
    );
    assert!(detect_and_take(&mut dt, 5, &[gain(1)]).is_empty());
    assert_eq!(dt.len(), 1);
    let out = detect_and_take(&mut dt, 5, &[gain(2)]);
    assert_eq!(amounts(&out), vec![vint(2)]);
    assert!(dt.is_empty());
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

// =============================================================================================
// `fire` and the activation limit at a phase or step (CR 603.2h, 603.7b)
// =============================================================================================

/// A delayed ability (no intervening "if") with the given limit.
fn fire_ability(per_turn: Option<u32>, per_game: Option<u32>) -> TriggeredAbilityAST {
    let mut a = delayed_ability();
    a.limit = Some(ActivationLimit { per_turn, per_game });
    a
}

/// REQ-EVENT-002; CR 603.2h, 603.7b: per-game limit 1 on a registration with a stated duration:
/// the first `fire` succeeds; later fires, in the same or a later turn, do not; the registration
/// stays.
#[test]
fn req_event_002_fire_per_game_limit() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        fire_ability(None, Some(1)),
        BTreeMap::new(),
        false,
    );
    assert!(dt.fire(&FireEnv::new(true), 1, id).is_some());
    assert_eq!(dt.fire(&FireEnv::new(true), 1, id), None);
    assert_eq!(dt.fire(&FireEnv::new(true), 2, id), None);
    assert_eq!(dt.len(), 1);
}

/// REQ-EVENT-002; CR 603.2h: per-turn limit 1: the count resets in a new turn.
#[test]
fn req_event_002_fire_per_turn_limit_resets() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        fire_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    assert!(dt.fire(&FireEnv::new(true), 5, id).is_some());
    assert_eq!(dt.fire(&FireEnv::new(true), 5, id), None);
    assert!(dt.fire(&FireEnv::new(true), 6, id).is_some());
}

/// REQ-EVENT-002; CR 603.2h: per-turn limit 2: two fires then none in one turn; again in the next.
#[test]
fn req_event_002_fire_per_turn_limit_two() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        fire_ability(Some(2), None),
        BTreeMap::new(),
        false,
    );
    assert!(dt.fire(&FireEnv::new(true), 3, id).is_some());
    assert!(dt.fire(&FireEnv::new(true), 3, id).is_some());
    assert_eq!(dt.fire(&FireEnv::new(true), 3, id), None);
    assert!(dt.fire(&FireEnv::new(true), 4, id).is_some());
    assert!(dt.fire(&FireEnv::new(true), 4, id).is_some());
    assert_eq!(dt.fire(&FireEnv::new(true), 4, id), None);
}

/// REQ-EVENT-002; CR 603.2h: the counters are shared: a triggering counted through
/// `take_triggered` makes `fire` refuse.
#[test]
fn req_event_002_fire_limit_shared_take_then_fire() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        limited_ability(None, Some(1)),
        BTreeMap::new(),
        false,
    );
    let out = detect_and_take(&mut dt, 5, &[gain(2)]);
    assert_eq!(out.len(), 1);
    assert_eq!(dt.fire(&FireEnv::new(true), 5, id), None);
    assert_eq!(dt.fire(&FireEnv::new(true), 6, id), None);
}

/// REQ-EVENT-002; CR 603.2h: the reverse: a triggering counted by `fire` makes `take_triggered`
/// yield nothing for a matching event.
#[test]
fn req_event_002_fire_limit_shared_fire_then_take() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        limited_ability(None, Some(1)),
        BTreeMap::new(),
        false,
    );
    assert!(dt.fire(&FireEnv::new(true), 5, id).is_some());
    assert!(detect_and_take(&mut dt, 5, &[gain(2)]).is_empty());
    assert!(detect_and_take(&mut dt, 6, &[gain(2)]).is_empty());
}

/// REQ-EVENT-002; CR 603.2h, 603.7b: a one-shot whose limit refuses (`per_game` 0) returns None
/// and is not consumed.
#[test]
fn req_event_002_fire_one_shot_refused_by_limit_is_kept() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        fire_ability(None, Some(0)),
        BTreeMap::new(),
        true,
    );
    assert_eq!(dt.fire(&FireEnv::new(true), 1, id), None);
    assert_eq!(dt.len(), 1);
    assert_eq!(dt.fire(&FireEnv::new(true), 2, id), None);
    assert_eq!(dt.len(), 1);
}

/// REQ-EVENT-002; CR 603.7b: a one-shot without a limit fires once and is removed.
#[test]
fn req_event_002_fire_one_shot_without_limit_removed() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(oid(9), P2, delayed_ability(), BTreeMap::new(), true);
    assert!(dt.fire(&FireEnv::new(true), 1, id).is_some());
    assert!(dt.is_empty());
    assert_eq!(dt.fire(&FireEnv::new(true), 1, id), None);
}

/// REQ-EVENT-002; CR 603.7b: a one-shot with room under its limit is removed once it fires.
#[test]
fn req_event_002_fire_one_shot_with_limit_removed_on_success() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        fire_ability(Some(1), None),
        BTreeMap::new(),
        true,
    );
    assert!(dt.fire(&FireEnv::new(true), 1, id).is_some());
    assert!(dt.is_empty());
}

/// REQ-EVENT-002; CR 603.2h: an unknown or cancelled id gives None.
#[test]
fn req_event_002_fire_unknown_or_cancelled_id() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        fire_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    assert!(dt.cancel(id));
    assert_eq!(dt.fire(&FireEnv::new(true), 1, id), None);
    assert_eq!(
        DelayedTriggers::new().fire(&FireEnv::new(true), 1, id),
        None
    );
}

// =============================================================================================
// `fire` evaluates the intervening "if" before counting the limit (CR 603.4, 603.2h, 603.7b)
// =============================================================================================

/// One recorded `condition_holds` call.
type FireCall = (StateView, TriggerRegistration, BTreeMap<String, EventField>);

/// An environment whose `condition_holds` answer can be switched and whose calls are recorded.
struct FireEnv {
    holds: Cell<bool>,
    calls: RefCell<Vec<FireCall>>,
}

impl FireEnv {
    fn new(holds: bool) -> FireEnv {
        FireEnv {
            holds: Cell::new(holds),
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl TriggerEnv for FireEnv {
    fn registrations(&self, _view: StateView) -> Vec<TriggerRegistration> {
        Vec::new()
    }

    fn object_matches(
        &self,
        _view: StateView,
        _reg: &TriggerRegistration,
        _object: ObjectId,
        _selector: &ObjectSelectorAST,
    ) -> bool {
        true
    }

    fn player_matches(
        &self,
        _view: StateView,
        _reg: &TriggerRegistration,
        _player: PlayerId,
        _group: &PlayerGroup,
    ) -> bool {
        true
    }

    fn predicate_holds(
        &self,
        _view: StateView,
        _reg: &TriggerRegistration,
        _pred: &PredicateAST,
        _field: Option<&EventField>,
    ) -> bool {
        false
    }

    fn condition_holds(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        _pred: &PredicateAST,
        bindings: &BTreeMap<String, EventField>,
    ) -> bool {
        self.calls
            .borrow_mut()
            .push((view, reg.clone(), bindings.clone()));
        self.holds.get()
    }

    fn is_visible_to_all(&self, _object: ObjectId) -> bool {
        true
    }
}

/// An ability with an intervening "if" and the given limit.
fn if_limit_ability(per_turn: Option<u32>, per_game: Option<u32>) -> TriggeredAbilityAST {
    let mut a = if_ability();
    a.limit = Some(ActivationLimit { per_turn, per_game });
    a
}

/// REQ-EVENT-002; CR 603.4, 603.2h, 603.7b: per-game limit 1, duration registration: false
/// occurrences do not use up the limit, so the first true one fires and the next true one does not.
#[test]
fn req_event_002_fire_if_false_before_true_per_game() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        if_limit_ability(None, Some(1)),
        BTreeMap::new(),
        false,
    );
    let no = FireEnv::new(false);
    let yes = FireEnv::new(true);
    assert_eq!(dt.fire(&no, 1, id), None);
    assert_eq!(dt.fire(&no, 1, id), None);
    assert_eq!(dt.len(), 1);
    assert!(dt.fire(&yes, 1, id).is_some());
    assert_eq!(dt.fire(&yes, 1, id), None);
    assert_eq!(dt.fire(&yes, 2, id), None);
    assert_eq!(dt.len(), 1);
}

/// REQ-EVENT-002; CR 603.4, 603.2h: per-turn limit 1: a false occurrence in turn 5 counts nothing;
/// one true occurrence fires, the next in the same turn does not, and turn 6 fires again.
#[test]
fn req_event_002_fire_if_false_before_true_per_turn() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        if_limit_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    let no = FireEnv::new(false);
    let yes = FireEnv::new(true);
    assert_eq!(dt.fire(&no, 5, id), None);
    assert!(dt.fire(&yes, 5, id).is_some());
    assert_eq!(dt.fire(&yes, 5, id), None);
    assert!(dt.fire(&yes, 6, id).is_some());
}

/// REQ-EVENT-002; CR 603.4, 603.7b: a one-shot whose condition is false returns None and stays
/// registered; a later true occurrence fires it and removes it.
#[test]
fn req_event_002_fire_if_false_one_shot_kept_until_true() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(oid(9), P2, if_ability(), BTreeMap::new(), true);
    assert_eq!(dt.fire(&FireEnv::new(false), 1, id), None);
    assert_eq!(dt.len(), 1);
    assert!(dt.fire(&FireEnv::new(true), 1, id).is_some());
    assert_eq!(dt.len(), 0);
    assert!(dt.is_empty());
}

/// REQ-EVENT-002; CR 603.4, 603.2h, 603.7b: a one-shot with a limit and a false condition is kept
/// and counts nothing, so it still fires once the condition holds.
#[test]
fn req_event_002_fire_if_false_one_shot_with_limit_not_counted() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        if_limit_ability(None, Some(1)),
        BTreeMap::new(),
        true,
    );
    assert_eq!(dt.fire(&FireEnv::new(false), 1, id), None);
    assert_eq!(dt.len(), 1);
    assert!(dt.fire(&FireEnv::new(true), 1, id).is_some());
    assert!(dt.is_empty());
}

/// REQ-EVENT-002; CR 603.4, 603.7c: `fire` asks the post-event view, with exactly the captured
/// bindings and the registration of the delayed trigger (`delayed#<n>`, the "if" kept, no limit).
#[test]
fn req_event_002_fire_if_condition_inputs() {
    let mut dt = DelayedTriggers::new();
    let _first = dt.register(oid(4), P1, delayed_ability(), BTreeMap::new(), false);
    let captured = binds(&[
        ("x", EventField::Object(oid(5))),
        ("n", vint(3)),
        ("p", EventField::Player(P1)),
    ]);
    let id = dt.register(
        oid(9),
        P2,
        if_limit_ability(Some(1), Some(1)),
        captured.clone(),
        false,
    );
    assert_eq!(id.get(), 1);
    let env = FireEnv::new(true);
    let p = dt.fire(&env, 7, id).expect("fires");
    assert_eq!(p.bindings, captured);
    assert_eq!(p.ability_id, s("delayed#1"));
    assert_eq!(p.controller, P2);
    assert_eq!(p.source, Some(oid(9)));
    let calls = env.calls.borrow();
    assert_eq!(calls.len(), 1);
    let (view, reg, bindings) = &calls[0];
    assert_eq!(*view, StateView::PostEvent);
    assert_eq!(*bindings, captured);
    assert_eq!(reg.ability.base.ability_id, s("delayed#1"));
    assert_eq!(reg.source, oid(9));
    assert_eq!(reg.controller, P2);
    assert!(reg.ability.intervening_if.is_some());
    assert_eq!(reg.ability.limit, None);
}

/// REQ-EVENT-002; CR 603.4: a registration without an intervening "if" never consults the
/// condition from `fire`; the per-turn limit still applies and resets with the turn.
#[test]
fn req_event_002_fire_no_if_never_consults_condition_per_turn() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        fire_ability(Some(2), None),
        BTreeMap::new(),
        false,
    );
    let env = FireEnv::new(false);
    assert!(dt.fire(&env, 3, id).is_some());
    assert!(dt.fire(&env, 3, id).is_some());
    assert_eq!(dt.fire(&env, 3, id), None);
    assert!(dt.fire(&env, 4, id).is_some());
    assert!(env.calls.borrow().is_empty());
}

/// REQ-EVENT-002; CR 603.4, 603.2h, 603.7b: without an "if", a one-shot refused by `per_game: 0`
/// returns None, stays registered, and the condition is never consulted.
#[test]
fn req_event_002_fire_no_if_one_shot_per_game_zero_kept() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        fire_ability(None, Some(0)),
        BTreeMap::new(),
        true,
    );
    let env = FireEnv::new(true);
    assert_eq!(dt.fire(&env, 1, id), None);
    assert_eq!(dt.len(), 1);
    assert!(env.calls.borrow().is_empty());
}

/// REQ-EVENT-002; CR 603.2h, 603.4: with a true condition and per-turn 1, a triggering counted by
/// `fire` stops `take_triggered` in the same turn and not in the next.
#[test]
fn req_event_002_fire_if_shared_fire_then_take() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        if_limit_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    let env = FireEnv::new(true);
    let waiting = vec![wt(9, P2, "delayed#0", BTreeMap::new())];
    assert!(dt.fire(&env, 5, id).is_some());
    assert!(dt.take_triggered(&env, 5, &waiting).is_empty());
    assert_eq!(dt.take_triggered(&env, 6, &waiting).len(), 1);
}

/// REQ-EVENT-002; CR 603.2h, 603.4: the reverse: a triggering counted by `take_triggered` stops
/// `fire` in the same turn and not in the next.
#[test]
fn req_event_002_fire_if_shared_take_then_fire() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        if_limit_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    let env = FireEnv::new(true);
    let waiting = vec![wt(9, P2, "delayed#0", BTreeMap::new())];
    assert_eq!(dt.take_triggered(&env, 5, &waiting).len(), 1);
    assert_eq!(dt.fire(&env, 5, id), None);
    assert!(dt.fire(&env, 6, id).is_some());
}

/// REQ-EVENT-002; CR 603.7b: an unknown or cancelled id gives None without consulting the
/// condition.
#[test]
fn req_event_002_fire_if_unknown_or_cancelled_never_consults_env() {
    let mut dt = DelayedTriggers::new();
    let id = dt.register(
        oid(9),
        P2,
        if_limit_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    assert!(dt.cancel(id));
    let env = FireEnv::new(true);
    assert_eq!(dt.fire(&env, 1, id), None);
    assert_eq!(DelayedTriggers::new().fire(&env, 1, id), None);
    assert!(env.calls.borrow().is_empty());
}
