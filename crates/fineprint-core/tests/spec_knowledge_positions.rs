// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Position knowledge of libraries and the SRS 2.4 shuffle/reveal sequences (REQ-CORE-004;
//! CR 401.2, 401.3, 401.4, 701.24a, 701.24b, 701.20a/b). Black-box: written from the
//! work-item brief only.

#[path = "spec_knowledge_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::ObjectId;
use fineprint_core::knowledge::KnowledgeError;
use fineprint_core::zones::{Position, ZoneKind};
use support::*;

/// A world whose P1 library is `n` cards, top first, plus an empty P2 library.
fn lib_world(n: usize, three: bool) -> (KWorld, Vec<ObjectId>) {
    let mut w = if three {
        KWorld::three()
    } else {
        KWorld::two()
    };
    let ids: Vec<ObjectId> = (0..n)
        .map(|_| w.create_bottom(ZoneKind::Library, P1))
        .collect();
    assert_eq!(w.library(P1), ids);
    (w, ids)
}

fn map(entries: &[(usize, ObjectId)]) -> BTreeMap<usize, ObjectId> {
    entries.iter().copied().collect()
}

/// REQ-CORE-004, CR 701.24a, SRS 2.4 "Shuffle": a player who looked at the top two cards knows
/// their positions until a shuffle; the shuffle clears positions for every viewer but keeps the
/// set knowledge (known_to and the projected known_cards).
#[test]
fn look_at_top_two_then_shuffle() {
    let (mut w, lib) = lib_world(5, true);
    w.k.set_position_known(&w.zones, P1, P1, 0, lib[0])
        .expect("pos");
    w.k.set_position_known(&w.zones, P1, P1, 1, lib[1])
        .expect("pos");
    w.k.set_position_known(&w.zones, P2, P1, 4, lib[4])
        .expect("pos");
    assert_eq!(w.positions(P1, P1), map(&[(0, lib[0]), (1, lib[1])]));
    assert_eq!(w.positions(P2, P1), map(&[(4, lib[4])]));
    assert!(w.positions(P3, P1).is_empty());
    // set_position_known also adds the viewer to KnownTo
    assert_eq!(w.known_to(lib[0]), set(&[P1]));
    assert_eq!(w.known_to(lib[1]), set(&[P1]));
    assert_eq!(w.known_to(lib[4]), set(&[P2]));
    let c0 = w.card(lib[0]);
    let c1 = w.card(lib[1]);
    let c4 = w.card(lib[4]);
    assert_eq!(lib_known(&w.view(P1), P1), set(&[c0, c1]));

    w.k.shuffle_library(&w.zones, P1, &[]).expect("shuffle");

    for v in [P1, P2, P3] {
        assert!(w.positions(v, P1).is_empty(), "{v:?}: positions cleared");
    }
    assert_eq!(w.known_to(lib[0]), set(&[P1]), "set knowledge kept");
    assert_eq!(w.known_to(lib[1]), set(&[P1]));
    assert_eq!(w.known_to(lib[4]), set(&[P2]));
    assert_eq!(lib_known(&w.view(P1), P1), set(&[c0, c1]));
    assert_eq!(lib_known(&w.view(P2), P1), set(&[c4]));
    assert!(lib_known(&w.view(P3), P1).is_empty());
    assert_eq!(w.view(P3).players[&P1].library.count, 5, "CR 401.3");
}

/// REQ-CORE-004, CR 701.20a/b, 701.24a, SRS 2.4: a revealed card put into a library and
/// shuffled stays known as a member of the set; the view has its CardId and no position.
#[test]
fn reveal_then_put_in_library_then_shuffle() {
    let mut w = KWorld::two();
    for _ in 0..3 {
        w.create(ZoneKind::Library, P1);
    }
    let h = w.create(ZoneKind::Hand, P1);
    let card = w.card(h);
    w.k.reveal_to_all(&w.zones, h).expect("reveal");
    let moved = w.mv(h, KWorld::req_at(ZoneKind::Library, Position::Index(1)));
    let in_lib = moved.new;
    assert_eq!(w.library(P1)[1], in_lib);
    // physically shuffle (reposition) and tell the knowledge
    w.mv(in_lib, KWorld::req_at(ZoneKind::Library, Position::Bottom));
    w.k.shuffle_library(&w.zones, P1, &[]).expect("shuffle");
    for p in [P1, P2] {
        assert!(w.known_to(in_lib).contains(&p), "{p:?} keeps set knowledge");
        assert!(w.can(p, in_lib));
        assert!(w.positions(p, P1).is_empty(), "no positions");
        let v = w.view(p);
        assert_eq!(lib_known(&v, P1), set(&[card]));
        assert_eq!(v.players[&P1].library.count, 4);
        assert!(
            find_view(&v, in_lib).is_none(),
            "no ObjectId of a library card"
        );
    }
}

/// REQ-CORE-004, CR 701.24b: an excluded card still at its recorded index keeps its entry; a
/// non-excluded card is always cleared.
#[test]
fn shuffle_excluding_found_card_at_its_index() {
    let (mut w, lib) = lib_world(4, false);
    for (i, id) in lib.iter().enumerate() {
        w.k.set_position_known(&w.zones, P1, P1, i, *id)
            .expect("pos");
    }
    w.k.set_position_known(&w.zones, P2, P1, 0, lib[0])
        .expect("pos");
    w.k.set_position_known(&w.zones, P2, P1, 3, lib[3])
        .expect("pos");
    w.k.shuffle_library(&w.zones, P1, &[lib[0]])
        .expect("shuffle");
    assert_eq!(w.positions(P1, P1), map(&[(0, lib[0])]));
    assert_eq!(w.positions(P2, P1), map(&[(0, lib[0])]));
}

/// REQ-CORE-004, CR 701.24b: an excluded card no longer at its recorded index loses the
/// entry, and it stays lost when the card returns to that index later.
#[test]
fn shuffle_excluding_card_that_moved_loses_its_entry() {
    let (mut w, lib) = lib_world(4, false);
    w.k.set_position_known(&w.zones, P1, P1, 1, lib[1])
        .expect("pos");
    w.k.set_position_known(&w.zones, P1, P1, 3, lib[3])
        .expect("pos");
    // move the top card to the bottom without telling the knowledge: lib[1] is now at 0
    w.raw_mv(lib[0], KWorld::req_at(ZoneKind::Library, Position::Bottom));
    assert_eq!(w.library(P1), vec![lib[1], lib[2], lib[3], lib[0]]);
    w.k.shuffle_library(&w.zones, P1, &[lib[1]])
        .expect("shuffle");
    assert!(w.positions(P1, P1).is_empty());
    // put it back: lib[1] is at index 1 again; its entry was cleared, not merely hidden
    w.raw_mv(lib[0], KWorld::req_at(ZoneKind::Library, Position::Top));
    assert_eq!(w.library(P1), lib);
    assert!(w.positions(P1, P1).is_empty(), "cleared by the shuffle");
}

/// REQ-CORE-004, CR 701.24a: a non-excluded entry is cleared by a shuffle even if the card
/// happens to stay at the recorded index.
#[test]
fn shuffle_clears_non_excluded_even_when_still_at_index() {
    let (mut w, lib) = lib_world(3, false);
    w.k.set_position_known(&w.zones, P2, P1, 2, lib[2])
        .expect("pos");
    w.k.shuffle_library(&w.zones, P1, &[lib[0]])
        .expect("shuffle");
    assert!(w.positions(P2, P1).is_empty());
    assert_eq!(w.library(P1)[2], lib[2], "still there physically");
}

/// REQ-CORE-004, CR 701.24b: excluded ids not in that library (unknown, in a hand, in another
/// library) give `NotInLibrary`; an unknown owner gives `UnknownPlayer`. Nothing changes.
#[test]
fn shuffle_library_errors() {
    let (mut w, lib) = lib_world(3, false);
    let other_lib = w.create(ZoneKind::Library, P2);
    let hand = w.create(ZoneKind::Hand, P1);
    w.k.set_position_known(&w.zones, P1, P1, 0, lib[0])
        .expect("pos");
    let snapshot = w.k.clone();
    for bad in [oid(9_999), oid(u64::MAX), other_lib, hand] {
        assert_eq!(
            w.k.shuffle_library(&w.zones, P1, &[bad]),
            Err(KnowledgeError::NotInLibrary {
                owner: P1,
                object: bad
            }),
            "{bad:?}"
        );
        assert_eq!(
            w.k.shuffle_library(&w.zones, P1, &[lib[1], bad]),
            Err(KnowledgeError::NotInLibrary {
                owner: P1,
                object: bad
            })
        );
    }
    for bad in [NOBODY, pid(0), pid(u64::MAX)] {
        assert_eq!(
            w.k.shuffle_library(&w.zones, bad, &[]),
            Err(KnowledgeError::UnknownPlayer(bad))
        );
    }
    assert_eq!(w.k, snapshot, "failed shuffles change nothing");
    assert_eq!(w.positions(P1, P1), map(&[(0, lib[0])]));
}

/// REQ-CORE-004, CR 401.4: the owner arranges cards put into the library and records their
/// positions; `position_known` reports exactly those entries, and only to the owner.
#[test]
fn owner_arranges_and_records_positions() {
    let (mut w, _lib) = lib_world(3, false);
    let h1 = w.create(ZoneKind::Hand, P1);
    let h2 = w.create(ZoneKind::Hand, P1);
    // put both on top in an order of the owner's choice: h2 then h1 on top
    let m2 = w.mv(h2, KWorld::req_at(ZoneKind::Library, Position::Top));
    let m1 = w.mv(h1, KWorld::req_at(ZoneKind::Library, Position::Top));
    assert_eq!(w.library(P1)[0], m1.new);
    assert_eq!(w.library(P1)[1], m2.new);
    w.k.set_position_known(&w.zones, P1, P1, 0, m1.new)
        .expect("pos");
    w.k.set_position_known(&w.zones, P1, P1, 1, m2.new)
        .expect("pos");
    assert_eq!(w.positions(P1, P1), map(&[(0, m1.new), (1, m2.new)]));
    assert!(
        w.positions(P2, P1).is_empty(),
        "CR 401.4: order not revealed"
    );
    assert!(!w.can(P2, m1.new));
    assert!(!w.can(P2, m2.new));
}

/// REQ-CORE-004, CR 401.2: after a reorder or removal, stale entries are dropped lazily and
/// no wrong index is ever reported.
#[test]
fn lazy_filtering_drops_stale_entries() {
    let (mut w, lib) = lib_world(4, false);
    w.k.set_position_known(&w.zones, P1, P1, 0, lib[0])
        .expect("pos");
    w.k.set_position_known(&w.zones, P1, P1, 2, lib[2])
        .expect("pos");
    w.k.set_position_known(&w.zones, P1, P1, 3, lib[3])
        .expect("pos");
    // reorder without telling the knowledge: lib[1] to the top shifts lib[0] to 1
    w.raw_mv(lib[1], KWorld::req_at(ZoneKind::Library, Position::Top));
    let live = w.library(P1);
    assert_eq!(live, vec![lib[1], lib[0], lib[2], lib[3]]);
    assert_eq!(w.positions(P1, P1), map(&[(2, lib[2]), (3, lib[3])]));
    // removal without telling the knowledge: lib[2] leaves
    let gone = w.raw_mv(lib[2], KWorld::req_at(ZoneKind::Graveyard, Position::Top));
    let live = w.library(P1);
    let pos = w.positions(P1, P1);
    for (i, id) in &pos {
        assert_eq!(live.get(*i), Some(id), "never a wrong index");
    }
    assert!(!pos.values().any(|id| *id == lib[2] || *id == gone.new));
    assert_eq!(
        pos,
        BTreeMap::new(),
        "lib[3] shifted to 2 so its entry is stale"
    );
}

/// REQ-CORE-004, CR 401.2: `set_position_known` replaces an earlier entry for the same object
/// and an earlier entry for the same index.
#[test]
fn set_position_known_replaces_by_object_and_by_index() {
    let (mut w, lib) = lib_world(3, false);
    w.k.set_position_known(&w.zones, P1, P1, 0, lib[0])
        .expect("pos");
    // physically move lib[0] to index 2 (unannounced), record again: replaces by object
    w.raw_mv(lib[0], KWorld::req_at(ZoneKind::Library, Position::Bottom));
    assert_eq!(w.library(P1), vec![lib[1], lib[2], lib[0]]);
    w.k.set_position_known(&w.zones, P1, P1, 2, lib[0])
        .expect("pos");
    assert_eq!(w.positions(P1, P1), map(&[(2, lib[0])]));
    // move it back to the top: the old index-0 entry must not resurface
    w.raw_mv(lib[0], KWorld::req_at(ZoneKind::Library, Position::Top));
    assert_eq!(w.library(P1), lib);
    assert!(w.positions(P1, P1).is_empty(), "replaced entry is gone");
    // replace by index: record lib[0] at 0, then (after a reorder) lib[1] at 0
    w.k.set_position_known(&w.zones, P1, P1, 0, lib[0])
        .expect("pos");
    w.raw_mv(lib[1], KWorld::req_at(ZoneKind::Library, Position::Top));
    assert_eq!(w.library(P1), vec![lib[1], lib[0], lib[2]]);
    w.k.set_position_known(&w.zones, P1, P1, 0, lib[1])
        .expect("pos");
    assert_eq!(w.positions(P1, P1), map(&[(0, lib[1])]));
    // swap back: the lib[0]-at-0 entry was replaced and must not resurface
    w.raw_mv(lib[0], KWorld::req_at(ZoneKind::Library, Position::Top));
    assert_eq!(w.library(P1), lib);
    assert!(w.positions(P1, P1).is_empty());
}

/// REQ-CORE-004, R12: `set_position_known` errors (IndexMismatch, NotInLibrary,
/// UnknownObject, UnknownPlayer) change nothing.
#[test]
fn set_position_known_errors() {
    let (mut w, lib) = lib_world(3, false);
    let p2_lib = w.create(ZoneKind::Library, P2);
    let hand = w.create(ZoneKind::Hand, P1);
    let bf = w.create(ZoneKind::Battlefield, P1);
    let snapshot = w.k.clone();
    for idx in [1usize, 3, 1_000, usize::MAX] {
        assert_eq!(
            w.k.set_position_known(&w.zones, P1, P1, idx, lib[0]),
            Err(KnowledgeError::IndexMismatch {
                owner: P1,
                index: idx,
                object: lib[0]
            })
        );
    }
    for bad in [hand, bf, p2_lib] {
        assert_eq!(
            w.k.set_position_known(&w.zones, P1, P1, 0, bad),
            Err(KnowledgeError::NotInLibrary {
                owner: P1,
                object: bad
            }),
            "{bad:?}"
        );
    }
    for bad in [oid(77_777), oid(u64::MAX)] {
        assert_eq!(
            w.k.set_position_known(&w.zones, P1, P1, 0, bad),
            Err(KnowledgeError::UnknownObject(bad))
        );
    }
    for bad in [NOBODY, pid(u64::MAX)] {
        assert_eq!(
            w.k.set_position_known(&w.zones, bad, P1, 0, lib[0]),
            Err(KnowledgeError::UnknownPlayer(bad))
        );
        assert_eq!(
            w.k.set_position_known(&w.zones, P1, bad, 0, lib[0]),
            Err(KnowledgeError::UnknownPlayer(bad))
        );
        assert!(w.k.position_known(&w.zones, bad, P1).is_empty());
        assert!(w.k.position_known(&w.zones, P1, bad).is_empty());
    }
    assert_eq!(w.k, snapshot);
    assert!(
        w.known_to(lib[0]).is_empty(),
        "no KnownTo from a failed call"
    );
}

/// REQ-CORE-004: position knowledge is per viewer; one viewer's entries are invisible to the
/// others, and per library owner.
#[test]
fn position_knowledge_is_per_viewer() {
    let (mut w, lib) = lib_world(3, true);
    let p2_top = w.create(ZoneKind::Library, P2);
    w.k.set_position_known(&w.zones, P2, P1, 1, lib[1])
        .expect("pos");
    w.k.set_position_known(&w.zones, P3, P2, 0, p2_top)
        .expect("pos");
    assert_eq!(w.positions(P2, P1), map(&[(1, lib[1])]));
    assert!(w.positions(P1, P1).is_empty());
    assert!(w.positions(P3, P1).is_empty());
    assert!(w.positions(P2, P2).is_empty());
    assert_eq!(w.positions(P3, P2), map(&[(0, p2_top)]));
    assert!(w.positions(P1, P2).is_empty());
    assert_eq!(w.known_to(lib[1]), set(&[P2]));
    assert_eq!(w.known_to(p2_top), set(&[P3]));
}

/// REQ-CORE-004, SRS 2.4: positions never appear in the projection. Two worlds that differ
/// only in position knowledge (same set knowledge) project identically for every viewer.
#[test]
fn positions_never_in_projection() {
    let (mut a, lib) = lib_world(4, true);
    for id in &lib {
        a.k.add_known(&a.zones, *id, P1).expect("add");
        a.k.add_known(&a.zones, *id, P2).expect("add");
    }
    let mut b = a.clone();
    for (i, id) in lib.iter().enumerate() {
        b.k.set_position_known(&b.zones, P1, P1, i, *id)
            .expect("pos");
    }
    b.k.set_position_known(&b.zones, P2, P1, 3, lib[3])
        .expect("pos");
    assert_ne!(a.k, b.k, "the knowledge differs");
    for p in [P1, P2, P3] {
        assert_eq!(a.view(p), b.view(p), "{p:?}");
    }
}
