// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of layer 1a (copy.rs): REQ-OBJ-002 (copiable values, copy effects with
// exceptions, merged permanents, mana value of copies), REQ-LAYER-001 (layer-1a effects through
// the evaluator), REQ-CORE-001 (determinism). Synthetic ids, characteristics and definitions
// only (R5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{
    AbilityAST, CardDefinitionDSL, CardFaceDSL, ColorBitmask, DSL_VERSION, FaceKind,
    LayoutBehavior, Origin, ProvenanceDSL, SupportStatus, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::expr::ObjectSelectorAST;
use fineprint_dsl::mana::ManaCostDSL;
use fineprint_dsl::vocab::RulesVocabulary;

use super::*;
use crate::layouts::{
    CastChoice, FaceUp, LayoutQuery, LayoutStatus, MeldedPair, MergeSide, MergedComponent,
    MergedPermanent, base_characteristics,
};
use crate::objects::{Characteristics, ManaValueContext, VariableValues};
use crate::zones::ZoneKind;

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

const OFF: ManaValueContext = ManaValueContext::OffStack;

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

fn vocab() -> &'static RulesVocabulary {
    RulesVocabulary::bundled().unwrap()
}

fn cost(text: &str) -> ManaCostDSL {
    ManaCostDSL::parse(text).unwrap()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn ability(tag: &str) -> AbilityAST {
    AbilityAST(format!("test.ability.{tag}").into())
}

/// A JSON object built from `(member, value)` pairs (values carried in `AbilityAST` wrappers).
fn obj(pairs: Vec<(&str, AbilityAST)>) -> AbilityAST {
    AbilityAST(pairs.into_iter().map(|(k, v)| (k, v.0)).collect())
}

/// A JSON string.
fn js(s: &str) -> AbilityAST {
    AbilityAST(s.into())
}

/// A static ability with one `CHAR` effect in `layer`, characteristic-defining when `cda`.
fn char_static(layer: &str, cda: bool) -> AbilityAST {
    let effect = obj(vec![
        ("type", js("CHAR")),
        ("layer", js(layer)),
        ("affected", obj(vec![("ref", js("SELF"))])),
        ("modification", obj(vec![("op", js("TEST_OP"))])),
        ("isCDA", AbilityAST(cda.into())),
    ]);
    obj(vec![
        ("kind", js("STATIC")),
        ("effects", AbilityAST(vec![effect.0].into())),
    ])
}

/// A synthetic creature `name` with literal P/T, cost `{2}{G}`, green, one ability.
fn creature(name: &str, power: i64, toughness: i64) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: Some(cost("{2}{G}")),
        color_indicator: None,
        colors: ColorBitmask::GREEN,
        supertypes: BTreeSet::new(),
        card_types: set(&["Creature"]),
        subtypes: set(&["Bear"]),
        rules_text: TextTokenStream(vec![format!("test.text.{name}").into()]),
        abilities: vec![ability(name)],
        power: Some(literal(power)),
        toughness: Some(literal(toughness)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A characteristics value with every field different from [`creature`]'s.
fn other(name: &str) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: Some(cost("{U}")),
        color_indicator: Some(ColorBitmask::RED),
        colors: ColorBitmask::new(ColorBitmask::BLUE.bits() | ColorBitmask::RED.bits()).unwrap(),
        supertypes: set(&["Legendary"]),
        card_types: set(&["Artifact", "Creature"]),
        subtypes: set(&["Golem"]),
        rules_text: TextTokenStream(vec![format!("test.text.{name}").into()]),
        abilities: vec![ability(name)],
        power: Some(literal(7)),
        toughness: Some(literal(8)),
        loyalty: Some(3),
        defense: Some(4),
        hand_modifier: Some(-1),
        life_modifier: Some(5),
    }
}

/// The face-down characteristics: a 2/2 creature with no name, cost, color or text.
fn face_down() -> Characteristics {
    Characteristics {
        names: BTreeSet::new(),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: set(&["Creature"]),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream(Vec::new()),
        abilities: Vec::new(),
        power: Some(literal(2)),
        toughness: Some(literal(2)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn objs(ids: &[ObjectId]) -> BTreeSet<ObjectId> {
    ids.iter().copied().collect()
}

/// A resolved layer-1a copy effect of `values` on `target` at `t`.
fn copy_eff(values: &CopiableValues, target: ObjectId, t: u64) -> ContinuousEffect {
    copiable_effect(
        values,
        None,
        p(1),
        EffectOrigin::Resolved,
        ts(t),
        objs(&[target]),
    )
}

/// A resolved effect on `target` in `layer` at `t`.
fn plain_eff(layer: Layer, t: u64, target: ObjectId, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: p(1),
        timestamp: ts(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected: Affected::Objects(objs(&[target])),
        modification: m,
    }
}

/// A synthetic environment: bases on the battlefield, counters, and the value 5 for every
/// non-literal expression.
#[derive(Default, Clone)]
struct Env {
    bases: BTreeMap<ObjectId, ObjectBase>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
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

    fn counter(mut self, id: ObjectId, kind: &str, n: i64) -> Env {
        self.counters
            .entry(id)
            .or_default()
            .insert(kind.to_owned(), n);
        self
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

    fn matches(&self, _: &ObjectSelectorAST, _: &MatchContext<'_>) -> bool {
        false
    }

    fn eval_value(&self, expr: &ValueExpressionAST, _: ObjectId) -> Option<i64> {
        literal_of(expr).or(Some(5))
    }
}

fn eval(r: &ContinuousEffects, env: &Env, id: ObjectId) -> Characteristics {
    evaluate(r, env, id).unwrap().characteristics
}

fn registry(list: Vec<(u64, ContinuousEffect)>) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    for (id, effect) in list {
        r.register(e(id), effect).unwrap();
    }
    r
}

// --- synthetic card definitions --------------------------------------------------------------

fn face(kind: FaceKind, tag: &str, mana_cost: Option<&str>, pt: Option<(i64, i64)>) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: kind,
        name: format!("test.{tag}"),
        mana_cost: mana_cost.map(cost),
        color_indicator: None,
        supertypes: Vec::new(),
        card_types: vec!["Creature".to_owned()],
        subtypes: Vec::new(),
        power: pt.map(|(p, _)| ValueExpressionAST(p.into())),
        toughness: pt.map(|(_, t)| ValueExpressionAST(t.into())),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: TextTokenStream(vec![format!("test.text.{tag}").into()]),
        abilities: vec![ability(tag)],
    }
}

fn def(id: &str, layout: LayoutBehavior, faces: Vec<CardFaceDSL>) -> CardDefinitionDSL {
    CardDefinitionDSL {
        card_def_id: id.to_owned(),
        dsl_version: DSL_VERSION.to_owned(),
        definition_hash: String::new(),
        source: ProvenanceDSL {
            oracle_text_hash: "0".repeat(64),
            origin: Origin::Parser,
            parser_version: None,
            override_source: None,
            verified_by: None,
            rules_baseline: "test-baseline".to_owned(),
        },
        support_status: SupportStatus::AutoParsed,
        known_gaps: None,
        layout,
        faces,
        meld_partner: None,
        color_identity: ColorBitmask::NONE,
        commander_eligibility: None,
        legalities: BTreeMap::new(),
        digital_only: false,
        related_tokens: None,
    }
}

/// A NORMAL creature card `tag` with `mana_cost` and P/T.
fn normal(tag: &str, mana_cost: &str, pt: (i64, i64)) -> CardDefinitionDSL {
    def(
        &format!("test.{tag}"),
        LayoutBehavior::Normal,
        vec![face(FaceKind::Main, tag, Some(mana_cost), Some(pt))],
    )
}

/// A nonmodal DFC: front `{3}{R}` 3/3, back (no mana cost) 5/5.
fn nonmodal_dfc() -> CardDefinitionDSL {
    def(
        "test.dfc",
        LayoutBehavior::DfcNonmodal,
        vec![
            face(FaceKind::Main, "dfc_front", Some("{3}{R}"), Some((3, 3))),
            face(FaceKind::Back, "dfc_back", None, Some((5, 5))),
        ],
    )
}

/// Meld card A: front `{2}{W}` 2/2, carries the combined 9/9 face.
fn meld_a() -> CardDefinitionDSL {
    let mut d = def(
        "test.meld_a",
        LayoutBehavior::Meld,
        vec![
            face(FaceKind::Main, "meld_a", Some("{2}{W}"), Some((2, 2))),
            face(FaceKind::MeldResult, "melded", None, Some((9, 9))),
        ],
    );
    d.meld_partner = Some("test.meld_b".to_owned());
    d
}

/// Meld card B: front `{X}{3}{U}` 3/3.
fn meld_b() -> CardDefinitionDSL {
    let mut d = def(
        "test.meld_b",
        LayoutBehavior::Meld,
        vec![face(
            FaceKind::Main,
            "meld_b",
            Some("{X}{3}{U}"),
            Some((3, 3)),
        )],
    );
    d.meld_partner = Some("test.meld_a".to_owned());
    d
}

/// A PROTOTYPE card: normal `{6}` 6/6 colorless; prototype `{1}{R}` 3/2.
fn prototype() -> CardDefinitionDSL {
    def(
        "test.prototype",
        LayoutBehavior::Prototype,
        vec![
            face(FaceKind::Main, "proto_normal", Some("{6}"), Some((6, 6))),
            face(
                FaceKind::Prototype,
                "proto_alt",
                Some("{1}{R}"),
                Some((3, 2)),
            ),
        ],
    )
}

fn component(d: &CardDefinitionDSL) -> MergedComponent<'_> {
    MergedComponent {
        def: d,
        query: LayoutQuery::new(ZoneKind::Battlefield),
        is_token: false,
        face_up: true,
    }
}

fn printed(d: &CardDefinitionDSL) -> Characteristics {
    base_characteristics(d, &LayoutQuery::new(ZoneKind::Battlefield), vocab()).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Copy of a copy (CR 707.2, 707.2b, 707.3)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-002; CR 707.2, 707.3, 707.2b: B copies A with an exception; C copies B and gets B's
// copiable values including the exception; changing A afterwards changes neither built effect.
#[test]
fn copy_of_a_copy_keeps_exceptions_and_is_a_snapshot() {
    let (a, b, c) = (o(1), o(2), o(3));
    let env = Env::default()
        .with(a, creature("test.a", 3, 3))
        .with(b, other("test.b"))
        .with(c, other("test.c"));
    let mut r = ContinuousEffects::new();
    // B becomes a copy of A, except it is an artifact in addition to its other types.
    let a_values = CopiableValues::plain(copiable_characteristics(&r, &env, a).unwrap());
    let b_values = a_values.copy_with(
        &env.base(b).unwrap().characteristics,
        &[CopyException::AddCardTypes(set(&["Artifact"]))],
    );
    r.register(e(1), copy_eff(&b_values, b, 10)).unwrap();
    // C copies B: B's copiable values are the copy's (CR 707.3).
    let b_copiable = copiable_characteristics(&r, &env, b).unwrap();
    assert_eq!(b_copiable, b_values.characteristics);
    let c_values =
        CopiableValues::plain(b_copiable).copy_with(&env.base(c).unwrap().characteristics, &[]);
    r.register(e(2), copy_eff(&c_values, c, 11)).unwrap();
    let got_c = eval(&r, &env, c);
    assert_eq!(got_c.names, set(&["test.a"]));
    assert_eq!(got_c.card_types, set(&["Artifact", "Creature"]));
    assert_eq!(got_c.power, Some(literal(3)));
    // CR 707.2b: A's copiable values change afterwards (a later copy effect on A).
    r.register(
        e(3),
        copy_eff(&CopiableValues::plain(other("test.z")), a, 12),
    )
    .unwrap();
    assert_eq!(eval(&r, &env, a).names, set(&["test.z"]));
    assert_eq!(eval(&r, &env, b).names, set(&["test.a"]));
    assert_eq!(eval(&r, &env, c), got_c);
    // The built values themselves are unchanged.
    assert_eq!(c_values.characteristics, b_values.characteristics);
}

// ---------------------------------------------------------------------------------------------
// Face-down (CR 707.2 example, 708.2, 708.10)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-002, REQ-LAYER-001; CR 708.2, 707.2 example, 708.10, 613.2a, 613.2b: a copy of a
// face-down object takes the face-down 2/2 and is face up; a face-down object that becomes a
// copy keeps its face-down characteristics (layer 1b after 1a); without the face-down effect
// the copied values show.
#[test]
fn copy_of_face_down_and_face_down_copy() {
    let (x, y) = (o(1), o(2));
    let env = Env::default()
        .with(x, creature("test.hidden", 6, 6))
        .with(y, other("test.copier"));
    let mut r = registry(vec![(
        1,
        plain_eff(
            Layer::L1b,
            5,
            x,
            LayerModification::FaceDown(Box::new(face_down())),
        ),
    )]);
    let x_values = copiable_characteristics(&r, &env, x).unwrap();
    assert_eq!(x_values, face_down());
    let y_values =
        CopiableValues::plain(x_values).copy_with(&env.base(y).unwrap().characteristics, &[]);
    r.register(e(2), copy_eff(&y_values, y, 6)).unwrap();
    assert_eq!(eval(&r, &env, y), face_down());
    // CR 707.2 example: the copy is face up: no face-down effect applies to it.
    assert!(!r.iter().any(|(_, eff)| {
        matches!(eff.modification, LayerModification::FaceDown(_))
            && matches!(&eff.affected, Affected::Objects(s) if s.contains(&y))
    }));
    assert_eq!(y_values.source, ValueSource::Plain);
    assert_eq!(y_values.mana_value(OFF), 0);

    // CR 708.10: X (face down) becomes a copy of another creature: it stays face down.
    let z_values = CopiableValues::plain(creature("test.z", 4, 4))
        .copy_with(&env.base(x).unwrap().characteristics, &[]);
    r.register(e(3), copy_eff(&z_values, x, 7)).unwrap();
    assert_eq!(eval(&r, &env, x), face_down());
    assert_eq!(copiable_characteristics(&r, &env, x).unwrap(), face_down());
    // CR 708.10 last sentence: without the face-down effect the copied values show.
    assert!(r.remove(e(1)));
    assert_eq!(eval(&r, &env, x), creature("test.z", 4, 4));
    assert_eq!(
        copiable_characteristics(&r, &env, x).unwrap(),
        creature("test.z", 4, 4)
    );
}

// ---------------------------------------------------------------------------------------------
// Prototype (CR 719.2a, 719.3b, 719.3d)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-002; CR 719.2a, 719.3b, 719.3d: a copy of a prototyped permanent has the prototype
// P/T, mana cost and colors, and the prototype cost's mana value; not the normal values.
#[test]
fn copy_of_prototyped_permanent() {
    let d = prototype();
    let q = LayoutQuery::new(ZoneKind::Battlefield).with_cast(CastChoice::Prototype);
    let proto = base_characteristics(&d, &q, vocab()).unwrap();
    let values = CopiableValues::plain(proto.clone());
    assert_eq!(values.mana_value(OFF), 2);
    let copy = values.copy_with(&other("test.copier"), &[]);
    let c = &copy.characteristics;
    assert_eq!(c.power, Some(literal(3)));
    assert_eq!(c.toughness, Some(literal(2)));
    assert_eq!(c.mana_cost, Some(cost("{1}{R}")));
    assert_eq!(c.colors, ColorBitmask::RED);
    assert_eq!(c.names, set(&["test.proto_normal"]));
    assert_eq!(copy.mana_value(OFF), 2);
    assert_ne!(c.power, Some(literal(6)));
    assert_ne!(c.mana_cost, Some(cost("{6}")));
    // Through the layers: an object copying the prototyped permanent.
    let (src, y) = (o(1), o(2));
    let env = Env::default()
        .with(src, proto)
        .with(y, other("test.copier"));
    let mut r = ContinuousEffects::new();
    let from = CopiableValues::plain(copiable_characteristics(&r, &env, src).unwrap());
    r.register(
        e(1),
        copy_eff(&from.copy_with(&other("test.copier"), &[]), y, 3),
    )
    .unwrap();
    let got = eval(&r, &env, y);
    assert_eq!(got.power, Some(literal(3)));
    assert_eq!(got.mana_cost, Some(cost("{1}{R}")));
    assert_eq!(got.colors, ColorBitmask::RED);
}

// ---------------------------------------------------------------------------------------------
// Exceptions (CR 707.9a-d)
// ---------------------------------------------------------------------------------------------

/// A copy of a 3/3 creature by [`other`] with the one exception.
fn copy1(exception: CopyException) -> Characteristics {
    CopiableValues::plain(creature("test.orig", 3, 3))
        .copy_with(&other("test.own"), &[exception])
        .characteristics
}

// REQ-OBJ-002; CR 707.9a: a gained ability is appended to the copied abilities.
#[test]
fn exception_gain_ability() {
    let c = copy1(CopyException::GainAbility(ability("gained")));
    assert_eq!(c.abilities, vec![ability("test.orig"), ability("gained")]);
}

// REQ-OBJ-002; CR 707.9b: names, type sets, power and toughness are set exactly.
#[test]
fn exception_set_fields() {
    assert_eq!(
        copy1(CopyException::SetNames(set(&["test.n"]))).names,
        set(&["test.n"])
    );
    assert_eq!(
        copy1(CopyException::SetSupertypes(set(&["Legendary"]))).supertypes,
        set(&["Legendary"])
    );
    assert_eq!(
        copy1(CopyException::SetCardTypes(set(&["Artifact"]))).card_types,
        set(&["Artifact"])
    );
    assert_eq!(
        copy1(CopyException::SetSubtypes(set(&["Golem"]))).subtypes,
        set(&["Golem"])
    );
    let c = copy1(CopyException::SetPower(literal(9)));
    assert_eq!((c.power, c.toughness), (Some(literal(9)), Some(literal(3))));
    let c = copy1(CopyException::SetToughness(literal(9)));
    assert_eq!((c.power, c.toughness), (Some(literal(3)), Some(literal(9))));
}

// REQ-OBJ-002; CR 707.9b, docs/DECISIONS.md "#38 FP-037 — SetManaCost and colors": the colors
// follow the new cost when they were the old cost's derivation, else they stay.
#[test]
fn exception_set_mana_cost_colors() {
    let c = copy1(CopyException::SetManaCost(Some(cost("{1}{W}"))));
    assert_eq!(c.mana_cost, Some(cost("{1}{W}")));
    assert_eq!(c.colors, ColorBitmask::WHITE);
    let c = copy1(CopyException::SetManaCost(None));
    assert_eq!(c.mana_cost, None);
    assert_eq!(c.colors, ColorBitmask::NONE);
    // Colors not derived from the cost: left as they are.
    let mut orig = creature("test.orig", 3, 3);
    orig.colors = ColorBitmask::BLACK;
    let c = CopiableValues::plain(orig)
        .copy_with(
            &other("test.own"),
            &[CopyException::SetManaCost(Some(cost("{1}{W}")))],
        )
        .characteristics;
    assert_eq!(c.colors, ColorBitmask::BLACK);
    assert_eq!(c.mana_cost, Some(cost("{1}{W}")));
}

// REQ-OBJ-002; CR 707.9b, 707.9d: SetColors sets the colors and clears the color indicator.
#[test]
fn exception_set_colors_clears_indicator() {
    let mut orig = creature("test.orig", 3, 3);
    orig.color_indicator = Some(ColorBitmask::BLUE);
    orig.colors =
        ColorBitmask::new(ColorBitmask::BLUE.bits() | ColorBitmask::GREEN.bits()).unwrap();
    let c = CopiableValues::plain(orig)
        .copy_with(
            &other("test.own"),
            &[CopyException::SetColors(ColorBitmask::WHITE)],
        )
        .characteristics;
    assert_eq!(c.colors, ColorBitmask::WHITE);
    assert_eq!(c.color_indicator, None);
}

// REQ-OBJ-002; CR 707.9d last sentence: "in addition to its other types" keeps the copied set.
#[test]
fn exception_add_types_union() {
    assert_eq!(
        copy1(CopyException::AddCardTypes(set(&["Artifact"]))).card_types,
        set(&["Artifact", "Creature"])
    );
    assert_eq!(
        copy1(CopyException::AddSubtypes(set(&["Golem"]))).subtypes,
        set(&["Bear", "Golem"])
    );
    assert_eq!(
        copy1(CopyException::AddSupertypes(set(&["Legendary"]))).supertypes,
        set(&["Legendary"])
    );
}

// REQ-OBJ-002; CR 707.9b, 707.9d: SetPower replaces a non-literal (CDA) power expression.
#[test]
fn exception_set_power_replaces_cda_expression() {
    let mut orig = creature("test.orig", 0, 0);
    let cda = ValueExpressionAST(js("test.cda_count").0);
    orig.power = Some(cda.clone());
    orig.toughness = Some(cda.clone());
    let c = CopiableValues::plain(orig)
        .copy_with(&other("test.own"), &[CopyException::SetPower(literal(4))])
        .characteristics;
    assert_eq!(c.power, Some(literal(4)));
    assert_eq!(c.toughness, Some(cda));
}

// REQ-OBJ-002; CR 707.9: exceptions apply in slice order: the last one on a field wins.
#[test]
fn exception_order_last_wins() {
    let v = CopiableValues::plain(creature("test.orig", 3, 3));
    let own = other("test.own");
    let a = v.copy_with(
        &own,
        &[
            CopyException::SetNames(set(&["test.first"])),
            CopyException::SetNames(set(&["test.second"])),
        ],
    );
    assert_eq!(a.characteristics.names, set(&["test.second"]));
    let b = v.copy_with(
        &own,
        &[
            CopyException::SetPower(literal(1)),
            CopyException::NotCopied(CopiedCharacteristic::Power),
        ],
    );
    assert_eq!(b.characteristics.power, own.power);
    let c = v.copy_with(
        &own,
        &[
            CopyException::NotCopied(CopiedCharacteristic::Power),
            CopyException::SetPower(literal(1)),
        ],
    );
    assert_eq!(c.characteristics.power, Some(literal(1)));
    // An empty slice is a plain copy.
    assert_eq!(
        v.copy_with(&own, &[]).characteristics,
        creature("test.orig", 3, 3)
    );
}

/// Every [`CopiedCharacteristic`].
const ALL_FIELDS: [CopiedCharacteristic; 15] = [
    CopiedCharacteristic::Names,
    CopiedCharacteristic::ManaCost,
    CopiedCharacteristic::ColorIndicator,
    CopiedCharacteristic::Colors,
    CopiedCharacteristic::Supertypes,
    CopiedCharacteristic::CardTypes,
    CopiedCharacteristic::Subtypes,
    CopiedCharacteristic::RulesText,
    CopiedCharacteristic::Abilities,
    CopiedCharacteristic::Power,
    CopiedCharacteristic::Toughness,
    CopiedCharacteristic::Loyalty,
    CopiedCharacteristic::Defense,
    CopiedCharacteristic::HandModifier,
    CopiedCharacteristic::LifeModifier,
];

/// `c` with field `f` taken from `own` (the expectation of `NotCopied(f)`).
fn with_field(
    mut c: Characteristics,
    own: &Characteristics,
    f: CopiedCharacteristic,
) -> Characteristics {
    match f {
        CopiedCharacteristic::Names => c.names.clone_from(&own.names),
        CopiedCharacteristic::ManaCost => c.mana_cost.clone_from(&own.mana_cost),
        CopiedCharacteristic::ColorIndicator => c.color_indicator = own.color_indicator,
        CopiedCharacteristic::Colors => {
            c.colors = own.colors;
            c.color_indicator = own.color_indicator;
        }
        CopiedCharacteristic::Supertypes => c.supertypes.clone_from(&own.supertypes),
        CopiedCharacteristic::CardTypes => c.card_types.clone_from(&own.card_types),
        CopiedCharacteristic::Subtypes => c.subtypes.clone_from(&own.subtypes),
        CopiedCharacteristic::RulesText => c.rules_text.clone_from(&own.rules_text),
        CopiedCharacteristic::Abilities => c.abilities.clone_from(&own.abilities),
        CopiedCharacteristic::Power => c.power.clone_from(&own.power),
        CopiedCharacteristic::Toughness => c.toughness.clone_from(&own.toughness),
        CopiedCharacteristic::Loyalty => c.loyalty = own.loyalty,
        CopiedCharacteristic::Defense => c.defense = own.defense,
        CopiedCharacteristic::HandModifier => c.hand_modifier = own.hand_modifier,
        CopiedCharacteristic::LifeModifier => c.life_modifier = own.life_modifier,
    }
    c
}

// REQ-OBJ-002; CR 707.9c, 707.9d: NotCopied(f) keeps the copier's own value of f, and only f
// (Colors also keeps the color indicator); for every CopiedCharacteristic.
#[test]
fn exception_not_copied_each_field() {
    let orig = creature("test.orig", 3, 3);
    let own = other("test.own");
    for f in ALL_FIELDS {
        let got = CopiableValues::plain(orig.clone())
            .copy_with(&own, &[CopyException::NotCopied(f)])
            .characteristics;
        assert_eq!(got, with_field(orig.clone(), &own, f), "{f:?}");
        assert_ne!(got, orig, "{f:?}");
    }
    // ColorIndicator alone leaves the colors.
    let got = CopiableValues::plain(orig.clone())
        .copy_with(
            &own,
            &[CopyException::NotCopied(
                CopiedCharacteristic::ColorIndicator,
            )],
        )
        .characteristics;
    assert_eq!(got.colors, orig.colors);
    assert_eq!(got.color_indicator, own.color_indicator);
}

// REQ-OBJ-002; CR 707.9a, 707.3: a gained ability is part of the copy's copiable values, so a
// later copy of the copy has it.
#[test]
fn gained_ability_is_copied_by_a_later_copy() {
    let first = CopiableValues::plain(creature("test.orig", 3, 3)).copy_with(
        &other("test.b"),
        &[CopyException::GainAbility(ability("gained"))],
    );
    let second = first.copy_with(&other("test.c"), &[]);
    assert!(
        second
            .characteristics
            .abilities
            .contains(&ability("gained"))
    );
    assert_eq!(second.characteristics, first.characteristics);
}

// REQ-OBJ-002; CR 707.9d: a copied CDA that defines a characteristic the effect sets, keeps or
// does not copy is not copied; "in addition to its other types" keeps type CDAs; gained
// abilities and unrelated abilities stay.
#[test]
fn exception_drops_matching_cda_abilities() {
    let types_cda = char_static("4", true);
    let colors_cda = char_static("5", true);
    let pt_cda = char_static("7a", true);
    let not_cda = char_static("5", false);
    let all = vec![
        ability("plain"),
        types_cda.clone(),
        colors_cda.clone(),
        pt_cda.clone(),
        not_cda.clone(),
    ];
    let mut orig = creature("test.orig", 3, 3);
    orig.abilities.clone_from(&all);
    let v = CopiableValues::plain(orig);
    let own = other("test.own");
    let abilities = |ex: &[CopyException]| v.copy_with(&own, ex).characteristics.abilities;
    let without = |gone: &AbilityAST| -> Vec<AbilityAST> {
        all.iter().filter(|a| *a != gone).cloned().collect()
    };
    assert_eq!(abilities(&[]), all);
    for ex in [
        CopyException::SetColors(ColorBitmask::WHITE),
        CopyException::NotCopied(CopiedCharacteristic::Colors),
    ] {
        assert_eq!(
            abilities(std::slice::from_ref(&ex)),
            without(&colors_cda),
            "{ex:?}"
        );
    }
    for ex in [
        CopyException::SetCardTypes(set(&["Artifact"])),
        CopyException::SetSubtypes(set(&["Golem"])),
        CopyException::SetSupertypes(set(&["Legendary"])),
        CopyException::NotCopied(CopiedCharacteristic::CardTypes),
        CopyException::NotCopied(CopiedCharacteristic::Subtypes),
        CopyException::NotCopied(CopiedCharacteristic::Supertypes),
    ] {
        assert_eq!(
            abilities(std::slice::from_ref(&ex)),
            without(&types_cda),
            "{ex:?}"
        );
    }
    for ex in [
        CopyException::SetPower(literal(1)),
        CopyException::SetToughness(literal(1)),
        CopyException::NotCopied(CopiedCharacteristic::Power),
        CopyException::NotCopied(CopiedCharacteristic::Toughness),
    ] {
        assert_eq!(
            abilities(std::slice::from_ref(&ex)),
            without(&pt_cda),
            "{ex:?}"
        );
    }
    // CR 707.9d last sentence: "in addition to its other types" copies the type CDA.
    for ex in [
        CopyException::AddCardTypes(set(&["Artifact"])),
        CopyException::AddSubtypes(set(&["Golem"])),
        CopyException::AddSupertypes(set(&["Legendary"])),
        CopyException::SetNames(set(&["test.n"])),
    ] {
        assert_eq!(abilities(std::slice::from_ref(&ex)), all, "{ex:?}");
    }
    // A gained CDA-shaped ability is never dropped.
    let got = abilities(&[
        CopyException::SetColors(ColorBitmask::WHITE),
        CopyException::GainAbility(colors_cda.clone()),
    ]);
    assert_eq!(got.last(), Some(&colors_cda));
    // Abilities that are not STATIC, have no effects, or define another layer: never dropped.
    let odd = vec![
        obj(vec![("kind", js("TRIGGERED"))]),
        obj(vec![("kind", js("STATIC"))]),
        obj(vec![
            ("kind", js("STATIC")),
            (
                "effects",
                AbilityAST(vec![char_static("6", true).0, js("x").0].into()),
            ),
        ]),
    ];
    let mut o2 = creature("test.orig", 3, 3);
    o2.abilities.clone_from(&odd);
    let got = CopiableValues::plain(o2)
        .copy_with(
            &own,
            &[
                CopyException::SetColors(ColorBitmask::WHITE),
                CopyException::SetCardTypes(set(&["Artifact"])),
                CopyException::SetPower(literal(1)),
            ],
        )
        .characteristics
        .abilities;
    assert_eq!(got, odd);
}

// ---------------------------------------------------------------------------------------------
// As-enters P/T (CR 613.2a, 707.2)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2a, 707.2: an "as enters" P/T setting is a layer-1a
// modification that is not a copy: source unchanged, nothing else changes, and a later copy
// copies it; a layer-7c effect on the object is not copied.
#[test]
fn as_enters_power_toughness() {
    let mut base = creature("test.entering", 0, 0);
    base.power = Some(ValueExpressionAST(js("test.star").0));
    for v in [
        CopiableValues::plain(base.clone()),
        CopiableValues::nonmodal_back_face(base.clone(), Some(cost("{3}{R}"))),
    ] {
        let m = v.modified(&[
            CopyException::SetPower(literal(4)),
            CopyException::SetToughness(literal(5)),
        ]);
        assert_eq!(m.source, v.source);
        assert_eq!(m.mana_value(OFF), v.mana_value(OFF));
        let mut want = base.clone();
        want.power = Some(literal(4));
        want.toughness = Some(literal(5));
        assert_eq!(m.characteristics, want);
        // NotCopied in a non-copy modification keeps the object's own value.
        assert_eq!(
            v.modified(&[CopyException::NotCopied(CopiedCharacteristic::Names)]),
            v
        );
    }
    let (x, y) = (o(1), o(2));
    let env = Env::default()
        .with(x, base.clone())
        .with(y, other("test.copier"));
    let entering = CopiableValues::plain(base).modified(&[
        CopyException::SetPower(literal(4)),
        CopyException::SetToughness(literal(5)),
    ]);
    let eff = copiable_effect(
        &entering,
        Some(x),
        p(1),
        EffectOrigin::Static { granted_at: None },
        ts(2),
        objs(&[x]),
    );
    assert_eq!(eff.layer, Layer::L1a);
    assert!(!eff.is_cda);
    assert_eq!(eff.source, Some(x));
    let mut r = registry(vec![(1, eff)]);
    r.register(
        e(2),
        plain_eff(
            Layer::L7c,
            3,
            x,
            LayerModification::ModifyPt {
                power: 1,
                toughness: 1,
            },
        ),
    )
    .unwrap();
    assert_eq!(eval(&r, &env, x).power, Some(literal(5)));
    let copiable = copiable_characteristics(&r, &env, x).unwrap();
    assert_eq!(copiable.power, Some(literal(4)));
    assert_eq!(copiable.toughness, Some(literal(5)));
    let copy = CopiableValues::plain(copiable).copy_with(&other("test.copier"), &[]);
    r.register(e(3), copy_eff(&copy, y, 4)).unwrap();
    let got = eval(&r, &env, y);
    assert_eq!(got.power, Some(literal(4)));
    assert_eq!(got.toughness, Some(literal(5)));
}

// ---------------------------------------------------------------------------------------------
// Merged permanents (CR 731.2a, 702.140e, 613.2)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-002, REQ-LAYER-001; CR 731.2a, 613.2a: the merge effect is a layer-1a CopyValues
// effect stamped at the merge; the object has the top component's characteristics; a new merge
// over it (later timestamp) changes them to the new top's.
#[test]
fn merge_effect_top_component_and_new_top() {
    let (bottom_d, top_d, newtop_d) = (
        normal("bottom", "{1}{B}", (1, 1)),
        normal("top", "{2}{G}", (4, 4)),
        normal("newtop", "{4}{U}", (6, 6)),
    );
    let obj_id = o(1);
    let env = Env::default().with(obj_id, printed(&bottom_d));
    let merged =
        MergedPermanent::new(vec![component(&top_d), component(&bottom_d)], false).unwrap();
    let eff = merge_effect(&merged, vocab(), obj_id, p(1), ts(10)).unwrap();
    assert_eq!(eff.layer, Layer::L1a);
    assert_eq!(eff.timestamp, ts(10));
    assert_eq!(eff.origin, EffectOrigin::Resolved);
    assert_eq!(eff.affected, Affected::Objects(objs(&[obj_id])));
    assert_eq!(eff.controller, p(1));
    assert!(!eff.is_cda);
    assert_eq!(
        eff.modification,
        LayerModification::CopyValues(Box::new(printed(&top_d)))
    );
    let mut r = registry(vec![(1, eff)]);
    assert_eq!(eval(&r, &env, obj_id), printed(&top_d));
    // A new component merged over it.
    let merged2 = merged.merge(component(&newtop_d), MergeSide::Over);
    r.register(
        e(2),
        merge_effect(&merged2, vocab(), obj_id, p(1), ts(20)).unwrap(),
    )
    .unwrap();
    assert_eq!(eval(&r, &env, obj_id), printed(&newtop_d));
    // The caller may remove the old effect: same result.
    assert!(r.remove(e(1)));
    assert_eq!(eval(&r, &env, obj_id), printed(&newtop_d));
    // A component with an unsupported layout: the layout error is returned.
    let mut bad = normal("bad", "{1}", (1, 1));
    bad.layout = LayoutBehavior::Token;
    let m = MergedPermanent::new(vec![component(&bad), component(&bottom_d)], false).unwrap();
    assert!(merge_effect(&m, vocab(), obj_id, p(1), ts(30)).is_err());
    assert!(CopiableValues::merged(&m, vocab()).is_err());
}

// REQ-OBJ-002; CR 731.2a, 702.140e: merging under changes nothing for a plain merge; a mutated
// permanent gains the new component's abilities.
#[test]
fn merge_under_mutated_gains_abilities() {
    let (bottom_d, top_d, under_d) = (
        normal("bottom", "{1}{B}", (1, 1)),
        normal("top", "{2}{G}", (4, 4)),
        normal("under", "{4}{U}", (6, 6)),
    );
    let obj_id = o(1);
    let env = Env::default().with(obj_id, printed(&bottom_d));
    for mutated in [false, true] {
        let merged =
            MergedPermanent::new(vec![component(&top_d), component(&bottom_d)], mutated).unwrap();
        let mut r = registry(vec![(
            1,
            merge_effect(&merged, vocab(), obj_id, p(1), ts(10)).unwrap(),
        )]);
        let before = eval(&r, &env, obj_id);
        let merged2 = merged.merge(component(&under_d), MergeSide::Under);
        r.register(
            e(2),
            merge_effect(&merged2, vocab(), obj_id, p(1), ts(20)).unwrap(),
        )
        .unwrap();
        let after = eval(&r, &env, obj_id);
        if mutated {
            let mut want = before.clone();
            want.abilities.push(ability("under"));
            assert_eq!(after, want);
            assert_eq!(
                after.abilities,
                vec![ability("top"), ability("bottom"), ability("under")]
            );
        } else {
            assert_eq!(after, before);
            assert_eq!(after, printed(&top_d));
        }
    }
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2, 613.7, 731.2a: merge and copy effects share layer 1a
// and order by timestamp: merge before copy, the copy wins; copy before merge, the merge wins.
// Both EffectId assignments.
#[test]
fn merge_and_copy_effects_order_by_timestamp() {
    let (bottom_d, top_d) = (
        normal("bottom", "{1}{B}", (1, 1)),
        normal("top", "{2}{G}", (4, 4)),
    );
    let obj_id = o(1);
    let env = Env::default().with(obj_id, printed(&bottom_d));
    let merged =
        MergedPermanent::new(vec![component(&top_d), component(&bottom_d)], false).unwrap();
    let copied = creature("test.copied", 2, 9);
    let copy_values = CopiableValues::plain(copied.clone()).copy_with(&printed(&bottom_d), &[]);
    for (merge_ts, copy_ts, want) in [(10, 20, copied.clone()), (20, 10, printed(&top_d))] {
        for (merge_id, copy_id) in [(1, 2), (2, 1)] {
            let r = registry(vec![
                (
                    merge_id,
                    merge_effect(&merged, vocab(), obj_id, p(1), ts(merge_ts)).unwrap(),
                ),
                (copy_id, copy_eff(&copy_values, obj_id, copy_ts)),
            ]);
            assert_eq!(eval(&r, &env, obj_id), want);
            assert_eq!(copiable_characteristics(&r, &env, obj_id).unwrap(), want);
        }
    }
}

// REQ-OBJ-002; CR 731.2a, 702.140e, 613.2c, docs/DECISIONS.md "#38 FP-037 — Copy of a merged
// or mutated permanent": a copy of a mutated permanent has the top component's characteristics
// plus every component's abilities.
#[test]
fn copy_of_mutated_permanent() {
    let (bottom_d, top_d) = (
        normal("bottom", "{1}{B}", (1, 1)),
        normal("top", "{2}{G}", (4, 4)),
    );
    let (m, y) = (o(1), o(2));
    let env = Env::default()
        .with(m, printed(&bottom_d))
        .with(y, other("test.copier"));
    let merged = MergedPermanent::new(vec![component(&top_d), component(&bottom_d)], true).unwrap();
    let mut r = registry(vec![(
        1,
        merge_effect(&merged, vocab(), m, p(1), ts(10)).unwrap(),
    )]);
    let values = CopiableValues::plain(copiable_characteristics(&r, &env, m).unwrap());
    r.register(
        e(2),
        copy_eff(&values.copy_with(&other("test.copier"), &[]), y, 11),
    )
    .unwrap();
    let got = eval(&r, &env, y);
    let mut want = printed(&top_d);
    want.abilities = vec![ability("top"), ability("bottom")];
    assert_eq!(got, want);
    // Same through CopiableValues::merged directly.
    let direct = CopiableValues::merged(&merged, vocab()).unwrap();
    assert_eq!(direct.characteristics, want);
    assert_eq!(direct.source, ValueSource::Plain);
    assert_eq!(direct.mana_value(OFF), 3);
    assert_eq!(direct.copy_with(&other("test.c"), &[]).mana_value(OFF), 3);
}

// ---------------------------------------------------------------------------------------------
// Mana value (CR 202.3a-c, 202.3e, 713.5c)
// ---------------------------------------------------------------------------------------------

fn on_stack(x: u32) -> ManaValueContext {
    ManaValueContext::OnStack(VariableValues { x, y: 0, z: 0 })
}

/// The back-face-up battlefield query.
fn back_up() -> LayoutQuery {
    LayoutQuery::new(ZoneKind::Battlefield).with_status(LayoutStatus {
        face_up: FaceUp::Back,
        ..LayoutStatus::default()
    })
}

// REQ-OBJ-002; CR 202.3b: the back face of a nonmodal DFC has the front cost's mana value; a
// copy of it, and a copy of that copy, have 0, even with a mana cost set by an exception.
#[test]
fn mana_value_copy_of_nonmodal_back_face() {
    let d = nonmodal_dfc();
    let back = base_characteristics(&d, &back_up(), vocab()).unwrap();
    let v = CopiableValues::nonmodal_back_face(back.clone(), Some(cost("{3}{R}")));
    assert_eq!(v.source, ValueSource::Fixed { mana_value: 4 });
    assert_eq!(v.mana_value(OFF), 4);
    assert_eq!(v.mana_value(on_stack(7)), 4);
    let copy = v.copy_with(&other("test.copier"), &[]);
    assert_eq!(copy.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(copy.mana_value(OFF), 0);
    assert_eq!(copy.characteristics, back);
    let copy2 = copy.copy_with(&other("test.c2"), &[]);
    assert_eq!(copy2.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(copy2.mana_value(OFF), 0);
    // CR 202.3b: the copy's mana value is 0 even with a mana cost from an exception.
    let costed = v.copy_with(
        &other("test.copier"),
        &[CopyException::SetManaCost(Some(cost("{5}")))],
    );
    assert_eq!(costed.characteristics.mana_cost, Some(cost("{5}")));
    assert_eq!(costed.mana_value(OFF), 0);
    // The original keeps its value.
    assert_eq!(v.mana_value(OFF), 4);
    // CR 202.3e: X in the front cost counts 0.
    let vx = CopiableValues::nonmodal_back_face(back, Some(cost("{X}{1}{R}")));
    assert_eq!(vx.mana_value(OFF), 2);
}

// REQ-OBJ-002; CR 202.3c, 713.5c: a melded permanent's mana value is the sum of its fronts; a
// copy and a copy of a copy have 0.
#[test]
fn mana_value_copy_of_melded() {
    let (a, b) = (meld_a(), meld_b());
    let pair = MeldedPair::new(&a, &b).unwrap();
    let v = CopiableValues::melded(&pair, vocab()).unwrap();
    assert_eq!(v.characteristics, pair.characteristics(vocab()).unwrap());
    assert_eq!(v.source, ValueSource::Fixed { mana_value: 7 });
    assert_eq!(v.mana_value(OFF), 7);
    let copy = v.copy_with(&other("test.copier"), &[]);
    assert_eq!(copy.mana_value(OFF), 0);
    assert_eq!(copy.characteristics.power, Some(literal(9)));
    let copy2 = copy.copy_with(&other("test.c2"), &[]);
    assert_eq!(copy2.mana_value(OFF), 0);
    assert_eq!(copy2.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(v.mana_value(OFF), 7);
    // An invalid combined face is reported.
    let mut bad = meld_a();
    bad.faces[1].card_types = vec!["test.not_a_type".to_owned()];
    let pair = MeldedPair::new(&bad, &b).unwrap();
    assert!(CopiableValues::melded(&pair, vocab()).is_err());
}

// REQ-OBJ-002; CR 202.3e: a plain copy's X is 0 off the stack and the chosen number on it.
#[test]
fn mana_value_plain_copy_x() {
    let mut c = creature("test.x", 1, 1);
    c.mana_cost = Some(cost("{X}{X}{G}"));
    let copy = CopiableValues::plain(c).copy_with(&other("test.copier"), &[]);
    assert_eq!(copy.source, ValueSource::Plain);
    assert_eq!(copy.mana_value(OFF), 1);
    assert_eq!(copy.mana_value(on_stack(3)), 7);
}

// REQ-OBJ-002; CR 731.2a, 202.3, 202.3b: a merged permanent's mana value is its top
// component's; with a nonmodal DFC back face on top it is Fixed and a copy has 0; with an
// ordinary top a copy has that object's mana value.
#[test]
fn mana_value_merged() {
    let dfc = nonmodal_dfc();
    let bottom_d = normal("bottom", "{1}{B}", (1, 1));
    let back_top = MergedComponent {
        query: back_up(),
        ..component(&dfc)
    };
    let merged = MergedPermanent::new(vec![back_top, component(&bottom_d)], true).unwrap();
    let v = CopiableValues::merged(&merged, vocab()).unwrap();
    assert_eq!(v.source, ValueSource::Fixed { mana_value: 4 });
    assert_eq!(v.mana_value(OFF), 4);
    assert_eq!(v.characteristics.power, Some(literal(5)));
    let copy = v.copy_with(&other("test.c"), &[]);
    assert_eq!(copy.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(copy.mana_value(OFF), 0);
    // A front-face DFC top: Plain, its own cost.
    let merged = MergedPermanent::new(vec![component(&dfc), component(&bottom_d)], false).unwrap();
    let v = CopiableValues::merged(&merged, vocab()).unwrap();
    assert_eq!(v.source, ValueSource::Plain);
    assert_eq!(v.copy_with(&other("test.c"), &[]).mana_value(OFF), 4);
    // An ordinary top component.
    let top_d = normal("top", "{X}{2}{G}", (4, 4));
    let merged =
        MergedPermanent::new(vec![component(&top_d), component(&bottom_d)], false).unwrap();
    let v = CopiableValues::merged(&merged, vocab()).unwrap();
    assert_eq!(v.source, ValueSource::Plain);
    assert_eq!(v.mana_value(OFF), 3);
    assert_eq!(v.copy_with(&other("test.c"), &[]).mana_value(OFF), 3);
}

// ---------------------------------------------------------------------------------------------
// copiable_characteristics (CR 613.2c, 707.2)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-002, REQ-LAYER-001; CR 707.2, 613.2c: counters and layer 2-7 effects are not part of
// the copiable values; a CDA power expression stays an expression; an unknown object errors.
#[test]
fn copiable_characteristics_excludes_counters_and_later_layers() {
    let x = o(1);
    let mut base = creature("test.x", 2, 2);
    base.power = Some(ValueExpressionAST(js("test.cda").0));
    let env = Env::default()
        .with(x, base.clone())
        .counter(x, "+1/+1", 2)
        .counter(x, "flying", 1);
    let r = registry(vec![
        (
            1,
            plain_eff(
                Layer::L7c,
                3,
                x,
                LayerModification::ModifyPt {
                    power: 3,
                    toughness: 3,
                },
            ),
        ),
        (
            2,
            plain_eff(
                Layer::L4,
                4,
                x,
                LayerModification::AddTypes {
                    supertypes: BTreeSet::new(),
                    card_types: set(&["Artifact"]),
                    subtypes: BTreeSet::new(),
                },
            ),
        ),
        (
            3,
            plain_eff(
                Layer::L6,
                5,
                x,
                LayerModification::AddAbility(ability("granted")),
            ),
        ),
    ]);
    let full = eval(&r, &env, x);
    assert_eq!(full.power, Some(literal(10)));
    assert!(full.card_types.contains("Artifact"));
    let copiable = copiable_characteristics(&r, &env, x).unwrap();
    assert_eq!(copiable, base);
    assert_eq!(
        copiable_characteristics(&r, &env, o(99)),
        Err(LayerError::UnknownObject(o(99)))
    );
}

// REQ-LAYER-001; CR 613.2, 613.2c: with layer-1 effects only and literal P/T on a creature,
// copiable_characteristics equals the full evaluation.
#[test]
fn copiable_characteristics_agrees_with_evaluate_on_layer_one() {
    let (x, y) = (o(1), o(2));
    let env = Env::default()
        .with(x, creature("test.x", 1, 1))
        .with(y, creature("test.y", 2, 2));
    let r = registry(vec![
        (1, copy_eff(&CopiableValues::plain(other("test.a")), x, 3)),
        (
            2,
            copy_eff(&CopiableValues::plain(creature("test.b", 3, 3)), x, 2),
        ),
        (
            3,
            plain_eff(
                Layer::L1b,
                1,
                y,
                LayerModification::FaceDown(Box::new(face_down())),
            ),
        ),
    ]);
    for id in [x, y] {
        assert_eq!(
            copiable_characteristics(&r, &env, id).unwrap(),
            eval(&r, &env, id)
        );
    }
    assert_eq!(eval(&r, &env, x).names, set(&["test.a"]));
}

// ---------------------------------------------------------------------------------------------
// Determinism and properties (REQ-CORE-001)
// ---------------------------------------------------------------------------------------------

/// A deterministic generator (the property style of the layers tests).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).unwrap()
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

/// Random characteristics drawn from small pools.
fn random_chars(rng: &mut Lcg, tag: u64) -> Characteristics {
    let costs = ["{1}", "{2}{G}", "{X}{U}", "{W}{B}"];
    let mut c = if rng.below(2) == 0 {
        creature(
            &format!("test.r{tag}"),
            i64::try_from(rng.below(9)).unwrap(),
            1,
        )
    } else {
        other(&format!("test.r{tag}"))
    };
    c.mana_cost = (rng.below(4) != 0).then(|| cost(costs[rng.below(costs.len())]));
    c.color_indicator = (rng.below(3) == 0).then_some(ColorBitmask::WHITE);
    if rng.below(3) == 0 {
        c.abilities
            .push(char_static(["4", "5", "7a"][rng.below(3)], true));
    }
    c
}

/// A random exception.
fn random_exception(rng: &mut Lcg) -> CopyException {
    match rng.below(14) {
        0 => CopyException::GainAbility(ability(&format!("g{}", rng.below(5)))),
        1 => CopyException::SetNames(set(&["test.set_name"])),
        2 => CopyException::SetManaCost(Some(cost("{3}{W}"))),
        3 => CopyException::SetColors(ColorBitmask::BLUE),
        4 => CopyException::SetSupertypes(set(&["Legendary"])),
        5 => CopyException::SetCardTypes(set(&["Artifact"])),
        6 => CopyException::SetSubtypes(set(&["Golem"])),
        7 => CopyException::AddSupertypes(set(&["Snow"])),
        8 => CopyException::AddCardTypes(set(&["Enchantment"])),
        9 => CopyException::AddSubtypes(set(&["Elf"])),
        10 => CopyException::SetPower(literal(i64::try_from(rng.below(9)).unwrap())),
        11 => CopyException::SetToughness(literal(i64::try_from(rng.below(9)).unwrap())),
        12 => CopyException::SetManaCost(None),
        _ => CopyException::NotCopied(ALL_FIELDS[rng.below(ALL_FIELDS.len())]),
    }
}

// REQ-OBJ-002, REQ-CORE-001; CR 707.2, 707.3, 202.3b: property over 300 random cases:
// copy_with(&[]) preserves every characteristic; a copy of a copy equals the first copy, its
// exceptions included, whatever the second copier's own values; copy_with is pure; the source
// follows the ValueSource rules.
#[test]
fn property_copy_of_copy_and_empty_copy() {
    let mut rng = Lcg(0x38);
    for n in 0..300 {
        let orig = random_chars(&mut rng, n);
        let own_b = random_chars(&mut rng, n + 1000);
        let own_c = random_chars(&mut rng, n + 2000);
        let v = match rng.below(3) {
            0 => CopiableValues::plain(orig.clone()),
            1 => CopiableValues::nonmodal_back_face(orig.clone(), Some(cost("{2}{R}"))),
            _ => CopiableValues::nonmodal_back_face(orig.clone(), None).copy_with(&own_b, &[]),
        };
        let empty = v.copy_with(&own_b, &[]);
        assert_eq!(empty.characteristics, orig);
        let exceptions: Vec<CopyException> = (0..rng.below(4))
            .map(|_| random_exception(&mut rng))
            .collect();
        let first = v.copy_with(&own_b, &exceptions);
        let second = first.copy_with(&own_c, &[]);
        assert_eq!(second, first);
        assert_eq!(v.copy_with(&own_b, &exceptions), first);
        let want_zero = !matches!(v.source, ValueSource::Plain);
        assert_eq!(
            first.source == ValueSource::CopyOfBackFaceOrMelded,
            want_zero
        );
    }
}

// REQ-CORE-001, REQ-LAYER-001; CR 613.2, 613.7: the same layer-1 effects registered in any
// order give the same copiable and evaluated characteristics (200 shuffles).
#[test]
fn registration_order_is_irrelevant() {
    let (x, y, z) = (o(1), o(2), o(3));
    let env = Env::default()
        .with(x, creature("test.x", 1, 1))
        .with(y, other("test.y"))
        .with(z, creature("test.z", 2, 2));
    let bottom_d = normal("bottom", "{1}{B}", (1, 1));
    let top_d = normal("top", "{2}{G}", (4, 4));
    let merged = MergedPermanent::new(vec![component(&top_d), component(&bottom_d)], true).unwrap();
    let list: Vec<(u64, ContinuousEffect)> = vec![
        (1, copy_eff(&CopiableValues::plain(other("test.a")), x, 5)),
        (
            2,
            copy_eff(&CopiableValues::plain(creature("test.b", 3, 3)), x, 5),
        ),
        (3, merge_effect(&merged, vocab(), y, p(1), ts(4)).unwrap()),
        (
            4,
            copy_eff(&CopiableValues::plain(creature("test.c", 7, 7)), y, 6),
        ),
        (
            5,
            plain_eff(
                Layer::L1b,
                1,
                z,
                LayerModification::FaceDown(Box::new(face_down())),
            ),
        ),
        (6, copy_eff(&CopiableValues::plain(other("test.d")), z, 9)),
    ];
    let reference = registry(list.clone());
    let want_eval = evaluate_all(&reference, &env).unwrap();
    let want_copiable: Vec<Characteristics> = [x, y, z]
        .iter()
        .map(|&id| copiable_characteristics(&reference, &env, id).unwrap())
        .collect();
    let mut rng = Lcg(0x5eed);
    for _ in 0..200 {
        let mut shuffled = list.clone();
        rng.shuffle(&mut shuffled);
        let r = registry(shuffled);
        assert_eq!(evaluate_all(&r, &env).unwrap(), want_eval);
        let got: Vec<Characteristics> = [x, y, z]
            .iter()
            .map(|&id| copiable_characteristics(&r, &env, id).unwrap())
            .collect();
        assert_eq!(got, want_copiable);
    }
}
