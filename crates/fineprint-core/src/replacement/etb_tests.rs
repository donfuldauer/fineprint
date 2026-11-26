// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the ETB look-ahead: REQ-EVENT-001 (CR 614.12 projected permanent, CR 614.12a
// choices before entering, CR 614.12b payable totals, CR 614.12c anchor words in Memory,
// CR 616.1 / 616.1f select-apply-repeat). Synthetic ids and synthetic characteristics only.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::ObjectSelectorAST;

use super::*;
use crate::choice::{
    AllowedValues, ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload,
    ChoiceReason, ChoiceResponse, ChoiceSession, ChoiceType, PendingChoiceRequest, RequestId,
    Status,
};
use crate::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerEnvironment, LayerError,
    LayerModification, MatchContext, ObjectBase, evaluate, literal, literal_of,
};
use crate::memory::{ChoiceValue, MemoryTable};
use crate::objects::Characteristics;
use crate::replacement::Tier;
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

fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A synthetic permanent card `name` of `card_type` with printed P/T.
fn card(name: &str, card_type: &str, power: i64, toughness: i64) -> Characteristics {
    Characteristics {
        names: set([name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::GREEN,
        supertypes: BTreeSet::new(),
        card_types: set([card_type]),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream(Vec::new()),
        abilities: Vec::new(),
        power: Some(literal(power)),
        toughness: Some(literal(toughness)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A synthetic artifact without P/T.
fn artifact(name: &str) -> Characteristics {
    let mut c = card(name, "Artifact", 0, 0);
    c.power = None;
    c.toughness = None;
    c
}

/// A synthetic environment: every object is off the battlefield (cards about to enter).
#[derive(Default, Clone, Debug, PartialEq)]
struct Env {
    bases: BTreeMap<ObjectId, ObjectBase>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
}

impl Env {
    fn with(mut self, id: ObjectId, c: Characteristics) -> Env {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(p(1)),
                on_battlefield: false,
            },
        );
        self
    }
}

impl LayerEnvironment for Env {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    /// Every selector matches the object it is evaluated for only if it is a creature.
    fn matches(&self, _: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        ctx.candidate_view
            .characteristics
            .card_types
            .contains("Creature")
    }
}

/// A layer-4 effect adding `card_type` to `target`.
fn add_type(source: Option<ObjectId>, target: ObjectId, card_type: &str) -> ContinuousEffect {
    ContinuousEffect {
        source,
        controller: p(1),
        timestamp: Timestamp::new(1),
        layer: Layer::L4,
        is_cda: false,
        origin: if source.is_some() {
            EffectOrigin::Static { granted_at: None }
        } else {
            EffectOrigin::Resolved
        },
        affected: Affected::Objects([target].into_iter().collect()),
        modification: LayerModification::AddTypes {
            supertypes: BTreeSet::new(),
            card_types: set([card_type]),
            subtypes: BTreeSet::new(),
        },
    }
}

/// A layer-4 effect adding `subtype` to `target`.
fn add_subtype(target: ObjectId, subtype: &str) -> ContinuousEffect {
    ContinuousEffect {
        modification: LayerModification::AddTypes {
            supertypes: BTreeSet::new(),
            card_types: BTreeSet::new(),
            subtypes: set([subtype]),
        },
        ..add_type(None, target, "Creature")
    }
}

fn entry(object: ObjectId, controller: PlayerId) -> EtbEntry {
    EtbEntry {
        object,
        controller,
        own_statics: Vec::new(),
    }
}

fn effects(list: Vec<EtbEffect>) -> EtbEffects {
    let mut r = EtbEffects::new();
    for effect in list {
        r.add(effect).unwrap();
    }
    r
}

fn applied(ids: &[u64]) -> EtbApplied {
    EtbApplied {
        effects: ids.iter().map(|&n| e(n)).collect(),
        chosen: BTreeMap::new(),
        ..Default::default()
    }
}

fn opt(text: &str, cost: u64) -> EtbOption {
    EtbOption {
        text: text.to_owned(),
        cost,
    }
}

fn choose(key: &str, kind: EtbChoiceKind, options: Vec<EtbOption>) -> EtbAction {
    EtbAction::Choose {
        key: key.to_owned(),
        kind,
        options,
    }
}

fn counters(amount: i64) -> EtbAction {
    EtbAction::EntersWithCounters {
        kind: "charge".to_owned(),
        amount,
    }
}

fn text(t: &str) -> ChoiceResponse {
    ChoiceResponse::SubmitChoiceValue {
        value: crate::choice::ChoiceValue::Text(t.to_owned()),
    }
}

fn pick(index: u32) -> ChoiceResponse {
    ChoiceResponse::SelectReplacement { index }
}

fn texts(items: &[&str]) -> ChoicePayload {
    ChoicePayload::ChooseOptionOrValue {
        option_type: "COLOR".to_owned(),
        allowed: AllowedValues::Texts(items.iter().map(|s| (*s).to_owned()).collect()),
    }
}

/// A state with nothing in it: the projection reads only its borrowed context.
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

/// What a driven run produced: the result (or error) and every request published, in order.
struct Run {
    result: Result<Vec<EtbResolved>, ChoiceError>,
    asked: Vec<PendingChoiceRequest>,
}

/// Runs `resolve_entering` through a `ChoiceSession`, submitting `answers` one by one; an
/// unanswered pending request ends the run with `ChoiceError::NotPending`.
fn drive(
    ctx: &EtbContext<'_>,
    entries: &[EtbEntry],
    payable: &BTreeMap<PlayerId, u64>,
    answers: &[ChoiceResponse],
) -> Run {
    let proc = |_s: &mut Nil, c: &mut ChoiceContext<'_>| resolve_entering(ctx, entries, payable, c);
    let mut asked = Vec::new();
    let mut session = match ChoiceSession::start(Nil, proc) {
        Ok(s) => s,
        Err(err) => {
            return Run {
                result: Err(err),
                asked,
            };
        }
    };
    let mut answers = answers.iter();
    loop {
        let request = match session.status() {
            Status::Done { output, .. } => {
                return Run {
                    result: Ok(output.clone()),
                    asked,
                };
            }
            Status::Pending(request) => request.clone(),
        };
        asked.push(request.clone());
        let Some(answer) = answers.next() else {
            return Run {
                result: Err(ChoiceError::NotPending),
                asked,
            };
        };
        if let Err(err) = session.submit(request.request_id, answer.clone()) {
            return Run {
                result: Err(err),
                asked,
            };
        }
    }
}

fn no_limit() -> BTreeMap<PlayerId, u64> {
    BTreeMap::new()
}

/// The procedure error message of a failed run.
fn procedure_message(err: Result<Vec<EtbResolved>, ChoiceError>) -> String {
    match err {
        Err(ChoiceError::Procedure(m)) => m,
        other => panic!("expected a procedure error, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Registry and builders
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001, CR 614.12: the registry keys by id; a duplicate is refused; the builders set
/// their fields.
#[test]
fn req_event_001_registry_and_builders() {
    let fx = EtbEffect::new(e(3), EtbAction::EntersTapped);
    assert_eq!(fx.tier, Tier::Other);
    assert_eq!(fx.scope, EtbScope::OtherPermanents);
    assert_eq!(fx.condition, EtbCondition::Always);
    assert_eq!(fx.source, None);
    assert_eq!(fx.description, "etb effect 3");
    let fx = fx
        .with_source(o(7))
        .with_tier(Tier::CopyEtb)
        .with_scope(EtbScope::ThisPermanent)
        .with_condition(EtbCondition::HasSubtype("Golem".to_owned()))
        .with_description("test.enter_tapped");
    assert_eq!(fx.source, Some(o(7)));
    assert_eq!(fx.tier, Tier::CopyEtb);
    assert_eq!(fx.scope, EtbScope::ThisPermanent);
    assert_eq!(fx.condition, EtbCondition::HasSubtype("Golem".to_owned()));
    assert_eq!(fx.description, "test.enter_tapped");

    let mut r = EtbEffects::new();
    assert!(r.is_empty());
    r.add(fx.clone()).unwrap();
    assert_eq!(r.add(fx.clone()), Err(EtbError::DuplicateEffect(e(3))));
    assert_eq!(r.len(), 1);
    assert_eq!(r.get(e(3)), Some(&fx));
    assert_eq!(r.remove(e(3)), Some(fx));
    assert_eq!(r.remove(e(3)), None);
    assert!(r.is_empty());
}

/// REQ-EVENT-001, CR 614.12b: every error has a message; `Unpayable` says "not payable"; a
/// layer error converts.
#[test]
fn req_event_001_error_display() {
    let un = EtbError::Unpayable {
        chooser: p(2),
        key: "k".to_owned(),
    };
    assert!(un.to_string().contains("not payable"));
    assert!(
        EtbError::DuplicateEffect(e(4))
            .to_string()
            .contains("already")
    );
    let layer: EtbError = LayerError::UnknownObject(o(9)).into();
    assert_eq!(layer, EtbError::Layer(LayerError::UnknownObject(o(9))));
    assert!(layer.to_string().starts_with("layer error"));
    let as_error: &dyn std::error::Error = &un;
    assert!(as_error.source().is_none());
}

// ---------------------------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001, CR 614.12 (second input): an own static layer-4 effect changes the projected
/// type, and an effect conditioned on that type applies only then.
#[test]
fn req_event_001_own_static_changes_projected_type() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let fx = effects(vec![
        EtbEffect::new(e(10), EtbAction::EntersTapped)
            .with_condition(EtbCondition::HasCardType("Creature".to_owned())),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let plain = entry(o(1), p(1));
    assert_eq!(applicable(&ctx, &plain, &EtbApplied::default()), Ok(vec![]));
    let mut with_static = entry(o(1), p(1));
    with_static
        .own_statics
        .push((e(100), add_type(Some(o(1)), o(1), "Creature")));
    let projected = project(&ctx, &with_static, &EtbApplied::default()).unwrap();
    assert!(
        projected
            .evaluated
            .characteristics
            .card_types
            .contains("Creature")
    );
    assert_eq!(
        applicable(&ctx, &with_static, &EtbApplied::default()),
        Ok(vec![e(10)])
    );
    // The own static is registered on a clone only.
    assert_eq!(continuous, ContinuousEffects::new());
    let out = drive(&ctx, &[with_static], &no_limit(), &[]);
    assert!(out.result.unwrap()[0].projected.tapped);
    let out = drive(&ctx, &[plain], &no_limit(), &[]);
    assert!(!out.result.unwrap()[0].projected.tapped);
}

/// REQ-EVENT-001, CR 614.12 (third input): a continuous effect already in force changes the
/// projected type and subtypes, and so which effects apply.
#[test]
fn req_event_001_continuous_effect_in_force_changes_projected_type() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let mut continuous = ContinuousEffects::new();
    continuous
        .register(e(200), add_type(None, o(1), "Creature"))
        .unwrap();
    continuous
        .register(e(201), add_subtype(o(1), "Golem"))
        .unwrap();
    let fx = effects(vec![
        EtbEffect::new(e(10), EtbAction::EntersTapped)
            .with_condition(EtbCondition::LacksCardType("Creature".to_owned())),
        EtbEffect::new(e(11), EtbAction::EntersTapped)
            .with_condition(EtbCondition::HasCardType("Creature".to_owned())),
        EtbEffect::new(e(12), EtbAction::EntersTapped)
            .with_condition(EtbCondition::HasSubtype("Golem".to_owned())),
        EtbEffect::new(e(13), EtbAction::EntersTapped)
            .with_condition(EtbCondition::HasSubtype("Wall".to_owned())),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    assert_eq!(
        applicable(&ctx, &entry(o(1), p(1)), &EtbApplied::default()),
        Ok(vec![e(11), e(12)])
    );
}

/// REQ-EVENT-001, CR 614.12: the entering object is evaluated as on the battlefield even if the
/// environment says it is not (CR 208.3: a noncreature permanent has no P/T; a creature does).
#[test]
fn req_event_001_projected_as_on_battlefield() {
    let env = Env::default().with(o(1), card("test.etb_vehicle_like", "Artifact", 3, 2));
    let continuous = ContinuousEffects::new();
    let fx = EtbEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    // Off the battlefield, the printed P/T applies.
    let off = evaluate(&continuous, &env, o(1)).unwrap();
    assert!(off.characteristics.power.is_some());
    // Projected on the battlefield, a noncreature has none.
    let projected = project(&ctx, &entry(o(1), p(1)), &EtbApplied::default()).unwrap();
    assert_eq!(projected.evaluated.characteristics.power, None);
    // As a creature (own static), it has its P/T.
    let mut as_creature = entry(o(1), p(1));
    as_creature
        .own_statics
        .push((e(100), add_type(Some(o(1)), o(1), "Creature")));
    let projected = project(&ctx, &as_creature, &EtbApplied::default()).unwrap();
    let c = &projected.evaluated.characteristics;
    assert_eq!(c.power.as_ref().and_then(literal_of), Some(3));
    assert_eq!(c.toughness.as_ref().and_then(literal_of), Some(2));
    assert_eq!(projected.object, o(1));
    assert_eq!(projected.applied, EtbApplied::default());
    // The environment still says off the battlefield (it is wrapped, not changed).
    assert!(!env.base(o(1)).unwrap().on_battlefield);
}

/// REQ-EVENT-001, CR 614.12: an unknown object and a duplicate own-static id are layer errors;
/// through `resolve_entering` they become procedure errors.
#[test]
fn req_event_001_projection_errors() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let mut continuous = ContinuousEffects::new();
    continuous
        .register(e(200), add_type(None, o(1), "Creature"))
        .unwrap();
    let fx = EtbEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let unknown = EtbError::Layer(LayerError::UnknownObject(o(5)));
    assert_eq!(
        project(&ctx, &entry(o(5), p(1)), &EtbApplied::default()),
        Err(unknown.clone())
    );
    assert_eq!(
        applicable(&ctx, &entry(o(5), p(1)), &EtbApplied::default()),
        Err(unknown.clone())
    );
    let mut dup = entry(o(1), p(1));
    dup.own_statics
        .push((e(200), add_type(Some(o(1)), o(1), "Enchantment")));
    assert_eq!(
        project(&ctx, &dup, &EtbApplied::default()),
        Err(EtbError::Layer(LayerError::DuplicateEffect(e(200))))
    );
    let out = drive(&ctx, &[entry(o(5), p(1))], &no_limit(), &[]);
    assert_eq!(procedure_message(out.result), unknown.to_string());
}

/// REQ-EVENT-001, CR 614.12: applied "enters tapped" and "enters with counters" are collected;
/// several effects of one tier are ordered by the chooser and all apply.
#[test]
fn req_event_001_projection_collects_tapped_and_counters() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let fx = effects(vec![
        EtbEffect::new(e(1), counters(2)),
        EtbEffect::new(e(2), counters(3)),
        EtbEffect::new(e(3), EtbAction::EntersTapped),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let none = project(&ctx, &entry(o(1), p(1)), &EtbApplied::default()).unwrap();
    assert!(!none.tapped);
    assert!(none.counters.is_empty());
    let all = project(&ctx, &entry(o(1), p(1)), &applied(&[1, 2, 3])).unwrap();
    assert!(all.tapped);
    assert_eq!(all.counters.get("charge"), Some(&5));
    let out = drive(&ctx, &[entry(o(1), p(1))], &no_limit(), &[pick(2), pick(0)]);
    assert_eq!(out.asked.len(), 2);
    let r = out.result.unwrap();
    assert_eq!(r[0].projected.counters.get("charge"), Some(&5));
    assert!(r[0].projected.tapped);
}

// ---------------------------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001, CR 614.12: a "permanents enter tapped" source enters untapped itself and taps
/// the others entering with it; a `ThisPermanent` effect applies only to its source.
#[test]
fn req_event_001_scope() {
    let env = Env::default()
        .with(o(1), artifact("test.etb_card_a"))
        .with(o(2), artifact("test.etb_card_b"));
    let continuous = ContinuousEffects::new();
    let fx = effects(vec![
        EtbEffect::new(e(1), EtbAction::EntersTapped).with_source(o(1)),
        EtbEffect::new(e(2), counters(1))
            .with_source(o(2))
            .with_scope(EtbScope::ThisPermanent),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let empty = EtbApplied::default();
    assert_eq!(applicable(&ctx, &entry(o(1), p(1)), &empty), Ok(vec![]));
    assert_eq!(
        applicable(&ctx, &entry(o(2), p(1)), &empty),
        Ok(vec![e(1), e(2)])
    );
    let out = drive(
        &ctx,
        &[entry(o(1), p(1)), entry(o(2), p(1))],
        &no_limit(),
        &[pick(0)],
    );
    let r = out.result.unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].object, o(1));
    assert_eq!(r[1].object, o(2));
    assert!(!r[0].projected.tapped);
    assert!(r[0].projected.counters.is_empty());
    assert!(r[1].projected.tapped);
    assert_eq!(r[1].projected.counters.get("charge"), Some(&1));
}

// ---------------------------------------------------------------------------------------------
// Acceptance 1: a type-changing applied effect changes which replacements apply
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001, CR 614.12, CR 616.2, CR 616.1f (acceptance 1): an applied `AddCardTypes`
/// makes a `HasCardType` effect applicable and stops a `LacksCardType` one; the loop applies the
/// newly applicable effect. Both id orders.
#[test]
fn req_event_001_add_card_types_changes_applicability_both_orders() {
    for (add_id, has_id, lacks_id) in [(1, 2, 3), (3, 2, 1)] {
        let env = Env::default().with(o(1), artifact("test.etb_card_a"));
        let continuous = ContinuousEffects::new();
        let fx = effects(vec![
            EtbEffect::new(e(add_id), EtbAction::AddCardTypes(set(["Creature"])))
                .with_tier(Tier::SelfReplacement),
            EtbEffect::new(e(has_id), EtbAction::EntersTapped)
                .with_condition(EtbCondition::HasCardType("Creature".to_owned())),
            EtbEffect::new(e(lacks_id), counters(1))
                .with_condition(EtbCondition::LacksCardType("Creature".to_owned())),
        ]);
        let ctx = EtbContext {
            env: &env,
            continuous: &continuous,
            effects: &fx,
        };
        let en = entry(o(1), p(1));
        let mut before = vec![e(add_id), e(lacks_id)];
        before.sort();
        assert_eq!(applicable(&ctx, &en, &EtbApplied::default()), Ok(before));
        assert_eq!(
            applicable(&ctx, &en, &applied(&[add_id])),
            Ok(vec![e(has_id)])
        );
        let out = drive(&ctx, &[en], &no_limit(), &[]);
        assert!(out.asked.is_empty());
        let r = &out.result.unwrap()[0];
        assert!(r.projected.tapped);
        assert!(r.projected.counters.is_empty());
        assert_eq!(r.projected.applied.effects, [e(add_id), e(has_id)].into());
        assert!(
            r.projected
                .evaluated
                .characteristics
                .card_types
                .contains("Creature")
        );
    }
}

/// REQ-EVENT-001, CR 614.12, CR 616.2 (acceptance 1): two type-adding effects in one tier, one
/// of which makes a conditioned effect applicable; both application orders reach the same final
/// set.
#[test]
fn req_event_001_type_changes_in_both_application_orders() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let fx = effects(vec![
        EtbEffect::new(e(1), EtbAction::AddCardTypes(set(["Creature"]))),
        EtbEffect::new(e(2), EtbAction::AddCardTypes(set(["Enchantment"]))),
        EtbEffect::new(e(3), EtbAction::EntersTapped)
            .with_condition(EtbCondition::HasCardType("Enchantment".to_owned()))
            .with_tier(Tier::SelfReplacement),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    for first in [0, 1] {
        let out = drive(&ctx, &[entry(o(1), p(1))], &no_limit(), &[pick(first)]);
        assert_eq!(out.asked.len(), 1);
        let r = &out.result.unwrap()[0];
        assert_eq!(r.projected.applied.effects, [e(1), e(2), e(3)].into());
        assert!(r.projected.tapped);
        assert_eq!(
            r.projected.evaluated.characteristics.card_types,
            set(["Artifact", "Creature", "Enchantment"])
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Copy (requirement 1 exception)
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001, CR 614.12, CR 616.1c: an applied copy replaces the base; the entry's own
/// statics are no longer registered and its own `ThisPermanent` effects no longer apply.
#[test]
fn req_event_001_copy_replaces_base_and_drops_own_abilities() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let copied = card("test.etb_copied_2_2", "Creature", 2, 2);
    let fx = effects(vec![
        EtbEffect::new(e(1), EtbAction::EntersAsCopyOf(Box::new(copied.clone())))
            .with_tier(Tier::CopyEtb),
        EtbEffect::new(e(2), EtbAction::EntersTapped)
            .with_source(o(1))
            .with_scope(EtbScope::ThisPermanent),
        EtbEffect::new(e(3), counters(1))
            .with_condition(EtbCondition::HasCardType("Creature".to_owned())),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let mut en = entry(o(1), p(1));
    en.own_statics
        .push((e(100), add_type(Some(o(1)), o(1), "Enchantment")));
    let before = project(&ctx, &en, &EtbApplied::default()).unwrap();
    assert_eq!(
        before.evaluated.characteristics.card_types,
        set(["Artifact", "Enchantment"])
    );
    assert_eq!(
        applicable(&ctx, &en, &EtbApplied::default()),
        Ok(vec![e(1), e(2)])
    );
    let after = project(&ctx, &en, &applied(&[1])).unwrap();
    assert_eq!(after.evaluated.characteristics, copied);
    assert_eq!(applicable(&ctx, &en, &applied(&[1])), Ok(vec![e(3)]));
    // The copy tier goes first; the own effect is then gone.
    let out = drive(&ctx, std::slice::from_ref(&en), &no_limit(), &[]);
    let r = &out.result.unwrap()[0];
    assert_eq!(r.projected.applied.effects, [e(1), e(3)].into());
    assert!(!r.projected.tapped);
    // An own effect applied before the copy stays applied.
    let kept = project(&ctx, &en, &applied(&[1, 2])).unwrap();
    assert!(kept.tapped);
    // The last copy (highest id) wins.
    let other = card("test.etb_copied_other", "Enchantment", 1, 1);
    let fx2 = effects(vec![
        EtbEffect::new(e(1), EtbAction::EntersAsCopyOf(Box::new(copied))),
        EtbEffect::new(e(5), EtbAction::EntersAsCopyOf(Box::new(other.clone()))),
    ]);
    let ctx2 = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx2,
    };
    let both = project(&ctx2, &en, &applied(&[1, 5])).unwrap();
    // (A noncreature permanent has no P/T, CR 208.3, so compare the type line and name.)
    assert_eq!(both.evaluated.characteristics.names, other.names);
    assert_eq!(both.evaluated.characteristics.card_types, other.card_types);
}

// ---------------------------------------------------------------------------------------------
// Tiers and the 616.1f loop
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001, CR 616.1a-e, CR 616.1: a higher-priority tier is applied first without
/// asking; several `Other` effects produce a `CHOOSE_REPLACEMENT_EFFECT` request with the exact
/// fields; an answer of the wrong kind or out of range is refused.
#[test]
fn req_event_001_tiers_and_request() {
    let mut env = Env::default().with(o(4), artifact("test.etb_card_a"));
    // One controller per projection (H2): the environment reports the entry's controller.
    env.bases.get_mut(&o(4)).unwrap().controller = Some(p(2));
    let continuous = ContinuousEffects::new();
    let fx = effects(vec![
        EtbEffect::new(e(9), EtbAction::EntersTapped).with_description("test.desc_tapped"),
        EtbEffect::new(e(3), counters(1)).with_description("test.desc_counter"),
        EtbEffect::new(e(5), EtbAction::AddCardTypes(set(["Creature"])))
            .with_tier(Tier::ControlEtb),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = [entry(o(4), p(2))];
    let out = drive(&ctx, &en, &no_limit(), &[]);
    assert_eq!(out.result, Err(ChoiceError::NotPending));
    let expected = PendingChoiceRequest {
        request_id: RequestId::new(0),
        chooser: p(2),
        choice_type: ChoiceType::ChooseReplacementEffect,
        reason: ChoiceReason {
            cr: "CR 616.1".to_owned(),
            source: Some(o(4)),
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SelectReplacement { index: 0 },
        options: ChoiceOptions::Enumerated(vec![ChoiceOption::Number(0), ChoiceOption::Number(1)]),
        payload: ChoicePayload::ChooseReplacementEffect {
            event: "ZONE_CHANGE".to_owned(),
            effects: vec![
                "test.desc_counter".to_owned(),
                "test.desc_tapped".to_owned(),
            ],
        },
    };
    assert_eq!(out.asked, vec![expected]);
    // The ControlEtb effect applied first, without asking; picking index 1 applies e(9), then
    // e(3) is the only one left.
    let out = drive(&ctx, &en, &no_limit(), &[pick(1)]);
    assert_eq!(out.asked.len(), 1);
    let r = &out.result.unwrap()[0];
    assert_eq!(r.projected.applied.effects, [e(3), e(5), e(9)].into());
    assert_eq!(r.projected.counters.get("charge"), Some(&1));
    assert!(r.projected.tapped);
    for bad in [pick(2), text("x")] {
        let out = drive(&ctx, &en, &no_limit(), &[bad]);
        assert!(matches!(out.result, Err(ChoiceError::Invalid { .. })));
    }
}

/// REQ-EVENT-001, CR 616.1f, CR 616.2: an effect made applicable by an applied one is applied
/// in a later round; an effect is never applied twice.
#[test]
fn req_event_001_repeat_until_none_and_never_twice() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let fx = effects(vec![
        EtbEffect::new(e(1), EtbAction::AddCardTypes(set(["Creature"]))),
        EtbEffect::new(e(2), EtbAction::AddCardTypes(set(["Enchantment"])))
            .with_condition(EtbCondition::HasCardType("Creature".to_owned())),
        EtbEffect::new(e(3), counters(2))
            .with_condition(EtbCondition::HasCardType("Enchantment".to_owned())),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &[entry(o(1), p(1))], &no_limit(), &[]);
    assert!(out.asked.is_empty());
    let r = &out.result.unwrap()[0];
    assert_eq!(r.projected.applied.effects, [e(1), e(2), e(3)].into());
    // Each applied once: 2 counters, not more.
    assert_eq!(r.projected.counters.get("charge"), Some(&2));
    assert_eq!(
        applicable(&ctx, &entry(o(1), p(1)), &r.projected.applied),
        Ok(vec![])
    );
}

// ---------------------------------------------------------------------------------------------
// Choices (acceptance 2)
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001, CR 614.12a, CR 614.12c, CR 607.2d, CR 607.2f (acceptance 2): the
/// `CHOOSE_OPTION_OR_VALUE` request for each kind; the value lands in Memory, in
/// `applied.chosen` and, through `record_into`, in a MemoryTable that keeps its other keys.
#[test]
fn req_event_001_choice_requests_and_memory() {
    let cases = [
        (
            EtbChoiceKind::Color,
            "COLOR",
            "CR 614.12a",
            ChoiceValue::Color("blue".to_owned()),
        ),
        (
            EtbChoiceKind::CreatureType,
            "CREATURE_TYPE",
            "CR 614.12a",
            ChoiceValue::CreatureType("blue".to_owned()),
        ),
        (
            EtbChoiceKind::Anchor,
            "ANCHOR_WORD",
            "CR 614.12c",
            ChoiceValue::Anchor("blue".to_owned()),
        ),
    ];
    for with_source in [false, true] {
        for (kind, option_type, cr, stored) in cases.clone() {
            let mut env = Env::default().with(o(1), artifact("test.etb_card_a"));
            // One controller per projection (H2): the environment reports the entry's.
            env.bases.get_mut(&o(1)).unwrap().controller = Some(p(3));
            let continuous = ContinuousEffects::new();
            let mut fx = EtbEffect::new(
                e(1),
                choose("chosen", kind, vec![opt("red", 0), opt("blue", 0)]),
            );
            if with_source {
                fx = fx.with_source(o(8));
            }
            let fx = effects(vec![fx]);
            let ctx = EtbContext {
                env: &env,
                continuous: &continuous,
                effects: &fx,
            };
            let en = [entry(o(1), p(3))];
            let expected = PendingChoiceRequest {
                request_id: RequestId::new(0),
                chooser: p(3),
                choice_type: ChoiceType::ChooseOptionOrValue,
                reason: ChoiceReason {
                    cr: cr.to_owned(),
                    source: Some(if with_source { o(8) } else { o(1) }),
                },
                min: 1,
                max: 1,
                hidden: false,
                reveal_to: Vec::new(),
                default_on_timeout: text("red"),
                options: ChoiceOptions::Enumerated(vec![
                    ChoiceOption::Text("red".to_owned()),
                    ChoiceOption::Text("blue".to_owned()),
                ]),
                payload: ChoicePayload::ChooseOptionOrValue {
                    option_type: option_type.to_owned(),
                    allowed: AllowedValues::Texts(vec!["red".to_owned(), "blue".to_owned()]),
                },
            };
            let out = drive(&ctx, &en, &no_limit(), &[text("blue")]);
            assert_eq!(out.asked, vec![expected]);
            let r = &out.result.unwrap()[0];
            assert_eq!(r.memory.choice("chosen"), Some(&stored));
            assert_eq!(r.projected.applied.chosen.get("chosen"), Some(&stored));
            let mut table = MemoryTable::new();
            table
                .get_mut_or_default(o(50))
                .set_choice("other", ChoiceValue::Number(4));
            r.record_into(&mut table, o(50));
            let m = table.get(o(50)).unwrap();
            assert_eq!(m.choice("chosen"), Some(&stored));
            assert_eq!(m.choice("other"), Some(&ChoiceValue::Number(4)));
            // A fresh table gets a new Memory.
            let mut fresh = MemoryTable::new();
            r.record_into(&mut fresh, o(60));
            assert_eq!(fresh.get(o(60)).unwrap().choice("chosen"), Some(&stored));
        }
    }
}

/// REQ-EVENT-001, CR 614.12a: an answer outside the allowed texts, or of another kind, is
/// refused; nothing is recorded.
#[test]
fn req_event_001_choice_wrong_answer_rejected() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let fx = effects(vec![EtbEffect::new(
        e(1),
        choose(
            "k",
            EtbChoiceKind::Color,
            vec![opt("red", 0), opt("blue", 0)],
        ),
    )]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = [entry(o(1), p(1))];
    for bad in [text("green"), pick(0)] {
        let out = drive(&ctx, &en, &no_limit(), &[bad]);
        assert!(matches!(out.result, Err(ChoiceError::Invalid { .. })));
    }
}

/// REQ-EVENT-001, CR 614.12a: a single allowed option is chosen without asking.
#[test]
fn req_event_001_single_option_not_asked() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let fx = effects(vec![EtbEffect::new(
        e(1),
        choose("k", EtbChoiceKind::Anchor, vec![opt("Alpha", 0)]),
    )]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &[entry(o(1), p(1))], &no_limit(), &[]);
    assert!(out.asked.is_empty());
    let r = &out.result.unwrap()[0];
    assert_eq!(
        r.memory.choice("k"),
        Some(&ChoiceValue::Anchor("Alpha".to_owned()))
    );
}

/// REQ-EVENT-001, CR 607.2d, CR 614.12a: two choices under one key: the last chosen value wins
/// in Memory and in `applied.chosen`; the earlier value is in the projection the later step
/// reads.
#[test]
fn req_event_001_last_choice_for_a_key_wins() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let fx = effects(vec![
        EtbEffect::new(
            e(1),
            choose(
                "k",
                EtbChoiceKind::Color,
                vec![opt("red", 0), opt("blue", 0)],
            ),
        )
        .with_tier(Tier::SelfReplacement),
        EtbEffect::new(
            e(2),
            choose(
                "k",
                EtbChoiceKind::Color,
                vec![opt("white", 0), opt("black", 0)],
            ),
        ),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let mut after_first = applied(&[1]);
    after_first
        .chosen
        .insert("k".to_owned(), ChoiceValue::Color("red".to_owned()));
    let projected = project(&ctx, &entry(o(1), p(1)), &after_first).unwrap();
    assert_eq!(
        projected.applied.chosen.get("k"),
        Some(&ChoiceValue::Color("red".to_owned()))
    );
    let out = drive(
        &ctx,
        &[entry(o(1), p(1))],
        &no_limit(),
        &[text("red"), text("black")],
    );
    assert_eq!(out.asked.len(), 2);
    assert_eq!(out.asked[1].request_id, RequestId::new(1));
    let r = &out.result.unwrap()[0];
    let black = ChoiceValue::Color("black".to_owned());
    assert_eq!(r.memory.choice("k"), Some(&black));
    assert_eq!(r.projected.applied.chosen.get("k"), Some(&black));
    assert_eq!(r.projected.applied.chosen.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// Payability (CR 614.12b)
// ---------------------------------------------------------------------------------------------

/// Two entering objects o(1), o(2), each with its own `Choose`: `a` (cheap 1, dear 3, free 0)
/// and `b` (cheap 1, dear 3).
fn payable_fixture() -> (Env, EtbEffects) {
    let env = Env::default()
        .with(o(1), artifact("test.etb_card_a"))
        .with(o(2), artifact("test.etb_card_b"));
    let fx = effects(vec![
        EtbEffect::new(
            e(1),
            choose(
                "a",
                EtbChoiceKind::Color,
                vec![opt("cheap_a", 1), opt("dear_a", 3), opt("free_a", 0)],
            ),
        )
        .with_source(o(1))
        .with_scope(EtbScope::ThisPermanent),
        EtbEffect::new(
            e(2),
            choose(
                "b",
                EtbChoiceKind::Color,
                vec![opt("cheap_b", 1), opt("dear_b", 3)],
            ),
        )
        .with_source(o(2))
        .with_scope(EtbScope::ThisPermanent),
    ]);
    (env, fx)
}

/// REQ-EVENT-001, CR 614.12b: with a budget of 3, the first entry's expensive option would
/// leave the second choice unpayable, so it is not offered (the cost-0 option is); the chosen
/// cost is spent, so the second entry has one allowed option and is not asked.
#[test]
fn req_event_001_payable_lookahead_two_entries() {
    let (env, fx) = payable_fixture();
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let entries = [entry(o(1), p(1)), entry(o(2), p(1))];
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 3)].into();
    let out = drive(&ctx, &entries, &budget, &[text("cheap_a")]);
    assert_eq!(out.asked.len(), 1);
    assert_eq!(out.asked[0].payload, texts(&["cheap_a", "free_a"]));
    let r = out.result.unwrap();
    assert_eq!(
        r[1].memory.choice("b"),
        Some(&ChoiceValue::Color("cheap_b".to_owned()))
    );
    // With budget 4, the expensive option fits the lookahead (3 + 1).
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 4)].into();
    let out = drive(&ctx, &entries, &budget, &[text("dear_a")]);
    assert_eq!(
        out.asked[0].payload,
        texts(&["cheap_a", "dear_a", "free_a"])
    );
    let r = out.result.unwrap();
    assert_eq!(
        r[1].memory.choice("b"),
        Some(&ChoiceValue::Color("cheap_b".to_owned()))
    );
    // Choosing the free option leaves 4 for the second entry: both of its options are offered.
    let out = drive(&ctx, &entries, &budget, &[text("free_a"), text("dear_b")]);
    assert_eq!(out.asked.len(), 2);
    assert_eq!(out.asked[1].payload, texts(&["cheap_b", "dear_b"]));
    assert!(out.result.is_ok());
}

/// REQ-EVENT-001, CR 614.12b: a choice with no payable option is `Unpayable` ("not payable");
/// a cost-0 option is always allowed; a player absent from `payable` has no limit; another
/// chooser's choices are not in the lookahead.
#[test]
fn req_event_001_payable_unpayable_free_and_unlimited() {
    let (env, fx) = payable_fixture();
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let zero: BTreeMap<PlayerId, u64> = [(p(1), 0)].into();
    // Unpayable.
    let out = drive(&ctx, &[entry(o(2), p(1))], &zero, &[]);
    let message = procedure_message(out.result);
    assert!(message.contains("not payable"));
    assert_eq!(
        message,
        EtbError::Unpayable {
            chooser: p(1),
            key: "b".to_owned()
        }
        .to_string()
    );
    // Cost 0 is allowed whatever the lookahead: one option, not asked.
    let out = drive(&ctx, &[entry(o(1), p(1))], &zero, &[]);
    assert!(out.asked.is_empty());
    assert_eq!(
        out.result.unwrap()[0].memory.choice("a"),
        Some(&ChoiceValue::Color("free_a".to_owned()))
    );
    // Absent chooser: no limit.
    let other: BTreeMap<PlayerId, u64> = [(p(2), 0)].into();
    let out = drive(
        &ctx,
        &[entry(o(1), p(1)), entry(o(2), p(1))],
        &other,
        &[text("dear_a"), text("dear_b")],
    );
    assert_eq!(
        out.asked[0].payload,
        texts(&["cheap_a", "dear_a", "free_a"])
    );
    assert_eq!(out.asked[1].payload, texts(&["cheap_b", "dear_b"]));
    assert!(out.result.is_ok());
    // A different chooser's entry is not reserved: budget 3 for p(1), the second entry is p(2)
    // (one controller per projection, H2: the environment reports p(2) for it).
    let mut env = env;
    env.bases.get_mut(&o(2)).unwrap().controller = Some(p(2));
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 3)].into();
    let out = drive(
        &ctx,
        &[entry(o(1), p(1)), entry(o(2), p(2))],
        &budget,
        &[text("dear_a"), text("dear_b")],
    );
    assert_eq!(
        out.asked[0].payload,
        texts(&["cheap_a", "dear_a", "free_a"])
    );
    assert!(out.result.is_ok());
}

/// REQ-EVENT-001, CR 614.12b: the lookahead counts the other `Choose` effects of the same entry
/// that are applicable now (any tier).
#[test]
fn req_event_001_payable_lookahead_same_entry() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let fx = effects(vec![
        EtbEffect::new(
            e(1),
            choose(
                "late",
                EtbChoiceKind::Color,
                vec![opt("big", 2), opt("small", 1)],
            ),
        ),
        EtbEffect::new(
            e(3),
            choose(
                "early",
                EtbChoiceKind::Color,
                vec![opt("c", 1), opt("d", 5), opt("f", 2)],
            ),
        )
        .with_tier(Tier::SelfReplacement),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 3)].into();
    // early: reserve 1 (cheapest of late): c (2 <= 3) and f (3 <= 3); d no.
    let out = drive(&ctx, &[entry(o(1), p(1))], &budget, &[text("f")]);
    assert_eq!(out.asked.len(), 1);
    assert_eq!(out.asked[0].payload, texts(&["c", "f"]));
    let r = &out.result.unwrap()[0];
    // 3 - 2 = 1 left: only small.
    assert_eq!(
        r.memory.choice("late"),
        Some(&ChoiceValue::Color("small".to_owned()))
    );
}

// ---------------------------------------------------------------------------------------------
// Determinism and invariants
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001, REQ-CORE-001, CR 616.1: answering one by one and replaying all answers at
/// once give the same results.
#[test]
fn req_event_001_session_equals_replay() {
    let (env, mut fx) = payable_fixture();
    fx.add(EtbEffect::new(e(7), EtbAction::EntersTapped))
        .unwrap();
    fx.add(EtbEffect::new(e(8), counters(1))).unwrap();
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let entries = [entry(o(1), p(1)), entry(o(2), p(1))];
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 9)].into();
    let answers = [
        pick(1),
        pick(0),
        text("dear_a"),
        pick(1),
        pick(0),
        text("cheap_b"),
    ];
    let out = drive(&ctx, &entries, &budget, &answers);
    let one_by_one = out.result.unwrap();
    assert_eq!(out.asked.len(), answers.len());
    for (i, r) in out.asked.iter().enumerate() {
        assert_eq!(r.request_id, RequestId::new(u64::try_from(i).unwrap()));
    }
    let proc =
        |_s: &mut Nil, c: &mut ChoiceContext<'_>| resolve_entering(&ctx, &entries, &budget, c);
    let replayed = ChoiceSession::replay(Nil, proc, &answers).unwrap();
    let Status::Done { output, .. } = replayed.status() else {
        panic!("replay did not finish");
    };
    assert_eq!(*output, one_by_one);
    // A pending interrupt reaches the caller unchanged.
    let mut fresh = Vec::new();
    let proc =
        |_s: &mut Nil, c: &mut ChoiceContext<'_>| resolve_entering(&ctx, &entries, &budget, c);
    let started = ChoiceSession::start(Nil, proc).unwrap();
    if let Some(r) = started.pending() {
        fresh.push(r.clone());
    }
    assert_eq!(fresh.first(), out.asked.first());
}

/// REQ-EVENT-001, CR 614.12, CR 616.1f: over a grid of scopes, conditions, tiers and applied
/// sets, `applicable` is strictly ascending, never contains an applied id, matches its
/// definition, and neither `project` nor `applicable` changes its inputs.
#[test]
fn req_event_001_property_grid() {
    let env = Env::default()
        .with(o(1), artifact("test.etb_card_a"))
        .with(o(2), artifact("test.etb_card_b"));
    let mut continuous = ContinuousEffects::new();
    continuous
        .register(e(500), add_subtype(o(1), "Golem"))
        .unwrap();
    let scopes = [EtbScope::ThisPermanent, EtbScope::OtherPermanents];
    let conditions = [
        EtbCondition::Always,
        EtbCondition::HasCardType("Artifact".to_owned()),
        EtbCondition::LacksCardType("Artifact".to_owned()),
        EtbCondition::HasCardType("Creature".to_owned()),
        EtbCondition::HasSubtype("Golem".to_owned()),
    ];
    let tiers = [Tier::SelfReplacement, Tier::CopyEtb, Tier::Other];
    let sources = [None, Some(o(1)), Some(o(2))];
    let mut list = Vec::new();
    let mut id = 1;
    for scope in scopes {
        for condition in &conditions {
            for tier in tiers {
                for source in sources {
                    let mut fx = EtbEffect::new(e(id), EtbAction::EntersTapped)
                        .with_scope(scope)
                        .with_condition(condition.clone())
                        .with_tier(tier);
                    if let Some(s) = source {
                        fx = fx.with_source(s);
                    }
                    list.push(fx);
                    id += 1;
                }
            }
        }
    }
    list.push(EtbEffect::new(
        e(id),
        EtbAction::AddCardTypes(set(["Creature"])),
    ));
    let fx = effects(list);
    let (fx_before, cont_before, env_before) = (fx.clone(), continuous.clone(), env.clone());
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    for object in [o(1), o(2)] {
        let en = entry(object, p(1));
        for step in 0..5u64 {
            let mut done: Vec<u64> = (1..=id).filter(|n| n % 5 == step).collect();
            if step == 4 {
                done.push(id);
            }
            let ap = applied(&done);
            let ap_before = ap.clone();
            let got = applicable(&ctx, &en, &ap).unwrap();
            assert!(got.windows(2).all(|w| w[0] < w[1]));
            assert!(got.iter().all(|i| !ap.effects.contains(i)));
            let projected = project(&ctx, &en, &ap).unwrap();
            let types = &projected.evaluated.characteristics.card_types;
            for n in 1..=id {
                let effect = fx.get(e(n)).unwrap();
                let scope_ok = match effect.scope {
                    EtbScope::ThisPermanent => effect.source == Some(object),
                    EtbScope::OtherPermanents => effect.source != Some(object),
                };
                let cond_ok = match &effect.condition {
                    EtbCondition::Always => true,
                    EtbCondition::HasCardType(t) => types.contains(t),
                    EtbCondition::LacksCardType(t) => !types.contains(t),
                    EtbCondition::HasSubtype(t) => {
                        projected.evaluated.characteristics.subtypes.contains(t)
                    }
                };
                let expected = !ap.effects.contains(&e(n)) && scope_ok && cond_ok;
                assert_eq!(
                    got.contains(&e(n)),
                    expected,
                    "effect {n}, object {object:?}"
                );
            }
            assert_eq!(ap, ap_before);
            assert_eq!(project(&ctx, &en, &ap).unwrap(), projected);
        }
    }
    assert_eq!(fx, fx_before);
    assert_eq!(continuous, cont_before);
    assert_eq!(env, env_before);
}

/// REQ-EVENT-001, CR 614.12: the projection reads the rest of the environment through the
/// wrapper unchanged: its counters (CR 613.4c) and selector matching against the projected view
/// (CR 613.5); the object list is the environment's.
#[test]
fn req_event_001_projection_forwards_environment() {
    let mut env = Env::default().with(o(1), card("test.etb_creature_2_2", "Creature", 2, 2));
    env.counters
        .insert(o(1), [("+1/+1".to_owned(), 1)].into_iter().collect());
    let mut continuous = ContinuousEffects::new();
    continuous
        .register(
            e(300),
            ContinuousEffect {
                affected: Affected::Selector(ObjectSelectorAST::Ref(
                    fineprint_dsl::expr::ReferenceAST::SelfRef,
                )),
                ..add_subtype(o(1), "Golem")
            },
        )
        .unwrap();
    let fx = effects(vec![
        EtbEffect::new(e(1), EtbAction::EntersTapped)
            .with_condition(EtbCondition::HasSubtype("Golem".to_owned())),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let projected = project(&ctx, &entry(o(1), p(1)), &EtbApplied::default()).unwrap();
    let c = &projected.evaluated.characteristics;
    assert_eq!(c.power.as_ref().and_then(literal_of), Some(3));
    assert!(c.subtypes.contains("Golem"));
    assert_eq!(
        applicable(&ctx, &entry(o(1), p(1)), &EtbApplied::default()),
        Ok(vec![e(1)])
    );
    assert_eq!(env.objects(), vec![o(1)]);
}

// ---------------------------------------------------------------------------------------------
// PR #202 review round 1 (F1-F8)
// ---------------------------------------------------------------------------------------------

/// An environment whose `generating_ability_exists` hook is `exists`; everything else is the
/// inner [`Env`].
#[derive(Clone, Debug)]
struct Hooked {
    inner: Env,
    exists: fn(EffectId, ObjectId) -> bool,
}

impl LayerEnvironment for Hooked {
    fn objects(&self) -> Vec<ObjectId> {
        self.inner.objects()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.inner.base(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.inner.counters(id)
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        self.inner.matches(selector, ctx)
    }

    fn generating_ability_exists(
        &self,
        effect: EffectId,
        source: ObjectId,
        _source_view: Option<&crate::layers::Evaluated>,
    ) -> bool {
        (self.exists)(effect, source)
    }
}

/// `applied` with the explicit application `order`.
fn in_order(order: &[u64]) -> EtbApplied {
    EtbApplied {
        effects: order.iter().map(|&n| e(n)).collect(),
        order: order.iter().map(|&n| e(n)).collect(),
        ..Default::default()
    }
}

/// A copy effect into `model`, tier CopyEtb.
fn copy_of(id: u64, model: Characteristics) -> EtbEffect {
    EtbEffect::new(e(id), EtbAction::EntersAsCopyOf(Box::new(model))).with_tier(Tier::CopyEtb)
}

fn creature_tapped(id: u64) -> EtbEffect {
    EtbEffect::new(e(id), EtbAction::EntersTapped)
        .with_condition(EtbCondition::HasCardType("Creature".to_owned()))
}

fn names(p: &ProjectedPermanent) -> BTreeSet<String> {
    p.evaluated.characteristics.names.clone()
}

/// F1 fixture: e1 copies a creature, e2 an artifact (both CopyEtb); e3 taps a creature.
fn copy_order_fixture() -> (Env, EtbEffects) {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let fx = effects(vec![
        copy_of(1, card("test.etb_model_creature", "Creature", 2, 2)),
        copy_of(2, artifact("test.etb_model_artifact")),
        creature_tapped(3),
    ]);
    (env, fx)
}

/// REQ-EVENT-001, CR 616.1f, CR 707.2 (F1): `sequence` is `order` without unknown or repeated
/// ids, then the rest ascending; an empty `order` is ascending.
#[test]
fn req_event_001_applied_sequence() {
    let mut a = applied(&[7, 1, 5, 3]);
    assert_eq!(a.sequence(), vec![e(1), e(3), e(5), e(7)]);
    a.order = vec![e(5), e(9), e(5), e(1)];
    assert_eq!(a.sequence(), vec![e(5), e(1), e(3), e(7)]);
    assert_eq!(EtbApplied::default().sequence(), Vec::<EffectId>::new());
}

/// REQ-EVENT-001, CR 616.1f, CR 707.2 (F1): the LAST applied copy supplies the base, in the
/// application order: e2 then e1 projects the creature (e3 applies), e1 then e2 the artifact;
/// an empty order is ascending id (the artifact).
#[test]
fn req_event_001_copy_order_project() {
    let (env, fx) = copy_order_fixture();
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = entry(o(1), p(1));
    let creature_last = project(&ctx, &en, &in_order(&[2, 1])).unwrap();
    assert_eq!(names(&creature_last), set(["test.etb_model_creature"]));
    assert_eq!(applicable(&ctx, &en, &in_order(&[2, 1])), Ok(vec![e(3)]));
    let artifact_last = project(&ctx, &en, &in_order(&[1, 2])).unwrap();
    assert_eq!(names(&artifact_last), set(["test.etb_model_artifact"]));
    assert_eq!(applicable(&ctx, &en, &in_order(&[1, 2])), Ok(vec![]));
    let ascending = project(&ctx, &en, &applied(&[1, 2])).unwrap();
    assert_eq!(names(&ascending), set(["test.etb_model_artifact"]));
}

/// REQ-EVENT-001, CR 616.1e, CR 616.1f, CR 707.2 (F1): through `resolve_entering`, choosing e2
/// first makes the creature copy last (tapped); choosing e1 first leaves the artifact
/// (untapped). `order` records the real order.
#[test]
fn req_event_001_copy_order_resolve() {
    let (env, fx) = copy_order_fixture();
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = [entry(o(1), p(1))];
    let out = drive(&ctx, &en, &no_limit(), &[pick(1)]);
    assert_eq!(out.asked.len(), 1);
    let r = &out.result.unwrap()[0];
    assert!(r.projected.tapped);
    assert_eq!(names(&r.projected), set(["test.etb_model_creature"]));
    assert_eq!(r.projected.applied.order, vec![e(2), e(1), e(3)]);
    assert_eq!(r.projected.applied.effects, [e(1), e(2), e(3)].into());
    let out = drive(&ctx, &en, &no_limit(), &[pick(0)]);
    let r = &out.result.unwrap()[0];
    assert!(!r.projected.tapped);
    assert_eq!(names(&r.projected), set(["test.etb_model_artifact"]));
    assert_eq!(r.projected.applied.order, vec![e(1), e(2)]);
}

/// F2 fixture: o(1) an artifact with its own ThisPermanent counter effect e4; e1 copies an
/// artifact model o(77) whose declared static (id 400, locked on o(77), source o(77)) adds
/// Creature and whose declared ETB effect e3 (ThisPermanent of o(77)) taps; e2 taps a creature.
fn copy_abilities_fixture(declare: bool) -> (Env, EtbEffects) {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let mut fx = effects(vec![
        copy_of(1, artifact("test.etb_model_artifact")),
        creature_tapped(2).with_description("creature tapped"),
        EtbEffect::new(e(3), counters(5))
            .with_source(o(77))
            .with_scope(EtbScope::ThisPermanent),
        EtbEffect::new(e(4), counters(1))
            .with_source(o(1))
            .with_scope(EtbScope::ThisPermanent),
    ]);
    if declare {
        fx.set_copy_abilities(
            e(1),
            CopyAbilities {
                statics: vec![(e(400), add_type(Some(o(77)), o(77), "Creature"))],
                etb: vec![e(3)],
            },
        )
        .unwrap();
    }
    (env, fx)
}

/// REQ-EVENT-001, CR 614.12, CR 707.5 (F2): the last copy's declared static, rebound to the
/// entrant (its locked set and its source), makes a creature-gated effect applicable; its
/// declared ETB effect applies; the original's own ThisPermanent effect does not.
#[test]
fn req_event_001_copy_abilities_apply() {
    for declare in [false, true] {
        let (env, fx) = copy_abilities_fixture(declare);
        // The hook keeps effect 400 only when its source is the entrant: the source is rebound.
        let hooked = Hooked {
            inner: env,
            exists: |effect, source| effect != e(400) || source == o(1),
        };
        let continuous = ContinuousEffects::new();
        let ctx = EtbContext {
            env: &hooked,
            continuous: &continuous,
            effects: &fx,
        };
        let en = entry(o(1), p(1));
        // Before the copy: the original's own effect applies, the declaration does nothing.
        assert_eq!(
            applicable(&ctx, &en, &EtbApplied::default()),
            Ok(vec![e(1), e(4)])
        );
        let before = project(&ctx, &en, &EtbApplied::default()).unwrap();
        assert!(
            !before
                .evaluated
                .characteristics
                .card_types
                .contains("Creature")
        );
        let after = project(&ctx, &en, &applied(&[1])).unwrap();
        let types = &after.evaluated.characteristics.card_types;
        if declare {
            assert_eq!(*types, set(["Artifact", "Creature"]));
            assert_eq!(applicable(&ctx, &en, &applied(&[1])), Ok(vec![e(2), e(3)]));
        } else {
            assert_eq!(*types, set(["Artifact"]));
            assert_eq!(applicable(&ctx, &en, &applied(&[1])), Ok(vec![]));
        }
        let out = drive(&ctx, std::slice::from_ref(&en), &no_limit(), &[pick(0)]);
        let r = &out.result.unwrap()[0];
        assert_eq!(r.projected.tapped, declare);
        assert_eq!(
            r.projected.counters.get("charge").copied(),
            declare.then_some(5)
        );
        // The caller's registry never holds the declared static.
        assert!(continuous.is_empty());
    }
}

/// REQ-EVENT-001, CR 707.5, CR 707.2, CR 616.1f (F2): with two copies, only the last applied
/// copy's declaration counts.
#[test]
fn req_event_001_copy_abilities_only_last_copy() {
    let (env, mut fx) = copy_abilities_fixture(true);
    fx.add(copy_of(5, artifact("test.etb_model_other")))
        .unwrap();
    fx.add(
        EtbEffect::new(e(6), counters(2))
            .with_source(o(78))
            .with_scope(EtbScope::ThisPermanent),
    )
    .unwrap();
    fx.set_copy_abilities(
        e(5),
        CopyAbilities {
            statics: Vec::new(),
            etb: vec![e(6)],
        },
    )
    .unwrap();
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = entry(o(1), p(1));
    // e5 last: e6 applies, not e3; no Creature from e1's static.
    assert_eq!(applicable(&ctx, &en, &in_order(&[1, 5])), Ok(vec![e(6)]));
    // e1 last: e3 and the creature-gated e2 apply, not e6.
    assert_eq!(
        applicable(&ctx, &en, &in_order(&[5, 1])),
        Ok(vec![e(2), e(3)])
    );
}

/// REQ-EVENT-001, CR 707.5 (F2): `set_copy_abilities` refuses an unknown or non-copy id;
/// `remove(copy)` drops the declaration; a re-declaration replaces the earlier one.
#[test]
fn req_event_001_copy_abilities_registry() {
    let (env, mut fx) = copy_abilities_fixture(true);
    assert_eq!(
        fx.set_copy_abilities(e(99), CopyAbilities::default()),
        Err(EtbError::NotACopyEffect(e(99)))
    );
    assert_eq!(
        fx.set_copy_abilities(e(3), CopyAbilities::default()),
        Err(EtbError::NotACopyEffect(e(3)))
    );
    assert!(!EtbError::NotACopyEffect(e(3)).to_string().is_empty());
    let continuous = ContinuousEffects::new();
    let en = entry(o(1), p(1));
    {
        let ctx = EtbContext {
            env: &env,
            continuous: &continuous,
            effects: &fx,
        };
        assert_eq!(applicable(&ctx, &en, &applied(&[1])), Ok(vec![e(2), e(3)]));
    }
    // Re-declaring replaces.
    let mut redeclared = fx.clone();
    redeclared
        .set_copy_abilities(e(1), CopyAbilities::default())
        .unwrap();
    {
        let ctx = EtbContext {
            env: &env,
            continuous: &continuous,
            effects: &redeclared,
        };
        assert_eq!(applicable(&ctx, &en, &applied(&[1])), Ok(vec![]));
    }
    // Removing the copy drops its declaration: re-adding it brings no abilities.
    let removed = fx.remove(e(1)).unwrap();
    fx.add(removed).unwrap();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    assert_eq!(applicable(&ctx, &en, &applied(&[1])), Ok(vec![]));
}

/// REQ-EVENT-001, CR 707.5 (F2): a declared static with a selector keeps it (bound to the
/// entrant as source); a declared static whose id is already in force is a layer error.
#[test]
fn req_event_001_copy_abilities_selector_and_duplicate() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let mut fx = effects(vec![copy_of(
        1,
        card("test.etb_model_creature", "Creature", 2, 2),
    )]);
    let golem = ContinuousEffect {
        affected: Affected::Selector(ObjectSelectorAST::Ref(
            fineprint_dsl::expr::ReferenceAST::SelfRef,
        )),
        source: Some(o(77)),
        origin: EffectOrigin::Static { granted_at: None },
        ..add_subtype(o(77), "Golem")
    };
    fx.set_copy_abilities(
        e(1),
        CopyAbilities {
            statics: vec![(e(400), golem)],
            etb: Vec::new(),
        },
    )
    .unwrap();
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let projected = project(&ctx, &entry(o(1), p(1)), &applied(&[1])).unwrap();
    assert!(
        projected
            .evaluated
            .characteristics
            .subtypes
            .contains("Golem")
    );
    let mut in_force = ContinuousEffects::new();
    in_force
        .register(e(400), add_subtype(o(1), "Wall"))
        .unwrap();
    let ctx = EtbContext {
        env: &env,
        continuous: &in_force,
        effects: &fx,
    };
    assert_eq!(
        project(&ctx, &entry(o(1), p(1)), &applied(&[1])),
        Err(EtbError::Layer(LayerError::DuplicateEffect(e(400))))
    );
}

/// A layer-7c effect giving +1/+1 to every creature (the test `Env` matches only creatures).
fn creatures_get_plus_one() -> ContinuousEffect {
    ContinuousEffect {
        layer: Layer::L7c,
        affected: Affected::Selector(ObjectSelectorAST::Ref(
            fineprint_dsl::expr::ReferenceAST::SelfRef,
        )),
        modification: LayerModification::ModifyPt {
            power: 1,
            toughness: 1,
        },
        ..add_type(None, o(1), "Creature")
    }
}

/// REQ-EVENT-001, CR 613.1d, CR 208.3, CR 205.1b (F3): a printed-3/2 noncreature artifact with
/// an applied `AddCardTypes{Creature}` projects as an artifact creature 3/2, and a layer-7
/// effect for creatures sees it (4/3); without the addition it has no P/T. The caller's
/// registry is unchanged.
#[test]
fn req_event_001_added_type_is_layer_four() {
    let env = Env::default().with(o(1), card("test.etb_artifact_3_2", "Artifact", 3, 2));
    let fx = effects(vec![EtbEffect::new(
        e(1),
        EtbAction::AddCardTypes(set(["Creature"])),
    )]);
    let empty = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &empty,
        effects: &fx,
    };
    let en = entry(o(1), p(1));
    let added = project(&ctx, &en, &applied(&[1])).unwrap();
    let c = &added.evaluated.characteristics;
    assert_eq!(c.card_types, set(["Artifact", "Creature"]));
    assert_eq!(c.power.as_ref().and_then(literal_of), Some(3));
    assert_eq!(c.toughness.as_ref().and_then(literal_of), Some(2));
    let plain = project(&ctx, &en, &EtbApplied::default()).unwrap();
    assert_eq!(plain.evaluated.characteristics.power, None);
    assert_eq!(plain.evaluated.characteristics.toughness, None);
    assert!(empty.is_empty());
    // A layer-7c effect for creatures, in force (selector) sees the added type.
    let mut continuous = ContinuousEffects::new();
    continuous
        .register(e(300), creatures_get_plus_one())
        .unwrap();
    let before = continuous.clone();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let c = project(&ctx, &en, &applied(&[1]))
        .unwrap()
        .evaluated
        .characteristics;
    assert_eq!(c.power.as_ref().and_then(literal_of), Some(4));
    assert_eq!(c.toughness.as_ref().and_then(literal_of), Some(3));
    let c = project(&ctx, &en, &EtbApplied::default())
        .unwrap()
        .evaluated
        .characteristics;
    assert_eq!(c.power, None);
    assert_eq!(continuous, before);
}

/// REQ-EVENT-001, CR 613.7, CR 205.1b (F3): the added type comes after an in-force type-setting
/// effect (it keeps the added type) and survives an applied copy.
#[test]
fn req_event_001_added_type_after_set_types_and_copy() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let mut continuous = ContinuousEffects::new();
    continuous
        .register(
            e(200),
            ContinuousEffect {
                timestamp: Timestamp::new(50),
                modification: LayerModification::SetTypes {
                    supertypes: None,
                    card_types: Some(set(["Enchantment"])),
                    subtypes: None,
                },
                ..add_type(None, o(1), "Creature")
            },
        )
        .unwrap();
    let fx = effects(vec![
        EtbEffect::new(e(1), EtbAction::AddCardTypes(set(["Creature"]))),
        copy_of(2, artifact("test.etb_model_artifact")),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = entry(o(1), p(1));
    let c = project(&ctx, &en, &applied(&[1])).unwrap();
    assert_eq!(
        c.evaluated.characteristics.card_types,
        set(["Enchantment", "Creature"])
    );
    let c = project(&ctx, &en, &in_order(&[1, 2])).unwrap();
    assert_eq!(
        c.evaluated.characteristics.card_types,
        set(["Enchantment", "Creature"])
    );
}

/// REQ-EVENT-001, CR 613.7 (F3): no id or timestamp above the registry's is a layer error, not
/// a panic.
#[test]
fn req_event_001_added_type_id_overflow() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let fx = effects(vec![EtbEffect::new(
        e(1),
        EtbAction::AddCardTypes(set(["Creature"])),
    )]);
    let mut max_id = ContinuousEffects::new();
    max_id
        .register(e(u64::MAX), add_subtype(o(1), "Golem"))
        .unwrap();
    let mut max_ts = ContinuousEffects::new();
    max_ts
        .register(
            e(5),
            ContinuousEffect {
                timestamp: Timestamp::new(u64::MAX),
                ..add_subtype(o(1), "Golem")
            },
        )
        .unwrap();
    for continuous in [max_id, max_ts] {
        let ctx = EtbContext {
            env: &env,
            continuous: &continuous,
            effects: &fx,
        };
        assert_eq!(
            project(&ctx, &entry(o(1), p(1)), &applied(&[1])),
            Err(EtbError::Layer(LayerError::IdExhausted))
        );
    }
}

/// REQ-EVENT-001, CR 614.12, CR 603.6d (F4): with a layer-6 "loses all abilities" in force, the
/// entrant's own "enters tapped" applies only if the environment says its generating ability
/// still exists on the projected permanent; the default hook (true) keeps it.
#[test]
fn req_event_001_this_permanent_needs_its_ability() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let mut continuous = ContinuousEffects::new();
    continuous
        .register(
            e(200),
            ContinuousEffect {
                layer: Layer::L6,
                modification: LayerModification::RemoveAllAbilities,
                ..add_type(None, o(1), "Creature")
            },
        )
        .unwrap();
    let fx = effects(vec![
        EtbEffect::new(e(1), EtbAction::EntersTapped)
            .with_source(o(1))
            .with_scope(EtbScope::ThisPermanent),
    ]);
    let gone = Hooked {
        inner: env.clone(),
        exists: |effect, _| effect != e(1),
    };
    let present = Hooked {
        inner: env.clone(),
        exists: |_, _| true,
    };
    let en = [entry(o(1), p(1))];
    for (layer_env, tapped) in [
        (&gone as &dyn LayerEnvironment, false),
        (&present, true),
        (&env, true),
    ] {
        let ctx = EtbContext {
            env: layer_env,
            continuous: &continuous,
            effects: &fx,
        };
        let out = drive(&ctx, &en, &no_limit(), &[]);
        assert_eq!(out.result.unwrap()[0].projected.tapped, tapped);
    }
}

/// REQ-EVENT-001, CR 614.12b, CR 616.2 (F5): budget 2; entry 1 chooses {two: 2, one: 1}; entry 2
/// copies itself into a creature, whereupon a creature-gated choice costing 1 applies. Only
/// `one` is allowed (applied without a request).
#[test]
fn req_event_001_payable_newly_applicable_later_entry() {
    let env = Env::default()
        .with(o(1), artifact("test.etb_card_a"))
        .with(o(2), artifact("test.etb_card_b"));
    let fx = effects(vec![
        EtbEffect::new(
            e(1),
            choose(
                "k1",
                EtbChoiceKind::Color,
                vec![opt("two", 2), opt("one", 1)],
            ),
        )
        .with_source(o(1))
        .with_scope(EtbScope::ThisPermanent),
        copy_of(2, card("test.etb_model_creature", "Creature", 2, 2))
            .with_source(o(2))
            .with_scope(EtbScope::ThisPermanent),
        EtbEffect::new(e(3), choose("k2", EtbChoiceKind::Color, vec![opt("c", 1)]))
            .with_condition(EtbCondition::HasCardType("Creature".to_owned())),
    ]);
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 2)].into();
    let out = drive(&ctx, &[entry(o(1), p(1)), entry(o(2), p(1))], &budget, &[]);
    assert!(out.asked.is_empty());
    let r = out.result.unwrap();
    assert_eq!(
        r[0].memory.choice("k1"),
        Some(&ChoiceValue::Color("one".to_owned()))
    );
    assert_eq!(
        r[1].memory.choice("k2"),
        Some(&ChoiceValue::Color("c".to_owned()))
    );
}

/// REQ-EVENT-001, CR 614.12b, CR 616.1c, CR 707.5 (F6): budget 2; entry 1 chooses {2}; entry 2's
/// own choice costing 1 is removed by a mandatory higher-tier copy into an artifact: the total
/// is 2, payable. Without the copy the 1 is reserved and entry 1 is not payable.
#[test]
fn req_event_001_payable_removed_by_mandatory_copy() {
    let env = Env::default()
        .with(o(1), artifact("test.etb_card_a"))
        .with(o(2), card("test.etb_creature_2_2", "Creature", 2, 2));
    let base = vec![
        EtbEffect::new(
            e(1),
            choose("k1", EtbChoiceKind::Color, vec![opt("big", 2)]),
        )
        .with_source(o(1))
        .with_scope(EtbScope::ThisPermanent),
        EtbEffect::new(e(2), choose("k2", EtbChoiceKind::Color, vec![opt("x", 1)]))
            .with_source(o(2))
            .with_scope(EtbScope::ThisPermanent),
    ];
    let mut with_copy = base.clone();
    with_copy.push(
        copy_of(3, artifact("test.etb_model_artifact"))
            .with_source(o(9))
            .with_condition(EtbCondition::HasCardType("Creature".to_owned())),
    );
    let continuous = ContinuousEffects::new();
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 2)].into();
    let entries = [entry(o(1), p(1)), entry(o(2), p(1))];
    let fx = effects(with_copy);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &entries, &budget, &[]);
    let r = out.result.unwrap();
    assert_eq!(
        r[0].memory.choice("k1"),
        Some(&ChoiceValue::Color("big".to_owned()))
    );
    assert_eq!(r[1].memory.choice("k2"), None);
    assert_eq!(r[1].projected.applied.effects, [e(3)].into());
    let fx = effects(base);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &entries, &budget, &[]);
    assert_eq!(
        procedure_message(out.result),
        EtbError::Unpayable {
            chooser: p(1),
            key: "k1".to_owned()
        }
        .to_string()
    );
}

/// REQ-EVENT-001, CR 614.12b, CR 616.2 (F7): budget 3; entry 1 chooses {one: 1, two: 2}
/// (self-replacement tier), then a mandatory copy enables a choice costing 1; entry 2 needs 1.
/// Only `one` is allowed.
#[test]
fn req_event_001_payable_newly_applicable_same_entry() {
    let env = Env::default()
        .with(o(1), artifact("test.etb_card_a"))
        .with(o(2), artifact("test.etb_card_b"));
    let fx = effects(vec![
        EtbEffect::new(
            e(1),
            choose(
                "k1",
                EtbChoiceKind::Color,
                vec![opt("one", 1), opt("two", 2)],
            ),
        )
        .with_tier(Tier::SelfReplacement)
        .with_source(o(1))
        .with_scope(EtbScope::ThisPermanent),
        copy_of(2, card("test.etb_model_creature", "Creature", 2, 2))
            .with_source(o(1))
            .with_scope(EtbScope::ThisPermanent),
        EtbEffect::new(e(3), choose("k3", EtbChoiceKind::Color, vec![opt("c", 1)]))
            .with_condition(EtbCondition::HasCardType("Creature".to_owned())),
        EtbEffect::new(e(4), choose("k4", EtbChoiceKind::Color, vec![opt("d", 1)]))
            .with_source(o(2))
            .with_scope(EtbScope::ThisPermanent),
    ]);
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 3)].into();
    let out = drive(&ctx, &[entry(o(1), p(1)), entry(o(2), p(1))], &budget, &[]);
    assert!(out.asked.is_empty());
    let r = out.result.unwrap();
    assert_eq!(
        r[0].memory.choice("k1"),
        Some(&ChoiceValue::Color("one".to_owned()))
    );
    assert_eq!(
        r[0].memory.choice("k3"),
        Some(&ChoiceValue::Color("c".to_owned()))
    );
    assert_eq!(
        r[1].memory.choice("k4"),
        Some(&ChoiceValue::Color("d".to_owned()))
    );
}

/// REQ-EVENT-001, CR 614.12b (F8): a budget of u64::MAX with options {u64::MAX, u64::MAX-1} and a
/// later cost of 1 allows only u64::MAX-1 (no saturation).
#[test]
fn req_event_001_payable_no_saturation() {
    let (env, _) = payable_fixture();
    let fx = effects(vec![
        EtbEffect::new(
            e(1),
            choose(
                "a",
                EtbChoiceKind::Color,
                vec![opt("max", u64::MAX), opt("almost", u64::MAX - 1)],
            ),
        )
        .with_source(o(1))
        .with_scope(EtbScope::ThisPermanent),
        EtbEffect::new(e(2), choose("b", EtbChoiceKind::Color, vec![opt("one", 1)]))
            .with_source(o(2))
            .with_scope(EtbScope::ThisPermanent),
    ]);
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let budget: BTreeMap<PlayerId, u64> = [(p(1), u64::MAX)].into();
    let out = drive(&ctx, &[entry(o(1), p(1)), entry(o(2), p(1))], &budget, &[]);
    assert!(out.asked.is_empty());
    let r = out.result.unwrap();
    assert_eq!(
        r[0].memory.choice("a"),
        Some(&ChoiceValue::Color("almost".to_owned()))
    );
    assert_eq!(
        r[1].memory.choice("b"),
        Some(&ChoiceValue::Color("one".to_owned()))
    );
}

/// REQ-EVENT-001, CR 616.1e, CR 614.12b: the `CHOOSE_REPLACEMENT_EFFECT` candidates are not
/// filtered by payability: an unpayable choice is still offered, and picking it is not payable.
#[test]
fn req_event_001_order_choice_not_filtered() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let fx = effects(vec![
        EtbEffect::new(e(1), choose("a", EtbChoiceKind::Color, vec![opt("x", 2)]))
            .with_description("dear"),
        EtbEffect::new(e(2), choose("b", EtbChoiceKind::Color, vec![opt("y", 1)]))
            .with_description("cheap"),
    ]);
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 1)].into();
    let out = drive(&ctx, &[entry(o(1), p(1))], &budget, &[pick(0)]);
    assert_eq!(
        out.asked[0].payload,
        ChoicePayload::ChooseReplacementEffect {
            event: "ZONE_CHANGE".to_owned(),
            effects: vec!["dear".to_owned(), "cheap".to_owned()],
        }
    );
    assert!(procedure_message(out.result).contains("not payable"));
}

/// `n` independent `Other`-tier choices of cost 1 for any entering permanent.
fn many_choices(n: u64) -> EtbEffects {
    effects(
        (1..=n)
            .map(|i| {
                EtbEffect::new(
                    e(i),
                    choose(&format!("k{i}"), EtbChoiceKind::Color, vec![opt("x", 1)]),
                )
            })
            .collect(),
    )
}

/// REQ-EVENT-001, REQ-CORE-001, CR 614.12b (F5-F8): the lookahead visits at most
/// `MAX_LOOKAHEAD_STATES` (4096) states per entry: 13 independent choices (8192 states) are a
/// procedure error for a budgeted chooser, 12 (4096 subsets, the full set not memoised) are
/// not; an unlimited chooser runs no lookahead.
#[test]
fn req_event_001_lookahead_state_cap() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 100)].into();
    let en = [entry(o(1), p(1))];
    let fx = many_choices(13);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &en, &budget, &[]);
    assert!(out.asked.is_empty());
    assert!(procedure_message(out.result).contains("4096"));
    let out = drive(&ctx, &en, &no_limit(), &[]);
    assert_eq!(out.result, Err(ChoiceError::NotPending));
    assert_eq!(
        out.asked[0].choice_type,
        ChoiceType::ChooseReplacementEffect
    );
    let fx = many_choices(12);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &en, &budget, &[]);
    assert_eq!(out.result, Err(ChoiceError::NotPending));
    assert_eq!(out.asked.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// PR #202 review round 2 (G1-G5)
// ---------------------------------------------------------------------------------------------

/// An environment whose selectors mean "artifacts the effect's controller controls" (matched
/// through `MatchContext::effect_controller`); everything else is the inner [`Env`].
#[derive(Clone, Debug)]
struct YourArtifacts {
    inner: Env,
}

impl LayerEnvironment for YourArtifacts {
    fn objects(&self) -> Vec<ObjectId> {
        self.inner.objects()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.inner.base(id)
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.inner.counters(id)
    }

    fn matches(&self, _: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        ctx.candidate_view
            .characteristics
            .card_types
            .contains("Artifact")
            && ctx.candidate_view.controller == Some(ctx.effect_controller)
    }
}

/// REQ-EVENT-001, CR 707.5 (G1, thread 4647215310): a copy model controlled by player 1
/// declares the selector static "artifacts you control are creatures"; an entrant controlled by
/// player 2 that enters as the copy has the static as its own, so it applies with the entrant's
/// controller: the entrant projects as a creature and a creature-gated ETB effect applies. The
/// same-controller case is unchanged.
#[test]
fn req_event_001_copied_static_takes_entrant_controller() {
    let your_artifacts_are_creatures = ContinuousEffect {
        affected: Affected::Selector(ObjectSelectorAST::Ref(
            fineprint_dsl::expr::ReferenceAST::SelfRef,
        )),
        ..add_type(Some(o(77)), o(77), "Creature")
    };
    assert_eq!(your_artifacts_are_creatures.controller, p(1));
    let mut fx = effects(vec![
        copy_of(1, artifact("test.etb_model_artifact")),
        creature_tapped(2),
    ]);
    fx.set_copy_abilities(
        e(1),
        CopyAbilities {
            statics: vec![(e(400), your_artifacts_are_creatures)],
            etb: Vec::new(),
        },
    )
    .unwrap();
    let continuous = ContinuousEffects::new();
    for controller in [p(2), p(1)] {
        let mut inner = Env::default().with(o(1), artifact("test.etb_card_a"));
        inner.bases.get_mut(&o(1)).unwrap().controller = Some(controller);
        let env = YourArtifacts { inner };
        let ctx = EtbContext {
            env: &env,
            continuous: &continuous,
            effects: &fx,
        };
        let en = entry(o(1), controller);
        let after = project(&ctx, &en, &applied(&[1])).unwrap();
        assert_eq!(
            after.evaluated.characteristics.card_types,
            set(["Artifact", "Creature"]),
            "controller {controller:?}"
        );
        assert_eq!(applicable(&ctx, &en, &applied(&[1])), Ok(vec![e(2)]));
        let out = drive(&ctx, std::slice::from_ref(&en), &no_limit(), &[]);
        let r = &out.result.unwrap()[0];
        assert_eq!(r.projected.applied.order, vec![e(1), e(2)]);
        assert!(r.projected.tapped);
    }
}

/// REQ-EVENT-001, CR 613.1d, CR 208.3 (G2, thread 4647215321): the layer-4 type addition of an
/// applied `AddCardTypes` is not a static ability's effect, so the environment's
/// generating-ability hook is never consulted for it: with a hook answering false for every
/// effect, a printed-3/2 artifact still projects as a 3/2 artifact creature and a creature-gated
/// effect applies; with the hook true, the same.
#[test]
fn req_event_001_added_type_ignores_generating_ability_hook() {
    let env = Env::default().with(o(1), card("test.etb_artifact_3_2", "Artifact", 3, 2));
    let fx = effects(vec![
        EtbEffect::new(e(1), EtbAction::AddCardTypes(set(["Creature"]))),
        creature_tapped(2),
    ]);
    let continuous = ContinuousEffects::new();
    let never = Hooked {
        inner: env.clone(),
        exists: |_, _| false,
    };
    let always = Hooked {
        inner: env,
        exists: |_, _| true,
    };
    for layer_env in [&never as &dyn LayerEnvironment, &always] {
        let ctx = EtbContext {
            env: layer_env,
            continuous: &continuous,
            effects: &fx,
        };
        let en = entry(o(1), p(1));
        let c = project(&ctx, &en, &applied(&[1]))
            .unwrap()
            .evaluated
            .characteristics;
        assert_eq!(c.card_types, set(["Artifact", "Creature"]));
        assert_eq!(c.power.as_ref().and_then(literal_of), Some(3));
        assert_eq!(c.toughness.as_ref().and_then(literal_of), Some(2));
        assert_eq!(applicable(&ctx, &en, &applied(&[1])), Ok(vec![e(2)]));
        let out = drive(&ctx, std::slice::from_ref(&en), &no_limit(), &[]);
        let r = &out.result.unwrap()[0];
        assert_eq!(r.projected.applied.order, vec![e(1), e(2)]);
        assert!(r.projected.tapped);
    }
}

/// REQ-EVENT-001, CR 614.12b (G3, thread 4647215332): a `Choose` with no options is refused by
/// `add` (`EmptyChoice`, the set unchanged), so a later entrant can no longer carry a choice that
/// the lookahead would reserve nothing for; a single cost-0 option is accepted and payable with
/// a zero budget (not asked).
#[test]
fn req_event_001_empty_choice_rejected() {
    let mut fx = effects(vec![
        EtbEffect::new(
            e(1),
            choose(
                "a",
                EtbChoiceKind::Color,
                vec![opt("cheap_a", 1), opt("dear_a", 3)],
            ),
        )
        .with_source(o(1))
        .with_scope(EtbScope::ThisPermanent),
    ]);
    let before = fx.clone();
    let empty = EtbEffect::new(e(2), choose("b", EtbChoiceKind::Color, Vec::new()))
        .with_source(o(2))
        .with_scope(EtbScope::ThisPermanent);
    assert_eq!(fx.add(empty), Err(EtbError::EmptyChoice(e(2))));
    assert_eq!(fx, before);
    assert!(fx.get(e(2)).is_none());
    assert!(!EtbError::EmptyChoice(e(2)).to_string().is_empty());

    // A single cost-0 option is accepted and payable with nothing left.
    fx.add(
        EtbEffect::new(
            e(3),
            choose("c", EtbChoiceKind::Color, vec![opt("free_c", 0)]),
        )
        .with_source(o(2))
        .with_scope(EtbScope::ThisPermanent),
    )
    .unwrap();
    let env = Env::default()
        .with(o(1), artifact("test.etb_card_a"))
        .with(o(2), artifact("test.etb_card_b"));
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let entries = [entry(o(1), p(1)), entry(o(2), p(1))];
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 3)].into();
    let out = drive(&ctx, &entries, &budget, &[text("dear_a")]);
    assert_eq!(out.asked.len(), 1);
    assert_eq!(out.asked[0].payload, texts(&["cheap_a", "dear_a"]));
    let r = out.result.unwrap();
    assert_eq!(
        r[1].memory.choice("c"),
        Some(&ChoiceValue::Color("free_c".to_owned()))
    );
}

/// `k` chained type additions (effect `i` adds `T<i>` once the projection has `T<i-1>`; effect
/// 1 always applies), then a mandatory cost-1 choice gated on `T<k>`. From the start, the
/// reachable lookahead states of one entry are the `k + 1` prefixes of the chain plus the
/// state after the choice: `k + 2`.
fn type_chain(k: u64) -> EtbEffects {
    let mut list: Vec<EtbEffect> = (1..=k)
        .map(|i| {
            let fx = EtbEffect::new(e(i), EtbAction::AddCardTypes(set_of(&format!("T{i}"))));
            if i == 1 {
                fx
            } else {
                fx.with_condition(EtbCondition::HasCardType(format!("T{}", i - 1)))
            }
        })
        .collect();
    list.push(
        EtbEffect::new(
            e(k + 1),
            choose("end", EtbChoiceKind::Color, vec![opt("x", 1)]),
        )
        .with_condition(EtbCondition::HasCardType(format!("T{k}"))),
    );
    effects(list)
}

fn set_of(item: &str) -> BTreeSet<String> {
    [item.to_owned()].into_iter().collect()
}

/// REQ-EVENT-001, REQ-CORE-001, CR 614.12b (G4, thread 4647215338): states are charged when
/// entered, so a deep chain is bounded too: 4094 chained additions plus the choice reach exactly
/// 4096 states (resolves, the choice applied without a request), 4095 reach 4097 (the procedure
/// error, no request, no stack overflow in the normal test thread).
#[test]
fn req_event_001_lookahead_deep_chain_bounded() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 1)].into();
    let en = [entry(o(1), p(1))];

    let fx = type_chain(4095);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &en, &budget, &[]);
    assert!(out.asked.is_empty());
    let msg = procedure_message(out.result);
    assert!(msg.contains("lookahead"), "{msg}");
    assert!(msg.contains("exceeds 4096 states"), "{msg}");

    let fx = type_chain(4094);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &en, &budget, &[]);
    assert!(out.asked.is_empty());
    let r = out.result.unwrap();
    assert_eq!(r[0].projected.applied.effects.len(), 4095);
    assert_eq!(
        r[0].memory.choice("end"),
        Some(&ChoiceValue::Color("x".to_owned()))
    );
}

/// `n` independent free type additions and one independent cost-1 choice: `2^(n+1)` states,
/// each reachable through many orders.
fn free_adds_and_choice(n: u64) -> EtbEffects {
    let mut list: Vec<EtbEffect> = (1..=n)
        .map(|i| EtbEffect::new(e(i), EtbAction::AddCardTypes(set_of(&format!("T{i}")))))
        .collect();
    list.push(EtbEffect::new(
        e(n + 1),
        choose("end", EtbChoiceKind::Color, vec![opt("x", 1)]),
    ));
    effects(list)
}

/// REQ-EVENT-001, REQ-CORE-001, CR 614.12b, CR 616.1f (G4, thread 4647215338): memo hits are
/// free: 11 independent free additions and one choice reach exactly 4096 states (2^12) through
/// 1 + 12 * 2^11 = 24577 visits, and stay within the limit (the order request is issued); with
/// one more addition (8192 states) the limit is exceeded.
#[test]
fn req_event_001_lookahead_memo_hits_free() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 1)].into();
    let en = [entry(o(1), p(1))];
    let fx = free_adds_and_choice(11);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &en, &budget, &[]);
    assert_eq!(out.result, Err(ChoiceError::NotPending));
    assert_eq!(out.asked.len(), 1);
    assert_eq!(
        out.asked[0].choice_type,
        ChoiceType::ChooseReplacementEffect
    );
    let fx = free_adds_and_choice(12);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &en, &budget, &[]);
    assert!(out.asked.is_empty());
    let msg = procedure_message(out.result);
    assert!(msg.contains("lookahead") && msg.contains("exceeds 4096 states"));
}

/// REQ-EVENT-001, CR 614.12a (G5, thread 4647215344): every entry's starting projection is
/// checked before any reserve or request: an unknown second entrant, or one whose own static
/// duplicates an id in force, fails the call before the first entry's choice of two free values
/// is asked, with or without a budget (through a `ChoiceSession`: no request is ever pending).
/// Valid entries are unchanged.
#[test]
fn req_event_001_every_start_validated_before_requests() {
    let env = Env::default()
        .with(o(1), artifact("test.etb_card_a"))
        .with(o(2), artifact("test.etb_card_b"));
    let fx = effects(vec![
        EtbEffect::new(
            e(1),
            choose(
                "a",
                EtbChoiceKind::Color,
                vec![opt("free_a", 0), opt("free_b", 0)],
            ),
        )
        .with_source(o(1))
        .with_scope(EtbScope::ThisPermanent),
    ]);
    let mut continuous = ContinuousEffects::new();
    continuous
        .register(e(500), add_subtype(o(2), "Golem"))
        .unwrap();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let unknown = entry(o(9), p(1));
    let duplicate = EtbEntry {
        own_statics: vec![(e(500), add_subtype(o(2), "Wall"))],
        ..entry(o(2), p(1))
    };
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 5)].into();
    for (second, expected) in [
        (unknown, EtbError::Layer(LayerError::UnknownObject(o(9)))),
        (
            duplicate,
            EtbError::Layer(LayerError::DuplicateEffect(e(500))),
        ),
    ] {
        let entries = [entry(o(1), p(1)), second];
        for payable in [&budget, &no_limit()] {
            let out = drive(&ctx, &entries, payable, &[text("free_a")]);
            assert!(out.asked.is_empty(), "{expected:?} {payable:?}");
            assert_eq!(
                out.result,
                Err(ChoiceError::Procedure(expected.to_string()))
            );
        }
    }
    // Valid entries: the first choice is asked as before.
    let entries = [entry(o(1), p(1)), entry(o(2), p(1))];
    for payable in [&budget, &no_limit()] {
        let out = drive(&ctx, &entries, payable, &[text("free_b")]);
        assert_eq!(out.asked.len(), 1);
        assert_eq!(out.asked[0].payload, texts(&["free_a", "free_b"]));
        let r = out.result.unwrap();
        assert_eq!(
            r[0].memory.choice("a"),
            Some(&ChoiceValue::Color("free_b".to_owned()))
        );
    }
}

// ---------------------------------------------------------------------------------------------
// PR #202 review round 3 (H1-H3)
// ---------------------------------------------------------------------------------------------

/// REQ-EVENT-001, CR 614.12a, CR 614.12b (H1, thread 4648051968): a `Choose` whose options repeat
/// a text is refused by `add` (`DuplicateOptionText`, the set unchanged): the answer `Text(x)`
/// could not tell the two costs apart. The empty-options check comes first. Distinct texts, a
/// single free option and distinct texts of equal cost are accepted.
#[test]
fn req_event_001_duplicate_option_text_rejected() {
    let mut fx = effects(vec![EtbEffect::new(e(1), EtbAction::EntersTapped)]);
    let before = fx.clone();
    let dup = EtbEffect::new(
        e(2),
        choose(
            "a",
            EtbChoiceKind::Color,
            vec![opt("x", 2), opt("y", 0), opt("x", 1)],
        ),
    );
    let expected = EtbError::DuplicateOptionText {
        effect: e(2),
        text: "x".to_owned(),
    };
    assert_eq!(fx.add(dup), Err(expected.clone()));
    assert_eq!(fx, before);
    assert!(fx.get(e(2)).is_none());
    assert!(!expected.to_string().is_empty());
    // Empty options are still `EmptyChoice`.
    assert_eq!(
        fx.add(EtbEffect::new(
            e(2),
            choose("a", EtbChoiceKind::Color, Vec::new())
        )),
        Err(EtbError::EmptyChoice(e(2)))
    );
    // Accepted: distinct texts, one free option, distinct texts of equal cost.
    for (id, options) in [
        (3, vec![opt("x", 2), opt("y", 1)]),
        (4, vec![opt("z", 0)]),
        (5, vec![opt("u", 1), opt("v", 1)]),
    ] {
        fx.add(EtbEffect::new(
            e(id),
            choose(&format!("k{id}"), EtbChoiceKind::Color, options),
        ))
        .unwrap();
    }
    assert_eq!(fx.len(), 4);
}

/// The H2 entrant: two free values asked for any entering permanent, so a run that passed the
/// controller check would publish a request.
fn free_choice_fx() -> EtbEffects {
    effects(vec![EtbEffect::new(
        e(1),
        choose(
            "a",
            EtbChoiceKind::Color,
            vec![opt("free_a", 0), opt("free_b", 0)],
        ),
    )])
}

/// REQ-EVENT-001, CR 614.12, CR 707.5 (H2, outside-diff etb.rs:490): one controller per
/// projection. The environment reports player 1 for the entrant and the entry names player 2:
/// `project`, `applicable` and `resolve_entering` (before any request, budgeted or not) fail
/// with `ControllerMismatch`. The check precedes layer evaluation (an own static duplicating an
/// id in force still reports the mismatch); an unknown object keeps `UnknownObject`; equal
/// controllers and a `None` base controller are unchanged, and the projected controller is never
/// the entry's.
#[test]
fn req_event_001_controller_mismatch_rejected() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let fx = free_choice_fx();
    let mut continuous = ContinuousEffects::new();
    continuous
        .register(e(500), add_subtype(o(9), "Golem"))
        .unwrap();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let mismatch = EtbError::ControllerMismatch {
        object: o(1),
        entry: p(2),
        environment: p(1),
    };
    assert!(!mismatch.to_string().is_empty());
    let other = entry(o(1), p(2));
    assert_eq!(
        project(&ctx, &other, &EtbApplied::default()),
        Err(mismatch.clone())
    );
    assert_eq!(
        applicable(&ctx, &other, &EtbApplied::default()),
        Err(mismatch.clone())
    );
    let budget: BTreeMap<PlayerId, u64> = [(p(2), 5)].into();
    for payable in [&budget, &no_limit()] {
        let out = drive(
            &ctx,
            std::slice::from_ref(&other),
            payable,
            &[text("free_a")],
        );
        assert!(out.asked.is_empty(), "{payable:?}");
        assert_eq!(
            out.result,
            Err(ChoiceError::Procedure(mismatch.to_string()))
        );
        // A valid first entry is not asked either: every start is checked first.
        let both = [entry(o(1), p(1)), other.clone()];
        let out = drive(&ctx, &both, payable, &[text("free_a")]);
        assert!(out.asked.is_empty(), "{payable:?}");
        assert_eq!(
            out.result,
            Err(ChoiceError::Procedure(mismatch.to_string()))
        );
    }
    // Before layer evaluation: a duplicate own-static id reports the mismatch.
    let dup = EtbEntry {
        own_statics: vec![(e(500), add_subtype(o(1), "Wall"))],
        ..entry(o(1), p(2))
    };
    assert_eq!(
        project(&ctx, &dup, &EtbApplied::default()),
        Err(mismatch.clone())
    );
    // An unknown object keeps `UnknownObject`.
    assert_eq!(
        project(&ctx, &entry(o(7), p(2)), &EtbApplied::default()),
        Err(EtbError::Layer(LayerError::UnknownObject(o(7))))
    );
    // Same controller: unchanged.
    let same = entry(o(1), p(1));
    let projected = project(&ctx, &same, &EtbApplied::default()).unwrap();
    assert_eq!(projected.evaluated.controller, Some(p(1)));
    let out = drive(
        &ctx,
        std::slice::from_ref(&same),
        &no_limit(),
        &[text("free_b")],
    );
    assert_eq!(out.asked.len(), 1);
    assert_eq!(
        out.result.unwrap()[0].memory.choice("a"),
        Some(&ChoiceValue::Color("free_b".to_owned()))
    );
    // No controller known: accepted, the projected controller stays `None`.
    let mut unknown_controller = env.clone();
    unknown_controller.bases.get_mut(&o(1)).unwrap().controller = None;
    let ctx = EtbContext {
        env: &unknown_controller,
        continuous: &continuous,
        effects: &fx,
    };
    let projected = project(&ctx, &other, &EtbApplied::default()).unwrap();
    assert_eq!(projected.evaluated.controller, None);
    assert_eq!(
        applicable(&ctx, &other, &EtbApplied::default()),
        Ok(vec![e(1)])
    );
    let out = drive(
        &ctx,
        std::slice::from_ref(&other),
        &budget,
        &[text("free_a")],
    );
    assert_eq!(out.asked.len(), 1);
    assert_eq!(out.asked[0].chooser, p(2));
    assert!(out.result.is_ok());
}

/// REQ-EVENT-001, CR 707.5, CR 614.12 (H2): with a copied "artifacts you control are creatures"
/// static, an environment reporting player 1 and an entry naming player 2 is a mismatch from
/// `project`, `applicable` and `resolve_entering` (no request); with player 2 on both sides the
/// copied static matches its own entrant as before.
#[test]
fn req_event_001_controller_mismatch_with_copied_static() {
    let your_artifacts_are_creatures = ContinuousEffect {
        affected: Affected::Selector(ObjectSelectorAST::Ref(
            fineprint_dsl::expr::ReferenceAST::SelfRef,
        )),
        ..add_type(Some(o(77)), o(77), "Creature")
    };
    let mut fx = effects(vec![
        copy_of(1, artifact("test.etb_model_artifact")),
        creature_tapped(2),
    ]);
    fx.set_copy_abilities(
        e(1),
        CopyAbilities {
            statics: vec![(e(400), your_artifacts_are_creatures)],
            etb: Vec::new(),
        },
    )
    .unwrap();
    let continuous = ContinuousEffects::new();
    let env = YourArtifacts {
        inner: Env::default().with(o(1), artifact("test.etb_card_a")),
    };
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = entry(o(1), p(2));
    let mismatch = EtbError::ControllerMismatch {
        object: o(1),
        entry: p(2),
        environment: p(1),
    };
    assert_eq!(project(&ctx, &en, &applied(&[1])), Err(mismatch.clone()));
    assert_eq!(applicable(&ctx, &en, &applied(&[1])), Err(mismatch.clone()));
    let budget: BTreeMap<PlayerId, u64> = [(p(2), 3)].into();
    for payable in [&budget, &no_limit()] {
        let out = drive(&ctx, std::slice::from_ref(&en), payable, &[]);
        assert!(out.asked.is_empty());
        assert_eq!(
            out.result,
            Err(ChoiceError::Procedure(mismatch.to_string()))
        );
    }
    // Player 2 on both sides: the copied static applies to its own entrant.
    let mut inner = Env::default().with(o(1), artifact("test.etb_card_a"));
    inner.bases.get_mut(&o(1)).unwrap().controller = Some(p(2));
    let env = YourArtifacts { inner };
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let after = project(&ctx, &en, &applied(&[1])).unwrap();
    assert_eq!(
        after.evaluated.characteristics.card_types,
        set(["Artifact", "Creature"])
    );
    assert_eq!(applicable(&ctx, &en, &applied(&[1])), Ok(vec![e(2)]));
}

/// The H3 effects: effect 1, a cost-1 choice that applies only while the entrant is not a
/// creature; effect 2, a free "enters as an additional creature"; with `third`, effect 3, a
/// second cost-1 choice gated like effect 1. All of one tier, ids ascending.
fn paid_first(third: bool) -> EtbEffects {
    let mut list = vec![
        EtbEffect::new(e(1), choose("a", EtbChoiceKind::Color, vec![opt("x", 1)]))
            .with_condition(EtbCondition::LacksCardType("Creature".to_owned()))
            .with_description("paid"),
        EtbEffect::new(e(2), EtbAction::AddCardTypes(set(["Creature"])))
            .with_description("free creature"),
    ];
    if third {
        list.push(
            EtbEffect::new(e(3), choose("c", EtbChoiceKind::Color, vec![opt("z", 1)]))
                .with_condition(EtbCondition::LacksCardType("Creature".to_owned()))
                .with_description("paid too"),
        );
    }
    effects(list)
}

/// REQ-EVENT-001, CR 616.1e, CR 614.12b (H3, outside-diff etb.rs:1135): with a budget of 0, the
/// paid choice sorts first but only the free creature addition has a payable continuation, so
/// the `CHOOSE_REPLACEMENT_EFFECT` default is index 1; every candidate is still offered. Answering
/// the default completes the entry as a creature with no choice made; answering index 0 is not
/// payable. With an unlimited chooser the default stays 0.
#[test]
fn req_event_001_default_is_first_payable_candidate() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let fx = paid_first(false);
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = [entry(o(1), p(1))];
    let zero: BTreeMap<PlayerId, u64> = [(p(1), 0)].into();
    let out = drive(&ctx, &en, &zero, &[]);
    assert_eq!(out.asked.len(), 1);
    let request = &out.asked[0];
    assert_eq!(request.choice_type, ChoiceType::ChooseReplacementEffect);
    assert_eq!(request.default_on_timeout, pick(1));
    assert_eq!(
        request.options,
        ChoiceOptions::Enumerated(vec![ChoiceOption::Number(0), ChoiceOption::Number(1)])
    );
    assert_eq!(
        request.payload,
        ChoicePayload::ChooseReplacementEffect {
            event: "ZONE_CHANGE".to_owned(),
            effects: vec!["paid".to_owned(), "free creature".to_owned()],
        }
    );
    // Answering the default completes the entry.
    let out = drive(&ctx, &en, &zero, &[pick(1)]);
    assert_eq!(out.asked.len(), 1);
    let r = &out.result.unwrap()[0];
    assert_eq!(r.projected.applied.order, vec![e(2)]);
    assert!(r.projected.applied.chosen.is_empty());
    assert_eq!(
        r.projected.evaluated.characteristics.card_types,
        set(["Artifact", "Creature"])
    );
    // A manual answer of the unpayable index is still refused.
    let out = drive(&ctx, &en, &zero, &[pick(0)]);
    assert!(procedure_message(out.result).contains("not payable"));
    // Unlimited chooser: default 0 (and picking it asks nothing more: the choice has one option).
    let out = drive(&ctx, &en, &no_limit(), &[]);
    assert_eq!(out.asked[0].default_on_timeout, pick(0));
    assert_eq!(out.asked[0].payload, request.payload);
    // A budget of 1: candidate 0 is payable, default 0.
    let one: BTreeMap<PlayerId, u64> = [(p(1), 1)].into();
    let out = drive(&ctx, &en, &one, &[]);
    assert_eq!(out.asked[0].default_on_timeout, pick(0));
}

/// REQ-EVENT-001, CR 616.1e, CR 614.12b (H3): the session's `submit_default` (a timeout) on the
/// paid-first request with a budget of 0 completes the entry.
#[test]
fn req_event_001_default_submit_completes_entry() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let fx = paid_first(false);
    let continuous = ContinuousEffects::new();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = [entry(o(1), p(1))];
    let zero: BTreeMap<PlayerId, u64> = [(p(1), 0)].into();
    let proc = |_s: &mut Nil, c: &mut ChoiceContext<'_>| resolve_entering(&ctx, &en, &zero, c);
    let mut session = ChoiceSession::start(Nil, proc).unwrap();
    let id = session.pending().unwrap().request_id;
    session.submit_default(id).unwrap();
    let Status::Done { output, .. } = session.status() else {
        panic!("the default did not complete the entry");
    };
    assert_eq!(output[0].projected.applied.order, vec![e(2)]);
    assert!(output[0].projected.applied.chosen.is_empty());
    assert_eq!(session.answers(), &[pick(1)]);
}

/// REQ-EVENT-001, CR 616.1e, CR 614.12b (H3): of three candidates only the second has a payable
/// continuation (budget 0): the default is 1 and all three are offered; with every candidate
/// unpayable (two cost-1 choices, budget 0) the default stays 0; with a budget of 1 and the
/// three candidates, candidate 0 is payable (default 0).
#[test]
fn req_event_001_default_payable_three_and_none() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let continuous = ContinuousEffects::new();
    let en = [entry(o(1), p(1))];
    let zero: BTreeMap<PlayerId, u64> = [(p(1), 0)].into();
    let fx = paid_first(true);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &en, &zero, &[]);
    assert_eq!(out.asked[0].default_on_timeout, pick(1));
    assert_eq!(
        out.asked[0].payload,
        ChoicePayload::ChooseReplacementEffect {
            event: "ZONE_CHANGE".to_owned(),
            effects: vec![
                "paid".to_owned(),
                "free creature".to_owned(),
                "paid too".to_owned()
            ],
        }
    );
    for unpayable in [0, 2] {
        let out = drive(&ctx, &en, &zero, &[pick(unpayable)]);
        assert!(procedure_message(out.result).contains("not payable"));
    }
    let out = drive(&ctx, &en, &zero, &[pick(1)]);
    assert_eq!(out.result.unwrap()[0].projected.applied.order, vec![e(2)]);
    let one: BTreeMap<PlayerId, u64> = [(p(1), 1)].into();
    let out = drive(&ctx, &en, &one, &[]);
    assert_eq!(out.asked[0].default_on_timeout, pick(0));
    // Every candidate unpayable: default 0.
    let fx = effects(vec![
        EtbEffect::new(e(1), choose("a", EtbChoiceKind::Color, vec![opt("x", 1)])),
        EtbEffect::new(e(2), choose("b", EtbChoiceKind::Color, vec![opt("y", 1)])),
    ]);
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let out = drive(&ctx, &en, &zero, &[]);
    assert_eq!(out.asked.len(), 1);
    assert_eq!(out.asked[0].default_on_timeout, pick(0));
}

// ---------------------------------------------------------------------------------------------
// Issue #204 (FP-153): the timeout default avoids a failing continuation
// ---------------------------------------------------------------------------------------------

/// A continuous-effect registry already holding the id `e(400)`.
fn in_force_400() -> ContinuousEffects {
    let mut in_force = ContinuousEffects::new();
    in_force
        .register(e(400), add_subtype(o(1), "Wall"))
        .unwrap();
    in_force
}

/// Declares for the copy effect `copy` a static whose id `e(400)` is already in force: applying
/// it as the last copy fails the re-projection with `LayerError::DuplicateEffect`.
fn declare_collision(fx: &mut EtbEffects, copy: u64) {
    fx.set_copy_abilities(
        e(copy),
        CopyAbilities {
            statics: vec![(e(400), add_type(Some(o(77)), o(77), "Creature"))],
            etb: Vec::new(),
        },
    )
    .unwrap();
}

/// A copy of an artifact model that applies while the entrant is not a creature.
fn artifact_copy(id: u64, desc: &str) -> EtbEffect {
    copy_of(id, artifact("test.etb_model_artifact"))
        .with_condition(EtbCondition::LacksCardType("Creature".to_owned()))
        .with_description(desc)
}

/// A copy of a creature model (after it applies, the artifact copies no longer apply).
fn creature_copy(id: u64, desc: &str) -> EtbEffect {
    copy_of(id, card("test.etb_model_creature", "Creature", 2, 2)).with_description(desc)
}

/// REQ-EVENT-001, CR 616.1e, CR 614.12b, CR 616.1f (issue #204): the first copy declares a static
/// whose id is already in force, so applying it fails; the second copy completes. The default is
/// index 1, both candidates are offered, and a timeout completes the entry with copy 2 applied,
/// for a chooser with a budget and for one without.
#[test]
fn req_event_001_default_skips_colliding_copy() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let mut fx = effects(vec![
        artifact_copy(1, "colliding"),
        creature_copy(2, "valid"),
    ]);
    declare_collision(&mut fx, 1);
    let continuous = in_force_400();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = [entry(o(1), p(1))];
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 5)].into();
    for payable in [budget, no_limit()] {
        let out = drive(&ctx, &en, &payable, &[]);
        assert_eq!(out.asked.len(), 1);
        let request = &out.asked[0];
        assert_eq!(request.default_on_timeout, pick(1));
        assert_eq!(
            request.options,
            ChoiceOptions::Enumerated(vec![ChoiceOption::Number(0), ChoiceOption::Number(1)])
        );
        assert_eq!(
            request.payload,
            ChoicePayload::ChooseReplacementEffect {
                event: "ZONE_CHANGE".to_owned(),
                effects: vec!["colliding".to_owned(), "valid".to_owned()],
            }
        );
        // submit_default (a timeout) completes the entry with copy 2 applied.
        let proc =
            |_s: &mut Nil, c: &mut ChoiceContext<'_>| resolve_entering(&ctx, &en, &payable, c);
        let mut session = ChoiceSession::start(Nil, proc).unwrap();
        let id = session.pending().unwrap().request_id;
        session.submit_default(id).unwrap();
        let Status::Done { output, .. } = session.status() else {
            panic!("the default did not complete the entry");
        };
        assert_eq!(output[0].projected.applied.order, vec![e(2)]);
        // A manual answer of the colliding candidate is still the projection error.
        let out = drive(&ctx, &en, &payable, &[pick(0)]);
        assert!(procedure_message(out.result).contains("400"));
    }
}

/// REQ-EVENT-001, CR 616.1e (issue #204): a valid first copy stays the default (index 0).
#[test]
fn req_event_001_default_valid_first_copy_stays() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let mut fx = effects(vec![artifact_copy(1, "first"), creature_copy(2, "second")]);
    declare_collision(&mut fx, 2);
    let continuous = in_force_400();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = [entry(o(1), p(1))];
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 5)].into();
    for payable in [budget, no_limit()] {
        let out = drive(&ctx, &en, &payable, &[]);
        assert_eq!(out.asked[0].default_on_timeout, pick(0));
    }
}

/// REQ-EVENT-001, CR 616.1e (issue #204): when every candidate fails the default is 0, the whole
/// list is offered, and a manual answer ends in the projection error.
#[test]
fn req_event_001_default_all_candidates_fail() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let mut fx = effects(vec![
        copy_of(1, artifact("test.etb_model_artifact")),
        copy_of(2, card("test.etb_model_creature", "Creature", 2, 2)),
    ]);
    declare_collision(&mut fx, 1);
    declare_collision(&mut fx, 2);
    let continuous = in_force_400();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = [entry(o(1), p(1))];
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 5)].into();
    for payable in [budget, no_limit()] {
        let out = drive(&ctx, &en, &payable, &[]);
        assert_eq!(out.asked[0].default_on_timeout, pick(0));
        assert_eq!(
            out.asked[0].options,
            ChoiceOptions::Enumerated(vec![ChoiceOption::Number(0), ChoiceOption::Number(1)])
        );
        for index in [0, 1] {
            let out = drive(&ctx, &en, &payable, &[pick(index)]);
            assert!(procedure_message(out.result).contains("400"));
        }
    }
}

/// REQ-EVENT-001, CR 616.1e, CR 616.1f (issue #204): the collision appears only after a second
/// effect is applied (the first adds the type that makes the colliding copy applicable); the
/// default is the candidate with another completing order.
#[test]
fn req_event_001_default_skips_deep_failing_continuation() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let mut fx = effects(vec![
        EtbEffect::new(e(1), EtbAction::AddCardTypes(set(["Widget"])))
            .with_condition(EtbCondition::LacksCardType("Creature".to_owned()))
            .with_description("enables"),
        EtbEffect::new(e(3), EtbAction::AddCardTypes(set(["Creature"]))).with_description("blocks"),
        copy_of(2, artifact("test.etb_model_artifact"))
            .with_condition(EtbCondition::HasCardType("Widget".to_owned())),
    ]);
    declare_collision(&mut fx, 2);
    let continuous = in_force_400();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = [entry(o(1), p(1))];
    let budget: BTreeMap<PlayerId, u64> = [(p(1), 5)].into();
    for payable in [budget, no_limit()] {
        let out = drive(&ctx, &en, &payable, &[]);
        assert_eq!(out.asked.len(), 1);
        assert_eq!(out.asked[0].default_on_timeout, pick(1));
        let out = drive(&ctx, &en, &payable, &[pick(1)]);
        assert_eq!(out.result.unwrap()[0].projected.applied.order, vec![e(3)]);
        let out = drive(&ctx, &en, &payable, &[pick(0)]);
        assert!(procedure_message(out.result).contains("400"));
    }
}

/// REQ-EVENT-001, REQ-CORE-001, CR 616.1e (issue #204): a completion search over its own bound
/// of 4096 states raises no error: 13 independent additions (8192 states) all end in a colliding
/// copy, so the search gives up and every candidate stays eligible (default 0); the request is
/// still published and the timeout ends in the projection error, not a lookahead error.
#[test]
fn req_event_001_completion_bound_is_best_effort() {
    let env = Env::default().with(o(1), artifact("test.etb_card_a"));
    let mut list: Vec<EtbEffect> = (1..=13)
        .map(|i| EtbEffect::new(e(i), EtbAction::AddCardTypes(set_of(&format!("T{i}")))))
        .collect();
    list.push(
        copy_of(30, artifact("test.etb_model_artifact"))
            .with_condition(EtbCondition::HasCardType("T13".to_owned())),
    );
    let mut fx = effects(list);
    declare_collision(&mut fx, 30);
    let continuous = in_force_400();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = [entry(o(1), p(1))];
    let out = drive(&ctx, &en, &no_limit(), &[]);
    assert_eq!(out.asked.len(), 1);
    assert_eq!(out.asked[0].default_on_timeout, pick(0));
    let ChoiceOptions::Enumerated(options) = &out.asked[0].options else {
        panic!("not enumerated");
    };
    assert_eq!(options.len(), 13);
    let answers = vec![pick(0); 13];
    let out = drive(&ctx, &en, &no_limit(), &answers);
    let msg = procedure_message(out.result);
    assert!(msg.contains("400") && !msg.contains("exceeds"), "{msg}");
}

/// REQ-EVENT-001, CR 616.1e, CR 614.12b (issue #204): with a costed `Choose` in the set, the cost
/// lookahead skips the infeasible branch of a colliding candidate instead of failing the entry
/// up front: candidate 0 completes but its continuation costs 1 (unpayable at budget 0),
/// candidate 1 collides, candidate 2 is a free valid copy. All three are offered, the default is
/// 2 (budget 0 and 5: with 5, candidate 0 is payable, default 0), and a manual answer of the
/// colliding candidate still ends in the projection error.
#[test]
fn req_event_001_default_costed_choose_and_colliding_candidate() {
    let env = Env::default().with(o(1), card("test.etb_card_a", "Enchantment", 0, 0));
    let ench = |c: EtbEffect| c.with_condition(EtbCondition::HasCardType("Enchantment".to_owned()));
    let mut fx = effects(vec![
        ench(copy_of(1, artifact("test.etb_model_artifact"))).with_description("costed"),
        ench(copy_of(2, artifact("test.etb_model_artifact_b"))).with_description("colliding"),
        ench(copy_of(
            3,
            card("test.etb_model_creature", "Creature", 2, 2),
        ))
        .with_description("valid"),
        EtbEffect::new(e(4), choose("a", EtbChoiceKind::Color, vec![opt("x", 1)]))
            .with_condition(EtbCondition::HasCardType("Artifact".to_owned())),
    ]);
    declare_collision(&mut fx, 2);
    let continuous = in_force_400();
    let ctx = EtbContext {
        env: &env,
        continuous: &continuous,
        effects: &fx,
    };
    let en = [entry(o(1), p(1))];
    for (budget, default) in [(0, 2), (5, 0)] {
        let payable: BTreeMap<PlayerId, u64> = [(p(1), budget)].into();
        let out = drive(&ctx, &en, &payable, &[]);
        assert_eq!(out.asked.len(), 1);
        assert_eq!(out.asked[0].default_on_timeout, pick(default));
        let ChoiceOptions::Enumerated(options) = &out.asked[0].options else {
            panic!("not enumerated");
        };
        assert_eq!(options.len(), 3);
        let out = drive(&ctx, &en, &payable, &[pick(1)]);
        assert!(procedure_message(out.result).contains("400"));
    }
}
