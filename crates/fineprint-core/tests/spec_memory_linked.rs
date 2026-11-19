// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for durable references, the Linked Store, `MoveFinder` and cross-object links
//! (REQ-CORE-007, CR 607.1, 607.1d, 607.2a, 607.3, 607.5a, 400.7, 400.7j, 400.7e). Black-box:
//! written from the work-item brief only.

#[path = "spec_memory_support.rs"]
mod support;

use std::cmp::Ordering;

use fineprint_core::memory::{
    DurableRef, LinkEntry, LinkedStore, MemoryError, MemoryTable, MoveFinder,
};
use fineprint_core::zones::{Moved, ZoneId, ZoneKind};
use support::*;

// ---------------------------------------------------------------------------------------------
// (a) LinkedStore basics
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007, CR 607.1, 607.3: entries keep insertion order and every LinkEntry variant.
#[test]
fn linked_push_entries_insertion_order_all_variants() {
    let mut s = LinkedStore::new();
    assert_eq!(s, LinkedStore::default());
    assert!(s.is_empty());
    let entries = vec![
        LinkEntry::Text("zeta".into()),
        LinkEntry::Object(DurableRef::from_parts(cid(9), oid(90))),
        LinkEntry::Int(-5),
        LinkEntry::Player(P2),
        LinkEntry::Object(DurableRef::from_parts(cid(1), oid(10))),
        LinkEntry::Text("alpha".into()),
        LinkEntry::Int(-5),
    ];
    for e in &entries {
        s.push(lid(3), e.clone());
    }
    assert_eq!(s.entries(lid(3)), Some(entries.as_slice()));
    assert!(!s.is_empty());
}

// REQ-CORE-007, CR 607.5a: links() ascending; clear_link reports existence; a cleared or
// never-set link is undefined (None).
#[test]
fn linked_links_ascending_clear_and_undefined() {
    let mut s = LinkedStore::new();
    assert_eq!(s.entries(lid(1)), None);
    s.push(lid(5), LinkEntry::Int(1));
    s.push(lid(1), LinkEntry::Int(2));
    s.push(lid(3), LinkEntry::Int(3));
    s.push(lid(1), LinkEntry::Int(4));
    assert_eq!(s.links(), vec![lid(1), lid(3), lid(5)]);
    assert_eq!(
        s.entries(lid(1)),
        Some([LinkEntry::Int(2), LinkEntry::Int(4)].as_slice())
    );
    assert!(s.clear_link(lid(3)));
    assert!(!s.clear_link(lid(3)));
    assert!(!s.clear_link(lid(99)));
    assert_eq!(s.entries(lid(3)), None);
    assert_eq!(s.links(), vec![lid(1), lid(5)]);
    assert!(s.clear_link(lid(1)));
    assert!(s.clear_link(lid(5)));
    assert!(s.is_empty());
    assert!(s.links().is_empty());
}

// ---------------------------------------------------------------------------------------------
// (b) DurableRef construction
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007, CR 607.1: a durable reference is the card plus the object generation.
#[test]
fn durable_ref_new_and_from_parts() {
    let mut w = World::new();
    let a = w.create_card(cid(44), ZoneKind::Exile, P1);
    let r = DurableRef::new(&w.zones, a).unwrap();
    assert_eq!(r.card(), cid(44));
    assert_eq!(r.generation(), a);
    assert_eq!(r, DurableRef::from_parts(cid(44), a));
    assert_eq!(
        DurableRef::new(&w.zones, oid(777)),
        Err(MemoryError::UnknownObject(oid(777)))
    );
    let p = DurableRef::from_parts(cid(3), oid(8));
    assert_eq!(p.card(), cid(3));
    assert_eq!(p.generation(), oid(8));
}

// REQ-CORE-007: equality is by both parts; the ordering is total and agrees with equality.
#[test]
fn durable_ref_ordering_and_equality() {
    let refs = [
        DurableRef::from_parts(cid(1), oid(1)),
        DurableRef::from_parts(cid(1), oid(2)),
        DurableRef::from_parts(cid(2), oid(1)),
        DurableRef::from_parts(cid(2), oid(2)),
    ];
    for (i, a) in refs.iter().enumerate() {
        for (j, b) in refs.iter().enumerate() {
            assert_eq!(a == b, i == j);
            assert_eq!(a.cmp(b) == Ordering::Equal, i == j);
            assert_eq!(a.cmp(b), b.cmp(a).reverse());
            assert_eq!(a.partial_cmp(b), Some(a.cmp(b)));
        }
    }
    let copy = refs[2];
    assert_eq!(copy, refs[2]);
    let mut sorted = refs.to_vec();
    sorted.reverse();
    sorted.sort();
    let mut again = refs.to_vec();
    again.sort();
    assert_eq!(sorted, again);
}

// ---------------------------------------------------------------------------------------------
// (c) resolve
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007, CR 607.2a, 400.7: an "exiled with" reference resolves while the card's
// current object is that generation, in the expected zone.
#[test]
fn durable_ref_resolves_current_generation() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let e = w.move_to(a, ZoneKind::Exile).new;
    let r = DurableRef::new(&w.zones, e).unwrap();
    assert_eq!(r.resolve(&w.zones, None), Some(e));
    assert_eq!(r.resolve(&w.zones, Some(ZoneKind::Exile)), Some(e));
    for k in ZoneKind::ALL {
        if k != ZoneKind::Exile {
            assert_eq!(r.resolve(&w.zones, Some(k)), None, "{k:?}");
        }
    }
    // A reference to the old generation does not resolve (CR 400.7).
    let stale = DurableRef::from_parts(r.card(), a);
    assert_eq!(stale.resolve(&w.zones, None), None);
}

// REQ-CORE-007, CR 607.2a, 400.7: scenario B: an outside effect moves the exiled card to the
// graveyard; the link breaks. When the card returns to exile as a new object it still does
// not resolve (generation mismatch); an exile -> exile move (CR 406.7) breaks it too.
#[test]
fn durable_ref_broken_by_outside_move() {
    let mut w = World::new();
    let a = w.create(ZoneKind::Battlefield, P1);
    let e = w.move_to(a, ZoneKind::Exile).new;
    let r = DurableRef::new(&w.zones, e).unwrap();
    let g = w.move_to(e, ZoneKind::Graveyard).new;
    assert_eq!(r.resolve(&w.zones, None), None);
    assert_eq!(r.resolve(&w.zones, Some(ZoneKind::Graveyard)), None);
    let e2 = w.move_to(g, ZoneKind::Exile).new;
    assert_ne!(e2, e);
    assert_eq!(r.resolve(&w.zones, Some(ZoneKind::Exile)), None);
    assert_eq!(r.resolve(&w.zones, None), None);

    let r2 = DurableRef::new(&w.zones, e2).unwrap();
    let e3 = w.move_to(e2, ZoneKind::Exile).new;
    assert_ne!(e3, e2);
    assert_eq!(r2.resolve(&w.zones, Some(ZoneKind::Exile)), None);
}

// REQ-CORE-007, CR 400.7: a same-zone reposition keeps the object, so the reference still
// resolves.
#[test]
fn durable_ref_survives_reposition() {
    let mut w = World::new();
    let g = w.create(ZoneKind::Graveyard, P1);
    let _h = w.create(ZoneKind::Graveyard, P1);
    let r = DurableRef::new(&w.zones, g).unwrap();
    let m = w.move_to(g, ZoneKind::Graveyard);
    assert_eq!(m.old, m.new);
    assert_eq!(r.resolve(&w.zones, Some(ZoneKind::Graveyard)), Some(g));
}

// ---------------------------------------------------------------------------------------------
// (d) resolve_objects
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007, CR 607.3, 607.2a, 607.5a: several exiled cards: only those still resolving,
// in insertion order; non-object entries skipped; undefined link gives nothing.
#[test]
fn linked_resolve_objects_several_entries() {
    let mut w = World::new();
    let mut exiled = Vec::new();
    for _ in 0..4 {
        let b = w.create(ZoneKind::Battlefield, P1);
        exiled.push(w.move_to(b, ZoneKind::Exile).new);
    }
    let mut s = LinkedStore::new();
    // Insert in a non-ascending order to check insertion order is kept.
    for &i in &[2usize, 0, 3, 1] {
        s.push(
            lid(7),
            LinkEntry::Object(DurableRef::new(&w.zones, exiled[i]).unwrap()),
        );
        s.push(lid(7), LinkEntry::Player(P1));
        s.push(lid(7), LinkEntry::Int(i as i64));
        s.push(lid(7), LinkEntry::Text("note".into()));
    }
    assert_eq!(
        s.resolve_objects(lid(7), &w.zones, None),
        vec![exiled[2], exiled[0], exiled[3], exiled[1]]
    );
    // One leaves exile (outside move): skipped.
    w.move_to(exiled[3], ZoneKind::Hand);
    assert_eq!(
        s.resolve_objects(lid(7), &w.zones, Some(ZoneKind::Exile)),
        vec![exiled[2], exiled[0], exiled[1]]
    );
    assert!(
        s.resolve_objects(lid(7), &w.zones, Some(ZoneKind::Graveyard))
            .is_empty()
    );
    assert!(s.resolve_objects(lid(8), &w.zones, None).is_empty());
    let finder = MoveFinder::new();
    assert!(
        s.resolve_objects_with(lid(8), &w.zones, None, &finder)
            .is_empty()
    );
}

// ---------------------------------------------------------------------------------------------
// (e) MoveFinder (CR 400.7j)
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007, CR 400.7j: scenario C: one effect exiles a card and returns it to a public
// zone; that effect's finder finds the new object; plain resolution does not.
#[test]
fn move_finder_finds_object_moved_by_same_effect() {
    for back in [ZoneKind::Battlefield, ZoneKind::Graveyard] {
        let mut w = World::new();
        let b = w.create(ZoneKind::Battlefield, P1);
        let r = DurableRef::new(&w.zones, b).unwrap();
        let mut finder = MoveFinder::new();
        assert_eq!(finder, MoveFinder::default());
        let m1 = w.move_to(b, ZoneKind::Exile);
        assert!(finder.record(&m1));
        assert_eq!(finder.find(b), Some(m1.new));
        let m2 = w.move_to(m1.new, back);
        assert!(finder.record(&m2), "{back:?}");
        assert_eq!(finder.find(b), Some(m2.new));
        assert_eq!(r.resolve(&w.zones, None), None);
        assert_eq!(finder.resolve(&r, &w.zones, None), Some(m2.new));
        assert_eq!(finder.resolve(&r, &w.zones, Some(back)), Some(m2.new));
        assert_eq!(finder.resolve(&r, &w.zones, Some(ZoneKind::Exile)), None);

        let mut s = LinkedStore::new();
        s.push(lid(1), LinkEntry::Object(r));
        assert!(s.resolve_objects(lid(1), &w.zones, None).is_empty());
        assert_eq!(
            s.resolve_objects_with(lid(1), &w.zones, None, &finder),
            vec![m2.new]
        );
        // Another effect's finder finds nothing.
        let other = MoveFinder::new();
        assert_eq!(other.find(b), None);
        assert_eq!(other.resolve(&r, &w.zones, None), None);
        assert!(
            s.resolve_objects_with(lid(1), &w.zones, None, &other)
                .is_empty()
        );
    }
}

// REQ-CORE-007, CR 400.7j: a chain across two public moves yields the latest object.
#[test]
fn move_finder_chain_yields_latest() {
    let mut w = World::new();
    let b = w.create(ZoneKind::Battlefield, P1);
    let r = DurableRef::new(&w.zones, b).unwrap();
    let mut finder = MoveFinder::new();
    let m1 = w.move_to(b, ZoneKind::Exile);
    let m2 = w.move_to(m1.new, ZoneKind::Graveyard);
    let m3 = w.move_to(m2.new, ZoneKind::Battlefield);
    assert!(finder.record(&m1));
    assert!(finder.record(&m2));
    assert!(finder.record(&m3));
    assert_eq!(finder.find(b), Some(m3.new));
    assert_eq!(
        finder.resolve(&r, &w.zones, Some(ZoneKind::Battlefield)),
        Some(m3.new)
    );
    assert_eq!(finder.find(oid(12_345)), None);
}

// REQ-CORE-007, CR 400.7j: moves to hidden zones are not recorded and remove the chain.
#[test]
fn move_finder_hidden_zone_not_recorded_and_removes_chain() {
    for hidden in [ZoneKind::Hand, ZoneKind::Library] {
        let mut w = World::new();
        let b = w.create(ZoneKind::Battlefield, P1);
        let r = DurableRef::new(&w.zones, b).unwrap();
        let mut finder = MoveFinder::new();
        let m1 = w.move_to(b, ZoneKind::Exile);
        assert!(finder.record(&m1));
        let m2 = w.move_to(m1.new, hidden);
        assert!(!finder.record(&m2), "{hidden:?}");
        assert_eq!(finder.find(b), None);
        assert_eq!(finder.find(m1.new), None);
        assert_eq!(finder.find(m2.old), None);
        assert_eq!(finder.resolve(&r, &w.zones, None), None);

        // A direct move to a hidden zone is never recorded.
        let c = w.create(ZoneKind::Graveyard, P2);
        let mut f2 = MoveFinder::new();
        let m = w.move_to(c, hidden);
        assert!(!f2.record(&m));
        assert_eq!(f2.find(c), None);
        assert_eq!(f2, MoveFinder::new());
    }
}

// REQ-CORE-007, CR 400.7j, 400.7: a same-zone reposition (old == new) is not recorded.
#[test]
fn move_finder_reposition_not_recorded() {
    let mut w = World::new();
    let g = w.create(ZoneKind::Graveyard, P1);
    let mut finder = MoveFinder::new();
    let m = w.move_to(g, ZoneKind::Graveyard);
    assert_eq!(m.old, m.new);
    assert!(!finder.record(&m));
    assert_eq!(finder.find(g), None);
    // Also a synthetic old == new move, even into a public zone.
    let fake = Moved {
        old: oid(50),
        new: oid(50),
        from: ZoneId::Battlefield,
        to: ZoneId::Battlefield,
    };
    assert!(!finder.record(&fake));
    assert_eq!(finder, MoveFinder::new());
}

// REQ-CORE-007, CR 400.7j: once the found object has moved on (not by this effect), the
// finder no longer resolves it, nor when the zone does not match `expected`.
#[test]
fn move_finder_does_not_resolve_after_object_moves_on() {
    let mut w = World::new();
    let b = w.create(ZoneKind::Battlefield, P1);
    let mut finder = MoveFinder::new();
    let m1 = w.move_to(b, ZoneKind::Exile);
    assert!(finder.record(&m1));
    let r = DurableRef::from_parts(w.zones.object(m1.new).unwrap().card, b);
    assert_eq!(
        finder.resolve(&r, &w.zones, Some(ZoneKind::Exile)),
        Some(m1.new)
    );
    assert_eq!(
        finder.resolve(&r, &w.zones, Some(ZoneKind::Battlefield)),
        None
    );
    // An outside effect moves it on; the finder does not follow.
    let m2 = w.move_to(m1.new, ZoneKind::Graveyard);
    assert_eq!(finder.resolve(&r, &w.zones, Some(ZoneKind::Exile)), None);
    assert_eq!(
        finder.resolve(&r, &w.zones, Some(ZoneKind::Graveyard)),
        None
    );
    assert_eq!(finder.resolve(&r, &w.zones, None), None);
    assert_ne!(m2.new, m1.new);
}

// REQ-CORE-007, CR 400.7j: finder.resolve prefers plain resolution when the ref still resolves.
#[test]
fn move_finder_resolve_falls_through_to_plain_resolution() {
    let mut w = World::new();
    let g = w.create(ZoneKind::Graveyard, P1);
    let r = DurableRef::new(&w.zones, g).unwrap();
    let finder = MoveFinder::new();
    assert_eq!(finder.resolve(&r, &w.zones, None), Some(g));
    assert_eq!(
        finder.resolve(&r, &w.zones, Some(ZoneKind::Graveyard)),
        Some(g)
    );
    assert_eq!(finder.resolve(&r, &w.zones, Some(ZoneKind::Exile)), None);
}

// ---------------------------------------------------------------------------------------------
// (f) link_created (CR 607.1d)
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007, CR 607.1d, 607.3: the source stores the created objects under the link, in
// order; each created object records the source.
#[test]
fn link_created_links_source_and_created() {
    let mut w = World::new();
    let src = w.create(ZoneKind::Battlefield, P1);
    let c1 = w.create(ZoneKind::Battlefield, P1);
    let c2 = w.create(ZoneKind::Exile, P2);
    w.memories.link_created(&w.zones, src, c1, lid(4)).unwrap();
    w.memories.link_created(&w.zones, src, c2, lid(4)).unwrap();
    let sm = w.memories.get(src).unwrap();
    assert_eq!(
        sm.linked().entries(lid(4)),
        Some(
            [
                LinkEntry::Object(DurableRef::new(&w.zones, c1).unwrap()),
                LinkEntry::Object(DurableRef::new(&w.zones, c2).unwrap()),
            ]
            .as_slice()
        )
    );
    assert_eq!(
        sm.linked().resolve_objects(lid(4), &w.zones, None),
        vec![c1, c2]
    );
    for c in [c1, c2] {
        let ls = w
            .memories
            .get(c)
            .unwrap()
            .link_source(lid(4))
            .expect("source");
        assert_eq!(ls, DurableRef::new(&w.zones, src).unwrap());
        assert_eq!(ls.resolve(&w.zones, Some(ZoneKind::Battlefield)), Some(src));
        assert_eq!(w.memories.get(c).unwrap().link_source(lid(5)), None);
    }
    // When the source leaves, the created object's link no longer resolves (CR 400.7).
    w.move_to(src, ZoneKind::Graveyard);
    let ls = w.memories.get(c1).unwrap().link_source(lid(4)).unwrap();
    assert_eq!(ls.resolve(&w.zones, None), None);
}

// REQ-CORE-007, CR 607.1d: either object absent is UnknownObject; the table is unchanged.
#[test]
fn link_created_unknown_object_is_atomic() {
    let mut w = World::new();
    let src = w.create(ZoneKind::Battlefield, P1);
    let c = w.create(ZoneKind::Battlefield, P1);
    w.memories.link_created(&w.zones, src, c, lid(1)).unwrap();
    let before: MemoryTable = w.memories.clone();
    let ghost = oid(5_000);
    assert_eq!(
        w.memories.link_created(&w.zones, src, ghost, lid(2)),
        Err(MemoryError::UnknownObject(ghost))
    );
    assert_eq!(w.memories, before);
    assert_eq!(
        w.memories.link_created(&w.zones, ghost, c, lid(2)),
        Err(MemoryError::UnknownObject(ghost))
    );
    assert_eq!(w.memories, before);
    // On a fresh table too: nothing created.
    let mut t = MemoryTable::new();
    assert!(t.link_created(&w.zones, ghost, c, lid(1)).is_err());
    assert!(t.link_created(&w.zones, src, ghost, lid(1)).is_err());
    assert_eq!(t, MemoryTable::new());
    assert!(t.is_empty());
}
