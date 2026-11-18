// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the zones module: REQ-ZONE-001, REQ-CORE-002, REQ-NFR-006 (canonical
// encoding). Synthetic ids only.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::encode_to_vec;
use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};
use fineprint_base::prng::Pcg64Dxsm;

use super::*;

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn c(n: u64) -> CardId {
    CardId::new(n)
}

fn two_players() -> Zones {
    Zones::new(&[p(1), p(2)], ZonesConfig::default())
}

fn req(to: ZoneKind) -> MoveRequest {
    MoveRequest::new(to)
}

fn at(to: ZoneKind, position: Position) -> MoveRequest {
    MoveRequest {
        position,
        ..MoveRequest::new(to)
    }
}

/// Creates cards `first..first+n` in `owner`'s library, each on the bottom, so the library is
/// top-first in creation order.
fn fill_library(
    z: &mut Zones,
    ids: &mut IdGenerator<ObjectId>,
    owner: PlayerId,
    first: u64,
    n: u64,
) -> Vec<ObjectId> {
    (first..first + n)
        .map(|k| {
            z.create_object(ids, c(k), owner, at(ZoneKind::Library, Position::Bottom))
                .unwrap()
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Zone kinds and identifiers
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001 items 1,2,4,5,6,7; CR 400.1, CR 400.2, CR 400.5
#[test]
fn zone_kind_predicates_and_order() {
    use ZoneKind::*;
    assert_eq!(
        ZoneKind::ALL,
        [Library, Hand, Battlefield, Graveyard, Stack, Exile, Command]
    );
    let per_player: Vec<_> = ZoneKind::ALL.iter().filter(|k| k.is_per_player()).collect();
    assert_eq!(per_player, [&Library, &Hand, &Graveyard]);
    let hidden: Vec<_> = ZoneKind::ALL.iter().filter(|k| !k.is_public()).collect();
    assert_eq!(hidden, [&Library, &Hand]);
    let ordered: Vec<_> = ZoneKind::ALL.iter().filter(|k| k.is_ordered()).collect();
    assert_eq!(ordered, [&Library, &Graveyard, &Stack]);
}

// REQ-ZONE-001; CR 400.1
#[test]
fn zone_id_kind_owner_and_of() {
    for kind in ZoneKind::ALL {
        let z = ZoneId::of(kind, p(7));
        assert_eq!(z.kind(), kind);
        if kind.is_per_player() {
            assert_eq!(z.owner(), Some(p(7)));
        } else {
            assert_eq!(z.owner(), None);
            assert_eq!(z, ZoneId::of(kind, p(1)));
        }
    }
}

// REQ-ZONE-001
#[test]
fn defaults() {
    let r = MoveRequest::new(ZoneKind::Exile);
    assert_eq!(r.to, ZoneKind::Exile);
    assert_eq!(r.position, Position::Top);
    assert!(!r.face_down);
    assert_eq!(r.controller, None);
    assert!(!ZonesConfig::default().free_graveyard_reorder);
}

// REQ-ZONE-001
#[test]
fn new_ignores_duplicates_and_starts_empty() {
    let cfg = ZonesConfig {
        free_graveyard_reorder: true,
    };
    let z = Zones::new(&[p(3), p(1), p(3), p(2)], cfg);
    assert_eq!(z.players(), vec![p(1), p(2), p(3)]);
    assert_eq!(z.config(), cfg);
    for player in z.players() {
        for kind in ZoneKind::ALL {
            let zone = ZoneId::of(kind, player);
            assert!(z.is_empty(zone));
            assert_eq!(z.len(zone), 0);
            assert!(z.contents(zone).is_empty());
        }
        assert!(z.outside_cards(player, OutsidePool::Sideboard).is_empty());
    }
    assert!(z.all_object_ids().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001, REQ-CORE-002
#[test]
fn error_display_and_error_trait() {
    let all = [
        ZoneError::UnknownPlayer(p(9)),
        ZoneError::UnknownObject(ObjectId::new(4)),
        ZoneError::CardAlreadyPresent(c(5)),
        ZoneError::IdExhausted,
        ZoneError::ControllerNotApplicable,
        ZoneError::ReorderNotAllowed,
        ZoneError::InvalidOrder,
        ZoneError::NoSuchOutsideCard(c(6)),
    ];
    let texts: BTreeSet<String> = all.iter().map(ToString::to_string).collect();
    assert_eq!(texts.len(), all.len());
    assert!(all[0].to_string().contains('9'));
    assert!(all[1].to_string().contains('4'));
    assert!(all[2].to_string().contains('5'));
    assert!(all[7].to_string().contains('6'));
    let e: &dyn std::error::Error = &all[3];
    assert!(e.source().is_none());
}

/// Asserts `f` fails with `want` and leaves both the container and the generator unchanged.
fn assert_atomic_err<T: std::fmt::Debug>(
    z: &mut Zones,
    ids: &mut IdGenerator<ObjectId>,
    want: ZoneError,
    f: impl FnOnce(&mut Zones, &mut IdGenerator<ObjectId>) -> Result<T, ZoneError>,
) {
    let z0 = z.clone();
    let ids0 = ids.clone();
    let b0 = encode_to_vec(z);
    assert_eq!(f(z, ids).unwrap_err(), want);
    assert_eq!(*z, z0);
    assert_eq!(*ids, ids0);
    assert_eq!(encode_to_vec(z), b0);
}

// REQ-ZONE-001, REQ-CORE-002; CR 400.7: failed calls change nothing
#[test]
fn errors_are_atomic() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let lib = fill_library(&mut z, &mut ids, p(1), 1, 3);
    z.add_outside_card(p(2), OutsidePool::Sideboard, c(50))
        .unwrap();
    let bf = z
        .create_object(&mut ids, c(60), p(1), req(ZoneKind::Battlefield))
        .unwrap();

    // create_object
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::UnknownPlayer(p(9)),
        |z, ids| z.create_object(ids, c(10), p(9), req(ZoneKind::Hand)),
    );
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::CardAlreadyPresent(c(1)),
        |z, ids| z.create_object(ids, c(1), p(1), req(ZoneKind::Hand)),
    );
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::CardAlreadyPresent(c(50)),
        |z, ids| z.create_object(ids, c(50), p(2), req(ZoneKind::Hand)),
    );
    let ctl = |to| MoveRequest {
        controller: Some(p(2)),
        ..MoveRequest::new(to)
    };
    for kind in [
        ZoneKind::Library,
        ZoneKind::Hand,
        ZoneKind::Graveyard,
        ZoneKind::Exile,
        ZoneKind::Command,
    ] {
        assert_atomic_err(
            &mut z,
            &mut ids,
            ZoneError::ControllerNotApplicable,
            |z, ids| z.create_object(ids, c(10), p(1), ctl(kind)),
        );
        assert_atomic_err(
            &mut z,
            &mut ids,
            ZoneError::ControllerNotApplicable,
            |z, ids| z.move_object(ids, lib[0], ctl(kind)),
        );
    }
    let bad_ctl = MoveRequest {
        controller: Some(p(9)),
        ..MoveRequest::new(ZoneKind::Battlefield)
    };
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::UnknownPlayer(p(9)),
        |z, ids| z.create_object(ids, c(10), p(1), bad_ctl),
    );

    // move_object
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::UnknownObject(ObjectId::new(999)),
        |z, ids| z.move_object(ids, ObjectId::new(999), req(ZoneKind::Exile)),
    );
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::UnknownPlayer(p(9)),
        |z, ids| z.move_object(ids, lib[1], bad_ctl),
    );

    // IdExhausted, with an exhausted generator
    let mut spent: IdGenerator<ObjectId> = IdGenerator::from_last_issued(u64::MAX);
    assert_atomic_err(&mut z, &mut spent, ZoneError::IdExhausted, |z, ids| {
        z.move_object(ids, lib[1], req(ZoneKind::Exile))
    });
    assert_atomic_err(&mut z, &mut spent, ZoneError::IdExhausted, |z, ids| {
        z.create_object(ids, c(10), p(1), req(ZoneKind::Exile))
    });
    assert_atomic_err(&mut z, &mut spent, ZoneError::IdExhausted, |z, ids| {
        z.enter_from_outside(
            ids,
            p(2),
            c(50),
            OutsidePool::Sideboard,
            req(ZoneKind::Hand),
        )
    });

    // outside the game
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::UnknownPlayer(p(9)),
        |z, ids| {
            z.enter_from_outside(
                ids,
                p(9),
                c(50),
                OutsidePool::Sideboard,
                req(ZoneKind::Hand),
            )
        },
    );
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::NoSuchOutsideCard(c(50)),
        |z, ids| {
            z.enter_from_outside(
                ids,
                p(1),
                c(50),
                OutsidePool::Sideboard,
                req(ZoneKind::Hand),
            )
        },
    );
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::NoSuchOutsideCard(c(50)),
        |z, ids| z.enter_from_outside(ids, p(2), c(50), OutsidePool::Lesson, req(ZoneKind::Hand)),
    );
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::NoSuchOutsideCard(c(1)),
        |z, ids| z.enter_from_outside(ids, p(1), c(1), OutsidePool::Sideboard, req(ZoneKind::Hand)),
    );
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::ControllerNotApplicable,
        |z, ids| {
            z.enter_from_outside(
                ids,
                p(2),
                c(50),
                OutsidePool::Sideboard,
                ctl(ZoneKind::Hand),
            )
        },
    );
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::UnknownObject(ObjectId::new(999)),
        |z, _| z.send_outside(ObjectId::new(999), OutsidePool::Sideboard),
    );
    assert_atomic_err(&mut z, &mut ids, ZoneError::UnknownPlayer(p(9)), |z, _| {
        z.add_outside_card(p(9), OutsidePool::Sideboard, c(70))
    });
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::CardAlreadyPresent(c(50)),
        |z, _| z.add_outside_card(p(1), OutsidePool::Lesson, c(50)),
    );
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::CardAlreadyPresent(c(2)),
        |z, _| z.add_outside_card(p(1), OutsidePool::Sideboard, c(2)),
    );

    // set_controller
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::UnknownObject(ObjectId::new(999)),
        |z, _| z.set_controller(ObjectId::new(999), p(2)),
    );
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::ControllerNotApplicable,
        |z, _| z.set_controller(lib[0], p(2)),
    );
    assert_atomic_err(&mut z, &mut ids, ZoneError::UnknownPlayer(p(9)), |z, _| {
        z.set_controller(bf, p(9))
    });

    // reorder_graveyard (flag off)
    assert_atomic_err(&mut z, &mut ids, ZoneError::ReorderNotAllowed, |z, _| {
        z.reorder_graveyard(p(1), Vec::new())
    });
}

// ---------------------------------------------------------------------------------------------
// The zone-move primitive
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002; CR 400.7: a move issues exactly one new id and forgets the old one
#[test]
fn move_creates_new_object_and_forgets_old() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let old = z
        .create_object(&mut ids, c(1), p(1), req(ZoneKind::Hand))
        .unwrap();
    let before = ids.last_issued();
    let m = z
        .move_object(&mut ids, old, req(ZoneKind::Battlefield))
        .unwrap();
    assert_eq!(ids.last_issued(), before + 1);
    assert_eq!(m.old, old);
    assert_eq!(m.new.get(), before + 1);
    assert!(m.new > old);
    assert_eq!(m.from, ZoneId::Hand(p(1)));
    assert_eq!(m.to, ZoneId::Battlefield);
    assert!(z.object(old).is_none());
    assert!(!z.all_object_ids().contains(&old));
    for player in z.players() {
        for kind in ZoneKind::ALL {
            assert!(!z.contents(ZoneId::of(kind, player)).contains(&old));
        }
    }
    let r = z.object(m.new).unwrap();
    assert_eq!(r.card, c(1));
    assert_eq!(r.owner, p(1));
    assert_eq!(r.controller, p(1));
    assert_eq!(r.zone, ZoneId::Battlefield);
    assert_eq!(r.id, m.new);
    assert_eq!(z.object_of_card(c(1)), Some(m.new));
    assert_eq!(z.all_object_ids(), vec![m.new]);
    assert_eq!(z.object_of_card(c(2)), None);
}

// REQ-CORE-002; CR 400.8, CR 406.7, CR 400.10: a same-zone move is a new object
#[test]
fn same_zone_move_creates_new_id() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    for kind in ZoneKind::ALL {
        let old = z
            .create_object(&mut ids, c(100 + kind as u64), p(2), req(kind))
            .unwrap();
        let m = z.move_object(&mut ids, old, req(kind)).unwrap();
        assert_ne!(m.new, old);
        assert_eq!(m.from, m.to);
        assert_eq!(m.to, ZoneId::of(kind, p(2)));
        assert!(z.object(old).is_none());
        assert_eq!(z.contents(m.to), vec![m.new]);
        z.send_outside(m.new, OutsidePool::Sideboard).unwrap();
    }
}

// REQ-ZONE-001; CR 400.3: per-player destinations are the owner's zones
#[test]
fn per_player_destination_is_owners_zone() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let o = z
        .create_object(
            &mut ids,
            c(1),
            p(2),
            MoveRequest {
                controller: Some(p(1)),
                ..req(ZoneKind::Battlefield)
            },
        )
        .unwrap();
    assert_eq!(z.object(o).unwrap().controller, p(1));
    for kind in [ZoneKind::Graveyard, ZoneKind::Hand, ZoneKind::Library] {
        let m = z.move_object(&mut ids, z.object_of_card(c(1)).unwrap(), req(kind));
        let m = m.unwrap();
        assert_eq!(m.to, ZoneId::of(kind, p(2)));
        assert!(z.is_empty(ZoneId::of(kind, p(1))));
        let r = z.object(m.new).unwrap();
        assert_eq!(r.owner, p(2));
        assert_eq!(r.controller, p(2));
    }
}

// REQ-ZONE-001 items 1,4,5; CR 400.5, CR 401.7: positions in ordered zones
#[test]
fn positions_in_ordered_zones() {
    for kind in [ZoneKind::Library, ZoneKind::Graveyard, ZoneKind::Stack] {
        let mut z = two_players();
        let mut ids = IdGenerator::new();
        let zone = ZoneId::of(kind, p(1));
        let a = z.create_object(&mut ids, c(1), p(1), req(kind)).unwrap();
        let b = z.create_object(&mut ids, c(2), p(1), req(kind)).unwrap();
        // CR 405.2 / CR 404.1: default (Top) puts on top.
        assert_eq!(z.contents(zone), vec![b, a]);
        let d = z
            .create_object(&mut ids, c(3), p(1), at(kind, Position::Bottom))
            .unwrap();
        assert_eq!(z.contents(zone), vec![b, a, d]);
        let e = z
            .create_object(&mut ids, c(4), p(1), at(kind, Position::Index(1)))
            .unwrap();
        assert_eq!(z.contents(zone), vec![b, e, a, d]);
        // CR 401.7: an index past the bottom means the bottom.
        let f = z
            .create_object(&mut ids, c(5), p(1), at(kind, Position::Index(99)))
            .unwrap();
        assert_eq!(z.contents(zone), vec![b, e, a, d, f]);
        let g = z
            .create_object(&mut ids, c(6), p(1), at(kind, Position::Index(5)))
            .unwrap();
        assert_eq!(z.contents(zone), vec![b, e, a, d, f, g]);
        // Moving one out keeps the rest in order (CR 400.5).
        let m = z.move_object(&mut ids, a, req(ZoneKind::Exile)).unwrap();
        assert_eq!(z.contents(zone), vec![b, e, d, f, g]);
        // Moving back in at Index(2).
        let m2 = z
            .move_object(&mut ids, m.new, at(kind, Position::Index(2)))
            .unwrap();
        assert_eq!(z.contents(zone), vec![b, e, m2.new, d, f, g]);
        // Same-zone move to the bottom.
        let m3 = z
            .move_object(&mut ids, b, at(kind, Position::Bottom))
            .unwrap();
        assert_eq!(z.contents(zone), vec![e, m2.new, d, f, g, m3.new]);
        assert_eq!(z.len(zone), 6);
        assert!(!z.is_empty(zone));
    }
}

// REQ-ZONE-001 items 2,6,7: unordered zones iterate ascending and ignore position
#[test]
fn unordered_zones_ascending() {
    for kind in [
        ZoneKind::Hand,
        ZoneKind::Battlefield,
        ZoneKind::Exile,
        ZoneKind::Command,
    ] {
        let mut z = two_players();
        let mut ids = IdGenerator::new();
        let zone = ZoneId::of(kind, p(2));
        let a = z.create_object(&mut ids, c(1), p(2), req(kind)).unwrap();
        let b = z
            .create_object(&mut ids, c(2), p(2), at(kind, Position::Bottom))
            .unwrap();
        let d = z
            .create_object(&mut ids, c(3), p(2), at(kind, Position::Index(0)))
            .unwrap();
        assert_eq!(z.contents(zone), vec![a, b, d]);
        assert_eq!(z.len(zone), 3);
    }
}

// REQ-ZONE-001 items 1,2,4,5,6; CR 400.2, CR 406.3: face-down flag and visibility
#[test]
fn face_down_and_public_info() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let down = |to| MoveRequest {
        face_down: true,
        ..MoveRequest::new(to)
    };
    for kind in ZoneKind::ALL {
        let up = z.create_object(&mut ids, c(10 + kind as u64), p(1), req(kind));
        let up = up.unwrap();
        assert_eq!(z.is_public_info(up), Some(kind.is_public()));
        assert!(!z.object(up).unwrap().face_down);
        let m = z.move_object(&mut ids, up, down(kind)).unwrap();
        assert!(z.object(m.new).unwrap().face_down);
        assert_eq!(z.is_public_info(m.new), Some(false));
        // The flag never carries over: the next move sets it from the request.
        let m2 = z.move_object(&mut ids, m.new, req(kind)).unwrap();
        assert!(!z.object(m2.new).unwrap().face_down);
        assert_eq!(z.is_public_info(m2.new), Some(kind.is_public()));
    }
    assert_eq!(z.is_public_info(ObjectId::new(9999)), None);
}

// REQ-ZONE-001; owner never changes, controller only on the battlefield and the stack
#[test]
fn controller_bookkeeping() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let s = z
        .create_object(
            &mut ids,
            c(1),
            p(1),
            MoveRequest {
                controller: Some(p(2)),
                ..req(ZoneKind::Stack)
            },
        )
        .unwrap();
    assert_eq!(z.object(s).unwrap().controller, p(2));
    z.set_controller(s, p(1)).unwrap();
    assert_eq!(z.object(s).unwrap().controller, p(1));
    let m = z
        .move_object(&mut ids, s, req(ZoneKind::Battlefield))
        .unwrap();
    assert_eq!(z.object(m.new).unwrap().controller, p(1));
    z.set_controller(m.new, p(2)).unwrap();
    assert_eq!(z.object(m.new).unwrap().controller, p(2));
    assert_eq!(z.object(m.new).unwrap().owner, p(1));
    let m2 = z
        .move_object(&mut ids, m.new, req(ZoneKind::Exile))
        .unwrap();
    let r = z.object(m2.new).unwrap();
    assert_eq!((r.owner, r.controller), (p(1), p(1)));
    assert_eq!(
        z.set_controller(m2.new, p(2)),
        Err(ZoneError::ControllerNotApplicable)
    );
}

// ---------------------------------------------------------------------------------------------
// Outside the game
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001 item 9; CR 400.11, CR 400.7: outside the game holds cards, not objects
#[test]
fn outside_round_trip() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    z.add_outside_card(p(1), OutsidePool::Sideboard, c(30))
        .unwrap();
    z.add_outside_card(p(1), OutsidePool::Sideboard, c(20))
        .unwrap();
    z.add_outside_card(p(1), OutsidePool::Spellbook, c(40))
        .unwrap();
    assert_eq!(
        z.outside_cards(p(1), OutsidePool::Sideboard),
        vec![c(20), c(30)]
    );
    assert_eq!(z.outside_cards(p(1), OutsidePool::Spellbook), vec![c(40)]);
    assert!(z.outside_cards(p(2), OutsidePool::Sideboard).is_empty());
    assert!(z.outside_cards(p(9), OutsidePool::Sideboard).is_empty());
    assert!(z.all_object_ids().is_empty());
    assert_eq!(z.object_of_card(c(20)), None);

    let o = z
        .enter_from_outside(
            &mut ids,
            p(1),
            c(20),
            OutsidePool::Sideboard,
            req(ZoneKind::Hand),
        )
        .unwrap();
    assert_eq!(z.outside_cards(p(1), OutsidePool::Sideboard), vec![c(30)]);
    assert_eq!(z.object_of_card(c(20)), Some(o));
    assert_eq!(z.contents(ZoneId::Hand(p(1))), vec![o]);

    let card = z.send_outside(o, OutsidePool::Lesson).unwrap();
    assert_eq!(card, c(20));
    assert!(z.object(o).is_none());
    assert!(z.all_object_ids().is_empty());
    assert_eq!(z.outside_cards(p(1), OutsidePool::Lesson), vec![c(20)]);

    let o2 = z
        .enter_from_outside(
            &mut ids,
            p(1),
            c(20),
            OutsidePool::Lesson,
            MoveRequest {
                controller: Some(p(2)),
                ..req(ZoneKind::Battlefield)
            },
        )
        .unwrap();
    assert!(o2 > o);
    assert_eq!(z.object(o2).unwrap().controller, p(2));
    assert!(z.outside_cards(p(1), OutsidePool::Lesson).is_empty());

    // An owner's card sent outside goes to the owner's pool even if another player controls it.
    z.send_outside(o2, OutsidePool::Companion).unwrap();
    assert_eq!(z.outside_cards(p(1), OutsidePool::Companion), vec![c(20)]);
    assert!(z.outside_cards(p(2), OutsidePool::Companion).is_empty());

    // Emptied pools leave no trace: entering from a pool equals direct creation.
    let mut a2 = two_players();
    let mut b2 = two_players();
    let mut g = IdGenerator::new();
    a2.create_object(&mut g, c(9), p(2), req(ZoneKind::Exile))
        .unwrap();
    b2.add_outside_card(p(2), OutsidePool::Sideboard, c(9))
        .unwrap();
    b2.enter_from_outside(
        &mut IdGenerator::new(),
        p(2),
        c(9),
        OutsidePool::Sideboard,
        req(ZoneKind::Exile),
    )
    .unwrap();
    assert_eq!(a2, b2);
    assert_eq!(encode_to_vec(&a2), encode_to_vec(&b2));
}

// ---------------------------------------------------------------------------------------------
// Graveyard reordering
// ---------------------------------------------------------------------------------------------

// REQ-ZONE-001 item 4; CR 400.5, CR 404.2
#[test]
fn reorder_graveyard_flag() {
    let mut off = two_players();
    let mut ids = IdGenerator::new();
    let a = off
        .create_object(&mut ids, c(1), p(1), req(ZoneKind::Graveyard))
        .unwrap();
    let b = off
        .create_object(&mut ids, c(2), p(1), req(ZoneKind::Graveyard))
        .unwrap();
    // Off: refused even for the unchanged order.
    assert_eq!(
        off.reorder_graveyard(p(1), vec![b, a]),
        Err(ZoneError::ReorderNotAllowed)
    );
    assert_eq!(
        off.reorder_graveyard(p(9), vec![]),
        Err(ZoneError::ReorderNotAllowed)
    );

    let mut on = Zones::new(
        &[p(1), p(2)],
        ZonesConfig {
            free_graveyard_reorder: true,
        },
    );
    let mut ids = IdGenerator::new();
    let a = on
        .create_object(&mut ids, c(1), p(1), req(ZoneKind::Graveyard))
        .unwrap();
    let b = on
        .create_object(&mut ids, c(2), p(1), req(ZoneKind::Graveyard))
        .unwrap();
    let d = on
        .create_object(&mut ids, c(3), p(1), req(ZoneKind::Graveyard))
        .unwrap();
    assert_eq!(on.contents(ZoneId::Graveyard(p(1))), vec![d, b, a]);
    let ids0 = ids.clone();
    for bad in [
        vec![a, b],
        vec![a, b, d, d],
        vec![a, b, b],
        vec![a, b, ObjectId::new(99)],
    ] {
        let before = on.clone();
        assert_eq!(
            on.reorder_graveyard(p(1), bad),
            Err(ZoneError::InvalidOrder)
        );
        assert_eq!(on, before);
    }
    assert_eq!(
        on.reorder_graveyard(p(9), vec![]),
        Err(ZoneError::UnknownPlayer(p(9)))
    );
    on.reorder_graveyard(p(1), vec![a, d, b]).unwrap();
    assert_eq!(on.contents(ZoneId::Graveyard(p(1))), vec![a, d, b]);
    assert_eq!(on.all_object_ids(), vec![a, b, d]);
    assert_eq!(ids, ids0);
    on.reorder_graveyard(p(2), vec![]).unwrap();
}

// ---------------------------------------------------------------------------------------------
// Snapshots and canonical encoding
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002: a clone is an independent snapshot
#[test]
fn clone_is_independent_snapshot() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let lib = fill_library(&mut z, &mut ids, p(1), 1, 5);
    z.add_outside_card(p(2), OutsidePool::Sideboard, c(90))
        .unwrap();
    let saved = z.clone();
    let bytes = encode_to_vec(&z);
    let mut copy = z.clone();
    assert_eq!(copy, z);
    copy.move_object(&mut ids, lib[2], req(ZoneKind::Graveyard))
        .unwrap();
    copy.send_outside(lib[0], OutsidePool::Sideboard).unwrap();
    copy.enter_from_outside(
        &mut ids,
        p(2),
        c(90),
        OutsidePool::Sideboard,
        req(ZoneKind::Stack),
    )
    .unwrap();
    assert_ne!(copy, z);
    assert_eq!(z, saved);
    assert_eq!(encode_to_vec(&z), bytes);
    assert_eq!(z.contents(ZoneId::Library(p(1))), lib);
}

// REQ-NFR-006, REQ-CORE-002: equal values encode equal bytes; any difference changes them
#[test]
fn canonical_encoding_distinguishes() {
    let build = |order_swap: bool, face_down: bool, controller: Option<PlayerId>| {
        let mut z = two_players();
        let mut ids = IdGenerator::new();
        let (x, y) = if order_swap { (2, 1) } else { (1, 2) };
        z.create_object(&mut ids, c(x), p(1), req(ZoneKind::Library))
            .unwrap();
        z.create_object(&mut ids, c(y), p(1), req(ZoneKind::Library))
            .unwrap();
        z.create_object(
            &mut ids,
            c(3),
            p(2),
            MoveRequest {
                face_down,
                controller,
                ..req(ZoneKind::Battlefield)
            },
        )
        .unwrap();
        z
    };
    let base = build(false, false, None);
    assert_eq!(
        encode_to_vec(&base),
        encode_to_vec(&build(false, false, None))
    );
    let variants = [
        build(true, false, None),
        build(false, true, None),
        build(false, false, Some(p(1))),
        Zones::new(&[p(1), p(2)], ZonesConfig::default()),
        Zones::new(&[p(1)], ZonesConfig::default()),
        Zones::new(
            &[p(1), p(2)],
            ZonesConfig {
                free_graveyard_reorder: true,
            },
        ),
    ];
    let mut seen = BTreeSet::new();
    seen.insert(encode_to_vec(&base));
    for v in &variants {
        assert!(
            seen.insert(encode_to_vec(v)),
            "duplicate encoding for {v:?}"
        );
    }
    // The same objects in different zones encode differently.
    let mut per_zone = BTreeSet::new();
    for kind in ZoneKind::ALL {
        let mut z = two_players();
        z.create_object(&mut IdGenerator::new(), c(1), p(1), req(kind))
            .unwrap();
        assert!(per_zone.insert(encode_to_vec(&z)));
    }
    // Pools are distinguished.
    let mut pools = BTreeSet::new();
    for pool in [
        OutsidePool::Sideboard,
        OutsidePool::Companion,
        OutsidePool::Lesson,
        OutsidePool::AttractionDeck,
        OutsidePool::ContraptionDeck,
        OutsidePool::Spellbook,
    ] {
        let mut z = two_players();
        z.add_outside_card(p(1), pool, c(1)).unwrap();
        assert!(pools.insert(encode_to_vec(&z)));
    }
}

// ---------------------------------------------------------------------------------------------
// Property test
// ---------------------------------------------------------------------------------------------

/// Where every card is: a zone object or an outside pool.
fn card_places(z: &Zones) -> BTreeMap<CardId, usize> {
    let mut count = BTreeMap::new();
    for player in z.players() {
        for kind in ZoneKind::ALL {
            if !kind.is_per_player() && player != z.players()[0] {
                continue;
            }
            for id in z.contents(ZoneId::of(kind, player)) {
                *count.entry(z.object(id).unwrap().card).or_insert(0) += 1;
            }
        }
        for pool in [
            OutsidePool::Sideboard,
            OutsidePool::Companion,
            OutsidePool::Lesson,
            OutsidePool::AttractionDeck,
            OutsidePool::ContraptionDeck,
            OutsidePool::Spellbook,
        ] {
            for card in z.outside_cards(player, pool) {
                *count.entry(card).or_insert(0) += 1;
            }
        }
    }
    count
}

fn ordered_zones(z: &Zones) -> Vec<ZoneId> {
    let mut v = vec![ZoneId::Stack];
    for player in z.players() {
        v.push(ZoneId::Library(player));
        v.push(ZoneId::Graveyard(player));
    }
    v
}

fn pick<T: Copy>(rng: &mut Pcg64Dxsm, v: &[T]) -> Option<T> {
    if v.is_empty() {
        return None;
    }
    let n = u64::try_from(v.len()).unwrap();
    v.get(usize::try_from(rng.next_u64() % n).unwrap()).copied()
}

/// Runs one random operation sequence; returns the encoding after every step.
fn run_random(seed: u64, steps: usize) -> Vec<Vec<u8>> {
    let mut rng = Pcg64Dxsm::from_u64_seed(seed);
    let player_count = 2 + rng.next_u64() % 3;
    let players: Vec<PlayerId> = (1..=player_count).map(p).collect();
    let mut z = Zones::new(&players, ZonesConfig::default());
    let mut ids = IdGenerator::new();
    let mut next_card = 1u64;
    let mut retired: BTreeSet<ObjectId> = BTreeSet::new();
    let pools = [
        OutsidePool::Sideboard,
        OutsidePool::Companion,
        OutsidePool::Lesson,
        OutsidePool::AttractionDeck,
        OutsidePool::ContraptionDeck,
        OutsidePool::Spellbook,
    ];
    let mut trace = Vec::new();
    for _ in 0..steps {
        let saved = z.clone();
        let saved_bytes = encode_to_vec(&z);
        let before_order: Vec<(ZoneId, Vec<ObjectId>)> = ordered_zones(&z)
            .into_iter()
            .map(|zone| (zone, z.contents(zone)))
            .collect();
        let last_before = ids.last_issued();
        let kind = pick(&mut rng, &ZoneKind::ALL).unwrap();
        let position = match rng.next_u64() % 4 {
            0 => Position::Top,
            1 => Position::Bottom,
            _ => Position::Index(usize::try_from(rng.next_u64() % 8).unwrap()),
        };
        let r = MoveRequest {
            to: kind,
            position,
            face_down: rng.next_u64().is_multiple_of(4),
            controller: None,
        };
        let objects = z.all_object_ids();
        let mut touched: BTreeSet<ObjectId> = BTreeSet::new();
        match rng.next_u64() % 5 {
            0 => {
                let owner = pick(&mut rng, &players).unwrap();
                let id = z.create_object(&mut ids, c(next_card), owner, r).unwrap();
                next_card += 1;
                assert_eq!(id.get(), last_before + 1);
                touched.insert(id);
            }
            1 | 2 => {
                if let Some(old) = pick(&mut rng, &objects) {
                    let m = z.move_object(&mut ids, old, r).unwrap();
                    assert_eq!(m.new.get(), last_before + 1);
                    retired.insert(old);
                    touched.insert(old);
                    touched.insert(m.new);
                }
            }
            3 => {
                if let Some(old) = pick(&mut rng, &objects) {
                    let pool = pick(&mut rng, &pools).unwrap();
                    z.send_outside(old, pool).unwrap();
                    retired.insert(old);
                    touched.insert(old);
                }
            }
            _ => {
                let mut outside = Vec::new();
                for &player in &players {
                    for pool in pools {
                        for card in z.outside_cards(player, pool) {
                            outside.push((player, pool, card));
                        }
                    }
                }
                if let Some((owner, pool, card)) = pick(&mut rng, &outside) {
                    let id = z
                        .enter_from_outside(&mut ids, owner, card, pool, r)
                        .unwrap();
                    assert_eq!(id.get(), last_before + 1);
                    touched.insert(id);
                }
            }
        }
        // (a) no duplicates, no retired id, ids within the issued range.
        let all = z.all_object_ids();
        let unique: BTreeSet<ObjectId> = all.iter().copied().collect();
        assert_eq!(unique.len(), all.len());
        assert!(unique.is_disjoint(&retired));
        assert!(all.iter().all(|id| id.get() <= ids.last_issued()));
        // (b) relative order of untouched objects preserved in ordered zones.
        for (zone, before) in before_order {
            let after: Vec<ObjectId> = z
                .contents(zone)
                .into_iter()
                .filter(|id| !touched.contains(id))
                .collect();
            let before: Vec<ObjectId> = before
                .into_iter()
                .filter(|id| !touched.contains(id))
                .collect();
            assert_eq!(after, before, "order changed in {zone:?}");
        }
        // (c) each card in exactly one place.
        let places = card_places(&z);
        assert!(places.values().all(|&n| n == 1));
        assert_eq!(places.len(), usize::try_from(next_card - 1).unwrap());
        // (d) the pre-step snapshot is unchanged.
        assert_eq!(encode_to_vec(&saved), saved_bytes);
        assert_ne!(saved_bytes.len(), 0);
        trace.push(encode_to_vec(&z));
    }
    trace
}

// REQ-CORE-002, REQ-ZONE-001 items 1,2,4,5,6,7,9; CR 400.7, CR 400.5, CR 400.11
#[test]
fn property_random_operation_sequences() {
    for seed in 0..40u64 {
        let first = run_random(seed, 120);
        // (e) identical operation sequences give identical bytes.
        let second = run_random(seed, 120);
        assert_eq!(first, second);
    }
}

// ---------------------------------------------------------------------------------------------
// The CR 400.7 exception registry
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002; CR 400.7: twelve exceptions a-k and m
#[test]
fn registry_has_twelve_entries_in_rule_order() {
    let rules: Vec<&str> = EXCEPTIONS.iter().map(|e| e.exception.rule()).collect();
    assert_eq!(
        rules,
        [
            "400.7a", "400.7b", "400.7c", "400.7d", "400.7e", "400.7f", "400.7g", "400.7h",
            "400.7i", "400.7j", "400.7k", "400.7m"
        ]
    );
    assert!(!rules.contains(&"400.7l"));
    let from_all: Vec<Exception400_7> = EXCEPTIONS.iter().map(|e| e.exception).collect();
    assert_eq!(from_all, Exception400_7::ALL);
    let names: BTreeSet<&str> = Exception400_7::ALL.iter().map(|e| e.name()).collect();
    assert_eq!(names.len(), 12);
    for n in &names {
        assert!(n.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_'));
    }
    let rule_set: BTreeSet<&str> = rules.iter().copied().collect();
    assert_eq!(rule_set.len(), 12);
    for entry in EXCEPTIONS {
        assert_eq!(
            (entry.apply)(ObjectId::new(1), ObjectId::new(2)),
            ExceptionOutcome::NotImplemented(entry.exception)
        );
    }
}

// REQ-CORE-002, CR 400.7a: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7a_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7a_permanent_spell_effects(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::A)
    );
}

// REQ-CORE-002, CR 400.7b: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7b_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7b_static_granted_abilities(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::B)
    );
}

// REQ-CORE-002, CR 400.7c: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7c_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7c_prevention_effects(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::C)
    );
}

// REQ-CORE-002, CR 400.7d: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7d_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7d_spell_information(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::D)
    );
}

// REQ-CORE-002, CR 400.7e: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7e_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7e_leaves_zone_triggers(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::E)
    );
}

// REQ-CORE-002, CR 400.7f: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7f_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7f_aura_leaves_triggers(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::F)
    );
}

// REQ-CORE-002, CR 400.7g: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7g_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7g_granted_cast_ability(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::G)
    );
}

// REQ-CORE-002, CR 400.7h: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7h_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7h_cast_effect_finds_spell(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::H)
    );
}

// REQ-CORE-002, CR 400.7i: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7i_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7i_play_land_effect_finds_permanent(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::I)
    );
}

// REQ-CORE-002, CR 400.7j: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7j_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7j_effect_finds_moved_object(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::J)
    );
}

// REQ-CORE-002, CR 400.7k: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7k_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7k_madness_finds_card(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::K)
    );
}

// REQ-CORE-002, CR 400.7m: stub. The issue that implements this exception replaces this test.
#[test]
fn exception_400_7m_stub_is_not_implemented() {
    assert_eq!(
        exception_400_7m_stickers_retained(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::M)
    );
}
