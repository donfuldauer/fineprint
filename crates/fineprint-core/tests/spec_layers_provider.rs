// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `LayeredProvider`, the layer system's implementation of the LKI hook
//! `memory::CharacteristicsProvider` (REQ-LAYER-001, REQ-CORE-003 consumer). Black-box:
//! written from the work-item brief only.

#[path = "spec_layers_support.rs"]
mod support;

use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};
use fineprint_core::layers::{ContinuousEffects, LayerModification, LayeredProvider, evaluate};
use fineprint_core::memory::{
    CharacteristicsProvider, LkiStore, MemoryError, MemoryTable, capture_lki_on_move,
};
use fineprint_core::permanent::Permanents;
use fineprint_core::zones::{MoveRequest, ZoneKind, Zones, ZonesConfig};
use support::*;

/// Zones, permanents and memories for two players.
struct World {
    zones: Zones,
    ids: IdGenerator<ObjectId>,
    permanents: Permanents,
    memories: MemoryTable,
    lki: LkiStore,
    next_card: u64,
}

impl World {
    fn new() -> World {
        World {
            zones: Zones::new(&[P1, P2], ZonesConfig::default()),
            ids: IdGenerator::new(),
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            next_card: 1,
        }
    }

    fn create(&mut self, kind: ZoneKind, owner: PlayerId) -> ObjectId {
        let card = CardId::new(self.next_card);
        self.next_card += 1;
        let id = self
            .zones
            .create_object(&mut self.ids, card, owner, MoveRequest::new(kind))
            .expect("create succeeds");
        if kind == ZoneKind::Battlefield {
            self.permanents.enter(id, owner, 1).expect("enter succeeds");
        }
        id
    }

    fn capture(
        &mut self,
        provider: &dyn CharacteristicsProvider,
        id: ObjectId,
        req: &MoveRequest,
    ) -> Result<bool, MemoryError> {
        capture_lki_on_move(
            &self.zones,
            &self.permanents,
            &self.memories,
            provider,
            &mut self.lki,
            id,
            req,
        )
    }
}

/// A battlefield creature with layer-4, layer-6 and layer-7c effects on it, and an attached
/// object.
fn setup() -> (World, TestEnv, ContinuousEffects, ObjectId, ObjectId) {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let aura = w.create(ZoneKind::Battlefield, P1);
    let mut env = TestEnv::new();
    env.put(
        a,
        with_abilities(
            creature("test.lki_subject_2_2", 2, 2),
            vec![static_ability("test.own"), kw_ability("k", "reach")],
        ),
    );
    env.put(aura, enchantment("test.attached_aura"));
    env.attached.insert(a, vec![aura]);
    env.counter(a, "+1/+1", 2, 1);
    env.counter(a, "test.zero", 0, 1);
    env.counter(a, "test.negative", -1, 1);
    let effects = registry(&[
        (
            1,
            eff(Layer::L4, 2, objs(&[a]), add_card_types(&["Artifact"])),
        ),
        (
            2,
            eff(
                Layer::L6,
                3,
                objs(&[a]),
                LayerModification::RemoveKeyword("reach".into()),
            ),
        ),
        (3, eff(Layer::L7c, 4, objs(&[a]), modify_pt(1, 0))),
    ]);
    (w, env, effects, a, aura)
}

// REQ-LAYER-001, REQ-CORE-003, CR 608.2h: LKI captured through LayeredProvider holds the
// evaluated characteristics (layers 4, 6, 7c and counters applied), not the base.
#[test]
fn lki_capture_stores_evaluated_characteristics() {
    let (mut w, env, effects, a, aura) = setup();
    let provider = LayeredProvider::new(&effects, &env);
    let expected = evaluate(&effects, &env, a).unwrap().characteristics;
    assert_ne!(norm(&expected), *env.base_chars(a));
    assert_eq!(expected.card_types, set(&["Artifact", "Creature"]));
    assert!(!has_kw(&expected, "reach"));

    let req = MoveRequest::new(ZoneKind::Graveyard);
    assert_eq!(w.capture(&provider, a, &req), Ok(true));
    w.zones
        .move_object(&mut w.ids, a, req)
        .expect("move succeeds");
    let record = w.lki.get(a).expect("record captured");
    assert_eq!(*record.characteristics(), expected);
    assert_ne!(norm(record.characteristics()), *env.base_chars(a));
    let lit_pt = (
        record.characteristics().power.as_ref().and_then(literal_of),
        record
            .characteristics()
            .toughness
            .as_ref()
            .and_then(literal_of),
    );
    assert_eq!(lit_pt, spt(5, 4));
    assert_eq!(record.attached_here(), &[aura]);
    assert_eq!(record.counters().get("+1/+1"), Some(&2));
    assert_eq!(record.counters().len(), 1);
}

// REQ-LAYER-001, REQ-CORE-003: the provider's characteristics equal `evaluate`'s; counters
// are the environment's filtered to amounts of at least 1; attached_here passes through.
#[test]
fn provider_methods_follow_the_environment() {
    let (_w, env, effects, a, aura) = setup();
    let provider = LayeredProvider::new(&effects, &env);
    assert_eq!(
        provider.characteristics(a),
        Some(evaluate(&effects, &env, a).unwrap().characteristics)
    );
    assert_eq!(
        provider.characteristics(aura),
        Some(env.base_chars(aura).clone())
    );
    let counters = provider.counters(a);
    assert_eq!(counters.len(), 1);
    assert_eq!(counters.get("+1/+1"), Some(&2));
    assert_eq!(provider.attached_here(a), vec![aura]);
    assert_eq!(provider.attached_here(aura), Vec::<ObjectId>::new());
    assert!(provider.counters(aura).is_empty());
}

// REQ-LAYER-001, REQ-CORE-003: an object the environment does not know gives None, and LKI
// capture of it fails with NoCharacteristics (the store is unchanged).
#[test]
fn unknown_object_gives_none_and_no_characteristics_error() {
    let (mut w, env, effects, _a, _aura) = setup();
    let stranger = w.create(ZoneKind::Battlefield, P2);
    let provider = LayeredProvider::new(&effects, &env);
    assert_eq!(provider.characteristics(stranger), None);
    assert_eq!(provider.characteristics(oid(999)), None);
    let req = MoveRequest::new(ZoneKind::Exile);
    assert_eq!(
        w.capture(&provider, stranger, &req),
        Err(MemoryError::NoCharacteristics(stranger))
    );
    assert!(w.lki.is_empty());
}

// REQ-LAYER-001, REQ-CORE-003: the provider reads the registry and environment it was built
// over; a different registry gives a different (still evaluated) answer.
#[test]
fn provider_reflects_its_registry() {
    let (_w, env, effects, a, _aura) = setup();
    let empty = ContinuousEffects::new();
    let p_empty = LayeredProvider::new(&empty, &env);
    let p_full = LayeredProvider::new(&effects, &env);
    let plain = p_empty.characteristics(a).unwrap();
    assert!(has_kw(&plain, "reach"));
    assert_eq!(plain.power.as_ref().and_then(literal_of), Some(4));
    assert!(!has_kw(&p_full.characteristics(a).unwrap(), "reach"));
}
