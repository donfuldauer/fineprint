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
//! characteristics replaced by the LAST applied [`EtbAction::EntersAsCopyOf`] in application
//! order ([`EtbApplied::sequence`], CR 707.2, CR 616.1f); the layers evaluated with the
//! continuous effects already in force plus, on a clone of the registry, the permanent's own
//! static abilities ([`EtbEntry::own_statics`]) or, once a copy has been applied, the statics the
//! last applied copy declares ([`CopyAbilities`], CR 707.5), and one synthetic layer-4 effect
//! adding the card types of every applied [`EtbAction::AddCardTypes`] (CR 613.1d), so power,
//! toughness and later layers see the added type. Once a copy effect has been applied, the
//! permanent's own statics and its own [`EtbScope::ThisPermanent`] effects no longer count: its
//! abilities are the last copy's declared ones. A [`EtbScope::ThisPermanent`] effect also needs
//! its generating ability on the projected permanent
//! ([`LayerEnvironment::generating_ability_exists`], CR 614.12, CR 603.6d). The three CR 614.12
//! inputs have no order in the CR; the order used is docs/DECISIONS.md "#40 FP-039 — ETB
//! projection API and modelling choices".
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
//! **Payability** (CR 614.12b, CR 616.2). Each option commits a cost to its chooser; an option is
//! offered only if its cost, plus the least total cost the rest of its entry can still commit
//! (an exact lookahead over the effects that apply or become applicable), plus that least total
//! of every later entry of the same chooser, fits the remaining budget (u128 arithmetic, at most
//! [`MAX_LOOKAHEAD_STATES`] states per entry, each charged when first entered; an iterative
//! search). The CR 616.1 order choice is never filtered. Every entry's starting projection is
//! checked before any reserve or request (CR 614.12a).
//!
//! **Standalone scope.** Not joined to the bus or `GameState`: the caller supplies every input
//! and applies the result. Determinism (REQ-CORE-001): ordered containers, integer arithmetic,
//! no clock.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::ValueExpressionAST;
use fineprint_dsl::expr::ObjectSelectorAST;

use super::{BusEventKind, Tier};
use crate::choice::{
    AllowedValues, ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload,
    ChoiceReason, ChoiceResponse, ChoiceType, Interrupt, PendingChoiceRequest,
};
use crate::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, Evaluated, LayerEnvironment,
    LayerError, LayerModification, ManaValueProvenance, MatchContext, ObjectBase, evaluate,
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
    /// copiable values (applied to the base, not as a layer effect). The last applied copy wins
    /// (CR 707.2); its abilities besides characteristics are declared with
    /// [`EtbEffects::set_copy_abilities`].
    EntersAsCopyOf(Box<Characteristics>),
    /// "enters as an additional <types>": these card types are added by a layer-4 effect
    /// (CR 613.1d) whose timestamp is later than every continuous effect of the projection
    /// (CR 613.7), so power, toughness and later layers see them; not copiable (CR 707.2).
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

/// What a copy effect gives its entrant besides characteristics (CR 707.5, CR 614.12).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopyAbilities {
    /// Static abilities of the copy model, as continuous effects. When this copy is the LAST
    /// applied copy of an entry, [`project`] registers them on the cloned registry bound to the
    /// entrant: `source` is set to `Some(entry.object)`, `controller` to `entry.controller`, and
    /// an [`Affected::Objects`] set is replaced by `{entry.object}` (an [`Affected::Selector`] is
    /// kept).
    pub statics: Vec<(EffectId, ContinuousEffect)>,
    /// Ids of [`EtbEffects`] entries (scope [`EtbScope::ThisPermanent`]) that are replacement
    /// abilities of the copy model. When this copy is the last applied copy, they apply to the
    /// entrant as if their source were the entrant (CR 707.5).
    pub etb: Vec<EffectId>,
}

/// The ETB effects in force, keyed and iterated by ascending [`EffectId`], and the abilities
/// each copy effect gives its entrant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EtbEffects {
    effects: BTreeMap<EffectId, EtbEffect>,
    copy_abilities: BTreeMap<EffectId, CopyAbilities>,
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
    /// [`EtbError::DuplicateEffect`] if an effect with that id is present;
    /// [`EtbError::EmptyChoice`] if it is an [`EtbAction::Choose`] without options (nothing
    /// changes).
    pub fn add(&mut self, effect: EtbEffect) -> Result<(), EtbError> {
        if self.effects.contains_key(&effect.id) {
            return Err(EtbError::DuplicateEffect(effect.id));
        }
        // CR 614.12b: a choice with no option has no payable continuation; it is refused here so
        // the lookahead never reserves a cost for it.
        if let EtbAction::Choose { options, .. } = &effect.action
            && options.is_empty()
        {
            return Err(EtbError::EmptyChoice(effect.id));
        }
        self.effects.insert(effect.id, effect);
        Ok(())
    }

    /// Removes and returns the effect `id`, if present; a copy effect's
    /// [`CopyAbilities`] declaration is dropped with it.
    pub fn remove(&mut self, id: EffectId) -> Option<EtbEffect> {
        self.copy_abilities.remove(&id);
        self.effects.remove(&id)
    }

    /// Declares the abilities copy effect `copy` gives its entrant (CR 707.5); replaces an
    /// earlier declaration for `copy`.
    ///
    /// # Errors
    /// [`EtbError::NotACopyEffect`] if `copy` is not in the set or its action is not
    /// [`EtbAction::EntersAsCopyOf`] (nothing changes).
    pub fn set_copy_abilities(
        &mut self,
        copy: EffectId,
        abilities: CopyAbilities,
    ) -> Result<(), EtbError> {
        match self.effects.get(&copy) {
            Some(EtbEffect {
                action: EtbAction::EntersAsCopyOf(_),
                ..
            }) => {
                self.copy_abilities.insert(copy, abilities);
                Ok(())
            }
            _ => Err(EtbError::NotACopyEffect(copy)),
        }
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
    /// [`EtbEffects::set_copy_abilities`] names an effect that is absent or not a copy effect.
    NotACopyEffect(EffectId),
    /// [`EtbEffects::add`] was given an [`EtbAction::Choose`] with an empty `options` list.
    EmptyChoice(EffectId),
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
            EtbError::NotACopyEffect(id) => {
                write!(f, "ETB effect {} is not a present copy effect", id.get())
            }
            EtbError::EmptyChoice(id) => {
                write!(f, "ETB effect {} is a choice without options", id.get())
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
    /// The applied effects: the set that stops an effect applying twice.
    pub effects: BTreeSet<EffectId>,
    /// Chosen values by key (the keys of applied [`EtbAction::Choose`] actions).
    pub chosen: BTreeMap<String, ChoiceValue>,
    /// The order the effects were applied in (CR 616.1f), first applied first.
    /// [`resolve_entering`] fills it.
    pub order: Vec<EffectId>,
}

impl EtbApplied {
    /// The application sequence: `order` (skipping ids not in `effects` and repeated ids), then
    /// every id of `effects` not in `order`, ascending. With an empty `order` this is the
    /// ascending-id order.
    #[must_use]
    pub fn sequence(&self) -> Vec<EffectId> {
        let mut seen: BTreeSet<EffectId> = BTreeSet::new();
        let mut out = Vec::with_capacity(self.effects.len());
        for id in &self.order {
            if self.effects.contains(id) && seen.insert(*id) {
                out.push(*id);
            }
        }
        out.extend(self.effects.iter().filter(|id| !seen.contains(id)).copied());
        out
    }
}

/// The permanent as it would exist on the battlefield (CR 614.12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedPermanent {
    /// The entering object.
    pub object: ObjectId,
    /// Characteristics and controller after the base was replaced by the last applied copy
    /// effect and the layers were evaluated (always as an on-battlefield object) with the
    /// continuous effects in force, the own statics (or the last copy's declared statics) and
    /// the layer-4 addition of applied [`EtbAction::AddCardTypes`].
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

/// The applied effects of `applied` in application order ([`EtbApplied::sequence`]; unknown
/// ids are skipped).
fn applied_effects<'a>(ctx: &EtbContext<'a>, applied: &EtbApplied) -> Vec<&'a EtbEffect> {
    let effects = ctx.effects;
    applied
        .sequence()
        .into_iter()
        .filter_map(|id| effects.get(id))
        .collect()
}

/// The last applied copy effect of `applied` in application order, if any (CR 707.2,
/// CR 616.1f).
fn last_copy<'a>(ctx: &EtbContext<'a>, applied: &EtbApplied) -> Option<&'a EtbEffect> {
    applied_effects(ctx, applied)
        .into_iter()
        .rev()
        .find(|e| matches!(e.action, EtbAction::EntersAsCopyOf(_)))
}

/// `effect` bound to the entrant `object` entering under `controller`: its source, and a
/// locked object set, become the entrant, and its controller the entrant's (CR 707.5: the
/// copy's static abilities are the entrant's).
fn bind_to(object: ObjectId, controller: PlayerId, effect: &ContinuousEffect) -> ContinuousEffect {
    let mut bound = effect.clone();
    bound.source = Some(object);
    bound.controller = controller;
    if let Affected::Objects(_) = bound.affected {
        bound.affected = Affected::Objects([object].into_iter().collect());
    }
    bound
}

/// The synthetic layer-4 effect adding `types` to `object` (CR 613.1d, CR 205.1b): an id and a
/// timestamp strictly above every effect of `continuous` (CR 613.7), so it applies after them.
/// It realises a replacement effect, not a static ability of `object`, so it is registered as
/// [`EffectOrigin::Resolved`] with the locked set `{object}`: the layer pipeline never asks
/// [`LayerEnvironment::generating_ability_exists`] about it.
///
/// # Errors
/// [`LayerError::IdExhausted`] if no such id or timestamp exists.
fn added_types_effect(
    continuous: &ContinuousEffects,
    object: ObjectId,
    controller: PlayerId,
    types: BTreeSet<String>,
) -> Result<(EffectId, ContinuousEffect), LayerError> {
    let mut max_id = 0u64;
    let mut max_ts = 0u64;
    for (id, effect) in continuous.iter() {
        max_id = max_id.max(id.get());
        max_ts = max_ts.max(effect.timestamp.get());
    }
    let id = max_id.checked_add(1).ok_or(LayerError::IdExhausted)?;
    let ts = max_ts.checked_add(1).ok_or(LayerError::IdExhausted)?;
    Ok((
        EffectId::new(id),
        ContinuousEffect {
            source: Some(object),
            controller,
            timestamp: Timestamp::new(ts),
            layer: Layer::L4,
            is_cda: false,
            // CR 613.1d: no generating ability to check; the set is locked to the entrant.
            origin: EffectOrigin::Resolved,
            affected: Affected::Objects([object].into_iter().collect()),
            modification: LayerModification::AddTypes {
                supertypes: BTreeSet::new(),
                card_types: types,
                subtypes: BTreeSet::new(),
            },
        },
    ))
}

/// The projected permanent for `applied` (REQ-EVENT-001, CR 614.12): a pure function, no
/// choice is asked and no input changes. Applied effects are replayed in
/// [`EtbApplied::sequence`] order, so the last applied copy supplies the base (CR 707.2,
/// CR 616.1f).
///
/// # Errors
/// [`EtbError::Layer`]: `LayerError::UnknownObject(entry.object)` if the environment does not
/// know the object, `LayerError::DuplicateEffect` if an own static's (or a declared copy
/// static's) id is already registered, `LayerError::IdExhausted` if no id or timestamp is left
/// for the added card types, or any other evaluation error.
pub fn project(
    ctx: &EtbContext<'_>,
    entry: &EtbEntry,
    applied: &EtbApplied,
) -> Result<ProjectedPermanent, EtbError> {
    let Some(base) = ctx.env.base(entry.object) else {
        return Err(EtbError::Layer(LayerError::UnknownObject(entry.object)));
    };
    // CR 614.12 (first input), CR 707.2, CR 616.1f: applied copy effects replace the base, the
    // last applied winning.
    let mut copy: Option<(EffectId, &Characteristics)> = None;
    let mut tapped = false;
    let mut counters: BTreeMap<String, i64> = BTreeMap::new();
    let mut added: BTreeSet<String> = BTreeSet::new();
    for effect in applied_effects(ctx, applied) {
        match &effect.action {
            EtbAction::EntersAsCopyOf(c) => copy = Some((effect.id, c)),
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
        copy: copy.map(|(_, c)| c),
    };
    // CR 614.12 (second input): the own statics, or once a copy replaced the permanent's
    // abilities, the statics the last copy declares, bound to the entrant (CR 707.5).
    let statics: Vec<(EffectId, ContinuousEffect)> = match copy {
        None => entry.own_statics.clone(),
        Some((id, _)) => ctx
            .effects
            .copy_abilities
            .get(&id)
            .map(|a| {
                a.statics
                    .iter()
                    .map(|(sid, s)| (*sid, bind_to(entry.object, entry.controller, s)))
                    .collect()
            })
            .unwrap_or_default(),
    };
    let evaluated = if statics.is_empty() && added.is_empty() {
        // CR 614.12 (third input): the effects already in force.
        evaluate(ctx.continuous, &env, entry.object)?
    } else {
        let mut continuous = ctx.continuous.clone();
        for (id, effect) in statics {
            continuous.register(id, effect)?;
        }
        if !added.is_empty() {
            // CR 613.1d, CR 613.7, CR 205.1b: "enters as an additional <type>" is a layer-4
            // type addition later than every other effect; P/T (CR 208.3) and later layers see
            // the type.
            let controller = base.controller.unwrap_or(entry.controller);
            let (id, effect) = added_types_effect(&continuous, entry.object, controller, added)?;
            continuous.register(id, effect)?;
        }
        evaluate(&continuous, &env, entry.object)?
    };
    Ok(ProjectedPermanent {
        object: entry.object,
        evaluated,
        tapped,
        counters,
        applied: applied.clone(),
    })
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
    // CR 707.5: once a copy applied, the permanent's own abilities are the last copy's.
    let copy_etb: Option<&[EffectId]> = last_copy(ctx, &projected.applied).map(|c| {
        ctx.effects
            .copy_abilities
            .get(&c.id)
            .map_or(&[][..], |a| a.etb.as_slice())
    });
    ctx.effects
        .effects
        .values()
        .filter(|e| !projected.applied.effects.contains(&e.id))
        .filter(|e| match e.scope {
            // CR 614.12, CR 603.6d: an effect of the permanent's own ability that affects only
            // it, while the projected permanent still has that ability (the ETB effect id is
            // asked in the static-effect id space of the layer hook); once it enters as a
            // copy, its own abilities are the last copy's declared ones (CR 707.5).
            EtbScope::ThisPermanent => {
                let own = match copy_etb {
                    None => e.source == Some(entry.object),
                    Some(declared) => declared.contains(&e.id),
                };
                own && ctx.env.generating_ability_exists(
                    e.id,
                    entry.object,
                    Some(&projected.evaluated),
                )
            }
            // CR 614.12: a general subset does not include its own source as it enters.
            EtbScope::OtherPermanents => e.source != Some(entry.object),
        })
        .filter(|e| holds(&e.condition, projected))
        .map(|e| e.id)
        .collect()
}

/// The ids, ascending, of the effects that apply to the projection now (REQ-EVENT-001,
/// CR 614.12): not in `applied.effects`, scope satisfied (for [`EtbScope::ThisPermanent`],
/// also the generating ability present on the projection), condition true of the projection
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
    /// permanent's abilities on entry; its `applied.order` the application order).
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

/// The most lookahead states (applied set, last copy) one entry may enter (R3: a fixed bound,
/// exceeding it is an error, never a silent truncation).
const MAX_LOOKAHEAD_STATES: usize = 4096;

/// The cheapest cost `effect` commits when it applies: the cheapest option of a `Choose`
/// (CR 614.12b; [`EtbEffects::add`] refuses a `Choose` without options), 0 for any other
/// action.
fn cheapest(effect: &EtbEffect) -> u128 {
    match &effect.action {
        EtbAction::Choose { options, .. } => {
            u128::from(options.iter().map(|o| o.cost).min().unwrap_or(0))
        }
        _ => 0,
    }
}

/// A lookahead state: the applied set and the last applied copy (which fixes the projection,
/// CR 707.2).
type State = (BTreeSet<EffectId>, Option<EffectId>);

/// The exact CR 614.12b lookahead of one entry: the least total cost the rest of the entry can
/// commit from a state, over every CR 616.1 order (CR 616.1f, CR 616.2: effects that become
/// applicable count, effects a mandatory replacement removes do not). The option costs never
/// change the projection, so a state is the applied set plus the last applied copy.
///
/// Every distinct state is charged against [`MAX_LOOKAHEAD_STATES`] when it is first entered,
/// before anything below it is explored; a state already evaluated is a memo hit and costs
/// nothing. The search is iterative (an explicit stack), so a deep chain of states cannot
/// overflow the call stack.
struct Lookahead<'c, 'a> {
    ctx: &'c EtbContext<'a>,
    entry: &'c EtbEntry,
    memo: BTreeMap<State, u128>,
    /// The distinct states entered so far (evaluated or being evaluated).
    entered: usize,
}

/// A state being evaluated: its top-tier candidates (CR 616.1a-e), the next one to explore and
/// the least total found so far.
struct Frame<'a> {
    state: State,
    candidates: Vec<&'a EtbEffect>,
    next: usize,
    best: Option<u128>,
}

impl Frame<'_> {
    /// Records `rest`, the least total after the candidate `next`, and moves to the next one.
    fn settle(&mut self, rest: u128) {
        let Some(effect) = self.candidates.get(self.next) else {
            return;
        };
        // At most one u64 per effect: u128 cannot overflow.
        let total = cheapest(effect).saturating_add(rest);
        self.best = Some(self.best.map_or(total, |b| b.min(total)));
        self.next = self.next.saturating_add(1);
    }
}

/// A state entered: its value is known at once, or it must be explored.
enum Entered<'a> {
    Done(u128),
    Open(Frame<'a>),
}

/// The lookahead state after `id` is applied to (`set`, `last`).
fn step(
    ctx: &EtbContext<'_>,
    set: &BTreeSet<EffectId>,
    last: Option<EffectId>,
    id: EffectId,
) -> State {
    let mut next = set.clone();
    next.insert(id);
    let is_copy = ctx
        .effects
        .get(id)
        .is_some_and(|e| matches!(e.action, EtbAction::EntersAsCopyOf(_)));
    (next, if is_copy { Some(id) } else { last })
}

impl<'c, 'a> Lookahead<'c, 'a> {
    fn new(ctx: &'c EtbContext<'a>, entry: &'c EtbEntry) -> Self {
        Lookahead {
            ctx,
            entry,
            memo: BTreeMap::new(),
            entered: 0,
        }
    }

    /// Enters the new `state` (not in the memo): charges it against the limit (R3), then gives
    /// its value when nothing below it can commit a cost, else its frame.
    fn enter(&mut self, state: State) -> Result<Entered<'a>, Interrupt> {
        if self.entered >= MAX_LOOKAHEAD_STATES {
            return Err(Interrupt::Error(ChoiceError::Procedure(format!(
                "the CR 614.12b lookahead of object {} exceeds {MAX_LOOKAHEAD_STATES} states",
                self.entry.object.get()
            ))));
        }
        self.entered = self.entered.saturating_add(1);
        // No unapplied effect can commit a cost: nothing to look at (no projection).
        let any_cost = self
            .ctx
            .effects
            .effects
            .values()
            .any(|e| !state.0.contains(&e.id) && cheapest(e) > 0);
        if !any_cost {
            self.memo.insert(state, 0);
            return Ok(Entered::Done(0));
        }
        // The projection of the state: the last copy applied last.
        let (set, last) = &state;
        let mut order: Vec<EffectId> = set.iter().copied().filter(|i| Some(*i) != *last).collect();
        order.extend(*last);
        let applied = EtbApplied {
            effects: set.clone(),
            chosen: BTreeMap::new(),
            order,
        };
        let projected = project(self.ctx, self.entry, &applied).map_err(|e| procedure(&e))?;
        let candidates = top_tier(self.ctx, &applicable_to(self.ctx, self.entry, &projected));
        if candidates.is_empty() {
            self.memo.insert(state, 0);
            return Ok(Entered::Done(0));
        }
        Ok(Entered::Open(Frame {
            state,
            candidates,
            next: 0,
            best: None,
        }))
    }

    /// `mincost(set, last)`: 0 if nothing applies, else the minimum over the candidates `c` of
    /// the top applicable tier (CR 616.1a-e) of `cheapest(c) + mincost(state + c)`.
    fn mincost(
        &mut self,
        set: &BTreeSet<EffectId>,
        last: Option<EffectId>,
    ) -> Result<u128, Interrupt> {
        let root: State = (set.clone(), last);
        if let Some(v) = self.memo.get(&root) {
            return Ok(*v);
        }
        let mut stack: Vec<Frame<'a>> = match self.enter(root)? {
            Entered::Done(v) => return Ok(v),
            Entered::Open(frame) => vec![frame],
        };
        // The value just found for the top frame's current candidate, or for a finished frame.
        let mut found: Option<u128> = None;
        while let Some(top) = stack.last_mut() {
            if let Some(v) = found.take() {
                top.settle(v);
            }
            let Some(effect) = top.candidates.get(top.next).copied() else {
                // Every candidate explored: the state's value is known.
                let value = top.best.unwrap_or(0);
                if let Some(done) = stack.pop() {
                    self.memo.insert(done.state, value);
                }
                found = Some(value);
                continue;
            };
            let child = step(self.ctx, &top.state.0, top.state.1, effect.id);
            found = match self.memo.get(&child).copied() {
                // A memo hit consumes nothing.
                Some(v) => Some(v),
                None => match self.enter(child)? {
                    Entered::Done(v) => Some(v),
                    Entered::Open(frame) => {
                        stack.push(frame);
                        None
                    }
                },
            };
        }
        Ok(found.unwrap_or(0))
    }
}

/// The effects of `candidates` in the highest-priority tier present (CR 616.1a-e), ascending
/// id.
fn top_tier<'a>(ctx: &EtbContext<'a>, candidates: &[EffectId]) -> Vec<&'a EtbEffect> {
    let effects: Vec<&'a EtbEffect> = candidates
        .iter()
        .filter_map(|id| ctx.effects.get(*id))
        .collect();
    let Some(tier) = effects.iter().map(|e| e.tier).min() else {
        return Vec::new();
    };
    effects.into_iter().filter(|e| e.tier == tier).collect()
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
    // CR 614.12a: every choice is made before anything enters, so every entry's starting
    // projection is checked first: a projection error (an unknown object, a duplicate
    // own-static id, ...) of any entry fails the call before any reserve or request.
    for entry in entries {
        project(ctx, entry, &EtbApplied::default()).map_err(|e| procedure(&e))?;
    }
    // CR 614.12b, CR 616.2: the least total cost each entry can commit from its start, for the
    // reserve of the entries not yet resolved (entries are independent). Unlimited choosers
    // need no reserve.
    let mut start_reserve: Vec<(PlayerId, u128)> = Vec::with_capacity(entries.len());
    for entry in entries {
        let reserve = if payable.contains_key(&entry.controller) {
            Lookahead::new(ctx, entry).mincost(&BTreeSet::new(), None)?
        } else {
            0
        };
        start_reserve.push((entry.controller, reserve));
    }
    let mut remaining = payable.clone();
    let mut out = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        // The least total of the later entries of the same chooser.
        let later: u128 = start_reserve
            .iter()
            .skip(index.saturating_add(1))
            .filter(|(p, _)| *p == entry.controller)
            .map(|(_, s)| *s)
            .fold(0u128, u128::saturating_add);
        out.push(resolve_one(ctx, entry, later, &mut remaining, choices)?);
    }
    Ok(out)
}

/// The CR 616.1 loop for one entry.
fn resolve_one(
    ctx: &EtbContext<'_>,
    entry: &EtbEntry,
    later: u128,
    remaining: &mut BTreeMap<PlayerId, u64>,
    choices: &mut ChoiceContext<'_>,
) -> Result<EtbResolved, Interrupt> {
    let mut applied = EtbApplied::default();
    let mut memory = Memory::new();
    let mut lookahead = Lookahead::new(ctx, entry);
    loop {
        // CR 616.1f, CR 616.2: re-project after every application.
        let projected = project(ctx, entry, &applied).map_err(|e| procedure(&e))?;
        // CR 616.1a-e: only the highest-priority tier present is choosable.
        let set = top_tier(ctx, &applicable_to(ctx, entry, &projected));
        if set.is_empty() {
            return Ok(EtbResolved {
                object: entry.object,
                projected,
                memory,
            });
        }
        // CR 616.1e: the order choice is never filtered by payability (CR 614.12b restricts
        // only the choices the effects require).
        let chosen = choose_effect(entry, &set, choices)?;
        if let EtbAction::Choose { key, kind, options } = &chosen.action {
            let budget = remaining.get(&entry.controller).copied();
            // CR 614.12b, CR 616.2: the least the rest of this entry can still commit once
            // this choice is made, plus the later entries' least totals.
            let reserve = match budget {
                None => 0,
                Some(_) => {
                    let last = last_copy(ctx, &applied).map(|c| c.id);
                    let (next, next_last) = step(ctx, &applied.effects, last, chosen.id);
                    lookahead.mincost(&next, next_last)?.saturating_add(later)
                }
            };
            // CR 614.12b: an option is allowed iff its cost and the reserve fit the budget
            // (exact u128 arithmetic); a cost-0 option always is.
            let allowed: Vec<&EtbOption> = options
                .iter()
                .filter(|o| match budget {
                    None => true,
                    Some(b) => {
                        o.cost == 0 || u128::from(o.cost).saturating_add(reserve) <= u128::from(b)
                    }
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
        // CR 616.1f: the application order is recorded.
        applied.effects.insert(chosen.id);
        applied.order.push(chosen.id);
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
