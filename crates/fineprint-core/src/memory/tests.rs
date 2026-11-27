// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the memory module: REQ-CORE-003 (LKI), REQ-CORE-007 (Memory, Linked Store),
// REQ-CORE-002 (CR 400.7d and 400.7j registry bodies), REQ-CORE-001/REQ-CORE-006/REQ-NFR-006
// (determinism, canonical encoding). Synthetic ids and synthetic characteristics only.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::encode_to_vec;
use fineprint_base::ids::{CardId, IdGenerator, LinkId, ObjectId, PlayerId};
use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};

use super::*;
use crate::objects::Characteristics;
use crate::permanent::{AttachTarget, Permanents};
use crate::zones::{
    EXCEPTIONS, Exception400_7, ExceptionOutcome, MoveRequest, Moved, OutsidePool, Position,
    ZoneId, ZoneKind, Zones, ZonesConfig, exception_400_7d_spell_information,
    exception_400_7j_effect_finds_moved_object,
};

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn c(n: u64) -> CardId {
    CardId::new(n)
}

fn l(n: u64) -> LinkId {
    LinkId::new(n)
}

fn req(to: ZoneKind) -> MoveRequest {
    MoveRequest::new(to)
}

struct World {
    zones: Zones,
    ids: IdGenerator<ObjectId>,
    permanents: Permanents,
    memories: MemoryTable,
    lki: LkiStore,
    provider: TestProvider,
}

impl World {
    fn new() -> World {
        World {
            zones: Zones::new(&[p(1), p(2)], ZonesConfig::default()),
            ids: IdGenerator::new(),
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            provider: TestProvider::default(),
        }
    }

    /// Creates card `card` owned by player 1 in a zone of kind `kind`, with synthetic
    /// characteristics; on the battlefield it also enters the permanent container.
    fn create(&mut self, card: u64, kind: ZoneKind) -> ObjectId {
        let id = self
            .zones
            .create_object(&mut self.ids, c(card), p(1), req(kind))
            .unwrap();
        if kind == ZoneKind::Battlefield {
            self.permanents.enter(id, p(1), 1).unwrap();
        }
        self.provider.chars.insert(id, chars("test.flier_2_2"));
        id
    }

    fn capture(&mut self, id: ObjectId, r: &MoveRequest) -> Result<bool, MemoryError> {
        capture_lki_on_move(
            &self.zones,
            &self.permanents,
            &self.memories,
            &self.provider,
            &mut self.lki,
            id,
            r,
        )
    }

    /// Captures, then moves, then applies CR 400.7 to the Memory table.
    fn capture_and_move(&mut self, id: ObjectId, r: MoveRequest) -> Moved {
        self.capture(id, &r).unwrap();
        let moved = self.zones.move_object(&mut self.ids, id, r).unwrap();
        self.memories.apply_zone_change(&moved);
        moved
    }

    fn mv(&mut self, id: ObjectId, to: ZoneKind) -> Moved {
        let moved = self.zones.move_object(&mut self.ids, id, req(to)).unwrap();
        self.memories.apply_zone_change(&moved);
        moved
    }
}

#[derive(Default)]
struct TestProvider {
    chars: BTreeMap<ObjectId, Characteristics>,
    counters: BTreeMap<ObjectId, BTreeMap<String, i64>>,
    attached: BTreeMap<ObjectId, Vec<ObjectId>>,
}

impl CharacteristicsProvider for TestProvider {
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        self.chars.get(&id).cloned()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.get(&id).cloned().unwrap_or_default()
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.attached.get(&id).cloned().unwrap_or_default()
    }
}

/// A provider that uses the trait's default `counters` and `attached_here`.
struct CharsOnly(Characteristics);

impl CharacteristicsProvider for CharsOnly {
    fn characteristics(&self, _id: ObjectId) -> Option<Characteristics> {
        Some(self.0.clone())
    }
}

fn chars(name: &str) -> Characteristics {
    Characteristics {
        names: BTreeSet::from([name.to_owned()]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::NONE,
        supertypes: BTreeSet::new(),
        card_types: BTreeSet::from(["Creature".to_owned()]),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream::default(),
        abilities: Vec::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// A placeholder DSL tree decoded from MessagePack: a one-member map `key -> n`.
/// A synthetic keyword ability whose parameter `key` is `n` (#232: replaces the former
/// placeholder JSON object `{key: n}`).
fn tree(key: &str, n: u64) -> AbilityAST {
    let mut k = fineprint_dsl::ability::KeywordInstanceAST::new(
        fineprint_dsl::ability::AbilityBase::new("test.tree"),
        "TEST_KEYWORD",
    );
    k.params = Some(BTreeMap::from([(key.to_owned(), n.into())]));
    AbilityAST::Keyword(k)
}

/// A typed value expression nested past the canonical JSON depth budget (its encoding is the
/// MessagePack fallback).
fn deep_power() -> ValueExpressionAST {
    let mut v = ValueExpressionAST::Literal(1);
    for _ in 0..200 {
        v = ValueExpressionAST::Math {
            op: fineprint_dsl::expr::MathOp::Add,
            lhs: Box::new(v),
            rhs: Box::new(ValueExpressionAST::X),
        };
    }
    v
}

fn kicked_history() -> SpellHistory {
    SpellHistory {
        costs_paid: vec!["mana".to_owned(), "additional.kicker".to_owned()],
        mana_spent: BTreeMap::from([
            (("green".to_owned(), "land".to_owned()), 2),
            (("colorless".to_owned(), "artifact".to_owned()), 1),
        ]),
        additional_costs: BTreeMap::from([("kicker".to_owned(), true)]),
        x_value: Some(3),
        cast_face: Some("front".to_owned()),
        exiled_to_pay: vec![DurableRef::from_parts(c(90), ObjectId::new(900))],
    }
}

// ---------------------------------------------------------------------------------------------
// LKI capture (REQ-CORE-003)
// ---------------------------------------------------------------------------------------------

// REQ-CORE-003; CR 603.10, CR 608.2h: the record equals the object immediately before the
// event and never changes afterwards.
#[test]
fn lki_record_matches_pre_event_object_and_is_immutable() {
    let mut w = World::new();
    let id = w.create(1, ZoneKind::Battlefield);
    let aura = w.create(2, ZoneKind::Battlefield);
    let other = w.create(3, ZoneKind::Battlefield);
    w.permanents.set_tapped(id, true).unwrap();
    w.provider.counters.insert(
        id,
        BTreeMap::from([("+1/+1".to_owned(), 2), ("zero".to_owned(), 0)]),
    );
    w.provider.attached.insert(id, vec![other, aura, other]);
    w.memories
        .get_mut_or_default(id)
        .set_choice("color", ChoiceValue::Color("green".to_owned()));
    let r = req(ZoneKind::Graveyard);
    assert_eq!(w.capture(id, &r), Ok(true));
    let snapshot = w.lki.get(id).unwrap().clone();
    let rec = w.lki.get(id).unwrap();
    assert_eq!(rec.object(), id);
    assert_eq!(rec.card(), c(1));
    assert_eq!(rec.owner(), p(1));
    assert_eq!(rec.controller(), Some(p(1)));
    assert_eq!(rec.zone(), ZoneId::Battlefield);
    assert!(!rec.face_down());
    assert_eq!(rec.characteristics(), &chars("test.flier_2_2"));
    // Amounts below 1 are not counters.
    assert_eq!(rec.counters(), &BTreeMap::from([("+1/+1".to_owned(), 2)]));
    // Ascending, no repeats.
    let mut expected_attached = vec![aura, other];
    expected_attached.sort();
    assert_eq!(rec.attached_here(), expected_attached.as_slice());
    assert!(rec.permanent().unwrap().status().tapped);
    assert_eq!(
        rec.memory().choice("color"),
        Some(&ChoiceValue::Color("green".to_owned()))
    );

    // Change every source after the capture.
    let moved = w.zones.move_object(&mut w.ids, id, r).unwrap();
    w.memories.apply_zone_change(&moved);
    w.permanents.leave(id).unwrap();
    w.provider.chars.insert(id, chars("test.changed"));
    w.provider.counters.clear();
    w.provider.attached.clear();
    w.memories
        .get_mut_or_default(id)
        .set_choice("color", ChoiceValue::Color("blue".to_owned()));
    assert_eq!(w.lki.get(id).unwrap(), &snapshot);
    assert_eq!(
        w.lki.get(id).unwrap().memory().choice("color"),
        Some(&ChoiceValue::Color("green".to_owned()))
    );
}

// REQ-CORE-003; CR 109.4: off the battlefield and the stack the record has no controller and
// no permanent state; the face-down flag is captured.
#[test]
fn lki_of_non_permanent_has_no_controller_and_no_permanent_state() {
    let mut w = World::new();
    let id = w
        .zones
        .create_object(
            &mut w.ids,
            c(1),
            p(2),
            MoveRequest {
                face_down: true,
                ..req(ZoneKind::Exile)
            },
        )
        .unwrap();
    w.provider.chars.insert(id, chars("test.vanilla_1_1"));
    assert_eq!(w.capture(id, &req(ZoneKind::Graveyard)), Ok(true));
    let rec = w.lki.get(id).unwrap();
    assert_eq!(rec.controller(), None);
    assert_eq!(rec.owner(), p(2));
    assert!(rec.face_down());
    assert!(rec.permanent().is_none());
    assert_eq!(rec.memory(), &Memory::new());
    assert!(rec.counters().is_empty());
    assert!(rec.attached_here().is_empty());
}

// REQ-CORE-003; CR 400.7, CR 400.8, CR 406.7, CR 400.10: the capture matrix over every pair
// of zone kinds. Every exit is captured (public -> hidden included), and exile -> exile and
// command -> command; a same-zone reposition elsewhere is not.
#[test]
fn capture_matrix_over_every_zone_kind_pair() {
    let mut public_to_hidden = 0;
    for from in ZoneKind::ALL {
        for to in ZoneKind::ALL {
            let mut w = World::new();
            let id = w.create(1, from);
            let r = req(to);
            let leaves = from != to || matches!(from, ZoneKind::Exile | ZoneKind::Command);
            assert_eq!(w.capture(id, &r), Ok(leaves), "{from:?} -> {to:?}");
            assert_eq!(w.lki.len(), usize::from(leaves), "{from:?} -> {to:?}");
            let moved = w.zones.move_object(&mut w.ids, id, r).unwrap();
            // The capture decision agrees with the zone primitive's notion of a new object.
            assert_eq!(leaves, moved.old != moved.new, "{from:?} -> {to:?}");
            if leaves {
                assert_eq!(w.lki.get(id).unwrap().zone().kind(), from);
                if from.is_public() && !to.is_public() {
                    public_to_hidden += 1;
                }
            } else {
                assert!(w.lki.is_empty());
            }
        }
    }
    // Five public kinds times two hidden kinds.
    assert_eq!(public_to_hidden, 10);
}

// REQ-CORE-003: the error cases leave the store unchanged.
#[test]
fn capture_errors_leave_the_store_unchanged() {
    let mut w = World::new();
    let id = w.create(1, ZoneKind::Battlefield);
    let r = req(ZoneKind::Graveyard);
    assert_eq!(
        w.capture(ObjectId::new(999), &r),
        Err(MemoryError::UnknownObject(ObjectId::new(999)))
    );
    assert!(w.lki.is_empty());

    w.provider.chars.remove(&id);
    assert_eq!(w.capture(id, &r), Err(MemoryError::NoCharacteristics(id)));
    assert!(w.lki.is_empty());

    w.provider.chars.insert(id, chars("test.first"));
    assert_eq!(w.capture(id, &r), Ok(true));
    let before = w.lki.clone();
    w.provider.chars.insert(id, chars("test.second"));
    assert_eq!(w.capture(id, &r), Err(MemoryError::AlreadyCaptured(id)));
    assert_eq!(w.lki, before);
    assert_eq!(
        w.lki.get(id).unwrap().characteristics(),
        &chars("test.first")
    );
}

// REQ-CORE-003: the trait's default counters and attachments are empty.
#[test]
fn provider_defaults_give_no_counters_and_no_attachments() {
    let mut w = World::new();
    let id = w.create(1, ZoneKind::Battlefield);
    let provider = CharsOnly(chars("test.plain"));
    assert!(provider.counters(id).is_empty());
    assert!(provider.attached_here(id).is_empty());
    let mut lki = LkiStore::new();
    let got = capture_lki_on_move(
        &w.zones,
        &w.permanents,
        &w.memories,
        &provider,
        &mut lki,
        id,
        &req(ZoneKind::Exile),
    );
    assert_eq!(got, Ok(true));
    assert!(lki.get(id).unwrap().counters().is_empty());
    assert_eq!(lki.ids(), vec![id]);
}

// REQ-CORE-003; CR 704.8: for simultaneous departures every object is captured before any of
// them moves, so each record shows the state before all of the moves.
#[test]
fn simultaneous_departures_capture_pre_event_state() {
    let mut w = World::new();
    let a = w.create(1, ZoneKind::Battlefield);
    let b = w.create(2, ZoneKind::Battlefield);
    w.provider.attached.insert(a, vec![b]);
    w.permanents.attach(b, AttachTarget::Permanent(a)).unwrap();
    let r = req(ZoneKind::Graveyard);
    assert_eq!(w.capture(a, &r), Ok(true));
    assert_eq!(w.capture(b, &r), Ok(true));
    // Moving A first changes B's state (its attachment ends); B was captured before.
    w.zones.move_object(&mut w.ids, a, r).unwrap();
    w.permanents.leave(a).unwrap();
    w.provider.attached.remove(&a);
    assert_eq!(w.permanents.get(b).unwrap().attached_to(), None);
    w.zones.move_object(&mut w.ids, b, r).unwrap();
    w.permanents.leave(b).unwrap();
    assert_eq!(w.lki.get(a).unwrap().attached_here(), &[b]);
    assert_eq!(
        w.lki.get(b).unwrap().permanent().unwrap().attached_to(),
        Some(AttachTarget::Permanent(a))
    );
    assert_eq!(w.lki.ids(), vec![a, b]);
}

// ---------------------------------------------------------------------------------------------
// information_for (REQ-CORE-003)
// ---------------------------------------------------------------------------------------------

// REQ-CORE-003; CR 608.2h, CR 113.7a, CR 608.2b, CR 604.7.
#[test]
fn information_for_current_lki_static_and_none() {
    let mut w = World::new();
    let id = w.create(1, ZoneKind::Battlefield);
    let spell = InformationUser::SpellOrAbility;
    let stat = InformationUser::StaticAbility;
    // CR 608.2h: in the expected zone, current information, for both users.
    match information_for(&w.zones, &w.lki, id, ZoneKind::Battlefield, spell) {
        Some(Information::Current(r)) => assert_eq!(r.id, id),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        information_for(&w.zones, &w.lki, id, ZoneKind::Battlefield, stat),
        Some(Information::Current(_))
    ));
    // Not in the expected zone and nothing captured: none.
    assert_eq!(
        information_for(&w.zones, &w.lki, id, ZoneKind::Graveyard, spell),
        None
    );
    // After leaving: LKI for a spell or ability, none for a static ability (CR 604.7).
    let moved = w.capture_and_move(id, req(ZoneKind::Hand));
    match information_for(&w.zones, &w.lki, id, ZoneKind::Battlefield, spell) {
        Some(Information::Lki(r)) => assert_eq!(r.zone(), ZoneId::Battlefield),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        information_for(&w.zones, &w.lki, id, ZoneKind::Battlefield, stat),
        None
    );
    // The new object is not the old one (CR 400.7).
    assert!(matches!(
        information_for(&w.zones, &w.lki, moved.new, ZoneKind::Hand, stat),
        Some(Information::Current(_))
    ));
    // Unknown id, nothing captured.
    assert_eq!(
        information_for(&w.zones, &w.lki, ObjectId::new(999), ZoneKind::Exile, spell),
        None
    );
}

// ---------------------------------------------------------------------------------------------
// Memory and CR 400.7 (REQ-CORE-007)
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007; CR 607.2d, CR 607.2p, CR 607.5a: the Memory accessors.
#[test]
fn memory_choices_history_and_links() {
    let mut m = Memory::new();
    assert!(m.is_empty());
    assert_eq!(m.choice("type"), None);
    m.set_choice("type", ChoiceValue::CreatureType("test_type".to_owned()));
    assert_eq!(
        m.choice("type"),
        Some(&ChoiceValue::CreatureType("test_type".to_owned()))
    );
    // A later choice under the same key replaces the earlier one, in both directions.
    m.set_persistent_choice("type", ChoiceValue::Anchor("left".to_owned()));
    assert_eq!(
        m.choice("type"),
        Some(&ChoiceValue::Anchor("left".to_owned()))
    );
    m.set_choice("type", ChoiceValue::Number(4));
    assert_eq!(m.choice("type"), Some(&ChoiceValue::Number(4)));
    assert_eq!(m.spell_history(), None);
    m.set_spell_history(kicked_history());
    assert_eq!(m.spell_history(), Some(&kicked_history()));
    assert!(m.linked().is_empty());
    m.linked_mut().push(l(1), LinkEntry::Int(5));
    assert_eq!(m.linked().entries(l(1)), Some(&[LinkEntry::Int(5)][..]));
    assert_eq!(m.link_source(l(1)), None);
    let r = DurableRef::from_parts(c(1), ObjectId::new(1));
    m.set_link_source(l(1), r);
    assert_eq!(m.link_source(l(1)), Some(r));
    assert!(!m.is_empty());
}

// REQ-CORE-007: the table accessors.
#[test]
fn memory_table_accessors() {
    let mut t = MemoryTable::new();
    assert!(t.is_empty());
    assert_eq!(t.len(), 0);
    assert_eq!(t.get(ObjectId::new(2)), None);
    t.get_mut_or_default(ObjectId::new(2));
    t.get_mut_or_default(ObjectId::new(1))
        .set_choice("k", ChoiceValue::Mode(1));
    assert_eq!(t.len(), 2);
    assert!(!t.is_empty());
    assert_eq!(t.ids(), vec![ObjectId::new(1), ObjectId::new(2)]);
    assert_eq!(t.get(ObjectId::new(2)), Some(&Memory::new()));
    // Clone independence (Arc + make_mut).
    let snapshot = t.clone();
    t.get_mut_or_default(ObjectId::new(1))
        .set_choice("k", ChoiceValue::Mode(2));
    assert_eq!(
        snapshot.get(ObjectId::new(1)).unwrap().choice("k"),
        Some(&ChoiceValue::Mode(1))
    );
}

// REQ-CORE-007, REQ-CORE-002; CR 400.7d, CR 607.2i, CR 607.2j: a permanent reads the kicker
// flag, X and the mana spent of the spell it was after stack -> battlefield.
#[test]
fn spell_history_carries_from_stack_to_battlefield() {
    let mut w = World::new();
    let spell = w.create(1, ZoneKind::Stack);
    w.memories
        .get_mut_or_default(spell)
        .set_spell_history(kicked_history());
    let moved = w.mv(spell, ZoneKind::Battlefield);
    assert_ne!(moved.old, moved.new);
    assert_eq!(w.memories.get(spell), None);
    let h = w.memories.get(moved.new).unwrap().spell_history().unwrap();
    assert_eq!(h.additional_costs.get("kicker"), Some(&true));
    assert_eq!(h.x_value, Some(3));
    assert_eq!(
        h.mana_spent.get(&("green".to_owned(), "land".to_owned())),
        Some(&2)
    );
    // CR 607.2q: the cards exiled to pay the cost stay with the spell history.
    assert_eq!(
        h.exiled_to_pay,
        vec![DurableRef::from_parts(c(90), ObjectId::new(900))]
    );
}

// REQ-CORE-007; CR 400.7, CR 400.7d: the spell history does not carry to a non-permanent
// destination, from a non-stack origin, or onward from the battlefield.
#[test]
fn spell_history_not_carried_elsewhere() {
    for (from, to) in [
        (ZoneKind::Stack, ZoneKind::Graveyard),
        (ZoneKind::Stack, ZoneKind::Exile),
        (ZoneKind::Stack, ZoneKind::Hand),
        (ZoneKind::Hand, ZoneKind::Battlefield),
        (ZoneKind::Graveyard, ZoneKind::Battlefield),
        (ZoneKind::Battlefield, ZoneKind::Exile),
        (ZoneKind::Exile, ZoneKind::Exile),
    ] {
        let mut w = World::new();
        let id = w.create(1, from);
        w.memories
            .get_mut_or_default(id)
            .set_spell_history(kicked_history());
        let moved = w.mv(id, to);
        assert_ne!(moved.old, moved.new);
        assert_eq!(w.memories.get(id), None, "{from:?} -> {to:?}");
        assert_eq!(
            w.memories.get(moved.new).unwrap().spell_history(),
            None,
            "{from:?} -> {to:?}"
        );
    }
}

// REQ-CORE-007; CR 400.7: a same-zone reposition on the battlefield is not a zone change:
// nothing changes.
#[test]
fn reposition_keeps_memory() {
    let mut w = World::new();
    let id = w.create(1, ZoneKind::Battlefield);
    {
        let m = w.memories.get_mut_or_default(id);
        m.set_spell_history(kicked_history());
        m.set_choice("k", ChoiceValue::Mode(0));
    }
    let before = w.memories.clone();
    let moved = w
        .zones
        .move_object(&mut w.ids, id, req(ZoneKind::Battlefield))
        .unwrap();
    assert_eq!(moved.old, moved.new);
    // A reposition changes nothing and reports false even though the object has a Memory.
    assert!(!w.memories.apply_zone_change(&moved));
    assert_eq!(w.memories, before);
    // A reposition of an object without a Memory reports false and changes nothing.
    let bare = w.create(2, ZoneKind::Graveyard);
    let bottom = MoveRequest {
        position: Position::Bottom,
        ..req(ZoneKind::Graveyard)
    };
    let moved = w.zones.move_object(&mut w.ids, bare, bottom).unwrap();
    assert!(!w.memories.apply_zone_change(&moved));
    assert_eq!(w.memories, before);
}

// REQ-CORE-007; CR 400.7, CR 607.2p, CR 607.2q: by default nothing carries over: ordinary
// choices, the Linked Store and link sources are dropped; persistent choices are kept on every
// move; the cards exiled to pay the cost carry only stack -> battlefield.
#[test]
fn default_zone_change_drops_all_but_persistent_choices() {
    for to in [
        ZoneKind::Battlefield,
        ZoneKind::Graveyard,
        ZoneKind::Exile,
        ZoneKind::Hand,
        ZoneKind::Library,
        ZoneKind::Command,
    ] {
        let mut w = World::new();
        let id = w.create(1, ZoneKind::Stack);
        {
            let m = w.memories.get_mut_or_default(id);
            m.set_choice("color", ChoiceValue::Color("red".to_owned()));
            m.set_persistent_choice("cda", ChoiceValue::Color("white".to_owned()));
            m.linked_mut().push(l(1), LinkEntry::Player(p(2)));
            m.set_link_source(l(2), DurableRef::from_parts(c(5), ObjectId::new(50)));
            m.set_spell_history(kicked_history());
        }
        let moved = w.mv(id, to);
        let m = w.memories.get(moved.new).unwrap();
        assert_eq!(m.choice("color"), None, "{to:?}");
        assert_eq!(
            m.choice("cda"),
            Some(&ChoiceValue::Color("white".to_owned())),
            "{to:?}"
        );
        assert!(m.linked().is_empty(), "{to:?}");
        assert_eq!(m.link_source(l(2)), None, "{to:?}");
        assert_eq!(
            m.spell_history().map(|h| h.exiled_to_pay.len()),
            (to == ZoneKind::Battlefield).then_some(1),
            "{to:?}"
        );
        // A second move drops the spell history too (it came from the stack only once).
        let again = w.mv(moved.new, ZoneKind::Exile);
        let m = w.memories.get(again.new).unwrap();
        assert_eq!(m.spell_history(), None);
        assert!(m.choice("cda").is_some());
    }
}

// REQ-CORE-007; CR 400.7: an object without a Memory gets none.
#[test]
fn zone_change_without_memory_creates_none() {
    let mut w = World::new();
    let id = w.create(1, ZoneKind::Hand);
    let moved = w
        .zones
        .move_object(&mut w.ids, id, req(ZoneKind::Graveyard))
        .unwrap();
    assert!(!w.memories.apply_zone_change(&moved));
    assert!(w.memories.is_empty());
}

// ---------------------------------------------------------------------------------------------
// Durable references and the Linked Store (REQ-CORE-007)
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007; CR 400.7: a durable reference resolves only to its own generation.
#[test]
fn durable_ref_resolution() {
    let mut w = World::new();
    let id = w.create(7, ZoneKind::Exile);
    let r = DurableRef::new(&w.zones, id).unwrap();
    assert_eq!(r.card(), c(7));
    assert_eq!(r.generation(), id);
    assert_eq!(r, DurableRef::from_parts(c(7), id));
    assert_eq!(r.resolve(&w.zones, None), Some(id));
    assert_eq!(r.resolve(&w.zones, Some(ZoneKind::Exile)), Some(id));
    // Wrong expected zone.
    assert_eq!(r.resolve(&w.zones, Some(ZoneKind::Graveyard)), None);
    // Generation mismatch.
    let stale = DurableRef::from_parts(c(7), ObjectId::new(id.get() + 100));
    assert_eq!(stale.resolve(&w.zones, None), None);
    // Card mismatch.
    assert_eq!(
        DurableRef::from_parts(c(8), id).resolve(&w.zones, None),
        None
    );
    // Unknown object.
    assert_eq!(
        DurableRef::new(&w.zones, ObjectId::new(999)),
        Err(MemoryError::UnknownObject(ObjectId::new(999)))
    );
    // After a zone change the old generation no longer resolves.
    w.mv(id, ZoneKind::Graveyard);
    assert_eq!(r.resolve(&w.zones, None), None);
    // Ordering: by card, then generation.
    assert!(
        DurableRef::from_parts(c(1), ObjectId::new(9))
            < DurableRef::from_parts(c(2), ObjectId::new(1))
    );
}

// REQ-CORE-007; CR 607.3, CR 607.5a: entries in insertion order; undefined links.
#[test]
fn linked_store_push_entries_define_clear() {
    let mut s = LinkedStore::new();
    assert!(s.is_empty());
    assert_eq!(s.entries(l(3)), None);
    s.push(l(3), LinkEntry::Text("b".to_owned()));
    s.push(l(3), LinkEntry::Int(-1));
    s.push(l(1), LinkEntry::Player(p(1)));
    assert_eq!(
        s.entries(l(3)),
        Some(&[LinkEntry::Text("b".to_owned()), LinkEntry::Int(-1)][..])
    );
    assert_eq!(s.links(), vec![l(1), l(3)]);
    s.define(l(2));
    assert_eq!(s.entries(l(2)), Some(&[][..]));
    s.define(l(3));
    assert_eq!(s.entries(l(3)).unwrap().len(), 2);
    let snapshot = s.clone();
    assert!(s.clear_link(l(3)));
    assert!(!s.clear_link(l(3)));
    assert_eq!(s.entries(l(3)), None);
    assert_eq!(snapshot.entries(l(3)).unwrap().len(), 2);
    assert!(!s.is_empty());
}

// REQ-CORE-007; CR 607.2a, CR 400.7: an "exiled with" link resolves while the card is still in
// exile and breaks after exile -> graveyard; non-object entries are skipped; an undefined
// link gives nothing.
#[test]
fn linked_store_resolution_follows_zone_change_rules() {
    let mut w = World::new();
    let a = w.create(1, ZoneKind::Exile);
    let b = w.create(2, ZoneKind::Exile);
    let mut s = LinkedStore::new();
    s.push(
        l(1),
        LinkEntry::Object(DurableRef::new(&w.zones, b).unwrap()),
    );
    s.push(l(1), LinkEntry::Int(9));
    s.push(
        l(1),
        LinkEntry::Object(DurableRef::new(&w.zones, a).unwrap()),
    );
    assert_eq!(
        s.resolve_objects(l(1), &w.zones, Some(ZoneKind::Exile)),
        vec![b, a]
    );
    assert!(
        s.resolve_objects(l(1), &w.zones, Some(ZoneKind::Battlefield))
            .is_empty()
    );
    assert!(s.resolve_objects(l(2), &w.zones, None).is_empty());
    w.mv(b, ZoneKind::Graveyard);
    assert_eq!(
        s.resolve_objects(l(1), &w.zones, Some(ZoneKind::Exile)),
        vec![a]
    );
    assert_eq!(s.resolve_objects(l(1), &w.zones, None), vec![a]);
    // The finder of another effect does not bring b back.
    assert_eq!(
        s.resolve_objects_with(l(1), &w.zones, None, &MoveFinder::new()),
        vec![a]
    );
}

// REQ-CORE-007; CR 607.1d: cross-object link in both directions; errors leave the table
// unchanged.
#[test]
fn link_created_both_directions_and_errors() {
    let mut w = World::new();
    let source = w.create(1, ZoneKind::Battlefield);
    let token = w.create(2, ZoneKind::Battlefield);
    let before = w.memories.clone();
    assert_eq!(
        w.memories
            .link_created(&w.zones, ObjectId::new(999), token, l(4)),
        Err(MemoryError::UnknownObject(ObjectId::new(999)))
    );
    assert_eq!(
        w.memories
            .link_created(&w.zones, source, ObjectId::new(998), l(4)),
        Err(MemoryError::UnknownObject(ObjectId::new(998)))
    );
    assert_eq!(w.memories, before);
    assert_eq!(
        w.memories.link_created(&w.zones, source, token, l(4)),
        Ok(())
    );
    assert_eq!(
        w.memories.get(source).unwrap().linked().entries(l(4)),
        Some(&[LinkEntry::Object(DurableRef::from_parts(c(2), token))][..])
    );
    assert_eq!(
        w.memories.get(token).unwrap().link_source(l(4)),
        Some(DurableRef::from_parts(c(1), source))
    );
    assert_eq!(
        w.memories.get(source).unwrap().linked().resolve_objects(
            l(4),
            &w.zones,
            Some(ZoneKind::Battlefield)
        ),
        vec![token]
    );
}

// ---------------------------------------------------------------------------------------------
// MoveFinder (REQ-CORE-007, CR 400.7j)
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007, REQ-CORE-002; CR 400.7j: the effect that exiled an object and returned it to a
// public zone finds it; another effect does not.
#[test]
fn move_finder_same_effect_finds_moved_object() {
    let mut w = World::new();
    let id = w.create(1, ZoneKind::Battlefield);
    let r = DurableRef::new(&w.zones, id).unwrap();
    let mut finder = MoveFinder::new();
    let first = w.mv(id, ZoneKind::Exile);
    assert!(finder.record(&first));
    assert_eq!(finder.find(id), Some(first.new));
    let second = w.mv(first.new, ZoneKind::Battlefield);
    assert!(finder.record(&second));
    // Chain across two public moves.
    assert_eq!(finder.find(id), Some(second.new));
    assert_eq!(finder.find(first.new), Some(second.new));
    assert_eq!(
        finder.resolve(&r, &w.zones, Some(ZoneKind::Battlefield)),
        Some(second.new)
    );
    assert_eq!(finder.resolve(&r, &w.zones, Some(ZoneKind::Exile)), None);
    // Another effect's finder finds nothing.
    let other = MoveFinder::new();
    assert_eq!(other.find(id), None);
    assert_eq!(other.resolve(&r, &w.zones, None), None);
    // Through a Linked Store.
    let mut s = LinkedStore::new();
    s.push(l(1), LinkEntry::Object(r));
    assert!(s.resolve_objects(l(1), &w.zones, None).is_empty());
    assert_eq!(
        s.resolve_objects_with(l(1), &w.zones, None, &finder),
        vec![second.new]
    );
    // A different card is not found; nor is a recorded object that is no longer in a zone.
    assert_eq!(
        finder.resolve(&DurableRef::from_parts(c(5), id), &w.zones, None),
        None
    );
    w.zones
        .send_outside(second.new, OutsidePool::Sideboard)
        .unwrap();
    assert_eq!(finder.resolve(&r, &w.zones, None), None);
}

// REQ-CORE-007; CR 400.7j: hidden destinations are not found; a reposition is not recorded.
#[test]
fn move_finder_hidden_and_reposition() {
    let mut w = World::new();
    let id = w.create(1, ZoneKind::Battlefield);
    let mut finder = MoveFinder::new();
    let to_hand = w.mv(id, ZoneKind::Hand);
    assert!(!finder.record(&to_hand));
    assert_eq!(finder.find(id), None);

    // Public then hidden: the chain is dropped.
    let a = w.create(2, ZoneKind::Battlefield);
    let to_exile = w.mv(a, ZoneKind::Exile);
    assert!(finder.record(&to_exile));
    let to_library = w.mv(to_exile.new, ZoneKind::Library);
    assert!(!finder.record(&to_library));
    assert_eq!(finder.find(a), None);
    assert_eq!(finder.find(to_exile.new), None);

    // Reposition (old == new).
    let g = w.create(3, ZoneKind::Graveyard);
    let repo = w.mv(g, ZoneKind::Graveyard);
    assert_eq!(repo.old, repo.new);
    assert!(!finder.record(&repo));
    assert_eq!(finder, MoveFinder::new());
}

// ---------------------------------------------------------------------------------------------
// Registry (REQ-CORE-002)
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002, REQ-CORE-007; CR 400.7d, CR 400.7j: D and J apply; the other ten are not
// implemented; the function names are unchanged.
#[test]
fn registry_d_and_j_apply() {
    assert_eq!(
        exception_400_7d_spell_information(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::Applies(Exception400_7::D)
    );
    assert_eq!(
        exception_400_7j_effect_finds_moved_object(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::Applies(Exception400_7::J)
    );
    for entry in EXCEPTIONS {
        let out = (entry.apply)(ObjectId::new(3), ObjectId::new(4));
        if matches!(entry.exception, Exception400_7::D | Exception400_7::J) {
            assert_eq!(out, ExceptionOutcome::Applies(entry.exception));
        } else {
            assert_eq!(out, ExceptionOutcome::NotImplemented(entry.exception));
        }
    }
    assert_eq!(Exception400_7::D.name(), "spell_information");
    assert_eq!(Exception400_7::J.name(), "effect_finds_moved_object");
}

// ---------------------------------------------------------------------------------------------
// Canonical encoding and errors (REQ-CORE-001, REQ-CORE-006, REQ-NFR-006)
// ---------------------------------------------------------------------------------------------

/// Captures a battlefield object of card 1 with characteristics `ch` and returns its record.
fn record_with(ch: Characteristics, tweak: impl FnOnce(&mut World, ObjectId)) -> LkiRecord {
    let mut w = World::new();
    let id = w.create(1, ZoneKind::Battlefield);
    w.provider.chars.insert(id, ch);
    tweak(&mut w, id);
    w.capture(id, &req(ZoneKind::Graveyard)).unwrap();
    w.lki.get(id).unwrap().clone()
}

/// A change to one field of the synthetic characteristics.
type CharChange = fn(&mut Characteristics);

fn record_with_chars(f: CharChange) -> LkiRecord {
    let mut ch = chars("test.flier_2_2");
    f(&mut ch);
    record_with(ch, |_, _| {})
}

// REQ-CORE-001, REQ-NFR-006, REQ-CORE-003: equal records encode equally; a change in any
// captured field changes the bytes.
#[test]
fn lki_encoding_is_deterministic_and_sensitive() {
    let base = record_with(chars("test.flier_2_2"), |_, _| {});
    let again = record_with(chars("test.flier_2_2"), |_, _| {});
    assert_eq!(base, again);
    assert_eq!(encode_to_vec(&base), encode_to_vec(&again));
    assert_eq!(encode_to_vec(&base), encode_to_vec(&base.clone()));

    let char_changes: [(&str, CharChange); 16] = [
        ("names", |ch| {
            ch.names = BTreeSet::from(["test.other".to_owned()]);
        }),
        ("mana_cost", |ch| {
            ch.mana_cost = Some("{1}".parse().unwrap())
        }),
        ("indicator", |ch| {
            ch.color_indicator = Some(ColorBitmask::RED)
        }),
        ("colors", |ch| ch.colors = ColorBitmask::BLUE),
        ("supertypes", |ch| {
            ch.supertypes = BTreeSet::from(["Legendary".to_owned()]);
        }),
        ("card_types", |ch| {
            ch.card_types = BTreeSet::from(["Artifact".to_owned()]);
        }),
        ("subtypes", |ch| {
            ch.subtypes = BTreeSet::from(["Test".to_owned()]);
        }),
        ("abilities", |ch| ch.abilities = vec![tree("k", 1)]),
        // Beyond the canonical JSON depth budget: the MessagePack fallback (#232: a typed
        // ability can no longer hold an integer outside the JSON safe range).
        ("power_too_deep", |ch| ch.power = Some(deep_power())),
        ("power", |ch| {
            ch.power = Some(ValueExpressionAST::Memory("test.p".to_owned()));
        }),
        ("toughness", |ch| {
            ch.toughness = Some(ValueExpressionAST::Memory("test.t".to_owned()));
        }),
        ("rules_text", |ch| {
            ch.rules_text = serde_json::from_value(serde_json::json!([
                {"id": "x", "kind": "LITERAL", "text": "x"}
            ]))
            .unwrap();
        }),
        ("loyalty", |ch| ch.loyalty = Some(3)),
        ("defense", |ch| ch.defense = Some(3)),
        ("hand", |ch| ch.hand_modifier = Some(1)),
        ("life", |ch| ch.life_modifier = Some(1)),
    ];
    let mut variants: Vec<(&str, LkiRecord)> = char_changes
        .iter()
        .map(|&(what, f)| (what, record_with_chars(f)))
        .collect();
    // Equal fallback-encoded values still encode equally.
    assert_eq!(
        encode_to_vec(&record_with_chars(char_changes[8].1)),
        encode_to_vec(&variants[8].1)
    );
    let fresh = || chars("test.flier_2_2");
    variants.push((
        "counters",
        record_with(fresh(), |w, id| {
            w.provider
                .counters
                .insert(id, BTreeMap::from([("charge".to_owned(), 1)]));
        }),
    ));
    variants.push((
        "attached",
        record_with(fresh(), |w, id| {
            w.provider.attached.insert(id, vec![ObjectId::new(77)]);
        }),
    ));
    variants.push((
        "status",
        record_with(fresh(), |w, id| {
            w.permanents.set_tapped(id, true).unwrap();
        }),
    ));
    variants.push((
        "permanent_absent",
        record_with(fresh(), |w, id| {
            w.permanents.leave(id).unwrap();
        }),
    ));
    variants.push((
        "memory",
        record_with(fresh(), |w, id| {
            w.memories
                .get_mut_or_default(id)
                .set_choice("n", ChoiceValue::Number(1));
        }),
    ));
    variants.push((
        "controller",
        record_with(fresh(), |w, id| {
            let r = MoveRequest {
                controller: Some(p(2)),
                ..req(ZoneKind::Battlefield)
            };
            w.zones.move_object(&mut w.ids, id, r).unwrap();
        }),
    ));
    variants.push((
        "face_down",
        record_with(fresh(), |w, id| {
            let r = MoveRequest {
                face_down: true,
                ..req(ZoneKind::Battlefield)
            };
            w.zones.move_object(&mut w.ids, id, r).unwrap();
        }),
    ));
    // Object id and card: an object created after another, of another card.
    let mut w = World::new();
    w.create(5, ZoneKind::Library);
    let id = w.create(6, ZoneKind::Battlefield);
    w.capture(id, &req(ZoneKind::Graveyard)).unwrap();
    variants.push(("id_and_card", w.lki.get(id).unwrap().clone()));
    // Zone only (no permanent state either way).
    let mut w = World::new();
    let id = w.create(1, ZoneKind::Graveyard);
    w.capture(id, &req(ZoneKind::Exile)).unwrap();
    let in_graveyard = w.lki.get(id).unwrap().clone();
    let mut w = World::new();
    let id = w.create(1, ZoneKind::Exile);
    w.capture(id, &req(ZoneKind::Graveyard)).unwrap();
    let in_exile = w.lki.get(id).unwrap().clone();
    assert_ne!(encode_to_vec(&in_graveyard), encode_to_vec(&in_exile));
    // Owner only: zone ids of different owners differ.
    let mut w = World::new();
    let id = w
        .zones
        .create_object(&mut w.ids, c(1), p(2), req(ZoneKind::Graveyard))
        .unwrap();
    w.provider.chars.insert(id, fresh());
    w.capture(id, &req(ZoneKind::Exile)).unwrap();
    assert_ne!(
        encode_to_vec(w.lki.get(id).unwrap()),
        encode_to_vec(&in_graveyard)
    );

    let mut seen = BTreeSet::from([encode_to_vec(&base)]);
    for (what, rec) in &variants {
        assert_ne!(rec, &base, "{what}");
        assert!(
            seen.insert(encode_to_vec(rec)),
            "{what} did not change the bytes"
        );
    }
}

// REQ-CORE-001, REQ-NFR-006: the LKI store encodes its records in ascending id order and is
// an independent snapshot when cloned.
#[test]
fn lki_store_encoding_and_clone_independence() {
    let mut w = World::new();
    let a = w.create(1, ZoneKind::Battlefield);
    let b = w.create(2, ZoneKind::Battlefield);
    let empty = encode_to_vec(&w.lki);
    w.capture(b, &req(ZoneKind::Exile)).unwrap();
    let snapshot = w.lki.clone();
    w.capture(a, &req(ZoneKind::Exile)).unwrap();
    assert_eq!(snapshot.len(), 1);
    assert_eq!(w.lki.len(), 2);
    assert!(!w.lki.is_empty());
    assert_eq!(w.lki.ids(), vec![a, b]);
    let mut expected = Vec::new();
    fineprint_base::encode::put_len(&mut expected, 2);
    for id in [a, b] {
        expected.extend(encode_to_vec(&id));
        expected.extend(encode_to_vec(w.lki.get(id).unwrap()));
    }
    assert_eq!(encode_to_vec(&w.lki), expected);
    assert_ne!(encode_to_vec(&snapshot), empty);
}

// REQ-CORE-001, REQ-NFR-006, REQ-CORE-007: Memory, MemoryTable, LinkedStore, DurableRef,
// LinkEntry and ChoiceValue encodings: equal values give equal bytes, every field matters and
// variant tags are distinct.
#[test]
fn memory_encodings_deterministic_sensitive_and_tagged() {
    let r = DurableRef::from_parts(c(1), ObjectId::new(2));
    let mut expected = encode_to_vec(&c(1));
    expected.extend(encode_to_vec(&ObjectId::new(2)));
    assert_eq!(encode_to_vec(&r), expected);

    let entries = [
        LinkEntry::Object(r),
        LinkEntry::Player(p(1)),
        LinkEntry::Int(1),
        LinkEntry::Text("1".to_owned()),
    ];
    let tags: BTreeSet<u8> = entries.iter().map(|e| encode_to_vec(e)[0]).collect();
    assert_eq!(tags.len(), 4);

    let choices = [
        ChoiceValue::Color("x".to_owned()),
        ChoiceValue::CreatureType("x".to_owned()),
        ChoiceValue::CardName("x".to_owned()),
        ChoiceValue::Player(p(1)),
        ChoiceValue::Number(1),
        ChoiceValue::Mode(1),
        ChoiceValue::Anchor("x".to_owned()),
    ];
    let tags: BTreeSet<u8> = choices.iter().map(|v| encode_to_vec(v)[0]).collect();
    assert_eq!(tags.len(), 7);

    // Memory: each field changes the bytes.
    let base = Memory::new();
    let mut variants = Vec::new();
    let mut m = Memory::new();
    m.set_choice("k", ChoiceValue::Number(1));
    variants.push(m);
    let mut m = Memory::new();
    m.set_persistent_choice("k", ChoiceValue::Number(1));
    variants.push(m);
    let mut m = Memory::new();
    m.set_spell_history(SpellHistory::default());
    variants.push(m);
    let mut m = Memory::new();
    m.linked_mut().define(l(1));
    variants.push(m);
    let mut m = Memory::new();
    m.set_link_source(l(1), r);
    variants.push(m);
    // SpellHistory: each field changes the bytes.
    let hist: [fn(&mut SpellHistory); 6] = [
        |h| h.costs_paid.push("mana".to_owned()),
        |h| {
            h.mana_spent
                .insert(("red".to_owned(), "land".to_owned()), 1);
        },
        |h| {
            h.additional_costs.insert("kicker".to_owned(), false);
        },
        |h| h.x_value = Some(0),
        |h| h.cast_face = Some("back".to_owned()),
        |h| {
            h.exiled_to_pay
                .push(DurableRef::from_parts(c(1), ObjectId::new(1)));
        },
    ];
    for f in hist {
        let mut h = SpellHistory::default();
        f(&mut h);
        let mut m = Memory::new();
        m.set_spell_history(h);
        variants.push(m);
    }
    let mut seen = BTreeSet::from([encode_to_vec(&base)]);
    for m in &variants {
        assert!(seen.insert(encode_to_vec(m)), "{m:?}");
        assert_eq!(encode_to_vec(m), encode_to_vec(&m.clone()));
    }

    // MemoryTable: ascending ids; an empty Memory is still an entry.
    let mut t = MemoryTable::new();
    let empty_table = encode_to_vec(&t);
    t.get_mut_or_default(ObjectId::new(2));
    let one = encode_to_vec(&t);
    assert_ne!(one, empty_table);
    let mut u = MemoryTable::new();
    u.get_mut_or_default(ObjectId::new(3));
    assert_ne!(encode_to_vec(&u), one);

    // LinkedStore: order of entries matters (CR 607.3).
    let mut s1 = LinkedStore::new();
    s1.push(l(1), LinkEntry::Int(1));
    s1.push(l(1), LinkEntry::Int(2));
    let mut s2 = LinkedStore::new();
    s2.push(l(1), LinkEntry::Int(2));
    s2.push(l(1), LinkEntry::Int(1));
    assert_ne!(encode_to_vec(&s1), encode_to_vec(&s2));
    assert_eq!(encode_to_vec(&s1), encode_to_vec(&s1.clone()));
}

// REQ-CORE-003, REQ-CORE-007: errors display their object id.
#[test]
fn memory_error_display() {
    let e: &dyn std::error::Error = &MemoryError::UnknownObject(ObjectId::new(4));
    assert_eq!(e.to_string(), "unknown object 4");
    assert!(
        MemoryError::AlreadyCaptured(ObjectId::new(5))
            .to_string()
            .contains('5')
    );
    assert!(
        MemoryError::NoCharacteristics(ObjectId::new(6))
            .to_string()
            .contains('6')
    );
}
