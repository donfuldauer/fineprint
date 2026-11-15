// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Merkle canonical state hash prototype (REQ-NFR-006, budget REQ-LAYER-005).
//!
//! The state is SYNTHETIC (R5): invented definition ids such as `test.perm_0017`.
//!
//! Encoding (all integers little-endian, fixed width; strings and lists carry a `u32` length):
//! every leaf is `H(0x00 || tag || fields)` and every parent is `H(0x01 || child digests)` in a
//! fixed order, so a leaf can never collide with a parent. Tree shape:
//!
//! ```text
//! root = node(players, battlefield, stack, triggers, turn, prng)
//! players = node(player leaf 0..n)            (seat order)
//! battlefield = node(membership leaf, permanents)
//! permanents = binary tree over permanent leaves in ObjectId order
//!              (pairs are hashed level by level; an odd last child is hashed alone)
//! ```
//!
//! No floats, no hash maps (REQ-CORE-006): the battlefield is a `BTreeMap` keyed by ObjectId.

use std::collections::BTreeMap;
use std::marker::PhantomData;

use crate::prng::Pcg64Dxsm;

/// A 32-byte digest.
pub type Digest = [u8; 32];

const LEAF_PREFIX: u8 = 0x00;
const NODE_PREFIX: u8 = 0x01;

const TAG_PLAYER: u8 = 1;
const TAG_MEMBERSHIP: u8 = 2;
const TAG_PERMANENT: u8 = 3;
const TAG_STACK: u8 = 4;
const TAG_TRIGGERS: u8 = 5;
const TAG_TURN: u8 = 6;
const TAG_PRNG: u8 = 7;

/// A streaming 32-byte hash function used for leaves and parents.
pub trait MerkleHasher: Default {
    /// Absorbs `bytes`.
    fn update(&mut self, bytes: &[u8]);
    /// Returns the digest.
    fn finish(self) -> Digest;
}

/// SHA-256 from the `sha2` crate (runtime CPU-feature detection unless built with
/// `--cfg sha2_backend="soft"`).
#[derive(Default)]
pub struct Sha256Hasher(sha2::Sha256);

impl MerkleHasher for Sha256Hasher {
    fn update(&mut self, bytes: &[u8]) {
        sha2::Digest::update(&mut self.0, bytes);
    }
    fn finish(self) -> Digest {
        sha2::Digest::finalize(self.0).into()
    }
}

/// BLAKE3 (pure Rust build): the comparison candidate for the REQ-NFR-006 escape clause.
#[derive(Default)]
pub struct Blake3Hasher(blake3::Hasher);

impl MerkleHasher for Blake3Hasher {
    fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    fn finish(self) -> Digest {
        *self.0.finalize().as_bytes()
    }
}

/// A synthetic player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    /// Life total.
    pub life: i64,
    /// Poison counters.
    pub poison: i64,
    /// Cards in hand.
    pub hand: i64,
    /// Cards in library.
    pub library: i64,
}

/// A synthetic permanent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permanent {
    /// Synthetic definition id, for example `test.perm_0017`.
    pub def: String,
    /// Controlling seat.
    pub controller: u8,
    /// Power.
    pub power: i64,
    /// Toughness.
    pub toughness: i64,
    /// Marked damage.
    pub damage: i64,
    /// Counter total.
    pub counters: i64,
    /// Tapped flag.
    pub tapped: bool,
    /// Integer-tagged attachments (ObjectIds).
    pub attachments: Vec<u64>,
}

/// A synthetic stack item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackItem {
    /// ObjectId.
    pub id: u64,
    /// Controlling seat.
    pub controller: u8,
    /// Target ObjectIds.
    pub targets: Vec<u64>,
}

/// A synthetic pending trigger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trigger {
    /// Source ObjectId.
    pub source: u64,
    /// Controlling seat.
    pub controller: u8,
    /// Integer trigger kind.
    pub kind: u32,
}

/// Turn position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnPosition {
    /// Turn number.
    pub turn: i64,
    /// Active seat.
    pub active: u8,
    /// Step index.
    pub step: u8,
    /// Seat holding priority.
    pub priority: u8,
}

/// The synthetic game state hashed by the spike.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// Players in seat order.
    pub players: Vec<Player>,
    /// Battlefield keyed by ObjectId.
    pub battlefield: BTreeMap<u64, Permanent>,
    /// Stack, bottom first.
    pub stack: Vec<StackItem>,
    /// Pending triggers in queue order.
    pub triggers: Vec<Trigger>,
    /// Turn position.
    pub turn: TurnPosition,
    /// The state's PRNG.
    pub prng: Pcg64Dxsm,
}

/// Builds the fixed synthetic state: 4 players, `permanents` permanents (ObjectIds 100..),
/// 3 stack items and 2 pending triggers, with field values drawn from a PCG64-DXSM seeded with
/// `seed` (that generator, after use, becomes the state's PRNG).
#[must_use]
pub fn synthetic_state(seed: u128, permanents: u64) -> State {
    let mut rng = Pcg64Dxsm::new(seed, 0x0f1e_2d3c_4b5a_6978);
    let mut small = |modulus: u64| -> i64 { (rng.next_u64() % modulus) as i64 };
    let players = (0..4)
        .map(|_| Player {
            life: 40 - small(10),
            poison: small(3),
            hand: small(8),
            library: 60 + small(40),
        })
        .collect();
    let battlefield = (0..permanents)
        .map(|i| {
            let id = 100 + i;
            let attachments = (0..small(3)).map(|k| 1000 + i * 4 + k as u64).collect();
            let permanent = Permanent {
                def: format!("test.perm_{i:04}"),
                controller: (i % 4) as u8,
                power: small(10),
                toughness: 1 + small(10),
                damage: 0,
                counters: small(4),
                tapped: small(2) == 1,
                attachments,
            };
            (id, permanent)
        })
        .collect();
    let stack = (0..3)
        .map(|k| StackItem {
            id: 5000 + k,
            controller: (k % 4) as u8,
            targets: vec![100 + k * 7, 101 + k * 7],
        })
        .collect();
    let triggers = (0..2)
        .map(|k| Trigger {
            source: 100 + k,
            controller: k as u8,
            kind: 3 + k as u32,
        })
        .collect();
    State {
        players,
        battlefield,
        stack,
        triggers,
        turn: TurnPosition {
            turn: 7,
            active: 2,
            step: 4,
            priority: 3,
        },
        prng: rng,
    }
}

/// One rules-relevant component of the state, as located by [`first_difference`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Component {
    /// A player, by seat index.
    Player(usize),
    /// The set of ObjectIds on the battlefield.
    BattlefieldMembership,
    /// A permanent, by ObjectId.
    Permanent(u64),
    /// The stack.
    Stack,
    /// The pending triggers.
    Triggers,
    /// The turn position.
    Turn,
    /// The PRNG state.
    Prng,
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_len(out: &mut Vec<u8>, len: usize) {
    put_u32(out, u32::try_from(len).unwrap_or(u32::MAX));
}

fn put_ids(out: &mut Vec<u8>, ids: &[u64]) {
    put_len(out, ids.len());
    ids.iter().for_each(|&id| put_u64(out, id));
}

fn encode_player(out: &mut Vec<u8>, seat: usize, player: &Player) {
    out.push(TAG_PLAYER);
    put_u32(out, u32::try_from(seat).unwrap_or(u32::MAX));
    for value in [player.life, player.poison, player.hand, player.library] {
        put_i64(out, value);
    }
}

fn encode_membership(out: &mut Vec<u8>, ids: &[u64]) {
    out.push(TAG_MEMBERSHIP);
    put_ids(out, ids);
}

fn encode_permanent(out: &mut Vec<u8>, id: u64, permanent: &Permanent) {
    out.push(TAG_PERMANENT);
    put_u64(out, id);
    put_len(out, permanent.def.len());
    out.extend_from_slice(permanent.def.as_bytes());
    out.push(permanent.controller);
    for value in [
        permanent.power,
        permanent.toughness,
        permanent.damage,
        permanent.counters,
    ] {
        put_i64(out, value);
    }
    out.push(u8::from(permanent.tapped));
    put_ids(out, &permanent.attachments);
}

fn encode_stack(out: &mut Vec<u8>, stack: &[StackItem]) {
    out.push(TAG_STACK);
    put_len(out, stack.len());
    for item in stack {
        put_u64(out, item.id);
        out.push(item.controller);
        put_ids(out, &item.targets);
    }
}

fn encode_triggers(out: &mut Vec<u8>, triggers: &[Trigger]) {
    out.push(TAG_TRIGGERS);
    put_len(out, triggers.len());
    for trigger in triggers {
        put_u64(out, trigger.source);
        out.push(trigger.controller);
        put_u32(out, trigger.kind);
    }
}

fn encode_turn(out: &mut Vec<u8>, turn: &TurnPosition) {
    out.push(TAG_TURN);
    put_i64(out, turn.turn);
    out.extend_from_slice(&[turn.active, turn.step, turn.priority]);
}

fn encode_prng(out: &mut Vec<u8>, prng: &Pcg64Dxsm) {
    out.push(TAG_PRNG);
    let (state, increment) = prng.raw();
    out.extend_from_slice(&state.to_le_bytes());
    out.extend_from_slice(&increment.to_le_bytes());
}

/// Total bytes of all leaf encodings of `state` (the hashed payload, for MB/s figures).
#[must_use]
pub fn encoded_len(state: &State) -> usize {
    let mut out = Vec::new();
    for (seat, player) in state.players.iter().enumerate() {
        encode_player(&mut out, seat, player);
    }
    let ids: Vec<u64> = state.battlefield.keys().copied().collect();
    encode_membership(&mut out, &ids);
    for (&id, permanent) in &state.battlefield {
        encode_permanent(&mut out, id, permanent);
    }
    encode_stack(&mut out, &state.stack);
    encode_triggers(&mut out, &state.triggers);
    encode_turn(&mut out, &state.turn);
    encode_prng(&mut out, &state.prng);
    out.len()
}

fn leaf<H: MerkleHasher>(scratch: &mut Vec<u8>, encode: impl FnOnce(&mut Vec<u8>)) -> Digest {
    scratch.clear();
    encode(scratch);
    let mut hasher = H::default();
    hasher.update(&[LEAF_PREFIX]);
    hasher.update(scratch);
    hasher.finish()
}

fn node<H: MerkleHasher>(children: &[Digest]) -> Digest {
    let mut hasher = H::default();
    hasher.update(&[NODE_PREFIX]);
    for child in children {
        hasher.update(child);
    }
    hasher.finish()
}

/// A Merkle tree over a [`State`] with every intermediate digest cached, so that changed
/// components can be rehashed along their path only. Equality compares every cached digest.
#[derive(Clone)]
pub struct MerkleTree<H> {
    player_leaves: Vec<Digest>,
    players: Digest,
    ids: Vec<u64>,
    membership: Digest,
    /// `levels[0]` holds the permanent leaves; the last level holds the permanents root.
    levels: Vec<Vec<Digest>>,
    battlefield: Digest,
    stack: Digest,
    triggers: Digest,
    turn: Digest,
    prng: Digest,
    root: Digest,
    hasher: PhantomData<H>,
}

impl<H> PartialEq for MerkleTree<H> {
    fn eq(&self, other: &Self) -> bool {
        (
            &self.player_leaves,
            self.players,
            &self.ids,
            self.membership,
            &self.levels,
            self.battlefield,
            [self.stack, self.triggers, self.turn, self.prng, self.root],
        ) == (
            &other.player_leaves,
            other.players,
            &other.ids,
            other.membership,
            &other.levels,
            other.battlefield,
            [
                other.stack,
                other.triggers,
                other.turn,
                other.prng,
                other.root,
            ],
        )
    }
}

impl<H> Eq for MerkleTree<H> {}

impl<H> core::fmt::Debug for MerkleTree<H> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "MerkleTree({})", to_hex(&self.root))
    }
}

impl<H: MerkleHasher> MerkleTree<H> {
    /// Hashes the whole state from scratch.
    #[must_use]
    pub fn full(state: &State) -> Self {
        let mut tree = Self {
            player_leaves: Vec::new(),
            players: [0; 32],
            ids: Vec::new(),
            membership: [0; 32],
            levels: Vec::new(),
            battlefield: [0; 32],
            stack: [0; 32],
            triggers: [0; 32],
            turn: [0; 32],
            prng: [0; 32],
            root: [0; 32],
            hasher: PhantomData,
        };
        let mut scratch = Vec::new();
        tree.hash_players(state, &mut scratch);
        tree.hash_battlefield(state, &mut scratch);
        tree.stack = leaf::<H>(&mut scratch, |out| encode_stack(out, &state.stack));
        tree.triggers = leaf::<H>(&mut scratch, |out| encode_triggers(out, &state.triggers));
        tree.turn = leaf::<H>(&mut scratch, |out| encode_turn(out, &state.turn));
        tree.prng = leaf::<H>(&mut scratch, |out| encode_prng(out, &state.prng));
        tree.hash_root();
        tree
    }

    /// Rehashes only the `dirty` components of `state` (and their paths to the root).
    ///
    /// Battlefield membership changes are detected automatically and rebuild the
    /// battlefield subtree; a `Player` entry rehashes all player leaves.
    pub fn update(&mut self, state: &State, dirty: &[Component]) {
        let mut scratch = Vec::new();
        if !state.battlefield.keys().eq(self.ids.iter()) {
            self.hash_battlefield(state, &mut scratch);
        }
        let mut players_dirty = false;
        let mut permanents_dirty = false;
        for &component in dirty {
            match component {
                Component::Player(_) => players_dirty = true,
                Component::BattlefieldMembership => {}
                Component::Permanent(id) => {
                    if let (Ok(index), Some(permanent)) =
                        (self.ids.binary_search(&id), state.battlefield.get(&id))
                    {
                        let digest = leaf::<H>(&mut scratch, |out| {
                            encode_permanent(out, id, permanent);
                        });
                        self.set_permanent_leaf(index, digest);
                        permanents_dirty = true;
                    }
                }
                Component::Stack => {
                    self.stack = leaf::<H>(&mut scratch, |out| encode_stack(out, &state.stack));
                }
                Component::Triggers => {
                    self.triggers =
                        leaf::<H>(&mut scratch, |out| encode_triggers(out, &state.triggers));
                }
                Component::Turn => {
                    self.turn = leaf::<H>(&mut scratch, |out| encode_turn(out, &state.turn));
                }
                Component::Prng => {
                    self.prng = leaf::<H>(&mut scratch, |out| encode_prng(out, &state.prng));
                }
            }
        }
        if players_dirty {
            self.hash_players(state, &mut scratch);
        }
        if permanents_dirty {
            self.hash_battlefield_node();
        }
        self.hash_root();
    }

    /// The root digest.
    #[must_use]
    pub fn root(&self) -> Digest {
        self.root
    }

    fn hash_players(&mut self, state: &State, scratch: &mut Vec<u8>) {
        self.player_leaves = state
            .players
            .iter()
            .enumerate()
            .map(|(seat, player)| leaf::<H>(scratch, |out| encode_player(out, seat, player)))
            .collect();
        self.players = node::<H>(&self.player_leaves);
    }

    fn hash_battlefield(&mut self, state: &State, scratch: &mut Vec<u8>) {
        self.ids = state.battlefield.keys().copied().collect();
        self.membership = leaf::<H>(scratch, |out| encode_membership(out, &self.ids));
        let leaves: Vec<Digest> = state
            .battlefield
            .iter()
            .map(|(&id, permanent)| leaf::<H>(scratch, |out| encode_permanent(out, id, permanent)))
            .collect();
        self.levels = vec![leaves];
        while let Some(last) = self.levels.last()
            && last.len() > 1
        {
            let next = last.chunks(2).map(node::<H>).collect();
            self.levels.push(next);
        }
        self.hash_battlefield_node();
    }

    fn set_permanent_leaf(&mut self, index: usize, digest: Digest) {
        let mut index = index;
        self.levels[0][index] = digest;
        for level in 1..self.levels.len() {
            let start = index & !1;
            let end = (start + 2).min(self.levels[level - 1].len());
            index /= 2;
            self.levels[level][index] = node::<H>(&self.levels[level - 1][start..end]);
        }
    }

    fn permanents_root(&self) -> Digest {
        match self.levels.last() {
            Some(top) if top.len() == 1 => top[0],
            _ => node::<H>(&[]),
        }
    }

    fn hash_battlefield_node(&mut self) {
        self.battlefield = node::<H>(&[self.membership, self.permanents_root()]);
    }

    fn hash_root(&mut self) {
        self.root = node::<H>(&[
            self.players,
            self.battlefield,
            self.stack,
            self.triggers,
            self.turn,
            self.prng,
        ]);
    }
}

/// Locates the first differing component between two hashed states by walking both trees in
/// the fixed child order (REQ-NFR-006 divergence debugging). Returns `None` if the roots match.
#[must_use]
pub fn first_difference<H: MerkleHasher>(
    a: &MerkleTree<H>,
    b: &MerkleTree<H>,
) -> Option<Component> {
    if a.root == b.root {
        return None;
    }
    if a.players != b.players {
        let seat = (0..a.player_leaves.len().max(b.player_leaves.len()))
            .find(|&seat| a.player_leaves.get(seat) != b.player_leaves.get(seat))
            .unwrap_or(0);
        return Some(Component::Player(seat));
    }
    if a.battlefield != b.battlefield {
        if a.membership != b.membership {
            return Some(Component::BattlefieldMembership);
        }
        // Same membership, hence the same tree shape: descend from the top level.
        let mut index = 0;
        for level in (0..a.levels.len().saturating_sub(1)).rev() {
            let start = index * 2;
            let end = (start + 2).min(a.levels[level].len());
            index = (start..end)
                .find(|&i| a.levels[level][i] != b.levels[level][i])
                .unwrap_or(start);
        }
        return a.ids.get(index).map(|&id| Component::Permanent(id));
    }
    [
        (a.stack != b.stack, Component::Stack),
        (a.triggers != b.triggers, Component::Triggers),
        (a.turn != b.turn, Component::Turn),
        (a.prng != b.prng, Component::Prng),
    ]
    .into_iter()
    .find_map(|(differs, component)| differs.then_some(component))
}

/// Lower-case hexadecimal rendering of a digest.
#[must_use]
pub fn to_hex(digest: &Digest) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    digest
        .iter()
        .flat_map(|&byte| [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 15)]])
        .map(char::from)
        .collect()
}
