// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the ordering within layers: REQ-LAYER-002 (CDA-first, dependency by trial
// application, selection, dependency loops with recomputation, the order record), with
// REQ-LAYER-001 and REQ-CORE-001 preserved. Synthetic ids, synthetic characteristics and
// neutral names only (REQ-TEST-001).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST, SelectorFilter, WordRef};

use super::*;
use crate::objects::Characteristics;

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A synthetic creature with the given P/T and abilities.
fn creature(power: i64, toughness: i64, abilities: Vec<AbilityAST>) -> Characteristics {
    Characteristics {
        names: set(["test.ordering_creature"]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::BLACK,
        supertypes: BTreeSet::new(),
        card_types: set(["Creature"]),
        subtypes: set(["Testkin"]),
        rules_text: TextTokenStream(Vec::new()),
        abilities,
        power: Some(literal(power)),
        toughness: Some(literal(toughness)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A synthetic artifact without P/T or subtypes.
fn artifact() -> Characteristics {
    Characteristics {
        names: set(["test.ordering_artifact"]),
        card_types: set(["Artifact"]),
        subtypes: BTreeSet::new(),
        power: None,
        toughness: None,
        ..creature(0, 0, Vec::new())
    }
}

/// A keyword ability.
fn kw(keyword: &str) -> AbilityAST {
    AbilityAST(
        [
            ("kind", "KEYWORD".to_owned()),
            ("abilityId", format!("test.kw.{keyword}")),
            ("keyword", keyword.to_owned()),
        ]
        .into_iter()
        .collect(),
    )
}

/// A non-keyword ability.
fn ability(tag: &str) -> AbilityAST {
    AbilityAST(format!("test.ability.{tag}").into())
}

fn filter(pred: PredicateAST) -> Affected {
    Affected::Selector(ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    }))
}

fn has_subtype(s: &str) -> PredicateAST {
    PredicateAST::HasSubtype {
        value: WordRef::Name(s.to_owned()),
    }
}

fn has_flying() -> PredicateAST {
    PredicateAST::HasKeyword {
        keyword: "flying".to_owned(),
    }
}

fn not(q: PredicateAST) -> PredicateAST {
    PredicateAST::Not(Box::new(q))
}

fn only(id: ObjectId) -> Affected {
    Affected::Objects([id].into_iter().collect())
}

fn add_subtypes<const N: usize>(s: [&str; N]) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: BTreeSet::new(),
        subtypes: set(s),
    }
}

fn add_card_types<const N: usize>(s: [&str; N]) -> LayerModification {
    LayerModification::AddTypes {
        supertypes: BTreeSet::new(),
        card_types: set(s),
        subtypes: BTreeSet::new(),
    }
}

fn set_pt(power: i64, toughness: i64) -> LayerModification {
    LayerModification::SetPt {
        power: Some(power),
        toughness: Some(toughness),
    }
}

fn modify_pt(power: i64, toughness: i64) -> LayerModification {
    LayerModification::ModifyPt { power, toughness }
}

fn text(word: &str) -> LayerModification {
    LayerModification::SetText(TextTokenStream(vec![word.into()]))
}

fn eff(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: p(1),
        timestamp: Timestamp::new(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected,
        modification: m,
    }
}

fn cda(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        is_cda: true,
        origin: EffectOrigin::Static { granted_at: None },
        ..eff(layer, t, affected, m)
    }
}

fn registry(list: &[(u64, ContinuousEffect)]) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    for (id, effect) in list {
        r.register(e(*id), effect.clone()).unwrap();
    }
    r
}

/// A synthetic environment.
#[derive(Default, Clone)]
struct Env {
    bases: BTreeMap<ObjectId, ObjectBase>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    counter_ts: BTreeMap<(ObjectId, String), Timestamp>,
    /// Effect id to the keyword its generating ability is (CR 613.8a existence): the ability
    /// exists while the source, seen through `source_view`, has that keyword.
    generators: BTreeMap<EffectId, String>,
    /// Every `generating_ability_exists` call: (effect, source, whether `source_view` was given).
    generator_calls: RefCell<Vec<(EffectId, ObjectId, bool)>>,
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

    fn counter(mut self, id: ObjectId, kind: &str, amount: i64, t: u64) -> Env {
        self.counters
            .entry(id)
            .or_default()
            .insert(kind.to_owned(), amount);
        self.counter_ts
            .insert((id, kind.to_owned()), Timestamp::new(t));
        self
    }

    fn generator(mut self, effect: EffectId, keyword: &str) -> Env {
        self.generators.insert(effect, keyword.to_owned());
        self
    }
}

fn holds(pred: &PredicateAST, view: &Evaluated) -> bool {
    let c = &view.characteristics;
    match pred {
        PredicateAST::Not(q) => !holds(q, view),
        PredicateAST::IsCreature => c.card_types.contains("Creature"),
        PredicateAST::HasSubtype {
            value: WordRef::Name(s),
        } => c.subtypes.contains(s),
        PredicateAST::HasKeyword { keyword } => has_kw(c, keyword),
        _ => false,
    }
}

impl LayerEnvironment for Env {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn counter_timestamp(&self, id: ObjectId, kind: &str) -> Option<Timestamp> {
        self.counter_ts.get(&(id, kind.to_owned())).copied()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        match selector {
            ObjectSelectorAST::Filter(f) => f
                .predicate
                .as_ref()
                .is_some_and(|q| holds(q, ctx.candidate_view)),
            ObjectSelectorAST::Ref(_) => false,
        }
    }

    fn generating_ability_exists(
        &self,
        effect: EffectId,
        source: ObjectId,
        source_view: Option<&Evaluated>,
    ) -> bool {
        self.generator_calls
            .borrow_mut()
            .push((effect, source, source_view.is_some()));
        match (self.generators.get(&effect), source_view) {
            (Some(keyword), Some(view)) => has_kw(&view.characteristics, keyword),
            _ => true,
        }
    }
}

fn run(list: &[(u64, ContinuousEffect)], env: &Env, id: ObjectId) -> (Evaluated, OrderRecord) {
    let r = registry(list);
    let out = evaluate_with_order(&r, env, id).unwrap();
    // REQ-LAYER-002: `evaluate` returns exactly the Evaluated of `evaluate_with_order`.
    assert_eq!(evaluate(&r, env, id).unwrap(), out.0);
    out
}

/// The record's steps of `layer` as (item, selection, applied).
fn steps_in(record: &OrderRecord, layer: Layer) -> Vec<(OrderItem, Selection, bool)> {
    record
        .steps
        .iter()
        .filter(|s| s.layer == layer)
        .map(|s| (s.item.clone(), s.selection, s.applied))
        .collect()
}

fn fx(n: u64) -> OrderItem {
    OrderItem::Effect(e(n))
}

fn counter_item(kind: &str) -> OrderItem {
    OrderItem::Counter(kind.to_owned())
}

fn pt(c: &Characteristics) -> Option<(i64, i64)> {
    Some((
        literal_of(c.power.as_ref()?)?,
        literal_of(c.toughness.as_ref()?)?,
    ))
}

fn has_kw(c: &Characteristics, keyword: &str) -> bool {
    c.abilities
        .iter()
        .any(|a| a.0.get("keyword").and_then(|k| k.as_str()) == Some(keyword))
}

fn layer_index(layer: Layer) -> usize {
    Layer::ALL.iter().position(|l| *l == layer).unwrap()
}

/// A small deterministic linear congruential generator (no rand crate, no clock).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn small(&mut self) -> i64 {
        i64::try_from(self.below(5)).unwrap()
    }

    fn pick(&mut self, xs: &[&str]) -> String {
        xs[usize::try_from(self.below(xs.len() as u64)).unwrap()].to_owned()
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = usize::try_from(self.below(i as u64 + 1)).unwrap();
            v.swap(i, j);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// CDA-first in layers 2-6 (CR 613.3)
// ---------------------------------------------------------------------------------------------

/// Runs a non-CDA (effect 1, timestamp 1) and a later CDA (effect 2, timestamp 5) in `layer` and
/// asserts the CDA is used first, labelled `CdaFirst`, then the non-CDA by timestamp.
fn cda_first_case(
    layer: Layer,
    plain: LayerModification,
    defining: LayerModification,
) -> Evaluated {
    let env = Env::default().with(o(1), creature(2, 2, vec![ability("printed")]));
    let (ev, rec) = run(
        &[
            (1, eff(layer, 1, only(o(1)), plain)),
            (2, cda(layer, 5, only(o(1)), defining)),
        ],
        &env,
        o(1),
    );
    assert_eq!(
        steps_in(&rec, layer),
        vec![
            (fx(2), Selection::CdaFirst, true),
            (fx(1), Selection::Timestamp, true),
        ]
    );
    ev
}

/// REQ-LAYER-002, CR 613.3: layer 2, a later CDA SetController applies before an earlier
/// non-CDA one, so the non-CDA controller wins.
#[test]
fn req_layer_002_cda_first_layer_2() {
    let ev = cda_first_case(
        Layer::L2,
        LayerModification::SetController(p(3)),
        LayerModification::SetController(p(2)),
    );
    assert_eq!(ev.controller, Some(p(3)));
}

/// REQ-LAYER-002, CR 613.3: layer 3, a later CDA SetText applies before an earlier non-CDA one.
#[test]
fn req_layer_002_cda_first_layer_3() {
    let ev = cda_first_case(
        Layer::L3,
        text("test.word_plain"),
        text("test.word_defining"),
    );
    assert_eq!(
        ev.characteristics.rules_text,
        TextTokenStream(vec!["test.word_plain".into()])
    );
}

/// REQ-LAYER-002, CR 613.3: layer 4, a later CDA AddTypes applies before an earlier non-CDA
/// SetTypes, which then overwrites the subtypes.
#[test]
fn req_layer_002_cda_first_layer_4() {
    let ev = cda_first_case(
        Layer::L4,
        LayerModification::SetTypes {
            supertypes: None,
            card_types: None,
            subtypes: Some(set(["Plainkin"])),
        },
        add_subtypes(["Definedkin"]),
    );
    assert_eq!(ev.characteristics.subtypes, set(["Plainkin"]));
}

/// REQ-LAYER-002, CR 613.3: layer 5, a later CDA AddColors applies before an earlier non-CDA
/// SetColors.
#[test]
fn req_layer_002_cda_first_layer_5() {
    let ev = cda_first_case(
        Layer::L5,
        LayerModification::SetColors(ColorBitmask::RED),
        LayerModification::AddColors(ColorBitmask::GREEN),
    );
    assert_eq!(ev.characteristics.colors, ColorBitmask::RED);
}

/// REQ-LAYER-002, CR 613.3: layer 6, a later CDA AddAbility applies before an earlier non-CDA
/// RemoveAllAbilities, so the ability is removed.
#[test]
fn req_layer_002_cda_first_layer_6() {
    let ev = cda_first_case(
        Layer::L6,
        LayerModification::RemoveAllAbilities,
        LayerModification::AddAbility(ability("defined")),
    );
    assert!(ev.characteristics.abilities.is_empty());
}

/// REQ-LAYER-002, CR 613.3, 613.8b: within the CDA group the order is the selection procedure:
/// two CDAs apply by timestamp; both are labelled `CdaFirst` while the non-CDA is unapplied.
#[test]
fn req_layer_002_cdas_ordered_by_timestamp_among_themselves() {
    let env = Env::default().with(o(1), creature(2, 2, Vec::new()));
    let (ev, rec) = run(
        &[
            (
                1,
                eff(
                    Layer::L2,
                    1,
                    only(o(1)),
                    LayerModification::SetController(p(4)),
                ),
            ),
            (
                3,
                cda(
                    Layer::L2,
                    7,
                    only(o(1)),
                    LayerModification::SetController(p(3)),
                ),
            ),
            (
                2,
                cda(
                    Layer::L2,
                    6,
                    only(o(1)),
                    LayerModification::SetController(p(2)),
                ),
            ),
        ],
        &env,
        o(1),
    );
    assert_eq!(ev.controller, Some(p(4)));
    assert_eq!(
        steps_in(&rec, Layer::L2),
        vec![
            (fx(2), Selection::CdaFirst, true),
            (fx(3), Selection::CdaFirst, true),
            (fx(1), Selection::Timestamp, true),
        ]
    );
    // Without a non-CDA the CDAs are plain timestamp order.
    let (_, rec) = run(
        &[
            (
                3,
                cda(
                    Layer::L2,
                    7,
                    only(o(1)),
                    LayerModification::SetController(p(3)),
                ),
            ),
            (
                2,
                cda(
                    Layer::L2,
                    6,
                    only(o(1)),
                    LayerModification::SetController(p(2)),
                ),
            ),
        ],
        &env,
        o(1),
    );
    assert_eq!(
        steps_in(&rec, Layer::L2),
        vec![
            (fx(2), Selection::Timestamp, true),
            (fx(3), Selection::Timestamp, true),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// No CDA-first in layers 1a, 1b, 7a-7d (CR 613.2, 613.4, 613.4a)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 613.2, 613.4: in layers 1a, 1b, 7b, 7c and 7d `is_cda` does not reorder:
/// the earlier effect applies first whichever of the two is the CDA.
#[test]
fn req_layer_002_no_cda_first_in_layers_1_and_7() {
    let copy = |n: &str| {
        let mut c = creature(1, 1, Vec::new());
        c.names = set([n]);
        Box::new(c)
    };
    let cases: Vec<(Layer, LayerModification, LayerModification)> = vec![
        (
            Layer::L1a,
            LayerModification::CopyValues(copy("test.copy_early")),
            LayerModification::CopyValues(copy("test.copy_late")),
        ),
        (
            Layer::L1b,
            LayerModification::FaceDown(copy("test.face_early")),
            LayerModification::FaceDown(copy("test.face_late")),
        ),
        (Layer::L7b, set_pt(1, 1), set_pt(4, 5)),
        (Layer::L7c, modify_pt(1, 0), modify_pt(0, 2)),
        (
            Layer::L7d,
            LayerModification::SwitchPt,
            LayerModification::SwitchPt,
        ),
    ];
    for (layer, early, late) in cases {
        for cda_on_late in [true, false] {
            let mk = |is_cda: bool, t: u64, m: LayerModification| ContinuousEffect {
                is_cda,
                ..eff(layer, t, only(o(1)), m)
            };
            let env = Env::default().with(o(1), creature(2, 3, Vec::new()));
            let (ev, rec) = run(
                &[
                    (2, mk(!cda_on_late, 1, early.clone())),
                    (1, mk(cda_on_late, 5, late.clone())),
                ],
                &env,
                o(1),
            );
            assert_eq!(
                steps_in(&rec, layer),
                vec![
                    (fx(2), Selection::Timestamp, true),
                    (fx(1), Selection::Timestamp, true),
                ],
                "{layer:?}"
            );
            match layer {
                Layer::L1a => assert_eq!(ev.characteristics.names, set(["test.copy_late"])),
                Layer::L1b => assert_eq!(ev.characteristics.names, set(["test.face_late"])),
                Layer::L7b => assert_eq!(pt(&ev.characteristics), Some((4, 5))),
                Layer::L7c => assert_eq!(pt(&ev.characteristics), Some((3, 5))),
                _ => assert_eq!(pt(&ev.characteristics), Some((2, 3))),
            }
        }
    }
}

/// REQ-LAYER-002, CR 613.4a: layer 7a is ordered by the selection procedure alone; a non-CDA
/// registered in 7a is kept and applies in its timestamp place (docs/DECISIONS.md "#33 FP-032 —
/// Non-CDA effects in layer 7a").
#[test]
fn req_layer_002_layer_7a_non_cda_kept_in_timestamp_order() {
    let env = Env::default().with(o(1), creature(2, 3, Vec::new()));
    for (t_cda, t_plain, want, first) in [(1, 5, (7, 7), fx(1)), (5, 1, (6, 6), fx(2))] {
        let (ev, rec) = run(
            &[
                (1, cda(Layer::L7a, t_cda, only(o(1)), set_pt(6, 6))),
                (2, eff(Layer::L7a, t_plain, only(o(1)), set_pt(7, 7))),
            ],
            &env,
            o(1),
        );
        assert_eq!(pt(&ev.characteristics), Some(want));
        let layer = steps_in(&rec, Layer::L7a);
        assert_eq!(layer[0].0, first);
        assert!(layer.iter().all(|s| s.1 == Selection::Timestamp));
    }
}

// ---------------------------------------------------------------------------------------------
// Dependency (CR 613.8a, 613.8b)
// ---------------------------------------------------------------------------------------------

/// test.dep_remover (effect 1) and test.dep_dependent (effect 2): the dependent grants an
/// ability to objects with flying, which the remover removes.
fn remover_and_dependent(
    remover: LayerModification,
    t_remover: u64,
    t_dependent: u64,
) -> Vec<(u64, ContinuousEffect)> {
    vec![
        // test.dep_remover
        (1, eff(Layer::L6, t_remover, only(o(1)), remover)),
        // test.dep_dependent
        (
            2,
            eff(
                Layer::L6,
                t_dependent,
                filter(has_flying()),
                LayerModification::AddAbility(ability("dependent_grant")),
            ),
        ),
    ]
}

/// REQ-LAYER-002, CR 613.8a, 613.8b (SRS 6.2 dependency example, ability removal): the
/// dependent grant depends on the remover (applying the remover changes what the grant applies
/// to), so the remover applies first in both timestamp orders and the grant no longer applies.
#[test]
fn req_layer_002_dependency_ability_removal_both_orders() {
    let env = Env::default().with(o(1), creature(2, 2, vec![kw("flying")]));
    for remover in [
        LayerModification::RemoveAllAbilities,
        LayerModification::RemoveKeyword("flying".to_owned()),
    ] {
        for (t_remover, t_dependent) in [(1, 2), (2, 1)] {
            let (ev, rec) = run(
                &remover_and_dependent(remover.clone(), t_remover, t_dependent),
                &env,
                o(1),
            );
            assert!(!has_kw(&ev.characteristics, "flying"));
            assert!(
                !ev.characteristics
                    .abilities
                    .contains(&ability("dependent_grant")),
                "{remover:?} {t_remover} {t_dependent}"
            );
            let first = if t_remover < t_dependent {
                Selection::Timestamp
            } else {
                Selection::Dependency
            };
            assert_eq!(
                steps_in(&rec, Layer::L6),
                vec![(fx(1), first, true), (fx(2), Selection::Timestamp, false)]
            );
        }
    }
}

/// REQ-LAYER-002, CR 613.8a, 613.8b (SRS 6.2 dependency example, type change): Q (all
/// creatures gain a subtype) depends on P (makes the object a creature); both timestamp orders
/// apply Q after P, and Q applies.
#[test]
fn req_layer_002_dependency_type_change_both_orders() {
    let env = Env::default().with(o(1), artifact());
    for (t_p, t_q) in [(1, 2), (2, 1)] {
        let (ev, rec) = run(
            &[
                (
                    1,
                    eff(Layer::L4, t_p, only(o(1)), add_card_types(["Creature"])),
                ),
                (
                    2,
                    eff(
                        Layer::L4,
                        t_q,
                        filter(PredicateAST::IsCreature),
                        add_subtypes(["Granted"]),
                    ),
                ),
            ],
            &env,
            o(1),
        );
        assert!(ev.characteristics.card_types.contains("Creature"));
        assert!(ev.characteristics.subtypes.contains("Granted"));
        let first = if t_p < t_q {
            Selection::Timestamp
        } else {
            Selection::Dependency
        };
        assert_eq!(
            steps_in(&rec, Layer::L4),
            vec![(fx(1), first, true), (fx(2), Selection::Timestamp, true)]
        );
    }
}

/// REQ-LAYER-002, CR 613.7, 613.8a (SRS 6.2 timestamp, not dependency): two 7b setters do not
/// depend on each other (the earlier changes neither what the later applies to nor what it
/// does), so the later timestamp's value wins in both orders and no step is selected by
/// dependency or loop.
#[test]
fn req_layer_002_timestamp_not_dependency_both_orders() {
    let env = Env::default().with(o(1), creature(2, 2, Vec::new()));
    for (t1, t2, want) in [(1, 2, (0, 1)), (2, 1, (5, 5))] {
        let (ev, rec) = run(
            &[
                (1, eff(Layer::L7b, t1, only(o(1)), set_pt(5, 5))),
                (2, eff(Layer::L7b, t2, only(o(1)), set_pt(0, 1))),
            ],
            &env,
            o(1),
        );
        assert_eq!(pt(&ev.characteristics), Some(want));
        assert!(
            rec.steps
                .iter()
                .all(|s| !matches!(s.selection, Selection::Dependency | Selection::Loop))
        );
    }
}

/// Two effects in layer 4 that each stop the other from applying: `a` gives subtype `sa` to
/// objects without `sb`, `b` gives `sb` to objects without `sa`.
fn loop_pair(
    (id_a, t_a): (u64, u64),
    (id_b, t_b): (u64, u64),
    sa: &str,
    sb: &str,
) -> Vec<(u64, ContinuousEffect)> {
    vec![
        (
            id_a,
            eff(
                Layer::L4,
                t_a,
                filter(not(has_subtype(sb))),
                add_subtypes([sa]),
            ),
        ),
        (
            id_b,
            eff(
                Layer::L4,
                t_b,
                filter(not(has_subtype(sa))),
                add_subtypes([sb]),
            ),
        ),
    ]
}

/// REQ-LAYER-002, CR 613.8b, 613.8c: a two-effect dependency loop is applied in timestamp
/// order: the earlier member is chosen as `Loop`; after it applies the relation is recomputed,
/// the loop is broken and the other is chosen by timestamp (not `Loop`) and no longer applies.
#[test]
fn req_layer_002_loop_timestamp_order_and_recomputation() {
    let env = Env::default().with(o(1), creature(1, 1, Vec::new()));
    for (t_a, t_b) in [(1, 2), (2, 1)] {
        let (ev, rec) = run(
            &loop_pair((1, t_a), (2, t_b), "Alphakin", "Betakin"),
            &env,
            o(1),
        );
        let (first, second, kept, lost) = if t_a < t_b {
            (fx(1), fx(2), "Alphakin", "Betakin")
        } else {
            (fx(2), fx(1), "Betakin", "Alphakin")
        };
        assert!(ev.characteristics.subtypes.contains(kept));
        assert!(!ev.characteristics.subtypes.contains(lost));
        assert_eq!(
            steps_in(&rec, Layer::L4),
            vec![
                (first, Selection::Loop, true),
                (second, Selection::Timestamp, false),
            ]
        );
    }
}

/// REQ-LAYER-002, CR 613.8b: a step that depends on a loop member is not part of the loop
/// (the loop's members depend on nothing outside it); the loop's earliest member applies first
/// even though the outside step is earlier, then the outside step is released.
#[test]
fn req_layer_002_loop_excludes_steps_depending_on_it() {
    let env = Env::default().with(o(1), creature(1, 1, Vec::new()));
    let mut list = loop_pair((1, 2), (2, 3), "Alphakin", "Betakin");
    // Effect 3, earliest, applies only once Alphakin is present: it depends on effect 1.
    list.push((
        3,
        eff(
            Layer::L4,
            1,
            filter(has_subtype("Alphakin")),
            add_subtypes(["Gammakin"]),
        ),
    ));
    let (ev, rec) = run(&list, &env, o(1));
    assert_eq!(
        ev.characteristics.subtypes,
        set(["Alphakin", "Gammakin", "Testkin"])
    );
    assert_eq!(
        steps_in(&rec, Layer::L4),
        vec![
            (fx(1), Selection::Loop, true),
            (fx(3), Selection::Timestamp, true),
            (fx(2), Selection::Timestamp, false),
        ]
    );
}

/// REQ-LAYER-002, CR 613.8b, 613.8c: with two separate loops, the one containing the earliest
/// step is broken first; a step released while an earlier step is unapplied is labelled
/// `Dependency`.
#[test]
fn req_layer_002_two_loops_earliest_first() {
    let env = Env::default().with(o(1), creature(1, 1, Vec::new()));
    let mut list = loop_pair((1, 1), (2, 4), "Alphakin", "Betakin");
    list.extend(loop_pair((3, 2), (4, 3), "Gammakin", "Deltakin"));
    let (ev, rec) = run(&list, &env, o(1));
    assert_eq!(
        ev.characteristics.subtypes,
        set(["Alphakin", "Gammakin", "Testkin"])
    );
    assert_eq!(
        steps_in(&rec, Layer::L4),
        vec![
            (fx(1), Selection::Loop, true),
            (fx(2), Selection::Dependency, false),
            (fx(3), Selection::Loop, true),
            (fx(4), Selection::Timestamp, false),
        ]
    );
}

/// REQ-LAYER-002, CR 613.8a, 613.3: a CDA and a non-CDA never depend on each other. In layer 4
/// a CDA granting a subtype to creatures applies first even though the non-CDA would make the
/// object a creature.
#[test]
fn req_layer_002_cda_and_non_cda_never_dependent_layer_4() {
    let env = Env::default().with(o(1), artifact());
    for (t_p, t_q) in [(1, 2), (2, 1)] {
        let (ev, rec) = run(
            &[
                (
                    1,
                    eff(Layer::L4, t_p, only(o(1)), add_card_types(["Creature"])),
                ),
                (
                    2,
                    cda(
                        Layer::L4,
                        t_q,
                        filter(PredicateAST::IsCreature),
                        add_subtypes(["Granted"]),
                    ),
                ),
            ],
            &env,
            o(1),
        );
        assert!(!ev.characteristics.subtypes.contains("Granted"));
        assert_eq!(
            steps_in(&rec, Layer::L4),
            vec![
                (fx(2), Selection::CdaFirst, false),
                (fx(1), Selection::Timestamp, true),
            ]
        );
    }
}

/// REQ-LAYER-002, CR 613.8a: in layer 1a (no CDA-first) a later copy effect that makes the
/// object a creature and an earlier copy effect that applies only to creatures: when exactly
/// one is a CDA there is no dependency and timestamp order holds; when both or neither are,
/// the earlier one waits (it depends on the later one).
#[test]
fn req_layer_002_cda_and_non_cda_never_dependent_layer_1a() {
    let env = Env::default().with(o(1), artifact());
    let mut made = creature(1, 1, Vec::new());
    made.names = set(["test.copy_maker"]);
    let mut follower = creature(9, 9, Vec::new());
    follower.names = set(["test.copy_creatures_only"]);
    for (maker_cda, follower_cda, want) in [
        (true, false, "test.copy_maker"),
        (false, true, "test.copy_maker"),
        (false, false, "test.copy_creatures_only"),
        (true, true, "test.copy_creatures_only"),
    ] {
        let list = [
            (
                1,
                ContinuousEffect {
                    is_cda: maker_cda,
                    ..eff(
                        Layer::L1a,
                        5,
                        only(o(1)),
                        LayerModification::CopyValues(Box::new(made.clone())),
                    )
                },
            ),
            (
                2,
                ContinuousEffect {
                    is_cda: follower_cda,
                    ..eff(
                        Layer::L1a,
                        1,
                        filter(PredicateAST::IsCreature),
                        LayerModification::CopyValues(Box::new(follower.clone())),
                    )
                },
            ),
        ];
        let (ev, _) = run(&list, &env, o(1));
        assert_eq!(
            ev.characteristics.names,
            set([want]),
            "{maker_cda} {follower_cda}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Counters (CR 613.7c, 613.4c)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 613.7c: a keyword counter is a step at its counter timestamp among the
/// effects of layer 6, both orders; at equal timestamps the effect comes first.
#[test]
fn req_layer_002_counter_ordered_by_timestamp_among_effects() {
    for (t_counter, t_remove, flying) in [(3, 2, true), (1, 2, false), (2, 2, true)] {
        let env = Env::default()
            .with(o(1), creature(2, 2, Vec::new()))
            .counter(o(1), "flying", 1, t_counter);
        let (ev, rec) = run(
            &[(
                1,
                eff(
                    Layer::L6,
                    t_remove,
                    only(o(1)),
                    LayerModification::RemoveAllAbilities,
                ),
            )],
            &env,
            o(1),
        );
        assert_eq!(has_kw(&ev.characteristics, "flying"), flying);
        let counter = (counter_item("flying"), Selection::Timestamp, true);
        let effect = (fx(1), Selection::Timestamp, true);
        let want = if flying {
            vec![effect, counter]
        } else {
            vec![counter, effect]
        };
        assert_eq!(steps_in(&rec, Layer::L6), want);
    }
}

/// REQ-LAYER-002, CR 613.7c, 613.8a: a counter step can be depended on: an earlier effect
/// granting an ability to objects with the counter's keyword waits for the counter.
#[test]
fn req_layer_002_effect_depends_on_counter() {
    let env = Env::default()
        .with(o(1), creature(2, 2, Vec::new()))
        .counter(o(1), "flying", 1, 5);
    let (ev, rec) = run(
        &[(
            1,
            eff(
                Layer::L6,
                1,
                filter(has_flying()),
                LayerModification::AddAbility(ability("after_counter")),
            ),
        )],
        &env,
        o(1),
    );
    assert!(
        ev.characteristics
            .abilities
            .contains(&ability("after_counter"))
    );
    assert_eq!(
        steps_in(&rec, Layer::L6),
        vec![
            (counter_item("flying"), Selection::Dependency, true),
            (fx(1), Selection::Timestamp, true),
        ]
    );
}

/// REQ-LAYER-002, CR 613.4c, 613.7c: P/T counters and modifying effects in 7c are used in
/// timestamp order and do not depend on each other.
#[test]
fn req_layer_002_pt_counters_in_7c_timestamp_order() {
    let env = Env::default()
        .with(o(1), creature(2, 2, Vec::new()))
        .counter(o(1), "+1/+1", 2, 3);
    let (ev, rec) = run(
        &[
            (1, eff(Layer::L7c, 4, only(o(1)), modify_pt(-1, 0))),
            (2, eff(Layer::L7c, 1, only(o(1)), modify_pt(0, 3))),
        ],
        &env,
        o(1),
    );
    assert_eq!(pt(&ev.characteristics), Some((3, 7)));
    assert_eq!(
        steps_in(&rec, Layer::L7c),
        vec![
            (fx(2), Selection::Timestamp, true),
            (counter_item("+1/+1"), Selection::Timestamp, true),
            (fx(1), Selection::Timestamp, true),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// The record, errors and determinism
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002: the record names the object, lists every step (an effect whose affected set
/// excludes the object with `applied: false`) and lists layers in `Layer::ALL` order.
#[test]
fn req_layer_002_record_contents() {
    let env = Env::default()
        .with(o(1), creature(2, 2, Vec::new()))
        .with(o(2), artifact());
    let list = vec![
        (
            1,
            eff(Layer::L7d, 1, only(o(1)), LayerModification::SwitchPt),
        ),
        (
            2,
            eff(
                Layer::L1a,
                9,
                only(o(2)),
                LayerModification::CopyValues(Box::new(artifact())),
            ),
        ),
        (
            3,
            eff(
                Layer::L5,
                3,
                only(o(1)),
                LayerModification::AddColors(ColorBitmask::WHITE),
            ),
        ),
        (4, eff(Layer::L7b, 2, only(o(1)), set_pt(1, 4))),
    ];
    let (ev, rec) = run(&list, &env, o(1));
    assert_eq!(pt(&ev.characteristics), Some((4, 1)));
    let step = |layer, n, applied| OrderStep {
        layer,
        item: fx(n),
        selection: Selection::Timestamp,
        applied,
    };
    assert_eq!(
        rec,
        OrderRecord {
            object: o(1),
            steps: vec![
                step(Layer::L1a, 2, false),
                step(Layer::L5, 3, true),
                step(Layer::L7b, 4, true),
                step(Layer::L7d, 1, true),
            ],
        }
    );
    let copy = rec.clone();
    assert_eq!(copy, rec);
    assert!(format!("{rec:?}").contains("OrderRecord"));
    // An object with no effects and no counters has an empty record.
    let (_, empty) = run(&[], &env, o(2));
    assert_eq!(
        empty,
        OrderRecord {
            object: o(2),
            steps: Vec::new(),
        }
    );
}

/// REQ-LAYER-002, REQ-LAYER-001: an object the environment does not know is still an error.
#[test]
fn req_layer_002_unknown_object_error_preserved() {
    let env = Env::default().with(o(1), creature(2, 2, Vec::new()));
    let r = registry(&[]);
    assert_eq!(
        evaluate_with_order(&r, &env, o(99)),
        Err(LayerError::UnknownObject(o(99)))
    );
    assert_eq!(
        evaluate(&r, &env, o(99)),
        Err(LayerError::UnknownObject(o(99)))
    );
}

/// A scene with dependencies, a loop, CDAs and counters across layers and two objects.
fn busy_scene() -> (Vec<(u64, ContinuousEffect)>, Env) {
    let env = Env::default()
        .with(o(1), artifact())
        .with(o(2), creature(1, 2, vec![kw("flying")]))
        .counter(o(2), "+1/+1", 1, 4)
        .counter(o(2), "flying", 1, 6);
    let mut list = loop_pair((10, 3), (11, 2), "Alphakin", "Betakin");
    list.extend(remover_and_dependent(
        LayerModification::RemoveAllAbilities,
        7,
        2,
    ));
    list.push((
        5,
        eff(Layer::L4, 4, only(o(1)), add_card_types(["Creature"])),
    ));
    list.push((
        6,
        eff(
            Layer::L4,
            1,
            filter(PredicateAST::IsCreature),
            add_subtypes(["Granted"]),
        ),
    ));
    list.push((
        7,
        cda(
            Layer::L5,
            8,
            filter(PredicateAST::IsCreature),
            LayerModification::AddColors(ColorBitmask::BLUE),
        ),
    ));
    list.push((
        8,
        eff(
            Layer::L5,
            1,
            filter(PredicateAST::IsCreature),
            LayerModification::SetColors(ColorBitmask::RED),
        ),
    ));
    list.push((
        9,
        eff(
            Layer::L7c,
            4,
            filter(PredicateAST::IsCreature),
            modify_pt(1, 1),
        ),
    ));
    list.push((12, cda(Layer::L7a, 2, only(o(1)), set_pt(3, 3))));
    (list, env)
}

/// REQ-LAYER-002, REQ-CORE-001: shuffled registration orders give an identical Evaluated and
/// OrderRecord for every object, over 100 deterministic shuffles.
#[test]
fn req_layer_002_registration_order_never_changes_order_or_result() {
    let (list, env) = busy_scene();
    let want: Vec<(Evaluated, OrderRecord)> = [o(1), o(2)]
        .iter()
        .map(|&id| run(&list, &env, id))
        .collect();
    // The scene exercises every kind of selection.
    let all: Vec<Selection> = want
        .iter()
        .flat_map(|(_, r)| r.steps.iter().map(|s| s.selection))
        .collect();
    for s in [
        Selection::Timestamp,
        Selection::CdaFirst,
        Selection::Dependency,
        Selection::Loop,
    ] {
        assert!(all.contains(&s), "{s:?}");
    }
    let mut rng = Lcg(0x0d3e);
    for _ in 0..100 {
        let mut shuffled = list.clone();
        rng.shuffle(&mut shuffled);
        let got: Vec<(Evaluated, OrderRecord)> = [o(1), o(2)]
            .iter()
            .map(|&id| run(&shuffled, &env, id))
            .collect();
        assert_eq!(got, want);
    }
}

// ---------------------------------------------------------------------------------------------
// Property: no dependencies and no CDAs give timestamp order (REQ-LAYER-002)
// ---------------------------------------------------------------------------------------------

/// A random non-CDA effect on locked set `affected` (so no effect changes what another applies
/// to) with a fixed modification.
fn random_effect(rng: &mut Lcg, affected: Affected) -> ContinuousEffect {
    let words = ["Alphakin", "Betakin", "Testkin"];
    let (layer, m) = match rng.below(15) {
        0 => {
            let mut c = creature(rng.small(), rng.small(), Vec::new());
            c.names = set(["test.random_copy"]);
            (Layer::L1a, LayerModification::CopyValues(Box::new(c)))
        }
        1 => (
            Layer::L1b,
            LayerModification::FaceDown(Box::new(artifact())),
        ),
        2 => (
            Layer::L2,
            LayerModification::SetController(p(1 + rng.below(3))),
        ),
        3 => (
            Layer::L3,
            LayerModification::SetText(TextTokenStream(vec![rng.pick(&words).into()])),
        ),
        4 => (Layer::L4, add_subtypes([rng.pick(&words).as_str()])),
        5 => (
            Layer::L4,
            LayerModification::SetTypes {
                supertypes: None,
                card_types: None,
                subtypes: Some([rng.pick(&words)].into_iter().collect()),
            },
        ),
        6 => (
            Layer::L4,
            LayerModification::RemoveSubtypes([rng.pick(&words)].into_iter().collect()),
        ),
        7 => (
            Layer::L5,
            LayerModification::SetColors(
                ColorBitmask::new(u8::try_from(rng.below(32)).unwrap()).unwrap(),
            ),
        ),
        8 => (
            Layer::L5,
            LayerModification::AddColors(
                ColorBitmask::new(u8::try_from(rng.below(32)).unwrap()).unwrap(),
            ),
        ),
        9 => (
            Layer::L6,
            LayerModification::AddAbility(if rng.below(2) == 0 {
                kw("flying")
            } else {
                ability("random")
            }),
        ),
        10 => (Layer::L6, LayerModification::RemoveAllAbilities),
        11 => (
            Layer::L6,
            LayerModification::RemoveKeyword("flying".to_owned()),
        ),
        12 => (
            Layer::L7b,
            LayerModification::SetPt {
                power: (rng.below(2) == 0).then(|| rng.small()),
                toughness: (rng.below(2) == 0).then(|| rng.small()),
            },
        ),
        13 => (Layer::L7c, modify_pt(rng.small() - 2, rng.small() - 2)),
        _ => (Layer::L7d, LayerModification::SwitchPt),
    };
    eff(layer, rng.below(6), affected, m)
}

/// The ordering key of the property's expected order: layer, timestamp, effects before
/// counters, then EffectId or counter kind.
type Key = (usize, Timestamp, u8, Option<EffectId>, Option<String>);

/// REQ-LAYER-002, CR 613.4c, 613.7, 613.8a, 613.8b: with no dependency and no CDA, the order
/// used equals timestamp order (ties: effects before counters, ascending EffectId, ascending
/// counter kind) and every step is selected by timestamp, for 300 random scenes in 4
/// registration orders each, each also run with the base power and toughness at the `i64`
/// bounds (where 7c adders and P/T counters saturate) to show saturation creates no dependency.
#[test]
fn req_layer_002_property_no_dependency_is_timestamp_order() {
    let mut rng = Lcg(0x0613_0008);
    for seed in 0..300 {
        let mut env = Env::default()
            .with(o(1), creature(2, 3, vec![kw("flying"), ability("printed")]))
            .with(o(2), artifact());
        for kind in ["+1/+1", "-1/-1", "flying", "trample"] {
            if rng.below(2) == 0 {
                let amount = 1 + i64::try_from(rng.below(2)).unwrap();
                env = env.counter(o(1), kind, amount, rng.below(6));
            }
        }
        let n = 4 + rng.below(10);
        let list: Vec<(u64, ContinuousEffect)> = (0..n)
            .map(|i| {
                let affected = if rng.below(5) == 0 {
                    only(o(2))
                } else {
                    only(o(1))
                };
                (100 + i * 2, random_effect(&mut rng, affected))
            })
            .collect();
        let mut want: Vec<Key> = list
            .iter()
            .map(|(id, x)| (layer_index(x.layer), x.timestamp, 0, Some(e(*id)), None))
            .collect();
        for kind in env.counters(o(1)).into_keys() {
            let layer = if keyword_counter_keyword(&kind).is_some() {
                Layer::L6
            } else {
                Layer::L7c
            };
            let at = env.counter_timestamp(o(1), &kind).unwrap();
            want.push((layer_index(layer), at, 1, None, Some(kind)));
        }
        want.sort();
        let want: Vec<OrderItem> = want
            .into_iter()
            .map(|(_, _, _, id, kind)| match (id, kind) {
                (Some(id), _) => OrderItem::Effect(id),
                (None, kind) => OrderItem::Counter(kind.unwrap()),
            })
            .collect();
        for _ in 0..4 {
            let mut shuffled = list.clone();
            rng.shuffle(&mut shuffled);
            // CR 613.4c, 613.8a: the same scene at the i64 bounds orders the same way.
            for (power, toughness) in [(2, 3), (i64::MAX, i64::MIN), (i64::MIN, i64::MAX)] {
                let env = env.clone().with(
                    o(1),
                    creature(power, toughness, vec![kw("flying"), ability("printed")]),
                );
                let (_, rec) = run(&shuffled, &env, o(1));
                let got: Vec<OrderItem> = rec.steps.iter().map(|s| s.item.clone()).collect();
                assert_eq!(got, want, "seed {seed} {power}");
                assert!(
                    rec.steps
                        .iter()
                        .all(|s| s.selection == Selection::Timestamp),
                    "seed {seed} {power}: {rec:?}"
                );
            }
        }
    }
}

/// REQ-LAYER-002, REQ-CORE-001, CR 613.3, 613.8b, 613.8c: for random scenes with selector
/// effects (so dependencies and loops occur), CDAs and counters: every step is used exactly
/// once, layers appear in `Layer::ALL` order, in layers 2-6 every CDA precedes every non-CDA,
/// a step is `applied` exactly when its affected set includes the object (checked by replaying
/// the record from the base, matching both applied and unapplied selector effects), and
/// registration order changes neither the Evaluated nor the record.
#[test]
fn req_layer_002_property_order_invariants_with_dependencies() {
    let mut rng = Lcg(0x0613_0803);
    let words = ["Alphakin", "Betakin", "Testkin"];
    let mut seen = BTreeSet::new();
    let mut selector_applied = BTreeSet::new();
    for seed in 0..300 {
        let mut env = Env::default().with(o(1), creature(2, 3, vec![kw("flying")]));
        for kind in ["+1/+1", "flying"] {
            if rng.below(2) == 0 {
                env = env.counter(o(1), kind, 1, rng.below(6));
            }
        }
        let n = 3 + rng.below(9);
        let list: Vec<(u64, ContinuousEffect)> = (0..n)
            .map(|i| {
                let affected = match rng.below(5) {
                    0 => filter(has_subtype(&rng.pick(&words))),
                    1 => filter(not(has_subtype(&rng.pick(&words)))),
                    2 => filter(PredicateAST::IsCreature),
                    3 => filter(has_flying()),
                    _ => only(o(1)),
                };
                let mut x = random_effect(&mut rng, affected);
                x.is_cda = rng.below(4) == 0;
                (1 + i, x)
            })
            .collect();
        let (ev, rec) = run(&list, &env, o(1));
        let mut used: Vec<OrderItem> = rec.steps.iter().map(|s| s.item.clone()).collect();
        let mut expected: Vec<OrderItem> = list.iter().map(|(id, _)| fx(*id)).collect();
        expected.extend(env.counters(o(1)).into_keys().map(OrderItem::Counter));
        let key = |i: &OrderItem| format!("{i:?}");
        used.sort_by_key(key);
        expected.sort_by_key(key);
        assert_eq!(used, expected, "seed {seed}");
        let index: Vec<usize> = rec.steps.iter().map(|s| layer_index(s.layer)).collect();
        assert!(index.windows(2).all(|w| w[0] <= w[1]), "seed {seed}");
        let cda_of = |item: &OrderItem| match item {
            OrderItem::Effect(id) => list.iter().any(|(k, x)| e(*k) == *id && x.is_cda),
            OrderItem::Counter(_) => false,
        };
        for layer in [Layer::L2, Layer::L3, Layer::L4, Layer::L5, Layer::L6] {
            let flags: Vec<bool> = rec
                .steps
                .iter()
                .filter(|s| s.layer == layer)
                .map(|s| cda_of(&s.item))
                .collect();
            assert!(flags.windows(2).all(|w| w[0] || !w[1]), "seed {seed}");
        }
        for s in &rec.steps {
            seen.insert(format!("{:?}", s.selection));
            if !s.applied {
                assert!(matches!(s.item, OrderItem::Effect(_)), "seed {seed}");
            }
        }
        // Replay the record from the base: `applied` is exactly whether the effect's affected
        // set includes the object at that step (CR 611.2c, 611.3a, 613.5); counters always.
        let replayed = replay(&rec, &list, &env, &mut selector_applied, seed);
        assert_eq!(replayed, ev, "seed {seed}");
        for _ in 0..3 {
            let mut shuffled = list.clone();
            rng.shuffle(&mut shuffled);
            assert_eq!(
                run(&shuffled, &env, o(1)),
                (ev.clone(), rec.clone()),
                "seed {seed}"
            );
        }
    }
    // The generator reaches every kind of selection.
    assert_eq!(seen.len(), 4, "{seen:?}");
    // Selector effects were seen both applying and not applying: the replay check is not vacuous.
    assert_eq!(
        selector_applied,
        [false, true].into_iter().collect(),
        "{selector_applied:?}"
    );
}

/// Replays `rec` on object 1 from its base, step by step in the recorded order, asserting that
/// each step's `applied` equals whether its affected set includes the object at that point,
/// computed from the effect's `affected` alone: a locked set by membership, a selector by
/// `env.matches` on the replayed state. Records in `selector_applied` every value seen for a
/// selector effect. Returns the replayed result.
fn replay(
    rec: &OrderRecord,
    list: &[(u64, ContinuousEffect)],
    env: &Env,
    selector_applied: &mut BTreeSet<bool>,
    seed: u64,
) -> Evaluated {
    use super::ordering::Scratch;
    use super::pipeline::{Step, apply_step, start_layer_7};
    let base = env.base(o(1)).unwrap();
    let counters = env.counters(o(1));
    let mut scratch = Scratch {
        state: Evaluated {
            characteristics: base.characteristics,
            controller: base.controller,
        },
        pt: None,
    };
    for &layer in Layer::ALL {
        if layer == Layer::L7a {
            // The evaluator's own start of layer 7 (CR 613.4, 208.3).
            scratch.pt = start_layer_7(&mut scratch.state, env, o(1), base.on_battlefield);
        }
        for s in rec.steps.iter().filter(|s| s.layer == layer) {
            match &s.item {
                OrderItem::Effect(id) => {
                    let x = &list.iter().find(|(k, _)| e(*k) == *id).unwrap().1;
                    let expected = if let Affected::Selector(selector) = &x.affected {
                        let m = env.matches(
                            selector,
                            &MatchContext {
                                source: x.source,
                                effect_controller: x.controller,
                                candidate: o(1),
                                candidate_view: &scratch.state,
                            },
                        );
                        selector_applied.insert(m);
                        m
                    } else {
                        matches!(&x.affected, Affected::Objects(set) if set.contains(&o(1)))
                    };
                    assert_eq!(s.applied, expected, "seed {seed}: {s:?}");
                    if expected {
                        apply_step(&mut scratch, &Step::Effect(*id, x));
                    }
                }
                OrderItem::Counter(kind) => {
                    assert!(s.applied, "seed {seed}: {s:?}");
                    apply_step(&mut scratch, &Step::Counter(kind, counters[kind]));
                }
            }
        }
    }
    scratch.state
}

// ---------------------------------------------------------------------------------------------
// Existence of the generating ability (CR 613.8a, 611.2, 611.3, 113.6)
// ---------------------------------------------------------------------------------------------

/// test.existence_remover (effect 1, layer 6, locked on object 1) and test.existence_grant
/// (effect 2, layer 6): the grant gives `marker` to objects without trample; `grant_origin` and
/// `grant_source` are its origin and source. The environment says the grant's generating ability
/// is object 1's flying.
fn existence_scene(
    remover: LayerModification,
    (t_remover, t_grant): (u64, u64),
    grant_origin: EffectOrigin,
    grant_source: Option<ObjectId>,
) -> (Vec<(u64, ContinuousEffect)>, Env) {
    let env = Env::default()
        .with(o(1), creature(2, 2, vec![kw("flying"), kw("trample")]))
        .with(o(2), artifact())
        .generator(e(2), "flying");
    let grant = ContinuousEffect {
        source: grant_source,
        origin: grant_origin,
        ..eff(
            Layer::L6,
            t_grant,
            filter(not(PredicateAST::HasKeyword {
                keyword: "trample".to_owned(),
            })),
            LayerModification::AddAbility(ability("marker")),
        )
    };
    (
        vec![
            // test.existence_remover
            (1, eff(Layer::L6, t_remover, only(o(1)), remover)),
            // test.existence_grant
            (2, grant),
        ],
        env,
    )
}

fn has_marker(ev: &Evaluated) -> bool {
    ev.characteristics.abilities.contains(&ability("marker"))
}

fn static_origin() -> EffectOrigin {
    EffectOrigin::Static { granted_at: None }
}

/// REQ-LAYER-002, CR 613.8a, 611.3, 113.6, 613.7: the remover removes every ability of the
/// grant's source, the grant's generating ability included. In both timestamp orders the grant
/// does not apply (before the remover its selector does not match; after it, its generating
/// ability no longer exists), so no dependency is found and no marker is granted.
#[test]
fn req_layer_002_removed_generating_ability_stops_effect_both_orders() {
    for (t_remover, t_grant) in [(1, 2), (2, 1)] {
        let (list, env) = existence_scene(
            LayerModification::RemoveAllAbilities,
            (t_remover, t_grant),
            static_origin(),
            Some(o(1)),
        );
        let (ev, rec) = run(&list, &env, o(1));
        assert!(!has_marker(&ev), "{t_remover} {t_grant}");
        assert!(ev.characteristics.abilities.is_empty());
        let mut want = vec![
            (fx(1), Selection::Timestamp, true),
            (fx(2), Selection::Timestamp, false),
        ];
        if t_grant < t_remover {
            want.reverse();
        }
        assert_eq!(steps_in(&rec, Layer::L6), want, "{t_remover} {t_grant}");
        // The hook saw the source's state so far (the source is the evaluated object).
        assert!(
            env.generator_calls
                .borrow()
                .iter()
                .all(|&call| call == (e(2), o(1), true))
        );
        assert!(!env.generator_calls.borrow().is_empty());
    }
}

/// REQ-LAYER-002, CR 613.8a, 613.8b (control): the remover removes trample only; the grant's
/// generating ability (flying) still exists and its selector now matches, so the grant depends
/// on the remover and applies after it in both timestamp orders.
#[test]
fn req_layer_002_generating_ability_kept_effect_applies_both_orders() {
    for (t_remover, t_grant) in [(1, 2), (2, 1)] {
        let (list, env) = existence_scene(
            LayerModification::RemoveKeyword("trample".to_owned()),
            (t_remover, t_grant),
            static_origin(),
            Some(o(1)),
        );
        let (ev, rec) = run(&list, &env, o(1));
        assert!(has_marker(&ev), "{t_remover} {t_grant}");
        assert!(has_kw(&ev.characteristics, "flying"));
        let first = if t_remover < t_grant {
            Selection::Timestamp
        } else {
            Selection::Dependency
        };
        assert_eq!(
            steps_in(&rec, Layer::L6),
            vec![(fx(1), first, true), (fx(2), Selection::Timestamp, true)]
        );
    }
}

/// REQ-LAYER-002, CR 613.8a, 611.2, 613.8b: exempt effects. A resolved effect with a source
/// (CR 611.2: it exists independently of its source's abilities), a static effect whose source
/// is another object (its existence is not judged here: `source_view` is `None`, treated as
/// existing), and a static effect without a source: in each case the grant depends on the
/// remover and applies after it in both timestamp orders, and the marker is granted.
#[test]
fn req_layer_002_existence_exemptions_both_orders() {
    let cases = [
        (EffectOrigin::Resolved, Some(o(1))),
        (static_origin(), Some(o(2))),
        (static_origin(), None),
    ];
    for (origin, source) in cases {
        for (t_remover, t_grant) in [(1, 2), (2, 1)] {
            let (list, env) = existence_scene(
                LayerModification::RemoveAllAbilities,
                (t_remover, t_grant),
                origin.clone(),
                source,
            );
            let (ev, rec) = run(&list, &env, o(1));
            assert!(
                has_marker(&ev),
                "{origin:?} {source:?} {t_remover} {t_grant}"
            );
            let first = if t_remover < t_grant {
                Selection::Timestamp
            } else {
                Selection::Dependency
            };
            assert_eq!(
                steps_in(&rec, Layer::L6),
                vec![(fx(1), first, true), (fx(2), Selection::Timestamp, true)]
            );
            let calls = env.generator_calls.borrow();
            if source == Some(o(2)) {
                // CR 613.8a: another object's source is asked without a view.
                assert!(!calls.is_empty());
                assert!(calls.iter().all(|&call| call == (e(2), o(2), false)));
            } else {
                // Resolved, or no source: the hook is never asked.
                assert!(calls.is_empty(), "{origin:?} {source:?}");
            }
        }
    }
}

/// An environment that keeps every default method.
struct Plain;

impl LayerEnvironment for Plain {
    fn objects(&self) -> Vec<ObjectId> {
        vec![o(1)]
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        (id == o(1)).then(|| ObjectBase {
            characteristics: creature(1, 1, vec![kw("flying")]),
            controller: Some(p(1)),
            on_battlefield: true,
        })
    }

    fn matches(&self, _: &ObjectSelectorAST, _: &MatchContext<'_>) -> bool {
        true
    }
}

/// REQ-LAYER-002, CR 613.8a: the default `generating_ability_exists` is `true`, so an
/// environment without the hook keeps the previous behaviour: a static effect of the evaluated
/// object applies after its abilities are removed.
#[test]
fn req_layer_002_generating_ability_default_is_true() {
    let view = Evaluated {
        characteristics: artifact(),
        controller: None,
    };
    assert!(Plain.generating_ability_exists(e(1), o(1), Some(&view)));
    assert!(Plain.generating_ability_exists(e(1), o(2), None));
    let list = [
        (
            1,
            eff(
                Layer::L6,
                1,
                only(o(1)),
                LayerModification::RemoveAllAbilities,
            ),
        ),
        (
            2,
            ContinuousEffect {
                source: Some(o(1)),
                origin: static_origin(),
                ..eff(
                    Layer::L6,
                    2,
                    filter(PredicateAST::IsCreature),
                    LayerModification::AddAbility(ability("marker")),
                )
            },
        ),
    ];
    let (ev, rec) = evaluate_with_order(&registry(&list), &Plain, o(1)).unwrap();
    assert!(has_marker(&ev));
    assert!(rec.steps.iter().all(|s| s.applied));
}

// ---------------------------------------------------------------------------------------------
// Declared P/T adjustments at the i64 bounds (CR 613.4c, 613.7, 613.8a)
// ---------------------------------------------------------------------------------------------

/// REQ-LAYER-002, CR 613.4c, 613.7, 613.8a: two 7c adders on a locked set never depend on each
/// other, even when one saturates at the `i64` bounds: they apply in timestamp order (both
/// selected by timestamp) and the result is that of timestamp order.
#[test]
fn req_layer_002_saturated_adders_are_independent_both_orders() {
    const MAX: i64 = i64::MAX;
    const MIN: i64 = i64::MIN;
    // (base, first (ts 1), second (ts 2), expected).
    let cases = [
        ((MAX, MAX), (1, 1), (-2, -2), (MAX - 2, MAX - 2)),
        ((MAX, MAX), (-2, -2), (1, 1), (MAX - 1, MAX - 1)),
        ((MIN, MIN), (-1, -1), (2, 2), (MIN + 2, MIN + 2)),
        ((MIN, MIN), (2, 2), (-1, -1), (MIN + 1, MIN + 1)),
        // Toughness alone at each bound, power far from them.
        ((2, MAX), (0, 1), (0, -2), (2, MAX - 2)),
        ((2, MIN), (0, -1), (0, 2), (2, MIN + 2)),
        // Power and toughness at opposite bounds.
        ((MAX, MIN), (1, -1), (-2, 2), (MAX - 2, MIN + 2)),
    ];
    for (base, first, second, want) in cases {
        let env = Env::default().with(o(1), creature(base.0, base.1, Vec::new()));
        let (ev, rec) = run(
            &[
                (
                    1,
                    eff(Layer::L7c, 1, only(o(1)), modify_pt(first.0, first.1)),
                ),
                (
                    2,
                    eff(Layer::L7c, 2, only(o(1)), modify_pt(second.0, second.1)),
                ),
            ],
            &env,
            o(1),
        );
        assert_eq!(pt(&ev.characteristics), Some(want), "{base:?} {first:?}");
        assert_eq!(
            steps_in(&rec, Layer::L7c),
            vec![
                (fx(1), Selection::Timestamp, true),
                (fx(2), Selection::Timestamp, true)
            ],
            "{base:?} {first:?}"
        );
    }
}

/// REQ-LAYER-002, CR 613.4c, 613.7, 613.7c, 613.8a, 122.1a: a P/T counter and a 7c adder at the
/// `i64` bounds are independent in both timestamp orders.
#[test]
fn req_layer_002_saturated_counter_and_adder_independent_both_orders() {
    const MAX: i64 = i64::MAX;
    const MIN: i64 = i64::MIN;
    // (base, counter kind, effect amount, expected counter-first, expected effect-first).
    let cases = [
        (
            (MAX, MAX),
            "+1/+1",
            -2,
            (MAX - 2, MAX - 2),
            (MAX - 1, MAX - 1),
        ),
        (
            (MIN, MIN),
            "-1/-1",
            2,
            (MIN + 2, MIN + 2),
            (MIN + 1, MIN + 1),
        ),
    ];
    for (base, kind, amount, counter_first, effect_first) in cases {
        for (t_counter, t_effect) in [(1, 2), (2, 1)] {
            let env = Env::default()
                .with(o(1), creature(base.0, base.1, Vec::new()))
                .counter(o(1), kind, 1, t_counter);
            let (ev, rec) = run(
                &[(
                    1,
                    eff(Layer::L7c, t_effect, only(o(1)), modify_pt(amount, amount)),
                )],
                &env,
                o(1),
            );
            let mut want_steps = vec![
                (counter_item(kind), Selection::Timestamp, true),
                (fx(1), Selection::Timestamp, true),
            ];
            let want = if t_counter < t_effect {
                counter_first
            } else {
                want_steps.reverse();
                effect_first
            };
            assert_eq!(pt(&ev.characteristics), Some(want), "{kind} {t_counter}");
            assert_eq!(steps_in(&rec, Layer::L7c), want_steps, "{kind} {t_counter}");
        }
    }
}

/// REQ-LAYER-002, CR 613.4c, 122.1a: the declared adjustment of each kind of step: a `ModifyPt`
/// effect its amounts, P/T counters amount times the per-counter change (no saturation, in
/// `i128`), and nothing for another modification, a keyword counter, an object without a P/T
/// pair, or P/T counters on a non-creature.
#[test]
fn req_layer_002_declared_pt_adjustment() {
    use super::ordering::Scratch;
    use super::pipeline::{Step, declared_pt_adjustment};
    let with_pt = |c: Characteristics, pt: Option<(i64, i64)>| Scratch {
        state: Evaluated {
            characteristics: c,
            controller: Some(p(1)),
        },
        pt,
    };
    let creature_scratch = with_pt(creature(1, 1, Vec::new()), Some((1, 1)));
    let adder = eff(Layer::L7c, 1, only(o(1)), modify_pt(i64::MAX, -3));
    let setter = eff(Layer::L7b, 1, only(o(1)), set_pt(0, 0));
    assert_eq!(
        declared_pt_adjustment(&Step::Effect(e(1), &adder), &creature_scratch),
        Some((i128::from(i64::MAX), -3))
    );
    assert_eq!(
        declared_pt_adjustment(&Step::Effect(e(1), &setter), &creature_scratch),
        None
    );
    assert_eq!(
        declared_pt_adjustment(&Step::Counter("-2/-1", i64::MAX), &creature_scratch),
        Some((-2 * i128::from(i64::MAX), -i128::from(i64::MAX)))
    );
    assert_eq!(
        declared_pt_adjustment(&Step::Counter("flying", 1), &creature_scratch),
        None
    );
    // No P/T pair.
    let no_pt = with_pt(creature(1, 1, Vec::new()), None);
    assert_eq!(
        declared_pt_adjustment(&Step::Effect(e(1), &adder), &no_pt),
        None
    );
    // A non-creature with a P/T pair: P/T counters do nothing (CR 122.1a), adders declare.
    let non_creature = with_pt(artifact(), Some((1, 1)));
    assert_eq!(
        declared_pt_adjustment(&Step::Counter("+1/+1", 1), &non_creature),
        None
    );
    assert_eq!(
        declared_pt_adjustment(&Step::Effect(e(1), &adder), &non_creature),
        Some((i128::from(i64::MAX), -3))
    );
}

/// REQ-LAYER-002, CR 613.4d, 613.7, 613.8a: two 7d switches do not depend on each other (each
/// switches the pair in both states, although the numbers it writes differ), so they apply in
/// timestamp order, both selected by timestamp, and the pair ends as it began.
#[test]
fn req_layer_002_two_switches_independent_both_orders() {
    let env = Env::default().with(o(1), creature(2, 3, Vec::new()));
    for (t1, t2) in [(1, 2), (2, 1)] {
        let (ev, rec) = run(
            &[
                (
                    1,
                    eff(Layer::L7d, t1, only(o(1)), LayerModification::SwitchPt),
                ),
                (
                    2,
                    eff(Layer::L7d, t2, only(o(1)), LayerModification::SwitchPt),
                ),
            ],
            &env,
            o(1),
        );
        assert_eq!(pt(&ev.characteristics), Some((2, 3)));
        let mut want = vec![
            (fx(1), Selection::Timestamp, true),
            (fx(2), Selection::Timestamp, true),
        ];
        if t2 < t1 {
            want.reverse();
        }
        assert_eq!(steps_in(&rec, Layer::L7d), want, "{t1} {t2}");
    }
}
