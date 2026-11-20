// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for REQ-LAYER-002 item 7 (with no dependencies and no CDAs, the order within
//! each layer is ascending timestamp, effects before counters, EffectId, counter kind; CR 613.7,
//! 613.7c) and item 8 (REQ-CORE-001 determinism: the registration order never changes the
//! evaluated object or the order record, with CDAs, dependencies and loops present; CR 613.3,
//! 613.8b). A seeded SplitMix64 generator written here (no clock, R3, R7). Every case also
//! checks `evaluate == evaluate_with_order(..).0` (item 9).

#[path = "spec_layers_ordering_support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::Timestamp;
use fineprint_core::layers::{
    ContinuousEffect, KEYWORD_COUNTER_KINDS, LayerModification, OrderItem, OrderRecord, OrderStep,
    Selection,
};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::ColorBitmask;
use support::*;

/// The P/T counter kinds used (layer 7c).
const PT_KINDS: [&str; 2] = ["+1/+1", "-1/-1"];

/// A random effect with a locked set (the subject or not) in one of layers 2-7d; never a CDA,
/// never a selector, so no 613.8a (b) relation exists between any two of them.
fn random_effect(rng: &mut Rng) -> ContinuousEffect {
    let t = rng.below(6) + 1;
    let affected = if rng.below(5) == 0 {
        elsewhere()
    } else {
        subject()
    };
    let n = rng.below(4);
    let (layer, m) = match rng.below(8) {
        0 => (Layer::L2, LayerModification::SetController(p(n + 1))),
        1 => (
            Layer::L3,
            LayerModification::SetText(text(&format!("test.text_{n}"))),
        ),
        2 => (Layer::L4, add_subtype(&format!("TestKind{n}"))),
        3 => (
            Layer::L5,
            LayerModification::AddColors(ColorBitmask::new(1 << n).unwrap()),
        ),
        4 => (Layer::L6, add_ability(&format!("test.kw_{n}"))),
        5 => {
            let v = i64::try_from(n).unwrap();
            (Layer::L7b, set_pt(v, v + 1))
        }
        6 => {
            let v = i64::try_from(n).unwrap() - 1;
            (Layer::L7c, modify_pt(v, 1))
        }
        _ => (Layer::L7d, LayerModification::SwitchPt),
    };
    eff(layer, t, affected, m)
}

/// The sort key of a step: timestamp, effects (0) before counters (1), EffectId, counter kind.
type Key = (Timestamp, u8, u64, String);

/// The expected record steps, layer by layer in `Layer::ALL` order, each layer ascending by
/// [`Key`]; every step chosen by Timestamp.
fn expected_steps(
    effects: &[(u64, ContinuousEffect)],
    counters: &BTreeMap<String, u64>,
) -> Vec<OrderStep> {
    let mut out = Vec::new();
    for &layer in Layer::ALL {
        let mut keyed: Vec<(Key, OrderStep)> = Vec::new();
        for (id, effect) in effects.iter().filter(|(_, e)| e.layer == layer) {
            let applied = effect.affected == subject();
            keyed.push((
                (effect.timestamp, 0, *id, String::new()),
                fx(layer, *id, Selection::Timestamp, applied),
            ));
        }
        for (kind, &t) in counters {
            let counter_layer = if PT_KINDS.contains(&kind.as_str()) {
                Layer::L7c
            } else {
                Layer::L6
            };
            if counter_layer == layer {
                keyed.push((
                    (ts(t), 1, 0, kind.clone()),
                    ctr(layer, kind, Selection::Timestamp, true),
                ));
            }
        }
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        out.extend(keyed.into_iter().map(|(_, s)| s));
    }
    out
}

/// REQ-LAYER-002, CR 613.7, 613.7c: for random effects and counters with no dependencies and no
/// CDAs, the record lists layers in `Layer::ALL` order, and within each layer the steps are in
/// ascending (timestamp, effects before counters, EffectId, counter kind) order, all chosen by
/// Timestamp; for every registration order tried.
#[test]
fn no_dependencies_order_equals_timestamp_order() {
    let mut rng = Rng::new(0x5EED_0032_0001);
    for case in 0..300 {
        let count = rng.below(12) + 1;
        let mut ids = BTreeSet::new();
        while u64::try_from(ids.len()).unwrap() < count {
            ids.insert(rng.below(1000) + 1);
        }
        let mut effects: Vec<(u64, ContinuousEffect)> = ids
            .into_iter()
            .map(|id| (id, random_effect(&mut rng)))
            .collect();
        // Counters: each kind present or not; timestamp 0 sometimes left to the default.
        let mut env = Env::subject(creature(3, 3));
        let mut counters: BTreeMap<String, u64> = BTreeMap::new();
        let kinds: Vec<&str> = PT_KINDS
            .iter()
            .copied()
            .chain(KEYWORD_COUNTER_KINDS.iter().take(3).copied())
            .collect();
        for kind in kinds {
            if rng.below(3) == 0 {
                let t = rng.below(7);
                let amount = i64::try_from(rng.below(3) + 1).unwrap();
                if t == 0 {
                    // No counter timestamp: treated as Timestamp 0.
                    env.counters
                        .entry(o(SUBJECT))
                        .or_default()
                        .insert(kind.to_owned(), amount);
                } else {
                    env = env.counter(kind, amount, t);
                }
                counters.insert(kind.to_owned(), t);
            }
        }
        let expected = expected_steps(&effects, &counters);
        let mut first: Option<(fineprint_core::layers::Evaluated, OrderRecord)> = None;
        for _ in 0..4 {
            rng.shuffle(&mut effects);
            let r = registry(&effects);
            let (ev, rec) = run(&r, &env);
            assert_layers_in_order(&rec);
            assert_eq!(rec.steps, expected, "case {case}: {effects:?}");
            match &first {
                None => first = Some((ev, rec)),
                Some((fev, frec)) => {
                    assert_eq!(&ev, fev, "case {case}");
                    assert_eq!(&rec, frec, "case {case}");
                }
            }
        }
    }
}

/// The determinism scenario: CDA-first (layers 2 and 5), a dependency (layer 4 type change),
/// an ability-removal dependency (layer 6), a three-effect loop (layer 4 markers), a CDA loop
/// (layer 3 is not used; layer 4 CDAs), layer 7 effects and counters.
fn mixed_scenario() -> (Vec<(u64, ContinuousEffect)>, Env) {
    let marker = |t: u64, unless: &str, mark: &str| {
        eff(
            Layer::L4,
            t,
            sel(not(has_subtype(unless))),
            add_subtype(mark),
        )
    };
    let cda_marker = |t: u64, unless: &str, mark: &str| {
        cda(
            Layer::L4,
            t,
            sel(not(has_subtype(unless))),
            add_subtype(mark),
        )
    };
    let effects = vec![
        (
            1,
            eff(
                Layer::L2,
                1,
                subject(),
                LayerModification::SetController(p(2)),
            ),
        ),
        (
            2,
            cda(
                Layer::L2,
                2,
                subject(),
                LayerModification::SetController(p(3)),
            ),
        ),
        (3, eff(Layer::L4, 3, subject(), add_card_type("Creature"))),
        (
            4,
            eff(Layer::L4, 1, sel(is_creature()), add_subtype("TestKind")),
        ),
        (5, marker(4, "Mark3", "Mark1")),
        (6, marker(5, "Mark1", "Mark2")),
        (7, marker(6, "Mark2", "Mark3")),
        (8, cda_marker(7, "CdaB", "CdaA")),
        (9, cda_marker(8, "CdaA", "CdaB")),
        (
            10,
            eff(
                Layer::L5,
                2,
                subject(),
                LayerModification::SetColors(ColorBitmask::GREEN),
            ),
        ),
        (
            11,
            cda(
                Layer::L5,
                3,
                subject(),
                LayerModification::SetColors(ColorBitmask::WHITE),
            ),
        ),
        (
            12,
            eff(
                Layer::L6,
                2,
                subject(),
                LayerModification::RemoveKeyword("test.marker".to_owned()),
            ),
        ),
        (
            13,
            eff(
                Layer::L6,
                1,
                sel(has_kw("test.marker")),
                add_ability("test.dependent_marker"),
            ),
        ),
        (14, eff(Layer::L7b, 2, subject(), set_pt(4, 4))),
        (15, eff(Layer::L7b, 1, subject(), set_pt(1, 1))),
        (16, eff(Layer::L7c, 3, subject(), modify_pt(1, 0))),
        (
            17,
            eff(Layer::L7d, 1, subject(), LayerModification::SwitchPt),
        ),
    ];
    let mut base = noncreature("Artifact");
    base.abilities = vec![keyword_ability("test.marker", "test.ability.marker")];
    let env = Env::subject(base)
        .counter("+1/+1", 2, 2)
        .counter(KEYWORD_COUNTER_KINDS[0], 1, 3);
    (effects, env)
}

/// REQ-LAYER-002, REQ-CORE-001, CR 613.3, 613.8b, 613.8c: registering the same effects in
/// different orders (seeded shuffles) yields an identical `Evaluated` and an identical
/// `OrderRecord`, with CDAs, dependencies and loops present; repeated evaluation is identical.
#[test]
fn registration_order_never_changes_result_or_record() {
    let (mut effects, env) = mixed_scenario();
    let (ev0, rec0) = run(&registry(&effects), &env);
    assert_layers_in_order(&rec0);
    assert_eq!(
        rec0.steps.len(),
        effects.len() + 2,
        "every effect and counter"
    );
    // Spot checks of the scenario: CDA-first in layer 2; the type-change dependency in layer 4.
    assert_eq!(ev0.controller, Some(p(2)));
    assert!(ev0.characteristics.subtypes.contains("TestKind"));
    assert_eq!(
        steps_in(&rec0, Layer::L2),
        vec![
            fx(Layer::L2, 2, Selection::CdaFirst, true),
            fx(Layer::L2, 1, Selection::Timestamp, true),
        ]
    );
    let mut rng = Rng::new(0x5EED_0032_0008);
    for round in 0..200 {
        rng.shuffle(&mut effects);
        let (ev, rec) = run(&registry(&effects), &env);
        assert_eq!(ev, ev0, "round {round}");
        assert_eq!(rec, rec0, "round {round}");
    }
    // Every item of the record appears exactly once.
    let items: BTreeSet<String> = rec0.steps.iter().map(|s| format!("{:?}", s.item)).collect();
    assert_eq!(items.len(), rec0.steps.len());
    assert!(
        rec0.steps
            .iter()
            .any(|s| s.item == OrderItem::Counter("+1/+1".to_owned()))
    );
}
