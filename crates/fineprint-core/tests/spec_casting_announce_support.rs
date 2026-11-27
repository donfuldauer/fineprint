// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared fake host and drivers for the casting announce spec tests (REQ-CAST-001,
//! REQ-NET-003, REQ-CORE-005, REQ-DSL-013, REQ-TEST-001; CR 601.2a-e, 601.3, 734.1, 734.2).
//! Black-box: written from the work-item brief only, against the pinned public API of
//! `fineprint_core::casting`.
//!
//! Each `spec_casting_announce*.rs` test file includes this file with `#[path]`; on its own it
//! is an empty test target. Nothing here names a real card (R5): objects are bare numbers, every
//! label is `test.*`, and the "qualities" are neutral strings.
//!
//! The fake host keeps every observable piece of its state in plain fields and hashes all of
//! them (its `Debug` text, which only contains `Vec`, `BTreeMap`, `BTreeSet` and the rule set,
//! itself built on `BTreeMap`; R3), so any change made by `put_on_stack` and not undone is
//! visible in `state_hash()`.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_core::casting::*;
use fineprint_core::choice::{
    ChoiceContext, ChoiceError, ChoicePayload, ChoiceResponse, ChoiceSession, ChoiceTarget,
    Interrupt, PendingChoiceRequest, Status, TargetSlot,
};
use fineprint_core::layers::{
    EffectOrigin, PlayerAffected, PostLayerEffects, PostLayerEnvironment, RuleEffect, RuleEntry,
    RuleSet, build_rule_set,
};
use fineprint_core::txn::Transactional;
use fineprint_dsl::ability::{RuleKind, RuleModification};
use fineprint_dsl::expr::PlayerSelectorAST;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

/// Player 1 (the default caster and the default priority holder).
pub const P1: PlayerId = PlayerId::new(1);
/// Player 2 (the default controller of every object, an opponent of P1).
pub const P2: PlayerId = PlayerId::new(2);

/// The raw id `put_on_stack` allocates first in a fresh fake.
pub const FIRST_STACK_ID: u64 = 100;

/// The object a spell request casts.
pub const SPELL_OBJECT: u64 = 5;
/// The source object of an ability request.
pub const ABILITY_SOURCE: u64 = 6;

/// A neutral source quality.
pub const QUALITY_A: &str = "test.quality_a";
/// A second neutral source quality.
pub const QUALITY_B: &str = "test.quality_b";

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn eid(n: u64) -> EffectId {
    EffectId::new(n)
}

/// `ChoiceTarget::Object` from a raw id.
pub fn to(n: u64) -> ChoiceTarget {
    ChoiceTarget::Object(oid(n))
}

/// `ChoiceTarget::Player` from a raw id.
pub fn tp(n: u64) -> ChoiceTarget {
    ChoiceTarget::Player(PlayerId::new(n))
}

pub fn other(p: PlayerId) -> PlayerId {
    if p == P1 { P2 } else { P1 }
}

// ---------------------------------------------------------------------------------------------
// requests
// ---------------------------------------------------------------------------------------------

/// A spell cast of [`SPELL_OBJECT`] by `caster`.
pub fn spell(caster: PlayerId) -> CastRequest {
    CastRequest {
        caster,
        object: oid(SPELL_OBJECT),
        kind: CastKind::Spell {
            face_index: 0,
            cast_mode: "test.cast_normal".to_owned(),
        },
    }
}

/// An activation of an ability of [`ABILITY_SOURCE`] by `caster`.
pub fn ability(caster: PlayerId) -> CastRequest {
    CastRequest {
        caster,
        object: oid(ABILITY_SOURCE),
        kind: CastKind::Ability {
            ability_id: "test.ability_a".to_owned(),
        },
    }
}

/// Both request kinds for `caster`, labelled.
pub fn both_kinds(caster: PlayerId) -> Vec<(&'static str, CastRequest)> {
    vec![("spell", spell(caster)), ("ability", ability(caster))]
}

/// The action string the begin check matches for this request (CR 601.3, 602.2).
pub fn action_of(req: &CastRequest) -> &'static str {
    match req.kind {
        CastKind::Spell { .. } => "CAST",
        CastKind::Ability { .. } => "ACTIVATE",
    }
}

// ---------------------------------------------------------------------------------------------
// rule sets
// ---------------------------------------------------------------------------------------------

/// The post-layer environment: two players, fixed player sets only.
pub struct TwoPlayers;

impl PostLayerEnvironment for TwoPlayers {
    fn players(&self) -> Vec<PlayerId> {
        vec![P1, P2]
    }

    fn matches_player(
        &self,
        _selector: &PlayerSelectorAST,
        _controller: PlayerId,
        _source: Option<ObjectId>,
        _candidate: PlayerId,
    ) -> bool {
        false
    }
}

/// One synthetic rule effect: id, kind, params (a JSON object), affected players, timestamp.
#[derive(Clone, Debug)]
pub struct R {
    pub id: u64,
    pub kind: RuleKind,
    pub params: Value,
    pub players: Vec<PlayerId>,
    pub ts: u64,
}

/// A rule effect description.
pub fn r(id: u64, kind: RuleKind, params: Value, players: &[PlayerId], ts: u64) -> R {
    R {
        id,
        kind,
        params,
        players: players.to_vec(),
        ts,
    }
}

/// A `TARGETING_RULE` entry (players both, which the convention ignores except for
/// IGNORE_HEXPROOF).
pub fn tr(id: u64, params: Value, ts: u64) -> R {
    r(id, RuleKind::TargetingRule, params, &[P1, P2], ts)
}

/// A `TARGETING_RULE` entry with an explicit `players` set.
pub fn tr_for(id: u64, params: Value, players: &[PlayerId], ts: u64) -> R {
    r(id, RuleKind::TargetingRule, params, players, ts)
}

/// A `PROHIBITION` entry for `players`.
pub fn prohibition(id: u64, params: Value, players: &[PlayerId], ts: u64) -> R {
    r(id, RuleKind::Prohibition, params, players, ts)
}

/// Hexproof (opponents only) on object `n` (CR 702.11b).
pub fn hexproof_obj(n: u64) -> Value {
    json!({"keyword": "HEXPROOF", "target_object": n, "sources": "OPPONENTS"})
}

/// Hexproof (opponents only) on player `n` (CR 702.11c).
pub fn hexproof_player(n: u64) -> Value {
    json!({"keyword": "HEXPROOF", "target_player": n, "sources": "OPPONENTS"})
}

/// Hexproof from `quality` on object `n` (CR 702.11d).
pub fn hexproof_from_obj(n: u64, quality: &str) -> Value {
    json!({"keyword": "HEXPROOF", "target_object": n, "sources": "OPPONENTS", "quality": quality})
}

/// Shroud on object `n` (CR 702.18a).
pub fn shroud_obj(n: u64) -> Value {
    json!({"keyword": "SHROUD", "target_object": n})
}

/// Shroud on player `n` (CR 702.18a).
pub fn shroud_player(n: u64) -> Value {
    json!({"keyword": "SHROUD", "target_player": n})
}

/// Protection from `quality` on object `n` (CR 702.16b).
pub fn protection_obj(n: u64, quality: &str) -> Value {
    json!({"keyword": "PROTECTION", "target_object": n, "quality": quality})
}

/// Protection from everything on object `n` (CR 702.16j).
pub fn protection_all_obj(n: u64) -> Value {
    json!({"keyword": "PROTECTION", "target_object": n})
}

/// Protection from player `p` on object `n` (CR 702.16k).
pub fn protection_from_player_obj(n: u64, p: u64) -> Value {
    json!({"keyword": "PROTECTION", "target_object": n, "source_player": p})
}

/// Some other "can't be the target" effect on object `n` (CR 101.2).
pub fn other_obj(n: u64) -> Value {
    json!({"keyword": "OTHER", "target_object": n})
}

/// "May target object `n` as though it didn't have hexproof" (CR 702.11e); the caster must be
/// in the entry's `players` set.
pub fn ignore_hexproof_obj(n: u64) -> Value {
    json!({"keyword": "HEXPROOF", "effect": "IGNORE_HEXPROOF", "target_object": n})
}

/// The params map of a JSON object (any other JSON value gives no params).
pub fn params_of(v: &Value) -> BTreeMap<String, Value> {
    match v {
        Value::Object(m) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => BTreeMap::new(),
    }
}

/// Builds the RuleSet of the given effects (CR 613.11, pass B).
pub fn rules(items: &[R]) -> RuleSet {
    let mut reg = PostLayerEffects::new();
    for it in items {
        let mut m = RuleModification::new(it.kind);
        m.params = params_of(&it.params);
        let effect = RuleEffect {
            source: Some(oid(900 + it.id)),
            controller: P2,
            timestamp: Timestamp::new(it.ts),
            origin: EffectOrigin::Static { granted_at: None },
            players: PlayerAffected::Fixed(it.players.iter().copied().collect::<BTreeSet<_>>()),
            modification: m,
        };
        reg.register_rule(eid(it.id), effect)
            .expect("register synthetic rule effect");
    }
    build_rule_set(&reg, &TwoPlayers)
}

// ---------------------------------------------------------------------------------------------
// specs
// ---------------------------------------------------------------------------------------------

/// A target slot spec (one instance of the word "target").
pub fn slot(min: u64, max: u64, candidates: &[ChoiceTarget]) -> TargetSlotSpec {
    TargetSlotSpec {
        min,
        max,
        candidates: candidates.to_vec(),
    }
}

/// A cost spec that offers nothing (no CHOOSE_COSTS_AND_X ask).
pub fn no_costs() -> CostSpec {
    CostSpec {
        alternative_costs: Vec::new(),
        additional_costs: Vec::new(),
        x_range: None,
        symbol_slots: Vec::new(),
        harmonize_candidates: Vec::new(),
    }
}

/// A cost spec with only an X range.
pub fn x_costs(lo: u64, hi: u64) -> CostSpec {
    CostSpec {
        x_range: Some((lo, hi)),
        ..no_costs()
    }
}

/// The cost choice when nothing is announced.
pub fn no_choice() -> CostChoice {
    CostChoice {
        alternative: None,
        additional: Vec::new(),
        x: None,
        symbol_options: Vec::new(),
        harmonize: None,
    }
}

/// A modal spec with `n` labels `test.mode_<i>`.
pub fn modes_spec(n: usize, min: u32, max: u32, repeats_allowed: bool) -> ModeSpec {
    ModeSpec {
        labels: mode_labels(n),
        min,
        max,
        repeats_allowed,
        mode_costs: Vec::new(),
    }
}

/// The labels of a [`modes_spec`].
pub fn mode_labels(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("test.mode_{i}")).collect()
}

// ---------------------------------------------------------------------------------------------
// the fake host
// ---------------------------------------------------------------------------------------------

/// A toy game state implementing [`Transactional`] and [`CastState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fake {
    /// The player with priority (CR 734.2).
    pub priority: Option<PlayerId>,
    /// Object raw id -> controller; any other object is controlled by P2.
    pub controllers: BTreeMap<u64, PlayerId>,
    /// The stack objects `put_on_stack` created, bottom first.
    pub stack: Vec<ObjectId>,
    /// The next raw id `put_on_stack` allocates.
    pub next_id: u64,
    /// The rule set the casting procedure reads.
    pub rules: RuleSet,
    /// `mode_spec` (the same for every request).
    pub mode_spec: Option<ModeSpec>,
    /// `cost_spec`.
    pub cost_spec: CostSpec,
    /// Target slots every proposal has, first.
    pub base_slots: Vec<TargetSlotSpec>,
    /// Target slots each chosen mode adds (indexed by ORIGINAL mode index), after the base
    /// slots, in chosen-mode order.
    pub mode_slots: Vec<Vec<TargetSlotSpec>>,
    /// Target slots added last when additional cost 0 is chosen (CR 601.2c, first example).
    pub additional_slots: Vec<TargetSlotSpec>,
    /// `division_total`.
    pub division: Option<u64>,
    /// The qualities `source_has_quality` reports for any object on this fake's stack.
    pub qualities: BTreeSet<String>,
    /// `proposal_combination_legal`.
    pub combination_legal: bool,
    /// An unrelated field (so two fakes can differ outside the casting inputs).
    pub marker: u64,
}

impl Default for Fake {
    fn default() -> Fake {
        Fake::new()
    }
}

impl Fake {
    /// No modes, no costs, no slots, no division, no rules, P1 has priority.
    pub fn new() -> Fake {
        Fake {
            priority: Some(P1),
            controllers: BTreeMap::new(),
            stack: Vec::new(),
            next_id: FIRST_STACK_ID,
            rules: RuleSet::default(),
            mode_spec: None,
            cost_spec: no_costs(),
            base_slots: Vec::new(),
            mode_slots: Vec::new(),
            additional_slots: Vec::new(),
            division: None,
            qualities: BTreeSet::new(),
            combination_legal: true,
            marker: 0,
        }
    }

    /// With one base slot.
    pub fn one_slot(min: u64, max: u64, candidates: &[ChoiceTarget]) -> Fake {
        Fake {
            base_slots: vec![slot(min, max, candidates)],
            ..Fake::new()
        }
    }

    pub fn with_rules(mut self, items: &[R]) -> Fake {
        self.rules = rules(items);
        self
    }

    pub fn with_qualities(mut self, qs: &[&str]) -> Fake {
        self.qualities = qs.iter().map(|q| (*q).to_owned()).collect();
        self
    }

    pub fn with_controller(mut self, object: u64, controller: PlayerId) -> Fake {
        self.controllers.insert(object, controller);
        self
    }
}

/// Reads a boolean param of an entry (`false` when absent or not a boolean).
fn flag(entry: &RuleEntry, key: &str) -> bool {
    entry.modification.params.get(key).and_then(Value::as_bool) == Some(true)
}

impl Transactional for Fake {
    fn state_hash(&self) -> Digest {
        sha256(format!("{self:?}").as_bytes())
    }

    fn priority_player(&self) -> Option<PlayerId> {
        self.priority
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

impl CastState for Fake {
    fn rule_set(&self) -> &RuleSet {
        &self.rules
    }

    fn are_opponents(&self, a: PlayerId, b: PlayerId) -> bool {
        a != b
    }

    fn target_controller(&self, t: ChoiceTarget) -> PlayerId {
        match t {
            ChoiceTarget::Player(p) => p,
            ChoiceTarget::Object(o) => self.controllers.get(&o.get()).copied().unwrap_or(P2),
        }
    }

    fn mode_spec(&self, _req: &CastRequest) -> Option<ModeSpec> {
        self.mode_spec.clone()
    }

    fn cost_spec(&self, _req: &CastRequest, _modes: &[u32]) -> CostSpec {
        self.cost_spec.clone()
    }

    fn target_slots(
        &self,
        _req: &CastRequest,
        modes: &[u32],
        costs: &CostChoice,
    ) -> Vec<TargetSlotSpec> {
        let mut out = self.base_slots.clone();
        for &m in modes {
            if let Some(extra) = self.mode_slots.get(m as usize) {
                out.extend(extra.iter().cloned());
            }
        }
        if costs.additional.contains(&0) {
            out.extend(self.additional_slots.iter().cloned());
        }
        out
    }

    fn division_total(&self, _req: &CastRequest, _proposal: &Proposal) -> Option<u64> {
        self.division
    }

    fn put_on_stack(&mut self, _req: &CastRequest) -> ObjectId {
        let id = oid(self.next_id);
        self.next_id += 1;
        self.stack.push(id);
        id
    }

    fn source_has_quality(
        &self,
        _req: &CastRequest,
        stack_object: ObjectId,
        quality: &str,
    ) -> bool {
        // CR 113.7a: the source is checked once the object is on the stack.
        self.stack.contains(&stack_object) && self.qualities.contains(quality)
    }

    /// Fake step-5 test: `"test.applies": true`, or `"test.parity": "even" | "odd"` compared
    /// with the announced X (no X: does not apply).
    fn prohibition_applies(
        &self,
        entry: &RuleEntry,
        _req: &CastRequest,
        _stack_object: ObjectId,
        proposal: &Proposal,
    ) -> bool {
        if flag(entry, "test.applies") {
            return true;
        }
        match (
            entry
                .modification
                .params
                .get("test.parity")
                .and_then(Value::as_str),
            proposal.costs.x,
        ) {
            (Some("even"), Some(x)) => x % 2 == 0,
            (Some("odd"), Some(x)) => x % 2 == 1,
            _ => false,
        }
    }

    /// Fake 601.3a test: `"test.could_lapse": true`, or any `"test.parity"` param (the chosen X
    /// may change the parity).
    fn prohibition_could_lapse(&self, entry: &RuleEntry, _req: &CastRequest) -> bool {
        flag(entry, "test.could_lapse") || entry.modification.params.contains_key("test.parity")
    }

    fn proposal_combination_legal(
        &self,
        _req: &CastRequest,
        _stack_object: ObjectId,
        _proposal: &Proposal,
    ) -> bool {
        self.combination_legal
    }
}

// ---------------------------------------------------------------------------------------------
// drivers
// ---------------------------------------------------------------------------------------------

/// What the direct harness observes inside one run of the procedure: the state hash and
/// priority right before and right after the `announce` call on the same state, and the
/// outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observed {
    pub before: Digest,
    pub after: Digest,
    pub priority_before: Option<PlayerId>,
    pub priority_after: Option<PlayerId>,
    pub outcome: AnnounceOutcome,
}

/// The session procedure: calls `announce` directly on the run's state with the run's
/// `ChoiceContext` and records the state around the call.
pub fn procedure(
    req: CastRequest,
) -> impl Fn(&mut Fake, &mut ChoiceContext<'_>) -> Result<Observed, Interrupt> {
    move |s: &mut Fake, ctx: &mut ChoiceContext<'_>| {
        let before = s.state_hash();
        let priority_before = s.priority_player();
        let outcome = announce(s, ctx, &req)?;
        Ok(Observed {
            before,
            after: s.state_hash(),
            priority_before,
            priority_after: s.priority_player(),
            outcome,
        })
    }
}

/// The boxed session procedure.
pub type Proc = Box<dyn Fn(&mut Fake, &mut ChoiceContext<'_>) -> Result<Observed, Interrupt>>;

/// A session type over the fake.
pub type Session = ChoiceSession<Fake, Observed, Proc>;

/// The boxed procedure (so sessions have a nameable type).
pub fn boxed(req: &CastRequest) -> Proc {
    Box::new(procedure(req.clone()))
}

/// Starts a session.
pub fn start(fake: &Fake, req: &CastRequest) -> Session {
    ChoiceSession::start(fake.clone(), boxed(req)).expect("session starts")
}

/// Replays `answers` in one call.
pub fn replay(
    fake: &Fake,
    req: &CastRequest,
    answers: &[ChoiceResponse],
) -> Result<Session, ChoiceError> {
    ChoiceSession::replay(fake.clone(), boxed(req), answers)
}

/// A finished run: every request asked (in order), the final state and the observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub requests: Vec<PendingChoiceRequest>,
    pub state: Fake,
    pub final_hash: Digest,
    pub observed: Observed,
    pub answers: Vec<ChoiceResponse>,
}

impl Run {
    pub fn outcome(&self) -> &AnnounceOutcome {
        &self.observed.outcome
    }
}

/// Drives a session answering each pending request with the next answer (submitted one at a
/// time); panics if an answer is rejected or the answers run out while a request is pending,
/// or if answers remain once the procedure is done.
pub fn drive(fake: &Fake, req: &CastRequest, answers: &[ChoiceResponse]) -> Run {
    let mut session = start(fake, req);
    let mut requests = Vec::new();
    let mut next = answers.iter();
    loop {
        match session.status() {
            Status::Pending(p) => {
                let p = p.clone();
                // While pending, the visible state is the untouched base (REQ-NET-003).
                assert_eq!(
                    session.state_hash(),
                    fake.state_hash(),
                    "the base hash changed while request {} was pending",
                    p.request_id
                );
                let Some(a) = next.next() else {
                    panic!(
                        "out of answers at request {} ({}): {:?}",
                        p.request_id, p.choice_type, p.payload
                    );
                };
                requests.push(p.clone());
                if let Err(e) = session.submit(p.request_id, a.clone()) {
                    panic!("answer {a:?} to {:?} rejected: {e}", p.payload);
                }
            }
            Status::Done { state, output } => {
                assert!(
                    next.next().is_none(),
                    "answers remain after the procedure finished with {:?}",
                    output.outcome
                );
                return Run {
                    requests,
                    state: state.clone(),
                    final_hash: session.state_hash(),
                    observed: output.clone(),
                    answers: session.answers().to_vec(),
                };
            }
        }
    }
}

/// The pending request of a session, if any.
pub fn pending_of(
    fake: &Fake,
    req: &CastRequest,
    answers: &[ChoiceResponse],
) -> PendingChoiceRequest {
    let s = replay(fake, req, answers).expect("replay succeeds");
    s.pending().cloned().expect("a request is pending")
}

/// The target slots of a `CHOOSE_TARGETS` request.
pub fn target_slots_of(p: &PendingChoiceRequest) -> Vec<TargetSlot> {
    match &p.payload {
        ChoicePayload::ChooseTargets { slots, .. } => slots.clone(),
        other => panic!("expected ChooseTargets, got {other:?}"),
    }
}

/// The `legal` lists of the first request, which must be `CHOOSE_TARGETS` (a fake with no
/// modes and no costs).
pub fn legal_lists(fake: &Fake, req: &CastRequest) -> Vec<Vec<ChoiceTarget>> {
    let s = start(fake, req);
    let Some(p) = s.pending() else {
        panic!(
            "expected a CHOOSE_TARGETS request, got {:?}",
            match s.status() {
                Status::Done { output, .. } => format!("{:?}", output.outcome),
                Status::Pending(_) => String::new(),
            }
        );
    };
    target_slots_of(p).into_iter().map(|s| s.legal).collect()
}

/// The `legal` list of the only slot.
pub fn legal(fake: &Fake, req: &CastRequest) -> Vec<ChoiceTarget> {
    let mut lists = legal_lists(fake, req);
    assert_eq!(lists.len(), 1, "expected exactly one slot");
    lists.remove(0)
}

// ---------------------------------------------------------------------------------------------
// answers
// ---------------------------------------------------------------------------------------------

pub fn select_modes(modes: &[u32]) -> ChoiceResponse {
    ChoiceResponse::SelectModes {
        modes: modes.to_vec(),
    }
}

pub fn configure(c: &CostChoice) -> ChoiceResponse {
    ChoiceResponse::ConfigureCosts {
        alternative: c.alternative,
        additional: c.additional.clone(),
        x: c.x,
        symbol_options: c.symbol_options.clone(),
        harmonize: c.harmonize,
    }
}

/// `ConfigureCosts` with only X.
pub fn with_x(x: u64) -> ChoiceResponse {
    configure(&CostChoice {
        x: Some(x),
        ..no_choice()
    })
}

pub fn select_targets(t: &[&[ChoiceTarget]]) -> ChoiceResponse {
    ChoiceResponse::SelectTargets {
        targets: t.iter().map(|s| s.to_vec()).collect(),
    }
}

pub fn submit_division(a: &[(ChoiceTarget, u64)]) -> ChoiceResponse {
    ChoiceResponse::SubmitDivision {
        assignments: a.to_vec(),
    }
}

// ---------------------------------------------------------------------------------------------
// assertions
// ---------------------------------------------------------------------------------------------

/// Asserts the run rolled back at `step` for `reason` to exactly the base state (CR 601.2,
/// 734.1, 734.2; REQ-CORE-005): `RolledBack.state_hash` and the final hash equal the base hash,
/// the state equals the base (the stack object is gone), priority is unchanged.
pub fn assert_rolled_back(run: &Run, base: &Fake, step: CastStep, reason: StopReason) {
    let AnnounceOutcome::RolledBack(rb) = run.outcome() else {
        panic!(
            "expected RolledBack({step:?}, {reason:?}), got {:?}",
            run.outcome()
        );
    };
    assert_eq!(rb.step, step, "rollback step");
    assert_eq!(rb.reason, reason, "rollback reason");
    assert_eq!(rb.state_hash, base.state_hash(), "RolledBack.state_hash");
    assert_eq!(
        rb.priority_player, base.priority,
        "RolledBack.priority_player"
    );
    assert_eq!(run.final_hash, base.state_hash(), "final hash");
    assert_eq!(
        run.observed.after, run.observed.before,
        "hash after the call"
    );
    assert_eq!(
        run.observed.priority_after, base.priority,
        "priority after the call"
    );
    assert_eq!(
        run.state.stack, base.stack,
        "the stack object was not removed"
    );
    assert_eq!(&run.state, base, "the state was not restored");
}

/// Asserts the run did not start (CR 601.3) and changed nothing.
pub fn assert_not_started(run: &Run, base: &Fake, by: EffectId) {
    assert_eq!(
        run.outcome(),
        &AnnounceOutcome::NotStarted {
            reason: StopReason::Prohibited { by }
        }
    );
    assert!(
        run.requests.is_empty(),
        "a request was asked: {:?}",
        run.requests
    );
    assert_eq!(&run.state, base, "the state changed");
    assert_eq!(run.final_hash, base.state_hash());
}

/// Unwraps `Announced` and checks the stack object is the fake's newly created one.
pub fn announced(run: &Run, base: &Fake) -> (ObjectId, Proposal) {
    let AnnounceOutcome::Announced {
        stack_object,
        proposal,
    } = run.outcome()
    else {
        panic!("expected Announced, got {:?}", run.outcome());
    };
    assert_eq!(
        *stack_object,
        oid(base.next_id),
        "the stack object is the new one"
    );
    assert_eq!(
        run.state.stack.last(),
        Some(stack_object),
        "the stack object is on top"
    );
    assert_eq!(run.state.stack.len(), base.stack.len() + 1);
    assert_eq!(run.state.priority, base.priority);
    (*stack_object, proposal.clone())
}
