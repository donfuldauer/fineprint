// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The canonical Merkle hash of a game state (REQ-NFR-006, REQ-CORE-001).

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::hash::{ComponentPath, Digest, Label, MerkleTree};

use super::GameState;

/// `/name/...` built from named labels.
fn named(names: &[&str]) -> ComponentPath {
    ComponentPath::new(names.iter().map(|n| Label::name(n)).collect())
}

/// Stores `value` at `path`. The layout below is fixed, shallow and never routes through a
/// leaf, so `set_leaf_value` cannot fail here.
fn put<T: CanonicalEncode + ?Sized>(tree: &mut MerkleTree, path: &ComponentPath, value: &T) {
    let stored = tree.set_leaf_value(path, value);
    debug_assert!(stored.is_ok(), "fixed state layout rejected at {path}");
}

impl GameState {
    /// The Merkle tree of this state, one leaf per component (REQ-NFR-006). FROZEN layout
    /// (golden hashes depend on it); each payload is the canonical encoding of the value named:
    ///
    /// - `/prng`: the `Pcg64Dxsm` (32 bytes);
    /// - `/ids/card`, `/ids/object`, `/ids/ability_instance`, `/ids/effect`,
    ///   `/ids/timestamp`, `/ids/link`, `/ids/player`: each generator's last issued value
    ///   as `u64`;
    /// - `/turn/number`: `u64`; `/turn/active_player` and `/turn/priority_player`:
    ///   `Option<PlayerId>`; `/turn/order`: `Vec<PlayerId>`;
    /// - `/players/<id>/life`: `i64`; `/players/<id>/counters`: `BTreeMap<String, i64>`
    ///   (amounts `1..=i64::MAX`, so the bytes equal the former `u64` encoding);
    ///   `/players/<id>/designations`: `BTreeMap<String, DesignationValue>`, where `<id>` is
    ///   `Label::index(id.get())`;
    /// - `/effects`: the `layers::ContinuousEffects` (count, then per effect in ascending
    ///   `EffectId` order the id, every field of the effect and its `SameAsLayer` anchor link;
    ///   the layout is in the `crate::layers` encoding documentation, CR-005);
    /// - `/timestamps`: the `layers::TimestampLedger` (count, then the object, counter and
    ///   sticker entries, each tagged, in ascending key order; CR 613.7);
    /// - `/effect_generators`: the generating-ability links,
    ///   `BTreeMap<EffectId, GeneratingAbility>` (each value the source `ObjectId` and the
    ///   ability as a DSL value in the `crate::layers` encoding documentation: tag `0` and
    ///   [`put_bytes`](fineprint_base::encode::put_bytes) of its RFC 8785 canonical JSON; else
    ///   tag `1` and the structural encoding of its JSON tree; else tag `2` alone; CR 613.8a);
    /// - `/history`: the `HistoryLedger`.
    ///
    /// Each empty collection (`/effects`, `/timestamps`, `/effect_generators`, `/history`) is
    /// `put_len(0)`, 4 zero bytes. The tree depends only on values, never on sharing or on how
    /// the state was built.
    #[must_use]
    pub fn merkle_tree(&self) -> MerkleTree {
        let mut tree = MerkleTree::new();
        put(&mut tree, &named(&["prng"]), &self.prng);

        let ids = &self.ids;
        put(&mut tree, &named(&["ids", "card"]), &ids.card.last_issued());
        put(
            &mut tree,
            &named(&["ids", "object"]),
            &ids.object.last_issued(),
        );
        put(
            &mut tree,
            &named(&["ids", "ability_instance"]),
            &ids.ability_instance.last_issued(),
        );
        put(
            &mut tree,
            &named(&["ids", "effect"]),
            &ids.effect.last_issued(),
        );
        put(
            &mut tree,
            &named(&["ids", "timestamp"]),
            &ids.timestamp.last_issued(),
        );
        put(&mut tree, &named(&["ids", "link"]), &ids.link.last_issued());
        put(
            &mut tree,
            &named(&["ids", "player"]),
            &ids.player.last_issued(),
        );

        put(
            &mut tree,
            &named(&["turn", "number"]),
            &self.turn.turn_number(),
        );
        put(
            &mut tree,
            &named(&["turn", "active_player"]),
            &self.turn.active_player(),
        );
        put(
            &mut tree,
            &named(&["turn", "priority_player"]),
            &self.turn.priority_player(),
        );
        put(&mut tree, &named(&["turn", "order"]), &*self.turn_order);

        for player in self.players() {
            let base = named(&["players"]).child(Label::index(player.id().get()));
            put(&mut tree, &base.child(Label::name("life")), &player.life());
            put(
                &mut tree,
                &base.child(Label::name("counters")),
                player.counters(),
            );
            put(
                &mut tree,
                &base.child(Label::name("designations")),
                player.designations(),
            );
        }

        put(&mut tree, &named(&["effects"]), &*self.effects);
        put(&mut tree, &named(&["timestamps"]), &*self.timestamps);
        put(&mut tree, &named(&["effect_generators"]), &*self.generators);
        put(&mut tree, &named(&["history"]), &self.history);
        tree
    }

    /// The canonical state hash: the root digest of [`GameState::merkle_tree`] (REQ-NFR-006).
    #[must_use]
    pub fn state_hash(&self) -> Digest {
        self.merkle_tree().root_digest()
    }
}
