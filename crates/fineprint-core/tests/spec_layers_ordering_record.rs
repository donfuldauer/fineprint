// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for REQ-LAYER-002 item 9 and the shape of the order record: `evaluate` equals
//! `evaluate_with_order(..).0`, an unknown object is `LayerError::UnknownObject` for both, the
//! record names its object and lists every step of every layer in `Layer::ALL` order (CR 613.2,
//! 613.3, 613.4, 613.7). Black-box: written from the work-item brief and SRS 6.2 only.

#[path = "spec_layers_ordering_support.rs"]
mod support;

use fineprint_core::layers::{
    ContinuousEffects, LayerError, LayerModification, OrderItem, OrderRecord, OrderStep, Selection,
    evaluate, evaluate_with_order,
};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::ColorBitmask;
use support::*;

/// REQ-LAYER-002, CR 613.7: `evaluate` and `evaluate_with_order` on an object the environment
/// does not know both fail with `LayerError::UnknownObject`.
#[test]
fn unknown_object_is_an_error_for_both() {
    let r = registry(&[(1, eff(Layer::L4, 1, subject(), add_subtype("TestKind")))]);
    let env = Env::subject(creature(2, 2));
    assert_eq!(
        evaluate(&r, &env, o(77)),
        Err(LayerError::UnknownObject(o(77)))
    );
    assert_eq!(
        evaluate_with_order(&r, &env, o(77)),
        Err(LayerError::UnknownObject(o(77)))
    );
}

/// REQ-LAYER-002, CR 613.7: with no effects and no counters the record names the object and
/// has no steps, and the result is the base.
#[test]
fn empty_registry_gives_empty_record() {
    let env = Env::subject(creature(2, 2));
    let (ev, rec) = run(&ContinuousEffects::new(), &env);
    assert_eq!(
        rec,
        OrderRecord {
            object: o(SUBJECT),
            steps: Vec::new(),
        }
    );
    assert_eq!(ev.characteristics, creature(2, 2));
    assert_eq!(ev.controller, Some(p(1)));
}

/// REQ-LAYER-002, CR 613.2, 613.3, 613.4: one effect in every layer, registered in reverse
/// layer order with descending timestamps: the record lists them in `Layer::ALL` order (layer
/// order wins over timestamps), each by Timestamp, and an effect whose locked set excludes the
/// object is listed with `applied: false`.
#[test]
fn record_lists_every_layer_in_layer_order() {
    let mods: Vec<(Layer, LayerModification)> = vec![
        (
            Layer::L1a,
            LayerModification::CopyValues(Box::new(creature(3, 3))),
        ),
        (
            Layer::L1b,
            LayerModification::FaceDown(Box::new(creature(2, 2))),
        ),
        (Layer::L2, LayerModification::SetController(p(2))),
        (Layer::L3, LayerModification::SetText(text("test.text"))),
        (Layer::L4, add_subtype("TestKind")),
        (Layer::L5, LayerModification::AddColors(ColorBitmask::RED)),
        (Layer::L6, add_ability("test.kw")),
        (Layer::L7a, set_pt(1, 1)),
        (Layer::L7b, set_pt(2, 3)),
        (Layer::L7c, modify_pt(1, 1)),
        (Layer::L7d, LayerModification::SwitchPt),
    ];
    let n = u64::try_from(mods.len()).unwrap();
    let mut list = Vec::new();
    for (i, (layer, m)) in mods.into_iter().enumerate().rev() {
        let i = u64::try_from(i).unwrap();
        // 7a is CDA-only (CR 613.4a).
        let effect = if layer == Layer::L7a {
            cda(layer, n - i, subject(), m)
        } else {
            eff(layer, n - i, subject(), m)
        };
        list.push((i + 1, effect));
    }
    list.push((
        100,
        eff(Layer::L4, 1, elsewhere(), add_subtype("ElsewhereKind")),
    ));
    let r = registry(&list);
    let (ev, rec) = run(&r, &Env::subject(creature(5, 5)));
    let mut expected: Vec<OrderStep> = Layer::ALL
        .iter()
        .enumerate()
        .map(|(i, &layer)| {
            fx(
                layer,
                u64::try_from(i).unwrap() + 1,
                Selection::Timestamp,
                true,
            )
        })
        .collect();
    // The excluded layer-4 effect (timestamp 1) precedes the layer-4 effect (timestamp 7).
    expected.insert(
        4,
        OrderStep {
            layer: Layer::L4,
            item: OrderItem::Effect(e(100)),
            selection: Selection::Timestamp,
            applied: false,
        },
    );
    assert_eq!(rec.steps, expected);
    assert_eq!(ev.controller, Some(p(2)));
    assert_eq!(pt(&ev.characteristics), Some((4, 3)));
    assert!(!ev.characteristics.subtypes.contains("ElsewhereKind"));
}

/// REQ-LAYER-002, CR 613.7: with two objects, each object's record names it and `evaluate`
/// equals `evaluate_with_order(..).0` for each; a selector effect is recorded as applied only
/// for the object it matches.
#[test]
fn records_are_per_object() {
    let r = registry(&[(
        1,
        eff(Layer::L4, 1, sel(is_creature()), add_subtype("TestKind")),
    )]);
    let env = Env::subject(creature(2, 2)).with(o(2), noncreature("Artifact"));
    for (id, applied) in [(o(SUBJECT), true), (o(2), false)] {
        let (ev, rec) = evaluate_with_order(&r, &env, id).unwrap();
        assert_eq!(evaluate(&r, &env, id).unwrap(), ev);
        assert_eq!(rec.object, id);
        assert_eq!(
            rec.steps,
            vec![fx(Layer::L4, 1, Selection::Timestamp, applied)]
        );
        assert_eq!(ev.characteristics.subtypes.contains("TestKind"), applied);
    }
}

/// REQ-LAYER-002, CR 613.7c: counters alone are recorded as `OrderItem::Counter(kind)` steps of
/// their layer (keyword counters in layer 6, P/T counters in layer 7c), ascending by counter
/// timestamp then kind within a layer.
#[test]
fn counters_alone_are_recorded() {
    let kw = fineprint_core::layers::KEYWORD_COUNTER_KINDS[1];
    let env = Env::subject(creature(2, 2))
        .counter("+1/+1", 1, 5)
        .counter("-1/-1", 1, 5)
        .counter(kw, 1, 9);
    let (ev, rec) = run(&ContinuousEffects::new(), &env);
    assert_eq!(pt(&ev.characteristics), Some((2, 2)));
    assert!(has_keyword(&ev.characteristics, kw));
    assert_eq!(
        rec.steps,
        vec![
            ctr(Layer::L6, kw, Selection::Timestamp, true),
            ctr(Layer::L7c, "+1/+1", Selection::Timestamp, true),
            ctr(Layer::L7c, "-1/-1", Selection::Timestamp, true),
        ]
    );
}
