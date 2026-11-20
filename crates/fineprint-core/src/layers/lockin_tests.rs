// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of lock-in (REQ-LAYER-003, SRS 6.3): cross-layer lock-in of a multi-layer effect
// (CR 613.6), concrete sets for resolved effects (CR 611.2c), static selectors matched on every
// evaluation (CR 611.3a), "for as long as" windows that never start (CR 611.2b). Synthetic ids
// and synthetic characteristics only (R5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST, ReferenceAST, SelectorFilter, WordRef};

use super::*;
use crate::objects::Characteristics;

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

/// Builds one part of an effect group: layer, timestamp, affected set, modification.
type Make = fn(Layer, u64, Affected, LayerModification) -> ContinuousEffect;

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

fn ts(n: u64) -> Timestamp {
    Timestamp::new(n)
}

fn strings(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A synthetic object `name` of the given card types, black, no abilities, optional P/T.
fn object(name: &str, card_types: &[&str], pt: Option<(i64, i64)>) -> Characteristics {
    Characteristics {
        names: strings(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::BLACK,
        supertypes: BTreeSet::new(),
        card_types: strings(card_types),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream(Vec::new()),
        abilities: Vec::new(),
        power: pt.map(|(p, _)| literal(p)),
        toughness: pt.map(|(_, t)| literal(t)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// The marker ability that generates the static effects of the removal tests.
fn generator() -> AbilityAST {
    AbilityAST("test.ability.generator".into())
}

fn filter(pred: PredicateAST) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    })
}

fn has_type(t: &str) -> PredicateAST {
    PredicateAST::HasCardType {
        value: WordRef::Name(t.to_owned()),
    }
}

fn of_type(t: &str) -> ObjectSelectorAST {
    filter(has_type(t))
}

fn creatures() -> ObjectSelectorAST {
    filter(PredicateAST::IsCreature)
}

fn noncreature_artifacts() -> ObjectSelectorAST {
    filter(PredicateAST::And(vec![
        has_type("Artifact"),
        PredicateAST::Not(Box::new(PredicateAST::IsCreature)),
    ]))
}

fn white_creatures() -> ObjectSelectorAST {
    filter(PredicateAST::And(vec![
        PredicateAST::IsCreature,
        PredicateAST::HasColor {
            value: WordRef::Name("white".to_owned()),
        },
    ]))
}

fn self_ref() -> ObjectSelectorAST {
    ObjectSelectorAST::Ref(ReferenceAST::SelfRef)
}

fn objects(ids: &[ObjectId]) -> Affected {
    Affected::Objects(ids.iter().copied().collect())
}

fn same(layer: Layer) -> Affected {
    Affected::SameAsLayer(layer)
}

fn add_types(card_types: &[&str], subtypes: &[&str]) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: strings(card_types),
        subtypes: strings(subtypes),
    }
}

fn set_pt(power: i64, toughness: i64) -> LayerModification {
    LayerModification::SetPt {
        power: Some(power),
        toughness: Some(toughness),
    }
}

fn modify(power: i64, toughness: i64) -> LayerModification {
    LayerModification::ModifyPt { power, toughness }
}

/// A resolved effect controlled by player 1 with no source.
fn res(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: p(1),
        timestamp: ts(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected,
        modification: m,
    }
}

/// A static effect of `source`'s own ability.
fn stat(
    source: ObjectId,
    layer: Layer,
    t: u64,
    affected: Affected,
    m: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source: Some(source),
        origin: EffectOrigin::Static { granted_at: None },
        ..res(layer, t, affected, m)
    }
}

/// A static effect of object 9's ability.
fn static_of_o9(layer: Layer, t: u64, a: Affected, m: LayerModification) -> ContinuousEffect {
    stat(o(9), layer, t, a, m)
}

/// A synthetic environment.
#[derive(Default, Clone)]
struct Env {
    bases: BTreeMap<ObjectId, ObjectBase>,
    /// Listed by `objects` without a base (to provoke `UnknownObject`).
    phantom: Vec<ObjectId>,
    /// Whether a static effect's generating ability exists only while its source (when it is
    /// the object under evaluation) has the [`generator`] ability.
    model_generators: bool,
}

impl Env {
    fn with(mut self, id: ObjectId, c: Characteristics) -> Env {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(p(1)),
                on_battlefield: true,
            },
        );
        self
    }
}

fn pred_holds(pred: &PredicateAST, view: &Evaluated) -> bool {
    let c = &view.characteristics;
    match pred {
        PredicateAST::And(all) => all.iter().all(|q| pred_holds(q, view)),
        PredicateAST::Not(q) => !pred_holds(q, view),
        PredicateAST::IsCreature => c.card_types.contains("Creature"),
        PredicateAST::HasCardType {
            value: WordRef::Name(t),
        } => c.card_types.contains(t),
        PredicateAST::HasColor {
            value: WordRef::Name(w),
        } => w == "white" && c.colors.bits() & ColorBitmask::WHITE.bits() != 0,
        _ => false,
    }
}

impl LayerEnvironment for Env {
    fn objects(&self) -> Vec<ObjectId> {
        let mut ids: Vec<ObjectId> = self.bases.keys().copied().collect();
        ids.extend(self.phantom.iter().copied());
        ids.sort_unstable();
        ids
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        match selector {
            ObjectSelectorAST::Ref(ReferenceAST::SelfRef) => ctx.source == Some(ctx.candidate),
            ObjectSelectorAST::Filter(f) => f
                .predicate
                .as_ref()
                .is_some_and(|q| pred_holds(q, ctx.candidate_view)),
            ObjectSelectorAST::Ref(_) => false,
        }
    }

    fn generating_ability_exists(
        &self,
        _effect: EffectId,
        _source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        if !self.model_generators {
            return true;
        }
        source_view.is_none_or(|v| v.characteristics.abilities.contains(&generator()))
    }
}

fn eval(r: &ContinuousEffects, env: &Env, id: ObjectId) -> Evaluated {
    evaluate(r, env, id).unwrap()
}

fn pt_of(c: &Characteristics) -> Option<(i64, i64)> {
    Some((
        literal_of(c.power.as_ref()?)?,
        literal_of(c.toughness.as_ref()?)?,
    ))
}

fn eval_pt(r: &ContinuousEffects, env: &Env, id: ObjectId) -> Option<(i64, i64)> {
    pt_of(&eval(r, env, id).characteristics)
}

fn is_creature(r: &ContinuousEffects, env: &Env, id: ObjectId) -> bool {
    eval(r, env, id)
        .characteristics
        .card_types
        .contains("Creature")
}

fn group(parts: Vec<(u64, ContinuousEffect)>) -> Vec<(EffectId, ContinuousEffect)> {
    parts.into_iter().map(|(id, x)| (e(id), x)).collect()
}

// ---------------------------------------------------------------------------------------------
// CR 613.6 / 611.2c: "all noncreature artifacts become 2/2 artifact creatures"
// ---------------------------------------------------------------------------------------------

/// o1 noncreature artifact, o2 artifact creature 1/1, o3 noncreature land.
fn artifacts_env() -> Env {
    Env::default()
        .with(o(1), object("test.plain_artifact", &["Artifact"], None))
        .with(
            o(2),
            object(
                "test.artifact_creature_1_1",
                &["Artifact", "Creature"],
                Some((1, 1)),
            ),
        )
        .with(o(3), object("test.plain_land", &["Land"], None))
}

fn animate_parts(make: Make) -> Vec<(EffectId, ContinuousEffect)> {
    group(vec![
        (
            1,
            make(
                Layer::L4,
                10,
                Affected::Selector(noncreature_artifacts()),
                add_types(&["Creature"], &[]),
            ),
        ),
        (2, make(Layer::L7b, 10, same(Layer::L4), set_pt(2, 2))),
    ])
}

// REQ-LAYER-003, CR 613.6, 611.2c: the resolved form locks the layer-4 set at registration and
// the layer-7b part reuses it although the object is a creature by then; an artifact that was
// already a creature is untouched; an artifact arriving later stays unaffected.
#[test]
fn lockin_resolved_animate_noncreature_artifacts() {
    let env = artifacts_env();
    let mut r = ContinuousEffects::new();
    r.register_group(&env, animate_parts(res)).unwrap();
    // CR 611.2c: the selector became a concrete set.
    assert_eq!(r.get(e(1)).unwrap().affected, objects(&[o(1)]));
    // CR 613.6: the linked part keeps SameAsLayer.
    assert_eq!(r.get(e(2)).unwrap().affected, same(Layer::L4));

    assert!(is_creature(&r, &env, o(1)));
    assert_eq!(eval_pt(&r, &env, o(1)), Some((2, 2)));
    assert_eq!(eval_pt(&r, &env, o(2)), Some((1, 1)));
    assert!(!is_creature(&r, &env, o(3)));
    assert_eq!(eval_pt(&r, &env, o(3)), None);

    // CR 611.2c: an artifact that meets the criteria later is not affected.
    let later = env.with(o(4), object("test.late_artifact", &["Artifact"], None));
    assert!(!is_creature(&r, &later, o(4)));
    assert_eq!(eval_pt(&r, &later, o(4)), None);
    assert_eq!(eval_pt(&r, &later, o(1)), Some((2, 2)));
}

// REQ-LAYER-003, CR 611.2c: an object that stops meeting the criteria after registration stays
// affected by the resolved group.
#[test]
fn lockin_resolved_keeps_object_that_stops_matching() {
    let env = artifacts_env();
    let mut r = ContinuousEffects::new();
    r.register_group(&env, animate_parts(res)).unwrap();
    // o1 stops being an artifact (an earlier-timestamp layer-4 effect); the set is fixed.
    r.register(
        e(3),
        res(
            Layer::L4,
            1,
            objects(&[o(1)]),
            LayerModification::SetTypes {
                supertypes: None,
                card_types: Some(strings(&["Enchantment"])),
                subtypes: None,
            },
        ),
    )
    .unwrap();
    let c = eval(&r, &env, o(1)).characteristics;
    assert!(c.card_types.contains("Creature"));
    assert!(!c.card_types.contains("Artifact"));
    assert_eq!(pt_of(&c), Some((2, 2)));
}

// REQ-LAYER-003, CR 613.6, 611.3a: the static form picks up a later artifact on the next
// evaluation, and inside each evaluation the layer-7b part still follows the layer-4 set,
// whereas an unlinked layer-7b part with the same selector no longer matches by then.
#[test]
fn lockin_static_animate_noncreature_artifacts() {
    let env = artifacts_env().with(o(9), object("test.static_source", &["Enchantment"], None));
    let mut r = ContinuousEffects::new();
    r.register_group(&env, animate_parts(static_of_o9)).unwrap();
    // CR 611.3a: the static selector is kept.
    assert_eq!(
        r.get(e(1)).unwrap().affected,
        Affected::Selector(noncreature_artifacts())
    );
    assert_eq!(eval_pt(&r, &env, o(1)), Some((2, 2)));
    assert_eq!(eval_pt(&r, &env, o(2)), Some((1, 1)));
    assert_eq!(eval_pt(&r, &env, o(3)), None);

    // CR 611.3a: a later-arriving artifact is picked up on the next evaluation.
    let later = env
        .clone()
        .with(o(4), object("test.late_artifact", &["Artifact"], None));
    assert!(is_creature(&r, &later, o(4)));
    assert_eq!(eval_pt(&r, &later, o(4)), Some((2, 2)));

    // Contrast: the same 7b modification with an unlinked selector does not apply, because the
    // object is a creature by layer 7b (CR 613.5).
    let mut unlinked = ContinuousEffects::new();
    unlinked
        .register_group(
            &env,
            group(vec![
                (
                    1,
                    static_of_o9(
                        Layer::L4,
                        10,
                        Affected::Selector(noncreature_artifacts()),
                        add_types(&["Creature"], &[]),
                    ),
                ),
                (
                    2,
                    static_of_o9(
                        Layer::L7b,
                        10,
                        Affected::Selector(noncreature_artifacts()),
                        set_pt(2, 2),
                    ),
                ),
            ]),
        )
        .unwrap();
    assert_eq!(eval_pt(&unlinked, &later, o(4)), Some((0, 0)));
}

// ---------------------------------------------------------------------------------------------
// SRS 6.2 canonical example (abstract)
// ---------------------------------------------------------------------------------------------

/// E3: generated by the land itself; a creature with set color and subtype, P/T = `count`.
fn e3_parts(count: i64) -> Vec<(EffectId, ContinuousEffect)> {
    let mut parts = group(vec![
        (
            31,
            res(
                Layer::L4,
                3,
                Affected::Selector(self_ref()),
                add_types(&["Creature"], &["Synthkin"]),
            ),
        ),
        (
            32,
            res(
                Layer::L5,
                3,
                same(Layer::L4),
                LayerModification::SetColors(ColorBitmask::GREEN),
            ),
        ),
        (
            33,
            res(Layer::L7b, 3, same(Layer::L4), set_pt(count, count)),
        ),
    ]);
    for (_, part) in &mut parts {
        part.source = Some(o(1));
    }
    parts
}

fn remove_e3(r: &mut ContinuousEffects) {
    for id in [31, 32, 33] {
        assert!(r.remove(e(id)));
    }
}

// REQ-LAYER-003, CR 613.6, 611.2c (SRS 6.2 example): land L (o1) becomes a 3/3 land creature
// (E1, layers 4 and 7b), gets +1/+1 (E2, 7c), then E3 makes it a green Synthkin creature with
// P/T equal to a count of 10 (layers 4, 5, 7b): 11/11. A changed count changes P/T but not the
// set; when E3 ends L is 4/4 again. Another land (o2) is never affected.
#[test]
fn lockin_srs_land_sequence() {
    let env = Env::default()
        .with(o(1), object("test.land_l", &["Land"], None))
        .with(o(2), object("test.land_other", &["Land"], None));
    let mut r = ContinuousEffects::new();
    // E1: the land o1 only (a resolved effect targeting it).
    r.register_group(
        &env,
        group(vec![
            (
                11,
                res(
                    Layer::L4,
                    1,
                    objects(&[o(1)]),
                    add_types(&["Creature"], &[]),
                ),
            ),
            (12, res(Layer::L7b, 1, same(Layer::L4), set_pt(3, 3))),
        ]),
    )
    .unwrap();
    assert_eq!(eval_pt(&r, &env, o(1)), Some((3, 3)));
    // E2: "creatures get +1/+1" locked to the creatures at resolution (o1 only).
    r.register_group(
        &env,
        group(vec![(
            21,
            res(Layer::L7c, 2, Affected::Selector(creatures()), modify(1, 1)),
        )]),
    )
    .unwrap();
    assert_eq!(r.get(e(21)).unwrap().affected, objects(&[o(1)]));
    assert_eq!(eval_pt(&r, &env, o(1)), Some((4, 4)));

    // E3 with 10 qualifying cards.
    r.register_group(&env, e3_parts(10)).unwrap();
    assert_eq!(r.get(e(31)).unwrap().affected, objects(&[o(1)]));
    let land = eval(&r, &env, o(1)).characteristics;
    assert_eq!(pt_of(&land), Some((11, 11)));
    assert_eq!(land.colors, ColorBitmask::GREEN);
    assert!(land.subtypes.contains("Synthkin"));
    assert!(land.card_types.contains("Land") && land.card_types.contains("Creature"));
    assert_eq!(eval_pt(&r, &env, o(2)), None);

    // The count changes to 7 (re-registered): P/T follows, the set does not change.
    remove_e3(&mut r);
    r.register_group(&env, e3_parts(7)).unwrap();
    assert_eq!(r.get(e(31)).unwrap().affected, objects(&[o(1)]));
    assert_eq!(eval_pt(&r, &env, o(1)), Some((8, 8)));
    assert_eq!(eval_pt(&r, &env, o(2)), None);

    // E3 ends: back to 4/4, still a land creature, original color.
    remove_e3(&mut r);
    let land = eval(&r, &env, o(1)).characteristics;
    assert_eq!(pt_of(&land), Some((4, 4)));
    assert_eq!(land.colors, ColorBitmask::BLACK);
    assert!(!land.subtypes.contains("Synthkin"));
}

// ---------------------------------------------------------------------------------------------
// CR 611.3a: static selectors are matched on every evaluation
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-003, CR 611.3a: "white creatures get +1/+1" affects a creature only while it is
// white, across several evaluations of the same registry.
#[test]
fn static_selector_reevaluated_every_computation() {
    let env = Env::default()
        .with(
            o(1),
            object("test.creature_2_2", &["Creature"], Some((2, 2))),
        )
        .with(o(9), object("test.static_source", &["Enchantment"], None));
    let mut r = ContinuousEffects::new();
    r.register_group(
        &env,
        group(vec![(
            1,
            stat(
                o(9),
                Layer::L7c,
                1,
                Affected::Selector(white_creatures()),
                modify(1, 1),
            ),
        )]),
    )
    .unwrap();
    assert_eq!(eval_pt(&r, &env, o(1)), Some((2, 2)));
    // It becomes white.
    r.register(
        e(2),
        res(
            Layer::L5,
            2,
            objects(&[o(1)]),
            LayerModification::SetColors(ColorBitmask::WHITE),
        ),
    )
    .unwrap();
    assert_eq!(eval_pt(&r, &env, o(1)), Some((3, 3)));
    assert_eq!(eval_pt(&r, &env, o(1)), Some((3, 3)));
    // It stops being white.
    assert!(r.remove(e(2)));
    assert_eq!(eval_pt(&r, &env, o(1)), Some((2, 2)));
    // The base itself becomes white: matched again.
    let mut white = object("test.creature_2_2", &["Creature"], Some((2, 2)));
    white.colors = ColorBitmask::WHITE;
    let env2 = env.with(o(1), white);
    assert_eq!(eval_pt(&r, &env2, o(1)), Some((3, 3)));
    // The selector was never locked.
    assert_eq!(
        r.get(e(1)).unwrap().affected,
        Affected::Selector(white_creatures())
    );
}

// ---------------------------------------------------------------------------------------------
// CR 611.2b: "for as long as" windows
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-003, CR 611.2b: the truth table of all eight windows.
#[test]
fn for_as_long_as_truth_table() {
    for starts in [false, true] {
        for ended in [false, true] {
            for again in [false, true] {
                let w = ForAsLongAsWindow {
                    starts,
                    ended_before_first_application: ended,
                    begins_again: again,
                };
                let expected = starts && (!ended || again);
                assert_eq!(w.effect_does_something(), expected, "{w:?}");
            }
        }
    }
}

fn control_parts() -> Vec<(EffectId, ContinuousEffect)> {
    group(vec![(
        1,
        res(
            Layer::L2,
            5,
            Affected::Selector(creatures()),
            LayerModification::SetController(p(2)),
        ),
    )])
}

fn creature_env() -> Env {
    Env::default().with(
        o(1),
        object("test.creature_2_2", &["Creature"], Some((2, 2))),
    )
}

// REQ-LAYER-003, CR 611.2b: a never-started control effect registers nothing and leaves the
// controller unchanged; ending before the first application without beginning again likewise;
// beginning again registers it (locked, CR 611.2c).
#[test]
fn for_as_long_as_registration() {
    let env = creature_env();
    let never = ForAsLongAsWindow {
        starts: false,
        ended_before_first_application: false,
        begins_again: false,
    };
    let ended = ForAsLongAsWindow {
        starts: true,
        ended_before_first_application: true,
        begins_again: false,
    };
    let again = ForAsLongAsWindow {
        begins_again: true,
        ..ended
    };
    let running = ForAsLongAsWindow {
        starts: true,
        ended_before_first_application: false,
        begins_again: false,
    };

    for window in [never, ended] {
        let mut r = ContinuousEffects::new();
        assert_eq!(
            r.register_group_for_as_long_as(&env, window, control_parts()),
            Ok(false)
        );
        assert!(r.is_empty());
        assert_eq!(eval(&r, &env, o(1)).controller, Some(p(1)));
    }
    for window in [again, running] {
        let mut r = ContinuousEffects::new();
        assert_eq!(
            r.register_group_for_as_long_as(&env, window, control_parts()),
            Ok(true)
        );
        assert_eq!(r.len(), 1);
        assert_eq!(r.get(e(1)).unwrap().affected, objects(&[o(1)]));
        assert_eq!(eval(&r, &env, o(1)).controller, Some(p(2)));
    }
}

// REQ-LAYER-003, CR 611.2b: the group is validated first; an invalid group is an error even
// when the effect would do nothing, and nothing is registered.
#[test]
fn for_as_long_as_validates_first() {
    let env = creature_env();
    let never = ForAsLongAsWindow {
        starts: false,
        ended_before_first_application: false,
        begins_again: false,
    };
    let mut r = ContinuousEffects::new();
    let bad = group(vec![(1, res(Layer::L7b, 1, same(Layer::L4), set_pt(1, 1)))]);
    assert_eq!(
        r.register_group_for_as_long_as(&env, never, bad),
        Err(LayerError::BadLink(e(1)))
    );
    assert_eq!(
        r.register_group_for_as_long_as(&env, never, Vec::new()),
        Err(LayerError::Unsupported("empty effect group"))
    );
    assert!(r.is_empty());
}

// ---------------------------------------------------------------------------------------------
// CR 613.6: removal of the generating ability during evaluation
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-003, CR 613.6: a static group of o1 whose layer-4 anchor applies keeps applying in
// layer 7c through its linked part after a layer-6 effect removed the generating ability; an
// unlinked layer-7c static part of the same source does not apply.
#[test]
fn lockin_survives_removal_of_generating_ability() {
    let mut base = object("test.self_animator", &["Artifact"], None);
    base.abilities.push(generator());
    let mut env = Env::default().with(o(1), base);
    env.model_generators = true;
    let mut r = ContinuousEffects::new();
    r.register_group(
        &env,
        group(vec![
            (
                1,
                stat(
                    o(1),
                    Layer::L4,
                    1,
                    Affected::Selector(self_ref()),
                    add_types(&["Creature"], &[]),
                ),
            ),
            (2, stat(o(1), Layer::L7c, 1, same(Layer::L4), modify(3, 3))),
            (
                3,
                stat(
                    o(1),
                    Layer::L7c,
                    1,
                    Affected::Selector(self_ref()),
                    modify(10, 10),
                ),
            ),
        ]),
    )
    .unwrap();
    // Without the removal both 7c parts apply.
    assert_eq!(eval_pt(&r, &env, o(1)), Some((13, 13)));
    // A layer-6 effect removes every ability, the generator included.
    r.register(
        e(4),
        res(
            Layer::L6,
            2,
            objects(&[o(1)]),
            LayerModification::RemoveAllAbilities,
        ),
    )
    .unwrap();
    let (out, record) = evaluate_with_order(&r, &env, o(1)).unwrap();
    assert!(out.characteristics.card_types.contains("Creature"));
    assert_eq!(pt_of(&out.characteristics), Some((3, 3)));
    let applied = |id: u64| {
        record
            .steps
            .iter()
            .find(|s| s.item == OrderItem::Effect(e(id)))
            .map(|s| s.applied)
    };
    assert_eq!(applied(1), Some(true));
    assert_eq!(applied(2), Some(true));
    assert_eq!(applied(3), Some(false));
}

// REQ-LAYER-003, CR 613.6: when the anchor does not apply (its generating ability is already
// gone in layer 4), the linked part does not apply either: the order record shows both the
// anchor step and the linked layer-7b step as not applied.
#[test]
fn lockin_anchor_without_generator_links_nothing() {
    let mut env = Env::default().with(o(1), object("test.no_generator", &["Artifact"], None));
    env.model_generators = true;
    let mut r = ContinuousEffects::new();
    r.register_group(
        &env,
        group(vec![
            (
                1,
                stat(
                    o(1),
                    Layer::L4,
                    1,
                    Affected::Selector(self_ref()),
                    add_types(&["Creature"], &[]),
                ),
            ),
            (2, stat(o(1), Layer::L7b, 1, same(Layer::L4), set_pt(5, 5))),
        ]),
    )
    .unwrap();
    let (out, record) = evaluate_with_order(&r, &env, o(1)).unwrap();
    assert!(!out.characteristics.card_types.contains("Creature"));
    assert_eq!(pt_of(&out.characteristics), None);
    let applied = |id: u64| {
        record
            .steps
            .iter()
            .find(|s| s.item == OrderItem::Effect(e(id)))
            .map(|s| s.applied)
    };
    assert_eq!(applied(1), Some(false));
    assert_eq!(applied(2), Some(false));
}

// ---------------------------------------------------------------------------------------------
// Empty sets, links and errors
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-003, CR 613.6, 611.2c (INFERENCE, docs/DECISIONS.md): an anchor whose set is empty
// locks its linked parts to the empty set, for a resolved and for a static group.
#[test]
fn empty_anchor_locks_to_empty_set() {
    let env = creature_env().with(o(2), object("test.plain_land", &["Land"], None));
    let parts = |make: Make| {
        group(vec![
            (
                1,
                make(
                    Layer::L4,
                    1,
                    Affected::Selector(of_type("Artifact")),
                    add_types(&["Creature"], &[]),
                ),
            ),
            (2, make(Layer::L7b, 1, same(Layer::L4), set_pt(9, 9))),
        ])
    };
    let mut resolved = ContinuousEffects::new();
    resolved.register_group(&env, parts(res)).unwrap();
    assert_eq!(resolved.get(e(1)).unwrap().affected, objects(&[]));
    let mut statik = ContinuousEffects::new();
    statik.register_group(&env, parts(static_of_o9)).unwrap();
    for r in [&resolved, &statik] {
        assert_eq!(eval_pt(r, &env, o(1)), Some((2, 2)));
        assert!(!is_creature(r, &env, o(2)));
        assert_eq!(eval_pt(r, &env, o(2)), None);
    }
}

// REQ-LAYER-003, CR 613.6: BadLink for a missing anchor, an anchor in the same or a later
// layer or sublayer, and an anchor that is itself a SameAsLayer part; the registry is unchanged.
#[test]
fn bad_links_are_rejected() {
    let env = creature_env();
    let mut r = ContinuousEffects::new();
    r.register(e(100), res(Layer::L7c, 1, objects(&[o(1)]), modify(1, 1)))
        .unwrap();
    let before = r.clone();
    let cases: Vec<(Vec<(EffectId, ContinuousEffect)>, u64)> = vec![
        // Missing anchor.
        (
            group(vec![(1, res(Layer::L7b, 1, same(Layer::L4), set_pt(1, 1)))]),
            1,
        ),
        // Anchor in the same layer.
        (
            group(vec![
                (
                    1,
                    res(
                        Layer::L4,
                        1,
                        objects(&[o(1)]),
                        add_types(&["Artifact"], &[]),
                    ),
                ),
                (
                    2,
                    res(Layer::L4, 1, same(Layer::L4), add_types(&["Land"], &[])),
                ),
            ]),
            2,
        ),
        // Anchor in a later layer.
        (
            group(vec![
                (1, res(Layer::L7b, 1, objects(&[o(1)]), set_pt(1, 1))),
                (
                    2,
                    res(Layer::L4, 1, same(Layer::L7b), add_types(&["Land"], &[])),
                ),
            ]),
            2,
        ),
        // Anchor in a later sublayer of layer 7.
        (
            group(vec![
                (1, res(Layer::L7c, 1, objects(&[o(1)]), modify(1, 1))),
                (2, res(Layer::L7b, 1, same(Layer::L7c), set_pt(1, 1))),
            ]),
            2,
        ),
        // The only part of the named layer is itself linked.
        (
            group(vec![
                (
                    1,
                    res(
                        Layer::L4,
                        1,
                        objects(&[o(1)]),
                        add_types(&["Artifact"], &[]),
                    ),
                ),
                (
                    2,
                    res(
                        Layer::L5,
                        1,
                        same(Layer::L4),
                        LayerModification::SetColors(ColorBitmask::RED),
                    ),
                ),
                (3, res(Layer::L7c, 1, same(Layer::L5), modify(1, 1))),
            ]),
            3,
        ),
    ];
    for (parts, bad) in cases {
        assert_eq!(
            r.register_group(&env, parts),
            Err(LayerError::BadLink(e(bad)))
        );
        assert_eq!(r, before);
    }
}

// REQ-LAYER-003: empty, duplicate (inside the group and already registered) and mismatched
// groups are rejected; the registry is unchanged; plain `register` still rejects SameAsLayer
// and never locks a selector.
#[test]
fn group_errors_leave_registry_unchanged() {
    let env = creature_env();
    let mut r = ContinuousEffects::new();
    r.register(e(5), res(Layer::L7c, 1, objects(&[o(1)]), modify(1, 1)))
        .unwrap();
    let before = r.clone();
    assert_eq!(
        r.register_group(&env, Vec::new()),
        Err(LayerError::Unsupported("empty effect group"))
    );
    assert_eq!(r, before);
    let dup_inside = group(vec![
        (1, res(Layer::L7c, 1, objects(&[o(1)]), modify(1, 1))),
        (
            1,
            res(Layer::L7d, 1, objects(&[o(1)]), LayerModification::SwitchPt),
        ),
    ]);
    assert_eq!(
        r.register_group(&env, dup_inside),
        Err(LayerError::DuplicateEffect(e(1)))
    );
    assert_eq!(r, before);
    let dup_registered = group(vec![
        (1, res(Layer::L7c, 1, objects(&[o(1)]), modify(1, 1))),
        (
            5,
            res(Layer::L7d, 1, objects(&[o(1)]), LayerModification::SwitchPt),
        ),
    ]);
    assert_eq!(
        r.register_group(&env, dup_registered),
        Err(LayerError::DuplicateEffect(e(5)))
    );
    assert_eq!(r, before);
    let mismatch = group(vec![
        (
            1,
            res(
                Layer::L4,
                1,
                objects(&[o(1)]),
                add_types(&["Artifact"], &[]),
            ),
        ),
        (2, res(Layer::L7c, 1, same(Layer::L4), set_pt(1, 1))),
    ]);
    assert_eq!(
        r.register_group(&env, mismatch),
        Err(LayerError::LayerMismatch { layer: Layer::L7c })
    );
    assert_eq!(r, before);
    assert!(matches!(
        r.register(e(9), res(Layer::L7b, 1, same(Layer::L4), set_pt(1, 1))),
        Err(LayerError::Unsupported(_))
    ));
    assert_eq!(r, before);
    // Plain `register` never locks a resolved selector.
    r.register(
        e(6),
        res(Layer::L7c, 1, Affected::Selector(creatures()), modify(1, 1)),
    )
    .unwrap();
    assert_eq!(
        r.get(e(6)).unwrap().affected,
        Affected::Selector(creatures())
    );
}

// REQ-LAYER-003, CR 611.2c: an object the environment lists without a base makes locking fail
// with UnknownObject; resolve_affected_set and register_group report it, nothing registered.
#[test]
fn locking_reports_unknown_object() {
    let mut env = creature_env();
    env.phantom.push(o(7));
    let r0 = ContinuousEffects::new();
    assert_eq!(
        resolve_affected_set(&r0, &env, None, p(1), &creatures()),
        Err(LayerError::UnknownObject(o(7)))
    );
    let mut r = ContinuousEffects::new();
    assert_eq!(
        r.register_group(&env, control_parts()),
        Err(LayerError::UnknownObject(o(7)))
    );
    assert!(r.is_empty());
    // A static part needs no locking: the same environment accepts it.
    r.register_group(
        &env,
        group(vec![(
            1,
            stat(
                o(1),
                Layer::L7c,
                1,
                Affected::Selector(creatures()),
                modify(1, 1),
            ),
        )]),
    )
    .unwrap();
    assert_eq!(r.len(), 1);
}

// REQ-LAYER-003, CR 611.2c: resolve_affected_set evaluates each object with the registry
// (an object made a creature by an effect is included) and passes source and controller.
#[test]
fn resolve_affected_set_uses_evaluated_view() {
    let env = artifacts_env();
    let mut r = ContinuousEffects::new();
    assert_eq!(
        resolve_affected_set(&r, &env, None, p(1), &creatures()),
        Ok([o(2)].into_iter().collect())
    );
    r.register(
        e(1),
        res(
            Layer::L4,
            1,
            objects(&[o(3)]),
            add_types(&["Creature"], &[]),
        ),
    )
    .unwrap();
    assert_eq!(
        resolve_affected_set(&r, &env, None, p(1), &creatures()),
        Ok([o(2), o(3)].into_iter().collect())
    );
    assert_eq!(
        resolve_affected_set(&r, &env, Some(o(3)), p(1), &self_ref()),
        Ok([o(3)].into_iter().collect())
    );
    assert_eq!(
        resolve_affected_set(&r, &env, None, p(1), &self_ref()),
        Ok(BTreeSet::new())
    );
}

// REQ-LAYER-003, CR 613.6: removing an anchor leaves its linked part registered but applying to
// nothing; removing a linked part removes its link; a reused anchor id links nothing.
#[test]
fn removing_an_anchor_unlinks_its_parts() {
    let env = artifacts_env();
    let mut r = ContinuousEffects::new();
    r.register_group(&env, animate_parts(res)).unwrap();
    assert!(r.remove(e(1)));
    assert!(r.get(e(2)).is_some());
    assert_eq!(eval_pt(&r, &env, o(1)), None);
    assert!(r.remove(e(2)));
    assert!(r.is_empty());
    assert_eq!(r, ContinuousEffects::new());
    r.register_group(&env, animate_parts(res)).unwrap();
    assert!(r.remove(e(1)));
    r.register(
        e(1),
        res(
            Layer::L4,
            1,
            objects(&[o(3)]),
            add_types(&["Creature"], &[]),
        ),
    )
    .unwrap();
    assert_eq!(eval_pt(&r, &env, o(3)), Some((0, 0)));
    // Removing the linked part of an intact group leaves the anchor working.
    let mut r = ContinuousEffects::new();
    r.register_group(&env, animate_parts(res)).unwrap();
    assert!(r.remove(e(2)));
    assert!(is_creature(&r, &env, o(1)));
    assert_eq!(eval_pt(&r, &env, o(1)), Some((0, 0)));
}

// ---------------------------------------------------------------------------------------------
// Sublayers, independence, ordering
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-003, CR 613.6: a chain anchored in 7a continues in 7b, 7c and 7d; the linked steps
// appear in the order record as applied steps; a non-matching object gets none of them.
#[test]
fn sublayer_chain_7a_to_7d() {
    let env = creature_env()
        .with(o(2), object("test.plain_artifact", &["Artifact"], None))
        .with(o(9), object("test.static_source", &["Enchantment"], None));
    let mut anchor = stat(
        o(9),
        Layer::L7a,
        1,
        Affected::Selector(creatures()),
        set_pt(1, 1),
    );
    anchor.is_cda = true;
    let mut r = ContinuousEffects::new();
    r.register_group(
        &env,
        group(vec![
            (
                4,
                stat(
                    o(9),
                    Layer::L7d,
                    1,
                    same(Layer::L7a),
                    LayerModification::SwitchPt,
                ),
            ),
            (3, stat(o(9), Layer::L7c, 1, same(Layer::L7a), modify(1, 1))),
            (
                2,
                stat(
                    o(9),
                    Layer::L7b,
                    1,
                    same(Layer::L7a),
                    LayerModification::SetPt {
                        power: Some(5),
                        toughness: None,
                    },
                ),
            ),
            (1, anchor),
        ]),
    )
    .unwrap();
    let (out, record) = evaluate_with_order(&r, &env, o(1)).unwrap();
    // 2/2 -> 7a 1/1 -> 7b 5/1 -> 7c 6/2 -> 7d 2/6.
    assert_eq!(pt_of(&out.characteristics), Some((2, 6)));
    let steps: Vec<(Layer, OrderItem, bool)> = record
        .steps
        .iter()
        .map(|s| (s.layer, s.item.clone(), s.applied))
        .collect();
    assert_eq!(
        steps,
        vec![
            (Layer::L7a, OrderItem::Effect(e(1)), true),
            (Layer::L7b, OrderItem::Effect(e(2)), true),
            (Layer::L7c, OrderItem::Effect(e(3)), true),
            (Layer::L7d, OrderItem::Effect(e(4)), true),
        ]
    );
    let (out2, record2) = evaluate_with_order(&r, &env, o(2)).unwrap();
    assert_eq!(pt_of(&out2.characteristics), None);
    assert_eq!(record2.steps.len(), 4);
    assert!(record2.steps.iter().all(|s| !s.applied));
}

// REQ-LAYER-003, CR 611.2c: each part of a resolved group is locked independently, against the
// registry as it was before the group (the linked pair and an independent selector part).
#[test]
fn resolved_group_parts_locked_independently() {
    let env = Env::default()
        .with(o(1), object("test.plain_artifact", &["Artifact"], None))
        .with(
            o(2),
            object("test.creature_2_2", &["Creature"], Some((2, 2))),
        );
    let mut r = ContinuousEffects::new();
    r.register_group(
        &env,
        group(vec![
            (
                1,
                res(
                    Layer::L4,
                    1,
                    Affected::Selector(of_type("Artifact")),
                    add_types(&["Creature"], &[]),
                ),
            ),
            (2, res(Layer::L7b, 1, same(Layer::L4), set_pt(2, 2))),
            (
                3,
                res(
                    Layer::L5,
                    1,
                    Affected::Selector(creatures()),
                    LayerModification::SetColors(ColorBitmask::WHITE),
                ),
            ),
        ]),
    )
    .unwrap();
    assert_eq!(r.get(e(1)).unwrap().affected, objects(&[o(1)]));
    assert_eq!(r.get(e(3)).unwrap().affected, objects(&[o(2)]));
    let a = eval(&r, &env, o(1)).characteristics;
    assert_eq!(pt_of(&a), Some((2, 2)));
    assert_eq!(a.colors, ColorBitmask::BLACK);
    let c = eval(&r, &env, o(2)).characteristics;
    assert_eq!(pt_of(&c), Some((2, 2)));
    assert_eq!(c.colors, ColorBitmask::WHITE);
    assert!(!c.card_types.contains("Artifact"));
}

// REQ-LAYER-003, CR 613.6, 613.8a: the dependency trial uses the lock-in membership. A linked
// layer-4 part does not depend on a later layer-4 effect that would stop the object matching a
// selector, so it applies first by timestamp; with a plain selector it depends on that effect.
#[test]
fn dependency_trial_uses_locked_membership() {
    let env = creature_env().with(o(9), object("test.static_source", &["Enchantment"], None));
    let make = |linked: bool| {
        let affected = if linked {
            same(Layer::L2)
        } else {
            Affected::Selector(creatures())
        };
        let mut r = ContinuousEffects::new();
        r.register_group(
            &env,
            group(vec![
                (
                    1,
                    stat(
                        o(9),
                        Layer::L2,
                        1,
                        Affected::Selector(creatures()),
                        LayerModification::SetController(p(2)),
                    ),
                ),
                (
                    2,
                    stat(o(9), Layer::L4, 1, affected, add_types(&["Artifact"], &[])),
                ),
            ]),
        )
        .unwrap();
        r.register(
            e(3),
            res(
                Layer::L4,
                2,
                objects(&[o(1)]),
                LayerModification::SetTypes {
                    supertypes: None,
                    card_types: Some(strings(&["Land"])),
                    subtypes: None,
                },
            ),
        )
        .unwrap();
        r
    };
    let layer4 = |r: &ContinuousEffects| -> Vec<(OrderItem, Selection, bool)> {
        evaluate_with_order(r, &env, o(1))
            .unwrap()
            .1
            .steps
            .into_iter()
            .filter(|s| s.layer == Layer::L4)
            .map(|s| (s.item, s.selection, s.applied))
            .collect()
    };
    assert_eq!(
        layer4(&make(true)),
        vec![
            (OrderItem::Effect(e(2)), Selection::Timestamp, true),
            (OrderItem::Effect(e(3)), Selection::Timestamp, true),
        ]
    );
    assert_eq!(
        layer4(&make(false)),
        vec![
            (OrderItem::Effect(e(3)), Selection::Dependency, true),
            (OrderItem::Effect(e(2)), Selection::Timestamp, false),
        ]
    );
    let out = eval(&make(true), &env, o(1));
    assert_eq!(out.controller, Some(p(2)));
    assert_eq!(out.characteristics.card_types, strings(&["Land"]));
}

// ---------------------------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------------------------

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let first = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, first.clone());
            out.push(tail);
        }
    }
    out
}

// REQ-LAYER-003, REQ-CORE-001, CR 613.6, 611.2c, 611.3a: every order of the parts of a group
// gives the same registry and the same evaluation of every object (property over all 120
// permutations, for a resolved and a static group with two parts in the anchor layer), and
// evaluate_all equals per-object evaluate.
#[test]
fn part_order_never_changes_result() {
    let env = artifacts_env().with(o(9), object("test.static_source", &["Enchantment"], None));
    let forms: [(Make, bool); 2] = [(res, false), (static_of_o9, true)];
    for (make, is_static) in forms {
        let parts = group(vec![
            (
                3,
                make(
                    Layer::L4,
                    1,
                    Affected::Selector(noncreature_artifacts()),
                    add_types(&["Creature"], &[]),
                ),
            ),
            (
                5,
                make(
                    Layer::L4,
                    1,
                    Affected::Selector(of_type("Land")),
                    add_types(&["Artifact"], &[]),
                ),
            ),
            (4, make(Layer::L7b, 1, same(Layer::L4), set_pt(2, 2))),
            (
                1,
                make(
                    Layer::L5,
                    1,
                    same(Layer::L4),
                    LayerModification::SetColors(ColorBitmask::WHITE),
                ),
            ),
            (
                2,
                make(Layer::L7c, 1, Affected::Selector(creatures()), modify(1, 0)),
            ),
        ]);
        let mut reference: Option<(ContinuousEffects, BTreeMap<ObjectId, Evaluated>)> = None;
        for order in permutations(&parts) {
            let mut r = ContinuousEffects::new();
            r.register_group(&env, order).unwrap();
            let all = evaluate_all(&r, &env).unwrap();
            for (id, value) in &all {
                assert_eq!(&evaluate(&r, &env, *id).unwrap(), value);
            }
            match &reference {
                None => reference = Some((r, all)),
                Some((r0, all0)) => {
                    assert_eq!(&r, r0);
                    assert_eq!(&all, all0);
                }
            }
        }
        let (_, all) = reference.unwrap();
        let view = |id: u64| &all[&o(id)].characteristics;
        // The anchor is the lowest-id layer-4 part (e3).
        assert_eq!(view(1).colors, ColorBitmask::WHITE);
        assert_eq!(pt_of(view(2)), Some((2, 1)));
        assert!(view(3).card_types.contains("Artifact"));
        if is_static {
            // CR 611.3a: matched in the evaluation; o3 becomes an artifact (e5) and then a
            // noncreature artifact animated by e3 (dependency, CR 613.8a), linked parts follow.
            assert_eq!(pt_of(view(1)), Some((3, 2)));
            assert_eq!(view(3).colors, ColorBitmask::WHITE);
            assert_eq!(pt_of(view(3)), Some((3, 2)));
        } else {
            // CR 611.2c: each part locked against the registry before the group: e2 holds o2
            // only, e3 holds o1 only.
            assert_eq!(pt_of(view(1)), Some((2, 2)));
            assert_eq!(view(3).colors, ColorBitmask::BLACK);
            assert_eq!(pt_of(view(3)), None);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Links follow only the first-layer anchor (CR 613.6, 611.2c; INFERENCE, docs/DECISIONS.md)
// ---------------------------------------------------------------------------------------------

/// o1 artifact creature 1/1 (set A), o2 land creature 2/2 (set B), o3 bystander creature 3/3,
/// o9 the static source.
fn chains_env() -> Env {
    Env::default()
        .with(
            o(1),
            object(
                "test.artifact_creature_1_1",
                &["Artifact", "Creature"],
                Some((1, 1)),
            ),
        )
        .with(
            o(2),
            object(
                "test.land_creature_2_2",
                &["Land", "Creature"],
                Some((2, 2)),
            ),
        )
        .with(
            o(3),
            object("test.creature_3_3", &["Creature"], Some((3, 3))),
        )
        .with(o(9), object("test.static_source", &["Enchantment"], None))
}

/// L4 Selector(artifacts) (the first layer), L5 independent Selector(lands), L7c +1/+1 linked
/// to `linked_to`.
fn chains_parts(make: Make, linked_to: Layer) -> Vec<(EffectId, ContinuousEffect)> {
    group(vec![
        (
            1,
            make(
                Layer::L4,
                1,
                Affected::Selector(of_type("Artifact")),
                add_types(&[], &["Synthkin"]),
            ),
        ),
        (
            2,
            make(
                Layer::L5,
                1,
                Affected::Selector(of_type("Land")),
                LayerModification::SetColors(ColorBitmask::WHITE),
            ),
        ),
        (3, make(Layer::L7c, 1, same(linked_to), modify(1, 1))),
    ])
}

// REQ-LAYER-003, CR 613.6, 611.2c: a link to an independent part that is not in the group's
// first layer is rejected with BadLink (static and resolved groups); the registry is unchanged.
#[test]
fn link_to_non_first_layer_is_rejected() {
    let env = chains_env();
    let mut r = ContinuousEffects::new();
    r.register(e(100), res(Layer::L7c, 1, objects(&[o(3)]), modify(1, 1)))
        .unwrap();
    let before = r.clone();
    for make in [static_of_o9 as Make, res] {
        assert_eq!(
            r.register_group(&env, chains_parts(make, Layer::L5)),
            Err(LayerError::BadLink(e(3)))
        );
        assert_eq!(r, before);
    }
}

// REQ-LAYER-003, CR 613.6, 611.2c: with the link on the first layer (L4), the L7c part applies
// exactly to what the L4 part applied to; the independent L5 part keeps its own set (B only).
#[test]
fn link_to_first_layer_with_independent_middle_part() {
    let env = chains_env();
    let mut r = ContinuousEffects::new();
    r.register_group(&env, chains_parts(static_of_o9, Layer::L4))
        .unwrap();
    for (id, l4, l5, pt, white) in [
        (1, true, false, (2, 2), false),
        (2, false, true, (2, 2), true),
        (3, false, false, (3, 3), false),
    ] {
        let (out, record) = evaluate_with_order(&r, &env, o(id)).unwrap();
        let applied = |eid: u64| {
            record
                .steps
                .iter()
                .find(|s| s.item == OrderItem::Effect(e(eid)))
                .map(|s| s.applied)
        };
        assert_eq!(applied(1), Some(l4), "o{id} L4");
        assert_eq!(applied(2), Some(l5), "o{id} L5");
        // CR 613.6: the linked part follows the L4 anchor exactly.
        assert_eq!(applied(3), Some(l4), "o{id} L7c");
        assert_eq!(pt_of(&out.characteristics), Some(pt), "o{id}");
        assert_eq!(out.characteristics.colors == ColorBitmask::WHITE, white);
    }
}

// REQ-LAYER-003, CR 613.6: several linked parts (L6 and L7c) may follow the same first-layer
// anchor, also with an independent part in between.
#[test]
fn several_links_to_first_layer() {
    let env = chains_env();
    let mut parts = chains_parts(static_of_o9, Layer::L4);
    parts.push((
        e(4),
        static_of_o9(
            Layer::L6,
            1,
            same(Layer::L4),
            LayerModification::RemoveAllAbilities,
        ),
    ));
    let mut r = ContinuousEffects::new();
    r.register_group(&env, parts).unwrap();
    let (out, record) = evaluate_with_order(&r, &env, o(1)).unwrap();
    assert_eq!(pt_of(&out.characteristics), Some((2, 2)));
    for id in [1, 3, 4] {
        assert!(
            record
                .steps
                .iter()
                .any(|s| s.item == OrderItem::Effect(e(id)) && s.applied),
            "e{id}"
        );
    }
    let (_, record2) = evaluate_with_order(&r, &env, o(2)).unwrap();
    for id in [1, 3, 4] {
        assert!(
            record2
                .steps
                .iter()
                .all(|s| s.item != OrderItem::Effect(e(id)) || !s.applied),
            "e{id}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Duplicate ids are reported before layer mismatches (REQ-CORE-001; docs/DECISIONS.md)
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-003, REQ-CORE-001, CR 613.6: two parts sharing an id, one valid and one with a
// layer mismatch, give DuplicateEffect in both caller orders; a registry duplicate combined
// with a mismatch on another (lower-id) part gives DuplicateEffect in both caller orders; the
// registry is unchanged after each rejection.
#[test]
fn duplicate_reported_before_mismatch_in_any_order() {
    let env = creature_env();
    let mut r = ContinuousEffects::new();
    r.register(e(5), res(Layer::L7c, 1, objects(&[o(1)]), modify(1, 1)))
        .unwrap();
    let before = r.clone();
    let valid = res(Layer::L7c, 1, objects(&[o(1)]), modify(1, 1));
    // set_pt belongs to layer 7b, not 7c.
    let mismatch = res(Layer::L7c, 1, objects(&[o(1)]), set_pt(1, 1));
    let cases = [
        (vec![(1, valid.clone()), (1, mismatch.clone())], 1),
        (vec![(1, mismatch.clone()), (1, valid.clone())], 1),
        (vec![(1, mismatch.clone()), (5, valid.clone())], 5),
        (vec![(5, valid), (1, mismatch)], 5),
    ];
    for (parts, dup) in cases {
        assert_eq!(
            r.register_group(&env, group(parts)),
            Err(LayerError::DuplicateEffect(e(dup)))
        );
        assert_eq!(r, before);
    }
}

// REQ-LAYER-003: the BadLink Display arm names the part.
#[test]
fn bad_link_display() {
    assert_eq!(
        LayerError::BadLink(e(7)).to_string(),
        "effect 7 links to no part of an earlier layer of its group"
    );
}
