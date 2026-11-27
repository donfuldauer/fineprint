// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::zones`. Black-box: written from the work-item brief only.
//!
//! Retired CardIds: a card whose object ceased to exist (CR 704.5d, CR 400.7) is remembered by
//! the container and never taken back by `create_object`, `enter_from_outside` or
//! `add_outside_card`; rejections are atomic; the memory is part of equality and of the canonical
//! encoding (REQ-CORE-002, REQ-ZONE-001, REQ-NFR-006).

use std::collections::BTreeSet;

use fineprint_base::encode::encode_to_vec;
use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::zones::{MoveRequest, OutsidePool, ZoneError, ZoneKind, Zones, ZonesConfig};

fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn cid(n: u64) -> CardId {
    CardId::new(n)
}

fn req(kind: ZoneKind) -> MoveRequest {
    MoveRequest::new(kind)
}

fn two() -> Zones {
    Zones::new(&[pid(1), pid(2)], ZonesConfig::default())
}

fn idgen() -> IdGenerator<ObjectId> {
    IdGenerator::<ObjectId>::new()
}

const POOLS: [OutsidePool; 3] = [
    OutsidePool::Sideboard,
    OutsidePool::Lesson,
    OutsidePool::Companion,
];

fn tail8(z: &Zones) -> u64 {
    let b = encode_to_vec(z);
    let t: [u8; 8] = b[b.len() - 8..].try_into().unwrap();
    u64::from_le_bytes(t)
}

/// Snapshot of container and generator, to assert a failed call changed nothing.
struct Snap(Zones, Vec<u8>, IdGenerator<ObjectId>);

impl Snap {
    fn take(z: &Zones, g: &IdGenerator<ObjectId>) -> Snap {
        Snap(z.clone(), encode_to_vec(z), g.clone())
    }
    fn assert_same(&self, z: &Zones, g: &IdGenerator<ObjectId>, ctx: &str) {
        assert_eq!(&self.0, z, "container changed: {ctx}");
        assert_eq!(self.1, encode_to_vec(z), "bytes changed: {ctx}");
        assert_eq!(&self.2, g, "generator changed: {ctx}");
    }
}

// REQ-CORE-002 (CR 704.5d, CR 400.7): a ceased card is refused by create_object in every zone
// kind, for the same owner and the other player, atomically.
#[test]
fn req_core_002_create_object_rejects_ceased_card_everywhere() {
    for from in ZoneKind::ALL {
        let mut z = two();
        let mut g = idgen();
        let card = cid(7);
        let o = z.create_object(&mut g, card, pid(1), req(from)).unwrap();
        assert_eq!(z.cease_to_exist(o), Ok(card));
        for to in ZoneKind::ALL {
            for owner in [pid(1), pid(2)] {
                let snap = Snap::take(&z, &g);
                let r = z.create_object(&mut g, card, owner, req(to));
                assert_eq!(r, Err(ZoneError::CardCeased(card)), "{from:?}->{to:?}");
                snap.assert_same(&z, &g, "create_object");
            }
        }
        assert_eq!(z.object_of_card(card), None);
    }
}

// REQ-CORE-002 (CR 704.5d): a token-like object on the battlefield ceases, and its card id
// cannot be re-created on the battlefield.
#[test]
fn req_core_002_token_like_battlefield_object_not_recreated() {
    let mut z = two();
    let mut g = idgen();
    let card = cid(11);
    let o = z
        .create_object(&mut g, card, pid(1), req(ZoneKind::Battlefield))
        .unwrap();
    z.cease_to_exist(o).unwrap();
    let snap = Snap::take(&z, &g);
    assert_eq!(
        z.create_object(&mut g, card, pid(1), req(ZoneKind::Battlefield)),
        Err(ZoneError::CardCeased(card))
    );
    snap.assert_same(&z, &g, "battlefield re-create");
}

// REQ-CORE-002: enter_from_outside with a ceased card is always CardCeased, never
// NoSuchOutsideCard; a legitimate pool card still enters.
#[test]
fn req_core_002_enter_from_outside_rejects_ceased_card() {
    let mut z = two();
    let mut g = idgen();
    z.add_outside_card(pid(1), OutsidePool::Sideboard, cid(50))
        .unwrap();
    let side = z
        .create_object(&mut g, cid(51), pid(2), req(ZoneKind::Hand))
        .unwrap();
    z.send_outside(side, OutsidePool::Lesson).unwrap();
    let dead = cid(52);
    let o = z
        .create_object(&mut g, dead, pid(1), req(ZoneKind::Graveyard))
        .unwrap();
    z.cease_to_exist(o).unwrap();

    for pool in POOLS {
        for owner in [pid(1), pid(2)] {
            let snap = Snap::take(&z, &g);
            let r = z.enter_from_outside(&mut g, owner, dead, pool, req(ZoneKind::Hand));
            assert_eq!(r, Err(ZoneError::CardCeased(dead)), "{pool:?} {owner:?}");
            snap.assert_same(&z, &g, "enter_from_outside");
        }
    }
    // Control.
    assert_eq!(
        z.outside_cards(pid(1), OutsidePool::Sideboard),
        vec![cid(50)]
    );
    assert_eq!(z.outside_cards(pid(2), OutsidePool::Lesson), vec![cid(51)]);
    let n = z
        .enter_from_outside(
            &mut g,
            pid(1),
            cid(50),
            OutsidePool::Sideboard,
            req(ZoneKind::Hand),
        )
        .unwrap();
    assert_eq!(z.object_of_card(cid(50)), Some(n));
    assert!(z.outside_cards(pid(1), OutsidePool::Sideboard).is_empty());
    assert_eq!(z.outside_cards(pid(2), OutsidePool::Lesson), vec![cid(51)]);
}

// REQ-CORE-002: add_outside_card with a ceased card is CardCeased and atomic; a fresh card works.
#[test]
fn req_core_002_add_outside_card_rejects_ceased_card() {
    let mut z = two();
    let mut g = idgen();
    let dead = cid(3);
    let o = z
        .create_object(&mut g, dead, pid(1), req(ZoneKind::Exile))
        .unwrap();
    z.cease_to_exist(o).unwrap();
    for pool in POOLS {
        for owner in [pid(1), pid(2)] {
            let snap = Snap::take(&z, &g);
            assert_eq!(
                z.add_outside_card(owner, pool, dead),
                Err(ZoneError::CardCeased(dead))
            );
            snap.assert_same(&z, &g, "add_outside_card");
        }
    }
    assert_eq!(
        z.add_outside_card(pid(1), OutsidePool::Sideboard, cid(4)),
        Ok(())
    );
    assert_eq!(
        z.outside_cards(pid(1), OutsidePool::Sideboard),
        vec![cid(4)]
    );
}

// REQ-CORE-002: precedence. Unknown player first; CardCeased before the controller check.
#[test]
fn req_core_002_precedence_of_errors() {
    let mut z = two();
    let mut g = idgen();
    let dead = cid(9);
    let o = z
        .create_object(&mut g, dead, pid(1), req(ZoneKind::Hand))
        .unwrap();
    z.cease_to_exist(o).unwrap();

    let snap = Snap::take(&z, &g);
    let stranger = pid(99);
    assert_eq!(
        z.create_object(&mut g, dead, stranger, req(ZoneKind::Hand)),
        Err(ZoneError::UnknownPlayer(stranger))
    );
    assert_eq!(
        z.add_outside_card(stranger, OutsidePool::Sideboard, dead),
        Err(ZoneError::UnknownPlayer(stranger))
    );
    assert_eq!(
        z.enter_from_outside(
            &mut g,
            stranger,
            dead,
            OutsidePool::Sideboard,
            req(ZoneKind::Hand)
        ),
        Err(ZoneError::UnknownPlayer(stranger))
    );
    snap.assert_same(&z, &g, "unknown player");

    let mut r = req(ZoneKind::Hand);
    r.controller = Some(pid(2));
    assert_eq!(
        z.create_object(&mut g, dead, pid(1), r),
        Err(ZoneError::CardCeased(dead))
    );
    assert_eq!(
        z.enter_from_outside(&mut g, pid(1), dead, OutsidePool::Sideboard, r),
        Err(ZoneError::CardCeased(dead))
    );
    snap.assert_same(&z, &g, "controller precedence");
}

// REQ-CORE-002, REQ-ZONE-001: send_outside and move_object never retire; failed ceases retire
// nothing; ceasing one object retires only its own card.
#[test]
fn req_core_002_non_retiring_operations() {
    let mut z = two();
    let mut g = idgen();
    let card = cid(20);
    let o = z
        .create_object(&mut g, card, pid(1), req(ZoneKind::Library))
        .unwrap();
    assert_eq!(z.send_outside(o, OutsidePool::Sideboard), Ok(card));
    let back = z
        .enter_from_outside(
            &mut g,
            pid(1),
            card,
            OutsidePool::Sideboard,
            req(ZoneKind::Hand),
        )
        .unwrap();

    // Move through every zone kind (twice round) never yields CardCeased.
    let mut cur = back;
    for kind in ZoneKind::ALL.into_iter().chain(ZoneKind::ALL) {
        cur = z.move_object(&mut g, cur, req(kind)).unwrap().new;
    }
    assert_eq!(z.object_of_card(card), Some(cur));

    // Failed ceases: unknown id and an already-ceased id.
    let other = z
        .create_object(&mut g, cid(21), pid(2), req(ZoneKind::Battlefield))
        .unwrap();
    let unknown = ObjectId::new(10_000);
    let snap = Snap::take(&z, &g);
    assert_eq!(
        z.cease_to_exist(unknown),
        Err(ZoneError::UnknownObject(unknown))
    );
    snap.assert_same(&z, &g, "failed cease");
    z.cease_to_exist(other).unwrap();
    let snap = Snap::take(&z, &g);
    assert_eq!(
        z.cease_to_exist(other),
        Err(ZoneError::UnknownObject(other))
    );
    snap.assert_same(&z, &g, "double cease");

    // Card 21 is retired; card 20 (live) is not.
    assert_eq!(
        z.create_object(&mut g, cid(21), pid(2), req(ZoneKind::Hand)),
        Err(ZoneError::CardCeased(cid(21)))
    );
    let cur = z
        .move_object(&mut g, cur, req(ZoneKind::Exile))
        .unwrap()
        .new;
    assert_eq!(z.cease_to_exist(cur), Ok(card));
    assert_eq!(
        z.create_object(&mut g, card, pid(1), req(ZoneKind::Hand)),
        Err(ZoneError::CardCeased(card))
    );
    // Unrelated card, created after the ceased objects' ids, works.
    let fresh = z
        .create_object(&mut g, cid(22), pid(1), req(ZoneKind::Hand))
        .unwrap();
    assert_eq!(z.object_of_card(cid(22)), Some(fresh));
}

// REQ-NFR-006, REQ-CORE-002: the retired set is part of equality and the canonical encoding.
#[test]
fn req_nfr_006_retired_set_in_equality_and_encoding() {
    // Same visible contents and same mark; only the retired card differs.
    let build = |dead: u64| {
        let mut z = two();
        let mut g = idgen();
        let a = z
            .create_object(&mut g, cid(dead), pid(1), req(ZoneKind::Hand))
            .unwrap();
        z.create_object(&mut g, cid(2), pid(1), req(ZoneKind::Hand))
            .unwrap();
        z.cease_to_exist(a).unwrap();
        z
    };
    let a = build(1);
    let c = build(9);
    assert_ne!(a, c);
    assert_ne!(encode_to_vec(&a), encode_to_vec(&c));
    assert_eq!(a.all_object_ids(), c.all_object_ids());
    assert_eq!(tail8(&a), tail8(&c));

    // Identical operations: equal, identical bytes; encoding is pure.
    let a2 = build(1);
    assert_eq!(a, a2);
    assert_eq!(encode_to_vec(&a), encode_to_vec(&a2));
    assert_eq!(encode_to_vec(&a), encode_to_vec(&a));

    // Ceased vs never created.
    // The survivor must end with the same object id and allocation mark as in `build(1)`
    // (the second allocated object) without retiring a card: create it in the library, then move
    // it to the hand.
    let mut never = two();
    let mut gn = idgen();
    let in_library = never
        .create_object(&mut gn, cid(2), pid(1), req(ZoneKind::Library))
        .unwrap();
    let survivor = never
        .move_object(&mut gn, in_library, req(ZoneKind::Hand))
        .unwrap()
        .new;
    assert_eq!(
        never.object_of_card(cid(2)),
        a.object_of_card(cid(2)),
        "same surviving object id"
    );
    assert_eq!(never.object_of_card(cid(2)), Some(survivor));
    assert_eq!(never.all_object_ids(), a.all_object_ids());
    assert_eq!(tail8(&never), tail8(&a), "same allocation mark");
    assert_ne!(never, a);
    assert_ne!(encode_to_vec(&never), encode_to_vec(&a));

    // The id mark is unchanged by ceasing.
    let mut z = two();
    let mut g = idgen();
    assert_eq!(tail8(&z), 0);
    let mut high = 0;
    let mut objs = Vec::new();
    for n in 1..=3 {
        let o = z
            .create_object(&mut g, cid(n), pid(2), req(ZoneKind::Graveyard))
            .unwrap();
        high = high.max(o.get());
        objs.push(o);
    }
    assert_eq!(tail8(&z), high);
    for o in objs {
        z.cease_to_exist(o).unwrap();
        assert_eq!(tail8(&z), high);
    }
}

// REQ-NFR-006: ceasing cards in opposite orders yields equal state and identical bytes.
#[test]
fn req_nfr_006_cease_order_does_not_matter() {
    let build = |order: [usize; 2]| {
        let mut z = two();
        let mut g = idgen();
        let ids: Vec<ObjectId> = (1..=3)
            .map(|n| {
                z.create_object(&mut g, cid(n), pid(1), req(ZoneKind::Battlefield))
                    .unwrap()
            })
            .collect();
        for i in order {
            z.cease_to_exist(ids[i]).unwrap();
        }
        z
    };
    let x = build([0, 1]);
    let y = build([1, 0]);
    assert_eq!(x, y);
    assert_eq!(encode_to_vec(&x), encode_to_vec(&y));
}

// REQ-CORE-002, REQ-NFR-006: clones are independent; a pre-cease snapshot still accepts the card.
#[test]
fn req_core_002_clone_independence() {
    let mut z = two();
    let mut g = idgen();
    let card = cid(5);
    let o = z
        .create_object(&mut g, card, pid(1), req(ZoneKind::Battlefield))
        .unwrap();
    let before = z.clone();
    let before_bytes = encode_to_vec(&z);

    let mut ceased = z.clone();
    ceased.cease_to_exist(o).unwrap();

    assert_eq!(z, before);
    assert_eq!(encode_to_vec(&z), before_bytes);
    assert_ne!(z, ceased);
    assert_ne!(encode_to_vec(&ceased), before_bytes);
    assert_eq!(
        ceased.create_object(&mut g.clone(), card, pid(1), req(ZoneKind::Hand)),
        Err(ZoneError::CardCeased(card))
    );
    // The original still holds the live object and the snapshot still accepts the card once the
    // object is gone from it (removed from the original by a move to another id is not a cease).
    assert_eq!(z.object_of_card(card), Some(o));
    let mut snap = before.clone();
    let moved = snap.move_object(&mut g, o, req(ZoneKind::Exile)).unwrap();
    assert_eq!(snap.object_of_card(card), Some(moved.new));
}

// REQ-CORE-002 (CR 704.5d): bounded property test, fixed seed. Ceased cards stay refused and are
// never present in any record or pool.
#[test]
fn req_core_002_property_ceased_cards_stay_retired() {
    let owners = [pid(1), pid(2)];
    let mut rng = Pcg64Dxsm::from_u64_seed(0x5e71_2ed0);
    let mut pick = |n: usize| usize::try_from(rng.next_bounded(n as u64).unwrap()).unwrap();

    let mut z = two();
    let mut g = idgen();
    let mut next_card = 1000_u64;
    let mut live: Vec<(ObjectId, CardId)> = Vec::new();
    let mut pooled: Vec<(PlayerId, OutsidePool, CardId)> = Vec::new();
    let mut ceased: BTreeSet<CardId> = BTreeSet::new();

    for step in 0..200 {
        match pick(6) {
            0 => {
                next_card += 1;
                let owner = owners[pick(2)];
                let kind = ZoneKind::ALL[pick(7)];
                let o = z
                    .create_object(&mut g, cid(next_card), owner, req(kind))
                    .unwrap();
                live.push((o, cid(next_card)));
            }
            1 if !live.is_empty() => {
                let i = pick(live.len());
                let kind = ZoneKind::ALL[pick(7)];
                let m = z.move_object(&mut g, live[i].0, req(kind)).unwrap();
                live[i].0 = m.new;
            }
            2 if !live.is_empty() => {
                let (o, c) = live.remove(pick(live.len()));
                let owner = z.object(o).unwrap().owner;
                let pool = POOLS[pick(3)];
                assert_eq!(z.send_outside(o, pool), Ok(c));
                pooled.push((owner, pool, c));
            }
            3 if !pooled.is_empty() => {
                let (owner, pool, c) = pooled.remove(pick(pooled.len()));
                let o = z
                    .enter_from_outside(&mut g, owner, c, pool, req(ZoneKind::ALL[pick(7)]))
                    .unwrap();
                live.push((o, c));
            }
            4 if !live.is_empty() => {
                let (o, c) = live.remove(pick(live.len()));
                assert_eq!(z.cease_to_exist(o), Ok(c));
                ceased.insert(c);
            }
            5 => {
                next_card += 1;
                let owner = owners[pick(2)];
                let pool = POOLS[pick(3)];
                z.add_outside_card(owner, pool, cid(next_card)).unwrap();
                pooled.push((owner, pool, cid(next_card)));
            }
            _ => {}
        }

        for &c in &ceased {
            let ctx = format!("step {step} card {c:?}");
            let mut zc = z.clone();
            let mut gc = g.clone();
            let e = ZoneError::CardCeased(c);
            assert_eq!(
                zc.create_object(&mut gc, c, owners[0], req(ZoneKind::Hand)),
                Err(e),
                "{ctx}"
            );
            assert_eq!(
                zc.add_outside_card(owners[1], OutsidePool::Lesson, c),
                Err(e),
                "{ctx}"
            );
            assert_eq!(
                zc.enter_from_outside(
                    &mut gc,
                    owners[0],
                    c,
                    OutsidePool::Sideboard,
                    req(ZoneKind::Hand)
                ),
                Err(e),
                "{ctx}"
            );
            assert_eq!(zc, z, "{ctx}");
            assert_eq!(gc, g, "{ctx}");
            assert_eq!(z.object_of_card(c), None, "{ctx}");
            for owner in owners {
                for pool in POOLS {
                    assert!(!z.outside_cards(owner, pool).contains(&c), "{ctx}");
                }
            }
        }
        for id in z.all_object_ids() {
            let card = z.object(id).unwrap().card;
            assert!(!ceased.contains(&card));
        }
    }
    assert!(!ceased.is_empty(), "property run must exercise cease");
}
