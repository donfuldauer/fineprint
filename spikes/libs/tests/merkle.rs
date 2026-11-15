// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! T4 (REQ-NFR-006, REQ-LAYER-005 correctness only; performance is measured by
//! `examples/bench.rs`, never asserted): Merkle state hash determinism, incremental == full,
//! golden roots, and first-differing-component location. Synthetic state only (R5).

use fineprint_spike_libs::merkle::{
    Blake3Hasher, Component, MerkleHasher, MerkleTree, Permanent, Sha256Hasher, StackItem, State,
    encoded_len, first_difference, synthetic_state, to_hex,
};
use fineprint_spike_libs::prng::Pcg64Dxsm;

const SEED: u128 = 0x5eed_0004;

/// Golden roots of `synthetic_state(SEED, 200)`: identical on every platform. The SHA-256 root
/// was also reproduced by an independent Python `hashlib` re-implementation of the documented
/// encoding during the spike (not committed).
const SHA256_ROOT: &str = "738f06e1f327b8745855fdd2837722d572e0488a76052b1a4864314b567c33ba";
const BLAKE3_ROOT: &str = "85bf5e48d505692983eb9715328151028010bb9c8048ad8b0dc31ddbcf827e17";
const ENCODED_LEN: usize = 16_345;

fn full<H: MerkleHasher>(state: &State) -> [u8; 32] {
    MerkleTree::<H>::full(state).root()
}

fn bump_permanent(state: &mut State, id: u64) {
    if let Some(permanent) = state.battlefield.get_mut(&id) {
        permanent.damage += 1;
    }
}

#[test]
fn req_nfr_006_full_hash_is_deterministic_and_matches_golden_root() {
    let roots: Vec<String> = (0..10)
        .map(|_| to_hex(&full::<Sha256Hasher>(&synthetic_state(SEED, 200))))
        .collect();
    assert!(roots.iter().all(|root| root == SHA256_ROOT), "{roots:?}");
    let state = synthetic_state(SEED, 200);
    assert_eq!(to_hex(&full::<Blake3Hasher>(&state)), BLAKE3_ROOT);
    assert_eq!(encoded_len(&state), ENCODED_LEN);
}

#[test]
fn req_nfr_006_encoding_is_length_exact() {
    let mut state = synthetic_state(SEED, 200);
    let before = encoded_len(&state);
    if let Some(permanent) = state.battlefield.get_mut(&150) {
        permanent.attachments.push(9);
    }
    assert_eq!(encoded_len(&state), before + 8);
    assert_eq!(state.battlefield.len(), 200);
    assert_eq!(state.players.len(), 4);
}

fn incremental_matches_full<H: MerkleHasher>() {
    let mut state = synthetic_state(SEED, 200);
    let mut tree = MerkleTree::<H>::full(&state);
    bump_permanent(&mut state, 117);
    tree.update(&state, &[Component::Permanent(117)]);
    assert_eq!(tree.root(), full::<H>(&state));
    let ids: Vec<u64> = (0..10).map(|k| 100 + k * 19).collect();
    for &id in &ids {
        bump_permanent(&mut state, id);
    }
    let dirty: Vec<Component> = ids.iter().map(|&id| Component::Permanent(id)).collect();
    tree.update(&state, &dirty);
    assert_eq!(tree.root(), full::<H>(&state));
    assert_eq!(tree, MerkleTree::<H>::full(&state));
}

#[test]
fn req_nfr_006_incremental_equals_full_for_one_and_ten_permanents() {
    incremental_matches_full::<Sha256Hasher>();
    incremental_matches_full::<Blake3Hasher>();
}

/// Applies one PRNG-chosen mutation and returns the components it dirtied.
fn mutate(state: &mut State, rng: &mut Pcg64Dxsm, next_id: &mut u64) -> Vec<Component> {
    let ids: Vec<u64> = state.battlefield.keys().copied().collect();
    let pick = |rng: &mut Pcg64Dxsm| ids.get((rng.next_u64() % ids.len().max(1) as u64) as usize);
    match rng.next_u64() % 9 {
        0 | 1 => {
            let count = 1 + rng.next_u64() % 10;
            let mut dirty = Vec::new();
            for _ in 0..count {
                if let Some(&id) = pick(rng)
                    && let Some(permanent) = state.battlefield.get_mut(&id)
                {
                    permanent.tapped = !permanent.tapped;
                    permanent.counters += rng.next_u64() as i64 % 3;
                    permanent.attachments.push(rng.next_u64() % 50);
                    dirty.push(Component::Permanent(id));
                }
            }
            dirty
        }
        2 => {
            let seat = (rng.next_u64() % 4) as usize;
            state.players[seat].life -= 1;
            vec![Component::Player(seat)]
        }
        3 => {
            *next_id += 1;
            state.stack.push(StackItem {
                id: *next_id,
                controller: 1,
                targets: Vec::new(),
            });
            vec![Component::Stack]
        }
        4 => {
            state.triggers.pop();
            vec![Component::Triggers]
        }
        5 => {
            state.turn.priority = (state.turn.priority + 1) % 4;
            vec![Component::Turn]
        }
        6 => {
            state.prng.next_u64();
            vec![Component::Prng]
        }
        7 => {
            *next_id += 1;
            let permanent = Permanent {
                def: format!("test.perm_{:04}", *next_id % 10_000),
                controller: 0,
                power: 1,
                toughness: 1,
                damage: 0,
                counters: 0,
                tapped: false,
                attachments: Vec::new(),
            };
            state.battlefield.insert(*next_id, permanent);
            vec![Component::BattlefieldMembership]
        }
        _ => {
            if let Some(&id) = pick(rng) {
                state.battlefield.remove(&id);
            }
            vec![Component::BattlefieldMembership]
        }
    }
}

/// Property-style: 600 PRNG-driven mutations (fixed seed, no clock or OS entropy); after each,
/// the incrementally updated tree equals a full rebuild.
#[test]
fn req_nfr_006_incremental_equals_full_over_random_mutations() {
    for (seed, permanents) in [(1_u128, 200_u64), (2, 0), (3, 1), (4, 7)] {
        let mut state = synthetic_state(seed, permanents);
        let mut tree = MerkleTree::<Sha256Hasher>::full(&state);
        let mut rng = Pcg64Dxsm::new(seed, 99);
        let mut next_id = 20_000;
        for _ in 0..150 {
            let dirty = mutate(&mut state, &mut rng, &mut next_id);
            tree.update(&state, &dirty);
            assert_eq!(tree, MerkleTree::<Sha256Hasher>::full(&state));
        }
    }
}

fn locate(change: impl FnOnce(&mut State)) -> Option<Component> {
    let before = synthetic_state(SEED, 200);
    let mut after = before.clone();
    change(&mut after);
    first_difference(
        &MerkleTree::<Sha256Hasher>::full(&before),
        &MerkleTree::<Sha256Hasher>::full(&after),
    )
}

#[test]
fn req_nfr_006_first_difference_locates_the_changed_component() {
    assert_eq!(locate(|_| {}), None);
    for id in [100, 101, 163, 298, 299] {
        assert_eq!(
            locate(|state| bump_permanent(state, id)),
            Some(Component::Permanent(id))
        );
    }
    assert_eq!(
        locate(|state| state.players[2].poison += 1),
        Some(Component::Player(2))
    );
    assert_eq!(
        locate(|state| {
            state.battlefield.remove(&140);
        }),
        Some(Component::BattlefieldMembership)
    );
    assert_eq!(locate(|state| state.stack.clear()), Some(Component::Stack));
    assert_eq!(
        locate(|state| state.triggers[1].kind += 1),
        Some(Component::Triggers)
    );
    assert_eq!(locate(|state| state.turn.step += 1), Some(Component::Turn));
    assert_eq!(
        locate(|state| {
            state.prng.next_u64();
        }),
        Some(Component::Prng)
    );
    // Several differences: the first in fixed child order wins.
    assert_eq!(
        locate(|state| {
            state.turn.turn += 1;
            bump_permanent(state, 250);
            state.players[3].hand += 1;
        }),
        Some(Component::Player(3))
    );
}
