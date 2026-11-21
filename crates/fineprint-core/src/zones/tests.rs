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
        ZoneError::IdCollision(ObjectId::new(8)),
    ];
    let texts: BTreeSet<String> = all.iter().map(ToString::to_string).collect();
    assert_eq!(texts.len(), all.len());
    assert!(all[0].to_string().contains('9'));
    assert!(all[1].to_string().contains('4'));
    assert!(all[2].to_string().contains('5'));
    assert!(all[7].to_string().contains('6'));
    assert!(all[8].to_string().contains('8'));
    assert!(all[8].to_string().contains("not above the highest id"));
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

// REQ-CORE-002, REQ-ZONE-001; CR 400.7, CR 400.5: a move into the zone the object is
// already in (library, hand, graveyard, battlefield, stack) is not a zone change: the object
// keeps its id and the generator is not touched.
// R10 correction for finding F1 (CodeRabbit comment 4621941423): this test replaces the former
// `same_zone_move_creates_new_id`, which asserted a new id for every same-zone move.
#[test]
fn same_zone_move_in_ordinary_zone_keeps_id() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    for kind in [
        ZoneKind::Library,
        ZoneKind::Hand,
        ZoneKind::Graveyard,
        ZoneKind::Battlefield,
        ZoneKind::Stack,
    ] {
        let old = z
            .create_object(&mut ids, c(100 + kind as u64), p(2), req(kind))
            .unwrap();
        let ids0 = ids.clone();
        let m = z.move_object(&mut ids, old, req(kind)).unwrap();
        assert_eq!(m.old, old);
        assert_eq!(m.new, old, "{kind:?}");
        assert_eq!(m.from, m.to);
        assert_eq!(m.to, ZoneId::of(kind, p(2)));
        assert_eq!(ids, ids0, "{kind:?}: generator touched");
        let r = z.object(old).unwrap();
        assert_eq!((r.id, r.card, r.zone), (old, c(100 + kind as u64), m.to));
        assert_eq!(z.object_of_card(r.card), Some(old));
        assert_eq!(z.contents(m.to), vec![old]);
        z.send_outside(old, OutsidePool::Sideboard).unwrap();
    }
}

// REQ-CORE-002; CR 400.8, CR 406.7 (exile), CR 400.10 (command): exiling an object in exile or
// putting an object in the command zone into the command zone makes a new object: a fresh id,
// the old id absent everywhere.
// R10 correction for finding F1 (CodeRabbit comment 4621941423).
#[test]
fn same_zone_move_in_exile_and_command_creates_new_id() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    for kind in [ZoneKind::Exile, ZoneKind::Command] {
        let old = z
            .create_object(&mut ids, c(100 + kind as u64), p(2), req(kind))
            .unwrap();
        let before = ids.last_issued();
        let m = z.move_object(&mut ids, old, req(kind)).unwrap();
        assert_eq!(m.old, old);
        assert_ne!(m.new, old);
        assert_eq!(m.new.get(), before + 1);
        assert_eq!(ids.last_issued(), before + 1);
        assert_eq!(m.from, m.to);
        assert_eq!(m.to, ZoneId::of(kind, p(2)));
        assert!(z.object(old).is_none());
        assert!(!z.all_object_ids().contains(&old));
        for player in z.players() {
            for k in ZoneKind::ALL {
                assert!(!z.contents(ZoneId::of(k, player)).contains(&old));
            }
        }
        assert_eq!(z.contents(m.to), vec![m.new]);
        assert_eq!(z.object_of_card(c(100 + kind as u64)), Some(m.new));
        z.send_outside(m.new, OutsidePool::Sideboard).unwrap();
    }
}

// REQ-ZONE-001 items 1,4,5, REQ-CORE-002; CR 400.5, CR 400.7, CR 401.7: Top, Bottom and Index
// repositions inside the library, graveyard and stack keep the id, move exactly that object and
// never touch the generator (finding F1, comment 4621941423).
#[test]
fn reposition_in_ordered_zones_keeps_id_and_moves_only_that_object() {
    for kind in [ZoneKind::Library, ZoneKind::Graveyard, ZoneKind::Stack] {
        let mut z = two_players();
        let mut ids = IdGenerator::new();
        let zone = ZoneId::of(kind, p(1));
        let a: Vec<ObjectId> = (1..=5)
            .map(|k| {
                z.create_object(&mut ids, c(k), p(1), at(kind, Position::Bottom))
                    .unwrap()
            })
            .collect();
        assert_eq!(z.contents(zone), a);
        let ids0 = ids.clone();
        let steps = [
            (a[0], Position::Index(2), vec![a[1], a[2], a[0], a[3], a[4]]),
            (a[4], Position::Top, vec![a[4], a[1], a[2], a[0], a[3]]),
            (a[1], Position::Bottom, vec![a[4], a[2], a[0], a[3], a[1]]),
            (
                a[2],
                Position::Index(usize::MAX),
                vec![a[4], a[0], a[3], a[1], a[2]],
            ),
            (a[3], Position::Index(0), vec![a[3], a[4], a[0], a[1], a[2]]),
        ];
        for (id, position, expected) in steps {
            let m = z.move_object(&mut ids, id, at(kind, position)).unwrap();
            assert_eq!((m.old, m.new, m.from, m.to), (id, id, zone, zone));
            assert_eq!(z.contents(zone), expected, "{kind:?} {position:?}");
            assert_eq!(z.object(id).unwrap().zone, zone);
        }
        assert_eq!(ids, ids0);
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(z.all_object_ids(), sorted);
    }
}

// REQ-ZONE-001, REQ-CORE-002; CR 400.7, CR 109.4, CR 110.2a, CR 405.4, CR 108.4a: a
// battlefield-to-battlefield (or stack-to-stack) move with a controller change keeps the id and
// takes face_down and the controller from the request (finding F1, comment 4621941423).
#[test]
fn battlefield_and_stack_reposition_with_controller_change_keeps_id() {
    for kind in [ZoneKind::Battlefield, ZoneKind::Stack] {
        let mut z = two_players();
        let mut ids = IdGenerator::new();
        let o = z.create_object(&mut ids, c(1), p(1), req(kind)).unwrap();
        let ids0 = ids.clone();
        let m = z
            .move_object(
                &mut ids,
                o,
                MoveRequest {
                    controller: Some(p(2)),
                    face_down: true,
                    ..req(kind)
                },
            )
            .unwrap();
        assert_eq!((m.old, m.new), (o, o));
        let r = *z.object(o).unwrap();
        assert_eq!((r.owner, r.controller, r.face_down), (p(1), p(2), true));
        assert_eq!(r.rules_controller(), Some(p(2)));
        // A default request: the controller is the owner again, face up.
        let m = z.move_object(&mut ids, o, req(kind)).unwrap();
        assert_eq!(m.new, o);
        let r = *z.object(o).unwrap();
        assert_eq!((r.controller, r.face_down), (p(1), false));
        // The controller check still applies to a same-zone request.
        assert_atomic_err(
            &mut z,
            &mut ids,
            ZoneError::UnknownPlayer(p(9)),
            |z, ids| {
                z.move_object(
                    ids,
                    o,
                    MoveRequest {
                        controller: Some(p(9)),
                        ..req(kind)
                    },
                )
            },
        );
        assert_eq!(ids, ids0);
    }
    // An explicit controller in a same-zone request elsewhere is still rejected (container rule).
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let h = z
        .create_object(&mut ids, c(1), p(1), req(ZoneKind::Hand))
        .unwrap();
    assert_atomic_err(
        &mut z,
        &mut ids,
        ZoneError::ControllerNotApplicable,
        |z, ids| {
            z.move_object(
                ids,
                h,
                MoveRequest {
                    controller: Some(p(1)),
                    ..req(ZoneKind::Hand)
                },
            )
        },
    );
}

// REQ-ZONE-001, REQ-CORE-002; CR 400.3, CR 400.7: a library move of an object owned by another
// player can only go to its owner's library, so a library-to-library move is always a same-ZoneId
// reposition (finding F1, comment 4621941423).
#[test]
fn library_to_library_is_always_the_owners_library() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let o = z
        .create_object(&mut ids, c(1), p(2), req(ZoneKind::Library))
        .unwrap();
    let m = z
        .move_object(&mut ids, o, at(ZoneKind::Library, Position::Bottom))
        .unwrap();
    assert_eq!((m.old, m.new), (o, o));
    assert_eq!(
        (m.from, m.to),
        (ZoneId::Library(p(2)), ZoneId::Library(p(2)))
    );
    assert!(z.is_empty(ZoneId::Library(p(1))));
    assert_eq!(ids.last_issued(), 1);
}

// REQ-CORE-002; CR 400.7: a generator that is behind the container issues an id that names a
// live object; every id-issuing call rejects it with IdCollision and changes nothing (finding
// F3, comment 4621941385). A reposition (finding F1) never touches the generator.
#[test]
fn behind_generator_is_rejected_with_id_collision() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let lib = z
        .create_object(&mut ids, c(1), p(1), req(ZoneKind::Library))
        .unwrap();
    let hand = z
        .create_object(&mut ids, c(2), p(1), req(ZoneKind::Hand))
        .unwrap();
    let ex = z
        .create_object(&mut ids, c(3), p(1), req(ZoneKind::Exile))
        .unwrap();
    assert_eq!(
        (lib, hand, ex),
        (ObjectId::new(1), ObjectId::new(2), ObjectId::new(3))
    );
    z.add_outside_card(p(1), OutsidePool::Sideboard, c(4))
        .unwrap();
    let mut behind: IdGenerator<ObjectId> = IdGenerator::from_last_issued(0);
    assert_atomic_err(&mut z, &mut behind, ZoneError::IdCollision(lib), |z, g| {
        z.create_object(g, c(5), p(1), req(ZoneKind::Hand))
    });
    assert_atomic_err(&mut z, &mut behind, ZoneError::IdCollision(lib), |z, g| {
        z.move_object(g, hand, req(ZoneKind::Battlefield))
    });
    assert_atomic_err(&mut z, &mut behind, ZoneError::IdCollision(lib), |z, g| {
        z.move_object(g, ex, req(ZoneKind::Exile))
    });
    assert_atomic_err(&mut z, &mut behind, ZoneError::IdCollision(lib), |z, g| {
        z.enter_from_outside(g, p(1), c(4), OutsidePool::Sideboard, req(ZoneKind::Hand))
    });
    // The issued id would be the moved object's own id: still a collision, so a real zone
    // change never returns Moved { old == new }.
    let mut at_one: IdGenerator<ObjectId> = IdGenerator::from_last_issued(1);
    assert_atomic_err(&mut z, &mut at_one, ZoneError::IdCollision(hand), |z, g| {
        z.move_object(g, hand, req(ZoneKind::Graveyard))
    });
    // A reposition needs no id.
    let m = z
        .move_object(&mut behind, lib, req(ZoneKind::Library))
        .unwrap();
    assert_eq!((m.old, m.new), (lib, lib));
    assert_eq!(behind.last_issued(), 0);
    // The right generator works as before.
    let m = z.move_object(&mut ids, hand, req(ZoneKind::Exile)).unwrap();
    assert_eq!(m.new, ObjectId::new(4));
}

// REQ-CORE-002; CR 400.7: CodeRabbit's scenario (PR #167 round 2, finding G5): a saved generator
// would re-issue the id retired by a zone change; the high-water mark rejects it.
#[test]
fn saved_generator_cannot_reissue_a_retired_id() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    z.create_object(&mut ids, c(1), p(1), req(ZoneKind::Library))
        .unwrap();
    let mut saved = ids.clone();
    let two = z
        .create_object(&mut ids, c(2), p(1), req(ZoneKind::Hand))
        .unwrap();
    let m = z.move_object(&mut ids, two, req(ZoneKind::Exile)).unwrap();
    assert_eq!((m.old, m.new), (ObjectId::new(2), ObjectId::new(3)));
    assert!(z.object(ObjectId::new(2)).is_none());
    assert_atomic_err(
        &mut z,
        &mut saved,
        ZoneError::IdCollision(ObjectId::new(2)),
        |z, g| z.create_object(g, c(3), p(1), req(ZoneKind::Hand)),
    );
    assert_atomic_err(
        &mut z,
        &mut saved,
        ZoneError::IdCollision(ObjectId::new(2)),
        |z, g| z.move_object(g, ObjectId::new(1), req(ZoneKind::Hand)),
    );
}

// REQ-CORE-002: a fresh generator against a container that already issued ids is rejected by
// every id-issuing call, and nothing changes.
#[test]
fn fresh_generator_is_rejected_by_every_issuing_call() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let a = z
        .create_object(&mut ids, c(1), p(1), req(ZoneKind::Library))
        .unwrap();
    z.add_outside_card(p(1), OutsidePool::Sideboard, c(2))
        .unwrap();
    let mut fresh: IdGenerator<ObjectId> = IdGenerator::new();
    let want = ZoneError::IdCollision(ObjectId::new(1));
    assert_atomic_err(&mut z, &mut fresh, want, |z, g| {
        z.create_object(g, c(3), p(1), req(ZoneKind::Hand))
    });
    assert_atomic_err(&mut z, &mut fresh, want, |z, g| {
        z.move_object(g, a, req(ZoneKind::Hand))
    });
    assert_atomic_err(&mut z, &mut fresh, want, |z, g| {
        z.enter_from_outside(g, p(1), c(2), OutsidePool::Sideboard, req(ZoneKind::Hand))
    });
}

// REQ-CORE-002: on the normal path ids strictly increase and nothing is rejected.
#[test]
fn normal_paths_issue_strictly_increasing_ids() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let mut last = 0;
    let mut cur = z
        .create_object(&mut ids, c(1), p(1), req(ZoneKind::Library))
        .unwrap();
    for kind in [
        ZoneKind::Hand,
        ZoneKind::Exile,
        ZoneKind::Exile,
        ZoneKind::Stack,
    ] {
        assert!(cur.get() > last);
        last = cur.get();
        cur = z.move_object(&mut ids, cur, req(kind)).unwrap().new;
    }
    assert!(cur.get() > last);
    assert_eq!(ids.last_issued(), cur.get());
}

// REQ-CORE-002, CR 400.7 (no zone change): a reposition leaves the mark and the generator alone.
#[test]
fn reposition_leaves_mark_and_generator_unchanged() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let a = z
        .create_object(&mut ids, c(1), p(1), req(ZoneKind::Library))
        .unwrap();
    z.create_object(&mut ids, c(2), p(1), req(ZoneKind::Library))
        .unwrap();
    let mark = tail8(&z);
    let ids0 = ids.clone();
    z.move_object(&mut ids, a, at(ZoneKind::Library, Position::Bottom))
        .unwrap();
    assert_eq!(tail8(&z), mark);
    assert_eq!(mark, 2_u64.to_le_bytes());
    assert_eq!(ids, ids0);
    assert_eq!(ids.last_issued(), 2);
}

/// The trailing eight bytes of the canonical encoding: the high-water mark, little-endian.
fn tail8(z: &Zones) -> [u8; 8] {
    let b = encode_to_vec(z);
    b[b.len() - 8..].try_into().unwrap()
}

// REQ-CORE-002, REQ-NFR-006: equal operations give equal values and bytes; the mark is part of
// both: two containers with the same visible contents but different marks are unequal and
// encode differently.
#[test]
fn high_water_mark_is_part_of_equality_and_encoding() {
    let mut z1 = two_players();
    z1.add_outside_card(p(1), OutsidePool::Sideboard, c(1))
        .unwrap();
    let mut z2 = two_players();
    let mut ids = IdGenerator::new();
    let id = z2
        .create_object(&mut ids, c(1), p(1), req(ZoneKind::Hand))
        .unwrap();
    z2.send_outside(id, OutsidePool::Sideboard).unwrap();
    assert_eq!(
        z1.outside_cards(p(1), OutsidePool::Sideboard),
        z2.outside_cards(p(1), OutsidePool::Sideboard)
    );
    assert!(z2.all_object_ids().is_empty());
    assert_ne!(z1, z2);
    assert_ne!(encode_to_vec(&z1), encode_to_vec(&z2));
    assert_ne!(tail8(&z1), tail8(&z2));
    assert_eq!(tail8(&z1), 0_u64.to_le_bytes());
    assert_eq!(tail8(&z2), id.get().to_le_bytes());
    // Equal operations give equal values and bytes.
    let mut z3 = two_players();
    z3.add_outside_card(p(1), OutsidePool::Sideboard, c(1))
        .unwrap();
    assert_eq!(z1, z3);
    assert_eq!(encode_to_vec(&z1), encode_to_vec(&z3));
    // Out and back to the same zone: a retired id, so the mark is 3.
    let mut z4 = two_players();
    let mut ids4 = IdGenerator::new();
    let a = z4
        .create_object(&mut ids4, c(1), p(1), req(ZoneKind::Hand))
        .unwrap();
    let m = z4.move_object(&mut ids4, a, req(ZoneKind::Exile)).unwrap();
    z4.move_object(&mut ids4, m.new, req(ZoneKind::Hand))
        .unwrap();
    assert_eq!(tail8(&z4), 3_u64.to_le_bytes());
}

// REQ-CORE-002, CR 400.7: IdCollision is a high-water rejection, not proof of reuse: an id below
// the mark that the container never held is rejected too, atomically.
#[test]
fn id_collision_is_a_high_water_rejection_of_never_held_ids() {
    let mut z = two_players();
    let mut restored: IdGenerator<ObjectId> = IdGenerator::from_last_issued(9);
    let first = z
        .create_object(&mut restored, c(1), p(1), req(ZoneKind::Hand))
        .unwrap();
    assert_eq!(first, ObjectId::new(10));
    let mut later: IdGenerator<ObjectId> = IdGenerator::from_last_issued(4);
    assert_atomic_err(
        &mut z,
        &mut later,
        ZoneError::IdCollision(ObjectId::new(5)),
        |z, g| z.create_object(g, c(2), p(1), req(ZoneKind::Hand)),
    );
    assert!(z.object(ObjectId::new(5)).is_none());
}

// REQ-ZONE-001, REQ-CORE-002; CR 109.4, CR 108.4a: the rules-level controller exists only on
// the battlefield and the stack; elsewhere the stored controller is the owner fallback
// (finding F4, comment 4621941395).
#[test]
fn rules_controller_only_on_battlefield_and_stack() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    for kind in ZoneKind::ALL {
        let r = if matches!(kind, ZoneKind::Battlefield | ZoneKind::Stack) {
            MoveRequest {
                controller: Some(p(2)),
                ..req(kind)
            }
        } else {
            req(kind)
        };
        let o = z
            .create_object(&mut ids, c(kind as u64 + 1), p(1), r)
            .unwrap();
        let rec = *z.object(o).unwrap();
        if matches!(kind, ZoneKind::Battlefield | ZoneKind::Stack) {
            assert_eq!(rec.rules_controller(), Some(p(2)), "{kind:?}");
            assert_eq!(rec.controller, p(2));
        } else {
            assert_eq!(rec.rules_controller(), None, "{kind:?}");
            assert_eq!(rec.controller, p(1), "{kind:?}: owner fallback");
        }
    }
}

// REQ-ZONE-001; CR 400.3: per-player destinations are the owner's zones; the stored controller
// there is the owner (CR 108.4a fallback)
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

// REQ-ZONE-001; CR 109.4, CR 110.2, CR 405.4, CR 108.4a: owner never changes; the container
// keeps an explicit controller only on the battlefield and the stack (elsewhere the owner, the
// CR 108.4a fallback). `set_controller` elsewhere is rejected as a container rule: emblems
// (CR 109.4c) and variant cards are not modelled yet.
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

/// How many times each operation of the property loop actually ran: create, zone-changing move
/// (including the exile / command same-zone new object), same-zone reposition, send outside,
/// enter from outside. Checked so that no branch is vacuous.
type OpCounts = [usize; 5];

/// Every zone of the container listed once: each player's library, hand and graveyard, then the
/// shared zones.
fn every_zone(z: &Zones) -> Vec<ZoneId> {
    let mut v = Vec::new();
    for player in z.players() {
        v.push(ZoneId::Library(player));
        v.push(ZoneId::Hand(player));
        v.push(ZoneId::Graveyard(player));
    }
    v.extend([
        ZoneId::Battlefield,
        ZoneId::Stack,
        ZoneId::Exile,
        ZoneId::Command,
    ]);
    v
}

/// Runs one random operation sequence; returns the encoding after every step.
fn run_random(seed: u64, steps: usize, counts: &mut OpCounts) -> Vec<Vec<u8>> {
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
                counts[0] += 1;
            }
            1 | 2 => {
                if let Some(old) = pick(&mut rng, &objects) {
                    let rec = *z.object(old).unwrap();
                    let dest = ZoneId::of(kind, rec.owner);
                    let m = z.move_object(&mut ids, old, r).unwrap();
                    assert_eq!((m.old, m.from, m.to), (old, rec.zone, dest));
                    if dest == rec.zone && !matches!(dest, ZoneId::Exile | ZoneId::Command) {
                        // CR 400.7, CR 400.5 (finding F1): a reposition keeps the id.
                        assert_eq!(m.new, old);
                        assert_eq!(ids.last_issued(), last_before);
                        counts[2] += 1;
                    } else {
                        // CR 400.7, CR 400.8, CR 406.7, CR 400.10: a new object.
                        assert_eq!(m.new.get(), last_before + 1);
                        retired.insert(old);
                        counts[1] += 1;
                    }
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
                    counts[3] += 1;
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
                    counts[4] += 1;
                }
            }
        }
        // (a) no duplicate membership, no retired id, ids within the issued range. The ids are
        // counted from the raw contents of every zone (finding N2), since `all_object_ids`
        // deduplicates; every listed id resolves back to a record in that zone and to its card.
        let all = z.all_object_ids();
        let mut listed = 0usize;
        let mut unique: BTreeSet<ObjectId> = BTreeSet::new();
        for zone in every_zone(&z) {
            for id in z.contents(zone) {
                listed += 1;
                unique.insert(id);
                let rec = z.object(id).unwrap();
                assert_eq!(rec.id, id);
                assert_eq!(rec.zone, zone);
                assert_eq!(z.object_of_card(rec.card), Some(id));
            }
        }
        assert_eq!(listed, unique.len(), "an id is listed twice");
        assert_eq!(listed, all.len());
        assert!(unique.iter().copied().eq(all.iter().copied()));
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

// REQ-CORE-002, REQ-ZONE-001 items 1,2,4,5,6,7,9; CR 400.7, CR 400.8, CR 406.7, CR 400.10,
// CR 400.5, CR 400.11
#[test]
fn property_random_operation_sequences() {
    let mut counts: OpCounts = [0; 5];
    for seed in 0..40u64 {
        let first = run_random(seed, 120, &mut counts);
        // (e) identical operation sequences give identical bytes.
        let second = run_random(seed, 120, &mut [0; 5]);
        assert_eq!(first, second);
    }
    // No operation branch was vacuous (finding F6 treatment), including the F1 reposition.
    assert!(counts.iter().all(|&n| n > 0), "{counts:?}");
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
    // REQ-CORE-007: CR 400.7d and CR 400.7j are implemented (#24 FP-023); the other ten are not.
    for entry in EXCEPTIONS {
        let expected = match entry.exception {
            Exception400_7::D | Exception400_7::J => ExceptionOutcome::Applies(entry.exception),
            other => ExceptionOutcome::NotImplemented(other),
        };
        assert_eq!((entry.apply)(ObjectId::new(1), ObjectId::new(2)), expected);
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

// REQ-CORE-002, REQ-CORE-007, CR 400.7d: implemented by the memory module (#24 FP-023).
#[test]
fn exception_400_7d_applies() {
    assert_eq!(
        exception_400_7d_spell_information(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::Applies(Exception400_7::D)
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

// REQ-CORE-002, REQ-CORE-007, CR 400.7j: implemented by the memory module (#24 FP-023).
#[test]
fn exception_400_7j_applies() {
    assert_eq!(
        exception_400_7j_effect_finds_moved_object(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::Applies(Exception400_7::J)
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

// ---------------------------------------------------------------------------------------------
// Ceasing to exist (#41 FP-040)
// ---------------------------------------------------------------------------------------------

// REQ-SBA-001, CR 704.5d, CR 704.5e, CR 111.7: the object and its record are removed, the card
// is returned, put into no outside pool, and a later object can be created for the same card.
#[test]
fn cease_to_exist_removes_object_and_frees_card() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let gy = z
        .create_object(&mut ids, c(5), p(1), req(ZoneKind::Graveyard))
        .unwrap();
    let other = z
        .create_object(&mut ids, c(6), p(1), req(ZoneKind::Graveyard))
        .unwrap();
    let ids_before = ids.clone();
    let card = z.cease_to_exist(gy).unwrap();
    assert_eq!(card, c(5));
    assert!(z.object(gy).is_none());
    assert_eq!(z.object_of_card(c(5)), None);
    assert_eq!(z.contents(ZoneId::Graveyard(p(1))), vec![other]);
    assert_eq!(z.all_object_ids(), vec![other]);
    for pool in [
        OutsidePool::Sideboard,
        OutsidePool::Lesson,
        OutsidePool::Companion,
    ] {
        assert!(z.outside_cards(p(1), pool).is_empty());
    }
    // Not a zone change: no id is issued.
    assert_eq!(ids, ids_before);
    // The card is free again.
    let again = z
        .create_object(&mut ids, c(5), p(1), req(ZoneKind::Exile))
        .unwrap();
    assert!(again > other);
    assert_eq!(z.object_of_card(c(5)), Some(again));
}

// REQ-SBA-001: an unknown id is an error and the container is unchanged.
#[test]
fn cease_to_exist_unknown_object_leaves_container_unchanged() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    z.create_object(&mut ids, c(1), p(2), req(ZoneKind::Battlefield))
        .unwrap();
    let saved = z.clone();
    let bytes = encode_to_vec(&z);
    assert_eq!(
        z.cease_to_exist(ObjectId::new(999)),
        Err(ZoneError::UnknownObject(ObjectId::new(999)))
    );
    assert_eq!(z, saved);
    assert_eq!(encode_to_vec(&z), bytes);
}

// REQ-SBA-001, REQ-CORE-002: a clone sharing structure is not affected; ordered zones keep the
// order of the remaining objects (CR 400.5).
#[test]
fn cease_to_exist_does_not_affect_clones() {
    let mut z = two_players();
    let mut ids = IdGenerator::new();
    let lib = fill_library(&mut z, &mut ids, p(1), 1, 3);
    let stack = z
        .create_object(&mut ids, c(10), p(2), req(ZoneKind::Stack))
        .unwrap();
    let saved = z.clone();
    let bytes = encode_to_vec(&z);
    let mut copy = z.clone();
    copy.cease_to_exist(lib[1]).unwrap();
    copy.cease_to_exist(stack).unwrap();
    assert_eq!(copy.contents(ZoneId::Library(p(1))), vec![lib[0], lib[2]]);
    assert!(copy.is_empty(ZoneId::Stack));
    assert_eq!(z, saved);
    assert_eq!(encode_to_vec(&z), bytes);
    assert_eq!(z.contents(ZoneId::Library(p(1))), lib);
    assert_eq!(z.contents(ZoneId::Stack), vec![stack]);
}
