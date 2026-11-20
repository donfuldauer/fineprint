// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of triggers on the stack: REQ-EVENT-002, REQ-ARCH-002 (state triggers CR 603.8,
// 603.4, 113.2c; delayed triggers CR 603.7, 603.7b; reflexive triggers CR 603.12, 603.12a;
// sourceless triggers CR 113.8, 726.2, 727.2, 729.1, 702.179d; placement CR 603.3b-d, 700.2b-c,
// 101.4; optional triggers CR 603.5). Synthetic object and player ids and neutral ability ids
// only.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_dsl::ability::{
    AbilityBase, ActivationLimit, EventKind, EventMatcher, FilterValue, ModeAST, ModeSetAST,
    TargetSlotAST, Trigger, TriggeredAbilityAST,
};
use fineprint_dsl::expr::{ObjectSelectorAST, PlayerGroup, PredicateAST, ReferenceAST};

use super::*;
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceResponse,
    ChoiceSession, ChoiceTarget, ChoiceType, PendingChoiceRequest, Status, ValidatorContract,
    ValidatorRule,
};
use crate::memory::MemoryTable;
use crate::txn::Transactional;

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn obj(n: u64) -> ChoiceTarget {
    ChoiceTarget::Object(o(n))
}

fn ability(id: &str, trigger: Trigger) -> TriggeredAbilityAST {
    TriggeredAbilityAST {
        base: AbilityBase::new(id),
        trigger,
        intervening_if: None,
        optional: None,
        limit: None,
        targets: None,
        modes: None,
        effect: Vec::new(),
    }
}

fn event_ability(id: &str, kind: EventKind) -> TriggeredAbilityAST {
    ability(id, Trigger::Event(EventMatcher::new(kind)))
}

/// The state condition every state trigger in these tests uses.
fn state_pred() -> PredicateAST {
    PredicateAST::IsCreature
}

/// The intervening-if condition these tests use (distinct from the state condition).
fn if_pred() -> PredicateAST {
    PredicateAST::IsPermanent
}

fn tslot(slot: u32, min: u32, max: u32) -> TargetSlotAST {
    TargetSlotAST {
        slot,
        selector: ObjectSelectorAST::Ref(ReferenceAST::SelfRef),
        min,
        max,
    }
}

fn mode(id: &str, targets: Option<Vec<TargetSlotAST>>) -> ModeAST {
    ModeAST {
        id: id.to_owned(),
        targets,
        effect: Vec::new(),
    }
}

/// A plain part-1 pending trigger.
fn pt(source: u64, controller: u64, id: &str) -> PendingTrigger {
    PendingTrigger {
        origin: TriggerOrigin::Event,
        controller: p(controller),
        source: Some(o(source)),
        ability_id: id.to_owned(),
        bindings: BTreeMap::new(),
        optional: false,
        responds_to_trigger: false,
        targets: Vec::new(),
        modes: None,
    }
}

/// A part-2 pending trigger (its condition is another ability triggering).
fn pt2(source: u64, controller: u64, id: &str) -> PendingTrigger {
    PendingTrigger {
        responds_to_trigger: true,
        ..pt(source, controller, id)
    }
}

fn ids(placement: &TriggerPlacement) -> Vec<String> {
    placement
        .stacked
        .iter()
        .map(|s| s.trigger.ability_id.clone())
        .collect()
}

fn removed_ids(placement: &TriggerPlacement) -> Vec<String> {
    placement
        .removed
        .iter()
        .map(|r| r.trigger.ability_id.clone())
        .collect()
}

// ---- trigger env fake (state and delayed triggers)

/// Registrations listed for both views; a state condition (`IS_CREATURE`) holds for instances
/// in `state_true`; an intervening-if (`IS_PERMANENT`) holds unless the instance is in
/// `if_false`. Any object or player matches; predicates hold.
#[derive(Default)]
struct Env {
    regs: Vec<TriggerRegistration>,
    state_true: BTreeSet<(ObjectId, String)>,
    if_false: BTreeSet<(ObjectId, String)>,
    calls: RefCell<Vec<(StateView, PredicateAST)>>,
}

impl Env {
    fn state_reg(&mut self, source: u64, controller: u64, id: &str) {
        self.regs.push(TriggerRegistration {
            source: o(source),
            controller: p(controller),
            ability: ability(id, Trigger::State(state_pred())),
        });
    }

    fn holds(&mut self, source: u64, id: &str, value: bool) {
        let key = (o(source), id.to_owned());
        if value {
            self.state_true.insert(key);
        } else {
            self.state_true.remove(&key);
        }
    }
}

impl TriggerEnv for Env {
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
        true
    }

    fn condition_holds(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        pred: &PredicateAST,
        _bindings: &BTreeMap<String, EventField>,
    ) -> bool {
        self.calls.borrow_mut().push((view, pred.clone()));
        let key = (reg.source, reg.ability.base.ability_id.clone());
        if *pred == if_pred() {
            !self.if_false.contains(&key)
        } else {
            self.state_true.contains(&key)
        }
    }

    fn is_visible_to_all(&self, _object: ObjectId) -> bool {
        true
    }
}

// ---- placement env fake and the session driver

/// Legal targets per (ability id, mode, slot); absent keys have none. Records the length of
/// `stacked_so_far` per call.
#[derive(Default)]
struct Places {
    legal: BTreeMap<(String, Option<usize>, usize), Vec<ChoiceTarget>>,
    seen_stacked: RefCell<Vec<usize>>,
}

impl Places {
    fn with(mut self, id: &str, mode: Option<usize>, slot: usize, legal: &[u64]) -> Places {
        self.legal.insert(
            (id.to_owned(), mode, slot),
            legal.iter().map(|&n| obj(n)).collect(),
        );
        self
    }
}

impl PlacementEnv for Places {
    fn legal_targets(
        &self,
        trigger: &PendingTrigger,
        mode: Option<usize>,
        slot: usize,
        stacked_so_far: &[StackedTrigger],
    ) -> Vec<ChoiceTarget> {
        self.seen_stacked.borrow_mut().push(stacked_so_far.len());
        self.legal
            .get(&(trigger.ability_id.clone(), mode, slot))
            .cloned()
            .unwrap_or_default()
    }
}

/// A state with nothing in it (placement does not touch the state).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Nil;

impl Transactional for Nil {
    fn state_hash(&self) -> Digest {
        sha256(b"")
    }

    fn priority_player(&self) -> Option<PlayerId> {
        None
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

/// Runs `place_triggers` to completion, answering each request with `answer`. Returns the
/// placement and every request asked, in order.
fn drive<E: PlacementEnv>(
    env: &E,
    apnap: &[PlayerId],
    waiting: &[PendingTrigger],
    mut answer: impl FnMut(&PendingChoiceRequest) -> ChoiceResponse,
) -> (TriggerPlacement, Vec<PendingChoiceRequest>) {
    let proc = |_s: &mut Nil, ctx: &mut ChoiceContext<'_>| {
        place_triggers(env, ctx, apnap, waiting.to_vec())
    };
    let mut session = ChoiceSession::start(Nil, proc).expect("start");
    let mut asked = Vec::new();
    loop {
        let r = match session.status() {
            Status::Done { output, .. } => return (output.clone(), asked),
            Status::Pending(r) => r.clone(),
        };
        let a = answer(&r);
        asked.push(r.clone());
        session.submit(r.request_id, a).expect("answer accepted");
    }
}

/// Answers every request with its default.
fn defaults(r: &PendingChoiceRequest) -> ChoiceResponse {
    r.default_on_timeout.clone()
}

fn order(v: &[u32]) -> ChoiceResponse {
    ChoiceResponse::OrderTriggers { order: v.to_vec() }
}

/// Every permutation of `0..n`.
fn permutations(n: u32) -> Vec<Vec<u32>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for rest in permutations(n - 1) {
        for pos in 0..=rest.len() {
            let mut v = rest.clone();
            v.insert(pos, n - 1);
            out.push(v);
        }
    }
    out
}

fn order_payload(r: &PendingChoiceRequest) -> Vec<String> {
    match &r.payload {
        ChoicePayload::OrderSimultaneousTriggers { triggers } => triggers.clone(),
        other => panic!("not an order payload: {other:?}"),
    }
}

fn idx(i: u32) -> usize {
    usize::try_from(i).expect("small index")
}

// ---------------------------------------------------------------------------------------------
// PendingTrigger
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-002, CR 603.3b: from_waiting copies the ability's option, targets and modes, and
/// derives the CR 603.3b part from a TRIGGERED event matcher; description formats.
#[test]
fn req_event_002_pending_from_waiting_and_description() {
    let mut bindings = BTreeMap::new();
    bindings.insert("x".to_owned(), EventField::Player(p(2)));
    let w = WaitingTrigger {
        source: o(7),
        controller: p(1),
        ability_id: "t.inst".to_owned(),
        look_back: None,
        event_indices: vec![0],
        bindings: bindings.clone(),
    };
    let mut a = event_ability("t.def", EventKind::Triggered);
    a.optional = Some(true);
    a.targets = Some(vec![tslot(0, 1, 1)]);
    a.modes = Some(ModeSetAST {
        min: 1,
        max: 1,
        modes: vec![mode("m0", None)],
    });
    let pend = PendingTrigger::from_waiting(w.clone(), &a, TriggerOrigin::Event);
    assert_eq!(pend.ability_id, "t.inst");
    assert_eq!(pend.source, Some(o(7)));
    assert_eq!(pend.controller, p(1));
    assert_eq!(pend.bindings, bindings);
    assert!(pend.optional);
    assert!(pend.responds_to_trigger);
    assert_eq!(pend.targets, vec![tslot(0, 1, 1)]);
    assert_eq!(pend.modes, a.modes);
    assert_eq!(pend.description(), "object 7 ability t.inst");

    let plain = PendingTrigger::from_waiting(
        w.clone(),
        &event_ability("t.def", EventKind::Attacks),
        TriggerOrigin::State,
    );
    assert!(!plain.optional && !plain.responds_to_trigger);
    assert!(plain.targets.is_empty() && plain.modes.is_none());
    assert_eq!(plain.origin, TriggerOrigin::State);
    let mut st = ability("t.state", Trigger::State(state_pred()));
    st.optional = Some(false);
    let from_state = PendingTrigger::from_waiting(w, &st, TriggerOrigin::State);
    assert!(!from_state.optional && !from_state.responds_to_trigger);

    let sourceless = PendingTrigger {
        source: None,
        ..pt(0, 3, "sourceless.speed")
    };
    assert_eq!(
        sourceless.description(),
        "player 3 ability sourceless.speed"
    );
}

// ---------------------------------------------------------------------------------------------
// State triggers
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-002, CR 603.8: a state trigger does not retrigger while outstanding (waiting or on
/// the stack) and re-arms after it left the stack only if the condition still holds.
#[test]
fn req_event_002_state_trigger_not_retriggered_while_on_stack() {
    let mut env = Env::default();
    env.state_reg(5, 1, "s");
    env.holds(5, "s", true);
    let mut st = StateTriggers::new();
    let first = st.check(&env);
    assert_eq!(
        first,
        vec![WaitingTrigger {
            source: o(5),
            controller: p(1),
            ability_id: "s".into(),
            look_back: None,
            event_indices: vec![],
            bindings: BTreeMap::new(),
        }]
    );
    assert!(st.is_outstanding(o(5), "s"));
    // Still true, still outstanding: nothing new (CR 603.8).
    assert!(st.check(&env).is_empty());
    assert!(st.check(&env).is_empty());
    // It leaves the stack; the condition still holds: it triggers again.
    assert!(st.instance_left_stack(o(5), "s"));
    assert!(!st.instance_left_stack(o(5), "s"));
    assert_eq!(st.check(&env).len(), 1);
    // Leaves again with the condition false: no trigger, not outstanding.
    assert!(st.instance_left_stack(o(5), "s"));
    env.holds(5, "s", false);
    assert!(st.check(&env).is_empty());
    assert!(!st.is_outstanding(o(5), "s"));
    // The condition becomes true later: it triggers as soon as it matches.
    env.holds(5, "s", true);
    assert_eq!(st.check(&env).len(), 1);
    // Every check reads the post-event view.
    assert!(
        env.calls
            .borrow()
            .iter()
            .all(|(v, _)| *v == StateView::PostEvent)
    );
}

/// REQ-EVENT-002, CR 603.8: an object no longer listed (not in the same zone) does not re-arm;
/// forget drops the entry.
#[test]
fn req_event_002_state_trigger_rearm_requires_listed_object_and_forget() {
    let mut env = Env::default();
    env.state_reg(5, 1, "s");
    env.holds(5, "s", true);
    let mut st = StateTriggers::new();
    assert_eq!(st.check(&env).len(), 1);
    assert!(st.instance_left_stack(o(5), "s"));
    env.regs.clear();
    assert!(st.check(&env).is_empty());
    assert!(!st.is_outstanding(o(5), "s"));

    env.state_reg(5, 1, "s");
    assert_eq!(st.check(&env).len(), 1);
    assert!(st.forget(o(5), "s"));
    assert!(!st.forget(o(5), "s"));
    assert!(!st.is_outstanding(o(5), "s"));
}

/// REQ-EVENT-002, CR 113.2c, CR 603.8: separate instances are independent; output in (source,
/// ability id) order; event triggers are ignored.
#[test]
fn req_event_002_state_trigger_independent_instances() {
    let mut env = Env::default();
    env.state_reg(9, 2, "b");
    env.state_reg(3, 1, "z");
    env.state_reg(3, 1, "a");
    env.regs.push(TriggerRegistration {
        source: o(1),
        controller: p(1),
        ability: event_ability("e", EventKind::Attacks),
    });
    for (s, id) in [(9, "b"), (3, "z"), (3, "a"), (1, "e")] {
        env.holds(s, id, true);
    }
    let mut st = StateTriggers::new();
    let got: Vec<(ObjectId, String)> = st
        .check(&env)
        .into_iter()
        .map(|w| (w.source, w.ability_id))
        .collect();
    assert_eq!(
        got,
        vec![
            (o(3), "a".to_owned()),
            (o(3), "z".to_owned()),
            (o(9), "b".to_owned())
        ]
    );
    assert!(st.instance_left_stack(o(3), "z"));
    let again = st.check(&env);
    assert_eq!(again.len(), 1);
    assert_eq!((again[0].source, again[0].ability_id.as_str()), (o(3), "z"));
    assert!(st.is_outstanding(o(3), "a") && st.is_outstanding(o(9), "b"));
    assert!(!st.is_outstanding(o(1), "e"));
}

/// REQ-EVENT-002, CR 603.4, CR 603.8: a false intervening "if" means no trigger and the
/// instance is not outstanding; a true one triggers.
#[test]
fn req_event_002_state_trigger_intervening_if() {
    let mut env = Env::default();
    env.regs.push(TriggerRegistration {
        source: o(4),
        controller: p(1),
        ability: TriggeredAbilityAST {
            intervening_if: Some(if_pred()),
            ..ability("s", Trigger::State(state_pred()))
        },
    });
    env.holds(4, "s", true);
    env.if_false.insert((o(4), "s".to_owned()));
    let mut st = StateTriggers::new();
    assert!(st.check(&env).is_empty());
    assert!(!st.is_outstanding(o(4), "s"));
    env.if_false.clear();
    assert_eq!(st.check(&env).len(), 1);
    assert!(st.is_outstanding(o(4), "s"));
    assert!(env.calls.borrow().iter().any(|(_, pr)| *pr == if_pred()));
}

// ---------------------------------------------------------------------------------------------
// Delayed and reflexive triggers
// ---------------------------------------------------------------------------------------------

fn waiting(source: u64, controller: u64, id: &str, binds: &[(&str, u64)]) -> WaitingTrigger {
    WaitingTrigger {
        source: o(source),
        controller: p(controller),
        ability_id: id.to_owned(),
        look_back: None,
        event_indices: vec![0],
        bindings: binds
            .iter()
            .map(|(k, v)| ((*k).to_owned(), EventField::Object(o(*v))))
            .collect(),
    }
}

/// REQ-EVENT-002, CR 603.7, SRS 8.2 item 5: registrations carry distinct delayed#n ids in id
/// order; captured bindings win over event bindings; non-delayed entries are ignored.
#[test]
fn req_event_002_delayed_registrations_and_captured_bindings() {
    let mut d = DelayedTriggers::new();
    assert!(d.is_empty());
    let mut captured = BTreeMap::new();
    captured.insert("it".to_owned(), EventField::Object(o(40)));
    let mut a = event_ability("orig", EventKind::Attacks);
    a.targets = Some(vec![tslot(0, 1, 1)]);
    let id0 = d.register(o(10), p(2), a.clone(), captured, false);
    let id1 = d.register(o(10), p(2), a, BTreeMap::new(), true);
    assert_eq!((id0.get(), id1.get()), (0, 1));
    assert_eq!(d.len(), 2);
    let regs = d.registrations();
    assert_eq!(
        regs.iter()
            .map(|r| r.ability.base.ability_id.clone())
            .collect::<Vec<_>>(),
        vec!["delayed#0".to_owned(), "delayed#1".to_owned()]
    );
    assert!(
        regs.iter()
            .all(|r| r.source == o(10) && r.controller == p(2))
    );

    let got = d.take_triggered(
        &Env::default(),
        1,
        &[
            waiting(10, 2, "delayed#0", &[("it", 99), ("other", 5)]),
            waiting(10, 2, "plain", &[]),
            waiting(11, 2, "delayed#0", &[]),
            waiting(10, 2, "delayed#7", &[]),
            waiting(10, 2, "delayed#x", &[]),
            waiting(10, 2, "delayed#00", &[]),
        ],
    );
    assert_eq!(got.len(), 1);
    let g = &got[0];
    assert_eq!(g.origin, TriggerOrigin::Delayed);
    assert_eq!(g.ability_id, "delayed#0");
    assert_eq!(g.source, Some(o(10)));
    assert_eq!(g.controller, p(2));
    assert_eq!(g.bindings["it"], EventField::Object(o(40)));
    assert_eq!(g.bindings["other"], EventField::Object(o(5)));
    assert_eq!(g.targets, vec![tslot(0, 1, 1)]);
    assert_eq!(d.len(), 2, "a registration with a duration stays");
}

/// REQ-EVENT-002, CR 603.7b: a one-shot delayed trigger yields only one pending trigger from
/// several matching entries (the first) and is removed; one with a duration yields one per entry
/// and stays.
#[test]
fn req_event_002_delayed_one_shot_and_duration() {
    let mut d = DelayedTriggers::new();
    let a = event_ability("orig", EventKind::Attacks);
    let once = d.register(o(1), p(1), a.clone(), BTreeMap::new(), true);
    let lasting = d.register(o(2), p(1), a, BTreeMap::new(), false);
    let got = d.take_triggered(
        &Env::default(),
        1,
        &[
            waiting(1, 1, "delayed#0", &[("e", 1)]),
            waiting(2, 1, "delayed#1", &[("e", 2)]),
            waiting(1, 1, "delayed#0", &[("e", 3)]),
            waiting(2, 1, "delayed#1", &[("e", 4)]),
        ],
    );
    let summary: Vec<(String, EventField)> = got
        .iter()
        .map(|g| (g.ability_id.clone(), g.bindings["e"].clone()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("delayed#0".to_owned(), EventField::Object(o(1))),
            ("delayed#1".to_owned(), EventField::Object(o(2))),
            ("delayed#1".to_owned(), EventField::Object(o(4))),
        ]
    );
    assert_eq!(d.len(), 1);
    assert!(!d.cancel(once));
    // A later event no longer triggers the one-shot.
    assert!(
        d.take_triggered(&Env::default(), 1, &[waiting(1, 1, "delayed#0", &[])])
            .is_empty()
    );
    assert!(d.cancel(lasting));
    assert!(d.is_empty());
    // Ids are never reused.
    let next = d.register(
        o(3),
        p(1),
        event_ability("x", EventKind::Attacks),
        BTreeMap::new(),
        true,
    );
    assert_eq!(next.get(), 2);
}

/// REQ-EVENT-002, CR 603.7b: fire (a phase/step delayed trigger) uses the captured bindings and
/// removes a one-shot; unknown ids give None.
#[test]
fn req_event_002_delayed_fire_and_cancel() {
    let mut d = DelayedTriggers::new();
    let mut captured = BTreeMap::new();
    captured.insert("it".to_owned(), EventField::Object(o(8)));
    let mut a = ability("orig", Trigger::State(state_pred()));
    a.optional = Some(true);
    let once = d.register(o(4), p(3), a.clone(), captured.clone(), true);
    let lasting = d.register(o(4), p(3), a, captured.clone(), false);
    let f = d.fire(&Env::default(), 1, once).expect("fires");
    assert_eq!(
        f,
        PendingTrigger {
            origin: TriggerOrigin::Delayed,
            controller: p(3),
            source: Some(o(4)),
            ability_id: "delayed#0".into(),
            bindings: captured,
            optional: true,
            responds_to_trigger: false,
            targets: vec![],
            modes: None,
        }
    );
    assert_eq!(d.fire(&Env::default(), 1, once), None);
    assert!(d.fire(&Env::default(), 1, lasting).is_some());
    assert!(d.fire(&Env::default(), 1, lasting).is_some());
    assert_eq!(d.len(), 1);
    assert!(d.cancel(lasting));
    assert!(!d.cancel(lasting));
    assert_eq!(d.fire(&Env::default(), 1, lasting), None);
}

/// REQ-EVENT-002, CR 603.7, CR 603.7a: end to end with detect(): the delayed registrations are
/// listed by the env, detect finds them and take_triggered turns them into pending triggers
/// with the captured binding winning.
#[test]
fn req_event_002_delayed_end_to_end_with_detect() {
    let mut d = DelayedTriggers::new();
    let mut m = EventMatcher::new(EventKind::Attacks);
    let mut bind = BTreeMap::new();
    bind.insert("amount".to_owned(), "n".to_owned());
    bind.insert("other".to_owned(), "it".to_owned());
    m.bind = Some(bind);
    let mut captured = BTreeMap::new();
    captured.insert("it".to_owned(), EventField::Object(o(77)));
    let id = d.register(
        o(6),
        p(2),
        ability("orig", Trigger::Event(m)),
        captured,
        true,
    );
    let env = Env {
        regs: d.registrations(),
        ..Env::default()
    };
    let events = vec![
        TriggerEvent::new(vec![EventKind::Attacks])
            .with_field("amount", EventField::Value(FilterValue::Int(3)))
            .with_field("other", EventField::Object(o(5))),
        TriggerEvent::new(vec![EventKind::Attacks])
            .with_field("amount", EventField::Value(FilterValue::Int(4))),
    ];
    let found = detect(&env, &mut MemoryTable::new(), 1, &events);
    assert_eq!(found.len(), 2);
    let got = d.take_triggered(&env, 1, &found);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].ability_id, format!("delayed#{}", id.get()));
    assert_eq!(got[0].bindings["n"], EventField::Value(FilterValue::Int(3)));
    assert_eq!(got[0].bindings["it"], EventField::Object(o(77)));
    assert!(d.is_empty());
}

/// REQ-EVENT-002, CR 603.12, CR 603.12a: reflexive triggers: one per occurrence, at most one
/// for a repeated payment, none for zero occurrences.
#[test]
fn req_event_002_reflexive_triggers() {
    let mut a = event_ability("r", EventKind::Attacks);
    a.targets = Some(vec![tslot(0, 1, 1)]);
    let mut b = BTreeMap::new();
    b.insert("x".to_owned(), EventField::Player(p(2)));
    assert!(reflexive_triggers(o(1), p(1), &a, b.clone(), 0, false).is_empty());
    assert!(reflexive_triggers(o(1), p(1), &a, b.clone(), 0, true).is_empty());
    let three = reflexive_triggers(o(1), p(1), &a, b.clone(), 3, false);
    assert_eq!(three.len(), 3);
    for t in &three {
        assert_eq!(t.origin, TriggerOrigin::Reflexive);
        assert_eq!(t.ability_id, "r");
        assert_eq!(t.bindings, b);
        assert_eq!(t.source, Some(o(1)));
        assert_eq!(t.controller, p(1));
        assert_eq!(t.targets, vec![tslot(0, 1, 1)]);
    }
    assert_eq!(reflexive_triggers(o(1), p(1), &a, b, 3, true).len(), 1);
}

// ---------------------------------------------------------------------------------------------
// Sourceless triggers
// ---------------------------------------------------------------------------------------------

const ALL_ABILITIES: [SourcelessAbility; 7] = [
    SourcelessAbility::MonarchEndStep,
    SourcelessAbility::MonarchCombatDamage,
    SourcelessAbility::InitiativeUpkeep,
    SourcelessAbility::InitiativeCombatDamage,
    SourcelessAbility::InitiativeTaken,
    SourcelessAbility::RadCounters,
    SourcelessAbility::Speed,
];

/// REQ-EVENT-002, CR 113.8, CR 726.2, CR 727.2, CR 729.1, CR 702.179d: kinds and ids of the
/// seven inherent abilities.
#[test]
fn req_event_002_sourceless_ability_kinds_and_ids() {
    let got: Vec<(SourcelessKind, &str)> = ALL_ABILITIES
        .iter()
        .map(|a| (a.kind(), a.ability_id()))
        .collect();
    assert_eq!(
        got,
        vec![
            (SourcelessKind::Monarch, "sourceless.monarch.end_step"),
            (SourcelessKind::Monarch, "sourceless.monarch.combat_damage"),
            (SourcelessKind::Initiative, "sourceless.initiative.upkeep"),
            (
                SourcelessKind::Initiative,
                "sourceless.initiative.combat_damage"
            ),
            (SourcelessKind::Initiative, "sourceless.initiative.taken"),
            (SourcelessKind::RadCounters, "sourceless.rad_counters"),
            (SourcelessKind::Speed, "sourceless.speed"),
        ]
    );
}

/// REQ-EVENT-002, CR 113.8 exception, CR 726.2, CR 727.2, CR 729.1, CR 702.179d: player-scoped
/// registrations; fire builds a sourceless trigger; RadCounters is controlled by the active
/// player, not the holder.
#[test]
fn req_event_002_sourceless_register_and_fire() {
    let mut s = SourcelessTriggers::new();
    assert!(s.register(p(2), SourcelessKind::Monarch));
    assert!(!s.register(p(2), SourcelessKind::Monarch));
    assert!(s.register(p(3), SourcelessKind::RadCounters));
    assert!(s.register(p(1), SourcelessKind::RadCounters));
    assert!(s.register(p(1), SourcelessKind::Speed));
    assert!(s.register(p(1), SourcelessKind::Initiative));
    assert_eq!(
        s.players_with(SourcelessKind::RadCounters),
        vec![p(1), p(3)]
    );
    assert_eq!(s.players_with(SourcelessKind::Monarch), vec![p(2)]);
    assert!(s.is_registered(p(2), SourcelessKind::Monarch));
    assert!(!s.is_registered(p(1), SourcelessKind::Monarch));

    for a in ALL_ABILITIES {
        let holder = match a.kind() {
            SourcelessKind::Monarch => p(2),
            SourcelessKind::RadCounters => p(3),
            _ => p(1),
        };
        // RadCounters: the active player (here 1) controls it while player 3 has the counters.
        let controller = if a == SourcelessAbility::RadCounters {
            p(1)
        } else {
            holder
        };
        let f = s.fire(holder, controller, a).expect("registered");
        let mut bindings = BTreeMap::new();
        bindings.insert("player".to_owned(), EventField::Player(holder));
        assert_eq!(
            f,
            PendingTrigger {
                origin: TriggerOrigin::Sourceless(a),
                controller,
                source: None,
                ability_id: a.ability_id().to_owned(),
                bindings,
                optional: false,
                responds_to_trigger: false,
                targets: vec![],
                modes: None,
            }
        );
    }
    assert_eq!(
        s.fire(p(1), p(1), SourcelessAbility::MonarchEndStep),
        None,
        "player 1 is not the monarch"
    );
    assert!(s.unregister(p(2), SourcelessKind::Monarch));
    assert!(!s.unregister(p(2), SourcelessKind::Monarch));
    assert_eq!(
        s.fire(p(2), p(2), SourcelessAbility::MonarchCombatDamage),
        None
    );
    assert!(s.players_with(SourcelessKind::Monarch).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Placement: APNAP and the two parts
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-002, REQ-ARCH-002, CR 603.3b, CR 101.4: two players' simultaneous triggers, every
/// ordering of each player's triggers, the active player not the lowest id: the active player's
/// triggers go on the stack first (deepest), each player's in their chosen order.
#[test]
fn req_event_002_apnap_permutations_two_players() {
    let env = Places::default();
    let apnap = [p(2), p(1)];
    let waiting = vec![
        pt(10, 1, "n0"),
        pt(20, 2, "a0"),
        pt(11, 1, "n1"),
        pt(21, 2, "a1"),
        pt(22, 2, "a2"),
    ];
    let a = ["a0", "a1", "a2"];
    let n = ["n0", "n1"];
    for perm_a in permutations(3) {
        for perm_n in permutations(2) {
            let (pl, asked) = drive(&env, &apnap, &waiting, |r| {
                if r.chooser == p(2) {
                    order(&perm_a)
                } else {
                    order(&perm_n)
                }
            });
            assert_eq!(
                asked.iter().map(|r| r.chooser).collect::<Vec<_>>(),
                vec![p(2), p(1)]
            );
            assert_eq!(
                order_payload(&asked[0]),
                vec![
                    "object 20 ability a0".to_owned(),
                    "object 21 ability a1".into(),
                    "object 22 ability a2".into()
                ]
            );
            let mut want: Vec<String> = perm_a.iter().map(|&i| a[idx(i)].to_owned()).collect();
            want.extend(perm_n.iter().map(|&i| n[idx(i)].to_owned()));
            assert_eq!(ids(&pl), want);
            assert!(pl.removed.is_empty());
        }
    }
}

/// REQ-EVENT-002, REQ-ARCH-002, CR 603.3b, CR 101.4: three players in APNAP order starting from
/// a middle id, every ordering per player.
#[test]
fn req_event_002_apnap_permutations_three_players() {
    let env = Places::default();
    let apnap = [p(3), p(1), p(2)];
    let waiting = vec![
        pt(1, 1, "p1a"),
        pt(2, 2, "p2a"),
        pt(3, 3, "p3a"),
        pt(4, 1, "p1b"),
        pt(5, 2, "p2b"),
        pt(6, 3, "p3b"),
    ];
    let pick = |pre: &str, perm: &[u32]| -> Vec<String> {
        perm.iter()
            .map(|&i| format!("{pre}{}", if i == 0 { "a" } else { "b" }))
            .collect()
    };
    for x in permutations(2) {
        for y in permutations(2) {
            for z in permutations(2) {
                let (pl, asked) = drive(&env, &apnap, &waiting, |r| match r.chooser.get() {
                    3 => order(&x),
                    1 => order(&y),
                    _ => order(&z),
                });
                assert_eq!(
                    asked.iter().map(|r| r.chooser).collect::<Vec<_>>(),
                    vec![p(3), p(1), p(2)]
                );
                let mut want = pick("p3", &x);
                want.extend(pick("p1", &y));
                want.extend(pick("p2", &z));
                assert_eq!(ids(&pl), want);
            }
        }
    }
}

/// REQ-EVENT-002, CR 603.3b: part 2 (triggers on another ability triggering) goes on after all
/// of part 1, ordering is asked per player per part, and a player with one trigger in each part
/// is asked nothing.
#[test]
fn req_event_002_two_part_placement() {
    let env = Places::default();
    // Player 1 is active; it has only a part-2 trigger; player 2 has a part-1 trigger.
    let (pl, asked) = drive(
        &env,
        &[p(1), p(2)],
        &[pt2(1, 1, "p1.part2"), pt(2, 2, "p2.part1")],
        defaults,
    );
    assert!(asked.is_empty());
    assert_eq!(ids(&pl), vec!["p2.part1", "p1.part2"]);

    // A player with 1 + 1 across the parts is asked nothing.
    let (pl, asked) = drive(
        &env,
        &[p(1)],
        &[pt2(1, 1, "two"), pt(2, 1, "one")],
        defaults,
    );
    assert!(asked.is_empty());
    assert_eq!(ids(&pl), vec!["one", "two"]);

    // 2 + 2 per player: four ORDER requests, part 1 for both players before part 2.
    let waiting = vec![
        pt2(1, 1, "1.B0"),
        pt(2, 1, "1.A0"),
        pt2(3, 2, "2.B0"),
        pt(4, 2, "2.A0"),
        pt2(5, 1, "1.B1"),
        pt(6, 1, "1.A1"),
        pt2(7, 2, "2.B1"),
        pt(8, 2, "2.A1"),
    ];
    let (pl, asked) = drive(&env, &[p(2), p(1)], &waiting, |_| order(&[1, 0]));
    let shape: Vec<(PlayerId, Vec<String>)> = asked
        .iter()
        .map(|r| (r.chooser, order_payload(r)))
        .collect();
    let d = |a: &str, b: &str| vec![a.to_owned(), b.to_owned()];
    assert_eq!(
        shape,
        vec![
            (p(2), d("object 4 ability 2.A0", "object 8 ability 2.A1")),
            (p(1), d("object 2 ability 1.A0", "object 6 ability 1.A1")),
            (p(2), d("object 3 ability 2.B0", "object 7 ability 2.B1")),
            (p(1), d("object 1 ability 1.B0", "object 5 ability 1.B1")),
        ]
    );
    assert_eq!(
        ids(&pl),
        vec![
            "2.A1", "2.A0", "1.A1", "1.A0", "2.B1", "2.B0", "1.B1", "1.B0"
        ]
    );
}

/// REQ-EVENT-002, CR 603.3b, CR 101.4: an unknown controller, or a player listed twice, is a
/// procedure error; an empty waiting list places nothing.
#[test]
fn req_event_002_placement_errors_and_empty() {
    let env = Places::default();
    let w = vec![pt(1, 9, "x")];
    let r = ChoiceSession::start(Nil, |_s: &mut Nil, ctx: &mut ChoiceContext<'_>| {
        place_triggers(&env, ctx, &[p(1), p(2)], w.clone())
    });
    assert!(matches!(r, Err(ChoiceError::Procedure(_))));
    let r = ChoiceSession::start(Nil, |_s: &mut Nil, ctx: &mut ChoiceContext<'_>| {
        place_triggers(&env, ctx, &[p(1), p(1)], Vec::new())
    });
    assert!(matches!(r, Err(ChoiceError::Procedure(_))));
    let (pl, asked) = drive(&env, &[p(1)], &[], defaults);
    assert_eq!(pl, TriggerPlacement::default());
    assert!(asked.is_empty());
}

// ---------------------------------------------------------------------------------------------
// Placement: request shapes and the protocol
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-002, REQ-ARCH-002, CR 603.3b: the ORDER_SIMULTANEOUS_TRIGGERS request shape; an
/// invalid ORDER_TRIGGERS answer is rejected by validate and by the session.
#[test]
fn req_event_002_order_request_shape_and_invalid_answer() {
    let env = Places::default();
    let apnap = [p(1)];
    let w = vec![pt(1, 1, "a"), pt(2, 1, "b"), pt(3, 1, "c")];
    let proc =
        |_s: &mut Nil, ctx: &mut ChoiceContext<'_>| place_triggers(&env, ctx, &apnap, w.clone());
    let mut s = ChoiceSession::start(Nil, proc).expect("start");
    let r = s.pending().expect("pending").clone();
    assert_eq!(r.request_id.get(), 0);
    assert_eq!(r.chooser, p(1));
    assert_eq!(r.choice_type, ChoiceType::OrderSimultaneousTriggers);
    assert_eq!(r.reason.cr, "CR 603.3b");
    assert_eq!(r.reason.source, None);
    assert_eq!((r.min, r.max), (3, 3));
    assert_eq!(r.default_on_timeout, order(&[0, 1, 2]));
    assert_eq!(r.validate(&r.default_on_timeout), Ok(()));
    assert_eq!(
        r.options,
        ChoiceOptions::Enumerated(vec![
            ChoiceOption::Number(0),
            ChoiceOption::Number(1),
            ChoiceOption::Number(2)
        ])
    );
    assert_eq!(
        order_payload(&r),
        vec![
            "object 1 ability a".to_owned(),
            "object 2 ability b".into(),
            "object 3 ability c".into()
        ]
    );
    for bad in [order(&[0, 1]), order(&[0, 0, 1]), order(&[0, 1, 3])] {
        assert!(r.validate(&bad).is_err());
        assert!(matches!(
            s.submit(r.request_id, bad),
            Err(ChoiceError::Invalid { .. })
        ));
    }
    assert!(s.submit(r.request_id, ChoiceResponse::Yes).is_err());
    s.submit(r.request_id, order(&[2, 0, 1])).expect("valid");
    match s.status() {
        Status::Done { output, .. } => assert_eq!(ids(output), vec!["c", "a", "b"]),
        Status::Pending(_) => panic!("done expected"),
    }
}

/// REQ-EVENT-002, CR 603.3b: with 8 or more triggers the ORDER request carries a validator
/// contract (8! is at least ENUMERATION_LIMIT) with the request bounds.
#[test]
fn req_event_002_order_request_validator_for_many_triggers() {
    let env = Places::default();
    let w: Vec<PendingTrigger> = (0..8).map(|i| pt(i, 1, &format!("t{i}"))).collect();
    let (pl, asked) = drive(&env, &[p(1)], &w, |_| order(&[7, 6, 5, 4, 3, 2, 1, 0]));
    assert_eq!(asked.len(), 1);
    match &asked[0].options {
        ChoiceOptions::Validator(c) => assert_eq!((c.min, c.max), (8, 8)),
        ChoiceOptions::Enumerated(_) => panic!("validator expected"),
    }
    assert_eq!(ids(&pl).first().map(String::as_str), Some("t7"));
}

/// REQ-EVENT-002, CR 603.3d: CHOOSE_TARGETS shape (chooser, reason, slots clamped, default
/// valid); targets recorded; `stacked_so_far` grows as triggers go on the stack.
#[test]
fn req_event_002_targets_request_shape() {
    let env = Places::default()
        .with("t", None, 0, &[30, 31, 32])
        .with("t", None, 1, &[40]);
    let mut a = pt(5, 2, "t");
    a.targets = vec![tslot(0, 1, 2), tslot(1, 0, 3)];
    let b = pt(6, 2, "u");
    let (pl, asked) = drive(&env, &[p(2)], &[b, a], |r| match r.choice_type {
        ChoiceType::ChooseTargets => ChoiceResponse::SelectTargets {
            targets: vec![vec![obj(32), obj(30)], vec![obj(40)]],
        },
        _ => order(&[0, 1]),
    });
    assert_eq!(asked.len(), 2);
    let r = &asked[1];
    assert_eq!(r.request_id.get(), 1);
    assert_eq!(r.chooser, p(2));
    assert_eq!(r.choice_type, ChoiceType::ChooseTargets);
    assert_eq!(r.reason.cr, "CR 603.3d");
    assert_eq!(r.reason.source, Some(o(5)));
    assert_eq!((r.min, r.max), (1, 3));
    match &r.payload {
        ChoicePayload::ChooseTargets {
            slots,
            may_keep_illegal,
        } => {
            assert!(!may_keep_illegal);
            assert_eq!(slots.len(), 2);
            assert_eq!((slots[0].min, slots[0].max), (1, 2));
            assert_eq!(slots[0].legal, vec![obj(30), obj(31), obj(32)]);
            assert!(slots[0].current.is_empty());
            assert_eq!((slots[1].min, slots[1].max), (0, 1));
        }
        other => panic!("unexpected payload {other:?}"),
    }
    assert_eq!(
        r.default_on_timeout,
        ChoiceResponse::SelectTargets {
            targets: vec![vec![obj(30)], vec![]]
        }
    );
    assert_eq!(r.validate(&r.default_on_timeout), Ok(()));
    assert_eq!(
        r.options,
        ChoiceOptions::Enumerated(vec![
            ChoiceOption::Object(o(30)),
            ChoiceOption::Object(o(31)),
            ChoiceOption::Object(o(32)),
            ChoiceOption::Object(o(40)),
        ])
    );
    assert_eq!(ids(&pl), vec!["u", "t"]);
    assert_eq!(
        pl.stacked[1].targets,
        vec![vec![obj(32), obj(30)], vec![obj(40)]]
    );
    assert!(pl.stacked[0].targets.is_empty() && pl.stacked[0].modes.is_empty());
    // "t" is second: its slots saw one trigger already on the stack (in every re-run).
    let seen = env.seen_stacked.borrow();
    assert!(!seen.is_empty() && seen.iter().all(|&n| n == 1));
}

/// A placement env with 20 objects (unordered, one duplicate) for slot 0 and a player and an
/// object for any other slot.
struct Wide;

impl PlacementEnv for Wide {
    fn legal_targets(
        &self,
        _t: &PendingTrigger,
        _m: Option<usize>,
        slot: usize,
        _s: &[StackedTrigger],
    ) -> Vec<ChoiceTarget> {
        if slot == 0 {
            (0..20).rev().map(obj).chain([obj(3)]).collect()
        } else {
            vec![obj(1), ChoiceTarget::Player(p(2))]
        }
    }
}

/// REQ-EVENT-002, CR 603.3d: unordered or duplicated legal lists from the env are normalised;
/// a large target domain uses a validator contract with the request bounds; player targets are
/// offered as player options.
#[test]
fn req_event_002_targets_validator_and_players() {
    let mut t = pt(1, 1, "w");
    t.targets = vec![tslot(0, 0, 20)];
    let proc = |_s: &mut Nil, ctx: &mut ChoiceContext<'_>| {
        place_triggers(&Wide, ctx, &[p(1)], vec![t.clone()])
    };
    let s = ChoiceSession::start(Nil, proc).expect("start");
    let r = s.pending().expect("pending");
    match (&r.options, &r.payload) {
        (ChoiceOptions::Validator(c), ChoicePayload::ChooseTargets { slots, .. }) => {
            assert_eq!((c.min, c.max), (0, 20));
            assert_eq!(slots[0].legal, (0..20).map(obj).collect::<Vec<_>>());
        }
        other => panic!("validator expected, got {other:?}"),
    }
    let mut t2 = pt(1, 1, "w");
    t2.targets = vec![tslot(0, 0, 0), tslot(1, 1, 1)];
    let proc = |_s: &mut Nil, ctx: &mut ChoiceContext<'_>| {
        place_triggers(&Wide, ctx, &[p(1)], vec![t2.clone()])
    };
    let s = ChoiceSession::start(Nil, proc).expect("start");
    let r = s.pending().expect("pending");
    let mut want = vec![ChoiceOption::Player(p(2))];
    want.extend((0..20).map(|n| ChoiceOption::Object(o(n))));
    assert_eq!(r.options, ChoiceOptions::Enumerated(want));
}

/// REQ-EVENT-002, CR 603.3d: a trigger whose required slot has no legal target is removed with
/// nothing asked for it; the others are unaffected and keep their relative order; a min-0 slot
/// with no legal target does not remove; too few legal targets for the minimum removes.
#[test]
fn req_event_002_no_legal_target_removal() {
    let env = Places::default()
        .with("ok", None, 0, &[50])
        .with("short", None, 0, &[51]);
    let mut dead = pt(1, 1, "dead");
    dead.targets = vec![tslot(0, 1, 1)];
    let mut ok = pt(2, 1, "ok");
    ok.targets = vec![tslot(0, 1, 1)];
    let mut zero = pt(3, 1, "zero");
    zero.targets = vec![tslot(0, 0, 2)];
    let mut short = pt(4, 1, "short");
    short.targets = vec![tslot(0, 2, 2)];
    let plain = pt(5, 1, "plain");
    let (pl, asked) = drive(
        &env,
        &[p(1)],
        &[dead, ok, zero, short, plain],
        |r| match r.choice_type {
            ChoiceType::OrderSimultaneousTriggers => order(&[4, 0, 1, 2, 3]),
            _ => r.default_on_timeout.clone(),
        },
    );
    assert_eq!(ids(&pl), vec!["plain", "ok", "zero"]);
    assert_eq!(removed_ids(&pl), vec!["dead", "short"]);
    assert!(
        pl.removed
            .iter()
            .all(|r| r.reason == RemovedReason::NoLegalTargets)
    );
    // ORDER, then targets for "ok" and "zero" only.
    let kinds: Vec<(ChoiceType, Option<ObjectId>)> = asked
        .iter()
        .map(|r| (r.choice_type, r.reason.source))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (ChoiceType::OrderSimultaneousTriggers, None),
            (ChoiceType::ChooseTargets, Some(o(2))),
            (ChoiceType::ChooseTargets, Some(o(3))),
        ]
    );
    match &asked[2].payload {
        ChoicePayload::ChooseTargets { slots, .. } => {
            assert_eq!((slots[0].min, slots[0].max), (0, 0));
            assert!(slots[0].legal.is_empty());
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(pl.stacked[2].targets, vec![Vec::<ChoiceTarget>::new()]);
}

// ---------------------------------------------------------------------------------------------
// Placement: modes
// ---------------------------------------------------------------------------------------------

fn modal(source: u64, id: &str, min: u32, max: u32, modes: Vec<ModeAST>) -> PendingTrigger {
    PendingTrigger {
        modes: Some(ModeSetAST { min, max, modes }),
        ..pt(source, 1, id)
    }
}

/// REQ-EVENT-002, CR 603.3c, CR 700.2b, CR 700.2c: only legal modes are offered; the answer maps
/// back to original indices (ascending); a chosen mode's targets are asked after the ability
/// level slots, an unchosen mode's are not.
#[test]
fn req_event_002_modes_only_legal_offered_and_mapped() {
    let env = Places::default()
        .with("m", None, 0, &[9])
        .with("m", Some(1), 0, &[20, 21])
        .with("m", Some(3), 0, &[30]);
    let mut t = modal(
        7,
        "m",
        1,
        2,
        vec![
            mode("zero", Some(vec![tslot(0, 1, 1)])), // no legal target: illegal
            mode("one", Some(vec![tslot(0, 1, 1)])),
            mode("two", None),
            mode("three", Some(vec![tslot(0, 1, 1)])),
        ],
    );
    t.targets = vec![tslot(0, 1, 1)];
    let (pl, asked) = drive(&env, &[p(1)], &[t], |r| match r.choice_type {
        // Request indices 2 and 0 = original modes 3 and 1.
        ChoiceType::ChooseModes => ChoiceResponse::SelectModes { modes: vec![2, 0] },
        _ => ChoiceResponse::SelectTargets {
            targets: vec![vec![obj(9)], vec![obj(21)], vec![obj(30)]],
        },
    });
    assert_eq!(asked.len(), 2);
    let m = &asked[0];
    assert_eq!(m.request_id.get(), 0);
    assert_eq!(m.choice_type, ChoiceType::ChooseModes);
    assert_eq!(m.chooser, p(1));
    assert_eq!(m.reason.cr, "CR 603.3c");
    assert_eq!(m.reason.source, Some(o(7)));
    assert_eq!((m.min, m.max), (1, 2));
    assert_eq!(
        m.payload,
        ChoicePayload::ChooseModes {
            labels: vec!["one".into(), "two".into(), "three".into()],
            repeats_allowed: false,
            mode_costs: vec![],
        }
    );
    assert_eq!(
        m.options,
        ChoiceOptions::Enumerated(vec![
            ChoiceOption::Mode(0),
            ChoiceOption::Mode(1),
            ChoiceOption::Mode(2)
        ])
    );
    assert_eq!(
        m.default_on_timeout,
        ChoiceResponse::SelectModes { modes: vec![0] }
    );
    assert_eq!(m.validate(&m.default_on_timeout), Ok(()));
    let tr = &asked[1];
    assert_eq!(tr.choice_type, ChoiceType::ChooseTargets);
    match &tr.payload {
        ChoicePayload::ChooseTargets { slots, .. } => {
            let legal: Vec<Vec<ChoiceTarget>> = slots.iter().map(|s| s.legal.clone()).collect();
            assert_eq!(
                legal,
                vec![vec![obj(9)], vec![obj(20), obj(21)], vec![obj(30)]]
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(pl.stacked[0].modes, vec![1, 3]);
    assert_eq!(
        pl.stacked[0].targets,
        vec![vec![obj(9)], vec![obj(21)], vec![obj(30)]]
    );
}

/// REQ-EVENT-002, CR 700.2c: a mode with no targets chosen alone asks no CHOOSE_TARGETS; the
/// max is clamped to the number of legal modes.
#[test]
fn req_event_002_modes_without_targets_and_clamped_max() {
    let env = Places::default();
    let t = modal(
        7,
        "m",
        1,
        2,
        vec![mode("a", Some(vec![tslot(0, 1, 1)])), mode("b", None)],
    );
    let (pl, asked) = drive(&env, &[p(1)], &[t], defaults);
    assert_eq!(asked.len(), 1);
    assert_eq!((asked[0].min, asked[0].max), (1, 1));
    assert_eq!(pl.stacked[0].modes, vec![1]);
    assert!(pl.stacked[0].targets.is_empty());
}

/// REQ-EVENT-002, CR 603.3c, CR 700.2b: no legal mode, or fewer legal modes than the minimum,
/// removes the trigger with nothing asked; a min-0 mode set answered with no mode is removed
/// (CR 603.3c).
#[test]
fn req_event_002_modes_none_legal_removed() {
    let env = Places::default().with("few", Some(0), 0, &[1]);
    let none = modal(1, "none", 1, 1, vec![mode("a", Some(vec![tslot(0, 1, 1)]))]);
    let few = modal(
        2,
        "few",
        2,
        2,
        vec![
            mode("a", Some(vec![tslot(0, 1, 1)])),
            mode("b", Some(vec![tslot(0, 1, 1)])),
        ],
    );
    let (pl, asked) = drive(&env, &[p(1)], &[none, few], defaults);
    assert_eq!(asked.len(), 1, "only the ORDER request");
    assert_eq!(asked[0].choice_type, ChoiceType::OrderSimultaneousTriggers);
    assert!(pl.stacked.is_empty());
    assert_eq!(removed_ids(&pl), vec!["none", "few"]);

    let optional_modes = modal(3, "opt", 0, 1, vec![mode("a", None)]);
    let (pl, asked) = drive(&env, &[p(1)], &[optional_modes], defaults);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].default_on_timeout,
        ChoiceResponse::SelectModes { modes: vec![] }
    );
    assert_eq!(removed_ids(&pl), vec!["opt"]);
}

/// REQ-EVENT-002, CR 603.3d: an ability-level slot with no legal target removes a modal trigger
/// before any mode is asked.
#[test]
fn req_event_002_modal_ability_level_slot_removed_first() {
    let env = Places::default();
    let mut t = modal(1, "m", 1, 1, vec![mode("a", None)]);
    t.targets = vec![tslot(0, 1, 1)];
    let (pl, asked) = drive(&env, &[p(1)], &[t], defaults);
    assert!(asked.is_empty());
    assert_eq!(removed_ids(&pl), vec!["m"]);
}

// ---------------------------------------------------------------------------------------------
// Optional triggers
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-002, CR 603.5: place_triggers never asks CHOOSE_OPTIONAL_TRIGGER; the optional
/// trigger is put on the stack.
#[test]
fn req_event_002_place_never_asks_optional() {
    let env = Places::default();
    let mut t = pt(1, 1, "may");
    t.optional = true;
    let (pl, asked) = drive(&env, &[p(1)], &[t.clone(), pt(2, 1, "x")], defaults);
    assert_eq!(asked.len(), 1);
    assert!(
        asked
            .iter()
            .all(|r| r.choice_type != ChoiceType::ChooseOptionalTrigger)
    );
    assert_eq!(pl.stacked[0].trigger, t);
    let (pl, asked) = drive(&env, &[p(1)], &[t.clone()], defaults);
    assert!(asked.is_empty());
    assert_eq!(pl.stacked[0].trigger, t);
}

fn run_optional(
    t: &PendingTrigger,
    answers: &[ChoiceResponse],
) -> (bool, Option<PendingChoiceRequest>) {
    let proc = |_s: &mut Nil, ctx: &mut ChoiceContext<'_>| ask_optional_trigger(ctx, t);
    let s = ChoiceSession::start(Nil, proc).expect("start");
    let first = s.pending().cloned();
    let s = ChoiceSession::replay(Nil, proc, answers).expect("replay");
    match s.status() {
        Status::Done { output, .. } => (*output, first),
        Status::Pending(_) => panic!("done expected"),
    }
}

/// REQ-EVENT-002, CR 603.5: ask_optional_trigger: no ask if not optional; YES and NO; the
/// request shape and its default NO.
#[test]
fn req_event_002_ask_optional_trigger() {
    let plain = pt(4, 2, "plain");
    assert_eq!(run_optional(&plain, &[]), (true, None));
    let mut may = pt(4, 2, "may");
    may.optional = true;
    let (yes, r) = run_optional(&may, &[ChoiceResponse::Yes]);
    assert!(yes);
    let r = r.expect("asked");
    assert_eq!(r.request_id.get(), 0);
    assert_eq!(r.chooser, p(2));
    assert_eq!(r.choice_type, ChoiceType::ChooseOptionalTrigger);
    assert_eq!(r.reason.cr, "CR 603.5");
    assert_eq!(r.reason.source, Some(o(4)));
    assert_eq!((r.min, r.max), (1, 1));
    assert_eq!(
        r.payload,
        ChoicePayload::ChooseOptionalTrigger {
            description: "object 4 ability may".into()
        }
    );
    assert_eq!(
        r.options,
        ChoiceOptions::Enumerated(vec![ChoiceOption::Yes, ChoiceOption::No])
    );
    assert_eq!(r.default_on_timeout, ChoiceResponse::No);
    assert!(!run_optional(&may, &[ChoiceResponse::No]).0);
    let proc = |_s: &mut Nil, ctx: &mut ChoiceContext<'_>| ask_optional_trigger(ctx, &may);
    let mut s = ChoiceSession::start(Nil, proc).expect("start");
    s.submit_default(r.request_id).expect("default");
    assert!(matches!(s.status(), Status::Done { output: false, .. }));
    let mut sourceless = may.clone();
    sourceless.source = None;
    let (_, r) = run_optional(&sourceless, &[ChoiceResponse::Yes]);
    assert_eq!(r.expect("asked").reason.source, None);
}

// ---------------------------------------------------------------------------------------------
// Determinism and properties
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-002, REQ-ARCH-002, REQ-CORE-001: replaying the same answers through ChoiceSession
/// gives an equal placement, answered one by one or all at once.
#[test]
fn req_event_002_replay_determinism() {
    let env = Places::default().with("t", None, 0, &[1, 2]);
    let mut t = pt(3, 1, "t");
    t.targets = vec![tslot(0, 1, 1)];
    let w = vec![pt(1, 2, "x"), t, pt(2, 2, "y"), pt2(4, 1, "z")];
    let apnap = [p(1), p(2)];
    let mut answers = Vec::new();
    let (first, _) = drive(&env, &apnap, &w, |r| {
        let a = match r.choice_type {
            ChoiceType::OrderSimultaneousTriggers => order(&[1, 0]),
            _ => ChoiceResponse::SelectTargets {
                targets: vec![vec![obj(2)]],
            },
        };
        answers.push(a.clone());
        a
    });
    let proc =
        |_s: &mut Nil, ctx: &mut ChoiceContext<'_>| place_triggers(&env, ctx, &apnap, w.clone());
    for _ in 0..3 {
        let s = ChoiceSession::replay(Nil, proc, &answers).expect("replay");
        match s.status() {
            Status::Done { output, .. } => assert_eq!(*output, first),
            Status::Pending(_) => panic!("done expected"),
        }
    }
    assert_eq!(ids(&first), vec!["t", "y", "x", "z"]);
    assert_eq!(first.stacked[0].targets, vec![vec![obj(2)]]);
}

/// REQ-EVENT-002, CR 603.3b: request ids are sequential from 0 across a whole placement.
#[test]
fn req_event_002_request_ids_sequential() {
    let env = Places::default().with("t", None, 0, &[1]);
    let mut t = pt(1, 1, "t");
    t.targets = vec![tslot(0, 1, 1)];
    let w = vec![t, pt(2, 1, "u"), pt(3, 2, "v"), pt(4, 2, "w")];
    let (_, asked) = drive(&env, &[p(1), p(2)], &w, defaults);
    let got: Vec<u64> = asked.iter().map(|r| r.request_id.get()).collect();
    assert_eq!(got, vec![0, 1, 2]);
}

/// A small deterministic generator for the hand-rolled property test (R3: seeded, no clock).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }

    fn shuffle(&mut self, n: usize) -> Vec<u32> {
        let mut v: Vec<u32> = (0..n).filter_map(|i| u32::try_from(i).ok()).collect();
        for i in (1..v.len()).rev() {
            let bound = u64::try_from(i + 1).expect("small");
            let j = usize::try_from(self.next(bound)).expect("small");
            v.swap(i, j);
        }
        v
    }
}

/// REQ-EVENT-002, REQ-ARCH-002, CR 603.3b, CR 101.4 (property): for random players, triggers,
/// parts and permutation answers, the stacked order is part 1 then part 2, players in APNAP order
/// within each part, and each player's chosen order within each group; every trigger is placed
/// exactly once and ORDER is asked exactly for groups of 2 or more.
#[test]
fn req_event_002_property_stacked_order() {
    let env = Places::default();
    let mut rng = Lcg(0x0f30_0031);
    for _ in 0..300 {
        let players = 1 + rng.next(4);
        let mut apnap: Vec<PlayerId> = (1..=players).map(p).collect();
        let rot = usize::try_from(rng.next(players)).expect("small");
        apnap.rotate_left(rot);
        let count = rng.next(9);
        let waiting: Vec<PendingTrigger> = (0..count)
            .map(|i| {
                let ctrl = 1 + rng.next(players);
                let id = format!("t{i}");
                if rng.next(2) == 0 {
                    pt(i, ctrl, &id)
                } else {
                    pt2(i, ctrl, &id)
                }
            })
            .collect();
        let mut chosen: Vec<(PlayerId, Vec<String>, Vec<u32>)> = Vec::new();
        let (pl, asked) = drive(&env, &apnap, &waiting, |r| {
            let names = order_payload(r);
            let perm = rng.shuffle(names.len());
            chosen.push((r.chooser, names, perm.clone()));
            order(&perm)
        });
        // The expected order, computed independently of the implementation.
        let mut want: Vec<String> = Vec::new();
        let mut asks = 0;
        let mut answers = chosen.iter();
        for part in [false, true] {
            for &player in &apnap {
                let group: Vec<&PendingTrigger> = waiting
                    .iter()
                    .filter(|t| t.controller == player && t.responds_to_trigger == part)
                    .collect();
                if group.len() >= 2 {
                    asks += 1;
                    let (chooser, names, perm) = answers.next().expect("an ORDER ask");
                    assert_eq!(*chooser, player);
                    let expect_names: Vec<String> = group.iter().map(|t| t.description()).collect();
                    assert_eq!(*names, expect_names);
                    want.extend(perm.iter().map(|&i| group[idx(i)].ability_id.clone()));
                } else {
                    want.extend(group.iter().map(|t| t.ability_id.clone()));
                }
            }
        }
        assert_eq!(asked.len(), asks);
        assert_eq!(ids(&pl), want);
        assert_eq!(pl.stacked.len(), waiting.len());
        assert!(pl.removed.is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// CR round 1: delayed intervening "if" with captured bindings
// ---------------------------------------------------------------------------------------------

/// A trigger env whose intervening "if" holds iff every `(name, value)` of `want` is in the
/// bindings it is given. Registrations are `regs`; any object or player matches; filter
/// predicates hold. Records the view and ability id of every condition check.
#[derive(Default)]
struct BindEnv {
    regs: Vec<TriggerRegistration>,
    want: BTreeMap<String, EventField>,
    calls: RefCell<Vec<(StateView, String, bool)>>,
}

impl TriggerEnv for BindEnv {
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
        true
    }

    fn condition_holds(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        _pred: &PredicateAST,
        bindings: &BTreeMap<String, EventField>,
    ) -> bool {
        self.calls.borrow_mut().push((
            view,
            reg.ability.base.ability_id.clone(),
            reg.ability.intervening_if.is_some(),
        ));
        self.want.iter().all(|(k, v)| bindings.get(k) == Some(v))
    }

    fn is_visible_to_all(&self, _object: ObjectId) -> bool {
        true
    }
}

/// A delayed ability on `Attacks` binding event field `subj` as `it` and `num` as `e`, with an
/// intervening "if".
fn delayed_if_ability() -> TriggeredAbilityAST {
    let mut m = EventMatcher::new(EventKind::Attacks);
    let mut bind = BTreeMap::new();
    bind.insert("subj".to_owned(), "it".to_owned());
    bind.insert("num".to_owned(), "e".to_owned());
    m.bind = Some(bind);
    let mut a = ability("orig", Trigger::Event(m));
    a.intervening_if = Some(if_pred());
    a
}

fn attack_event(subj: u64, num: u64) -> TriggerEvent {
    TriggerEvent::new(vec![EventKind::Attacks])
        .with_field("subj", EventField::Object(o(subj)))
        .with_field("num", EventField::Object(o(num)))
}

fn want(pairs: &[(&str, u64)]) -> BTreeMap<String, EventField> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), EventField::Object(o(*v))))
        .collect()
}

/// REQ-EVENT-002, CR 603.4, CR 603.7: a delayed trigger's intervening "if" sees its captured
/// value: the event binds `it` to 99, the capture to 40, the condition holds only for 40, so it
/// triggers (with detect end to end). The registrations listed for detect carry no condition;
/// take_triggered checks the stored one under the `delayed#<n>` id, in the post-event view.
#[test]
fn req_event_002_delayed_intervening_if_uses_captured_value() {
    let mut d = DelayedTriggers::new();
    let id = d.register(o(6), p(2), delayed_if_ability(), want(&[("it", 40)]), true);
    let regs = d.registrations();
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].ability.intervening_if, None);
    assert_eq!(regs[0].ability.base.ability_id, "delayed#0");
    let env = BindEnv {
        regs,
        want: want(&[("it", 40)]),
        ..BindEnv::default()
    };
    let found = detect(&env, &mut MemoryTable::new(), 1, &[attack_event(99, 1)]);
    assert_eq!(found.len(), 1);
    let got = d.take_triggered(&env, 1, &found);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].ability_id, format!("delayed#{}", id.get()));
    assert_eq!(got[0].bindings["it"], EventField::Object(o(40)));
    assert_eq!(got[0].bindings["e"], EventField::Object(o(1)));
    assert!(d.is_empty(), "the one-shot fired and is removed");
    assert_eq!(
        *env.calls.borrow(),
        vec![(StateView::PostEvent, "delayed#0".to_owned(), true)]
    );
}

/// REQ-EVENT-002, CR 603.4, CR 603.7, CR 603.7b: the mirror case: the condition holds only for
/// the event's value of a captured name, so the delayed trigger does not trigger and the
/// one-shot stays registered.
#[test]
fn req_event_002_delayed_intervening_if_ignores_event_value_of_captured_name() {
    let mut d = DelayedTriggers::new();
    d.register(o(6), p(2), delayed_if_ability(), want(&[("it", 40)]), true);
    let env = BindEnv {
        regs: d.registrations(),
        want: want(&[("it", 99)]),
        ..BindEnv::default()
    };
    let found = detect(&env, &mut MemoryTable::new(), 1, &[attack_event(99, 1)]);
    assert_eq!(found.len(), 1);
    assert!(d.take_triggered(&env, 1, &found).is_empty());
    assert_eq!(
        d.len(),
        1,
        "a one-shot that did not trigger stays registered"
    );
}

/// REQ-EVENT-002, CR 603.4, CR 603.7b: a one-shot with two matching elements, the first failing
/// its intervening "if" and the second passing, fires once on the second and is removed.
#[test]
fn req_event_002_delayed_one_shot_skips_element_failing_condition() {
    let mut d = DelayedTriggers::new();
    d.register(o(6), p(2), delayed_if_ability(), want(&[("it", 40)]), true);
    let env = BindEnv {
        regs: d.registrations(),
        want: want(&[("it", 40), ("e", 2)]),
        ..BindEnv::default()
    };
    let events = [
        attack_event(99, 1),
        attack_event(98, 2),
        attack_event(97, 2),
    ];
    let found = detect(&env, &mut MemoryTable::new(), 1, &events);
    assert_eq!(found.len(), 3);
    let got = d.take_triggered(&env, 1, &found);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].bindings["e"], EventField::Object(o(2)));
    assert_eq!(got[0].bindings["it"], EventField::Object(o(40)));
    assert!(d.is_empty());
}

/// REQ-EVENT-002, CR 603.4, CR 603.7b: with a stated duration every passing element triggers
/// and the registration stays; an element with a look-back category is checked in the
/// pre-event view; an ability without an intervening "if" is not checked.
#[test]
fn req_event_002_delayed_condition_views_and_duration() {
    let mut d = DelayedTriggers::new();
    d.register(o(6), p(2), delayed_if_ability(), want(&[("it", 40)]), false);
    d.register(
        o(7),
        p(2),
        event_ability("plain", EventKind::Attacks),
        BTreeMap::new(),
        false,
    );
    let env = BindEnv {
        want: want(&[("e", 2)]),
        ..BindEnv::default()
    };
    let mut pre = waiting(6, 2, "delayed#0", &[("e", 2)]);
    pre.look_back = Some(LookBackKind::LeavesBattlefield);
    let got = d.take_triggered(
        &env,
        1,
        &[
            waiting(6, 2, "delayed#0", &[("e", 1)]),
            pre,
            waiting(6, 2, "delayed#0", &[("e", 2)]),
            waiting(7, 2, "delayed#1", &[("e", 1)]),
        ],
    );
    assert_eq!(
        got.iter().map(|g| g.ability_id.clone()).collect::<Vec<_>>(),
        vec!["delayed#0", "delayed#0", "delayed#1"]
    );
    assert_eq!(d.len(), 2);
    assert_eq!(
        env.calls.borrow().iter().map(|c| c.0).collect::<Vec<_>>(),
        vec![
            StateView::PostEvent,
            StateView::PreEvent,
            StateView::PostEvent
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// CR round 1: ordering labels
// ---------------------------------------------------------------------------------------------

/// A placement env with no legal targets that calls the objects in `public` public.
#[derive(Default)]
struct Public {
    public: BTreeSet<ObjectId>,
}

impl PlacementEnv for Public {
    fn legal_targets(
        &self,
        _trigger: &PendingTrigger,
        _mode: Option<usize>,
        _slot: usize,
        _stacked_so_far: &[StackedTrigger],
    ) -> Vec<ChoiceTarget> {
        Vec::new()
    }

    fn is_public_object(&self, object: ObjectId) -> bool {
        self.public.contains(&object)
    }
}

fn with_binds(mut t: PendingTrigger, binds: &[(&str, EventField)]) -> PendingTrigger {
    for (k, v) in binds {
        t.bindings.insert((*k).to_owned(), v.clone());
    }
    t
}

/// Two triggers of one ability bound to different objects, plus a unique trigger.
fn label_group() -> Vec<PendingTrigger> {
    vec![
        with_binds(
            pt(3, 1, "a"),
            &[
                ("n", EventField::Value(FilterValue::Int(5))),
                ("target", EventField::Object(o(9))),
                ("who", EventField::Player(p(2))),
            ],
        ),
        with_binds(pt(3, 1, "a"), &[("target", EventField::Object(o(10)))]),
        with_binds(pt(4, 1, "b"), &[("target", EventField::Object(o(9)))]),
    ]
}

/// REQ-EVENT-002, REQ-ARCH-002, CR 603.3b, CR 101.4c: triggers sharing a base label are told
/// apart by their public bindings in name order (players, public objects); values are never
/// shown; a unique base label is unchanged; the chosen order still maps to the same triggers.
#[test]
fn req_event_002_order_labels_show_public_bindings() {
    let env = Public {
        public: [o(9), o(10)].into_iter().collect(),
    };
    let (pl, asked) = drive(&env, &[p(1)], &label_group(), |_| order(&[1, 2, 0]));
    assert_eq!(asked.len(), 1);
    assert_eq!(
        order_payload(&asked[0]),
        vec![
            "object 3 ability a with target=object 9, who=player 2".to_owned(),
            "object 3 ability a with target=object 10".into(),
            "object 4 ability b".into(),
        ]
    );
    let stacked: Vec<(String, EventField)> = pl
        .stacked
        .iter()
        .map(|s| {
            (
                s.trigger.ability_id.clone(),
                s.trigger.bindings["target"].clone(),
            )
        })
        .collect();
    assert_eq!(
        stacked,
        vec![
            ("a".to_owned(), EventField::Object(o(10))),
            ("b".to_owned(), EventField::Object(o(9))),
            ("a".to_owned(), EventField::Object(o(9))),
        ]
    );
    // PendingTrigger::description itself is unchanged.
    assert_eq!(label_group()[0].description(), "object 3 ability a");
}

/// REQ-EVENT-002, REQ-ARCH-002, CR 603.3b: with the default `is_public_object` (false) no object
/// binding leaks: two triggers bound only to objects keep identical base labels; a non-public
/// object is not shown while a public one is; value-only bindings are never shown.
#[test]
fn req_event_002_order_labels_hide_private_bindings() {
    let objects_only = vec![
        with_binds(pt(3, 1, "a"), &[("target", EventField::Object(o(9)))]),
        with_binds(pt(3, 1, "a"), &[("target", EventField::Object(o(10)))]),
    ];
    let (_, asked) = drive(&Places::default(), &[p(1)], &objects_only, defaults);
    assert_eq!(
        order_payload(&asked[0]),
        vec!["object 3 ability a".to_owned(), "object 3 ability a".into()]
    );
    let half = Public {
        public: [o(9)].into_iter().collect(),
    };
    let (_, asked) = drive(&half, &[p(1)], &objects_only, defaults);
    assert_eq!(
        order_payload(&asked[0]),
        vec![
            "object 3 ability a with target=object 9".to_owned(),
            "object 3 ability a".into()
        ]
    );
    let values = vec![
        with_binds(
            pt(3, 1, "a"),
            &[("n", EventField::Value(FilterValue::Int(1)))],
        ),
        with_binds(
            pt(3, 1, "a"),
            &[("n", EventField::Value(FilterValue::Int(2)))],
        ),
    ];
    let (_, asked) = drive(&half, &[p(1)], &values, defaults);
    assert_eq!(
        order_payload(&asked[0]),
        vec!["object 3 ability a".to_owned(), "object 3 ability a".into()]
    );
}

// ---------------------------------------------------------------------------------------------
// CR round 1: CHOOSE_MODES validator
// ---------------------------------------------------------------------------------------------

/// `n` modes without targets, except that the modes in `illegal` have a required slot with no
/// legal target (so they are not offered).
fn many_modes(n: usize, illegal: &[usize]) -> Vec<ModeAST> {
    (0..n)
        .map(|i| {
            let targets = illegal.contains(&i).then(|| vec![tslot(0, 1, 1)]);
            mode(&format!("m{i}"), targets)
        })
        .collect()
}

fn modes_request(t: PendingTrigger, answer: Vec<u32>) -> (TriggerPlacement, PendingChoiceRequest) {
    let (pl, asked) = drive(&Places::default(), &[p(1)], &[t], move |_| {
        ChoiceResponse::SelectModes {
            modes: answer.clone(),
        }
    });
    assert_eq!(asked.len(), 1);
    (pl, asked[0].clone())
}

/// REQ-EVENT-002, CR 603.3c, CR 700.2b: 16 legal modes (of 18) choosing exactly 8 has
/// C(16, 8) = 12,870 answers, at least ENUMERATION_LIMIT: the request carries a Modes validator
/// with the request bounds, and an answer maps back to the original mode indices.
#[test]
fn req_event_002_modes_validator_for_many_answers() {
    let t = modal(7, "m", 8, 8, many_modes(18, &[0, 5]));
    let (pl, r) = modes_request(t, vec![0, 1, 2, 3, 4, 5, 6, 15]);
    assert_eq!(r.choice_type, ChoiceType::ChooseModes);
    assert_eq!((r.min, r.max), (8, 8));
    assert_eq!(
        r.options,
        ChoiceOptions::Validator(ValidatorContract {
            min: 8,
            max: 8,
            rule: ValidatorRule::Modes {
                count: 16,
                repeats_allowed: false,
            },
        })
    );
    assert_eq!(r.validate(&r.default_on_timeout), Ok(()));
    assert_eq!(pl.stacked[0].modes, vec![1, 2, 3, 4, 6, 7, 8, 17]);
    // The smallest counts at or above the limit (C(24, 4) = 10,626) also use a validator.
    let t = modal(7, "m", 4, 4, many_modes(24, &[]));
    let (_, r) = modes_request(t, vec![0, 1, 2, 3]);
    assert!(matches!(r.options, ChoiceOptions::Validator(_)));
}

/// REQ-EVENT-002, CR 603.3c, CR 700.2b: fewer than ENUMERATION_LIMIT answers keep the
/// enumerated mode list: 3 modes, and the largest count below the limit (15 modes choosing 9 to
/// 15: 9,949 answers; no mode count and bounds give exactly ENUMERATION_LIMIT - 1).
#[test]
fn req_event_002_modes_enumerated_below_limit() {
    let t = modal(7, "m", 1, 1, many_modes(3, &[]));
    let (pl, r) = modes_request(t, vec![2]);
    assert_eq!(
        r.options,
        ChoiceOptions::Enumerated((0..3).map(ChoiceOption::Mode).collect())
    );
    assert_eq!(pl.stacked[0].modes, vec![2]);
    let t = modal(7, "m", 9, 15, many_modes(15, &[]));
    let (_, r) = modes_request(t, (0..9).collect());
    assert_eq!(
        r.options,
        ChoiceOptions::Enumerated((0..15).map(ChoiceOption::Mode).collect())
    );
}

// ---------------------------------------------------------------------------------------------
// CR round 2: delayed trigger limits are counted after the intervening "if"
// ---------------------------------------------------------------------------------------------

/// `delayed_if_ability` (an intervening "if") with the trigger limit `per_turn` / `per_game`.
fn limited_if_ability(per_turn: Option<u32>, per_game: Option<u32>) -> TriggeredAbilityAST {
    let mut a = delayed_if_ability();
    a.limit = Some(ActivationLimit { per_turn, per_game });
    a
}

/// A `BindEnv` listing `d`'s registrations whose condition holds iff `e` is bound to object 2.
fn passes_on_e2(d: &DelayedTriggers) -> BindEnv {
    BindEnv {
        regs: d.registrations(),
        want: want(&[("e", 2)]),
        ..BindEnv::default()
    }
}

/// detect, then take_triggered, for `turn`; the `e` binding of every pending trigger.
fn detect_and_take(
    d: &mut DelayedTriggers,
    env: &BindEnv,
    memory: &mut MemoryTable,
    turn: u64,
    events: &[TriggerEvent],
) -> Vec<EventField> {
    let found = detect(env, memory, turn, events);
    assert_eq!(found.len(), events.len(), "detect never applies the limit");
    d.take_triggered(env, turn, &found)
        .iter()
        .map(|g| g.bindings["e"].clone())
        .collect()
}

/// REQ-EVENT-002, CR 603.2h, CR 603.4, CR 603.7b: one batch: a delayed ability with a stated
/// duration and a limit of once each turn; of two matching events the first fails the
/// intervening "if" and the second passes it. The failing event uses up nothing, so exactly one
/// pending trigger results, from the second event.
#[test]
fn req_event_002_delayed_limit_after_condition_one_batch() {
    let mut d = DelayedTriggers::new();
    d.register(
        o(6),
        p(2),
        limited_if_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    let env = passes_on_e2(&d);
    let mut memory = MemoryTable::new();
    let got = detect_and_take(
        &mut d,
        &env,
        &mut memory,
        1,
        &[attack_event(1, 1), attack_event(1, 2)],
    );
    assert_eq!(got, vec![EventField::Object(o(2))]);
    assert_eq!(d.len(), 1);
}

/// REQ-EVENT-002, CR 603.2h, CR 603.4, CR 603.7b: two calls in the same turn: the first event
/// fails the intervening "if" (no trigger, no limit used); a passing event later that turn
/// triggers.
#[test]
fn req_event_002_delayed_limit_after_condition_two_calls_same_turn() {
    let mut d = DelayedTriggers::new();
    d.register(
        o(6),
        p(2),
        limited_if_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    let env = passes_on_e2(&d);
    let mut memory = MemoryTable::new();
    assert!(detect_and_take(&mut d, &env, &mut memory, 5, &[attack_event(1, 1)]).is_empty());
    assert_eq!(
        detect_and_take(&mut d, &env, &mut memory, 5, &[attack_event(1, 2)]),
        vec![EventField::Object(o(2))]
    );
}

/// REQ-EVENT-002, CR 603.2h, CR 603.7b: the limit still applies: once each turn gives one
/// trigger from two passing events, none from a third that turn, and one again the next turn;
/// once each game (no per-turn limit, no intervening "if") does not trigger again the next turn.
/// detect keeps no limit counters in Memory for delayed abilities.
#[test]
fn req_event_002_delayed_limit_per_turn_and_per_game() {
    let mut d = DelayedTriggers::new();
    d.register(
        o(6),
        p(2),
        limited_if_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    let env = passes_on_e2(&d);
    let mut memory = MemoryTable::new();
    let two = [attack_event(1, 2), attack_event(3, 2)];
    assert_eq!(
        detect_and_take(&mut d, &env, &mut memory, 5, &two),
        vec![EventField::Object(o(2))]
    );
    assert!(detect_and_take(&mut d, &env, &mut memory, 5, &[attack_event(1, 2)]).is_empty());
    assert_eq!(
        detect_and_take(&mut d, &env, &mut memory, 6, &[attack_event(1, 2)]),
        vec![EventField::Object(o(2))]
    );
    assert_eq!(memory, MemoryTable::new(), "no trigger.limit.* entries");

    let mut g = DelayedTriggers::new();
    let mut a = event_ability("orig", EventKind::Attacks);
    let mut bind = BTreeMap::new();
    bind.insert("num".to_owned(), "e".to_owned());
    if let Trigger::Event(m) = &mut a.trigger {
        m.bind = Some(bind);
    }
    a.limit = Some(ActivationLimit {
        per_turn: None,
        per_game: Some(1),
    });
    g.register(o(7), p(2), a, BTreeMap::new(), false);
    let genv = BindEnv {
        regs: g.registrations(),
        ..BindEnv::default()
    };
    assert_eq!(
        detect_and_take(&mut g, &genv, &mut memory, 5, &[attack_event(1, 4)]),
        vec![EventField::Object(o(4))]
    );
    assert!(detect_and_take(&mut g, &genv, &mut memory, 6, &[attack_event(1, 4)]).is_empty());
    assert!(detect_and_take(&mut g, &genv, &mut memory, 7, &[attack_event(1, 4)]).is_empty());
    assert!(genv.calls.borrow().is_empty(), "no intervening if to check");
    assert_eq!(memory, MemoryTable::new());
    assert_eq!(g.len(), 1);
}

/// REQ-EVENT-002, CR 603.2h, CR 603.4: the registrations listed for detect carry neither the
/// intervening "if" nor the limit; the condition check inside take_triggered still sees the
/// condition; detect writes nothing to Memory for the delayed ability.
#[test]
fn req_event_002_delayed_registrations_hide_limit() {
    let mut d = DelayedTriggers::new();
    d.register(
        o(6),
        p(2),
        limited_if_ability(Some(1), Some(3)),
        BTreeMap::new(),
        false,
    );
    let regs = d.registrations();
    assert_eq!(regs.len(), 1);
    assert_eq!(regs[0].ability.intervening_if, None);
    assert_eq!(regs[0].ability.limit, None);
    let env = passes_on_e2(&d);
    let mut memory = MemoryTable::new();
    assert_eq!(
        detect_and_take(&mut d, &env, &mut memory, 1, &[attack_event(1, 2)]).len(),
        1
    );
    assert!(memory.get(o(6)).is_none());
    assert_eq!(memory, MemoryTable::new());
    assert_eq!(
        *env.calls.borrow(),
        vec![(StateView::PostEvent, "delayed#0".to_owned(), true)]
    );
}

/// REQ-EVENT-002, CR 603.2h, CR 603.4, CR 603.7b: a one-shot whose every element is dropped,
/// one by its intervening "if" and one by its limit, does not trigger and stays registered.
/// (A one-shot that triggers is removed, so its limit can only be reached here with a limit of
/// zero, which the DSL rejects but the AST can carry.)
#[test]
fn req_event_002_delayed_one_shot_dropped_by_condition_or_limit_stays() {
    let mut d = DelayedTriggers::new();
    d.register(
        o(6),
        p(2),
        limited_if_ability(Some(0), None),
        BTreeMap::new(),
        true,
    );
    let env = passes_on_e2(&d);
    let mut memory = MemoryTable::new();
    assert!(
        detect_and_take(
            &mut d,
            &env,
            &mut memory,
            1,
            &[attack_event(1, 1), attack_event(1, 2)]
        )
        .is_empty()
    );
    assert_eq!(
        d.len(),
        1,
        "a one-shot that did not trigger stays registered"
    );
    assert_eq!(env.calls.borrow().len(), 2, "both conditions were checked");
}

// ---------------------------------------------------------------------------------------------
// CR round 3: fire enforces the registration's limit with the counters take_triggered uses
// ---------------------------------------------------------------------------------------------

/// An event ability with the trigger limit `per_turn` / `per_game`.
fn limited_event_ability(per_turn: Option<u32>, per_game: Option<u32>) -> TriggeredAbilityAST {
    let mut a = event_ability("orig", EventKind::Attacks);
    a.limit = Some(ActivationLimit { per_turn, per_game });
    a
}

/// REQ-EVENT-002, CR 603.2h, CR 603.7b: a duration registration with a limit of once each game
/// fires once through `fire`; later calls, in the same or a later turn, give None and the
/// registration stays. (The unlimited-by-`fire` code returned Some on every call.)
#[test]
fn req_event_002_fire_applies_per_game_limit() {
    let mut d = DelayedTriggers::new();
    let id = d.register(
        o(1),
        p(1),
        limited_event_ability(None, Some(1)),
        BTreeMap::new(),
        false,
    );
    assert!(d.fire(&Env::default(), 1, id).is_some());
    assert_eq!(d.fire(&Env::default(), 1, id), None);
    assert_eq!(d.fire(&Env::default(), 2, id), None);
    assert_eq!(d.len(), 1);
}

/// REQ-EVENT-002, CR 603.2h: a once-each-turn limit counts per turn in `fire`.
#[test]
fn req_event_002_fire_applies_per_turn_limit() {
    let mut d = DelayedTriggers::new();
    let id = d.register(
        o(1),
        p(1),
        limited_event_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    assert!(d.fire(&Env::default(), 5, id).is_some());
    assert_eq!(d.fire(&Env::default(), 5, id), None);
    assert!(d.fire(&Env::default(), 6, id).is_some());
    assert_eq!(d.len(), 1);
}

/// REQ-EVENT-002, CR 603.2h, CR 603.7b: `fire` and `take_triggered` share one counter per
/// registration, in both orders.
#[test]
fn req_event_002_fire_and_take_triggered_share_counters() {
    let mut d = DelayedTriggers::new();
    let id = d.register(
        o(1),
        p(1),
        limited_event_ability(None, Some(1)),
        BTreeMap::new(),
        false,
    );
    let got = d.take_triggered(&Env::default(), 1, &[waiting(1, 1, "delayed#0", &[])]);
    assert_eq!(got.len(), 1);
    assert_eq!(d.fire(&Env::default(), 1, id), None);
    assert_eq!(d.fire(&Env::default(), 2, id), None);

    let mut r = DelayedTriggers::new();
    let id = r.register(
        o(1),
        p(1),
        limited_event_ability(None, Some(1)),
        BTreeMap::new(),
        false,
    );
    assert!(r.fire(&Env::default(), 1, id).is_some());
    assert!(
        r.take_triggered(&Env::default(), 1, &[waiting(1, 1, "delayed#0", &[])])
            .is_empty()
    );
    assert_eq!(r.len(), 1);

    // A per-turn count made by either counts for both in the same turn.
    let mut t = DelayedTriggers::new();
    let id = t.register(
        o(1),
        p(1),
        limited_event_ability(Some(1), None),
        BTreeMap::new(),
        false,
    );
    assert!(t.fire(&Env::default(), 3, id).is_some());
    assert!(
        t.take_triggered(&Env::default(), 3, &[waiting(1, 1, "delayed#0", &[])])
            .is_empty()
    );
    assert_eq!(
        t.take_triggered(&Env::default(), 4, &[waiting(1, 1, "delayed#0", &[])])
            .len(),
        1
    );
}

/// REQ-EVENT-002, CR 603.2h, CR 603.7b: a one-shot whose limit is reached from the start does
/// not fire and is not consumed.
#[test]
fn req_event_002_fire_one_shot_at_limit_stays() {
    let mut d = DelayedTriggers::new();
    let id = d.register(
        o(1),
        p(1),
        limited_event_ability(None, Some(0)),
        BTreeMap::new(),
        true,
    );
    assert_eq!(d.fire(&Env::default(), 1, id), None);
    assert_eq!(d.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// CR round 4: `fire` evaluates the stored intervening "if" before it counts the limit
// ---------------------------------------------------------------------------------------------

/// A `BindEnv` whose condition holds iff the captured `it` is 40 (`holds`), or never (not).
fn cond_env(holds: bool) -> BindEnv {
    BindEnv {
        want: want(&[("it", if holds { 40 } else { 99 })]),
        ..BindEnv::default()
    }
}

/// REQ-EVENT-002, CR 603.4, CR 603.2h: a false occurrence at a step does not use up a
/// once-each-game limit; only the true one does. (The old `fire` counted the limit without the
/// condition: Some on the first, false, call and None afterwards.)
#[test]
fn req_event_002_fire_false_before_true_per_game() {
    let mut d = DelayedTriggers::new();
    let id = d.register(
        o(6),
        p(2),
        limited_if_ability(None, Some(1)),
        want(&[("it", 40)]),
        false,
    );
    assert_eq!(d.fire(&cond_env(false), 1, id), None);
    assert_eq!(d.fire(&cond_env(false), 1, id), None);
    assert_eq!(d.len(), 1);
    assert!(d.fire(&cond_env(true), 1, id).is_some());
    assert_eq!(d.fire(&cond_env(true), 1, id), None);
    assert_eq!(d.fire(&cond_env(true), 2, id), None);
    assert_eq!(d.len(), 1);
}

/// REQ-EVENT-002, CR 603.4, CR 603.2h: the same for a once-each-turn limit.
#[test]
fn req_event_002_fire_false_before_true_per_turn() {
    let mut d = DelayedTriggers::new();
    let id = d.register(
        o(6),
        p(2),
        limited_if_ability(Some(1), None),
        want(&[("it", 40)]),
        false,
    );
    assert_eq!(d.fire(&cond_env(false), 5, id), None);
    assert!(d.fire(&cond_env(true), 5, id).is_some());
    assert_eq!(d.fire(&cond_env(true), 5, id), None);
    assert!(d.fire(&cond_env(true), 6, id).is_some());
}

/// REQ-EVENT-002, CR 603.4, CR 603.7b: a one-shot whose condition is false does not fire and
/// stays; a later true firing fires and removes it.
#[test]
fn req_event_002_fire_one_shot_false_condition_stays() {
    let mut d = DelayedTriggers::new();
    let id = d.register(o(6), p(2), delayed_if_ability(), want(&[("it", 40)]), true);
    assert_eq!(d.fire(&cond_env(false), 1, id), None);
    assert_eq!(d.len(), 1);
    assert!(d.fire(&cond_env(true), 1, id).is_some());
    assert_eq!(d.len(), 0);
}

/// REQ-EVENT-002, CR 603.4, CR 603.7: the condition sees the captured bindings, the post-event
/// view, and a registration `delayed#<n>` that keeps its "if" and has no limit.
#[test]
fn req_event_002_fire_condition_sees_captured_bindings() {
    let mut d = DelayedTriggers::new();
    d.register(o(1), p(1), delayed_if_ability(), BTreeMap::new(), false);
    let captured = want(&[("it", 40)]);
    let id = d.register(
        o(6),
        p(2),
        limited_if_ability(Some(1), Some(1)),
        captured.clone(),
        false,
    );
    let seen = RecordEnv::default();
    assert!(d.fire(&seen, 1, id).is_some());
    let got = seen.seen.borrow();
    assert_eq!(got.len(), 1);
    let (view, bindings, ability_id, has_if, has_limit) = &got[0];
    assert_eq!(*view, StateView::PostEvent);
    assert_eq!(*bindings, captured);
    assert_eq!(ability_id, "delayed#1");
    assert!(*has_if);
    assert!(!*has_limit);
}

/// A `BindEnv` look-alike that records view, bindings and registration facts of each condition
/// check, and lets every condition hold.
/// View, bindings, ability id, has an "if", has a limit.
type SeenCheck = (StateView, BTreeMap<String, EventField>, String, bool, bool);

#[derive(Default)]
struct RecordEnv {
    inner: BindEnv,
    seen: RefCell<Vec<SeenCheck>>,
}

impl TriggerEnv for RecordEnv {
    fn registrations(&self, view: StateView) -> Vec<TriggerRegistration> {
        self.inner.registrations(view)
    }

    fn object_matches(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        object: ObjectId,
        selector: &ObjectSelectorAST,
    ) -> bool {
        self.inner.object_matches(view, reg, object, selector)
    }

    fn player_matches(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        player: PlayerId,
        group: &PlayerGroup,
    ) -> bool {
        self.inner.player_matches(view, reg, player, group)
    }

    fn predicate_holds(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        pred: &PredicateAST,
        field: Option<&EventField>,
    ) -> bool {
        self.inner.predicate_holds(view, reg, pred, field)
    }

    fn condition_holds(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        _pred: &PredicateAST,
        bindings: &BTreeMap<String, EventField>,
    ) -> bool {
        self.seen.borrow_mut().push((
            view,
            bindings.clone(),
            reg.ability.base.ability_id.clone(),
            reg.ability.intervening_if.is_some(),
            reg.ability.limit.is_some(),
        ));
        true
    }

    fn is_visible_to_all(&self, object: ObjectId) -> bool {
        self.inner.is_visible_to_all(object)
    }
}

/// REQ-EVENT-002, CR 603.2h: with no intervening "if", `fire` never calls `condition_holds` and
/// the limit behaves as before.
#[test]
fn req_event_002_fire_without_condition_does_not_consult_env() {
    let mut d = DelayedTriggers::new();
    let id = d.register(
        o(1),
        p(1),
        limited_event_ability(None, Some(1)),
        BTreeMap::new(),
        false,
    );
    let env = BindEnv::default();
    assert!(d.fire(&env, 1, id).is_some());
    assert_eq!(d.fire(&env, 1, id), None);
    assert!(env.calls.borrow().is_empty());
}

/// REQ-EVENT-002, CR 603.4, CR 603.2h: with a condition present, `fire` and `take_triggered`
/// still share one counter, in both orders.
#[test]
fn req_event_002_fire_and_take_triggered_share_counter_with_condition() {
    let captured = want(&[("it", 40)]);
    let env = BindEnv {
        want: captured.clone(),
        ..BindEnv::default()
    };
    let w = [waiting(6, 1, "delayed#0", &[])];

    let mut d = DelayedTriggers::new();
    let id = d.register(
        o(6),
        p(2),
        limited_if_ability(Some(1), None),
        captured.clone(),
        false,
    );
    assert!(d.fire(&env, 3, id).is_some());
    assert!(d.take_triggered(&env, 3, &w).is_empty());
    assert_eq!(d.take_triggered(&env, 4, &w).len(), 1);

    let mut r = DelayedTriggers::new();
    let id = r.register(
        o(6),
        p(2),
        limited_if_ability(Some(1), None),
        captured,
        false,
    );
    assert_eq!(r.take_triggered(&env, 3, &w).len(), 1);
    assert_eq!(r.fire(&env, 3, id), None);
    assert!(r.fire(&env, 4, id).is_some());
}
