// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Position knowledge of libraries and the SRS 2.4 shuffle/reveal sequences (REQ-CORE-004;
//! CR 401.2, 401.3, 401.4, 701.24a, 701.24b, 701.20a/b). Black-box: written from the
//! work-item brief only.

#[path = "spec_knowledge_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::knowledge::KnowledgeError;
use fineprint_core::zones::{MoveRequest, OutsidePool, Position, ZoneKind};
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
/// shuffled stays known as a member of the set; the view has its CardId and no position. The
/// other players watched the reveal, so the move is reported with every player as an observer
/// (`on_move_observed`).
#[test]
fn reveal_then_put_in_library_then_shuffle() {
    let mut w = KWorld::two();
    for _ in 0..3 {
        w.create(ZoneKind::Library, P1);
    }
    let h = w.create(ZoneKind::Hand, P1);
    let card = w.card(h);
    w.k.reveal_to_all(&w.zones, h).expect("reveal");
    let moved = w.mv_obs(
        h,
        KWorld::req_at(ZoneKind::Library, Position::Index(1)),
        &[P1, P2],
    );
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

/// REQ-CORE-004, CR 701.20a/b (hidden-source identity rule, design decision of the brief):
/// contrast to the test above: if the move into the library is reported without observers,
/// the opponent forgets the revealed card; the hand owner keeps it (iii).
#[test]
fn reveal_then_unobserved_put_in_library_forgotten_by_opponent() {
    let mut w = KWorld::two();
    for _ in 0..3 {
        w.create(ZoneKind::Library, P1);
    }
    let h = w.create(ZoneKind::Hand, P1);
    let card = w.card(h);
    w.k.reveal_to_all(&w.zones, h).expect("reveal");
    let moved = w.mv(h, KWorld::req_at(ZoneKind::Library, Position::Index(1)));
    w.k.shuffle_library(&w.zones, P1, &[]).expect("shuffle");
    assert!(!w.known_to(moved.new).contains(&P2));
    assert!(!w.can(P2, moved.new));
    assert!(lib_known(&w.view(P2), P1).is_empty());
    assert!(w.can(P1, moved.new));
    assert_eq!(lib_known(&w.view(P1), P1), set(&[card]));
}

/// REQ-CORE-004, CR 701.24a, 701.24b, SRS 2.4 "Shuffle": a shuffle clears every position entry
/// of that library for every viewer, the excluded found card at its recorded index included.
/// Entries for other libraries are kept; KnownTo is never touched (the viewer still identifies
/// the card as a set member of the library view).
#[test]
fn shuffle_excluding_found_card_at_its_index() {
    let (mut w, lib) = lib_world(4, false);
    let other = w.create_bottom(ZoneKind::Library, P2);
    for (i, id) in lib.iter().enumerate() {
        w.k.set_position_known(&w.zones, P1, P1, i, *id)
            .expect("pos");
    }
    w.k.set_position_known(&w.zones, P2, P1, 0, lib[0])
        .expect("pos");
    w.k.set_position_known(&w.zones, P2, P1, 3, lib[3])
        .expect("pos");
    w.k.set_position_known(&w.zones, P1, P2, 0, other)
        .expect("pos");
    let known: Vec<_> = lib.iter().map(|id| w.known_to(*id)).collect();
    let card0 = w.card(lib[0]);
    w.k.shuffle_library(&w.zones, P1, &[lib[0]])
        .expect("shuffle");
    assert!(
        w.positions(P1, P1).is_empty(),
        "P1: excluded entry gone too"
    );
    assert!(
        w.positions(P2, P1).is_empty(),
        "P2: excluded entry gone too"
    );
    assert_eq!(
        w.positions(P1, P2),
        map(&[(0, other)]),
        "other library untouched"
    );
    let after: Vec<_> = lib.iter().map(|id| w.known_to(*id)).collect();
    assert_eq!(known, after, "KnownTo untouched");
    assert!(lib_known(&w.view(P1), P1).contains(&card0), "set member");
}

/// REQ-CORE-004, CR 701.24b, SRS 2.4: an excluded card no longer at its recorded index has no
/// entry after the shuffle, and a stale entry never comes back when the card returns to that
/// index later.
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

/// REQ-CORE-004, CR 701.24a, SRS 2.4: every entry is cleared by a shuffle even if the card
/// happens to stay at the recorded index; excluded or not makes no difference.
#[test]
fn shuffle_clears_non_excluded_even_when_still_at_index() {
    let (mut w, lib) = lib_world(3, false);
    w.k.set_position_known(&w.zones, P2, P1, 2, lib[2])
        .expect("pos");
    w.k.set_position_known(&w.zones, P2, P1, 0, lib[0])
        .expect("pos");
    w.k.shuffle_library(&w.zones, P1, &[lib[0]])
        .expect("shuffle");
    assert!(w.positions(P2, P1).is_empty());
    assert_eq!(w.library(P1)[2], lib[2], "still there physically");
}

/// REQ-CORE-004, CR 701.24b, 401.4: after a shuffle the caller re-establishes the disclosed
/// placement of a found card with `set_position_known`; `position_known` reports exactly that.
#[test]
fn disclosed_placement_after_shuffle_is_reestablished() {
    let (mut w, lib) = lib_world(4, false);
    w.k.set_position_known(&w.zones, P1, P1, 0, lib[0])
        .expect("pos");
    w.k.set_position_known(&w.zones, P1, P1, 2, lib[2])
        .expect("pos");
    w.k.shuffle_library(&w.zones, P1, &[lib[0]])
        .expect("shuffle");
    assert!(w.positions(P1, P1).is_empty());
    w.k.set_position_known(&w.zones, P1, P1, 0, lib[0])
        .expect("pos");
    assert_eq!(w.positions(P1, P1), map(&[(0, lib[0])]));
    assert!(w.positions(P2, P1).is_empty());
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

/// REQ-CORE-004, CR 401.2: `on_library_changed` after a card is put on top outside `on_move`
/// drops the entries that no longer match; drawing that card afterwards does not restore the
/// old entry (the viewers observe the draw: P1 by its own hand, P2 as an explicit observer).
/// Contrast: the same draw with no observers makes P2 forget A (the hidden-source identity rule
/// of the brief: holding position knowledge in the library never makes P2 an observer), while
/// P1, the owner of the destination hand, keeps it.
#[test]
fn library_changed_drops_entries_not_restored_by_a_draw() {
    let build = || -> (KWorld, Vec<ObjectId>, ObjectId) {
        let (mut w, lib) = lib_world(3, false);
        for v in [P1, P2] {
            w.k.set_position_known(&w.zones, v, P1, 0, lib[0])
                .expect("pos");
        }
        let x = w.create(ZoneKind::Library, P1);
        assert_eq!(w.library(P1)[0], x, "created on top, Zones only");
        w.k.on_library_changed(&w.zones, P1).expect("changed");
        for v in [P1, P2] {
            assert!(w.positions(v, P1).is_empty(), "{v:?}");
        }
        (w, lib, x)
    };
    let (mut w, lib, x) = build();
    w.mv_obs(x, MoveRequest::new(ZoneKind::Hand), &[P2]);
    assert_eq!(w.library(P1)[0], lib[0], "A is back at index 0");
    for v in [P1, P2] {
        assert!(w.positions(v, P1).is_empty(), "{v:?}: not restored");
        assert!(w.can(v, lib[0]), "{v:?}: identity knowledge kept");
    }
    // contrast: unobserved draw
    let (mut u, lib, x) = build();
    u.mv_to(x, ZoneKind::Hand);
    assert_eq!(u.library(P1)[0], lib[0]);
    for v in [P1, P2] {
        assert!(u.positions(v, P1).is_empty(), "{v:?}: not restored");
    }
    assert!(u.can(P1, lib[0]), "P1 observed (destination is its hand)");
    assert!(!u.can(P2, lib[0]), "P2 did not observe: A forgotten");
    assert!(!u.known_to(lib[0]).contains(&P2));
}

/// REQ-CORE-004, CR 401.2 (index-disclosure rule of the brief, a design decision): an
/// insertion at a deeper index whose index P2 knows (`on_library_changed_indexed` with [P2])
/// keeps P2's entries above it (smaller indexes) and drops those at or below it in the pile
/// (indexes at or after the insertion); the shorthand `on_library_changed` (index unknown)
/// leaves P2 no entry. Removing the inserted card again follows the caller contract
/// (`send_outside`, then the removal hook): watched by P2 with the index known to P2 the
/// dropped entries do not come back and the kept ones stay; watched by P2 without the index
/// (shorthand) P2 loses every entry; with no observers P2 loses every entry too.
#[test]
fn library_changed_keeps_entries_above_the_insertion() {
    let build = |disclosed: bool| -> (KWorld, Vec<ObjectId>, ObjectId) {
        let (mut w, lib) = lib_world(4, false);
        for (i, id) in lib.iter().enumerate() {
            w.k.set_position_known(&w.zones, P2, P1, i, *id)
                .expect("pos");
        }
        let x = w.create_req(P1, KWorld::req_at(ZoneKind::Library, Position::Index(2)));
        assert_eq!(w.library(P1), vec![lib[0], lib[1], x, lib[2], lib[3]]);
        if disclosed {
            w.library_changed(P1, Some(&[P2]));
            assert_eq!(w.positions(P2, P1), map(&[(0, lib[0]), (1, lib[1])]));
        } else {
            w.k.on_library_changed(&w.zones, P1).expect("changed");
            assert!(w.positions(P2, P1).is_empty(), "shorthand: index unknown");
        }
        for id in &lib {
            assert!(w.known_to(*id).contains(&P2), "set knowledge kept");
        }
        (w, lib, x)
    };
    build(false);
    // removing x, watched by P2 who knows the index: the dropped entries do not come back
    let (mut w, lib, x) = build(true);
    w.send_out_idx(x, &[P2], &[P2]);
    assert_eq!(w.library(P1), lib);
    assert_eq!(w.positions(P2, P1), map(&[(0, lib[0]), (1, lib[1])]));
    // removing x watched by P2, index not disclosed: no entry left
    let (mut v, lib, x) = build(true);
    v.send_out(x, &[P2]);
    assert_eq!(v.library(P1), lib);
    assert!(v.positions(P2, P1).is_empty());
    // removing x unobserved: P2 forgets every entry for that library (hidden-source rule)
    let (mut u, lib, x) = build(true);
    u.send_out(x, &[]);
    assert_eq!(u.library(P1), lib);
    assert!(u.positions(P2, P1).is_empty());
}

/// REQ-CORE-004, CR 401.2: removal (`send_outside` + the removal hook) and insertion
/// (`enter_from_outside` + `on_library_changed[_indexed]`) of library cards outside `on_move`.
/// P1 watches its own removals (passed in `observers`) and knows their index (passed in
/// `index_known_by`); an unobserved removal drops P1's entries for that library too (the owner
/// of a library is not an observer by itself), and so does an observed removal whose index is
/// not disclosed (the shorthand).
#[test]
fn library_changed_after_send_outside_and_enter_from_outside() {
    let (mut w, lib) = lib_world(3, false);
    for (i, id) in lib.iter().enumerate() {
        w.k.set_position_known(&w.zones, P1, P1, i, *id)
            .expect("pos");
    }
    // unobserved removal of the bottom card: P1 is not an observer, every entry goes
    let mut u = w.clone();
    u.send_out(lib[2], &[]);
    assert!(u.positions(P1, P1).is_empty());
    // observed removal of the bottom card, index not disclosed: every entry goes
    let mut o = w.clone();
    o.send_out(lib[2], &[P1]);
    assert!(o.positions(P1, P1).is_empty());
    assert!(
        o.can(P1, lib[0]) && o.can(P1, lib[1]),
        "identity kept by the observer"
    );
    // observed removal of the bottom card at an index P1 knows: the entries above it match
    let (_, card) = w.send_out_idx(lib[2], &[P1], &[P1]);
    assert_eq!(w.positions(P1, P1), map(&[(0, lib[0]), (1, lib[1])]));
    // bring it back on top: everything shifts, nothing matches
    let back = w
        .zones
        .enter_from_outside(
            &mut w.ids,
            P1,
            card,
            OutsidePool::Sideboard,
            MoveRequest::new(ZoneKind::Library),
        )
        .expect("in");
    assert_eq!(w.library(P1), vec![back, lib[0], lib[1]]);
    w.library_changed(P1, Some(&[P1]));
    assert!(w.positions(P1, P1).is_empty());
    // remove it again (observed, index known): lib[0] and lib[1] are back at 0 and 1, but
    // their entries stay dropped
    w.send_out_idx(back, &[P1], &[P1]);
    assert!(w.positions(P1, P1).is_empty());
}

/// REQ-CORE-004, CR 401.2, SRS 2.4: insertions stay position-only. P2 knows the set {A, B} of
/// P1's library and no positions; P1 knows every position. A card entering from outside the
/// game at index 2 (`enter_from_outside` + `on_library_changed_indexed` with [P1], the index
/// known to P1 only) leaves P2's stored KnownTo of A and B and its projected `known_cards`
/// intact (set knowledge is not forgotten by an insertion); P1's entries below the insertion
/// index are kept, those at or after it dropped. With the shorthand `on_library_changed` P1
/// keeps no entry (index-disclosure rule, a design decision) and every KnownTo is the same.
/// An insertion into P1's hand does not touch P2's knowledge of P1's hand cards either.
#[test]
fn insertion_from_outside_keeps_set_knowledge() {
    let (mut w, lib) = lib_world(4, false);
    let (a, b) = (lib[1], lib[3]);
    w.k.add_known(&w.zones, a, P2).expect("add");
    w.k.add_known(&w.zones, b, P2).expect("add");
    for (i, id) in lib.iter().enumerate() {
        w.k.set_position_known(&w.zones, P1, P1, i, *id)
            .expect("pos");
    }
    let h = w.create(ZoneKind::Hand, P1);
    w.k.add_known(&w.zones, h, P2).expect("add");
    let (ca, cb) = (w.card(a), w.card(b));
    let mut s = w.clone();
    let req2 = KWorld::req_at(ZoneKind::Library, Position::Index(2));
    let card = w.fresh_card();
    let x = w.enter_card(P1, card, req2, Some(&[P1]));
    assert_eq!(w.library(P1), vec![lib[0], lib[1], x, lib[2], lib[3]]);
    // contrast: the same insertion through the shorthand
    let sx = s.enter_card(P1, card, req2, None);
    assert_eq!(sx, x);
    assert!(s.positions(P1, P1).is_empty(), "shorthand: no entry kept");
    assert!(s.positions(P2, P1).is_empty());
    for id in s.zones.all_object_ids() {
        assert_eq!(s.known_to(id), w.known_to(id), "only positions differ");
    }
    assert!(w.known_to(a).contains(&P2), "set knowledge of A kept");
    assert!(w.known_to(b).contains(&P2), "set knowledge of B kept");
    assert!(w.can(P2, a) && w.can(P2, b));
    assert_eq!(lib_known(&w.view(P2), P1), [ca, cb].into_iter().collect());
    assert!(w.positions(P2, P1).is_empty());
    assert_eq!(w.positions(P1, P1), map(&[(0, lib[0]), (1, lib[1])]));
    assert!(w.known_to(lib[0]).contains(&P1), "P1 identity kept");
    assert!(
        w.known_to(lib[2]).contains(&P1),
        "P1 identity kept (entry dropped)"
    );
    // an insertion into the hand
    w.enter_fresh(P1, MoveRequest::new(ZoneKind::Hand));
    assert!(w.known_to(h).contains(&P2));
    assert_eq!(
        opp_hand_known(&w.view(P2), P1),
        [w.card(h)].into_iter().collect()
    );
    assert_eq!(lib_known(&w.view(P2), P1), [ca, cb].into_iter().collect());
}

/// REQ-CORE-004, R12: `on_library_changed` for an unknown owner is `UnknownPlayer` and
/// changes nothing; `on_library_changed_indexed` with P2 in `index_known_by` for a library whose
/// entries all match changes nothing; the shorthand drops P2's entries (nobody knows the index,
/// index-disclosure rule of the brief) but no KnownTo.
#[test]
fn library_changed_errors_and_no_op() {
    let (mut w, lib) = lib_world(3, false);
    w.k.set_position_known(&w.zones, P2, P1, 1, lib[1])
        .expect("pos");
    let snapshot = w.k.clone();
    for bad in [NOBODY, pid(0), pid(u64::MAX)] {
        assert_eq!(
            w.k.on_library_changed(&w.zones, bad),
            Err(KnowledgeError::UnknownPlayer(bad))
        );
    }
    assert_eq!(w.k, snapshot);
    w.library_changed(P1, Some(&[P2]));
    w.library_changed(P2, Some(&[P2]));
    assert_eq!(w.positions(P2, P1), map(&[(1, lib[1])]));
    assert_eq!(w.k, snapshot, "nothing to drop");
    // the shorthand on the other library keeps P2's entry for P1's library
    w.k.on_library_changed(&w.zones, P2).expect("changed");
    assert_eq!(w.positions(P2, P1), map(&[(1, lib[1])]));
    // the shorthand on P1's library drops it, but not the KnownTo
    w.k.on_library_changed(&w.zones, P1).expect("changed");
    assert!(
        w.positions(P2, P1).is_empty(),
        "shorthand: index unknown to P2"
    );
    assert_eq!(w.known_to(lib[1]), set(&[P2]));
}

// ---------------------------------------------------------------------------------------------
// index disclosure (PR #174 review round 4, findings F1/F2 and their class): position
// knowledge survives a library change only for the viewers who know the index of the change
// (the brief's index-disclosure rule, a design decision; no CR). Twin worlds differ only in
// the hidden index of the change.
// ---------------------------------------------------------------------------------------------

const CA: u64 = 10;
const CB: u64 = 11;
const CX: u64 = 20;
/// The inserted card.
const CN: u64 = 30;

/// P1's library, top first: X, A, B (fixed synthetic cards); P2 knows A at index 1 and B as a
/// member of the set; P3 (three players) knows nothing.
fn index_world(players: &[PlayerId]) -> KWorld {
    let mut w = KWorld::with_players(players);
    let lib = w.build_library(P1, &[CX, CA, CB]);
    w.k.set_position_known(&w.zones, P2, P1, 1, lib[1])
        .expect("pos");
    w.k.add_known(&w.zones, lib[2], P2).expect("add");
    assert_eq!(w.positions(P2, P1), map(&[(1, w.id_of(CA))]));
    w
}

/// Inserts the card N at `index` of P1's library, by `Zones::create_object` (`via_enter`
/// false) or `Zones::enter_from_outside` (true), then informs the knowledge with the shorthand
/// (`ikb` None) or `on_library_changed_indexed` (`ikb` Some).
fn insert_n(w: &mut KWorld, index: usize, via_enter: bool, ikb: Option<&[PlayerId]>) -> ObjectId {
    let req = KWorld::req_at(ZoneKind::Library, Position::Index(index));
    if via_enter {
        w.enter_card(P1, cid(CN), req, ikb)
    } else {
        w.insert_card(P1, cid(CN), req, ikb)
    }
}

/// REQ-CORE-004, CR 401.2, SRS 2.4 (PR #174 review finding F2; index-disclosure rule of the
/// brief): P2 knows A at index 1. World 1 inserts N at index 0 (above A), world 2 at index 2
/// (below A), by `create_object` and by `enter_from_outside`, then the shorthand
/// `on_library_changed`: P2 cannot tell where N went, so `position_known(P2, P1)` is equal
/// (empty) in both worlds, every player's whole view is equal, and P2's KnownTo of A and B is
/// unchanged (set knowledge survives an insertion). P3 (no knowledge) sees equal views.
/// Control: `on_library_changed_indexed(.., [P2])`: index 0 drops A's entry (A shifted to 2),
/// index 2 keeps A@1, so a viewer who knows the index CAN tell the worlds apart.
#[test]
fn insertion_twin_above_and_below() {
    for players in [vec![P1, P2], vec![P1, P2, P3]] {
        for via_enter in [false, true] {
            let ctx = format!("{players:?} enter {via_enter}");
            // shorthand
            let mut w1 = index_world(&players);
            let mut w2 = index_world(&players);
            assert_twins_equal_for(&w1, &w2, &players, "precondition");
            let n1 = insert_n(&mut w1, 0, via_enter, None);
            let n2 = insert_n(&mut w2, 2, via_enter, None);
            assert_eq!(n1, n2, "{ctx}: same new id");
            let a = w1.id_of(CA);
            assert_eq!(w1.library(P1)[0], n1);
            assert_eq!(w2.library(P1)[2], n2);
            assert_eq!(w2.library(P1)[1], a, "A still at 1 in world 2");
            assert_twins_equal_for(&w1, &w2, &players, &ctx);
            assert!(w1.positions(P2, P1).is_empty(), "{ctx}");
            for w in [&w1, &w2] {
                assert!(w.known_to(a).contains(&P2), "{ctx}: A set knowledge kept");
                assert!(w.known_to(w.id_of(CB)).contains(&P2), "{ctx}: B kept");
                assert_eq!(
                    lib_known(&w.view(P2), P1),
                    set(&[cid(CA), cid(CB)]),
                    "{ctx}"
                );
            }
            // control: P2 knows the index
            let mut c1 = index_world(&players);
            let mut c2 = index_world(&players);
            insert_n(&mut c1, 0, via_enter, Some(&[P2]));
            insert_n(&mut c2, 2, via_enter, Some(&[P2]));
            assert!(c1.positions(P2, P1).is_empty(), "{ctx}: A shifted");
            assert_eq!(c2.positions(P2, P1), map(&[(1, a)]), "{ctx}: A kept");
            assert_ne!(c1.positions(P2, P1), c2.positions(P2, P1));
            let others: Vec<PlayerId> = players.iter().copied().filter(|p| *p != P2).collect();
            assert_twins_equal_for(&c1, &c2, &others, &ctx);
            assert_eq!(
                c1.view(P2),
                c2.view(P2),
                "{ctx}: positions are never projected"
            );
        }
    }
}

/// P1's library, top first: A, X, B; P2 knows A at index 0 and B as a member of the set.
fn shuffle_world(players: &[PlayerId]) -> KWorld {
    let mut w = KWorld::with_players(players);
    let lib = w.build_library(P1, &[CA, CX, CB]);
    w.k.set_position_known(&w.zones, P2, P1, 0, lib[0])
        .expect("pos");
    w.k.add_known(&w.zones, lib[2], P2).expect("add");
    w
}

/// REQ-CORE-004, CR 701.24a, 701.24b, SRS 2.4: P2 holds an entry for A. World 1 shuffles
/// excluding A, world 2 excluding X: `project` and `position_known` are equal (empty) for every
/// player (nobody is exempt), so the excluded card cannot be told apart. KnownTo is never
/// touched by a shuffle.
#[test]
fn shuffle_excluded_twin() {
    for players in [vec![P1, P2], vec![P1, P2, P3]] {
        let base = shuffle_world(&players);
        let (a, x) = (base.id_of(CA), base.id_of(CX));
        let known: Vec<_> = base
            .zones
            .all_object_ids()
            .into_iter()
            .map(|id| base.known_to(id))
            .collect();
        let known_now = |w: &KWorld| -> Vec<_> {
            w.zones
                .all_object_ids()
                .into_iter()
                .map(|id| w.known_to(id))
                .collect()
        };
        let mut w1 = base.clone();
        let mut w2 = base.clone();
        w1.k.shuffle_library(&w1.zones, P1, &[a]).expect("shuffle");
        w2.k.shuffle_library(&w2.zones, P1, &[x]).expect("shuffle");
        assert_twins_equal_for(&w1, &w2, &players, "excluded A vs excluded X");
        for p in &players {
            assert!(w1.positions(*p, P1).is_empty(), "{p:?} world 1");
            assert!(w2.positions(*p, P1).is_empty(), "{p:?} world 2");
            assert_eq!(w1.view(*p), w2.view(*p), "{p:?} view");
        }
        assert_eq!(known_now(&w1), known);
        assert_eq!(known_now(&w2), known);
    }
}

/// REQ-CORE-004, R12: errors of `on_library_changed_indexed` (UnknownPlayer(owner) first, then
/// the first unknown `index_known_by` player) and of `shuffle_library` (UnknownPlayer(owner),
/// then NotInLibrary for the first bad excluded id); rejected calls change nothing. The
/// shorthand `on_library_changed` equals the indexed method with an empty disclosure.
#[test]
fn index_disclosure_errors_and_shorthands() {
    let mut w = index_world(&[P1, P2, P3]);
    let lib = w.library(P1);
    let hand = w.create(ZoneKind::Hand, P1);
    let snapshot = w.k.clone();
    let lc = |w: &mut KWorld, owner: PlayerId, ikb: &[PlayerId]| {
        w.k.on_library_changed_indexed(&w.zones, owner, ikb)
    };
    assert_eq!(
        lc(&mut w, NOBODY, &[pid(77)]),
        Err(KnowledgeError::UnknownPlayer(NOBODY)),
        "owner first"
    );
    assert_eq!(
        lc(&mut w, P1, &[P2, pid(77), NOBODY]),
        Err(KnowledgeError::UnknownPlayer(pid(77)))
    );
    assert_eq!(
        lc(&mut w, P1, &[pid(u64::MAX)]),
        Err(KnowledgeError::UnknownPlayer(pid(u64::MAX)))
    );
    assert_eq!(w.k, snapshot);
    let sh =
        |w: &mut KWorld, owner: PlayerId, ex: &[ObjectId]| w.k.shuffle_library(&w.zones, owner, ex);
    assert_eq!(
        sh(&mut w, NOBODY, &[oid(9_999)]),
        Err(KnowledgeError::UnknownPlayer(NOBODY)),
        "owner first"
    );
    assert_eq!(
        sh(&mut w, P1, &[lib[0], hand, oid(9_999)]),
        Err(KnowledgeError::NotInLibrary {
            owner: P1,
            object: hand
        }),
        "the first bad excluded id"
    );
    assert_eq!(w.k, snapshot, "rejected calls change nothing");
    assert_eq!(w.positions(P2, P1), map(&[(1, lib[1])]));
    // shorthands
    let (mut a, mut b) = (w.clone(), w.clone());
    a.k.on_library_changed(&a.zones, P1).expect("lc");
    b.k.on_library_changed_indexed(&b.zones, P1, &[])
        .expect("lc");
    assert_eq!(a.k, b.k);
    assert_ne!(a.k, snapshot, "the shorthand dropped P2's entry");
    let mut c = w.clone();
    assert_eq!(
        sh(&mut c, P1, &[lib[1]]),
        Ok(()),
        "an excluded id in the library"
    );
    assert!(c.positions(P2, P1).is_empty());
}
