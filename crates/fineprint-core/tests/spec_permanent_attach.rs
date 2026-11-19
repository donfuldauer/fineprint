// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::permanent`. Black-box: written from the work-item brief only.
//!
//! Attachments: error cases and their check order, cycle rejection, re-attaching, detaching,
//! dependents cleared when a permanent leaves, `attachments_of` ordering, player targets, and a
//! property test that attachments never form a cycle (REQ-ZONE-001 item 3; REQ-CORE-006).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::permanent::{AttachTarget, PermanentError, Permanents};

fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn on(n: u64) -> AttachTarget {
    AttachTarget::Permanent(oid(n))
}

fn holding(ids: &[u64]) -> Permanents {
    let mut p = Permanents::new();
    for &i in ids {
        p.enter(oid(i), pid(1), 1).unwrap();
    }
    p
}

/// Asserts `p.attach(id, target)` fails with `err` and leaves `p` unchanged.
fn assert_attach_err(p: &mut Permanents, id: u64, target: AttachTarget, err: PermanentError) {
    let before = p.clone();
    assert_eq!(p.attach(oid(id), target), Err(err));
    assert_eq!(*p, before);
}

// REQ-ZONE-001 item 3: unknown attaching object, checked before anything else.
#[test]
fn req_zone_001_attach_unknown_object_first() {
    let mut p = holding(&[1, 2]);
    assert_attach_err(
        &mut p,
        5,
        AttachTarget::Player(pid(1)),
        PermanentError::UnknownObject(oid(5)),
    );
    assert_attach_err(&mut p, 5, on(1), PermanentError::UnknownObject(oid(5)));
    // Both unknown: the attaching id is reported.
    assert_attach_err(&mut p, 5, on(6), PermanentError::UnknownObject(oid(5)));
    // Unknown id attaching to itself: UnknownObject, not SelfAttach.
    assert_attach_err(&mut p, 5, on(5), PermanentError::UnknownObject(oid(5)));
}

// REQ-ZONE-001 item 3: unknown target permanent.
#[test]
fn req_zone_001_attach_unknown_target() {
    let mut p = holding(&[1, 2]);
    assert_attach_err(&mut p, 1, on(3), PermanentError::UnknownObject(oid(3)));
    assert_attach_err(
        &mut p,
        1,
        on(u64::MAX),
        PermanentError::UnknownObject(oid(u64::MAX)),
    );
    assert_attach_err(&mut p, 1, on(0), PermanentError::UnknownObject(oid(0)));
    // An existing link is kept when a later attach fails.
    p.attach(oid(1), on(2)).unwrap();
    assert_attach_err(&mut p, 1, on(3), PermanentError::UnknownObject(oid(3)));
    assert_eq!(p.attached_to(oid(1)), Some(on(2)));
}

// REQ-ZONE-001 item 3: a permanent cannot attach to itself.
#[test]
fn req_zone_001_attach_self() {
    let mut p = holding(&[1, 2]);
    assert_attach_err(&mut p, 1, on(1), PermanentError::SelfAttach(oid(1)));
    p.attach(oid(1), on(2)).unwrap();
    assert_attach_err(&mut p, 1, on(1), PermanentError::SelfAttach(oid(1)));
    // Self-attach is reported even when the permanent is attached into a chain.
    p.attach(oid(2), AttachTarget::Player(pid(1))).unwrap();
    assert_attach_err(&mut p, 2, on(2), PermanentError::SelfAttach(oid(2)));
}

// REQ-ZONE-001 item 3: a two-permanent cycle is rejected.
#[test]
fn req_zone_001_attach_cycle_length_2() {
    let mut p = holding(&[1, 2]);
    p.attach(oid(1), on(2)).unwrap();
    assert_attach_err(
        &mut p,
        2,
        on(1),
        PermanentError::AttachmentCycle {
            object: oid(2),
            target: oid(1),
        },
    );
}

// REQ-ZONE-001 item 3: a three-permanent cycle is rejected.
#[test]
fn req_zone_001_attach_cycle_length_3() {
    let mut p = holding(&[1, 2, 3]);
    p.attach(oid(1), on(2)).unwrap();
    p.attach(oid(2), on(3)).unwrap();
    assert_attach_err(
        &mut p,
        3,
        on(1),
        PermanentError::AttachmentCycle {
            object: oid(3),
            target: oid(1),
        },
    );
    assert_attach_err(
        &mut p,
        3,
        on(2),
        PermanentError::AttachmentCycle {
            object: oid(3),
            target: oid(2),
        },
    );
    // Re-pointing a chain member back into its own chain.
    assert_attach_err(
        &mut p,
        2,
        on(1),
        PermanentError::AttachmentCycle {
            object: oid(2),
            target: oid(1),
        },
    );
}

// REQ-ZONE-001 item 3: a fifty-permanent cycle is rejected at every closing point.
#[test]
fn req_zone_001_attach_cycle_length_50() {
    let ids: Vec<u64> = (1..=50).collect();
    let mut p = holding(&ids);
    for i in 1..50 {
        p.attach(oid(i), on(i + 1)).unwrap();
    }
    for j in 1..50 {
        assert_attach_err(
            &mut p,
            50,
            on(j),
            PermanentError::AttachmentCycle {
                object: oid(50),
                target: oid(j),
            },
        );
    }
    // 50 may attach to a player, ending the chain, and the chain stays acyclic.
    p.attach(oid(50), AttachTarget::Player(pid(2))).unwrap();
    assert_eq!(p.attached_to(oid(1)), Some(on(2)));
    assert_eq!(p.attached_to(oid(50)), Some(AttachTarget::Player(pid(2))));
}

// REQ-ZONE-001 item 3; CR 701.3b: attaching to the current target is Ok and changes nothing.
#[test]
fn req_zone_001_reattach_same_target_is_noop() {
    let mut p = holding(&[1, 2]);
    p.attach(oid(1), on(2)).unwrap();
    let before = p.clone();
    assert_eq!(p.attach(oid(1), on(2)), Ok(()));
    assert_eq!(p, before);
    p.attach(oid(2), AttachTarget::Player(pid(4))).unwrap();
    let before = p.clone();
    assert_eq!(p.attach(oid(2), AttachTarget::Player(pid(4))), Ok(()));
    assert_eq!(p, before);
}

// REQ-ZONE-001 item 3: a new attach replaces the old link, across target kinds.
#[test]
fn req_zone_001_reattach_replaces() {
    let mut p = holding(&[1, 2, 3]);
    p.attach(oid(1), on(2)).unwrap();
    p.attach(oid(1), on(3)).unwrap();
    assert_eq!(p.attached_to(oid(1)), Some(on(3)));
    assert_eq!(p.attachments_of(on(2)), Vec::<ObjectId>::new());
    assert_eq!(p.attachments_of(on(3)), vec![oid(1)]);
    p.attach(oid(1), AttachTarget::Player(pid(3))).unwrap();
    assert_eq!(p.attached_to(oid(1)), Some(AttachTarget::Player(pid(3))));
    assert!(p.attachments_of(on(3)).is_empty());
    assert_eq!(p.attachments_of(AttachTarget::Player(pid(3))), vec![oid(1)]);
    // Player(3) and Permanent(3) are distinct targets.
    p.attach(oid(2), on(3)).unwrap();
    assert_eq!(p.attachments_of(on(3)), vec![oid(2)]);
    assert_eq!(p.attachments_of(AttachTarget::Player(pid(3))), vec![oid(1)]);
    p.attach(oid(1), on(2)).unwrap();
    assert_eq!(p.attached_to(oid(1)), Some(on(2)));
    assert_eq!(p.get(oid(1)).unwrap().attached_to(), Some(on(2)));
}

// REQ-ZONE-001 item 3: detach returns the old target and is None when unattached.
#[test]
fn req_zone_001_detach() {
    let mut p = holding(&[1, 2]);
    assert_eq!(p.detach(oid(1)), Ok(None));
    p.attach(oid(1), on(2)).unwrap();
    assert_eq!(p.detach(oid(1)), Ok(Some(on(2))));
    assert_eq!(p.attached_to(oid(1)), None);
    assert!(p.attachments_of(on(2)).is_empty());
    assert_eq!(p.detach(oid(1)), Ok(None));
    p.attach(oid(1), AttachTarget::Player(pid(2))).unwrap();
    assert_eq!(p.detach(oid(1)), Ok(Some(AttachTarget::Player(pid(2)))));
    assert_eq!(p, holding(&[1, 2]));
    let before = p.clone();
    assert_eq!(p.detach(oid(3)), Err(PermanentError::UnknownObject(oid(3))));
    assert_eq!(p, before);
    // After detaching, the former cycle direction is allowed.
    p.attach(oid(1), on(2)).unwrap();
    p.detach(oid(1)).unwrap();
    assert_eq!(p.attach(oid(2), on(1)), Ok(()));
}

// REQ-ZONE-001 item 3; CR 701.3d: when a permanent leaves, everything attached to it becomes
// unattached; its own link goes with it.
#[test]
fn req_zone_001_leave_clears_dependents() {
    let mut p = holding(&[1, 2, 3, 4, 5]);
    p.attach(oid(1), on(5)).unwrap();
    p.attach(oid(2), on(5)).unwrap();
    p.attach(oid(3), on(2)).unwrap();
    p.attach(oid(5), on(4)).unwrap();
    let left = p.leave(oid(5)).unwrap();
    assert_eq!(left.attached_to(), Some(on(4)));
    assert_eq!(p.attached_to(oid(1)), None);
    assert_eq!(p.attached_to(oid(2)), None);
    assert_eq!(p.attached_to(oid(3)), Some(on(2)));
    assert_eq!(p.attached_to(oid(5)), None);
    assert!(p.attachments_of(on(5)).is_empty());
    assert!(p.attachments_of(on(4)).is_empty());
    // Re-entering gives a fresh, unattached permanent (CR 400.7) with no dependents.
    p.enter(oid(5), pid(1), 1).unwrap();
    assert_eq!(p.attached_to(oid(5)), None);
    assert!(p.attachments_of(on(5)).is_empty());
}

// REQ-ZONE-001 item 3: attachments_of lists exactly the matching ids in ascending order.
#[test]
fn req_zone_001_attachments_of_ascending() {
    let mut p = holding(&[1, 3, 7, 20, 40, 100]);
    for i in [100u64, 7, 40, 3, 20] {
        p.attach(oid(i), on(1)).unwrap();
    }
    assert_eq!(
        p.attachments_of(on(1)),
        vec![oid(3), oid(7), oid(20), oid(40), oid(100)]
    );
    assert!(p.attachments_of(on(3)).is_empty());
    assert!(p.attachments_of(on(999)).is_empty());
    assert!(p.attachments_of(AttachTarget::Player(pid(1))).is_empty());
    p.detach(oid(20)).unwrap();
    assert_eq!(
        p.attachments_of(on(1)),
        vec![oid(3), oid(7), oid(40), oid(100)]
    );
}

// REQ-ZONE-001 item 3: player targets are always accepted for a held id, any player number.
#[test]
fn req_zone_001_player_targets() {
    let mut p = holding(&[1, 2, 3]);
    for (i, player) in [(3u64, u64::MAX), (1, 0), (2, 0)] {
        p.attach(oid(i), AttachTarget::Player(pid(player))).unwrap();
    }
    assert_eq!(
        p.attachments_of(AttachTarget::Player(pid(0))),
        vec![oid(1), oid(2)]
    );
    assert_eq!(
        p.attachments_of(AttachTarget::Player(pid(u64::MAX))),
        vec![oid(3)]
    );
    // A chain may end at a player.
    p.attach(oid(1), on(2)).unwrap();
    assert_eq!(p.attached_to(oid(1)), Some(on(2)));
    assert_eq!(p.attached_to(oid(2)), Some(AttachTarget::Player(pid(0))));
}

// REQ-ZONE-001 item 3: AttachTarget ordering and equality are total and kind-sensitive.
#[test]
fn req_zone_001_attach_target_derives() {
    let a = AttachTarget::Permanent(oid(1));
    let b = AttachTarget::Player(pid(1));
    assert_ne!(a, b);
    assert_ne!(a.cmp(&b), std::cmp::Ordering::Equal);
    assert_eq!(a.cmp(&b), b.cmp(&a).reverse());
    let c = a;
    assert_eq!(c, a);
    assert!(on(1) < on(2));
}

/// The reference model of attachments used by the property test.
struct Model {
    held: BTreeSet<ObjectId>,
    links: BTreeMap<ObjectId, AttachTarget>,
}

impl Model {
    fn expected_attach(&self, id: ObjectId, target: AttachTarget) -> Result<(), PermanentError> {
        if !self.held.contains(&id) {
            return Err(PermanentError::UnknownObject(id));
        }
        if let AttachTarget::Permanent(x) = target {
            if !self.held.contains(&x) {
                return Err(PermanentError::UnknownObject(x));
            }
            if x == id {
                return Err(PermanentError::SelfAttach(id));
            }
            let mut cur = x;
            let mut steps = 0;
            while let Some(AttachTarget::Permanent(next)) = self.links.get(&cur) {
                if *next == id {
                    return Err(PermanentError::AttachmentCycle {
                        object: id,
                        target: x,
                    });
                }
                cur = *next;
                steps += 1;
                assert!(steps <= self.held.len(), "model itself cycled");
            }
        }
        Ok(())
    }

    fn leave(&mut self, id: ObjectId) {
        self.held.remove(&id);
        self.links.remove(&id);
        self.links.retain(|_, t| *t != AttachTarget::Permanent(id));
    }
}

fn pick(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_bounded(n).unwrap()
}

/// Checks the container against the model and that every chain ends without revisiting.
fn check(p: &Permanents, m: &Model, ctx: &str) {
    let ids: Vec<ObjectId> = m.held.iter().copied().collect();
    assert_eq!(p.ids(), ids, "{ctx}: id set");
    assert_eq!(p.len(), ids.len(), "{ctx}: len");
    for &id in &ids {
        // Chain walk with a step bound of len()+1.
        let mut seen = BTreeSet::new();
        seen.insert(id);
        let mut cur = id;
        let mut steps = 0usize;
        loop {
            match p.attached_to(cur) {
                None | Some(AttachTarget::Player(_)) => break,
                Some(AttachTarget::Permanent(next)) => {
                    assert!(
                        seen.insert(next),
                        "{ctx}: chain from {id:?} revisits {next:?}"
                    );
                    assert!(p.get(next).is_some(), "{ctx}: link to unheld {next:?}");
                    cur = next;
                    steps += 1;
                    assert!(steps <= p.len() + 1, "{ctx}: chain from {id:?} too long");
                }
            }
        }
        assert_eq!(
            p.attached_to(id),
            m.links.get(&id).copied(),
            "{ctx}: link of {id:?}"
        );
        assert_eq!(
            p.get(id).unwrap().attached_to(),
            m.links.get(&id).copied(),
            "{ctx}: Permanent::attached_to of {id:?}"
        );
    }
    let mut targets: BTreeSet<AttachTarget> = BTreeSet::new();
    for raw in 0..=15u64 {
        targets.insert(AttachTarget::Permanent(oid(raw)));
    }
    for raw in 0..=3u64 {
        targets.insert(AttachTarget::Player(pid(raw)));
    }
    for t in targets {
        let expected: Vec<ObjectId> = m
            .links
            .iter()
            .filter(|(_, lt)| **lt == t)
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(p.attachments_of(t), expected, "{ctx}: attachments_of {t:?}");
    }
}

// REQ-ZONE-001 item 3 (acceptance criterion: attachments can never form a cycle).
#[test]
fn property_attachments_never_cycle() {
    for seed in 0..200u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(0xA77A_0000 + seed);
        let mut p = Permanents::new();
        let mut m = Model {
            held: BTreeSet::new(),
            links: BTreeMap::new(),
        };
        let initial = 8 + pick(&mut rng, 5);
        for i in 1..=initial {
            p.enter(oid(i), pid(1 + pick(&mut rng, 3)), pick(&mut rng, 10))
                .unwrap();
            m.held.insert(oid(i));
        }
        check(&p, &m, &format!("seed {seed} init"));
        for step in 0..300u32 {
            let ctx = format!("seed {seed} step {step}");
            // Ids 1..=14 so that some operations name unheld ids.
            let id = oid(1 + pick(&mut rng, 14));
            match pick(&mut rng, 10) {
                0 | 1 => {
                    let r = p.enter(id, pid(1 + pick(&mut rng, 3)), pick(&mut rng, 10));
                    if m.held.contains(&id) {
                        assert_eq!(r, Err(PermanentError::AlreadyPresent(id)), "{ctx}");
                    } else {
                        assert_eq!(r, Ok(()), "{ctx}");
                        m.held.insert(id);
                    }
                }
                2 => {
                    let before = p.clone();
                    let r = p.leave(id);
                    if m.held.contains(&id) {
                        let left = r.unwrap();
                        assert_eq!(left.id(), id, "{ctx}");
                        assert_eq!(left.attached_to(), m.links.get(&id).copied(), "{ctx}");
                        m.leave(id);
                    } else {
                        assert_eq!(
                            r.map(|x| x.id()),
                            Err(PermanentError::UnknownObject(id)),
                            "{ctx}"
                        );
                        assert_eq!(p, before, "{ctx}");
                    }
                }
                3..=7 => {
                    let target = if pick(&mut rng, 4) == 0 {
                        AttachTarget::Player(pid(pick(&mut rng, 4)))
                    } else {
                        AttachTarget::Permanent(oid(1 + pick(&mut rng, 14)))
                    };
                    let expected = m.expected_attach(id, target);
                    let before = p.clone();
                    let r = p.attach(id, target);
                    assert_eq!(r, expected, "{ctx}: attach {id:?} -> {target:?}");
                    if r.is_ok() {
                        m.links.insert(id, target);
                    } else {
                        assert_eq!(p, before, "{ctx}: Err changed container");
                    }
                }
                8 => {
                    let r = p.detach(id);
                    if m.held.contains(&id) {
                        assert_eq!(r, Ok(m.links.remove(&id)), "{ctx}");
                    } else {
                        assert_eq!(r, Err(PermanentError::UnknownObject(id)), "{ctx}");
                    }
                }
                _ => {
                    let r = p.set_controller(id, pid(1 + pick(&mut rng, 3)), pick(&mut rng, 20));
                    if m.held.contains(&id) {
                        assert_eq!(r, Ok(()), "{ctx}");
                    } else {
                        assert_eq!(r, Err(PermanentError::UnknownObject(id)), "{ctx}");
                    }
                }
            }
            check(&p, &m, &ctx);
        }
    }
}
