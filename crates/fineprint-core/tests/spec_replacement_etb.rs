// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the ETB look-ahead of `fineprint_core::replacement` (REQ-EVENT-001;
//! CR 614.12, 614.12a, 614.12b, 614.12c, 607.2d, 607.2f, 616.1, 616.1a-f, 616.2): the projected
//! permanent, the applicable set, the CR 616.1 loop over the entering permanent, the "as this
//! enters, choose" choices made before entering and stored in Memory, and payable totals.
//! Black-box: written from the work-item brief only.
//!
//! Every object, type, counter kind, option text and effect here is synthetic and neutrally
//! named (R5). The module is driven directly, through a `ChoiceSession` over a trivial state,
//! since a `ChoiceContext` is only handed out by a session.

#[path = "spec_layers_support.rs"]
mod layers;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::choice::{
    AllowedValues, ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload,
    ChoiceReason, ChoiceResponse, ChoiceSession, ChoiceType, ChoiceValue, PendingChoiceRequest,
    RequestId, Status,
};
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, Evaluated, LayerEnvironment,
    LayerError, LayerModification, MatchContext, ObjectBase,
};
use fineprint_core::memory::{ChoiceValue as MemChoice, Memory, MemoryTable};
use fineprint_core::objects::Characteristics;
use fineprint_core::replacement::{
    BusEventKind, CopyAbilities, EtbAction, EtbApplied, EtbChoiceKind, EtbCondition, EtbContext,
    EtbEffect, EtbEffects, EtbEntry, EtbError, EtbOption, EtbResolved, EtbScope,
    ProjectedPermanent, Tier, applicable, project, resolve_entering,
};
use fineprint_core::txn::Transactional;
use fineprint_dsl::expr::ObjectSelectorAST;
use layers::*;

// ---------------------------------------------------------------------------------------------
// synthetic vocabulary
// ---------------------------------------------------------------------------------------------

const TYPE_X: &str = "test.type_x";
const TYPE_Y: &str = "test.type_y";
const SUB_A: &str = "test.sub_a";
const CTR_A: &str = "test.counter_a";
const CTR_B: &str = "test.counter_b";
const KEY_A: &str = "test.key_a";
const KEY_B: &str = "test.key_b";
const OPT_A: &str = "test.option_a";
const OPT_B: &str = "test.option_b";
const OPT_C: &str = "test.option_c";
const CREATURE: &str = "Creature";
const ARTIFACT: &str = "Artifact";

const ALL_TIERS: [Tier; 5] = [
    Tier::SelfReplacement,
    Tier::ControlEtb,
    Tier::CopyEtb,
    Tier::BackFaceEtb,
    Tier::Other,
];

// ---------------------------------------------------------------------------------------------
// builders
// ---------------------------------------------------------------------------------------------

fn ids(ns: &[u64]) -> BTreeSet<EffectId> {
    ns.iter().map(|&n| eid(n)).collect()
}

fn ascending(ns: &[u64]) -> Vec<EffectId> {
    let mut v: Vec<EffectId> = ns.iter().map(|&n| eid(n)).collect();
    v.sort();
    v
}

/// A synthetic environment: each object is a base that is NOT on the battlefield (it is about
/// to enter), controlled by the given player.
fn env_of(objs: &[(u64, Characteristics, PlayerId)]) -> TestEnv {
    let mut env = TestEnv::new();
    for (n, c, p) in objs {
        env.put_full(oid(*n), c.clone(), Some(*p), false);
    }
    env
}

fn body(n: u64) -> Characteristics {
    creature(&format!("test.etb_body_{n}"), 2, 2)
}

/// Objects 1..=n, each a 2/2 synthetic creature, all controlled by P1.
fn env_n(n: u64) -> TestEnv {
    let objs: Vec<(u64, Characteristics, PlayerId)> = (1..=n).map(|i| (i, body(i), P1)).collect();
    env_of(&objs)
}

fn entry(o: u64, controller: PlayerId) -> EtbEntry {
    EtbEntry {
        object: oid(o),
        controller,
        own_statics: Vec::new(),
    }
}

fn entry_with(
    o: u64,
    controller: PlayerId,
    statics: Vec<(EffectId, ContinuousEffect)>,
) -> EtbEntry {
    EtbEntry {
        object: oid(o),
        controller,
        own_statics: statics,
    }
}

fn add_types_mod(card_types: &[&str]) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: set(card_types),
        subtypes: BTreeSet::new(),
    }
}

fn add_subtypes_mod(subtypes: &[&str]) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: BTreeSet::new(),
        subtypes: set(subtypes),
    }
}

/// A static ability of object `o` affecting only `o` (an "own static", CR 614.12 second input).
fn own_static(id: u64, o: u64, layer: Layer, m: LayerModification) -> (EffectId, ContinuousEffect) {
    (
        eid(id),
        ContinuousEffect {
            source: Some(oid(o)),
            controller: P1,
            timestamp: ts(id),
            layer,
            is_cda: false,
            origin: EffectOrigin::Static { granted_at: None },
            affected: Affected::Objects([oid(o)].into_iter().collect()),
            modification: m,
        },
    )
}

/// A continuous effect already in force affecting object `o` (CR 614.12 third input).
fn in_force(o: u64, t: u64, layer: Layer, m: LayerModification) -> ContinuousEffect {
    eff(
        layer,
        t,
        Affected::Objects([oid(o)].into_iter().collect()),
        m,
    )
}

fn desc(n: u64) -> String {
    format!("test.etb_effect_{n:03}")
}

/// An effect with a neutral description and the brief's defaults otherwise.
fn fx(n: u64, action: EtbAction) -> EtbEffect {
    EtbEffect::new(eid(n), action).with_description(desc(n))
}

/// A `ThisPermanent` effect of object `o` (it affects only `o`).
fn own_fx(n: u64, o: u64, action: EtbAction) -> EtbEffect {
    fx(n, action)
        .with_scope(EtbScope::ThisPermanent)
        .with_source(oid(o))
}

fn counters(kind: &str, amount: i64) -> EtbAction {
    EtbAction::EntersWithCounters {
        kind: kind.to_owned(),
        amount,
    }
}

fn add_card_types(types: &[&str]) -> EtbAction {
    EtbAction::AddCardTypes(set(types))
}

fn opts(list: &[(&str, u64)]) -> Vec<EtbOption> {
    list.iter()
        .map(|(t, c)| EtbOption {
            text: (*t).to_owned(),
            cost: *c,
        })
        .collect()
}

/// A copyable stand-in for `EtbChoiceKind` (the brief names no derives for it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum K {
    Color,
    CreatureType,
    Anchor,
}

fn etb_kind(k: K) -> EtbChoiceKind {
    match k {
        K::Color => EtbChoiceKind::Color,
        K::CreatureType => EtbChoiceKind::CreatureType,
        K::Anchor => EtbChoiceKind::Anchor,
    }
}

fn choose(key: &str, kind: K, list: &[(&str, u64)]) -> EtbAction {
    EtbAction::Choose {
        key: key.to_owned(),
        kind: etb_kind(kind),
        options: opts(list),
    }
}

fn copy_of(c: Characteristics) -> EtbAction {
    EtbAction::EntersAsCopyOf(Box::new(c))
}

fn effects_of(list: Vec<EtbEffect>) -> EtbEffects {
    let mut e = EtbEffects::new();
    for f in list {
        let id = f.id;
        if let Err(err) = e.add(f) {
            panic!("adding effect {id:?} failed: {err}");
        }
    }
    e
}

fn applied_of(ns: &[u64]) -> EtbApplied {
    EtbApplied {
        effects: ids(ns),
        chosen: BTreeMap::new(),
        ..Default::default()
    }
}

/// The applied set of `ns` with `order` = `ns` as given (CR 616.1f: the order the effects were
/// applied in, first applied first).
fn applied_in_order(ns: &[u64]) -> EtbApplied {
    EtbApplied {
        effects: ids(ns),
        chosen: BTreeMap::new(),
        order: ns.iter().map(|&n| eid(n)).collect(),
    }
}

fn no_budget() -> BTreeMap<PlayerId, u64> {
    BTreeMap::new()
}

fn budget(list: &[(PlayerId, u64)]) -> BTreeMap<PlayerId, u64> {
    list.iter().copied().collect()
}

// ---------------------------------------------------------------------------------------------
// observations (compared field by field: only the derives the brief names are relied on)
// ---------------------------------------------------------------------------------------------

/// Everything observable about a projection.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PObs {
    object: ObjectId,
    evaluated: Evaluated,
    tapped: bool,
    counters: BTreeMap<String, i64>,
    effects: BTreeSet<EffectId>,
    chosen: BTreeMap<String, MemChoice>,
    /// The application order (CR 616.1f).
    order: Vec<EffectId>,
}

fn pobs(p: &ProjectedPermanent) -> PObs {
    PObs {
        object: p.object,
        evaluated: Evaluated {
            characteristics: norm(&p.evaluated.characteristics),
            ..p.evaluated.clone()
        },
        tapped: p.tapped,
        counters: p.counters.clone(),
        effects: p.applied.effects.clone(),
        chosen: p.applied.chosen.clone(),
        order: p.applied.order.clone(),
    }
}

/// Everything observable about one resolved entry.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Obs {
    object: ObjectId,
    projected: PObs,
    memory: Memory,
}

fn obs(r: &EtbResolved) -> Obs {
    Obs {
        object: r.object,
        projected: pobs(&r.projected),
        memory: r.memory.clone(),
    }
}

/// What can be observed of an effect without relying on derives the brief does not name.
#[derive(Clone, Debug, PartialEq, Eq)]
struct EffectObs {
    id: EffectId,
    source: Option<ObjectId>,
    description: String,
    tier: Tier,
    scope: EtbScope,
    condition: String,
    action: String,
}

fn effect_obs(e: &EtbEffect) -> EffectObs {
    EffectObs {
        id: e.id,
        source: e.source,
        description: e.description.clone(),
        tier: e.tier,
        scope: e.scope,
        condition: cond_str(&e.condition),
        action: action_str(&e.action),
    }
}

fn cond_str(c: &EtbCondition) -> String {
    match c {
        EtbCondition::Always => "always".to_owned(),
        EtbCondition::HasCardType(t) => format!("has-type {t}"),
        EtbCondition::LacksCardType(t) => format!("lacks-type {t}"),
        EtbCondition::HasSubtype(t) => format!("has-subtype {t}"),
    }
}

fn kind_str(k: &EtbChoiceKind) -> &'static str {
    match k {
        EtbChoiceKind::Color => "color",
        EtbChoiceKind::CreatureType => "creature-type",
        EtbChoiceKind::Anchor => "anchor",
    }
}

fn action_str(a: &EtbAction) -> String {
    match a {
        EtbAction::EntersTapped => "tapped".to_owned(),
        EtbAction::EntersWithCounters { kind, amount } => format!("counters {kind} {amount}"),
        EtbAction::EntersAsCopyOf(c) => format!("copy {:?}", c.names),
        EtbAction::AddCardTypes(t) => format!("add-types {t:?}"),
        EtbAction::Choose { key, kind, options } => format!(
            "choose {key} {} {:?}",
            kind_str(kind),
            options
                .iter()
                .map(|o| (o.text.clone(), o.cost))
                .collect::<Vec<_>>()
        ),
    }
}

fn effects_snapshot(e: &EtbEffects, probe: &[u64]) -> (usize, Vec<Option<EffectObs>>) {
    (
        e.len(),
        probe
            .iter()
            .map(|&n| e.get(eid(n)).map(effect_obs))
            .collect(),
    )
}

fn entry_snapshot(e: &EtbEntry) -> (ObjectId, PlayerId, Vec<(EffectId, ContinuousEffect)>) {
    (e.object, e.controller, e.own_statics.clone())
}

fn types_of(p: &ProjectedPermanent) -> BTreeSet<String> {
    p.evaluated.characteristics.card_types.clone()
}

// ---------------------------------------------------------------------------------------------
// the session driver
// ---------------------------------------------------------------------------------------------

/// The trivial state of the session: `resolve_entering` is pure computation (CR 614.12a:
/// nothing moves while the choices are made).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Nil;

impl Transactional for Nil {
    fn state_hash(&self) -> Digest {
        sha256(b"test.etb_nil")
    }

    fn priority_player(&self) -> Option<PlayerId> {
        Some(P1)
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

/// How a test answers a request.
#[derive(Clone, Debug)]
enum Ans {
    /// `SelectReplacement { index }`.
    Index(u32),
    /// `SelectReplacement` of the effect with this description.
    Desc(String),
    /// `SubmitChoiceValue { Text(..) }`.
    Text(String),
}

fn response_for(req: &PendingChoiceRequest, a: &Ans) -> ChoiceResponse {
    match a {
        Ans::Index(i) => ChoiceResponse::SelectReplacement { index: *i },
        Ans::Desc(d) => {
            let ChoicePayload::ChooseReplacementEffect { effects, .. } = &req.payload else {
                panic!("expected a CHOOSE_REPLACEMENT_EFFECT request, got {req:?}");
            };
            let index = effects
                .iter()
                .position(|e| e == d)
                .unwrap_or_else(|| panic!("{d} is not offered in {effects:?}"));
            ChoiceResponse::SelectReplacement {
                index: u32::try_from(index).expect("small"),
            }
        }
        Ans::Text(t) => ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Text(t.clone()),
        },
    }
}

/// The result of driving `resolve_entering` to completion.
struct Run {
    out: Vec<Obs>,
    requests: Vec<PendingChoiceRequest>,
    answers: Vec<ChoiceResponse>,
}

const MAX_REQUESTS: usize = 64;

/// Drives `resolve_entering` through a `ChoiceSession`, answering request `i` with
/// `answer(i, request)`; `inspect` sees the final output. A procedure error is returned.
fn drive_inspect<T>(
    ctx: &EtbContext<'_>,
    entries: &[EtbEntry],
    payable: &BTreeMap<PlayerId, u64>,
    mut answer: impl FnMut(usize, &PendingChoiceRequest) -> ChoiceResponse,
    inspect: impl FnOnce(&[EtbResolved]) -> T,
) -> Result<(T, Vec<PendingChoiceRequest>, Vec<ChoiceResponse>), ChoiceError> {
    let procedure =
        |_: &mut Nil, cx: &mut ChoiceContext<'_>| resolve_entering(ctx, entries, payable, cx);
    let mut session = ChoiceSession::start(Nil, procedure)?;
    let mut requests = Vec::new();
    while let Some(req) = session.pending().cloned() {
        assert!(
            requests.len() < MAX_REQUESTS,
            "resolve_entering did not terminate within {MAX_REQUESTS} requests"
        );
        let resp = answer(requests.len(), &req);
        session.submit(req.request_id, resp)?;
        requests.push(req);
    }
    let answers = session.answers().to_vec();
    match session.status() {
        Status::Done { output, .. } => Ok((inspect(output), requests, answers)),
        Status::Pending(_) => unreachable!("loop exits only when done"),
    }
}

fn drive(
    ctx: &EtbContext<'_>,
    entries: &[EtbEntry],
    payable: &BTreeMap<PlayerId, u64>,
    script: &[Ans],
) -> Result<Run, ChoiceError> {
    let (out, requests, answers) = drive_inspect(
        ctx,
        entries,
        payable,
        |i, req| {
            let a = script
                .get(i)
                .unwrap_or_else(|| panic!("unscripted request {i}: {req:?}"));
            response_for(req, a)
        },
        |o| o.iter().map(obs).collect::<Vec<_>>(),
    )?;
    Ok(Run {
        out,
        requests,
        answers,
    })
}

fn drive_ok(
    ctx: &EtbContext<'_>,
    entries: &[EtbEntry],
    payable: &BTreeMap<PlayerId, u64>,
    script: &[Ans],
) -> Run {
    match drive(ctx, entries, payable, script) {
        Ok(r) => r,
        Err(e) => panic!("resolve_entering failed: {e}"),
    }
}

/// The error of starting (or answering) a run that must fail.
fn drive_err(
    ctx: &EtbContext<'_>,
    entries: &[EtbEntry],
    payable: &BTreeMap<PlayerId, u64>,
    script: &[Ans],
) -> ChoiceError {
    match drive(ctx, entries, payable, script) {
        Ok(r) => panic!("expected an error, got {:?}", r.out),
        Err(e) => e,
    }
}

fn the_request_allowed(req: &PendingChoiceRequest) -> Vec<String> {
    match &req.payload {
        ChoicePayload::ChooseOptionOrValue {
            allowed: AllowedValues::Texts(t),
            ..
        } => t.clone(),
        other => panic!("expected CHOOSE_OPTION_OR_VALUE with texts, got {other:?}"),
    }
}

/// The exact `CHOOSE_REPLACEMENT_EFFECT` request of R-4.
fn replacement_request(
    id: u64,
    chooser: PlayerId,
    object: u64,
    descs: &[String],
) -> PendingChoiceRequest {
    PendingChoiceRequest {
        request_id: RequestId::new(id),
        chooser,
        choice_type: ChoiceType::ChooseReplacementEffect,
        reason: ChoiceReason {
            cr: "CR 616.1".to_owned(),
            source: Some(oid(object)),
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SelectReplacement { index: 0 },
        options: ChoiceOptions::Enumerated(
            (0..descs.len())
                .map(|i| ChoiceOption::Number(i64::try_from(i).expect("small")))
                .collect(),
        ),
        payload: ChoicePayload::ChooseReplacementEffect {
            event: BusEventKind::ZoneChange.name().to_owned(),
            effects: descs.to_vec(),
        },
    }
}

/// The exact `CHOOSE_OPTION_OR_VALUE` request of R-5.
fn option_request(
    id: u64,
    chooser: PlayerId,
    cr: &str,
    source: Option<ObjectId>,
    option_type: &str,
    allowed: &[&str],
) -> PendingChoiceRequest {
    let allowed: Vec<String> = allowed.iter().map(|s| (*s).to_owned()).collect();
    PendingChoiceRequest {
        request_id: RequestId::new(id),
        chooser,
        choice_type: ChoiceType::ChooseOptionOrValue,
        reason: ChoiceReason {
            cr: cr.to_owned(),
            source,
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Text(allowed[0].clone()),
        },
        options: ChoiceOptions::Enumerated(
            allowed.iter().cloned().map(ChoiceOption::Text).collect(),
        ),
        payload: ChoicePayload::ChooseOptionOrValue {
            option_type: option_type.to_owned(),
            allowed: AllowedValues::Texts(allowed),
        },
    }
}

fn permutations(items: &[u64]) -> Vec<Vec<u64>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut p in permutations(&rest) {
            p.insert(0, head);
            out.push(p);
        }
    }
    out
}

fn proj_ok(ctx: &EtbContext<'_>, e: &EtbEntry, a: &EtbApplied) -> ProjectedPermanent {
    match project(ctx, e, a) {
        Ok(p) => p,
        Err(err) => panic!("project failed: {err}"),
    }
}

fn appl_ok(ctx: &EtbContext<'_>, e: &EtbEntry, a: &EtbApplied) -> Vec<EffectId> {
    match applicable(ctx, e, a) {
        Ok(v) => v,
        Err(err) => panic!("applicable failed: {err}"),
    }
}

// =============================================================================================
// TESTS REQUIRED 1 / R-3 — Acceptance 1: a synthetic type-changing effect changes which
// replacements apply (CR 614.12, CR 616.1f, CR 616.2)
// =============================================================================================

/// The three effects of acceptance 1: A adds `test.type_x` (tier `tier_a`), B "enters tapped"
/// if the permanent has `test.type_x`, C "enters with a counter" if it lacks it.
fn acceptance_1_effects(a: u64, b: u64, c: u64, tier_a: Tier) -> EtbEffects {
    effects_of(vec![
        fx(a, add_card_types(&[TYPE_X])).with_tier(tier_a),
        fx(b, EtbAction::EntersTapped).with_condition(EtbCondition::HasCardType(TYPE_X.to_owned())),
        fx(c, counters(CTR_A, 1)).with_condition(EtbCondition::LacksCardType(TYPE_X.to_owned())),
    ])
}

// REQ-EVENT-001, CR 614.12, CR 616.2 — Acceptance criterion "A synthetic effect that changes
// the entering permanent's type changes which replacements apply": `applicable` before and after
// the type-adding effect is applied.
#[test]
fn req_event_001_acceptance_1_applicable_follows_projected_type() {
    for p in permutations(&[1, 2, 3]) {
        let (a, b, c) = (p[0], p[1], p[2]);
        let env = env_n(1);
        let cont = ContinuousEffects::new();
        let effects = acceptance_1_effects(a, b, c, Tier::Other);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let e = entry(1, P1);
        // Before: A (Always) and C (lacks the type) apply; B does not.
        assert_eq!(
            appl_ok(&ctx, &e, &EtbApplied::default()),
            ascending(&[a, c]),
            "{p:?}"
        );
        // After A: the projection has the type; B applies, C stops applying.
        let after = applied_of(&[a]);
        assert!(types_of(&proj_ok(&ctx, &e, &after)).contains(TYPE_X));
        assert_eq!(appl_ok(&ctx, &e, &after), vec![eid(b)], "{p:?}");
    }
}

// REQ-EVENT-001, CR 614.12, CR 616.1a, CR 616.1e, CR 616.1f, CR 616.2 — Acceptance criterion
// "A synthetic effect that changes the entering permanent's type changes which replacements
// apply": with the type-adding effect in a higher tier, it applies first in every id order,
// the HasCardType effect becomes applicable and applies, the LacksCardType one never does; no
// request is asked.
#[test]
fn req_event_001_acceptance_1_tier_decides_every_id_order() {
    for p in permutations(&[1, 2, 3]) {
        let (a, b, c) = (p[0], p[1], p[2]);
        let env = env_n(1);
        let cont = ContinuousEffects::new();
        let effects = acceptance_1_effects(a, b, c, Tier::SelfReplacement);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
        assert!(run.requests.is_empty(), "{p:?}");
        assert_eq!(run.out.len(), 1);
        let r = &run.out[0];
        assert_eq!(r.object, oid(1));
        assert_eq!(r.projected.effects, ids(&[a, b]), "{p:?}");
        assert!(r.projected.tapped);
        assert!(r.projected.counters.is_empty());
        assert!(
            r.projected
                .evaluated
                .characteristics
                .card_types
                .contains(TYPE_X)
        );
        assert!(
            r.projected
                .evaluated
                .characteristics
                .card_types
                .contains(CREATURE)
        );
    }
}

// REQ-EVENT-001, CR 616.1, CR 616.1e, CR 616.1f, CR 616.2 — Acceptance criterion "A synthetic
// effect that changes the entering permanent's type changes which replacements apply", equal
// tiers: the controller is asked between A and C (descriptions ascending by id); choosing A
// makes B apply and C never; choosing C first applies C, then A alone, then B alone. Every id
// order.
#[test]
fn req_event_001_acceptance_1_equal_tier_asked_order_every_id_order() {
    for p in permutations(&[1, 2, 3]) {
        let (a, b, c) = (p[0], p[1], p[2]);
        let env = env_n(1);
        let cont = ContinuousEffects::new();
        let effects = acceptance_1_effects(a, b, c, Tier::Other);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let offered: Vec<String> = ascending(&[a, c]).iter().map(|e| desc(e.get())).collect();

        // Choose A.
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[Ans::Desc(desc(a))]);
        assert_eq!(
            run.requests,
            vec![replacement_request(0, P1, 1, &offered)],
            "{p:?}"
        );
        let r = &run.out[0].projected;
        assert_eq!(r.effects, ids(&[a, b]), "{p:?}");
        assert!(r.tapped);
        assert!(r.counters.is_empty());

        // Choose C.
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[Ans::Desc(desc(c))]);
        assert_eq!(
            run.requests,
            vec![replacement_request(0, P1, 1, &offered)],
            "{p:?}"
        );
        let r = &run.out[0].projected;
        assert_eq!(r.effects, ids(&[a, b, c]), "{p:?}");
        assert!(r.tapped);
        assert_eq!(r.counters, BTreeMap::from([(CTR_A.to_owned(), 1)]));
    }
}

// REQ-EVENT-001, CR 614.12 (continuous effects from the permanent's own static abilities),
// CR 616.2 — Acceptance criterion "A synthetic effect that changes the entering permanent's type
// changes which replacements apply", second form: an own static with a layer-4 AddTypes makes
// the HasCardType effect apply and the LacksCardType effect not, in every id order.
#[test]
fn req_event_001_acceptance_1_own_static_layer4_changes_applicable() {
    for p in permutations(&[1, 2]) {
        let (b, c) = (p[0], p[1]);
        let env = env_n(1);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![
            fx(b, EtbAction::EntersTapped)
                .with_condition(EtbCondition::HasCardType(TYPE_X.to_owned())),
            fx(c, counters(CTR_A, 1))
                .with_condition(EtbCondition::LacksCardType(TYPE_X.to_owned())),
        ]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let plain = entry(1, P1);
        let typed = entry_with(
            1,
            P1,
            vec![own_static(100, 1, Layer::L4, add_types_mod(&[TYPE_X]))],
        );

        assert_eq!(appl_ok(&ctx, &plain, &EtbApplied::default()), vec![eid(c)]);
        assert_eq!(appl_ok(&ctx, &typed, &EtbApplied::default()), vec![eid(b)]);
        assert!(types_of(&proj_ok(&ctx, &typed, &EtbApplied::default())).contains(TYPE_X));

        let run = drive_ok(&ctx, std::slice::from_ref(&typed), &no_budget(), &[]);
        assert!(run.requests.is_empty());
        assert_eq!(run.out[0].projected.effects, ids(&[b]));
        assert!(run.out[0].projected.tapped);
        assert!(run.out[0].projected.counters.is_empty());

        let run = drive_ok(&ctx, &[plain], &no_budget(), &[]);
        assert_eq!(run.out[0].projected.effects, ids(&[c]));
        assert!(!run.out[0].projected.tapped);
    }
}

// REQ-EVENT-001, CR 614.12, CR 616.1f, CR 616.2 — re-projection after each application: an
// effect made applicable by a copy (a creature model) applies, one made inapplicable (it needed
// the base's artifact type) does not, although it was applicable at the start.
#[test]
fn req_event_001_r3_reprojection_after_copy() {
    for p in permutations(&[1, 2, 3]) {
        let (k, z, w) = (p[0], p[1], p[2]);
        let env = env_of(&[(1, artifact("test.etb_base_artifact"), P1)]);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![
            fx(k, copy_of(creature("test.etb_model_creature", 1, 1))).with_tier(Tier::CopyEtb),
            fx(z, counters(CTR_A, 1))
                .with_condition(EtbCondition::HasCardType(CREATURE.to_owned())),
            fx(w, counters(CTR_B, 1))
                .with_condition(EtbCondition::HasCardType(ARTIFACT.to_owned())),
        ]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        assert_eq!(
            appl_ok(&ctx, &entry(1, P1), &EtbApplied::default()),
            ascending(&[k, w])
        );
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
        assert!(run.requests.is_empty(), "{p:?}");
        let r = &run.out[0].projected;
        assert_eq!(r.effects, ids(&[k, z]), "{p:?}");
        assert_eq!(r.counters, BTreeMap::from([(CTR_A.to_owned(), 1)]));
        assert_eq!(r.evaluated.characteristics.card_types, set(&[CREATURE]));
    }
}

// REQ-EVENT-001, CR 614.12 — HasSubtype reads the projected subtypes (an own static adding a
// subtype in layer 4 makes it applicable).
#[test]
fn req_event_001_r2_has_subtype_reads_projection() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![
        fx(1, EtbAction::EntersTapped).with_condition(EtbCondition::HasSubtype(SUB_A.to_owned())),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    assert!(appl_ok(&ctx, &entry(1, P1), &EtbApplied::default()).is_empty());
    let typed = entry_with(
        1,
        P1,
        vec![own_static(50, 1, Layer::L4, add_subtypes_mod(&[SUB_A]))],
    );
    assert_eq!(appl_ok(&ctx, &typed, &EtbApplied::default()), vec![eid(1)]);
    // A subtype in the environment's base works too.
    let mut c = body(1);
    c.subtypes = set(&[SUB_A]);
    let env2 = env_of(&[(1, c, P1)]);
    let ctx2 = EtbContext {
        env: &env2,
        continuous: &cont,
        effects: &effects,
    };
    assert_eq!(
        appl_ok(&ctx2, &entry(1, P1), &EtbApplied::default()),
        vec![eid(1)]
    );
}

// =============================================================================================
// TESTS REQUIRED 2, 3 / R-5 — Acceptance 2: choices before entering, stored in Memory
// (CR 614.12a, CR 614.12c, CR 607.2d, CR 607.2f)
// =============================================================================================

// REQ-EVENT-001, CR 614.12a, CR 607.2d — Acceptance criterion "Chosen values are visible to the
// permanent's abilities on entry": "as this enters, choose a color" is asked before the result
// exists (request 0 with exactly the R-5 fields); the answer is in `memory`, in
// `projected.applied.chosen`, and `record_into` puts it under the new object's id.
#[test]
fn req_event_001_acceptance_2_choose_color_before_entering() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![own_fx(
        3,
        1,
        choose(KEY_A, K::Color, &[(OPT_A, 0), (OPT_B, 0)]),
    )]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let entries = vec![entry(1, P1)];
    let payable = no_budget();

    // Suspended before anything is produced.
    let procedure =
        |_: &mut Nil, cx: &mut ChoiceContext<'_>| resolve_entering(&ctx, &entries, &payable, cx);
    let mut session = match ChoiceSession::start(Nil, procedure) {
        Ok(s) => s,
        Err(e) => panic!("start failed: {e}"),
    };
    let expected = option_request(0, P1, "CR 614.12a", Some(oid(1)), "COLOR", &[OPT_A, OPT_B]);
    assert_eq!(session.pending(), Some(&expected));
    assert_eq!(session.state_hash(), Nil.state_hash());

    let answer = ChoiceResponse::SubmitChoiceValue {
        value: ChoiceValue::Text(OPT_B.to_owned()),
    };
    if let Err(e) = session.submit(RequestId::new(0), answer) {
        panic!("answer rejected: {e}");
    }
    let Status::Done { output, .. } = session.status() else {
        panic!("expected done after one answer");
    };
    assert_eq!(output.len(), 1);
    let r = &output[0];
    assert_eq!(r.object, oid(1));
    let chosen = MemChoice::Color(OPT_B.to_owned());
    assert_eq!(r.memory.choice(KEY_A), Some(&chosen));
    assert_eq!(r.projected.applied.chosen.get(KEY_A), Some(&chosen));
    assert!(r.projected.applied.effects.contains(&eid(3)));

    // record_into: under a new object id, keeping other keys already there.
    let new_id = oid(500);
    let mut table = MemoryTable::new();
    table
        .get_mut_or_default(new_id)
        .set_choice(KEY_B, MemChoice::Number(7));
    r.record_into(&mut table, new_id);
    let m = table.get(new_id).expect("memory recorded under the new id");
    assert_eq!(m.choice(KEY_A), Some(&chosen));
    assert_eq!(m.choice(KEY_B), Some(&MemChoice::Number(7)));
    assert!(table.get(oid(1)).is_none_or(|m| m.choice(KEY_A).is_none()));

    // Into an empty table too.
    let mut empty = MemoryTable::new();
    r.record_into(&mut empty, oid(501));
    assert_eq!(
        empty.get(oid(501)).and_then(|m| m.choice(KEY_A)),
        Some(&chosen)
    );
}

// REQ-EVENT-001, CR 614.12a, CR 607.2d — "choose a creature type": cites CR 614.12a,
// option_type CREATURE_TYPE, stored as ChoiceValue::CreatureType; each allowed option can be
// chosen, and the default is the first allowed option.
#[test]
fn req_event_001_acceptance_2_choose_creature_type_each_option() {
    for pick in [OPT_A, OPT_B, OPT_C] {
        let env = env_n(1);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![fx(
            4,
            choose(
                KEY_A,
                K::CreatureType,
                &[(OPT_A, 0), (OPT_B, 0), (OPT_C, 0)],
            ),
        )]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let run = drive_ok(
            &ctx,
            &[entry(1, P2)],
            &no_budget(),
            &[Ans::Text(pick.to_owned())],
        );
        // Source None: the reason's source is the entering object.
        assert_eq!(
            run.requests,
            vec![option_request(
                0,
                P2,
                "CR 614.12a",
                Some(oid(1)),
                "CREATURE_TYPE",
                &[OPT_A, OPT_B, OPT_C]
            )]
        );
        let v = MemChoice::CreatureType(pick.to_owned());
        assert_eq!(run.out[0].memory.choice(KEY_A), Some(&v));
        assert_eq!(run.out[0].projected.chosen.get(KEY_A), Some(&v));
    }
}

// REQ-EVENT-001, CR 614.12c, CR 607.2f — anchor words: the request cites CR 614.12c,
// option_type ANCHOR_WORD, the value is stored as ChoiceValue::Anchor. The reason's source is
// the effect's source when it has one.
#[test]
fn req_event_001_anchor_word_choice() {
    let env = env_n(2);
    let cont = ContinuousEffects::new();
    // Source object 2 (not entering in this call) affecting other permanents.
    let effects = effects_of(vec![
        fx(1, choose(KEY_A, K::Anchor, &[(OPT_A, 0), (OPT_B, 0)])).with_source(oid(2)),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(
        &ctx,
        &[entry(1, P1)],
        &no_budget(),
        &[Ans::Text(OPT_A.to_owned())],
    );
    assert_eq!(
        run.requests,
        vec![option_request(
            0,
            P1,
            "CR 614.12c",
            Some(oid(2)),
            "ANCHOR_WORD",
            &[OPT_A, OPT_B]
        )]
    );
    let v = MemChoice::Anchor(OPT_A.to_owned());
    assert_eq!(run.out[0].memory.choice(KEY_A), Some(&v));
    assert_eq!(run.out[0].projected.chosen.get(KEY_A), Some(&v));

    // The anchor-word ability's own permanent: ThisPermanent with its own source.
    let effects = effects_of(vec![own_fx(
        1,
        1,
        choose(KEY_A, K::Anchor, &[(OPT_A, 0), (OPT_B, 0)]),
    )]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(
        &ctx,
        &[entry(1, P1)],
        &no_budget(),
        &[Ans::Text(OPT_B.to_owned())],
    );
    assert_eq!(run.requests[0].reason.cr, "CR 614.12c");
    assert_eq!(run.requests[0].reason.source, Some(oid(1)));
    assert_eq!(
        run.out[0].memory.choice(KEY_A),
        Some(&MemChoice::Anchor(OPT_B.to_owned()))
    );
}

// REQ-EVENT-001, CR 614.12a — exactly one allowed option is chosen without asking.
#[test]
fn req_event_001_r5_single_option_not_asked() {
    for kind in [K::Color, K::CreatureType, K::Anchor] {
        let env = env_n(1);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![fx(1, choose(KEY_A, kind, &[(OPT_C, 0)]))]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
        assert!(run.requests.is_empty(), "{kind:?}");
        let expected = match kind {
            K::Color => MemChoice::Color(OPT_C.to_owned()),
            K::CreatureType => MemChoice::CreatureType(OPT_C.to_owned()),
            K::Anchor => MemChoice::Anchor(OPT_C.to_owned()),
        };
        assert_eq!(run.out[0].memory.choice(KEY_A), Some(&expected));
        assert_eq!(run.out[0].projected.chosen.get(KEY_A), Some(&expected));
    }
}

// REQ-EVENT-001, CR 607.2d — a later Choose with the same key overwrites it (the last chosen
// value); the first choice is part of the projection (its `applied.chosen`) when the second is
// asked. Requests are 0 then 1 in tier order.
#[test]
fn req_event_001_r5_same_key_last_chosen_wins() {
    for p in permutations(&[1, 2]) {
        let (first, second) = (p[0], p[1]);
        let env = env_n(1);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![
            fx(first, choose(KEY_A, K::Color, &[(OPT_A, 0), (OPT_B, 0)]))
                .with_tier(Tier::SelfReplacement),
            fx(
                second,
                choose(KEY_A, K::CreatureType, &[(OPT_B, 0), (OPT_C, 0)]),
            ),
        ]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let run = drive_ok(
            &ctx,
            &[entry(1, P1)],
            &no_budget(),
            &[Ans::Text(OPT_A.to_owned()), Ans::Text(OPT_C.to_owned())],
        );
        assert_eq!(run.requests.len(), 2);
        assert_eq!(run.requests[0].request_id, RequestId::new(0));
        assert_eq!(run.requests[1].request_id, RequestId::new(1));
        assert_eq!(
            run.requests[0].payload,
            ChoicePayload::ChooseOptionOrValue {
                option_type: "COLOR".to_owned(),
                allowed: AllowedValues::Texts(vec![OPT_A.to_owned(), OPT_B.to_owned()])
            }
        );
        assert_eq!(
            run.requests[1].payload,
            ChoicePayload::ChooseOptionOrValue {
                option_type: "CREATURE_TYPE".to_owned(),
                allowed: AllowedValues::Texts(vec![OPT_B.to_owned(), OPT_C.to_owned()])
            }
        );
        let v = MemChoice::CreatureType(OPT_C.to_owned());
        assert_eq!(run.out[0].memory.choice(KEY_A), Some(&v));
        assert_eq!(run.out[0].projected.chosen.get(KEY_A), Some(&v));
        assert_eq!(run.out[0].projected.effects, ids(&[1, 2]));
    }
}

// REQ-EVENT-001, CR 614.12a — the chosen value is part of the projection: `project` given an
// `applied.chosen` carries it into `projected.applied.chosen` unchanged.
#[test]
fn req_event_001_r5_project_carries_chosen() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![fx(
        1,
        choose(KEY_A, K::Color, &[(OPT_A, 0), (OPT_B, 0)]),
    )]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let applied = EtbApplied {
        effects: ids(&[1]),
        chosen: BTreeMap::from([(KEY_A.to_owned(), MemChoice::Color(OPT_B.to_owned()))]),
        ..Default::default()
    };
    let p = proj_ok(&ctx, &entry(1, P1), &applied);
    assert_eq!(p.applied.chosen, applied.chosen);
    assert_eq!(p.applied.effects, applied.effects);
    assert!(appl_ok(&ctx, &entry(1, P1), &applied).is_empty());
}

// REQ-EVENT-001, CR 614.12a — a text outside the allowed list, or an answer of the wrong kind,
// is rejected and the session is unchanged.
#[test]
fn req_event_001_r5_answer_outside_allowed_rejected() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![fx(
        1,
        choose(KEY_A, K::Color, &[(OPT_A, 0), (OPT_B, 0)]),
    )]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let entries = vec![entry(1, P1)];
    let payable = no_budget();
    let procedure =
        |_: &mut Nil, cx: &mut ChoiceContext<'_>| resolve_entering(&ctx, &entries, &payable, cx);
    let mut session = match ChoiceSession::start(Nil, procedure) {
        Ok(s) => s,
        Err(e) => panic!("start failed: {e}"),
    };
    let before = session.pending().cloned();
    assert!(before.is_some());
    for bad in [
        ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Text(OPT_C.to_owned()),
        },
        ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Number(0),
        },
        ChoiceResponse::SelectReplacement { index: 0 },
    ] {
        assert!(
            session.submit(RequestId::new(0), bad.clone()).is_err(),
            "{bad:?}"
        );
        assert_eq!(session.pending().cloned(), before);
        assert!(session.answers().is_empty());
    }
}

// =============================================================================================
// TESTS REQUIRED 4 / R-2 — scope (CR 614.12: "Permanents enter tapped" does not affect itself)
// =============================================================================================

// REQ-EVENT-001, CR 614.12 — a general "enters tapped" OtherPermanents effect sourced on A:
// entering A (same call) is untapped, entering B is tapped; in both slice orders.
#[test]
fn req_event_001_scope_general_effect_does_not_affect_its_source() {
    for order in [[1u64, 2], [2, 1]] {
        let env = env_n(2);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![fx(1, EtbAction::EntersTapped).with_source(oid(1))]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let entries: Vec<EtbEntry> = order.iter().map(|&o| entry(o, P1)).collect();
        let run = drive_ok(&ctx, &entries, &no_budget(), &[]);
        assert_eq!(run.out.len(), 2);
        for (r, &o) in run.out.iter().zip(order.iter()) {
            assert_eq!(r.object, oid(o), "one result per entry, in slice order");
            assert_eq!(r.projected.object, oid(o));
            if o == 1 {
                assert!(!r.projected.tapped);
                assert!(r.projected.effects.is_empty());
            } else {
                assert!(r.projected.tapped);
                assert_eq!(r.projected.effects, ids(&[1]));
            }
        }
        assert!(appl_ok(&ctx, &entry(1, P1), &EtbApplied::default()).is_empty());
        assert_eq!(
            appl_ok(&ctx, &entry(2, P1), &EtbApplied::default()),
            vec![eid(1)]
        );
    }
}

// REQ-EVENT-001, CR 614.12 — a ThisPermanent effect applies only to its source; a sourceless
// OtherPermanents effect applies to every entering object; a sourceless ThisPermanent effect
// applies to none.
#[test]
fn req_event_001_scope_this_permanent_and_sourceless() {
    let env = env_n(3);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![
        own_fx(1, 2, counters(CTR_A, 1)),
        fx(2, EtbAction::EntersTapped),
        fx(3, counters(CTR_B, 1)).with_scope(EtbScope::ThisPermanent),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    for o in 1..=3 {
        let expected = if o == 2 {
            vec![eid(1), eid(2)]
        } else {
            vec![eid(2)]
        };
        assert_eq!(
            appl_ok(&ctx, &entry(o, P1), &EtbApplied::default()),
            expected,
            "{o}"
        );
    }
    // Effect 1 (Other) and 2 (Other) for object 2 need a choice; answer index 0.
    let entries = vec![entry(1, P1), entry(2, P1), entry(3, P1)];
    let run = drive_ok(&ctx, &entries, &no_budget(), &[Ans::Index(0)]);
    assert_eq!(
        run.requests,
        vec![replacement_request(0, P1, 2, &[desc(1), desc(2)])]
    );
    assert_eq!(run.out[0].projected.effects, ids(&[2]));
    assert_eq!(run.out[1].projected.effects, ids(&[1, 2]));
    assert_eq!(run.out[2].projected.effects, ids(&[2]));
    for r in &run.out {
        assert!(r.projected.tapped);
        assert!(!r.projected.counters.contains_key(CTR_B));
    }
    assert_eq!(
        run.out[1].projected.counters,
        BTreeMap::from([(CTR_A.to_owned(), 1)])
    );
}

// REQ-EVENT-001, CR 614.12 — `applicable` excludes effects already applied and is ascending.
#[test]
fn req_event_001_r2_applicable_ascending_excludes_applied() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![
        fx(9, EtbAction::EntersTapped),
        fx(4, counters(CTR_A, 1)),
        fx(6, counters(CTR_B, 2)).with_tier(Tier::SelfReplacement),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let e = entry(1, P1);
    assert_eq!(
        appl_ok(&ctx, &e, &EtbApplied::default()),
        vec![eid(4), eid(6), eid(9)]
    );
    assert_eq!(appl_ok(&ctx, &e, &applied_of(&[6])), vec![eid(4), eid(9)]);
    assert!(appl_ok(&ctx, &e, &applied_of(&[4, 6, 9])).is_empty());
}

// =============================================================================================
// TESTS REQUIRED 5 / R-1 — copy (CR 614.12, CR 616.1c)
// =============================================================================================

// REQ-EVENT-001, CR 614.12, CR 616.1c — an applied EntersAsCopyOf replaces the base; the
// entry's own statics and own ThisPermanent effects stop applying (compared with the run
// without the copy); the copy (CopyEtb) is taken before the Other effects.
#[test]
fn req_event_001_copy_replaces_base_and_own_abilities() {
    let model = artifact("test.etb_copy_model");
    for p in permutations(&[1, 2, 3]) {
        let (k, t, h) = (p[0], p[1], p[2]);
        let env = env_n(1);
        let cont = ContinuousEffects::new();
        let statics = vec![own_static(100, 1, Layer::L4, add_types_mod(&[TYPE_X]))];
        let e = entry_with(1, P1, statics);
        let with_copy = effects_of(vec![
            fx(k, copy_of(model.clone())).with_tier(Tier::CopyEtb),
            own_fx(t, 1, counters(CTR_A, 1)),
            fx(h, EtbAction::EntersTapped)
                .with_condition(EtbCondition::HasCardType(TYPE_X.to_owned())),
        ]);
        let without_copy = effects_of(vec![
            own_fx(t, 1, counters(CTR_A, 1)),
            fx(h, EtbAction::EntersTapped)
                .with_condition(EtbCondition::HasCardType(TYPE_X.to_owned())),
        ]);

        // Without the copy: the own static gives test.type_x; T and H apply (asked: same tier).
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &without_copy,
        };
        assert_eq!(
            appl_ok(&ctx, &e, &EtbApplied::default()),
            ascending(&[t, h])
        );
        let run = drive_ok(
            &ctx,
            std::slice::from_ref(&e),
            &no_budget(),
            &[Ans::Index(0)],
        );
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.out[0].projected.effects, ids(&[t, h]));
        assert!(run.out[0].projected.tapped);
        assert!(
            run.out[0]
                .projected
                .evaluated
                .characteristics
                .card_types
                .contains(TYPE_X)
        );

        // With the copy: K applies first without a request; then neither T nor H applies.
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &with_copy,
        };
        assert_eq!(
            appl_ok(&ctx, &e, &EtbApplied::default()),
            ascending(&[k, t, h])
        );
        assert!(appl_ok(&ctx, &e, &applied_of(&[k])).is_empty(), "{p:?}");
        let run = drive_ok(&ctx, std::slice::from_ref(&e), &no_budget(), &[]);
        assert!(run.requests.is_empty(), "{p:?}");
        let r = &run.out[0].projected;
        assert_eq!(r.effects, ids(&[k]), "{p:?}");
        assert!(!r.tapped);
        assert!(r.counters.is_empty());
        assert_eq!(r.evaluated.characteristics, norm(&model));
    }
}

// REQ-EVENT-001, CR 614.12, CR 616.1c — `project` with a copy applied: the base is the copy's,
// the own statics are not registered, continuous effects already in force still apply; an own
// ThisPermanent effect applied before the copy keeps its result.
#[test]
fn req_event_001_copy_project_details() {
    let model = creature("test.etb_copy_model_c", 1, 1);
    let env = env_n(1);
    let mut cont = ContinuousEffects::new();
    cont.register(
        eid(200),
        in_force(
            1,
            1,
            Layer::L7c,
            LayerModification::ModifyPt {
                power: 1,
                toughness: 1,
            },
        ),
    )
    .expect("register");
    let effects = effects_of(vec![
        fx(1, copy_of(model.clone())).with_tier(Tier::CopyEtb),
        own_fx(2, 1, counters(CTR_A, 2)),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let e = entry_with(
        1,
        P1,
        vec![own_static(100, 1, Layer::L4, add_types_mod(&[TYPE_X]))],
    );

    let plain = proj_ok(&ctx, &e, &EtbApplied::default());
    assert!(plain.evaluated.characteristics.card_types.contains(TYPE_X));
    assert_eq!(pt(&plain.evaluated), spt(3, 3));

    let copied = proj_ok(&ctx, &e, &applied_of(&[1]));
    assert_eq!(copied.evaluated.characteristics.names, model.names);
    assert_eq!(
        copied.evaluated.characteristics.card_types,
        set(&[CREATURE])
    );
    assert_eq!(pt(&copied.evaluated), spt(2, 2));

    // Own ThisPermanent effect applied before the copy: its result is kept.
    let both = proj_ok(&ctx, &e, &applied_of(&[1, 2]));
    assert_eq!(both.counters, BTreeMap::from([(CTR_A.to_owned(), 2)]));
    assert_eq!(both.evaluated.characteristics.names, model.names);
}

// REQ-EVENT-001, CR 614.12, CR 616.1c, CR 707.2 — the documented fallback of an EMPTY `order`:
// `EtbApplied::sequence` is then the ascending-id order, so several applied copies replace the
// base in ascending EffectId order (the highest id is the final base), in both id assignments.
// The order-aware rule (the last APPLIED copy wins) is
// `req_event_001_f1_project_follows_order_last_copy_wins` below.
#[test]
fn req_event_001_copy_ascending_effect_id_empty_order_fallback() {
    let m1 = artifact("test.etb_model_one");
    let m2 = enchantment("test.etb_model_two");
    for (low, high) in [(m1.clone(), m2.clone()), (m2, m1)] {
        let env = env_n(1);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![
            fx(1, copy_of(low.clone())).with_tier(Tier::CopyEtb),
            fx(2, copy_of(high.clone())).with_tier(Tier::CopyEtb),
        ]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let both = applied_of(&[1, 2]);
        assert!(both.order.is_empty(), "the fallback case: no order");
        assert_eq!(both.sequence(), vec![eid(1), eid(2)]);
        let p = proj_ok(&ctx, &entry(1, P1), &both);
        assert_eq!(p.evaluated.characteristics, norm(&high));
        let p = proj_ok(&ctx, &entry(1, P1), &applied_of(&[1]));
        assert_eq!(p.evaluated.characteristics, norm(&low));
    }
}

// =============================================================================================
// TESTS REQUIRED 6 / R-4 — tiers and repeat (CR 616.1a-f, CR 616.2)
// =============================================================================================

/// S1, S2 (SelfReplacement), K (CopyEtb, a creature model), X1, X2 (Other). S2 adds
/// test.type_y; S1 applies only while the permanent lacks it (so the order S1/S2 is observable).
fn tier_effects(s1: u64, s2: u64, k: u64, x1: u64, x2: u64) -> EtbEffects {
    effects_of(vec![
        fx(s1, counters(CTR_A, 1))
            .with_tier(Tier::SelfReplacement)
            .with_condition(EtbCondition::LacksCardType(TYPE_Y.to_owned())),
        fx(s2, add_card_types(&[TYPE_Y])).with_tier(Tier::SelfReplacement),
        fx(k, copy_of(creature("test.etb_tier_model", 4, 4))).with_tier(Tier::CopyEtb),
        fx(x1, EtbAction::EntersTapped),
        fx(x2, counters(CTR_B, 1)),
    ])
}

// REQ-EVENT-001, CR 616.1a, CR 616.1c, CR 616.1e, CR 616.1f — SelfReplacement, CopyEtb and
// Other all applicable: the first request offers only the SelfReplacement effects, the second
// only the Other ones (the copy applied alone in between); each answer index is honoured;
// nothing applies twice. Every assignment of ids.
#[test]
fn req_event_001_tiers_in_order_with_requests() {
    for p in permutations(&[1, 2, 3, 4, 5]) {
        let (s1, s2, k, x1, x2) = (p[0], p[1], p[2], p[3], p[4]);
        let env = env_n(1);
        let cont = ContinuousEffects::new();
        let effects = tier_effects(s1, s2, k, x1, x2);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let self_descs: Vec<String> = ascending(&[s1, s2]).iter().map(|e| desc(e.get())).collect();
        let other_descs: Vec<String> = ascending(&[x1, x2]).iter().map(|e| desc(e.get())).collect();

        for idx0 in [0u32, 1] {
            for idx1 in [0u32, 1] {
                let run = drive_ok(
                    &ctx,
                    &[entry(1, P1)],
                    &no_budget(),
                    &[Ans::Index(idx0), Ans::Index(idx1)],
                );
                let first_pick = &self_descs[idx0 as usize];
                if *first_pick == desc(s1) {
                    // S1 first, then S2 alone, then K, then the Other pair.
                    assert_eq!(
                        run.requests,
                        vec![
                            replacement_request(0, P1, 1, &self_descs),
                            replacement_request(1, P1, 1, &other_descs),
                        ],
                        "{p:?}"
                    );
                    assert_eq!(run.out[0].projected.effects, ids(&[s1, s2, k, x1, x2]));
                    assert_eq!(
                        run.out[0].projected.counters,
                        BTreeMap::from([(CTR_A.to_owned(), 1), (CTR_B.to_owned(), 1)])
                    );
                } else {
                    // S2 first makes S1 inapplicable.
                    assert_eq!(
                        run.requests,
                        vec![
                            replacement_request(0, P1, 1, &self_descs),
                            replacement_request(1, P1, 1, &other_descs),
                        ],
                        "{p:?}"
                    );
                    assert_eq!(run.out[0].projected.effects, ids(&[s2, k, x1, x2]));
                    assert_eq!(
                        run.out[0].projected.counters,
                        BTreeMap::from([(CTR_B.to_owned(), 1)])
                    );
                }
                let r = &run.out[0].projected;
                assert!(r.tapped);
                assert_eq!(
                    r.evaluated.characteristics.names,
                    set(&["test.etb_tier_model"])
                );
                // AddCardTypes is a layer-4 addition (CR 613.1d, CR 205.1b): it survives the
                // layer-1 copy.
                assert_eq!(
                    r.evaluated.characteristics.card_types,
                    set(&[CREATURE, TYPE_Y])
                );
                assert_eq!(pt(&r.evaluated), spt(4, 4));
            }
        }
    }
}

// REQ-EVENT-001, CR 616.1, CR 616.1f — an out-of-range or wrong-kind answer to the
// CHOOSE_REPLACEMENT_EFFECT request is rejected and the session is unchanged.
#[test]
fn req_event_001_tiers_bad_replacement_answer_rejected() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![
        fx(1, EtbAction::EntersTapped),
        fx(2, counters(CTR_A, 1)),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let entries = vec![entry(1, P1)];
    let payable = no_budget();
    let procedure =
        |_: &mut Nil, cx: &mut ChoiceContext<'_>| resolve_entering(&ctx, &entries, &payable, cx);
    let mut session = match ChoiceSession::start(Nil, procedure) {
        Ok(s) => s,
        Err(e) => panic!("start failed: {e}"),
    };
    let pending = session.pending().cloned();
    assert_eq!(
        pending,
        Some(replacement_request(0, P1, 1, &[desc(1), desc(2)]))
    );
    for bad in [
        ChoiceResponse::SelectReplacement { index: 2 },
        ChoiceResponse::SelectReplacement { index: u32::MAX },
        ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Text(desc(1)),
        },
        ChoiceResponse::PassPriority,
    ] {
        assert!(
            session.submit(RequestId::new(0), bad.clone()).is_err(),
            "{bad:?}"
        );
        assert_eq!(session.pending().cloned(), pending);
        assert!(session.answers().is_empty());
    }
    // The default is a valid answer and applies effect 1 first; nothing applies twice.
    session
        .submit_default(RequestId::new(0))
        .expect("default accepted");
    let Status::Done { output, .. } = session.status() else {
        panic!("done after one answer");
    };
    assert_eq!(output[0].projected.applied.effects, ids(&[1, 2]));
    assert_eq!(
        output[0].projected.counters,
        BTreeMap::from([(CTR_A.to_owned(), 1)])
    );
}

// REQ-EVENT-001, CR 616.1b, CR 616.1d, CR 616.1e — the full tier order SelfReplacement,
// ControlEtb, CopyEtb, BackFaceEtb, Other: each effect gates the next lower tier (it needs the
// type added by the effect of the tier just above), so the run completes without a request only
// if the tiers are taken in that order; ids in reverse and forward order.
#[test]
fn req_event_001_tiers_full_order() {
    let tags = [
        "test.tier_0",
        "test.tier_1",
        "test.tier_2",
        "test.tier_3",
        "test.tier_4",
    ];
    for reverse in [false, true] {
        let env = env_n(1);
        let cont = ContinuousEffects::new();
        let mut list = Vec::new();
        for (i, tier) in ALL_TIERS.iter().enumerate() {
            let n = if reverse { 5 - i as u64 } else { i as u64 + 1 };
            let mut f = fx(n, add_card_types(&[tags[i]])).with_tier(*tier);
            // Every effect except the first is applicable only while the permanent lacks the
            // next tier's tag: taken out of order, an effect would be skipped.
            if i + 1 < ALL_TIERS.len() {
                f = f.with_condition(EtbCondition::LacksCardType(tags[i + 1].to_owned()));
            }
            list.push(f);
        }
        let effects = effects_of(list);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
        assert!(run.requests.is_empty());
        assert_eq!(
            run.out[0].projected.effects,
            ids(&[1, 2, 3, 4, 5]),
            "reverse {reverse}"
        );
        let types = &run.out[0].projected.evaluated.characteristics.card_types;
        for t in tags {
            assert!(types.contains(t));
        }
    }
}

// REQ-EVENT-001, CR 616.1f — no effect applies twice to one entry, and an effect applied to
// one entry still applies to the next.
#[test]
fn req_event_001_tiers_nothing_applies_twice() {
    let env = env_n(2);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![fx(1, counters(CTR_A, 3))]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(&ctx, &[entry(1, P1), entry(2, P1)], &no_budget(), &[]);
    for r in &run.out {
        assert_eq!(r.projected.effects, ids(&[1]));
        assert_eq!(
            r.projected.counters,
            BTreeMap::from([(CTR_A.to_owned(), 3)])
        );
    }
}

// =============================================================================================
// TESTS REQUIRED 7 / R-1, R-6 — own statics and continuous effects in force (CR 614.12)
// =============================================================================================

// REQ-EVENT-001, CR 614.12 — the base says off the battlefield, yet the projection is of the
// permanent "as it would exist on the battlefield": power/toughness are present, continuous
// effects in force that affect it apply.
#[test]
fn req_event_001_project_treats_entering_as_on_battlefield() {
    let env = env_of(&[(1, creature("test.etb_body_pt", 2, 3), P2)]);
    assert!(!env.bases[&oid(1)].on_battlefield);
    let cont = ContinuousEffects::new();
    let effects = EtbEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let p = proj_ok(&ctx, &entry(1, P2), &EtbApplied::default());
    assert_eq!(p.object, oid(1));
    assert_eq!(pt(&p.evaluated), spt(2, 3));
    assert!(!p.tapped);
    assert!(p.counters.is_empty());
    assert!(p.applied.effects.is_empty());
    assert!(p.applied.chosen.is_empty());

    let mut cont = ContinuousEffects::new();
    cont.register(
        eid(10),
        in_force(
            1,
            1,
            Layer::L7c,
            LayerModification::ModifyPt {
                power: 1,
                toughness: 1,
            },
        ),
    )
    .expect("register");
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let p = proj_ok(&ctx, &entry(1, P2), &EtbApplied::default());
    assert_eq!(pt(&p.evaluated), spt(3, 4));
}

// REQ-EVENT-001, CR 614.12 (continuous effects that already exist), CR 616.2 — a continuous
// effect in force changing types (layer 4, by object or by selector) changes the projection and
// which replacements apply.
#[test]
fn req_event_001_continuous_in_force_changes_types() {
    let env = env_n(1);
    let effects = effects_of(vec![
        fx(1, EtbAction::EntersTapped).with_condition(EtbCondition::HasCardType(TYPE_X.to_owned())),
        fx(2, counters(CTR_A, 1)).with_condition(EtbCondition::LacksCardType(TYPE_X.to_owned())),
    ]);
    for affected in [
        Affected::Objects([oid(1)].into_iter().collect()),
        Affected::Selector(sel(SEL_CREATURE)),
    ] {
        let mut cont = ContinuousEffects::new();
        cont.register(
            eid(300),
            eff(Layer::L4, 1, affected, add_types_mod(&[TYPE_X])),
        )
        .expect("register");
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let p = proj_ok(&ctx, &entry(1, P1), &EtbApplied::default());
        assert!(types_of(&p).contains(TYPE_X));
        assert_eq!(
            appl_ok(&ctx, &entry(1, P1), &EtbApplied::default()),
            vec![eid(1)]
        );
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
        assert_eq!(run.out[0].projected.effects, ids(&[1]));
        assert!(run.out[0].projected.tapped);
    }
    // A layer-4 SetTypes in force, then AddCardTypes applied: the added type survives (its
    // layer-4 effect has a later timestamp, CR 613.7, CR 205.1b).
    let mut cont = ContinuousEffects::new();
    cont.register(
        eid(301),
        in_force(
            1,
            1,
            Layer::L4,
            LayerModification::SetTypes {
                supertypes: None,
                card_types: Some(set(&[ARTIFACT])),
                subtypes: None,
            },
        ),
    )
    .expect("register");
    let effects = effects_of(vec![fx(5, add_card_types(&[TYPE_X]))]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let p = proj_ok(&ctx, &entry(1, P1), &applied_of(&[5]));
    assert_eq!(types_of(&p), set(&[ARTIFACT, TYPE_X]));
    let p = proj_ok(&ctx, &entry(1, P1), &EtbApplied::default());
    assert_eq!(types_of(&p), set(&[ARTIFACT]));
}

// REQ-EVENT-001, CR 614.12, CR 614.12a (R-6) — own statics are not leaked into the caller's
// ContinuousEffects, and no input is mutated by project, applicable or resolve_entering.
#[test]
fn req_event_001_r6_no_input_mutated() {
    let env = env_n(2);
    let env_before = env.bases.clone();
    let mut cont = ContinuousEffects::new();
    cont.register(
        eid(300),
        in_force(2, 1, Layer::L4, add_types_mod(&[TYPE_Y])),
    )
    .expect("register");
    let cont_before = cont.clone();
    let effects = effects_of(vec![
        fx(1, EtbAction::EntersTapped).with_condition(EtbCondition::HasCardType(TYPE_X.to_owned())),
        own_fx(2, 1, choose(KEY_A, K::Color, &[(OPT_A, 1), (OPT_B, 2)]))
            .with_tier(Tier::SelfReplacement),
        fx(3, copy_of(artifact("test.etb_r6_model")))
            .with_tier(Tier::CopyEtb)
            .with_condition(EtbCondition::HasCardType(TYPE_Y.to_owned())),
        fx(4, counters(CTR_A, 1)),
    ]);
    let effects_before = effects_snapshot(&effects, &[1, 2, 3, 4, 5]);
    let entries = vec![
        entry_with(
            1,
            P1,
            vec![own_static(100, 1, Layer::L4, add_types_mod(&[TYPE_X]))],
        ),
        entry(2, P1),
    ];
    let entries_before: Vec<_> = entries.iter().map(entry_snapshot).collect();
    let payable = budget(&[(P1, 5)]);
    let payable_before = payable.clone();
    let applied = applied_of(&[4]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };

    let p1 = pobs(&proj_ok(&ctx, &entries[0], &applied));
    let a1 = appl_ok(&ctx, &entries[0], &applied);
    let run1 = drive_ok(
        &ctx,
        &entries,
        &payable,
        &[Ans::Text(OPT_B.to_owned()), Ans::Index(0)],
    );
    let p2 = pobs(&proj_ok(&ctx, &entries[0], &applied));
    let a2 = appl_ok(&ctx, &entries[0], &applied);
    let run2 = drive_ok(
        &ctx,
        &entries,
        &payable,
        &[Ans::Text(OPT_B.to_owned()), Ans::Index(0)],
    );

    assert_eq!(p1, p2);
    assert_eq!(a1, a2);
    assert_eq!(run1.out, run2.out);
    assert_eq!(run1.requests, run2.requests);
    assert_eq!(env.bases, env_before);
    assert_eq!(cont, cont_before);
    assert!(
        cont.clone()
            .register(
                eid(100),
                in_force(1, 1, Layer::L4, add_types_mod(&[TYPE_X]))
            )
            .is_ok(),
        "the own static's id is not registered in the caller's ContinuousEffects"
    );
    assert_eq!(effects_snapshot(&effects, &[1, 2, 3, 4, 5]), effects_before);
    assert_eq!(
        entries.iter().map(entry_snapshot).collect::<Vec<_>>(),
        entries_before
    );
    assert_eq!(payable, payable_before);
    assert_eq!(applied.effects, ids(&[4]));
    assert!(applied.chosen.is_empty());
    assert!(applied.order.is_empty());

    // Sanity of the run itself: object 1 (own static type_x) gets 1, 2, 4; object 2 (type_y in
    // force) is copied first, then 4.
    assert_eq!(run1.out[0].projected.effects, ids(&[1, 2, 4]));
    assert_eq!(
        run1.out[0].memory.choice(KEY_A),
        Some(&MemChoice::Color(OPT_B.to_owned()))
    );
    assert_eq!(run1.out[1].projected.effects, ids(&[3, 4]));
}

// =============================================================================================
// TESTS REQUIRED 8 / R-7 — payability (CR 614.12b)
// =============================================================================================

/// Objects 1..=n each with its own Choose (ThisPermanent, effect id = object id) over `list`.
fn choose_per_entry(n: u64, kind: K, list: &[(&str, u64)]) -> EtbEffects {
    effects_of(
        (1..=n)
            .map(|o| own_fx(o, o, choose(&format!("test.key_{o}"), kind, list)))
            .collect(),
    )
}

fn key(o: u64) -> String {
    format!("test.key_{o}")
}

// REQ-EVENT-001, CR 614.12b — budget 3, two entries each choosing between costs {1, 3}: the
// first may not take 3 (3 + the second's cheapest 1 > 3), so cost 1 is chosen without a
// request; the second then has 2 left and also only cost 1.
#[test]
fn req_event_001_payable_lookahead_forces_cheap() {
    let env = env_n(2);
    let cont = ContinuousEffects::new();
    let effects = choose_per_entry(2, K::Color, &[(OPT_A, 1), (OPT_B, 3)]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(
        &ctx,
        &[entry(1, P1), entry(2, P1)],
        &budget(&[(P1, 3)]),
        &[],
    );
    assert!(run.requests.is_empty());
    for (i, r) in run.out.iter().enumerate() {
        let o = i as u64 + 1;
        assert_eq!(
            r.memory.choice(&key(o)),
            Some(&MemChoice::Color(OPT_A.to_owned()))
        );
    }
}

// REQ-EVENT-001, CR 614.12b — budget 1 for two entries that each need at least 1: not payable
// ("not payable" in a ChoiceError::Procedure); budget 0 for one entry needing 1 as well.
#[test]
fn req_event_001_payable_unpayable_errors() {
    let env = env_n(2);
    let cont = ContinuousEffects::new();
    let effects = choose_per_entry(2, K::Color, &[(OPT_A, 1), (OPT_B, 3)]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let e = drive_err(
        &ctx,
        &[entry(1, P1), entry(2, P1)],
        &budget(&[(P1, 1)]),
        &[],
    );
    match e {
        ChoiceError::Procedure(msg) => assert!(msg.contains("not payable"), "{msg}"),
        other => panic!("expected Procedure(not payable), got {other:?}"),
    }
    let e = drive_err(&ctx, &[entry(1, P1)], &budget(&[(P1, 0)]), &[]);
    match e {
        ChoiceError::Procedure(msg) => assert!(msg.contains("not payable"), "{msg}"),
        other => panic!("expected Procedure(not payable), got {other:?}"),
    }
    // The same entries with budget 2 are payable.
    let run = drive_ok(
        &ctx,
        &[entry(1, P1), entry(2, P1)],
        &budget(&[(P1, 2)]),
        &[],
    );
    assert_eq!(run.out.len(), 2);
}

// REQ-EVENT-001, CR 614.12b — budget 2, costs {1, 2}: one entry alone lists both; two entries
// list neither (the lookahead forces cost 1 for both, without requests); budget 3 with two
// entries lists both for the first, and the second's list depends on the first answer.
#[test]
fn req_event_001_payable_request_lists_follow_lookahead() {
    let env = env_n(2);
    let cont = ContinuousEffects::new();
    let effects = choose_per_entry(2, K::CreatureType, &[(OPT_A, 1), (OPT_B, 2)]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    // One entry, budget 2: both allowed.
    let run = drive_ok(
        &ctx,
        &[entry(1, P1)],
        &budget(&[(P1, 2)]),
        &[Ans::Text(OPT_B.to_owned())],
    );
    assert_eq!(
        run.requests,
        vec![option_request(
            0,
            P1,
            "CR 614.12a",
            Some(oid(1)),
            "CREATURE_TYPE",
            &[OPT_A, OPT_B]
        )]
    );
    assert_eq!(
        run.out[0].memory.choice(&key(1)),
        Some(&MemChoice::CreatureType(OPT_B.to_owned()))
    );

    // Two entries, budget 2: no request, both cost 1.
    let run = drive_ok(
        &ctx,
        &[entry(1, P1), entry(2, P1)],
        &budget(&[(P1, 2)]),
        &[],
    );
    assert!(run.requests.is_empty());
    for o in 1..=2u64 {
        assert_eq!(
            run.out[o as usize - 1].memory.choice(&key(o)),
            Some(&MemChoice::CreatureType(OPT_A.to_owned()))
        );
    }

    // Two entries, budget 3: first lists both. Choosing 2 leaves 1: the second is forced.
    let run = drive_ok(
        &ctx,
        &[entry(1, P1), entry(2, P1)],
        &budget(&[(P1, 3)]),
        &[Ans::Text(OPT_B.to_owned())],
    );
    assert_eq!(
        run.requests,
        vec![option_request(
            0,
            P1,
            "CR 614.12a",
            Some(oid(1)),
            "CREATURE_TYPE",
            &[OPT_A, OPT_B]
        )]
    );
    assert_eq!(
        run.out[1].memory.choice(&key(2)),
        Some(&MemChoice::CreatureType(OPT_A.to_owned()))
    );
    // Choosing 1 leaves 2: the second lists both.
    let run = drive_ok(
        &ctx,
        &[entry(1, P1), entry(2, P1)],
        &budget(&[(P1, 3)]),
        &[Ans::Text(OPT_A.to_owned()), Ans::Text(OPT_B.to_owned())],
    );
    assert_eq!(
        run.requests,
        vec![
            option_request(
                0,
                P1,
                "CR 614.12a",
                Some(oid(1)),
                "CREATURE_TYPE",
                &[OPT_A, OPT_B]
            ),
            option_request(
                1,
                P1,
                "CR 614.12a",
                Some(oid(2)),
                "CREATURE_TYPE",
                &[OPT_A, OPT_B]
            ),
        ]
    );
    assert_eq!(
        run.out[1].memory.choice(&key(2)),
        Some(&MemChoice::CreatureType(OPT_B.to_owned()))
    );
}

// REQ-EVENT-001, CR 614.12b — the reserve counts another Choose of the same entry that is
// applicable now (two Choose effects on one entry, budget 3, costs {1, 3} each).
#[test]
fn req_event_001_payable_reserve_same_entry() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![
        fx(1, choose(KEY_A, K::Color, &[(OPT_A, 1), (OPT_B, 3)])).with_tier(Tier::SelfReplacement),
        fx(2, choose(KEY_B, K::Color, &[(OPT_A, 1), (OPT_B, 3)])),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(&ctx, &[entry(1, P1)], &budget(&[(P1, 3)]), &[]);
    assert!(run.requests.is_empty());
    assert_eq!(
        run.out[0].memory.choice(KEY_A),
        Some(&MemChoice::Color(OPT_A.to_owned()))
    );
    assert_eq!(
        run.out[0].memory.choice(KEY_B),
        Some(&MemChoice::Color(OPT_A.to_owned()))
    );
    // Unlimited: both are asked with both options.
    let run = drive_ok(
        &ctx,
        &[entry(1, P1)],
        &no_budget(),
        &[Ans::Text(OPT_B.to_owned()), Ans::Text(OPT_B.to_owned())],
    );
    assert_eq!(run.requests.len(), 2);
    for r in &run.requests {
        assert_eq!(
            the_request_allowed(r),
            vec![OPT_A.to_owned(), OPT_B.to_owned()]
        );
    }
}

// REQ-EVENT-001, CR 614.12b — allowed options keep their declared order and the default is the
// first allowed one.
#[test]
fn req_event_001_payable_allowed_keep_declared_order() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![own_fx(
        1,
        1,
        choose(KEY_A, K::Color, &[(OPT_C, 5), (OPT_B, 2), (OPT_A, 1)]),
    )]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(
        &ctx,
        &[entry(1, P1)],
        &budget(&[(P1, 2)]),
        &[Ans::Text(OPT_A.to_owned())],
    );
    assert_eq!(
        run.requests,
        vec![option_request(
            0,
            P1,
            "CR 614.12a",
            Some(oid(1)),
            "COLOR",
            &[OPT_B, OPT_A]
        )]
    );
    let run = drive_ok(
        &ctx,
        &[entry(1, P1)],
        &budget(&[(P1, 9)]),
        &[Ans::Text(OPT_C.to_owned())],
    );
    assert_eq!(
        run.requests,
        vec![option_request(
            0,
            P1,
            "CR 614.12a",
            Some(oid(1)),
            "COLOR",
            &[OPT_C, OPT_B, OPT_A]
        )]
    );
    assert_eq!(
        run.out[0].memory.choice(KEY_A),
        Some(&MemChoice::Color(OPT_C.to_owned()))
    );
}

// REQ-EVENT-001, CR 614.12b — a chooser absent from `payable` is unlimited; cost 0 is always
// allowed; different choosers do not share budgets.
#[test]
fn req_event_001_payable_absent_zero_and_separate_choosers() {
    // Absent chooser (P2) while P1 has a budget of 0: all options listed.
    let env = env_of(&[(1, body(1), P2)]);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![own_fx(
        1,
        1,
        choose(KEY_A, K::Anchor, &[(OPT_A, 1), (OPT_B, 1_000_000)]),
    )]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(
        &ctx,
        &[entry(1, P2)],
        &budget(&[(P1, 0)]),
        &[Ans::Text(OPT_B.to_owned())],
    );
    assert_eq!(
        run.requests,
        vec![option_request(
            0,
            P2,
            "CR 614.12c",
            Some(oid(1)),
            "ANCHOR_WORD",
            &[OPT_A, OPT_B]
        )]
    );
    assert_eq!(
        run.out[0].memory.choice(KEY_A),
        Some(&MemChoice::Anchor(OPT_B.to_owned()))
    );

    // Cost 0 with budget 0: chosen without asking.
    let env = env_n(1);
    let effects = effects_of(vec![own_fx(
        1,
        1,
        choose(KEY_A, K::Color, &[(OPT_A, 0), (OPT_B, 2)]),
    )]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(&ctx, &[entry(1, P1)], &budget(&[(P1, 0)]), &[]);
    assert!(run.requests.is_empty());
    assert_eq!(
        run.out[0].memory.choice(KEY_A),
        Some(&MemChoice::Color(OPT_A.to_owned()))
    );

    // Two choosers with budget 1 each, options {1, 2}: neither reserves for the other, both
    // pay 1; in both slice orders.
    let env = env_of(&[(1, body(1), P1), (2, body(2), P2)]);
    let effects = choose_per_entry(2, K::Color, &[(OPT_A, 1), (OPT_B, 2)]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    for order in [[1u64, 2], [2, 1]] {
        let entries: Vec<EtbEntry> = order
            .iter()
            .map(|&o| entry(o, if o == 1 { P1 } else { P2 }))
            .collect();
        let run = drive_ok(&ctx, &entries, &budget(&[(P1, 1), (P2, 1)]), &[]);
        assert!(run.requests.is_empty());
        for (r, &o) in run.out.iter().zip(order.iter()) {
            assert_eq!(
                r.memory.choice(&key(o)),
                Some(&MemChoice::Color(OPT_A.to_owned()))
            );
        }
        // With the budgets shared, the second would be unpayable; P2 with budget 0 is.
        let e = drive_err(&ctx, &entries, &budget(&[(P1, 1), (P2, 0)]), &[]);
        assert!(
            matches!(e, ChoiceError::Procedure(ref m) if m.contains("not payable")),
            "{e:?}"
        );
    }
}

// REQ-EVENT-001, CR 614.12b — EtbError::Unpayable carries the chooser and key and displays a
// non-empty message.
#[test]
fn req_event_001_payable_unpayable_error_display() {
    let e = EtbError::Unpayable {
        chooser: P1,
        key: KEY_A.to_owned(),
    };
    assert!(!e.to_string().is_empty());
    let as_error: &dyn std::error::Error = &e;
    assert!(!as_error.to_string().is_empty());
}

// =============================================================================================
// TESTS REQUIRED 9 / R-8 — determinism and sessions (REQ-CORE-001, R3)
// =============================================================================================

/// A scenario with three requests: a same-tier replacement choice, a color, an anchor word.
fn three_request_effects() -> EtbEffects {
    effects_of(vec![
        fx(1, EtbAction::EntersTapped),
        fx(2, counters(CTR_A, 2)),
        own_fx(3, 1, choose(KEY_A, K::Color, &[(OPT_A, 0), (OPT_B, 0)]))
            .with_tier(Tier::SelfReplacement),
        own_fx(4, 2, choose(KEY_B, K::Anchor, &[(OPT_B, 0), (OPT_C, 0)]))
            .with_tier(Tier::SelfReplacement),
    ])
}

// REQ-EVENT-001, CR 614.12a, CR 616.1 (R3, REQ-CORE-001) — one-by-one answers equal a replay;
// request ids are 0, 1, 2, ... in ask order; a wrong answer is rejected with the session
// unchanged; two runs are equal.
#[test]
fn req_event_001_determinism_session_replay() {
    let env = env_n(2);
    let cont = ContinuousEffects::new();
    let effects = three_request_effects();
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let entries = vec![entry(1, P1), entry(2, P1)];
    let payable = no_budget();
    let script = [
        Ans::Text(OPT_B.to_owned()),
        Ans::Index(1),
        Ans::Text(OPT_C.to_owned()),
        Ans::Index(0),
    ];
    let a = drive_ok(&ctx, &entries, &payable, &script);
    let b = drive_ok(&ctx, &entries, &payable, &script);
    assert_eq!(a.out, b.out);
    assert_eq!(a.requests, b.requests);
    assert_eq!(a.answers, b.answers);
    assert_eq!(a.requests.len(), 4);
    for (i, r) in a.requests.iter().enumerate() {
        assert_eq!(r.request_id, RequestId::new(i as u64));
        assert_eq!(r.chooser, P1);
    }
    assert_eq!(
        a.requests[0],
        option_request(0, P1, "CR 614.12a", Some(oid(1)), "COLOR", &[OPT_A, OPT_B])
    );
    assert_eq!(
        a.requests[1],
        replacement_request(1, P1, 1, &[desc(1), desc(2)])
    );
    assert_eq!(
        a.requests[2],
        option_request(
            2,
            P1,
            "CR 614.12c",
            Some(oid(2)),
            "ANCHOR_WORD",
            &[OPT_B, OPT_C]
        )
    );
    assert_eq!(
        a.requests[3],
        replacement_request(3, P1, 2, &[desc(1), desc(2)])
    );

    // Replay all at once.
    let procedure =
        |_: &mut Nil, cx: &mut ChoiceContext<'_>| resolve_entering(&ctx, &entries, &payable, cx);
    let replayed = match ChoiceSession::replay(Nil, procedure, &a.answers) {
        Ok(s) => s,
        Err(e) => panic!("replay failed: {e}"),
    };
    let Status::Done { output, .. } = replayed.status() else {
        panic!("replay not done");
    };
    assert_eq!(output.iter().map(obs).collect::<Vec<_>>(), a.out);
    assert_eq!(replayed.answers(), &a.answers[..]);

    // Results.
    assert_eq!(
        a.out[0].memory.choice(KEY_A),
        Some(&MemChoice::Color(OPT_B.to_owned()))
    );
    assert_eq!(a.out[0].projected.effects, ids(&[1, 2, 3]));
    assert_eq!(
        a.out[1].memory.choice(KEY_B),
        Some(&MemChoice::Anchor(OPT_C.to_owned()))
    );
    assert_eq!(a.out[1].projected.effects, ids(&[1, 2, 4]));

    // Mid-session wrong answer: rejected, unchanged.
    let procedure =
        |_: &mut Nil, cx: &mut ChoiceContext<'_>| resolve_entering(&ctx, &entries, &payable, cx);
    let mut s = match ChoiceSession::replay(Nil, procedure, &a.answers[..2]) {
        Ok(s) => s,
        Err(e) => panic!("partial replay failed: {e}"),
    };
    let pending = s.pending().cloned().expect("pending");
    assert_eq!(pending.request_id, RequestId::new(2));
    let hash = s.state_hash();
    assert!(
        s.submit(
            RequestId::new(2),
            ChoiceResponse::SelectReplacement { index: 5 }
        )
        .is_err()
    );
    assert!(
        s.submit(
            RequestId::new(1),
            ChoiceResponse::SelectReplacement { index: 0 }
        )
        .is_err()
    );
    assert_eq!(s.pending(), Some(&pending));
    assert_eq!(s.answers(), &a.answers[..2]);
    assert_eq!(s.state_hash(), hash);
}

// =============================================================================================
// TESTS REQUIRED 10 — property tests (seeded, R3)
// =============================================================================================

fn below(rng: &mut Pcg64Dxsm, bound: u64) -> u64 {
    rng.next_bounded(bound).expect("bound > 0")
}

const TAGS: [&str; 3] = [TYPE_X, TYPE_Y, "test.type_z"];
const SUBS: [&str; 2] = [SUB_A, "test.sub_b"];

fn random_condition(rng: &mut Pcg64Dxsm) -> EtbCondition {
    match below(rng, 5) {
        0 | 1 => EtbCondition::Always,
        2 => EtbCondition::HasCardType(TAGS[below(rng, 3) as usize].to_owned()),
        3 => EtbCondition::LacksCardType(TAGS[below(rng, 3) as usize].to_owned()),
        _ => EtbCondition::HasSubtype(SUBS[below(rng, 2) as usize].to_owned()),
    }
}

fn random_action(rng: &mut Pcg64Dxsm, n: u64) -> EtbAction {
    match below(rng, 6) {
        0 => EtbAction::EntersTapped,
        1 => counters(
            [CTR_A, CTR_B][below(rng, 2) as usize],
            1 + below(rng, 3) as i64,
        ),
        2 => add_card_types(&[TAGS[below(rng, 3) as usize]]),
        3 => {
            let mut c = creature(&format!("test.etb_prop_model_{n}"), 1, 1);
            c.card_types.insert(TAGS[below(rng, 3) as usize].to_owned());
            if below(rng, 2) == 0 {
                c.subtypes.insert(SUBS[below(rng, 2) as usize].to_owned());
            }
            copy_of(c)
        }
        _ => {
            let kind = [K::Color, K::CreatureType, K::Anchor][below(rng, 3) as usize];
            let all = [(OPT_A, 0), (OPT_B, 0), (OPT_C, 0)];
            let k = 1 + below(rng, 3) as usize;
            choose([KEY_A, KEY_B][below(rng, 2) as usize], kind, &all[..k])
        }
    }
}

/// Up to 8 random effects over `objects`. Sources are None or one of the objects.
fn random_effects(rng: &mut Pcg64Dxsm, objects: u64) -> EtbEffects {
    let count = 1 + below(rng, 8);
    let mut list = Vec::new();
    for n in 1..=count {
        let mut f = fx(n, random_action(rng, n))
            .with_tier(ALL_TIERS[below(rng, 5) as usize])
            .with_condition(random_condition(rng));
        if below(rng, 2) == 0 {
            f = f.with_scope(EtbScope::ThisPermanent);
        }
        if below(rng, 3) > 0 {
            f = f.with_source(oid(1 + below(rng, objects)));
        }
        list.push(f);
    }
    effects_of(list)
}

fn random_env(rng: &mut Pcg64Dxsm, objects: u64) -> TestEnv {
    let objs: Vec<(u64, Characteristics, PlayerId)> = (1..=objects)
        .map(|o| {
            let mut c = body(o);
            if below(rng, 2) == 0 {
                c.card_types.insert(TAGS[below(rng, 3) as usize].to_owned());
            }
            if below(rng, 3) == 0 {
                c.subtypes.insert(SUBS[below(rng, 2) as usize].to_owned());
            }
            (o, c, if below(rng, 2) == 0 { P1 } else { P2 })
        })
        .collect();
    env_of(&objs)
}

fn random_entry(rng: &mut Pcg64Dxsm, env: &TestEnv, o: u64) -> EtbEntry {
    let controller = env.bases[&oid(o)].controller.expect("controlled");
    let mut statics = Vec::new();
    if below(rng, 3) == 0 {
        statics.push(own_static(
            100 + o,
            o,
            Layer::L4,
            add_types_mod(&[TAGS[below(rng, 3) as usize]]),
        ));
    }
    entry_with(o, controller, statics)
}

/// A content-only answer (independent of the request id): the last offered effect, or the last
/// allowed text.
fn content_answer(_: usize, req: &PendingChoiceRequest) -> ChoiceResponse {
    match &req.payload {
        ChoicePayload::ChooseReplacementEffect { effects, .. } => {
            ChoiceResponse::SelectReplacement {
                index: u32::try_from(effects.len() - 1).expect("small"),
            }
        }
        ChoicePayload::ChooseOptionOrValue {
            allowed: AllowedValues::Texts(t),
            ..
        } => ChoiceResponse::SubmitChoiceValue {
            value: ChoiceValue::Text(t.last().expect("non-empty").clone()),
        },
        other => panic!("unexpected request payload {other:?}"),
    }
}

fn random_subset(rng: &mut Pcg64Dxsm, max: u64) -> BTreeSet<EffectId> {
    (1..=max).filter(|_| below(rng, 2) == 0).map(eid).collect()
}

// REQ-EVENT-001, CR 614.12, CR 616.1f — properties of `applicable` and `project` over seeded
// random effect sets: ascending, excludes applied ids, an applied effect never reappears;
// `project` is idempotent and changes no input.
#[test]
fn req_event_001_prop_applicable_and_project() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0e7b_0001);
    for _ in 0..300 {
        let env = random_env(&mut rng, 3);
        let effects = random_effects(&mut rng, 3);
        let cont = ContinuousEffects::new();
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let o = 1 + below(&mut rng, 3);
        let e = random_entry(&mut rng, &env, o);
        let applied = EtbApplied {
            effects: random_subset(&mut rng, 8),
            chosen: BTreeMap::new(),
            ..Default::default()
        };
        let probe: Vec<u64> = (1..=9).collect();
        let effects_before = effects_snapshot(&effects, &probe);
        let entry_before = entry_snapshot(&e);
        let env_before = env.bases.clone();

        let list = appl_ok(&ctx, &e, &applied);
        assert!(list.windows(2).all(|w| w[0] < w[1]), "ascending: {list:?}");
        for id in &list {
            assert!(!applied.effects.contains(id));
            assert!(effects.get(*id).is_some());
            let mut more = applied.effects.clone();
            more.insert(*id);
            let next = EtbApplied {
                effects: more,
                chosen: BTreeMap::new(),
                ..Default::default()
            };
            assert!(!appl_ok(&ctx, &e, &next).contains(id), "{id:?} reappeared");
        }
        assert_eq!(appl_ok(&ctx, &e, &applied), list);

        let p1 = pobs(&proj_ok(&ctx, &e, &applied));
        let p2 = pobs(&proj_ok(&ctx, &e, &applied));
        assert_eq!(p1, p2);
        assert_eq!(p1.object, oid(o));
        assert_eq!(p1.effects, applied.effects);

        assert_eq!(effects_snapshot(&effects, &probe), effects_before);
        assert_eq!(entry_snapshot(&e), entry_before);
        assert_eq!(env.bases, env_before);
        assert_eq!(cont, ContinuousEffects::new());
    }
}

// REQ-EVENT-001, CR 616.1, CR 616.1f, CR 707.2 — `resolve_entering` of independent entries (no
// budget) in any permutation gives, per entry, the same result (including the application
// order); two runs are equal; every result's applied record (set and order) leaves nothing
// applicable and re-projects to the result.
#[test]
fn req_event_001_prop_permutation_independent() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0x0e7b_0002);
    for _ in 0..80 {
        let env = random_env(&mut rng, 3);
        let effects = random_effects(&mut rng, 3);
        let cont = ContinuousEffects::new();
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let entries: Vec<EtbEntry> = (1..=3).map(|o| random_entry(&mut rng, &env, o)).collect();
        let mut reference: Option<BTreeMap<ObjectId, Obs>> = None;
        for perm in permutations(&[0, 1, 2]) {
            let ordered: Vec<EtbEntry> =
                perm.iter().map(|&i| entries[i as usize].clone()).collect();
            let (out, requests, _) =
                match drive_inspect(&ctx, &ordered, &no_budget(), content_answer, |o| {
                    o.iter().map(obs).collect::<Vec<_>>()
                }) {
                    Ok(r) => r,
                    Err(e) => panic!("resolve_entering failed: {e}"),
                };
            for (i, r) in requests.iter().enumerate() {
                assert_eq!(r.request_id, RequestId::new(i as u64));
            }
            assert_eq!(out.len(), 3);
            for (r, e) in out.iter().zip(ordered.iter()) {
                assert_eq!(r.object, e.object);
                // CR 616.1f: `order` lists every applied effect once, in application order.
                let order_set: BTreeSet<EffectId> = r.projected.order.iter().copied().collect();
                assert_eq!(
                    order_set.len(),
                    r.projected.order.len(),
                    "order repeats an id"
                );
                assert_eq!(
                    order_set, r.projected.effects,
                    "order is not the applied set"
                );
                // The re-projection supplies the real `order`: with several copies among the
                // effects, the bare set (ascending-id fallback) can name a different last copy
                // (CR 707.2, CR 616.1f), so the order is part of the input here.
                let applied = EtbApplied {
                    effects: r.projected.effects.clone(),
                    chosen: r.projected.chosen.clone(),
                    order: r.projected.order.clone(),
                };
                assert!(appl_ok(&ctx, e, &applied).is_empty(), "loop stopped early");
                // The result is the projection of its own applied record.
                let again = pobs(&proj_ok(&ctx, e, &applied));
                assert_eq!(again.evaluated, r.projected.evaluated);
                assert_eq!(again.tapped, r.projected.tapped);
                assert_eq!(again.counters, r.projected.counters);
                for (k, v) in &r.projected.chosen {
                    assert_eq!(r.memory.choice(k), Some(v));
                }
            }
            let by_object: BTreeMap<ObjectId, Obs> =
                out.into_iter().map(|r| (r.object, r)).collect();
            match &reference {
                None => reference = Some(by_object),
                Some(r) => assert_eq!(r, &by_object, "permutation {perm:?}"),
            }
        }
    }
}

// =============================================================================================
// TESTS REQUIRED 11 — API and edge cases (CR 614.12)
// =============================================================================================

// REQ-EVENT-001, CR 614.12 — EtbEffects: add, duplicate add (set unchanged), get, remove, len,
// is_empty; EtbEffect defaults and builders.
#[test]
fn req_event_001_api_effects_set() {
    let mut set_ = EtbEffects::new();
    assert!(set_.is_empty());
    assert_eq!(set_.len(), 0);
    assert!(EtbEffects::default().is_empty());

    let plain = EtbEffect::new(eid(7), EtbAction::EntersTapped);
    let plain_obs = effect_obs(&plain);
    assert_eq!(plain.id, eid(7));
    assert_eq!(plain.source, None);
    assert_eq!(plain.tier, Tier::Other);
    assert_eq!(plain.scope, EtbScope::OtherPermanents);
    assert!(matches!(plain.condition, EtbCondition::Always));
    assert!(matches!(plain.action, EtbAction::EntersTapped));
    assert_eq!(plain.description, format!("etb effect {}", eid(7).get()));

    let built = EtbEffect::new(eid(8), counters(CTR_A, 2))
        .with_source(oid(3))
        .with_tier(Tier::CopyEtb)
        .with_scope(EtbScope::ThisPermanent)
        .with_condition(EtbCondition::HasSubtype(SUB_A.to_owned()))
        .with_description("test.described");
    assert_eq!(built.source, Some(oid(3)));
    assert_eq!(built.tier, Tier::CopyEtb);
    assert_eq!(built.scope, EtbScope::ThisPermanent);
    assert!(matches!(&built.condition, EtbCondition::HasSubtype(s) if s == SUB_A));
    assert_eq!(built.description, "test.described");

    set_.add(plain).expect("first add");
    set_.add(built).expect("second add");
    assert_eq!(set_.len(), 2);
    assert!(!set_.is_empty());
    let before = effects_snapshot(&set_, &[7, 8]);
    let dup = EtbEffect::new(eid(7), counters(CTR_B, 9)).with_description("test.other");
    match set_.add(dup) {
        Err(EtbError::DuplicateEffect(id)) => assert_eq!(id, eid(7)),
        other => panic!("expected DuplicateEffect, got {other:?}"),
    }
    assert_eq!(effects_snapshot(&set_, &[7, 8]), before);
    assert_eq!(set_.get(eid(7)).map(effect_obs), Some(plain_obs.clone()));
    assert!(set_.get(eid(9)).is_none());

    let removed = set_.remove(eid(7)).expect("removed");
    assert_eq!(effect_obs(&removed), plain_obs);
    assert!(set_.remove(eid(7)).is_none());
    assert!(set_.get(eid(7)).is_none());
    assert_eq!(set_.len(), 1);
    set_.remove(eid(8)).expect("removed");
    assert!(set_.is_empty());
}

// REQ-EVENT-001, CR 614.12 — project of an unknown object is a layer error.
#[test]
fn req_event_001_api_unknown_object() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = EtbEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    match project(&ctx, &entry(42, P1), &EtbApplied::default()) {
        Err(EtbError::Layer(LayerError::UnknownObject(o))) => assert_eq!(o, oid(42)),
        Err(e) => panic!("expected Layer(UnknownObject), got {e}"),
        Ok(_) => panic!("expected an error"),
    }
}

// REQ-EVENT-001, CR 614.12 — an own static whose id is already registered (in force, or twice
// in own_statics) is a duplicate-effect layer error; inputs unchanged.
#[test]
fn req_event_001_api_duplicate_own_static() {
    let env = env_n(1);
    let mut cont = ContinuousEffects::new();
    cont.register(
        eid(100),
        in_force(1, 1, Layer::L4, add_types_mod(&[TYPE_Y])),
    )
    .expect("register");
    let before = cont.clone();
    let effects = EtbEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let clash = entry_with(
        1,
        P1,
        vec![own_static(100, 1, Layer::L4, add_types_mod(&[TYPE_X]))],
    );
    match project(&ctx, &clash, &EtbApplied::default()) {
        Err(EtbError::Layer(LayerError::DuplicateEffect(id))) => assert_eq!(id, eid(100)),
        Err(e) => panic!("expected Layer(DuplicateEffect), got {e}"),
        Ok(_) => panic!("expected an error"),
    }
    let twice = entry_with(
        1,
        P1,
        vec![
            own_static(101, 1, Layer::L4, add_types_mod(&[TYPE_X])),
            own_static(101, 1, Layer::L4, add_types_mod(&[TYPE_Y])),
        ],
    );
    match project(&ctx, &twice, &EtbApplied::default()) {
        Err(EtbError::Layer(LayerError::DuplicateEffect(id))) => assert_eq!(id, eid(101)),
        Err(e) => panic!("expected Layer(DuplicateEffect), got {e}"),
        Ok(_) => panic!("expected an error"),
    }
    assert_eq!(cont, before);
}

// REQ-EVENT-001, CR 614.12 — EtbError: Display non-empty for every variant; From<LayerError>.
#[test]
fn req_event_001_api_error_display_and_from() {
    let all = [
        EtbError::DuplicateEffect(eid(1)),
        EtbError::Layer(LayerError::UnknownObject(oid(2))),
        EtbError::Unpayable {
            chooser: P2,
            key: KEY_B.to_owned(),
        },
        EtbError::from(LayerError::DuplicateEffect(eid(3))),
    ];
    for e in &all {
        assert!(!e.to_string().is_empty(), "{e:?}");
    }
    assert!(matches!(
        EtbError::from(LayerError::UnknownObject(oid(5))),
        EtbError::Layer(LayerError::UnknownObject(o)) if o == oid(5)
    ));
}

// REQ-EVENT-001, CR 614.12 — R-1 details of `project`: tapped iff an applied EntersTapped;
// counters summed per kind; an unapplied effect contributes nothing; no request (pure).
#[test]
fn req_event_001_project_tapped_and_counters() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![
        fx(1, counters(CTR_A, 2)),
        fx(2, counters(CTR_A, 3)),
        fx(3, counters(CTR_B, 1)),
        fx(4, EtbAction::EntersTapped),
        fx(5, add_card_types(&[TYPE_X, TYPE_Y])),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let e = entry(1, P1);
    let none = proj_ok(&ctx, &e, &EtbApplied::default());
    assert!(!none.tapped);
    assert!(none.counters.is_empty());
    assert_eq!(types_of(&none), set(&[CREATURE]));

    let p = proj_ok(&ctx, &e, &applied_of(&[1, 2, 3]));
    assert!(!p.tapped);
    assert_eq!(
        p.counters,
        BTreeMap::from([(CTR_A.to_owned(), 5), (CTR_B.to_owned(), 1)])
    );
    let p = proj_ok(&ctx, &e, &applied_of(&[4, 5]));
    assert!(p.tapped);
    assert!(p.counters.is_empty());
    assert_eq!(types_of(&p), set(&[CREATURE, TYPE_X, TYPE_Y]));
    assert_eq!(pt(&p.evaluated), spt(2, 2));
}

// REQ-EVENT-001, CR 616.1 — an empty entry list gives an empty result; an entry with nothing
// applicable resolves to its plain projection.
#[test]
fn req_event_001_api_empty_inputs() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = EtbEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(&ctx, &[], &no_budget(), &[]);
    assert!(run.out.is_empty());
    assert!(run.requests.is_empty());
    let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
    assert_eq!(run.out.len(), 1);
    assert_eq!(
        run.out[0].projected,
        pobs(&proj_ok(&ctx, &entry(1, P1), &EtbApplied::default()))
    );
    assert_eq!(run.out[0].memory, Memory::new());
}

// =============================================================================================
// FIX ROUND 1 (CodeRabbit F1-F8) — shared helpers
// =============================================================================================

/// The object a copy model's static abilities are printed on (never entering, never in an
/// environment): a declared static is rebound from it to the entrant (CR 707.5).
const MODEL_OBJ: u64 = 77;
/// The source of the "external" ETB effects below (a permanent that is not entering).
const EXTERNAL: u64 = 9;

/// An environment of noncreature enchantment bases (none is on the battlefield yet).
fn enchantment_env(objs: &[(u64, PlayerId)]) -> TestEnv {
    let list: Vec<(u64, Characteristics, PlayerId)> = objs
        .iter()
        .map(|&(o, p)| (o, enchantment(&format!("test.etb_fix_base_{o}")), p))
        .collect();
    env_of(&list)
}

fn eids(ns: &[u64]) -> Vec<EffectId> {
    ns.iter().map(|&n| eid(n)).collect()
}

/// A static ability of the copy model (printed on `MODEL_OBJ`), layer 4.
fn model_static(id: u64, affected: Affected, m: LayerModification) -> (EffectId, ContinuousEffect) {
    (
        eid(id),
        ContinuousEffect {
            source: Some(oid(MODEL_OBJ)),
            controller: P1,
            timestamp: ts(id),
            layer: Layer::L4,
            is_cda: false,
            origin: EffectOrigin::Static { granted_at: None },
            affected,
            modification: m,
        },
    )
}

/// Declares what copy effect `copy` gives its entrant (CR 707.5, CR 614.12).
fn declare(e: &mut EtbEffects, copy: u64, statics: Vec<(EffectId, ContinuousEffect)>, etb: &[u64]) {
    let abilities = CopyAbilities {
        statics,
        etb: eids(etb),
    };
    if let Err(err) = e.set_copy_abilities(eid(copy), abilities) {
        panic!("declaring the abilities of copy {copy} failed: {err}");
    }
}

fn assert_not_payable(e: &ChoiceError, what: &str) {
    match e {
        ChoiceError::Procedure(msg) => assert!(msg.contains("not payable"), "{what}: {msg}"),
        other => panic!("{what}: expected Procedure(not payable), got {other:?}"),
    }
}

fn reversed<T: Clone>(v: &[T]) -> Vec<T> {
    v.iter().rev().cloned().collect()
}

// =============================================================================================
// F1 — the last APPLIED copy supplies the base (CR 616.1f, CR 707.2)
// =============================================================================================

const F1_CREATURE_MODEL: &str = "test.etb_f1_model_creature";
const F1_ARTIFACT_MODEL: &str = "test.etb_f1_model_artifact";

/// `c` copies a creature model, `a` an artifact model (both tier 616.1c); `t` "enters tapped"
/// if the permanent is a creature.
fn f1_effects(c: u64, a: u64, t: u64) -> EtbEffects {
    effects_of(vec![
        fx(c, copy_of(creature(F1_CREATURE_MODEL, 3, 3))).with_tier(Tier::CopyEtb),
        fx(a, copy_of(artifact(F1_ARTIFACT_MODEL))).with_tier(Tier::CopyEtb),
        fx(t, EtbAction::EntersTapped)
            .with_condition(EtbCondition::HasCardType(CREATURE.to_owned())),
    ])
}

// REQ-EVENT-001, CR 616.1e, CR 616.1f, CR 707.2 — finding F1 through `resolve_entering`: the
// controller picks which copy applies first (one request); the copy applied LAST is the final
// base. Artifact first then creature: a creature, so the creature-only effect becomes applicable
// and applies (tapped); creature first then artifact: an artifact, untapped. Every id
// assignment, so the result never follows the ids. `order` records the real sequence.
// Fails before the fix: the base followed ascending ids, so one of the two answers gave the
// wrong model in every id assignment.
#[test]
fn req_event_001_f1_resolve_last_applied_copy_wins() {
    let creature_model = norm(&creature(F1_CREATURE_MODEL, 3, 3));
    let artifact_model = norm(&artifact(F1_ARTIFACT_MODEL));
    for p in permutations(&[1, 2, 3]) {
        let (c, a, t) = (p[0], p[1], p[2]);
        let env = enchantment_env(&[(1, P1)]);
        let cont = ContinuousEffects::new();
        let effects = f1_effects(c, a, t);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let offered: Vec<String> = ascending(&[c, a]).iter().map(|e| desc(e.get())).collect();

        // Artifact copy first, creature copy last.
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[Ans::Desc(desc(a))]);
        assert_eq!(
            run.requests,
            vec![replacement_request(0, P1, 1, &offered)],
            "{p:?}"
        );
        let r = &run.out[0].projected;
        assert_eq!(r.evaluated.characteristics, creature_model, "{p:?}");
        assert_eq!(r.effects, ids(&[c, a, t]), "{p:?}");
        assert_eq!(r.order, eids(&[a, c, t]), "{p:?}");
        assert!(r.tapped, "{p:?}");
        assert_eq!(pt(&r.evaluated), spt(3, 3));

        // Creature copy first, artifact copy last.
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[Ans::Desc(desc(c))]);
        assert_eq!(
            run.requests,
            vec![replacement_request(0, P1, 1, &offered)],
            "{p:?}"
        );
        let r = &run.out[0].projected;
        assert_eq!(r.evaluated.characteristics, artifact_model, "{p:?}");
        assert_eq!(r.effects, ids(&[c, a]), "{p:?}");
        assert_eq!(r.order, eids(&[c, a]), "{p:?}");
        assert!(!r.tapped, "{p:?}");
    }
}

// REQ-EVENT-001, CR 616.1f, CR 707.2 — finding F1 through `project`/`applicable` with a
// hand-built `order`: the last copy in `order` is the base whatever the ids; the
// creature-only effect is applicable exactly when the creature copy is last. Fails before the
// fix (`order` was ignored).
#[test]
fn req_event_001_f1_project_follows_order_last_copy_wins() {
    let creature_model = norm(&creature(F1_CREATURE_MODEL, 3, 3));
    let artifact_model = norm(&artifact(F1_ARTIFACT_MODEL));
    for p in permutations(&[1, 2, 3]) {
        let (c, a, t) = (p[0], p[1], p[2]);
        let env = enchantment_env(&[(1, P1)]);
        let cont = ContinuousEffects::new();
        let effects = f1_effects(c, a, t);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let e = entry(1, P1);

        let creature_last = applied_in_order(&[a, c]);
        let pc = proj_ok(&ctx, &e, &creature_last);
        assert_eq!(norm(&pc.evaluated.characteristics), creature_model, "{p:?}");
        assert_eq!(appl_ok(&ctx, &e, &creature_last), vec![eid(t)], "{p:?}");
        assert!(proj_ok(&ctx, &e, &applied_in_order(&[a, c, t])).tapped);

        let artifact_last = applied_in_order(&[c, a]);
        let pa = proj_ok(&ctx, &e, &artifact_last);
        assert_eq!(norm(&pa.evaluated.characteristics), artifact_model, "{p:?}");
        assert!(appl_ok(&ctx, &e, &artifact_last).is_empty(), "{p:?}");

        // A partial order: the listed copy first, the unlisted one after it.
        let only_a = EtbApplied {
            order: eids(&[a]),
            ..applied_of(&[c, a])
        };
        assert_eq!(
            norm(&proj_ok(&ctx, &e, &only_a).evaluated.characteristics),
            creature_model,
            "{p:?}"
        );
        // Ids not applied and repeated ids in `order` are skipped (first occurrence kept).
        let noisy = EtbApplied {
            order: eids(&[40, a, c, a, 41]),
            ..applied_of(&[c, a])
        };
        assert_eq!(noisy.sequence(), eids(&[a, c]));
        assert_eq!(
            norm(&proj_ok(&ctx, &e, &noisy).evaluated.characteristics),
            creature_model,
            "{p:?}"
        );

        // The documented fallback: an empty order is the ascending-id order.
        let fallback = applied_of(&[c, a]);
        let expected = if c > a {
            &creature_model
        } else {
            &artifact_model
        };
        assert_eq!(
            &norm(&proj_ok(&ctx, &e, &fallback).evaluated.characteristics),
            expected,
            "{p:?}"
        );
    }
}

// REQ-EVENT-001, CR 616.1f — `EtbApplied::sequence`: `order` without ids outside `effects`
// and without repeats, then the unlisted ids ascending; empty `order` = ascending ids.
#[test]
fn req_event_001_f1_sequence() {
    let a = EtbApplied {
        effects: ids(&[1, 2, 3, 5]),
        chosen: BTreeMap::new(),
        order: eids(&[3, 9, 3, 1]),
    };
    assert_eq!(a.sequence(), eids(&[3, 1, 2, 5]));
    assert!(EtbApplied::default().sequence().is_empty());
    assert_eq!(applied_of(&[4, 2, 7]).sequence(), eids(&[2, 4, 7]));
    assert_eq!(applied_in_order(&[7, 2, 4]).sequence(), eids(&[7, 2, 4]));
    // An order of ids that were not applied contributes nothing.
    let stray = EtbApplied {
        order: eids(&[8, 6]),
        ..applied_of(&[5])
    };
    assert_eq!(stray.sequence(), eids(&[5]));
    // Derives kept.
    let copy = a.clone();
    assert_eq!(copy, a);
    assert_ne!(copy, applied_of(&[1, 2, 3, 5]));
}

// =============================================================================================
// F2 — a copy brings its model's static and ETB abilities (CR 614.12, CR 707.5)
// =============================================================================================

// REQ-EVENT-001, CR 614.12, CR 707.5 — finding F2, statics: the copy model's declared static
// (a layer-4 "is a creature", printed on another object and affecting it) is rebound to the
// entrant, so after the copy the entrant is an artifact creature and the creature-only effect
// applies; an `Affected::Selector` static works as well. The original's own static is gone.
// Without a declaration the copy is a plain artifact. Both id orders; the caller's registry is
// unchanged. Fails before the fix (no copy abilities: never a creature, never tapped).
#[test]
fn req_event_001_f2_copy_model_static_rebound_to_entrant() {
    let model = artifact("test.etb_f2_model");
    for p in permutations(&[1, 2]) {
        let (k, h) = (p[0], p[1]);
        for affected in [
            Affected::Objects([oid(MODEL_OBJ)].into_iter().collect()),
            Affected::Selector(sel(SEL_ALL)),
        ] {
            let env = enchantment_env(&[(1, P1)]);
            let cont = ContinuousEffects::new();
            let e = entry_with(
                1,
                P1,
                vec![own_static(100, 1, Layer::L4, add_types_mod(&[TYPE_X]))],
            );
            let list = || {
                vec![
                    fx(k, copy_of(model.clone())).with_tier(Tier::CopyEtb),
                    fx(h, EtbAction::EntersTapped)
                        .with_condition(EtbCondition::HasCardType(CREATURE.to_owned())),
                ]
            };
            let plain = effects_of(list());
            let mut declared = effects_of(list());
            declare(
                &mut declared,
                k,
                vec![model_static(
                    500,
                    affected.clone(),
                    add_types_mod(&[CREATURE]),
                )],
                &[],
            );

            // Without a declaration: a plain artifact copy.
            let ctx = EtbContext {
                env: &env,
                continuous: &cont,
                effects: &plain,
            };
            let run = drive_ok(&ctx, std::slice::from_ref(&e), &no_budget(), &[]);
            let r = &run.out[0].projected;
            assert_eq!(r.effects, ids(&[k]), "{p:?}");
            assert!(!r.tapped);
            assert_eq!(r.evaluated.characteristics.card_types, set(&[ARTIFACT]));

            // Declared: the static makes it a creature, so `h` applies.
            let ctx = EtbContext {
                env: &env,
                continuous: &cont,
                effects: &declared,
            };
            assert!(
                !appl_ok(&ctx, &e, &EtbApplied::default()).contains(&eid(h)),
                "{p:?}: the model's static does not apply before the copy"
            );
            let after = applied_in_order(&[k]);
            assert_eq!(
                types_of(&proj_ok(&ctx, &e, &after)),
                set(&[ARTIFACT, CREATURE]),
                "{p:?} {affected:?}"
            );
            assert_eq!(appl_ok(&ctx, &e, &after), vec![eid(h)], "{p:?}");
            let run = drive_ok(&ctx, std::slice::from_ref(&e), &no_budget(), &[]);
            assert!(run.requests.is_empty());
            let r = &run.out[0].projected;
            assert_eq!(r.effects, ids(&[k, h]), "{p:?}");
            assert_eq!(r.order, eids(&[k, h]), "{p:?}");
            assert!(r.tapped, "{p:?}");
            assert_eq!(
                r.evaluated.characteristics.card_types,
                set(&[ARTIFACT, CREATURE])
            );
            // Not leaked into the caller's registry.
            assert_eq!(cont, ContinuousEffects::new());
        }
    }
}

// REQ-EVENT-001, CR 707.2, CR 707.5, CR 616.1f — finding F2, two copies: only the declaration
// of the LAST applied copy counts. `k1` declares "is a creature", `k2` declares a subtype; both
// application orders, every id assignment. Fails before the fix.
#[test]
fn req_event_001_f2_only_last_copy_declaration_counts() {
    for p in permutations(&[1, 2, 3]) {
        let (k1, k2, h) = (p[0], p[1], p[2]);
        let env = enchantment_env(&[(1, P1)]);
        let cont = ContinuousEffects::new();
        let mut effects = effects_of(vec![
            fx(k1, copy_of(artifact("test.etb_f2_model_one"))).with_tier(Tier::CopyEtb),
            fx(k2, copy_of(artifact("test.etb_f2_model_two"))).with_tier(Tier::CopyEtb),
            fx(h, EtbAction::EntersTapped)
                .with_condition(EtbCondition::HasCardType(CREATURE.to_owned())),
        ]);
        let objs = || Affected::Objects([oid(MODEL_OBJ)].into_iter().collect());
        declare(
            &mut effects,
            k1,
            vec![model_static(500, objs(), add_types_mod(&[CREATURE]))],
            &[],
        );
        declare(
            &mut effects,
            k2,
            vec![model_static(501, objs(), add_subtypes_mod(&[SUB_A]))],
            &[],
        );
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let e = entry(1, P1);

        // k1 first, k2 last: k2's subtype only.
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[Ans::Desc(desc(k1))]);
        assert_eq!(run.requests.len(), 1);
        let r = &run.out[0].projected;
        assert_eq!(r.order, eids(&[k1, k2]), "{p:?}");
        assert_eq!(r.effects, ids(&[k1, k2]), "{p:?}");
        assert!(!r.tapped, "{p:?}");
        assert_eq!(r.evaluated.characteristics.card_types, set(&[ARTIFACT]));
        assert_eq!(r.evaluated.characteristics.subtypes, set(&[SUB_A]));
        let pr = proj_ok(&ctx, &e, &applied_in_order(&[k1, k2]));
        assert_eq!(types_of(&pr), set(&[ARTIFACT]));
        assert_eq!(pr.evaluated.characteristics.subtypes, set(&[SUB_A]));

        // k2 first, k1 last: k1's creature type only.
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[Ans::Desc(desc(k2))]);
        let r = &run.out[0].projected;
        assert_eq!(r.order, eids(&[k2, k1, h]), "{p:?}");
        assert_eq!(r.effects, ids(&[k1, k2, h]), "{p:?}");
        assert!(r.tapped, "{p:?}");
        assert_eq!(
            r.evaluated.characteristics.card_types,
            set(&[ARTIFACT, CREATURE])
        );
        assert!(r.evaluated.characteristics.subtypes.is_empty());
        let pr = proj_ok(&ctx, &e, &applied_in_order(&[k2, k1]));
        assert_eq!(types_of(&pr), set(&[ARTIFACT, CREATURE]));
        assert!(pr.evaluated.characteristics.subtypes.is_empty());
    }
}

// REQ-EVENT-001, CR 614.12, CR 707.5 — finding F2, ETB abilities: the copy model's declared
// "enters tapped" (a ThisPermanent effect sourced on the model) applies to the entrant once the
// copy applied, and not before; the ORIGINAL's own ThisPermanent effect does not apply once the
// copy applied. Without a declaration nothing of the model applies. Every id assignment.
// Fails before the fix (never tapped).
#[test]
fn req_event_001_f2_copy_model_etb_ability_applies_to_entrant() {
    for p in permutations(&[1, 2, 3]) {
        let (k, m, t) = (p[0], p[1], p[2]);
        let env = enchantment_env(&[(1, P1)]);
        let cont = ContinuousEffects::new();
        let list = || {
            vec![
                fx(k, copy_of(artifact("test.etb_f2_etb_model"))).with_tier(Tier::CopyEtb),
                own_fx(m, MODEL_OBJ, EtbAction::EntersTapped),
                own_fx(t, 1, counters(CTR_A, 1)),
            ]
        };
        let plain = effects_of(list());
        let mut declared = effects_of(list());
        declare(&mut declared, k, Vec::new(), &[m]);
        let e = entry(1, P1);

        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &plain,
        };
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
        let r = &run.out[0].projected;
        assert_eq!(r.effects, ids(&[k]), "{p:?}");
        assert!(!r.tapped);
        assert!(r.counters.is_empty());

        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &declared,
        };
        assert_eq!(
            appl_ok(&ctx, &e, &EtbApplied::default()),
            ascending(&[k, t]),
            "{p:?}: the model's ETB ability waits for the copy"
        );
        assert_eq!(
            appl_ok(&ctx, &e, &applied_in_order(&[k])),
            vec![eid(m)],
            "{p:?}: the model's ability applies, the original's own does not"
        );
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
        assert!(run.requests.is_empty(), "{p:?}");
        let r = &run.out[0].projected;
        assert_eq!(r.effects, ids(&[k, m]), "{p:?}");
        assert_eq!(r.order, eids(&[k, m]), "{p:?}");
        assert!(r.tapped, "{p:?}");
        assert!(r.counters.is_empty(), "{p:?}");
    }
}

// REQ-EVENT-001, CR 707.5, CR 616.1f — finding F2, two copies with ETB abilities: only the last
// applied copy's declared ETB ability applies (both application orders, every id assignment).
#[test]
fn req_event_001_f2_two_copies_only_last_etb_abilities() {
    for p in permutations(&[1, 2, 3, 4]) {
        let (k1, k2, m1, m2) = (p[0], p[1], p[2], p[3]);
        let env = enchantment_env(&[(1, P1)]);
        let cont = ContinuousEffects::new();
        let mut effects = effects_of(vec![
            fx(k1, copy_of(artifact("test.etb_f2_model_one"))).with_tier(Tier::CopyEtb),
            fx(k2, copy_of(artifact("test.etb_f2_model_two"))).with_tier(Tier::CopyEtb),
            own_fx(m1, MODEL_OBJ, EtbAction::EntersTapped),
            own_fx(m2, MODEL_OBJ + 1, counters(CTR_B, 1)),
        ]);
        declare(&mut effects, k1, Vec::new(), &[m1]);
        declare(&mut effects, k2, Vec::new(), &[m2]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };

        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[Ans::Desc(desc(k1))]);
        let r = &run.out[0].projected;
        assert_eq!(r.order, eids(&[k1, k2, m2]), "{p:?}");
        assert_eq!(r.effects, ids(&[k1, k2, m2]), "{p:?}");
        assert!(!r.tapped, "{p:?}");
        assert_eq!(r.counters, BTreeMap::from([(CTR_B.to_owned(), 1)]));

        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[Ans::Desc(desc(k2))]);
        let r = &run.out[0].projected;
        assert_eq!(r.order, eids(&[k2, k1, m1]), "{p:?}");
        assert_eq!(r.effects, ids(&[k1, k2, m1]), "{p:?}");
        assert!(r.tapped, "{p:?}");
        assert!(r.counters.is_empty(), "{p:?}");
    }
}

// REQ-EVENT-001, CR 707.5 — finding F2, `set_copy_abilities`: an id not in the set, or an
// effect that is not a copy, is `NotACopyEffect` (set unchanged, Display non-empty); a second
// declaration replaces the first; `remove` drops the declaration.
#[test]
fn req_event_001_f2_set_copy_abilities_api() {
    let (k, n, m1, m2) = (1, 2, 3, 4);
    let list = || {
        vec![
            fx(k, copy_of(artifact("test.etb_f2_api_model"))).with_tier(Tier::CopyEtb),
            fx(n, counters(CTR_A, 1)),
            own_fx(m1, MODEL_OBJ, EtbAction::EntersTapped),
            own_fx(m2, MODEL_OBJ, counters(CTR_B, 2)),
        ]
    };
    let mut effects = effects_of(list());
    let before = effects_snapshot(&effects, &[1, 2, 3, 4, 99]);
    for bad in [n, 99] {
        match effects.set_copy_abilities(eid(bad), CopyAbilities::default()) {
            Err(EtbError::NotACopyEffect(id)) => assert_eq!(id, eid(bad)),
            other => panic!("expected NotACopyEffect({bad}), got {other:?}"),
        }
    }
    assert_eq!(effects_snapshot(&effects, &[1, 2, 3, 4, 99]), before);
    assert!(!EtbError::NotACopyEffect(eid(5)).to_string().is_empty());
    assert_eq!(CopyAbilities::default(), CopyAbilities::default());

    let env = enchantment_env(&[(1, P1)]);
    let cont = ContinuousEffects::new();

    // Declared [m1], then replaced by [m2]: only m2 applies.
    declare(&mut effects, k, Vec::new(), &[m1]);
    declare(&mut effects, k, Vec::new(), &[m2]);
    {
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        // `n` and `m2` share a tier: the controller is asked between them (and only them).
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[Ans::Index(0)]);
        assert_eq!(
            run.requests,
            vec![replacement_request(0, P1, 1, &[desc(n), desc(m2)])]
        );
        let r = &run.out[0].projected;
        assert_eq!(r.effects, ids(&[k, m2, n]));
        assert_eq!(r.order, eids(&[k, n, m2]));
        assert!(!r.tapped);
        assert_eq!(
            r.counters,
            BTreeMap::from([(CTR_A.to_owned(), 1), (CTR_B.to_owned(), 2)])
        );
    }
    // Removed and added back without a declaration: a plain copy.
    let removed = effects.remove(eid(k)).expect("the copy is in the set");
    if let Err(err) = effects.add(removed) {
        panic!("re-adding the copy failed: {err}");
    }
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
    let r = &run.out[0].projected;
    assert_eq!(r.effects, ids(&[k, n]));
    assert!(!r.tapped);
    assert_eq!(r.counters, BTreeMap::from([(CTR_A.to_owned(), 1)]));
}

// =============================================================================================
// F3 — AddCardTypes is a layer-4 effect (CR 613.1d, CR 613.7, CR 205.1b, CR 208.3)
// =============================================================================================

fn printed_pt_artifact() -> Characteristics {
    Characteristics {
        power: Some(lit(3)),
        toughness: Some(lit(2)),
        ..artifact("test.etb_f3_artifact")
    }
}

// REQ-EVENT-001, CR 613.1d, CR 208.3, CR 205.1b — finding F3: a noncreature artifact with a
// printed 3/2 has no power or toughness; with an applied AddCardTypes{Creature} it is an artifact
// creature 3/2 (layer 7 sees the type), and a creature-only effect becomes applicable and
// applies. Fails before the fix (the type was unioned after the layers: P/T stayed absent).
#[test]
fn req_event_001_f3_added_creature_type_gets_printed_pt() {
    for p in permutations(&[1, 2]) {
        let (a, b) = (p[0], p[1]);
        let env = env_of(&[(1, printed_pt_artifact(), P1)]);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![
            fx(a, add_card_types(&[CREATURE])).with_tier(Tier::SelfReplacement),
            fx(b, EtbAction::EntersTapped)
                .with_condition(EtbCondition::HasCardType(CREATURE.to_owned())),
        ]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let e = entry(1, P1);

        let plain = proj_ok(&ctx, &e, &EtbApplied::default());
        assert_eq!(types_of(&plain), set(&[ARTIFACT]));
        assert_eq!(plain.evaluated.characteristics.power, None);
        assert_eq!(plain.evaluated.characteristics.toughness, None);

        let added = proj_ok(&ctx, &e, &applied_of(&[a]));
        assert_eq!(types_of(&added), set(&[ARTIFACT, CREATURE]));
        assert_eq!(pt(&added.evaluated), spt(3, 2), "{p:?}");
        assert_eq!(appl_ok(&ctx, &e, &applied_of(&[a])), vec![eid(b)]);

        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
        let r = &run.out[0].projected;
        assert_eq!(r.effects, ids(&[a, b]), "{p:?}");
        assert!(r.tapped);
        assert_eq!(pt(&r.evaluated), spt(3, 2));
        // The synthetic layer-4 effect is invisible to the caller.
        assert_eq!(cont, ContinuousEffects::new());
    }
}

// REQ-EVENT-001, CR 613.1d, CR 208.3a — finding F3: layers 6 and 7 and creature selectors see
// the added type: an in-force layer-6 "creatures have a keyword" and a layer-7c "creatures get
// +1/+1" apply to the entrant only when AddCardTypes{Creature} is applied. Fails before the fix.
#[test]
fn req_event_001_f3_added_type_seen_by_layers_6_and_7() {
    let env = env_of(&[(1, printed_pt_artifact(), P1)]);
    let mut cont = ContinuousEffects::new();
    cont.register(
        eid(300),
        eff(
            Layer::L6,
            1,
            Affected::Selector(sel(SEL_CREATURE)),
            LayerModification::AddAbility(kw_ability("test.kw_granted", "test_keyword")),
        ),
    )
    .expect("register");
    cont.register(
        eid(301),
        eff(
            Layer::L7c,
            2,
            Affected::Selector(sel(SEL_CREATURE)),
            LayerModification::ModifyPt {
                power: 1,
                toughness: 1,
            },
        ),
    )
    .expect("register");
    let before = cont.clone();
    let effects = effects_of(vec![fx(1, add_card_types(&[CREATURE]))]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let e = entry(1, P1);

    let plain = proj_ok(&ctx, &e, &EtbApplied::default());
    assert_eq!(pt(&plain.evaluated), (None, None));
    assert!(!has_kw(&plain.evaluated.characteristics, "test_keyword"));

    let added = proj_ok(&ctx, &e, &applied_of(&[1]));
    assert_eq!(pt(&added.evaluated), spt(4, 3));
    assert!(has_kw(&added.evaluated.characteristics, "test_keyword"));
    assert_eq!(cont, before);
}

// REQ-EVENT-001, CR 613.7, CR 205.1b — finding F3: the synthetic layer-4 effect has a timestamp
// later than every existing one (an own static SetTypes with a high timestamp, an in-force one
// near the top of the range) and an id that clashes with no registered id or own static; the
// added type survives, nothing is leaked into the caller's registry.
#[test]
fn req_event_001_f3_synthetic_effect_timestamp_and_id() {
    let set_artifact = || LayerModification::SetTypes {
        supertypes: None,
        card_types: Some(set(&[ARTIFACT])),
        subtypes: None,
    };
    let env = env_n(1);
    let effects = effects_of(vec![fx(1, add_card_types(&[TYPE_X]))]);

    // An own static SetTypes (timestamp 900) and in-force effects with ids 2..=4.
    let mut cont = ContinuousEffects::new();
    for id in 2..=4 {
        cont.register(eid(id), in_force(1, id, Layer::L7c, modify_pt(0, 0)))
            .expect("register");
    }
    let before = cont.clone();
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let e = entry_with(1, P1, vec![own_static(900, 1, Layer::L4, set_artifact())]);
    assert_eq!(
        types_of(&proj_ok(&ctx, &e, &applied_of(&[1]))),
        set(&[ARTIFACT, TYPE_X])
    );
    assert_eq!(
        types_of(&proj_ok(&ctx, &e, &EtbApplied::default())),
        set(&[ARTIFACT])
    );
    assert_eq!(cont, before);

    // In force with the second-highest id and timestamp: the synthetic effect still fits.
    let mut cont = ContinuousEffects::new();
    cont.register(
        eid(u64::MAX - 1),
        in_force(1, u64::MAX - 1, Layer::L4, set_artifact()),
    )
    .expect("register");
    let before = cont.clone();
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    assert_eq!(
        types_of(&proj_ok(&ctx, &entry(1, P1), &applied_of(&[1]))),
        set(&[ARTIFACT, TYPE_X])
    );
    assert_eq!(cont, before);
}

// REQ-EVENT-001, CR 613.7 — finding F3: no fresh id or timestamp above the maximum exists:
// an in-force effect (or own static) with the maximum id or timestamp, plus an applied
// AddCardTypes, is a layer error (checked arithmetic, no wrap-around).
#[test]
fn req_event_001_f3_fresh_id_or_timestamp_overflow_is_layer_error() {
    let env = env_n(1);
    let effects = effects_of(vec![fx(1, add_card_types(&[TYPE_X]))]);
    let none = || Affected::Selector(sel(SEL_NONE));
    let cases: Vec<(&str, ContinuousEffects, EtbEntry)> = vec![
        (
            "max id in force",
            {
                let mut c = ContinuousEffects::new();
                c.register(eid(u64::MAX), eff(Layer::L7c, 1, none(), modify_pt(0, 0)))
                    .expect("register");
                c
            },
            entry(1, P1),
        ),
        (
            "max timestamp in force",
            {
                let mut c = ContinuousEffects::new();
                c.register(eid(5), eff(Layer::L7c, u64::MAX, none(), modify_pt(0, 0)))
                    .expect("register");
                c
            },
            entry(1, P1),
        ),
        (
            "max id and timestamp as an own static",
            ContinuousEffects::new(),
            entry_with(
                1,
                P1,
                vec![own_static(u64::MAX, 1, Layer::L7c, modify_pt(0, 0))],
            ),
        ),
    ];
    for (what, cont, e) in cases {
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        match project(&ctx, &e, &applied_of(&[1])) {
            Err(EtbError::Layer(_)) => {}
            Err(other) => panic!("{what}: expected a layer error, got {other}"),
            Ok(p) => panic!("{what}: expected a layer error, got {:?}", types_of(&p)),
        }
    }
}

// =============================================================================================
// F4 — the generating ability must still exist (CR 614.12, CR 603.6d)
// =============================================================================================

/// How the test environment answers `generating_ability_exists`.
#[derive(Clone, Debug)]
enum Hook {
    /// False for the listed effect ids, true otherwise.
    Deny(BTreeSet<EffectId>),
    /// True iff the source's view (when given) still has an ability.
    AbilitiesPresent,
    /// False for every effect id.
    DenyAll,
}

/// A `TestEnv` with an overridable ability hook that records every call as
/// (effect, source, number of abilities in the source view).
struct HookEnv {
    inner: TestEnv,
    hook: Hook,
    calls: RefCell<Vec<(EffectId, ObjectId, Option<usize>)>>,
}

impl HookEnv {
    fn new(inner: TestEnv, hook: Hook) -> HookEnv {
        HookEnv {
            inner,
            hook,
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl LayerEnvironment for HookEnv {
    fn objects(&self) -> Vec<ObjectId> {
        self.inner.objects()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.inner.base(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.inner.counters(id)
    }

    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        self.inner.counter_timestamp(id, kind)
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.inner.attached_here(id)
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        self.inner.matches(selector, ctx)
    }

    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        self.inner.eval_value(expr, object)
    }

    fn generating_ability_exists(
        &self,
        effect: EffectId,
        source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        self.calls.borrow_mut().push((
            effect,
            source,
            source_view.map(|v| v.characteristics.abilities.len()),
        ));
        match &self.hook {
            Hook::Deny(ids) => !ids.contains(&effect),
            Hook::DenyAll => false,
            Hook::AbilitiesPresent => {
                source_view.is_none_or(|v| !v.characteristics.abilities.is_empty())
            }
        }
    }
}

// REQ-EVENT-001, CR 614.12, CR 603.6d — finding F4: a ThisPermanent "enters tapped" applies only
// while its generating ability exists, as the environment's hook says when asked with the
// effect id, the entrant and its projected view. An in-force layer-6 "loses all abilities" on
// the entrant with a hook answering false for the effect: not tapped (not applicable); the
// same with a hook answering true (or false only for another id): tapped. A hook that models
// existence from the view: no ability left, not tapped; ability present, tapped. Fails before
// the fix (the hook was never asked).
#[test]
fn req_event_001_f4_this_permanent_needs_its_generating_ability() {
    let t = 5;
    let effects = effects_of(vec![own_fx(t, 1, EtbAction::EntersTapped)]);
    let mut removed = ContinuousEffects::new();
    removed
        .register(
            eid(300),
            in_force(1, 1, Layer::L6, LayerModification::RemoveAllAbilities),
        )
        .expect("register");
    let intact = ContinuousEffects::new();
    let base = || enchantment_env(&[(1, P1)]);

    let cases: Vec<(&str, Hook, &ContinuousEffects, bool)> = vec![
        (
            "removed, hook false",
            Hook::Deny(ids(&[t])),
            &removed,
            false,
        ),
        (
            "removed, hook true",
            Hook::Deny(BTreeSet::new()),
            &removed,
            true,
        ),
        (
            "removed, other id denied",
            Hook::Deny(ids(&[6])),
            &removed,
            true,
        ),
        ("intact, hook false", Hook::Deny(ids(&[t])), &intact, false),
        ("removed, modelled", Hook::AbilitiesPresent, &removed, false),
        ("intact, modelled", Hook::AbilitiesPresent, &intact, true),
    ];
    for (what, hook, cont, applies) in cases {
        let env = HookEnv::new(base(), hook);
        let ctx = EtbContext {
            env: &env,
            continuous: cont,
            effects: &effects,
        };
        let expected = if applies { vec![eid(t)] } else { Vec::new() };
        assert_eq!(
            appl_ok(&ctx, &entry(1, P1), &EtbApplied::default()),
            expected,
            "{what}"
        );
        let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
        let r = &run.out[0].projected;
        assert_eq!(r.tapped, applies, "{what}");
        assert_eq!(
            r.effects,
            if applies { ids(&[t]) } else { ids(&[]) },
            "{what}"
        );
        // The hook was asked about the effect, for the entrant, with its projected view.
        let calls = env.calls.borrow();
        let asked: Vec<&(EffectId, ObjectId, Option<usize>)> =
            calls.iter().filter(|c| c.0 == eid(t)).collect();
        assert!(!asked.is_empty(), "{what}: the hook was not asked");
        for c in &asked {
            assert_eq!(c.1, oid(1), "{what}");
            let abilities =
                c.2.unwrap_or_else(|| panic!("{what}: no source view given"));
            if std::ptr::eq(cont, &removed) {
                assert_eq!(abilities, 0, "{what}: the view is the projected one");
            } else {
                assert!(abilities > 0, "{what}");
            }
        }
    }
}

// =============================================================================================
// F5-F8 — exact payability lookahead (CR 614.12b, CR 616.2)
// =============================================================================================

// REQ-EVENT-001, CR 614.12b, CR 616.2 — finding F5: budget 2; entry 1 chooses {two: 2, one: 1};
// entry 2 first copies itself into a creature, whereupon an external creature-only Choose {1}
// applies to it. The lookahead reserves that 1, so entry 1 may only take "one" (applied
// without a request). Both declared option orders, both slice orders; with budget 3 entry 1 is
// asked with both options in declared order. Fails before the fix (entry 2's reserve was read
// on its unprojected, noncreature form: 0).
#[test]
fn req_event_001_f5_reserve_sees_choice_enabled_by_later_copy() {
    let two_one = [(OPT_B, 2u64), (OPT_A, 1u64)];
    for list in [two_one.to_vec(), reversed(&two_one)] {
        let env = enchantment_env(&[(1, P1), (2, P1)]);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![
            own_fx(10, 1, choose(KEY_A, K::Color, &list)),
            own_fx(20, 2, copy_of(creature("test.etb_f5_model", 1, 1))).with_tier(Tier::CopyEtb),
            fx(30, choose(KEY_B, K::Color, &[(OPT_C, 1)]))
                .with_source(oid(EXTERNAL))
                .with_condition(EtbCondition::HasCardType(CREATURE.to_owned())),
        ]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        for order in [[1u64, 2], [2, 1]] {
            let entries: Vec<EtbEntry> = order.iter().map(|&o| entry(o, P1)).collect();
            let run = drive_ok(&ctx, &entries, &budget(&[(P1, 2)]), &[]);
            assert!(run.requests.is_empty(), "{list:?} {order:?}");
            for (r, &o) in run.out.iter().zip(order.iter()) {
                if o == 1 {
                    assert_eq!(
                        r.memory.choice(KEY_A),
                        Some(&MemChoice::Color(OPT_A.to_owned())),
                        "{list:?} {order:?}"
                    );
                    assert_eq!(r.projected.effects, ids(&[10]));
                } else {
                    assert_eq!(
                        r.memory.choice(KEY_B),
                        Some(&MemChoice::Color(OPT_C.to_owned()))
                    );
                    assert_eq!(r.projected.order, eids(&[20, 30]));
                }
            }
        }
        // Budget 3: both options are payable for entry 1.
        let declared: Vec<&str> = list.iter().map(|(t, _)| *t).collect();
        let run = drive_ok(
            &ctx,
            &[entry(1, P1), entry(2, P1)],
            &budget(&[(P1, 3)]),
            &[Ans::Text(OPT_B.to_owned())],
        );
        assert_eq!(
            run.requests,
            vec![option_request(
                0,
                P1,
                "CR 614.12a",
                Some(oid(1)),
                "COLOR",
                &declared
            )]
        );
    }
}

// REQ-EVENT-001, CR 614.12b, CR 616.2 — finding F6: budget 2; entry 1 needs 2; entry 2 (a
// creature) has its own Choose {1}, but an external mandatory copy into an artifact (higher
// tier, creature-only) applies first and removes it: the real total is 2, no error. Without
// the copy the 1 is reserved and the run is not payable (budget 3 is). Both declared option
// orders, both slice orders. Fails before the fix (entry 2's own Choose was reserved from its
// unprojected form although the copy removes it).
#[test]
fn req_event_001_f6_reserve_drops_choice_removed_by_copy() {
    let opts2 = [(OPT_A, 2u64), (OPT_B, 3u64)];
    for list in [opts2.to_vec(), reversed(&opts2)] {
        let env = env_of(&[(1, enchantment("test.etb_f6_base_1"), P1), (2, body(2), P1)]);
        let cont = ContinuousEffects::new();
        let choices = || {
            vec![
                own_fx(10, 1, choose(KEY_A, K::Color, &list)),
                own_fx(20, 2, choose(KEY_B, K::Color, &[(OPT_C, 1)])),
            ]
        };
        let mut with_copy_list = choices();
        with_copy_list.push(
            fx(30, copy_of(artifact("test.etb_f6_model")))
                .with_tier(Tier::CopyEtb)
                .with_source(oid(EXTERNAL))
                .with_condition(EtbCondition::HasCardType(CREATURE.to_owned())),
        );
        let with_copy = effects_of(with_copy_list);
        let without_copy = effects_of(choices());
        for order in [[1u64, 2], [2, 1]] {
            let entries: Vec<EtbEntry> = order.iter().map(|&o| entry(o, P1)).collect();

            let ctx = EtbContext {
                env: &env,
                continuous: &cont,
                effects: &with_copy,
            };
            let run = drive_ok(&ctx, &entries, &budget(&[(P1, 2)]), &[]);
            assert!(run.requests.is_empty(), "{list:?} {order:?}");
            for (r, &o) in run.out.iter().zip(order.iter()) {
                if o == 1 {
                    assert_eq!(
                        r.memory.choice(KEY_A),
                        Some(&MemChoice::Color(OPT_A.to_owned()))
                    );
                } else {
                    assert_eq!(r.projected.effects, ids(&[30]), "{list:?} {order:?}");
                    assert_eq!(r.memory.choice(KEY_B), None);
                    assert_eq!(
                        r.projected.evaluated.characteristics.card_types,
                        set(&[ARTIFACT])
                    );
                }
            }

            let ctx = EtbContext {
                env: &env,
                continuous: &cont,
                effects: &without_copy,
            };
            let e = drive_err(&ctx, &entries, &budget(&[(P1, 2)]), &[]);
            assert_not_payable(&e, &format!("no copy, {list:?} {order:?}"));
            let run = drive_ok(&ctx, &entries, &budget(&[(P1, 3)]), &[]);
            assert_eq!(run.out.len(), 2);
        }
    }
}

// REQ-EVENT-001, CR 614.12b, CR 616.2 — finding F7: budget 3; entry 1 first chooses (tier
// 616.1a-style SelfReplacement) between {one: 1, two: 2}, then a mandatory copy makes it a
// creature and an external creature-only Choose {1} applies; entry 2 needs 1. Only "one" is
// allowed (1 + 1 + 1 = 3), applied without a request. Both declared option orders, both slice
// orders; with budget 4 both options are offered in declared order. Fails before the fix (the
// same-entry reserve was read on the current projection, without the copy).
#[test]
fn req_event_001_f7_reserve_sees_same_entry_choice_after_copy() {
    let one_two = [(OPT_A, 1u64), (OPT_B, 2u64)];
    for list in [one_two.to_vec(), reversed(&one_two)] {
        let env = enchantment_env(&[(1, P1), (2, P1)]);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![
            own_fx(10, 1, choose(KEY_A, K::Color, &list)).with_tier(Tier::SelfReplacement),
            own_fx(20, 1, copy_of(creature("test.etb_f7_model", 1, 1))).with_tier(Tier::CopyEtb),
            fx(30, choose(KEY_B, K::Color, &[(OPT_C, 1)]))
                .with_source(oid(EXTERNAL))
                .with_condition(EtbCondition::HasCardType(CREATURE.to_owned())),
            own_fx(40, 2, choose(&key(2), K::Color, &[(OPT_C, 1)])),
        ]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        for order in [[1u64, 2], [2, 1]] {
            let entries: Vec<EtbEntry> = order.iter().map(|&o| entry(o, P1)).collect();
            let run = drive_ok(&ctx, &entries, &budget(&[(P1, 3)]), &[]);
            assert!(run.requests.is_empty(), "{list:?} {order:?}");
            for (r, &o) in run.out.iter().zip(order.iter()) {
                if o == 1 {
                    assert_eq!(
                        r.memory.choice(KEY_A),
                        Some(&MemChoice::Color(OPT_A.to_owned())),
                        "{list:?} {order:?}"
                    );
                    assert_eq!(
                        r.memory.choice(KEY_B),
                        Some(&MemChoice::Color(OPT_C.to_owned()))
                    );
                    assert_eq!(r.projected.order, eids(&[10, 20, 30]));
                } else {
                    assert_eq!(
                        r.memory.choice(&key(2)),
                        Some(&MemChoice::Color(OPT_C.to_owned()))
                    );
                }
            }
        }
        let declared: Vec<&str> = list.iter().map(|(t, _)| *t).collect();
        let run = drive_ok(
            &ctx,
            &[entry(1, P1), entry(2, P1)],
            &budget(&[(P1, 4)]),
            &[Ans::Text(OPT_B.to_owned())],
        );
        assert_eq!(
            run.requests,
            vec![option_request(
                0,
                P1,
                "CR 614.12a",
                Some(oid(1)),
                "COLOR",
                &declared
            )]
        );
    }
}

// REQ-EVENT-001, CR 614.12b — finding F8: the payability sum is exact (no saturation). Budget
// u64::MAX, entry 1 chooses between {u64::MAX, u64::MAX - 1}, entry 2 needs 1: only
// u64::MAX - 1 is allowed (applied without a request). Alone, entry 1 is asked with both. Both
// declared option orders, both slice orders. Fails before the fix (u64::MAX + 1 saturated to
// u64::MAX, which looked payable).
#[test]
fn req_event_001_f8_exact_sum_no_saturation() {
    let big = [(OPT_A, u64::MAX), (OPT_B, u64::MAX - 1)];
    for list in [big.to_vec(), reversed(&big)] {
        let env = env_n(2);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![
            own_fx(1, 1, choose(KEY_A, K::Color, &list)),
            own_fx(2, 2, choose(KEY_B, K::Color, &[(OPT_C, 1)])),
        ]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        for order in [[1u64, 2], [2, 1]] {
            let entries: Vec<EtbEntry> = order.iter().map(|&o| entry(o, P1)).collect();
            let run = drive_ok(&ctx, &entries, &budget(&[(P1, u64::MAX)]), &[]);
            assert!(run.requests.is_empty(), "{list:?} {order:?}");
            for (r, &o) in run.out.iter().zip(order.iter()) {
                if o == 1 {
                    assert_eq!(
                        r.memory.choice(KEY_A),
                        Some(&MemChoice::Color(OPT_B.to_owned())),
                        "{list:?} {order:?}"
                    );
                } else {
                    assert_eq!(
                        r.memory.choice(KEY_B),
                        Some(&MemChoice::Color(OPT_C.to_owned()))
                    );
                }
            }
        }
        let declared: Vec<&str> = list.iter().map(|(t, _)| *t).collect();
        let run = drive_ok(
            &ctx,
            &[entry(1, P1)],
            &budget(&[(P1, u64::MAX)]),
            &[Ans::Text(OPT_A.to_owned())],
        );
        assert_eq!(
            run.requests,
            vec![option_request(
                0,
                P1,
                "CR 614.12a",
                Some(oid(1)),
                "COLOR",
                &declared
            )]
        );
        assert_eq!(
            run.out[0].memory.choice(KEY_A),
            Some(&MemChoice::Color(OPT_A.to_owned()))
        );
    }
}

// REQ-EVENT-001, CR 614.12b, CR 616.1e — the CHOOSE_REPLACEMENT_EFFECT candidates are never
// filtered by payability: budget 1; a Choose {2} applicable only while the permanent lacks
// test.type_x, and an effect adding test.type_x, in one tier. Both are offered; taking the type
// first is payable (the Choose never applies), taking the Choose first is not.
#[test]
fn req_event_001_payable_replacement_candidates_not_filtered() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![
        fx(1, choose(KEY_A, K::Color, &[(OPT_A, 2)]))
            .with_condition(EtbCondition::LacksCardType(TYPE_X.to_owned())),
        fx(2, add_card_types(&[TYPE_X])),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(&ctx, &[entry(1, P1)], &budget(&[(P1, 1)]), &[Ans::Index(1)]);
    assert_eq!(
        run.requests,
        vec![replacement_request(0, P1, 1, &[desc(1), desc(2)])]
    );
    assert_eq!(run.out[0].projected.effects, ids(&[2]));
    assert_eq!(run.out[0].memory.choice(KEY_A), None);
    let e = drive_err(&ctx, &[entry(1, P1)], &budget(&[(P1, 1)]), &[Ans::Index(0)]);
    assert_not_payable(&e, "the Choose first");
}

/// Entry 1 with a first-tier Choose {1, 2} and `others` Other-tier single-option Choose {1}
/// effects (ids 2..), all applicable together.
fn wide_choices(others: u64) -> EtbEffects {
    let mut list = vec![
        own_fx(1, 1, choose(KEY_A, K::Color, &[(OPT_A, 1), (OPT_B, 2)]))
            .with_tier(Tier::SelfReplacement),
    ];
    for n in 2..2 + others {
        list.push(fx(n, choose(&key(n), K::Color, &[(OPT_C, 1)])));
    }
    effects_of(list)
}

/// Asserts that `e` is the lookahead state-limit procedure error (R3): a
/// `ChoiceError::Procedure` whose message names the lookahead and the 4096-state bound.
fn assert_lookahead_limit(e: &ChoiceError, what: &str) {
    match e {
        ChoiceError::Procedure(msg) => {
            assert!(msg.contains("lookahead"), "{what}: {msg}");
            assert!(msg.contains("exceeds 4096 states"), "{what}: {msg}");
        }
        other => panic!("{what}: expected Procedure(lookahead limit), got {other:?}"),
    }
}

/// Drives `resolve_entering` answering every request with `content_answer`; the observations
/// of the output, or the procedure error.
fn drive_content(
    ctx: &EtbContext<'_>,
    entries: &[EtbEntry],
    payable: &BTreeMap<PlayerId, u64>,
) -> Result<Vec<Obs>, ChoiceError> {
    drive_inspect(ctx, entries, payable, content_answer, |o| {
        o.iter().map(obs).collect::<Vec<_>>()
    })
    .map(|(out, _, _)| out)
}

// REQ-EVENT-001, CR 614.12b (R3) — the lookahead is bounded: 13 further Choose effects of one
// tier give 2^13 states, beyond the bound, which is the lookahead-limit procedure error (the
// message names the lookahead and "exceeds 4096 states"), never a silent truncation and never
// another error; 7 of them (2^7 states) are within it and resolve through the same helper.
// (CodeRabbit round 2, G6: any nonempty procedure error used to pass.)
#[test]
fn req_event_001_payable_lookahead_bounded() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();

    let effects = wide_choices(13);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    match drive_content(&ctx, &[entry(1, P1)], &budget(&[(P1, 1000)])) {
        Err(e) => assert_lookahead_limit(&e, "13 further Choose effects"),
        Ok(out) => panic!("expected the lookahead-limit error, got {out:?}"),
    }

    let effects = wide_choices(7);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let out = match drive_content(&ctx, &[entry(1, P1)], &budget(&[(P1, 1000)])) {
        Ok(out) => out,
        Err(e) => panic!("resolve_entering failed: {e}"),
    };
    assert_eq!(out[0].projected.effects, ids(&[1, 2, 3, 4, 5, 6, 7, 8]));
}

// =============================================================================================
// FIX ROUND 2 (CodeRabbit G1-G5) — regression tests
// =============================================================================================

// ---------------------------------------------------------------------------------------------
// G1 — copied statics take the entrant's controller (CR 707.5)
// ---------------------------------------------------------------------------------------------

/// A selector slot private to this file: "artifacts you control", relative to the effect's
/// controller (`MatchContext::effect_controller`) and the candidate's controller.
const SEL_ARTIFACTS_YOU_CONTROL: u32 = 60;

/// An environment of noncreature enchantment bases with the controller-relative selector.
fn controlled_env(objs: &[(u64, PlayerId)]) -> TestEnv {
    let mut env = enchantment_env(objs);
    env.add_selector(
        sel(SEL_ARTIFACTS_YOU_CONTROL),
        std::rc::Rc::new(|c: &MatchContext<'_>| {
            c.candidate_view
                .characteristics
                .card_types
                .contains(ARTIFACT)
                && c.candidate_view.controller == Some(c.effect_controller)
        }),
    );
    env
}

/// `copy` copies an artifact model whose declared static (controlled by `model_controller`)
/// says "artifacts you control are creatures"; `h` "enters tapped" if the permanent is a
/// creature. `copy` is sourceless, so it applies to every entrant.
fn g1_effects(copy: u64, h: u64, model_controller: PlayerId) -> EtbEffects {
    let mut e = effects_of(vec![
        fx(copy, copy_of(artifact("test.etb_g1_model"))).with_tier(Tier::CopyEtb),
        fx(h, EtbAction::EntersTapped)
            .with_condition(EtbCondition::HasCardType(CREATURE.to_owned())),
    ]);
    let (id, mut st) = model_static(
        500,
        Affected::Selector(sel(SEL_ARTIFACTS_YOU_CONTROL)),
        add_types_mod(&[CREATURE]),
    );
    st.controller = model_controller;
    declare(&mut e, copy, vec![(id, st)], &[]);
    e
}

// REQ-EVENT-001, CR 707.5 — finding G1: a copy model's declared static "artifacts you control
// are creatures" is controlled, once rebound to the entrant, by the entrant's controller. For
// every model controller and entrant controller (different and equal) and both id orders: the
// entrant projects as an artifact creature after the copy, the creature-only effect becomes
// applicable and applies (tapped). Fails at head 30f1b2c when the controllers differ (the
// static kept the model's controller, so the selector did not match the entrant).
#[test]
fn req_event_001_g1_copied_static_uses_entrant_controller() {
    for p in permutations(&[1, 2]) {
        let (k, h) = (p[0], p[1]);
        for model_controller in [P1, P2] {
            for entrant_controller in [P1, P2] {
                let what =
                    format!("{p:?} model {model_controller:?} entrant {entrant_controller:?}");
                let env = controlled_env(&[(1, entrant_controller)]);
                let cont = ContinuousEffects::new();
                let effects = g1_effects(k, h, model_controller);
                let ctx = EtbContext {
                    env: &env,
                    continuous: &cont,
                    effects: &effects,
                };
                let e = entry(1, entrant_controller);
                let after = applied_in_order(&[k]);
                assert_eq!(
                    types_of(&proj_ok(&ctx, &e, &after)),
                    set(&[ARTIFACT, CREATURE]),
                    "{what}"
                );
                assert_eq!(appl_ok(&ctx, &e, &after), vec![eid(h)], "{what}");
                let run = drive_ok(&ctx, std::slice::from_ref(&e), &no_budget(), &[]);
                assert!(run.requests.is_empty(), "{what}");
                let r = &run.out[0].projected;
                assert_eq!(r.effects, ids(&[k, h]), "{what}");
                assert_eq!(r.order, eids(&[k, h]), "{what}");
                assert!(r.tapped, "{what}");
                assert_eq!(
                    r.evaluated.characteristics.card_types,
                    set(&[ARTIFACT, CREATURE]),
                    "{what}"
                );
                assert_eq!(cont, ContinuousEffects::new());
            }
        }
    }
}

// REQ-EVENT-001, CR 707.5 — finding G1, two entrants with different controllers (P1 and P2)
// entering together, both slice orders, model controlled by P1 and by P2: each entrant's copy of
// the static is controlled by that entrant's controller, so both are artifact creatures and both
// enter tapped. Fails at head 30f1b2c (the entrant whose controller differs from the model's
// stayed a noncreature artifact, untapped).
#[test]
fn req_event_001_g1_two_entrants_different_controllers() {
    let (k, h) = (1, 2);
    for model_controller in [P1, P2] {
        let env = controlled_env(&[(1, P1), (2, P2)]);
        let cont = ContinuousEffects::new();
        let effects = g1_effects(k, h, model_controller);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        for entries in [
            vec![entry(1, P1), entry(2, P2)],
            vec![entry(2, P2), entry(1, P1)],
        ] {
            let what = format!(
                "model {model_controller:?} order {:?}",
                entries.iter().map(|e| e.object).collect::<Vec<_>>()
            );
            let run = drive_ok(&ctx, &entries, &no_budget(), &[]);
            assert!(run.requests.is_empty(), "{what}");
            assert_eq!(run.out.len(), 2, "{what}");
            for (r, e) in run.out.iter().zip(entries.iter()) {
                assert_eq!(r.object, e.object, "{what}");
                assert_eq!(r.projected.effects, ids(&[k, h]), "{what} {:?}", e.object);
                assert!(r.projected.tapped, "{what} {:?}", e.object);
                assert_eq!(
                    r.projected.evaluated.characteristics.card_types,
                    set(&[ARTIFACT, CREATURE]),
                    "{what} {:?}",
                    e.object
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// G2 — the synthetic layer-4 effect of AddCardTypes is not gated by the ability hook
// (CR 613.1d)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, CR 613.1d, CR 208.3 — finding G2: with an environment hook answering false for
// EVERY effect id, an applied AddCardTypes{Creature} on a printed 3/2 artifact still makes it a
// 3/2 artifact creature (the type change is a layer-4 effect, not a static ability that could
// stop existing), and the creature-only effect applies; with a hook answering true the result
// is identical. A ThisPermanent effect of the entrant still needs its generating ability
// (round-1 F4): denied, it does not apply; allowed, it does. Both id orders. Fails at head
// 30f1b2c (the synthetic effect was asked about and dropped: no creature, no P/T, untapped).
#[test]
fn req_event_001_g2_added_type_ignores_ability_hook() {
    for p in permutations(&[1, 2]) {
        let (a, b) = (p[0], p[1]);
        let mut results = Vec::new();
        for hook in [Hook::DenyAll, Hook::Deny(BTreeSet::new())] {
            let what = format!("{p:?} {hook:?}");
            let env = HookEnv::new(env_of(&[(1, printed_pt_artifact(), P1)]), hook);
            let cont = ContinuousEffects::new();
            let effects = effects_of(vec![
                fx(a, add_card_types(&[CREATURE])).with_tier(Tier::SelfReplacement),
                fx(b, EtbAction::EntersTapped)
                    .with_condition(EtbCondition::HasCardType(CREATURE.to_owned())),
            ]);
            let ctx = EtbContext {
                env: &env,
                continuous: &cont,
                effects: &effects,
            };
            let e = entry(1, P1);
            let added = proj_ok(&ctx, &e, &applied_of(&[a]));
            assert_eq!(types_of(&added), set(&[ARTIFACT, CREATURE]), "{what}");
            assert_eq!(pt(&added.evaluated), spt(3, 2), "{what}");
            assert_eq!(appl_ok(&ctx, &e, &applied_of(&[a])), vec![eid(b)], "{what}");
            let run = drive_ok(&ctx, &[entry(1, P1)], &no_budget(), &[]);
            assert!(run.requests.is_empty(), "{what}");
            let r = &run.out[0].projected;
            assert_eq!(r.effects, ids(&[a, b]), "{what}");
            assert!(r.tapped, "{what}");
            assert_eq!(pt(&r.evaluated), spt(3, 2), "{what}");
            assert_eq!(cont, ContinuousEffects::new());
            results.push(run.out);
        }
        assert_eq!(results[0], results[1], "{p:?}: the hook changes nothing");
    }

    // Round-1 F4 stays: a ThisPermanent effect needs its generating ability.
    let c = 3;
    for (hook, applies) in [(Hook::DenyAll, false), (Hook::Deny(BTreeSet::new()), true)] {
        let what = format!("{hook:?}");
        let env = HookEnv::new(env_of(&[(1, printed_pt_artifact(), P1)]), hook);
        let cont = ContinuousEffects::new();
        let effects = effects_of(vec![
            fx(1, add_card_types(&[CREATURE])).with_tier(Tier::SelfReplacement),
            fx(2, EtbAction::EntersTapped)
                .with_condition(EtbCondition::HasCardType(CREATURE.to_owned())),
            own_fx(c, 1, counters(CTR_A, 1)),
        ]);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let run = drive_content(&ctx, &[entry(1, P1)], &no_budget())
            .unwrap_or_else(|e| panic!("{what}: resolve_entering failed: {e}"));
        let r = &run[0].projected;
        assert!(r.tapped, "{what}");
        assert_eq!(pt(&r.evaluated), spt(3, 2), "{what}");
        if applies {
            assert_eq!(r.effects, ids(&[1, 2, c]), "{what}");
            assert_eq!(
                r.counters,
                BTreeMap::from([(CTR_A.to_owned(), 1)]),
                "{what}"
            );
        } else {
            assert_eq!(r.effects, ids(&[1, 2]), "{what}");
            assert!(r.counters.is_empty(), "{what}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// G3 — a Choose without options is rejected at registration (CR 614.12b)
// ---------------------------------------------------------------------------------------------

// REQ-EVENT-001, CR 614.12b — finding G3: `EtbEffects::add` of a Choose whose options list is
// empty is `Err(EtbError::EmptyChoice(id))`, with a non-empty Display, and leaves the set
// unchanged; a Choose with a single cost-0 option is accepted and is payable with budget 0
// (applied without a request). Fails at head 30f1b2c (the empty Choose was accepted).
#[test]
fn req_event_001_g3_empty_choose_rejected() {
    let mut set_ = effects_of(vec![fx(1, EtbAction::EntersTapped)]);
    let before = effects_snapshot(&set_, &[1, 2]);
    match set_.add(own_fx(2, 1, choose(KEY_A, K::Color, &[]))) {
        Err(EtbError::EmptyChoice(id)) => assert_eq!(id, eid(2)),
        other => panic!("expected EmptyChoice, got {other:?}"),
    }
    assert_eq!(effects_snapshot(&set_, &[1, 2]), before);
    assert!(set_.get(eid(2)).is_none());
    assert!(!EtbError::EmptyChoice(eid(2)).to_string().is_empty());

    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![own_fx(1, 1, choose(KEY_A, K::Color, &[(OPT_A, 0)]))]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(&ctx, &[entry(1, P1)], &budget(&[(P1, 0)]), &[]);
    assert!(run.requests.is_empty());
    assert_eq!(run.out[0].projected.effects, ids(&[1]));
    assert_eq!(
        run.out[0].memory.choice(KEY_A),
        Some(&MemChoice::Color(OPT_A.to_owned()))
    );
}

// ---------------------------------------------------------------------------------------------
// G4 — lookahead states are counted before descending (R3, CR 614.12b)
// ---------------------------------------------------------------------------------------------

fn chain_type(i: u64) -> String {
    format!("test.chain_type_{i}")
}

/// The deep chain of finding G4 for one entry (object 1): effects 1..=k, effect i adding
/// `test.chain_type_i` if the permanent has `test.chain_type_(i-1)` (effect 1: always), then
/// effect k+1, a mandatory single-option Choose {cost 1} if it has `test.chain_type_k`.
///
/// Counted from the scenario: the entry's reachable states from its empty applied set are {}
/// (the starting state, the first), {1}, {1,2}, ..., {1..k} (k more; exactly one effect is
/// applicable in each, so every state is reached once, by one path), then {1..k+1} with the one
/// chosen value (one more): k + 2 states in all.
fn deep_chain(k: u64) -> EtbEffects {
    let mut list = Vec::new();
    for i in 1..=k {
        let cond = if i == 1 {
            EtbCondition::Always
        } else {
            EtbCondition::HasCardType(chain_type(i - 1))
        };
        list.push(fx(i, add_card_types(&[&chain_type(i)])).with_condition(cond));
    }
    list.push(
        fx(k + 1, choose(KEY_B, K::Color, &[(OPT_C, 1)]))
            .with_condition(EtbCondition::HasCardType(chain_type(k))),
    );
    effects_of(list)
}

// REQ-EVENT-001, CR 614.12b, R3 — finding G4, exactness (passing side): k = 4094 gives exactly
// 4096 reachable states (k + 2, see `deep_chain`) for a budgeted chooser: within the bound, so
// it resolves; the entry ends with the whole chain and its choice, without a request. (At head
// 30f1b2c this overflowed the test thread's stack: the recursion descended before counting.)
#[test]
fn req_event_001_g4_deep_chain_4096_states_resolve() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let k = 4094;
    let effects = deep_chain(k);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let run = drive_ok(&ctx, &[entry(1, P1)], &budget(&[(P1, 1000)]), &[]);
    assert!(run.requests.is_empty());
    let all: Vec<u64> = (1..=k + 1).collect();
    assert_eq!(run.out[0].projected.effects, ids(&all));
    assert_eq!(
        run.out[0].memory.choice(KEY_B),
        Some(&MemChoice::Color(OPT_C.to_owned()))
    );
}

// REQ-EVENT-001, CR 614.12b, R3 — finding G4, exactness (failing side): k = 4095 gives 4097
// reachable states: the lookahead-limit procedure error (the message names the lookahead and
// "exceeds 4096 states"), never a crash or stack overflow. Fails at head 30f1b2c (stack
// overflow on the deep recursion; with a large stack, 4097 states resolved without the error).
#[test]
fn req_event_001_g4_deep_chain_4097_states_error() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let effects = deep_chain(4095);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    match drive_content(&ctx, &[entry(1, P1)], &budget(&[(P1, 1000)])) {
        Err(e) => assert_lookahead_limit(&e, "4097 states"),
        Ok(out) => panic!("4097 states must be the lookahead-limit error, got {out:?}"),
    }
}

// REQ-EVENT-001, CR 614.12b, R3 — finding G4: memo hits cost nothing. One entry with 10
// independent "enters with a counter" effects and a mandatory single-option Choose {cost 1},
// all applicable at once in one tier: 11! application orders, but only 2^11 = 2048 distinct
// states (the subsets of applied effects), within the bound, so it resolves for a budgeted
// chooser (every order request answered with the last offered effect). A pinning test: it
// passes at head 30f1b2c.
#[test]
fn req_event_001_g4_wide_redundant_orders_within_bound() {
    let env = env_n(1);
    let cont = ContinuousEffects::new();
    let mut list = Vec::new();
    for i in 1..=10 {
        list.push(fx(i, counters(CTR_A, 1)));
    }
    list.push(fx(11, choose(KEY_B, K::Color, &[(OPT_C, 1)])));
    let effects = effects_of(list);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let out = drive_content(&ctx, &[entry(1, P1)], &budget(&[(P1, 1000)]))
        .unwrap_or_else(|e| panic!("2048 distinct states must resolve: {e}"));
    let all: Vec<u64> = (1..=11).collect();
    assert_eq!(out[0].projected.effects, ids(&all));
    assert_eq!(
        out[0].projected.counters,
        BTreeMap::from([(CTR_A.to_owned(), 10)])
    );
    assert_eq!(
        out[0].memory.choice(KEY_B),
        Some(&MemChoice::Color(OPT_C.to_owned()))
    );
}

// ---------------------------------------------------------------------------------------------
// G5 — every entry is validated before anything is asked (CR 614.12a)
// ---------------------------------------------------------------------------------------------

/// The first status of a session over `resolve_entering`: the procedure error, or the first
/// pending request (`None` when done without one).
fn first_status(
    ctx: &EtbContext<'_>,
    entries: &[EtbEntry],
    payable: &BTreeMap<PlayerId, u64>,
) -> Result<Option<PendingChoiceRequest>, ChoiceError> {
    let procedure =
        |_: &mut Nil, cx: &mut ChoiceContext<'_>| resolve_entering(ctx, entries, payable, cx);
    ChoiceSession::start(Nil, procedure).map(|s| s.pending().cloned())
}

// REQ-EVENT-001, CR 614.12a — finding G5: entry A (object 1) has a Choose between two free
// values; entry B is invalid (an unknown object, or a duplicate own-static id). In both slice
// orders, with a budget for A's chooser and with none, the first status of the session is the
// procedure error carrying B's error (no request for A first). Valid inputs still ask A and
// resolve as before. Fails at head 30f1b2c for the order [A, B] (A was asked first).
#[test]
fn req_event_001_g5_validate_every_entry_before_asking() {
    let env = env_n(2);
    let cont = ContinuousEffects::new();
    let effects = effects_of(vec![own_fx(
        1,
        1,
        choose(KEY_A, K::Color, &[(OPT_A, 0), (OPT_B, 0)]),
    )]);
    let ctx = EtbContext {
        env: &env,
        continuous: &cont,
        effects: &effects,
    };
    let a = entry(1, P1);
    let invalid: Vec<(EtbEntry, String)> = vec![
        (
            entry(42, P1),
            EtbError::Layer(LayerError::UnknownObject(oid(42))).to_string(),
        ),
        (
            entry_with(
                2,
                P1,
                vec![
                    own_static(101, 2, Layer::L4, add_types_mod(&[TYPE_X])),
                    own_static(101, 2, Layer::L4, add_types_mod(&[TYPE_Y])),
                ],
            ),
            EtbError::Layer(LayerError::DuplicateEffect(eid(101))).to_string(),
        ),
    ];
    for (b, expected) in &invalid {
        for payable in [no_budget(), budget(&[(P1, 5)])] {
            for entries in [vec![a.clone(), b.clone()], vec![b.clone(), a.clone()]] {
                let what = format!(
                    "{expected} {payable:?} {:?}",
                    entries.iter().map(|e| e.object).collect::<Vec<_>>()
                );
                match first_status(&ctx, &entries, &payable) {
                    Err(ChoiceError::Procedure(msg)) => {
                        assert!(msg.contains(expected.as_str()), "{what}: {msg}");
                    }
                    Err(other) => panic!("{what}: expected a procedure error, got {other:?}"),
                    Ok(pending) => panic!("{what}: expected the error first, got {pending:?}"),
                }
            }
        }
    }

    // Valid inputs: A is asked (both free values), B resolves plainly.
    for payable in [no_budget(), budget(&[(P1, 5)])] {
        let entries = [a.clone(), entry(2, P1)];
        let first = first_status(&ctx, &entries, &payable)
            .unwrap_or_else(|e| panic!("valid inputs failed: {e}"));
        assert_eq!(
            first,
            Some(option_request(
                0,
                P1,
                "CR 614.12a",
                Some(oid(1)),
                "COLOR",
                &[OPT_A, OPT_B]
            ))
        );
        let run = drive_ok(&ctx, &entries, &payable, &[Ans::Text(OPT_B.to_owned())]);
        assert_eq!(run.requests.len(), 1);
        assert_eq!(
            run.out[0].memory.choice(KEY_A),
            Some(&MemChoice::Color(OPT_B.to_owned()))
        );
        assert_eq!(run.out[1].projected.effects, ids(&[]));
        assert_eq!(run.out[1].memory, Memory::new());
    }
}
