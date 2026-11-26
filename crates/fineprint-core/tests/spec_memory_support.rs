// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `fineprint_core::memory` spec tests (REQ-CORE-003, REQ-CORE-007).
//! Black-box: written from the work-item brief only.
//!
//! Each `spec_memory_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. Every card here is synthetic and neutrally named (R5).

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::ids::{CardId, IdGenerator, LinkId, ObjectId, PlayerId};
use fineprint_core::memory::{
    CharacteristicsProvider, LkiStore, Memory, MemoryError, MemoryTable, capture_lki_on_move,
};
use fineprint_core::objects::Characteristics;
use fineprint_core::permanent::Permanents;
use fineprint_core::zones::{MoveRequest, Moved, ZoneKind, Zones, ZonesConfig};
use fineprint_dsl::card::{ColorBitmask, TextTokenStream, ValueExpressionAST};

// ---------------------------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------------------------

pub const P1: PlayerId = PlayerId::new(1);
pub const P2: PlayerId = PlayerId::new(2);

pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn cid(n: u64) -> CardId {
    CardId::new(n)
}

pub fn lid(n: u64) -> LinkId {
    LinkId::new(n)
}

pub fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

// ---------------------------------------------------------------------------------------------
// zones fixtures
// ---------------------------------------------------------------------------------------------

/// A test world: the zones of two players, an id generator, permanents, memories and LKI.
#[derive(Clone, Debug)]
pub struct World {
    pub zones: Zones,
    pub ids: IdGenerator<ObjectId>,
    pub permanents: Permanents,
    pub memories: MemoryTable,
    pub lki: LkiStore,
    pub provider: TestProvider,
    next_card: u64,
}

impl Default for World {
    fn default() -> World {
        World::new()
    }
}

impl World {
    pub fn new() -> World {
        World::with_generator(IdGenerator::new())
    }

    pub fn with_generator(ids: IdGenerator<ObjectId>) -> World {
        World {
            zones: two_player_zones(),
            ids,
            permanents: Permanents::new(),
            memories: MemoryTable::new(),
            lki: LkiStore::new(),
            provider: TestProvider::with_fallback(chars("test.vanilla_1_1", 1, 1)),
            next_card: 1,
        }
    }

    /// Creates a new card-backed object owned by `owner` in a zone of kind `kind`. A
    /// battlefield object is also entered into the permanents (controller `owner`, turn 1).
    pub fn create(&mut self, kind: ZoneKind, owner: PlayerId) -> ObjectId {
        let card = cid(self.next_card);
        self.next_card += 1;
        self.create_card(card, kind, owner)
    }

    /// As [`World::create`] with a chosen card id.
    pub fn create_card(&mut self, card: CardId, kind: ZoneKind, owner: PlayerId) -> ObjectId {
        let id = self
            .zones
            .create_object(&mut self.ids, card, owner, MoveRequest::new(kind))
            .expect("create succeeds");
        if kind == ZoneKind::Battlefield {
            self.permanents.enter(id, owner, 1).expect("enter succeeds");
        }
        id
    }

    /// Captures LKI with the world's provider.
    pub fn capture(&mut self, id: ObjectId, req: &MoveRequest) -> Result<bool, MemoryError> {
        capture_lki_on_move(
            &self.zones,
            &self.permanents,
            &self.memories,
            &self.provider,
            &mut self.lki,
            id,
            req,
        )
    }

    /// The capture-then-move protocol: capture LKI, move, update permanents (leave the
    /// battlefield / enter it) and apply the memory zone change. Returns the `Moved`.
    pub fn move_to(&mut self, id: ObjectId, kind: ZoneKind) -> Moved {
        self.move_req(id, MoveRequest::new(kind))
    }

    pub fn move_req(&mut self, id: ObjectId, req: MoveRequest) -> Moved {
        self.capture(id, &req).expect("capture succeeds");
        let moved = self
            .zones
            .move_object(&mut self.ids, id, req)
            .expect("move succeeds");
        if moved.old != moved.new {
            if moved.from.kind() == ZoneKind::Battlefield {
                self.permanents.leave(moved.old).expect("leave succeeds");
            }
            if moved.to.kind() == ZoneKind::Battlefield {
                let controller = self.zones.object(moved.new).expect("present").controller;
                self.permanents
                    .enter(moved.new, controller, 1)
                    .expect("enter succeeds");
            }
        }
        self.memories.apply_zone_change(&moved);
        moved
    }

    /// A raw move without LKI capture or memory upkeep (an "outside" event in a test).
    pub fn raw_move(&mut self, id: ObjectId, kind: ZoneKind) -> Moved {
        let moved = self
            .zones
            .move_object(&mut self.ids, id, MoveRequest::new(kind))
            .expect("move succeeds");
        if moved.old != moved.new {
            if moved.from.kind() == ZoneKind::Battlefield {
                let _ = self.permanents.leave(moved.old);
            }
            if moved.to.kind() == ZoneKind::Battlefield {
                let controller = self.zones.object(moved.new).expect("present").controller;
                let _ = self.permanents.enter(moved.new, controller, 1);
            }
        }
        moved
    }

    pub fn memory_mut(&mut self, id: ObjectId) -> &mut Memory {
        self.memories.get_mut_or_default(id)
    }
}

pub fn two_player_zones() -> Zones {
    Zones::new(&[P1, P2], ZonesConfig::default())
}

/// The Memory of `id`, or an empty Memory when the table holds none. Used where the brief
/// leaves open whether a fresh, empty Memory is stored for a new object.
pub fn memory_or_empty(table: &MemoryTable, id: ObjectId) -> Memory {
    table.get(id).cloned().unwrap_or_default()
}

/// True if the move request makes the object leave its zone per the brief: a different zone,
/// or exile -> exile / command -> command (CR 400.8, 406.7, 400.10).
pub fn expect_capture(from: ZoneKind, to: ZoneKind) -> bool {
    from != to || matches!(from, ZoneKind::Exile | ZoneKind::Command)
}

// ---------------------------------------------------------------------------------------------
// characteristics
// ---------------------------------------------------------------------------------------------

/// Round-trips a serialisable value through MessagePack into one of the DSL placeholder types
/// (which wrap a JSON value), so no direct `serde_json` dependency is needed.
fn via_msgpack<In: serde::Serialize, Out: serde::de::DeserializeOwned>(value: &In) -> Out {
    let bytes = rmp_serde::to_vec(value).expect("encode");
    rmp_serde::from_slice(&bytes).expect("decode")
}

/// A printed value `Literal(n)` (#232 FP-164: formerly the bare JSON integer placeholder).
fn printed(n: i64) -> ValueExpressionAST {
    ValueExpressionAST::Literal(n)
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// A synthetic creature `Characteristics` with the given name, power and toughness.
pub fn chars(name: &str, power: i64, toughness: i64) -> Characteristics {
    Characteristics {
        names: set(&[name]),
        mana_cost: None,
        color_indicator: None,
        colors: ColorBitmask::new(0).expect("valid mask"),
        supertypes: BTreeSet::new(),
        card_types: set(&["Creature"]),
        subtypes: BTreeSet::new(),
        rules_text: TextTokenStream::default(),
        abilities: Vec::new(),
        power: Some(printed(power)),
        toughness: Some(printed(toughness)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

// ---------------------------------------------------------------------------------------------
// provider
// ---------------------------------------------------------------------------------------------

/// A characteristics provider whose data can be changed between calls (interior mutability),
/// to prove the captured LKI does not follow later changes.
#[derive(Clone, Debug, Default)]
pub struct TestProvider {
    fallback: RefCell<Option<Characteristics>>,
    chars: RefCell<BTreeMap<ObjectId, Characteristics>>,
    counters: RefCell<BTreeMap<ObjectId, BTreeMap<String, i64>>>,
    attached: RefCell<BTreeMap<ObjectId, Vec<ObjectId>>>,
}

impl TestProvider {
    /// A provider that returns nothing for any object.
    pub fn empty() -> TestProvider {
        TestProvider::default()
    }

    /// A provider that returns `fallback` for any object without its own characteristics.
    pub fn with_fallback(fallback: Characteristics) -> TestProvider {
        let p = TestProvider::default();
        *p.fallback.borrow_mut() = Some(fallback);
        p
    }

    pub fn set_fallback(&self, c: Option<Characteristics>) {
        *self.fallback.borrow_mut() = c;
    }

    pub fn set_chars(&self, id: ObjectId, c: Characteristics) {
        self.chars.borrow_mut().insert(id, c);
    }

    pub fn set_counter(&self, id: ObjectId, kind: &str, n: i64) {
        self.counters
            .borrow_mut()
            .entry(id)
            .or_default()
            .insert(kind.to_string(), n);
    }

    pub fn clear_counters(&self, id: ObjectId) {
        self.counters.borrow_mut().remove(&id);
    }

    pub fn set_attached(&self, id: ObjectId, attached: Vec<ObjectId>) {
        self.attached.borrow_mut().insert(id, attached);
    }
}

impl CharacteristicsProvider for TestProvider {
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        if let Some(c) = self.chars.borrow().get(&id) {
            return Some(c.clone());
        }
        self.fallback.borrow().clone()
    }

    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.counters.borrow().get(&id).cloned().unwrap_or_default()
    }

    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.attached.borrow().get(&id).cloned().unwrap_or_default()
    }
}

/// A provider that only implements the required method (defaults for counters/attachments).
#[derive(Clone, Debug)]
pub struct MinimalProvider(pub Characteristics);

impl CharacteristicsProvider for MinimalProvider {
    fn characteristics(&self, _id: ObjectId) -> Option<Characteristics> {
        Some(self.0.clone())
    }
}

// ---------------------------------------------------------------------------------------------
// encoding
// ---------------------------------------------------------------------------------------------

/// The canonical bytes of `v`.
pub fn bytes<T: CanonicalEncode + ?Sized>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode_canonical(&mut out);
    out
}

/// Asserts that encoding appends to a non-empty buffer without clearing it.
pub fn assert_appends<T: CanonicalEncode + ?Sized>(v: &T) {
    let prefix = vec![0xA5u8, 0x5A, 0x01];
    let mut out = prefix.clone();
    v.encode_canonical(&mut out);
    assert!(out.starts_with(&prefix), "prefix kept");
    assert_eq!(
        &out[prefix.len()..],
        bytes(v).as_slice(),
        "suffix is the encoding"
    );
}

// ---------------------------------------------------------------------------------------------
// deterministic pseudo-random numbers for property tests (R3: seeded, no system source)
// ---------------------------------------------------------------------------------------------

/// A 64-bit linear congruential generator (Knuth MMIX constants).
#[derive(Clone, Debug)]
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    /// A value in `0..n` (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        let i = usize::try_from(self.below(items.len() as u64)).expect("fits");
        items[i]
    }
}
