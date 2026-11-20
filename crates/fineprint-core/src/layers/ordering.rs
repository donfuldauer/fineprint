// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Ordering within a layer or sublayer (REQ-LAYER-002, CR 613.2-613.4, 613.7, 613.8):
//! characteristic-defining abilities first in layers 2-6, dependency found by trial application
//! on a scratch copy, earliest-timestamp selection among the candidates, dependency loops applied
//! in timestamp order, and the order recomputed after every application.

use std::collections::BTreeSet;

use fineprint_base::ids::{EffectId, ObjectId, Timestamp};
use fineprint_dsl::ability::Layer;

use super::pipeline::{
    Evaluated, LayerEnvironment, Step, apply_step, declared_pt_adjustment, step_applies, step_order,
};
use crate::objects::Characteristics;

/// The order in which the steps of one object's evaluation were used (REQ-LAYER-002). Plain,
/// owned data, ready to be carried by a rules-trace event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderRecord {
    /// The object evaluated.
    pub object: ObjectId,
    /// Every step in the order it was used, layers in [`Layer::ALL`] order.
    pub steps: Vec<OrderStep>,
}

/// One step of an [`OrderRecord`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderStep {
    /// The layer or sublayer the step belongs to.
    pub layer: Layer,
    /// The effect or counter kind.
    pub item: OrderItem,
    /// Why the step was chosen when it was.
    pub selection: Selection,
    /// `false` when the effect's affected set did not include the object at that point, or its
    /// generating ability no longer existed (CR 613.8a); the step was still part of the order.
    /// Counters always apply.
    pub applied: bool,
}

/// What a step is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderItem {
    /// A continuous effect.
    Effect(EffectId),
    /// The counters of this kind on the object (CR 613.7c).
    Counter(String),
}

/// Why a step was chosen when it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    /// It was the earliest unapplied step of its layer (CR 613.7).
    Timestamp,
    /// It is a characteristic-defining effect chosen in layers 2-6 while a non-CDA step of the
    /// layer was still unapplied (CR 613.3).
    CdaFirst,
    /// An unapplied step with an earlier ordering key existed: it was released by dependency
    /// (CR 613.8b).
    Dependency,
    /// It was the earliest member of a dependency loop (CR 613.8b).
    Loop,
}

/// The object's state during evaluation: the evaluated characteristics and controller, and the
/// P/T pair once layer 7 has started (`None` before, or when it has no power and toughness).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Scratch {
    /// The object as evaluated so far.
    pub state: Evaluated,
    /// The P/T pair in layer 7.
    pub pt: Option<(i64, i64)>,
}

/// One step and its ordering key's timestamp.
pub(super) type Keyed<'a> = (Timestamp, Step<'a>);

/// Whether CR 613.3 (CDAs first) applies in `layer`: layers 2-6 only. Layer 7a holds only CDAs
/// (CR 613.4a) and layer 1 and 7b-7d have no CDA-first rule (CR 613.2, 613.4).
fn cda_first_layer(layer: Layer) -> bool {
    matches!(
        layer,
        Layer::L2 | Layer::L3 | Layer::L4 | Layer::L5 | Layer::L6
    )
}

/// Whether a step is from a characteristic-defining ability; counters are not (CR 613.7c).
fn is_cda(step: &Step<'_>) -> bool {
    match step {
        Step::Effect(_, e) => e.is_cda,
        Step::Counter(..) => false,
    }
}

/// The record item of a step.
fn item_of(step: &Step<'_>) -> OrderItem {
    match step {
        Step::Effect(id, _) => OrderItem::Effect(*id),
        Step::Counter(kind, _) => OrderItem::Counter((*kind).to_owned()),
    }
}

/// Applies every step of one layer to `scratch` in the REQ-LAYER-002 order and appends each
/// step to `out` as it is used.
///
/// CR 613.3: in layers 2-6 the CDA steps form a first group and the others a second; elsewhere
/// all steps form one group. Within a group (CR 613.8b, 613.8c): let U be the unapplied steps;
/// compute the dependency relation over U against the current state; apply the candidate (a step
/// of U that depends on no other step of U) with the earliest ordering key; if there is none,
/// apply the earliest member of the dependency loop found by [`find_loop`]; recompute; repeat.
pub(super) fn run_layer(
    layer: Layer,
    mut steps: Vec<Keyed<'_>>,
    env: &dyn LayerEnvironment,
    id: ObjectId,
    scratch: &mut Scratch,
    out: &mut Vec<OrderStep>,
) {
    // CR 613.7: index order is ordering-key order (timestamp, effects before counters, EffectId,
    // counter kind); registration order never matters.
    steps.sort_by(step_order);
    let cda: Vec<bool> = steps.iter().map(|(_, s)| is_cda(s)).collect();
    let cda_first = cda_first_layer(layer);
    let all: Vec<usize> = (0..steps.len()).collect();
    let groups: Vec<Vec<usize>> = if cda_first {
        // CR 613.3: effects from characteristic-defining abilities first.
        let (first, second): (Vec<usize>, Vec<usize>) = all.into_iter().partition(|&i| cda[i]);
        vec![first, second]
    } else {
        vec![all]
    };
    let mut applied = vec![false; steps.len()];
    for group in groups {
        let mut remaining = group;
        while !remaining.is_empty() {
            // CR 613.8c: the relation is recomputed against the current state each time.
            let deps = dependencies(&steps, &remaining, env, id, scratch);
            let candidate = (0..remaining.len()).find(|&i| !deps[i].iter().any(|&d| d));
            let (pick, from_loop) = match candidate {
                // CR 613.8b: the earliest independent step.
                Some(i) => (i, false),
                // CR 613.8b: a dependency loop is applied in timestamp order.
                None => (find_loop(&deps), true),
            };
            let index = remaining.remove(pick);
            let selection = if from_loop {
                Selection::Loop
            } else if cda_first && cda[index] && (0..steps.len()).any(|j| !applied[j] && !cda[j]) {
                Selection::CdaFirst
            } else if (0..index).any(|j| !applied[j]) {
                Selection::Dependency
            } else {
                Selection::Timestamp
            };
            let step = &steps[index].1;
            let applies = step_applies(step, env, id, &scratch.state);
            if applies {
                apply_step(scratch, step);
            }
            applied[index] = true;
            out.push(OrderStep {
                layer,
                item: item_of(step),
                selection,
                applied: applies,
            });
        }
    }
}

/// The dependency relation over `remaining` (indices into `steps`, ascending key):
/// `result[i][j]` is whether `remaining[i]` depends on `remaining[j]` against `scratch`.
///
/// CR 613.8a: B depends on A iff (a) both are in this layer (they are), (b) applying A to a
/// scratch copy of the current state changes whether B applies to the object (its existence and
/// what it applies to) or what B does to it (its output, which carries its text), and (c) both or
/// neither are from characteristic-defining abilities. The real state is never touched.
fn dependencies(
    steps: &[Keyed<'_>],
    remaining: &[usize],
    env: &dyn LayerEnvironment,
    id: ObjectId,
    scratch: &Scratch,
) -> Vec<Vec<bool>> {
    let n = remaining.len();
    let mut deps = vec![vec![false; n]; n];
    if n < 2 {
        return deps;
    }
    // B's behaviour without A: whether it applies, and its output if it does.
    let without: Vec<Option<Scratch>> = remaining
        .iter()
        .map(|&b| trial(&steps[b].1, env, id, scratch))
        .collect();
    for (j, &a) in remaining.iter().enumerate() {
        let step_a = &steps[a].1;
        // CR 613.8a (b): apply A to a scratch copy.
        let Some(after_a) = trial(step_a, env, id, scratch) else {
            continue;
        };
        if after_a == *scratch {
            // A changes nothing, so it changes nothing about B.
            continue;
        }
        for (i, &b) in remaining.iter().enumerate() {
            let step_b = &steps[b].1;
            // CR 613.8a (c): a CDA and a non-CDA never depend on each other.
            if i == j || is_cda(step_a) != is_cda(step_b) {
                continue;
            }
            let with = trial(step_b, env, id, &after_a);
            deps[i][j] = match (&without[i], &with) {
                // B applies (or exists) in neither state: nothing changed.
                (None, None) => false,
                // CR 613.8a (b): its output differs.
                (Some(out0), Some(out1)) => !same_output(
                    (&scratch.state, &out0.state, &after_a.state, &out1.state),
                    declared_pt_adjustment(step_b, scratch),
                    declared_pt_adjustment(step_b, &after_a),
                ),
                // CR 613.8a (b): its existence or what it applies to changed.
                _ => true,
            };
        }
    }
    deps
}

/// `step` applied to a clone of `from`, or `None` if it does not apply to the object there.
fn trial(
    step: &Step<'_>,
    env: &dyn LayerEnvironment,
    id: ObjectId,
    from: &Scratch,
) -> Option<Scratch> {
    if !step_applies(step, env, id, &from.state) {
        return None;
    }
    let mut copy = from.clone();
    apply_step(&mut copy, step);
    Some(copy)
}

/// The dependency loop to break when no step is a candidate (CR 613.8b), as an index into the
/// relation: the earliest member of the earliest loop.
///
/// A loop is a strongly connected component of the dependency graph whose members depend on no
/// step outside it (no incoming edge from outside; an engine interpretation, docs/DECISIONS.md
/// "#33 FP-032 — Dependency loop definition"). With several, the one containing the earliest
/// step is taken. Because indices are in key order, the first index whose component is such a
/// loop is that loop's earliest member.
fn find_loop(deps: &[Vec<bool>]) -> usize {
    let n = deps.len();
    // reach[i][j]: i depends on j, directly or through other steps.
    let mut reach: Vec<Vec<bool>> = deps.to_vec();
    for k in 0..n {
        let through = reach[k].clone();
        for row in &mut reach {
            if row[k] {
                for (cell, &via) in row.iter_mut().zip(&through) {
                    *cell |= via;
                }
            }
        }
    }
    (0..n)
        .find(|&i| {
            let component: BTreeSet<usize> = (0..n)
                .filter(|&j| j == i || (reach[i][j] && reach[j][i]))
                .collect();
            component.len() > 1
                && component
                    .iter()
                    .all(|&m| (0..n).all(|d| !deps[m][d] || component.contains(&d)))
        })
        // Unreachable when there is no candidate (a finite graph where every node has an
        // outgoing edge has such a component); the earliest step keeps the result defined.
        .unwrap_or(0)
}

/// Whether B did the same thing to the object in the state without A (`before0` to `after0`)
/// and in the state with A (`before1` to `after1`), field by field (CR 613.8a (b)).
///
/// A field counts as unchanged in what B does when B leaves it with the same value in both, or
/// leaves it untouched in both, or makes the same change to it: for a set or the colors, each
/// element is kept as it was in both or left present (or absent) in both; for the ability list,
/// each ability's number of copies is the same in both or changed by the same amount; for a
/// numeric power or toughness adjustment, the same amount is declared in both; for the P/T pair,
/// it is switched in both. The comparison reads only the object's characteristics and controller,
/// never which modification B is (REQ-DSL-013), with one exception: when B declares a numeric
/// P/T adjustment in either state (`declared0` without A, `declared1` with A, from
/// [`declared_pt_adjustment`]), power and toughness count as unchanged exactly when the two
/// declarations are equal, so that saturation at the `i64` bounds is not taken for a different
/// amount added (docs/DECISIONS.md "#33 FP-032 — Review round 1 dispositions").
fn same_output(
    (before0, after0, before1, after1): (&Evaluated, &Evaluated, &Evaluated, &Evaluated),
    declared0: Option<(i128, i128)>,
    declared1: Option<(i128, i128)>,
) -> bool {
    let (b0, a0, b1, a1) = (
        &before0.characteristics,
        &after0.characteristics,
        &before1.characteristics,
        &after1.characteristics,
    );
    // Every field is listed so that a new characteristic cannot be forgotten.
    let Characteristics {
        names,
        mana_cost,
        color_indicator,
        colors,
        supertypes,
        card_types,
        subtypes,
        rules_text,
        abilities,
        power: _,
        toughness: _,
        loyalty,
        defense,
        hand_modifier,
        life_modifier,
    } = a0;
    scalar(
        &before0.controller,
        &after0.controller,
        &before1.controller,
        &after1.controller,
    ) && set(&b0.names, names, &b1.names, &a1.names)
        && scalar(&b0.mana_cost, mana_cost, &b1.mana_cost, &a1.mana_cost)
        && scalar(
            &b0.color_indicator,
            color_indicator,
            &b1.color_indicator,
            &a1.color_indicator,
        )
        && bits(
            b0.colors.bits(),
            colors.bits(),
            b1.colors.bits(),
            a1.colors.bits(),
        )
        && set(&b0.supertypes, supertypes, &b1.supertypes, &a1.supertypes)
        && set(&b0.card_types, card_types, &b1.card_types, &a1.card_types)
        && set(&b0.subtypes, subtypes, &b1.subtypes, &a1.subtypes)
        && scalar(&b0.rules_text, rules_text, &b1.rules_text, &a1.rules_text)
        && list(&b0.abilities, abilities, &b1.abilities, &a1.abilities)
        && match (declared0, declared1) {
            (None, None) => power_toughness(b0, a0, b1, a1),
            // CR 613.4c, 613.8a (b): the declared adjustments, not the saturated writes.
            (d0, d1) => d0 == d1,
        }
        && scalar(&b0.loyalty, loyalty, &b1.loyalty, &a1.loyalty)
        && scalar(&b0.defense, defense, &b1.defense, &a1.defense)
        && scalar(
            &b0.hand_modifier,
            hand_modifier,
            &b1.hand_modifier,
            &a1.hand_modifier,
        )
        && scalar(
            &b0.life_modifier,
            life_modifier,
            &b1.life_modifier,
            &a1.life_modifier,
        )
}

/// Same value after B in both states, or untouched by B in both.
fn scalar<T: PartialEq>(before0: &T, after0: &T, before1: &T, after1: &T) -> bool {
    after0 == after1 || (before0 == after0 && before1 == after1)
}

/// Whether one per-element action explains both observations of an element: B kept it as it
/// was in both states, or B left it present in both, or absent in both.
fn element(before0: bool, after0: bool, before1: bool, after1: bool) -> bool {
    after0 == after1 || (before0 == after0 && before1 == after1)
}

/// Element by element ([`element`]) over every element of the four sets.
fn set(
    before0: &BTreeSet<String>,
    after0: &BTreeSet<String>,
    before1: &BTreeSet<String>,
    after1: &BTreeSet<String>,
) -> bool {
    let all: BTreeSet<&String> = before0
        .iter()
        .chain(after0)
        .chain(before1)
        .chain(after1)
        .collect();
    all.into_iter().all(|x| {
        element(
            before0.contains(x),
            after0.contains(x),
            before1.contains(x),
            after1.contains(x),
        )
    })
}

/// Bit by bit ([`element`]) over the color bits.
fn bits(before0: u8, after0: u8, before1: u8, after1: u8) -> bool {
    (0..8).all(|k| {
        let has = |v: u8| v & (1 << k) != 0;
        element(has(before0), has(after0), has(before1), has(after1))
    })
}

/// Ability by ability, counting copies: for every distinct ability of the four lists, B left
/// the same number of copies in both states or added (or removed) the same number in both.
fn list<T: PartialEq>(before0: &[T], after0: &[T], before1: &[T], after1: &[T]) -> bool {
    let count = |v: &[T], x: &T| v.iter().filter(|y| *y == x).count();
    before0
        .iter()
        .chain(after0)
        .chain(before1)
        .chain(after1)
        .all(|x| {
            let (b0, a0, b1, a1) = (
                count(before0, x),
                count(after0, x),
                count(before1, x),
                count(after1, x),
            );
            // Same count after, or the same change (a0 - b0 == a1 - b1, without subtraction).
            a0 == a1 || a0 + b1 == a1 + b0
        })
}

/// Power and toughness when B declares no numeric adjustment (a setter, a switch, a copy):
/// each [`scalar`], or switched in both states. "The same amount added" is decided by the
/// declared adjustment in [`same_output`], never by the saturated values.
fn power_toughness(
    b0: &Characteristics,
    a0: &Characteristics,
    b1: &Characteristics,
    a1: &Characteristics,
) -> bool {
    let each = scalar(&b0.power, &a0.power, &b1.power, &a1.power)
        && scalar(&b0.toughness, &a0.toughness, &b1.toughness, &a1.toughness);
    let switched =
        |b: &Characteristics, a: &Characteristics| a.power == b.toughness && a.toughness == b.power;
    each || (switched(b0, a0) && switched(b1, a1))
}
