// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for REQ-LAYER-002 item 1 (characteristic-defining abilities first in layers 2-6,
//! CR 613.3), item 2 (no CDA-first rule in layer 1, CR 613.2, nor in layers 7b-7d, CR 613.4;
//! layer 7a in timestamp order among CDAs, CR 613.4a) and item 3e (a CDA and a non-CDA effect
//! are never dependent, CR 613.8a (c)). Black-box: written from the work-item brief and SRS 6.2
//! only. Every scenario also checks `evaluate == evaluate_with_order(..).0` (item 9).

#[path = "spec_layers_ordering_support.rs"]
mod support;

use fineprint_core::layers::{Affected, Evaluated, LayerModification, OrderRecord, Selection};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::ColorBitmask;
use support::*;

// ---------------------------------------------------------------------------------------------
// Item 1: CDA-first in layers 2-6 (CR 613.3)
// ---------------------------------------------------------------------------------------------

/// Registers a non-CDA effect (id 1, timestamp `n_ts`) and a CDA effect (id 2, timestamp
/// `c_ts`), both on the subject in `layer`, and returns the run.
fn pair(
    layer: Layer,
    n_ts: u64,
    n: LayerModification,
    c_ts: u64,
    c: LayerModification,
) -> (Evaluated, OrderRecord) {
    let r = registry(&[
        (1, eff(layer, n_ts, subject(), n)),
        (2, cda(layer, c_ts, subject(), c)),
    ]);
    let env = Env::subject(creature(2, 2));
    run(&r, &env)
}

/// The record of the layer shows the CDA (id 2) first, chosen as CdaFirst, then the non-CDA
/// (id 1) by timestamp.
fn assert_cda_first(record: &OrderRecord, layer: Layer) {
    assert_eq!(
        steps_in(record, layer),
        vec![
            fx(layer, 2, Selection::CdaFirst, true),
            fx(layer, 1, Selection::Timestamp, true),
        ]
    );
    assert_layers_in_order(record);
}

/// REQ-LAYER-002, CR 613.3: layer 2, an earlier non-CDA SetController and a later CDA
/// SetController; the CDA applies first, so the non-CDA value is the one left.
#[test]
fn layer_2_cda_first_non_cda_controller_wins() {
    let (ev, rec) = pair(
        Layer::L2,
        1,
        LayerModification::SetController(p(2)),
        2,
        LayerModification::SetController(p(3)),
    );
    assert_eq!(ev.controller, Some(p(2)));
    assert_cda_first(&rec, Layer::L2);
}

/// REQ-LAYER-002, CR 613.3: layer 3, an earlier non-CDA SetText and a later CDA SetText; the
/// CDA applies first, so the non-CDA text is the one left.
#[test]
fn layer_3_cda_first_non_cda_text_wins() {
    let (ev, rec) = pair(
        Layer::L3,
        1,
        LayerModification::SetText(text("test.text_non_cda")),
        2,
        LayerModification::SetText(text("test.text_cda")),
    );
    assert_eq!(ev.characteristics.rules_text, text("test.text_non_cda"));
    assert_cda_first(&rec, Layer::L3);
}

/// REQ-LAYER-002, CR 613.3: layer 4 AddTypes, an earlier non-CDA and a later CDA; the record
/// shows the CDA first and both types are added.
#[test]
fn layer_4_cda_first_add_types() {
    let (ev, rec) = pair(
        Layer::L4,
        1,
        add_subtype("TestKindN"),
        2,
        add_subtype("TestKindC"),
    );
    assert_eq!(ev.characteristics.subtypes, set(["TestKindC", "TestKindN"]));
    assert_cda_first(&rec, Layer::L4);
}

/// REQ-LAYER-002, CR 613.3: layer 4 SetTypes, an earlier non-CDA and a later CDA; the CDA
/// applies first, so the non-CDA subtypes are the ones left.
#[test]
fn layer_4_cda_first_non_cda_set_types_wins() {
    let (ev, rec) = pair(
        Layer::L4,
        1,
        set_subtypes("TestKindN"),
        2,
        set_subtypes("TestKindC"),
    );
    assert_eq!(ev.characteristics.subtypes, set(["TestKindN"]));
    assert_cda_first(&rec, Layer::L4);
}

/// REQ-LAYER-002, CR 613.3: layer 5 AddColors, an earlier non-CDA and a later CDA; the record
/// shows the CDA first and both colors are added.
#[test]
fn layer_5_cda_first_add_colors() {
    let (ev, rec) = pair(
        Layer::L5,
        1,
        LayerModification::AddColors(ColorBitmask::RED),
        2,
        LayerModification::AddColors(ColorBitmask::BLUE),
    );
    let expected =
        ColorBitmask::BLACK.bits() | ColorBitmask::RED.bits() | ColorBitmask::BLUE.bits();
    assert_eq!(ev.characteristics.colors.bits(), expected);
    assert_cda_first(&rec, Layer::L5);
}

/// REQ-LAYER-002, CR 613.3: layer 5 SetColors, an earlier non-CDA and a later CDA; the CDA
/// applies first, so the non-CDA color is the one left.
#[test]
fn layer_5_cda_first_non_cda_set_colors_wins() {
    let (ev, rec) = pair(
        Layer::L5,
        1,
        LayerModification::SetColors(ColorBitmask::GREEN),
        2,
        LayerModification::SetColors(ColorBitmask::WHITE),
    );
    assert_eq!(ev.characteristics.colors, ColorBitmask::GREEN);
    assert_cda_first(&rec, Layer::L5);
}

/// REQ-LAYER-002, CR 613.3: layer 6 AddAbility, an earlier non-CDA and a later CDA; the CDA's
/// ability is added first (it comes first in the ability list).
#[test]
fn layer_6_cda_first_add_ability() {
    let (ev, rec) = pair(
        Layer::L6,
        1,
        add_ability("test.kw_non_cda"),
        2,
        add_ability("test.kw_cda"),
    );
    let kws: Vec<&str> = ev
        .characteristics
        .abilities
        .iter()
        .filter_map(|a| a.0.get("keyword").and_then(|k| k.as_str()))
        .collect();
    assert_eq!(kws, vec!["test.kw_cda", "test.kw_non_cda"]);
    assert_cda_first(&rec, Layer::L6);
}

/// REQ-LAYER-002, CR 613.3: in each of layers 2-6, a CDA with an earlier timestamp than the
/// non-CDA also applies first and is recorded as CdaFirst (a non-CDA step of its layer was still
/// unapplied when it was chosen).
#[test]
fn layers_2_to_6_earlier_cda_is_still_cda_first() {
    let cases = [
        (
            Layer::L2,
            LayerModification::SetController(p(2)),
            LayerModification::SetController(p(3)),
        ),
        (
            Layer::L3,
            LayerModification::SetText(text("test.text_n")),
            LayerModification::SetText(text("test.text_c")),
        ),
        (
            Layer::L4,
            add_subtype("TestKindN"),
            add_subtype("TestKindC"),
        ),
        (
            Layer::L5,
            LayerModification::AddColors(ColorBitmask::RED),
            LayerModification::AddColors(ColorBitmask::BLUE),
        ),
        (
            Layer::L6,
            add_ability("test.kw_n"),
            add_ability("test.kw_c"),
        ),
    ];
    for (layer, n, c) in cases {
        let (_, rec) = pair(layer, 2, n, 1, c);
        assert_cda_first(&rec, layer);
    }
}

/// REQ-LAYER-002, CR 613.3, 613.7: several CDAs in one layer apply before the non-CDA effect
/// and in timestamp order among themselves; the non-CDA effect follows.
#[test]
fn layer_5_several_cdas_in_timestamp_order_then_non_cda() {
    for (c1_ts, c2_ts) in [(2, 3), (3, 2)] {
        let r = registry(&[
            (
                1,
                eff(
                    Layer::L5,
                    1,
                    subject(),
                    LayerModification::SetColors(ColorBitmask::GREEN),
                ),
            ),
            (
                2,
                cda(
                    Layer::L5,
                    c1_ts,
                    subject(),
                    LayerModification::SetColors(ColorBitmask::WHITE),
                ),
            ),
            (
                3,
                cda(
                    Layer::L5,
                    c2_ts,
                    subject(),
                    LayerModification::SetColors(ColorBitmask::BLUE),
                ),
            ),
        ]);
        let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
        let (first, second) = if c1_ts < c2_ts { (2, 3) } else { (3, 2) };
        assert_eq!(
            steps_in(&rec, Layer::L5),
            vec![
                fx(Layer::L5, first, Selection::CdaFirst, true),
                fx(Layer::L5, second, Selection::CdaFirst, true),
                fx(Layer::L5, 1, Selection::Timestamp, true),
            ]
        );
        assert_eq!(ev.characteristics.colors, ColorBitmask::GREEN);
    }
}

// ---------------------------------------------------------------------------------------------
// Item 2: no CDA-first in layer 1 (CR 613.2) and layers 7b-7d (CR 613.4); 7a (CR 613.4a)
// ---------------------------------------------------------------------------------------------

fn named(name: &str, power: i64, toughness: i64) -> Characteristics {
    let mut c = creature(power, toughness);
    c.names = set([name]);
    c
}

/// REQ-LAYER-002, CR 613.2, 613.7: layers 1a and 1b, an earlier non-CDA and a later effect
/// marked `is_cda`; `is_cda` does not reorder, timestamp order holds and the later one's values
/// are the ones left.
#[test]
fn layer_1_is_cda_does_not_reorder() {
    for layer in [Layer::L1a, Layer::L1b] {
        let m = |c: Characteristics| {
            if layer == Layer::L1a {
                LayerModification::CopyValues(Box::new(c))
            } else {
                LayerModification::FaceDown(Box::new(c))
            }
        };
        let r = registry(&[
            (1, eff(layer, 1, subject(), m(named("test.copy_n", 3, 3)))),
            (2, cda(layer, 2, subject(), m(named("test.copy_c", 4, 4)))),
        ]);
        let (ev, rec) = run(&r, &Env::subject(creature(2, 2)));
        assert_eq!(ev.characteristics.names, set(["test.copy_c"]));
        assert_eq!(pt(&ev.characteristics), Some((4, 4)));
        assert_eq!(
            steps_in(&rec, layer),
            vec![
                fx(layer, 1, Selection::Timestamp, true),
                fx(layer, 2, Selection::Timestamp, true),
            ]
        );
    }
}

/// REQ-LAYER-002, CR 613.4, 613.4b, 613.7: layer 7b, an earlier non-CDA SetPt and a later SetPt
/// marked `is_cda`; timestamp order holds, so the later value is left.
#[test]
fn layer_7b_is_cda_does_not_reorder() {
    let r = registry(&[
        (1, eff(Layer::L7b, 1, subject(), set_pt(2, 2))),
        (2, cda(Layer::L7b, 2, subject(), set_pt(5, 5))),
    ]);
    let (ev, rec) = run(&r, &Env::subject(creature(1, 1)));
    assert_eq!(pt(&ev.characteristics), Some((5, 5)));
    assert_eq!(
        steps_in(&rec, Layer::L7b),
        vec![
            fx(Layer::L7b, 1, Selection::Timestamp, true),
            fx(Layer::L7b, 2, Selection::Timestamp, true),
        ]
    );
}

/// REQ-LAYER-002, CR 613.4, 613.4c, 613.7: layer 7c, an earlier non-CDA ModifyPt and a later
/// ModifyPt marked `is_cda`; timestamp order holds in the record.
#[test]
fn layer_7c_is_cda_does_not_reorder() {
    let r = registry(&[
        (1, eff(Layer::L7c, 1, subject(), modify_pt(1, 0))),
        (2, cda(Layer::L7c, 2, subject(), modify_pt(0, 1))),
    ]);
    let (ev, rec) = run(&r, &Env::subject(creature(1, 1)));
    assert_eq!(pt(&ev.characteristics), Some((2, 2)));
    assert_eq!(
        steps_in(&rec, Layer::L7c),
        vec![
            fx(Layer::L7c, 1, Selection::Timestamp, true),
            fx(Layer::L7c, 2, Selection::Timestamp, true),
        ]
    );
}

/// REQ-LAYER-002, CR 613.4, 613.7: layer 7d, an earlier non-CDA SwitchPt (id 2) and a later
/// SwitchPt marked `is_cda` (id 1); timestamp order holds in the record.
#[test]
fn layer_7d_is_cda_does_not_reorder() {
    let r = registry(&[
        (
            2,
            eff(Layer::L7d, 1, subject(), LayerModification::SwitchPt),
        ),
        (
            1,
            cda(Layer::L7d, 2, subject(), LayerModification::SwitchPt),
        ),
    ]);
    let (ev, rec) = run(&r, &Env::subject(creature(1, 3)));
    assert_eq!(pt(&ev.characteristics), Some((1, 3)));
    assert_eq!(
        steps_in(&rec, Layer::L7d),
        vec![
            fx(Layer::L7d, 2, Selection::Timestamp, true),
            fx(Layer::L7d, 1, Selection::Timestamp, true),
        ]
    );
}

/// REQ-LAYER-002, CR 613.4a, 613.7: layer 7a, two CDA SetPt effects apply in timestamp order;
/// swapping the timestamps swaps the result.
#[test]
fn layer_7a_cdas_in_timestamp_order_both_orders() {
    for (t1, t2, expected_pt, first, second) in [(1, 2, (4, 4), 1, 2), (2, 1, (2, 2), 2, 1)] {
        let r = registry(&[
            (1, cda(Layer::L7a, t1, subject(), set_pt(2, 2))),
            (2, cda(Layer::L7a, t2, subject(), set_pt(4, 4))),
        ]);
        let (ev, rec) = run(&r, &Env::subject(creature(1, 1)));
        assert_eq!(pt(&ev.characteristics), Some(expected_pt));
        assert_eq!(
            steps_in(&rec, Layer::L7a),
            vec![
                fx(Layer::L7a, first, Selection::Timestamp, true),
                fx(Layer::L7a, second, Selection::Timestamp, true),
            ]
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Item 3e: a CDA and a non-CDA effect are never dependent (CR 613.8a (c))
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 613.8a, 613.3: layer 4, a CDA that applies only to creatures and a non-CDA
/// that makes the object a creature. Applying the non-CDA would change what the CDA applies to,
/// but by 613.8a (c) they are not dependent: the CDA applies first (to a noncreature, so it does
/// not apply), in both timestamp orders.
#[test]
fn cda_never_depends_on_non_cda_layer_4_both_orders() {
    for (c_ts, n_ts) in [(2, 1), (1, 2)] {
        let r = registry(&[
            (
                1,
                cda(
                    Layer::L4,
                    c_ts,
                    sel(is_creature()),
                    add_subtype("TestKindC"),
                ),
            ),
            (
                2,
                eff(Layer::L4, n_ts, subject(), add_card_type("Creature")),
            ),
        ]);
        let (ev, rec) = run(&r, &Env::subject(noncreature("Artifact")));
        assert_eq!(ev.characteristics.card_types, set(["Artifact", "Creature"]));
        assert!(!ev.characteristics.subtypes.contains("TestKindC"));
        assert_eq!(
            steps_in(&rec, Layer::L4),
            vec![
                fx(Layer::L4, 1, Selection::CdaFirst, false),
                fx(Layer::L4, 2, Selection::Timestamp, true),
            ]
        );
    }
}

/// REQ-LAYER-002, CR 613.8a, 613.3: layer 6, a CDA that applies only while the object has a
/// marker ability and a non-CDA that removes all abilities. They are not dependent (613.8a (c));
/// the CDA applies first and does apply, in both timestamp orders.
#[test]
fn cda_never_depends_on_non_cda_layer_6_both_orders() {
    for (c_ts, n_ts) in [(2, 1), (1, 2)] {
        let r = registry(&[
            (
                1,
                cda(
                    Layer::L6,
                    c_ts,
                    sel(has_kw("test.marker")),
                    add_ability("test.cda_granted"),
                ),
            ),
            (
                2,
                eff(
                    Layer::L6,
                    n_ts,
                    Affected::Selector(any()),
                    LayerModification::RemoveAllAbilities,
                ),
            ),
        ]);
        let mut base = creature(2, 2);
        base.abilities = vec![keyword_ability("test.marker", "test.ability.marker")];
        let (ev, rec) = run(&r, &Env::subject(base));
        assert!(ev.characteristics.abilities.is_empty());
        assert_eq!(
            steps_in(&rec, Layer::L6),
            vec![
                fx(Layer::L6, 1, Selection::CdaFirst, true),
                fx(Layer::L6, 2, Selection::Timestamp, true),
            ]
        );
    }
}

/// REQ-LAYER-002, CR 613.8a, 613.8b, 613.3: two CDAs can depend on each other (613.8a (c):
/// both are from CDAs). Layer 4: CDA id 1 (earlier) applies only to creatures, CDA id 2 (later)
/// makes the object a creature; id 2 applies first, then id 1 (which now applies), then the
/// non-CDA. Both CDAs are chosen while a non-CDA step is unapplied, so both are CdaFirst.
#[test]
fn two_cdas_can_depend_on_each_other() {
    let r = registry(&[
        (
            1,
            cda(Layer::L4, 1, sel(is_creature()), add_subtype("TestKindOne")),
        ),
        (2, cda(Layer::L4, 2, subject(), add_card_type("Creature"))),
        (3, eff(Layer::L4, 3, subject(), add_subtype("TestKindN"))),
    ]);
    let (ev, rec) = run(&r, &Env::subject(noncreature("Artifact")));
    assert_eq!(ev.characteristics.card_types, set(["Artifact", "Creature"]));
    assert_eq!(
        ev.characteristics.subtypes,
        set(["TestKindN", "TestKindOne"])
    );
    assert_eq!(
        steps_in(&rec, Layer::L4),
        vec![
            fx(Layer::L4, 2, Selection::CdaFirst, true),
            fx(Layer::L4, 1, Selection::CdaFirst, true),
            fx(Layer::L4, 3, Selection::Timestamp, true),
        ]
    );
}

/// REQ-LAYER-002, CR 613.8a, 613.8b: the same two dependent CDAs without a non-CDA effect in
/// the layer: the later CDA jumps ahead (Dependency), the dependent one follows (Timestamp).
#[test]
fn two_dependent_cdas_without_non_cda_record_dependency() {
    let r = registry(&[
        (
            1,
            cda(Layer::L4, 1, sel(is_creature()), add_subtype("TestKindOne")),
        ),
        (2, cda(Layer::L4, 2, subject(), add_card_type("Creature"))),
    ]);
    let (ev, rec) = run(&r, &Env::subject(noncreature("Artifact")));
    assert_eq!(ev.characteristics.subtypes, set(["TestKindOne"]));
    assert_eq!(
        steps_in(&rec, Layer::L4),
        vec![
            fx(Layer::L4, 2, Selection::Dependency, true),
            fx(Layer::L4, 1, Selection::Timestamp, true),
        ]
    );
}
