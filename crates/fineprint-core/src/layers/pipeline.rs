// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Per-object evaluation through layers 1a-7d (REQ-LAYER-001, REQ-LAYER-002, CR 613.1-613.5).

use std::cmp::Ordering;
use std::collections::BTreeMap;

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, ValueExpressionAST};
use fineprint_dsl::expr::ObjectSelectorAST;

use super::error::LayerError;
use super::lockin::LockRecord;
use super::ordering::{OrderRecord, Scratch, run_layer};
use super::registry::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerModification,
};
use crate::objects::Characteristics;

/// The counter kinds that are keyword counters (CR 122.1b), without variants. Ward is not one.
/// Variants such as "hexproof from red" (CR 702.11d) are recognised by
/// [`keyword_counter_keyword`].
pub const KEYWORD_COUNTER_KINDS: [&str; 15] = [
    "flying",
    "first strike",
    "double strike",
    "deathtouch",
    "decayed",
    "exalted",
    "haste",
    "hexproof",
    "indestructible",
    "lifelink",
    "menace",
    "reach",
    "shadow",
    "trample",
    "vigilance",
];

/// The base of one object as the caller knows it: its characteristics before any continuous
/// effect (CR 613.1), its controller and whether it is on the battlefield.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectBase {
    /// The characteristics of the face in use, or the token or copy definition (CR 613.1).
    pub characteristics: Characteristics,
    /// The controller before layer 2, if it has one.
    pub controller: Option<PlayerId>,
    /// Whether the object is on the battlefield (decides P/T presence, CR 208.3).
    pub on_battlefield: bool,
}

/// An object's characteristics and controller after the layers applied so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evaluated {
    /// The characteristics.
    pub characteristics: Characteristics,
    /// The controller (set in layer 2, CR 613.1b).
    pub controller: Option<PlayerId>,
}

/// What [`LayerEnvironment::matches`] sees when a selector effect is about to apply.
#[derive(Clone, Copy, Debug)]
pub struct MatchContext<'a> {
    /// The effect's source object, if any.
    pub source: Option<ObjectId>,
    /// The effect's controller.
    pub effect_controller: PlayerId,
    /// The object being evaluated.
    pub candidate: ObjectId,
    /// The candidate as evaluated by every earlier step (CR 613.5).
    pub candidate_view: &'a Evaluated,
}

/// The game as the layer system reads it; supplied by the caller. Every method must be a pure
/// function of the caller's state (REQ-CORE-001).
pub trait LayerEnvironment {
    /// Every object to evaluate, ascending [`ObjectId`].
    fn objects(&self) -> Vec<ObjectId>;

    /// The base of `id`; `None` means unknown.
    fn base(&self, id: ObjectId) -> Option<ObjectBase>;

    /// The counters on `id`: kind to amount (at least 1). Default: none.
    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        let _ = id;
        BTreeMap::new()
    }

    /// The timestamp of the counters of `kind` on `id` (CR 613.7c). Default: `None`, treated as
    /// `Timestamp::new(0)`.
    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        let _ = (id, kind);
        None
    }

    /// The objects attached to `id`. Default: none.
    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        let _ = id;
        Vec::new()
    }

    /// Whether the candidate in `ctx` matches `selector`, judged on `ctx.candidate_view`.
    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool;

    /// The value of a non-literal base power or toughness of `object` (a characteristic-defining
    /// expression, CR 604.3). Default: a literal gives its number, anything else `None`.
    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        let _ = object;
        literal_of(expr)
    }

    /// Whether the static ability that generates `effect` still exists on its source.
    /// Called only for `EffectOrigin::Static` effects whose `source` is `Some(source)`.
    /// `source_view` is `Some(state so far)` only when `source` is the object being
    /// evaluated; for any other source it is `None` (cross-object evaluation is #186).
    /// Default: `true` (existence is not modelled).
    ///
    /// CR 613.8a, 611.3, 113.6: an effect of a static ability exists only while its generating
    /// ability does, so an effect whose ability has been removed does not apply, and removing it
    /// changes the effect's existence (a dependency). Effects from resolved spells and abilities
    /// (CR 611.2) are never asked.
    fn generating_ability_exists(
        &self,
        effect: EffectId,
        source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        let _ = (effect, source, source_view);
        true
    }
}

/// The number of a literal power or toughness: a JSON integer or `{"LITERAL": n}`; `None` for
/// any other expression.
#[must_use]
pub fn literal_of(expr: &ValueExpressionAST) -> Option<i64> {
    if let Some(n) = expr.0.as_i64() {
        return Some(n);
    }
    let object = expr.0.as_object()?;
    if object.len() != 1 {
        return None;
    }
    object.get("LITERAL")?.as_i64()
}

/// The literal power or toughness `n` (a JSON integer, the form card faces use).
#[must_use]
pub fn literal(n: i64) -> ValueExpressionAST {
    ValueExpressionAST(n.into())
}

/// The keyword ability a keyword counter of `kind` grants (CR 122.1b): kind `KEYWORD`, ability
/// id `counter:<kind>`, keyword `<kind>`.
#[must_use]
pub fn keyword_counter_ability(kind: &str) -> AbilityAST {
    AbilityAST(
        [
            ("kind", "KEYWORD".to_owned()),
            ("abilityId", format!("counter:{kind}")),
            ("keyword", kind.to_owned()),
        ]
        .into_iter()
        .collect(),
    )
}

/// The keyword a keyword counter of `kind` grants (CR 122.1b): `Some(kind)` when `kind` is one of
/// [`KEYWORD_COUNTER_KINDS`] or a variant of one, that is the name followed by a single ASCII
/// space and a parameter (CR 702.11d: "hexproof from red"); `None` otherwise. Ward is not a
/// keyword counter, so "ward" and "ward 2" give `None`.
#[must_use]
pub fn keyword_counter_keyword(kind: &str) -> Option<&str> {
    // CR 122.1b: the listed keywords "as well as any variants of those keywords".
    KEYWORD_COUNTER_KINDS
        .iter()
        .any(|name| kind == *name || is_variant_of(kind, name))
        .then_some(kind)
}

/// The keywords whose removal also removes every variant of them. CR 702.11e: "Any effect that
/// causes an object to lose hexproof will cause an object to lose all 'hexproof from [quality]'
/// abilities." The baseline states this for hexproof only.
const REMOVAL_REMOVES_VARIANTS: &[&str] = &["hexproof"]; // CR 702.11e

/// Whether `candidate` is a variant of keyword `name`: `name`, one ASCII space, then a non-empty
/// parameter that does not start with a space (CR 702.11d: "hexproof from red").
pub(super) fn is_variant_of(candidate: &str, name: &str) -> bool {
    candidate
        .strip_prefix(name)
        .and_then(|rest| rest.strip_prefix(' '))
        .is_some_and(|param| !param.is_empty() && !param.starts_with(' '))
}

/// Whether `ability` is a keyword ability that removing keyword `keyword` removes (CR 613.1f):
/// its keyword is `keyword` exactly, or `keyword` is hexproof and its keyword is a
/// "hexproof from [quality]" variant ([`is_variant_of`], CR 702.11e).
fn is_keyword(ability: &AbilityAST, keyword: &str) -> bool {
    let Some(object) = ability.0.as_object() else {
        return false;
    };
    if object.get("kind").and_then(|k| k.as_str()) != Some("KEYWORD") {
        return false;
    }
    let Some(candidate) = object.get("keyword").and_then(|k| k.as_str()) else {
        return false;
    };
    // CR 702.11e: losing hexproof loses every "hexproof from [quality]" ability.
    candidate == keyword
        || (REMOVAL_REMOVES_VARIANTS.contains(&keyword) && is_variant_of(candidate, keyword))
}

/// The per-counter (power, toughness) change of a P/T counter kind `+X/+Y` or `-X/-Y`
/// (CR 122.1a); `None` for any other kind.
#[must_use]
pub fn pt_counter_delta(kind: &str) -> Option<(i64, i64)> {
    let sign = kind.chars().next()?;
    let factor = match sign {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let (x, y) = kind[1..].split_once('/')?;
    let y = y.strip_prefix(sign)?;
    let parse = |s: &str| -> Option<i64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse::<i64>().ok()
    };
    Some((factor * parse(x)?, factor * parse(y)?))
}

/// One thing applied within a layer: an effect, or the counters of one kind.
pub(super) enum Step<'a> {
    /// A continuous effect.
    Effect(EffectId, &'a ContinuousEffect),
    /// The counters of one kind and their amount.
    Counter(&'a str, i64),
}

/// The order key of a step: timestamp, then effects before counters, then EffectId or kind
/// (CR 613.7; the tie-break is docs/DECISIONS.md "#32 FP-031 - Ordering, tie-break and P/T
/// presence").
pub(super) fn step_order(a: &(Timestamp, Step<'_>), b: &(Timestamp, Step<'_>)) -> Ordering {
    a.0.cmp(&b.0).then_with(|| match (&a.1, &b.1) {
        (Step::Effect(x, _), Step::Effect(y, _)) => x.cmp(y),
        (Step::Effect(..), Step::Counter(..)) => Ordering::Less,
        (Step::Counter(..), Step::Effect(..)) => Ordering::Greater,
        (Step::Counter(x, _), Step::Counter(y, _)) => x.cmp(y),
    })
}

/// Whether `step` applies to `id` in `state`: an effect by its affected set ([`applies_to`])
/// and the existence of its generating ability ([`generator_exists`]); a linked
/// ([`Affected::SameAsLayer`]) part by whether its anchor applied to `id` earlier in this
/// evaluation (`locks`, CR 613.6); counters always. Used both by the real application and by
/// the dependency trial, so the two agree.
pub(super) fn step_applies(
    step: &Step<'_>,
    env: &dyn LayerEnvironment,
    id: ObjectId,
    state: &Evaluated,
    locks: &LockRecord<'_>,
) -> bool {
    match step {
        // CR 613.6: once the effect starts to apply in one layer, it continues to apply to the
        // same set of objects in each later layer and sublayer, even if the ability generating
        // it is removed during this process; so the generating ability is not consulted.
        Step::Effect(eid, effect) if matches!(effect.affected, Affected::SameAsLayer(_)) => {
            locks.linked_applies(*eid)
        }
        Step::Effect(eid, effect) => {
            applies_to(effect, env, id, state) && generator_exists(*eid, effect, env, id, state)
        }
        Step::Counter(..) => true,
    }
}

/// Whether the effect's generating ability still exists (CR 613.8a, 611.3, 113.6): asked of
/// the environment ([`LayerEnvironment::generating_ability_exists`]) only for a static effect
/// with a source, with the state so far when the source is the object `id` being evaluated.
/// An effect of a resolved spell or ability exists independently of its source's abilities
/// (CR 611.2); a static effect without a source is not checked.
fn generator_exists(
    eid: EffectId,
    effect: &ContinuousEffect,
    env: &dyn LayerEnvironment,
    id: ObjectId,
    state: &Evaluated,
) -> bool {
    match (&effect.origin, effect.source) {
        (EffectOrigin::Static { .. }, Some(source)) => {
            env.generating_ability_exists(eid, source, (source == id).then_some(state))
        }
        _ => true,
    }
}

/// The power and toughness adjustment `step` declares when applied to `from`, in `i128`
/// without saturation (CR 613.4c, 122.1a): a `ModifyPt` effect its two amounts, P/T counters
/// the amount times the per-counter change. `None` when the step declares no numeric P/T
/// adjustment: another modification, the object has no P/T pair, or P/T counters on a
/// non-creature (which do nothing, [`apply_counter`]). Reads the modification generically,
/// never card identity (R4); used by the dependency comparison so that saturation at the `i64`
/// bounds is not mistaken for a change in what the step does (docs/DECISIONS.md "#33 FP-032 —
/// Review round 1 dispositions").
pub(super) fn declared_pt_adjustment(step: &Step<'_>, from: &Scratch) -> Option<(i128, i128)> {
    from.pt?;
    match step {
        Step::Effect(_, effect) => match effect.modification {
            LayerModification::ModifyPt { power, toughness } => {
                Some((i128::from(power), i128::from(toughness)))
            }
            _ => None,
        },
        Step::Counter(kind, amount) => {
            if !from.state.characteristics.card_types.contains("Creature") {
                return None;
            }
            let (dp, dt) = pt_counter_delta(kind)?;
            let n = i128::from(*amount);
            Some((i128::from(dp) * n, i128::from(dt) * n))
        }
    }
}

/// Applies `step` to `scratch` (its affected set already checked).
pub(super) fn apply_step(scratch: &mut Scratch, step: &Step<'_>) {
    match step {
        Step::Effect(_, effect) => {
            apply_effect(&mut scratch.state, &mut scratch.pt, &effect.modification);
        }
        Step::Counter(kind, amount) => {
            apply_counter(&mut scratch.state, &mut scratch.pt, kind, *amount);
        }
    }
}

/// Evaluates `id`: its base characteristics through layers 1a, 1b, 2, 3, 4, 5, 6, 7a, 7b, 7c and
/// 7d (REQ-LAYER-001, CR 613.1). Pure: nothing is mutated, the base included. Equal to the
/// [`Evaluated`] of [`evaluate_with_order`].
///
/// Within a layer, steps apply in the REQ-LAYER-002 order ([`evaluate_with_order`]): with no
/// characteristic-defining effect and no dependency, ascending timestamp (CR 613.2, 613.3,
/// 613.4, 613.7); at equal timestamps effects come before counters, effects by ascending
/// [`EffectId`], counters by ascending kind. Keyword counters, variants included
/// ([`keyword_counter_keyword`]), apply in layer 6 (CR 122.1b, 613.1f), granting their full kind
/// string as the keyword; P/T counters apply in layer 7c (CR 122.1a, 613.4c), and change power
/// and toughness only when the object is a creature after layers 1-6 (a noncreature card with
/// printed P/T off the battlefield keeps its printed values; effects of layer 7 still apply to
/// it). Each counter kind applies at its counter timestamp. Layer 7 is the last layer.
///
/// # Errors
/// [`LayerError::UnknownObject`] if the environment has no base for `id`.
pub fn evaluate(
    effects: &ContinuousEffects,
    env: &dyn LayerEnvironment,
    id: ObjectId,
) -> Result<Evaluated, LayerError> {
    evaluate_with_order(effects, env, id).map(|(evaluated, _)| evaluated)
}

/// [`evaluate`], also returning the order in which the steps were used (REQ-LAYER-002).
///
/// Within each layer or sublayer: in layers 2-6, effects from characteristic-defining abilities
/// apply before all others (CR 613.3); then, repeatedly, the unapplied step that depends on no
/// other unapplied step and has the earliest ordering key applies, dependency being found by
/// trial application on a scratch copy (CR 613.8a, 613.8b); a dependency loop is applied in
/// timestamp order, one member at a time; the relation is recomputed after every application
/// (CR 613.8c). Counters are steps like effects, at their counter timestamp (CR 613.7c), and are
/// never characteristic-defining.
///
/// # Errors
/// [`LayerError::UnknownObject`] if the environment has no base for `id`.
pub fn evaluate_with_order(
    effects: &ContinuousEffects,
    env: &dyn LayerEnvironment,
    id: ObjectId,
) -> Result<(Evaluated, OrderRecord), LayerError> {
    // CR 613.1: start with the actual object.
    let base = env.base(id).ok_or(LayerError::UnknownObject(id))?;
    let on_battlefield = base.on_battlefield;
    // `pt` is `None` until layer 7 starts or when the object has no power and toughness.
    let mut scratch = Scratch {
        state: Evaluated {
            characteristics: base.characteristics,
            controller: base.controller,
        },
        pt: None,
    };
    let counters = env.counters(id);
    let counter_ts =
        |kind: &str| -> Timestamp { env.counter_timestamp(id, kind).unwrap_or(Timestamp::new(0)) };
    let mut record = OrderRecord {
        object: id,
        steps: Vec::new(),
    };
    // CR 613.6, 611.3a: the lock-in record of this evaluation only.
    let mut locks = LockRecord::new(effects);
    for &layer in Layer::ALL {
        if layer == Layer::L7a {
            scratch.pt = start_layer_7(&mut scratch.state, env, id, on_battlefield);
        }
        let mut steps: Vec<(Timestamp, Step<'_>)> = effects
            .iter()
            .filter(|(_, e)| e.layer == layer)
            .map(|(eid, e)| (e.timestamp, Step::Effect(eid, e)))
            .collect();
        for (kind, &amount) in counters.iter().filter(|&(_, &n)| n >= 1) {
            let applies = match layer {
                // CR 122.1b, 613.1f: keyword counters in layer 6.
                Layer::L6 => keyword_counter_keyword(kind).is_some(),
                // CR 122.1a, 613.4c: P/T counters in layer 7c.
                Layer::L7c => pt_counter_delta(kind).is_some(),
                _ => false,
            };
            if applies {
                // CR 613.7c: counters apply at their counter timestamp.
                steps.push((counter_ts(kind), Step::Counter(kind, amount)));
            }
        }
        // CR 613.2, 613.3, 613.4, 613.7, 613.8: the order within the layer or sublayer.
        run_layer(
            layer,
            steps,
            env,
            id,
            &mut scratch,
            &mut locks,
            &mut record.steps,
        );
    }
    Ok((scratch.state, record))
}

/// Evaluates every object of `env.objects()`, in that order; equal to [`evaluate`] for each.
///
/// # Errors
/// The first error of [`evaluate`].
pub fn evaluate_all(
    effects: &ContinuousEffects,
    env: &dyn LayerEnvironment,
) -> Result<BTreeMap<ObjectId, Evaluated>, LayerError> {
    let mut out = BTreeMap::new();
    for id in env.objects() {
        out.insert(id, evaluate(effects, env, id)?);
    }
    Ok(out)
}

/// Whether `effect` applies to `id` now: a locked set (CR 611.2c) or a selector matched against
/// the object as evaluated so far (CR 611.3a, CR 613.5).
fn applies_to(
    effect: &ContinuousEffect,
    env: &dyn LayerEnvironment,
    id: ObjectId,
    state: &Evaluated,
) -> bool {
    match &effect.affected {
        Affected::Objects(set) => set.contains(&id),
        Affected::Selector(selector) => env.matches(
            selector,
            &MatchContext {
                source: effect.source,
                effect_controller: effect.controller,
                candidate: id,
                candidate_view: state,
            },
        ),
        // Decided by the lock-in record in [`step_applies`] (CR 613.6).
        Affected::SameAsLayer(_) => false,
    }
}

/// The start of layer 7 (CR 613.4): whether the object has power and toughness after layers
/// 1-6 (CR 208.3, 208.5), and if so their starting values, written back as literals.
pub(super) fn start_layer_7(
    state: &mut Evaluated,
    env: &dyn LayerEnvironment,
    id: ObjectId,
    on_battlefield: bool,
) -> Option<(i64, i64)> {
    let c = &mut state.characteristics;
    if !c.pt_applies(on_battlefield) {
        // CR 208.3: no power and toughness.
        c.power = None;
        c.toughness = None;
        return None;
    }
    // CR 208.5: a missing value is 0; a CDA expression is evaluated by the environment.
    let value = |v: &Option<ValueExpressionAST>| -> i64 {
        v.as_ref().map_or(0, |e| {
            literal_of(e).unwrap_or_else(|| env.eval_value(e, id).unwrap_or(0))
        })
    };
    let pt = (value(&c.power), value(&c.toughness));
    write_pt(state, pt);
    Some(pt)
}

/// Writes `pt` into the characteristics as literals.
fn write_pt(state: &mut Evaluated, (p, t): (i64, i64)) {
    state.characteristics.power = Some(literal(p));
    state.characteristics.toughness = Some(literal(t));
}

/// Applies one modification to the object (CR 613.1a-g, 613.2a-b, 613.4a-d).
fn apply_effect(state: &mut Evaluated, pt: &mut Option<(i64, i64)>, m: &LayerModification) {
    let c = &mut state.characteristics;
    match m {
        // CR 613.2a, 613.2b: layer 1 replaces the characteristics (CR 613.2c: copiable values).
        LayerModification::CopyValues(v) | LayerModification::FaceDown(v) => {
            *c = (**v).clone();
        }
        // CR 613.1b
        LayerModification::SetController(p) => state.controller = Some(*p),
        // CR 613.1c
        LayerModification::SetText(t) => c.rules_text = t.clone(),
        // CR 613.1d
        LayerModification::AddTypes {
            supertypes,
            card_types,
            subtypes,
        } => {
            c.supertypes.extend(supertypes.iter().cloned());
            c.card_types.extend(card_types.iter().cloned());
            c.subtypes.extend(subtypes.iter().cloned());
        }
        // CR 613.1d
        LayerModification::SetTypes {
            supertypes,
            card_types,
            subtypes,
        } => {
            if let Some(s) = supertypes {
                c.supertypes.clone_from(s);
            }
            if let Some(s) = card_types {
                c.card_types.clone_from(s);
            }
            if let Some(s) = subtypes {
                c.subtypes.clone_from(s);
            }
        }
        // CR 613.1d
        LayerModification::RemoveSubtypes(s) => c.subtypes.retain(|t| !s.contains(t)),
        // CR 613.1e
        LayerModification::SetColors(m) => c.colors = *m,
        // CR 613.1e
        LayerModification::AddColors(m) => {
            c.colors = ColorBitmask::new(c.colors.bits() | m.bits()).unwrap_or(c.colors);
        }
        // CR 613.1f
        LayerModification::AddAbility(a) => c.abilities.push(a.clone()),
        // CR 613.1f
        LayerModification::RemoveAllAbilities => c.abilities.clear(),
        // CR 613.1f, 702.11e
        LayerModification::RemoveKeyword(k) => c.abilities.retain(|a| !is_keyword(a, k)),
        // CR 613.4a, 613.4b
        LayerModification::SetPt { power, toughness } => {
            if let Some((p, t)) = pt.as_mut() {
                *p = power.unwrap_or(*p);
                *t = toughness.unwrap_or(*t);
            }
            update_pt(state, *pt);
        }
        // CR 613.4c
        LayerModification::ModifyPt { power, toughness } => {
            if let Some((p, t)) = pt.as_mut() {
                *p = p.saturating_add(*power);
                *t = t.saturating_add(*toughness);
            }
            update_pt(state, *pt);
        }
        // CR 613.4d
        LayerModification::SwitchPt => {
            *pt = pt.map(|(p, t)| (t, p));
            update_pt(state, *pt);
        }
    }
}

/// Writes `pt` back when the object has power and toughness.
fn update_pt(state: &mut Evaluated, pt: Option<(i64, i64)>) {
    if let Some(pt) = pt {
        write_pt(state, pt);
    }
}

/// Applies the counters of one kind (CR 122.1a, 122.1b).
fn apply_counter(state: &mut Evaluated, pt: &mut Option<(i64, i64)>, kind: &str, amount: i64) {
    if let Some(keyword) = keyword_counter_keyword(kind) {
        // CR 122.1b, 613.1f: the object gains the keyword, once whatever the amount.
        state
            .characteristics
            .abilities
            .push(keyword_counter_ability(keyword));
    } else if let Some((dp, dt)) = pt_counter_delta(kind) {
        // CR 122.1a: a P/T counter modifies a creature or a creature card in another zone only.
        if !state.characteristics.card_types.contains("Creature") {
            return;
        }
        // CR 122.1a, 613.4c: X and Y per counter.
        if let Some((p, t)) = pt.as_mut() {
            *p = p.saturating_add(dp.saturating_mul(amount));
            *t = t.saturating_add(dt.saturating_mul(amount));
        }
        update_pt(state, *pt);
    }
}
