// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit and property tests of the layer components of the game state: registration with
//! generating-ability links, the 7a boundary, the timestamp wrappers, hashing, the version-2
//! wire form and rollback (REQ-LAYER-001, REQ-LAYER-002, REQ-CORE-001, REQ-CORE-002,
//! REQ-CORE-006, REQ-NFR-006, REQ-DSL-004, R12). Synthetic cards and neutral names only;
//! deterministic SplitMix64 harness with literal seeds.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::encode_to_vec;
use fineprint_base::hash::{ComponentPath, Label, diff_first_divergence};
use fineprint_base::ids::{EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};
use fineprint_base::prng::SplitMix64;
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};
use fineprint_dsl::expr::{ObjectSelectorAST, PredicateAST, SelectorFilter};
use fineprint_dsl::text::RewritableCategory;

use super::wire::StateWire;
use super::*;
use crate::layers::{
    Affected, ContinuousEffect, ContinuousEffects, CopiedCharacteristic, CopyException,
    CounterHolder, EffectOrigin, LayerEnvironment, LayerError, LayerModification, MatchContext,
    ObjectBase, SimultaneousStamp, StampReason, ValueSource, literal, provenance_from_parts,
};
use crate::objects::Characteristics;
use crate::txn::Transactions;

// ---------------------------------------------------------------------------------------------
// Fixtures (shared with environment_tests)
// ---------------------------------------------------------------------------------------------

pub(super) fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub(super) fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

pub(super) fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

pub(super) fn t(n: u64) -> Timestamp {
    Timestamp::new(n)
}

pub(super) fn set<const N: usize>(items: [&str; N]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A synthetic non-keyword (STATIC) ability `test.ability.<tag>`.
pub(super) fn ability(tag: &str) -> AbilityAST {
    serde_json::from_value(serde_json::json!({
        "kind": "STATIC",
        "abilityId": format!("test.ability.{tag}"),
        "effects": [{"type": "RULE", "kind": "SKIP"}],
    }))
    .expect("valid synthetic ability")
}

/// A one-token synthetic rules text whose literal is `text`.
pub(super) fn literal_text(text: &str) -> TextTokenStream {
    serde_json::from_value(serde_json::json!([{"id": "t1", "kind": "LITERAL", "text": text}]))
        .expect("valid synthetic text")
}

/// A synthetic 2/2 creature `test.state_creature` with `abilities`.
pub(super) fn creature(abilities: Vec<AbilityAST>) -> Characteristics {
    Characteristics {
        names: set(["test.state_creature"]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::GREEN,
        supertypes: BTreeSet::new(),
        card_types: set(["Creature"]),
        subtypes: set(["Testkin"]),
        rules_text: TextTokenStream::default(),
        abilities,
        power: Some(literal(2)),
        toughness: Some(literal(2)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A filter selector with predicate `pred`.
pub(super) fn selector(pred: PredicateAST) -> ObjectSelectorAST {
    ObjectSelectorAST::Filter(SelectorFilter {
        zones: None,
        count: None,
        ordering: None,
        predicate: Some(Box::new(pred)),
    })
}

/// The locked set of these objects.
pub(super) fn only(ids: &[u64]) -> Affected {
    Affected::Objects(ids.iter().map(|&n| o(n)).collect())
}

/// A non-CDA effect of a static ability printed on `source` (`granted_at: None`).
pub(super) fn static_effect(
    source: ObjectId,
    timestamp: Timestamp,
    layer: Layer,
    affected: Affected,
    modification: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source: Some(source),
        controller: p(1),
        timestamp,
        layer,
        is_cda: false,
        origin: EffectOrigin::Static { granted_at: None },
        affected,
        modification,
    }
}

/// A non-CDA effect of a resolved spell or ability, without a source.
pub(super) fn resolved_effect(
    timestamp: Timestamp,
    layer: Layer,
    affected: Affected,
    modification: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: p(1),
        timestamp,
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected,
        modification,
    }
}

fn below(rng: &mut SplitMix64, n: u64) -> u64 {
    rng.next_u64() % n
}

fn pick<T: Clone>(rng: &mut SplitMix64, items: &[T]) -> T {
    items[usize::try_from(below(rng, items.len() as u64)).unwrap()].clone()
}

fn any_set(rng: &mut SplitMix64) -> BTreeSet<String> {
    (0..below(rng, 3))
        .map(|_| format!("Test{}", below(rng, 5)))
        .collect()
}

fn any_characteristics(rng: &mut SplitMix64) -> Characteristics {
    let mut c = creature(
        (0..below(rng, 3))
            .map(|i| ability(&format!("c{i}")))
            .collect(),
    );
    c.names = set(["test.copy_source"]);
    c.subtypes = any_set(rng);
    c.colors = ColorBitmask::new(u8::try_from(below(rng, 32)).unwrap()).unwrap();
    c.loyalty = (below(rng, 2) == 0).then(|| rng.next_u64().cast_signed());
    c.mana_cost = (below(rng, 2) == 0).then(|| "{2}{G}".parse().unwrap());
    c
}

fn any_copy_exception(rng: &mut SplitMix64) -> CopyException {
    match below(rng, 13) {
        0 => CopyException::GainAbility(ability("gain")),
        1 => CopyException::SetNames(set(["test.renamed"])),
        2 => CopyException::SetManaCost(Some("{1}".parse().unwrap())),
        3 => CopyException::SetColors(ColorBitmask::RED),
        4 => CopyException::SetSupertypes(any_set(rng)),
        5 => CopyException::SetCardTypes(set(["Artifact"])),
        6 => CopyException::SetSubtypes(any_set(rng)),
        7 => CopyException::AddSupertypes(any_set(rng)),
        8 => CopyException::AddCardTypes(set(["Creature"])),
        9 => CopyException::AddSubtypes(any_set(rng)),
        10 => CopyException::SetPower(literal(rng.next_u64().cast_signed() % 9)),
        11 => CopyException::SetToughness(literal(3)),
        _ => CopyException::NotCopied(pick(
            rng,
            &[
                CopiedCharacteristic::Names,
                CopiedCharacteristic::Colors,
                CopiedCharacteristic::LifeModifier,
            ],
        )),
    }
}

/// A random modification with a layer it belongs to; every variant is reachable (7a is left to
/// characteristic-defining effects).
pub(super) fn any_modification(rng: &mut SplitMix64) -> (Layer, LayerModification) {
    match below(rng, 18) {
        0 => (
            Layer::L1a,
            LayerModification::CopyValues(Box::new(any_characteristics(rng))),
        ),
        1 => (
            Layer::L1a,
            LayerModification::CopyValuesWithProvenance {
                characteristics: Box::new(any_characteristics(rng)),
                provenance: provenance_from_parts(
                    ValueSource::Fixed {
                        mana_value: rng.next_u64().cast_signed() % 20,
                    },
                    below(rng, 2) == 0,
                    below(rng, 2) == 0,
                )
                .unwrap(),
            },
        ),
        2 => (
            Layer::L1a,
            LayerModification::ApplyCopyExceptions(
                (0..below(rng, 3))
                    .map(|_| any_copy_exception(rng))
                    .collect(),
            ),
        ),
        3 => (
            Layer::L1b,
            LayerModification::FaceDown(Box::new(any_characteristics(rng))),
        ),
        4 => (
            Layer::L2,
            LayerModification::SetController(p(1 + below(rng, 3))),
        ),
        5 => (
            Layer::L3,
            LayerModification::SetText(literal_text(&format!("w{}", below(rng, 9)))),
        ),
        6 => (
            Layer::L3,
            LayerModification::ReplaceTextWord {
                category: RewritableCategory::ColorWord,
                from: "test.from".to_owned(),
                to: format!("test.to{}", below(rng, 9)),
            },
        ),
        7 => (
            Layer::L4,
            LayerModification::AddTypes {
                supertypes: any_set(rng),
                card_types: any_set(rng),
                subtypes: any_set(rng),
            },
        ),
        8 => (
            Layer::L4,
            LayerModification::SetTypes {
                supertypes: (below(rng, 2) == 0).then(|| any_set(rng)),
                card_types: None,
                subtypes: Some(any_set(rng)),
            },
        ),
        9 => (Layer::L4, LayerModification::RemoveSubtypes(any_set(rng))),
        10 => (Layer::L5, LayerModification::SetColors(ColorBitmask::BLUE)),
        11 => (Layer::L5, LayerModification::AddColors(ColorBitmask::WHITE)),
        12 => (
            Layer::L6,
            LayerModification::AddAbility(ability(&format!("g{}", below(rng, 9)))),
        ),
        13 => (Layer::L6, LayerModification::RemoveAllAbilities),
        14 => (
            Layer::L6,
            LayerModification::RemoveKeyword("flying".to_owned()),
        ),
        15 => (
            Layer::L7b,
            LayerModification::SetPt {
                power: Some(rng.next_u64().cast_signed()),
                toughness: None,
            },
        ),
        16 => (
            Layer::L7c,
            LayerModification::ModifyPt {
                power: rng.next_u64().cast_signed(),
                toughness: -1,
            },
        ),
        _ => (Layer::L7d, LayerModification::SwitchPt),
    }
}

fn any_affected(rng: &mut SplitMix64) -> Affected {
    match below(rng, 3) {
        0 => Affected::Objects((0..below(rng, 4)).map(|_| o(1 + below(rng, 8))).collect()),
        1 => Affected::Selector(selector(PredicateAST::IsCreature)),
        _ => Affected::Selector(selector(PredicateAST::Not(Box::new(PredicateAST::IsLand)))),
    }
}

/// A random object of the small pool the populated states use.
fn any_object(rng: &mut SplitMix64) -> ObjectId {
    o(1 + below(rng, 6))
}

/// Applies `steps` random valid layer operations to `s` through the public API: stamps of
/// every kind, static effects with their links, resolved effects, groups, ETB links and
/// removals.
pub(super) fn populate_layers(rng: &mut SplitMix64, s: &mut GameState, steps: u64) {
    let reasons = [
        StampReason::EntersZone,
        StampReason::Attached,
        StampReason::TurnedFaceUpOrDown,
        StampReason::TransformedOrConverted,
        StampReason::SpecialCardTurnedFaceUp,
        StampReason::GameStart,
        StampReason::JoinedMergedPermanent,
    ];
    let kinds = ["test.counter_a", "test.counter_b", "flying"];
    for _ in 0..steps {
        match below(rng, 12) {
            0 => {
                let r = pick(rng, &reasons);
                s.stamp_object(any_object(rng), r).unwrap();
            }
            1 => {
                let holder = if below(rng, 3) == 0 {
                    CounterHolder::Player(pick(rng, s.turn_order()))
                } else {
                    CounterHolder::Object(any_object(rng))
                };
                s.stamp_counters(holder, pick(rng, &kinds)).unwrap();
            }
            2 => {
                let sticker = u32::try_from(below(rng, 3)).unwrap();
                s.stamp_sticker(any_object(rng), sticker).unwrap();
            }
            3 | 4 => {
                // CR 613.7a: a static effect has its source's timestamp.
                let source = any_object(rng);
                let ts = match s.timestamps().object_timestamp(source) {
                    Some(ts) => ts,
                    None => s.stamp_object(source, StampReason::EntersZone).unwrap(),
                };
                let (layer, m) = any_modification(rng);
                let mut effect = static_effect(source, ts, layer, any_affected(rng), m);
                effect.is_cda = below(rng, 4) == 0;
                if below(rng, 3) == 0 {
                    effect.origin = EffectOrigin::Static {
                        granted_at: Some(ts),
                    };
                }
                let g = GeneratingAbility::new(source, ability(&format!("s{}", below(rng, 4))));
                s.register_effect(effect, Some(g)).unwrap();
            }
            5 | 6 => {
                // CR 613.7b.
                let ts = s.stamp_resolved_effect().unwrap();
                let (layer, m) = any_modification(rng);
                let mut effect = resolved_effect(ts, layer, any_affected(rng), m);
                if below(rng, 2) == 0 {
                    effect.source = Some(any_object(rng));
                }
                s.register_effect(effect, None).unwrap();
            }
            7 => {
                let id = s.issue_effect_id().unwrap();
                let g = GeneratingAbility::new(any_object(rng), ability("etb"));
                s.link_effect_generator(id, g).unwrap();
            }
            8 => {
                let ids: Vec<EffectId> = s
                    .effects()
                    .iter()
                    .map(|(id, _)| id)
                    .chain(s.effect_generators().keys().copied())
                    .collect();
                if !ids.is_empty() {
                    let id = pick(rng, &ids);
                    assert!(s.remove_effect(id));
                }
            }
            9 => {
                // CR 613.6: a two-part group; the 7b part is locked to the layer-4 part.
                let ts = s.stamp_resolved_effect().unwrap();
                let anchor = resolved_effect(
                    ts,
                    Layer::L4,
                    only(&[1, 2]),
                    LayerModification::AddTypes {
                        supertypes: BTreeSet::new(),
                        card_types: set(["Creature"]),
                        subtypes: BTreeSet::new(),
                    },
                );
                let part = resolved_effect(
                    ts,
                    Layer::L7b,
                    Affected::SameAsLayer(Layer::L4),
                    LayerModification::SetPt {
                        power: Some(1),
                        toughness: Some(1),
                    },
                );
                s.register_effect_group(&NoWorld, vec![(anchor, None), (part, None)])
                    .unwrap();
            }
            10 => {
                let players = s.turn_order().to_vec();
                let batch: Vec<SimultaneousStamp> = (1..=1 + below(rng, 3))
                    .map(|n| SimultaneousStamp {
                        object: o(10 + n),
                        player: pick(rng, &players),
                        choice: u32::try_from(below(rng, 3)).unwrap(),
                    })
                    .collect();
                s.stamp_objects_simultaneously(&batch, StampReason::EntersZone)
                    .unwrap();
            }
            _ => {
                if below(rng, 2) == 0 {
                    s.forget_object_stamps(any_object(rng));
                } else {
                    let kind = pick(rng, &kinds);
                    s.clear_counter_stamp(CounterHolder::Object(any_object(rng)), kind);
                }
            }
        }
    }
}

/// A layer environment with no objects.
pub(super) struct NoWorld;

impl LayerEnvironment for NoWorld {
    fn objects(&self) -> Vec<ObjectId> {
        Vec::new()
    }

    fn base(&self, _: ObjectId) -> Option<ObjectBase> {
        None
    }

    fn matches(&self, _: &ObjectSelectorAST, _: &MatchContext<'_>) -> bool {
        false
    }
}

fn wire_of(s: &GameState) -> StateWire {
    StateWire::from_bytes(&s.serialize().unwrap()).unwrap()
}

fn decode_wire(w: &StateWire) -> Result<GameState, StateError> {
    GameState::deserialize(&w.to_bytes().unwrap())
}

fn assert_decode_err(r: Result<GameState, StateError>) {
    match r {
        Err(StateError::Decode(_)) => {}
        other => panic!("expected Decode error, got {other:?}"),
    }
}

pub(super) fn check_round_trip(s: &GameState) {
    let b = s.serialize().unwrap();
    assert_eq!(b, s.serialize().unwrap(), "byte-deterministic");
    let back = GameState::deserialize(&b).unwrap();
    assert_eq!(&back, s);
    assert_eq!(back.state_hash(), s.state_hash());
    assert_eq!(back.serialize().unwrap(), b);
}

/// A 3-player state whose timestamp and effect generators have issued `last_ts` and
/// `last_effect`.
fn state_at(last_ts: u64, last_effect: u64) -> GameState {
    let mut w = wire_of(&GameState::new(3, 3, 20).unwrap());
    w.ids.timestamp = last_ts;
    w.ids.effect = last_effect;
    decode_wire(&w).unwrap()
}

// ---------------------------------------------------------------------------------------------
// The pinned non-empty state (D9)
// ---------------------------------------------------------------------------------------------

/// Two stamped objects, one counter stamp, one static effect with its link, one resolved
/// effect and one ETB link, built in the plain order.
pub(super) fn golden_layers() -> GameState {
    let mut s = GameState::new(42, 2, 20).unwrap();
    let t1 = s.stamp_object(o(1), StampReason::EntersZone).unwrap();
    s.stamp_object(o(2), StampReason::EntersZone).unwrap();
    s.stamp_counters(CounterHolder::Object(o(1)), "test.counter_a")
        .unwrap();
    s.register_effect(
        static_effect(
            o(1),
            t1,
            Layer::L6,
            only(&[2]),
            LayerModification::AddAbility(ability("marker")),
        ),
        Some(GeneratingAbility::new(o(1), ability("granter"))),
    )
    .unwrap();
    let t4 = s.stamp_resolved_effect().unwrap();
    s.register_effect(
        resolved_effect(
            t4,
            Layer::L7c,
            only(&[2]),
            LayerModification::ModifyPt {
                power: 1,
                toughness: 1,
            },
        ),
        None,
    )
    .unwrap();
    let etb = s.issue_effect_id().unwrap();
    s.link_effect_generator(etb, GeneratingAbility::new(o(2), ability("enters_tapped")))
        .unwrap();
    s
}

/// The same state, its calls in another, semantically equal order.
fn golden_layers_reordered() -> GameState {
    let mut s = GameState::new(42, 1, 20).unwrap();
    let t1 = s.stamp_object(o(1), StampReason::Attached).unwrap();
    // The static effect is registered before the second object is stamped.
    s.register_effect(
        static_effect(
            o(1),
            t1,
            Layer::L6,
            only(&[2]),
            LayerModification::AddAbility(ability("marker")),
        ),
        Some(GeneratingAbility::new(o(1), ability("granter"))),
    )
    .unwrap();
    s.add_player(20).unwrap();
    s.stamp_object(o(2), StampReason::TransformedOrConverted)
        .unwrap();
    // A clear of a stamp that does not exist changes nothing.
    s.clear_counter_stamp(CounterHolder::Object(o(2)), "test.counter_a");
    s.stamp_counters(CounterHolder::Object(o(1)), "test.counter_a")
        .unwrap();
    let t4 = s.stamp_resolved_effect().unwrap();
    s.register_effect(
        resolved_effect(
            t4,
            Layer::L7c,
            only(&[2]),
            LayerModification::ModifyPt {
                power: 1,
                toughness: 1,
            },
        ),
        None,
    )
    .unwrap();
    let etb = s.issue_effect_id().unwrap();
    s.forget_object_stamps(o(99));
    s.link_effect_generator(etb, GeneratingAbility::new(o(2), ability("enters_tapped")))
        .unwrap();
    s
}

/// The state hash of [`golden_layers`]. #232 FP-164 changed it from
/// `49b4707ebee9e40df30c49d6ca4fe2289e328f66814ef2b84833e58400ce7cc1` only because the fixture
/// ability became a valid STATIC ability (it lacked `effects`); the base tree 242545c gives this
/// value for the same valid content (docs/DECISIONS.md "#232 FP-164 — Golden hashes and replaced
/// tests").
const GOLDEN_LAYERS: &str = "e4d5ee18abcb3d3e96fbcfd9e7eea993b2d62d639a89028fe5945cf2b09f947f";

// REQ-NFR-006, REQ-CORE-001, REQ-CORE-002, REQ-LAYER-001 (D9): the non-empty layer encoding
// pinned as a literal digest; identical over 10 builds; equal for a semantically equal build
// order.
#[test]
fn golden_layers_state_hash() {
    let s = golden_layers();
    assert_eq!(s.state_hash().to_hex(), GOLDEN_LAYERS);
    for _ in 0..10 {
        let again = golden_layers();
        assert_eq!(again, s);
        assert_eq!(again.state_hash().to_hex(), GOLDEN_LAYERS);
        assert_eq!(again.serialize().unwrap(), s.serialize().unwrap());
    }
    let r = golden_layers_reordered();
    assert_eq!(r, s);
    assert_eq!(r.state_hash(), s.state_hash());
    check_round_trip(&s);
}

// REQ-NFR-006 (D5): the layer leaves carry exactly their canonical encodings; an empty one is
// put_len(0).
#[test]
fn layer_leaves_payloads() {
    let fresh = GameState::new(1, 1, 20).unwrap();
    let tree = fresh.merkle_tree();
    for name in ["effects", "timestamps", "effect_generators", "history"] {
        let payload = tree
            .leaf_payload(&ComponentPath::new(vec![Label::name(name)]))
            .unwrap()
            .to_vec();
        assert_eq!(payload, [0, 0, 0, 0], "{name}");
    }
    let s = golden_layers();
    let tree = s.merkle_tree();
    let leaf = |name: &str| {
        tree.leaf_payload(&ComponentPath::new(vec![Label::name(name)]))
            .unwrap()
            .to_vec()
    };
    assert_eq!(leaf("effects"), encode_to_vec(s.effects()));
    assert_eq!(leaf("timestamps"), encode_to_vec(s.timestamps()));
    assert_eq!(
        leaf("effect_generators"),
        encode_to_vec(s.effect_generators())
    );
    assert_eq!(&leaf("effects")[..4], [2, 0, 0, 0]);
    assert_eq!(&leaf("timestamps")[..4], [3, 0, 0, 0]);
    assert_eq!(&leaf("effect_generators")[..4], [2, 0, 0, 0]);
}

// ---------------------------------------------------------------------------------------------
// Registration and links (D3)
// ---------------------------------------------------------------------------------------------

fn effect_ids(s: &GameState) -> Vec<u64> {
    s.effects().iter().map(|(id, _)| id.get()).collect()
}

/// Asserts `register_effect(effect, g)` fails with `want` and changes nothing.
fn refuse(
    s: &mut GameState,
    effect: ContinuousEffect,
    g: Option<GeneratingAbility>,
    want: &StateError,
) {
    let before = s.clone();
    let h = s.state_hash();
    assert_eq!(s.register_effect(effect, g).as_ref(), Err(want));
    assert_eq!(*s, before);
    assert_eq!(s.state_hash(), h);
}

// REQ-LAYER-001, REQ-LAYER-002, REQ-CORE-002: CR 613.8a, 611.3: register_effect issues ids from
// the state's generator and records links for static effects with a source and only for them;
// every refusal leaves the state, its hash and the effect generator unchanged.
#[test]
fn register_effect_validates_links() {
    let mut s = GameState::new(5, 2, 20).unwrap();
    let ts = s.stamp_object(o(1), StampReason::EntersZone).unwrap();
    let grant = || {
        static_effect(
            o(1),
            ts,
            Layer::L6,
            only(&[1]),
            LayerModification::AddAbility(ability("x")),
        )
    };
    let g = || GeneratingAbility::new(o(1), ability("g"));
    refuse(&mut s, grant(), None, &StateError::MissingGenerator);
    refuse(
        &mut s,
        grant(),
        Some(GeneratingAbility::new(o(2), ability("g"))),
        &StateError::GeneratorSourceMismatch,
    );
    let mut sourceless = grant();
    sourceless.source = None;
    refuse(
        &mut s,
        sourceless.clone(),
        Some(g()),
        &StateError::UnexpectedGenerator,
    );
    let mut resolved = grant();
    resolved.origin = EffectOrigin::Resolved;
    refuse(
        &mut s,
        resolved.clone(),
        Some(g()),
        &StateError::UnexpectedGenerator,
    );
    let mut mismatch = grant();
    mismatch.layer = Layer::L5;
    refuse(
        &mut s,
        mismatch,
        Some(g()),
        &StateError::Layer(LayerError::LayerMismatch { layer: Layer::L5 }),
    );
    let mut linked = resolved.clone();
    linked.affected = Affected::SameAsLayer(Layer::L1a);
    assert!(matches!(
        s.clone().register_effect(linked, None),
        Err(StateError::Layer(LayerError::Unsupported(_)))
    ));
    let card_name = resolved_effect(
        ts,
        Layer::L3,
        only(&[1]),
        LayerModification::ReplaceTextWord {
            category: RewritableCategory::CardName,
            from: "a".into(),
            to: "b".into(),
        },
    );
    assert!(matches!(
        s.clone().register_effect(card_name, None),
        Err(StateError::Layer(LayerError::Unsupported(_)))
    ));
    assert_eq!(s.ids().effect.last_issued(), 0);

    assert_eq!(s.register_effect(grant(), Some(g())), Ok(e(1)));
    assert_eq!(s.register_effect(sourceless, None), Ok(e(2)));
    assert_eq!(s.register_effect(resolved, None), Ok(e(3)));
    assert_eq!(effect_ids(&s), [1, 2, 3]);
    assert_eq!(s.effect_generator(e(1)), Some(&g()));
    assert_eq!(s.effect_generator(e(2)), None);
    assert_eq!(s.effect_generators().len(), 1);
    assert_eq!(s.effects().get(e(1)), Some(&grant()));
    // An id registered directly makes register_effect refuse it, unchanged.
    let remove_all = || {
        resolved_effect(
            ts,
            Layer::L6,
            only(&[1]),
            LayerModification::RemoveAllAbilities,
        )
    };
    let mut taken = s.clone();
    taken.effects_mut().register(e(4), remove_all()).unwrap();
    let before = taken.clone();
    assert_eq!(
        taken.register_effect(remove_all(), None),
        Err(StateError::Layer(LayerError::DuplicateEffect(e(4))))
    );
    assert_eq!(taken, before);
    // Effect-id exhaustion.
    let mut full = state_at(0, u64::MAX);
    let before = full.clone();
    assert_eq!(
        full.register_effect(remove_all(), None),
        Err(StateError::IdExhausted)
    );
    assert_eq!(full, before);
    let mut full_group = full.clone();
    assert_eq!(
        full_group.register_effect_group(&NoWorld, vec![(remove_all(), None)]),
        Err(StateError::IdExhausted)
    );
    assert_eq!(full_group, before);
}

// REQ-LAYER-002, REQ-CORE-002: ETB links share the effect id space; remove_effect leaves no
// dangling link in state or hash.
#[test]
fn link_effect_generator_and_remove() {
    let mut s = GameState::new(5, 2, 20).unwrap();
    let g = GeneratingAbility::new(o(3), ability("enters_tapped"));
    for bad in [e(0), e(1), e(7)] {
        assert_eq!(
            s.link_effect_generator(bad, g.clone()),
            Err(StateError::EffectIdNotIssued(bad))
        );
    }
    let etb = s.issue_effect_id().unwrap();
    let before = s.clone();
    assert_eq!(s.link_effect_generator(etb, g.clone()), Ok(()));
    assert_ne!(s.state_hash(), before.state_hash());
    assert_eq!(
        s.link_effect_generator(etb, g.clone()),
        Err(StateError::AlreadyLinked(etb))
    );
    let ts = s.stamp_object(o(3), StampReason::EntersZone).unwrap();
    let id = s
        .register_effect(
            resolved_effect(
                ts,
                Layer::L6,
                only(&[3]),
                LayerModification::RemoveAllAbilities,
            ),
            None,
        )
        .unwrap();
    assert_eq!(
        s.link_effect_generator(id, g.clone()),
        Err(StateError::EffectRegistered(id))
    );
    let sid = s
        .register_effect(
            static_effect(
                o(3),
                ts,
                Layer::L6,
                only(&[3]),
                LayerModification::AddAbility(ability("m")),
            ),
            Some(GeneratingAbility::new(o(3), ability("s"))),
        )
        .unwrap();
    assert!(s.remove_effect(sid));
    assert!(s.effects().get(sid).is_none());
    assert!(s.effect_generator(sid).is_none());
    assert!(s.remove_effect(etb));
    assert!(s.effect_generator(etb).is_none());
    assert!(!s.remove_effect(etb));
    assert!(s.remove_effect(id));
    assert!(!s.remove_effect(e(99)));
    assert!(s.effects().is_empty());
    assert!(s.effect_generators().is_empty());
    let mut fresh = GameState::new(5, 2, 20).unwrap();
    fresh.stamp_object(o(3), StampReason::EntersZone).unwrap();
    for _ in 0..3 {
        fresh.issue_effect_id().unwrap();
    }
    assert_eq!(s, fresh);
    assert_eq!(s.state_hash(), fresh.state_hash());
}

// REQ-DSL-004, REQ-LAYER-002 (S3): SRS 6.2 item 1, CR 613.4a: layer 7a consists only of
// characteristic-defining abilities. register_effect refuses a non-CDA 7a effect, leaving state,
// hash and generators unchanged, and accepts a CDA one; ContinuousEffects::register itself still
// accepts a non-CDA 7a effect.
#[test]
fn s3_layer_7a_boundary() {
    let mut s = GameState::new(8, 2, 20).unwrap();
    let ts = s.stamp_object(o(1), StampReason::EntersZone).unwrap();
    let pt = LayerModification::SetPt {
        power: Some(3),
        toughness: Some(3),
    };
    let mut cda = static_effect(o(1), ts, Layer::L7a, only(&[1]), pt.clone());
    cda.is_cda = true;
    let mut non_cda = cda.clone();
    non_cda.is_cda = false;
    let g = GeneratingAbility::new(o(1), ability("cda"));
    let before = s.clone();
    let h = s.state_hash();
    let bytes = s.serialize().unwrap();
    refuse(
        &mut s,
        non_cda.clone(),
        Some(g.clone()),
        &StateError::NonCdaInLayer7a,
    );
    refuse(
        &mut s,
        resolved_effect(ts, Layer::L7a, only(&[1]), pt.clone()),
        None,
        &StateError::NonCdaInLayer7a,
    );
    assert_eq!(s, before);
    assert_eq!(s.state_hash(), h);
    assert_eq!(s.serialize().unwrap(), bytes);
    assert_eq!(s.ids().effect.last_issued(), 0);
    assert_eq!(s.register_effect(cda, Some(g)), Ok(e(1)));
    let mut seven_b = non_cda.clone();
    seven_b.layer = Layer::L7b;
    assert!(
        s.register_effect(seven_b, Some(GeneratingAbility::new(o(1), ability("b"))))
            .is_ok()
    );
    check_round_trip(&s);
    // The registry itself accepts a non-CDA 7a effect (existing behaviour).
    let mut r = ContinuousEffects::new();
    assert_eq!(r.register(e(1), non_cda.clone()), Ok(()));
    // Written directly into a state, it does not deserialize.
    let mut direct = GameState::new(8, 2, 20).unwrap();
    direct.stamp_object(o(1), StampReason::EntersZone).unwrap();
    let id = direct.issue_effect_id().unwrap();
    direct.effects_mut().register(id, non_cda).unwrap();
    direct
        .link_effect_generator(EffectId::new(1), GeneratingAbility::new(o(1), ability("x")))
        .unwrap_err();
    assert_decode_err(GameState::deserialize(&direct.serialize().unwrap()));
    // Group parts are checked too.
    let mut grouped = GameState::new(8, 2, 20).unwrap();
    let ts = grouped.stamp_resolved_effect().unwrap();
    let before = grouped.clone();
    assert_eq!(
        grouped.register_effect_group(
            &NoWorld,
            vec![(resolved_effect(ts, Layer::L7a, only(&[1]), pt), None)]
        ),
        Err(StateError::NonCdaInLayer7a)
    );
    assert_eq!(grouped, before);
}

// REQ-LAYER-001, REQ-CORE-002: CR 613.6: register_effect_group issues one id per part, links the
// parts and records their generating abilities; all or nothing; an orphaned part round-trips.
#[test]
fn register_effect_group_links_parts() {
    let mut s = GameState::new(8, 2, 20).unwrap();
    let ts = s.stamp_object(o(4), StampReason::EntersZone).unwrap();
    let anchor = static_effect(
        o(4),
        ts,
        Layer::L4,
        only(&[4]),
        LayerModification::AddTypes {
            supertypes: BTreeSet::new(),
            card_types: set(["Creature"]),
            subtypes: BTreeSet::new(),
        },
    );
    let part = static_effect(
        o(4),
        ts,
        Layer::L7b,
        Affected::SameAsLayer(Layer::L4),
        LayerModification::SetPt {
            power: Some(4),
            toughness: Some(4),
        },
    );
    let g = GeneratingAbility::new(o(4), ability("animate"));
    let before = s.clone();
    assert_eq!(
        s.register_effect_group(
            &NoWorld,
            vec![(anchor.clone(), Some(g.clone())), (part.clone(), None)]
        ),
        Err(StateError::MissingGenerator)
    );
    assert_eq!(s, before);
    let mut bad = part.clone();
    bad.affected = Affected::SameAsLayer(Layer::L5);
    assert!(matches!(
        s.register_effect_group(
            &NoWorld,
            vec![(anchor.clone(), Some(g.clone())), (bad, Some(g.clone()))]
        ),
        Err(StateError::Layer(LayerError::BadLink(_)))
    ));
    assert_eq!(s, before);
    let ids = s
        .register_effect_group(
            &NoWorld,
            vec![(anchor, Some(g.clone())), (part, Some(g.clone()))],
        )
        .unwrap();
    assert_eq!(ids, [e(1), e(2)]);
    assert_eq!(s.effect_generator(e(2)), Some(&g));
    check_round_trip(&s);
    assert!(s.remove_effect(e(1)));
    assert!(s.effects().get(e(2)).is_some());
    check_round_trip(&s);
}

// ---------------------------------------------------------------------------------------------
// S5: timestamps through the state (CR 613.7)
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002: CR 613.7d (zone entry), 613.7e (attach), 613.7f (face up or down), 613.7g
// (transform), 613.7h-j: each stamp is a new, strictly later timestamp from the state's
// generator; a sticker follows right after its object's new timestamp (CR 613.7k).
#[test]
fn s5_object_stamps_by_reason() {
    for reason in [
        StampReason::EntersZone,
        StampReason::Attached,
        StampReason::TurnedFaceUpOrDown,
        StampReason::TransformedOrConverted,
        StampReason::SpecialCardTurnedFaceUp,
        StampReason::GameStart,
        StampReason::JoinedMergedPermanent,
    ] {
        let mut s = GameState::new(9, 2, 20).unwrap();
        assert_eq!(s.stamp_object(o(1), reason), Ok(t(1)), "{reason:?}");
        assert_eq!(s.timestamps().object_timestamp(o(1)), Some(t(1)));
        assert_eq!(s.ids().timestamp.last_issued(), 1);
        assert_eq!(s.stamp_sticker(o(1), 7), Ok(t(2)));
        assert_eq!(s.stamp_object(o(1), reason), Ok(t(3)));
        assert_eq!(s.timestamps().sticker_timestamp(o(1), 7), Some(t(4)));
        assert_eq!(s.ids().timestamp.last_issued(), 4);
        check_round_trip(&s);
    }
}

// REQ-CORE-002: CR 613.7c: a counter of a kind gets a new timestamp shared by every counter of
// that kind on its holder; clearing and forgetting remove stamps.
#[test]
fn s5_counter_stamps() {
    let mut s = GameState::new(9, 2, 20).unwrap();
    let a = CounterHolder::Object(o(1));
    let b = CounterHolder::Player(p(2));
    assert_eq!(s.stamp_counters(a, "test.counter_a"), Ok(t(1)));
    assert_eq!(s.stamp_counters(b, "test.counter_a"), Ok(t(2)));
    assert_eq!(s.stamp_counters(a, "test.counter_a"), Ok(t(3)));
    assert_eq!(
        s.timestamps().counter_timestamp(a, "test.counter_a"),
        Some(t(3))
    );
    check_round_trip(&s);
    let before = s.clone();
    s.clear_counter_stamp(CounterHolder::Object(o(9)), "test.counter_a");
    assert_eq!(s, before);
    s.clear_counter_stamp(a, "test.counter_a");
    assert_eq!(s.timestamps().counter_timestamp(a, "test.counter_a"), None);
    assert_eq!(s.ids().timestamp.last_issued(), 3);
    s.stamp_object(o(1), StampReason::EntersZone).unwrap();
    s.stamp_sticker(o(1), 1).unwrap();
    s.stamp_counters(a, "x").unwrap();
    s.forget_object_stamps(o(1));
    assert_eq!(s.timestamps().object_timestamp(o(1)), None);
    assert_eq!(s.timestamps().sticker_timestamp(o(1), 1), None);
    assert_eq!(s.timestamps().counter_timestamp(a, "x"), None);
    assert_eq!(
        s.timestamps().counter_timestamp(b, "test.counter_a"),
        Some(t(2))
    );
    let before = s.clone();
    s.forget_object_stamps(o(1));
    assert_eq!(s, before);
}

// REQ-CORE-002: CR 613.7b (a resolved effect: a new timestamp, the ledger untouched), 613.7n
// (the object's own static effect earlier than the resolved one) and 613.7a (re-stamping an
// object's static effects).
#[test]
fn s5_effect_stamps() {
    let mut s = GameState::new(9, 2, 20).unwrap();
    s.stamp_object(o(1), StampReason::EntersZone).unwrap();
    let ledger = s.timestamps().clone();
    assert_eq!(s.stamp_resolved_effect(), Ok(t(2)));
    assert_eq!(s.timestamps(), &ledger);
    assert_eq!(s.stamp_static_and_resolved(), Ok((t(3), t(4))));
    assert_eq!(s.ids().timestamp.last_issued(), 4);
    let ts = s.stamp_object(o(1), StampReason::EntersZone).unwrap();
    let id = s
        .register_effect(
            static_effect(
                o(1),
                t(1),
                Layer::L6,
                only(&[1]),
                LayerModification::RemoveAllAbilities,
            ),
            Some(GeneratingAbility::new(o(1), ability("r"))),
        )
        .unwrap();
    s.restamp_static_effects(o(1), t(1), ts).unwrap();
    assert_eq!(s.effects().get(id).unwrap().timestamp, ts);
    let before = s.clone();
    s.restamp_static_effects(o(1), ts, ts).unwrap();
    assert_eq!(s, before);
}

// REQ-CORE-002: CR 613.7m, CR 101.4: simultaneous stamps in APNAP order from the active player,
// each player's objects in their chosen order; the order the batch is listed in does not matter.
#[test]
fn s5_simultaneous_stamps_apnap() {
    let mut s = GameState::new(9, 3, 20).unwrap();
    s.set_active_player(Some(p(2))).unwrap();
    assert_eq!(s.apnap_order(), [p(2), p(3), p(1)]);
    let stamp = |object: u64, player: u64, choice: u32| SimultaneousStamp {
        object: o(object),
        player: p(player),
        choice,
    };
    let batch = [
        stamp(1, 1, 0),
        stamp(2, 2, 1),
        stamp(3, 2, 0),
        stamp(4, 3, 0),
    ];
    let mut a = s.clone();
    let out = a
        .stamp_objects_simultaneously(&batch, StampReason::EntersZone)
        .unwrap();
    assert_eq!(
        out,
        [(o(3), t(1)), (o(2), t(2)), (o(4), t(3)), (o(1), t(4))]
    );
    let mut b = s.clone();
    let mut reversed = batch;
    reversed.reverse();
    b.stamp_objects_simultaneously(&reversed, StampReason::Attached)
        .unwrap();
    assert_eq!(a, b);
    let mut c = GameState::new(9, 3, 20).unwrap();
    c.set_turn_order(vec![p(3), p(1), p(2)]).unwrap();
    assert_eq!(c.apnap_order(), [p(3), p(1), p(2)]);
    let out = c
        .stamp_objects_simultaneously(&batch, StampReason::EntersZone)
        .unwrap();
    assert_eq!(out[0], (o(4), t(1)));
    assert_eq!(out[3], (o(2), t(4)));
    let before = c.clone();
    assert_eq!(
        c.stamp_objects_simultaneously(&[stamp(1, 9, 0)], StampReason::EntersZone),
        Err(StateError::Layer(LayerError::UnknownPlayer(p(9))))
    );
    assert_eq!(
        c.stamp_objects_simultaneously(&[batch[0], batch[0]], StampReason::EntersZone),
        Err(StateError::Layer(LayerError::DuplicateObject(o(1))))
    );
    assert_eq!(c, before);
    assert_eq!(
        c.stamp_objects_simultaneously(&[], StampReason::EntersZone),
        Ok(vec![])
    );
    assert_eq!(c, before);
}

type Op = fn(&mut GameState) -> Result<(), StateError>;
type Mutator = fn(&mut GameState);
type EffectEdit = Box<dyn Fn(&mut ContinuousEffect)>;
type StateEdit = Box<dyn Fn(&mut GameState)>;

/// Asserts `f` fails with exhaustion and leaves `s` and its hash unchanged.
fn exhausted(s: &mut GameState, f: Op) {
    let before = s.clone();
    let h = s.state_hash();
    assert_eq!(f(s), Err(StateError::IdExhausted));
    assert_eq!(*s, before);
    assert_eq!(s.state_hash(), h);
}

// REQ-CORE-002: every timestamp wrapper maps exhaustion to IdExhausted and leaves the state, its
// generators and its hash unchanged.
#[test]
fn s5_exhaustion_changes_nothing() {
    let mut at_max = state_at(u64::MAX, 0);
    exhausted(&mut at_max, |s| {
        s.stamp_object(o(1), StampReason::EntersZone).map(|_| ())
    });
    exhausted(&mut at_max, |s| {
        s.stamp_counters(CounterHolder::Object(o(1)), "k")
            .map(|_| ())
    });
    exhausted(&mut at_max, |s| s.stamp_sticker(o(1), 1).map(|_| ()));
    exhausted(&mut at_max, |s| s.stamp_resolved_effect().map(|_| ()));
    let mut one_left = state_at(u64::MAX - 1, 0);
    exhausted(&mut one_left, |s| s.stamp_static_and_resolved().map(|_| ()));
    exhausted(&mut one_left, |s| {
        let batch = [
            SimultaneousStamp {
                object: o(1),
                player: p(1),
                choice: 0,
            },
            SimultaneousStamp {
                object: o(2),
                player: p(1),
                choice: 1,
            },
        ];
        s.stamp_objects_simultaneously(&batch, StampReason::EntersZone)
            .map(|_| ())
    });
    // One left, but the sticker needs a second.
    let mut sticker = state_at(u64::MAX - 3, 0);
    sticker.stamp_object(o(1), StampReason::EntersZone).unwrap();
    sticker.stamp_sticker(o(1), 1).unwrap();
    exhausted(&mut sticker, |s| {
        s.stamp_object(o(1), StampReason::Attached).map(|_| ())
    });
    // Re-stamping needs a new timestamp for a granted static effect (CR 613.7a).
    let mut granted = state_at(u64::MAX - 2, 0);
    let ts = granted.stamp_object(o(1), StampReason::EntersZone).unwrap();
    assert_eq!(ts, t(u64::MAX - 1));
    let g = granted.stamp_resolved_effect().unwrap();
    let mut effect = static_effect(
        o(1),
        g,
        Layer::L6,
        only(&[1]),
        LayerModification::RemoveAllAbilities,
    );
    effect.origin = EffectOrigin::Static {
        granted_at: Some(g),
    };
    granted
        .register_effect(effect, Some(GeneratingAbility::new(o(1), ability("q"))))
        .unwrap();
    exhausted(&mut granted, |s| {
        s.restamp_static_effects(o(1), t(u64::MAX - 1), t(u64::MAX - 1))
    });
}

// ---------------------------------------------------------------------------------------------
// S4: hash, round trip, independence, rollback
// ---------------------------------------------------------------------------------------------

// REQ-CORE-001, REQ-NFR-006, REQ-LAYER-001: property: populated states round-trip by value,
// bytes and hash; 10 repeated builds from one seed are identical.
#[test]
fn s4_prop_populated_round_trip_and_determinism() {
    let mut rng = SplitMix64::new(0x0183_0001);
    for round in 0..300_u64 {
        let seed = rng.next_u64();
        let build = || {
            let mut r = SplitMix64::new(seed);
            let players = 1 + usize::try_from(round % 4).unwrap();
            let mut s = GameState::new(r.next_u64(), players, 20).unwrap();
            populate_layers(&mut r, &mut s, 1 + round % 40);
            s
        };
        let s = build();
        check_round_trip(&s);
        if round % 30 == 0 {
            for _ in 0..10 {
                let again = build();
                assert_eq!(again, s);
                assert_eq!(again.state_hash(), s.state_hash());
            }
        }
    }
}

// REQ-CORE-001: mutating a clone's effects, timestamps or links never changes the original, and
// the reverse.
#[test]
fn s4_clone_independence() {
    let mut rng = SplitMix64::new(0x0183_0002);
    let mut base = GameState::new(1, 2, 20).unwrap();
    populate_layers(&mut rng, &mut base, 30);
    let h = base.state_hash();
    let bytes = base.serialize().unwrap();
    let mutators: Vec<(&str, Mutator)> = vec![
        ("register_effect", |s| {
            let ts = s.stamp_resolved_effect().unwrap();
            s.register_effect(
                resolved_effect(
                    ts,
                    Layer::L6,
                    only(&[1]),
                    LayerModification::RemoveAllAbilities,
                ),
                None,
            )
            .unwrap();
        }),
        ("effects_mut", |s| {
            s.effects_mut()
                .register(
                    e(9_999),
                    resolved_effect(
                        t(1),
                        Layer::L5,
                        only(&[1]),
                        LayerModification::SetColors(ColorBitmask::RED),
                    ),
                )
                .unwrap();
        }),
        ("timestamps_mut", |s| {
            let mut ids = s.ids().timestamp.clone();
            s.timestamps_mut()
                .stamp_object(&mut ids, o(77), StampReason::EntersZone)
                .unwrap();
        }),
        ("stamp_object", |s| {
            s.stamp_object(o(78), StampReason::EntersZone).unwrap();
        }),
        ("stamp_counters", |s| {
            s.stamp_counters(CounterHolder::Object(o(78)), "k").unwrap();
        }),
        ("link", |s| {
            let id = s.issue_effect_id().unwrap();
            s.link_effect_generator(id, GeneratingAbility::new(o(1), ability("l")))
                .unwrap();
        }),
    ];
    for (name, m) in mutators {
        let mut c = base.clone();
        m(&mut c);
        assert_ne!(c, base, "{name}");
        assert_ne!(c.state_hash(), h, "{name}");
        assert_eq!(base.state_hash(), h, "{name}");
        assert_eq!(base.serialize().unwrap(), bytes, "{name}");
        let orig = base.clone();
        let mut snap = orig.clone();
        m(&mut snap);
        assert_eq!(orig.state_hash(), h, "{name}");
        assert_eq!(orig, base, "{name}");
        assert_ne!(snap, orig, "{name}");
    }
    let rebuilt = GameState::deserialize(&bytes).unwrap();
    assert_eq!(rebuilt, base);
}

// REQ-CORE-001, REQ-CORE-002 (D8): CR 734.1: a rollback restores effects, timestamps and links,
// and never reuses a timestamp or effect id issued by a durable sub-transaction.
#[test]
fn s4_rollback_restores_layers_and_keeps_ids_monotonic() {
    let mut start = GameState::new(4, 2, 20).unwrap();
    let ts = start.stamp_object(o(1), StampReason::EntersZone).unwrap();
    start
        .register_effect(
            static_effect(
                o(1),
                ts,
                Layer::L6,
                only(&[2]),
                LayerModification::AddAbility(ability("m")),
            ),
            Some(GeneratingAbility::new(o(1), ability("g"))),
        )
        .unwrap();
    let mut tx = Transactions::new(start.clone());
    let outer = tx.checkpoint();
    {
        let s = tx.state_mut();
        s.stamp_object(o(2), StampReason::EntersZone).unwrap();
        let rt = s.stamp_resolved_effect().unwrap();
        s.register_effect(
            resolved_effect(
                rt,
                Layer::L7c,
                only(&[2]),
                LayerModification::ModifyPt {
                    power: 1,
                    toughness: 1,
                },
            ),
            None,
        )
        .unwrap();
        let etb = s.issue_effect_id().unwrap();
        s.link_effect_generator(etb, GeneratingAbility::new(o(2), ability("etb")))
            .unwrap();
        assert!(s.remove_effect(e(1)));
    }
    let inner = tx.checkpoint();
    let (durable_ts, durable_effect) = {
        let s = tx.state_mut();
        let a = s.stamp_object(o(3), StampReason::EntersZone).unwrap();
        let b = s.stamp_counters(CounterHolder::Object(o(3)), "k").unwrap();
        let id = s
            .register_effect(
                resolved_effect(
                    b,
                    Layer::L6,
                    only(&[3]),
                    LayerModification::RemoveAllAbilities,
                ),
                None,
            )
            .unwrap();
        (a.max(b), id)
    };
    tx.commit_durable(inner).unwrap();
    tx.rollback(outer).unwrap();
    let s = tx.state().clone();
    assert_eq!(s.effects(), start.effects());
    assert_eq!(s.timestamps(), start.timestamps());
    assert_eq!(s.effect_generators(), start.effect_generators());
    assert!(s.ids().timestamp.last_issued() >= durable_ts.get());
    assert!(s.ids().effect.last_issued() >= durable_effect.get());
    let mut next = s.clone();
    assert!(next.stamp_resolved_effect().unwrap() > durable_ts);
    assert!(next.issue_effect_id().unwrap() > durable_effect);
    check_round_trip(&s);
}

/// The first Merkle divergence of `a` and `b`.
fn diverges(a: &GameState, b: &GameState) -> Option<ComponentPath> {
    diff_first_divergence(&a.merkle_tree(), &b.merkle_tree())
}

fn leaf(name: &str) -> ComponentPath {
    ComponentPath::new(vec![Label::name(name)])
}

/// `a` with the effect `id` replaced by `f` of it (through the unvalidated registry).
fn with_effect(a: &GameState, id: EffectId, f: &dyn Fn(&mut ContinuousEffect)) -> GameState {
    let mut b = a.clone();
    let mut changed = a.effects().get(id).unwrap().clone();
    f(&mut changed);
    assert!(b.effects_mut().remove(id));
    b.effects_mut().register(id, changed).unwrap();
    b
}

// REQ-NFR-006, REQ-CORE-006: property: mutating any single hashed value of an effect, a ledger
// entry or a link changes the state hash, and the first divergence is that component's leaf.
#[test]
fn s4_prop_single_field_mutations_change_hash() {
    let mut rng = SplitMix64::new(0x0183_0003);
    for _ in 0..40 {
        let mut a = GameState::new(rng.next_u64(), 2, 20).unwrap();
        populate_layers(&mut rng, &mut a, 25);
        let ts = a.stamp_object(o(1), StampReason::EntersZone).unwrap();
        a.stamp_sticker(o(1), 2).unwrap();
        a.stamp_counters(CounterHolder::Object(o(1)), "test.counter_z")
            .unwrap();
        let (layer, m) = any_modification(&mut rng);
        let id = a
            .register_effect(
                static_effect(o(1), ts, layer, only(&[1, 2]), m.clone()),
                Some(GeneratingAbility::new(o(1), ability("field"))),
            )
            .unwrap();
        let etb = a.issue_effect_id().unwrap();
        a.link_effect_generator(etb, GeneratingAbility::new(o(2), ability("etb")))
            .unwrap();
        let (layer2, m2) = loop {
            let candidate = any_modification(&mut rng);
            if candidate.1 != m {
                break candidate;
            }
        };
        let cases: Vec<EffectEdit> = vec![
            Box::new(|c| c.source = Some(o(2))),
            Box::new(|c| c.controller = p(2)),
            Box::new(|c| c.timestamp = t(c.timestamp.get() + 1)),
            Box::new(|c| c.is_cda = !c.is_cda),
            Box::new(|c| {
                c.origin = EffectOrigin::Static {
                    granted_at: Some(t(1)),
                }
            }),
            Box::new(|c| c.origin = EffectOrigin::Resolved),
            Box::new(|c| c.affected = only(&[1])),
            Box::new(|c| c.affected = Affected::Selector(selector(PredicateAST::IsCreature))),
            Box::new(move |c| {
                c.layer = layer2;
                c.modification = m2.clone();
            }),
        ];
        for f in &cases {
            let b = with_effect(&a, id, f.as_ref());
            assert_eq!(diverges(&a, &b), Some(leaf("effects")));
        }
        // The layer alone (7b and 7a for one P/T setting).
        let b = with_effect(&a, id, &|c| {
            c.layer = Layer::L7b;
            c.modification = LayerModification::SetPt {
                power: Some(1),
                toughness: None,
            };
        });
        let c = with_effect(&b, id, &|c| c.layer = Layer::L7a);
        assert_eq!(diverges(&b, &c), Some(leaf("effects")));
        // The ledger: an object, a counter and a sticker entry each.
        let ledger_cases: Vec<StateEdit> = vec![
            Box::new(move |s| {
                let mut g = IdGenerator::from_last_issued(ts.get());
                s.timestamps_mut()
                    .stamp_object(&mut g, o(1), StampReason::EntersZone)
                    .unwrap();
            }),
            Box::new(|s| s.clear_counter_stamp(CounterHolder::Object(o(1)), "test.counter_z")),
            Box::new(|s| {
                let mut g = IdGenerator::from_last_issued(0);
                s.timestamps_mut().put_sticker(&mut g, o(1), 2).unwrap();
            }),
            Box::new(|s| {
                let mut g = IdGenerator::from_last_issued(0);
                s.timestamps_mut()
                    .put_counters(&mut g, CounterHolder::Player(p(1)), "test.counter_z")
                    .unwrap();
            }),
        ];
        for f in &ledger_cases {
            let mut b = a.clone();
            f(&mut b);
            assert_eq!(diverges(&a, &b), Some(leaf("timestamps")));
        }
        // A link's ability, then its source.
        for g in [
            GeneratingAbility::new(o(2), ability("other")),
            GeneratingAbility::new(o(3), ability("etb")),
        ] {
            let mut b = a.clone();
            assert!(b.remove_effect(etb));
            b.link_effect_generator(etb, g).unwrap();
            assert_eq!(diverges(&a, &b), Some(leaf("effect_generators")));
        }
    }
}

#[derive(Clone)]
enum LedgerOp {
    Object(u64),
    Counter(u64),
    Sticker(u64, u32),
    Batch(Vec<SimultaneousStamp>),
}

#[derive(Clone)]
enum EffectOp {
    Static(u64, u64, Layer, LayerModification),
    Resolved(u64, Layer, LayerModification),
    Etb(u64),
}

/// Runs the two independent operation streams on a fresh state: in order (ledger first, links
/// made at once) or interleaved by `rng` (ETB links deferred, batches listed in reverse).
fn run_streams(
    ledger: &[LedgerOp],
    effects: &[EffectOp],
    rng: Option<&mut SplitMix64>,
) -> GameState {
    let mut s = GameState::new(11, 2, 20).unwrap();
    let shuffle = rng.is_some();
    let mut rng = rng;
    let (mut li, mut ei) = (0, 0);
    let mut pending: Vec<(EffectId, GeneratingAbility)> = Vec::new();
    while li < ledger.len() || ei < effects.len() || !pending.is_empty() {
        let choice = rng.as_deref_mut().map_or(0, |r| below(r, 3));
        if (choice == 2 || (li >= ledger.len() && ei >= effects.len())) && !pending.is_empty() {
            let (id, g) = pending.remove(0);
            s.link_effect_generator(id, g).unwrap();
        } else if li < ledger.len() && (choice == 0 || ei >= effects.len()) {
            match &ledger[li] {
                LedgerOp::Object(n) => {
                    s.stamp_object(o(*n), StampReason::EntersZone).unwrap();
                }
                LedgerOp::Counter(n) => {
                    s.stamp_counters(CounterHolder::Object(o(*n)), "test.counter_a")
                        .unwrap();
                }
                LedgerOp::Sticker(n, k) => {
                    s.stamp_sticker(o(*n), *k).unwrap();
                }
                LedgerOp::Batch(b) => {
                    let mut b = b.clone();
                    if shuffle {
                        b.reverse();
                    }
                    s.stamp_objects_simultaneously(&b, StampReason::EntersZone)
                        .unwrap();
                }
            }
            li += 1;
        } else if ei < effects.len() {
            match &effects[ei] {
                EffectOp::Static(src, ts, layer, m) => {
                    let effect = static_effect(o(*src), t(*ts), *layer, only(&[*src]), m.clone());
                    let g = GeneratingAbility::new(o(*src), ability("p"));
                    s.register_effect(effect, Some(g)).unwrap();
                }
                EffectOp::Resolved(ts, layer, m) => {
                    let effect = resolved_effect(t(*ts), *layer, only(&[1]), m.clone());
                    s.register_effect(effect, None).unwrap();
                }
                EffectOp::Etb(src) => {
                    let id = s.issue_effect_id().unwrap();
                    let g = GeneratingAbility::new(o(*src), ability("etb"));
                    if shuffle {
                        pending.push((id, g));
                    } else {
                        s.link_effect_generator(id, g).unwrap();
                    }
                }
            }
            ei += 1;
        }
    }
    s
}

// REQ-CORE-001, REQ-NFR-006: property: semantically equal registration and stamping orders
// (independent ledger and effect operations interleaved differently, ETB links made later,
// simultaneous batches listed in another order) give equal states and hashes.
#[test]
fn s4_prop_call_order_independence() {
    let mut rng = SplitMix64::new(0x0183_0004);
    for _ in 0..150 {
        let ledger: Vec<LedgerOp> = (0..1 + below(&mut rng, 8))
            .map(|i| match below(&mut rng, 4) {
                0 => LedgerOp::Object(1 + below(&mut rng, 4)),
                1 => LedgerOp::Counter(1 + below(&mut rng, 4)),
                2 => LedgerOp::Sticker(
                    1 + below(&mut rng, 4),
                    u32::try_from(below(&mut rng, 2)).unwrap(),
                ),
                _ => LedgerOp::Batch(
                    (0..1 + below(&mut rng, 3))
                        .map(|k| SimultaneousStamp {
                            object: o(100 + 10 * i + k),
                            player: p(1 + below(&mut rng, 2)),
                            choice: u32::try_from(below(&mut rng, 2)).unwrap(),
                        })
                        .collect(),
                ),
            })
            .collect();
        let n_ts = ledger.len() as u64;
        let effects: Vec<EffectOp> = (0..below(&mut rng, 6))
            .map(|_| {
                let (layer, m) = any_modification(&mut rng);
                match below(&mut rng, 3) {
                    0 => EffectOp::Static(
                        1 + below(&mut rng, 4),
                        1 + below(&mut rng, n_ts),
                        layer,
                        m,
                    ),
                    1 => EffectOp::Resolved(1 + below(&mut rng, n_ts), layer, m),
                    _ => EffectOp::Etb(1 + below(&mut rng, 4)),
                }
            })
            .collect();
        let plain = run_streams(&ledger, &effects, None);
        let shuffled = run_streams(&ledger, &effects, Some(&mut rng));
        assert_eq!(plain, shuffled);
        assert_eq!(plain.state_hash(), shuffled.state_hash());
        check_round_trip(&plain);
    }
}

// ---------------------------------------------------------------------------------------------
// Wire form version 2 (D6, R12)
// ---------------------------------------------------------------------------------------------

/// The golden state plus a ledger sticker and player counter, a two-part group and a copy
/// effect with a fixed-mana-value provenance.
fn rich_state() -> (GameState, u64, u64) {
    let mut s = golden_layers();
    let ts = s.stamp_object(o(5), StampReason::EntersZone).unwrap();
    s.stamp_sticker(o(5), 1).unwrap();
    s.stamp_counters(CounterHolder::Player(p(1)), "test.counter_p")
        .unwrap();
    let anchor = resolved_effect(
        ts,
        Layer::L4,
        only(&[5]),
        LayerModification::AddTypes {
            supertypes: BTreeSet::new(),
            card_types: set(["Artifact"]),
            subtypes: BTreeSet::new(),
        },
    );
    let part = resolved_effect(
        ts,
        Layer::L7b,
        Affected::SameAsLayer(Layer::L4),
        LayerModification::SetPt {
            power: Some(0),
            toughness: Some(1),
        },
    );
    let ids = s
        .register_effect_group(&NoWorld, vec![(anchor, None), (part, None)])
        .unwrap();
    let copy = resolved_effect(
        ts,
        Layer::L1a,
        only(&[5]),
        LayerModification::CopyValuesWithProvenance {
            characteristics: Box::new(creature(vec![])),
            provenance: provenance_from_parts(ValueSource::Fixed { mana_value: 3 }, true, false)
                .unwrap(),
        },
    );
    s.register_effect(copy, None).unwrap();
    (s, ids[0].get(), ids[1].get())
}

// REQ-CORE-001, R12 (D6): the decoder rejects every inconsistent layer component, each from a
// valid wire value with one corruption; format version 1 is not read.
#[test]
fn wire_rejects_inconsistent_layer_data() {
    use super::effects_wire::{
        AffectedWire, HolderWire, LinkWire, ModificationWire, OriginWire, ValueSourceWire,
    };
    let (s, anchor_id, part_id) = rich_state();
    let good = wire_of(&s);
    assert_eq!(decode_wire(&good).unwrap(), s);
    let n = good.effects.effects.len();
    assert_eq!(n, 5);
    let corrupt = |f: &dyn Fn(&mut StateWire)| {
        let mut w = good.clone();
        f(&mut w);
        assert_decode_err(decode_wire(&w));
    };
    let index = |w: &StateWire, id: u64| w.effects.effects.iter().position(|x| x.id == id).unwrap();
    // Versions.
    corrupt(&|w| w.format_version = 1);
    corrupt(&|w| w.format_version = 3);
    // Effect ids: duplicate, unordered, 0, beyond the generator.
    corrupt(&|w| {
        let first = w.effects.effects[0].clone();
        w.effects.effects.insert(0, first);
    });
    corrupt(&|w| w.effects.effects.swap(0, 1));
    corrupt(&|w| w.effects.effects[0].id = 0);
    corrupt(&|w| w.ids.effect = w.effects.effects[n - 1].id - 1);
    // Timestamps 0 or beyond the generator: effects, grants and every ledger group.
    corrupt(&|w| w.effects.effects[0].effect.timestamp = 0);
    corrupt(&|w| w.effects.effects[0].effect.timestamp = w.ids.timestamp + 1);
    corrupt(&|w| {
        w.effects.effects[0].effect.origin = OriginWire::Static {
            granted_at: Some(w.ids.timestamp + 1),
        };
    });
    corrupt(&|w| w.timestamps.objects[0].timestamp = w.ids.timestamp + 1);
    corrupt(&|w| w.timestamps.objects[0].timestamp = 0);
    corrupt(&|w| w.timestamps.counters[0].timestamp = w.ids.timestamp + 1);
    corrupt(&|w| w.timestamps.stickers[0].timestamp = w.ids.timestamp + 1);
    corrupt(&|w| w.ids.timestamp -= 1);
    // Ledger order and duplicates.
    corrupt(&|w| w.timestamps.objects.swap(0, 1));
    corrupt(&|w| {
        let first = w.timestamps.objects[0].clone();
        w.timestamps.objects.push(first);
    });
    corrupt(&|w| {
        let first = w.timestamps.counters[0].clone();
        w.timestamps.counters.push(first);
    });
    corrupt(&|w| {
        w.timestamps.counters[0].holder = HolderWire::Player(1);
        w.timestamps.counters[0].kind = "test.counter_p".into();
    });
    corrupt(&|w| {
        let first = w.timestamps.stickers[0].clone();
        w.timestamps.stickers.push(first);
    });
    // A static effect without its link; links for the wrong effects or sources; order.
    corrupt(&|w| w.effect_generators.retain(|g| g.effect != 1));
    corrupt(&|w| w.effect_generators[0].source += 1);
    corrupt(&|w| {
        let mut g = w.effect_generators[0].clone();
        g.effect = 2;
        w.effect_generators.insert(1, g);
    });
    corrupt(&|w| {
        let mut g = w.effect_generators[0].clone();
        g.effect = w.ids.effect + 1;
        w.effect_generators.push(g);
    });
    corrupt(&|w| w.effect_generators.swap(0, 1));
    // A non-CDA effect in layer 7a; a layer mismatch; an invalid text change.
    corrupt(&|w| {
        let i = index(w, part_id);
        assert_eq!(w.effects.effects[i].effect.layer, Layer::L7b);
        w.effects.effects[i].effect.layer = Layer::L7a;
    });
    corrupt(&|w| w.effects.effects[0].effect.layer = Layer::L5);
    corrupt(&|w| {
        w.effects.effects[1].effect.layer = Layer::L3;
        w.effects.effects[1].effect.modification = ModificationWire::ReplaceTextWord {
            category: RewritableCategory::CardName,
            from: "a".into(),
            to: "b".into(),
        };
    });
    // Links: unknown effects, a part that is not SameAsLayer, a bad anchor layer, unordered,
    // and a resolved selector left unlocked in a group.
    corrupt(&|w| w.effects.links[0].anchor = 99);
    corrupt(&|w| w.effects.links[0].part = 99);
    corrupt(&|w| {
        w.effects.links.insert(
            0,
            LinkWire {
                part: 1,
                anchor: anchor_id,
            },
        );
    });
    corrupt(&|w| {
        w.effects.links.push(LinkWire {
            part: 1,
            anchor: anchor_id,
        });
    });
    corrupt(&|w| {
        let i = index(w, part_id);
        w.effects.effects[i].effect.affected = AffectedWire::SameAsLayer(Layer::L7c);
    });
    corrupt(&|w| {
        let i = index(w, anchor_id);
        w.effects.effects[i].effect.affected =
            AffectedWire::Selector(selector(PredicateAST::IsCreature));
    });
    // Affected sets must be strictly ascending.
    corrupt(&|w| w.effects.effects[0].effect.affected = AffectedWire::Objects(vec![2, 1]));
    corrupt(&|w| w.effects.effects[0].effect.affected = AffectedWire::Objects(vec![2, 2]));
    // A provenance that no layer operation builds (zero-when-copied on a plain mana value).
    corrupt(&|w| {
        let ModificationWire::CopyValuesWithProvenance { provenance, .. } =
            &mut w.effects.effects[n - 1].effect.modification
        else {
            panic!("the copy effect is last");
        };
        provenance.source = ValueSourceWire::Plain;
    });
    // An orphaned SameAsLayer part decodes, and must still follow an earlier layer.
    let mut orphaned = s.clone();
    assert!(orphaned.remove_effect(EffectId::new(anchor_id)));
    let good_orphan = wire_of(&orphaned);
    assert_eq!(decode_wire(&good_orphan).unwrap(), orphaned);
    let mut w = good_orphan.clone();
    let i = index(&w, part_id);
    w.effects.effects[i].effect.affected = AffectedWire::SameAsLayer(Layer::L7d);
    assert_decode_err(decode_wire(&w));
    // A Some(power) on the wire is an array of at most one element.
    let mut w = good.clone();
    let ModificationWire::CopyValuesWithProvenance {
        characteristics, ..
    } = &mut w.effects.effects[n - 1].effect.modification
    else {
        panic!("the copy effect is last");
    };
    characteristics.power = Some(literal(1));
    assert!(decode_wire(&w).is_ok());
}

// REQ-CORE-001, REQ-DSL-003 (#232: replaces "Some(JSON null) of a power stays distinct from
// None", a placeholder the typed power can no longer hold): Some(X) of a power stays distinct
// from None through the wire, and a two-element option is rejected.
#[test]
fn wire_keeps_some_power_distinct() {
    assert!(serde_json::from_value::<ValueExpressionAST>(serde_json::Value::Null).is_err());
    let mut s = GameState::new(2, 1, 20).unwrap();
    let ts = s.stamp_resolved_effect().unwrap();
    let mut c = creature(vec![]);
    c.power = Some(ValueExpressionAST::X);
    c.toughness = None;
    s.register_effect(
        resolved_effect(
            ts,
            Layer::L1a,
            only(&[1]),
            LayerModification::CopyValues(Box::new(c)),
        ),
        None,
    )
    .unwrap();
    check_round_trip(&s);
    // Two elements where at most one is allowed: rejected.
    let bytes = s.serialize().unwrap();
    let one = [0x91_u8, 0xa1, b'X'];
    let at = bytes
        .windows(one.len())
        .position(|w| w == one)
        .expect("Some(X) is [\"X\"]");
    let mut two = bytes.clone();
    two.splice(at..at + 3, [0x92, 0xa1, b'X', 0xa1, b'X']);
    assert_decode_err(GameState::deserialize(&two));
}

// R12: decoding random bytes, truncations and byte flips of populated version-2 states never
// panics; anything accepted round-trips.
#[test]
fn wire_survives_hostile_layer_bytes() {
    let mut rng = SplitMix64::new(0x0183_0005);
    let mut samples: Vec<Vec<u8>> = (0..6)
        .map(|_| {
            let mut s = GameState::new(rng.next_u64(), 2, 20).unwrap();
            populate_layers(&mut rng, &mut s, 30);
            s.serialize().unwrap()
        })
        .collect();
    samples.push(rich_state().0.serialize().unwrap());
    let survive = |b: &[u8]| match GameState::deserialize(b) {
        Ok(s) => check_round_trip(&s),
        Err(e) => assert!(matches!(e, StateError::Decode(_)), "{e:?}"),
    };
    for b in &samples {
        for len in (0..b.len()).step_by(7) {
            assert_decode_err(GameState::deserialize(&b[..len]));
        }
    }
    for _ in 0..4_000 {
        let mut b = pick(&mut rng, &samples);
        let at = usize::try_from(below(&mut rng, b.len() as u64)).unwrap();
        b[at] ^= u8::try_from(1 + below(&mut rng, 255)).unwrap();
        survive(&b);
    }
    for _ in 0..2_000 {
        let len = usize::try_from(below(&mut rng, 600)).unwrap();
        let b: Vec<u8> = (0..len).map(|_| rng.next_u64().to_le_bytes()[0]).collect();
        survive(&b);
    }
}

// REQ-CORE-001: Display of the new StateError variants; GeneratingAbility basics.
#[test]
fn layer_state_error_display() {
    let cases = [
        (
            StateError::Layer(LayerError::IdExhausted),
            "layer system: timestamp space exhausted",
        ),
        (
            StateError::NonCdaInLayer7a,
            "layer 7a holds only characteristic-defining abilities (CR 613.4a)",
        ),
        (
            StateError::MissingGenerator,
            "a static effect with a source needs its generating ability",
        ),
        (
            StateError::GeneratorSourceMismatch,
            "the generating ability is not on the effect's source",
        ),
        (
            StateError::UnexpectedGenerator,
            "only a static effect with a source has a generating ability",
        ),
        (
            StateError::EffectIdNotIssued(e(4)),
            "effect id 4 was not issued by this state",
        ),
        (
            StateError::AlreadyLinked(e(4)),
            "effect 4 already has a generating ability",
        ),
        (
            StateError::EffectRegistered(e(4)),
            "effect 4 is a registered continuous effect",
        ),
    ];
    for (err, text) in cases {
        assert_eq!(err.to_string(), text);
    }
    let g = GeneratingAbility::new(o(1), ability("d"));
    assert!(format!("{g:?}").contains("GeneratingAbility"));
    assert_eq!(g.clone(), g);
    let fresh = GameState::new(1, 1, 1).unwrap();
    let links: &BTreeMap<EffectId, GeneratingAbility> = fresh.effect_generators();
    assert!(links.is_empty());
}

// REQ-CORE-001, REQ-NFR-006, REQ-LAYER-001: every modification, copy exception and copied
// characteristic, and an orphaned SameAsLayer part after every earlier layer (CR 613.6),
// survive the version-2 wire by value and hash.
#[test]
fn wire_round_trips_every_variant() {
    let mut s = GameState::new(6, 2, 20).unwrap();
    let ts = s.stamp_resolved_effect().unwrap();
    let mut modifications: Vec<(Layer, LayerModification)> = Vec::new();
    let mut rng = SplitMix64::new(0x0183_0006);
    while modifications.len() < 400 {
        modifications.push(any_modification(&mut rng));
    }
    let every_exception: Vec<CopyException> = [
        CopyException::GainAbility(ability("g")),
        CopyException::SetNames(set(["N"])),
        CopyException::SetManaCost(None),
        CopyException::SetColors(ColorBitmask::RED),
        CopyException::SetSupertypes(set(["N"])),
        CopyException::SetCardTypes(set(["N"])),
        CopyException::SetSubtypes(set(["N"])),
        CopyException::AddSupertypes(set(["N"])),
        CopyException::AddCardTypes(set(["N"])),
        CopyException::AddSubtypes(set(["N"])),
        CopyException::SetPower(literal(1)),
        CopyException::SetToughness(literal(1)),
    ]
    .into_iter()
    .chain(
        [
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
        ]
        .into_iter()
        .map(CopyException::NotCopied),
    )
    .collect();
    modifications.push((
        Layer::L1a,
        LayerModification::ApplyCopyExceptions(every_exception),
    ));
    for (source, cda) in [
        (ValueSource::Plain, false),
        (ValueSource::CopyOfBackFaceOrMelded, true),
    ] {
        modifications.push((
            Layer::L1a,
            LayerModification::CopyValuesWithProvenance {
                characteristics: Box::new(creature(vec![])),
                provenance: provenance_from_parts(source, false, cda).unwrap(),
            },
        ));
    }
    for (layer, m) in modifications {
        s.register_effect(resolved_effect(ts, layer, only(&[1]), m), None)
            .unwrap();
    }
    // An anchor in every layer before 7d, a 7d part linked to it; the anchor then removed.
    let anchors: [(Layer, LayerModification); 10] = [
        (
            Layer::L1a,
            LayerModification::ApplyCopyExceptions(Vec::new()),
        ),
        (
            Layer::L1b,
            LayerModification::FaceDown(Box::new(creature(vec![]))),
        ),
        (Layer::L2, LayerModification::SetController(p(2))),
        (
            Layer::L3,
            LayerModification::SetText(TextTokenStream::default()),
        ),
        (
            Layer::L4,
            LayerModification::RemoveSubtypes(BTreeSet::new()),
        ),
        (Layer::L5, LayerModification::AddColors(ColorBitmask::RED)),
        (Layer::L6, LayerModification::RemoveAllAbilities),
        (
            Layer::L7a,
            LayerModification::SetPt {
                power: Some(1),
                toughness: Some(1),
            },
        ),
        (
            Layer::L7b,
            LayerModification::SetPt {
                power: Some(1),
                toughness: Some(1),
            },
        ),
        (
            Layer::L7c,
            LayerModification::ModifyPt {
                power: 1,
                toughness: 1,
            },
        ),
    ];
    for (layer, m) in anchors {
        let mut anchor = resolved_effect(ts, layer, only(&[2]), m);
        anchor.is_cda = layer == Layer::L7a;
        let part = resolved_effect(
            ts,
            Layer::L7d,
            Affected::SameAsLayer(layer),
            LayerModification::SwitchPt,
        );
        let ids = s
            .register_effect_group(&NoWorld, vec![(anchor, None), (part, None)])
            .unwrap();
        check_round_trip(&s);
        assert!(s.remove_effect(ids[0]));
    }
    check_round_trip(&s);
}

// ---------------------------------------------------------------------------------------------
// #232: typed DSL values through the MessagePack wire
// ---------------------------------------------------------------------------------------------

/// `v` decoded as `T` (valid synthetic content).
fn typed<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> T {
    serde_json::from_value(v).unwrap()
}

/// Valid synthetic abilities of all six kinds (R5).
fn typed_abilities() -> Vec<AbilityAST> {
    [
        serde_json::json!({"abilityId": "w1", "kind": "SPELL", "effect": [{"op": "POPULATE"}]}),
        serde_json::json!({"abilityId": "w2", "kind": "ACTIVATED",
            "cost": [{"cost": "TAP_SOURCE"}], "effect": [{"op": "POPULATE"}]}),
        serde_json::json!({"abilityId": "w3", "kind": "TRIGGERED",
            "trigger": {"mode": "STATE", "state": {"pred": "IS_CREATURE"}},
            "effect": [{"op": "POPULATE"}]}),
        serde_json::json!({"abilityId": "w4", "kind": "STATIC",
            "effects": [{"type": "RULE", "kind": "SKIP"}]}),
        serde_json::json!({"abilityId": "w5", "kind": "REPLACEMENT",
            "match": {"event": "DIES", "affected": {"ref": "SELF"}},
            "replaceWith": [{"op": "SKIP"}]}),
        serde_json::json!({"abilityId": "w6", "kind": "KEYWORD", "keyword": "TEST_KEYWORD",
            "functionsIn": ["BATTLEFIELD"], "textSpan": [0, 1], "params": {"n": 2, "s": "x"}}),
    ]
    .into_iter()
    .map(typed)
    .collect()
}

/// A valid synthetic token stream with one token of each kind (R5).
fn typed_text() -> TextTokenStream {
    typed(serde_json::json!([
        {"id": "t1", "kind": "NAME_REF", "ref": "SELF"},
        {"id": "t2", "kind": "LITERAL", "text": " gets +1/+1"},
        {"id": "t3", "kind": "WORD", "category": "CREATURE_TYPE", "value": "Testkin"},
        {"id": "t4", "kind": "NAME_REF", "ref": "OTHER", "name": "test.other"}
    ]))
}

/// A valid non-literal value expression.
fn typed_math() -> ValueExpressionAST {
    typed(serde_json::json!({"MATH": {
        "op": "DIV_FLOOR",
        "lhs": {"MATH": {"op": "MUL", "lhs": "X", "rhs": {"LITERAL": 2}}},
        "rhs": {"LITERAL": 3}
    }}))
}

// REQ-DSL-003, REQ-DSL-010, REQ-DSL-012, REQ-CORE-001, REQ-NFR-006 (#232): every wire type that
// holds typed ASTs (SetText, AddAbility, GainAbility, SetPower, SetToughness and the
// characteristics snapshots of CopyValues, CopyValuesWithProvenance and FaceDown, and the
// generating ability links) round-trips through its MessagePack encoding to an equal value
// with the same state hash.
#[test]
fn typed_dsl_values_round_trip_through_the_wire() {
    let mut s = GameState::new(5, 2, 20).unwrap();
    let mut snapshot = creature(typed_abilities());
    snapshot.rules_text = typed_text();
    snapshot.power = Some(typed_math());
    snapshot.toughness = Some(ValueExpressionAST::X);
    let mut modifications = vec![
        (Layer::L3, LayerModification::SetText(typed_text())),
        (
            Layer::L3,
            LayerModification::SetText(TextTokenStream::default()),
        ),
        (
            Layer::L1a,
            LayerModification::CopyValues(Box::new(snapshot.clone())),
        ),
        (
            Layer::L1a,
            LayerModification::CopyValuesWithProvenance {
                characteristics: Box::new(snapshot.clone()),
                provenance: crate::layers::ManaValueProvenance::plain(),
            },
        ),
        (Layer::L1b, LayerModification::FaceDown(Box::new(snapshot))),
        (
            Layer::L1a,
            LayerModification::ApplyCopyExceptions(
                typed_abilities()
                    .into_iter()
                    .map(CopyException::GainAbility)
                    .chain([
                        CopyException::SetPower(typed_math()),
                        CopyException::SetToughness(literal(-4)),
                        CopyException::SetPower(ValueExpressionAST::Memory("test.m".to_owned())),
                    ])
                    .collect(),
            ),
        ),
    ];
    modifications.extend(
        typed_abilities()
            .into_iter()
            .map(|a| (Layer::L6, LayerModification::AddAbility(a))),
    );
    for (layer, m) in modifications {
        let ts = s.stamp_resolved_effect().unwrap();
        s.register_effect(resolved_effect(ts, layer, only(&[1]), m), None)
            .unwrap();
    }
    for (i, a) in typed_abilities().into_iter().enumerate() {
        let id = s.issue_effect_id().unwrap();
        s.link_effect_generator(id, GeneratingAbility::new(o(10 + i as u64), a))
            .unwrap();
    }
    check_round_trip(&s);
    let back = GameState::deserialize(&s.serialize().unwrap()).unwrap();
    assert_eq!(back.effects(), s.effects());
}
