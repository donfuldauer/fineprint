// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the "layers into GameState" spec tests (REQ-LAYER-001, REQ-LAYER-002,
//! REQ-CORE-001, REQ-CORE-002, REQ-CORE-006, REQ-NFR-006, REQ-DSL-004). Black-box: written
//! from the work-item brief (#183 FP-144) only.
//!
//! Each `spec_layers_state_*.rs` test file includes this file with `#[path]`; on its own it is
//! an empty test target. Every ability and object here is synthetic and neutrally named (R5).

#![allow(dead_code)]

#[path = "typed_ast_support/mod.rs"]
mod typed_ast_support;
use typed_ast_support::{synthetic_ability, tx};

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::{ComponentPath, Label};
use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_base::prng::SplitMix64;
use fineprint_core::layers::{
    Affected, ContinuousEffect, CounterHolder, EffectOrigin, LayerModification, MatchContext,
    ObjectBase, SimultaneousStamp, StampReason,
};
use fineprint_core::objects::Characteristics;
use fineprint_core::state::{
    GameEnvironment, GameState, GeneratingAbility, ObjectWorld, StateError,
};
pub use fineprint_dsl::ability::Layer;
pub use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::{ObjectSelectorAST, ReferenceAST};

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);
pub const P3: PlayerId = PlayerId::new(3);

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn eid(n: u64) -> EffectId {
    EffectId::new(n)
}

pub fn ts(n: u64) -> Timestamp {
    Timestamp::new(n)
}

pub fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

pub fn objs(ids: &[u64]) -> BTreeSet<ObjectId> {
    ids.iter().map(|&n| oid(n)).collect()
}

// ---------------------------------------------------------------------------------------------
// synthetic abilities and characteristics
// ---------------------------------------------------------------------------------------------

/// A synthetic ability identified by value (#232 FP-164: a valid inert KEYWORD ability instead
/// of the former free-form `{"synthetic": name}` placeholder).
pub fn ability(name: &str) -> AbilityAST {
    synthetic_ability(name)
}

/// The marker ability granted in the PR #187 scenario.
pub fn marker() -> AbilityAST {
    ability("test.marker")
}

/// An object with the given name, card types and abilities, nothing else.
pub fn chars(name: &str, types: &[&str], abilities: Vec<AbilityAST>) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: set(types),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream::default(),
        abilities,
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

pub fn has_ability(c: &Characteristics, a: &AbilityAST) -> bool {
    c.abilities.iter().any(|x| x == a)
}

// ---------------------------------------------------------------------------------------------
// selectors
// ---------------------------------------------------------------------------------------------

/// An opaque selector key.
pub fn sel(slot: u32) -> ObjectSelectorAST {
    ObjectSelectorAST::Ref(ReferenceAST::Target { slot })
}

/// Matches objects that currently have no abilities.
pub const SEL_NO_ABILITIES: u32 = 1;
/// Matches every object.
pub const SEL_ALL: u32 = 2;
/// Matches nothing.
pub const SEL_NONE: u32 = 3;
/// Matches creatures.
pub const SEL_CREATURE: u32 = 4;

// ---------------------------------------------------------------------------------------------
// the ObjectWorld test double
// ---------------------------------------------------------------------------------------------

/// A synthetic object world over ordered maps.
#[derive(Clone, Debug, Default)]
pub struct World {
    pub bases: BTreeMap<ObjectId, ObjectBase>,
    pub counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
}

impl World {
    pub fn new() -> World {
        World::default()
    }

    /// Adds a battlefield permanent controlled by `controller`.
    pub fn put(&mut self, id: ObjectId, c: Characteristics, controller: PlayerId) {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(controller),
                on_battlefield: true,
            },
        );
    }

    /// Adds an object that is not on the battlefield (an entering permanent).
    pub fn put_off(&mut self, id: ObjectId, c: Characteristics, controller: PlayerId) {
        self.bases.insert(
            id,
            ObjectBase {
                characteristics: c,
                controller: Some(controller),
                on_battlefield: false,
            },
        );
    }

    pub fn counter(&mut self, id: ObjectId, kind: &str, amount: i64) {
        self.counters
            .entry(id)
            .or_default()
            .insert(kind.to_string(), amount);
    }
}

impl ObjectWorld for World {
    fn objects(&self) -> Vec<ObjectId> {
        self.bases.keys().copied().collect()
    }

    fn base(&self, id: ObjectId) -> Option<ObjectBase> {
        self.bases.get(&id).cloned()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn matches(&self, selector: &ObjectSelectorAST, ctx: &MatchContext<'_>) -> bool {
        let c = &ctx.candidate_view.characteristics;
        if *selector == sel(SEL_NO_ABILITIES) {
            c.abilities.is_empty()
        } else if *selector == sel(SEL_ALL) {
            true
        } else if *selector == sel(SEL_CREATURE) {
            c.card_types.contains("Creature")
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------------------------
// effects
// ---------------------------------------------------------------------------------------------

/// A continuous effect with every field given.
#[allow(clippy::too_many_arguments)]
pub fn effect(
    source: Option<ObjectId>,
    controller: PlayerId,
    timestamp: Timestamp,
    layer: Layer,
    is_cda: bool,
    origin: EffectOrigin,
    affected: Affected,
    modification: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source,
        controller,
        timestamp,
        layer,
        is_cda,
        origin,
        affected,
        modification,
    }
}

/// A resolved effect affecting the locked set `objects`.
pub fn resolved(
    timestamp: Timestamp,
    layer: Layer,
    objects: &[u64],
    modification: LayerModification,
) -> ContinuousEffect {
    effect(
        None,
        P1,
        timestamp,
        layer,
        false,
        EffectOrigin::Resolved,
        Affected::Objects(objs(objects)),
        modification,
    )
}

/// A static effect of `source`'s own (printed) ability with a selector.
pub fn static_sel(
    source: ObjectId,
    timestamp: Timestamp,
    layer: Layer,
    slot: u32,
    modification: LayerModification,
) -> ContinuousEffect {
    effect(
        Some(source),
        P1,
        timestamp,
        layer,
        false,
        EffectOrigin::Static { granted_at: None },
        Affected::Selector(sel(slot)),
        modification,
    )
}

// ---------------------------------------------------------------------------------------------
// hash leaves
// ---------------------------------------------------------------------------------------------

pub fn leaf_path(labels: &[&str]) -> ComponentPath {
    ComponentPath::new(labels.iter().map(|l| Label::name(l)).collect())
}

/// The payload of the leaf at `labels` (names only).
pub fn leaf(s: &GameState, labels: &[&str]) -> Option<Vec<u8>> {
    s.merkle_tree()
        .leaf_payload(&leaf_path(labels))
        .map(<[u8]>::to_vec)
}

/// Every pre-existing (format version 1) leaf of `s` keyed by its path text
/// (`"ids/effect"`, `"players/1/life"`), with its payload.
pub fn old_leaves(s: &GameState) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut paths: Vec<(String, ComponentPath)> = Vec::new();
    paths.push(("prng".into(), leaf_path(&["prng"])));
    for k in [
        "card",
        "object",
        "ability_instance",
        "effect",
        "timestamp",
        "link",
        "player",
    ] {
        paths.push((format!("ids/{k}"), leaf_path(&["ids", k])));
    }
    for k in ["number", "active_player", "priority_player", "order"] {
        paths.push((format!("turn/{k}"), leaf_path(&["turn", k])));
    }
    for p in s.players() {
        for k in ["life", "counters", "designations"] {
            paths.push((
                format!("players/{}/{k}", p.id().get()),
                ComponentPath::new(vec![
                    Label::name("players"),
                    Label::index(p.id().get()),
                    Label::name(k),
                ]),
            ));
        }
    }
    paths.push(("history".into(), leaf_path(&["history"])));
    let tree = s.merkle_tree();
    paths
        .into_iter()
        .map(|(name, p)| {
            let payload = tree.leaf_payload(&p).map(<[u8]>::to_vec);
            (name, payload)
        })
        .collect()
}

/// Every leaf of `s` (old and new), keyed by path text.
pub fn all_leaves(s: &GameState) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut m = old_leaves(s);
    for l in NEW_LEAVES {
        m.insert(l.to_string(), leaf(s, &[l]));
    }
    m
}

/// The path texts of the leaves whose payloads differ between `a` and `b`.
pub fn changed_leaves(a: &GameState, b: &GameState) -> BTreeSet<String> {
    let la = all_leaves(a);
    let lb = all_leaves(b);
    la.keys()
        .chain(lb.keys())
        .filter(|k| la.get(*k) != lb.get(*k))
        .cloned()
        .collect()
}

/// A set of path texts.
pub fn names(items: &[&str]) -> BTreeSet<String> {
    set(items)
}

/// The three leaves this work item adds.
pub const NEW_LEAVES: [&str; 3] = ["effects", "timestamps", "effect_generators"];

// ---------------------------------------------------------------------------------------------
// wire manipulation (MessagePack <-> serde_json::Value)
// ---------------------------------------------------------------------------------------------

/// Decodes serialized state bytes into a generic JSON value (map with named members).
pub fn wire(bytes: &[u8]) -> serde_json::Value {
    rmp_serde::from_slice(bytes).expect("state bytes decode as a generic value")
}

/// Re-encodes a generic value as named MessagePack.
pub fn unwire(v: &serde_json::Value) -> Vec<u8> {
    rmp_serde::to_vec_named(v).expect("generic value encodes")
}

pub fn is_decode_err(r: &Result<GameState, StateError>) -> bool {
    matches!(r, Err(StateError::Decode(_)))
}

// ---------------------------------------------------------------------------------------------
// deterministic test PRNG (fixed seeds only)
// ---------------------------------------------------------------------------------------------

pub struct Rng(SplitMix64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(SplitMix64::new(seed))
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

// ---------------------------------------------------------------------------------------------
// shared states
// ---------------------------------------------------------------------------------------------

/// A two-player state with four object ids and three timestamps issued.
pub fn base() -> GameState {
    let mut s = GameState::new(7, 2, 20).unwrap();
    for _ in 0..4 {
        s.issue_object_id().unwrap();
    }
    for _ in 0..3 {
        s.issue_timestamp().unwrap();
    }
    s
}

/// The world of [`base`]'s objects: o1..o4, each with one own synthetic ability.
pub fn world() -> World {
    let mut w = World::new();
    for n in 1..=4u64 {
        w.put(
            oid(n),
            chars(
                &format!("test.object_{n}"),
                &["Creature"],
                vec![ability(&format!("test.o{n}_static"))],
            ),
            P1,
        );
    }
    w
}

pub fn own_ability(n: u64) -> AbilityAST {
    ability(&format!("test.o{n}_static"))
}

/// A state with effects in every layer, a multi-layer group, an ETB link, and a ledger with
/// objects, counters, stickers and simultaneous stamps.
pub fn rich_state() -> GameState {
    let mut s = GameState::new(11, 3, 20).unwrap();
    s.set_active_player(Some(P2)).unwrap();
    for _ in 0..6 {
        s.issue_object_id().unwrap();
    }
    let t1 = s.stamp_object(oid(1), StampReason::EntersZone).unwrap();
    let t2 = s.stamp_object(oid(2), StampReason::EntersZone).unwrap();
    s.stamp_sticker(oid(2), 1).unwrap();
    s.stamp_sticker(oid(2), 7).unwrap();
    s.stamp_counters(CounterHolder::Object(oid(1)), "+1/+1")
        .unwrap();
    s.stamp_counters(CounterHolder::Player(P3), "test.player_counter")
        .unwrap();
    s.stamp_objects_simultaneously(
        &[
            SimultaneousStamp {
                object: oid(3),
                player: P1,
                choice: 0,
            },
            SimultaneousStamp {
                object: oid(4),
                player: P2,
                choice: 1,
            },
            SimultaneousStamp {
                object: oid(5),
                player: P2,
                choice: 0,
            },
        ],
        StampReason::EntersZone,
    )
    .unwrap();
    let rt = |s: &mut GameState| s.stamp_resolved_effect().unwrap();
    let mods: Vec<(Layer, LayerModification)> = vec![
        (
            Layer::L1a,
            LayerModification::CopyValues(Box::new(chars("test.copy", &["Creature"], vec![]))),
        ),
        (
            Layer::L1b,
            LayerModification::FaceDown(Box::new(chars("", &["Creature"], vec![]))),
        ),
        (Layer::L2, LayerModification::SetController(P2)),
        (
            Layer::L3,
            // #232 FP-164: a valid LITERAL token instead of the bare string `"test.word"`.
            LayerModification::SetText(tx(vec![
                serde_json::json!({"id": "w0", "kind": "LITERAL", "text": "test.word"}),
            ])),
        ),
        (
            Layer::L4,
            LayerModification::AddTypes {
                supertypes: set(&[]),
                card_types: set(&["Artifact"]),
                subtypes: set(&[]),
            },
        ),
        (Layer::L5, LayerModification::SetColors(ColorBitmask::GREEN)),
        (Layer::L6, LayerModification::RemoveKeyword("flying".into())),
        (
            Layer::L7b,
            LayerModification::SetPt {
                power: Some(0),
                toughness: Some(1),
            },
        ),
        (
            Layer::L7c,
            LayerModification::ModifyPt {
                power: 1,
                toughness: -1,
            },
        ),
        (Layer::L7d, LayerModification::SwitchPt),
    ];
    for (layer, m) in mods {
        let t = rt(&mut s);
        s.register_effect(resolved(t, layer, &[1, 6], m), None)
            .unwrap();
    }
    // Static, linked: a layer-6 grant of o1 and a layer-7a characteristic-defining effect of o2
    // (CR 613.4a).
    s.register_effect(
        static_sel(
            oid(1),
            t1,
            Layer::L6,
            SEL_CREATURE,
            LayerModification::AddAbility(marker()),
        ),
        Some(GeneratingAbility::new(oid(1), own_ability(1))),
    )
    .unwrap();
    let mut cda = static_sel(
        oid(2),
        t2,
        Layer::L7a,
        SEL_NONE,
        LayerModification::SetPt {
            power: Some(3),
            toughness: Some(3),
        },
    );
    cda.is_cda = true;
    cda.affected = Affected::Objects(objs(&[2]));
    s.register_effect(cda, Some(GeneratingAbility::new(oid(2), own_ability(2))))
        .unwrap();
    // A multi-layer group bypassing register_effect.
    let a = s.issue_effect_id().unwrap();
    let b = s.issue_effect_id().unwrap();
    let t = rt(&mut s);
    let w = world();
    let snapshot = s.clone();
    let env = GameEnvironment::new(&snapshot, &w);
    s.effects_mut()
        .register_group(
            &env,
            vec![
                (
                    a,
                    resolved(
                        t,
                        Layer::L4,
                        &[3],
                        LayerModification::AddTypes {
                            supertypes: set(&[]),
                            card_types: set(&["Artifact"]),
                            subtypes: set(&[]),
                        },
                    ),
                ),
                (
                    b,
                    effect(
                        None,
                        P1,
                        t,
                        Layer::L7b,
                        false,
                        EffectOrigin::Resolved,
                        Affected::SameAsLayer(Layer::L4),
                        LayerModification::SetPt {
                            power: Some(2),
                            toughness: Some(2),
                        },
                    ),
                ),
            ],
        )
        .unwrap();
    // An ETB replacement effect id linked to its generating ability.
    let etb = s.issue_effect_id().unwrap();
    s.link_effect_generator(etb, GeneratingAbility::new(oid(4), own_ability(4)))
        .unwrap();
    s
}

/// A small non-empty state: one linked static effect, one stamped object, one ETB link.
pub fn small_state() -> GameState {
    let mut s = GameState::new(3, 2, 20).unwrap();
    s.issue_object_id().unwrap();
    let t = s.stamp_object(oid(1), StampReason::EntersZone).unwrap();
    s.register_effect(
        static_sel(
            oid(1),
            t,
            Layer::L6,
            SEL_NO_ABILITIES,
            LayerModification::AddAbility(marker()),
        ),
        Some(GeneratingAbility::new(oid(1), own_ability(1))),
    )
    .unwrap();
    let etb = s.issue_effect_id().unwrap();
    s.link_effect_generator(etb, GeneratingAbility::new(oid(1), ability("test.etb")))
        .unwrap();
    s
}
