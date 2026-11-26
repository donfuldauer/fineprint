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
    test_ability(&format!("test.ability.{tag}"))
}

/// A static ability with one `CHAR` effect in `layer`, characteristic-defining when `cda`.
fn char_static(layer: &str, cda: bool) -> AbilityAST {
    char_static_defines(layer, cda, None)
}

/// A static ability `test.static.<tag>` with no effects (hand-built: the decoder requires at
/// least one effect).
fn empty_static(tag: &str) -> AbilityAST {
    AbilityAST::Static(fineprint_dsl::ability::StaticAbilityAST {
        base: fineprint_dsl::ability::AbilityBase::new(format!("test.static.{tag}")),
        effects: Vec::new(),
    })
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
        rules_text: literal_text(&format!("test.text.{name}")),
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
        rules_text: literal_text(&format!("test.text.{name}")),
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
        rules_text: TextTokenStream::default(),
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
/// non-literal expression. The generating ability of every effect exists except those in
/// `lost` (its source lost the ability). The base mana-value provenance is plain except for
/// the objects in `base_provenance` (round 3).
#[derive(Default, Clone)]
struct Env {
    bases: BTreeMap<ObjectId, ObjectBase>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    lost: BTreeSet<EffectId>,
    base_provenance: BTreeMap<ObjectId, ManaValueProvenance>,
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

    /// The base of `id` is `values`: its characteristics and its base mana-value provenance.
    fn with_values(mut self, id: ObjectId, values: &CopiableValues) -> Env {
        self.base_provenance.insert(id, values.provenance());
        self.with(id, values.characteristics.clone())
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

    fn base_mana_value_provenance(&self, id: ObjectId) -> ManaValueProvenance {
        self.base_provenance.get(&id).cloned().unwrap_or_default()
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

    fn generating_ability_exists(
        &self,
        effect: EffectId,
        _: ObjectId,
        _: Option<&Evaluated>,
    ) -> bool {
        !self.lost.contains(&effect)
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
        power: pt.map(|(p, _)| ValueExpressionAST::Literal(p)),
        toughness: pt.map(|(_, t)| ValueExpressionAST::Literal(t)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: literal_text(&format!("test.text.{tag}")),
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
    let cda = ValueExpressionAST::Memory("test.cda_count".to_owned());
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
    // Abilities that are not STATIC, have no effects, have no CHAR effect, or define another
    // layer: never dropped.
    let triggered: AbilityAST = serde_json::from_value(serde_json::json!({
        "abilityId": "test.odd.triggered",
        "kind": "TRIGGERED",
        "trigger": {"mode": "STATE", "state": {"pred": "IS_CREATURE"}},
        "effect": [],
    }))
    .unwrap();
    let odd = vec![
        triggered,
        empty_static("odd"),
        test_ability("test.odd.rule"),
        char_static("6", true),
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

/// A static ability `test.char.<layer>` with one `CHAR` effect in `layer` (its wire name),
/// characteristic-defining when `cda`, whose modification carries `defines` when given
/// (REQ-OBJ-002; SRS §14.4.3). Hand-built, so it can also hold a malformed `defines` that the
/// decoder would refuse.
fn char_static_defines(layer: &str, cda: bool, defines: Option<serde_json::Value>) -> AbilityAST {
    let mut modification = fineprint_dsl::ability::Modification::new("TEST_OP");
    if let Some(d) = defines {
        modification.params.insert("defines".to_owned(), d);
    }
    let effect = fineprint_dsl::ability::CharacteristicEffect {
        layer: Layer::from_wire(layer).unwrap(),
        affected: fineprint_dsl::ability::Affected::Selector(ObjectSelectorAST::Ref(
            fineprint_dsl::expr::ReferenceAST::SelfRef,
        )),
        modification,
        is_cda: Some(cda),
    };
    AbilityAST::Static(fineprint_dsl::ability::StaticAbilityAST {
        base: fineprint_dsl::ability::AbilityBase::new(format!("test.char.{layer}")),
        effects: vec![fineprint_dsl::ability::StaticEffect::Char(effect)],
    })
}

/// A layer-4 CDA whose `defines` lists `parts`.
fn type_cda(parts: &[&str]) -> AbilityAST {
    char_static_defines("4", true, Some(serde_json::json!(parts)))
}

/// The six type-line exceptions that set or do not copy one part, with the wire name of the
/// part each touches (CR 707.9b, 707.9c, 707.9d).
fn type_line_exceptions() -> Vec<(CopyException, &'static str)> {
    vec![
        (
            CopyException::SetSupertypes(set(&["Legendary"])),
            "SUPERTYPES",
        ),
        (
            CopyException::NotCopied(CopiedCharacteristic::Supertypes),
            "SUPERTYPES",
        ),
        (
            CopyException::SetCardTypes(set(&["Artifact"])),
            "CARD_TYPES",
        ),
        (
            CopyException::NotCopied(CopiedCharacteristic::CardTypes),
            "CARD_TYPES",
        ),
        (CopyException::SetSubtypes(set(&["Golem"])), "SUBTYPES"),
        (
            CopyException::NotCopied(CopiedCharacteristic::Subtypes),
            "SUBTYPES",
        ),
    ]
}

/// The "in addition to its other types" exceptions and an unrelated one (CR 707.9d last
/// sentence).
fn keeping_exceptions() -> Vec<CopyException> {
    vec![
        CopyException::AddSupertypes(set(&["Legendary"])),
        CopyException::AddCardTypes(set(&["Artifact"])),
        CopyException::AddSubtypes(set(&["Golem"])),
        CopyException::SetNames(set(&["test.n"])),
        CopyException::GainAbility(ability("gained.other")),
    ]
}

/// Whether `cda` is still among the abilities of a copy of a creature carrying it (after
/// [`ability`]`("plain")`) under `exceptions`; the plain ability always stays.
fn cda_kept(cda: &AbilityAST, exceptions: &[CopyException]) -> bool {
    let mut orig = creature("test.orig", 3, 3);
    orig.abilities = vec![ability("plain"), cda.clone()];
    let got = CopiableValues::plain(orig)
        .copy_with(&other("test.own"), exceptions)
        .characteristics
        .abilities;
    assert_eq!(got.first(), Some(&ability("plain")), "{exceptions:?}");
    got.iter().skip(1).any(|a| a == cda)
}

// REQ-OBJ-002, REQ-DSL-008; CR 707.9d, 109.3, 205.1b: a single-part layer-4 CDA is not copied
// exactly when an exception sets or does not copy its own part; a subtype-only CDA survives
// SetSupertypes, SetCardTypes, NotCopied(Supertypes) and NotCopied(CardTypes); likewise for
// supertype-only and card-type-only CDAs.
#[test]
fn single_part_type_cda_dropped_only_by_its_part() {
    for part in ["SUPERTYPES", "CARD_TYPES", "SUBTYPES"] {
        let cda = type_cda(&[part]);
        for (ex, touched) in type_line_exceptions() {
            assert_eq!(
                cda_kept(&cda, std::slice::from_ref(&ex)),
                touched != part,
                "{part} CDA under {ex:?}"
            );
        }
    }
}

// REQ-OBJ-002; CR 707.9d: the acceptance criterion: a subtype-only layer-4 CDA survives a
// SetSupertypes exception, and the supertypes are still set.
#[test]
fn subtype_only_cda_survives_set_supertypes() {
    let cda = type_cda(&["SUBTYPES"]);
    let mut orig = creature("test.orig", 3, 3);
    orig.abilities = vec![cda.clone()];
    let c = CopiableValues::plain(orig)
        .copy_with(
            &other("test.own"),
            &[CopyException::SetSupertypes(set(&["Legendary"]))],
        )
        .characteristics;
    assert_eq!(c.abilities, vec![cda]);
    assert_eq!(c.supertypes, set(&["Legendary"]));
}

// REQ-OBJ-002; CR 707.9d (judgement call, docs/DECISIONS.md "#198 FP-151 — Per-part CDA targets
// for copy exceptions"): a multi-part CDA is not copied as a whole as soon as any of its parts
// is touched, and is copied when none is.
#[test]
fn multi_part_type_cda_dropped_whole_when_any_part_touched() {
    let cda = type_cda(&["CARD_TYPES", "SUBTYPES"]);
    for (ex, touched) in type_line_exceptions() {
        assert_eq!(
            cda_kept(&cda, std::slice::from_ref(&ex)),
            touched == "SUPERTYPES",
            "{ex:?}"
        );
    }
    let two = type_cda(&["SUPERTYPES", "SUBTYPES"]);
    for (ex, touched) in type_line_exceptions() {
        assert_eq!(
            cda_kept(&two, std::slice::from_ref(&ex)),
            touched == "CARD_TYPES",
            "{ex:?}"
        );
    }
    // Two exceptions, neither touching the CDA's parts: kept.
    assert!(cda_kept(
        &type_cda(&["SUBTYPES"]),
        &[
            CopyException::SetSupertypes(set(&["Legendary"])),
            CopyException::NotCopied(CopiedCharacteristic::CardTypes),
        ]
    ));
}

// REQ-OBJ-002; CR 707.9d: a layer-4 CDA without `defines`, or listing all three parts, defines
// the whole type line and is dropped by any type-line exception (the #38 FP-037 behaviour).
#[test]
fn type_cda_without_defines_dropped_by_any_type_exception() {
    for cda in [
        char_static("4", true),
        char_static_defines("4", true, None),
        type_cda(&["SUBTYPES", "SUPERTYPES", "CARD_TYPES"]),
    ] {
        for (ex, _) in type_line_exceptions() {
            assert!(!cda_kept(&cda, std::slice::from_ref(&ex)), "{cda:?} {ex:?}");
        }
    }
}

// REQ-OBJ-002, REQ-NFR-006; CR 707.9d: a malformed `defines` (which the DSL decode refuses)
// never panics in core and counts as all three parts.
#[test]
fn malformed_defines_counts_as_all_parts() {
    for bad in [
        serde_json::json!([]),
        serde_json::json!("SUBTYPES"),
        serde_json::json!(["subtypes"]),
        serde_json::json!(["SUBTYPE"]),
        serde_json::json!([1]),
        serde_json::json!([null]),
        serde_json::json!(["SUBTYPES", "SUBTYPES"]),
        serde_json::json!(["SUBTYPES", "x"]),
        serde_json::json!({"SUBTYPES": true}),
        serde_json::json!(null),
    ] {
        let cda = char_static_defines("4", true, Some(bad.clone()));
        for (ex, _) in type_line_exceptions() {
            assert!(!cda_kept(&cda, std::slice::from_ref(&ex)), "{bad} {ex:?}");
        }
        for ex in keeping_exceptions() {
            assert!(cda_kept(&cda, std::slice::from_ref(&ex)), "{bad} {ex:?}");
        }
    }
    // A modification that is not an object cannot be held by a typed ability any more (#232):
    // the decoder refuses it.
    assert!(
        serde_json::from_value::<AbilityAST>(serde_json::json!({
            "abilityId": "test.odd",
            "kind": "STATIC",
            "effects": [{"type": "CHAR", "layer": "4", "modification": 3, "isCDA": true}],
        }))
        .is_err()
    );
}

// REQ-OBJ-002; CR 707.9d last sentence: "in addition to its other types" exceptions (and
// exceptions that define no CDA characteristic) keep every type CDA, whatever it defines.
#[test]
fn add_type_exceptions_keep_every_type_cda() {
    for cda in [
        char_static("4", true),
        type_cda(&["SUPERTYPES"]),
        type_cda(&["CARD_TYPES"]),
        type_cda(&["SUBTYPES"]),
        type_cda(&["CARD_TYPES", "SUBTYPES"]),
    ] {
        for ex in keeping_exceptions() {
            assert!(cda_kept(&cda, std::slice::from_ref(&ex)), "{cda:?} {ex:?}");
        }
        assert!(cda_kept(&cda, &keeping_exceptions()), "{cda:?}");
    }
}

// REQ-OBJ-002; CR 707.9d: color and power/toughness CDAs are unaffected by type-line
// exceptions (a `defines` param there is opaque and ignored), and still dropped by their own.
#[test]
fn color_and_pt_cdas_ignore_type_exceptions_and_defines() {
    for (layer, own) in [
        ("5", CopyException::SetColors(ColorBitmask::WHITE)),
        ("7a", CopyException::SetPower(literal(1))),
    ] {
        for cda in [
            char_static(layer, true),
            char_static_defines(layer, true, Some(serde_json::json!(["SUBTYPES"]))),
        ] {
            for (ex, _) in type_line_exceptions() {
                assert!(cda_kept(&cda, std::slice::from_ref(&ex)), "{cda:?} {ex:?}");
            }
            assert!(!cda_kept(&cda, std::slice::from_ref(&own)), "{cda:?}");
        }
    }
}

// REQ-OBJ-002; CR 707.9d: a layer-4 CHAR effect that is not a CDA is never dropped, nor is a
// gained type CDA (CR 707.9a: part of the copy's copiable values), nor does a modification
// that is not a copy drop anything (CR 613.2a, 707.2).
#[test]
fn non_cda_and_gained_type_abilities_never_dropped() {
    for not_cda in [
        char_static_defines("4", false, Some(serde_json::json!(["SUBTYPES"]))),
        char_static_defines("4", false, None),
    ] {
        for (ex, _) in type_line_exceptions() {
            assert!(cda_kept(&not_cda, std::slice::from_ref(&ex)), "{ex:?}");
        }
    }
    let cda = type_cda(&["SUBTYPES"]);
    for (ex, _) in type_line_exceptions() {
        let mut orig = creature("test.orig", 3, 3);
        orig.abilities = vec![ability("plain")];
        let got = CopiableValues::plain(orig)
            .copy_with(
                &other("test.own"),
                &[ex.clone(), CopyException::GainAbility(cda.clone())],
            )
            .characteristics
            .abilities;
        assert_eq!(got, vec![ability("plain"), cda.clone()], "{ex:?}");
        let mut orig = creature("test.orig", 3, 3);
        orig.abilities = vec![cda.clone()];
        let got = CopiableValues::plain(orig)
            .modified(std::slice::from_ref(&ex))
            .characteristics
            .abilities;
        assert_eq!(got, vec![cda.clone()], "modified {ex:?}");
    }
}

// REQ-OBJ-002; CR 707.9d: an ability with a subtype-defining layer-4 CDA effect and a colors
// CDA effect is dropped (whole) by an exception touching either, and kept by SetSupertypes.
#[test]
fn ability_with_several_cda_effects_uses_their_union() {
    let both: AbilityAST = serde_json::from_value(serde_json::json!({
        "abilityId": "test.both",
        "kind": "STATIC",
        "effects": [
            {"type": "CHAR", "layer": "4", "affected": {"ref": "SELF"},
             "modification": {"op": "TEST_OP", "defines": ["SUBTYPES"]}, "isCDA": true},
            {"type": "CHAR", "layer": "5", "affected": {"ref": "SELF"},
             "modification": {"op": "TEST_OP"}, "isCDA": true},
        ],
    }))
    .unwrap();
    assert!(cda_kept(
        &both,
        &[CopyException::SetSupertypes(set(&["Legendary"]))]
    ));
    assert!(!cda_kept(
        &both,
        &[CopyException::SetSubtypes(set(&["Golem"]))]
    ));
    assert!(!cda_kept(
        &both,
        &[CopyException::SetColors(ColorBitmask::WHITE)]
    ));
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
    base.power = Some(ValueExpressionAST::Memory("test.star".to_owned()));
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
    base.power = Some(ValueExpressionAST::Memory("test.cda".to_owned()));
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
        // The provenance-carrying copy and the P/T-only modification (CodeRabbit round 1).
        (7, copy_eff(&back_face_copy_costed(), y, 6)),
        (8, pt_mod(x, 6, 4, 5)),
        (9, pt_mod(y, 6, 2, 3)),
        // Pinned colors and an ignored NotCopied (CodeRabbit round 2).
        (
            10,
            mod_eff(
                &[
                    CopyException::SetColors(ColorBitmask::WHITE),
                    CopyException::NotCopied(CopiedCharacteristic::Colors),
                ],
                x,
                10,
            ),
        ),
        (
            11,
            mod_eff(&[CopyException::SetManaCost(Some(cost("{1}{B}")))], x, 11),
        ),
    ];
    let reference = registry(list.clone());
    let want_eval = evaluate_all(&reference, &env).unwrap();
    assert!(want_eval[&x].mana_value_provenance.colors_pinned());
    assert_eq!(want_eval[&x].characteristics.colors, ColorBitmask::WHITE);
    let want_copiable: Vec<Characteristics> = [x, y, z]
        .iter()
        .map(|&id| copiable_characteristics(&reference, &env, id).unwrap())
        .collect();
    let want_values: Vec<CopiableValues> = [x, y, z]
        .iter()
        .map(|&id| copiable_values(&reference, &env, id).unwrap())
        .collect();
    assert_eq!(want_eval[&y].mana_value(OFF), 0);
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
        let got: Vec<CopiableValues> = [x, y, z]
            .iter()
            .map(|&id| copiable_values(&r, &env, id).unwrap())
            .collect();
        assert_eq!(got, want_values);
    }
}

// ---------------------------------------------------------------------------------------------
// CodeRabbit round 1 (PR #197): pinned colors, back-face merged tops, mana-value provenance in
// layer 1a and `Evaluated`, P/T-only layer-1a modifications
// ---------------------------------------------------------------------------------------------

/// A copy of a green `{2}{G}` 3/3 by `own` with `exceptions`.
fn copy_n(own: &Characteristics, exceptions: &[CopyException]) -> Characteristics {
    CopiableValues::plain(creature("test.orig", 3, 3))
        .copy_with(own, exceptions)
        .characteristics
}

fn colors(a: ColorBitmask, b: ColorBitmask) -> ColorBitmask {
    ColorBitmask::new(a.bits() | b.bits()).unwrap()
}

// REQ-OBJ-002; CR 707.9b, 707.9c, 707.9d, docs/DECISIONS.md "#38 FP-037 — CodeRabbit round 1:
// colors pinned by SetColors and NotCopied(Colors)": colors an exception set or kept are not
// re-derived by a later SetManaCost of the same effect; a cost-only exception still re-derives
// cost-derived colors.
#[test]
fn exception_set_mana_cost_respects_pinned_colors() {
    let own = other("test.own");
    let set_w = CopyException::SetManaCost(Some(cost("{1}{W}")));
    // Explicit green, then a white cost: green stays.
    let c = copy_n(
        &own,
        &[CopyException::SetColors(ColorBitmask::GREEN), set_w.clone()],
    );
    assert_eq!(c.colors, ColorBitmask::GREEN);
    assert_eq!(c.mana_cost, Some(cost("{1}{W}")));
    // Cost only: the cost-derived colors follow it.
    let c = copy_n(&own, std::slice::from_ref(&set_w));
    assert_eq!(c.colors, ColorBitmask::WHITE);
    let c = copy_n(&own, &[CopyException::SetManaCost(None)]);
    assert_eq!(c.colors, ColorBitmask::NONE);
    // Cost then explicit colors: the explicit colors win.
    let c = copy_n(
        &own,
        &[set_w.clone(), CopyException::SetColors(ColorBitmask::GREEN)],
    );
    assert_eq!(c.colors, ColorBitmask::GREEN);
    assert_eq!(c.mana_cost, Some(cost("{1}{W}")));
    // Two costs: each re-derives.
    let c = copy_n(
        &own,
        &[
            set_w.clone(),
            CopyException::SetManaCost(Some(cost("{1}{B}"))),
        ],
    );
    assert_eq!(c.colors, ColorBitmask::BLACK);
    assert_eq!(c.mana_cost, Some(cost("{1}{B}")));
    // Kept own colors (green, equal to the copied cost's derivation) are pinned.
    let mut green_own = other("test.green_own");
    green_own.colors = ColorBitmask::GREEN;
    green_own.color_indicator = None;
    let not_colors = CopyException::NotCopied(CopiedCharacteristic::Colors);
    let c = copy_n(&green_own, &[not_colors.clone(), set_w.clone()]);
    assert_eq!(c.colors, ColorBitmask::GREEN);
    assert_eq!(c.color_indicator, None);
    assert_eq!(c.mana_cost, Some(cost("{1}{W}")));
    // Kept own colors that are not white (blue and red with a red indicator) stay too.
    let c = copy_n(&own, &[not_colors, set_w.clone()]);
    assert_eq!(c.colors, own.colors);
    assert_eq!(c.color_indicator, own.color_indicator);
    // A color indicator: the derivation includes it, as before.
    let mut orig = creature("test.orig", 3, 3);
    orig.color_indicator = Some(ColorBitmask::BLUE);
    orig.colors = colors(ColorBitmask::BLUE, ColorBitmask::GREEN);
    let c = CopiableValues::plain(orig)
        .copy_with(&own, std::slice::from_ref(&set_w))
        .characteristics;
    assert_eq!(c.colors, colors(ColorBitmask::BLUE, ColorBitmask::WHITE));
    assert_eq!(c.color_indicator, Some(ColorBitmask::BLUE));
}

/// A nonmodal DFC whose front costs `{0}` and whose back has no mana cost.
fn nonmodal_dfc_zero_front() -> CardDefinitionDSL {
    def(
        "test.dfc_zero",
        LayoutBehavior::DfcNonmodal,
        vec![
            face(FaceKind::Main, "dfc0_front", Some("{0}"), Some((1, 1))),
            face(FaceKind::Back, "dfc0_back", None, Some((2, 2))),
        ],
    )
}

// REQ-OBJ-002; CR 731.2a, 202.3a, 202.3b, docs/DECISIONS.md "#38 FP-037 — CodeRabbit round 1:
// a back-face top of a merged permanent is always Fixed": a back-face-up top whose front costs
// {0} is Fixed { 0 } even though the numbers match, and its copy has mana value 0 even with a
// set cost; an ordinary {0} top and a front-face top stay Plain.
#[test]
fn mana_value_merged_back_face_top_zero_cost() {
    let dfc = nonmodal_dfc_zero_front();
    let bottom_d = normal("bottom", "{1}{B}", (1, 1));
    let five = [CopyException::SetManaCost(Some(cost("{5}")))];
    let back_top = MergedComponent {
        query: back_up(),
        ..component(&dfc)
    };
    let merged = MergedPermanent::new(vec![back_top, component(&bottom_d)], false).unwrap();
    let v = CopiableValues::merged(&merged, vocab()).unwrap();
    assert_eq!(v.source, ValueSource::Fixed { mana_value: 0 });
    assert_eq!(v.mana_value(OFF), 0);
    let copy = v.copy_with(&other("test.c"), &five);
    assert_eq!(copy.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(copy.characteristics.mana_cost, Some(cost("{5}")));
    assert_eq!(copy.mana_value(OFF), 0);
    // An ordinary {0} top component: Plain, and its copy takes the set cost.
    let zero_d = normal("zero", "{0}", (1, 1));
    let merged =
        MergedPermanent::new(vec![component(&zero_d), component(&bottom_d)], false).unwrap();
    let v = CopiableValues::merged(&merged, vocab()).unwrap();
    assert_eq!(v.source, ValueSource::Plain);
    let copy = v.copy_with(&other("test.c"), &five);
    assert_eq!(copy.source, ValueSource::Plain);
    assert_eq!(copy.mana_value(OFF), 5);
    // The same DFC front face up: values match, Plain.
    let merged = MergedPermanent::new(vec![component(&dfc), component(&bottom_d)], false).unwrap();
    let v = CopiableValues::merged(&merged, vocab()).unwrap();
    assert_eq!(v.source, ValueSource::Plain);
    assert_eq!(v.copy_with(&other("test.c"), &five).mana_value(OFF), 5);
}

/// The back face of [`nonmodal_dfc`] (front `{3}{R}`) as copiable values: `Fixed { 4 }`.
fn back_face_values() -> CopiableValues {
    let back = base_characteristics(&nonmodal_dfc(), &back_up(), vocab()).unwrap();
    CopiableValues::nonmodal_back_face(back, Some(cost("{3}{R}")))
}

/// A copy of [`back_face_values`] with `SetManaCost({5})`: cost {5}, mana value 0.
fn back_face_copy_costed() -> CopiableValues {
    back_face_values().copy_with(
        &other("test.copier"),
        &[CopyException::SetManaCost(Some(cost("{5}")))],
    )
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2a, 202.3b: (i) a back-face copy with a set cost {5},
// registered through copiable_effect, evaluates to cost {5} and mana value 0; the effect is
// the provenance-carrying layer-1a variant; plain values still build CopyValues.
#[test]
fn evaluate_reports_back_face_copy_mana_value() {
    let y = o(2);
    let env = Env::default().with(y, other("test.copier"));
    let costed = back_face_copy_costed();
    assert_eq!(costed.source, ValueSource::CopyOfBackFaceOrMelded);
    let eff = copy_eff(&costed, y, 1);
    assert_eq!(
        eff.modification,
        LayerModification::CopyValuesWithProvenance {
            characteristics: Box::new(costed.characteristics.clone()),
            provenance: costed.provenance(),
        }
    );
    assert!(eff.modification.belongs_to(Layer::L1a));
    assert!(!eff.modification.belongs_to(Layer::L1b));
    let r = registry(vec![(1, eff)]);
    let ev = evaluate(&r, &env, y).unwrap();
    assert_eq!(ev.characteristics.mana_cost, Some(cost("{5}")));
    assert_eq!(ev.mana_value(OFF), 0);
    assert_eq!(ev.mana_value(on_stack(3)), 0);
    assert_eq!(
        ev.mana_value_provenance.source(),
        &ValueSource::CopyOfBackFaceOrMelded
    );
    assert!(ev.mana_value_provenance.copies_as_zero());
    // Plain values: CopyValues, plain provenance.
    let plain = CopiableValues::plain(creature("test.p", 1, 1));
    assert_eq!(
        copy_eff(&plain, y, 1).modification,
        LayerModification::CopyValues(Box::new(plain.characteristics.clone()))
    );
    assert!(plain.provenance().is_plain());
    assert_eq!(ManaValueProvenance::default(), ManaValueProvenance::plain());
    assert_eq!(ManaValueProvenance::plain().source(), &ValueSource::Plain);
    assert!(!ManaValueProvenance::plain().copies_as_zero());
}

// REQ-OBJ-002; CR 613.2c, 707.2, 202.3b: (ii) copiable_values returns exactly the registered
// values, mana value 0; copiable_characteristics returns their characteristics.
#[test]
fn copiable_values_round_trips_back_face_copy() {
    let y = o(2);
    let env = Env::default().with(y, other("test.copier"));
    let costed = back_face_copy_costed();
    let r = registry(vec![(1, copy_eff(&costed, y, 1))]);
    let got = copiable_values(&r, &env, y).unwrap();
    assert_eq!(got, costed);
    assert_eq!(got.mana_value(OFF), 0);
    assert_eq!(
        copiable_characteristics(&r, &env, y).unwrap(),
        costed.characteristics
    );
    assert_eq!(
        copiable_values(&r, &env, o(99)),
        Err(LayerError::UnknownObject(o(99)))
    );
}

// REQ-OBJ-002, REQ-LAYER-001; CR 202.3b, 707.2, 613.2c: (iii) the reviewer's workflow. A
// back-face copy with SetManaCost({5}) goes through copiable_effect, evaluate, copiable_values
// and copy_with: a copy of the copy, and a further copy with another set cost, still have mana
// value 0. The characteristics-only path (plain of copiable_characteristics) gives 5: it
// carries characteristics only, as documented.
#[test]
fn regression_back_face_copy_set_mana_cost_copy_of_copy() {
    let (y, z) = (o(2), o(3));
    let env = Env::default()
        .with(y, other("test.copier"))
        .with(z, creature("test.z", 1, 1));
    let costed = back_face_copy_costed();
    let mut r = registry(vec![(1, copy_eff(&costed, y, 1))]);
    assert_eq!(evaluate(&r, &env, y).unwrap().mana_value(OFF), 0);
    let read = copiable_values(&r, &env, y).unwrap();
    assert_eq!(read.mana_value(OFF), 0);
    let copy2 = read.copy_with(&creature("test.z", 1, 1), &[]);
    assert_eq!(copy2.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(copy2.mana_value(OFF), 0);
    r.register(e(2), copy_eff(&copy2, z, 2)).unwrap();
    let ev = evaluate(&r, &env, z).unwrap();
    assert_eq!(ev.mana_value(OFF), 0);
    assert_eq!(ev.characteristics.mana_cost, Some(cost("{5}")));
    let copy3 = copiable_values(&r, &env, z).unwrap().copy_with(
        &other("test.c3"),
        &[CopyException::SetManaCost(Some(cost("{7}")))],
    );
    assert_eq!(copy3.characteristics.mana_cost, Some(cost("{7}")));
    assert_eq!(copy3.mana_value(OFF), 0);
    // The characteristics-only path loses the provenance.
    let chars_only = CopiableValues::plain(copiable_characteristics(&r, &env, y).unwrap());
    assert_eq!(chars_only.mana_value(OFF), 5);
}

// REQ-OBJ-002; CR 202.3c, 713.5c, 731.2a, 202.3b: (iv) a melded value and a merged
// back-face-top value round-trip with their fixed numbers (7 and 4) and copy to 0.
#[test]
fn fixed_values_round_trip_through_layer_one() {
    let y = o(2);
    let env = Env::default().with(y, other("test.base"));
    let (a, b) = (meld_a(), meld_b());
    let pair = MeldedPair::new(&a, &b).unwrap();
    let melded = CopiableValues::melded(&pair, vocab()).unwrap();
    let r = registry(vec![(1, copy_eff(&melded, y, 1))]);
    let ev = evaluate(&r, &env, y).unwrap();
    assert_eq!(ev.mana_value(OFF), 7);
    assert_eq!(ev.mana_value(on_stack(4)), 7);
    let read = copiable_values(&r, &env, y).unwrap();
    assert_eq!(read, melded);
    assert_eq!(read.mana_value(OFF), 7);
    assert_eq!(read.copy_with(&other("test.c"), &[]).mana_value(OFF), 0);
    // A merged permanent with a nonmodal back face on top, through merge_effect.
    let dfc = nonmodal_dfc();
    let bottom_d = normal("bottom", "{1}{B}", (1, 1));
    let back_top = MergedComponent {
        query: back_up(),
        ..component(&dfc)
    };
    let merged = MergedPermanent::new(vec![back_top, component(&bottom_d)], true).unwrap();
    let r = registry(vec![(
        1,
        merge_effect(&merged, vocab(), y, p(1), ts(5)).unwrap(),
    )]);
    let ev = evaluate(&r, &env, y).unwrap();
    assert_eq!(ev.mana_value(OFF), 4);
    let read = copiable_values(&r, &env, y).unwrap();
    assert_eq!(read, CopiableValues::merged(&merged, vocab()).unwrap());
    assert_eq!(read.source, ValueSource::Fixed { mana_value: 4 });
    assert_eq!(read.copy_with(&other("test.c"), &[]).mana_value(OFF), 0);
}

// REQ-OBJ-002; CR 202.3e: (v) a plain value's mana value is its own cost's: X is 0 off the
// stack and the chosen number on it; the base object is plain too.
#[test]
fn plain_provenance_mana_value_from_own_cost() {
    let (x, y) = (o(1), o(2));
    let mut c = creature("test.x", 1, 1);
    c.mana_cost = Some(cost("{X}{X}{G}"));
    let env = Env::default()
        .with(x, c.clone())
        .with(y, other("test.copier"));
    let copy = CopiableValues::plain(c).copy_with(&other("test.copier"), &[]);
    let r = registry(vec![(1, copy_eff(&copy, y, 1))]);
    for id in [x, y] {
        let ev = evaluate(&r, &env, id).unwrap();
        assert!(ev.mana_value_provenance.is_plain());
        assert_eq!(ev.mana_value(OFF), 1);
        assert_eq!(ev.mana_value(on_stack(3)), 7);
        assert_eq!(
            copiable_values(&r, &env, id)
                .unwrap()
                .mana_value(on_stack(3)),
            7
        );
    }
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2, 613.7, 202.3b: (vi) a later plain CopyValues effect
// resets the provenance to plain; an earlier one does not. Both timestamp orders and both
// EffectId assignments.
#[test]
fn later_plain_copy_resets_provenance() {
    let y = o(2);
    let env = Env::default().with(y, other("test.copier"));
    let costed = back_face_copy_costed();
    let plain = CopiableValues::plain(creature("test.p", 2, 2));
    for (zero_ts, plain_ts) in [(1, 2), (2, 1)] {
        for (zero_id, plain_id) in [(1, 2), (2, 1)] {
            let r = registry(vec![
                (zero_id, copy_eff(&costed, y, zero_ts)),
                (plain_id, copy_eff(&plain, y, plain_ts)),
            ]);
            let ev = evaluate(&r, &env, y).unwrap();
            let read = copiable_values(&r, &env, y).unwrap();
            if plain_ts > zero_ts {
                assert!(ev.mana_value_provenance.is_plain());
                assert_eq!(ev.mana_value(OFF), 3);
                assert_eq!(read, plain);
            } else {
                assert_eq!(ev.mana_value(OFF), 0);
                assert_eq!(read, costed);
            }
        }
    }
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2b, 708.2, 202.3a: (vii) a layer-1b face-down effect over
// a zero-provenance copy gives plain provenance and mana value 0, whatever the timestamps.
#[test]
fn face_down_over_zero_copy_is_plain() {
    let y = o(2);
    let env = Env::default().with(y, other("test.copier"));
    for (copy_ts, down_ts) in [(1, 2), (2, 1)] {
        let r = registry(vec![
            (1, copy_eff(&back_face_copy_costed(), y, copy_ts)),
            (
                2,
                plain_eff(
                    Layer::L1b,
                    down_ts,
                    y,
                    LayerModification::FaceDown(Box::new(face_down())),
                ),
            ),
        ]);
        let ev = evaluate(&r, &env, y).unwrap();
        assert!(ev.mana_value_provenance.is_plain());
        assert_eq!(ev.mana_value(OFF), 0);
        assert_eq!(
            copiable_values(&r, &env, y).unwrap(),
            CopiableValues::plain(face_down())
        );
    }
}

/// Every kind of mana-value provenance over `c`: Plain, Fixed copying as 0, Fixed copying as
/// itself, a copy of a back face; then Plain, Fixed copying as 0 and a copy of a back face with
/// pinned colors.
fn provenance_kinds(c: &Characteristics) -> Vec<CopiableValues> {
    vec![
        CopiableValues::plain(c.clone()),
        CopiableValues::nonmodal_back_face(c.clone(), Some(cost("{2}{R}"))),
        CopiableValues::with_provenance(
            c.clone(),
            ManaValueProvenance {
                source: ValueSource::Fixed { mana_value: 2 },
                zero_when_copied: false,
                colors_pinned: false,
            },
        ),
        CopiableValues::nonmodal_back_face(c.clone(), None).copy_with(&other("test.k"), &[]),
        // CR 707.9b: the same sources with pinned colors (round 2).
        CopiableValues::with_provenance(
            c.clone(),
            ManaValueProvenance {
                source: ValueSource::Plain,
                zero_when_copied: false,
                colors_pinned: true,
            },
        ),
        CopiableValues::with_provenance(
            c.clone(),
            ManaValueProvenance {
                source: ValueSource::Fixed { mana_value: 2 },
                zero_when_copied: true,
                colors_pinned: true,
            },
        ),
        CopiableValues::with_provenance(
            c.clone(),
            ManaValueProvenance {
                source: ValueSource::CopyOfBackFaceOrMelded,
                zero_when_copied: false,
                colors_pinned: true,
            },
        ),
    ]
}

// REQ-OBJ-002, REQ-CORE-001; CR 613.2c, 707.2, 202.3b, 202.3c: round-trip property over 200
// random cases and every provenance kind: registering copiable_effect(V) on an object with any
// other base (and any base provenance) and reading copiable_values gives V; evaluate reports
// V's mana value; a copy of the read value has the same source as a copy of V.
#[test]
fn property_copiable_values_round_trip() {
    let mut rng = Lcg(0x197);
    let y = o(2);
    for n in 0..200 {
        let orig = random_chars(&mut rng, n);
        let base = random_chars(&mut rng, n + 1000);
        // Round 3: the base has a random provenance too; the copy replaces it.
        let base_kinds = provenance_kinds(&base);
        let env = Env::default().with_values(y, &base_kinds[rng.below(base_kinds.len())]);
        let kinds = provenance_kinds(&orig);
        let v = &kinds[rng.below(kinds.len())];
        let r = registry(vec![(1, copy_eff(v, y, 1 + rng.below(5) as u64))]);
        let read = copiable_values(&r, &env, y).unwrap();
        assert_eq!(&read, v);
        assert_eq!(read.provenance(), v.provenance());
        let ev = evaluate(&r, &env, y).unwrap();
        assert_eq!(ev.mana_value_provenance, v.provenance());
        for ctx in [OFF, on_stack(2)] {
            assert_eq!(ev.mana_value(ctx), v.mana_value(ctx));
            assert_eq!(read.mana_value(ctx), v.mana_value(ctx));
        }
        assert_eq!(
            read.copy_with(&base, &[]).source,
            v.copy_with(&base, &[]).source
        );
    }
}

// REQ-LAYER-005, REQ-CORE-001: the digest covers the provenance: equal inputs give equal
// digests, and maps that differ only in the provenance (each kind, including Fixed with and
// without the copy-as-0 flag at the same number) give different ones.
#[test]
fn digest_covers_mana_value_provenance() {
    let c = creature("test.d", 1, 1);
    let mut kinds = provenance_kinds(&c);
    kinds.push(CopiableValues::with_provenance(
        c.clone(),
        ManaValueProvenance {
            source: ValueSource::Fixed { mana_value: 2 },
            zero_when_copied: true,
            colors_pinned: false,
        },
    ));
    let maps: Vec<BTreeMap<ObjectId, Evaluated>> = kinds
        .iter()
        .map(|v| {
            BTreeMap::from([(
                o(1),
                Evaluated {
                    characteristics: c.clone(),
                    controller: Some(p(1)),
                    mana_value_provenance: v.provenance(),
                },
            )])
        })
        .collect();
    for (i, a) in maps.iter().enumerate() {
        assert_eq!(evaluated_digest(a), evaluated_digest(&a.clone()));
        for b in &maps[i + 1..] {
            assert_ne!(evaluated_digest(a), evaluated_digest(b));
        }
    }
}

// REQ-LAYER-005, REQ-CORE-001: the cache stores and returns non-plain provenance: cached and
// uncached evaluation agree (also under the full-recompute feature) for zero, fixed and plain
// provenance, before and after a registry change.
#[test]
fn cache_agrees_on_non_plain_provenance() {
    let (x, y, z) = (o(1), o(2), o(3));
    let env = Env::default()
        .with(x, creature("test.x", 1, 1))
        .with(y, other("test.y"))
        .with(z, creature("test.z", 2, 2));
    let (a, b) = (meld_a(), meld_b());
    let pair = MeldedPair::new(&a, &b).unwrap();
    let mut r = registry(vec![
        (1, copy_eff(&back_face_copy_costed(), x, 1)),
        (
            2,
            copy_eff(&CopiableValues::melded(&pair, vocab()).unwrap(), y, 2),
        ),
        (
            3,
            modification_effect(
                &[CopyException::SetPower(literal(6))],
                None,
                p(1),
                EffectOrigin::Resolved,
                ts(3),
                objs(&[x]),
            ),
        ),
    ]);
    let mut cache = LayerCache::new();
    for _ in 0..2 {
        let cached = cache.evaluate_all(&r, &env).unwrap();
        let fresh = evaluate_all(&r, &env).unwrap();
        assert_eq!(cached, fresh);
        assert_eq!(evaluated_digest(&cached), evaluated_digest(&fresh));
        assert_eq!(cached[&x].mana_value(OFF), 0);
        assert_eq!(cached[&y].mana_value(OFF), 7);
        assert!(cached[&z].mana_value_provenance.is_plain());
    }
    // A later plain copy on x: the cache sees the registry change.
    r.register(
        e(4),
        copy_eff(&CopiableValues::plain(creature("test.p", 1, 1)), x, 9),
    )
    .unwrap();
    let cached = cache.evaluate_all(&r, &env).unwrap();
    assert_eq!(cached, evaluate_all(&r, &env).unwrap());
    assert!(cached[&x].mana_value_provenance.is_plain());
    assert_eq!(cached[&x].mana_value(OFF), 3);
}

/// An "as enters" style P/T-only modification of `target` at `t`.
fn pt_mod(target: ObjectId, t: u64, power: i64, toughness: i64) -> ContinuousEffect {
    modification_effect(
        &[
            CopyException::SetPower(literal(power)),
            CopyException::SetToughness(literal(toughness)),
        ],
        Some(target),
        p(1),
        EffectOrigin::Static { granted_at: None },
        ts(t),
        objs(&[target]),
    )
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2a, 707.2, docs/DECISIONS.md "#38 FP-037 — CodeRabbit
// round 1: P/T-only layer-1a modification": a P/T-only modification over a copy changes only
// power and toughness; removing the copy restores the original name and abilities and keeps
// the set power and toughness. Both EffectId assignments.
#[test]
fn modification_effect_sets_only_its_fields() {
    let x = o(1);
    let base = creature("test.x0", 1, 1);
    let env = Env::default().with(x, base.clone());
    let copied = CopiableValues::plain(other("test.y1")).copy_with(&base, &[]);
    for (copy_id, mod_id) in [(1, 2), (2, 1)] {
        let mut r = registry(vec![
            (copy_id, copy_eff(&copied, x, 1)),
            (mod_id, pt_mod(x, 2, 4, 5)),
        ]);
        let got = eval(&r, &env, x);
        assert_eq!(got.names, set(&["test.y1"]));
        assert_eq!(got.abilities, vec![ability("test.y1")]);
        assert_eq!(got.mana_cost, Some(cost("{U}")));
        assert_eq!(
            (got.power, got.toughness),
            (Some(literal(4)), Some(literal(5)))
        );
        assert!(r.remove(e(copy_id)));
        let got = eval(&r, &env, x);
        let mut want = base.clone();
        want.power = Some(literal(4));
        want.toughness = Some(literal(5));
        assert_eq!(got, want);
        assert_eq!(got.names, set(&["test.x0"]));
        assert_eq!(got.abilities, vec![ability("test.x0")]);
    }
    // NotCopied is a no-op; a CDA ability is not dropped by a set power.
    let mut with_cda = base.clone();
    with_cda.abilities.push(char_static("7a", true));
    let env = Env::default().with(x, with_cda.clone());
    let r = registry(vec![(
        1,
        modification_effect(
            &[CopyException::NotCopied(CopiedCharacteristic::Names)],
            None,
            p(1),
            EffectOrigin::Resolved,
            ts(1),
            objs(&[x]),
        ),
    )]);
    assert_eq!(copiable_characteristics(&r, &env, x).unwrap(), with_cda);
    let r = registry(vec![(1, pt_mod(x, 1, 4, 5))]);
    assert_eq!(
        copiable_characteristics(&r, &env, x).unwrap().abilities,
        with_cda.abilities
    );
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2, 613.7: a modification before a copy is replaced by the
// copy; two modifications apply in timestamp order. Both timestamp orders and both EffectId
// assignments.
#[test]
fn modification_effect_orders_by_timestamp() {
    let x = o(1);
    let env = Env::default().with(x, creature("test.x0", 1, 1));
    let copied = CopiableValues::plain(other("test.y1")).copy_with(&creature("test.x0", 1, 1), &[]);
    for (copy_id, mod_id) in [(1, 2), (2, 1)] {
        // Modification at 0, copy at 1: the copy replaces everything.
        let r = registry(vec![
            (copy_id, copy_eff(&copied, x, 1)),
            (mod_id, pt_mod(x, 0, 4, 5)),
        ]);
        assert_eq!(eval(&r, &env, x), copied.characteristics);
    }
    for ((ta, tb), want) in [((2, 3), (6, 6)), ((3, 2), (4, 5))] {
        for (ida, idb) in [(1, 2), (2, 1)] {
            let r = registry(vec![(ida, pt_mod(x, ta, 4, 5)), (idb, pt_mod(x, tb, 6, 6))]);
            let got = eval(&r, &env, x);
            assert_eq!(
                (got.power, got.toughness),
                (Some(literal(want.0)), Some(literal(want.1)))
            );
        }
    }
}

// REQ-OBJ-002; CR 613.2a, 613.2c, 707.2, 202.3b: copiable_values after a modification reflect
// it and keep the copy's mana-value provenance; a later copy through copy_with sees the set
// power and toughness and the zero mana value.
#[test]
fn modification_effect_in_copiable_values_and_later_copy() {
    let (x, z) = (o(1), o(3));
    let env = Env::default()
        .with(x, other("test.x"))
        .with(z, creature("test.z", 1, 1));
    for (values, want_mv) in [(back_face_copy_costed(), 0), (back_face_values(), 4)] {
        let mut r = registry(vec![(1, copy_eff(&values, x, 1)), (2, pt_mod(x, 2, 4, 5))]);
        let read = copiable_values(&r, &env, x).unwrap();
        assert_eq!(read.source, values.source);
        assert_eq!(read.mana_value(OFF), want_mv);
        assert_eq!(read.characteristics.power, Some(literal(4)));
        assert_eq!(read.characteristics.toughness, Some(literal(5)));
        assert_eq!(evaluate(&r, &env, x).unwrap().mana_value(OFF), want_mv);
        let copy = read.copy_with(&creature("test.z", 1, 1), &[]);
        r.register(e(3), copy_eff(&copy, z, 3)).unwrap();
        let ev = evaluate(&r, &env, z).unwrap();
        assert_eq!(ev.characteristics.power, Some(literal(4)));
        assert_eq!(ev.characteristics.toughness, Some(literal(5)));
        assert_eq!(ev.mana_value(OFF), 0);
    }
}

// REQ-OBJ-002; CR 707.9b, 707.9d: inside one modification effect, SetColors then SetManaCost
// keeps the set colors; SetManaCost alone re-derives cost-derived colors.
#[test]
fn modification_effect_pinned_colors() {
    let x = o(1);
    let env = Env::default().with(x, creature("test.x0", 1, 1));
    let run = |exceptions: &[CopyException]| {
        let r = registry(vec![(
            1,
            modification_effect(
                exceptions,
                None,
                p(1),
                EffectOrigin::Resolved,
                ts(1),
                objs(&[x]),
            ),
        )]);
        eval(&r, &env, x)
    };
    let set_w = CopyException::SetManaCost(Some(cost("{1}{W}")));
    let got = run(&[CopyException::SetColors(ColorBitmask::GREEN), set_w.clone()]);
    assert_eq!(got.colors, ColorBitmask::GREEN);
    assert_eq!(got.mana_cost, Some(cost("{1}{W}")));
    let got = run(&[set_w]);
    assert_eq!(got.colors, ColorBitmask::WHITE);
}

// ---------------------------------------------------------------------------------------------
// CodeRabbit round 2 (PR #197): explicit colors carried with the copiable values, provenance
// export, NotCopied in non-copy modifications, the qualified round trip, Resolved-origin
// as-enters P/T settings
// ---------------------------------------------------------------------------------------------

/// A resolved layer-1a non-copy modification of `target` with `exceptions` at `t`.
fn mod_eff(exceptions: &[CopyException], target: ObjectId, t: u64) -> ContinuousEffect {
    modification_effect(
        exceptions,
        None,
        p(1),
        EffectOrigin::Resolved,
        ts(t),
        objs(&[target]),
    )
}

/// The green `{2}{G}` 3/3 source of the round-2 color cases.
fn green_source() -> CopiableValues {
    CopiableValues::plain(creature("test.orig", 3, 3))
}

fn set_white_cost() -> CopyException {
    CopyException::SetManaCost(Some(cost("{1}{W}")))
}

// REQ-OBJ-002; CR 707.9b, 707.9c, 707.9d, docs/DECISIONS.md "#38 FP-037 — CodeRabbit round 2:
// explicit colors carried with the copiable values": explicit green set by a first copy stays
// green when that copy is copied with only a white cost; cost-derived colors still follow the
// cost; colors kept by NotCopied(Colors) stay; the three round-1 single-effect cases keep their
// results and pin.
#[test]
fn round2_pin_survives_copy_of_copy() {
    let own = other("test.own");
    let first = green_source().copy_with(&own, &[CopyException::SetColors(ColorBitmask::GREEN)]);
    assert!(first.provenance().colors_pinned());
    assert!(!first.provenance().is_plain());
    let second = first.copy_with(&own, &[set_white_cost()]);
    assert_eq!(second.characteristics.colors, ColorBitmask::GREEN);
    assert_eq!(second.characteristics.mana_cost, Some(cost("{1}{W}")));
    assert!(second.provenance().colors_pinned());
    // No color exception first: the colors are cost-derived and follow the cost.
    let unpinned = green_source().copy_with(&own, &[]);
    assert!(!unpinned.provenance().colors_pinned());
    assert_eq!(unpinned, CopiableValues::plain(creature("test.orig", 3, 3)));
    let second = unpinned.copy_with(&own, &[set_white_cost()]);
    assert_eq!(second.characteristics.colors, ColorBitmask::WHITE);
    assert!(!second.provenance().colors_pinned());
    // NotCopied(Colors) first: the copier's own colors are kept by the later cost.
    let kept = green_source().copy_with(
        &own,
        &[CopyException::NotCopied(CopiedCharacteristic::Colors)],
    );
    assert!(kept.provenance().colors_pinned());
    let second = kept.copy_with(&creature("test.c", 1, 1), &[set_white_cost()]);
    assert_eq!(second.characteristics.colors, own.colors);
    assert_eq!(second.characteristics.color_indicator, own.color_indicator);
    // The round-1 cases: same colors as before; SetColors pins, two costs do not.
    for (exceptions, want, pinned) in [
        (
            vec![
                CopyException::SetColors(ColorBitmask::GREEN),
                CopyException::SetManaCost(Some(cost("{W}"))),
            ],
            ColorBitmask::GREEN,
            true,
        ),
        (
            vec![
                set_white_cost(),
                CopyException::SetColors(ColorBitmask::GREEN),
            ],
            ColorBitmask::GREEN,
            true,
        ),
        (
            vec![
                set_white_cost(),
                CopyException::SetManaCost(Some(cost("{1}{B}"))),
            ],
            ColorBitmask::BLACK,
            false,
        ),
    ] {
        let v = green_source().copy_with(&own, &exceptions);
        assert_eq!(v.characteristics.colors, want);
        assert_eq!(v.provenance().colors_pinned(), pinned);
        assert_eq!(v.source, ValueSource::Plain);
    }
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2a, 613.2c, 707.2, 707.9b: the pin survives the round trip
// copiable_effect -> copiable_values (and evaluate), so a copy of the read values with only a
// white cost stays green; a first copy without color exception gives white. The second copy
// registered on another object gives the same, in both EffectId assignments.
#[test]
fn round2_pin_round_trips_through_layer_one() {
    let (y, z) = (o(2), o(3));
    let own = other("test.own");
    let env = Env::default()
        .with(y, own.clone())
        .with(z, creature("test.z", 1, 1));
    for (first_exceptions, want) in [
        (
            vec![CopyException::SetColors(ColorBitmask::GREEN)],
            ColorBitmask::GREEN,
        ),
        (Vec::new(), ColorBitmask::WHITE),
    ] {
        let first = green_source().copy_with(&own, &first_exceptions);
        for (id_a, id_b) in [(1, 2), (2, 1)] {
            let mut r = registry(vec![(id_a, copy_eff(&first, y, 1))]);
            let read = copiable_values(&r, &env, y).unwrap();
            assert_eq!(read, first);
            let ev = evaluate(&r, &env, y).unwrap();
            assert_eq!(ev.mana_value_provenance, first.provenance());
            let second = read.copy_with(&creature("test.z", 1, 1), &[set_white_cost()]);
            assert_eq!(second.characteristics.colors, want);
            assert_eq!(second.characteristics.mana_cost, Some(cost("{1}{W}")));
            r.register(e(id_b), copy_eff(&second, z, 2)).unwrap();
            assert_eq!(eval(&r, &env, z).colors, want);
            assert_eq!(copiable_values(&r, &env, z).unwrap(), second);
        }
    }
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2a, 707.2, 707.9b: a SetColors applied by a non-copy
// modification (ApplyCopyExceptions) pins the colors in the evaluated provenance and in
// copiable_values; a later cost-only modification or copy keeps them. Without SetColors the
// cost-only modification re-derives them. Both EffectId assignments and both timestamp orders
// of the two modifications (the later one sees the earlier one's pin).
#[test]
fn round2_pin_set_by_modification_effect() {
    let x = o(1);
    let env = Env::default().with(x, creature("test.x0", 1, 1));
    let set_green = [CopyException::SetColors(ColorBitmask::GREEN)];
    let r = registry(vec![(1, mod_eff(&set_green, x, 1))]);
    let read = copiable_values(&r, &env, x).unwrap();
    assert!(read.provenance().colors_pinned());
    assert_eq!(read.source, ValueSource::Plain);
    assert!(
        evaluate(&r, &env, x)
            .unwrap()
            .mana_value_provenance
            .colors_pinned()
    );
    let copy = read.copy_with(&other("test.own"), &[set_white_cost()]);
    assert_eq!(copy.characteristics.colors, ColorBitmask::GREEN);
    for (id_a, id_b) in [(1, 2), (2, 1)] {
        // SetColors at 1, cost at 2: green stays.
        let r = registry(vec![
            (id_a, mod_eff(&set_green, x, 1)),
            (id_b, mod_eff(&[set_white_cost()], x, 2)),
        ]);
        let got = evaluate(&r, &env, x).unwrap();
        assert_eq!(got.characteristics.colors, ColorBitmask::GREEN);
        assert_eq!(got.characteristics.mana_cost, Some(cost("{1}{W}")));
        assert!(got.mana_value_provenance.colors_pinned());
        // Cost at 1, SetColors at 2: green by the later SetColors.
        let r = registry(vec![
            (id_a, mod_eff(&set_green, x, 2)),
            (id_b, mod_eff(&[set_white_cost()], x, 1)),
        ]);
        assert_eq!(eval(&r, &env, x).colors, ColorBitmask::GREEN);
    }
    // Cost only: re-derived, unpinned.
    let r = registry(vec![(1, mod_eff(&[set_white_cost()], x, 1))]);
    let got = evaluate(&r, &env, x).unwrap();
    assert_eq!(got.characteristics.colors, ColorBitmask::WHITE);
    assert!(got.mana_value_provenance.is_plain());
    // A pinned modification over a provenance-carrying copy keeps its source and zero flag.
    let r = registry(vec![
        (1, copy_eff(&back_face_copy_costed(), x, 1)),
        (2, mod_eff(&set_green, x, 2)),
    ]);
    let read = copiable_values(&r, &env, x).unwrap();
    assert_eq!(read.source, ValueSource::CopyOfBackFaceOrMelded);
    assert!(read.provenance().copies_as_zero());
    assert!(read.provenance().colors_pinned());
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2a, 613.2b, 708.2: a later plain copy or a face-down
// effect resets the pin (plain provenance), so a later cost-only copy re-derives the colors.
#[test]
fn round2_pin_reset_by_plain_copy_and_face_down() {
    let y = o(2);
    let env = Env::default().with(y, other("test.copier"));
    let pinned = green_source().copy_with(
        &other("test.copier"),
        &[CopyException::SetColors(ColorBitmask::GREEN)],
    );
    for (id_a, id_b) in [(1, 2), (2, 1)] {
        let r = registry(vec![
            (id_a, copy_eff(&pinned, y, 1)),
            (id_b, copy_eff(&green_source(), y, 2)),
        ]);
        let read = copiable_values(&r, &env, y).unwrap();
        assert_eq!(read, green_source());
        assert!(
            evaluate(&r, &env, y)
                .unwrap()
                .mana_value_provenance
                .is_plain()
        );
        let copy = read.copy_with(&other("test.c"), &[set_white_cost()]);
        assert_eq!(copy.characteristics.colors, ColorBitmask::WHITE);
        let r = registry(vec![
            (id_a, copy_eff(&pinned, y, 1)),
            (
                id_b,
                plain_eff(
                    Layer::L1b,
                    2,
                    y,
                    LayerModification::FaceDown(Box::new(face_down())),
                ),
            ),
        ]);
        assert_eq!(
            copiable_values(&r, &env, y).unwrap(),
            CopiableValues::plain(face_down())
        );
        assert!(
            evaluate(&r, &env, y)
                .unwrap()
                .mana_value_provenance
                .is_plain()
        );
    }
}

// REQ-LAYER-005, REQ-CORE-001; CR 707.9b: the digest differs when only the pin differs, is equal
// for equal inputs; the cache and a fresh evaluation agree on a pinned provenance, before and
// after a registry change (also under full-recompute).
#[test]
fn round2_pin_digest_and_cache() {
    let c = creature("test.d", 1, 1);
    let map = |pinned: bool| {
        let v = if pinned {
            CopiableValues::plain(c.clone()).modified(&[CopyException::SetColors(c.colors)])
        } else {
            CopiableValues::plain(c.clone()).modified(&[])
        };
        assert_eq!(v.characteristics, c);
        BTreeMap::from([(
            o(1),
            Evaluated {
                characteristics: c.clone(),
                controller: Some(p(1)),
                mana_value_provenance: v.provenance(),
            },
        )])
    };
    assert_eq!(evaluated_digest(&map(true)), evaluated_digest(&map(true)));
    assert_eq!(evaluated_digest(&map(false)), evaluated_digest(&map(false)));
    assert_ne!(evaluated_digest(&map(true)), evaluated_digest(&map(false)));
    let (x, y) = (o(1), o(2));
    let env = Env::default()
        .with(x, creature("test.x", 1, 1))
        .with(y, other("test.y"));
    let pinned = green_source().copy_with(
        &other("test.y"),
        &[CopyException::SetColors(ColorBitmask::GREEN)],
    );
    let mut r = registry(vec![
        (1, copy_eff(&pinned, y, 1)),
        (
            2,
            mod_eff(&[CopyException::SetColors(ColorBitmask::BLUE)], x, 2),
        ),
    ]);
    let mut cache = LayerCache::new();
    for _ in 0..2 {
        let cached = cache.evaluate_all(&r, &env).unwrap();
        let fresh = evaluate_all(&r, &env).unwrap();
        assert_eq!(cached, fresh);
        assert_eq!(evaluated_digest(&cached), evaluated_digest(&fresh));
        assert!(cached[&x].mana_value_provenance.colors_pinned());
        assert!(cached[&y].mana_value_provenance.colors_pinned());
    }
    r.register(e(3), mod_eff(&[set_white_cost()], x, 3))
        .unwrap();
    let cached = cache.evaluate_all(&r, &env).unwrap();
    let fresh = evaluate_all(&r, &env).unwrap();
    assert_eq!(cached, fresh);
    assert_eq!(evaluated_digest(&cached), evaluated_digest(&fresh));
    assert_eq!(cached[&x].characteristics.colors, ColorBitmask::BLUE);
}

// REQ-OBJ-002; CR 202.3b, 202.3c, docs/DECISIONS.md "#38 FP-037 — CodeRabbit round 2: provenance
// export and NotCopied in non-copy modifications": the zero flag is exported only for a Fixed
// source. Back-face values reassigned to Plain export a plain provenance and digest as plain;
// reassigned to CopyOfBackFaceOrMelded they still copy as 0. Genuine back-face and melded
// values keep the flag; a Fixed value without it does not copy as 0.
#[test]
fn round2_provenance_export_normalizes_zero_flag() {
    let c = creature("test.b", 2, 2);
    let digest = |v: &CopiableValues| {
        evaluated_digest(&BTreeMap::from([(
            o(1),
            Evaluated {
                characteristics: c.clone(),
                controller: None,
                mana_value_provenance: v.provenance(),
            },
        )]))
    };
    let genuine = CopiableValues::nonmodal_back_face(c.clone(), Some(cost("{2}{R}")));
    assert!(genuine.provenance().copies_as_zero());
    assert_eq!(
        genuine.copy_with(&other("test.o"), &[]).source,
        ValueSource::CopyOfBackFaceOrMelded
    );
    let mut as_plain = genuine.clone();
    as_plain.source = ValueSource::Plain;
    assert!(as_plain.provenance().is_plain());
    assert_eq!(as_plain.provenance(), ManaValueProvenance::plain());
    assert_eq!(digest(&as_plain), digest(&CopiableValues::plain(c.clone())));
    assert_eq!(
        as_plain.copy_with(&other("test.o"), &[]).source,
        ValueSource::Plain
    );
    assert_eq!(as_plain.mana_value(OFF), 3);
    // `==` still compares the raw private flag.
    assert_ne!(as_plain, CopiableValues::plain(c.clone()));
    let mut as_zero = genuine.clone();
    as_zero.source = ValueSource::CopyOfBackFaceOrMelded;
    assert!(as_zero.provenance().copies_as_zero());
    assert_eq!(as_zero.mana_value(OFF), 0);
    assert_eq!(
        as_zero.provenance(),
        CopiableValues::nonmodal_back_face(c.clone(), None)
            .copy_with(&other("test.o"), &[])
            .provenance()
    );
    let (a, b) = (meld_a(), meld_b());
    let melded = CopiableValues::melded(&MeldedPair::new(&a, &b).unwrap(), vocab()).unwrap();
    assert!(melded.provenance().copies_as_zero());
    assert_eq!(
        melded.copy_with(&other("test.o"), &[]).source,
        ValueSource::CopyOfBackFaceOrMelded
    );
    let fixed = CopiableValues::with_provenance(
        c.clone(),
        ManaValueProvenance {
            source: ValueSource::Fixed { mana_value: 2 },
            zero_when_copied: false,
            colors_pinned: false,
        },
    );
    assert!(!fixed.provenance().copies_as_zero());
    assert_eq!(
        fixed.copy_with(&other("test.o"), &[]).source,
        ValueSource::Plain
    );
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2a, 707.2, 707.9c: in a non-copy modification NotCopied is
// ignored: on a 3/3, [SetPower(5), NotCopied(Power)] gives 5 in either exception order, through
// CopiableValues::modified and the evaluator, alone, over a copy and split over two effects (both
// EffectId assignments); NotCopied(Colors) neither keeps nor pins; in copy_with NotCopied still
// keeps the copier's value.
#[test]
fn round2_not_copied_ignored_in_modifications() {
    let x = o(1);
    let base = creature("test.x0", 3, 3);
    let env = Env::default().with(x, base.clone());
    let set5 = CopyException::SetPower(literal(5));
    let not_p = CopyException::NotCopied(CopiedCharacteristic::Power);
    for exceptions in [
        vec![set5.clone(), not_p.clone()],
        vec![not_p.clone(), set5.clone()],
    ] {
        let m = CopiableValues::plain(base.clone()).modified(&exceptions);
        assert_eq!(m.characteristics.power, Some(literal(5)));
        assert!(m.provenance().is_plain());
        let r = registry(vec![(1, mod_eff(&exceptions, x, 1))]);
        assert_eq!(eval(&r, &env, x).power, Some(literal(5)));
        assert_eq!(
            copiable_values(&r, &env, x).unwrap(),
            CopiableValues::plain(base.clone()).modified(&exceptions)
        );
        for (id_a, id_b) in [(1, 2), (2, 1)] {
            // Over a copy of a 7/8: the set 5 stays, the copied 7 is not restored.
            let r = registry(vec![
                (
                    id_a,
                    copy_eff(&CopiableValues::plain(other("test.y")), x, 1),
                ),
                (id_b, mod_eff(&exceptions, x, 2)),
            ]);
            assert_eq!(eval(&r, &env, x).power, Some(literal(5)));
        }
    }
    for (id_a, id_b) in [(1, 2), (2, 1)] {
        let r = registry(vec![
            (id_a, mod_eff(std::slice::from_ref(&set5), x, 1)),
            (
                id_b,
                mod_eff(
                    &[not_p.clone(), CopyException::SetToughness(literal(6))],
                    x,
                    2,
                ),
            ),
        ]);
        let got = eval(&r, &env, x);
        assert_eq!(
            (got.power, got.toughness),
            (Some(literal(5)), Some(literal(6)))
        );
    }
    // NotCopied(Colors) then a cost == the cost alone.
    let not_c = CopyException::NotCopied(CopiedCharacteristic::Colors);
    let with = [not_c.clone(), set_white_cost()];
    let alone = [set_white_cost()];
    let r_with = registry(vec![(1, mod_eff(&with, x, 1))]);
    let r_alone = registry(vec![(1, mod_eff(&alone, x, 1))]);
    assert_eq!(
        evaluate(&r_with, &env, x).unwrap(),
        evaluate(&r_alone, &env, x).unwrap()
    );
    assert_eq!(eval(&r_with, &env, x).colors, ColorBitmask::WHITE);
    let src = CopiableValues::plain(base.clone());
    assert_eq!(src.modified(&with), src.modified(&alone));
    assert!(!src.modified(&[not_c]).provenance().colors_pinned());
    // A copy keeps the copier's own power (7).
    let copy = src.copy_with(&other("test.own"), &[set5, not_p]);
    assert_eq!(copy.characteristics.power, Some(literal(7)));
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2b, 613.2c, 708.2, docs/DECISIONS.md "#38 FP-037 —
// CodeRabbit round 2: round-trip guarantee qualified": with a layer-1b FaceDown effect,
// copiable_values of a registered copiable_effect(V) is the face-down values with plain
// provenance, not V; without it the round trip gives V. Both timestamp orders.
#[test]
fn round2_round_trip_qualified_by_face_down() {
    let y = o(2);
    let env = Env::default().with(y, other("test.copier"));
    let pinned = green_source().copy_with(
        &other("test.copier"),
        &[CopyException::SetColors(ColorBitmask::GREEN)],
    );
    for v in [back_face_values(), back_face_copy_costed(), pinned] {
        let r = registry(vec![(1, copy_eff(&v, y, 1))]);
        assert_eq!(copiable_values(&r, &env, y).unwrap(), v);
        for (copy_ts, down_ts) in [(1, 2), (2, 1)] {
            let r = registry(vec![
                (1, copy_eff(&v, y, copy_ts)),
                (
                    2,
                    plain_eff(
                        Layer::L1b,
                        down_ts,
                        y,
                        LayerModification::FaceDown(Box::new(face_down())),
                    ),
                ),
            ]);
            let read = copiable_values(&r, &env, y).unwrap();
            assert_ne!(read, v);
            assert_eq!(read, CopiableValues::plain(face_down()));
            assert!(read.provenance().is_plain());
        }
    }
}

// REQ-OBJ-002, REQ-LAYER-001, REQ-CORE-001, REQ-LAYER-005; CR 613.2a, 707.2, 707.9b, 707.9c:
// property over 200 random cases: two random non-copy modifications (NotCopied and SetColors
// included) evaluate exactly as CopiableValues::modified applied twice, characteristics and
// provenance (pin included), in both EffectId assignments; the cache and a fresh evaluation
// give the same digest.
#[test]
fn property_modification_effect_matches_modified() {
    let mut rng = Lcg(0x2b2);
    let x = o(1);
    for n in 0..200 {
        let base = random_chars(&mut rng, n);
        let env = Env::default().with(x, base.clone());
        let first: Vec<CopyException> = (0..rng.below(4))
            .map(|_| random_exception(&mut rng))
            .collect();
        let second: Vec<CopyException> = (0..rng.below(4))
            .map(|_| random_exception(&mut rng))
            .collect();
        let want = CopiableValues::plain(base.clone())
            .modified(&first)
            .modified(&second);
        for (id_a, id_b) in [(1, 2), (2, 1)] {
            let r = registry(vec![
                (id_a, mod_eff(&first, x, 1)),
                (id_b, mod_eff(&second, x, 2)),
            ]);
            assert_eq!(copiable_values(&r, &env, x).unwrap(), want);
            let fresh = evaluate_all(&r, &env).unwrap();
            assert_eq!(fresh[&x].mana_value_provenance, want.provenance());
            let cached = LayerCache::new().evaluate_all(&r, &env).unwrap();
            assert_eq!(evaluated_digest(&cached), evaluated_digest(&fresh));
        }
    }
}

// REQ-OBJ-002, REQ-LAYER-001; CR 208.2b, 614.1c, 614.1e, 611.2a, 611.3b, docs/DECISIONS.md
// "#38 FP-037 — CodeRabbit round 2: as-enters P/T settings are Resolved-origin effects": an
// as-enters P/T setting registered with EffectOrigin::Resolved keeps applying after its source
// loses the generating ability, alone, over an earlier copy and after that copy is removed (both
// EffectId assignments); the same setting with a Static origin is dropped.
#[test]
fn round2_as_enters_pt_survives_lost_ability() {
    let x = o(1);
    let base = creature("test.x0", 1, 1);
    let as_enters = |origin: EffectOrigin, t: u64| {
        modification_effect(
            &[
                CopyException::SetPower(literal(4)),
                CopyException::SetToughness(literal(5)),
            ],
            Some(x),
            p(1),
            origin,
            ts(t),
            objs(&[x]),
        )
    };
    let pt = |c: &Characteristics| (c.power.clone(), c.toughness.clone());
    let want = (Some(literal(4)), Some(literal(5)));
    for (copy_id, mod_id) in [(1, 2), (2, 1)] {
        let mut env = Env::default().with(x, base.clone());
        env.lost.insert(e(mod_id));
        let r = registry(vec![(mod_id, as_enters(EffectOrigin::Resolved, 2))]);
        assert_eq!(pt(&eval(&r, &env, x)), want);
        let mut r = registry(vec![
            (
                copy_id,
                copy_eff(&CopiableValues::plain(other("test.y")), x, 1),
            ),
            (mod_id, as_enters(EffectOrigin::Resolved, 2)),
        ]);
        let got = eval(&r, &env, x);
        assert_eq!(got.names, set(&["test.y"]));
        assert_eq!(pt(&got), want);
        assert!(r.remove(e(copy_id)));
        let got = eval(&r, &env, x);
        assert_eq!(got.names, set(&["test.x0"]));
        assert_eq!(pt(&got), want);
        // Contrast: a Static-origin effect whose ability is gone does not apply (CR 611.3b).
        let r = registry(vec![(
            mod_id,
            as_enters(EffectOrigin::Static { granted_at: None }, 2),
        )]);
        assert_eq!(eval(&r, &env, x), base);
    }
}

// ---------------------------------------------------------------------------------------------
// CodeRabbit round 3
// ---------------------------------------------------------------------------------------------

/// A layer-1a copy effect of `values` on `target` at `t` from a static ability of `target`.
fn static_copy_eff(values: &CopiableValues, target: ObjectId, t: u64) -> ContinuousEffect {
    copiable_effect(
        values,
        Some(target),
        p(1),
        EffectOrigin::Static { granted_at: None },
        ts(t),
        objs(&[target]),
    )
}

/// Asserts that `read` has `want`'s characteristics and exported provenance.
fn assert_same_values(read: &CopiableValues, want: &CopiableValues) {
    assert_eq!(read.characteristics, want.characteristics);
    assert_eq!(read.provenance(), want.provenance());
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2c, 707.2, docs/DECISIONS.md "#38 FP-037 — CodeRabbit
// round 3: round-trip guarantee requires an applying effect and preserves characteristics and
// provenance": the round trip needs the copy effect to apply. A Static-origin copiable_effect(V)
// whose generating ability is absent is skipped, so copiable_values gives the base (or the other
// effect); the same effect with a Resolved origin, or with the ability present, gives V's
// characteristics and provenance. With a second (earlier) copy effect, both EffectId orders.
#[test]
fn round3_round_trip_requires_applying_effect() {
    let y = o(2);
    let base = other("test.copier");
    let earlier = CopiableValues::plain(creature("test.earlier", 2, 2));
    for v in [back_face_values(), back_face_copy_costed(), green_source()] {
        // Alone.
        let mut env = Env::default().with(y, base.clone());
        env.lost.insert(e(1));
        let r = registry(vec![(1, static_copy_eff(&v, y, 1))]);
        assert_eq!(
            copiable_values(&r, &env, y).unwrap(),
            CopiableValues::plain(base.clone())
        );
        let r = registry(vec![(1, copy_eff(&v, y, 1))]);
        assert_same_values(&copiable_values(&r, &env, y).unwrap(), &v);
        let present = Env::default().with(y, base.clone());
        let r = registry(vec![(1, static_copy_eff(&v, y, 1))]);
        assert_same_values(&copiable_values(&r, &present, y).unwrap(), &v);
        // Over an earlier copy, both EffectId orders.
        for (v_id, earlier_id) in [(1, 2), (2, 1)] {
            let mut env = Env::default().with(y, base.clone());
            env.lost.insert(e(v_id));
            let r = registry(vec![
                (earlier_id, copy_eff(&earlier, y, 1)),
                (v_id, static_copy_eff(&v, y, 2)),
            ]);
            assert_same_values(&copiable_values(&r, &env, y).unwrap(), &earlier);
            assert_same_values(&copiable_values(&r, &present, y).unwrap(), &v);
            let r = registry(vec![
                (earlier_id, copy_eff(&earlier, y, 1)),
                (v_id, copy_eff(&v, y, 2)),
            ]);
            assert_same_values(&copiable_values(&r, &env, y).unwrap(), &v);
        }
    }
}

// REQ-OBJ-002; CR 202.3b, 613.2c, docs/DECISIONS.md "#38 FP-037 — CodeRabbit round 3:
// round-trip guarantee requires an applying effect and preserves characteristics and
// provenance": what the round trip preserves is the characteristics and the exported provenance,
// not CopiableValues equality. A back-face value whose public source is reassigned to Plain reads
// back with equal characteristics and equal provenance() but is not == to the original, because
// == compares the raw private flag.
#[test]
fn round3_round_trip_preserves_characteristics_and_provenance() {
    let y = o(2);
    let env = Env::default().with(y, other("test.copier"));
    let mut v = back_face_values();
    v.source = ValueSource::Plain;
    let r = registry(vec![(1, copy_eff(&v, y, 1))]);
    let read = copiable_values(&r, &env, y).unwrap();
    assert_eq!(read.characteristics, v.characteristics);
    assert_eq!(read.provenance(), v.provenance());
    assert!(read.provenance().is_plain());
    // Documented: == still compares the raw flag, so the read is not == to V.
    assert_ne!(read, v);
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2c, 707.2, docs/DECISIONS.md "#38 FP-037 — CodeRabbit
// round 3: as-enters P/T settings use modification_effect": an as-enters P/T setting built by
// modification_effect with EffectOrigin::Resolved while an earlier copy C is active keeps only
// the P/T it sets when C is removed: the object's own name, cost and abilities come back. The
// snapshot route (copiable_effect of CopiableValues::modified of the values read while C is
// active) would keep C's name, cost and abilities. Both timestamp orders, both EffectId orders,
// with the setting's ability lost.
#[test]
fn round3_as_enters_pt_with_modification_effect_after_copy_expires() {
    let x = o(1);
    let base = creature("test.x0", 1, 1);
    let mut c_values = other("test.c");
    c_values.abilities.push(ability("test.c_extra"));
    let c = CopiableValues::plain(c_values.clone());
    let set_pt = [
        CopyException::SetPower(literal(4)),
        CopyException::SetToughness(literal(5)),
    ];
    let pt = |ch: &Characteristics| (ch.power.clone(), ch.toughness.clone());
    let want = (Some(literal(4)), Some(literal(5)));
    for (c_ts, mod_ts) in [(1, 2), (2, 1)] {
        for (c_id, mod_id) in [(1, 2), (2, 1)] {
            let mut env = Env::default().with(x, base.clone());
            env.lost.insert(e(mod_id));
            let mut r = registry(vec![(c_id, copy_eff(&c, x, c_ts))]);
            // Built while C is active.
            let setting = modification_effect(
                &set_pt,
                Some(x),
                p(1),
                EffectOrigin::Resolved,
                ts(mod_ts),
                objs(&[x]),
            );
            r.register(e(mod_id), setting).unwrap();
            let got = eval(&r, &env, x);
            assert_eq!(got.names, set(&["test.c"]));
            if mod_ts > c_ts {
                assert_eq!(pt(&got), want);
            } else {
                // CR 613.2a: the later copy replaces all characteristics.
                assert_eq!(pt(&got), (Some(literal(7)), Some(literal(8))));
            }
            // C expires.
            assert!(r.remove(e(c_id)));
            let got = eval(&r, &env, x);
            assert_eq!(got.names, base.names);
            assert_eq!(got.mana_cost, base.mana_cost);
            assert_eq!(got.abilities, base.abilities);
            assert_eq!(pt(&got), want);
            let read = copiable_values(&r, &env, x).unwrap();
            assert_eq!(read, CopiableValues::plain(base.clone()).modified(&set_pt));
        }
    }
    // The snapshot route keeps the expired copy's other characteristics.
    let env = Env::default().with(x, base.clone());
    let mut r = registry(vec![(1, copy_eff(&c, x, 1))]);
    let snapshot = copiable_values(&r, &env, x).unwrap().modified(&set_pt);
    r.register(e(2), copy_eff(&snapshot, x, 2)).unwrap();
    assert!(r.remove(e(1)));
    let got = eval(&r, &env, x);
    assert_eq!(got.names, set(&["test.c"]));
    assert_eq!(got.abilities, c_values.abilities);
}

/// The back face of [`nonmodal_dfc`] with the front cost `front` as copiable values.
fn back_face_with_front(front: &str) -> CopiableValues {
    let back = base_characteristics(&nonmodal_dfc(), &back_up(), vocab()).unwrap();
    CopiableValues::nonmodal_back_face(back, Some(cost(front)))
}

/// The melded permanent of [`meld_a`] and [`meld_b`] as copiable values (mana value 7).
fn melded_values() -> CopiableValues {
    let (a, b) = (meld_a(), meld_b());
    let pair = MeldedPair::new(&a, &b).unwrap();
    CopiableValues::melded(&pair, vocab()).unwrap()
}

// REQ-OBJ-002, REQ-LAYER-001; CR 202.3b, 202.3e, docs/DECISIONS.md "#38 FP-037 — CodeRabbit
// round 3: base mana-value provenance from the environment": an environment that supplies the
// base provenance of a nonmodal back face (front {3}{R}) makes evaluate report mana value 4 with
// no effects, in every context; X in the front cost counts 0. With the default method the same
// costless base still gives 0.
#[test]
fn round3_base_provenance_evaluate_back_face() {
    let x = o(1);
    let r = ContinuousEffects::new();
    let v = back_face_with_front("{3}{R}");
    assert_eq!(v.characteristics.mana_cost, None);
    let env = Env::default().with_values(x, &v);
    let ev = evaluate(&r, &env, x).unwrap();
    assert_eq!(ev.characteristics, v.characteristics);
    assert_eq!(ev.mana_value_provenance, v.provenance());
    assert_eq!(ev.mana_value(OFF), 4);
    assert_eq!(ev.mana_value(on_stack(3)), 4);
    // CR 202.3e: X in the front cost counts 0.
    let vx = back_face_with_front("{X}{3}{R}");
    let env_x = Env::default().with_values(x, &vx);
    assert_eq!(evaluate(&r, &env_x, x).unwrap().mana_value(OFF), 4);
    assert_eq!(evaluate(&r, &env_x, x).unwrap().mana_value(on_stack(5)), 4);
    // The default method: plain provenance, the costless face's own 0.
    let plain_env = Env::default().with(x, v.characteristics.clone());
    let ev = evaluate(&r, &plain_env, x).unwrap();
    assert!(ev.mana_value_provenance.is_plain());
    assert_eq!(ev.mana_value(OFF), 0);
}

// REQ-OBJ-002, REQ-LAYER-001; CR 202.3b, 613.2c, 707.2, docs/DECISIONS.md "#38 FP-037 —
// CodeRabbit round 3: base mana-value provenance from the environment": copiable_values of such a
// base carries the back-face provenance, so a copy of the read with SetManaCost({5}) has mana
// value 0, directly and once registered through copiable_effect on another object and read back.
#[test]
fn round3_base_provenance_copiable_values_back_face() {
    let (x, y) = (o(1), o(2));
    let v = back_face_with_front("{3}{R}");
    let env = Env::default()
        .with_values(x, &v)
        .with(y, creature("test.y", 1, 1));
    let r = ContinuousEffects::new();
    let read = copiable_values(&r, &env, x).unwrap();
    assert_eq!(read.provenance(), v.provenance());
    assert_eq!(read.characteristics, v.characteristics);
    assert_eq!(read.mana_value(OFF), 4);
    let five = [CopyException::SetManaCost(Some(cost("{5}")))];
    let copy = read.copy_with(&creature("test.y", 1, 1), &five);
    assert_eq!(copy.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(copy.mana_value(OFF), 0);
    let r = registry(vec![(1, copy_eff(&copy, y, 1))]);
    let ev = evaluate(&r, &env, y).unwrap();
    assert_eq!(ev.characteristics.mana_cost, Some(cost("{5}")));
    assert_eq!(ev.mana_value(OFF), 0);
    let back = copiable_values(&r, &env, y).unwrap();
    assert_eq!(back.mana_value(OFF), 0);
    assert_eq!(back.copy_with(&other("test.z"), &five).mana_value(OFF), 0);
    // The default method: plain provenance, so the costed copy is 5.
    let plain_env = Env::default().with(x, v.characteristics.clone());
    let plain_read = copiable_values(&ContinuousEffects::new(), &plain_env, x).unwrap();
    assert!(plain_read.provenance().is_plain());
    assert_eq!(
        plain_read
            .copy_with(&creature("test.y", 1, 1), &five)
            .mana_value(OFF),
        5
    );
}

// REQ-OBJ-002, REQ-LAYER-001; CR 202.3c, 713.5c, docs/DECISIONS.md "#38 FP-037 — CodeRabbit
// round 3: base mana-value provenance from the environment": a melded base supplied with its
// provenance evaluates to the sum of its front faces (7, X counting 0); a copy of it is 0, also
// through copiable_effect on another object.
#[test]
fn round3_base_provenance_melded() {
    let (x, y) = (o(1), o(2));
    let v = melded_values();
    let env = Env::default()
        .with_values(x, &v)
        .with(y, creature("test.y", 1, 1));
    let r = ContinuousEffects::new();
    let ev = evaluate(&r, &env, x).unwrap();
    assert_eq!(ev.mana_value(OFF), 7);
    assert_eq!(ev.mana_value(on_stack(4)), 7);
    let read = copiable_values(&r, &env, x).unwrap();
    assert_eq!(read, v);
    let copy = read.copy_with(&creature("test.y", 1, 1), &[]);
    assert_eq!(copy.mana_value(OFF), 0);
    let r = registry(vec![(1, copy_eff(&copy, y, 1))]);
    assert_eq!(evaluate(&r, &env, y).unwrap().mana_value(OFF), 0);
    assert_eq!(copiable_values(&r, &env, y).unwrap().mana_value(OFF), 0);
}

// REQ-OBJ-002, REQ-LAYER-001; CR 613.2b, 708.2, 202.3a, 202.3b, docs/DECISIONS.md "#38 FP-037 —
// CodeRabbit round 3: base mana-value provenance from the environment": over a back-face base
// with its provenance, a FaceDown effect resets the provenance to plain (mana value 0), a plain
// CopyValues effect resets it to plain, a modification_effect keeps it (4, copies as 0). Two
// interacting effects in both EffectId orders and both timestamp orders.
#[test]
fn round3_base_provenance_replaced_or_kept_in_layer_one() {
    let x = o(1);
    let v = back_face_with_front("{3}{R}");
    let env = Env::default().with_values(x, &v);
    let down = |t: u64| {
        plain_eff(
            Layer::L1b,
            t,
            x,
            LayerModification::FaceDown(Box::new(face_down())),
        )
    };
    let plain = CopiableValues::plain(creature("test.p", 2, 2));
    // Each alone.
    let r = registry(vec![(1, down(1))]);
    let ev = evaluate(&r, &env, x).unwrap();
    assert!(ev.mana_value_provenance.is_plain());
    assert_eq!(ev.mana_value(OFF), 0);
    assert_eq!(
        copiable_values(&r, &env, x).unwrap(),
        CopiableValues::plain(face_down())
    );
    let r = registry(vec![(1, copy_eff(&plain, x, 1))]);
    let ev = evaluate(&r, &env, x).unwrap();
    assert!(ev.mana_value_provenance.is_plain());
    assert_eq!(ev.mana_value(OFF), 3);
    let r = registry(vec![(1, pt_mod(x, 1, 6, 6))]);
    let ev = evaluate(&r, &env, x).unwrap();
    assert_eq!(ev.mana_value_provenance, v.provenance());
    assert_eq!(ev.mana_value(OFF), 4);
    let read = copiable_values(&r, &env, x).unwrap();
    assert_eq!(read.provenance(), v.provenance());
    assert_eq!(read.characteristics.power, Some(literal(6)));
    assert_eq!(read.copy_with(&other("test.c"), &[]).mana_value(OFF), 0);
    // Pairs, both EffectId orders and both timestamp orders.
    for (ta, tb) in [(1, 2), (2, 1)] {
        for (ia, ib) in [(1, 2), (2, 1)] {
            // modification + FaceDown: 1b is after 1a, so face down whatever the timestamps.
            let r = registry(vec![(ia, pt_mod(x, ta, 6, 6)), (ib, down(tb))]);
            let ev = evaluate(&r, &env, x).unwrap();
            assert!(ev.mana_value_provenance.is_plain());
            assert_eq!(ev.mana_value(OFF), 0);
            assert_eq!(ev.characteristics, face_down());
            // modification + plain copy: the later one decides the source.
            let r = registry(vec![
                (ia, pt_mod(x, ta, 6, 6)),
                (ib, copy_eff(&plain, x, tb)),
            ]);
            let ev = evaluate(&r, &env, x).unwrap();
            let read = copiable_values(&r, &env, x).unwrap();
            assert!(ev.mana_value_provenance.is_plain());
            assert_eq!(ev.mana_value(OFF), 3);
            assert!(read.provenance().is_plain());
            let want_power = if ta > tb { 6 } else { 2 };
            assert_eq!(ev.characteristics.power, Some(literal(want_power)));
        }
    }
}

// REQ-OBJ-002, REQ-CORE-001; CR 613.2c, 202.3b, 202.3c: property over 200 random cases and
// every provenance kind as the base provenance: with no effects copiable_values gives the base's
// characteristics and provenance and evaluate reports its mana value; the result does not depend
// on the call order.
#[test]
fn round3_property_base_provenance() {
    let mut rng = Lcg(0x3b3);
    let x = o(1);
    let r = ContinuousEffects::new();
    for n in 0..200 {
        let c = random_chars(&mut rng, n);
        let kinds = provenance_kinds(&c);
        let v = &kinds[rng.below(kinds.len())];
        let env = Env::default().with_values(x, v);
        let read = copiable_values(&r, &env, x).unwrap();
        assert_same_values(&read, v);
        let ev = evaluate(&r, &env, x).unwrap();
        assert_eq!(ev.mana_value_provenance, v.provenance());
        for ctx in [OFF, on_stack(2)] {
            assert_eq!(ev.mana_value(ctx), v.mana_value(ctx));
        }
        assert_eq!(copiable_values(&r, &env, x).unwrap(), read);
        assert_eq!(evaluate(&r, &env, x).unwrap(), ev);
    }
}

/// A synthetic neutral ability (a STATIC ability with one RULE `SKIP` effect) with ability id
/// `id` (R5; #232: replaces the former string placeholder abilities).
fn test_ability(id: &str) -> AbilityAST {
    serde_json::from_value(serde_json::json!({
        "abilityId": id,
        "kind": "STATIC",
        "effects": [{"type": "RULE", "kind": "SKIP"}],
    }))
    .expect("valid synthetic ability")
}

/// A one-token synthetic rules text: a LITERAL token `t1` whose text is `text` (R5; #232:
/// replaces the former string placeholder text).
fn literal_text(text: &str) -> TextTokenStream {
    TextTokenStream::new(vec![fineprint_dsl::text::TextToken::Literal {
        id: "t1".to_owned(),
        text: text.to_owned(),
    }])
    .expect("valid synthetic text")
}
