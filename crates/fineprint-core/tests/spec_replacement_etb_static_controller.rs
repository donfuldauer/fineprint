// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the controller of the entering permanent's statics in the ETB look-ahead of
//! `fineprint_core::replacement` (REQ-LAYER-002, REQ-LAYER-001; CR 614.12, 110.2, 110.2a,
//! 707.5, 109.5, 611.3a, 613.1b, 613.1d): the entrant is checked as it would exist on the
//! battlefield, with the continuous effects already in force (a layer-2 control change of the
//! entrant) applied; "you" in its own and copied statics is that post-layer-2 controller.
//!
//! The projection returns only the entrant's evaluation, so "P2's artifact is affected, P1's is
//! not" is observed on the entrant itself: an artifact entering under P1 and given to P2 by a
//! layer-2 effect in force is P2's artifact, and a static "artifacts you control are creatures"
//! on it must make it a creature, while a static "artifacts you don't control get a type" must
//! not apply to it.
//!
//! Black-box: written from the FP-152 brief only (issue #203). Every object, type and effect
//! here is synthetic and neutrally named (R5).

#[path = "spec_layers_support.rs"]
mod layers;

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use fineprint_base::ids::{EffectId, PlayerId};
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, LayerModification, MatchContext,
};
use fineprint_core::objects::Characteristics;
use fineprint_core::replacement::{
    CopyAbilities, EtbAction, EtbApplied, EtbCondition, EtbContext, EtbEffect, EtbEffects,
    EtbEntry, ProjectedPermanent, Tier, applicable, project,
};
use layers::*;

// ---------------------------------------------------------------------------------------------
// synthetic vocabulary
// ---------------------------------------------------------------------------------------------

const CREATURE: &str = "Creature";
const ARTIFACT: &str = "Artifact";
/// A synthetic card type added by the "artifacts you don't control" static.
const TYPE_Y: &str = "test.type_y";

/// "artifacts you control": an Artifact whose controller is the effect's controller.
const SEL_AYC: u32 = 50;
/// "artifacts you don't control": an Artifact whose controller is not the effect's controller.
const SEL_AYDC: u32 = 51;

/// The entering object.
const ENTRANT: u64 = 1;
/// The copy model's object (the source of its declared statics before rebinding).
const MODEL_OBJ: u64 = 77;

/// Effect ids.
const E_STATIC_AYC: u64 = 100;
const E_STATIC_AYDC: u64 = 101;
const E_CONTROL: u64 = 200;
const E_COPY: u64 = 1;
const E_TAPPED: u64 = 2;

/// Timestamp of the statics.
const T_STATIC: u64 = 10;

fn env_with(entrant: Characteristics) -> TestEnv {
    let mut env = TestEnv::new();
    env.put_full(oid(ENTRANT), entrant, Some(P1), false);
    env.add_selector(
        sel(SEL_AYC),
        Rc::new(|c: &MatchContext<'_>| {
            c.candidate_view
                .characteristics
                .card_types
                .contains(ARTIFACT)
                && c.candidate_view.controller == Some(c.effect_controller)
        }),
    );
    env.add_selector(
        sel(SEL_AYDC),
        Rc::new(|c: &MatchContext<'_>| {
            c.candidate_view
                .characteristics
                .card_types
                .contains(ARTIFACT)
                && c.candidate_view.controller != Some(c.effect_controller)
        }),
    );
    env
}

fn add_types_mod(card_types: &[&str]) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: set(card_types),
        subtypes: BTreeSet::new(),
    }
}

/// A layer-4 static of `source` with stored controller `stored`.
fn static_of(source: u64, stored: PlayerId, slot: u32, types: &[&str]) -> ContinuousEffect {
    ContinuousEffect {
        source: Some(oid(source)),
        controller: stored,
        timestamp: ts(T_STATIC),
        layer: Layer::L4,
        is_cda: false,
        origin: EffectOrigin::Static { granted_at: None },
        affected: Affected::Selector(sel(slot)),
        modification: add_types_mod(types),
    }
}

/// The two statics of `source`: "artifacts you control are creatures" and "artifacts you don't
/// control have `test.type_y`".
fn statics_of(source: u64, stored: PlayerId) -> Vec<(EffectId, ContinuousEffect)> {
    vec![
        (
            eid(E_STATIC_AYC),
            static_of(source, stored, SEL_AYC, &[CREATURE]),
        ),
        (
            eid(E_STATIC_AYDC),
            static_of(source, stored, SEL_AYDC, &[TYPE_Y]),
        ),
    ]
}

/// The continuous effects in force: optionally a layer-2 effect giving `to` control of the
/// entrant, at timestamp `t` (CR 614.12, third input).
fn in_force(control: Option<(PlayerId, u64)>) -> ContinuousEffects {
    let mut c = ContinuousEffects::new();
    if let Some((to, t)) = control {
        c.register(
            eid(E_CONTROL),
            ContinuousEffect {
                source: None,
                controller: to,
                timestamp: ts(t),
                layer: Layer::L2,
                is_cda: false,
                origin: EffectOrigin::Resolved,
                affected: Affected::Objects([oid(ENTRANT)].into_iter().collect()),
                modification: LayerModification::SetController(to),
            },
        )
        .expect("register");
    }
    c
}

/// "Enters tapped" if the permanent is a creature (to show the projected type is used
/// downstream, CR 614.12).
fn tapped_if_creature() -> EtbEffect {
    EtbEffect::new(eid(E_TAPPED), EtbAction::EntersTapped)
        .with_description("test.etb_tapped_if_creature".to_owned())
        .with_condition(EtbCondition::HasCardType(CREATURE.to_owned()))
}

fn effects_of(list: Vec<EtbEffect>) -> EtbEffects {
    let mut e = EtbEffects::new();
    for f in list {
        let id = f.id;
        if let Err(err) = e.add(f) {
            panic!("adding effect {id:?} failed: {err}");
        }
    }
    e
}

/// A copy effect making the entrant a copy of a synthetic artifact model whose declared statics
/// are `statics_of(MODEL_OBJ, model_controller)` (CR 707.5, CR 614.12), plus the tapped effect.
fn copy_effects(model_controller: PlayerId) -> EtbEffects {
    let mut e = effects_of(vec![
        EtbEffect::new(
            eid(E_COPY),
            EtbAction::EntersAsCopyOf(Box::new(artifact("test.etb_static_model"))),
        )
        .with_description("test.etb_copy".to_owned())
        .with_tier(Tier::CopyEtb),
        tapped_if_creature(),
    ]);
    e.set_copy_abilities(
        eid(E_COPY),
        CopyAbilities {
            statics: statics_of(MODEL_OBJ, model_controller),
            etb: Vec::new(),
        },
    )
    .expect("declare copy abilities");
    e
}

fn applied_copy() -> EtbApplied {
    EtbApplied {
        effects: [eid(E_COPY)].into_iter().collect(),
        chosen: BTreeMap::new(),
        order: vec![eid(E_COPY)],
    }
}

fn proj(ctx: &EtbContext<'_>, e: &EtbEntry, a: &EtbApplied) -> ProjectedPermanent {
    match project(ctx, e, a) {
        Ok(p) => p,
        Err(err) => panic!("project failed: {err}"),
    }
}

fn appl(ctx: &EtbContext<'_>, e: &EtbEntry, a: &EtbApplied) -> Vec<EffectId> {
    match applicable(ctx, e, a) {
        Ok(v) => v,
        Err(err) => panic!("applicable failed: {err}"),
    }
}

/// Asserts the entrant projects with controller `controller`, as an artifact creature without
/// `test.type_y` (its "you" is its own post-layer-2 controller), and that the creature-only
/// effect is applicable.
fn assert_you_is_post_layer_2(
    p: &ProjectedPermanent,
    tapped_applicable: &[EffectId],
    controller: PlayerId,
    what: &str,
) {
    let types = &p.evaluated.characteristics.card_types;
    assert_eq!(
        p.evaluated.controller,
        Some(controller),
        "{what}: controller"
    );
    assert!(types.contains(ARTIFACT), "{what}: artifact");
    assert!(
        types.contains(CREATURE),
        "{what}: \"artifacts you control are creatures\" applies to its controller's artifact"
    );
    assert!(
        !types.contains(TYPE_Y),
        "{what}: \"artifacts you don't control\" must not see the old controller's view"
    );
    assert_eq!(
        tapped_applicable,
        &[eid(E_TAPPED)],
        "{what}: creature-only effect"
    );
}

// ---------------------------------------------------------------------------------------------
// own statics (CR 614.12, second input)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 614.12, 110.2, 109.5, 611.3a, 613.1b, 613.1d — an artifact enters under P1
/// (entry and environment controller P1); a layer-2 effect already in force gives it to P2, at a
/// timestamp earlier and later than its statics. Its own static "artifacts you control are
/// creatures" applies for P2 (it is P2's artifact: a creature, the creature-only ETB effect is
/// applicable) and "artifacts you don't control" does not apply to it.
#[test]
fn req_layer_002_cr_614_12_own_static_uses_post_layer_2_controller() {
    let env = env_with(artifact("test.etb_entrant_artifact"));
    let effects = effects_of(vec![tapped_if_creature()]);
    for t in [T_STATIC - 5, T_STATIC + 5] {
        for stored in [P1, P2] {
            let what = format!("t={t} stored {stored:?}");
            let cont = in_force(Some((P2, t)));
            let ctx = EtbContext {
                env: &env,
                continuous: &cont,
                effects: &effects,
            };
            let entry = EtbEntry {
                object: oid(ENTRANT),
                controller: P1,
                own_statics: statics_of(ENTRANT, stored),
            };
            let applied = EtbApplied::default();
            let p = proj(&ctx, &entry, &applied);
            let a = appl(&ctx, &entry, &applied);
            assert_you_is_post_layer_2(&p, &a, P2, &what);
            assert_eq!(
                cont,
                in_force(Some((P2, t))),
                "{what}: in-force effects untouched"
            );
        }
    }
}

/// REQ-LAYER-002, CR 614.12, 110.2 — no layer-2 effect: unchanged; the artifact enters under P1,
/// its own static makes it a creature (P1's artifact) and "artifacts you don't control" does
/// not apply. A layer-2 effect in force giving it to P1 (its entry controller) is the same.
#[test]
fn req_layer_002_cr_110_2_own_static_without_control_change_unchanged() {
    let env = env_with(artifact("test.etb_entrant_artifact"));
    let effects = effects_of(vec![tapped_if_creature()]);
    for control in [None, Some((P1, T_STATIC - 5)), Some((P1, T_STATIC + 5))] {
        let what = format!("{control:?}");
        let cont = in_force(control);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let entry = EtbEntry {
            object: oid(ENTRANT),
            controller: P1,
            own_statics: statics_of(ENTRANT, P1),
        };
        let applied = EtbApplied::default();
        let p = proj(&ctx, &entry, &applied);
        let a = appl(&ctx, &entry, &applied);
        assert_you_is_post_layer_2(&p, &a, P1, &what);
    }
}

/// REQ-LAYER-002, CR 614.12 — a non-artifact entrant is not selected by either static, with or
/// without the control change (the statics only reach artifacts).
#[test]
fn req_layer_002_cr_614_12_non_artifact_entrant_unaffected() {
    let env = env_with(enchantment("test.etb_entrant_enchantment"));
    let effects = effects_of(vec![tapped_if_creature()]);
    for control in [None, Some((P2, T_STATIC + 5))] {
        let cont = in_force(control);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let entry = EtbEntry {
            object: oid(ENTRANT),
            controller: P1,
            own_statics: statics_of(ENTRANT, P1),
        };
        let p = proj(&ctx, &entry, &EtbApplied::default());
        let types = &p.evaluated.characteristics.card_types;
        assert!(!types.contains(CREATURE), "{control:?}");
        assert!(!types.contains(TYPE_Y), "{control:?}");
        assert!(appl(&ctx, &entry, &EtbApplied::default()).is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// copied statics (CR 707.5)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 614.12, 707.5, 109.5, 613.1b, 613.1d — a non-artifact enters under P1 as a
/// copy of an artifact model whose declared statics say "artifacts you control are creatures"
/// and "artifacts you don't control have `test.type_y`" (model controlled by P1 or P2); a
/// layer-2 effect in force gives the entrant to P2 (timestamp before and after the statics).
/// The copied statics are the entrant's, and their "you" is its post-layer-2 controller P2:
/// the entrant is an artifact creature without `test.type_y`, and the creature-only effect is
/// applicable.
#[test]
fn req_layer_002_cr_707_5_copied_static_uses_post_layer_2_controller() {
    let env = env_with(enchantment("test.etb_entrant_base"));
    for model_controller in [P1, P2] {
        let effects = copy_effects(model_controller);
        for t in [T_STATIC - 5, T_STATIC + 5] {
            let what = format!("model {model_controller:?} t={t}");
            let cont = in_force(Some((P2, t)));
            let ctx = EtbContext {
                env: &env,
                continuous: &cont,
                effects: &effects,
            };
            let entry = EtbEntry {
                object: oid(ENTRANT),
                controller: P1,
                own_statics: Vec::new(),
            };
            let applied = applied_copy();
            let p = proj(&ctx, &entry, &applied);
            let a = appl(&ctx, &entry, &applied);
            assert_you_is_post_layer_2(&p, &a, P2, &what);
        }
    }
}

/// REQ-LAYER-002, CR 614.12, 707.5, 110.2 — the copied-statics variant with no layer-2 effect:
/// unchanged; the entrant is P1's artifact creature without `test.type_y`, whoever controls
/// the model.
#[test]
fn req_layer_002_cr_707_5_copied_static_without_control_change_unchanged() {
    let env = env_with(enchantment("test.etb_entrant_base"));
    for model_controller in [P1, P2] {
        let effects = copy_effects(model_controller);
        let cont = in_force(None);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        let entry = EtbEntry {
            object: oid(ENTRANT),
            controller: P1,
            own_statics: Vec::new(),
        };
        let applied = applied_copy();
        let p = proj(&ctx, &entry, &applied);
        let a = appl(&ctx, &entry, &applied);
        assert_you_is_post_layer_2(&p, &a, P1, &format!("model {model_controller:?}"));
    }
}

/// REQ-LAYER-001, REQ-LAYER-002, CR 614.12 — the projection is a pure function: projecting the
/// same entry twice, and in either order of the two control variants, gives equal results.
#[test]
fn req_layer_001_cr_614_12_projection_is_repeatable() {
    let env = env_with(enchantment("test.etb_entrant_base"));
    let effects = copy_effects(P1);
    let entry = EtbEntry {
        object: oid(ENTRANT),
        controller: P1,
        own_statics: Vec::new(),
    };
    let run = |control: Option<(PlayerId, u64)>| {
        let cont = in_force(control);
        let ctx = EtbContext {
            env: &env,
            continuous: &cont,
            effects: &effects,
        };
        proj(&ctx, &entry, &applied_copy()).evaluated
    };
    let a1 = run(Some((P2, 5)));
    let b1 = run(None);
    let b2 = run(None);
    let a2 = run(Some((P2, 5)));
    assert_eq!(a1, a2);
    assert_eq!(b1, b2);
    assert_ne!(a1.controller, b1.controller);
}
