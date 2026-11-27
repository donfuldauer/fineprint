// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared synthetic host and helpers for the legal-action spec tests (REQ-DSL-013,
//! REQ-ARCH-002; CR 101.2, 116.2, 117.1). Black-box: written from the work-item brief only,
//! against the public API of `fineprint_core::actions`.
//!
//! Each `spec_actions_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. Nothing here names a real card (R5): objects are bare numbers and every
//! label is `test.*`.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::actions::*;
use fineprint_core::choice::{
    ChoiceOptions, ChoicePayload, ChoiceReason, ChoiceResponse, ChoiceType, PendingChoiceRequest,
    RequestId, ValidatorContract, ValidatorRule,
};
use fineprint_core::events::{Phase, Step};
use fineprint_core::layers::{
    EffectOrigin, PlayerAffected, PostLayerEffects, PostLayerEnvironment, RuleEffect, RuleSet,
    build_rule_set,
};
use fineprint_dsl::ability::{ActivationLimit, ActivationTiming, RuleKind, RuleModification};
use fineprint_dsl::expr::{PlayerSelectorAST, Zone};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn eid(n: u64) -> EffectId {
    EffectId::new(n)
}

pub fn other(p: PlayerId) -> PlayerId {
    if p == P1 { P2 } else { P1 }
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
            controller: P1,
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
// the fake host
// ---------------------------------------------------------------------------------------------

/// The game situation the fake host reports (the `player` field of the TimingContext is the
/// asked player).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Situation {
    pub active_player: Option<PlayerId>,
    pub phase: Option<Phase>,
    pub step: Option<Step>,
    pub stack_empty: bool,
}

/// A synthetic [`ActionHost`]: every player is offered the same candidate vectors.
#[derive(Clone, Debug)]
pub struct FakeHost {
    pub rules: RuleSet,
    pub situation: Situation,
    pub lands_played: BTreeMap<PlayerId, i64>,
    pub lands: Vec<LandCandidate>,
    pub casts: Vec<CastCandidate>,
    pub abilities: Vec<AbilityCandidate>,
    pub face_up: Vec<TurnFaceUpCandidate>,
    pub specials: Vec<SpecialCandidate>,
}

impl FakeHost {
    /// P1's precombat main phase, empty stack, no lands played, no rule effects, no candidates.
    pub fn new() -> FakeHost {
        FakeHost {
            rules: RuleSet::default(),
            situation: Situation {
                active_player: Some(P1),
                phase: Some(Phase::PrecombatMain),
                step: None,
                stack_empty: true,
            },
            lands_played: BTreeMap::new(),
            lands: Vec::new(),
            casts: Vec::new(),
            abilities: Vec::new(),
            face_up: Vec::new(),
            specials: Vec::new(),
        }
    }

    pub fn with_rules(mut self, items: &[R]) -> FakeHost {
        self.rules = rules(items);
        self
    }

    pub fn active(mut self, p: Option<PlayerId>) -> FakeHost {
        self.situation.active_player = p;
        self
    }

    pub fn phase(mut self, phase: Phase, step: Option<Step>) -> FakeHost {
        self.situation.phase = Some(phase);
        self.situation.step = step;
        self
    }

    pub fn stack(mut self, empty: bool) -> FakeHost {
        self.situation.stack_empty = empty;
        self
    }

    pub fn played(mut self, p: PlayerId, n: i64) -> FakeHost {
        self.lands_played.insert(p, n);
        self
    }
}

impl Default for FakeHost {
    fn default() -> FakeHost {
        FakeHost::new()
    }
}

impl ActionHost for FakeHost {
    fn rule_set(&self) -> &RuleSet {
        &self.rules
    }

    fn timing(&self, player: PlayerId) -> TimingContext {
        TimingContext {
            player,
            active_player: self.situation.active_player,
            phase: self.situation.phase,
            step: self.situation.step,
            stack_empty: self.situation.stack_empty,
        }
    }

    fn lands_played_this_turn(&self, player: PlayerId) -> i64 {
        self.lands_played.get(&player).copied().unwrap_or(0)
    }

    fn land_candidates(&self, _p: PlayerId) -> Vec<LandCandidate> {
        self.lands.clone()
    }

    fn cast_candidates(&self, _p: PlayerId) -> Vec<CastCandidate> {
        self.casts.clone()
    }

    fn ability_candidates(&self, _p: PlayerId) -> Vec<AbilityCandidate> {
        self.abilities.clone()
    }

    fn turn_face_up_candidates(&self, _p: PlayerId) -> Vec<TurnFaceUpCandidate> {
        self.face_up.clone()
    }

    fn special_candidates(&self, _p: PlayerId) -> Vec<SpecialCandidate> {
        self.specials.clone()
    }
}

// ---------------------------------------------------------------------------------------------
// candidate builders (defaults: the simplest legal candidate)
// ---------------------------------------------------------------------------------------------

pub fn land(n: u64, zone: Zone) -> LandCandidate {
    LandCandidate {
        object: oid(n),
        face_index: 0,
        zone,
        own_permission: None,
    }
}

/// A non-land, non-instant cast candidate, face 0, mode `test.normal`.
pub fn cast(n: u64, zone: Zone) -> CastCandidate {
    CastCandidate {
        object: oid(n),
        face_index: 0,
        zone,
        cast_mode: "test.normal".to_owned(),
        is_land: false,
        instant_timing: false,
        own_permission: None,
    }
}

/// A cast candidate with instant timing.
pub fn instant(n: u64, zone: Zone) -> CastCandidate {
    CastCandidate {
        instant_timing: true,
        ..cast(n, zone)
    }
}

/// An ability on the battlefield with no timing, no limit, met conditions.
pub fn ability(n: u64, id: &str) -> AbilityCandidate {
    AbilityCandidate {
        object: oid(n),
        ability_id: id.to_owned(),
        zone: Zone::Battlefield,
        timing: None,
        limits: None,
        uses_this_turn: 0,
        uses_this_game: 0,
        is_mana_ability: false,
        requires_tap_symbol: false,
        summoning_sick: false,
        only_if_met: true,
    }
}

pub fn timed(n: u64, id: &str, timing: ActivationTiming) -> AbilityCandidate {
    AbilityCandidate {
        timing: Some(timing),
        ..ability(n, id)
    }
}

pub fn limit(per_turn: Option<u32>, per_game: Option<u32>) -> ActivationLimit {
    ActivationLimit { per_turn, per_game }
}

pub fn special(kind: &str, n: u64) -> SpecialCandidate {
    SpecialCandidate {
        kind: kind.to_owned(),
        object: oid(n),
        could_begin_to_cast: true,
        available: true,
    }
}

pub fn face_up(n: u64, origin: FaceDownOrigin) -> TurnFaceUpCandidate {
    TurnFaceUpCandidate {
        object: oid(n),
        on_battlefield: true,
        merged_instant_or_sorcery: false,
        origin,
    }
}

// ---------------------------------------------------------------------------------------------
// responses
// ---------------------------------------------------------------------------------------------

pub fn play_land(n: u64) -> ChoiceResponse {
    ChoiceResponse::PlayLand {
        object: oid(n),
        face_index: 0,
    }
}

pub fn land_resp(c: &LandCandidate) -> ChoiceResponse {
    ChoiceResponse::PlayLand {
        object: c.object,
        face_index: c.face_index,
    }
}

pub fn cast_resp(c: &CastCandidate) -> ChoiceResponse {
    ChoiceResponse::InitiateCast {
        object: c.object,
        face_index: c.face_index,
        cast_mode: c.cast_mode.clone(),
    }
}

pub fn ability_resp(c: &AbilityCandidate) -> ChoiceResponse {
    ChoiceResponse::Activate {
        object: c.object,
        ability_id: c.ability_id.clone(),
    }
}

pub fn special_resp(c: &SpecialCandidate) -> ChoiceResponse {
    ChoiceResponse::SpecialAction {
        kind: c.kind.clone(),
        object: c.object,
    }
}

pub fn face_up_resp(c: &TurnFaceUpCandidate) -> ChoiceResponse {
    ChoiceResponse::SpecialAction {
        kind: "TURN_FACE_UP".to_owned(),
        object: c.object,
    }
}

// ---------------------------------------------------------------------------------------------
// observations
// ---------------------------------------------------------------------------------------------

/// The offered responses for `p`.
pub fn offered(host: &FakeHost, p: PlayerId) -> Vec<ChoiceResponse> {
    legal_actions(host, p)
        .into_iter()
        .map(|a| a.response)
        .collect()
}

pub fn is_offered(host: &FakeHost, p: PlayerId, resp: &ChoiceResponse) -> bool {
    offered(host, p).contains(resp)
}

/// The basis of the offered action `resp`; panics if it is not offered.
pub fn basis_of(host: &FakeHost, p: PlayerId, resp: &ChoiceResponse) -> Vec<Allowance> {
    legal_actions(host, p)
        .into_iter()
        .find(|a| &a.response == resp)
        .unwrap_or_else(|| panic!("{resp:?} should be offered"))
        .basis
}

/// The reasons of an illegal action; panics if explain says Legal.
pub fn reasons(host: &FakeHost, p: PlayerId, resp: &ChoiceResponse) -> Vec<Reason> {
    match explain(host, p, resp) {
        Legality::Illegal { reasons } => reasons,
        Legality::Legal { basis } => panic!("{resp:?} should be illegal, got basis {basis:?}"),
    }
}

/// Asserts `resp` is offered and explain says Legal with a non-empty basis equal to the offered
/// one; returns the basis.
pub fn assert_legal(host: &FakeHost, p: PlayerId, resp: &ChoiceResponse) -> Vec<Allowance> {
    let basis = basis_of(host, p, resp);
    assert!(!basis.is_empty(), "{resp:?}: empty basis");
    match explain(host, p, resp) {
        Legality::Legal { basis: b } => assert_eq!(b, basis, "{resp:?}: explain basis"),
        Legality::Illegal { reasons } => panic!("{resp:?} offered but explain says {reasons:?}"),
    }
    basis
}

/// Asserts `resp` is not offered and explain gives exactly `expected`.
pub fn assert_illegal(host: &FakeHost, p: PlayerId, resp: &ChoiceResponse, expected: &[Reason]) {
    assert!(
        !is_offered(host, p, resp),
        "{resp:?} must not be offered (expected {expected:?})"
    );
    assert_eq!(reasons(host, p, resp), expected.to_vec(), "{resp:?}");
}

/// Asserts `resp` is not offered and explain's reasons start with `Prohibited{by}`.
pub fn assert_prohibited(host: &FakeHost, p: PlayerId, resp: &ChoiceResponse, by: u64) {
    assert!(!is_offered(host, p, resp), "{resp:?} must not be offered");
    let rs = reasons(host, p, resp);
    assert_eq!(
        rs.first(),
        Some(&Reason::Prohibited { by: eid(by) }),
        "{resp:?}: {rs:?}"
    );
}

/// Whether the basis contains a CR rule entry (`Rule` whose string starts with "CR ").
pub fn has_rule(basis: &[Allowance]) -> bool {
    basis
        .iter()
        .any(|a| matches!(a, Allowance::Rule { cr } if cr.starts_with("CR ")))
}

pub fn has_effect(basis: &[Allowance], id: u64) -> bool {
    basis.contains(&Allowance::Effect { id: eid(id) })
}

/// Every `Rule` entry of a basis starts with "CR ".
pub fn rule_strings_ok(basis: &[Allowance]) -> bool {
    basis.iter().all(|a| match a {
        Allowance::Rule { cr } => cr.starts_with("CR "),
        _ => true,
    })
}

// ---------------------------------------------------------------------------------------------
// the PRIORITY_ACTION request (as turn::heartbeat::ask_priority builds it)
// ---------------------------------------------------------------------------------------------

pub fn priority_request(host: &FakeHost, p: PlayerId) -> PendingChoiceRequest {
    PendingChoiceRequest {
        request_id: RequestId::new(1),
        chooser: p,
        choice_type: ChoiceType::PriorityAction,
        reason: ChoiceReason {
            cr: "CR 117.3".to_owned(),
            source: None,
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::PassPriority,
        options: ChoiceOptions::Validator(ValidatorContract {
            min: 1,
            max: 1,
            rule: ValidatorRule::Payload,
        }),
        payload: priority_payload(host, p, false),
    }
}

/// The actions of a PRIORITY_ACTION payload.
pub fn payload_actions(payload: &ChoicePayload) -> Vec<ChoiceResponse> {
    match payload {
        ChoicePayload::PriorityAction { actions, .. } => actions.clone(),
        other => panic!("not a PRIORITY_ACTION payload: {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// seeded randomness (Pcg64Dxsm only; R3)
// ---------------------------------------------------------------------------------------------

pub struct Rng(pub Pcg64Dxsm);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(Pcg64Dxsm::from_u64_seed(seed))
    }

    /// In `0..n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }

    pub fn coin(&mut self) -> bool {
        self.below(2) == 0
    }

    /// True with probability 1/n.
    pub fn one_in(&mut self, n: u64) -> bool {
        self.below(n) == 0
    }

    pub fn index(&mut self, len: usize) -> usize {
        usize::try_from(self.below(u64::try_from(len).expect("len fits u64")))
            .expect("index fits usize")
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.index(items.len())]
    }

    /// Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.index(i + 1);
            items.swap(i, j);
        }
    }
}

/// The eleven special-action kind ids of the registry table, in order.
pub const KIND_IDS: [&str; 11] = [
    "TURN_FACE_UP",
    "END_EFFECT",
    "IGNORE_STATIC_EFFECT",
    "DISCARD_AS_INSTANT",
    "SUSPEND",
    "COMPANION",
    "FORETELL",
    "ROLL_PLANAR_DIE",
    "TURN_CONSPIRACY_FACE_UP",
    "PLOT",
    "UNLOCK",
];
