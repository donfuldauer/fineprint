// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the announce procedure, steps 0-5 of casting and activation: REQ-CAST-001
// (steps and rollback), REQ-NET-003 (CHOOSE_MODES, CHOOSE_COSTS_AND_X, CHOOSE_TARGETS,
// DIVIDE_VALUE), REQ-CORE-005 (rollback to the identical hash, priority kept), REQ-DSL-013
// (targeting consults only TARGETING_RULE entries and host query points), REQ-CORE-001
// (replay determinism). Synthetic ids, synthetic rule effects and neutral labels only (R5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::ability::{RuleKind, RuleModification};
use fineprint_dsl::expr::PlayerSelectorAST;
use serde_json::{Value, json};

use super::*;
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOptions, ChoicePayload, ChoiceResponse, ChoiceSession,
    ChoiceTarget, ChoiceType, ENUMERATION_LIMIT, Interrupt, PendingChoiceRequest, Status,
};
use crate::layers::{
    EffectOrigin, PlayerAffected, PostLayerEffects, PostLayerEnvironment, RuleEffect, RuleEntry,
    RuleSet, build_rule_set,
};
use crate::txn::Transactional;

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn tp(n: u64) -> ChoiceTarget {
    ChoiceTarget::Player(p(n))
}

fn to(n: u64) -> ChoiceTarget {
    ChoiceTarget::Object(o(n))
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

/// Three players, every selector matches.
struct Env;

impl PostLayerEnvironment for Env {
    fn players(&self) -> Vec<PlayerId> {
        vec![p(1), p(2), p(3)]
    }

    fn matches_player(
        &self,
        _selector: &PlayerSelectorAST,
        _controller: PlayerId,
        _source: Option<ObjectId>,
        _candidate: PlayerId,
    ) -> bool {
        true
    }
}

/// One synthetic rule effect: id, timestamp, kind, players and params.
#[derive(Clone)]
struct Fx {
    id: u64,
    ts: u64,
    kind: RuleKind,
    players: Vec<u64>,
    params: Vec<(String, Value)>,
}

fn fx(id: u64, ts: u64, kind: RuleKind) -> Fx {
    Fx {
        id,
        ts,
        kind,
        players: vec![1],
        params: Vec::new(),
    }
}

impl Fx {
    fn with(mut self, key: &str, v: impl Into<Value>) -> Fx {
        self.params.push((key.to_owned(), v.into()));
        self
    }

    fn for_players(mut self, players: &[u64]) -> Fx {
        self.players = players.to_vec();
        self
    }
}

/// A `PROHIBITION` on `action` for player 1.
fn prohibit(id: u64, ts: u64, action: &str) -> Fx {
    fx(id, ts, RuleKind::Prohibition).with("action", action)
}

/// A `TARGETING_RULE` with `keyword` protecting `target`.
fn targeting(id: u64, ts: u64, keyword: &str, target: ChoiceTarget) -> Fx {
    let f = fx(id, ts, RuleKind::TargetingRule).with("keyword", keyword);
    match target {
        ChoiceTarget::Object(x) => f.with("target_object", x.get()),
        ChoiceTarget::Player(x) => f.with("target_player", x.get()),
    }
}

fn rules(list: &[Fx]) -> RuleSet {
    let mut reg = PostLayerEffects::new();
    for f in list {
        let mut m = RuleModification::new(f.kind);
        for (k, v) in &f.params {
            m.params.insert(k.clone(), v.clone());
        }
        reg.register_rule(
            EffectId::new(f.id),
            RuleEffect {
                source: Some(o(900)),
                controller: p(2),
                timestamp: Timestamp::new(f.ts),
                origin: EffectOrigin::Static { granted_at: None },
                players: PlayerAffected::Fixed(f.players.iter().map(|&n| p(n)).collect()),
                modification: m,
            },
        )
        .expect("register rule");
    }
    build_rule_set(&reg, &Env)
}

/// A toy host: everything is a field. Players 1 and 3 are teammates; player 2 opposes both.
#[derive(Clone, Debug)]
struct Toy {
    rules: RuleSet,
    priority: Option<PlayerId>,
    /// Team of each player: different teams are opponents.
    teams: BTreeMap<PlayerId, u8>,
    /// Controller of each object.
    controllers: BTreeMap<ObjectId, PlayerId>,
    /// Qualities per object (spell qualities are copied to the stack object).
    qualities: BTreeMap<ObjectId, BTreeSet<String>>,
    modes: Option<ModeSpec>,
    costs: CostSpec,
    /// Slots every proposal has.
    base_slots: Vec<TargetSlotSpec>,
    /// Extra slots per chosen mode (original index).
    mode_slots: BTreeMap<u32, Vec<TargetSlotSpec>>,
    /// Extra slots when any additional cost is chosen.
    kicker_slots: Vec<TargetSlotSpec>,
    /// The amount to divide, if any.
    division: Option<u64>,
    /// The spell's mana value before X.
    base_mana_value: u64,
    /// The stack, bottom first.
    stack: Vec<ObjectId>,
    next_object: u64,
    /// Whether the same target in two slots is an illegal combination.
    forbid_cross_slot_repeat: bool,
    /// Log of `put_on_stack` calls (proves rollback undoes host mutation).
    log: Vec<String>,
}

impl Toy {
    fn new() -> Toy {
        let mut teams = BTreeMap::new();
        teams.insert(p(1), 0);
        teams.insert(p(2), 1);
        teams.insert(p(3), 0);
        let mut controllers = BTreeMap::new();
        for (obj, ctl) in [
            (10, 1),
            (11, 1),
            (20, 2),
            (21, 2),
            (22, 2),
            (30, 3),
            (50, 1),
        ] {
            controllers.insert(o(obj), p(ctl));
        }
        Toy {
            rules: RuleSet::default(),
            priority: Some(p(1)),
            teams,
            controllers,
            qualities: BTreeMap::new(),
            modes: None,
            costs: CostSpec::default(),
            base_slots: Vec::new(),
            mode_slots: BTreeMap::new(),
            kicker_slots: Vec::new(),
            division: None,
            base_mana_value: 3,
            stack: Vec::new(),
            next_object: STACK_ID,
            forbid_cross_slot_repeat: false,
            log: Vec::new(),
        }
    }

    fn with_rules(mut self, list: &[Fx]) -> Toy {
        self.rules = rules(list);
        self
    }

    fn quality(mut self, obj: u64, q: &str) -> Toy {
        self.qualities
            .entry(o(obj))
            .or_default()
            .insert(q.to_owned());
        self
    }

    fn slot(mut self, min: u64, max: u64, candidates: Vec<ChoiceTarget>) -> Toy {
        self.base_slots.push(spec(min, max, candidates));
        self
    }
}

fn spec(min: u64, max: u64, candidates: Vec<ChoiceTarget>) -> TargetSlotSpec {
    TargetSlotSpec {
        min,
        max,
        candidates,
    }
}

/// The id the toy gives the next stack object.
const STACK_ID: u64 = 100;

impl Transactional for Toy {
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

/// The toy's mana-value parity rule: an entry with `"parity": "EVEN"` / `"ODD"` applies when
/// the spell's mana value (base + X) has that parity; any other entry always applies.
fn parity_applies(entry: &RuleEntry, mana_value: u64) -> bool {
    match entry
        .modification
        .params
        .get("parity")
        .and_then(Value::as_str)
    {
        Some("EVEN") => mana_value.is_multiple_of(2),
        Some("ODD") => !mana_value.is_multiple_of(2),
        _ => true,
    }
}

impl CastState for Toy {
    fn rule_set(&self) -> &RuleSet {
        &self.rules
    }

    fn are_opponents(&self, a: PlayerId, b: PlayerId) -> bool {
        self.teams.get(&a) != self.teams.get(&b)
    }

    fn target_controller(&self, t: ChoiceTarget) -> PlayerId {
        match t {
            ChoiceTarget::Player(x) => x,
            ChoiceTarget::Object(x) => self.controllers.get(&x).copied().unwrap_or(p(0)),
        }
    }

    fn mode_spec(&self, _req: &CastRequest) -> Option<ModeSpec> {
        self.modes.clone()
    }

    fn cost_spec(&self, _req: &CastRequest, _modes: &[u32]) -> CostSpec {
        self.costs.clone()
    }

    fn target_slots(
        &self,
        _req: &CastRequest,
        modes: &[u32],
        costs: &CostChoice,
    ) -> Vec<TargetSlotSpec> {
        let mut out = self.base_slots.clone();
        for m in modes {
            if let Some(s) = self.mode_slots.get(m) {
                out.extend(s.iter().cloned());
            }
        }
        if !costs.additional.is_empty() {
            out.extend(self.kicker_slots.iter().cloned());
        }
        out
    }

    fn division_total(&self, _req: &CastRequest, _proposal: &Proposal) -> Option<u64> {
        self.division
    }

    fn put_on_stack(&mut self, req: &CastRequest) -> ObjectId {
        let id = o(self.next_object);
        self.next_object += 1;
        self.stack.push(id);
        self.controllers.insert(id, req.caster);
        if let CastKind::Spell { .. } = req.kind
            && let Some(q) = self.qualities.get(&req.object).cloned()
        {
            self.qualities.insert(id, q);
        }
        self.log.push(format!("stack {}", id.get()));
        id
    }

    fn source_has_quality(&self, req: &CastRequest, stack_object: ObjectId, quality: &str) -> bool {
        let holder = match req.kind {
            CastKind::Spell { .. } => stack_object,
            CastKind::Ability { .. } => req.object,
        };
        self.qualities
            .get(&holder)
            .is_some_and(|q| q.contains(quality))
    }

    fn prohibition_applies(
        &self,
        entry: &RuleEntry,
        _req: &CastRequest,
        _stack_object: ObjectId,
        proposal: &Proposal,
    ) -> bool {
        let mv = self.base_mana_value + proposal.costs.x.unwrap_or(0);
        parity_applies(entry, mv)
    }

    fn prohibition_could_lapse(&self, entry: &RuleEntry, _req: &CastRequest) -> bool {
        // CR 601.3a: a parity rule could lapse when X can change the mana value.
        entry.modification.params.contains_key("parity") && self.costs.x_range.is_some()
    }

    fn proposal_combination_legal(
        &self,
        _req: &CastRequest,
        _stack_object: ObjectId,
        proposal: &Proposal,
    ) -> bool {
        if !self.forbid_cross_slot_repeat {
            return true;
        }
        let mut seen = BTreeSet::new();
        proposal.targets.iter().flatten().all(|t| seen.insert(*t))
    }
}

/// A host that keeps the default `proposal_combination_legal`.
#[derive(Clone, Debug)]
struct Plain(Toy);

impl Transactional for Plain {
    fn state_hash(&self) -> Digest {
        self.0.state_hash()
    }

    fn priority_player(&self) -> Option<PlayerId> {
        self.0.priority_player()
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

impl CastState for Plain {
    fn rule_set(&self) -> &RuleSet {
        self.0.rule_set()
    }
    fn are_opponents(&self, a: PlayerId, b: PlayerId) -> bool {
        self.0.are_opponents(a, b)
    }
    fn target_controller(&self, t: ChoiceTarget) -> PlayerId {
        self.0.target_controller(t)
    }
    fn mode_spec(&self, req: &CastRequest) -> Option<ModeSpec> {
        self.0.mode_spec(req)
    }
    fn cost_spec(&self, req: &CastRequest, modes: &[u32]) -> CostSpec {
        self.0.cost_spec(req, modes)
    }
    fn target_slots(
        &self,
        req: &CastRequest,
        modes: &[u32],
        costs: &CostChoice,
    ) -> Vec<TargetSlotSpec> {
        self.0.target_slots(req, modes, costs)
    }
    fn division_total(&self, req: &CastRequest, proposal: &Proposal) -> Option<u64> {
        self.0.division_total(req, proposal)
    }
    fn put_on_stack(&mut self, req: &CastRequest) -> ObjectId {
        self.0.put_on_stack(req)
    }
    fn source_has_quality(&self, req: &CastRequest, s: ObjectId, q: &str) -> bool {
        self.0.source_has_quality(req, s, q)
    }
    fn prohibition_applies(
        &self,
        entry: &RuleEntry,
        req: &CastRequest,
        s: ObjectId,
        proposal: &Proposal,
    ) -> bool {
        self.0.prohibition_applies(entry, req, s, proposal)
    }
    fn prohibition_could_lapse(&self, entry: &RuleEntry, req: &CastRequest) -> bool {
        self.0.prohibition_could_lapse(entry, req)
    }
}

fn spell() -> CastRequest {
    CastRequest {
        caster: p(1),
        object: o(50),
        kind: CastKind::Spell {
            face_index: 0,
            cast_mode: "normal".to_owned(),
        },
    }
}

fn ability() -> CastRequest {
    CastRequest {
        caster: p(1),
        object: o(11),
        kind: CastKind::Ability {
            ability_id: "a0".to_owned(),
        },
    }
}

/// Runs `announce` through a session with `answers`.
fn drive<S: CastState>(
    toy: &S,
    req: &CastRequest,
    answers: &[ChoiceResponse],
) -> Result<Status<S, AnnounceOutcome>, ChoiceError> {
    let req = req.clone();
    let session = ChoiceSession::replay(
        toy.clone(),
        move |s: &mut S, ctx: &mut ChoiceContext<'_>| announce(s, ctx, &req),
        answers,
    )?;
    Ok(session.status().clone())
}

/// The finished state and outcome.
fn done<S: CastState + core::fmt::Debug>(
    toy: &S,
    req: &CastRequest,
    answers: &[ChoiceResponse],
) -> (S, AnnounceOutcome) {
    match drive(toy, req, answers).expect("session") {
        Status::Done { state, output } => (state, output),
        Status::Pending(r) => panic!("unexpected pending {r:?}"),
    }
}

/// The request pending after `answers`.
fn pending(toy: &Toy, req: &CastRequest, answers: &[ChoiceResponse]) -> PendingChoiceRequest {
    match drive(toy, req, answers).expect("session") {
        Status::Pending(r) => r,
        Status::Done { output, .. } => panic!("unexpected done {output:?}"),
    }
}

/// Asserts a rollback at `step` for `reason` that restored `before` exactly (CR 734.1, 734.2).
fn assert_rolled_back(
    before: &Toy,
    after: &Toy,
    out: &AnnounceOutcome,
    step: CastStep,
    reason: StopReason,
) {
    match out {
        AnnounceOutcome::RolledBack(rb) => {
            assert_eq!(rb.step, step);
            assert_eq!(rb.reason, reason);
            assert_eq!(rb.state_hash, before.state_hash());
            assert_eq!(rb.priority_player, before.priority_player());
        }
        other => panic!("expected a rollback, got {other:?}"),
    }
    assert_eq!(after.state_hash(), before.state_hash());
    assert!(after.stack.is_empty() && after.log.is_empty());
}

fn select_targets(targets: Vec<Vec<ChoiceTarget>>) -> ChoiceResponse {
    ChoiceResponse::SelectTargets { targets }
}

fn costs(alternative: Option<u32>, additional: Vec<u32>, x: Option<u64>) -> ChoiceResponse {
    ChoiceResponse::ConfigureCosts {
        alternative,
        additional,
        x,
        symbol_options: Vec::new(),
        harmonize: None,
    }
}

fn legal_of(r: &PendingChoiceRequest) -> Vec<Vec<ChoiceTarget>> {
    match &r.payload {
        ChoicePayload::ChooseTargets { slots, .. } => {
            slots.iter().map(|s| s.legal.clone()).collect()
        }
        other => panic!("not a target request: {other:?}"),
    }
}

/// The legal candidates of the single slot of a one-slot spell with `candidates`.
fn legal_single(toy: Toy, req: &CastRequest, candidates: Vec<ChoiceTarget>) -> Vec<ChoiceTarget> {
    let toy = toy.slot(0, 1, candidates);
    legal_of(&pending(&toy, req, &[])).remove(0)
}

fn announced(out: AnnounceOutcome) -> Proposal {
    match out {
        AnnounceOutcome::Announced { proposal, .. } => proposal,
        other => panic!("expected an announcement, got {other:?}"),
    }
}

const NONE: Vec<ChoiceTarget> = Vec::new();

// ---------------------------------------------------------------------------------------------
// Steps 0, 1 and success (REQ-CAST-001)
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, CR 601.2a: with nothing to choose the object goes on the stack (topmost, with
/// the caster as controller) and the change is kept.
#[test]
fn req_cast_001_plain_spell_is_announced_and_kept() {
    let toy = Toy::new();
    let (after, out) = done(&toy, &spell(), &[]);
    assert_eq!(
        out,
        AnnounceOutcome::Announced {
            stack_object: o(STACK_ID),
            proposal: Proposal::default(),
        }
    );
    assert_eq!(after.stack, vec![o(STACK_ID)]);
    assert_eq!(after.controllers.get(&o(STACK_ID)), Some(&p(1)));
    assert_ne!(after.state_hash(), toy.state_hash());
}

/// REQ-CAST-001, CR 602.2a: an ability is announced the same way; the default
/// `proposal_combination_legal` accepts any combination.
#[test]
fn req_cast_001_ability_is_announced_with_default_combination() {
    let (after, out) = done(&Toy::new(), &ability(), &[]);
    assert!(
        matches!(out, AnnounceOutcome::Announced { stack_object, .. } if stack_object == o(STACK_ID))
    );
    assert_eq!(after.stack.len(), 1);
    let plain = Plain(Toy::new().slot(1, 1, vec![to(20)]).slot(1, 1, vec![to(20)]));
    let (_, out) = done(
        &plain,
        &ability(),
        &[select_targets(vec![vec![to(20)], vec![to(20)]])],
    );
    assert_eq!(announced(out).targets, vec![vec![to(20)], vec![to(20)]]);
}

/// REQ-CAST-001, CR 601.3: a CAST prohibition refuses before anything changes; ACTIVATE
/// prohibitions refuse abilities only.
#[test]
fn req_cast_001_begin_check_refuses_with_not_started() {
    let toy = Toy::new().with_rules(&[prohibit(7, 1, "CAST")]);
    let (after, out) = done(&toy, &spell(), &[]);
    assert_eq!(
        out,
        AnnounceOutcome::NotStarted {
            reason: StopReason::Prohibited {
                by: EffectId::new(7)
            }
        }
    );
    assert_eq!(after.state_hash(), toy.state_hash());
    assert!(matches!(
        done(&toy, &ability(), &[]).1,
        AnnounceOutcome::Announced { .. }
    ));
    let toy = Toy::new().with_rules(&[prohibit(8, 1, "ACTIVATE")]);
    assert_eq!(
        done(&toy, &ability(), &[]).1,
        AnnounceOutcome::NotStarted {
            reason: StopReason::Prohibited {
                by: EffectId::new(8)
            }
        }
    );
    assert!(matches!(
        done(&toy, &spell(), &[]).1,
        AnnounceOutcome::Announced { .. }
    ));
}

/// REQ-CAST-001, CR 601.3, R12: the "object" filter, the "zone" convention, other players and
/// malformed params.
#[test]
fn req_cast_001_begin_check_filters() {
    let cases: Vec<(Fx, bool)> = vec![
        (prohibit(1, 1, "CAST").with("object", 50u64), true),
        (prohibit(1, 1, "CAST").with("object", 51u64), false),
        (prohibit(1, 1, "CAST").with("object", "50"), false),
        (prohibit(1, 1, "CAST").with("zone", "HAND"), false),
        (prohibit(1, 1, "CAST").for_players(&[2]), false),
        (prohibit(1, 1, "PLAY_LAND"), false),
        (fx(1, 1, RuleKind::Prohibition).with("action", 5u64), false),
    ];
    for (f, refused) in cases {
        let toy = Toy::new().with_rules(&[f]);
        let out = done(&toy, &spell(), &[]).1;
        assert_eq!(
            matches!(out, AnnounceOutcome::NotStarted { .. }),
            refused,
            "{out:?}"
        );
    }
}

/// REQ-CAST-001, CR 601.3a, 601.2e: a prohibition X could lapse lets the cast begin; X = 2
/// (mana value 4, even) rolls back at step 5, X = 1 (mana value 3) succeeds.
#[test]
fn req_cast_001_601_3a_leniency_then_step_5_check() {
    let mut toy = Toy::new().with_rules(&[prohibit(9, 1, "CAST").with("parity", "EVEN")]);
    toy.base_mana_value = 2;
    // Without X the prohibition cannot lapse: refused.
    assert_eq!(
        done(&toy, &spell(), &[]).1,
        AnnounceOutcome::NotStarted {
            reason: StopReason::Prohibited {
                by: EffectId::new(9)
            }
        }
    );
    toy.costs.x_range = Some((0, 5));
    let r = pending(&toy, &spell(), &[]);
    assert_eq!(r.choice_type, ChoiceType::ChooseCostsAndX);
    let (after, out) = done(&toy, &spell(), &[costs(None, vec![], Some(2))]);
    assert_rolled_back(
        &toy,
        &after,
        &out,
        CastStep::Legality,
        StopReason::ProposalProhibited {
            by: EffectId::new(9),
        },
    );
    let (after, out) = done(&toy, &spell(), &[costs(None, vec![], Some(1))]);
    assert_eq!(announced(out).costs.x, Some(1));
    assert_eq!(after.stack.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// Rollback at each step (REQ-CAST-001, REQ-CORE-005, CR 734.1, 734.2)
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, REQ-CORE-005, CR 734.1, 734.2, 700.2a: step 2 with no legal mode.
#[test]
fn req_core_005_rollback_at_step_2_no_legal_mode() {
    let mut toy = Toy::new();
    toy.priority = Some(p(2));
    toy.modes = Some(ModeSpec {
        labels: strs(&["m0", "m1"]),
        min: 1,
        max: 1,
        ..ModeSpec::default()
    });
    for m in 0..2 {
        toy.mode_slots.insert(m, vec![spec(1, 1, vec![])]);
    }
    let (after, out) = done(&toy, &spell(), &[]);
    assert_rolled_back(
        &toy,
        &after,
        &out,
        CastStep::Choices,
        StopReason::NoLegalMode,
    );
}

/// REQ-CAST-001, REQ-CORE-005, CR 734.1, 601.2c: step 3 with a required slot that has no legal
/// target (the only candidate has shroud).
#[test]
fn req_core_005_rollback_at_step_3_missing_target() {
    let toy = Toy::new()
        .with_rules(&[targeting(1, 1, "SHROUD", to(20))])
        .slot(0, 1, vec![to(21)])
        .slot(1, 1, vec![to(20)]);
    let (after, out) = done(&toy, &spell(), &[]);
    assert_rolled_back(
        &toy,
        &after,
        &out,
        CastStep::Targets,
        StopReason::MissingTarget { slot: 1 },
    );
}

/// REQ-CAST-001, REQ-CORE-005, CR 734.1, 601.2d: step 4 with less to divide than targets.
#[test]
fn req_core_005_rollback_at_step_4_division_impossible() {
    let mut toy = Toy::new().slot(2, 2, vec![to(20), to(21)]);
    toy.division = Some(1);
    let answers = [select_targets(vec![vec![to(20), to(21)]])];
    let (after, out) = done(&toy, &spell(), &answers);
    assert_rolled_back(
        &toy,
        &after,
        &out,
        CastStep::Division,
        StopReason::DivisionImpossible {
            total: 1,
            recipients: 2,
        },
    );
}

/// REQ-CAST-001, REQ-CORE-005, CR 734.1, 601.2e: step 5, a prohibition on the proposal and an
/// illegal cross-slot combination.
#[test]
fn req_core_005_rollback_at_step_5() {
    let mut toy = Toy::new()
        .with_rules(&[prohibit(4, 1, "CAST").with("parity", "ODD")])
        .slot(1, 1, vec![to(20)])
        .slot(1, 1, vec![to(20)]);
    toy.costs.x_range = Some((0, 3));
    toy.base_mana_value = 0;
    let answers = [
        costs(None, vec![], Some(3)),
        select_targets(vec![vec![to(20)], vec![to(20)]]),
    ];
    let (after, out) = done(&toy, &spell(), &answers);
    assert_rolled_back(
        &toy,
        &after,
        &out,
        CastStep::Legality,
        StopReason::ProposalProhibited {
            by: EffectId::new(4),
        },
    );
    // X = 2: no prohibition, but the combination is illegal.
    toy.forbid_cross_slot_repeat = true;
    let answers = [
        costs(None, vec![], Some(2)),
        select_targets(vec![vec![to(20)], vec![to(20)]]),
    ];
    let (after, out) = done(&toy, &spell(), &answers);
    assert_rolled_back(
        &toy,
        &after,
        &out,
        CastStep::Legality,
        StopReason::CombinationIllegal,
    );
}

// ---------------------------------------------------------------------------------------------
// Targeting (REQ-CAST-001, REQ-DSL-013; CR 115, 601.2c, 702.11, 702.16, 702.18, 101.2)
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, REQ-DSL-013, CR 702.11b-c: hexproof is legal for its own controller (and
/// teammates), illegal for an opponent; on objects and on players.
#[test]
fn req_cast_001_hexproof_opponents_only() {
    let hex = |id, t| targeting(id, 1, "HEXPROOF", t).with("sources", "OPPONENTS");
    let toy =
        Toy::new().with_rules(&[hex(1, to(10)), hex(2, to(20)), hex(3, tp(2)), hex(4, tp(1))]);
    let legal = legal_single(
        toy.clone(),
        &spell(),
        vec![tp(1), tp(2), to(10), to(20), to(30)],
    );
    assert_eq!(legal, vec![tp(1), to(10), to(30)]);
    // Player 2 casts: the reverse.
    let mut req = spell();
    req.caster = p(2);
    let legal = legal_single(toy, &req, vec![tp(1), tp(2), to(10), to(20)]);
    assert_eq!(legal, vec![tp(2), to(20)]);
}

/// REQ-CAST-001, CR 702.18a: shroud is illegal for everyone.
#[test]
fn req_cast_001_shroud_illegal_for_all() {
    let toy = Toy::new().with_rules(&[
        targeting(1, 1, "SHROUD", to(10)),
        targeting(2, 1, "SHROUD", tp(2)),
    ]);
    let legal = legal_single(toy.clone(), &spell(), vec![tp(2), to(10), to(20)]);
    assert_eq!(legal, vec![to(20)]);
    let mut req = spell();
    req.caster = p(2);
    let legal = legal_single(toy, &req, vec![tp(2), to(10), to(20)]);
    assert_eq!(legal, vec![to(20)]);
}

/// REQ-CAST-001, CR 702.16a-b: protection from a quality stops spells with it and abilities
/// from sources with it, and nothing else.
#[test]
fn req_cast_001_protection_from_quality_spell_and_ability() {
    let prot = targeting(1, 1, "PROTECTION", to(20)).with("quality", "q.red");
    let cands = vec![to(20), to(21)];
    let toy = Toy::new()
        .with_rules(std::slice::from_ref(&prot))
        .quality(50, "q.red");
    assert_eq!(legal_single(toy, &spell(), cands.clone()), vec![to(21)]);
    let toy = Toy::new()
        .with_rules(std::slice::from_ref(&prot))
        .quality(50, "q.blue");
    assert_eq!(
        legal_single(toy, &spell(), cands.clone()),
        vec![to(20), to(21)]
    );
    // Ability whose source (object 11) has the quality.
    let toy = Toy::new()
        .with_rules(std::slice::from_ref(&prot))
        .quality(11, "q.red");
    assert_eq!(legal_single(toy, &ability(), cands.clone()), vec![to(21)]);
    // Ability whose source lacks it, even if card 50 has it.
    let toy = Toy::new().with_rules(&[prot]).quality(50, "q.red");
    assert_eq!(legal_single(toy, &ability(), cands), vec![to(20), to(21)]);
}

/// REQ-CAST-001, CR 702.16j, 702.16k: protection from everything; protection from a player.
#[test]
fn req_cast_001_protection_everything_and_from_player() {
    let toy = Toy::new().with_rules(&[targeting(1, 1, "PROTECTION", to(20))]);
    assert_eq!(
        legal_single(toy, &spell(), vec![to(20), to(21)]),
        vec![to(21)]
    );
    let toy = Toy::new().with_rules(&[
        targeting(1, 1, "PROTECTION", to(20)).with("source_player", 1u64),
        targeting(2, 1, "PROTECTION", to(21)).with("source_player", 3u64),
    ]);
    assert_eq!(
        legal_single(toy, &spell(), vec![to(20), to(21)]),
        vec![to(21)]
    );
}

/// REQ-CAST-001, CR 702.11d: hexproof from a quality applies only to sources with it.
#[test]
fn req_cast_001_hexproof_from_quality() {
    let hexq = targeting(1, 1, "HEXPROOF", to(20))
        .with("sources", "OPPONENTS")
        .with("quality", "q.green");
    let toy = Toy::new()
        .with_rules(std::slice::from_ref(&hexq))
        .quality(50, "q.green");
    assert_eq!(legal_single(toy, &spell(), vec![to(20)]), NONE);
    let toy = Toy::new().with_rules(&[hexq]).quality(50, "q.white");
    assert_eq!(legal_single(toy, &spell(), vec![to(20)]), vec![to(20)]);
}

/// REQ-CAST-001, CR 702.11e, 101.2: IGNORE_HEXPROOF removes hexproof and hexproof-from, but not
/// shroud, protection or OTHER; "can't" beats "can" in both timestamp orders.
#[test]
fn req_cast_001_ignore_hexproof_scope_and_timestamp_orders() {
    for (ts_ignore, ts_rest) in [(1, 2), (2, 1)] {
        let ignore =
            |id, t| targeting(id, ts_ignore, "HEXPROOF", t).with("effect", "IGNORE_HEXPROOF");
        let toy = Toy::new()
            .with_rules(&[
                targeting(1, ts_rest, "HEXPROOF", to(20)).with("sources", "OPPONENTS"),
                targeting(2, ts_rest, "HEXPROOF", to(21))
                    .with("sources", "OPPONENTS")
                    .with("quality", "q.red"),
                targeting(3, ts_rest, "SHROUD", to(22)),
                targeting(4, ts_rest, "PROTECTION", to(21)).with("quality", "q.blue"),
                ignore(9, to(20)),
                ignore(10, to(21)),
                ignore(11, to(22)),
            ])
            .quality(50, "q.red");
        // 20: hexproof ignored; 21: hexproof-from-red ignored and protection from blue does
        // not apply; 22: shroud stays.
        assert_eq!(
            legal_single(toy.clone(), &spell(), vec![to(20), to(21), to(22)]),
            vec![to(20), to(21)]
        );
        // A red and blue spell: protection from blue applies despite the permission.
        let toy = toy.quality(50, "q.blue");
        assert_eq!(
            legal_single(toy, &spell(), vec![to(20), to(21), to(22)]),
            vec![to(20)]
        );
        // A permission for another player does nothing for player 1.
        let toy = Toy::new().with_rules(&[
            targeting(1, ts_rest, "HEXPROOF", to(20)).with("sources", "OPPONENTS"),
            ignore(9, to(20)).for_players(&[2]),
        ]);
        assert_eq!(legal_single(toy, &spell(), vec![to(20)]), NONE);
        // OTHER ("can't be the target of") wins over the permission.
        let toy =
            Toy::new().with_rules(&[targeting(1, ts_rest, "OTHER", to(20)), ignore(9, to(20))]);
        assert_eq!(legal_single(toy, &spell(), vec![to(20)]), NONE);
    }
}

/// REQ-CAST-001, CR 115.5, 115.3, 601.2c: the stack object is never its own candidate; the same
/// object may be chosen in two slots, never twice in one slot.
#[test]
fn req_cast_001_self_and_repeats() {
    let toy = Toy::new()
        .slot(1, 2, vec![to(STACK_ID), to(21), to(20), to(20)])
        .slot(1, 1, vec![to(20), to(STACK_ID)]);
    let r = pending(&toy, &spell(), &[]);
    assert_eq!(legal_of(&r), vec![vec![to(20), to(21)], vec![to(20)]]);
    assert!(
        r.validate(&select_targets(vec![vec![to(20), to(20)], vec![to(20)]]))
            .is_err()
    );
    assert!(
        r.validate(&select_targets(vec![vec![to(STACK_ID)], vec![to(20)]]))
            .is_err()
    );
    let (_, out) = done(
        &toy,
        &spell(),
        &[select_targets(vec![vec![to(21), to(20)], vec![to(20)]])],
    );
    assert_eq!(
        announced(out).targets,
        vec![vec![to(20), to(21)], vec![to(20)]]
    );
}

/// REQ-CAST-001, CR 115.6: a slot with min 0 may stay empty, even with no candidate.
#[test]
fn req_cast_001_zero_targets_allowed() {
    let toy = Toy::new().slot(0, 2, vec![]).slot(0, 1, vec![to(20)]);
    let r = pending(&toy, &spell(), &[]);
    assert_eq!(r.default_on_timeout, select_targets(vec![vec![], vec![]]));
    let (_, out) = done(&toy, &spell(), &[select_targets(vec![vec![], vec![]])]);
    assert_eq!(announced(out).targets, vec![NONE, NONE]);
}

/// REQ-CAST-001, R12, SRS 7.1 step 3: ward is not checked (an unknown keyword matches nothing),
/// malformed entries match nothing, and no trigger is queued.
#[test]
fn req_cast_001_ward_and_malformed_entries_not_checked() {
    let toy = Toy::new().with_rules(&[
        targeting(1, 1, "WARD", to(20)),
        targeting(2, 1, "HEXPROOF", to(20)).with("effect", "UNKNOWN"),
        targeting(3, 1, "SHROUD", to(20)).with("sources", "TEAM"),
        targeting(4, 1, "SHROUD", to(20)).with("quality", 3u64),
        targeting(5, 1, "SHROUD", to(20)).with("source_player", "x"),
        targeting(6, 1, "SHROUD", to(20)).with("target_player", 1u64),
        targeting(12, 1, "SHROUD", to(20)).with("effect", 1u64),
        targeting(13, 1, "SHROUD", to(20)).with("sources", 1u64),
        fx(7, 1, RuleKind::TargetingRule).with("keyword", "SHROUD"),
        fx(8, 1, RuleKind::TargetingRule).with("target_object", 20u64),
        fx(9, 1, RuleKind::TargetingRule)
            .with("keyword", 4u64)
            .with("target_object", 20u64),
        fx(10, 1, RuleKind::TargetingRule)
            .with("keyword", "SHROUD")
            .with("target_object", -20i64),
    ]);
    assert_eq!(
        legal_single(toy.clone(), &spell(), vec![to(20)]),
        vec![to(20)]
    );
    let (after, _) = done(
        &toy.slot(1, 1, vec![to(20)]),
        &spell(),
        &[select_targets(vec![vec![to(20)]])],
    );
    assert_eq!(after.log, vec![format!("stack {STACK_ID}")]);
}

/// REQ-NET-003: many legal targets give a validator contract with the request bounds.
#[test]
fn req_net_003_many_targets_use_a_validator() {
    let cands: Vec<ChoiceTarget> = (200..260).map(to).collect();
    let toy = Toy::new().slot(0, 30, cands);
    let r = pending(&toy, &spell(), &[]);
    match &r.options {
        ChoiceOptions::Validator(c) => assert_eq!((c.min, c.max), (0, 30)),
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Modes (REQ-CAST-001, REQ-NET-003; CR 601.2b, 700.2a, 700.2d)
// ---------------------------------------------------------------------------------------------

fn modal(min: u32, max: u32, repeats: bool) -> Toy {
    let mut toy = Toy::new();
    toy.modes = Some(ModeSpec {
        labels: strs(&["m0", "m1", "m2"]),
        min,
        max,
        repeats_allowed: repeats,
        mode_costs: strs(&["c0", "c1", "c2"]),
    });
    toy
}

/// REQ-CAST-001, REQ-NET-003, CR 700.2a: a mode whose required target has no legal candidate
/// is not offered and the answer maps back to original indices.
#[test]
fn req_net_003_illegal_mode_hidden_and_indices_mapped() {
    let mut toy = modal(1, 2, false).with_rules(&[targeting(1, 1, "SHROUD", to(20))]);
    toy.mode_slots.insert(0, vec![spec(1, 1, vec![to(20)])]);
    toy.mode_slots.insert(2, vec![spec(1, 1, vec![to(21)])]);
    let r = pending(&toy, &spell(), &[]);
    assert_eq!(r.choice_type, ChoiceType::ChooseModes);
    assert_eq!(r.chooser, p(1));
    assert_eq!(r.reason.cr, "CR 601.2b");
    assert_eq!(r.reason.source, Some(o(STACK_ID)));
    assert_eq!((r.min, r.max), (1, 2));
    assert_eq!(
        r.payload,
        ChoicePayload::ChooseModes {
            labels: strs(&["m1", "m2"]),
            repeats_allowed: false,
            mode_costs: strs(&["c1", "c2"]),
        }
    );
    assert!(r.validate(&r.default_on_timeout).is_ok());
    // Presented index 1 is original mode 2 (which brings a target slot).
    let answers = [
        ChoiceResponse::SelectModes { modes: vec![1, 0] },
        select_targets(vec![vec![to(21)]]),
    ];
    let proposal = announced(done(&toy, &spell(), &answers).1);
    assert_eq!(proposal.modes, vec![1, 2]);
    assert_eq!(proposal.targets, vec![vec![to(21)]]);
    // Fewer legal modes than the minimum: rollback.
    let mut toy3 = toy.clone();
    if let Some(m) = toy3.modes.as_mut() {
        m.min = 3;
        m.max = 3;
    }
    let (after, out) = done(&toy3, &spell(), &[]);
    assert_rolled_back(
        &toy3,
        &after,
        &out,
        CastStep::Choices,
        StopReason::NoLegalMode,
    );
}

/// REQ-NET-003, CR 700.2d: repeats follow `repeats_allowed`.
#[test]
fn req_net_003_mode_repeats() {
    let toy = modal(1, 3, false);
    let r = pending(&toy, &spell(), &[]);
    assert!(
        r.validate(&ChoiceResponse::SelectModes { modes: vec![0, 0] })
            .is_err()
    );
    let toy = modal(1, 3, true);
    let r = pending(&toy, &spell(), &[]);
    let repeated = ChoiceResponse::SelectModes {
        modes: vec![2, 0, 2],
    };
    assert!(r.validate(&repeated).is_ok());
    let proposal = announced(done(&toy, &spell(), &[repeated]).1);
    assert_eq!(proposal.modes, vec![0, 2, 2]);
    // With repeats, the default is valid even when min exceeds the number of legal modes.
    let mut toy = modal(4, 5, true);
    toy.mode_slots.insert(0, vec![spec(1, 1, vec![])]);
    toy.mode_slots.insert(1, vec![spec(1, 1, vec![])]);
    let r = pending(&toy, &spell(), &[]);
    assert_eq!(
        r.default_on_timeout,
        ChoiceResponse::SelectModes {
            modes: vec![0, 0, 0, 0]
        }
    );
    assert!(r.validate(&r.default_on_timeout).is_ok());
}

/// REQ-NET-003: a mode request with many answers carries a validator contract.
#[test]
fn req_net_003_many_modes_use_a_validator() {
    let mut toy = Toy::new();
    toy.modes = Some(ModeSpec {
        labels: (0..40).map(|i| format!("m{i}")).collect(),
        min: 1,
        max: 40,
        ..ModeSpec::default()
    });
    let r = pending(&toy, &spell(), &[]);
    match &r.options {
        ChoiceOptions::Validator(c) => assert_eq!((c.min, c.max), (1, 40)),
        other => panic!("{other:?}"),
    }
}

/// REQ-CAST-001, CR 700.2a: a malformed spec (max below min) has no legal choice.
#[test]
fn req_cast_001_mode_spec_max_below_min_rolls_back() {
    let toy = modal(2, 1, false);
    let (after, out) = done(&toy, &spell(), &[]);
    assert_rolled_back(
        &toy,
        &after,
        &out,
        CastStep::Choices,
        StopReason::NoLegalMode,
    );
}

// ---------------------------------------------------------------------------------------------
// Costs (REQ-NET-003; CR 601.2b)
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003, CR 601.2b: at most one alternative cost, distinct additional costs, X in range,
/// one option per symbol slot, harmonize from the candidates; invalid answers rejected.
#[test]
fn req_net_003_costs_request_and_validation() {
    let mut toy = Toy::new();
    toy.costs = CostSpec {
        alternative_costs: strs(&["alt0", "alt1"]),
        additional_costs: strs(&["add0", "add1"]),
        x_range: Some((1, 4)),
        symbol_slots: vec![strs(&["s.mana", "s.life"])],
        harmonize_candidates: vec![o(10)],
    };
    let r = pending(&toy, &spell(), &[]);
    assert_eq!(r.choice_type, ChoiceType::ChooseCostsAndX);
    assert_eq!(r.reason.cr, "CR 601.2b");
    assert_eq!(r.chooser, p(1));
    assert!(!r.hidden && r.reveal_to.is_empty());
    assert!(matches!(r.options, ChoiceOptions::Enumerated(_)));
    assert!(r.validate(&r.default_on_timeout).is_ok());
    let cc = |alternative, additional: Vec<u32>, x, symbol_options: Vec<u32>, harmonize| {
        ChoiceResponse::ConfigureCosts {
            alternative,
            additional,
            x,
            symbol_options,
            harmonize,
        }
    };
    let good = cc(Some(1), vec![1, 0], Some(4), vec![1], Some(o(10)));
    assert!(r.validate(&good).is_ok());
    for bad in [
        cc(Some(2), vec![], Some(1), vec![0], None),
        cc(None, vec![0, 0], Some(1), vec![0], None),
        cc(None, vec![], Some(5), vec![0], None),
        cc(None, vec![], Some(0), vec![0], None),
        cc(None, vec![], None, vec![0], None),
        cc(None, vec![], Some(1), vec![2], None),
        cc(None, vec![], Some(1), vec![], None),
        cc(None, vec![], Some(1), vec![0], Some(o(20))),
    ] {
        assert!(r.validate(&bad).is_err(), "{bad:?}");
        assert!(drive(&toy, &spell(), &[bad]).is_err());
    }
    // The answer is kept (additional costs ascending).
    assert_eq!(
        announced(done(&toy, &spell(), &[good]).1).costs,
        CostChoice {
            alternative: Some(1),
            additional: vec![0, 1],
            x: Some(4),
            symbol_options: vec![1],
            harmonize: Some(o(10)),
        }
    );
}

/// REQ-NET-003: a huge X range uses a validator; an additional cost brings its target slot
/// (CR 601.2c).
#[test]
fn req_net_003_costs_validator_and_kicker_targets() {
    let mut toy = Toy::new();
    toy.costs.x_range = Some((0, u64::MAX));
    toy.costs.additional_costs = strs(&["add0"]);
    toy.kicker_slots = vec![spec(1, 1, vec![to(20)])];
    let r = pending(&toy, &spell(), &[]);
    match &r.options {
        ChoiceOptions::Validator(c) => assert_eq!((c.min, c.max), (r.min, r.max)),
        other => panic!("{other:?}"),
    }
    // Not kicked: no target ask.
    let proposal = announced(done(&toy, &spell(), &[costs(None, vec![], Some(7))]).1);
    assert!(proposal.targets.is_empty());
    // Kicked: the target is asked.
    let r = pending(&toy, &spell(), &[costs(None, vec![0], Some(7))]);
    assert_eq!(r.choice_type, ChoiceType::ChooseTargets);
    assert_eq!(r.reason.cr, "CR 601.2c");
}

/// REQ-NET-003, R12: a malformed host spec is a protocol error, never a panic.
#[test]
fn req_net_003_malformed_host_spec_is_an_error() {
    let mut toy = Toy::new();
    toy.costs.x_range = Some((3, 1));
    assert!(drive(&toy, &spell(), &[]).is_err());
    let mut toy = Toy::new();
    toy.costs.symbol_slots = vec![vec![]];
    assert!(drive(&toy, &spell(), &[]).is_err());
    let toy = Toy::new().slot(2, 1, vec![to(20), to(21)]);
    assert!(drive(&toy, &spell(), &[]).is_err());
}

// ---------------------------------------------------------------------------------------------
// Division (REQ-NET-003; CR 601.2d)
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003, CR 601.2d: 3 among two targets: 3/0 rejected, 2/1 succeeds; recipients are
/// distinct even when one target is in two slots; nothing divided without a target.
#[test]
fn req_net_003_division() {
    let mut toy = Toy::new().slot(1, 2, vec![to(20), to(21)]);
    toy.division = Some(3);
    let t = select_targets(vec![vec![to(20), to(21)]]);
    let r = pending(&toy, &spell(), std::slice::from_ref(&t));
    assert_eq!(r.choice_type, ChoiceType::DivideValue);
    assert_eq!(r.reason.cr, "CR 601.2d");
    assert_eq!((r.min, r.max), (2, 2));
    assert_eq!(
        r.payload,
        ChoicePayload::DivideValue {
            total: 3,
            recipients: vec![(to(20), 1), (to(21), 1)],
        }
    );
    assert!(r.validate(&r.default_on_timeout).is_ok());
    let bad = ChoiceResponse::SubmitDivision {
        assignments: vec![(to(20), 3), (to(21), 0)],
    };
    assert!(r.validate(&bad).is_err());
    assert!(drive(&toy, &spell(), &[t.clone(), bad]).is_err());
    let good = ChoiceResponse::SubmitDivision {
        assignments: vec![(to(21), 1), (to(20), 2)],
    };
    assert_eq!(
        announced(done(&toy, &spell(), &[t, good]).1).division,
        Some(vec![(to(20), 2), (to(21), 1)])
    );
    // The same object in two slots is one recipient.
    let mut toy = Toy::new()
        .slot(1, 1, vec![to(20)])
        .slot(1, 1, vec![to(20), to(21)]);
    toy.division = Some(1);
    let t = select_targets(vec![vec![to(20)], vec![to(20)]]);
    let r = pending(&toy, &spell(), std::slice::from_ref(&t));
    assert_eq!(
        r.payload,
        ChoicePayload::DivideValue {
            total: 1,
            recipients: vec![(to(20), 1)],
        }
    );
    // No target chosen: nothing is divided.
    let mut toy = Toy::new().slot(0, 1, vec![to(20)]);
    toy.division = Some(2);
    let proposal = announced(done(&toy, &spell(), &[select_targets(vec![vec![]])]).1);
    assert!(proposal.division.is_none());
}

// ---------------------------------------------------------------------------------------------
// Suspension (REQ-CORE-001, REQ-NET-003)
// ---------------------------------------------------------------------------------------------

/// A toy that asks every type: modes, costs, targets, division.
fn full_toy() -> Toy {
    let mut toy = modal(1, 1, false);
    toy.mode_slots
        .insert(0, vec![spec(1, 2, vec![to(20), to(21), tp(2)])]);
    toy.costs.x_range = Some((0, 3));
    toy.division = Some(4);
    toy
}

/// REQ-CORE-001, REQ-NET-003: answering one by one and replaying the recorded answers give the
/// same final hash; each request has the right type, chooser, reason and ascending ids.
#[test]
fn req_net_003_suspension_replay_identical() {
    let toy = full_toy();
    let req = spell();
    let r2 = req.clone();
    let mut session = ChoiceSession::start(
        toy.clone(),
        move |s: &mut Toy, ctx: &mut ChoiceContext<'_>| announce(s, ctx, &r2),
    )
    .expect("start");
    let expected = [
        (ChoiceType::ChooseModes, "CR 601.2b"),
        (ChoiceType::ChooseCostsAndX, "CR 601.2b"),
        (ChoiceType::ChooseTargets, "CR 601.2c"),
        (ChoiceType::DivideValue, "CR 601.2d"),
    ];
    let answers = [
        ChoiceResponse::SelectModes { modes: vec![0] },
        costs(None, vec![], Some(2)),
        select_targets(vec![vec![to(20), tp(2)]]),
        ChoiceResponse::SubmitDivision {
            assignments: vec![(to(20), 3), (tp(2), 1)],
        },
    ];
    for (i, ((ty, cr), a)) in expected.iter().zip(&answers).enumerate() {
        let r = session.pending().expect("pending").clone();
        assert_eq!(r.request_id.get(), u64::try_from(i).unwrap());
        assert_eq!(r.choice_type, *ty);
        assert_eq!(r.reason.cr, *cr);
        assert_eq!(r.reason.source, Some(o(STACK_ID)));
        assert_eq!(r.chooser, p(1));
        assert!(!r.hidden);
        assert!(r.reveal_to.is_empty());
        // While pending, the visible state is the base.
        assert_eq!(session.state_hash(), toy.state_hash());
        session.submit(r.request_id, a.clone()).expect("submit");
    }
    let live = session.state_hash();
    let (replayed, out) = done(&toy, &req, &answers);
    assert_eq!(replayed.state_hash(), live);
    let proposal = announced(out);
    assert_eq!(proposal.targets, vec![vec![tp(2), to(20)]]);
    assert_eq!(proposal.division, Some(vec![(tp(2), 1), (to(20), 3)]));
    // The default answers always validate and finish.
    let mut s = ChoiceSession::start(toy, move |s: &mut Toy, ctx: &mut ChoiceContext<'_>| {
        announce(s, ctx, &req)
    })
    .expect("start");
    while let Some(r) = s.pending() {
        let id = r.request_id;
        s.submit_default(id).expect("default");
    }
    assert!(matches!(s.status(), Status::Done { .. }));
}

/// REQ-NET-003: a suspended call propagates `Interrupt::Pending` and leaves the state unchanged.
#[test]
fn req_net_003_pending_leaves_state_unchanged() {
    let toy = full_toy();
    let req = spell();
    let before = toy.state_hash();
    let session = ChoiceSession::start(toy, move |s: &mut Toy, ctx: &mut ChoiceContext<'_>| {
        let out = announce(s, ctx, &req);
        if matches!(out, Err(Interrupt::Pending(_))) {
            assert_eq!(s.state_hash(), before);
        }
        out
    })
    .expect("start");
    assert!(session.pending().is_some());
}

// ---------------------------------------------------------------------------------------------
// Property tests (REQ-CAST-001, REQ-CORE-005, R12)
// ---------------------------------------------------------------------------------------------

fn bounded(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    if n == 0 { 0 } else { rng.next_u64() % n }
}

fn pick<T: Copy>(rng: &mut Pcg64Dxsm, items: &[T]) -> T {
    items[usize::try_from(bounded(rng, items.len() as u64)).unwrap_or(0)]
}

/// A random answer to `r` (it may be invalid; the caller falls back to the default).
fn random_answer(rng: &mut Pcg64Dxsm, r: &PendingChoiceRequest) -> ChoiceResponse {
    match &r.payload {
        ChoicePayload::ChooseModes { labels, .. } => {
            let n = bounded(rng, 3) + 1;
            ChoiceResponse::SelectModes {
                modes: (0..n)
                    .map(|_| u32::try_from(bounded(rng, labels.len() as u64 + 1)).unwrap_or(0))
                    .collect(),
            }
        }
        ChoicePayload::ChooseCostsAndX {
            x_range,
            additional_costs,
            symbol_slots,
            ..
        } => {
            let x = x_range.map(|(lo, hi)| lo + bounded(rng, hi - lo + 1));
            let additional = (0..additional_costs.len())
                .filter(|_| bounded(rng, 2) == 0)
                .map(|i| u32::try_from(i).unwrap_or(0))
                .collect();
            ChoiceResponse::ConfigureCosts {
                alternative: None,
                additional,
                x,
                symbol_options: vec![0; symbol_slots.len()],
                harmonize: None,
            }
        }
        ChoicePayload::ChooseTargets { slots, .. } => select_targets(
            slots
                .iter()
                .map(|s| {
                    let mut chosen: Vec<ChoiceTarget> = s
                        .legal
                        .iter()
                        .copied()
                        .filter(|_| bounded(rng, 2) == 0)
                        .collect();
                    chosen.truncate(usize::try_from(s.max).unwrap_or(usize::MAX));
                    chosen
                })
                .collect(),
        ),
        ChoicePayload::DivideValue { total, recipients } => {
            let n = recipients.len() as u64;
            let mut left = total - n;
            let mut out = Vec::new();
            for (i, (t, _)) in recipients.iter().enumerate() {
                let extra = if i + 1 == recipients.len() {
                    left
                } else {
                    bounded(rng, left + 1)
                };
                left -= extra;
                out.push((*t, 1 + extra));
            }
            ChoiceResponse::SubmitDivision { assignments: out }
        }
        _ => r.default_on_timeout.clone(),
    }
}

fn random_candidates(rng: &mut Pcg64Dxsm) -> Vec<ChoiceTarget> {
    let pool = [to(10), to(20), to(21), to(30), tp(1), tp(2), to(STACK_ID)];
    pool.iter()
        .copied()
        .filter(|_| bounded(rng, 2) == 0)
        .collect()
}

fn random_toy(rng: &mut Pcg64Dxsm) -> Toy {
    let mut toy = Toy::new();
    toy.priority = Some(p(1 + bounded(rng, 2)));
    if bounded(rng, 2) == 0 {
        let n = 1 + bounded(rng, 3);
        toy.modes = Some(ModeSpec {
            labels: (0..n).map(|i| format!("m{i}")).collect(),
            min: u32::try_from(bounded(rng, 2) + 1).unwrap_or(1),
            max: u32::try_from(bounded(rng, 3) + 1).unwrap_or(1),
            repeats_allowed: bounded(rng, 2) == 0,
            mode_costs: Vec::new(),
        });
        for m in 0..n {
            if bounded(rng, 2) == 0 {
                let c = random_candidates(rng);
                let s = spec(bounded(rng, 2), 1 + bounded(rng, 2), c);
                toy.mode_slots
                    .insert(u32::try_from(m).unwrap_or(0), vec![s]);
            }
        }
    }
    if bounded(rng, 2) == 0 {
        toy.costs.x_range = Some((0, bounded(rng, 4)));
    }
    if bounded(rng, 2) == 0 {
        toy.costs.additional_costs = strs(&["add0"]);
        let c = random_candidates(rng);
        toy.kicker_slots = vec![spec(1, 1, c)];
    }
    for _ in 0..bounded(rng, 3) {
        let c = random_candidates(rng);
        let min = bounded(rng, 2);
        toy.base_slots.push(spec(min, min + bounded(rng, 2), c));
    }
    if bounded(rng, 2) == 0 {
        toy.division = Some(bounded(rng, 4));
    }
    toy.forbid_cross_slot_repeat = bounded(rng, 3) == 0;
    toy.base_mana_value = bounded(rng, 3);
    let pool = [to(10), to(20), to(21), to(30), tp(1), tp(2), to(STACK_ID)];
    let mut fxs = Vec::new();
    for i in 0..bounded(rng, 5) {
        let kw = pick(rng, &["HEXPROOF", "SHROUD", "PROTECTION", "OTHER"]);
        let target = pick(rng, &pool);
        let mut f = targeting(i + 1, bounded(rng, 4), kw, target);
        if bounded(rng, 2) == 0 {
            f = f.with("sources", "OPPONENTS");
        }
        if bounded(rng, 3) == 0 {
            f = f.with("effect", "IGNORE_HEXPROOF");
        }
        fxs.push(f);
    }
    if bounded(rng, 3) == 0 {
        let parity = pick(rng, &["EVEN", "ODD"]);
        fxs.push(prohibit(50, 1, "CAST").with("parity", parity));
    }
    toy.with_rules(&fxs)
}

/// REQ-CAST-001, REQ-CORE-005, CR 734.1, 734.2 (property): for random hosts and random answer
/// sequences, every rolled-back or refused cast leaves the identical hash and priority player;
/// every announced one has exactly one new stack object that is not its own target; replays are
/// identical (REQ-CORE-001).
#[test]
fn req_core_005_property_rollback_identical_hash() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x52_0050);
    let mut seen = BTreeSet::new();
    for _ in 0..400 {
        let toy = random_toy(&mut rng);
        let req = if bounded(&mut rng, 4) == 0 {
            ability()
        } else {
            spell()
        };
        let mut answers = Vec::new();
        let mut status = drive(&toy, &req, &answers).expect("start");
        let mut asks = 0;
        while let Status::Pending(r) = &status {
            let a = random_answer(&mut rng, r);
            let a = if r.validate(&a).is_ok() {
                a
            } else {
                r.default_on_timeout.clone()
            };
            answers.push(a);
            status = drive(&toy, &req, &answers).expect("valid answers");
            asks += 1;
            assert!(asks <= 4, "at most four asks");
        }
        let Status::Done { state, output } = status else {
            unreachable!()
        };
        match &output {
            AnnounceOutcome::RolledBack(rb) => {
                seen.insert(format!("{:?}", rb.step));
                assert_eq!(rb.state_hash, toy.state_hash());
                assert_eq!(state.state_hash(), toy.state_hash());
                assert_eq!(rb.priority_player, toy.priority_player());
            }
            AnnounceOutcome::NotStarted { .. } => {
                seen.insert("NotStarted".to_owned());
                assert_eq!(state.state_hash(), toy.state_hash());
            }
            AnnounceOutcome::Announced {
                stack_object,
                proposal,
            } => {
                seen.insert("Announced".to_owned());
                assert_eq!(state.stack, vec![*stack_object]);
                for slot in &proposal.targets {
                    assert!(!slot.contains(&ChoiceTarget::Object(*stack_object)));
                }
            }
        }
        let (again, out2) = done(&toy, &req, &answers);
        assert_eq!(again.state_hash(), state.state_hash());
        assert_eq!(out2, output);
    }
    for k in ["Announced", "Choices", "Targets", "Legality", "NotStarted"] {
        assert!(seen.contains(k), "outcome {k} never produced: {seen:?}");
    }
}

/// A bounded random JSON value.
fn random_json(rng: &mut Pcg64Dxsm, depth: u32) -> Value {
    let words = [
        "HEXPROOF",
        "SHROUD",
        "PROTECTION",
        "OTHER",
        "OPPONENTS",
        "ANY",
        "IGNORE_HEXPROOF",
        "PROHIBIT",
        "CAST",
        "ACTIVATE",
        "EVEN",
        "q.red",
        "",
    ];
    match bounded(rng, if depth == 0 { 6 } else { 8 }) {
        0 => Value::Null,
        1 => Value::Bool(bounded(rng, 2) == 0),
        2 => json!(bounded(rng, 120)),
        3 => json!(-i64::try_from(bounded(rng, 50)).unwrap_or(0)),
        4 => json!(pick(rng, &words)),
        5 => json!(1.5),
        6 => Value::Array(
            (0..bounded(rng, 3))
                .map(|_| random_json(rng, depth - 1))
                .collect(),
        ),
        _ => {
            let mut m = serde_json::Map::new();
            for _ in 0..bounded(rng, 3) {
                m.insert(format!("k{}", bounded(rng, 3)), random_json(rng, depth - 1));
            }
            Value::Object(m)
        }
    }
}

/// R12, REQ-CAST-001 (property): `announce` never panics on arbitrary rule-entry params, and a
/// rollback still restores the identical hash.
#[test]
fn req_cast_001_property_arbitrary_params_never_panic() {
    let keys = [
        "keyword",
        "effect",
        "target_object",
        "target_player",
        "sources",
        "quality",
        "source_player",
        "action",
        "object",
        "zone",
        "parity",
    ];
    let mut rng = Pcg64Dxsm::from_u64_seed(0x52_0051);
    for _ in 0..400 {
        let mut fxs = Vec::new();
        for i in 0..bounded(&mut rng, 6) {
            let kind = pick(&mut rng, &[RuleKind::TargetingRule, RuleKind::Prohibition]);
            let mut f = fx(i + 1, bounded(&mut rng, 3), kind);
            for _ in 0..bounded(&mut rng, 6) {
                let k = pick(&mut rng, &keys);
                let v = random_json(&mut rng, 2);
                f = f.with(k, v);
            }
            fxs.push(f);
        }
        let mut toy = Toy::new()
            .with_rules(&fxs)
            .slot(0, 2, vec![to(10), to(20), tp(1), tp(2), to(STACK_ID)])
            .quality(50, "q.red")
            .quality(11, "q.red");
        toy.costs.x_range = Some((0, 2));
        let req = if bounded(&mut rng, 2) == 0 {
            spell()
        } else {
            ability()
        };
        let mut answers = Vec::new();
        let mut status = drive(&toy, &req, &answers).expect("start");
        while let Status::Pending(r) = &status {
            answers.push(r.default_on_timeout.clone());
            status = drive(&toy, &req, &answers).expect("default answers");
        }
        if let Status::Done {
            state,
            output: AnnounceOutcome::RolledBack(rb),
        } = &status
        {
            assert_eq!(rb.state_hash, toy.state_hash());
            assert_eq!(state.state_hash(), toy.state_hash());
        }
    }
}

/// REQ-NET-003: the capped counters that pick enumerated options versus a validator.
#[test]
fn req_net_003_answer_counters() {
    use super::announce::tests_support as ts;
    assert_eq!(ts::binomial(5, 2), 10);
    assert_eq!(ts::binomial(2, 5), 0);
    assert_eq!(ts::binomial(200, 100), ENUMERATION_LIMIT);
    assert_eq!(ts::modes(3, 1, 2, false), 6);
    assert_eq!(ts::modes(3, 2, 2, true), 6);
    assert_eq!(ts::modes(0, 0, 1, true), 1);
    assert_eq!(ts::modes(50, 1, 50, false), ENUMERATION_LIMIT);
}
