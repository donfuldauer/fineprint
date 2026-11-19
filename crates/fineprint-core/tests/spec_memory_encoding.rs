// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the canonical encodings of the memory types (REQ-CORE-006, REQ-CORE-001):
//! deterministic, sensitive to every field, appending, variant-tagged and independent of
//! insertion order for keyed maps. Black-box: written from the work-item brief only.

#[path = "spec_memory_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::{IdGenerator, ObjectId, PlayerId};
use fineprint_core::memory::{
    ChoiceValue, DurableRef, LinkEntry, LinkedStore, LkiRecord, LkiStore, Memory, MemoryTable,
    SpellHistory,
};
use fineprint_core::zones::{MoveRequest, ZoneKind};
use support::*;

/// Asserts that all the byte strings are pairwise different.
fn all_distinct(label: &str, encodings: &[Vec<u8>]) {
    for (i, a) in encodings.iter().enumerate() {
        for (j, b) in encodings.iter().enumerate().skip(i + 1) {
            assert_ne!(a, b, "{label}: variants {i} and {j} encode the same");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// DurableRef
// ---------------------------------------------------------------------------------------------

// REQ-CORE-006, REQ-CORE-001: DurableRef bytes are deterministic and depend on both parts.
#[test]
fn enc_durable_ref() {
    let a = DurableRef::from_parts(cid(1), oid(2));
    assert_eq!(bytes(&a), bytes(&DurableRef::from_parts(cid(1), oid(2))));
    #[allow(clippy::clone_on_copy)]
    let c = a.clone();
    assert_eq!(bytes(&c), bytes(&a));
    all_distinct(
        "DurableRef",
        &[
            bytes(&a),
            bytes(&DurableRef::from_parts(cid(2), oid(1))),
            bytes(&DurableRef::from_parts(cid(1), oid(1))),
            bytes(&DurableRef::from_parts(cid(3), oid(2))),
        ],
    );
    assert_appends(&a);
}

// ---------------------------------------------------------------------------------------------
// LinkedStore / LinkEntry
// ---------------------------------------------------------------------------------------------

fn store_with(entries: &[(u64, LinkEntry)]) -> LinkedStore {
    let mut s = LinkedStore::new();
    for (l, e) in entries {
        s.push(lid(*l), e.clone());
    }
    s
}

// REQ-CORE-006, REQ-CORE-007: LinkEntry variants with the same payload encode differently;
// every payload and the link id matter; entry order within a link matters.
#[test]
fn enc_linked_store_variants_and_fields() {
    let five = 5u64;
    let variants = [
        LinkEntry::Player(PlayerId::new(five)),
        LinkEntry::Int(5),
        LinkEntry::Object(DurableRef::from_parts(cid(five), oid(five))),
        LinkEntry::Text("5".into()),
    ];
    let mut encs: Vec<Vec<u8>> = variants
        .iter()
        .map(|v| bytes(&store_with(&[(1, v.clone())])))
        .collect();
    encs.push(bytes(&store_with(&[(2, LinkEntry::Int(5))])));
    encs.push(bytes(&store_with(&[(1, LinkEntry::Int(6))])));
    encs.push(bytes(&store_with(&[(1, LinkEntry::Text("6".into()))])));
    encs.push(bytes(&store_with(&[(
        1,
        LinkEntry::Player(PlayerId::new(6)),
    )])));
    encs.push(bytes(&store_with(&[(
        1,
        LinkEntry::Object(DurableRef::from_parts(cid(5), oid(6))),
    )])));
    encs.push(bytes(&store_with(&[
        (1, LinkEntry::Int(5)),
        (1, LinkEntry::Int(6)),
    ])));
    encs.push(bytes(&store_with(&[
        (1, LinkEntry::Int(6)),
        (1, LinkEntry::Int(5)),
    ])));
    encs.push(bytes(&LinkedStore::new()));
    all_distinct("LinkedStore", &encs);
}

// REQ-CORE-006, REQ-CORE-001: equal stores give equal bytes; building links in a different
// LinkId order gives the same value and bytes; encoding appends; clones encode the same.
#[test]
fn enc_linked_store_order_independent_and_appends() {
    let a = store_with(&[
        (2, LinkEntry::Int(1)),
        (1, LinkEntry::Text("x".into())),
        (2, LinkEntry::Int(2)),
    ]);
    let b = store_with(&[
        (1, LinkEntry::Text("x".into())),
        (2, LinkEntry::Int(1)),
        (2, LinkEntry::Int(2)),
    ]);
    assert_eq!(a, b);
    assert_eq!(bytes(&a), bytes(&b));
    assert_eq!(bytes(&a.clone()), bytes(&a));
    assert_eq!(a.clone(), a);
    assert_appends(&a);
    assert_appends(&LinkedStore::new());
}

// ---------------------------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------------------------

fn base_history() -> SpellHistory {
    let mut mana_spent = BTreeMap::new();
    mana_spent.insert(("U".to_string(), "land".to_string()), 1);
    let mut additional_costs = BTreeMap::new();
    additional_costs.insert("kicker".to_string(), true);
    SpellHistory {
        costs_paid: vec!["mana".into()],
        mana_spent,
        additional_costs,
        x_value: Some(1),
        cast_face: Some("test.face_a".into()),
        exiled_to_pay: vec![DurableRef::from_parts(cid(8), oid(80))],
    }
}

fn base_memory() -> Memory {
    let mut m = Memory::new();
    m.set_choice("c", ChoiceValue::Number(1));
    m.set_persistent_choice("p", ChoiceValue::Color("green".into()));
    m.set_spell_history(base_history());
    m.linked_mut().push(lid(1), LinkEntry::Int(1));
    m
}

fn with_history(f: impl FnOnce(&mut SpellHistory)) -> Memory {
    let mut m = base_memory();
    let mut h = base_history();
    f(&mut h);
    m.set_spell_history(h);
    m
}

// REQ-CORE-006, REQ-CORE-007: a change in any single Memory or SpellHistory field changes the
// bytes.
#[test]
fn enc_memory_every_field_matters() {
    assert_eq!(bytes(&base_memory()), bytes(&base_memory()));
    let mut variants = vec![base_memory(), Memory::new()];
    // ordinary choices
    let mut m = base_memory();
    m.set_choice("c", ChoiceValue::Number(2));
    variants.push(m);
    let mut m = base_memory();
    m.set_choice("d", ChoiceValue::Number(1));
    variants.push(m);
    // persistent choices
    let mut m = base_memory();
    m.set_persistent_choice("p", ChoiceValue::Color("red".into()));
    variants.push(m);
    let mut m = base_memory();
    m.set_persistent_choice("q", ChoiceValue::Number(1));
    variants.push(m);
    // linked store
    let mut m = base_memory();
    m.linked_mut().push(lid(1), LinkEntry::Int(2));
    variants.push(m);
    let mut m = base_memory();
    m.linked_mut().push(lid(2), LinkEntry::Int(1));
    variants.push(m);
    // spell history: each field
    variants.push(with_history(|h| h.costs_paid.push("tap".into())));
    variants.push(with_history(|h| {
        h.mana_spent
            .insert(("U".to_string(), "land".to_string()), 2);
    }));
    variants.push(with_history(|h| {
        h.mana_spent
            .insert(("R".to_string(), "land".to_string()), 1);
    }));
    variants.push(with_history(|h| {
        h.additional_costs.insert("kicker".to_string(), false);
    }));
    variants.push(with_history(|h| {
        h.additional_costs.insert("gift".to_string(), true);
    }));
    variants.push(with_history(|h| h.x_value = Some(2)));
    variants.push(with_history(|h| h.x_value = None));
    variants.push(with_history(|h| h.cast_face = Some("test.face_b".into())));
    variants.push(with_history(|h| h.cast_face = None));
    variants.push(with_history(|h| {
        h.exiled_to_pay = vec![DurableRef::from_parts(cid(8), oid(81))];
    }));
    variants.push(with_history(|h| h.exiled_to_pay.clear()));
    // no history at all
    let mut m = Memory::new();
    m.set_choice("c", ChoiceValue::Number(1));
    m.set_persistent_choice("p", ChoiceValue::Color("green".into()));
    m.linked_mut().push(lid(1), LinkEntry::Int(1));
    variants.push(m);
    let encs: Vec<Vec<u8>> = variants.iter().map(bytes).collect();
    all_distinct("Memory", &encs);
}

// REQ-CORE-006, REQ-CORE-007, CR 607.2p: an ordinary and a persistent choice with the same key
// and value are different values and encode differently.
#[test]
fn enc_memory_ordinary_vs_persistent_choice() {
    let mut a = Memory::new();
    a.set_choice("k", ChoiceValue::Mode(1));
    let mut b = Memory::new();
    b.set_persistent_choice("k", ChoiceValue::Mode(1));
    assert_ne!(a, b);
    assert_ne!(bytes(&a), bytes(&b));
}

// REQ-CORE-006, REQ-CORE-007: ChoiceValue variants with the same payload encode differently.
#[test]
fn enc_choice_value_variants_distinct() {
    let values = [
        ChoiceValue::Color("x".into()),
        ChoiceValue::CreatureType("x".into()),
        ChoiceValue::CardName("x".into()),
        ChoiceValue::Anchor("x".into()),
        ChoiceValue::Player(PlayerId::new(5)),
        ChoiceValue::Number(5),
        ChoiceValue::Mode(5),
    ];
    let encs: Vec<Vec<u8>> = values
        .iter()
        .map(|v| {
            let mut m = Memory::new();
            m.set_choice("k", v.clone());
            bytes(&m)
        })
        .collect();
    all_distinct("ChoiceValue", &encs);
}

// REQ-CORE-006, REQ-CORE-007, CR 607.1d: a link source is part of the Memory encoding.
#[test]
fn enc_memory_link_source_matters() {
    let mut w = World::new();
    let s1 = w.create(ZoneKind::Battlefield, P1);
    let s2 = w.create(ZoneKind::Battlefield, P1);
    let c = w.create(ZoneKind::Battlefield, P1);
    let mut t1 = MemoryTable::new();
    t1.link_created(&w.zones, s1, c, lid(1)).unwrap();
    let mut t2 = MemoryTable::new();
    t2.link_created(&w.zones, s2, c, lid(1)).unwrap();
    let mut t3 = MemoryTable::new();
    t3.link_created(&w.zones, s1, c, lid(2)).unwrap();
    all_distinct(
        "link source",
        &[
            bytes(t1.get(c).unwrap()),
            bytes(t2.get(c).unwrap()),
            bytes(t3.get(c).unwrap()),
            bytes(&Memory::new()),
        ],
    );
    assert_ne!(bytes(&t1), bytes(&t2));
}

// REQ-CORE-006, REQ-CORE-001: choices keyed by name: the order they were made in does not
// change the value or the bytes; encoding appends; clones encode the same.
#[test]
fn enc_memory_order_independent_and_appends() {
    let mut a = Memory::new();
    a.set_choice("b", ChoiceValue::Number(2));
    a.set_choice("a", ChoiceValue::Number(1));
    a.set_persistent_choice("z", ChoiceValue::Mode(1));
    a.set_persistent_choice("y", ChoiceValue::Mode(2));
    a.linked_mut().push(lid(9), LinkEntry::Int(1));
    a.linked_mut().push(lid(3), LinkEntry::Int(1));
    let mut b = Memory::new();
    b.linked_mut().push(lid(3), LinkEntry::Int(1));
    b.set_persistent_choice("y", ChoiceValue::Mode(2));
    b.set_choice("a", ChoiceValue::Number(1));
    b.linked_mut().push(lid(9), LinkEntry::Int(1));
    b.set_persistent_choice("z", ChoiceValue::Mode(1));
    b.set_choice("b", ChoiceValue::Number(2));
    assert_eq!(a, b);
    assert_eq!(bytes(&a), bytes(&b));
    assert_eq!(a.clone(), a);
    assert_eq!(bytes(&a.clone()), bytes(&a));
    assert_appends(&a);
    assert_appends(&Memory::new());
}

// ---------------------------------------------------------------------------------------------
// MemoryTable
// ---------------------------------------------------------------------------------------------

// REQ-CORE-006, REQ-CORE-001: the table encoding depends on ids and memories, not on the
// order entries were created in.
#[test]
fn enc_memory_table() {
    let mut a = MemoryTable::new();
    *a.get_mut_or_default(oid(5)) = base_memory();
    a.get_mut_or_default(oid(3))
        .set_choice("k", ChoiceValue::Number(3));
    let mut b = MemoryTable::new();
    b.get_mut_or_default(oid(3))
        .set_choice("k", ChoiceValue::Number(3));
    *b.get_mut_or_default(oid(5)) = base_memory();
    assert_eq!(a, b);
    assert_eq!(bytes(&a), bytes(&b));
    assert_eq!(bytes(&a.clone()), bytes(&a));
    assert_appends(&a);
    assert_appends(&MemoryTable::new());

    let mut other_id = MemoryTable::new();
    *other_id.get_mut_or_default(oid(6)) = base_memory();
    other_id
        .get_mut_or_default(oid(3))
        .set_choice("k", ChoiceValue::Number(3));
    let mut other_mem = a.clone();
    other_mem
        .get_mut_or_default(oid(3))
        .set_choice("k", ChoiceValue::Number(4));
    let mut extra = a.clone();
    extra.get_mut_or_default(oid(9));
    all_distinct(
        "MemoryTable",
        &[
            bytes(&a),
            bytes(&other_id),
            bytes(&other_mem),
            bytes(&extra),
            bytes(&MemoryTable::new()),
        ],
    );
}

// ---------------------------------------------------------------------------------------------
// LkiRecord / LkiStore
// ---------------------------------------------------------------------------------------------

#[derive(Clone)]
struct Spec {
    last_issued: u64,
    card: u64,
    owner: PlayerId,
    zone: ZoneKind,
    controller: Option<PlayerId>,
    face_down: bool,
    name: &'static str,
    counter: Option<i64>,
    attached: Vec<ObjectId>,
    tapped: bool,
    choice: Option<ChoiceValue>,
}

fn spec(zone: ZoneKind) -> Spec {
    Spec {
        last_issued: 0,
        card: 1,
        owner: P1,
        zone,
        controller: None,
        face_down: false,
        name: "test.base_1_1",
        counter: None,
        attached: Vec::new(),
        tapped: false,
        choice: None,
    }
}

/// Builds one object as `s` describes, captures its LKI for a move to exile, and returns it.
fn record(s: &Spec) -> LkiRecord {
    let mut w = World::with_generator(IdGenerator::from_last_issued(s.last_issued));
    let req = MoveRequest {
        controller: s.controller,
        face_down: s.face_down,
        ..MoveRequest::new(s.zone)
    };
    let id = w
        .zones
        .create_object(&mut w.ids, cid(s.card), s.owner, req)
        .unwrap();
    if s.zone == ZoneKind::Battlefield {
        w.permanents
            .enter(id, s.controller.unwrap_or(s.owner), 1)
            .unwrap();
        w.permanents.set_tapped(id, s.tapped).unwrap();
    }
    w.provider.set_chars(id, chars(s.name, 1, 1));
    if let Some(n) = s.counter {
        w.provider.set_counter(id, "+1/+1", n);
    }
    w.provider.set_attached(id, s.attached.clone());
    if let Some(c) = &s.choice {
        w.memory_mut(id).set_choice("k", c.clone());
    }
    assert_eq!(w.capture(id, &MoveRequest::new(ZoneKind::Exile)), Ok(true));
    w.lki.get(id).unwrap().clone()
}

// REQ-CORE-006, REQ-CORE-003: equal records give equal bytes; each getter reachable through
// the provider/zones changes the bytes when it alone changes.
#[test]
fn enc_lki_record_every_getter_matters() {
    let base = spec(ZoneKind::Graveyard);
    assert_eq!(bytes(&record(&base)), bytes(&record(&base)));
    assert_eq!(record(&base), record(&base));
    let rec = record(&base);
    assert_eq!(bytes(&rec.clone()), bytes(&rec));
    assert_appends(&rec);

    let mut variants = vec![base.clone()];
    variants.push(Spec {
        last_issued: 50,
        ..base.clone()
    });
    variants.push(Spec {
        card: 2,
        ..base.clone()
    });
    variants.push(Spec {
        owner: P2,
        ..base.clone()
    });
    variants.push(Spec {
        zone: ZoneKind::Hand,
        ..base.clone()
    });
    variants.push(Spec {
        face_down: true,
        ..base.clone()
    });
    variants.push(Spec {
        name: "test.other_1_1",
        ..base.clone()
    });
    variants.push(Spec {
        counter: Some(1),
        ..base.clone()
    });
    variants.push(Spec {
        counter: Some(2),
        ..base.clone()
    });
    variants.push(Spec {
        attached: vec![oid(500)],
        ..base.clone()
    });
    variants.push(Spec {
        choice: Some(ChoiceValue::Number(1)),
        ..base.clone()
    });
    let records: Vec<LkiRecord> = variants.iter().map(record).collect();
    // Sanity: each variant really differs only where intended.
    assert_ne!(records[1].object(), records[0].object());
    assert_eq!(records[2].object(), records[0].object());
    let encs: Vec<Vec<u8>> = records.iter().map(bytes).collect();
    all_distinct("LkiRecord (graveyard)", &encs);

    // On the battlefield: controller and permanent status matter.
    let bf = spec(ZoneKind::Battlefield);
    let bf_variants = [
        bf.clone(),
        Spec {
            controller: Some(P2),
            ..bf.clone()
        },
        Spec {
            tapped: true,
            ..bf.clone()
        },
        Spec {
            zone: ZoneKind::Stack,
            ..bf.clone()
        },
    ];
    let encs: Vec<Vec<u8>> = bf_variants.iter().map(|s| bytes(&record(s))).collect();
    all_distinct("LkiRecord (battlefield)", &encs);
}

// REQ-CORE-006, REQ-CORE-001: the LKI store is keyed by ObjectId: capture order does not
// change the value or the bytes; the contents do.
#[test]
fn enc_lki_store_order_independent() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let b = w.create(ZoneKind::Graveyard, P2);
    let req = MoveRequest::new(ZoneKind::Exile);
    let mut one = w.clone();
    let mut two = w.clone();
    one.capture(a, &req).unwrap();
    one.capture(b, &req).unwrap();
    two.capture(b, &req).unwrap();
    two.capture(a, &req).unwrap();
    assert_eq!(one.lki, two.lki);
    assert_eq!(bytes(&one.lki), bytes(&two.lki));
    assert_eq!(bytes(&one.lki.clone()), bytes(&one.lki));
    assert_appends(&one.lki);
    assert_appends(&LkiStore::new());

    let mut only_a = w.clone();
    only_a.capture(a, &req).unwrap();
    all_distinct(
        "LkiStore",
        &[bytes(&one.lki), bytes(&only_a.lki), bytes(&LkiStore::new())],
    );
}
