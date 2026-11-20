// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for merged permanents in layer 1a (REQ-OBJ-002, REQ-LAYER-001; CR 731.2,
//! 731.2a, 613.2a, 613.7b, 702.140e): the merge effect, a change of top component, merging
//! under, timestamps against copy effects, later layers on top, copies of mutated permanents
//! and the merged mana value. Black-box: written from the work-item brief only.

#[path = "spec_layers_copy_support.rs"]
mod support;

use fineprint_core::layers::{
    Affected, ContinuousEffects, CopiableValues, EffectOrigin, LayerModification, evaluate,
    merge_effect,
};
use fineprint_core::layouts::{LayoutQuery, MergeSide, MergedComponent, MergedPermanent};
use fineprint_core::objects::{Characteristics, ManaValueContext};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::CardDefinitionDSL;
use support::*;

const X: u64 = 1;

fn comp(def: &CardDefinitionDSL) -> MergedComponent<'_> {
    MergedComponent {
        def,
        query: LayoutQuery::new(ZoneKind::Battlefield),
        is_token: false,
        face_up: true,
    }
}

/// The characteristics of a single-face synthetic definition (its MAIN face).
fn single(def: &CardDefinitionDSL) -> Characteristics {
    assert_eq!(def.faces.len(), 1, "single-face synthetic definition");
    chars_of(&def.faces[0])
}

/// `top`'s characteristics with the abilities of `all` (top to bottom).
fn top_with_all_abilities(top: &CardDefinitionDSL, all: &[&CardDefinitionDSL]) -> Characteristics {
    let mut c = single(top);
    c.abilities = all.iter().flat_map(|d| single(d).abilities).collect();
    c
}

/// The environment: object X is the permanent that was mutated onto (the base component).
fn env() -> TestEnv {
    let mut e = TestEnv::new();
    e.put(oid(X), single(&merge_base_def()));
    e.put(oid(2), own_b());
    e
}

/// REQ-OBJ-002, REQ-LAYER-001, CR 731.2a, 613.2a, 613.7b: the merge effect is a resolved
/// layer-1a copiable effect with the merge's timestamp, applied to the merged object.
#[test]
fn merge_effect_731_2a_shape() {
    let (m, b) = (mutant_def(), merge_base_def());
    for mutated in [false, true] {
        let mp = MergedPermanent::new(vec![comp(&m), comp(&b)], mutated).unwrap();
        let e = merge_effect(&mp, vocab(), oid(X), P2, ts(33)).expect("merge effect");
        assert_eq!(e.layer, Layer::L1a);
        assert_eq!(e.timestamp, ts(33));
        assert_eq!(e.affected, Affected::Objects(oset(&[oid(X)])));
        assert_eq!(e.source, None);
        assert_eq!(e.controller, P2);
        assert_eq!(e.origin, EffectOrigin::Resolved);
        assert!(!e.is_cda);
        let values = CopiableValues::merged(&mp, vocab()).unwrap();
        assert_eq!(
            e.modification,
            LayerModification::CopyValues(Box::new(values.characteristics.clone()))
        );
        assert_eq!(
            values.characteristics,
            mp.base_characteristics(vocab()).unwrap()
        );
    }
}

/// REQ-OBJ-002, CR 731.2a: a merged (not mutated) permanent has only its topmost component's
/// characteristics.
#[test]
fn merged_731_2a_characteristics_are_the_tops() {
    let (m, b) = (mutant_def(), merge_base_def());
    let mp = MergedPermanent::new(vec![comp(&m), comp(&b)], false).unwrap();
    let effects = registry(&[(1, merge_effect(&mp, vocab(), oid(X), P1, ts(10)).unwrap())]);
    assert_eq!(eval_chars(&effects, &env(), oid(X)), norm(&single(&m)));
    let e = evaluate(&effects, &env(), oid(X)).unwrap();
    assert_eq!(pt(&e), spt(4, 4));
}

/// REQ-OBJ-002, CR 731.2a, 613.7b (acceptance criterion: merged permanent characteristics
/// change when the top component changes): merging a new component over the permanent, with a
/// later timestamp, gives the new top's characteristics, whether the old merge effect is
/// removed or kept.
#[test]
fn merged_731_2a_characteristics_change_when_the_top_changes() {
    let (m, b, n) = (mutant_def(), merge_base_def(), mutant_new_def());
    for mutated in [false, true] {
        let mp = MergedPermanent::new(vec![comp(&m), comp(&b)], mutated).unwrap();
        let old = merge_effect(&mp, vocab(), oid(X), P1, ts(10)).unwrap();
        let mp2 = mp.merge(comp(&n), MergeSide::Over);
        let new = merge_effect(&mp2, vocab(), oid(X), P1, ts(20)).unwrap();
        let want = if mutated {
            top_with_all_abilities(&n, &[&n, &m, &b])
        } else {
            single(&n)
        };

        let mut effects = registry(&[(1, old.clone())]);
        let before = eval_chars(&effects, &env(), oid(X));
        assert_eq!(before.names, single(&m).names);

        // Old effect kept: the later timestamp wins in layer 1a.
        effects.register(eid(2), new.clone()).unwrap();
        assert_eq!(eval_chars(&effects, &env(), oid(X)), norm(&want), "kept");
        assert_ne!(eval_chars(&effects, &env(), oid(X)), before);

        // Old effect removed.
        assert!(effects.remove(eid(1)));
        assert_eq!(eval_chars(&effects, &env(), oid(X)), norm(&want), "removed");
        assert_eq!(pt(&evaluate(&effects, &env(), oid(X)).unwrap()), spt(5, 5));
    }
}

/// REQ-OBJ-002, CR 731.2a, 702.140e: merging under keeps the top's characteristics; mutated,
/// the abilities grow to every component's, top to bottom; not mutated, they do not.
#[test]
fn merged_under_keeps_top_and_mutate_unions_abilities() {
    let (m, b, u) = (mutant_def(), merge_base_def(), merge_under_def());
    // Not mutated.
    let mp = MergedPermanent::new(vec![comp(&m), comp(&b)], false).unwrap();
    let mp3 = mp.merge(comp(&u), MergeSide::Under);
    let effects = registry(&[
        (1, merge_effect(&mp, vocab(), oid(X), P1, ts(10)).unwrap()),
        (2, merge_effect(&mp3, vocab(), oid(X), P1, ts(20)).unwrap()),
    ]);
    let got = eval_chars(&effects, &env(), oid(X));
    assert_eq!(got, norm(&single(&m)));
    assert_eq!(got.abilities, single(&m).abilities);

    // Mutated.
    let mp = MergedPermanent::new(vec![comp(&m), comp(&b)], true).unwrap();
    let first = merge_effect(&mp, vocab(), oid(X), P1, ts(10)).unwrap();
    let effects = registry(&[(1, first)]);
    assert_eq!(
        eval_chars(&effects, &env(), oid(X)),
        norm(&top_with_all_abilities(&m, &[&m, &b]))
    );
    let mp3 = mp.merge(comp(&u), MergeSide::Under);
    let mut effects = effects;
    effects
        .register(
            eid(2),
            merge_effect(&mp3, vocab(), oid(X), P1, ts(20)).unwrap(),
        )
        .unwrap();
    let got = eval_chars(&effects, &env(), oid(X));
    let want = top_with_all_abilities(&m, &[&m, &b, &u]);
    assert_eq!(got, norm(&want));
    assert_eq!(got.names, single(&m).names);
    assert_eq!(got.abilities.len(), 3);
}

/// REQ-OBJ-002, CR 613.2, 613.7b, 731.2a: a merge effect and a copy effect on the same object
/// apply in timestamp order: the later one's values win, in both orders.
#[test]
fn merged_timestamp_order_against_a_copy_effect_both_ways() {
    let (m, b) = (mutant_def(), merge_base_def());
    let mp = MergedPermanent::new(vec![comp(&m), comp(&b)], true).unwrap();
    let merged_values = top_with_all_abilities(&m, &[&m, &b]);
    let copied = CopiableValues::plain(other_d()).copy_with(&single(&b), &[]);
    for (merge_ts, copy_ts, want) in [(10, 20, other_d()), (20, 10, merged_values.clone())] {
        for (merge_id, copy_id) in [(1, 2), (2, 1)] {
            let effects = registry(&[
                (
                    merge_id,
                    merge_effect(&mp, vocab(), oid(X), P1, ts(merge_ts)).unwrap(),
                ),
                (copy_id, copy_eff(&copied, copy_ts, &[oid(X)])),
            ]);
            assert_eq!(
                eval_chars(&effects, &env(), oid(X)),
                norm(&want),
                "merge {merge_ts} copy {copy_ts} ids {merge_id}/{copy_id}"
            );
        }
    }
}

/// REQ-OBJ-002, REQ-LAYER-001, CR 613.2a, 613.1d, 613.4c: the merge effect applies in layer 1a,
/// so layer-4 type additions, layer-7c modifications and counters still apply on top of the
/// merged values, whatever their timestamps.
#[test]
fn merged_later_layers_and_counters_apply_on_top() {
    let (m, b) = (mutant_def(), merge_base_def());
    let mp = MergedPermanent::new(vec![comp(&m), comp(&b)], false).unwrap();
    let mut e = env();
    e.counter(oid(X), "+1/+1", 2, 1);
    let effects = registry(&[
        (
            1,
            eff(Layer::L4, 1, objs(&[oid(X)]), add_card_types(&["Artifact"])),
        ),
        (2, eff(Layer::L7c, 2, objs(&[oid(X)]), modify_pt(1, 1))),
        (3, merge_effect(&mp, vocab(), oid(X), P1, ts(30)).unwrap()),
    ]);
    let got = evaluate(&effects, &e, oid(X)).unwrap();
    assert_eq!(
        got.characteristics.card_types,
        set(&["Artifact", "Creature"])
    );
    assert_eq!(got.characteristics.names, single(&m).names);
    assert_eq!(pt(&got), spt(4 + 1 + 2, 4 + 1 + 2));
}

/// REQ-OBJ-002, CR 731.2a, 702.140e, 613.2c, Orchestrator decision (#38): a copy of a mutated
/// permanent has the top component's characteristics plus every component's abilities.
#[test]
fn merged_copy_of_a_mutated_permanent_has_all_abilities() {
    let (m, b, u) = (mutant_def(), merge_base_def(), merge_under_def());
    let mp = MergedPermanent::new(vec![comp(&m), comp(&b), comp(&u)], true).unwrap();
    let vm = CopiableValues::merged(&mp, vocab()).unwrap();
    let want = top_with_all_abilities(&m, &[&m, &b, &u]);
    assert_eq!(vm.characteristics, want);
    let cp = vm.copy_with(&own_b(), &[]);
    assert_eq!(cp.characteristics, want);

    // Through the layers: copying the merged object's copiable characteristics.
    let mut effects = registry(&[(1, merge_effect(&mp, vocab(), oid(X), P1, ts(10)).unwrap())]);
    let from_layers = CopiableValues::plain(
        fineprint_core::layers::copiable_characteristics(&effects, &env(), oid(X)).unwrap(),
    )
    .copy_with(&own_b(), &[]);
    effects
        .register(eid(2), copy_eff(&from_layers, 20, &[oid(2)]))
        .unwrap();
    assert_eq!(eval_chars(&effects, &env(), oid(2)), norm(&want));

    // Not mutated: only the top's abilities.
    let mp = MergedPermanent::new(vec![comp(&m), comp(&b), comp(&u)], false).unwrap();
    let cp = CopiableValues::merged(&mp, vocab())
        .unwrap()
        .copy_with(&own_b(), &[]);
    assert_eq!(cp.characteristics, single(&m));
}

/// REQ-OBJ-002, CR 731.2a, 202.3: a merged permanent's mana value is its top component's.
#[test]
fn merged_mana_value_is_the_tops() {
    let (m, b, n) = (mutant_def(), merge_base_def(), mutant_new_def());
    for mutated in [false, true] {
        let mp = MergedPermanent::new(vec![comp(&m), comp(&b)], mutated).unwrap();
        let vm = CopiableValues::merged(&mp, vocab()).unwrap();
        assert_eq!(vm.mana_value(ManaValueContext::OffStack), 4);
        assert_eq!(mp.mana_value(vocab()), Ok(4));
        let under = MergedPermanent::new(vec![comp(&b), comp(&m)], mutated).unwrap();
        assert_eq!(
            CopiableValues::merged(&under, vocab())
                .unwrap()
                .mana_value(ManaValueContext::OffStack),
            2
        );
        let over = mp.merge(comp(&n), MergeSide::Over);
        assert_eq!(
            CopiableValues::merged(&over, vocab())
                .unwrap()
                .mana_value(ManaValueContext::OffStack),
            6
        );
    }
}

/// REQ-OBJ-002, CR 731.2a: an empty registry leaves the base permanent unchanged (the merge
/// is only the registered effect).
#[test]
fn merged_without_effect_is_the_base() {
    let effects = ContinuousEffects::new();
    assert_eq!(
        eval_chars(&effects, &env(), oid(X)),
        norm(&single(&merge_base_def()))
    );
}
