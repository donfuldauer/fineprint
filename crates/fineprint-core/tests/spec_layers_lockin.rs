// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for layer lock-in (REQ-LAYER-003; CR 613.6, 611.2c, 611.3a, 611.2b):
//! `ContinuousEffects::register_group`, `register_group_for_as_long_as`, `ForAsLongAsWindow`,
//! `resolve_affected_set`, `Affected::SameAsLayer` links and `LayerError::BadLink`.
//! Black-box: written from the work-item brief only (FP-033). Scenario letters A-I follow the
//! brief. Every card here is synthetic and neutrally named (R5).

#[path = "spec_layers_support.rs"]
mod support;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_core::layers::{
    Affected, ContinuousEffect, ContinuousEffects, EffectOrigin, Evaluated, ForAsLongAsWindow,
    LayerEnvironment, LayerError, LayerModification, MatchContext, ObjectBase, evaluate,
    evaluate_all, resolve_affected_set,
};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::expr::{ObjectSelectorAST, ReferenceAST};
use support::*;

// ---------------------------------------------------------------------------------------------
// selectors of this file (slots above the shared ones)
// ---------------------------------------------------------------------------------------------

/// "noncreature artifact": the candidate, as evaluated so far, is an Artifact and not a Creature.
const SEL_NC_ARTIFACT: u32 = 101;
/// "this object": the candidate is the effect's source.
const SEL_SELF: u32 = 102;
/// "land you control": a Land controlled by the effect's controller.
const SEL_LAND_YOU_CONTROL: u32 = 103;
/// "toughness exactly 2": the candidate's literal toughness, as evaluated so far, is 2.
const SEL_TOUGHNESS_2: u32 = 104;

// ---------------------------------------------------------------------------------------------
// environment
// ---------------------------------------------------------------------------------------------

/// A synthetic `LayerEnvironment` for this file: ordered maps only (R3).
#[derive(Clone, Default)]
struct LockEnv {
    bases: BTreeMap<ObjectId, ObjectBase>,
    /// Effect -> ability id that generates it. While the source (as evaluated so far, when the
    /// view is given) lacks that ability, `generating_ability_exists` is false.
    generated_by: BTreeMap<EffectId, String>,
    /// Every effect id `generating_ability_exists` was asked about, in call order.
    hook_calls: RefCell<Vec<EffectId>>,
}

impl LockEnv {
    fn new() -> LockEnv {
        LockEnv::default()
    }

    fn put(&mut self, id: ObjectId, c: Characteristics) -> ObjectId {
        self.put_ctl(id, c, P1)
    }

    fn put_ctl(&mut self, id: ObjectId, c: Characteristics, controller: PlayerId) -> ObjectId {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(controller),
                on_battlefield: true,
            },
        );
        id
    }

    fn generated_by(&mut self, effect: EffectId, ability_id: &str) {
        self.generated_by.insert(effect, ability_id.to_string());
    }

    fn hook_effects(&self) -> Vec<EffectId> {
        self.hook_calls.borrow().clone()
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct IdProbe {
    #[serde(rename = "abilityId")]
    ability_id: Option<String>,
}

/// The ability id of a placeholder ability, if it has one.
fn ability_id(a: &AbilityAST) -> Option<String> {
    let bytes = rmp_serde::to_vec_named(a).ok()?;
    let p: IdProbe = rmp_serde::from_slice(&bytes).ok()?;
    p.ability_id
}

fn has_ability(c: &Characteristics, id: &str) -> bool {
    c.abilities
        .iter()
        .any(|a| ability_id(a).as_deref() == Some(id))
}

fn literal_toughness(e: &Evaluated) -> Option<i64> {
    e.characteristics.toughness.as_ref().and_then(literal_of)
}

fn view_has_type(ctx: &MatchContext<'_>, t: &str) -> bool {
    ctx.candidate_view.characteristics.card_types.contains(t)
}

impl LayerEnvironment for LockEnv {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn counters(&self, _id: ObjectId) -> BTreeMap<String, i64> {
        BTreeMap::new()
    }

    fn counter_timestamp(&self, _id: ObjectId, _kind: &str) -> Option<Timestamp> {
        None
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        let slot = match selector {
            ObjectSelectorAST::Ref(ReferenceAST::Target { slot }) => Some(*slot),
            _ => None,
        };
        match slot {
            Some(SEL_CREATURE) => view_has_type(ctx, "Creature"),
            Some(SEL_WHITE_CREATURE) => {
                view_has_type(ctx, "Creature")
                    && ctx.candidate_view.characteristics.colors.bits() & 1 == 1
            }
            Some(SEL_ALL) => true,
            Some(SEL_NONE) => false,
            Some(SEL_ENCHANTMENT) => view_has_type(ctx, "Enchantment"),
            Some(SEL_NC_ARTIFACT) => {
                view_has_type(ctx, "Artifact") && !view_has_type(ctx, "Creature")
            }
            Some(SEL_SELF) => ctx.source == Some(ctx.candidate),
            Some(SEL_LAND_YOU_CONTROL) => {
                view_has_type(ctx, "Land")
                    && ctx.candidate_view.controller == Some(ctx.effect_controller)
            }
            Some(SEL_TOUGHNESS_2) => literal_toughness(ctx.candidate_view) == Some(2),
            _ => false,
        }
    }

    fn generating_ability_exists(
        &self,
        effect: EffectId,
        _source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        self.hook_calls.borrow_mut().push(effect);
        match (source_view, self.generated_by.get(&effect)) {
            (Some(view), Some(id)) => has_ability(&view.characteristics, id),
            _ => true,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

type Part = (EffectId, ContinuousEffect);

fn same(l: Layer) -> Affected {
    Affected::SameAsLayer(l)
}

/// A resolved effect (no source, controller P1).
fn res(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    eff(layer, t, affected, m)
}

/// A printed static effect of `source`.
fn stat(
    source: Option<ObjectId>,
    layer: Layer,
    t: u64,
    affected: Affected,
    m: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source,
        controller: P1,
        timestamp: ts(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Static { granted_at: None },
        affected,
        modification: m,
    }
}

fn with_origin(mut e: ContinuousEffect, static_origin: bool) -> ContinuousEffect {
    e.origin = if static_origin {
        EffectOrigin::Static { granted_at: None }
    } else {
        EffectOrigin::Resolved
    };
    e
}

fn ev(effects: &ContinuousEffects, env: &LockEnv, id: ObjectId) -> Evaluated {
    evaluate(effects, env, id).expect("evaluate succeeds")
}

fn is_creature(e: &Evaluated) -> bool {
    e.characteristics.card_types.contains("Creature")
}

fn ids(v: &[u64]) -> BTreeSet<ObjectId> {
    v.iter().map(|n| oid(*n)).collect()
}

fn group(parts: Vec<Part>, env: &LockEnv) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    r.register_group(env, parts)
        .expect("register_group succeeds");
    r
}

/// A snapshot of the registry: the whole value, its length and its iteration (debug form).
fn snapshot(r: &ContinuousEffects) -> (ContinuousEffects, usize, String) {
    (
        r.clone(),
        r.len(),
        format!("{:?}", r.iter().collect::<Vec<_>>()),
    )
}

fn artifact_creature(name: &str, p: i64, t: i64) -> Characteristics {
    Characteristics {
        card_types: set(&["Artifact", "Creature"]),
        ..creature(name, p, t)
    }
}

// ---------------------------------------------------------------------------------------------
// A. 613.6 lock-in: "all noncreature artifacts become 2/2 artifact creatures"
// ---------------------------------------------------------------------------------------------

const A_SOURCE: u64 = 9;

fn env_a() -> LockEnv {
    let mut env = LockEnv::new();
    env.put(oid(1), artifact("test.trinket_a"));
    env.put(oid(2), artifact("test.trinket_b"));
    env.put(oid(3), artifact_creature("test.golem_5_5", 5, 5));
    env.put(oid(4), land("test.plain_land"));
    env.put(
        oid(A_SOURCE),
        with_abilities(
            blank("test.animator", &["Enchantment"]),
            vec![static_ability("test.animator.gen")],
        ),
    );
    env
}

/// Layer 4 "becomes an artifact creature" on noncreature artifacts, and layer 7b "is 2/2"
/// linked to the layer-4 part (CR 613.6).
fn group_a(static_origin: bool) -> Vec<Part> {
    let src = if static_origin {
        Some(oid(A_SOURCE))
    } else {
        None
    };
    let mut l4 = res(
        Layer::L4,
        10,
        by_sel(SEL_NC_ARTIFACT),
        add_card_types(&["Creature"]),
    );
    let mut l7b = res(Layer::L7b, 10, same(Layer::L4), set_pt(Some(2), Some(2)));
    l4.source = src;
    l7b.source = src;
    vec![
        (eid(1), with_origin(l4, static_origin)),
        (eid(2), with_origin(l7b, static_origin)),
    ]
}

fn assert_animated(effects: &ContinuousEffects, env: &LockEnv, id: ObjectId) {
    let e = ev(effects, env, id);
    assert!(is_creature(&e), "{id:?} became a creature");
    assert_eq!(pt(&e), spt(2, 2), "{id:?} is 2/2");
}

fn assert_scenario_a(effects: &ContinuousEffects, env: &LockEnv) {
    assert_animated(effects, env, oid(1));
    assert_animated(effects, env, oid(2));
    for id in [oid(1), oid(2)] {
        assert!(
            ev(effects, env, id)
                .characteristics
                .card_types
                .contains("Artifact"),
            "{id:?} stays an artifact"
        );
    }
    let ac = ev(effects, env, oid(3));
    assert!(is_creature(&ac));
    assert_eq!(
        pt(&ac),
        spt(5, 5),
        "an artifact already a creature is unchanged"
    );
    let l = ev(effects, env, oid(4));
    assert!(!is_creature(&l), "the land is unchanged");
    assert_eq!(pt(&l), (None, None));
}

/// REQ-LAYER-003, CR 613.6: an effect that starts to apply in layer 4 keeps applying to the
/// same objects in layer 7b, whether it comes from a resolved spell or a static ability.
#[test]
fn req_layer_003_cr_613_6_a_noncreature_artifacts_become_2_2_both_origins() {
    let env = env_a();
    for static_origin in [false, true] {
        let effects = group(group_a(static_origin), &env);
        assert_scenario_a(&effects, &env);
    }
}

/// REQ-LAYER-003, CR 613.6 (control): without the link, a 7b part with its own selector
/// "noncreature artifact" does not touch the artifacts that are creatures by layer 7b (static
/// origin, re-evaluated; and a resolved effect added with plain `register`, which never locks).
#[test]
fn req_layer_003_cr_613_6_a_control_unlinked_7b_part_misses_the_animated_artifacts() {
    let env = env_a();
    // static origin through register_group: the 7b part is independent (brief rule 3).
    let l4 = stat(
        Some(oid(A_SOURCE)),
        Layer::L4,
        10,
        by_sel(SEL_NC_ARTIFACT),
        add_card_types(&["Creature"]),
    );
    let l7b = stat(
        Some(oid(A_SOURCE)),
        Layer::L7b,
        10,
        by_sel(SEL_NC_ARTIFACT),
        set_pt(Some(2), Some(2)),
    );
    let static_reg = group(vec![(eid(1), l4.clone()), (eid(2), l7b.clone())], &env);
    // resolved origin through plain register (brief rule 5: plain register never locks).
    let mut plain = ContinuousEffects::new();
    plain
        .register(eid(1), with_origin(l4, false))
        .expect("register");
    plain
        .register(eid(2), with_origin(l7b, false))
        .expect("register");
    for effects in [&static_reg, &plain] {
        for id in [oid(1), oid(2)] {
            let e = ev(effects, &env, id);
            assert!(is_creature(&e), "layer 4 still applies to {id:?}");
            assert_ne!(pt(&e), spt(2, 2), "the unlinked 7b part misses {id:?}");
        }
    }
}

/// REQ-LAYER-003, CR 611.2c and brief rule 4: every part of a resolved group locks against the
/// SAME pre-registration state, so an independent resolved 7b part with selector "noncreature
/// artifact" locks to the artifacts that were noncreature before the group was registered.
#[test]
fn req_layer_003_cr_611_2c_a_resolved_parts_lock_against_the_pre_registration_state() {
    let env = env_a();
    let l4 = res(
        Layer::L4,
        10,
        by_sel(SEL_NC_ARTIFACT),
        add_card_types(&["Creature"]),
    );
    let l7b = res(
        Layer::L7b,
        10,
        by_sel(SEL_NC_ARTIFACT),
        set_pt(Some(2), Some(2)),
    );
    let effects = group(vec![(eid(1), l4), (eid(2), l7b)], &env);
    assert_eq!(
        effects.get(eid(1)).expect("stored").affected,
        Affected::Objects(ids(&[1, 2]))
    );
    assert_eq!(
        effects.get(eid(2)).expect("stored").affected,
        Affected::Objects(ids(&[1, 2]))
    );
    assert_scenario_a(&effects, &env);
}

/// REQ-LAYER-003, CR 611.2c, 613.6 (A1): the resolved form fixes its set at registration: an
/// artifact added later is not affected, and an object that stops being an artifact stays
/// affected.
#[test]
fn req_layer_003_cr_611_2c_a1_resolved_set_is_locked_at_registration() {
    let env = env_a();
    let effects = group(group_a(false), &env);
    assert_eq!(
        effects.get(eid(1)).expect("stored").affected,
        Affected::Objects(ids(&[1, 2])),
        "the resolved selector part is replaced by the locked set"
    );

    // a new noncreature artifact appears after the effect began
    let mut later = env.clone();
    later.put(oid(5), artifact("test.trinket_c"));
    let a3 = ev(&effects, &later, oid(5));
    assert!(!is_creature(&a3), "a later artifact is not picked up");
    assert_eq!(pt(&a3), (None, None));
    assert_scenario_a(&effects, &later);

    // an affected object stops being an artifact: still affected
    let mut changed = env.clone();
    changed.put(oid(2), blank("test.trinket_b", &["Enchantment"]));
    let a2 = ev(&effects, &changed, oid(2));
    assert!(is_creature(&a2));
    assert!(a2.characteristics.card_types.contains("Enchantment"));
    assert_eq!(pt(&a2), spt(2, 2));
    assert_animated(&effects, &changed, oid(1));
}

/// REQ-LAYER-003, CR 611.3a, 613.6 (A2): the static form is re-evaluated on every evaluation
/// (a later artifact IS affected, one that stopped being an artifact is not), while within one
/// evaluation the layer-7b part follows layer 4.
#[test]
fn req_layer_003_cr_611_3a_a2_static_set_is_re_evaluated() {
    let env = env_a();
    let effects = group(group_a(true), &env);
    assert_eq!(
        effects.get(eid(1)).expect("stored").affected,
        by_sel(SEL_NC_ARTIFACT),
        "a static part keeps its selector"
    );
    let mut later = env.clone();
    later.put(oid(5), artifact("test.trinket_c"));
    assert_animated(&effects, &later, oid(5));
    assert_scenario_a(&effects, &later);

    let mut changed = env.clone();
    changed.put(oid(2), blank("test.trinket_b", &["Enchantment"]));
    let a2 = ev(&effects, &changed, oid(2));
    assert!(!is_creature(&a2), "no longer an artifact: not affected");
    assert_eq!(pt(&a2), (None, None));
    assert_animated(&effects, &changed, oid(1));
}

/// REQ-LAYER-003, CR 611.2c, 613.6 (A, property): for every assignment of three kinds
/// (noncreature artifact, artifact creature, land) to four objects, `resolve_affected_set`
/// returns exactly the noncreature artifacts, the resolved group locks to that set, those
/// objects become 2/2 creatures, and after every object turns into a noncreature artifact only
/// the originally locked ones are affected.
#[test]
fn req_layer_003_cr_611_2c_a_property_locked_set_over_all_configurations() {
    for code in 0..81u64 {
        let mut env = LockEnv::new();
        let mut expected = BTreeSet::new();
        let mut c = code;
        for i in 1..=4u64 {
            let kind = c % 3;
            c /= 3;
            let ch = match kind {
                0 => {
                    expected.insert(oid(i));
                    artifact("test.trinket")
                }
                1 => artifact_creature("test.golem_5_5", 5, 5),
                _ => land("test.plain_land"),
            };
            env.put(oid(i), ch);
        }
        let empty = ContinuousEffects::new();
        let got = resolve_affected_set(&empty, &env, None, P1, &sel(SEL_NC_ARTIFACT))
            .expect("resolve succeeds");
        assert_eq!(got, expected, "config {code}");
        let effects = group(group_a(false), &env);
        assert_eq!(
            effects.get(eid(1)).expect("stored").affected,
            Affected::Objects(expected.clone()),
            "config {code}"
        );
        let all = evaluate_all(&effects, &env).expect("evaluate_all");
        for (id, e) in &all {
            if expected.contains(id) {
                assert!(is_creature(e), "config {code} {id:?}");
                assert_eq!(pt(e), spt(2, 2), "config {code} {id:?}");
            }
        }
        // every object becomes a noncreature artifact: only the locked ones are affected
        let mut flipped = env.clone();
        for i in 1..=4u64 {
            flipped.put(oid(i), artifact("test.trinket"));
        }
        for i in 1..=4u64 {
            let e = ev(&effects, &flipped, oid(i));
            assert_eq!(
                is_creature(&e),
                expected.contains(&oid(i)),
                "config {code} object {i}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// B. SRS 6.2 abstract land sequence (CR 613.6, 611.2c)
// ---------------------------------------------------------------------------------------------

const B_LAND: u64 = 1;
const B_ANIMATE: &str = "test.living_land.animate";

fn env_b(with_own_ability: bool) -> LockEnv {
    let mut env = LockEnv::new();
    let mut abilities = vec![static_ability("test.living_land.mana")];
    if with_own_ability {
        abilities.push(static_ability(B_ANIMATE));
    }
    env.put(
        oid(B_LAND),
        with_abilities(blank("test.living_land", &["Land"]), abilities),
    );
    env.put_ctl(oid(2), land("test.other_land"), P2);
    env
}

/// E1: "target land becomes a 3/3 creature that's still a land" (layers 4 and 7b, linked).
fn e1(t: u64) -> Vec<Part> {
    vec![
        (
            eid(11),
            res(
                Layer::L4,
                t,
                Affected::Objects(ids(&[B_LAND])),
                add_card_types(&["Creature"]),
            ),
        ),
        (
            eid(12),
            res(Layer::L7b, t, same(Layer::L4), set_pt(Some(3), Some(3))),
        ),
    ]
}

/// E2: "+1/+1" in layer 7c.
fn e2(t: u64) -> ContinuousEffect {
    res(
        Layer::L7c,
        t,
        Affected::Objects(ids(&[B_LAND])),
        modify_pt(1, 1),
    )
}

/// E3: generated by the land itself, "lands you control become creatures with these subtypes
/// and colors, with power and toughness equal to a count": the count is passed in when the
/// effect is built (layers 4, 5 and 7b; 5 and 7b linked to 4).
fn e3(t: u64, count: i64) -> Vec<Part> {
    let mut parts = vec![
        (
            eid(31),
            res(
                Layer::L4,
                t,
                by_sel(SEL_LAND_YOU_CONTROL),
                LayerModification::AddTypes {
                    supertypes: BTreeSet::new(),
                    card_types: set(&["Creature"]),
                    subtypes: set(&["Testkin", "Testshade"]),
                },
            ),
        ),
        (
            eid(32),
            res(
                Layer::L5,
                t,
                same(Layer::L4),
                LayerModification::SetColors(mask(4 | 8)),
            ),
        ),
        (
            eid(33),
            res(
                Layer::L7b,
                t,
                same(Layer::L4),
                set_pt(Some(count), Some(count)),
            ),
        ),
    ];
    for (_, e) in &mut parts {
        e.source = Some(oid(B_LAND));
    }
    parts
}

/// E1 (t1), E2 (t2), E3 (t3) registered in timestamp order.
fn registry_b(env: &LockEnv, count: i64) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    r.register_group(env, e1(1)).expect("E1");
    r.register(eid(21), e2(2)).expect("E2");
    r.register_group(env, e3(3, count)).expect("E3");
    r
}

/// REQ-LAYER-003, CR 613.6, 611.2c: the SRS 6.2 lock-in example, abstract. E1 makes the land a
/// 3/3 land creature, E2 gives +1/+1, E3 (from the land itself) sets P/T to a count of 10:
/// 11/11. The land keeps its land type; the other player's land is untouched.
#[test]
fn req_layer_003_cr_613_6_b_srs_land_sequence_step_by_step() {
    let env = env_b(true);
    let land_id = oid(B_LAND);

    let mut r = ContinuousEffects::new();
    r.register_group(&env, e1(1)).expect("E1");
    let after_e1 = ev(&r, &env, land_id);
    assert!(is_creature(&after_e1));
    assert!(after_e1.characteristics.card_types.contains("Land"));
    assert_eq!(pt(&after_e1), spt(3, 3), "E1: 3/3 land creature");

    r.register(eid(21), e2(2)).expect("E2");
    assert_eq!(pt(&ev(&r, &env, land_id)), spt(4, 4), "E2: 4/4");

    r.register_group(&env, e3(3, 10)).expect("E3");
    let e = ev(&r, &env, land_id);
    assert_eq!(pt(&e), spt(11, 11), "E3 with a count of 10 (+1/+1 from E2)");
    assert!(e.characteristics.card_types.contains("Land"));
    assert!(is_creature(&e));
    assert_eq!(e.characteristics.subtypes, set(&["Testkin", "Testshade"]));
    assert_eq!(e.characteristics.colors, mask(4 | 8));
    assert_eq!(
        r.get(eid(31)).expect("stored").affected,
        Affected::Objects(ids(&[B_LAND])),
        "E3's selector part is locked to the land"
    );
    let other = ev(&r, &env, oid(2));
    assert!(
        !is_creature(&other),
        "the other player's land is not affected"
    );
    assert_eq!(r, registry_b(&env, 10));
}

/// REQ-LAYER-003, CR 611.2c, 613.6: rebuilding the registry with a new count changes the P/T,
/// and the land stays in E3's locked set even when it no longer meets the selector (now
/// controlled by another player), while a land that later meets it is not added.
#[test]
fn req_layer_003_cr_611_2c_b_pt_follows_the_count_and_the_set_stays_locked() {
    let env = env_b(true);
    let land_id = oid(B_LAND);
    let r = registry_b(&env, 12);
    assert_eq!(pt(&ev(&r, &env, land_id)), spt(13, 13));

    let mut changed = env.clone();
    let land_chars = env.bases[&land_id].characteristics.clone();
    changed.put_ctl(land_id, land_chars, P2);
    changed.put(oid(3), land("test.new_land"));
    let e = ev(&r, &changed, land_id);
    assert!(is_creature(&e));
    assert_eq!(pt(&e), spt(13, 13), "the land stays in E3's set");
    assert_eq!(e.characteristics.colors, mask(4 | 8));
    let newcomer = ev(&r, &changed, oid(3));
    assert!(
        !is_creature(&newcomer),
        "a later land you control is not added"
    );
    assert_eq!(newcomer.characteristics.subtypes, BTreeSet::new());
}

/// REQ-LAYER-003, CR 613.6: removing E3's parts restores 4/4 (E1 + E2) and removes its
/// subtypes and colors.
#[test]
fn req_layer_003_cr_613_6_b_removing_e3_restores_4_4() {
    let env = env_b(true);
    let mut r = registry_b(&env, 10);
    for id in [31, 32, 33] {
        assert!(r.remove(eid(id)), "E3 part {id} removed");
    }
    let e = ev(&r, &env, oid(B_LAND));
    assert_eq!(pt(&e), spt(4, 4));
    assert!(is_creature(&e));
    assert!(e.characteristics.card_types.contains("Land"));
    assert_eq!(e.characteristics.subtypes, BTreeSet::new());
    assert_eq!(e.characteristics.colors, ColorBitmask::NONE);
}

/// REQ-LAYER-003, CR 611.2c, 613.6: E3 is resolved, so it does not depend on the land's own
/// ability still existing: with the ability absent the land is still 11/11, and
/// `generating_ability_exists` is never consulted for E3's parts.
#[test]
fn req_layer_003_cr_611_2c_b_resolved_e3_ignores_the_generating_ability() {
    let land_id = oid(B_LAND);
    for with_ability in [true, false] {
        let mut env = env_b(with_ability);
        for id in [31, 32, 33] {
            env.generated_by(eid(id), B_ANIMATE);
        }
        let r = registry_b(&env, 10);
        let e = ev(&r, &env, land_id);
        assert_eq!(pt(&e), spt(11, 11), "own ability present: {with_ability}");
        assert_eq!(e.characteristics.colors, mask(4 | 8));
        let consulted = env.hook_effects();
        for id in [31, 32, 33] {
            assert!(
                !consulted.contains(&eid(id)),
                "the hook is not consulted for resolved part {id}"
            );
        }
    }
}

/// REQ-LAYER-003, CR 613.6 with timestamps (CR 613.1): in the order E3 (t1), E2 (t2), E1 (t3)
/// each layer still applies by timestamp: E1's later 3/3 overrides E3's 10/10 in layer 7b, and
/// E3's subtypes and colors still apply: 4/4.
#[test]
fn req_layer_003_cr_613_6_b_e3_before_e1_applies_by_timestamp() {
    let env = env_b(true);
    let land_id = oid(B_LAND);
    let mut r = ContinuousEffects::new();
    r.register_group(&env, e3(1, 10)).expect("E3");
    r.register(eid(21), e2(2)).expect("E2");
    r.register_group(&env, e1(3)).expect("E1");
    let e = ev(&r, &env, land_id);
    assert_eq!(pt(&e), spt(4, 4));
    assert!(is_creature(&e));
    assert!(e.characteristics.card_types.contains("Land"));
    assert_eq!(e.characteristics.subtypes, set(&["Testkin", "Testshade"]));
    assert_eq!(e.characteristics.colors, mask(4 | 8));
}

// ---------------------------------------------------------------------------------------------
// C. 611.3a static re-evaluation, resolve_affected_set
// ---------------------------------------------------------------------------------------------

fn white_creature(name: &str, p: i64, t: i64) -> Characteristics {
    with_colors(creature(name, p, t), 1)
}

/// REQ-LAYER-003, CR 611.3a: a static "white creatures get +1/+1" is re-evaluated each time:
/// the bonus appears, disappears and appears again as the creature's colour changes.
#[test]
fn req_layer_003_cr_611_3a_c_static_bonus_follows_colour_changes() {
    let mut env = LockEnv::new();
    env.put(oid(9), enchantment("test.pale_anthem"));
    let c = env.put(oid(1), white_creature("test.pale_2_2", 2, 2));
    let mut r = ContinuousEffects::new();
    r.register(
        eid(1),
        stat(
            Some(oid(9)),
            Layer::L7c,
            1,
            by_sel(SEL_WHITE_CREATURE),
            modify_pt(1, 1),
        ),
    )
    .expect("register");
    let mut blue = env.clone();
    blue.put(c, with_colors(creature("test.pale_2_2", 2, 2), 2));
    assert_eq!(pt(&ev(&r, &env, c)), spt(3, 3), "white: bonus");
    assert_eq!(pt(&ev(&r, &blue, c)), spt(2, 2), "not white: no bonus");
    assert_eq!(pt(&ev(&r, &env, c)), spt(3, 3), "white again: bonus");
    assert_eq!(
        r.get(eid(1)).expect("stored").affected,
        by_sel(SEL_WHITE_CREATURE),
        "the static effect keeps its selector"
    );
}

fn env_c() -> LockEnv {
    let mut env = LockEnv::new();
    env.put(oid(5), white_creature("test.pale_1_1", 1, 1));
    env.put(oid(2), white_creature("test.pale_2_2", 2, 2));
    env.put(oid(3), with_colors(creature("test.blue_2_2", 2, 2), 2));
    env.put(oid(4), with_colors(enchantment("test.pale_charm"), 1));
    env.put(oid(6), artifact("test.trinket"));
    env
}

/// REQ-LAYER-003, CR 611.2c: `resolve_affected_set` returns exactly the matching objects, in
/// ascending order, each evaluated fully with the given registry.
#[test]
fn req_layer_003_cr_611_2c_c_resolve_affected_set_matches_evaluated_objects() {
    let env = env_c();
    let s = sel(SEL_WHITE_CREATURE);
    let empty = ContinuousEffects::new();
    let got = resolve_affected_set(&empty, &env, None, P1, &s).expect("resolve");
    assert_eq!(got.into_iter().collect::<Vec<_>>(), vec![oid(2), oid(5)]);

    // the candidates are evaluated with the registry: a colour effect and a type effect count
    let mut r = ContinuousEffects::new();
    r.register(
        eid(1),
        res(
            Layer::L5,
            1,
            Affected::Objects(ids(&[3])),
            LayerModification::SetColors(mask(1)),
        ),
    )
    .expect("register");
    r.register(
        eid(2),
        res(
            Layer::L4,
            2,
            Affected::Objects(ids(&[4])),
            add_card_types(&["Creature"]),
        ),
    )
    .expect("register");
    let got = resolve_affected_set(&r, &env, None, P1, &s).expect("resolve");
    assert_eq!(
        got.into_iter().collect::<Vec<_>>(),
        vec![oid(2), oid(3), oid(4), oid(5)]
    );

    // source and controller reach the matcher
    let got = resolve_affected_set(&empty, &env, Some(oid(3)), P1, &sel(SEL_SELF)).expect("self");
    assert_eq!(got, ids(&[3]));
    let mut lands = LockEnv::new();
    lands.put_ctl(oid(1), land("test.land_a"), P1);
    lands.put_ctl(oid(2), land("test.land_b"), P2);
    lands.put_ctl(oid(3), land("test.land_c"), P2);
    let got =
        resolve_affected_set(&empty, &lands, None, P2, &sel(SEL_LAND_YOU_CONTROL)).expect("lands");
    assert_eq!(got, ids(&[2, 3]));
    let got = resolve_affected_set(&empty, &lands, None, P1, &sel(SEL_NONE)).expect("none");
    assert_eq!(got, BTreeSet::new());
}

/// REQ-LAYER-003, CR 611.2c: a resolved "white creatures get +1/+1" locks its set: an
/// affected object that stops being white keeps the bonus, one that becomes white does not
/// get it.
#[test]
fn req_layer_003_cr_611_2c_c_locked_set_ignores_colour_changes() {
    let env = env_c();
    let r = group(
        vec![(
            eid(1),
            res(Layer::L7c, 1, by_sel(SEL_WHITE_CREATURE), modify_pt(1, 1)),
        )],
        &env,
    );
    assert_eq!(
        r.get(eid(1)).expect("stored").affected,
        Affected::Objects(ids(&[2, 5]))
    );
    let mut recoloured = env.clone();
    recoloured.put(oid(2), with_colors(creature("test.pale_2_2", 2, 2), 2));
    recoloured.put(oid(3), white_creature("test.blue_2_2", 2, 2));
    assert_eq!(pt(&ev(&r, &recoloured, oid(2))), spt(3, 3));
    assert_eq!(pt(&ev(&r, &recoloured, oid(3))), spt(2, 2));
    assert_eq!(pt(&ev(&r, &recoloured, oid(5))), spt(2, 2));
}

// ---------------------------------------------------------------------------------------------
// D. 611.2b "for as long as"
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-003, CR 611.2b: the full truth table of `ForAsLongAsWindow::effect_does_something`.
#[test]
fn req_layer_003_cr_611_2b_d_window_truth_table() {
    for bits in 0..8u8 {
        let w = ForAsLongAsWindow {
            starts: bits & 1 != 0,
            ended_before_first_application: bits & 2 != 0,
            begins_again: bits & 4 != 0,
        };
        let expected = if !w.starts {
            false
        } else {
            !(w.ended_before_first_application && !w.begins_again)
        };
        assert_eq!(w.effect_does_something(), expected, "{w:?}");
        let copy = w;
        assert_eq!(copy, w, "Copy + Eq");
    }
}

fn env_d() -> LockEnv {
    let mut env = LockEnv::new();
    env.put(oid(1), creature("test.drifter_2_2", 2, 2));
    env
}

fn steal_x() -> Vec<Part> {
    vec![(
        eid(1),
        res(
            Layer::L2,
            1,
            Affected::Objects(ids(&[1])),
            LayerModification::SetController(P2),
        ),
    )]
}

fn window(starts: bool, ended: bool, again: bool) -> ForAsLongAsWindow {
    ForAsLongAsWindow {
        starts,
        ended_before_first_application: ended,
        begins_again: again,
    }
}

/// REQ-LAYER-003, CR 611.2b: a "for as long as" duration that never starts, or ends before the
/// first application and does not begin again, registers nothing; otherwise the effect is
/// registered and applies.
#[test]
fn req_layer_003_cr_611_2b_d_register_group_for_as_long_as() {
    let env = env_d();
    let x = oid(1);
    let does_nothing = [
        window(false, false, false),
        window(false, true, true),
        window(false, true, false),
        window(false, false, true),
        window(true, true, false),
    ];
    for w in does_nothing {
        let mut r = ContinuousEffects::new();
        assert_eq!(
            r.register_group_for_as_long_as(&env, w, steal_x()),
            Ok(false),
            "{w:?}"
        );
        assert!(r.is_empty(), "{w:?}");
        assert_eq!(r.len(), 0);
        assert_eq!(ev(&r, &env, x).controller, Some(P1), "{w:?}");
    }
    for w in [
        window(true, true, true),
        window(true, false, false),
        window(true, false, true),
    ] {
        let mut r = ContinuousEffects::new();
        assert_eq!(
            r.register_group_for_as_long_as(&env, w, steal_x()),
            Ok(true),
            "{w:?}"
        );
        assert_eq!(r.len(), 1);
        assert_eq!(ev(&r, &env, x).controller, Some(P2), "{w:?}");
    }
}

/// REQ-LAYER-003, CR 611.2b with the group validation of CR 613.6: a bad group is an error even
/// when the window says the effect does nothing, and the registry is unchanged.
#[test]
fn req_layer_003_cr_611_2b_d_errors_surface_even_when_the_effect_does_nothing() {
    let env = env_d();
    let mismatch = vec![(eid(5), res(Layer::L2, 1, objs(&[oid(1)]), modify_pt(1, 1)))];
    let bad_link = vec![
        (
            eid(5),
            res(
                Layer::L2,
                1,
                objs(&[oid(1)]),
                LayerModification::SetController(P2),
            ),
        ),
        (eid(6), res(Layer::L7c, 1, same(Layer::L4), modify_pt(1, 1))),
    ];
    let dup = vec![(
        eid(1),
        res(
            Layer::L2,
            1,
            objs(&[oid(1)]),
            LayerModification::SetController(P2),
        ),
    )];
    for w in [
        window(false, false, false),
        window(true, true, false),
        window(true, false, false),
    ] {
        let mut r = ContinuousEffects::new();
        r.register(eid(1), res(Layer::L7c, 1, objs(&[oid(1)]), modify_pt(1, 1)))
            .expect("register");
        let before = snapshot(&r);
        assert_eq!(
            r.register_group_for_as_long_as(&env, w, mismatch.clone()),
            Err(LayerError::LayerMismatch { layer: Layer::L2 }),
            "{w:?}"
        );
        assert_eq!(snapshot(&r), before);
        assert_eq!(
            r.register_group_for_as_long_as(&env, w, bad_link.clone()),
            Err(LayerError::BadLink(eid(6))),
            "{w:?}"
        );
        assert_eq!(snapshot(&r), before);
        assert_eq!(
            r.register_group_for_as_long_as(&env, w, dup.clone()),
            Err(LayerError::DuplicateEffect(eid(1))),
            "{w:?}"
        );
        assert_eq!(snapshot(&r), before);
        assert!(
            matches!(
                r.register_group_for_as_long_as(&env, w, Vec::new()),
                Err(LayerError::Unsupported(_))
            ),
            "{w:?}"
        );
        assert_eq!(snapshot(&r), before);
    }
}

// ---------------------------------------------------------------------------------------------
// E. 613.6 survives removal of the generating ability
// ---------------------------------------------------------------------------------------------

const E_SRC: u64 = 1;
const E_GEN: &str = "gen";

fn env_e(has_gen: bool) -> LockEnv {
    let mut env = LockEnv::new();
    let abilities = if has_gen {
        vec![static_ability(E_GEN), static_ability("test.other")]
    } else {
        vec![static_ability("test.other")]
    };
    env.put(
        oid(E_SRC),
        Characteristics {
            power: Some(lit(1)),
            toughness: Some(lit(1)),
            ..with_abilities(blank("test.self_animator", &["Enchantment"]), abilities)
        },
    );
    env.put(oid(2), creature("test.bystander_2_2", 2, 2));
    for id in [1, 3] {
        env.generated_by(eid(id), E_GEN);
    }
    env
}

fn e_anchor() -> Part {
    (
        eid(1),
        stat(
            Some(oid(E_SRC)),
            Layer::L4,
            1,
            by_sel(SEL_SELF),
            add_card_types(&["Creature"]),
        ),
    )
}

fn e_layer7c(affected: Affected) -> Part {
    (
        eid(3),
        stat(Some(oid(E_SRC)), Layer::L7c, 1, affected, modify_pt(2, 2)),
    )
}

/// The separate, later, unlinked layer-6 effect that removes every ability of the source.
fn e_remove_all(r: &mut ContinuousEffects) {
    r.register(
        eid(2),
        res(
            Layer::L6,
            2,
            objs(&[oid(E_SRC)]),
            LayerModification::RemoveAllAbilities,
        ),
    )
    .expect("register");
}

/// REQ-LAYER-003, CR 613.6: once the static effect starts to apply in layer 4, its linked 7c
/// part keeps applying to the same object even though a layer-6 effect removed the generating
/// ability during the evaluation; the linked part does not consult `generating_ability_exists`.
#[test]
fn req_layer_003_cr_613_6_e_linked_part_survives_removal_of_the_ability() {
    let env = env_e(true);
    let mut r = group(vec![e_anchor(), e_layer7c(same(Layer::L4))], &env);
    e_remove_all(&mut r);
    let s = ev(&r, &env, oid(E_SRC));
    assert!(is_creature(&s), "layer 4 applied while the ability existed");
    assert!(s.characteristics.abilities.is_empty(), "layer 6 removed it");
    assert_eq!(pt(&s), spt(3, 3), "the linked 7c part still applies");
    assert!(
        !env.hook_effects().contains(&eid(3)),
        "a SameAsLayer part does not consult generating_ability_exists"
    );
    let other = ev(&r, &env, oid(2));
    assert_eq!(pt(&other), spt(2, 2), "the bystander is not affected");
}

/// REQ-LAYER-003, CR 613.6 (control): the same 7c part with its own static selector does not
/// apply, because the generating ability no longer exists when layer 7c is reached.
#[test]
fn req_layer_003_cr_613_6_e_control_unlinked_part_needs_the_ability() {
    let env = env_e(true);
    let mut r = group(vec![e_anchor(), e_layer7c(by_sel(SEL_SELF))], &env);
    e_remove_all(&mut r);
    let mut reference = group(vec![e_anchor()], &env);
    e_remove_all(&mut reference);
    let s = ev(&r, &env, oid(E_SRC));
    assert!(is_creature(&s));
    assert_eq!(pt(&s), spt(1, 1), "the unlinked 7c part does not apply");
    assert_eq!(s, ev(&reference, &env, oid(E_SRC)));
}

/// REQ-LAYER-003, CR 613.6: if the anchor never applied (the ability was already absent at
/// layer 4), the linked part does not apply either.
#[test]
fn req_layer_003_cr_613_6_e_anchor_never_applied_so_link_does_not_apply() {
    let env = env_e(false);
    let mut r = group(vec![e_anchor(), e_layer7c(same(Layer::L4))], &env);
    e_remove_all(&mut r);
    let mut reference = ContinuousEffects::new();
    e_remove_all(&mut reference);
    let s = ev(&r, &env, oid(E_SRC));
    assert!(!is_creature(&s), "the anchor did not apply");
    assert_ne!(pt(&s), spt(3, 3));
    assert_eq!(s, ev(&reference, &env, oid(E_SRC)), "neither part applied");
}

// ---------------------------------------------------------------------------------------------
// F. empty set, links and group errors
// ---------------------------------------------------------------------------------------------

fn env_f() -> LockEnv {
    let mut env = LockEnv::new();
    env.put(oid(1), artifact("test.trinket"));
    env.put(oid(2), creature("test.vanilla_2_2", 2, 2));
    env.put(oid(3), land("test.plain_land"));
    env
}

/// REQ-LAYER-003, CR 613.6, 611.2c: an anchor whose selector matches nothing locks to the empty
/// set, and its linked part affects nothing (resolved and static).
#[test]
fn req_layer_003_cr_613_6_f_empty_anchor_set_affects_nothing() {
    let env = env_f();
    let empty = ContinuousEffects::new();
    for static_origin in [false, true] {
        let parts = vec![
            (
                eid(1),
                with_origin(
                    res(
                        Layer::L4,
                        1,
                        by_sel(SEL_NONE),
                        add_card_types(&["Creature"]),
                    ),
                    static_origin,
                ),
            ),
            (
                eid(2),
                with_origin(
                    res(Layer::L7b, 1, same(Layer::L4), set_pt(Some(7), Some(7))),
                    static_origin,
                ),
            ),
        ];
        let r = group(parts, &env);
        if !static_origin {
            assert_eq!(
                r.get(eid(1)).expect("stored").affected,
                Affected::Objects(BTreeSet::new())
            );
        }
        assert_eq!(
            evaluate_all(&r, &env).expect("all"),
            evaluate_all(&empty, &env).expect("all"),
            "static: {static_origin}"
        );
    }
}

fn l4_anchor(id: u64) -> Part {
    (
        eid(id),
        res(Layer::L4, 1, by_sel(SEL_ALL), add_card_types(&["Creature"])),
    )
}

/// The invalid groups of the brief with the expected error.
fn bad_groups() -> Vec<(&'static str, Vec<Part>, LayerError)> {
    vec![
        (
            "no part in the named layer",
            vec![
                l4_anchor(1),
                (
                    eid(2),
                    res(Layer::L7b, 1, same(Layer::L5), set_pt(Some(1), Some(1))),
                ),
            ],
            LayerError::BadLink(eid(2)),
        ),
        (
            "anchor in the same layer",
            vec![
                (
                    eid(1),
                    res(Layer::L7b, 1, by_sel(SEL_ALL), set_pt(Some(1), Some(1))),
                ),
                (
                    eid(2),
                    res(Layer::L7b, 1, same(Layer::L7b), set_pt(Some(2), None)),
                ),
            ],
            LayerError::BadLink(eid(2)),
        ),
        (
            "anchor in a later layer",
            vec![
                (
                    eid(1),
                    res(Layer::L7b, 1, by_sel(SEL_ALL), set_pt(Some(1), Some(1))),
                ),
                (
                    eid(2),
                    res(
                        Layer::L4,
                        1,
                        same(Layer::L7b),
                        add_card_types(&["Creature"]),
                    ),
                ),
            ],
            LayerError::BadLink(eid(2)),
        ),
        (
            "anchor in a later sublayer",
            vec![
                (eid(1), res(Layer::L7c, 1, by_sel(SEL_ALL), modify_pt(1, 1))),
                (
                    eid(2),
                    res(Layer::L7b, 1, same(Layer::L7c), set_pt(Some(1), Some(1))),
                ),
            ],
            LayerError::BadLink(eid(2)),
        ),
        (
            "the only part of the named layer is itself SameAsLayer",
            vec![
                l4_anchor(1),
                (
                    eid(2),
                    res(
                        Layer::L5,
                        1,
                        same(Layer::L4),
                        LayerModification::AddColors(mask(1)),
                    ),
                ),
                (
                    eid(3),
                    res(Layer::L7b, 1, same(Layer::L5), set_pt(Some(1), Some(1))),
                ),
            ],
            LayerError::BadLink(eid(3)),
        ),
        (
            "duplicate id within the group",
            vec![
                l4_anchor(1),
                (
                    eid(1),
                    res(Layer::L7b, 1, same(Layer::L4), set_pt(Some(1), Some(1))),
                ),
            ],
            LayerError::DuplicateEffect(eid(1)),
        ),
        (
            "duplicate id against a registered effect",
            vec![
                l4_anchor(1),
                (
                    eid(100),
                    res(Layer::L7b, 1, same(Layer::L4), set_pt(Some(1), Some(1))),
                ),
            ],
            LayerError::DuplicateEffect(eid(100)),
        ),
        (
            "layer mismatch",
            vec![
                l4_anchor(1),
                (eid(2), res(Layer::L7b, 1, same(Layer::L4), modify_pt(1, 1))),
            ],
            LayerError::LayerMismatch { layer: Layer::L7b },
        ),
    ]
}

/// A registry with one effect already registered as eid(100).
fn pre_registered() -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    r.register(
        eid(100),
        res(Layer::L7c, 1, objs(&[oid(2)]), modify_pt(1, 1)),
    )
    .expect("register");
    r
}

/// REQ-LAYER-003, CR 613.6: an invalid link (no anchor in the named layer, anchor in the same
/// or a later layer or sublayer, anchor that is itself SameAsLayer), a duplicate id or a layer
/// mismatch is rejected with the brief's error, and the registry is unchanged (all-or-nothing).
/// Static-origin copies of each group are rejected the same way.
#[test]
fn req_layer_003_cr_613_6_f_bad_groups_are_rejected_and_change_nothing() {
    let env = env_f();
    for (what, parts, err) in bad_groups() {
        for static_origin in [false, true] {
            let parts: Vec<Part> = parts
                .iter()
                .map(|(id, e)| (*id, with_origin(e.clone(), static_origin)))
                .collect();
            let mut r = pre_registered();
            let before = snapshot(&r);
            assert_eq!(
                r.register_group(&env, parts.clone()),
                Err(err.clone()),
                "{what} (static: {static_origin})"
            );
            assert_eq!(snapshot(&r), before, "{what}: registry unchanged");
            // and in reverse part order
            let mut reversed = parts;
            reversed.reverse();
            assert_eq!(
                r.register_group(&env, reversed),
                Err(err.clone()),
                "{what} reversed"
            );
            assert_eq!(snapshot(&r), before, "{what} reversed: registry unchanged");
        }
    }
}

/// REQ-LAYER-003, CR 613.6: an empty group is Unsupported and changes nothing.
#[test]
fn req_layer_003_cr_613_6_f_empty_group_is_unsupported() {
    let env = env_f();
    let mut r = pre_registered();
    let before = snapshot(&r);
    assert!(matches!(
        r.register_group(&env, Vec::new()),
        Err(LayerError::Unsupported(_))
    ));
    assert_eq!(snapshot(&r), before);
    let mut fresh = ContinuousEffects::new();
    assert!(matches!(
        fresh.register_group(&env, Vec::new()),
        Err(LayerError::Unsupported(_))
    ));
    assert!(fresh.is_empty());
    assert_eq!(fresh.iter().count(), 0);
}

/// REQ-LAYER-003, CR 613.6: plain `register` still rejects a SameAsLayer part with Unsupported,
/// and `BadLink` is an error with a message.
#[test]
fn req_layer_003_cr_613_6_f_plain_register_rejects_same_as_layer() {
    let mut r = ContinuousEffects::new();
    let res_ = r.register(
        eid(1),
        res(Layer::L7b, 1, same(Layer::L4), set_pt(Some(1), Some(1))),
    );
    assert!(matches!(res_, Err(LayerError::Unsupported(_))), "{res_:?}");
    assert!(r.is_empty());
    let e = LayerError::BadLink(eid(3));
    let as_err: &dyn std::error::Error = &e;
    assert!(!as_err.to_string().is_empty());
    assert_eq!(e.clone(), e);
}

/// REQ-LAYER-003, CR 613.6: a valid link to the lowest-id anchor when the named layer has
/// several non-linked parts registers every part (brief rule 2).
#[test]
fn req_layer_003_cr_613_6_f_valid_groups_register_all_parts() {
    let env = env_f();
    let mut r = pre_registered();
    r.register_group(
        &env,
        vec![
            (
                eid(2),
                res(
                    Layer::L4,
                    1,
                    by_sel(SEL_NONE),
                    add_card_types(&["Enchantment"]),
                ),
            ),
            l4_anchor(1),
            (
                eid(3),
                res(Layer::L7b, 1, same(Layer::L4), set_pt(Some(6), Some(6))),
            ),
        ],
    )
    .expect("valid group");
    assert_eq!(r.len(), 4);
    for id in [1, 2, 3, 100] {
        assert!(r.get(eid(id)).is_some(), "{id} stored");
    }
    // the link follows eid(1) (lowest id among the layer-4 anchors), which matched everything
    // (object 2 also has the pre-registered +1/+1 of eid(100) in layer 7c)
    for (id, want) in [(1, spt(6, 6)), (2, spt(7, 7)), (3, spt(6, 6))] {
        let e = ev(&r, &env, oid(id));
        assert!(is_creature(&e), "{id}");
        assert!(
            !e.characteristics.card_types.contains("Enchantment"),
            "{id}: the SEL_NONE part matched nothing"
        );
        assert_eq!(pt(&e), want, "{id}");
    }
}

// ---------------------------------------------------------------------------------------------
// G. sublayers
// ---------------------------------------------------------------------------------------------

fn env_g() -> LockEnv {
    let mut env = LockEnv::new();
    env.put(oid(1), creature("test.vanilla_4_2", 4, 2));
    env.put(oid(2), creature("test.vanilla_5_1", 5, 1));
    env.put(oid(3), creature("test.vanilla_1_2", 1, 2));
    env
}

/// REQ-LAYER-003, CR 613.6 (sublayers): a static group anchored in 7a (CDA "1/1" on creatures
/// with toughness 2) with linked parts in 7b, 7c and 7d keeps the 7a set even though the
/// objects no longer have toughness 2 after 7a; arithmetic 1/1 -> 1/3 -> 4/3 -> switch 3/4.
#[test]
fn req_layer_003_cr_613_6_g_static_sublayer_chain_from_7a() {
    let env = env_g();
    let mut anchor = cda_eff(
        Layer::L7a,
        1,
        by_sel(SEL_TOUGHNESS_2),
        set_pt(Some(1), Some(1)),
    );
    anchor.source = None;
    let parts = vec![
        (eid(1), anchor),
        (
            eid(2),
            stat(None, Layer::L7b, 1, same(Layer::L7a), set_pt(None, Some(3))),
        ),
        (
            eid(3),
            stat(None, Layer::L7c, 1, same(Layer::L7a), modify_pt(3, 0)),
        ),
        (
            eid(4),
            stat(
                None,
                Layer::L7d,
                1,
                same(Layer::L7a),
                LayerModification::SwitchPt,
            ),
        ),
    ];
    let r = group(parts, &env);
    assert_eq!(pt(&ev(&r, &env, oid(1))), spt(3, 4));
    assert_eq!(pt(&ev(&r, &env, oid(3))), spt(3, 4));
    assert_eq!(pt(&ev(&r, &env, oid(2))), spt(5, 1), "not in the 7a set");
}

/// REQ-LAYER-003, CR 613.6, 611.2c (sublayers): a resolved group anchored in 7b (set 1/1 on
/// creatures with toughness 2) with linked 7c and 7d parts: the same set in each sublayer,
/// 1/1 -> 4/1 -> switch 1/4.
#[test]
fn req_layer_003_cr_613_6_g_resolved_sublayer_chain_from_7b() {
    let env = env_g();
    let parts = vec![
        (
            eid(1),
            res(
                Layer::L7b,
                1,
                by_sel(SEL_TOUGHNESS_2),
                set_pt(Some(1), Some(1)),
            ),
        ),
        (
            eid(2),
            res(Layer::L7c, 1, same(Layer::L7b), modify_pt(3, 0)),
        ),
        (
            eid(3),
            res(Layer::L7d, 1, same(Layer::L7b), LayerModification::SwitchPt),
        ),
    ];
    let r = group(parts, &env);
    assert_eq!(
        r.get(eid(1)).expect("stored").affected,
        Affected::Objects(ids(&[1, 3]))
    );
    assert_eq!(pt(&ev(&r, &env, oid(1))), spt(1, 4));
    assert_eq!(pt(&ev(&r, &env, oid(3))), spt(1, 4));
    assert_eq!(pt(&ev(&r, &env, oid(2))), spt(5, 1));
}

// ---------------------------------------------------------------------------------------------
// H. per-part independence
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-003, CR 611.2c: in one resolved group, the linked pair (layer 4 + 7b) and an
/// independent layer-5 part with another selector lock their sets independently.
#[test]
fn req_layer_003_cr_611_2c_h_parts_lock_independently() {
    let mut env = LockEnv::new();
    env.put(oid(1), artifact("test.trinket"));
    env.put(oid(2), enchantment("test.charm"));
    env.put(
        oid(3),
        blank("test.charmed_trinket", &["Artifact", "Enchantment"]),
    );
    env.put(oid(4), land("test.plain_land"));
    let parts = vec![
        (
            eid(1),
            res(
                Layer::L4,
                1,
                by_sel(SEL_NC_ARTIFACT),
                add_card_types(&["Creature"]),
            ),
        ),
        (
            eid(2),
            res(Layer::L7b, 1, same(Layer::L4), set_pt(Some(2), Some(2))),
        ),
        (
            eid(3),
            res(
                Layer::L5,
                1,
                by_sel(SEL_ENCHANTMENT),
                LayerModification::AddColors(mask(1)),
            ),
        ),
    ];
    let r = group(parts, &env);
    assert_eq!(
        r.get(eid(1)).expect("stored").affected,
        Affected::Objects(ids(&[1, 3]))
    );
    assert_eq!(
        r.get(eid(3)).expect("stored").affected,
        Affected::Objects(ids(&[2, 3]))
    );
    let white = |e: &Evaluated| e.characteristics.colors.bits() & 1 == 1;
    let e1 = ev(&r, &env, oid(1));
    assert!(is_creature(&e1) && !white(&e1));
    assert_eq!(pt(&e1), spt(2, 2));
    let e2 = ev(&r, &env, oid(2));
    assert!(!is_creature(&e2) && white(&e2));
    assert_eq!(pt(&e2), (None, None));
    let e3 = ev(&r, &env, oid(3));
    assert!(is_creature(&e3) && white(&e3));
    assert_eq!(pt(&e3), spt(2, 2));
    let e4 = ev(&r, &env, oid(4));
    assert!(!is_creature(&e4) && !white(&e4));
}

// ---------------------------------------------------------------------------------------------
// I. determinism and order independence
// ---------------------------------------------------------------------------------------------

fn env_i() -> LockEnv {
    let mut env = LockEnv::new();
    env.put(oid(1), artifact("test.trinket_a"));
    env.put(oid(2), artifact("test.trinket_b"));
    env.put(oid(3), artifact_creature("test.golem_5_5", 5, 5));
    env.put(oid(4), with_colors(enchantment("test.pale_charm"), 1));
    env.put(oid(5), white_creature("test.pale_2_2", 2, 2));
    env.put(oid(9), enchantment("test.anthem_source"));
    env
}

/// Resolved group: noncreature artifacts become 2/2 creatures (linked) and enchantments turn
/// white (independent).
fn group_i1() -> Vec<Part> {
    vec![
        (
            eid(1),
            res(
                Layer::L4,
                5,
                by_sel(SEL_NC_ARTIFACT),
                add_card_types(&["Creature"]),
            ),
        ),
        (
            eid(2),
            res(Layer::L7b, 5, same(Layer::L4), set_pt(Some(2), Some(2))),
        ),
        (
            eid(3),
            res(
                Layer::L5,
                5,
                by_sel(SEL_ENCHANTMENT),
                LayerModification::AddColors(mask(2)),
            ),
        ),
    ]
}

/// Static group: white creatures get +1/+1 and then have P/T switched (linked).
fn group_i2() -> Vec<Part> {
    vec![
        (
            eid(4),
            stat(
                Some(oid(9)),
                Layer::L7c,
                3,
                by_sel(SEL_WHITE_CREATURE),
                modify_pt(1, 0),
            ),
        ),
        (
            eid(5),
            stat(
                Some(oid(9)),
                Layer::L7d,
                3,
                same(Layer::L7c),
                LayerModification::SwitchPt,
            ),
        ),
    ]
}

/// Single effects registered with plain register (they change no type or colour).
fn singles_i() -> Vec<Part> {
    vec![
        (
            eid(6),
            res(Layer::L7c, 7, objs(&[oid(3), oid(5)]), modify_pt(1, 1)),
        ),
        (
            eid(7),
            res(
                Layer::L2,
                8,
                objs(&[oid(1)]),
                LayerModification::SetController(P2),
            ),
        ),
    ]
}

fn permuted(parts: &[Part], order: &[usize]) -> Vec<Part> {
    order.iter().map(|i| parts[*i].clone()).collect()
}

/// REQ-LAYER-003, CR 613.6, 611.2c, 611.3a, R3: every permutation of each group's parts, both
/// group orders and singles before or after the groups give identical evaluate and evaluate_all
/// results; repeated evaluation is identical; evaluate_all equals per-object evaluate.
#[test]
fn req_layer_003_cr_613_6_i_order_independence_and_determinism() {
    let env = env_i();
    let g1 = group_i1();
    let g2 = group_i2();
    let singles = singles_i();
    let mut reference: Option<BTreeMap<ObjectId, Evaluated>> = None;
    let mut runs = 0;
    for p1 in permutations(g1.len()) {
        for p2 in permutations(g2.len()) {
            for g1_first in [true, false] {
                for singles_first in [true, false] {
                    let mut r = ContinuousEffects::new();
                    let add_singles = |r: &mut ContinuousEffects| {
                        for (id, e) in &singles {
                            r.register(*id, e.clone()).expect("single");
                        }
                    };
                    if singles_first {
                        add_singles(&mut r);
                    }
                    let a = permuted(&g1, &p1);
                    let b = permuted(&g2, &p2);
                    if g1_first {
                        r.register_group(&env, a).expect("g1");
                        r.register_group(&env, b).expect("g2");
                    } else {
                        r.register_group(&env, b).expect("g2");
                        r.register_group(&env, a).expect("g1");
                    }
                    if !singles_first {
                        add_singles(&mut r);
                    }
                    assert_eq!(r.len(), 7);
                    let all = evaluate_all(&r, &env).expect("all");
                    let again = evaluate_all(&r, &env).expect("all again");
                    assert_eq!(all, again, "repeated evaluate_all is identical");
                    assert_eq!(
                        all.keys().copied().collect::<Vec<_>>(),
                        env.objects(),
                        "evaluate_all covers env.objects()"
                    );
                    for (id, e) in &all {
                        assert_eq!(*e, ev(&r, &env, *id), "evaluate_all == evaluate");
                        assert_eq!(*e, ev(&r, &env, *id), "repeated evaluate");
                    }
                    match &reference {
                        None => reference = Some(all),
                        Some(want) => assert_eq!(
                            &all, want,
                            "order {p1:?} {p2:?} g1_first={g1_first} singles_first={singles_first}"
                        ),
                    }
                    runs += 1;
                }
            }
        }
    }
    assert_eq!(runs, 6 * 2 * 2 * 2);
    // spot checks of the common result
    let all = reference.expect("at least one run");
    assert_eq!(pt(&all[&oid(1)]), spt(2, 2));
    assert_eq!(all[&oid(1)].controller, Some(P2));
    assert_eq!(pt(&all[&oid(2)]), spt(2, 2));
    assert_eq!(
        pt(&all[&oid(3)]),
        spt(6, 6),
        "already a creature: only +1/+1"
    );
    // white creature: +1/+0 (7c, t3) and +1/+1 (7c, t7) = 4/3, then switched (7d) = 3/4
    assert_eq!(pt(&all[&oid(5)]), spt(3, 4));
    assert_eq!(all[&oid(4)].characteristics.colors, mask(1 | 2));
}
