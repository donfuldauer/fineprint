// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for REQ-LAYER-001 (#186): what the read views contain. S-5 / B-1: the view given
//! to a characteristic-defining value expression at layer 7a (CR 604.3, 613.4a) shows every
//! object after layers 1a-6 (controller after layer 2, CR 613.1b; types, colors, abilities after
//! layers 4-6), identically whatever the evaluation order. B-6: `generating_ability_exists` gets
//! a view of a foreign source through the end of the effect's own layer, the effect left out
//! (CR 613.8a, 611.3, 113.6). B-7: same-layer ability removal kills another source's effect in
//! both timestamp orders (CR 613.8a, 613.7). Black-box: written from the brief only.

#[path = "spec_layers_cross_object_support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::ObjectId;
use fineprint_core::layers::{
    Affected, ContinuousEffect, EffectOrigin, LayerModification, evaluate, evaluate_all,
};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::ColorBitmask;
use support::*;

// ---------------------------------------------------------------------------------------------
// S-5 / B-1: the layer-7a view
// ---------------------------------------------------------------------------------------------

/// C (CDA, P1), K1 (creature, P1), K3 (creature, P2), N (artifact, P1).
#[derive(Clone, Copy, Debug)]
struct ViewBoard {
    c: ObjectId,
    k1: ObjectId,
    k3: ObjectId,
    n: ObjectId,
}

fn view_layouts() -> [ViewBoard; 2] {
    [
        ViewBoard {
            c: o(1),
            k1: o(2),
            k3: o(3),
            n: o(4),
        },
        ViewBoard {
            c: o(9),
            k1: o(2),
            k3: o(3),
            n: o(4),
        },
    ]
}

fn view_env(b: ViewBoard) -> Env {
    Env::new()
        .with(b.c, P1, cda_counter())
        .with(b.k1, P1, creature(2, 2))
        .with(b.k3, P2, creature(2, 2))
        .with(b.n, P1, noncreature("Artifact"))
}

/// Layer 2: K1 to P2; layer 4: N a creature; layer 5: K3 red; layer 6: K3 gains an ability;
/// layer 7c: K1 +1/+1 (not observable in the view).
fn view_effects(b: ViewBoard) -> Vec<(u64, ContinuousEffect)> {
    vec![
        (1, eff(Layer::L2, 1, objs(&[b.k1]), set_controller(P2))),
        (
            2,
            eff(Layer::L4, 2, objs(&[b.n]), add_card_type("Creature")),
        ),
        (
            3,
            eff(
                Layer::L5,
                3,
                objs(&[b.k3]),
                LayerModification::AddColors(ColorBitmask::RED),
            ),
        ),
        (
            4,
            eff(Layer::L6, 4, objs(&[b.k3]), add_ability("test.grant")),
        ),
        (5, eff(Layer::L7c, 5, objs(&[b.k1]), modify_pt(1, 1))),
    ]
}

fn snap(ctrl: u64, types: BTreeSet<String>, colors: u8, abilities: &[&str]) -> Option<Snap> {
    Some(Snap {
        controller: Some(p(ctrl)),
        card_types: types,
        colors,
        abilities: abilities.iter().map(|s| (*s).to_owned()).collect(),
    })
}

fn expected_snaps(b: ViewBoard) -> BTreeMap<ObjectId, Option<Snap>> {
    [
        (b.c, snap(P1, set(["Creature"]), 0, &[])),
        (b.k1, snap(P2, set(["Creature"]), 0, &[])),
        (
            b.k3,
            snap(
                P2,
                set(["Creature"]),
                ColorBitmask::RED.bits(),
                &["test.grant"],
            ),
        ),
        (b.n, snap(P1, set(["Artifact", "Creature"]), 0, &[])),
    ]
    .into_iter()
    .collect()
}

/// Asserts every recorded view call is the 7a view of C with the expected contents; returns
/// the number of calls.
fn assert_view_calls(env: &Env, b: ViewBoard, ctx: &str) -> usize {
    let calls = env.view_calls.borrow();
    let mut ascending = vec![b.c, b.k1, b.k3, b.n];
    ascending.sort();
    for call in calls.iter() {
        assert_eq!(call.object, b.c, "{ctx}: only the CDA object asks");
        assert_eq!(call.layer, Layer::L7a, "{ctx}: asked at layer 7a");
        assert_eq!(call.objects, ascending, "{ctx}: ascending universe");
        assert_eq!(call.snaps, expected_snaps(b), "{ctx}: view through layer 6");
    }
    assert!(
        env.plain_calls.borrow().is_empty(),
        "{ctx}: no plain eval_value"
    );
    calls.len()
}

/// REQ-LAYER-001, CR 604.3, 613.4a, 613.1b: the 7a view shows every object after layers 1-6
/// (S-5, B-1); C counts itself and N = 2 (control without the effects: 2, C and K1).
#[test]
fn req_layer_001_s5_view_at_7a_shows_layers_1_to_6_of_every_object() {
    for b in view_layouts() {
        let list = view_effects(b);
        let env = view_env(b);
        let ev = evaluate(&registry(&list), &env, b.c).unwrap();
        assert_eq!(pt_of(&ev), (2, 2));
        assert!(assert_view_calls(&env, b, "evaluate(C)") >= 1);

        let control = view_env(b);
        let ev = evaluate(&registry(&[]), &control, b.c).unwrap();
        assert_eq!(pt_of(&ev), (2, 2));
        let calls = control.view_calls.borrow();
        assert!(!calls.is_empty());
        for call in calls.iter() {
            assert_eq!(call.snaps[&b.k1].as_ref().unwrap().controller, Some(p(P1)));
            assert_eq!(
                call.snaps[&b.n].as_ref().unwrap().card_types,
                set(["Artifact"])
            );
        }
    }
}

/// REQ-LAYER-001, REQ-CORE-001, CR 613.4a: the recorded 7a views are identical whichever object
/// is evaluated first, through `evaluate_all`, and for any permutation or duplication of
/// `objects()` and either registration order (S-5).
#[test]
fn req_layer_001_s5_view_identical_for_any_order() {
    for b in view_layouts() {
        let list = view_effects(b);
        let ids = view_env(b).base_ids();
        let mut rng = Rng::new(0x0005_0186);
        for round in 0..12 {
            let mut listing = ids.clone();
            rng.shuffle(&mut listing);
            if round % 3 == 0 {
                let extra = listing[0];
                listing.push(extra);
            }
            let effects = if round % 2 == 0 {
                registry(&list)
            } else {
                registry_rev(&list)
            };
            let env = view_env(b).listing(listing.clone());
            // Evaluate every object one at a time, in the listing order.
            for id in &listing {
                evaluate(&effects, &env, *id).unwrap();
            }
            assert!(assert_view_calls(&env, b, &format!("seq {listing:?}")) >= 1);
            env.clear_records();
            let all = evaluate_all(&effects, &env).unwrap();
            assert_eq!(pt_of(&all[&b.c]), (2, 2));
            assert!(assert_view_calls(&env, b, &format!("all {listing:?}")) >= 1);
        }
    }
}

/// REQ-LAYER-001, CR 604.3: a listed id without a base is not in the 7a view; the other
/// objects still evaluate (B-1, B-5).
#[test]
fn req_layer_001_b1_view_excludes_listed_object_without_base() {
    let b = view_layouts()[0];
    let env = view_env(b).listing(vec![o(77), b.n, b.k3, b.k1, b.c]);
    let ev = evaluate(&registry(&view_effects(b)), &env, b.c).unwrap();
    assert_eq!(pt_of(&ev), (2, 2));
    assert!(assert_view_calls(&env, b, "baseless listed") >= 1);
}

// ---------------------------------------------------------------------------------------------
// B-6: views of foreign sources for generating_ability_exists
// ---------------------------------------------------------------------------------------------

const SRC_ABILITY: &str = "test.src_ability";

/// The B-6 board: source sS (creature, P1, one printed ability) and target t (creature, P1).
fn hook_env(s: ObjectId, t: ObjectId) -> Env {
    let mut src = creature(1, 1);
    src.abilities = vec![keyword_ability(SRC_ABILITY, SRC_ABILITY)];
    Env::new().with(s, P1, src).with(t, P1, creature(1, 1))
}

/// Effects:
/// 1 L2 resolved sS -> P2; 2 L4 resolved sS +Artifact; 3 L5 resolved sS red; 4 L6 resolved sS
/// gains `test.later_grant`; 5 static L4 of sS on {sS, t} +Enchantment; 6 static L6 of sS on t
/// gains `test.from_source`; 7 resolved L6 with source sS on t; 8 static L6 without source on t;
/// 9 static L6 of a source with no base on t.
fn hook_effects(s: ObjectId, t: ObjectId) -> Vec<(u64, ContinuousEffect)> {
    let mut resolved_with_source = eff(Layer::L6, 7, objs(&[t]), add_ability("test.resolved"));
    resolved_with_source.source = Some(s);
    let mut sourceless_static = eff(Layer::L6, 8, objs(&[t]), add_ability("test.sourceless"));
    sourceless_static.origin = EffectOrigin::Static { granted_at: None };
    vec![
        (1, eff(Layer::L2, 1, objs(&[s]), set_controller(P2))),
        (2, eff(Layer::L4, 2, objs(&[s]), add_card_type("Artifact"))),
        (
            3,
            eff(
                Layer::L5,
                3,
                objs(&[s]),
                LayerModification::AddColors(ColorBitmask::RED),
            ),
        ),
        (
            4,
            eff(Layer::L6, 4, objs(&[s]), add_ability("test.later_grant")),
        ),
        (
            5,
            static_eff(Layer::L4, 5, s, objs(&[s, t]), add_card_type("Enchantment")),
        ),
        (
            6,
            static_eff(Layer::L6, 6, s, objs(&[t]), add_ability("test.from_source")),
        ),
        (7, resolved_with_source),
        (8, sourceless_static),
        (
            9,
            static_eff(Layer::L6, 9, o(77), objs(&[t]), add_ability("test.ghost")),
        ),
    ]
}

fn hook_layouts() -> [(ObjectId, ObjectId); 2] {
    [(o(5), o(6)), (o(7), o(3))]
}

/// REQ-LAYER-001, CR 613.8a, 611.3, 113.6, 613.1b: evaluating t, the hook gets `Some` view of
/// the foreign source sS through the end of each effect's own layer with the effect itself left
/// out: the layer-6 effect sees sS under P2, Artifact and Enchantment (layer 4), red (layer 5)
/// and the same-layer grant; the layer-4 effect sees P2 and Artifact but neither its own
/// Enchantment nor layer-5/6 results. Resolved and sourceless effects are never asked; a source
/// without a base gets `None` (B-6, S-5).
#[test]
fn req_layer_001_b6_foreign_source_view_through_own_layer() {
    for (s, t) in hook_layouts() {
        let list = hook_effects(s, t);
        for (j, effects) in [registry(&list), registry_rev(&list)].iter().enumerate() {
            let env = hook_env(s, t);
            let ev = evaluate(effects, &env, t).unwrap();
            let ctx = format!("{s:?} {t:?} reg#{j}");
            // Every hook answer was true: t got everything.
            assert!(
                ev.characteristics.card_types.contains("Enchantment"),
                "{ctx}"
            );
            for id in [
                "test.from_source",
                "test.resolved",
                "test.sourceless",
                "test.ghost",
            ] {
                assert!(
                    ability_ids(&ev.characteristics).contains(&id.to_owned()),
                    "{ctx}: {id}"
                );
            }
            let calls = env.hook_calls.borrow();
            for c in calls.iter() {
                if c.effect == e(5) || c.effect == e(6) {
                    assert_eq!(c.source, s, "{ctx}");
                }
                if c.effect == e(9) {
                    assert_eq!(c.source, o(77), "{ctx}");
                }
            }
            let asked: BTreeSet<_> = calls.iter().map(|c| c.effect).collect();
            for never in [1, 2, 3, 4, 7, 8] {
                assert!(
                    !asked.contains(&e(never)),
                    "{ctx}: effect {never} asked: {calls:?}"
                );
            }
            for must in [5, 6, 9] {
                assert!(asked.contains(&e(must)), "{ctx}: effect {must} not asked");
            }
            let l6_expected = Snap {
                controller: Some(p(P2)),
                card_types: set(["Artifact", "Creature", "Enchantment"]),
                colors: ColorBitmask::RED.bits(),
                abilities: vec![SRC_ABILITY.to_owned(), "test.later_grant".to_owned()],
            };
            let l4_expected = Snap {
                controller: Some(p(P2)),
                card_types: set(["Artifact", "Creature"]),
                colors: 0,
                abilities: vec![SRC_ABILITY.to_owned()],
            };
            // Effect 5 also affects sS, so in a layer-major pass some of its calls are for sS as
            // the object in progress (before the same-layer Artifact grant applied); only the
            // bounds below hold for every call, and the full foreign view must appear.
            for c in calls.iter() {
                if c.effect == e(9) {
                    assert_eq!(c.view, None, "{ctx}: a source without a base gets None");
                }
                if c.effect == e(6) {
                    assert_eq!(c.view.as_ref(), Some(&l6_expected), "{ctx}: layer-6 view");
                }
                if c.effect == e(5) {
                    let v = c
                        .view
                        .as_ref()
                        .expect("foreign source with a base gets Some");
                    assert_eq!(v.controller, Some(p(P2)), "{ctx}: layer 2 complete");
                    assert!(
                        !v.card_types.contains("Enchantment"),
                        "{ctx}: the effect itself is left out"
                    );
                    assert_eq!(v.colors, 0, "{ctx}: layer 5 not yet");
                    assert_eq!(v.abilities, vec![SRC_ABILITY.to_owned()], "{ctx}: no L6");
                }
            }
            assert!(
                calls
                    .iter()
                    .any(|c| c.effect == e(5) && c.view.as_ref() == Some(&l4_expected)),
                "{ctx}: the layer-4 view of sS: {calls:?}"
            );
        }
    }
}

/// REQ-LAYER-001, CR 613.8a, 611.3, 613.1b: when the source is the evaluated object, the hook
/// gets its in-progress state (`Some`), with layer 2 complete for both of its static effects and
/// layers 4-5 complete for the layer-6 one (B-6, as before).
#[test]
fn req_layer_001_b6_own_source_in_progress_view() {
    for (s, t) in hook_layouts() {
        let env = hook_env(s, t);
        let ev = evaluate(&registry(&hook_effects(s, t)), &env, s).unwrap();
        assert!(ev.characteristics.card_types.contains("Enchantment"));
        let calls = env.hook_calls.borrow();
        assert!(calls.iter().any(|c| c.effect == e(5)));
        for c in calls.iter() {
            assert!(
                c.effect == e(5) || c.effect == e(6) || c.effect == e(9),
                "never asked about {:?}",
                c.effect
            );
            if c.effect == e(5) || c.effect == e(6) {
                let v = c.view.as_ref().expect("Some for the evaluated object");
                assert_eq!(v.controller, Some(p(P2)));
            }
            if c.effect == e(6) {
                let v = c.view.as_ref().unwrap();
                assert!(v.card_types.contains("Artifact"));
                assert!(v.card_types.contains("Enchantment"));
                assert_eq!(v.colors, ColorBitmask::RED.bits());
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// B-7: same-layer removal of another source's generating ability
// ---------------------------------------------------------------------------------------------

const ABILITY_A: &str = "test.ability_remover";
const ABILITY_B: &str = "test.ability_marker_grant";
const MARKER: &str = "test.marker";

fn trap_env(sa: ObjectId, sb: ObjectId, t: ObjectId) -> Env {
    let mut a = creature(1, 1);
    a.abilities = vec![keyword_ability(ABILITY_A, ABILITY_A)];
    let mut b = creature(1, 1);
    b.abilities = vec![keyword_ability(ABILITY_B, ABILITY_B)];
    let mut env = Env::new()
        .with(sa, P1, a)
        .with(sb, P1, b)
        .with(t, P1, creature(1, 1));
    env.hook_mode = HookMode::FromView;
    env.generated_by.insert(e(1), ABILITY_A.to_owned());
    env.generated_by.insert(e(2), ABILITY_B.to_owned());
    env
}

/// Effect 1 (A): static L6 of sA, every object loses all abilities. Effect 2 (B): static L6 of
/// sB, every object with no abilities gains the marker.
fn trap_effects(
    sa: ObjectId,
    sb: ObjectId,
    ta: u64,
    tb: u64,
    with_a: bool,
) -> Vec<(u64, ContinuousEffect)> {
    let mut list = Vec::new();
    if with_a {
        list.push((
            1,
            static_eff(
                Layer::L6,
                ta,
                sa,
                Affected::Selector(any()),
                LayerModification::RemoveAllAbilities,
            ),
        ));
    }
    list.push((
        2,
        static_eff(
            Layer::L6,
            tb,
            sb,
            sel(has_no_abilities()),
            add_ability(MARKER),
        ),
    ));
    list
}

fn trap_layouts() -> [(ObjectId, ObjectId, ObjectId); 3] {
    [(o(1), o(2), o(3)), (o(3), o(2), o(1)), (o(2), o(3), o(1))]
}

/// REQ-LAYER-001, CR 613.8a, 611.3, 113.6, 613.7: A strips sB's ability in layer 6, so B's
/// effect does not exist: nobody gets the marker, and sA and sB lose their abilities, with sA
/// stamped before sB and after, in both registration orders and every entry point (B-7).
#[test]
fn req_layer_001_b7_same_layer_removal_kills_other_source_both_orders() {
    for (sa, sb, t) in trap_layouts() {
        for (ta, tb) in [(1, 2), (2, 1)] {
            let list = trap_effects(sa, sb, ta, tb, true);
            let env = trap_env(sa, sb, t);
            let all = check_invariant(&list, &env, &format!("b7 ta={ta} tb={tb}"));
            for id in [sa, sb, t] {
                let abilities = ability_ids(&all[&id].characteristics);
                assert!(
                    abilities.is_empty(),
                    "ta={ta} tb={tb}: {id:?} has {abilities:?}"
                );
            }
        }
    }
}

/// REQ-LAYER-001, CR 613.8a, 611.3: control: without A, B exists (sB keeps its ability) and
/// only t, which has no abilities, gets the marker (B-7).
#[test]
fn req_layer_001_b7_control_without_remover_t_gets_marker() {
    for (sa, sb, t) in trap_layouts() {
        for tb in [1, 2] {
            let list = trap_effects(sa, sb, 0, tb, false);
            let env = trap_env(sa, sb, t);
            let all = check_invariant(&list, &env, "b7 control");
            assert_eq!(
                ability_ids(&all[&t].characteristics),
                vec![MARKER.to_owned()]
            );
            assert_eq!(
                ability_ids(&all[&sa].characteristics),
                vec![ABILITY_A.to_owned()]
            );
            assert_eq!(
                ability_ids(&all[&sb].characteristics),
                vec![ABILITY_B.to_owned()]
            );
        }
    }
}
