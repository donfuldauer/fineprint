// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the layers module: REQ-LAYER-001 (evaluation through layers 1a-7d, counters,
// base never mutated), REQ-CORE-002 (timestamps, CR 613.7a-n), REQ-CORE-003 (LKI consumer),
// REQ-CORE-001 (determinism). Synthetic ids and synthetic characteristics only.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST, ReferenceAST, SelectorFilter, WordRef};

use super::*;
use crate::memory::{CharacteristicsProvider, LkiStore, MemoryTable, capture_lki_on_move};
use crate::objects::Characteristics;
use crate::permanent::Permanents;
use crate::zones::{MoveRequest, ZoneKind, Zones, ZonesConfig};

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

fn ts(n: u64) -> Timestamp {
    Timestamp::new(n)
}

fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A synthetic black creature `name` with the given P/T and no abilities.
fn creature(name: &str, power: i64, toughness: i64) -> Characteristics {
    Characteristics {
        names: set([name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::BLACK,
        supertypes: BTreeSet::new(),
        card_types: set(["Creature"]),
        subtypes: set(["Bear"]),
        rules_text: TextTokenStream(Vec::new()),
        abilities: Vec::new(),
        power: Some(literal(power)),
        toughness: Some(literal(toughness)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A synthetic noncreature of `card_type` without P/T.
fn noncreature(name: &str, card_type: &str) -> Characteristics {
    let mut c = creature(name, 0, 0);
    c.card_types = set([card_type]);
    c.subtypes = BTreeSet::new();
    c.power = None;
    c.toughness = None;
    c
}

fn ability(tag: &str) -> AbilityAST {
    AbilityAST(format!("test.ability.{tag}").into())
}

fn pt(c: &Characteristics) -> Option<(i64, i64)> {
    Some((
        literal_of(c.power.as_ref()?)?,
        literal_of(c.toughness.as_ref()?)?,
    ))
}

fn has_keyword(c: &Characteristics, keyword: &str) -> bool {
    c.abilities.iter().any(|a| {
        a.0.get("kind").and_then(|k| k.as_str()) == Some("KEYWORD")
            && a.0.get("keyword").and_then(|k| k.as_str()) == Some(keyword)
    })
}

/// A keyword ability granted by an effect or printed (not a counter).
fn keyword(keyword: &str, id: &str) -> AbilityAST {
    AbilityAST(
        [
            ("kind", "KEYWORD".to_owned()),
            ("abilityId", id.to_owned()),
            ("keyword", keyword.to_owned()),
        ]
        .into_iter()
        .collect(),
    )
}

fn filter(pred: PredicateAST) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    })
}

fn creatures() -> ObjectSelectorAST {
    filter(PredicateAST::IsCreature)
}

fn white_creatures() -> ObjectSelectorAST {
    filter(PredicateAST::And(vec![
        PredicateAST::IsCreature,
        PredicateAST::HasColor {
            value: WordRef::Name("white".to_owned()),
        },
    ]))
}

fn card_type(t: &str) -> ObjectSelectorAST {
    filter(PredicateAST::HasCardType {
        value: WordRef::Name(t.to_owned()),
    })
}

fn objects<const N: usize>(ids: [ObjectId; N]) -> Affected {
    Affected::Objects(ids.into_iter().collect())
}

fn none() -> Affected {
    Affected::Objects(BTreeSet::new())
}

fn modify(power: i64, toughness: i64) -> LayerModification {
    LayerModification::ModifyPt { power, toughness }
}

fn set_pt(power: Option<i64>, toughness: Option<i64>) -> LayerModification {
    LayerModification::SetPt { power, toughness }
}

/// A resolved effect controlled by player 1 with no source.
fn eff(layer: Layer, t: u64, affected: Affected, m: LayerModification) -> ContinuousEffect {
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
        ..eff(layer, t, affected, m)
    }
}

/// A static effect of `source` from an ability granted at `granted`.
fn granted_stat(
    source: ObjectId,
    granted: u64,
    layer: Layer,
    t: u64,
    m: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        origin: EffectOrigin::Static {
            granted_at: Some(ts(granted)),
        },
        ..stat(source, layer, t, none(), m)
    }
}

fn registry(list: Vec<(u64, ContinuousEffect)>) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    for (id, effect) in list {
        r.register(e(id), effect).unwrap();
    }
    r
}

/// A synthetic environment.
#[derive(Default, Clone)]
struct Env {
    bases: BTreeMap<ObjectId, ObjectBase>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    counter_ts: BTreeMap<(ObjectId, String), Timestamp>,
    attached: BTreeMap<ObjectId, Vec<ObjectId>>,
    /// The value of every non-literal expression, per object.
    cda: BTreeMap<ObjectId, i64>,
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

    /// Moves `id` off the battlefield (a card in another zone).
    fn off_battlefield(mut self, id: ObjectId) -> Env {
        if let Some(base) = self.bases.get_mut(&id) {
            base.on_battlefield = false;
        }
        self
    }

    fn counter(mut self, id: ObjectId, kind: &str, amount: i64, t: u64) -> Env {
        self.counters
            .entry(id)
            .or_default()
            .insert(kind.to_owned(), amount);
        self.counter_ts.insert((id, kind.to_owned()), ts(t));
        self
    }
}

fn pred_holds(pred: &PredicateAST, view: &Evaluated) -> bool {
    let c = &view.characteristics;
    match pred {
        PredicateAST::And(all) => all.iter().all(|q| pred_holds(q, view)),
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

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.attached.get(&id).cloned().unwrap_or_default()
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

    fn eval_value(&self, expr: &ValueExpressionAST, object: ObjectId) -> Option<i64> {
        literal_of(expr).or_else(|| self.cda.get(&object).copied())
    }
}

/// An environment with only the required methods (the trait defaults are used); its one
/// object is off the battlefield.
struct Minimal(Characteristics);

impl LayerEnvironment for Minimal {
    fn objects(&self) -> Vec<ObjectId> {
        vec![o(1)]
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        (id == o(1)).then(|| ObjectBase {
            characteristics: self.0.clone(),
            controller: None,
            on_battlefield: false,
        })
    }

    fn matches(&self, _: &ObjectSelectorAST, _: &MatchContext<'_>) -> bool {
        true
    }
}

fn eval(r: &ContinuousEffects, env: &Env, id: ObjectId) -> Evaluated {
    evaluate(r, env, id).unwrap()
}

fn eval_pt(r: &ContinuousEffects, env: &Env, id: ObjectId) -> Option<(i64, i64)> {
    pt(&eval(r, env, id).characteristics)
}

// ---------------------------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-001: registration, lookup, timestamps, removal; ascending EffectId iteration.
#[test]
fn registry_basic_operations() {
    let mut r = ContinuousEffects::new();
    assert!(r.is_empty());
    let a = eff(Layer::L7d, 3, objects([o(1)]), LayerModification::SwitchPt);
    r.register(e(2), a.clone()).unwrap();
    r.register(e(1), a.clone()).unwrap();
    assert_eq!(r.len(), 2);
    assert!(!r.is_empty());
    assert_eq!(r.iter().map(|(id, _)| id).collect::<Vec<_>>(), [e(1), e(2)]);
    assert_eq!(r.get(e(1)), Some(&a));
    r.set_timestamp(e(1), ts(9)).unwrap();
    assert_eq!(r.get(e(1)).unwrap().timestamp, ts(9));
    assert_eq!(
        r.set_timestamp(e(7), ts(1)),
        Err(LayerError::UnknownEffect(e(7)))
    );
    assert!(r.remove(e(1)));
    assert!(!r.remove(e(1)));
    assert_eq!(r.get(e(1)), None);
    assert_eq!(r.len(), 1);
}

// REQ-LAYER-001: duplicate ids, a modification in the wrong layer and SameAsLayer (CR 613.6
// lock-in, a later unit) are rejected; the registry is unchanged.
#[test]
fn registry_rejections() {
    let mut r = ContinuousEffects::new();
    let good = eff(Layer::L7c, 1, objects([o(1)]), modify(1, 1));
    r.register(e(1), good.clone()).unwrap();
    let before = r.clone();
    assert_eq!(
        r.register(e(1), good),
        Err(LayerError::DuplicateEffect(e(1)))
    );
    let wrong = eff(Layer::L7b, 1, objects([o(1)]), modify(1, 1));
    assert_eq!(
        r.register(e(2), wrong),
        Err(LayerError::LayerMismatch { layer: Layer::L7b })
    );
    let same = eff(
        Layer::L7c,
        1,
        Affected::SameAsLayer(Layer::L4),
        modify(1, 1),
    );
    assert!(matches!(
        r.register(e(3), same),
        Err(LayerError::Unsupported(_))
    ));
    assert_eq!(r, before);
}

// REQ-LAYER-001; CR 613.1-613.4: the layer table of every modification.
#[test]
fn modification_layer_table() {
    let c = Box::new(creature("test.copy", 1, 1));
    let table: Vec<(LayerModification, Vec<Layer>)> = vec![
        (LayerModification::CopyValues(c.clone()), vec![Layer::L1a]),
        (LayerModification::FaceDown(c), vec![Layer::L1b]),
        (LayerModification::SetController(p(1)), vec![Layer::L2]),
        (
            LayerModification::SetText(TextTokenStream(Vec::new())),
            vec![Layer::L3],
        ),
        (
            LayerModification::AddTypes {
                supertypes: BTreeSet::new(),
                card_types: BTreeSet::new(),
                subtypes: BTreeSet::new(),
            },
            vec![Layer::L4],
        ),
        (
            LayerModification::SetTypes {
                supertypes: None,
                card_types: None,
                subtypes: None,
            },
            vec![Layer::L4],
        ),
        (
            LayerModification::RemoveSubtypes(BTreeSet::new()),
            vec![Layer::L4],
        ),
        (
            LayerModification::SetColors(ColorBitmask::NONE),
            vec![Layer::L5],
        ),
        (
            LayerModification::AddColors(ColorBitmask::NONE),
            vec![Layer::L5],
        ),
        (LayerModification::AddAbility(ability("x")), vec![Layer::L6]),
        (LayerModification::RemoveAllAbilities, vec![Layer::L6]),
        (
            LayerModification::RemoveKeyword("flying".to_owned()),
            vec![Layer::L6],
        ),
        (set_pt(None, None), vec![Layer::L7a, Layer::L7b]),
        (modify(0, 0), vec![Layer::L7c]),
        (LayerModification::SwitchPt, vec![Layer::L7d]),
        // REQ-DSL-012, CR 613.1c: the text-changing effect belongs to layer 3 only.
        (
            LayerModification::ReplaceTextWord {
                category: fineprint_dsl::text::RewritableCategory::BasicLandType,
                from: "Swamp".to_owned(),
                to: "Island".to_owned(),
            },
            vec![Layer::L3],
        ),
    ];
    for (m, layers) in table {
        for &l in Layer::ALL {
            assert_eq!(m.belongs_to(l), layers.contains(&l), "{m:?} in {l}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// One test per layer
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-001; CR 613.2a, 613.2c: layer 1a replaces all characteristics with the copiable
// values; later layers still apply on top (CR 613.1).
#[test]
fn layer_1a_copy_values() {
    let env = Env::default().with(o(1), creature("test.original", 1, 1));
    let copied = creature("test.copied_3_3", 3, 3);
    let r = registry(vec![
        (
            1,
            eff(
                Layer::L1a,
                5,
                objects([o(1)]),
                LayerModification::CopyValues(Box::new(copied.clone())),
            ),
        ),
        (2, eff(Layer::L7c, 1, objects([o(1)]), modify(1, 0))),
    ]);
    let got = eval(&r, &env, o(1));
    assert_eq!(got.characteristics.names, copied.names);
    assert_eq!(pt(&got.characteristics), Some((4, 3)));
}

// REQ-LAYER-001; CR 613.2b, 613.2: layer 1b gives the face-down characteristics, after 1a
// whatever the timestamps.
#[test]
fn layer_1b_face_down() {
    let env = Env::default().with(o(1), creature("test.hidden", 5, 5));
    let mut face_down = creature("", 2, 2);
    face_down.names = BTreeSet::new();
    face_down.subtypes = BTreeSet::new();
    face_down.colors = ColorBitmask::NONE;
    let r = registry(vec![
        (
            1,
            eff(
                Layer::L1b,
                1,
                objects([o(1)]),
                LayerModification::FaceDown(Box::new(face_down.clone())),
            ),
        ),
        (
            2,
            eff(
                Layer::L1a,
                9,
                objects([o(1)]),
                LayerModification::CopyValues(Box::new(creature("test.copy", 7, 7))),
            ),
        ),
    ]);
    assert_eq!(eval(&r, &env, o(1)).characteristics, face_down);
}

// REQ-LAYER-001; CR 613.1b, 613.7: layer 2 changes control, the later timestamp wins.
#[test]
fn layer_2_control() {
    let env = Env::default().with(o(1), creature("test.bear", 2, 2));
    let r = registry(vec![
        (
            1,
            eff(
                Layer::L2,
                7,
                objects([o(1)]),
                LayerModification::SetController(p(3)),
            ),
        ),
        (
            2,
            eff(
                Layer::L2,
                4,
                objects([o(1)]),
                LayerModification::SetController(p(2)),
            ),
        ),
    ]);
    assert_eq!(eval(&r, &env, o(1)).controller, Some(p(3)));
    assert_eq!(
        eval(&ContinuousEffects::new(), &env, o(1)).controller,
        Some(p(1))
    );
}

// REQ-LAYER-001; CR 613.1c: layer 3 replaces the rules text.
#[test]
fn layer_3_text() {
    let env = Env::default().with(o(1), creature("test.bear", 2, 2));
    let text = TextTokenStream(vec!["test.word".into()]);
    let r = registry(vec![(
        1,
        eff(
            Layer::L3,
            1,
            objects([o(1)]),
            LayerModification::SetText(text.clone()),
        ),
    )]);
    assert_eq!(eval(&r, &env, o(1)).characteristics.rules_text, text);
}

// REQ-LAYER-001; CR 613.1d, 208.5: layer 4 AddTypes is a union; a new creature without printed
// P/T is 0/0.
#[test]
fn layer_4_add_types() {
    let env = Env::default().with(o(1), noncreature("test.relic", "Artifact"));
    let r = registry(vec![(
        1,
        eff(
            Layer::L4,
            1,
            objects([o(1)]),
            LayerModification::AddTypes {
                supertypes: set(["Legendary"]),
                card_types: set(["Creature"]),
                subtypes: set(["Construct"]),
            },
        ),
    )]);
    let c = eval(&r, &env, o(1)).characteristics;
    assert_eq!(c.card_types, set(["Artifact", "Creature"]));
    assert_eq!(c.supertypes, set(["Legendary"]));
    assert_eq!(c.subtypes, set(["Construct"]));
    assert_eq!(pt(&c), Some((0, 0)));
}

// REQ-LAYER-001; CR 613.1d, 305.7: SetTypes replaces only the lines it sets.
#[test]
fn layer_4_set_types() {
    let mut land = noncreature("test.land", "Land");
    land.subtypes = set(["Forest"]);
    land.supertypes = set(["Basic"]);
    let env = Env::default().with(o(1), land);
    let r = registry(vec![(
        1,
        eff(
            Layer::L4,
            1,
            objects([o(1)]),
            LayerModification::SetTypes {
                supertypes: None,
                card_types: None,
                subtypes: Some(set(["Island"])),
            },
        ),
    )]);
    let c = eval(&r, &env, o(1)).characteristics;
    assert_eq!(c.subtypes, set(["Island"]));
    assert_eq!(c.supertypes, set(["Basic"]));
    assert_eq!(c.card_types, set(["Land"]));
    let r = registry(vec![(
        1,
        eff(
            Layer::L4,
            1,
            objects([o(1)]),
            LayerModification::SetTypes {
                supertypes: Some(BTreeSet::new()),
                card_types: Some(set(["Artifact"])),
                subtypes: None,
            },
        ),
    )]);
    let c = eval(&r, &env, o(1)).characteristics;
    assert_eq!(c.subtypes, set(["Forest"]));
    assert!(c.supertypes.is_empty());
    assert_eq!(c.card_types, set(["Artifact"]));
}

// REQ-LAYER-001; CR 613.1d: RemoveSubtypes removes only the given subtypes.
#[test]
fn layer_4_remove_subtypes() {
    let mut c = creature("test.bear", 2, 2);
    c.subtypes = set(["Bear", "Warrior"]);
    let env = Env::default().with(o(1), c);
    let r = registry(vec![(
        1,
        eff(
            Layer::L4,
            1,
            objects([o(1)]),
            LayerModification::RemoveSubtypes(set(["Bear", "Elf"])),
        ),
    )]);
    assert_eq!(
        eval(&r, &env, o(1)).characteristics.subtypes,
        set(["Warrior"])
    );
}

// REQ-LAYER-001; CR 613.1e, 613.7: SetColors and AddColors in timestamp order, both orders; the
// color indicator is untouched.
#[test]
fn layer_5_colors() {
    let mut c = creature("test.bear", 2, 2);
    c.color_indicator = Some(ColorBitmask::BLACK);
    let env = Env::default().with(o(1), c);
    let add = |t| {
        eff(
            Layer::L5,
            t,
            objects([o(1)]),
            LayerModification::AddColors(ColorBitmask::GREEN),
        )
    };
    let red = |t| {
        eff(
            Layer::L5,
            t,
            objects([o(1)]),
            LayerModification::SetColors(ColorBitmask::RED),
        )
    };
    let r = registry(vec![(1, add(2)), (2, red(1))]);
    let got = eval(&r, &env, o(1)).characteristics;
    assert_eq!(
        got.colors.bits(),
        ColorBitmask::RED.bits() | ColorBitmask::GREEN.bits()
    );
    assert_eq!(got.color_indicator, Some(ColorBitmask::BLACK));
    let r = registry(vec![(1, add(1)), (2, red(2))]);
    assert_eq!(
        eval(&r, &env, o(1)).characteristics.colors,
        ColorBitmask::RED
    );
}

// REQ-LAYER-001; CR 613.1f: AddAbility appends the ability.
#[test]
fn layer_6_add_ability() {
    let env = Env::default().with(o(1), creature("test.bear", 2, 2));
    let r = registry(vec![(
        1,
        eff(
            Layer::L6,
            1,
            objects([o(1)]),
            LayerModification::AddAbility(ability("granted")),
        ),
    )]);
    assert_eq!(
        eval(&r, &env, o(1)).characteristics.abilities,
        vec![ability("granted")]
    );
}

// REQ-LAYER-001; CR 613.1f: RemoveAllAbilities clears every ability.
#[test]
fn layer_6_remove_all_abilities() {
    let mut c = creature("test.bear", 2, 2);
    c.abilities = vec![ability("a"), keyword("FLYING", "a2")];
    let env = Env::default().with(o(1), c);
    let r = registry(vec![(
        1,
        eff(
            Layer::L6,
            1,
            objects([o(1)]),
            LayerModification::RemoveAllAbilities,
        ),
    )]);
    assert!(eval(&r, &env, o(1)).characteristics.abilities.is_empty());
}

// REQ-LAYER-001; CR 613.1f: RemoveKeyword removes every keyword instance with that keyword and
// nothing else.
#[test]
fn layer_6_remove_keyword() {
    let mut c = creature("test.bear", 2, 2);
    c.abilities = vec![
        keyword("flying", "a1"),
        ability("flying"),
        keyword("reach", "a2"),
        keyword("flying", "a3"),
        AbilityAST([("keyword", "flying")].into_iter().collect()),
    ];
    let env = Env::default().with(o(1), c);
    let r = registry(vec![(
        1,
        eff(
            Layer::L6,
            1,
            objects([o(1)]),
            LayerModification::RemoveKeyword("flying".to_owned()),
        ),
    )]);
    assert_eq!(
        eval(&r, &env, o(1)).characteristics.abilities,
        vec![
            ability("flying"),
            keyword("reach", "a2"),
            AbilityAST([("keyword", "flying")].into_iter().collect()),
        ]
    );
}

// REQ-LAYER-001; CR 613.4a, 604.3: a non-literal base P/T is evaluated by the environment at
// the start of layer 7a; a 7a setter applies before 7b even with a later timestamp.
#[test]
fn layer_7a_cda() {
    let mut c = creature("test.cda_star", 0, 0);
    c.power = Some(ValueExpressionAST("test.count_cards".into()));
    c.toughness = Some(ValueExpressionAST("test.count_cards".into()));
    let mut env = Env::default().with(o(1), c);
    env.cda.insert(o(1), 6);
    assert_eq!(eval_pt(&ContinuousEffects::new(), &env, o(1)), Some((6, 6)));
    let r = registry(vec![
        (
            1,
            eff(Layer::L7a, 9, objects([o(1)]), set_pt(Some(4), None)),
        ),
        (
            2,
            eff(Layer::L7b, 1, objects([o(1)]), set_pt(None, Some(1))),
        ),
    ]);
    assert_eq!(eval_pt(&r, &env, o(1)), Some((4, 1)));
    // An expression the environment cannot evaluate counts as 0.
    env.cda.clear();
    assert_eq!(eval_pt(&ContinuousEffects::new(), &env, o(1)), Some((0, 0)));
}

// REQ-LAYER-001; CR 613.4b: a 7b setter; `None` leaves that value.
#[test]
fn layer_7b_set() {
    let env = Env::default().with(o(1), creature("test.bear", 2, 3));
    let r = registry(vec![(
        1,
        eff(Layer::L7b, 1, objects([o(1)]), set_pt(Some(0), None)),
    )]);
    assert_eq!(eval_pt(&r, &env, o(1)), Some((0, 3)));
}

// REQ-LAYER-001; CR 613.4c: a 7c modifier with an earlier timestamp still follows a 7b setter.
#[test]
fn layer_7c_modify() {
    let env = Env::default().with(o(1), creature("test.bear", 2, 2));
    let r = registry(vec![
        (1, eff(Layer::L7c, 1, objects([o(1)]), modify(3, -1))),
        (
            2,
            eff(Layer::L7b, 5, objects([o(1)]), set_pt(Some(1), Some(1))),
        ),
    ]);
    assert_eq!(eval_pt(&r, &env, o(1)), Some((4, 0)));
}

// REQ-LAYER-001; CR 613.4d and its three examples: 1/3, +0/+1, switch -> 4/1; then +5/+0 ->
// 4/6; the +0/+1 ending -> 3/1; two switches -> 1/4.
#[test]
fn layer_7d_switch_examples() {
    let env = Env::default().with(o(1), creature("test.one_three", 1, 3));
    let plus = |t| eff(Layer::L7c, t, objects([o(1)]), modify(0, 1));
    let switch = |t| eff(Layer::L7d, t, objects([o(1)]), LayerModification::SwitchPt);
    let mut r = registry(vec![(1, plus(1)), (2, switch(2))]);
    assert_eq!(eval_pt(&r, &env, o(1)), Some((4, 1)));
    r.register(e(3), eff(Layer::L7c, 3, objects([o(1)]), modify(5, 0)))
        .unwrap();
    assert_eq!(eval_pt(&r, &env, o(1)), Some((4, 6)));
    r.remove(e(3));
    r.remove(e(1));
    assert_eq!(eval_pt(&r, &env, o(1)), Some((3, 1)));
    let r = registry(vec![(1, plus(1)), (2, switch(2)), (3, switch(3))]);
    assert_eq!(eval_pt(&r, &env, o(1)), Some((1, 4)));
}

// REQ-LAYER-001; CR 613.5 second example: a 2/2 with a +1/+1 counter (7c), a resolved +4/+4
// (7c), a static +0/+2 (7c) and a resolved "becomes 0/1" (7b) is 5/8.
#[test]
fn layer_7_worked_example_5_8() {
    let env = Env::default()
        .with(o(1), creature("test.bear_2_2", 2, 2))
        .with(o(2), noncreature("test.anthem_0_2", "Enchantment"))
        .counter(o(1), "+1/+1", 1, 1);
    let mut r = ContinuousEffects::new();
    assert_eq!(eval_pt(&r, &env, o(1)), Some((3, 3)));
    r.register(e(1), eff(Layer::L7c, 2, objects([o(1)]), modify(4, 4)))
        .unwrap();
    assert_eq!(eval_pt(&r, &env, o(1)), Some((7, 7)));
    r.register(
        e(2),
        stat(
            o(2),
            Layer::L7c,
            3,
            Affected::Selector(creatures()),
            modify(0, 2),
        ),
    )
    .unwrap();
    assert_eq!(eval_pt(&r, &env, o(1)), Some((7, 9)));
    r.register(
        e(3),
        eff(Layer::L7b, 4, objects([o(1)]), set_pt(Some(0), Some(1))),
    )
    .unwrap();
    assert_eq!(eval_pt(&r, &env, o(1)), Some((5, 8)));
    // The enchantment has no P/T (CR 208.3).
    assert_eq!(eval_pt(&r, &env, o(2)), None);
}

// ---------------------------------------------------------------------------------------------
// Timestamp order within a layer
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-001; CR 613.3, 613.7: in layer 6 the later of lose-all-abilities and add-keyword
// wins, in both timestamp orders.
#[test]
fn layer_6_timestamp_order_both_ways() {
    let env = Env::default().with(o(1), creature("test.bear", 2, 2));
    for (lose, add, has) in [(1, 2, true), (2, 1, false)] {
        let r = registry(vec![
            (
                1,
                eff(
                    Layer::L6,
                    lose,
                    objects([o(1)]),
                    LayerModification::RemoveAllAbilities,
                ),
            ),
            (
                2,
                eff(
                    Layer::L6,
                    add,
                    objects([o(1)]),
                    LayerModification::AddAbility(keyword("flying", "g1")),
                ),
            ),
        ]);
        assert_eq!(
            has_keyword(&eval(&r, &env, o(1)).characteristics, "flying"),
            has
        );
    }
}

// REQ-LAYER-001; CR 613.4b, 613.7: two 7b setters, the later wins, in both orders; at equal
// timestamps the higher EffectId applies last.
#[test]
fn layer_7b_timestamp_order_both_ways() {
    let env = Env::default().with(o(1), creature("test.bear", 2, 2));
    let set_to = |t, n| eff(Layer::L7b, t, objects([o(1)]), set_pt(Some(n), Some(n)));
    for (a, b, want) in [(1, 2, 5), (2, 1, 1), (3, 3, 5)] {
        let r = registry(vec![(1, set_to(a, 1)), (2, set_to(b, 5))]);
        assert_eq!(eval_pt(&r, &env, o(1)), Some((want, want)));
    }
}

/// An ability-suppressing enchantment (6: creatures lose all abilities; 7b: creatures are 1/1),
/// an enchantment-animating one (4: enchantments are creatures; 7b: enchantments are 4/4) and a
/// 3/3 flier.
fn ability_suppressor_and_animator(h_ts: u64, o_ts: u64) -> (ContinuousEffects, Env) {
    let mut h = noncreature("test.global_ability_suppressor", "Enchantment");
    h.abilities = vec![ability("ability_suppressor")];
    let mut op = noncreature("test.enchantment_animator", "Enchantment");
    op.abilities = vec![ability("enchantment_animator")];
    let mut bear = creature("test.flier_3_3", 3, 3);
    bear.abilities = vec![keyword("flying", "a1")];
    let env = Env::default().with(o(1), h).with(o(2), op).with(o(3), bear);
    let enchantments = || Affected::Selector(card_type("Enchantment"));
    let r = registry(vec![
        (
            1,
            stat(
                o(1),
                Layer::L6,
                h_ts,
                Affected::Selector(creatures()),
                LayerModification::RemoveAllAbilities,
            ),
        ),
        (
            2,
            stat(
                o(1),
                Layer::L7b,
                h_ts,
                Affected::Selector(creatures()),
                set_pt(Some(1), Some(1)),
            ),
        ),
        (
            3,
            stat(
                o(2),
                Layer::L4,
                o_ts,
                enchantments(),
                LayerModification::AddTypes {
                    supertypes: BTreeSet::new(),
                    card_types: set(["Creature"]),
                    subtypes: BTreeSet::new(),
                },
            ),
        ),
        (
            4,
            stat(
                o(2),
                Layer::L7b,
                o_ts,
                enchantments(),
                set_pt(Some(4), Some(4)),
            ),
        ),
    ]);
    (r, env)
}

// REQ-LAYER-001; CR 613.1, 613.5, 613.7: the ability suppressor and the enchantment animator in
// both timestamp orders. The layer-4 type change is visible to the layer-6 and 7b selectors; in
// 7b the later timestamp wins for the enchantments; every creature loses its abilities either
// way.
#[test]
fn ability_suppressor_and_animator_both_orders() {
    for (h_ts, o_ts, ench_pt) in [(1, 2, (4, 4)), (2, 1, (1, 1))] {
        let (r, env) = ability_suppressor_and_animator(h_ts, o_ts);
        let all = evaluate_all(&r, &env).unwrap();
        for id in [o(1), o(2)] {
            let c = &all[&id].characteristics;
            assert!(c.card_types.contains("Creature"));
            assert!(c.abilities.is_empty(), "{id:?}");
            assert_eq!(pt(c), Some(ench_pt));
        }
        let bear = &all[&o(3)].characteristics;
        assert!(bear.abilities.is_empty());
        assert_eq!(pt(bear), Some((1, 1)));
    }
}

// REQ-LAYER-001; CR 611.3a vs 611.2c, 613.5: "white creatures get +1/+1" as a selector applies
// to a creature turned white in layer 5; as a locked set fixed before it turned white, not.
#[test]
fn selector_rematch_versus_locked_set() {
    let env = Env::default()
        .with(o(1), creature("test.black_bear", 2, 2))
        .with(o(2), noncreature("test.anthem", "Enchantment"));
    let whiten = eff(
        Layer::L5,
        3,
        objects([o(1)]),
        LayerModification::SetColors(ColorBitmask::WHITE),
    );
    let selector = stat(
        o(2),
        Layer::L7c,
        1,
        Affected::Selector(white_creatures()),
        modify(1, 1),
    );
    // The resolved version locked its set (no white creature) when it began.
    let locked = eff(Layer::L7c, 2, none(), modify(1, 1));
    assert_eq!(
        eval_pt(&registry(vec![(1, locked.clone())]), &env, o(1)),
        Some((2, 2))
    );
    let r = registry(vec![(1, locked), (2, whiten.clone())]);
    assert_eq!(eval_pt(&r, &env, o(1)), Some((2, 2)));
    assert_eq!(
        eval_pt(&registry(vec![(1, selector.clone())]), &env, o(1)),
        Some((2, 2))
    );
    let r = registry(vec![(1, selector), (2, whiten)]);
    assert_eq!(eval_pt(&r, &env, o(1)), Some((3, 3)));
}

// REQ-LAYER-001; CR 613.6 (first sentence), 611.3a: an effect modifies only the objects it
// applies to; a SelfRef selector matches only its source.
#[test]
fn effects_apply_only_to_matching_objects() {
    let env = Env::default()
        .with(o(1), creature("test.bear", 2, 2))
        .with(o(2), creature("test.other", 2, 2));
    let r = registry(vec![
        (
            1,
            stat(
                o(1),
                Layer::L7c,
                1,
                Affected::Selector(ObjectSelectorAST::Ref(ReferenceAST::SelfRef)),
                modify(1, 0),
            ),
        ),
        (
            2,
            eff(
                Layer::L7c,
                1,
                Affected::Selector(ObjectSelectorAST::Ref(ReferenceAST::Controller)),
                modify(9, 9),
            ),
        ),
    ]);
    assert_eq!(eval_pt(&r, &env, o(1)), Some((3, 2)));
    assert_eq!(eval_pt(&r, &env, o(2)), Some((2, 2)));
}

// ---------------------------------------------------------------------------------------------
// Counters
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-001; CR 122.1b, 613.1f, 613.7c: a keyword counter applies in layer 6 at its own
// timestamp against a lose-all-abilities effect, in both orders; at equal timestamps effects
// come first, so the counter survives.
#[test]
fn keyword_counter_in_layer_6_both_orders() {
    for (counter_ts, lose_ts, has) in [(5, 10, false), (10, 5, true), (7, 7, true)] {
        let env = Env::default()
            .with(o(1), creature("test.bear", 2, 2))
            .counter(o(1), "flying", 2, counter_ts);
        let r = registry(vec![(
            1,
            eff(
                Layer::L6,
                lose_ts,
                objects([o(1)]),
                LayerModification::RemoveAllAbilities,
            ),
        )]);
        let c = eval(&r, &env, o(1)).characteristics;
        assert_eq!(has_keyword(&c, "flying"), has);
        if has {
            // Once, whatever the amount.
            assert_eq!(c.abilities, vec![keyword_counter_ability("flying")]);
        }
    }
}

// REQ-LAYER-001; CR 122.1b: every keyword-counter kind grants its keyword; ward and other
// kinds grant nothing.
#[test]
fn keyword_counter_kinds_and_ward() {
    let mut env = Env::default().with(o(1), creature("test.bear", 2, 2));
    for (i, kind) in (0u64..).zip(KEYWORD_COUNTER_KINDS) {
        env = env.counter(o(1), kind, 1, i);
    }
    env = env
        .counter(o(1), "ward", 1, 1)
        .counter(o(1), "charge", 3, 1);
    let c = eval(&ContinuousEffects::new(), &env, o(1)).characteristics;
    assert_eq!(c.abilities.len(), KEYWORD_COUNTER_KINDS.len());
    for kind in KEYWORD_COUNTER_KINDS {
        assert!(has_keyword(&c, kind), "{kind}");
    }
    assert!(!has_keyword(&c, "ward"));
    let ab = keyword_counter_ability("first strike");
    assert_eq!(
        ab.0.get("abilityId").and_then(|v| v.as_str()),
        Some("counter:first strike")
    );
}

// REQ-LAYER-001; CR 122.1a, 613.4c: P/T counters by amount; zero amounts and other kinds do
// nothing.
#[test]
fn pt_counters_amounts() {
    let env = Env::default()
        .with(o(1), creature("test.bear", 2, 2))
        .counter(o(1), "+1/+1", 2, 1)
        .counter(o(1), "-1/-1", 1, 2)
        .counter(o(1), "+2/+0", 1, 3)
        .counter(o(1), "+1/-1", 4, 4)
        .counter(o(1), "-0/-9", 0, 5);
    assert_eq!(eval_pt(&ContinuousEffects::new(), &env, o(1)), Some((5, 3)));
}

// REQ-LAYER-001; CR 122.1a: the P/T counter kind parser.
#[test]
fn pt_counter_kind_parser() {
    assert_eq!(pt_counter_delta("+1/+1"), Some((1, 1)));
    assert_eq!(pt_counter_delta("-1/-1"), Some((-1, -1)));
    assert_eq!(pt_counter_delta("+2/+0"), Some((2, 0)));
    assert_eq!(pt_counter_delta("-0/-2"), Some((0, -2)));
    for bad in [
        "",
        "+",
        "+1",
        "+1/1",
        "1/1",
        "+1/-1",
        "+a/+1",
        "+/+1",
        "+1/+",
        "+ 1/+1",
        "*1/*1",
        "+99999999999999999999/+1",
        "flying",
    ] {
        assert_eq!(pt_counter_delta(bad), None, "{bad}");
    }
}

// REQ-LAYER-001; CR 122.1a, 613.4c: the counter parser survives a bounded run of random
// printable strings without panicking and accepts only the "+X/+Y" / "-X/-Y" shape.
#[test]
fn pt_counter_parser_random_inputs() {
    let alphabet: Vec<char> = "+-/0123456789 aé".chars().collect();
    let mut rng = Lcg(42);
    for _ in 0..5_000 {
        let len = (rng.next() % 8) as usize;
        let s: String = (0..len)
            .map(|_| alphabet[(rng.next() % alphabet.len() as u64) as usize])
            .collect();
        if let Some((x, y)) = pt_counter_delta(&s) {
            let sign = if x < 0 || y < 0 { '-' } else { '+' };
            assert!(s.starts_with(sign) || (x == 0 && y == 0), "{s}");
            assert_eq!(s.matches('/').count(), 1, "{s}");
        }
    }
}

// REQ-LAYER-001; CR 613.1, 613.4c, 613.4d: counters apply in 7c, never after 7d (there is no
// later layer): a +0/+1 counter with a later timestamp than a switch still applies before it.
#[test]
fn counters_never_after_switch() {
    let env = Env::default()
        .with(o(1), creature("test.one_three", 1, 3))
        .counter(o(1), "+0/+1", 1, 100);
    let r = registry(vec![(
        1,
        eff(Layer::L7d, 1, objects([o(1)]), LayerModification::SwitchPt),
    )]);
    assert_eq!(eval_pt(&r, &env, o(1)), Some((4, 1)));
    assert_eq!(Layer::ALL.last(), Some(&Layer::L7d));
}

// REQ-LAYER-001; CR 208.3, 208.5: a noncreature permanent has no P/T even if printed, and P/T
// effects and counters do nothing to it; off the battlefield a printed P/T stays.
#[test]
fn pt_presence() {
    let mut vehicle = noncreature("test.vehicle_3_3", "Artifact");
    vehicle.power = Some(literal(3));
    vehicle.toughness = Some(literal(3));
    let env = Env::default()
        .with(o(1), vehicle.clone())
        .counter(o(1), "+1/+1", 1, 1);
    let r = registry(vec![
        (
            1,
            eff(Layer::L7b, 1, objects([o(1)]), set_pt(Some(1), Some(1))),
        ),
        (2, eff(Layer::L7c, 1, objects([o(1)]), modify(1, 1))),
        (
            3,
            eff(Layer::L7d, 1, objects([o(1)]), LayerModification::SwitchPt),
        ),
    ]);
    let c = eval(&r, &env, o(1)).characteristics;
    assert_eq!((c.power, c.toughness), (None, None));
    // The trait defaults; off the battlefield the printed P/T stays.
    let minimal = Minimal(vehicle);
    let got = evaluate(&ContinuousEffects::new(), &minimal, o(1)).unwrap();
    assert_eq!(pt(&got.characteristics), Some((3, 3)));
    assert_eq!(got.controller, None);
    assert!(minimal.counters(o(1)).is_empty());
    assert_eq!(minimal.counter_timestamp(o(1), "+1/+1"), None);
    assert!(minimal.attached_here(o(1)).is_empty());
    assert_eq!(
        minimal.eval_value(&ValueExpressionAST("test.x".into()), o(1)),
        None
    );
    assert_eq!(minimal.eval_value(&literal(2), o(1)), Some(2));
}

// REQ-LAYER-001: literal forms of power and toughness.
#[test]
fn literal_forms() {
    assert_eq!(literal_of(&literal(-3)), Some(-3));
    let tagged = ValueExpressionAST([("LITERAL", 7)].into_iter().collect());
    assert_eq!(literal_of(&tagged), Some(7));
    let two = ValueExpressionAST([("LITERAL", 7), ("X", 1)].into_iter().collect());
    assert_eq!(literal_of(&two), None);
    let other = ValueExpressionAST([("COUNT", 7)].into_iter().collect());
    assert_eq!(literal_of(&other), None);
    assert_eq!(literal_of(&ValueExpressionAST("X".into())), None);
}

// ---------------------------------------------------------------------------------------------
// Purity and determinism
// ---------------------------------------------------------------------------------------------

/// A mixed scene with effects in every layer and counters.
fn scene() -> (Vec<(u64, ContinuousEffect)>, Env) {
    let (r, env) = ability_suppressor_and_animator(4, 6);
    let mut list: Vec<(u64, ContinuousEffect)> =
        r.iter().map(|(id, x)| (id.get(), x.clone())).collect();
    let env = env
        .with(o(4), creature("test.bear_2_2", 2, 2))
        .counter(o(4), "+1/+1", 2, 3)
        .counter(o(4), "flying", 1, 5);
    let more = vec![
        eff(
            Layer::L1a,
            1,
            objects([o(4)]),
            LayerModification::CopyValues(Box::new(creature("test.copy", 3, 1))),
        ),
        eff(
            Layer::L2,
            2,
            objects([o(4)]),
            LayerModification::SetController(p(2)),
        ),
        eff(
            Layer::L3,
            2,
            objects([o(4)]),
            LayerModification::SetText(TextTokenStream(vec!["t".into()])),
        ),
        eff(
            Layer::L5,
            5,
            Affected::Selector(creatures()),
            LayerModification::AddColors(ColorBitmask::WHITE),
        ),
        eff(
            Layer::L7c,
            5,
            Affected::Selector(white_creatures()),
            modify(1, 2),
        ),
        eff(Layer::L7c, 5, objects([o(4), o(3)]), modify(-1, 0)),
        eff(Layer::L7d, 8, objects([o(4)]), LayerModification::SwitchPt),
        eff(
            Layer::L6,
            5,
            objects([o(4)]),
            LayerModification::AddAbility(ability("late")),
        ),
    ];
    for (i, m) in (10u64..).zip(more) {
        list.push((i, m));
    }
    (list, env)
}

// REQ-LAYER-001: evaluation never mutates the base: the environment and the registry compare
// equal before and after evaluating twice, and the two evaluations agree.
#[test]
fn base_never_mutated() {
    let (list, env) = scene();
    let r = registry(list);
    let env_before = env.bases.clone();
    let r_before = r.clone();
    let first = evaluate_all(&r, &env).unwrap();
    let second = evaluate_all(&r, &env).unwrap();
    assert_eq!(first, second);
    assert_eq!(env.bases, env_before);
    assert_eq!(r, r_before);
    // The effects did change the evaluated values.
    assert_ne!(
        first[&o(4)].characteristics,
        env.bases[&o(4)].characteristics
    );
}

// REQ-LAYER-001: evaluate_all equals evaluate for each object; an unknown object is an error.
#[test]
fn evaluate_all_equals_evaluate_and_unknown_object() {
    struct Lying(Env);
    impl LayerEnvironment for Lying {
        fn objects(&self) -> Vec<ObjectId> {
            vec![o(1), o(77)]
        }
        fn base(&self, id: ObjectId) -> Option<ObjectBase> {
            self.0.base(id)
        }
        fn matches(&self, s: &ObjectSelectorAST, c: &MatchContext<'_>) -> bool {
            self.0.matches(s, c)
        }
    }
    let (list, env) = scene();
    let r = registry(list);
    let all = evaluate_all(&r, &env).unwrap();
    assert_eq!(all.keys().copied().collect::<Vec<_>>(), env.objects());
    for id in env.objects() {
        assert_eq!(all[&id], eval(&r, &env, id));
    }
    assert_eq!(
        evaluate(&r, &env, o(99)),
        Err(LayerError::UnknownObject(o(99)))
    );
    // An environment listing an object it has no base for fails as a whole.
    assert_eq!(
        evaluate_all(&r, &Lying(env)),
        Err(LayerError::UnknownObject(o(77)))
    );
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

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = usize::try_from(self.next() % (i as u64 + 1)).unwrap();
            v.swap(i, j);
        }
    }
}

// REQ-CORE-001, REQ-LAYER-001; CR 613.7: registration order never changes the result (ordering
// is by timestamp then EffectId), over 200 deterministic shuffles.
#[test]
fn registration_order_is_irrelevant() {
    let (list, env) = scene();
    let want = evaluate_all(&registry(list.clone()), &env).unwrap();
    let mut rng = Lcg(0x5eed);
    for _ in 0..200 {
        let mut shuffled = list.clone();
        rng.shuffle(&mut shuffled);
        let mut r = ContinuousEffects::new();
        for (id, effect) in shuffled {
            r.register(e(id), effect).unwrap();
        }
        assert_eq!(evaluate_all(&r, &env).unwrap(), want);
    }
}

// ---------------------------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------------------------

// REQ-CORE-003, REQ-LAYER-001; SRS 2.3, CR 608.2h: LKI captured through LayeredProvider holds
// the evaluated characteristics (layers applied) and the counters with amount >= 1.
#[test]
fn provider_feeds_lki_with_evaluated_characteristics() {
    let mut zones = Zones::new(&[p(1), p(2)], ZonesConfig::default());
    let mut ids: IdGenerator<ObjectId> = IdGenerator::new();
    let id = zones
        .create_object(
            &mut ids,
            CardId::new(1),
            p(1),
            MoveRequest::new(ZoneKind::Battlefield),
        )
        .unwrap();
    let mut env = Env::default()
        .with(id, creature("test.bear_2_2", 2, 2))
        .counter(id, "+1/+1", 1, 1)
        .counter(id, "charge", 0, 1);
    env.attached.insert(id, vec![o(50)]);
    let r = registry(vec![
        (1, eff(Layer::L7c, 2, objects([id]), modify(2, 0))),
        (
            2,
            eff(
                Layer::L6,
                2,
                objects([id]),
                LayerModification::AddAbility(ability("granted")),
            ),
        ),
    ]);
    let provider = LayeredProvider::new(&r, &env);
    assert_eq!(provider.characteristics(o(999)), None);
    assert_eq!(provider.attached_here(id), vec![o(50)]);
    assert_eq!(
        provider.counters(id),
        BTreeMap::from([("+1/+1".to_owned(), 1)])
    );
    let mut lki = LkiStore::new();
    let stored = capture_lki_on_move(
        &zones,
        &Permanents::new(),
        &MemoryTable::new(),
        &provider,
        &mut lki,
        id,
        &MoveRequest::new(ZoneKind::Graveyard),
    );
    assert_eq!(stored, Ok(true));
    let rec = lki.get(id).unwrap();
    assert_eq!(rec.characteristics(), &eval(&r, &env, id).characteristics);
    assert_ne!(rec.characteristics(), &env.bases[&id].characteristics);
    assert_eq!(pt(rec.characteristics()), Some((5, 3)));
    assert_eq!(rec.characteristics().abilities, vec![ability("granted")]);
    assert_eq!(rec.counters(), &BTreeMap::from([("+1/+1".to_owned(), 1)]));
    assert_eq!(rec.attached_here(), &[o(50)]);
}

// ---------------------------------------------------------------------------------------------
// Timestamps (REQ-CORE-002)
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002; CR 613.7d, 613.7e, 613.7f, 613.7g, 613.7h, 613.7i, 613.7j: every reason gives a
// new, strictly later timestamp, also to an object that already has one.
#[test]
fn stamp_object_every_reason() {
    let mut g = IdGenerator::new();
    let mut l = TimestampLedger::new();
    assert_eq!(l.object_timestamp(o(1)), None);
    let mut last = ts(0);
    for reason in [
        StampReason::EntersZone,
        StampReason::Attached,
        StampReason::TurnedFaceUpOrDown,
        StampReason::TransformedOrConverted,
        StampReason::SpecialCardTurnedFaceUp,
        StampReason::GameStart,
        StampReason::JoinedMergedPermanent,
    ] {
        let t = l.stamp_object(&mut g, o(1), reason).unwrap();
        assert!(t > last);
        assert_eq!(l.object_timestamp(o(1)), Some(t));
        last = t;
    }
}

// REQ-CORE-002; CR 613.7k: a sticker gets a new timestamp when put on; when its host is
// re-stamped, every sticker gets a new one immediately after it, in the previous relative order.
#[test]
fn stickers_follow_host_in_order() {
    let mut g = IdGenerator::new();
    let mut l = TimestampLedger::new();
    l.stamp_object(&mut g, o(1), StampReason::EntersZone)
        .unwrap();
    let s2 = l.put_sticker(&mut g, o(1), 2).unwrap();
    let s1 = l.put_sticker(&mut g, o(1), 1).unwrap();
    l.put_sticker(&mut g, o(9), 1).unwrap();
    assert!(s2 < s1);
    assert_eq!(l.sticker_timestamp(o(1), 2), Some(s2));
    let host = l
        .stamp_object(&mut g, o(1), StampReason::JoinedMergedPermanent)
        .unwrap();
    let n2 = l.sticker_timestamp(o(1), 2).unwrap();
    let n1 = l.sticker_timestamp(o(1), 1).unwrap();
    assert_eq!(n2.get(), host.get() + 1);
    assert_eq!(n1.get(), host.get() + 2);
    assert_eq!(g.last_issued(), n1.get());
    // Another object's sticker is untouched.
    assert_eq!(l.sticker_timestamp(o(9), 1), Some(ts(4)));
}

// REQ-CORE-002; CR 613.7c: every counter of a kind shares the newest counter's timestamp; kinds
// and holders are independent; clearing forgets the entry.
#[test]
fn counters_share_new_timestamp() {
    let mut g = IdGenerator::new();
    let mut l = TimestampLedger::new();
    let a = CounterHolder::Object(o(1));
    let pl = CounterHolder::Player(p(1));
    let t1 = l.put_counters(&mut g, a, "+1/+1").unwrap();
    let tp = l.put_counters(&mut g, pl, "poison").unwrap();
    let tc = l.put_counters(&mut g, a, "charge").unwrap();
    let t2 = l.put_counters(&mut g, a, "+1/+1").unwrap();
    assert!(t1 < tp && tp < tc && tc < t2);
    assert_eq!(l.counter_timestamp(a, "+1/+1"), Some(t2));
    assert_eq!(l.counter_timestamp(a, "charge"), Some(tc));
    assert_eq!(l.counter_timestamp(pl, "poison"), Some(tp));
    l.clear_counters(a, "+1/+1");
    assert_eq!(l.counter_timestamp(a, "+1/+1"), None);
    assert_eq!(l.counter_timestamp(a, "charge"), Some(tc));
}

// REQ-CORE-002: forgetting an object drops its timestamp, its stickers and its counters only.
#[test]
fn forget_object_drops_its_entries() {
    let mut g = IdGenerator::new();
    let mut l = TimestampLedger::new();
    for id in [o(1), o(2)] {
        l.stamp_object(&mut g, id, StampReason::EntersZone).unwrap();
        l.put_sticker(&mut g, id, 1).unwrap();
        l.put_counters(&mut g, CounterHolder::Object(id), "+1/+1")
            .unwrap();
    }
    l.put_counters(&mut g, CounterHolder::Player(p(1)), "poison")
        .unwrap();
    l.forget_object(o(1));
    assert_eq!(l.object_timestamp(o(1)), None);
    assert_eq!(l.sticker_timestamp(o(1), 1), None);
    assert_eq!(
        l.counter_timestamp(CounterHolder::Object(o(1)), "+1/+1"),
        None
    );
    assert!(l.object_timestamp(o(2)).is_some());
    assert!(l.sticker_timestamp(o(2), 1).is_some());
    assert!(
        l.counter_timestamp(CounterHolder::Object(o(2)), "+1/+1")
            .is_some()
    );
    assert!(
        l.counter_timestamp(CounterHolder::Player(p(1)), "poison")
            .is_some()
    );
}

fn stamp(object: u64, player: u64, choice: u32) -> SimultaneousStamp {
    SimultaneousStamp {
        object: o(object),
        player: p(player),
        choice,
    }
}

// REQ-CORE-002; CR 613.7m, 101.4: simultaneous stamps go in APNAP order, each player's own
// objects in their chosen order; tested with each of the two players active.
#[test]
fn simultaneous_stamps_apnap_and_choice() {
    let batch = [
        stamp(1, 1, 2),
        stamp(2, 2, 0),
        stamp(3, 1, 1),
        stamp(4, 2, 5),
        stamp(5, 1, 1),
    ];
    let order = |apnap: &[PlayerId]| {
        let mut g = IdGenerator::from_last_issued(10);
        let mut l = TimestampLedger::new();
        let out = l
            .stamp_objects_simultaneously(&mut g, apnap, &batch, StampReason::EntersZone)
            .unwrap();
        for (id, t) in &out {
            assert_eq!(l.object_timestamp(*id), Some(*t));
        }
        let stamps: Vec<u64> = out.iter().map(|(_, t)| t.get()).collect();
        assert_eq!(stamps, [11, 12, 13, 14, 15]);
        out.into_iter().map(|(id, _)| id).collect::<Vec<_>>()
    };
    // Player 1 active: their objects first, in their choice order (ties by ObjectId).
    assert_eq!(order(&[p(1), p(2)]), [o(3), o(5), o(1), o(2), o(4)]);
    // Player 2 active.
    assert_eq!(order(&[p(2), p(1)]), [o(2), o(4), o(3), o(5), o(1)]);
}

// REQ-CORE-002; CR 613.7m, 613.7k: errors leave everything unchanged; an empty batch issues
// nothing; each object's stickers follow it.
#[test]
fn simultaneous_stamps_errors_and_stickers() {
    let mut g = IdGenerator::new();
    let mut l = TimestampLedger::new();
    l.put_sticker(&mut g, o(1), 7).unwrap();
    let (g0, l0) = (g.clone(), l.clone());
    assert_eq!(
        l.stamp_objects_simultaneously(
            &mut g,
            &[p(1)],
            &[stamp(1, 1, 0), stamp(2, 3, 0)],
            StampReason::EntersZone
        ),
        Err(LayerError::UnknownPlayer(p(3)))
    );
    assert_eq!(
        l.stamp_objects_simultaneously(
            &mut g,
            &[p(1)],
            &[stamp(1, 1, 0), stamp(1, 1, 1)],
            StampReason::EntersZone
        ),
        Err(LayerError::DuplicateObject(o(1)))
    );
    assert_eq!(
        l.stamp_objects_simultaneously(&mut g, &[p(1)], &[], StampReason::Attached),
        Ok(vec![])
    );
    assert_eq!((&g, &l), (&g0, &l0));
    let out = l
        .stamp_objects_simultaneously(
            &mut g,
            &[p(1)],
            &[stamp(2, 1, 0), stamp(1, 1, 0)],
            StampReason::Attached,
        )
        .unwrap();
    // o(1) at 2, its sticker at 3, o(2) at 4.
    assert_eq!(out, vec![(o(1), ts(2)), (o(2), ts(4))]);
    assert_eq!(l.sticker_timestamp(o(1), 7), Some(ts(3)));
}

// REQ-CORE-002; CR 613.7a: a static effect takes the object's timestamp, or the later of the
// object's and the granting effect's for a granted ability; Resolved leaves it unchanged.
#[test]
fn static_effect_timestamp_later_of() {
    let granted = |t| EffectOrigin::Static {
        granted_at: Some(ts(t)),
    };
    let own = EffectOrigin::Static { granted_at: None };
    assert_eq!(static_effect_timestamp(ts(5), &own), ts(5));
    assert_eq!(static_effect_timestamp(ts(5), &granted(9)), ts(9));
    assert_eq!(static_effect_timestamp(ts(5), &granted(2)), ts(5));
    assert_eq!(
        static_effect_timestamp(ts(5), &EffectOrigin::Resolved),
        ts(5)
    );
}

// REQ-CORE-002; CR 613.7b: each resolved effect gets a new timestamp at creation.
#[test]
fn resolved_effects_get_new_timestamps() {
    let mut g = IdGenerator::from_last_issued(3);
    assert_eq!(stamp_resolved_effect(&mut g), Ok(ts(4)));
    assert_eq!(stamp_resolved_effect(&mut g), Ok(ts(5)));
}

// REQ-CORE-002; CR 613.7n, 611.2e: the object's own static effect is earlier than the
// simultaneous resolved effect.
#[test]
fn static_before_simultaneous_resolved() {
    let mut g = IdGenerator::from_last_issued(7);
    let (s, r) = stamp_static_and_resolved(&mut g).unwrap();
    assert!(s < r);
    assert_eq!((s, r), (ts(8), ts(9)));
}

// REQ-CORE-002; CR 613.7a (second sentence), 613.7e: an Equipment with a printed effect and
// an effect granted later by an Aura granting the Equipment a static ability becomes attached:
// both effects get new timestamps, the granted one still after the printed one; a granted
// ability older than the object follows the object; resolved effects and other objects'
// effects are untouched.
#[test]
fn restamp_static_effects_keeps_relative_order() {
    let mut g = IdGenerator::new();
    let mut l = TimestampLedger::new();
    let equipment = o(1);
    let old = l
        .stamp_object(&mut g, equipment, StampReason::EntersZone)
        .unwrap();
    let aura_ts = l.stamp_object(&mut g, o(2), StampReason::Attached).unwrap();
    let granted_origin = EffectOrigin::Static {
        granted_at: Some(aura_ts),
    };
    let mut r = registry(vec![
        // Printed: "equipped creature loses flying".
        (
            1,
            stat(
                equipment,
                Layer::L6,
                old.get(),
                none(),
                LayerModification::RemoveKeyword("flying".to_owned()),
            ),
        ),
        // Granted by the Aura: "equipped creature has flying".
        (
            2,
            granted_stat(
                equipment,
                aura_ts.get(),
                Layer::L6,
                static_effect_timestamp(old, &granted_origin).get(),
                LayerModification::AddAbility(keyword("flying", "g")),
            ),
        ),
        (
            3,
            granted_stat(equipment, 0, Layer::L7c, old.get(), modify(1, 1)),
        ),
        (
            4,
            ContinuousEffect {
                source: Some(equipment),
                ..eff(Layer::L7c, 2, none(), modify(1, 1))
            },
        ),
        (
            5,
            stat(
                o(2),
                Layer::L6,
                2,
                none(),
                LayerModification::RemoveAllAbilities,
            ),
        ),
    ]);
    assert_eq!(r.get(e(2)).unwrap().timestamp, aura_ts);
    let new = l
        .stamp_object(&mut g, equipment, StampReason::Attached)
        .unwrap();
    restamp_static_effects(&mut r, &mut g, equipment, old, new).unwrap();
    let t = |id| r.get(e(id)).unwrap().timestamp;
    assert_eq!(t(1), new);
    assert_eq!(t(3), new);
    assert!(t(2) > new);
    assert!(t(1) < t(2));
    assert_eq!(t(4), ts(2));
    assert_eq!(t(5), ts(2));
}

// REQ-CORE-002; CR 613.7a: several granted effects keep their relative order (by old timestamp,
// then EffectId).
#[test]
fn restamp_granted_effects_in_old_order() {
    let mut g = IdGenerator::from_last_issued(20);
    let granted =
        |t: u64| granted_stat(o(1), t, Layer::L6, t, LayerModification::RemoveAllAbilities);
    let mut r = registry(vec![(1, granted(9)), (2, granted(7)), (3, granted(9))]);
    restamp_static_effects(&mut r, &mut g, o(1), ts(5), ts(20)).unwrap();
    let t = |id| r.get(e(id)).unwrap().timestamp;
    assert_eq!((t(2), t(1), t(3)), (ts(21), ts(22), ts(23)));
}

// REQ-CORE-002: an exhausted generator (`from_last_issued(u64::MAX)`) is IdExhausted for every
// operation, leaving the ledger, the registry and the generator unchanged, also when it runs
// out in the middle of an operation.
#[test]
fn exhaustion_leaves_everything_unchanged() {
    let mut l = TimestampLedger::new();
    let mut g = IdGenerator::from_last_issued(u64::MAX - 3);
    l.stamp_object(&mut g, o(1), StampReason::EntersZone)
        .unwrap();
    l.put_sticker(&mut g, o(1), 1).unwrap();
    // One stamp left: the object would get it, its sticker could not.
    let (g0, l0) = (g.clone(), l.clone());
    assert_eq!(
        l.stamp_object(&mut g, o(1), StampReason::Attached),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(
        l.stamp_objects_simultaneously(&mut g, &[p(1)], &[stamp(1, 1, 0)], StampReason::EntersZone),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(
        stamp_static_and_resolved(&mut g),
        Err(LayerError::IdExhausted)
    );
    assert_eq!((&g, &l), (&g0, &l0));

    let mut g = IdGenerator::from_last_issued(u64::MAX);
    let g0 = g.clone();
    assert_eq!(
        l.stamp_object(&mut g, o(2), StampReason::EntersZone),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(
        l.put_counters(&mut g, CounterHolder::Object(o(1)), "+1/+1"),
        Err(LayerError::IdExhausted)
    );
    assert_eq!(l.put_sticker(&mut g, o(1), 2), Err(LayerError::IdExhausted));
    assert_eq!(stamp_resolved_effect(&mut g), Err(LayerError::IdExhausted));
    assert_eq!(
        stamp_static_and_resolved(&mut g),
        Err(LayerError::IdExhausted)
    );
    let mut r = registry(vec![
        (
            1,
            granted_stat(o(1), 9, Layer::L6, 9, LayerModification::RemoveAllAbilities),
        ),
        (
            2,
            stat(
                o(1),
                Layer::L6,
                1,
                none(),
                LayerModification::RemoveAllAbilities,
            ),
        ),
    ]);
    let r0 = r.clone();
    assert_eq!(
        restamp_static_effects(&mut r, &mut g, o(1), ts(1), ts(3)),
        Err(LayerError::IdExhausted)
    );
    assert_eq!((&g, &l, &r), (&g0, &l0, &r0));
}

// REQ-LAYER-001, REQ-CORE-002: every error has a distinct message.
#[test]
fn error_display() {
    let all = [
        LayerError::DuplicateEffect(e(1)),
        LayerError::UnknownEffect(e(2)),
        LayerError::UnknownObject(o(3)),
        LayerError::LayerMismatch { layer: Layer::L7b },
        LayerError::Unsupported("x"),
        LayerError::UnknownPlayer(p(4)),
        LayerError::DuplicateObject(o(5)),
        LayerError::IdExhausted,
    ];
    let texts: BTreeSet<String> = all.iter().map(ToString::to_string).collect();
    assert_eq!(texts.len(), all.len());
    let as_error: &dyn std::error::Error = &all[0];
    assert!(as_error.source().is_none());
    assert_eq!(
        LayerError::from(fineprint_base::ids::IdExhausted),
        LayerError::IdExhausted
    );
}

// ---------------------------------------------------------------------------------------------
// CodeRabbit round 1 regressions
// ---------------------------------------------------------------------------------------------

// REQ-LAYER-001; CR 122.1b, 702.11d: the keyword-counter classifier accepts the fifteen names and
// their variants (name, one space, a parameter); ward and near-misses are not keyword counters.
#[test]
fn keyword_counter_keyword_classifier() {
    for kind in KEYWORD_COUNTER_KINDS {
        assert_eq!(keyword_counter_keyword(kind), Some(kind));
    }
    for kind in [
        "hexproof from red",
        "hexproof from artifacts",
        "first strike from test",
    ] {
        assert_eq!(keyword_counter_keyword(kind), Some(kind));
    }
    for kind in [
        "ward",
        "ward 2",
        "flyingx",
        "hexproofish",
        "hexproof ",
        "hexproof  from red",
        "Hexproof",
        "+1/+1",
        "charge",
        "",
    ] {
        assert_eq!(keyword_counter_keyword(kind), None, "{kind:?}");
    }
}

// REQ-LAYER-001; CR 122.1b, 702.11d, 613.1f: a variant keyword counter grants one ability with
// the full kind string as its keyword, whatever the amount; amount 0 grants nothing.
#[test]
fn variant_keyword_counter_grants_full_keyword() {
    let env = Env::default()
        .with(o(1), creature("test.bear", 2, 2))
        .counter(o(1), "hexproof from red", 3, 1)
        .with(o(2), creature("test.bear", 2, 2))
        .counter(o(2), "hexproof from red", 0, 1);
    let c = eval(&ContinuousEffects::new(), &env, o(1)).characteristics;
    assert_eq!(
        c.abilities,
        vec![keyword_counter_ability("hexproof from red")]
    );
    assert!(has_keyword(&c, "hexproof from red"));
    assert_eq!(
        c.abilities[0].0.get("abilityId").and_then(|v| v.as_str()),
        Some("counter:hexproof from red")
    );
    let c = eval(&ContinuousEffects::new(), &env, o(2)).characteristics;
    assert!(c.abilities.is_empty());
}

// REQ-LAYER-001; CR 122.1b, 613.1f, 613.7: a variant keyword counter applies in layer 6 at its
// counter timestamp: a later RemoveAllAbilities removes it, an earlier one does not.
#[test]
fn variant_keyword_counter_in_layer_6_timestamp_order() {
    for (remove_ts, has) in [(5, false), (1, true)] {
        let env = Env::default()
            .with(o(1), creature("test.bear", 2, 2))
            .counter(o(1), "hexproof from red", 1, 3);
        let r = registry(vec![(
            1,
            eff(
                Layer::L6,
                remove_ts,
                objects([o(1)]),
                LayerModification::RemoveAllAbilities,
            ),
        )]);
        let c = eval(&r, &env, o(1)).characteristics;
        assert_eq!(has_keyword(&c, "hexproof from red"), has, "{remove_ts}");
    }
}

// REQ-LAYER-001; CR 122.1b: ward is not a keyword counter, with or without a parameter.
#[test]
fn ward_counters_grant_nothing() {
    let env = Env::default()
        .with(o(1), creature("test.bear", 2, 2))
        .counter(o(1), "ward", 1, 1)
        .counter(o(1), "ward 2", 1, 2);
    let c = eval(&ContinuousEffects::new(), &env, o(1)).characteristics;
    assert!(c.abilities.is_empty());
}

// REQ-LAYER-001; CR 122.1a, 208.3, 613.4c: a noncreature card off the battlefield with printed
// 2/1 keeps 2/1 with a +1/+1 counter (counters modify creatures only); a layer-7c effect still
// applies to its printed P/T.
#[test]
fn noncreature_off_battlefield_with_printed_pt_ignores_counters() {
    let mut artifact = noncreature("test.artifact_2_1", "Artifact");
    artifact.power = Some(literal(2));
    artifact.toughness = Some(literal(1));
    let env = Env::default()
        .with(o(1), artifact)
        .off_battlefield(o(1))
        .counter(o(1), "+1/+1", 1, 1);
    assert_eq!(eval_pt(&ContinuousEffects::new(), &env, o(1)), Some((2, 1)));
    let r = registry(vec![(1, eff(Layer::L7c, 2, objects([o(1)]), modify(1, 0)))]);
    assert_eq!(eval_pt(&r, &env, o(1)), Some((3, 1)));
}

// REQ-LAYER-001; CR 122.1a, 613.1d, 613.4c: an artifact animated in layer 4 is a creature at
// layer 7c, so its +1/+1 counter applies (0/0 + 1/1).
#[test]
fn animated_artifact_gets_pt_counters() {
    let env = Env::default()
        .with(o(1), noncreature("test.artifact", "Artifact"))
        .counter(o(1), "+1/+1", 1, 1);
    assert_eq!(eval_pt(&ContinuousEffects::new(), &env, o(1)), None);
    let r = registry(vec![(
        1,
        eff(
            Layer::L4,
            2,
            objects([o(1)]),
            LayerModification::AddTypes {
                supertypes: BTreeSet::new(),
                card_types: set(["Creature"]),
                subtypes: BTreeSet::new(),
            },
        ),
    )]);
    assert_eq!(eval_pt(&r, &env, o(1)), Some((1, 1)));
}

// REQ-LAYER-001; CR 122.1a: a creature card in a zone other than the battlefield gets its P/T
// counters.
#[test]
fn creature_card_off_battlefield_gets_pt_counters() {
    let env = Env::default()
        .with(o(1), creature("test.bear", 2, 2))
        .off_battlefield(o(1))
        .counter(o(1), "+1/+1", 2, 1);
    assert_eq!(eval_pt(&ContinuousEffects::new(), &env, o(1)), Some((4, 4)));
}

// REQ-CORE-002, REQ-LAYER-001; CR 613.7a (second sentence), 613.1f: an Equipment has a printed
// "equipped creature loses flying" (timestamp 1) and, from an Aura granting it a static ability,
// "equipped creature has flying" (timestamp 2) whose EffectId is LOWER than the removal's. It is
// reattached three times; after each, the removal stays before the grant, the creature has
// flying, and `granted_at` is unchanged.
#[test]
fn repeated_reattachment_keeps_grant_after_removal() {
    let mut g = IdGenerator::new();
    let mut l = TimestampLedger::new();
    let equipment = o(1);
    let creature_id = o(3);
    let mut object_ts = l
        .stamp_object(&mut g, equipment, StampReason::EntersZone)
        .unwrap();
    let aura_ts = l.stamp_object(&mut g, o(2), StampReason::Attached).unwrap();
    assert_eq!((object_ts, aura_ts), (ts(1), ts(2)));
    let origin = EffectOrigin::Static {
        granted_at: Some(aura_ts),
    };
    let mut r = registry(vec![
        // The grant, lower EffectId.
        (
            1,
            ContinuousEffect {
                affected: objects([creature_id]),
                ..granted_stat(
                    equipment,
                    aura_ts.get(),
                    Layer::L6,
                    static_effect_timestamp(object_ts, &origin).get(),
                    LayerModification::AddAbility(keyword("flying", "g")),
                )
            },
        ),
        // The printed removal, higher EffectId.
        (
            2,
            stat(
                equipment,
                Layer::L6,
                object_ts.get(),
                objects([creature_id]),
                LayerModification::RemoveKeyword("flying".to_owned()),
            ),
        ),
    ]);
    let env = Env::default().with(creature_id, creature("test.bear", 2, 2));
    assert!(has_keyword(
        &eval(&r, &env, creature_id).characteristics,
        "flying"
    ));
    for _ in 0..3 {
        let new = l
            .stamp_object(&mut g, equipment, StampReason::Attached)
            .unwrap();
        restamp_static_effects(&mut r, &mut g, equipment, object_ts, new).unwrap();
        object_ts = new;
        let grant = r.get(e(1)).unwrap();
        let removal = r.get(e(2)).unwrap();
        assert_eq!(removal.timestamp, new);
        assert!(removal.timestamp < grant.timestamp);
        assert_eq!(grant.origin, origin);
        assert!(has_keyword(
            &eval(&r, &env, creature_id).characteristics,
            "flying"
        ));
    }
}

/// A layer-6 `RemoveKeyword(k)` effect on o(1) at timestamp `t`.
fn remove_keyword(k: &str, t: u64) -> ContinuousEffects {
    registry(vec![(
        1,
        eff(
            Layer::L6,
            t,
            objects([o(1)]),
            LayerModification::RemoveKeyword(k.to_owned()),
        ),
    )])
}

// REQ-LAYER-001; CR 613.1f, 702.11e: removing hexproof removes hexproof and every
// "hexproof from [quality]" variant, and nothing else.
#[test]
fn remove_hexproof_removes_variants() {
    let mut c = creature("test.bear", 2, 2);
    c.abilities = vec![
        keyword("hexproof", "a1"),
        keyword("hexproof from red", "a2"),
        keyword("hexproof from artifacts", "a3"),
        keyword("hexproofish", "a4"),
        keyword("flying", "a5"),
        keyword("ward 2", "a6"),
        ability("hexproof"),
    ];
    let env = Env::default().with(o(1), c);
    assert_eq!(
        eval(&remove_keyword("hexproof", 1), &env, o(1))
            .characteristics
            .abilities,
        vec![
            keyword("hexproofish", "a4"),
            keyword("flying", "a5"),
            keyword("ward 2", "a6"),
            ability("hexproof"),
        ]
    );
}

// REQ-LAYER-001; CR 613.1f, 702.11e: removing one variant removes that string only.
#[test]
fn remove_hexproof_variant_is_exact() {
    let mut c = creature("test.bear", 2, 2);
    c.abilities = vec![
        keyword("hexproof", "a1"),
        keyword("hexproof from red", "a2"),
        keyword("hexproof from blue", "a3"),
    ];
    let env = Env::default().with(o(1), c);
    assert_eq!(
        eval(&remove_keyword("hexproof from red", 1), &env, o(1))
            .characteristics
            .abilities,
        vec![
            keyword("hexproof", "a1"),
            keyword("hexproof from blue", "a3"),
        ]
    );
}

// REQ-LAYER-001; CR 613.1f, 702.11e: only hexproof removal removes variants; removing flying
// leaves "flying from red" and "protection from red".
#[test]
fn remove_other_keyword_leaves_variants() {
    let mut c = creature("test.bear", 2, 2);
    c.abilities = vec![
        keyword("flying", "a1"),
        keyword("flying from red", "a2"),
        keyword("protection from red", "a3"),
    ];
    let env = Env::default().with(o(1), c);
    assert_eq!(
        eval(&remove_keyword("flying", 1), &env, o(1))
            .characteristics
            .abilities,
        vec![
            keyword("flying from red", "a2"),
            keyword("protection from red", "a3"),
        ]
    );
}

// REQ-LAYER-001; CR 122.1b, 613.1f, 613.7, 702.11e: a "hexproof from red" keyword counter is
// removed by a later RemoveKeyword("hexproof") and survives an earlier one.
#[test]
fn remove_hexproof_vs_variant_counter_timestamp_order() {
    for (remove_ts, has) in [(5, false), (1, true)] {
        let env = Env::default()
            .with(o(1), creature("test.bear", 2, 2))
            .counter(o(1), "hexproof from red", 1, 3);
        let c = eval(&remove_keyword("hexproof", remove_ts), &env, o(1)).characteristics;
        assert_eq!(has_keyword(&c, "hexproof from red"), has, "{remove_ts}");
    }
}

// REQ-LAYER-001; CR 702.11d, 702.11e: the single definition of a keyword variant.
#[test]
fn variant_helper_accepts_and_rejects() {
    assert!(pipeline::is_variant_of("hexproof from red", "hexproof"));
    for candidate in ["hexproof", "hexproof ", "hexproof  x", "hexproofish"] {
        assert!(
            !pipeline::is_variant_of(candidate, "hexproof"),
            "{candidate:?}"
        );
    }
}
