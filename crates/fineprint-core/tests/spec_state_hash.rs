// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::state`. Black-box: written from the work-item brief only.
//!
//! The canonical state hash: the frozen Merkle layout, single-component sensitivity and
//! first-divergence location (REQ-NFR-006, REQ-CORE-001, REQ-CORE-006).

use std::collections::BTreeMap;

use fineprint_base::encode::{encode_to_vec, put_len};
use fineprint_base::hash::{ComponentPath, Label, MerkleTree, diff_first_divergence};
use fineprint_base::ids::PlayerId;
use fineprint_core::state::{DesignationValue, GameState};

fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn n(s: &str) -> Label {
    Label::name(s)
}

fn path(labels: Vec<Label>) -> ComponentPath {
    ComponentPath::new(labels)
}

fn put(t: &mut MerkleTree, labels: Vec<Label>, payload: Vec<u8>) {
    t.set_leaf(&path(labels), payload).unwrap();
}

/// Independent encoding of a designation map, written from the documented byte layout.
fn enc_designations(m: &BTreeMap<String, DesignationValue>) -> Vec<u8> {
    let mut out = Vec::new();
    put_len(&mut out, m.len());
    for (k, v) in m {
        put_len(&mut out, k.len());
        out.extend(k.as_bytes());
        match v {
            DesignationValue::Flag => out.push(0x00),
            DesignationValue::Int(i) => {
                out.push(0x01);
                out.extend(i.to_le_bytes());
            }
            DesignationValue::Text(t) => {
                out.push(0x02);
                put_len(&mut out, t.len());
                out.extend(t.as_bytes());
            }
            _ => panic!("unexpected designation variant"),
        }
    }
    out
}

/// The frozen layout, rebuilt from the public getters only.
fn expected_tree(s: &GameState) -> MerkleTree {
    let mut t = MerkleTree::new();
    put(&mut t, vec![n("prng")], encode_to_vec(s.prng()));
    let ids = s.ids();
    put(
        &mut t,
        vec![n("ids"), n("card")],
        encode_to_vec(&ids.card.last_issued()),
    );
    put(
        &mut t,
        vec![n("ids"), n("object")],
        encode_to_vec(&ids.object.last_issued()),
    );
    put(
        &mut t,
        vec![n("ids"), n("ability_instance")],
        encode_to_vec(&ids.ability_instance.last_issued()),
    );
    put(
        &mut t,
        vec![n("ids"), n("effect")],
        encode_to_vec(&ids.effect.last_issued()),
    );
    put(
        &mut t,
        vec![n("ids"), n("timestamp")],
        encode_to_vec(&ids.timestamp.last_issued()),
    );
    put(
        &mut t,
        vec![n("ids"), n("link")],
        encode_to_vec(&ids.link.last_issued()),
    );
    put(
        &mut t,
        vec![n("ids"), n("player")],
        encode_to_vec(&ids.player.last_issued()),
    );
    put(
        &mut t,
        vec![n("turn"), n("number")],
        encode_to_vec(&s.turn().turn_number()),
    );
    put(
        &mut t,
        vec![n("turn"), n("active_player")],
        encode_to_vec(&s.turn().active_player()),
    );
    put(
        &mut t,
        vec![n("turn"), n("priority_player")],
        encode_to_vec(&s.turn().priority_player()),
    );
    put(
        &mut t,
        vec![n("turn"), n("order")],
        encode_to_vec(&s.turn_order().to_vec()),
    );
    for p in s.players() {
        let k = Label::index(p.id().get());
        put(
            &mut t,
            vec![n("players"), k.clone(), n("life")],
            encode_to_vec(&p.life()),
        );
        put(
            &mut t,
            vec![n("players"), k.clone(), n("counters")],
            encode_to_vec(p.counters()),
        );
        put(
            &mut t,
            vec![n("players"), k, n("designations")],
            enc_designations(p.designations()),
        );
    }
    put(&mut t, vec![n("effects")], vec![0, 0, 0, 0]);
    // Format version 2 (#183 FP-144): the timestamp ledger and the effect-to-ability links,
    // each an empty list here.
    put(&mut t, vec![n("timestamps")], vec![0, 0, 0, 0]);
    put(&mut t, vec![n("effect_generators")], vec![0, 0, 0, 0]);
    put(&mut t, vec![n("history")], vec![0, 0, 0, 0]);
    t
}

/// A state touching every component.
fn rich(seed: u64, players: usize) -> GameState {
    let mut s = GameState::new(seed, players, 20).unwrap();
    for id in 1..=players as u64 {
        let mut p = s.player_mut(pid(id)).unwrap();
        p.set_life(i64::MAX - id as i64);
        p.set_counter("test.counter_a", id as i64);
        p.set_counter("\u{4e2d}", i64::MAX);
        p.set_designation("test.designation_a", DesignationValue::Flag);
        p.set_designation("test.designation_b", DesignationValue::Int(-(id as i64)));
        p.set_designation(
            "test.designation_c",
            DesignationValue::Text("t\u{fc}".into()),
        );
    }
    s.prng_mut().next_u64();
    s.prng_mut().next_u64();
    s.issue_card_id().unwrap();
    s.issue_object_id().unwrap();
    s.issue_object_id().unwrap();
    s.issue_ability_instance_id().unwrap();
    s.issue_effect_id().unwrap();
    s.issue_timestamp().unwrap();
    s.issue_link_id().unwrap();
    s.set_turn_number(12);
    s.set_active_player(Some(pid(players as u64))).unwrap();
    s.set_priority_player(Some(pid(1))).unwrap();
    let mut order: Vec<PlayerId> = s.turn_order().to_vec();
    order.reverse();
    s.set_turn_order(order).unwrap();
    s
}

// REQ-NFR-006: the root equals an independently built Merkle tree of the frozen layout.
#[test]
fn root_matches_independent_layout() {
    let mut states = vec![
        GameState::new(42, 1, 20).unwrap(),
        GameState::new(42, 2, 20).unwrap(),
        GameState::new(42, 3, 20).unwrap(),
        rich(1, 1),
        rich(2, 2),
        rich(3, 3),
        rich(4, 8),
        GameState::new(9, 256, i64::MIN).unwrap(),
    ];
    let mut shuffled = GameState::new(8, 4, 1).unwrap();
    shuffled
        .set_turn_order(vec![pid(3), pid(1), pid(4), pid(2)])
        .unwrap();
    states.push(shuffled);
    for s in &states {
        let mut want = expected_tree(s);
        assert_eq!(s.state_hash(), want.root_digest());
        assert_eq!(s.state_hash(), want.full_root_digest());
        assert_eq!(s.merkle_tree().root_digest(), want.root_digest());
        assert_eq!(s.merkle_tree().full_root_digest(), want.full_root_digest());
        assert_eq!(diff_first_divergence(&s.merkle_tree(), &want), None);
    }
}

// REQ-NFR-006, REQ-CORE-001: effects, timestamps, effect_generators and history leaves are
// four zero bytes at their paths.
#[test]
fn effects_and_history_leaves_hash_as_zero_length_lists() {
    let s = rich(5, 2);
    let t = s.merkle_tree();
    assert_eq!(
        t.leaf_payload(&path(vec![n("effects")])),
        Some(&[0u8; 4][..])
    );
    assert_eq!(
        t.leaf_payload(&path(vec![n("timestamps")])),
        Some(&[0u8; 4][..])
    );
    assert_eq!(
        t.leaf_payload(&path(vec![n("effect_generators")])),
        Some(&[0u8; 4][..])
    );
    assert_eq!(
        t.leaf_payload(&path(vec![n("history")])),
        Some(&[0u8; 4][..])
    );
}

/// Number of single-component mutations in [`mutate`].
const MUTATIONS: usize = 20;

fn base() -> GameState {
    let mut s = GameState::new(5, 3, 20).unwrap();
    {
        let mut p = s.player_mut(pid(2)).unwrap();
        p.set_counter("test.counter_a", 3);
        p.set_designation("test.designation_a", DesignationValue::Flag);
        p.set_designation("test.designation_b", DesignationValue::Int(5));
    }
    s.issue_card_id().unwrap();
    s.issue_card_id().unwrap();
    s
}

/// Applies mutation `k` and returns the exact leaf path that must be the first divergence.
fn mutate(k: usize, s: &mut GameState) -> ComponentPath {
    let pl = |id: u64, leaf: &str| path(vec![n("players"), Label::index(id), n(leaf)]);
    match k {
        0 => {
            s.player_mut(pid(2)).unwrap().set_life(99);
            pl(2, "life")
        }
        1 => {
            s.player_mut(pid(2))
                .unwrap()
                .set_counter("test.counter_c", 1);
            pl(2, "counters")
        }
        2 => {
            s.player_mut(pid(2))
                .unwrap()
                .set_counter("test.counter_a", 8);
            pl(2, "counters")
        }
        3 => {
            s.player_mut(pid(2))
                .unwrap()
                .set_counter("test.counter_a", 0);
            pl(2, "counters")
        }
        4 => {
            s.player_mut(pid(2))
                .unwrap()
                .set_designation("test.designation_c", DesignationValue::Text("x".into()));
            pl(2, "designations")
        }
        5 => {
            s.player_mut(pid(2))
                .unwrap()
                .set_designation("test.designation_a", DesignationValue::Int(1));
            pl(2, "designations")
        }
        6 => {
            s.player_mut(pid(2))
                .unwrap()
                .set_designation("test.designation_b", DesignationValue::Int(6));
            pl(2, "designations")
        }
        7 => {
            s.player_mut(pid(2))
                .unwrap()
                .clear_designation("test.designation_a");
            pl(2, "designations")
        }
        8 => {
            s.prng_mut().next_u64();
            path(vec![n("prng")])
        }
        9 => {
            s.issue_card_id().unwrap();
            path(vec![n("ids"), n("card")])
        }
        10 => {
            s.issue_object_id().unwrap();
            path(vec![n("ids"), n("object")])
        }
        11 => {
            s.issue_ability_instance_id().unwrap();
            path(vec![n("ids"), n("ability_instance")])
        }
        12 => {
            s.issue_effect_id().unwrap();
            path(vec![n("ids"), n("effect")])
        }
        13 => {
            s.issue_timestamp().unwrap();
            path(vec![n("ids"), n("timestamp")])
        }
        14 => {
            s.issue_link_id().unwrap();
            path(vec![n("ids"), n("link")])
        }
        15 => {
            s.set_turn_number(9);
            path(vec![n("turn"), n("number")])
        }
        16 => {
            s.set_active_player(Some(pid(2))).unwrap();
            path(vec![n("turn"), n("active_player")])
        }
        17 => {
            s.set_priority_player(Some(pid(3))).unwrap();
            path(vec![n("turn"), n("priority_player")])
        }
        18 => {
            s.set_turn_order(vec![pid(2), pid(3), pid(1)]).unwrap();
            path(vec![n("turn"), n("order")])
        }
        19 => {
            s.player_mut(pid(1)).unwrap().set_life(i64::MIN);
            pl(1, "life")
        }
        _ => unreachable!(),
    }
}

// REQ-NFR-006, REQ-CORE-001: each single-component change moves the hash and is located exactly.
#[test]
fn single_component_sensitivity_and_divergence_path() {
    let a = base();
    assert_eq!(
        diff_first_divergence(&a.merkle_tree(), &a.merkle_tree()),
        None
    );
    let mut seen = std::collections::BTreeSet::new();
    for k in 0..MUTATIONS {
        let mut b = a.clone();
        let want = mutate(k, &mut b);
        assert_ne!(a.state_hash(), b.state_hash(), "mutation {k}");
        assert_ne!(a, b, "mutation {k}");
        assert_eq!(
            diff_first_divergence(&a.merkle_tree(), &b.merkle_tree()),
            Some(want.clone()),
            "mutation {k}"
        );
        assert_eq!(
            diff_first_divergence(&b.merkle_tree(), &a.merkle_tree()),
            Some(want),
            "mutation {k} (reversed)"
        );
        // The mutated state still matches the independent layout.
        assert_eq!(
            b.state_hash(),
            expected_tree(&b).root_digest(),
            "mutation {k}"
        );
        assert!(
            seen.insert(b.state_hash().to_hex()),
            "mutation {k} collides"
        );
    }
}

// REQ-NFR-006, REQ-NFR-010: adding a player changes the hash and is located.
#[test]
fn add_player_changes_hash_and_is_located() {
    for count in [1usize, 2, 3, 8] {
        let a = GameState::new(3, count, 20).unwrap();
        let mut b = a.clone();
        b.add_player(20).unwrap();
        assert_ne!(a.state_hash(), b.state_hash());
        let d = diff_first_divergence(&a.merkle_tree(), &b.merkle_tree())
            .expect("an added player is a divergence");
        let first = d.labels()[0].clone();
        assert!(
            first == n("players") || first == n("ids") || first == n("turn"),
            "{d:?}"
        );
        assert_eq!(b.state_hash(), expected_tree(&b).root_digest());
    }
}

// REQ-NFR-006: the hash depends only on values, not on how the state was built.
#[test]
fn equal_states_have_equal_hashes() {
    // Different mutation orders.
    let mut a = GameState::new(1, 2, 20).unwrap();
    let mut b = GameState::new(1, 2, 20).unwrap();
    {
        let mut p = a.player_mut(pid(1)).unwrap();
        p.set_counter("test.counter_a", 1);
        p.set_counter("test.counter_b", 2);
        p.set_designation("test.designation_a", DesignationValue::Flag);
        p.set_designation("test.designation_b", DesignationValue::Int(2));
    }
    {
        let mut p = b.player_mut(pid(1)).unwrap();
        p.set_designation("test.designation_b", DesignationValue::Int(2));
        p.set_counter("test.counter_b", 2);
        p.set_designation("test.designation_a", DesignationValue::Flag);
        p.set_counter("test.counter_a", 1);
    }
    a.issue_card_id().unwrap();
    a.set_turn_number(4);
    b.set_turn_number(4);
    b.issue_card_id().unwrap();
    assert_eq!(a, b);
    assert_eq!(a.state_hash(), b.state_hash());
    // Values reached through an intermediate change.
    let mut c = b.clone();
    c.player_mut(pid(1)).unwrap().set_life(5);
    c.player_mut(pid(1)).unwrap().set_life(20);
    c.player_mut(pid(2)).unwrap().set_counter("test.tmp", 9);
    c.player_mut(pid(2)).unwrap().set_counter("test.tmp", 0);
    c.player_mut(pid(2))
        .unwrap()
        .set_designation("test.tmp", DesignationValue::Flag);
    c.player_mut(pid(2)).unwrap().clear_designation("test.tmp");
    assert_eq!(c, b);
    assert_eq!(c.state_hash(), b.state_hash());
    // Clone and serialize round trip.
    assert_eq!(b.clone().state_hash(), b.state_hash());
    let d = GameState::deserialize(&b.serialize().unwrap()).unwrap();
    assert_eq!(d.state_hash(), b.state_hash());
    // new(.., 1, ..) plus add_player equals new(.., 2, ..).
    let mut grown = GameState::new(7, 1, 20).unwrap();
    grown.add_player(20).unwrap();
    let direct = GameState::new(7, 2, 20).unwrap();
    assert_eq!(grown, direct);
    assert_eq!(grown.state_hash(), direct.state_hash());
    // Fresh builds agree.
    assert_eq!(
        GameState::new(42, 2, 20).unwrap().state_hash(),
        GameState::new(42, 2, 20).unwrap().state_hash()
    );
    assert_ne!(
        GameState::new(42, 2, 20).unwrap().state_hash(),
        GameState::new(43, 2, 20).unwrap().state_hash()
    );
}

// REQ-NFR-006: the digest of a player subtree depends on that player alone.
#[test]
fn player_subtree_digest_is_local() {
    let a = GameState::new(1, 2, 20).unwrap();
    let mut b = GameState::new(2, 3, 20).unwrap();
    b.prng_mut().next_u64();
    b.issue_card_id().unwrap();
    b.set_turn_number(3);
    b.player_mut(pid(2)).unwrap().set_life(1);
    b.player_mut(pid(3))
        .unwrap()
        .set_counter("test.counter_a", 4);
    assert_ne!(a.state_hash(), b.state_hash());
    let p1 = path(vec![n("players"), Label::index(1)]);
    let p2 = path(vec![n("players"), Label::index(2)]);
    let d1a = a.merkle_tree().digest_at(&p1);
    assert!(d1a.is_some());
    assert_eq!(d1a, b.merkle_tree().digest_at(&p1));
    assert_ne!(
        a.merkle_tree().digest_at(&p2),
        b.merkle_tree().digest_at(&p2)
    );
    assert_eq!(
        a.merkle_tree()
            .digest_at(&path(vec![n("players"), Label::index(9)])),
        None
    );
}

// REQ-CORE-001, REQ-NFR-006: a counter of zero is the same as never set.
#[test]
fn counter_zero_equals_never_set() {
    let a = GameState::new(1, 2, 20).unwrap();
    let mut b = a.clone();
    b.player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_a", 0);
    assert_eq!(a, b);
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(a.serialize().unwrap(), b.serialize().unwrap());
    b.player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_a", 2);
    b.player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_a", 0);
    assert_eq!(a, b);
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(a.serialize().unwrap(), b.serialize().unwrap());
}

// REQ-NFR-006: digest format; the Implementer pins literal golden digests.
#[test]
fn state_hash_is_sha256_hex() {
    let h = GameState::new(42, 2, 20).unwrap().state_hash().to_hex();
    assert_eq!(h.len(), 64);
    assert!(
        h.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    );
    assert_eq!(h, GameState::new(42, 2, 20).unwrap().state_hash().to_hex());
}

// REQ-NFR-006: MessagePack is transport only; changing the transport bytes of a state
// without changing its value cannot happen, and equal states agree on hash and bytes.
#[test]
fn hash_is_not_a_function_of_transport_bytes() {
    let a = rich(11, 3);
    let b = GameState::deserialize(&a.serialize().unwrap()).unwrap();
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(
        a.merkle_tree().full_root_digest(),
        b.merkle_tree().full_root_digest()
    );
}
