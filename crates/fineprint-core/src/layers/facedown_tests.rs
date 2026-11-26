// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of layer 1b (facedown.rs): REQ-OBJ-003 (face-down spells and permanents),
// REQ-OBJ-002 (copiable values), REQ-LAYER-001 and REQ-LAYER-002 (layer order), REQ-CORE-004
// (redaction), REQ-LAYER-005 (cache agrees with full evaluation), REQ-CORE-006 (determinism). Synthetic ids, characteristics and definitions only
// (R5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{CardId, EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{
    AbilityAST, CardDefinitionDSL, CardFaceDSL, ColorBitmask, DSL_VERSION, FaceKind,
    LayoutBehavior, Origin, ProvenanceDSL, SupportStatus, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST, SelectorFilter};
use fineprint_dsl::mana::ManaCostDSL;
use fineprint_dsl::vocab::RulesVocabulary;

use super::*;
use crate::knowledge::Knowledge;
use crate::layouts::{
    FaceDownCause, FaceDownProfile, LayoutQuery, MergedComponent, MergedPermanent,
    base_characteristics,
};
use crate::memory::CharacteristicsProvider;
use crate::objects::Characteristics;
use crate::zones::{MoveRequest, ZoneKind, Zones, ZonesConfig};

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

const P1: PlayerId = PlayerId::new(1);
const P2: PlayerId = PlayerId::new(2);

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

fn objs(ids: &[ObjectId]) -> BTreeSet<ObjectId> {
    ids.iter().copied().collect()
}

/// `v` decoded as a typed ability (valid synthetic content).
fn ability_json(v: serde_json::Value) -> AbilityAST {
    serde_json::from_value(v).expect("valid synthetic ability")
}

/// A synthetic keyword ability `id` with keyword name `keyword`.
fn keyword_ability(id: &str, keyword: &str) -> AbilityAST {
    ability_json(serde_json::json!({"kind": "KEYWORD", "abilityId": id, "keyword": keyword}))
}

/// An enters-the-battlefield-like triggered ability (synthetic).
fn etb_ability(tag: &str) -> AbilityAST {
    ability_json(serde_json::json!({
        "kind": "TRIGGERED",
        "abilityId": format!("test.{tag}.etb"),
        "trigger": {"mode": "EVENT", "event": {"kind": "ENTERS", "subject": {"ref": "SELF"}}},
        "effect": [],
    }))
}

/// The ward {2} ability of the disguise and cloak profiles (#232: keyword `WARD`, the cost as
/// the keyword parameter `cost`).
fn ward_2() -> AbilityAST {
    ability_json(serde_json::json!({
        "kind": "KEYWORD",
        "abilityId": "facedown:ward",
        "keyword": "WARD",
        "params": {"cost": "{2}"},
    }))
}

/// A synthetic face-up card `tag`: a name, a cost, a subtype, rules text, an ETB-like ability,
/// green, 5/5.
fn card(tag: &str) -> Characteristics {
    Characteristics {
        names: set(&[&format!("test.{tag}")]),
        mana_cost: Some(cost("{4}{G}")),
        color_indicator: None,
        colors: ColorBitmask::GREEN,
        supertypes: set(&["Legendary"]),
        card_types: set(&["Creature"]),
        subtypes: set(&[&format!("Sub{tag}")]),
        rules_text: literal_text(&format!("test.text.{tag}")),
        abilities: vec![etb_ability(tag)],
        power: Some(literal(5)),
        toughness: Some(literal(5)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A different synthetic card, used as the original of a copy.
fn other(tag: &str) -> Characteristics {
    Characteristics {
        names: set(&[&format!("test.{tag}")]),
        mana_cost: Some(cost("{1}{U}")),
        color_indicator: None,
        colors: ColorBitmask::BLUE,
        supertypes: BTreeSet::new(),
        card_types: set(&["Artifact", "Creature"]),
        subtypes: set(&[&format!("Other{tag}")]),
        rules_text: literal_text(&format!("test.text.{tag}")),
        abilities: vec![test_ability(&format!("test.{tag}.static"))],
        power: Some(literal(1)),
        toughness: Some(literal(4)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A synthetic environment: bases (on the battlefield unless listed in `off_battlefield`),
/// counters, no selector matches.
#[derive(Default, Clone)]
struct Env {
    bases: BTreeMap<ObjectId, ObjectBase>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
}

impl Env {
    fn with(self, id: ObjectId, c: Characteristics) -> Env {
        self.with_zone(id, c, true)
    }

    fn with_zone(mut self, id: ObjectId, c: Characteristics, on_battlefield: bool) -> Env {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(P1),
                on_battlefield,
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
}

fn eval(r: &ContinuousEffects, env: &Env, id: ObjectId) -> Characteristics {
    evaluate(r, env, id).unwrap().characteristics
}

/// The characteristics of `profile` (its P/T are literals, as layer 7 writes them).
fn profile_chars(profile: &FaceDownProfile) -> Characteristics {
    profile.characteristics().clone()
}

/// A resolved face-down effect of `profile` on `object` at `t`.
fn fd(profile: &FaceDownProfile, object: ObjectId, t: u64) -> ContinuousEffect {
    face_down_effect(profile, None, P1, EffectOrigin::Resolved, ts(t), object)
}

/// A resolved layer-1a copy of `values` on `object` at `t`.
fn copy(values: &Characteristics, object: ObjectId, t: u64) -> ContinuousEffect {
    copiable_effect(
        &CopiableValues::plain(values.clone()),
        None,
        P1,
        EffectOrigin::Resolved,
        ts(t),
        objs(&[object]),
    )
}

/// A resolved effect on `object` in `layer` at `t`.
fn plain(layer: Layer, t: u64, object: ObjectId, m: LayerModification) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: P1,
        timestamp: ts(t),
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected: Affected::Objects(objs(&[object])),
        modification: m,
    }
}

fn registry(list: Vec<(u64, ContinuousEffect)>) -> ContinuousEffects {
    let mut r = ContinuousEffects::new();
    for (id, effect) in list {
        r.register(e(id), effect).unwrap();
    }
    r
}

fn pt(c: &Characteristics) -> Option<(i64, i64)> {
    Some((
        literal_of(c.power.as_ref()?)?,
        literal_of(c.toughness.as_ref()?)?,
    ))
}

// --- synthetic card definitions (merged permanents) ------------------------------------------

fn face(tag: &str, mana_cost: &str, pt: (i64, i64), subtype: &str) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: FaceKind::Main,
        name: format!("test.{tag}"),
        mana_cost: Some(cost(mana_cost)),
        color_indicator: None,
        supertypes: Vec::new(),
        card_types: vec!["Creature".to_owned()],
        subtypes: vec![subtype.to_owned()],
        power: Some(ValueExpressionAST::Literal(pt.0)),
        toughness: Some(ValueExpressionAST::Literal(pt.1)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: literal_text(&format!("test.text.{tag}")),
        abilities: vec![etb_ability(tag)],
    }
}

fn normal(tag: &str, mana_cost: &str, pt: (i64, i64), subtype: &str) -> CardDefinitionDSL {
    CardDefinitionDSL {
        card_def_id: format!("test.{tag}"),
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
        layout: LayoutBehavior::Normal,
        faces: vec![face(tag, mana_cost, pt, subtype)],
        meld_partner: None,
        color_identity: ColorBitmask::NONE,
        commander_eligibility: None,
        legalities: BTreeMap::new(),
        digital_only: false,
        related_tokens: None,
    }
}

fn component(d: &CardDefinitionDSL, face_up: bool) -> MergedComponent<'_> {
    MergedComponent {
        def: d,
        query: LayoutQuery::new(ZoneKind::Battlefield),
        is_token: false,
        face_up,
    }
}

fn printed(d: &CardDefinitionDSL) -> Characteristics {
    base_characteristics(d, &LayoutQuery::new(ZoneKind::Battlefield), vocab()).unwrap()
}

/// A ledger and generator after `n` timestamps were issued.
fn clock(n: u64) -> (TimestampLedger, IdGenerator<Timestamp>) {
    (TimestampLedger::new(), IdGenerator::from_last_issued(n))
}

/// `turn_face_down` with the default profile, no source, controller P1, resolved origin.
fn down(
    r: &mut ContinuousEffects,
    id: u64,
    target: FaceDownTarget,
    object: ObjectId,
    ledger: &mut TimestampLedger,
    ids: &mut IdGenerator<Timestamp>,
) -> Result<Timestamp, FaceDownRefusal> {
    turn_face_down(
        r,
        e(id),
        &FaceDownProfile::default_2_2(),
        &target,
        None,
        P1,
        EffectOrigin::Resolved,
        object,
        ledger,
        ids,
    )
}

// ---------------------------------------------------------------------------------------------
// face_down_effect (CR 613.2b, 708.2, 708.3, 708.4)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-003, REQ-LAYER-001; CR 613.2b, 708.2: the builder's shape; `register` accepts it.
#[test]
fn req_obj_003_face_down_effect_shape() {
    let profile = FaceDownProfile::with_ward_2();
    let eff = face_down_effect(
        &profile,
        Some(o(9)),
        P2,
        EffectOrigin::Static { granted_at: None },
        ts(7),
        o(1),
    );
    assert_eq!(eff.layer, Layer::L1b);
    assert!(!eff.is_cda);
    assert_eq!(eff.affected, Affected::Objects(objs(&[o(1)])));
    assert_eq!(
        eff.modification,
        LayerModification::FaceDown(Box::new(profile.characteristics().clone()))
    );
    assert_eq!(
        (eff.source, eff.controller, eff.timestamp),
        (Some(o(9)), P2, ts(7))
    );
    assert_eq!(eff.origin, EffectOrigin::Static { granted_at: None });
    let mut r = ContinuousEffects::new();
    r.register(e(1), eff.clone()).unwrap();
    assert_eq!(r.get(e(1)), Some(&eff));
}

// REQ-OBJ-003; CR 708.2, 708.2a, 708.3, 708.4: an object whose base has a name, cost, subtypes,
// an ETB-like ability, rules text and 5/5 evaluates to exactly the profile, as a permanent and
// as a spell (not on the battlefield); the ward variant adds ward {2}. A double-faced card may
// enter face down (CR 712.12): the builder does not look at the card.
#[test]
fn req_obj_003_enters_or_cast_face_down_is_exactly_the_profile() {
    for on_battlefield in [true, false] {
        for cause in [
            FaceDownCause::Morph,
            FaceDownCause::Megamorph,
            FaceDownCause::Manifest,
            FaceDownCause::ManifestDread,
            FaceDownCause::Disguise,
            FaceDownCause::Cloak,
        ] {
            let profile = FaceDownProfile::for_cause(cause);
            let env = Env::default().with_zone(o(1), card("hidden"), on_battlefield);
            let r = registry(vec![(1, fd(&profile, o(1), 5))]);
            let got = evaluate(&r, &env, o(1)).unwrap();
            assert_eq!(got.characteristics, profile_chars(&profile), "{cause:?}");
            assert!(got.mana_value_provenance.is_plain());
            // CR 708.3: the face-up ETB ability is not there to trigger or apply.
            assert!(
                !got.characteristics
                    .abilities
                    .contains(&etb_ability("hidden"))
            );
            let ward = matches!(cause, FaceDownCause::Disguise | FaceDownCause::Cloak);
            assert_eq!(got.characteristics.abilities == vec![ward_2()], ward);
            assert_eq!(pt(&got.characteristics), Some((2, 2)));
        }
    }
}

// REQ-OBJ-003; CR 708.2: a custom profile listed by a card effect is applied as given.
#[test]
fn req_obj_003_custom_profile_applies_as_given() {
    let mut listed = FaceDownProfile::default_2_2().characteristics().clone();
    listed.card_types = set(&["Artifact", "Creature"]);
    listed.subtypes = set(&["Golem"]);
    let profile = FaceDownProfile::custom(listed.clone());
    let env = Env::default().with(o(1), card("hidden"));
    let r = registry(vec![(1, fd(&profile, o(1), 5))]);
    assert_eq!(eval(&r, &env, o(1)), listed);
}

// ---------------------------------------------------------------------------------------------
// Copiable values (CR 613.2b, 613.2c, 707.2, 707.3, 708.10)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-003, REQ-OBJ-002; CR 613.2c, 708.2, 708.2a: the copiable values of a face-down object
// are the profile with plain mana-value provenance, even over a layer-1a copy carrying a fixed
// mana value.
#[test]
fn req_obj_003_copiable_values_are_the_profile() {
    let profile = FaceDownProfile::default_2_2();
    let env = Env::default().with(o(1), card("hidden"));
    let mut fixed = CopiableValues::plain(other("orig"));
    fixed.source = ValueSource::Fixed { mana_value: 7 };
    let r = registry(vec![
        (
            1,
            copiable_effect(
                &fixed,
                None,
                P1,
                EffectOrigin::Resolved,
                ts(9),
                objs(&[o(1)]),
            ),
        ),
        (2, fd(&profile, o(1), 5)),
    ]);
    let values = copiable_values(&r, &env, o(1)).unwrap();
    assert_eq!(values.characteristics, profile_chars(&profile));
    assert!(values.provenance().is_plain());
    assert_eq!(values.source, ValueSource::Plain);
    assert_eq!(
        copiable_characteristics(&r, &env, o(1)).unwrap(),
        profile_chars(&profile)
    );
}

// REQ-OBJ-003, REQ-LAYER-001, REQ-LAYER-002; CR 613.2a, 613.2b, 708.10: a layer-1a copy and a
// layer-1b face-down effect give the profile in both timestamp orders and both id orders (layer
// order, not timestamp, decides).
#[test]
fn req_obj_003_copy_and_face_down_both_orders() {
    let profile = FaceDownProfile::default_2_2();
    let env = Env::default().with(o(1), card("hidden"));
    for (copy_ts, fd_ts) in [(10, 20), (20, 10)] {
        for (copy_id, fd_id) in [(1, 2), (2, 1)] {
            let r = registry(vec![
                (copy_id, copy(&other("orig"), o(1), copy_ts)),
                (fd_id, fd(&profile, o(1), fd_ts)),
            ]);
            assert_eq!(eval(&r, &env, o(1)), profile_chars(&profile));
            assert_eq!(
                copiable_characteristics(&r, &env, o(1)).unwrap(),
                profile_chars(&profile)
            );
        }
    }
}

// REQ-OBJ-003, REQ-OBJ-002; CR 707.2, 707.3, 708.2: a copy of a face-down object copies the
// profile; the copy itself has no face-down effect, so it is face up with the profile's
// characteristics.
#[test]
fn req_obj_003_copy_of_a_face_down_object_copies_the_profile() {
    let profile = FaceDownProfile::with_ward_2();
    let (hidden, copier) = (o(1), o(2));
    let env = Env::default()
        .with(hidden, card("hidden"))
        .with(copier, other("copier"));
    let mut r = registry(vec![(1, fd(&profile, hidden, 5))]);
    let values = copiable_values(&r, &env, hidden).unwrap();
    r.register(
        e(2),
        copiable_effect(
            &values,
            None,
            P1,
            EffectOrigin::Resolved,
            ts(6),
            objs(&[copier]),
        ),
    )
    .unwrap();
    assert_eq!(eval(&r, &env, copier), profile_chars(&profile));
    // No layer-1b effect affects the copy (CR 707.3: it is face up).
    assert!(
        r.iter().all(|(_, eff)| eff.layer != Layer::L1b
            || eff.affected != Affected::Objects(objs(&[copier])))
    );
    // Turning the original face up does not change the copy (a snapshot, CR 707.2).
    let (mut l, mut g) = clock(10);
    turn_face_up(&mut r, e(1), hidden, false, &mut l, &mut g).unwrap();
    assert_eq!(eval(&r, &env, hidden), card("hidden"));
    assert_eq!(eval(&r, &env, copier), profile_chars(&profile));
}

// REQ-OBJ-003, REQ-LAYER-001; CR 708.2, 613.2c: effects of later layers and counters still
// apply to a face-down object: layer 4 adds a type, layer 6 adds an ability, layer 7c makes it
// 3/3; a +1/+1 counter makes it 4/4.
#[test]
fn req_obj_003_later_layers_apply_to_face_down_object() {
    let profile = FaceDownProfile::default_2_2();
    let granted = keyword_ability("t", "FLYING");
    let env = Env::default().with(o(1), card("hidden"));
    let r = registry(vec![
        (1, fd(&profile, o(1), 5)),
        (
            2,
            plain(
                Layer::L4,
                3,
                o(1),
                LayerModification::AddTypes {
                    supertypes: BTreeSet::new(),
                    card_types: set(&["Artifact"]),
                    subtypes: BTreeSet::new(),
                },
            ),
        ),
        (
            3,
            plain(
                Layer::L6,
                4,
                o(1),
                LayerModification::AddAbility(granted.clone()),
            ),
        ),
        (
            4,
            plain(
                Layer::L7c,
                2,
                o(1),
                LayerModification::ModifyPt {
                    power: 1,
                    toughness: 1,
                },
            ),
        ),
    ]);
    let c = eval(&r, &env, o(1));
    assert_eq!(c.card_types, set(&["Artifact", "Creature"]));
    assert_eq!(c.abilities, vec![granted]);
    assert_eq!(pt(&c), Some((3, 3)));
    assert!(c.names.is_empty() && c.mana_cost.is_none() && c.subtypes.is_empty());
    let env = env.counter(o(1), "+1/+1", 1);
    assert_eq!(pt(&eval(&r, &env, o(1))), Some((4, 4)));
    // Copiable values ignore layers 2-7 (CR 707.2).
    assert_eq!(
        copiable_characteristics(&r, &env, o(1)).unwrap(),
        profile_chars(&profile)
    );
}

// ---------------------------------------------------------------------------------------------
// Merged permanents (CR 731.2a, 731.2e, 731.2f, 731.2g, 731.2j)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-003, REQ-OBJ-002; CR 731.2a, 613.2a, 613.2b, 708.8: a merge effect (1a) and a
// face-down effect (1b) on the merged object give the profile in both timestamp orders; after
// turning face up it shows the topmost component's characteristics. A face-down lower component
// contributes nothing (DECISIONS "#39 FP-038 — Face-down merged components").
#[test]
fn req_obj_003_merged_permanent_face_down_both_orders() {
    let top_d = normal("top", "{1}{G}", (3, 3), "Bear");
    let bottom_d = normal("bottom", "{5}{B}", (6, 6), "Golem");
    let m = o(1);
    let env = Env::default().with(m, printed(&bottom_d));
    let profile = FaceDownProfile::default_2_2();
    for bottom_face_up in [true, false] {
        let merged = MergedPermanent::new(
            vec![
                component(&top_d, true),
                component(&bottom_d, bottom_face_up),
            ],
            false,
        )
        .unwrap();
        for (merge_ts, fd_ts) in [(10, 20), (20, 10)] {
            let mut r = registry(vec![
                (
                    1,
                    merge_effect(&merged, vocab(), m, P1, ts(merge_ts)).unwrap(),
                ),
                (2, fd(&profile, m, fd_ts)),
            ]);
            assert_eq!(eval(&r, &env, m), profile_chars(&profile));
            assert_eq!(
                copiable_characteristics(&r, &env, m).unwrap(),
                profile_chars(&profile)
            );
            let (mut l, mut g) = clock(30);
            turn_face_up(&mut r, e(2), m, false, &mut l, &mut g).unwrap();
            // CR 731.2a: only the topmost component's characteristics.
            let shown = eval(&r, &env, m);
            assert_eq!(shown, printed(&top_d));
            assert!(!format!("{shown:?}").contains("bottom"));
            assert!(!shown.subtypes.contains("Golem"));
        }
    }
}

// REQ-OBJ-003; CR 731.2j, 731.2g: the merged-permanent refusals.
#[test]
fn req_obj_003_merged_refusals() {
    let mut r = ContinuousEffects::new();
    let (mut l, mut g) = clock(0);
    let target = FaceDownTarget {
        merged_contains_double_faced: true,
        ..FaceDownTarget::default()
    };
    assert_eq!(
        down(&mut r, 1, target, o(1), &mut l, &mut g),
        Err(FaceDownRefusal::MergedContainsDoubleFaced)
    );
    r.register(e(1), fd(&FaceDownProfile::default_2_2(), o(1), 1))
        .unwrap();
    assert_eq!(
        turn_face_up(&mut r, e(1), o(1), true, &mut l, &mut g),
        Err(FaceUpRefusal::InstantOrSorcery)
    );
    assert!(r.get(e(1)).is_some());
}

// ---------------------------------------------------------------------------------------------
// turn_face_down (CR 708.2a, 708.2b, 712.13, 731.2j, 613.7f)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-003; CR 708.2a, 613.7f: success stamps a strictly newer timestamp, registers the
// face-down effect with it, and the object evaluates to the profile.
#[test]
fn req_obj_003_turn_face_down_success() {
    let env = Env::default().with(o(1), card("hidden"));
    let mut r = registry(vec![(1, copy(&other("orig"), o(1), 3))]);
    let (mut l, mut g) = clock(4);
    l.stamp_object(&mut g, o(1), StampReason::EntersZone)
        .unwrap();
    let before = l.object_timestamp(o(1)).unwrap();
    let t = down(&mut r, 2, FaceDownTarget::default(), o(1), &mut l, &mut g).unwrap();
    assert!(t > before);
    assert_eq!(l.object_timestamp(o(1)), Some(t));
    assert_eq!(g.last_issued(), t.get());
    assert_eq!(
        r.get(e(2)),
        Some(&fd(&FaceDownProfile::default_2_2(), o(1), t.get()))
    );
    assert_eq!(
        eval(&r, &env, o(1)),
        FaceDownProfile::default_2_2().characteristics().clone()
    );
    // The ward profile through turn_face_down.
    let mut r2 = ContinuousEffects::new();
    turn_face_down(
        &mut r2,
        e(1),
        &FaceDownProfile::with_ward_2(),
        &FaceDownTarget::default(),
        Some(o(5)),
        P2,
        EffectOrigin::Resolved,
        o(1),
        &mut l,
        &mut g,
    )
    .unwrap();
    let eff = r2.get(e(1)).unwrap();
    assert_eq!((eff.source, eff.controller), (Some(o(5)), P2));
    assert!(eval(&r2, &env, o(1)).abilities == vec![ward_2()]);
}

// REQ-OBJ-003; CR 708.2b, 712.13, 731.2j: each refusal, and every combination, changes nothing;
// the checks run in the order AlreadyFaceDown, DoubleFacedPermanent, MergedContainsDoubleFaced.
#[test]
fn req_obj_003_turn_face_down_refusals_and_order() {
    for bits in 1u8..8 {
        let target = FaceDownTarget {
            already_face_down: bits & 1 != 0,
            double_faced_permanent: bits & 2 != 0,
            merged_contains_double_faced: bits & 4 != 0,
        };
        let want = if target.already_face_down {
            FaceDownRefusal::AlreadyFaceDown
        } else if target.double_faced_permanent {
            FaceDownRefusal::DoubleFacedPermanent
        } else {
            FaceDownRefusal::MergedContainsDoubleFaced
        };
        let mut r = registry(vec![(1, copy(&other("orig"), o(1), 3))]);
        let (mut l, mut g) = clock(4);
        l.stamp_object(&mut g, o(1), StampReason::EntersZone)
            .unwrap();
        let (r0, l0, g0) = (r.clone(), l.clone(), g.clone());
        assert_eq!(
            down(&mut r, 2, target, o(1), &mut l, &mut g),
            Err(want.clone()),
            "{target:?}"
        );
        assert_eq!((&r, &l, &g), (&r0, &l0, &g0));
        assert!(!want.to_string().is_empty());
    }
}

// REQ-OBJ-003, REQ-CORE-006; CR 613.7f: a failure of stamping (exhausted generator) or of
// registration (duplicate effect id) leaves everything unchanged.
#[test]
fn req_obj_003_turn_face_down_layer_errors_change_nothing() {
    // Registration fails after stamping.
    let mut r = registry(vec![(1, copy(&other("orig"), o(1), 3))]);
    let (mut l, mut g) = clock(4);
    let (r0, l0, g0) = (r.clone(), l.clone(), g.clone());
    let err = down(&mut r, 1, FaceDownTarget::default(), o(1), &mut l, &mut g).unwrap_err();
    assert_eq!(
        err,
        FaceDownRefusal::Layer(LayerError::DuplicateEffect(e(1)))
    );
    assert_eq!((&r, &l, &g), (&r0, &l0, &g0));
    assert!(!err.to_string().is_empty());
    // Stamping fails.
    let (mut l, mut g) = (
        TimestampLedger::new(),
        IdGenerator::from_last_issued(u64::MAX),
    );
    let (l0, g0) = (l.clone(), g.clone());
    assert_eq!(
        down(&mut r, 2, FaceDownTarget::default(), o(1), &mut l, &mut g),
        Err(FaceDownRefusal::Layer(LayerError::IdExhausted))
    );
    assert_eq!((&r, &l, &g), (&r0, &l0, &g0));
}

// ---------------------------------------------------------------------------------------------
// turn_face_up (CR 708.8, 708.10, 613.7f, 701.40g, 701.58g, 731.2g)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-003; CR 708.8, 613.7f: turning face up restores the base copiable values with a new
// timestamp strictly greater than the face-down effect's and than a copy registered in between;
// effects applied while face down (layers 6 and 7c, counters) still apply after.
#[test]
fn req_obj_003_turn_face_up_restores_with_new_timestamp() {
    let env = Env::default()
        .with(o(1), card("hidden"))
        .counter(o(1), "+1/+1", 1);
    let mut r = ContinuousEffects::new();
    let (mut l, mut g) = clock(0);
    let t_down = down(&mut r, 1, FaceDownTarget::default(), o(1), &mut l, &mut g).unwrap();
    let granted = keyword_ability("t", "REACH");
    let t_l6 = stamp_resolved_effect(&mut g).unwrap();
    r.register(
        e(2),
        plain(
            Layer::L6,
            t_l6.get(),
            o(1),
            LayerModification::AddAbility(granted.clone()),
        ),
    )
    .unwrap();
    let t_l7 = stamp_resolved_effect(&mut g).unwrap();
    r.register(
        e(3),
        plain(
            Layer::L7c,
            t_l7.get(),
            o(1),
            LayerModification::ModifyPt {
                power: 2,
                toughness: 0,
            },
        ),
    )
    .unwrap();
    assert_eq!(pt(&eval(&r, &env, o(1))), Some((5, 3)));
    let t_up = turn_face_up(&mut r, e(1), o(1), false, &mut l, &mut g).unwrap();
    assert!(t_up > t_down && t_up > t_l6 && t_up > t_l7);
    assert_eq!(l.object_timestamp(o(1)), Some(t_up));
    assert!(r.get(e(1)).is_none());
    assert!(r.get(e(2)).is_some() && r.get(e(3)).is_some());
    // CR 708.8: the copiable values revert; the other effects still apply.
    assert_eq!(
        copiable_characteristics(&r, &env, o(1)).unwrap(),
        card("hidden")
    );
    let c = eval(&r, &env, o(1));
    assert_eq!(c.names, card("hidden").names);
    assert_eq!(c.abilities, vec![etb_ability("hidden"), granted]);
    assert_eq!(pt(&c), Some((8, 6)));
}

// REQ-OBJ-003; CR 708.10, 613.7f: a face-down permanent that becomes a copy keeps the profile;
// turned face up it shows the copied values; its new timestamp is later than the copy's.
#[test]
fn req_obj_003_copy_then_face_down_then_face_up_shows_copy() {
    let env = Env::default().with(o(1), card("hidden"));
    let mut r = ContinuousEffects::new();
    let (mut l, mut g) = clock(0);
    down(&mut r, 1, FaceDownTarget::default(), o(1), &mut l, &mut g).unwrap();
    let t_copy = stamp_resolved_effect(&mut g).unwrap();
    r.register(e(2), copy(&other("orig"), o(1), t_copy.get()))
        .unwrap();
    assert_eq!(
        eval(&r, &env, o(1)),
        FaceDownProfile::default_2_2().characteristics().clone()
    );
    let t_up = turn_face_up(&mut r, e(1), o(1), false, &mut l, &mut g).unwrap();
    assert!(t_up > t_copy);
    assert_eq!(eval(&r, &env, o(1)), other("orig"));
    assert_eq!(
        copiable_characteristics(&r, &env, o(1)).unwrap(),
        other("orig")
    );
    // And with the copy registered before turning face down.
    let mut r = registry(vec![(5, copy(&other("orig"), o(1), 1))]);
    let (mut l, mut g) = clock(1);
    down(&mut r, 6, FaceDownTarget::default(), o(1), &mut l, &mut g).unwrap();
    turn_face_up(&mut r, e(6), o(1), false, &mut l, &mut g).unwrap();
    assert_eq!(eval(&r, &env, o(1)), other("orig"));
}

// REQ-OBJ-003; CR 708.8: NotFaceDown for an unknown id, a non-FaceDown effect, a FaceDown effect
// on another object and a FaceDown effect with a selector; nothing changes.
#[test]
fn req_obj_003_turn_face_up_not_face_down() {
    let selector = ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(PredicateAST::IsCreature)),
    });
    let mut selected = fd(&FaceDownProfile::default_2_2(), o(1), 4);
    selected.affected = Affected::Selector(selector);
    let mut r = registry(vec![
        (1, copy(&other("orig"), o(1), 1)),
        (2, fd(&FaceDownProfile::default_2_2(), o(2), 2)),
        (3, selected),
    ]);
    let (mut l, mut g) = clock(9);
    let (r0, l0, g0) = (r.clone(), l.clone(), g.clone());
    for id in [1, 2, 3, 99] {
        for instant_or_sorcery in [false, true] {
            let err =
                turn_face_up(&mut r, e(id), o(1), instant_or_sorcery, &mut l, &mut g).unwrap_err();
            assert_eq!(err, FaceUpRefusal::NotFaceDown, "{id}");
            assert!(!err.to_string().is_empty());
            assert_eq!((&r, &l, &g), (&r0, &l0, &g0));
        }
    }
}

// REQ-OBJ-003; CR 701.40g, 701.58g, 731.2g: an instant or sorcery stays face down: no timestamp,
// the effect stays, nothing changes.
#[test]
fn req_obj_003_turn_face_up_instant_or_sorcery_stays_down() {
    let env = Env::default().with(o(1), card("hidden"));
    let mut r = ContinuousEffects::new();
    let (mut l, mut g) = clock(0);
    down(&mut r, 1, FaceDownTarget::default(), o(1), &mut l, &mut g).unwrap();
    let (r0, l0, g0) = (r.clone(), l.clone(), g.clone());
    let err = turn_face_up(&mut r, e(1), o(1), true, &mut l, &mut g).unwrap_err();
    assert_eq!(err, FaceUpRefusal::InstantOrSorcery);
    assert!(!err.to_string().is_empty());
    assert_eq!((&r, &l, &g), (&r0, &l0, &g0));
    assert_eq!(
        eval(&r, &env, o(1)),
        FaceDownProfile::default_2_2().characteristics().clone()
    );
}

// REQ-OBJ-003, REQ-CORE-006; CR 613.7f: an exhausted generator refuses with Layer(IdExhausted)
// and changes nothing.
#[test]
fn req_obj_003_turn_face_up_exhausted_changes_nothing() {
    let mut r = registry(vec![(1, fd(&FaceDownProfile::default_2_2(), o(1), 1))]);
    let (mut l, mut g) = (
        TimestampLedger::new(),
        IdGenerator::from_last_issued(u64::MAX),
    );
    let (r0, l0, g0) = (r.clone(), l.clone(), g.clone());
    let err = turn_face_up(&mut r, e(1), o(1), false, &mut l, &mut g).unwrap_err();
    assert_eq!(err, FaceUpRefusal::Layer(LayerError::IdExhausted));
    assert!(!err.to_string().is_empty());
    assert_eq!((&r, &l, &g), (&r0, &l0, &g0));
}

// REQ-OBJ-003; CR 708.8: a FaceDown effect whose locked set holds several objects turns only the
// named object face up; the others stay face down under the same effect id and timestamp.
#[test]
fn req_obj_003_turn_face_up_narrows_shared_effect() {
    let env = Env::default()
        .with(o(1), card("one"))
        .with(o(2), card("two"));
    let mut shared = fd(&FaceDownProfile::default_2_2(), o(1), 3);
    shared.affected = Affected::Objects(objs(&[o(1), o(2)]));
    let mut r = registry(vec![(1, shared.clone())]);
    let (mut l, mut g) = clock(3);
    turn_face_up(&mut r, e(1), o(1), false, &mut l, &mut g).unwrap();
    assert_eq!(eval(&r, &env, o(1)), card("one"));
    assert_eq!(
        eval(&r, &env, o(2)),
        FaceDownProfile::default_2_2().characteristics().clone()
    );
    let left = r.get(e(1)).unwrap();
    assert_eq!(left.affected, Affected::Objects(objs(&[o(2)])));
    assert_eq!(left.timestamp, shared.timestamp);
}

/// A shared face-down group: the layer-1b anchor (effect 1) on `{o(1), o(2)}` and a layer-7c
/// `SameAsLayer(L1b)` part (effect 2) adding +1/+1 (CR 613.6).
fn shared_group(r: &mut ContinuousEffects, env: &Env) {
    let mut anchor = fd(&FaceDownProfile::default_2_2(), o(1), 3);
    anchor.affected = Affected::Objects(objs(&[o(1), o(2)]));
    let mut part = plain(
        Layer::L7c,
        3,
        o(1),
        LayerModification::ModifyPt {
            power: 1,
            toughness: 1,
        },
    );
    part.affected = Affected::SameAsLayer(Layer::L1b);
    r.register_group(env, vec![(e(1), anchor), (e(2), part)])
        .unwrap();
}

// REQ-OBJ-003, REQ-LAYER-001, REQ-LAYER-002; CR 613.6, 708.8: turning one object of a shared
// face-down effect up keeps the links of the group's other parts, so the object that stays face
// down still gets the linked +1/+1 part; the object turned up gets neither.
#[test]
fn req_obj_003_613_6_turn_up_one_of_shared_effect_keeps_group_links() {
    let env = Env::default()
        .with(o(1), card("one"))
        .with(o(2), card("two"));
    let mut r = ContinuousEffects::new();
    shared_group(&mut r, &env);
    assert_eq!(pt(&eval(&r, &env, o(1))), Some((3, 3)));
    assert_eq!(pt(&eval(&r, &env, o(2))), Some((3, 3)));
    let links_before = r.links().clone();
    assert_eq!(links_before.get(&e(2)), Some(&e(1)));
    let (mut l, mut g) = clock(3);
    let t_up = turn_face_up(&mut r, e(1), o(1), false, &mut l, &mut g).unwrap();
    assert!(t_up > ts(3));
    assert_eq!(l.object_timestamp(o(1)), Some(t_up));
    assert_eq!(r.links(), &links_before);
    // A is face up and outside the anchor: its base values, no linked part.
    assert_eq!(eval(&r, &env, o(1)), card("one"));
    // B stays face down with the linked part still applying.
    let b = eval(&r, &env, o(2));
    assert_eq!(pt(&b), Some((3, 3)));
    assert_eq!(
        b.names,
        FaceDownProfile::default_2_2().characteristics().names
    );
    // Turning B up removes the anchor and its links; B is at its base values.
    turn_face_up(&mut r, e(1), o(2), false, &mut l, &mut g).unwrap();
    assert!(r.get(e(1)).is_none());
    assert!(r.links().is_empty());
    assert_eq!(eval(&r, &env, o(2)), card("two"));
}

// REQ-OBJ-003; CR 613.6, 708.8: `narrow_objects` on an unknown id is an error and changes
// nothing.
#[test]
fn req_obj_003_narrow_objects_unknown_effect() {
    let env = Env::default()
        .with(o(1), card("one"))
        .with(o(2), card("two"));
    let mut r = ContinuousEffects::new();
    shared_group(&mut r, &env);
    let before = r.clone();
    assert_eq!(
        r.narrow_objects(e(9), objs(&[o(1)])),
        Err(LayerError::UnknownEffect(e(9)))
    );
    assert_eq!(r, before);
}

// REQ-OBJ-003; CR 613.6: `narrow_objects` refuses an effect that is not a locked object set (a
// `SameAsLayer` part) and changes nothing.
#[test]
fn req_obj_003_narrow_objects_needs_objects() {
    let env = Env::default()
        .with(o(1), card("one"))
        .with(o(2), card("two"));
    let mut r = ContinuousEffects::new();
    shared_group(&mut r, &env);
    let before = r.clone();
    assert_eq!(
        r.narrow_objects(e(2), objs(&[o(1)])),
        Err(LayerError::Unsupported(
            "narrow_objects needs an Affected::Objects effect"
        ))
    );
    assert_eq!(r, before);
}

// REQ-OBJ-003; CR 613.6, 708.8: `narrow_objects` replaces only the object set; id, timestamp,
// every other field and the links stay.
#[test]
fn req_obj_003_narrow_objects_keeps_everything_else() {
    let env = Env::default()
        .with(o(1), card("one"))
        .with(o(2), card("two"));
    let mut r = ContinuousEffects::new();
    shared_group(&mut r, &env);
    let before = r.get(e(1)).unwrap().clone();
    let links = r.links().clone();
    r.narrow_objects(e(1), objs(&[o(2)])).unwrap();
    let after = r.get(e(1)).unwrap();
    assert_eq!(after.affected, Affected::Objects(objs(&[o(2)])));
    assert_eq!(
        after,
        &ContinuousEffect {
            affected: Affected::Objects(objs(&[o(2)])),
            ..before
        }
    );
    assert_eq!(r.links(), &links);
    assert_eq!(r.len(), 2);
}

// ---------------------------------------------------------------------------------------------
// Redaction and LKI (REQ-CORE-004, CR 708.5, 603.10)
// ---------------------------------------------------------------------------------------------

// REQ-OBJ-003, REQ-CORE-004; CR 708.2, 708.5: the evaluated face-down object contains none of
// its face-up name, cost, subtypes, rules text or abilities anywhere in its Debug output; an
// opponent's projection has no card for it, the controller's has.
#[test]
fn req_core_004_face_down_redaction() {
    let mut secret = card("secretcard");
    secret.mana_cost = Some(cost("{7}{B}{B}"));
    let env = Env::default().with(o(1), secret.clone());
    for profile in [
        FaceDownProfile::default_2_2(),
        FaceDownProfile::with_ward_2(),
    ] {
        let r = registry(vec![(1, fd(&profile, o(1), 5))]);
        let shown = format!("{:?}", evaluate(&r, &env, o(1)).unwrap().characteristics);
        for leak in [
            "test.secretcard",
            "Subsecretcard",
            "test.text.secretcard",
            "test.secretcard.etb",
            "ENTERS_BATTLEFIELD",
            "Legendary",
        ] {
            assert!(!shown.contains(leak), "{leak} leaked: {shown}");
        }
        let c = eval(&r, &env, o(1));
        assert_eq!(c.mana_cost, None);
        assert!(!shown.contains(&format!("{:?}", secret.mana_cost.as_ref().unwrap())));
        // LayeredProvider (LKI source) sees the same.
        let provider = LayeredProvider::new(&r, &env);
        assert_eq!(provider.characteristics(o(1)), Some(c));
    }
    // The projection: a face-down permanent controlled by P1.
    let mut zones = Zones::new(&[P1, P2], ZonesConfig::default());
    let mut ids = IdGenerator::new();
    let id = zones
        .create_object(
            &mut ids,
            CardId::new(42),
            P1,
            MoveRequest {
                face_down: true,
                ..MoveRequest::new(ZoneKind::Battlefield)
            },
        )
        .unwrap();
    let k = Knowledge::new();
    let opponent = k.project(&zones, P2).unwrap();
    assert_eq!(opponent.battlefield.len(), 1);
    assert_eq!(opponent.battlefield[0].id, id);
    assert!(opponent.battlefield[0].face_down);
    assert_eq!(opponent.battlefield[0].card, None);
    let controller = k.project(&zones, P1).unwrap();
    assert!(controller.battlefield[0].face_down);
    assert_eq!(controller.battlefield[0].card, Some(CardId::new(42)));
}

// REQ-OBJ-003; CR 603.10, 708.2: a face-down permanent looked at just before it leaves the
// battlefield (the face-down effect still registered) has the profile, not the card's
// characteristics; after the effect is removed the card shows.
#[test]
fn req_obj_003_lki_before_leaving_is_the_profile() {
    let env = Env::default().with(o(1), card("hidden"));
    let profile = FaceDownProfile::with_ward_2();
    let mut r = registry(vec![(1, fd(&profile, o(1), 5))]);
    let lki = LayeredProvider::new(&r, &env).characteristics(o(1));
    assert_eq!(lki, Some(profile_chars(&profile)));
    assert!(
        LayeredProvider::new(&r, &env)
            .characteristics(o(9))
            .is_none()
    );
    r.remove(e(1));
    assert_eq!(
        LayeredProvider::new(&r, &env).characteristics(o(1)),
        Some(card("hidden"))
    );
}

// ---------------------------------------------------------------------------------------------
// Determinism and properties (REQ-CORE-006)
// ---------------------------------------------------------------------------------------------

/// A scenario built by the public operations: two objects, one turned face down, one copy.
fn scenario() -> (ContinuousEffects, Env) {
    let env = Env::default()
        .with(o(1), card("a"))
        .with(o(2), other("b"))
        .counter(o(1), "+1/+1", 2);
    let mut r = ContinuousEffects::new();
    let (mut l, mut g) = clock(0);
    down(&mut r, 1, FaceDownTarget::default(), o(1), &mut l, &mut g).unwrap();
    let values = copiable_values(&r, &env, o(1)).unwrap();
    let t = stamp_resolved_effect(&mut g).unwrap();
    r.register(
        e(2),
        copiable_effect(&values, None, P1, EffectOrigin::Resolved, t, objs(&[o(2)])),
    )
    .unwrap();
    (r, env)
}

// REQ-OBJ-003, REQ-LAYER-005, REQ-CORE-006: the same operations twice give the same digest; the
// cache (or full recompute under the feature) agrees with plain evaluation, including a cache
// warmed on the face-up state and reused after turning face down and face up.
#[test]
fn req_core_006_face_down_deterministic_and_cache_agrees() {
    let (r1, env1) = scenario();
    let (r2, env2) = scenario();
    let a = evaluate_all(&r1, &env1).unwrap();
    let b = evaluate_all(&r2, &env2).unwrap();
    assert_eq!(evaluated_digest(&a), evaluated_digest(&b));
    let mut cache = LayerCache::new();
    let first = cache.evaluate_all(&r1, &env1).unwrap();
    let second = cache.evaluate_all(&r1, &env1).unwrap();
    assert_eq!(evaluated_digest(&first), evaluated_digest(&a));
    assert_eq!(evaluated_digest(&second), evaluated_digest(&a));
    assert_eq!(
        a[&o(2)].characteristics,
        profile_chars(&FaceDownProfile::default_2_2())
    );

    // A cache warmed face up, then reused across turn_face_down and turn_face_up (CR 708.8).
    let env = Env::default()
        .with(o(1), card("a"))
        .with(o(2), other("b"))
        .counter(o(1), "+1/+1", 2);
    let mut r = ContinuousEffects::new();
    let (mut l, mut g) = clock(0);
    let mut cache = LayerCache::new();
    let warm = cache.evaluate_all(&r, &env).unwrap();
    assert_eq!(warm, evaluate_all(&r, &env).unwrap());
    down(&mut r, 1, FaceDownTarget::default(), o(1), &mut l, &mut g).unwrap();
    let cached = cache.evaluate_all(&r, &env).unwrap();
    let fresh = evaluate_all(&r, &env).unwrap();
    assert_eq!(cached, fresh);
    assert_eq!(evaluated_digest(&cached), evaluated_digest(&fresh));
    assert_eq!(
        fresh[&o(1)].characteristics.names,
        FaceDownProfile::default_2_2().characteristics().names
    );
    turn_face_up(&mut r, e(1), o(1), false, &mut l, &mut g).unwrap();
    let cached = cache.evaluate_all(&r, &env).unwrap();
    let fresh = evaluate_all(&r, &env).unwrap();
    assert_eq!(cached, fresh);
    assert_eq!(evaluated_digest(&cached), evaluated_digest(&fresh));
    assert_eq!(fresh[&o(1)].characteristics.names, card("a").names);
}

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

/// Random synthetic characteristics, every hideable field non-empty.
fn random_chars(rng: &mut Lcg, tag: u64) -> Characteristics {
    let costs = ["{1}", "{2}{G}", "{X}{U}", "{W}{B}", "{0}"];
    let mut c = if rng.below(2) == 0 {
        card(&format!("r{tag}"))
    } else {
        other(&format!("r{tag}"))
    };
    c.mana_cost = Some(cost(costs[rng.below(costs.len())]));
    for k in 0..rng.below(3) {
        c.abilities.push(test_ability(&format!("test.r{tag}.{k}")));
    }
    c
}

// REQ-OBJ-003, REQ-CORE-006; CR 613.2b, 613.2c, 708.2: property over 300 random cases: for
// random bases, random layer-1a copies and a face-down effect registered in a random order with
// random timestamps and ids, the evaluated object's names, mana cost, subtypes, rules text and
// abilities never hold any base or copied value: they are the profile's.
#[test]
fn property_face_down_hides_everything() {
    let mut rng = Lcg(0x00FA_CED0);
    for case in 0..300u64 {
        let base = random_chars(&mut rng, case);
        let env = Env::default().with(o(1), base.clone());
        let profile = if rng.below(2) == 0 {
            FaceDownProfile::default_2_2()
        } else {
            FaceDownProfile::with_ward_2()
        };
        let mut list: Vec<ContinuousEffect> = (0..rng.below(4))
            .map(|k| {
                let t = u64::try_from(rng.below(50)).unwrap() + 1;
                copy(
                    &random_chars(&mut rng, 1000 + case * 10 + k as u64),
                    o(1),
                    t,
                )
            })
            .collect();
        let fd_ts = u64::try_from(rng.below(50)).unwrap() + 1;
        list.push(fd(&profile, o(1), fd_ts));
        rng.shuffle(&mut list);
        let r = registry(
            list.into_iter()
                .enumerate()
                .map(|(i, x)| (i as u64 + 1, x))
                .collect(),
        );
        let want = profile.characteristics();
        for c in [
            eval(&r, &env, o(1)),
            copiable_characteristics(&r, &env, o(1)).unwrap(),
        ] {
            assert_eq!(c.names, want.names, "case {case}");
            assert_eq!(c.mana_cost, want.mana_cost, "case {case}");
            assert_eq!(c.subtypes, want.subtypes, "case {case}");
            assert_eq!(c.rules_text, want.rules_text, "case {case}");
            assert_eq!(c.abilities, want.abilities, "case {case}");
            for name in &base.names {
                assert!(!format!("{c:?}").contains(name.as_str()), "case {case}");
            }
        }
    }
}

// REQ-OBJ-003, REQ-OBJ-002, REQ-CORE-006; CR 708.8, 708.10, 613.7f: property over 200 random
// cases: turning face down then face up is the identity on copiable values (and provenance),
// whatever copy effects were registered before; each turn issues a new, later timestamp.
#[test]
fn property_down_then_up_is_identity_on_copiable_values() {
    let mut rng = Lcg(0x5EED_0039);
    for case in 0..200u64 {
        let env = Env::default().with(o(1), random_chars(&mut rng, case));
        let mut g = IdGenerator::<Timestamp>::new();
        let mut l = TimestampLedger::new();
        let mut r = ContinuousEffects::new();
        for k in 0..rng.below(4) as u64 {
            let t = stamp_resolved_effect(&mut g).unwrap();
            let mut values = CopiableValues::plain(random_chars(&mut rng, 500 + case * 10 + k));
            if rng.below(3) == 0 {
                values.source = ValueSource::Fixed {
                    mana_value: i64::try_from(rng.below(9)).unwrap(),
                };
            }
            r.register(
                e(10 + k),
                copiable_effect(&values, None, P1, EffectOrigin::Resolved, t, objs(&[o(1)])),
            )
            .unwrap();
        }
        let before = copiable_values(&r, &env, o(1)).unwrap();
        let profile = FaceDownProfile::for_cause(
            [
                FaceDownCause::Morph,
                FaceDownCause::Disguise,
                FaceDownCause::Cloak,
                FaceDownCause::Manifest,
            ][rng.below(4)],
        );
        let t_down = turn_face_down(
            &mut r,
            e(1),
            &profile,
            &FaceDownTarget::default(),
            None,
            P1,
            EffectOrigin::Resolved,
            o(1),
            &mut l,
            &mut g,
        )
        .unwrap();
        assert_eq!(
            copiable_characteristics(&r, &env, o(1)).unwrap(),
            profile_chars(&profile),
            "case {case}"
        );
        let t_up = turn_face_up(&mut r, e(1), o(1), false, &mut l, &mut g).unwrap();
        assert!(t_up > t_down, "case {case}");
        let after = copiable_values(&r, &env, o(1)).unwrap();
        assert_eq!(after.characteristics, before.characteristics, "case {case}");
        assert_eq!(after.provenance(), before.provenance(), "case {case}");
    }
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

/// A synthetic neutral ability (a STATIC ability with one RULE `SKIP` effect) with ability id
/// `id` (R5; #232: replaces the former placeholder abilities).
fn test_ability(id: &str) -> AbilityAST {
    ability_json(serde_json::json!({
        "abilityId": id,
        "kind": "STATIC",
        "effects": [{"type": "RULE", "kind": "SKIP"}],
    }))
}
