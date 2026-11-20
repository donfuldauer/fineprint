// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The ETB look-ahead of SRS 8.1 (REQ-EVENT-001, CR 614.12): the projected permanent, the
//! CR 616.1 select-apply-repeat loop over ETB-modifying replacement effects, "as this enters,
//! choose ..." choices made before the permanent enters (CR 614.12a) and the payable total of
//! simultaneous choices (CR 614.12b).
//!
//! **Projection** ([`project`]). The permanent as it would exist on the battlefield (CR 614.12):
//! the caller's base of the entering object, always presented as on the battlefield; the base
//! characteristics replaced by every applied [`EtbAction::EntersAsCopyOf`] (ascending
//! [`EffectId`]); the layers evaluated with the continuous effects already in force plus the
//! permanent's own static abilities ([`EtbEntry::own_statics`], registered on a clone); then
//! every applied [`EtbAction::AddCardTypes`]. Once a copy effect has been applied, the
//! permanent's own statics and its own [`EtbScope::ThisPermanent`] effects no longer count: its
//! abilities are the copy's. The three CR 614.12 inputs have no order in the CR; the order used
//! is docs/DECISIONS.md "#40 FP-039 — ETB projection API and modelling choices".
//!
//! **Loop** ([`resolve_entering`]). Per entry, in slice order: the applicable effects
//! ([`applicable`]) of the current projection; only the highest-priority [`Tier`] present is
//! choosable (CR 616.1a-e); one is applied without asking, several are offered through
//! `CHOOSE_REPLACEMENT_EFFECT`; the projection is rebuilt after every application, so an effect
//! made applicable by an earlier one is applied too (CR 616.1f, CR 616.2). An effect never
//! applies twice to one entry.
//!
//! **Choices** (CR 614.12a, CR 614.12c, CR 607.2d, CR 607.2f). A [`EtbAction::Choose`] asks
//! `CHOOSE_OPTION_OR_VALUE` of the entry's controller before anything moves; the answer is
//! carried in [`EtbApplied::chosen`] and in the entry's [`Memory`].
//!
//! **Payability** (CR 614.12b). Each option commits a cost to its chooser; an option is offered
//! only if its cost plus the cheapest cost of every other undecided choice of that chooser fits
//! the remaining budget.
//!
//! **Standalone scope.** Not joined to the bus or `GameState`: the caller supplies every input
//! and applies the result. Determinism (REQ-CORE-001): ordered containers, integer arithmetic,
//! no clock.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::card::ValueExpressionAST;
use fineprint_dsl::expr::ObjectSelectorAST;

use super::{BusEventKind, Tier};
use crate::choice::{
    AllowedValues, ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload,
    ChoiceReason, ChoiceResponse, ChoiceType, Interrupt, PendingChoiceRequest,
};
use crate::layers::{
    ContinuousEffect, ContinuousEffects, Evaluated, LayerEnvironment, LayerError,
    ManaValueProvenance, MatchContext, ObjectBase, evaluate,
};
use crate::memory::{ChoiceValue, Memory, MemoryTable};
use crate::objects::Characteristics;

/// Which entering permanents an ETB effect can apply to (CR 614.12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtbScope {
    /// Only the permanent that is the effect's `source` (it affects only that permanent).
    ThisPermanent,
    /// Any entering permanent except its own `source` (a general subset that includes the source
    /// does not apply to the source as it enters: the "Permanents enter tapped" example).
    OtherPermanents,
}

/// A test of the PROJECTED permanent (its characteristics as they would be on the battlefield,
/// CR 614.12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EtbCondition {
    /// Always true.
    Always,
    /// The projected permanent has this card type.
    HasCardType(String),
    /// The projected permanent does not have this card type.
    LacksCardType(String),
    /// The projected permanent has this subtype.
    HasSubtype(String),
}

/// What kind of value a [`EtbAction::Choose`] asks for; fixes the request's `option_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtbChoiceKind {
    /// `option_type` `"COLOR"`, stored as [`ChoiceValue::Color`] (CR 614.12a).
    Color,
    /// `option_type` `"CREATURE_TYPE"`, stored as [`ChoiceValue::CreatureType`] (CR 614.12a).
    CreatureType,
    /// `option_type` `"ANCHOR_WORD"`, stored as [`ChoiceValue::Anchor`] (CR 614.12c).
    Anchor,
}

impl EtbChoiceKind {
    /// The request's `option_type`.
    fn option_type(self) -> &'static str {
        match self {
            EtbChoiceKind::Color => "COLOR",
            EtbChoiceKind::CreatureType => "CREATURE_TYPE",
            EtbChoiceKind::Anchor => "ANCHOR_WORD",
        }
    }

    /// The rule cited in the request's reason.
    fn cr(self) -> &'static str {
        match self {
            // CR 614.12a
            EtbChoiceKind::Color | EtbChoiceKind::CreatureType => "CR 614.12a",
            // CR 614.12c
            EtbChoiceKind::Anchor => "CR 614.12c",
        }
    }

    /// The Memory value of the chosen `text`.
    fn value(self, text: &str) -> ChoiceValue {
        match self {
            EtbChoiceKind::Color => ChoiceValue::Color(text.to_owned()),
            EtbChoiceKind::CreatureType => ChoiceValue::CreatureType(text.to_owned()),
            EtbChoiceKind::Anchor => ChoiceValue::Anchor(text.to_owned()),
        }
    }
}

/// One allowed value of a choice and the cost it commits (CR 614.12b); units are abstract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EtbOption {
    /// The value offered (and stored when chosen).
    pub text: String,
    /// The cost choosing it commits to the chooser.
    pub cost: u64,
}

/// What an ETB effect does to the entering permanent (CR 614.1c, CR 614.1d).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EtbAction {
    /// "enters tapped".
    EntersTapped,
    /// "enters with N counters of kind".
    EntersWithCounters {
        /// The counter kind.
        kind: String,
        /// How many.
        amount: i64,
    },
    /// "enters as a copy": the entering permanent's base characteristics are replaced by these
    /// copiable values (applied to the base, not as a layer effect).
    EntersAsCopyOf(Box<Characteristics>),
    /// "enters as an additional <types>": these card types are added to the projected
    /// permanent's card types AFTER the layers of the continuous effects have been evaluated.
    AddCardTypes(BTreeSet<String>),
    /// "As this enters, choose ...": asks the entry's controller (CR 614.12a) and stores the
    /// answer under `key`.
    Choose {
        /// The Memory key the answer is stored under.
        key: String,
        /// What is chosen.
        kind: EtbChoiceKind,
        /// The values, in the order offered, with their costs (CR 614.12b).
        options: Vec<EtbOption>,
    },
}

/// One ETB-modifying replacement effect in force (CR 614.1c, CR 614.1d, CR 614.12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EtbEffect {
    /// The effect's id (its iteration key).
    pub id: EffectId,
    /// The object whose ability generates it, if any.
    pub source: Option<ObjectId>,
    /// Offered to the chooser in the `CHOOSE_REPLACEMENT_EFFECT` request.
    pub description: String,
    /// Its precedence tier (CR 616.1a-e).
    pub tier: Tier,
    /// Which entering permanents it can apply to.
    pub scope: EtbScope,
    /// The test of the projected permanent.
    pub condition: EtbCondition,
    /// What it does.
    pub action: EtbAction,
}

impl EtbEffect {
    /// An effect of tier [`Tier::Other`], scope [`EtbScope::OtherPermanents`], condition
    /// [`EtbCondition::Always`], no source and description `"etb effect <id>"`.
    #[must_use]
    pub fn new(id: EffectId, action: EtbAction) -> EtbEffect {
        EtbEffect {
            id,
            source: None,
            description: format!("etb effect {}", id.get()),
            tier: Tier::Other,
            scope: EtbScope::OtherPermanents,
            condition: EtbCondition::Always,
            action,
        }
    }

    /// Sets the source.
    #[must_use]
    pub fn with_source(mut self, source: ObjectId) -> Self {
        self.source = Some(source);
        self
    }

    /// Sets the tier.
    #[must_use]
    pub fn with_tier(mut self, tier: Tier) -> Self {
        self.tier = tier;
        self
    }

    /// Sets the scope.
    #[must_use]
    pub fn with_scope(mut self, scope: EtbScope) -> Self {
        self.scope = scope;
        self
    }

    /// Sets the condition.
    #[must_use]
    pub fn with_condition(mut self, condition: EtbCondition) -> Self {
        self.condition = condition;
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}

/// The ETB effects in force, keyed and iterated by ascending [`EffectId`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EtbEffects {
    effects: BTreeMap<EffectId, EtbEffect>,
}

impl EtbEffects {
    /// No effects.
    #[must_use]
    pub fn new() -> EtbEffects {
        EtbEffects::default()
    }

    /// Adds `effect` under its id.
    ///
    /// # Errors
    /// [`EtbError::DuplicateEffect`] if an effect with that id is present (nothing changes).
    pub fn add(&mut self, effect: EtbEffect) -> Result<(), EtbError> {
        if self.effects.contains_key(&effect.id) {
            return Err(EtbError::DuplicateEffect(effect.id));
        }
        self.effects.insert(effect.id, effect);
        Ok(())
    }

    /// Removes and returns the effect `id`, if present.
    pub fn remove(&mut self, id: EffectId) -> Option<EtbEffect> {
        self.effects.remove(&id)
    }

    /// The effect `id`, if present.
    #[must_use]
    pub fn get(&self, id: EffectId) -> Option<&EtbEffect> {
        self.effects.get(&id)
    }

    /// The number of effects.
    #[must_use]
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Whether there are no effects.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

/// Errors of this module (not choice errors).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EtbError {
    /// An effect with this id is already present.
    DuplicateEffect(EffectId),
    /// The layer system failed (an unknown object, a duplicate own-static id, ...).
    Layer(LayerError),
    /// A choice has no option whose cost leaves the combined costs payable (CR 614.12b).
    Unpayable {
        /// The player making the choice.
        chooser: PlayerId,
        /// The choice's key.
        key: String,
    },
}

impl fmt::Display for EtbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EtbError::DuplicateEffect(id) => {
                write!(f, "ETB effect {} is already present", id.get())
            }
            EtbError::Layer(e) => write!(f, "layer error: {e}"),
            EtbError::Unpayable { chooser, key } => write!(
                f,
                "choice {key:?} of player {}: no option leaves the combined costs payable \
                 (not payable, CR 614.12b)",
                chooser.get()
            ),
        }
    }
}

impl std::error::Error for EtbError {}

impl From<LayerError> for EtbError {
    fn from(e: LayerError) -> EtbError {
        EtbError::Layer(e)
    }
}

/// Everything the projection reads (all borrowed, all supplied by the caller).
pub struct EtbContext<'a> {
    /// The game as the layer system reads it. It must contain every entering object; the
    /// projection always treats an entering object as on the battlefield, whatever
    /// `base(..).on_battlefield` says (the environment is wrapped, never mutated).
    pub env: &'a dyn LayerEnvironment,
    /// Continuous effects already in force (CR 614.12, third input).
    pub continuous: &'a ContinuousEffects,
    /// The ETB replacement effects in force.
    pub effects: &'a EtbEffects,
}

/// One permanent about to enter.
#[derive(Clone, Debug)]
pub struct EtbEntry {
    /// The entering object.
    pub object: ObjectId,
    /// Chooser of every choice and of the CR 616.1 order for this entry (the controller it
    /// will enter under).
    pub controller: PlayerId,
    /// Continuous effects from the permanent's own static abilities that apply once it is on
    /// the battlefield (CR 614.12, second input), registered temporarily on a clone of
    /// [`EtbContext::continuous`] (a duplicate id is
    /// `EtbError::Layer(LayerError::DuplicateEffect(id))`).
    pub own_statics: Vec<(EffectId, ContinuousEffect)>,
}

/// What has already modified this entry (CR 614.12, first input; CR 616.1f).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EtbApplied {
    /// The applied effects.
    pub effects: BTreeSet<EffectId>,
    /// Chosen values by key (the keys of applied [`EtbAction::Choose`] actions).
    pub chosen: BTreeMap<String, ChoiceValue>,
}

/// The permanent as it would exist on the battlefield (CR 614.12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedPermanent {
    /// The entering object.
    pub object: ObjectId,
    /// Characteristics and controller after the base was modified by applied copy effects, the
    /// layers were evaluated with the own statics and continuous effects in force (always as
    /// an on-battlefield object), and applied [`EtbAction::AddCardTypes`] were added.
    pub evaluated: Evaluated,
    /// Whether an applied effect makes it enter tapped.
    pub tapped: bool,
    /// The counters applied effects make it enter with (kind to total).
    pub counters: BTreeMap<String, i64>,
    /// What has modified the entry.
    pub applied: EtbApplied,
}

/// The environment seen by the projection: the caller's, with the entering object on the
/// battlefield and its base replaced by an applied copy (CR 614.12).
struct Projected<'a> {
    inner: &'a dyn LayerEnvironment,
    object: ObjectId,
    copy: Option<&'a Characteristics>,
}

impl LayerEnvironment for Projected<'_> {
    fn objects(&self) -> Vec<ObjectId> {
        self.inner.objects()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        let mut base = self.inner.base(id)?;
        if id == self.object {
            // CR 614.12: as it would exist on the battlefield.
            base.on_battlefield = true;
            if let Some(copy) = self.copy {
                base.characteristics = copy.clone();
            }
        }
        Some(base)
    }

    fn base_mana_value_provenance(&self, id: ObjectId) -> ManaValueProvenance {
        if id == self.object && self.copy.is_some() {
            // The copied values are plain characteristics.
            return ManaValueProvenance::plain();
        }
        self.inner.base_mana_value_provenance(id)
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
        self.inner
            .generating_ability_exists(effect, source, source_view)
    }
}

/// The applied effects of `applied`, ascending id (unknown ids are skipped).
fn applied_effects<'a>(
    ctx: &EtbContext<'a>,
    applied: &'a EtbApplied,
) -> impl Iterator<Item = &'a EtbEffect> + use<'a> {
    let effects = ctx.effects;
    applied
        .effects
        .iter()
        .filter_map(move |id| effects.get(*id))
}

/// The projected permanent for `applied` (REQ-EVENT-001, CR 614.12): a pure function, no
/// choice is asked and no input changes.
///
/// # Errors
/// [`EtbError::Layer`]: `LayerError::UnknownObject(entry.object)` if the environment does not
/// know the object, `LayerError::DuplicateEffect` if an own static's id is already registered,
/// or any other evaluation error.
pub fn project(
    ctx: &EtbContext<'_>,
    entry: &EtbEntry,
    applied: &EtbApplied,
) -> Result<ProjectedPermanent, EtbError> {
    if ctx.env.base(entry.object).is_none() {
        return Err(EtbError::Layer(LayerError::UnknownObject(entry.object)));
    }
    // CR 614.12 (first input): applied copy effects replace the base, the last (highest id)
    // winning.
    let mut copy: Option<&Characteristics> = None;
    let mut tapped = false;
    let mut counters: BTreeMap<String, i64> = BTreeMap::new();
    let mut added: BTreeSet<String> = BTreeSet::new();
    for effect in applied_effects(ctx, applied) {
        match &effect.action {
            EtbAction::EntersAsCopyOf(c) => copy = Some(c),
            EtbAction::EntersTapped => tapped = true,
            EtbAction::EntersWithCounters { kind, amount } => {
                let n = counters.entry(kind.clone()).or_insert(0);
                *n = n.saturating_add(*amount);
            }
            EtbAction::AddCardTypes(types) => added.extend(types.iter().cloned()),
            EtbAction::Choose { .. } => {}
        }
    }
    let env = Projected {
        inner: ctx.env,
        object: entry.object,
        copy,
    };
    // CR 614.12 (second and third inputs): the own statics (not once a copy replaced the
    // permanent's abilities) on a clone of the effects already in force.
    let mut evaluated = if copy.is_none() && !entry.own_statics.is_empty() {
        let mut continuous = ctx.continuous.clone();
        for (id, effect) in &entry.own_statics {
            continuous.register(*id, effect.clone())?;
        }
        evaluate(&continuous, &env, entry.object)?
    } else {
        evaluate(ctx.continuous, &env, entry.object)?
    };
    // CR 614.12: "enters as an additional <type>" is part of how it enters.
    evaluated.characteristics.card_types.extend(added);
    Ok(ProjectedPermanent {
        object: entry.object,
        evaluated,
        tapped,
        counters,
        applied: applied.clone(),
    })
}

/// Whether any applied effect of `applied` is a copy effect.
fn copy_applied(ctx: &EtbContext<'_>, applied: &EtbApplied) -> bool {
    applied_effects(ctx, applied).any(|e| matches!(e.action, EtbAction::EntersAsCopyOf(_)))
}

/// Whether `condition` holds of `projected`.
fn holds(condition: &EtbCondition, projected: &ProjectedPermanent) -> bool {
    let c = &projected.evaluated.characteristics;
    match condition {
        EtbCondition::Always => true,
        EtbCondition::HasCardType(t) => c.card_types.contains(t),
        EtbCondition::LacksCardType(t) => !c.card_types.contains(t),
        EtbCondition::HasSubtype(t) => c.subtypes.contains(t),
    }
}

/// The ids of the effects that apply to `projected` now (CR 614.12, CR 616.1f).
fn applicable_to(
    ctx: &EtbContext<'_>,
    entry: &EtbEntry,
    projected: &ProjectedPermanent,
) -> Vec<EffectId> {
    let copied = copy_applied(ctx, &projected.applied);
    ctx.effects
        .effects
        .values()
        .filter(|e| !projected.applied.effects.contains(&e.id))
        .filter(|e| match e.scope {
            // CR 614.12: an effect from the permanent itself that affects only it; once it
            // enters as a copy, its own abilities are the copy's.
            EtbScope::ThisPermanent => e.source == Some(entry.object) && !copied,
            // CR 614.12: a general subset does not include its own source as it enters.
            EtbScope::OtherPermanents => e.source != Some(entry.object),
        })
        .filter(|e| holds(&e.condition, projected))
        .map(|e| e.id)
        .collect()
}

/// The ids, ascending, of the effects that apply to the projection now (REQ-EVENT-001,
/// CR 614.12): not in `applied.effects`, scope satisfied, condition true of the projection
/// ([`project`]). No tier restriction.
///
/// # Errors
/// As [`project`].
pub fn applicable(
    ctx: &EtbContext<'_>,
    entry: &EtbEntry,
    applied: &EtbApplied,
) -> Result<Vec<EffectId>, EtbError> {
    let projected = project(ctx, entry, applied)?;
    Ok(applicable_to(ctx, entry, &projected))
}

/// The result for one entering permanent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EtbResolved {
    /// The entering object.
    pub object: ObjectId,
    /// The final projection (its `applied.chosen` holds the chosen values: visible to the
    /// permanent's abilities on entry).
    pub projected: ProjectedPermanent,
    /// Memory for the entering permanent: each chosen value stored with
    /// [`Memory::set_choice`].
    pub memory: Memory,
}

impl EtbResolved {
    /// Writes the choices of `memory` (those under the keys of `projected.applied.chosen`)
    /// into `table` under `new_object`, the id the permanent has once it has entered
    /// (CR 400.7), through [`MemoryTable::get_mut_or_default`] and [`Memory::set_choice`]; the
    /// other keys of an existing Memory are kept.
    pub fn record_into(&self, table: &mut MemoryTable, new_object: ObjectId) {
        let target = table.get_mut_or_default(new_object);
        for key in self.projected.applied.chosen.keys() {
            if let Some(value) = self.memory.choice(key) {
                target.set_choice(key, value.clone());
            }
        }
    }
}

/// The error of a procedure step as an interrupt.
fn procedure(e: &EtbError) -> Interrupt {
    Interrupt::Error(ChoiceError::Procedure(e.to_string()))
}

/// The cheapest option cost of a `Choose` effect, if `effect` is one (CR 614.12b).
fn cheapest_choice(effect: &EtbEffect) -> Option<u64> {
    match &effect.action {
        EtbAction::Choose { options, .. } => {
            Some(options.iter().map(|o| o.cost).min().unwrap_or(0))
        }
        _ => None,
    }
}

/// The CR 616.1 loop for each entry, in slice order (the caller passes APNAP order), all before
/// any permanent enters (REQ-EVENT-001, CR 614.12, CR 614.12a, CR 614.12b, CR 616.1,
/// CR 616.1f). `payable` is the remaining budget per choosing player; a player absent from the
/// map has no limit. Returns one [`EtbResolved`] per entry, in the order of `entries`.
///
/// # Errors
/// [`Interrupt::Pending`] when a choice is pending; [`Interrupt::Error`] with
/// [`ChoiceError::Procedure`] for a wrong answer or any [`EtbError`] (its `Display`).
pub fn resolve_entering(
    ctx: &EtbContext<'_>,
    entries: &[EtbEntry],
    payable: &BTreeMap<PlayerId, u64>,
    choices: &mut ChoiceContext<'_>,
) -> Result<Vec<EtbResolved>, Interrupt> {
    // CR 614.12b: the cheapest cost of the `Choose` effects applicable to each entry at its
    // start (empty `EtbApplied`), for the lookahead of the entries not yet resolved.
    let mut start_reserve: Vec<(PlayerId, u64)> = Vec::with_capacity(entries.len());
    for entry in entries {
        let ids = applicable(ctx, entry, &EtbApplied::default()).map_err(|e| procedure(&e))?;
        let sum = ids
            .iter()
            .filter_map(|id| ctx.effects.get(*id).and_then(cheapest_choice))
            .fold(0u64, u64::saturating_add);
        start_reserve.push((entry.controller, sum));
    }
    let mut remaining = payable.clone();
    let mut out = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        // The undecided choices of the later entries of the same chooser.
        let later: u64 = start_reserve
            .iter()
            .skip(index.saturating_add(1))
            .filter(|(p, _)| *p == entry.controller)
            .map(|(_, s)| *s)
            .fold(0u64, u64::saturating_add);
        out.push(resolve_one(ctx, entry, later, &mut remaining, choices)?);
    }
    Ok(out)
}

/// The CR 616.1 loop for one entry.
fn resolve_one(
    ctx: &EtbContext<'_>,
    entry: &EtbEntry,
    later: u64,
    remaining: &mut BTreeMap<PlayerId, u64>,
    choices: &mut ChoiceContext<'_>,
) -> Result<EtbResolved, Interrupt> {
    let mut applied = EtbApplied::default();
    let mut memory = Memory::new();
    loop {
        // CR 616.1f, CR 616.2: re-project after every application.
        let projected = project(ctx, entry, &applied).map_err(|e| procedure(&e))?;
        let candidates = applicable_to(ctx, entry, &projected);
        let Some(tier) = candidates
            .iter()
            .filter_map(|id| ctx.effects.get(*id).map(|e| e.tier))
            .min()
        else {
            return Ok(EtbResolved {
                object: entry.object,
                projected,
                memory,
            });
        };
        // CR 616.1a-e: only the highest-priority tier present is choosable.
        let set: Vec<&EtbEffect> = candidates
            .iter()
            .filter_map(|id| ctx.effects.get(*id))
            .filter(|e| e.tier == tier)
            .collect();
        let chosen = choose_effect(entry, &set, choices)?;
        if let EtbAction::Choose { key, kind, options } = &chosen.action {
            // CR 614.12b: the other undecided choices of this entry, applicable now.
            let here = candidates
                .iter()
                .filter(|id| **id != chosen.id)
                .filter_map(|id| ctx.effects.get(*id).and_then(cheapest_choice))
                .fold(0u64, u64::saturating_add);
            let reserve = here.saturating_add(later);
            let budget = remaining.get(&entry.controller).copied();
            // CR 614.12b: an option is allowed iff its cost and the reserve fit the budget.
            let allowed: Vec<&EtbOption> = options
                .iter()
                .filter(|o| match budget {
                    None => true,
                    Some(b) => o.cost == 0 || o.cost.saturating_add(reserve) <= b,
                })
                .collect();
            if allowed.is_empty() {
                return Err(procedure(&EtbError::Unpayable {
                    chooser: entry.controller,
                    key: key.clone(),
                }));
            }
            let option = choose_option(entry, chosen, *kind, &allowed, choices)?;
            if let Some(b) = remaining.get_mut(&entry.controller) {
                *b = b.saturating_sub(option.cost);
            }
            // CR 614.12a, CR 614.12c, CR 607.2d: the last chosen value under a key wins.
            let value = kind.value(&option.text);
            memory.set_choice(key, value.clone());
            applied.chosen.insert(key.clone(), value);
        }
        applied.effects.insert(chosen.id);
    }
}

/// SRS 8.1 step [4] (CR 616.1): the one candidate, or the chooser's pick through
/// `CHOOSE_REPLACEMENT_EFFECT`.
fn choose_effect<'e>(
    entry: &EtbEntry,
    set: &[&'e EtbEffect],
    choices: &mut ChoiceContext<'_>,
) -> Result<&'e EtbEffect, Interrupt> {
    if let [only] = set {
        return Ok(only);
    }
    let options = (0..set.len())
        .map(|i| ChoiceOption::Number(i64::try_from(i).unwrap_or(i64::MAX)))
        .collect();
    // CR 616.1
    let request = PendingChoiceRequest {
        request_id: choices.next_request_id(),
        chooser: entry.controller,
        choice_type: ChoiceType::ChooseReplacementEffect,
        reason: ChoiceReason {
            cr: "CR 616.1".to_owned(),
            source: Some(entry.object),
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SelectReplacement { index: 0 },
        options: ChoiceOptions::Enumerated(options),
        payload: ChoicePayload::ChooseReplacementEffect {
            event: BusEventKind::ZoneChange.name().to_owned(),
            effects: set.iter().map(|e| e.description.clone()).collect(),
        },
    };
    match choices.ask(request)? {
        ChoiceResponse::SelectReplacement { index } => usize::try_from(index)
            .ok()
            .and_then(|i| set.get(i).copied())
            .ok_or_else(|| {
                Interrupt::Error(ChoiceError::Procedure(format!(
                    "replacement index {index} is out of range"
                )))
            }),
        other => Err(Interrupt::Error(ChoiceError::Procedure(format!(
            "unexpected answer {}",
            other.wire_name()
        )))),
    }
}

/// CR 614.12a: the choice of a `Choose` action, made before the permanent enters: the one
/// allowed option, or the chooser's pick through `CHOOSE_OPTION_OR_VALUE`.
fn choose_option<'o>(
    entry: &EtbEntry,
    effect: &EtbEffect,
    kind: EtbChoiceKind,
    allowed: &[&'o EtbOption],
    choices: &mut ChoiceContext<'_>,
) -> Result<&'o EtbOption, Interrupt> {
    let first = match allowed {
        [only] => return Ok(only),
        [first, ..] => first.text.clone(),
        [] => {
            return Err(Interrupt::Error(ChoiceError::Procedure(
                "a choice with no allowed option".to_owned(),
            )));
        }
    };
    let texts: Vec<String> = allowed.iter().map(|o| o.text.clone()).collect();
    let request = PendingChoiceRequest {
        request_id: choices.next_request_id(),
        chooser: entry.controller,
        choice_type: ChoiceType::ChooseOptionOrValue,
        reason: ChoiceReason {
            cr: kind.cr().to_owned(),
            source: effect.source.or(Some(entry.object)),
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SubmitChoiceValue {
            value: crate::choice::ChoiceValue::Text(first),
        },
        options: ChoiceOptions::Enumerated(texts.iter().cloned().map(ChoiceOption::Text).collect()),
        payload: ChoicePayload::ChooseOptionOrValue {
            option_type: kind.option_type().to_owned(),
            allowed: AllowedValues::Texts(texts),
        },
    };
    match choices.ask(request)? {
        ChoiceResponse::SubmitChoiceValue {
            value: crate::choice::ChoiceValue::Text(t),
        } => allowed
            .iter()
            .copied()
            .find(|o| o.text == t)
            .ok_or_else(|| {
                Interrupt::Error(ChoiceError::Procedure(format!(
                    "{t:?} is not one of the allowed values"
                )))
            }),
        other => Err(Interrupt::Error(ChoiceError::Procedure(format!(
            "unexpected answer {}",
            other.wire_name()
        )))),
    }
}

#[cfg(test)]
#[path = "etb_tests.rs"]
mod tests;
