// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit and property tests of the game-state model (REQ-CORE-001, REQ-CORE-002, REQ-NFR-006,
//! REQ-NFR-010, R12). Synthetic data only; deterministic SplitMix64 harness with literal seeds.

use std::collections::BTreeMap;

use fineprint_base::encode::{CanonicalEncode, encode_to_vec, put_bytes, put_len};
use fineprint_base::hash::{
    ComponentPath, Digest, Label, diff_first_divergence, leaf_digest, node_digest,
};
use fineprint_base::ids::{IdGenerators, PlayerId};
use fineprint_base::prng::{ALGORITHM_ID, Pcg64Dxsm, SplitMix64};

use super::effects_tests::populate_layers;
use super::wire::{DesignationWire, PlayerWire, StateWire};
use super::*;
use crate::layers::{CounterHolder, LayerModification, StampReason};

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

const I64_EDGES: [i64; 5] = [i64::MIN, -1, 0, 1, i64::MAX];
const AMOUNT_EDGES: [i64; 6] = [i64::MIN, -1, 0, 1, 2, i64::MAX];
const KINDS: [&str; 6] = [
    "",
    "test.counter_a",
    "test.counter_b",
    "t\u{e9}st.\u{fc}",
    "\u{8a08}\u{6570}",
    "test.counter_c",
];

fn below(rng: &mut SplitMix64, n: u64) -> u64 {
    rng.next_u64() % n
}

fn idx(rng: &mut SplitMix64, n: usize) -> usize {
    usize::try_from(below(rng, n as u64)).unwrap()
}

fn any_i64(rng: &mut SplitMix64) -> i64 {
    if below(rng, 3) == 0 {
        I64_EDGES[idx(rng, I64_EDGES.len())]
    } else {
        rng.next_u64().cast_signed()
    }
}

fn any_amount(rng: &mut SplitMix64) -> i64 {
    match below(rng, 4) {
        0 => AMOUNT_EDGES[idx(rng, AMOUNT_EDGES.len())],
        1 => rng.next_u64().cast_signed(),
        _ => 1 + below(rng, 20).cast_signed(),
    }
}

fn any_kind(rng: &mut SplitMix64) -> String {
    if below(rng, 4) == 0 {
        format!("test.k{}", below(rng, 1000))
    } else {
        KINDS[idx(rng, KINDS.len())].to_owned()
    }
}

fn any_designation(rng: &mut SplitMix64) -> DesignationValue {
    match below(rng, 3) {
        0 => DesignationValue::Flag,
        1 => DesignationValue::Int(any_i64(rng)),
        _ => DesignationValue::Text(any_kind(rng)),
    }
}

fn shuffle<T>(rng: &mut SplitMix64, items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let j = idx(rng, i + 1);
        items.swap(i, j);
    }
}

/// A random valid state with `count` players.
fn gen_state_with(rng: &mut SplitMix64, count: usize) -> GameState {
    let mut s = GameState::new(rng.next_u64(), count, any_i64(rng)).unwrap();
    let ids: Vec<PlayerId> = s.turn_order().to_vec();
    for &id in &ids {
        let mut p = s.player_mut(id).unwrap();
        p.set_life(any_i64(rng));
        for _ in 0..below(rng, 4) {
            let k = any_kind(rng);
            p.set_counter(&k, any_amount(rng));
        }
        for _ in 0..below(rng, 4) {
            let k = any_kind(rng);
            p.set_designation(&k, any_designation(rng));
        }
    }
    for _ in 0..below(rng, 5) {
        s.prng_mut().next_u64();
    }
    for _ in 0..below(rng, 12) {
        match below(rng, 6) {
            0 => ok(s.issue_card_id()),
            1 => ok(s.issue_object_id()),
            2 => ok(s.issue_ability_instance_id()),
            3 => ok(s.issue_effect_id()),
            4 => ok(s.issue_timestamp()),
            _ => ok(s.issue_link_id()),
        }
    }
    s.set_turn_number(if below(rng, 4) == 0 {
        u64::MAX
    } else {
        below(rng, 100)
    });
    let some_player = |rng: &mut SplitMix64| {
        if below(rng, 3) == 0 {
            None
        } else {
            Some(ids[idx(rng, ids.len())])
        }
    };
    let a = some_player(rng);
    let p = some_player(rng);
    s.set_active_player(a).unwrap();
    s.set_priority_player(p).unwrap();
    let mut order = ids.clone();
    shuffle(rng, &mut order);
    s.set_turn_order(order).unwrap();
    // The layer components (REQ-LAYER-001, REQ-CORE-002): effects, links and timestamps.
    let steps = below(rng, 12);
    populate_layers(rng, &mut s, steps);
    s
}

/// A random valid state with 1..=MAX_PLAYERS players (mostly small).
fn gen_state(rng: &mut SplitMix64) -> GameState {
    let count = if below(rng, 32) == 0 {
        1 + idx(rng, MAX_PLAYERS)
    } else {
        1 + idx(rng, 8)
    };
    gen_state_with(rng, count)
}

/// Unwraps a successful mutator result, discarding the issued value.
fn ok<T>(r: Result<T, StateError>) {
    r.unwrap();
}

fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn path(labels: &[Label]) -> ComponentPath {
    ComponentPath::new(labels.to_vec())
}

fn n(s: &str) -> Label {
    Label::name(s)
}

fn wire_of(s: &GameState) -> StateWire {
    StateWire::from_bytes(&s.serialize().unwrap()).unwrap()
}

fn decode_wire(w: &StateWire) -> Result<GameState, StateError> {
    GameState::deserialize(&w.to_bytes().unwrap())
}

fn assert_decode_err(r: Result<GameState, StateError>) {
    match r {
        Err(StateError::Decode(_)) => {}
        other => panic!("expected Decode error, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// 1. Round trip (REQ-CORE-001)
// ---------------------------------------------------------------------------------------------

fn check_round_trip(s: &GameState) {
    let b = s.serialize().unwrap();
    assert_eq!(b, s.serialize().unwrap(), "byte-deterministic");
    let back = GameState::deserialize(&b).unwrap();
    assert_eq!(&back, s);
    assert_eq!(back.state_hash(), s.state_hash());
    assert_eq!(back.serialize().unwrap(), b);
}

// REQ-CORE-001, REQ-NFR-006: property, deserialize(serialize(s)) == s with an identical hash;
// serialization is byte-deterministic and serialize(deserialize(b)) == b.
#[test]
fn prop_round_trip_identical_hash() {
    let mut rng = SplitMix64::new(0x0018_0001);
    for _ in 0..2_000 {
        check_round_trip(&gen_state(&mut rng));
    }
    // Fixed edges: the smallest and largest player counts.
    check_round_trip(&gen_state_with(&mut rng, 1));
    check_round_trip(&gen_state_with(&mut rng, MAX_PLAYERS));
    check_round_trip(&GameState::new(0, 1, i64::MIN).unwrap());
    check_round_trip(&GameState::new(u64::MAX, 2, i64::MAX).unwrap());
}

// REQ-CORE-001: the wire shape has exactly the specified top-level members, PRNG words as
// [hi, lo], and None round-trips.
#[test]
fn wire_shape_members() {
    let s = GameState::new(7, 2, 20).unwrap();
    let b = s.serialize().unwrap();
    let top: BTreeMap<String, serde::de::IgnoredAny> = rmp_serde::from_slice(&b).unwrap();
    let keys: Vec<&str> = top.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "effect_generators",
            "effects",
            "format_version",
            "history",
            "ids",
            "players",
            "prng",
            "prng_algorithm",
            "timestamps",
            "turn"
        ]
    );
    let w = wire_of(&s);
    assert_eq!(w.format_version, STATE_FORMAT_VERSION);
    assert_eq!(w.prng_algorithm, ALGORITHM_ID);
    let (st, inc) = s.prng().raw();
    assert_eq!(
        (u128::from(w.prng.state[0]) << 64) | u128::from(w.prng.state[1]),
        st
    );
    assert_eq!(
        (u128::from(w.prng.increment[0]) << 64) | u128::from(w.prng.increment[1]),
        inc
    );
    assert_eq!(w.turn.active_player, None);
    assert_eq!(w.turn.priority_player, None);
    assert_eq!(w.turn.order, vec![1, 2]);
    assert_eq!(w.players.iter().map(|p| p.id).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(w.ids.player, 2);
    let mut s2 = s.clone();
    s2.player_mut(pid(1))
        .unwrap()
        .set_designation("test.d", DesignationValue::Flag);
    s2.player_mut(pid(2))
        .unwrap()
        .set_designation("test.e", DesignationValue::Int(-3));
    s2.player_mut(pid(2))
        .unwrap()
        .set_designation("test.f", DesignationValue::Text("x".into()));
    let w2 = wire_of(&s2);
    assert_eq!(w2.players[0].designations["test.d"], DesignationWire::Flag);
    assert_eq!(
        w2.players[1].designations["test.e"],
        DesignationWire::Int(-3)
    );
    assert_eq!(
        w2.players[1].designations["test.f"],
        DesignationWire::Text("x".into())
    );
}

// ---------------------------------------------------------------------------------------------
// 2. Snapshot independence (REQ-CORE-001)
// ---------------------------------------------------------------------------------------------

type Mutator = fn(&mut GameState);

fn mutators() -> Vec<(&'static str, Mutator)> {
    vec![
        ("set_life", |s| {
            let mut p = s.player_mut(pid(1)).unwrap();
            let l = p.life();
            p.set_life(l.wrapping_add(1));
        }),
        ("set_counter", |s| {
            let mut p = s.player_mut(pid(1)).unwrap();
            let c = p.counter("test.counter_z");
            p.set_counter("test.counter_z", c + 1);
        }),
        ("set_designation", |s| {
            s.player_mut(pid(1))
                .unwrap()
                .set_designation("test.d_new", DesignationValue::Int(5));
        }),
        ("clear_designation", |s| {
            let mut p = s.player_mut(pid(1)).unwrap();
            p.set_designation("test.d_clear", DesignationValue::Flag);
            assert_eq!(
                p.clear_designation("test.d_clear"),
                Some(DesignationValue::Flag)
            );
            p.set_designation("test.d_mark", DesignationValue::Flag);
        }),
        ("prng", |s| {
            s.prng_mut().next_u64();
        }),
        ("card", |s| ok(s.issue_card_id())),
        ("object", |s| ok(s.issue_object_id())),
        ("ability_instance", |s| {
            ok(s.issue_ability_instance_id());
        }),
        ("effect", |s| ok(s.issue_effect_id())),
        ("timestamp", |s| ok(s.issue_timestamp())),
        ("link", |s| ok(s.issue_link_id())),
        ("add_player", |s| ok(s.add_player(3))),
        ("register_effect", |s| {
            let ts = s.stamp_resolved_effect().unwrap();
            let effect = crate::layers::ContinuousEffect {
                source: None,
                controller: pid(1),
                timestamp: ts,
                layer: fineprint_dsl::ability::Layer::L6,
                is_cda: false,
                origin: crate::layers::EffectOrigin::Resolved,
                affected: crate::layers::Affected::Objects(Default::default()),
                modification: LayerModification::RemoveAllAbilities,
            };
            ok(s.register_effect(effect, None));
        }),
        ("stamp_object", |s| {
            ok(s.stamp_object(
                fineprint_base::ids::ObjectId::new(900),
                StampReason::EntersZone,
            ));
        }),
        ("stamp_counters", |s| {
            let holder = CounterHolder::Player(pid(1));
            ok(s.stamp_counters(holder, "test.counter_s"));
        }),
        ("link_effect_generator", |s| {
            let id = s.issue_effect_id().unwrap();
            let g = GeneratingAbility::new(
                fineprint_base::ids::ObjectId::new(900),
                synthetic_ability("test.ability.link"),
            );
            ok(s.link_effect_generator(id, g));
        }),
        ("set_turn_order", |s| {
            let mut o = s.turn_order().to_vec();
            o.reverse();
            s.set_turn_order(o).unwrap();
        }),
        ("set_turn_number", |s| {
            let t = s.turn().turn_number();
            s.set_turn_number(t.wrapping_add(1));
        }),
        ("set_active_player", |s| {
            let next = if s.turn().active_player() == Some(pid(2)) {
                pid(1)
            } else {
                pid(2)
            };
            s.set_active_player(Some(next)).unwrap();
        }),
        ("set_priority_player", |s| {
            let next = if s.turn().priority_player() == Some(pid(2)) {
                pid(1)
            } else {
                pid(2)
            };
            s.set_priority_player(Some(next)).unwrap();
        }),
    ]
}

// REQ-CORE-001: mutating the original through every mutator never changes a snapshot, and
// mutating the snapshot never changes the original (by value, bytes and hash).
#[test]
fn snapshots_are_independent() {
    let mut rng = SplitMix64::new(0x0018_0002);
    for round in 0..40 {
        let base = gen_state_with(&mut rng, 2 + round % 4);
        for (name, m) in mutators() {
            // Original mutated, snapshot preserved.
            let mut s = base.clone();
            let snap = s.clone();
            let (bytes, hash) = (snap.serialize().unwrap(), snap.state_hash());
            m(&mut s);
            assert_ne!(s, snap, "{name} must change the state");
            assert_ne!(s.state_hash(), hash, "{name} must change the hash");
            assert_eq!(snap.serialize().unwrap(), bytes, "{name}");
            assert_eq!(snap.state_hash(), hash, "{name}");
            assert_eq!(snap, base, "{name}");
            // Snapshot mutated, original preserved.
            let orig = base.clone();
            let mut snap = orig.clone();
            m(&mut snap);
            assert_eq!(orig.serialize().unwrap(), bytes, "{name}");
            assert_eq!(orig.state_hash(), hash, "{name}");
            assert_eq!(orig, base, "{name}");
            assert_ne!(snap, orig, "{name}");
        }
    }
}

// REQ-CORE-001: structural sharing; a write un-shares only the player written.
#[test]
fn structural_sharing_copy_on_write() {
    let mut rng = SplitMix64::new(0x0018_0003);
    for count in [1_usize, 2, 3, 8, MAX_PLAYERS] {
        let s = gen_state_with(&mut rng, count);
        let mut c = s.clone();
        let ids: Vec<PlayerId> = s.players().map(PlayerState::id).collect();
        assert!(ids.iter().all(|&id| s.shares_player_with(&c, id)));
        let target = ids[idx(&mut rng, ids.len())];
        let old_life = s.player(target).unwrap().life();
        c.player_mut(target)
            .unwrap()
            .set_life(old_life.wrapping_sub(1));
        for &id in &ids {
            assert_eq!(s.shares_player_with(&c, id), id != target);
            assert_eq!(c.shares_player_with(&s, id), id != target);
        }
        assert_eq!(s.player(target).unwrap().life(), old_life);
        // A second write to the now-unshared player keeps the rest shared.
        c.player_mut(target)
            .unwrap()
            .set_counter("test.counter_a", 2);
        for &id in &ids {
            assert_eq!(s.shares_player_with(&c, id), id != target);
        }
        // Unknown player: never shared.
        assert!(!s.shares_player_with(&c, pid(10_000)));
        // Equality is by value, not by pointer.
        let mut d = s.clone();
        let l = d.player(target).unwrap().life();
        d.player_mut(target).unwrap().set_life(l);
        assert!(!d.shares_player_with(&s, target));
        assert_eq!(d, s);
        assert_eq!(d.state_hash(), s.state_hash());
    }
    // A player present in only one state is not shared.
    let a = GameState::new(1, 1, 20).unwrap();
    let mut b = a.clone();
    let p2 = b.add_player(20).unwrap();
    assert!(!a.shares_player_with(&b, p2));
    assert!(!b.shares_player_with(&a, p2));
    assert!(a.shares_player_with(&b, pid(1)));
}

// REQ-CORE-001, REQ-CORE-002: a failed mutator changes nothing.
#[test]
fn failed_mutators_change_nothing() {
    let mut rng = SplitMix64::new(0x0018_0004);
    let s = gen_state_with(&mut rng, 3);
    let check = |f: &dyn Fn(&mut GameState) -> bool| {
        let mut t = s.clone();
        let h = t.state_hash();
        assert!(f(&mut t), "mutator must fail");
        assert_eq!(t, s);
        assert_eq!(t.state_hash(), h);
        for p in s.players() {
            assert!(t.shares_player_with(&s, p.id()));
        }
    };
    check(&|t| t.set_turn_order(vec![pid(1), pid(2)]) == Err(StateError::InvalidTurnOrder));
    check(&|t| t.set_active_player(Some(pid(4))) == Err(StateError::UnknownPlayer(pid(4))));
    check(&|t| t.set_priority_player(Some(pid(0))) == Err(StateError::UnknownPlayer(pid(0))));
    let full = GameState::new(9, MAX_PLAYERS, 1).unwrap();
    let mut t = full.clone();
    assert_eq!(t.add_player(1), Err(StateError::TooManyPlayers));
    assert_eq!(t, full);
    assert_eq!(t.state_hash(), full.state_hash());
}

// ---------------------------------------------------------------------------------------------
// 3. Hash (REQ-CORE-001, REQ-NFR-006)
// ---------------------------------------------------------------------------------------------

/// The further mutated state pinned by the golden test.
fn golden_mutated() -> GameState {
    let mut s = GameState::new(42, 3, 20).unwrap();
    s.player_mut(pid(1)).unwrap().set_life(-7);
    s.player_mut(pid(2))
        .unwrap()
        .set_counter("test.counter_a", 3);
    s.player_mut(pid(3))
        .unwrap()
        .set_designation("test.designation_a", DesignationValue::Text("x".into()));
    s.player_mut(pid(3))
        .unwrap()
        .set_designation("test.designation_b", DesignationValue::Int(-2));
    s.player_mut(pid(1))
        .unwrap()
        .set_designation("test.designation_c", DesignationValue::Flag);
    s.prng_mut().next_u64();
    s.issue_card_id().unwrap();
    s.issue_object_id().unwrap();
    s.issue_object_id().unwrap();
    s.issue_ability_instance_id().unwrap();
    s.issue_effect_id().unwrap();
    s.issue_timestamp().unwrap();
    s.issue_link_id().unwrap();
    s.set_turn_number(4);
    s.set_active_player(Some(pid(2))).unwrap();
    s.set_priority_player(Some(pid(3))).unwrap();
    s.set_turn_order(vec![pid(2), pid(3), pid(1)]).unwrap();
    s
}

const GOLDEN_NEW_42_2_20: &str = "7389c79ba648f778a6925577d462382782418b603f487e7eef53a39898a87030";
const GOLDEN_MUTATED: &str = "61143120a7a8869bc2fb299244f20d4d6085bf324fefe398afb16d6cf354cb7c";

// REQ-CORE-001, REQ-NFR-006: pinned golden digests (computed once and recorded; never changed
// to make a test pass).
#[test]
fn golden_state_hashes() {
    let s = GameState::new(42, 2, 20).unwrap();
    assert_eq!(s.state_hash().to_hex(), GOLDEN_NEW_42_2_20);
    assert_eq!(golden_mutated().state_hash().to_hex(), GOLDEN_MUTATED);
    // Repeated runs are identical.
    for _ in 0..3 {
        assert_eq!(
            GameState::new(42, 2, 20).unwrap().state_hash().to_hex(),
            GOLDEN_NEW_42_2_20
        );
    }
}

/// Recomputes the root digest by hand from the frozen layout, using only leaf and node digests.
fn manual_root(s: &GameState) -> Digest {
    fn leaf<T: CanonicalEncode + ?Sized>(v: &T) -> Digest {
        leaf_digest(&encode_to_vec(v))
    }
    fn node(mut children: Vec<(Label, Digest)>) -> Digest {
        children.sort_by(|a, b| a.0.cmp(&b.0));
        node_digest(&children)
    }
    let ids = s.ids();
    let ids_node = node(vec![
        (n("card"), leaf(&ids.card.last_issued())),
        (n("object"), leaf(&ids.object.last_issued())),
        (
            n("ability_instance"),
            leaf(&ids.ability_instance.last_issued()),
        ),
        (n("effect"), leaf(&ids.effect.last_issued())),
        (n("timestamp"), leaf(&ids.timestamp.last_issued())),
        (n("link"), leaf(&ids.link.last_issued())),
        (n("player"), leaf(&ids.player.last_issued())),
    ]);
    let order: Vec<PlayerId> = s.turn_order().to_vec();
    let turn_node = node(vec![
        (n("number"), leaf(&s.turn().turn_number())),
        (n("active_player"), leaf(&s.turn().active_player())),
        (n("priority_player"), leaf(&s.turn().priority_player())),
        (n("order"), leaf(&order)),
    ]);
    let players_node = node(
        s.players()
            .map(|p| {
                (
                    Label::index(p.id().get()),
                    node(vec![
                        (n("life"), leaf(&p.life())),
                        (n("counters"), leaf(p.counters())),
                        (n("designations"), leaf(p.designations())),
                    ]),
                )
            })
            .collect(),
    );
    // The history placeholder: exactly put_len(0).
    let mut empty = Vec::new();
    put_len(&mut empty, 0);
    node(vec![
        (n("prng"), leaf(s.prng())),
        (n("ids"), ids_node),
        (n("turn"), turn_node),
        (n("players"), players_node),
        (n("effects"), leaf(s.effects())),
        (n("timestamps"), leaf(s.timestamps())),
        (n("effect_generators"), leaf(s.effect_generators())),
        (n("history"), leaf_digest(&empty)),
    ])
}

// REQ-NFR-006, REQ-CORE-001: the tree matches a by-hand rebuild of the frozen layout.
#[test]
fn hash_matches_manual_layout() {
    let mut rng = SplitMix64::new(0x0018_0005);
    let mut states = vec![GameState::new(42, 2, 20).unwrap(), golden_mutated()];
    for _ in 0..200 {
        states.push(gen_state(&mut rng));
    }
    for s in &states {
        let tree = s.merkle_tree();
        assert_eq!(tree.full_root_digest(), manual_root(s));
        assert_eq!(s.state_hash(), manual_root(s));
        assert_eq!(
            tree.leaf_payload(&path(&[n("prng")])).unwrap().len(),
            32,
            "prng payload is 32 bytes"
        );
    }
}

/// Asserts `b` differs from `a` exactly at `expected`.
fn assert_diverges_at(a: &GameState, b: &GameState, expected: &ComponentPath) {
    assert_ne!(
        a.state_hash(),
        b.state_hash(),
        "root must change at {expected}"
    );
    let (ta, tb) = (a.merkle_tree(), b.merkle_tree());
    assert_eq!(diff_first_divergence(&ta, &tb).as_ref(), Some(expected));
    assert_eq!(diff_first_divergence(&tb, &ta).as_ref(), Some(expected));
}

// REQ-NFR-006, REQ-CORE-001: changing one component changes the root and the first divergence
// names exactly that component's path.
#[test]
fn hash_per_component_sensitivity() {
    let mut rng = SplitMix64::new(0x0018_0006);
    for _ in 0..30 {
        let a = gen_state_with(&mut rng, 3);
        let p1 = Label::index(1);
        let ids_path = |k: &str| path(&[n("ids"), n(k)]);
        let turn_path = |k: &str| path(&[n("turn"), n(k)]);
        let cases: Vec<(ComponentPath, Mutator)> = vec![
            (path(&[n("players"), p1.clone(), n("life")]), |s| {
                let mut p = s.player_mut(pid(1)).unwrap();
                let l = p.life();
                p.set_life(l.wrapping_add(1));
            }),
            (path(&[n("players"), p1.clone(), n("counters")]), |s| {
                let mut p = s.player_mut(pid(1)).unwrap();
                let c = p.counter("test.counter_q");
                p.set_counter("test.counter_q", c + 1);
            }),
            (path(&[n("players"), p1.clone(), n("designations")]), |s| {
                s.player_mut(pid(1))
                    .unwrap()
                    .set_designation("test.dq", DesignationValue::Int(1));
            }),
            (path(&[n("prng")]), |s| {
                s.prng_mut().next_u64();
            }),
            (ids_path("card"), |s| ok(s.issue_card_id())),
            (ids_path("object"), |s| ok(s.issue_object_id())),
            (ids_path("ability_instance"), |s| {
                ok(s.issue_ability_instance_id());
            }),
            (ids_path("effect"), |s| ok(s.issue_effect_id())),
            (ids_path("timestamp"), |s| ok(s.issue_timestamp())),
            (ids_path("link"), |s| ok(s.issue_link_id())),
            (turn_path("number"), |s| {
                let t = s.turn().turn_number();
                s.set_turn_number(t.wrapping_add(1));
            }),
            (turn_path("active_player"), |s| {
                let next = if s.turn().active_player() == Some(pid(2)) {
                    None
                } else {
                    Some(pid(2))
                };
                s.set_active_player(next).unwrap();
            }),
            (turn_path("priority_player"), |s| {
                let next = if s.turn().priority_player() == Some(pid(3)) {
                    None
                } else {
                    Some(pid(3))
                };
                s.set_priority_player(next).unwrap();
            }),
            (turn_path("order"), |s| {
                let mut o = s.turn_order().to_vec();
                o.rotate_left(1);
                s.set_turn_order(o).unwrap();
            }),
        ];
        for (expected, m) in cases {
            let mut b = a.clone();
            m(&mut b);
            assert_diverges_at(&a, &b, &expected);
        }
        // The layer components (CR-005): each leaf on its own.
        let mut a = a.clone();
        let object = fineprint_base::ids::ObjectId::new(901);
        a.stamp_object(object, StampReason::EntersZone).unwrap();
        a.stamp_counters(CounterHolder::Object(object), "test.counter_l")
            .unwrap();
        let etb = a.issue_effect_id().unwrap();
        let ability = synthetic_ability;
        a.link_effect_generator(etb, GeneratingAbility::new(object, ability("test.l1")))
            .unwrap();
        let mut b = a.clone();
        b.forget_object_stamps(object);
        assert_diverges_at(&a, &b, &path(&[n("timestamps")]));
        let mut b = a.clone();
        b.clear_counter_stamp(CounterHolder::Object(object), "test.counter_l");
        assert_diverges_at(&a, &b, &path(&[n("timestamps")]));
        let mut b = a.clone();
        assert!(b.remove_effect(etb));
        assert_diverges_at(&a, &b, &path(&[n("effect_generators")]));
        let mut b = a.clone();
        assert!(b.remove_effect(etb));
        b.link_effect_generator(etb, GeneratingAbility::new(object, ability("test.l2")))
            .unwrap();
        assert_diverges_at(&a, &b, &path(&[n("effect_generators")]));
        let mut b = a.clone();
        b.effects_mut()
            .register(
                fineprint_base::ids::EffectId::new(etb.get() + 1),
                crate::layers::ContinuousEffect {
                    source: None,
                    controller: pid(1),
                    timestamp: fineprint_base::ids::Timestamp::new(1),
                    layer: fineprint_dsl::ability::Layer::L7d,
                    is_cda: false,
                    origin: crate::layers::EffectOrigin::Resolved,
                    affected: crate::layers::Affected::Objects(Default::default()),
                    modification: LayerModification::SwitchPt,
                },
            )
            .unwrap();
        assert_diverges_at(&a, &b, &path(&[n("effects")]));
        // Adding a player: the first component to differ is the player generator, and the new
        // player's subtree exists only in the bigger state.
        let mut b = a.clone();
        let new_id = b.add_player(5).unwrap();
        assert_diverges_at(&a, &b, &ids_path("player"));
        let new_path = path(&[n("players"), Label::index(new_id.get())]);
        assert!(a.merkle_tree().digest_at(&new_path).is_none());
        assert!(b.merkle_tree().digest_at(&new_path).is_some());
        // Removing a player (same generator; the turn fields sort after /players): the first
        // difference is that player's subtree.
        let mut w = wire_of(&a);
        w.players.retain(|p| p.id != 3);
        w.turn.order.retain(|&p| p != 3);
        if w.turn.active_player == Some(3) {
            w.turn.active_player = None;
        }
        if w.turn.priority_player == Some(3) {
            w.turn.priority_player = None;
        }
        let smaller = decode_wire(&w).unwrap();
        assert_ne!(smaller.state_hash(), a.state_hash());
        assert_eq!(
            diff_first_divergence(&a.merkle_tree(), &smaller.merkle_tree()),
            Some(path(&[n("players"), Label::index(3)]))
        );
    }
}

// REQ-NFR-006, REQ-CORE-001: equal states built in different ways have equal hashes.
#[test]
fn hash_is_order_and_construction_independent() {
    let mut a = GameState::new(5, 2, 20).unwrap();
    {
        let mut p = a.player_mut(pid(1)).unwrap();
        p.set_counter("test.counter_a", 1);
        p.set_counter("test.counter_b", 2);
        p.set_designation("test.d1", DesignationValue::Flag);
        p.set_designation("test.d2", DesignationValue::Int(9));
    }
    let mut b = GameState::new(5, 1, 20).unwrap();
    b.add_player(20).unwrap();
    {
        let mut p = b.player_mut(pid(1)).unwrap();
        p.set_designation("test.d2", DesignationValue::Text("tmp".into()));
        p.set_counter("test.counter_c", 7);
        p.set_counter("test.counter_b", 2);
        p.set_designation("test.d1", DesignationValue::Flag);
        p.set_counter("test.counter_a", 1);
        p.set_designation("test.d2", DesignationValue::Int(9));
        p.set_counter("test.counter_c", 0);
        p.set_designation("test.d3", DesignationValue::Flag);
        assert_eq!(p.clear_designation("test.d3"), Some(DesignationValue::Flag));
    }
    b.set_turn_order(vec![pid(2), pid(1)]).unwrap();
    b.set_turn_order(vec![pid(1), pid(2)]).unwrap();
    b.set_active_player(Some(pid(1))).unwrap();
    b.set_active_player(None).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.state_hash(), b.state_hash());
    let c = GameState::deserialize(&b.serialize().unwrap()).unwrap();
    assert_eq!(c.state_hash(), a.state_hash());
    // Sharing does not affect the hash.
    let shared = a.clone();
    assert_eq!(shared.state_hash(), a.state_hash());
}

// REQ-NFR-006: a player subtree digest equals the digest of an identical player elsewhere.
#[test]
fn player_subtree_digest_is_location_independent() {
    let mut a = GameState::new(1, 2, 20).unwrap();
    let mut b = GameState::new(2, 4, 20).unwrap();
    for s in [&mut a, &mut b] {
        let mut p = s.player_mut(pid(2)).unwrap();
        p.set_life(13);
        p.set_counter("test.counter_a", 4);
        p.set_designation("test.d", DesignationValue::Text("v".into()));
    }
    b.issue_object_id().unwrap();
    assert_ne!(a.state_hash(), b.state_hash());
    let p2 = path(&[n("players"), Label::index(2)]);
    let p1 = path(&[n("players"), Label::index(1)]);
    let (mut ta, mut tb) = (a.merkle_tree(), b.merkle_tree());
    assert_eq!(ta.digest_at(&p2), tb.digest_at(&p2));
    assert!(ta.digest_at(&p2).is_some());
    assert_eq!(ta.digest_at(&p1), tb.digest_at(&p1));
    b.player_mut(pid(1)).unwrap().set_life(19);
    assert_ne!(ta.digest_at(&p1), b.merkle_tree().digest_at(&p1));
}

// ---------------------------------------------------------------------------------------------
// 4. Generators (REQ-CORE-002)
// ---------------------------------------------------------------------------------------------

// REQ-CORE-002: the first id of every kind is 1 and ids are strictly increasing.
#[test]
fn generators_start_at_one_and_increase() {
    let mut s = GameState::new(3, 2, 20).unwrap();
    assert_eq!(s.ids().card.last_issued(), 0);
    assert_eq!(s.ids().player.last_issued(), 2);
    assert_eq!(s.issue_card_id().unwrap().get(), 1);
    assert_eq!(s.issue_object_id().unwrap().get(), 1);
    assert_eq!(s.issue_ability_instance_id().unwrap().get(), 1);
    assert_eq!(s.issue_effect_id().unwrap().get(), 1);
    assert_eq!(s.issue_timestamp().unwrap().get(), 1);
    assert_eq!(s.issue_link_id().unwrap().get(), 1);
    let mut prev = [1_u64; 6];
    for _ in 0..2_000 {
        let next = [
            s.issue_card_id().unwrap().get(),
            s.issue_object_id().unwrap().get(),
            s.issue_ability_instance_id().unwrap().get(),
            s.issue_effect_id().unwrap().get(),
            s.issue_timestamp().unwrap().get(),
            s.issue_link_id().unwrap().get(),
        ];
        for k in 0..6 {
            assert_eq!(next[k], prev[k] + 1);
        }
        prev = next;
    }
    assert_eq!(s.ids().timestamp.last_issued(), 2_001);
    // Players: 1..=n in turn order, then add_player continues.
    let mut t = GameState::new(3, 5, 20).unwrap();
    let expected: Vec<PlayerId> = (1..=5).map(pid).collect();
    assert_eq!(t.turn_order(), expected.as_slice());
    assert_eq!(
        t.players().map(PlayerState::id).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(t.add_player(1).unwrap(), pid(6));
    assert_eq!(t.turn_order().last(), Some(&pid(6)));
    assert_eq!(t.player(pid(6)).unwrap().life(), 1);
}

// REQ-CORE-002: exhaustion is an error, never a wrap, and leaves the state unchanged.
#[test]
fn generators_exhaust_without_change() {
    let base = GameState::new(3, 2, 20).unwrap();
    let mut w = wire_of(&base);
    w.ids.card = u64::MAX;
    w.ids.object = u64::MAX;
    w.ids.ability_instance = u64::MAX;
    w.ids.effect = u64::MAX;
    w.ids.timestamp = u64::MAX - 1;
    w.ids.link = u64::MAX;
    w.ids.player = u64::MAX;
    let s = decode_wire(&w).unwrap();
    let check = |f: &dyn Fn(&mut GameState) -> Result<(), StateError>| {
        let mut t = s.clone();
        assert_eq!(f(&mut t), Err(StateError::IdExhausted));
        assert_eq!(t, s);
        assert_eq!(t.state_hash(), s.state_hash());
    };
    check(&|t| t.issue_card_id().map(|_| ()));
    check(&|t| t.issue_object_id().map(|_| ()));
    check(&|t| t.issue_ability_instance_id().map(|_| ()));
    check(&|t| t.issue_effect_id().map(|_| ()));
    check(&|t| t.issue_link_id().map(|_| ()));
    check(&|t| t.add_player(1).map(|_| ()));
    let mut t = s.clone();
    assert_eq!(t.issue_timestamp().unwrap().get(), u64::MAX);
    let after = t.clone();
    assert_eq!(t.issue_timestamp(), Err(StateError::IdExhausted));
    assert_eq!(t, after);
    // Survives a round trip at the edge.
    check_round_trip(&t);
}

// ---------------------------------------------------------------------------------------------
// 5. N players (REQ-NFR-010)
// ---------------------------------------------------------------------------------------------

// REQ-NFR-010, REQ-CORE-001: 1, 2, 3, 8 and MAX_PLAYERS players; rotations and reversals.
#[test]
fn n_player_counts() {
    let mut rng = SplitMix64::new(0x0018_0007);
    for count in [1_usize, 2, 3, 8, MAX_PLAYERS] {
        let mut s = GameState::new(11, count, 20).unwrap();
        assert_eq!(s.player_count(), count);
        assert_eq!(s.turn_order().len(), count);
        check_round_trip(&s);
        assert_eq!(s.state_hash(), manual_root(&s));
        let ids = s.turn_order().to_vec();
        for r in 0..count.min(5) {
            let mut o = ids.clone();
            o.rotate_left(r);
            s.set_turn_order(o.clone()).unwrap();
            assert_eq!(s.turn_order(), o.as_slice());
            check_round_trip(&s);
            o.reverse();
            s.set_turn_order(o.clone()).unwrap();
            assert_eq!(s.turn_order(), o.as_slice());
            check_round_trip(&s);
        }
        let last = *ids.last().unwrap();
        s.set_active_player(Some(last)).unwrap();
        s.set_priority_player(Some(last)).unwrap();
        assert_eq!(s.turn().active_player(), Some(last));
        assert_eq!(s.turn().priority_player(), Some(last));
        check_round_trip(&s);
        check_round_trip(&gen_state_with(&mut rng, count));
        // Invalid orders: duplicate, missing, extra, unknown, zero, doubled, empty.
        let before = s.clone();
        let mut dup = ids.clone();
        dup[0] = *ids.last().unwrap();
        let missing = ids[1..].to_vec();
        let mut extra = ids.clone();
        extra.push(pid(10_000));
        let mut unknown = ids.clone();
        unknown[0] = pid(10_000);
        let mut zero = ids.clone();
        zero[0] = pid(0);
        let mut doubled = ids.clone();
        doubled.push(ids[0]);
        let mut bads = vec![missing, extra, unknown, zero, doubled, Vec::new()];
        if count > 1 {
            bads.push(dup);
        }
        for bad in bads {
            assert_eq!(s.set_turn_order(bad), Err(StateError::InvalidTurnOrder));
            assert_eq!(s, before);
        }
        assert_eq!(
            s.set_active_player(Some(pid(10_000))),
            Err(StateError::UnknownPlayer(pid(10_000)))
        );
        assert_eq!(
            s.set_priority_player(Some(pid(10_000))),
            Err(StateError::UnknownPlayer(pid(10_000)))
        );
        assert_eq!(s, before);
    }
    assert_eq!(GameState::new(1, 0, 20), Err(StateError::NoPlayers));
    assert_eq!(
        GameState::new(1, MAX_PLAYERS + 1, 20),
        Err(StateError::TooManyPlayers)
    );
    assert_eq!(
        GameState::new(1, usize::MAX, 20),
        Err(StateError::TooManyPlayers)
    );
    let mut s = GameState::new(1, MAX_PLAYERS - 1, 20).unwrap();
    assert_eq!(s.add_player(20).unwrap().get(), 256);
    assert_eq!(s.add_player(20), Err(StateError::TooManyPlayers));
    assert_eq!(s.player_count(), MAX_PLAYERS);
}

// ---------------------------------------------------------------------------------------------
// 6. Untrusted input (R12)
// ---------------------------------------------------------------------------------------------

fn find(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len())
        .position(|w| w == needle)
        .expect("needle present")
}

/// `key` as a MessagePack fixstr.
fn fixstr(key: &str) -> Vec<u8> {
    let len = u8::try_from(key.len()).unwrap();
    assert!(len < 32);
    let mut v = vec![0xa0 | len];
    v.extend_from_slice(key.as_bytes());
    v
}

// R12, REQ-CORE-001: every rejection case, each from a valid value with one corruption.
#[test]
fn deserialize_rejects_each_invalid_case() {
    let mut rng = SplitMix64::new(0x0018_0008);
    let mut base = gen_state_with(&mut rng, 3);
    base.player_mut(pid(2))
        .unwrap()
        .set_counter("test.counter_a", 4);
    let good = wire_of(&base);
    assert_eq!(decode_wire(&good).unwrap(), base);

    let corrupt = |f: &dyn Fn(&mut StateWire)| {
        let mut w = good.clone();
        f(&mut w);
        assert_decode_err(decode_wire(&w));
    };
    corrupt(&|w| w.format_version = 0);
    // Version 1 (no layer members) is not read (CR-005).
    corrupt(&|w| w.format_version = 1);
    corrupt(&|w| w.format_version = 3);
    corrupt(&|w| w.format_version = u32::MAX);
    corrupt(&|w| w.prng_algorithm = "PCG64".into());
    corrupt(&|w| w.prng_algorithm = String::new());
    corrupt(&|w| w.prng.increment[1] &= !1);
    corrupt(&|w| w.players.clear());
    corrupt(&|w| w.players[0].id = 0);
    corrupt(&|w| w.players[1].id = 1);
    corrupt(&|w| w.players[2].id = 2);
    corrupt(&|w| w.players.swap(0, 1));
    corrupt(&|w| w.ids.player = 2);
    corrupt(&|w| w.ids.player = 0);
    corrupt(&|w| w.players[2].id = 4);
    corrupt(&|w| {
        w.ids.player = 4;
        w.players[2].id = 4;
    });
    corrupt(&|w| w.turn.order = vec![1, 2]);
    corrupt(&|w| w.turn.order = vec![1, 2, 2]);
    corrupt(&|w| w.turn.order = vec![1, 2, 3, 3]);
    corrupt(&|w| w.turn.order = vec![1, 2, 3, 4]);
    corrupt(&|w| w.turn.order = vec![1, 2, 4]);
    corrupt(&|w| w.turn.order = vec![0, 1, 2]);
    corrupt(&|w| w.turn.order.clear());
    corrupt(&|w| w.turn.active_player = Some(4));
    corrupt(&|w| w.turn.active_player = Some(0));
    corrupt(&|w| w.turn.priority_player = Some(4));
    corrupt(&|w| w.turn.priority_player = Some(u64::MAX));
    corrupt(&|w| {
        w.players[1].counters.insert("test.counter_a".into(), 0);
    });
    corrupt(&|w| {
        w.players[0].counters.insert(String::new(), 0);
    });
    // Too many players (MAX_PLAYERS + 1), otherwise consistent.
    let big = GameState::new(1, MAX_PLAYERS, 20).unwrap();
    let mut w = wire_of(&big);
    let extra = u64::try_from(MAX_PLAYERS + 1).unwrap();
    w.ids.player = extra;
    w.players.push(PlayerWire {
        id: extra,
        life: 20,
        counters: BTreeMap::new(),
        designations: BTreeMap::new(),
    });
    w.turn.order.push(extra);
    assert_decode_err(decode_wire(&w));
    w.players.pop();
    w.turn.order.pop();
    assert!(decode_wire(&w).is_ok());

    // Byte-level corruptions (a state with empty layer components, so the last member's bytes
    // are known).
    let bytes = GameState::new(8, 3, 20).unwrap().serialize().unwrap();
    assert_eq!(bytes[0], 0x8a, "top level is a 10-member map");
    // Trailing bytes.
    for tail in [&[0x00_u8][..], &[0xc0], &bytes[..]] {
        let mut b = bytes.clone();
        b.extend_from_slice(tail);
        assert_decode_err(GameState::deserialize(&b));
    }
    // Unknown top-level member.
    let mut b = bytes.clone();
    b[0] = 0x8b;
    b.extend_from_slice(&fixstr("zzz"));
    b.push(0x00);
    assert_decode_err(GameState::deserialize(&b));
    // Missing top-level member (effect_generators is last, an empty array).
    let mut b = bytes.clone();
    let mut last = fixstr("effect_generators");
    last.push(0x90);
    assert!(b.ends_with(&last));
    b.truncate(b.len() - last.len());
    b[0] = 0x89;
    assert_decode_err(GameState::deserialize(&b));
    // Missing Option member (active_player) inside `turn`.
    let mut w = good.clone();
    w.turn.active_player = None;
    let mut b = w.to_bytes().unwrap();
    assert!(GameState::deserialize(&b).is_ok());
    let mut key = fixstr("active_player");
    key.push(0xc0);
    let at = find(&b, &key);
    b.drain(at..at + key.len());
    let mut turn_head = vec![0x84];
    turn_head.extend_from_slice(&fixstr("number"));
    let head = find(&b, &turn_head);
    b[head] = 0x83;
    assert_decode_err(GameState::deserialize(&b));
    // Unknown member inside a nested struct (`turn`).
    let mut b = w.to_bytes().unwrap();
    let head = find(&b, &turn_head);
    b[head] = 0x85;
    let mut ins = fixstr("zz");
    ins.push(0x01);
    b.splice(head + 1..head + 1, ins);
    assert_decode_err(GameState::deserialize(&b));
    // Wrong type: format_version as a string.
    let mut b = bytes.clone();
    let mut fv = fixstr("format_version");
    fv.push(0x02);
    let at = find(&b, &fv) + fv.len() - 1;
    b.splice(at..=at, fixstr("1"));
    assert_decode_err(GameState::deserialize(&b));
    // Wrong type: a negative integer where a u64 player id is expected.
    let mut b = bytes.clone();
    let mut id = fixstr("id");
    id.push(0x01);
    let at = find(&b, &id) + id.len() - 1;
    b[at] = 0xff; // negative fixint -1
    assert_decode_err(GameState::deserialize(&b));
    // Not MessagePack of the right shape at all.
    for junk in [&[][..], &[0xc0], &[0x80], &[0x90], &[0xc1]] {
        assert_decode_err(GameState::deserialize(junk));
    }
}

/// Deserializes `b`: must not panic; on success, the state round-trips.
fn survive(b: &[u8]) {
    match GameState::deserialize(b) {
        Ok(s) => check_round_trip(&s),
        Err(e) => assert!(matches!(e, StateError::Decode(_)), "{e:?}"),
    }
}

// R12: every prefix truncation, 5 000 single-byte flips and 5 000 random buffers never panic.
#[test]
fn deserialize_survives_hostile_bytes() {
    let mut rng = SplitMix64::new(0x0018_0009);
    let samples: Vec<Vec<u8>> = (0..8)
        .map(|_| gen_state(&mut rng).serialize().unwrap())
        .collect();
    for b in &samples {
        for len in 0..b.len() {
            assert_decode_err(GameState::deserialize(&b[..len]));
        }
    }
    for _ in 0..5_000 {
        let mut b = samples[idx(&mut rng, samples.len())].clone();
        let at = idx(&mut rng, b.len());
        let flip = u8::try_from(1 + below(&mut rng, 255)).unwrap();
        b[at] ^= flip;
        survive(&b);
    }
    for _ in 0..5_000 {
        let len = idx(&mut rng, 513);
        let b: Vec<u8> = (0..len).map(|_| rng.next_u64().to_le_bytes()[0]).collect();
        survive(&b);
    }
    // Hostile length headers: huge array, map, string and bin lengths.
    for junk in [
        &[0xdd, 0xff, 0xff, 0xff, 0xff][..],
        &[0xdf, 0xff, 0xff, 0xff, 0xff],
        &[0xdb, 0xff, 0xff, 0xff, 0xff],
        &[0xc6, 0xff, 0xff, 0xff, 0xff],
        &[0x88, 0xdb, 0xff, 0xff, 0xff, 0xff],
    ] {
        assert_decode_err(GameState::deserialize(junk));
    }
}

// ---------------------------------------------------------------------------------------------
// 7. Accessors, placeholders, encodings and Display
// ---------------------------------------------------------------------------------------------

// REQ-CORE-001, REQ-CORE-002: PlayerState accessors; counters never hold 0.
#[test]
fn player_state_accessors() {
    let mut p = PlayerState::new(pid(7), -3);
    assert_eq!(p.id(), pid(7));
    assert_eq!(p.life(), -3);
    p.set_life(i64::MAX);
    assert_eq!(p.life(), i64::MAX);
    assert_eq!(p.counter("test.counter_a"), 0);
    p.set_counter("test.counter_a", 2);
    p.set_counter("", i64::MAX);
    assert_eq!(p.counter("test.counter_a"), 2);
    assert_eq!(p.counter(""), i64::MAX);
    assert_eq!(p.counters().len(), 2);
    p.set_counter("test.counter_a", 0);
    p.set_counter("test.counter_never", 0);
    assert_eq!(p.counter("test.counter_a"), 0);
    assert!(!p.counters().contains_key("test.counter_a"));
    assert!(!p.counters().contains_key("test.counter_never"));
    assert!(p.counters().values().all(|&v| v != 0));
    assert_eq!(p.designation("test.d"), None);
    p.set_designation("test.d", DesignationValue::Int(1));
    p.set_designation("test.d", DesignationValue::Text("\u{e9}".into()));
    assert_eq!(
        p.designation("test.d"),
        Some(&DesignationValue::Text("\u{e9}".into()))
    );
    assert_eq!(p.designations().len(), 1);
    assert_eq!(
        p.clear_designation("test.d"),
        Some(DesignationValue::Text("\u{e9}".into()))
    );
    assert_eq!(p.clear_designation("test.d"), None);
    assert!(p.designations().is_empty());
    let q = p.clone();
    assert_eq!(p, q);
    assert!(format!("{q:?}").contains("PlayerState"));
}

// REQ-CORE-001: DesignationValue canonical encoding.
#[test]
fn designation_value_encoding() {
    assert_eq!(encode_to_vec(&DesignationValue::Flag), [0x00]);
    let mut int = vec![0x01];
    int.extend_from_slice(&(-2_i64).to_le_bytes());
    assert_eq!(encode_to_vec(&DesignationValue::Int(-2)), int);
    let mut text = vec![0x02];
    put_bytes(&mut text, "ab".as_bytes());
    assert_eq!(encode_to_vec(&DesignationValue::Text("ab".into())), text);
    assert_eq!(
        encode_to_vec(&DesignationValue::Text(String::new())),
        [0x02, 0, 0, 0, 0]
    );
}

// REQ-CORE-001: TurnInfo, placeholders and the remaining GameState accessors.
#[test]
fn turn_info_and_placeholders() {
    let t = TurnInfo::default();
    assert_eq!(t.turn_number(), 0);
    assert_eq!(t.active_player(), None);
    assert_eq!(t.priority_player(), None);
    let s = GameState::new(42, 2, 20).unwrap();
    assert_eq!(s.turn(), &t);
    assert!(s.effects().is_empty());
    assert!(s.history().is_empty());
    assert_eq!(s.effects(), &crate::layers::ContinuousEffects::default());
    assert_eq!(s.timestamps(), &crate::layers::TimestampLedger::default());
    assert!(s.effect_generators().is_empty());
    assert_eq!(s.history(), &HistoryLedger::default());
    // CR-005: each empty layer component, like the history placeholder, is put_len(0).
    assert_eq!(encode_to_vec(s.effects()), [0, 0, 0, 0]);
    assert_eq!(encode_to_vec(s.timestamps()), [0, 0, 0, 0]);
    assert_eq!(encode_to_vec(s.effect_generators()), [0, 0, 0, 0]);
    assert_eq!(encode_to_vec(s.history()), [0, 0, 0, 0]);
    assert_eq!(s.prng(), &Pcg64Dxsm::from_u64_seed(42));
    let mut gens = IdGenerators::new();
    gens.player.issue().unwrap();
    gens.player.issue().unwrap();
    assert_eq!(s.ids(), &gens);
    assert_eq!(s.player(pid(3)), None);
    assert_eq!(s.player(pid(1)).unwrap().life(), 20);
    let mut m = s.clone();
    assert!(m.player_mut(pid(3)).is_none());
    assert!(format!("{s:?}").contains("GameState"));
    assert!(format!("{t:?}").contains("TurnInfo"));
    assert!(format!("{:?}", HistoryLedger::default()).contains("HistoryLedger"));
    // REQ-CORE-001: all randomness comes from the state's PRNG.
    let mut a = s.clone();
    let mut b = s.clone();
    let mut reference = Pcg64Dxsm::from_u64_seed(42);
    for _ in 0..10 {
        let x = a.prng_mut().next_u64();
        assert_eq!(x, b.prng_mut().next_u64());
        assert_eq!(x, reference.next_u64());
    }
}

// REQ-CORE-001: StateError Display and Error.
#[test]
fn state_error_display() {
    let cases = [
        (
            StateError::NoPlayers,
            "a game state needs at least one player",
        ),
        (
            StateError::TooManyPlayers,
            "a game state holds at most 256 players",
        ),
        (StateError::IdExhausted, "identifier space exhausted"),
        (StateError::UnknownPlayer(pid(9)), "unknown player 9"),
        (
            StateError::InvalidTurnOrder,
            "turn order is not a permutation of the player ids",
        ),
        (
            StateError::Encode("e".into()),
            "state serialization failed: e",
        ),
        (
            StateError::Decode("d".into()),
            "state deserialization failed: d",
        ),
    ];
    for (e, msg) in cases {
        assert_eq!(e.to_string(), msg);
        let dynerr: &dyn std::error::Error = &e;
        assert!(dynerr.source().is_none());
        assert_eq!(e.clone(), e);
        assert!(!format!("{e:?}").is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// PlayerMut guard and i64 counters (PR #166 review round 1)
// ---------------------------------------------------------------------------------------------

/// Every stored player's `id()` equals its key and the state round-trips with the same hash.
fn assert_ids_match_keys_and_round_trip(s: &GameState) {
    for &id in s.turn_order() {
        assert_eq!(s.player(id).unwrap().id(), id);
    }
    let ids: Vec<PlayerId> = s.players().map(PlayerState::id).collect();
    assert_eq!(ids, s.players.keys().copied().collect::<Vec<_>>());
    check_round_trip(s);
}

// REQ-CORE-001, REQ-CORE-002: after every PlayerMut setter the written player's id still equals
// its key, and serialize/deserialize round-trips with an identical hash. Deref reads through.
#[test]
fn player_mut_setters_keep_id_and_round_trip() {
    let mut s = GameState::new(7, 3, 20).unwrap();
    {
        let mut p = s.player_mut(pid(2)).unwrap();
        assert_eq!(p.id(), pid(2)); // Deref
        p.set_life(-5);
        assert_eq!(p.life(), -5);
    }
    assert_ids_match_keys_and_round_trip(&s);
    {
        let mut p = s.player_mut(pid(2)).unwrap();
        p.set_counter("test.counter_a", 4);
        assert_eq!(p.counter("test.counter_a"), 4);
        assert_eq!(p.id(), pid(2));
    }
    assert_ids_match_keys_and_round_trip(&s);
    {
        let mut p = s.player_mut(pid(3)).unwrap();
        p.set_designation("test.designation_a", DesignationValue::Int(3));
        assert_eq!(
            p.designation("test.designation_a"),
            Some(&DesignationValue::Int(3))
        );
        assert_eq!(p.id(), pid(3));
    }
    assert_ids_match_keys_and_round_trip(&s);
    {
        let mut p = s.player_mut(pid(3)).unwrap();
        assert_eq!(
            p.clear_designation("test.designation_a"),
            Some(DesignationValue::Int(3))
        );
        assert_eq!(p.clear_designation("test.designation_a"), None);
        assert_eq!(p.designation("test.designation_a"), None);
        assert_eq!(p.id(), pid(3));
        assert!(format!("{p:?}").contains("PlayerMut"));
    }
    assert_ids_match_keys_and_round_trip(&s);
    assert_eq!(s.player(pid(2)).unwrap().life(), -5);
    assert_eq!(s.player(pid(2)).unwrap().counter("test.counter_a"), 4);
}

// REQ-CORE-001, REQ-CORE-002: player_mut on a clone un-shares only the player it is called for
// (copy-on-write when player_mut is called); an unknown id gives None.
#[test]
fn player_mut_unshares_only_that_player() {
    let base = GameState::new(9, 4, 20).unwrap();
    for target in 1..=4 {
        let mut c = base.clone();
        assert!(c.player_mut(pid(target)).is_some());
        for id in 1..=4 {
            assert_eq!(c.shares_player_with(&base, pid(id)), id != target);
        }
        assert_eq!(c, base);
        c.player_mut(pid(target)).unwrap().set_life(1);
        assert_eq!(base.player(pid(target)).unwrap().life(), 20);
        assert_eq!(c.player(pid(target)).unwrap().life(), 1);
    }
    let mut c = base.clone();
    assert!(c.player_mut(pid(0)).is_none());
    assert!(c.player_mut(pid(5)).is_none());
    assert!(c.player_mut(pid(u64::MAX)).is_none());
    for id in 1..=4 {
        assert!(c.shares_player_with(&base, pid(id)));
    }
}

// REQ-CORE-006, REQ-CORE-001: counters are signed 64-bit; i64::MAX round-trips with an
// identical hash; 0, -1 and i64::MIN remove the entry or never insert one.
#[test]
fn counters_are_nonnegative_i64() {
    let mut s = GameState::new(3, 2, 20).unwrap();
    s.player_mut(pid(1))
        .unwrap()
        .set_counter("test.counter_a", i64::MAX);
    assert_eq!(
        s.player(pid(1)).unwrap().counter("test.counter_a"),
        i64::MAX
    );
    let back = GameState::deserialize(&s.serialize().unwrap()).unwrap();
    assert_eq!(back, s);
    assert_eq!(back.state_hash(), s.state_hash());
    check_round_trip(&s);

    for removal in [0, -1, i64::MIN] {
        let mut p = PlayerState::new(pid(1), 20);
        p.set_counter("test.counter_a", removal);
        assert!(p.counters().is_empty(), "{removal} never inserts");
        p.set_counter("test.counter_a", 5);
        p.set_counter("test.counter_a", removal);
        assert!(p.counters().is_empty(), "{removal} removes");
        assert_eq!(p.counter("test.counter_a"), 0);

        let mut t = s.clone();
        t.player_mut(pid(1))
            .unwrap()
            .set_counter("test.counter_a", removal);
        assert!(t.player(pid(1)).unwrap().counters().is_empty());
        assert_eq!(
            t.state_hash(),
            GameState::new(3, 2, 20).unwrap().state_hash()
        );
    }
}

// REQ-CORE-006, R12: the decoder accepts a counter of exactly i64::MAX and rejects -1, 0 and an
// unsigned MessagePack value above i64::MAX with a Decode error.
#[test]
fn deserialize_counter_bounds() {
    let base = GameState::new(5, 2, 20).unwrap();
    let good = wire_of(&base);

    let mut w = good.clone();
    w.players[1]
        .counters
        .insert("test.counter_a".into(), i64::MAX);
    let s = decode_wire(&w).unwrap();
    assert_eq!(
        s.player(pid(2)).unwrap().counter("test.counter_a"),
        i64::MAX
    );

    for bad in [-1, 0, i64::MIN] {
        let mut w = good.clone();
        w.players[1].counters.insert("test.counter_a".into(), bad);
        assert_decode_err(decode_wire(&w));
    }

    // Byte level: replace the encoding of i64::MAX after the counter key with MessagePack
    // uint64 values above i64::MAX.
    let mut with_max = base.clone();
    with_max
        .player_mut(pid(2))
        .unwrap()
        .set_counter("test.counter_z", i64::MAX);
    let bytes = with_max.serialize().unwrap();
    let max_enc = rmp_serde::to_vec(&i64::MAX).unwrap();
    let mut needle = fixstr("test.counter_z");
    needle.extend_from_slice(&max_enc);
    let at = find(&bytes, &needle) + fixstr("test.counter_z").len();
    let spliced = |v: u64| {
        let mut b = bytes[..at].to_vec();
        b.push(0xcf);
        b.extend_from_slice(&v.to_be_bytes());
        b.extend_from_slice(&bytes[at + max_enc.len()..]);
        b
    };
    for big in [i64::MAX.cast_unsigned() + 1, u64::MAX] {
        assert_decode_err(GameState::deserialize(&spliced(big)));
    }
    // Controls: the unmodified bytes and a uint64 spelling of exactly i64::MAX decode.
    assert_eq!(GameState::deserialize(&bytes).unwrap(), with_max);
    assert_eq!(
        GameState::deserialize(&spliced(i64::MAX.cast_unsigned())).unwrap(),
        with_max
    );
}

/// A synthetic `KEYWORD` ability with ability id `id` (R5).
fn synthetic_ability(id: &str) -> fineprint_dsl::card::AbilityAST {
    fineprint_dsl::card::AbilityAST::Keyword(fineprint_dsl::ability::KeywordInstanceAST::new(
        fineprint_dsl::ability::AbilityBase::new(id),
        "TEST_KEYWORD",
    ))
}
