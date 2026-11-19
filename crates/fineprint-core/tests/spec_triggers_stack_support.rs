// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the trigger-stack spec tests (REQ-EVENT-002, REQ-ARCH-002; CR 603.3b-d,
//! 603.5, 603.7, 603.8, 603.12, 113.8). Black-box: written from the work-item brief only.
//!
//! Each `spec_triggers_stack_*.rs` file includes this file with `#[path]`. Nothing here is card
//! data (R5): abilities are synthetic (`test.trigger_*`), ids are numbers and predicates are
//! opaque tags the fake environments answer.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::choice::{
    ChoiceContext, ChoiceError, ChoicePayload, ChoiceResponse, ChoiceSession, ChoiceTarget,
    ChoiceType, PendingChoiceRequest, Status,
};
use fineprint_core::triggers::{
    EventField, PendingTrigger, PlacementEnv, StackedTrigger, StateView, TriggerEnv, TriggerOrigin,
    TriggerPlacement, TriggerRegistration, WaitingTrigger, place_triggers,
};
use fineprint_core::txn::Transactional;
use fineprint_dsl::ability::{
    AbilityBase, EventKind, EventMatcher, ModeAST, ModeSetAST, TargetSlotAST, Trigger,
    TriggeredAbilityAST,
};
use fineprint_dsl::expr::{ObjectSelectorAST, PlayerGroup, PredicateAST, SelectorFilter};

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);
pub const P4: PlayerId = PlayerId::new(4);

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn obj(n: u64) -> ChoiceTarget {
    ChoiceTarget::Object(oid(n))
}

pub fn ply(p: PlayerId) -> ChoiceTarget {
    ChoiceTarget::Player(p)
}

pub fn s(x: &str) -> String {
    x.to_string()
}

// ---------------------------------------------------------------------------------------------
// opaque DSL values
// ---------------------------------------------------------------------------------------------

/// A fixed object selector; no fake ever looks at it.
pub fn sel() -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(PredicateAST::IsCreature)),
    })
}

/// An opaque predicate carrying a test-chosen tag.
pub fn tag_pred(tag: &str) -> PredicateAST {
    PredicateAST::HasKeyword {
        keyword: tag.to_string(),
    }
}

/// The tag of a predicate built by [`tag_pred`]; `"?"` for anything else.
pub fn tag_of(pred: &PredicateAST) -> String {
    match pred {
        PredicateAST::HasKeyword { keyword } => keyword.clone(),
        _ => "?".to_string(),
    }
}

/// A matcher with only `kind`.
pub fn m(kind: EventKind) -> EventMatcher {
    EventMatcher::new(kind)
}

/// `mm` with one more binding (`field` -> `var`).
pub fn bind(mut mm: EventMatcher, field: &str, var: &str) -> EventMatcher {
    mm.bind
        .get_or_insert_with(BTreeMap::new)
        .insert(field.to_string(), var.to_string());
    mm
}

/// A triggered ability with id `id` and trigger `trigger`, nothing else set.
pub fn ability_with(id: &str, trigger: Trigger) -> TriggeredAbilityAST {
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

/// A triggered ability with id `id` that triggers on events of `kind`.
pub fn ev_ability(id: &str, kind: EventKind) -> TriggeredAbilityAST {
    ability_with(id, Trigger::Event(m(kind)))
}

/// A state-triggered ability (CR 603.8) whose condition is the tag `cond`.
pub fn state_ability(id: &str, cond: &str) -> TriggeredAbilityAST {
    ability_with(id, Trigger::State(tag_pred(cond)))
}

/// `a` with an intervening-if clause tagged `tag` (CR 603.4).
pub fn with_if(mut a: TriggeredAbilityAST, tag: &str) -> TriggeredAbilityAST {
    a.intervening_if = Some(tag_pred(tag));
    a
}

/// A target slot.
pub fn slot(n: u32, min: u32, max: u32) -> TargetSlotAST {
    TargetSlotAST {
        slot: n,
        selector: sel(),
        min,
        max,
    }
}

/// A mode with id `id` and its own target slots (none if empty).
pub fn mode(id: &str, targets: Vec<TargetSlotAST>) -> ModeAST {
    ModeAST {
        id: id.to_string(),
        targets: if targets.is_empty() {
            None
        } else {
            Some(targets)
        },
        effect: Vec::new(),
    }
}

/// A mode set.
pub fn modeset(min: u32, max: u32, modes: Vec<ModeAST>) -> ModeSetAST {
    ModeSetAST { min, max, modes }
}

pub fn reg(source: u64, controller: PlayerId, ability: TriggeredAbilityAST) -> TriggerRegistration {
    TriggerRegistration {
        source: oid(source),
        controller,
        ability,
    }
}

/// A waiting trigger literal.
pub fn wt(
    source: u64,
    controller: PlayerId,
    id: &str,
    bindings: BTreeMap<String, EventField>,
) -> WaitingTrigger {
    WaitingTrigger {
        source: oid(source),
        controller,
        ability_id: id.to_string(),
        look_back: None,
        event_indices: vec![0],
        bindings,
    }
}

// ---------------------------------------------------------------------------------------------
// pending triggers
// ---------------------------------------------------------------------------------------------

/// A part-1 event trigger with no targets, modes or option.
pub fn pt(source: u64, controller: PlayerId, id: &str) -> PendingTrigger {
    PendingTrigger {
        origin: TriggerOrigin::Event,
        controller,
        source: Some(oid(source)),
        ability_id: id.to_string(),
        bindings: BTreeMap::new(),
        optional: false,
        responds_to_trigger: false,
        targets: Vec::new(),
        modes: None,
    }
}

/// A part-2 trigger (its trigger condition is another ability triggering, CR 603.3b).
pub fn pt2(source: u64, controller: PlayerId, id: &str) -> PendingTrigger {
    PendingTrigger {
        responds_to_trigger: true,
        ..pt(source, controller, id)
    }
}

/// `t` with ability-level target slots.
pub fn targeted(mut t: PendingTrigger, slots: Vec<TargetSlotAST>) -> PendingTrigger {
    t.targets = slots;
    t
}

/// `t` with modes.
pub fn modal(mut t: PendingTrigger, ms: ModeSetAST) -> PendingTrigger {
    t.modes = Some(ms);
    t
}

/// `t` made optional ("may", CR 603.5).
pub fn optional(mut t: PendingTrigger) -> PendingTrigger {
    t.optional = true;
    t
}

/// The ORDER payload text the contract fixes for a trigger with a source.
pub fn desc_obj(source: u64, id: &str) -> String {
    format!("object {source} ability {id}")
}

/// The ORDER payload text the contract fixes for a sourceless trigger.
pub fn desc_player(p: PlayerId, id: &str) -> String {
    format!("player {} ability {id}", p.get())
}

// ---------------------------------------------------------------------------------------------
// a scripted placement environment
// ---------------------------------------------------------------------------------------------

/// One recorded `legal_targets` call: (ability id, mode, slot, ability ids stacked so far).
pub type TargetCall = (String, Option<usize>, usize, Vec<String>);

/// A [`PlacementEnv`] that answers from a table keyed on (ability id, mode, slot); anything
/// absent has no legal target. Every call is recorded (across re-executions).
#[derive(Debug, Default)]
pub struct ScriptEnv {
    pub legal: BTreeMap<(String, Option<usize>, usize), Vec<ChoiceTarget>>,
    pub calls: RefCell<Vec<TargetCall>>,
}

impl ScriptEnv {
    pub fn new() -> ScriptEnv {
        ScriptEnv::default()
    }

    /// Legal targets for an ability-level slot.
    pub fn slot(mut self, id: &str, slot: usize, legal: Vec<ChoiceTarget>) -> ScriptEnv {
        self.legal.insert((id.to_string(), None, slot), legal);
        self
    }

    /// Legal targets for slot `slot` of original mode `mode`.
    pub fn mode_slot(
        mut self,
        id: &str,
        mode: usize,
        slot: usize,
        legal: Vec<ChoiceTarget>,
    ) -> ScriptEnv {
        self.legal.insert((id.to_string(), Some(mode), slot), legal);
        self
    }

    /// Whether some recorded call matches.
    pub fn saw(&self, id: &str, mode: Option<usize>, slot: usize, stacked: &[&str]) -> bool {
        let stacked: Vec<String> = stacked.iter().map(|x| x.to_string()).collect();
        self.calls
            .borrow()
            .iter()
            .any(|c| c.0 == id && c.1 == mode && c.2 == slot && c.3 == stacked)
    }

    /// All recorded calls for ability `id`.
    pub fn calls_for(&self, id: &str) -> Vec<TargetCall> {
        self.calls
            .borrow()
            .iter()
            .filter(|c| c.0 == id)
            .cloned()
            .collect()
    }
}

impl PlacementEnv for ScriptEnv {
    fn legal_targets(
        &self,
        trigger: &PendingTrigger,
        mode: Option<usize>,
        slot: usize,
        stacked_so_far: &[StackedTrigger],
    ) -> Vec<ChoiceTarget> {
        self.calls.borrow_mut().push((
            trigger.ability_id.clone(),
            mode,
            slot,
            stacked_so_far
                .iter()
                .map(|t| t.trigger.ability_id.clone())
                .collect(),
        ));
        self.legal
            .get(&(trigger.ability_id.clone(), mode, slot))
            .cloned()
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------------------------
// a blank transactional state for ChoiceSession
// ---------------------------------------------------------------------------------------------

/// A state with nothing in it: placement reads only its environment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Blank;

impl Transactional for Blank {
    fn state_hash(&self) -> Digest {
        sha256(b"blank")
    }

    fn priority_player(&self) -> Option<PlayerId> {
        None
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

// ---------------------------------------------------------------------------------------------
// driving place_triggers through a ChoiceSession
// ---------------------------------------------------------------------------------------------

/// The outcome of one driven placement.
#[derive(Debug)]
pub struct Driven {
    /// Every request published, in order (the last one is still pending if `placement` is None).
    pub requests: Vec<PendingChoiceRequest>,
    /// The answers given, in order.
    pub answers: Vec<ChoiceResponse>,
    /// The final placement, once done.
    pub placement: Option<TriggerPlacement>,
}

impl Driven {
    pub fn placement(&self) -> &TriggerPlacement {
        self.placement.as_ref().expect("placement finished")
    }

    pub fn types(&self) -> Vec<ChoiceType> {
        self.requests.iter().map(|r| r.choice_type).collect()
    }

    pub fn of_type(&self, t: ChoiceType) -> Vec<&PendingChoiceRequest> {
        self.requests
            .iter()
            .filter(|r| r.choice_type == t)
            .collect()
    }
}

/// Runs `place_triggers` in a session, answering each pending request with `answer`. Every
/// published request is checked for sequential ids and a default that validates.
pub fn drive_with<E, A>(
    env: &E,
    apnap: &[PlayerId],
    waiting: &[PendingTrigger],
    mut answer: A,
) -> Driven
where
    E: PlacementEnv,
    A: FnMut(&PendingChoiceRequest) -> ChoiceResponse,
{
    let procedure = |_s: &mut Blank, ctx: &mut ChoiceContext<'_>| {
        place_triggers(env, ctx, apnap, waiting.to_vec())
    };
    let mut session = ChoiceSession::start(Blank, procedure).expect("placement starts");
    let mut requests = Vec::new();
    let mut answers = Vec::new();
    loop {
        let pending = match session.status() {
            Status::Done { output, .. } => {
                return Driven {
                    requests,
                    answers,
                    placement: Some(output.clone()),
                };
            }
            Status::Pending(r) => r.clone(),
        };
        assert_eq!(
            pending.request_id.get(),
            requests.len() as u64,
            "request ids are sequential from 0"
        );
        assert!(
            pending.validate(&pending.default_on_timeout).is_ok(),
            "default_on_timeout validates: {pending:?}"
        );
        let a = answer(&pending);
        requests.push(pending.clone());
        if let Err(e) = session.submit(pending.request_id, a.clone()) {
            panic!("answer {a:?} to {pending:?} rejected: {e:?}");
        }
        answers.push(a);
        assert!(requests.len() < 1000, "runaway placement");
    }
}

/// Runs placement answering with the scripted list in order; panics if it runs out.
pub fn drive(
    env: &impl PlacementEnv,
    apnap: &[PlayerId],
    waiting: &[PendingTrigger],
    script: &[ChoiceResponse],
) -> Driven {
    let mut i = 0;
    let d = drive_with(env, apnap, waiting, |r| {
        let a = script
            .get(i)
            .unwrap_or_else(|| panic!("no scripted answer for {r:?}"))
            .clone();
        i += 1;
        a
    });
    assert_eq!(i, script.len(), "every scripted answer was used");
    d
}

/// Runs placement answering every request with its default.
pub fn drive_default(
    env: &impl PlacementEnv,
    apnap: &[PlayerId],
    waiting: &[PendingTrigger],
) -> Driven {
    drive_with(env, apnap, waiting, |r| r.default_on_timeout.clone())
}

/// Starts a placement session and returns its result as-is (for error cases).
pub fn start_placement(
    env: &impl PlacementEnv,
    apnap: &[PlayerId],
    waiting: &[PendingTrigger],
) -> Result<Option<PendingChoiceRequest>, ChoiceError> {
    let procedure = |_s: &mut Blank, ctx: &mut ChoiceContext<'_>| {
        place_triggers(env, ctx, apnap, waiting.to_vec())
    };
    ChoiceSession::start(Blank, procedure).map(|s| s.pending().cloned())
}

/// `ORDER_TRIGGERS` with the given order.
pub fn order(o: &[u32]) -> ChoiceResponse {
    ChoiceResponse::OrderTriggers { order: o.to_vec() }
}

/// `SELECT_MODES` with the given request-list indices.
pub fn modes(o: &[u32]) -> ChoiceResponse {
    ChoiceResponse::SelectModes { modes: o.to_vec() }
}

/// `SELECT_TARGETS` with the given lists.
pub fn targets(t: Vec<Vec<ChoiceTarget>>) -> ChoiceResponse {
    ChoiceResponse::SelectTargets { targets: t }
}

/// The ability ids of the stacked triggers, bottom first.
pub fn stacked_ids(p: &TriggerPlacement) -> Vec<String> {
    p.stacked
        .iter()
        .map(|t| t.trigger.ability_id.clone())
        .collect()
}

/// The ability ids of the removed triggers, in removal order.
pub fn removed_ids(p: &TriggerPlacement) -> Vec<String> {
    p.removed
        .iter()
        .map(|t| t.trigger.ability_id.clone())
        .collect()
}

/// The ORDER payload's descriptions, panicking on another payload.
pub fn order_payload(r: &PendingChoiceRequest) -> Vec<String> {
    match &r.payload {
        ChoicePayload::OrderSimultaneousTriggers { triggers } => triggers.clone(),
        other => panic!("not an ORDER payload: {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// a scripted TriggerEnv (state triggers, delayed end to end)
// ---------------------------------------------------------------------------------------------

/// One recorded `condition_holds` call: view, tag, bindings.
pub type CondCall = (StateView, String, BTreeMap<String, EventField>);

/// A [`TriggerEnv`] listing `post` (and `pre`) registrations; `condition_holds` is true for the
/// (view, tag) pairs in `conds`; every object and player matches; no predicate holds.
#[derive(Debug, Default)]
pub struct CondEnv {
    pub pre: Vec<TriggerRegistration>,
    pub post: Vec<TriggerRegistration>,
    pub conds: BTreeSet<(StateView, String)>,
    pub cond_calls: RefCell<Vec<CondCall>>,
}

impl CondEnv {
    pub fn new() -> CondEnv {
        CondEnv::default()
    }

    pub fn with_post(mut self, r: TriggerRegistration) -> CondEnv {
        self.post.push(r);
        self
    }

    pub fn with_pre(mut self, r: TriggerRegistration) -> CondEnv {
        self.pre.push(r);
        self
    }

    pub fn set(&mut self, tag: &str, holds: bool) {
        let key = (StateView::PostEvent, tag.to_string());
        if holds {
            self.conds.insert(key);
        } else {
            self.conds.remove(&key);
        }
    }

    pub fn cond_true(mut self, tag: &str) -> CondEnv {
        self.set(tag, true);
        self
    }
}

impl TriggerEnv for CondEnv {
    fn registrations(&self, view: StateView) -> Vec<TriggerRegistration> {
        match view {
            StateView::PreEvent => self.pre.clone(),
            StateView::PostEvent => self.post.clone(),
        }
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
        _reg: &TriggerRegistration,
        pred: &PredicateAST,
        bindings: &BTreeMap<String, EventField>,
    ) -> bool {
        let tag = tag_of(pred);
        self.cond_calls
            .borrow_mut()
            .push((view, tag.clone(), bindings.clone()));
        self.conds.contains(&(view, tag))
    }

    fn is_visible_to_all(&self, _object: ObjectId) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------------------------
// deterministic pseudo-random numbers for property tests (R3: seeded, no system source)
// ---------------------------------------------------------------------------------------------

/// A 64-bit linear congruential generator (Knuth MMIX constants).
#[derive(Clone, Debug)]
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    /// A value in `0..n` (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub fn idx(&mut self, n: usize) -> usize {
        usize::try_from(self.below(n as u64)).expect("fits")
    }

    pub fn coin(&mut self) -> bool {
        self.below(2) == 0
    }

    /// Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.idx(i + 1);
            items.swap(i, j);
        }
    }

    /// A random permutation of `0..n`.
    pub fn perm(&mut self, n: usize) -> Vec<u32> {
        let mut v: Vec<u32> = (0..n as u32).collect();
        self.shuffle(&mut v);
        v
    }
}
