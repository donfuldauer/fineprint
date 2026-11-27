// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Consumers of object characteristics see the layer-evaluated view (#184 FP-145): REQ-LAYER-001
// (rules consumers read characteristics evaluated through layers 1a-7d, CR 613.1-613.4) and
// REQ-CORE-003 (LKI holds the evaluated characteristics, CR 608.2h).
//
// Two parts:
// - a source guard: every non-test line of each consumer file is free of base-characteristic
//   reads (`include_str!` at compile time, no runtime I/O, R3);
// - the shared scene the consumer regression tests (events, replacement, triggers, SBA) build:
//   a synthetic creature with an effect-granted layer-4 type, two layer-7b set-P/T effects in a
//   chosen timestamp order and a layer-7c modification. Synthetic names only (R5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{ObjectId, PlayerId, Timestamp};
use fineprint_dsl::ability::Layer;
use fineprint_dsl::card::{ColorBitmask, TextTokenStream};
use fineprint_dsl::expr::ObjectSelectorAST;

use super::CharacteristicsProvider;
use crate::events::ObjectMarkers;
use crate::layers::{
    Affected, ContinuousEffect, EffectOrigin, LayerModification, MatchContext, ObjectBase, literal,
    literal_of,
};
use crate::objects::Characteristics;
use crate::state::{GameState, ObjectWorld};
use crate::zones::{ZoneId, Zones};

// ---------------------------------------------------------------------------------------------
// Source guard
// ---------------------------------------------------------------------------------------------

/// The base-characteristic reads no consumer may contain outside its tests.
const FORBIDDEN: [&str; 3] = ["ObjectBase", ".base(", "LayerEnvironment::base"];

/// Every consumer of object characteristics or counters on the LKI, trigger, replacement-bus
/// and SBA ceasing paths, with its path for the failure message.
const CONSUMERS: [(&str, &str); 8] = [
    ("events/exec.rs", include_str!("../events/exec.rs")),
    ("events/state.rs", include_str!("../events/state.rs")),
    ("triggers/detect.rs", include_str!("../triggers/detect.rs")),
    ("triggers/event.rs", include_str!("../triggers/event.rs")),
    (
        "triggers/lookback.rs",
        include_str!("../triggers/lookback.rs"),
    ),
    ("triggers/state.rs", include_str!("../triggers/state.rs")),
    ("replacement/bus.rs", include_str!("../replacement/bus.rs")),
    ("sba/perform.rs", include_str!("../sba/perform.rs")),
];

/// The ETB projection, exempt from the guard: its `Projected` adaptor rewrites the base and
/// feeds it to `layers::evaluate` (CR 614.12).
const ETB: &str = include_str!("../replacement/etb.rs");

/// The code of `source` before its first `#[cfg(test)]` line, with every `//` comment removed
/// (whole comment lines and trailing comments), so documentation naming a forbidden item does
/// not count.
fn code_of(source: &str) -> String {
    let mut out = String::new();
    for line in source.lines() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        let code = line.find("//").map_or(line, |i| &line[..i]);
        out.push_str(code);
        out.push('\n');
    }
    out
}

/// The forbidden items `source`'s code contains.
fn base_reads(source: &str) -> Vec<&'static str> {
    let code = code_of(source);
    FORBIDDEN
        .iter()
        .copied()
        .filter(|f| code.contains(f))
        .collect()
}

// REQ-LAYER-001, REQ-CORE-003: CR 613.1, CR 608.2h. No consumer of object characteristics on
// the LKI, trigger, replacement-bus or SBA ceasing path reads base characteristics outside its
// tests; they all take the injected, layer-evaluated provider.
#[test]
fn req_layer_001_consumers_read_no_base_characteristics() {
    for (path, source) in CONSUMERS {
        assert!(
            source.contains("fn "),
            "{path}: the guard read an empty or wrong file"
        );
        assert_eq!(
            base_reads(source),
            Vec::<&str>::new(),
            "{path} reads base characteristics outside its tests"
        );
    }
}

// REQ-LAYER-001: CR 614.12, CR 613.1. The exempt ETB projection evaluates its rewritten base
// through the layer pipeline.
#[test]
fn req_layer_001_etb_projection_evaluates_through_layers() {
    assert!(code_of(ETB).contains("evaluate("));
}

// REQ-LAYER-001: the guard discriminates. A forbidden read in code is caught (also before a
// trailing comment); one in a comment, a doc comment or after the test marker is not.
#[test]
fn req_layer_001_guard_discriminates() {
    assert_eq!(base_reads("fn f() { env.base(id); }\n"), [".base("]);
    assert_eq!(
        base_reads("fn f() -> ObjectBase { x } // fine\n"),
        ["ObjectBase"]
    );
    assert_eq!(
        base_reads("fn f() { LayerEnvironment::base(e, id) }\n"),
        ["LayerEnvironment::base"]
    );
    assert!(base_reads("// env.base(id)\n/// ObjectBase\n//! LayerEnvironment::base\n").is_empty());
    assert!(base_reads("fn f() {} // ObjectBase\n").is_empty());
    assert!(base_reads("fn f() {}\n#[cfg(test)]\nfn g() { env.base(id); }\n").is_empty());
    assert!(!code_of("// evaluate(\n").contains("evaluate("));
}

// ---------------------------------------------------------------------------------------------
// The shared scene (consumer regression tests)
// ---------------------------------------------------------------------------------------------

/// The name of the scene's synthetic creature (base 2/2).
pub(crate) const SCENE_NAME: &str = "test.lki_granted_type_2_2";

/// The card type the scene's layer-4 effect grants.
pub(crate) const GRANTED_TYPE: &str = "Artifact";

/// The subtype the scene's layer-4 effect grants.
pub(crate) const GRANTED_SUBTYPE: &str = "Testgrant";

/// The base characteristics of the scene's creature: a 2/2 creature, subtype `Testkin`.
pub(crate) fn scene_base() -> Characteristics {
    Characteristics {
        names: BTreeSet::from([SCENE_NAME.to_owned()]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: BTreeSet::from(["Creature".to_owned()]),
        subtypes: BTreeSet::from(["Testkin".to_owned()]),
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

/// Whether `c` has the granted card type and subtype.
pub(crate) fn has_granted_type(c: &Characteristics) -> bool {
    c.card_types.contains(GRANTED_TYPE) && c.subtypes.contains(GRANTED_SUBTYPE)
}

/// The literal power and toughness of `c`.
pub(crate) fn pt_of(c: &Characteristics) -> Option<(i64, i64)> {
    Some((
        literal_of(c.power.as_ref()?)?,
        literal_of(c.toughness.as_ref()?)?,
    ))
}

/// A synthetic object world over a snapshot of `zones` and the object markers: every object has
/// the scene's base characteristics under its zone controller; counters are the markers'.
/// Selectors match nothing (the scene's effects list their objects).
///
/// It is also a base-only [`CharacteristicsProvider`], the control that proves a test
/// discriminates: a unit fake, never a rules consumer's provider.
#[derive(Clone, Debug, Default)]
pub(crate) struct SceneWorld {
    bases: BTreeMap<ObjectId, ObjectBase>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
}

impl SceneWorld {
    /// The world of every object in `zones`, counters from `markers`.
    pub(crate) fn of(zones: &Zones, markers: &ObjectMarkers) -> SceneWorld {
        let mut world = SceneWorld::default();
        for id in zones.all_object_ids() {
            let Some(record) = zones.object(id) else {
                continue;
            };
            world.bases.insert(
                id,
                ObjectBase {
                    characteristics: scene_base(),
                    controller: Some(record.controller),
                    on_battlefield: record.zone == ZoneId::Battlefield,
                },
            );
            world.counters.insert(id, markers.counters(id));
        }
        world
    }
}

impl ObjectWorld for SceneWorld {
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

impl CharacteristicsProvider for SceneWorld {
    /// The base characteristics: no layer applied.
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        self.bases.get(&id).map(|b| b.characteristics.clone())
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }
}

/// A resolved effect of player 1 on `target` (CR 611.2).
fn resolved(
    target: ObjectId,
    timestamp: Timestamp,
    layer: Layer,
    m: LayerModification,
) -> ContinuousEffect {
    ContinuousEffect {
        source: None,
        controller: PlayerId::new(1),
        timestamp,
        layer,
        is_cda: false,
        origin: EffectOrigin::Resolved,
        affected: Affected::Objects(BTreeSet::from([target])),
        modification: m,
    }
}

/// Registers the scene's effects on `target` in `game` and returns the evaluated power and
/// toughness they give a 2/2 without counters:
/// - layer 4 (CR 613.1d): adds the card type [`GRANTED_TYPE`] and the subtype
///   [`GRANTED_SUBTYPE`];
/// - layer 7b (CR 613.4b): A sets 4/4 and B sets 6/6; `a_older` gives A the earlier timestamp,
///   otherwise B has it (CR 613.7: within a layer, timestamp order), so the later one wins;
/// - layer 7c (CR 613.4c): +1/+1.
///
/// The effects are registered in the same order (A before B) either way: only the timestamps
/// swap. Returns 7/7 when `a_older` (B wins), 5/5 otherwise (A wins).
pub(crate) fn register_scene(game: &mut GameState, target: ObjectId, a_older: bool) -> (i64, i64) {
    let t1 = game.stamp_resolved_effect().unwrap();
    let t2 = game.stamp_resolved_effect().unwrap();
    let (t_a, t_b) = if a_older { (t1, t2) } else { (t2, t1) };
    let parts = [
        resolved(
            target,
            t1,
            Layer::L4,
            LayerModification::AddTypes {
                supertypes: BTreeSet::new(),
                card_types: BTreeSet::from([GRANTED_TYPE.to_owned()]),
                subtypes: BTreeSet::from([GRANTED_SUBTYPE.to_owned()]),
            },
        ),
        resolved(
            target,
            t_a,
            Layer::L7b,
            LayerModification::SetPt {
                power: Some(4),
                toughness: Some(4),
            },
        ),
        resolved(
            target,
            t_b,
            Layer::L7b,
            LayerModification::SetPt {
                power: Some(6),
                toughness: Some(6),
            },
        ),
        resolved(
            target,
            t2,
            Layer::L7c,
            LayerModification::ModifyPt {
                power: 1,
                toughness: 1,
            },
        ),
    ];
    for effect in parts {
        game.register_effect(effect, None).unwrap();
    }
    if a_older { (7, 7) } else { (5, 5) }
}
