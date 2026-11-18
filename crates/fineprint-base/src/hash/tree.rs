// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The incremental Merkle tree and the divergence finder (REQ-NFR-006, REQ-LOOP-001).
//!
//! **Cache invariant.** Every node carries an optional cached digest; `None` means dirty. A
//! mutation clears the cache of every ancestor of the changed node, and a digest computation
//! fills the cache of every node it visits, so a node with a cached digest always has only
//! cached (and correct) descendants.
//!
//! **Depth bound.** [`MerkleTree::set_leaf`] rejects any path of more than [`MAX_PATH_DEPTH`]
//! labels with [`PathError::TooDeep`] before it mutates anything, so every tree that exists has
//! bounded depth. The digest, view, comparison, clone and drop code recurses once per level and
//! relies on this bound to stay well inside a small thread stack (REQ-CORE-006).

use core::cmp::Ordering;
use core::fmt;
use std::collections::BTreeMap;

use super::{ComponentPath, Digest, Label, leaf_digest, node_digest_iter};
use crate::encode::{CanonicalEncode, encode_to_vec};

/// The maximum depth of a leaf path accepted by [`MerkleTree::set_leaf`] (REQ-CORE-006).
///
/// Depth is the number of labels in the path, so a leaf at depth N sits N levels below the
/// root. The limit is inclusive: a path of exactly `MAX_PATH_DEPTH` labels is accepted, a longer
/// one is rejected with [`PathError::TooDeep`]. The recursive digest, view, comparison, clone
/// and drop code relies on this bound.
pub const MAX_PATH_DEPTH: usize = 256;

/// Why [`MerkleTree::set_leaf`] rejected a path. The tree is unchanged in every case.
///
/// [`PathError::EmptyPath`] is checked first, then [`PathError::TooDeep`], then the structural
/// errors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathError {
    /// The path is the root, which is always an internal node.
    EmptyPath,
    /// A proper prefix of the path is an existing leaf.
    ThroughLeaf,
    /// The path names an existing internal node.
    IsInternal,
    /// The path has more than [`MAX_PATH_DEPTH`] labels.
    TooDeep,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PathError::EmptyPath => "the root path cannot hold a leaf",
            PathError::ThroughLeaf => "a proper prefix of the path is a leaf",
            PathError::IsInternal => "the path names an internal node",
            PathError::TooDeep => "the path is deeper than the maximum depth",
        })
    }
}

impl std::error::Error for PathError {}

/// One node of a [`MerkleTree`].
#[derive(Clone, Debug)]
pub(super) enum Node {
    /// A component: its canonical payload bytes.
    Leaf {
        payload: Vec<u8>,
        cache: Option<Digest>,
    },
    /// A group of components keyed by label, hashed in ascending label order.
    Internal {
        children: BTreeMap<Label, Node>,
        cache: Option<Digest>,
    },
}

impl Node {
    fn empty_internal() -> Node {
        Node::Internal {
            children: BTreeMap::new(),
            cache: None,
        }
    }

    fn cache(&self) -> Option<Digest> {
        match self {
            Node::Leaf { cache, .. } | Node::Internal { cache, .. } => *cache,
        }
    }

    fn invalidate(&mut self) {
        match self {
            Node::Leaf { cache, .. } | Node::Internal { cache, .. } => *cache = None,
        }
    }

    fn child(&self, label: &Label) -> Option<&Node> {
        match self {
            Node::Internal { children, .. } => children.get(label),
            Node::Leaf { .. } => None,
        }
    }

    /// The node at `labels` below `self`.
    fn get(&self, labels: &[Label]) -> Option<&Node> {
        let mut node = self;
        for label in labels {
            node = node.child(label)?;
        }
        Some(node)
    }

    /// The node at `labels` below `self`, without touching any cache.
    fn get_mut(&mut self, labels: &[Label]) -> Option<&mut Node> {
        let mut node = self;
        for label in labels {
            node = match node {
                Node::Internal { children, .. } => children.get_mut(label)?,
                Node::Leaf { .. } => return None,
            };
        }
        Some(node)
    }

    /// Incremental digest: reuses clean caches, fills the cache of every node it computes and
    /// counts each computation in `ops`.
    fn digest_incremental(&mut self, ops: &mut u64) -> Digest {
        if let Some(d) = self.cache() {
            return d;
        }
        let digest = match self {
            Node::Leaf { payload, cache } => {
                let d = leaf_digest(payload);
                *cache = Some(d);
                d
            }
            Node::Internal { children, cache } => {
                let count = children.len();
                let mut kids = Vec::with_capacity(count);
                for (label, child) in children.iter_mut() {
                    kids.push((label, child.digest_incremental(ops)));
                }
                let d = node_digest_iter(count, kids.into_iter());
                *cache = Some(d);
                d
            }
        };
        *ops = ops.saturating_add(1);
        digest
    }

    /// From-scratch digest; ignores every cache.
    fn digest_full(&self) -> Digest {
        match self {
            Node::Leaf { payload, .. } => leaf_digest(payload),
            Node::Internal { children, .. } => node_digest_iter(
                children.len(),
                children
                    .iter()
                    .map(|(label, child)| (label, child.digest_full())),
            ),
        }
    }
}

/// A Merkle tree of components named by [`ComponentPath`]s (REQ-NFR-006, REQ-LOOP-001).
///
/// The root is an internal node. Leaves hold canonical payload bytes; internal nodes hold
/// children keyed by [`Label`]. Mutations only mark the changed node's ancestors dirty;
/// [`MerkleTree::root_digest`] then recomputes exactly the dirty nodes. Emptied internal nodes
/// are kept (they still hash, as a node with zero children).
#[derive(Clone, Debug)]
pub struct MerkleTree {
    pub(super) root: Node,
    hash_ops: u64,
}

impl Default for MerkleTree {
    fn default() -> Self {
        MerkleTree::new()
    }
}

impl MerkleTree {
    /// An empty tree: the root is an internal node with zero children.
    #[must_use]
    pub fn new() -> MerkleTree {
        MerkleTree {
            root: Node::empty_internal(),
            hash_ops: 0,
        }
    }

    /// Stores `payload` as the leaf at `path`, creating any missing internal nodes on the way
    /// and replacing an existing leaf's payload.
    ///
    /// Setting a leaf to the payload it already holds changes nothing (nothing becomes dirty).
    ///
    /// # Errors
    ///
    /// [`PathError::EmptyPath`] if `path` is the root, [`PathError::TooDeep`] if `path` has more
    /// than [`MAX_PATH_DEPTH`] labels (checked before the structural errors),
    /// [`PathError::ThroughLeaf`] if a proper prefix of `path` is a leaf,
    /// [`PathError::IsInternal`] if `path` is an internal node. On error the tree is unchanged.
    pub fn set_leaf(&mut self, path: &ComponentPath, payload: Vec<u8>) -> Result<(), PathError> {
        let Some((last, prefix)) = path.labels().split_last() else {
            return Err(PathError::EmptyPath);
        };
        // The depth bound keeps the recursive digest, view, clone and drop code safe.
        if path.labels().len() > MAX_PATH_DEPTH {
            return Err(PathError::TooDeep);
        }
        // Validate first so that an error leaves the tree untouched.
        let mut node = Some(&self.root);
        for label in prefix {
            match node {
                Some(Node::Leaf { .. }) => return Err(PathError::ThroughLeaf),
                Some(n) => node = n.child(label),
                None => break,
            }
        }
        match node {
            Some(Node::Leaf { .. }) => return Err(PathError::ThroughLeaf),
            Some(parent) => match parent.child(last) {
                Some(Node::Internal { .. }) => return Err(PathError::IsInternal),
                Some(Node::Leaf { payload: old, .. }) if *old == payload => return Ok(()),
                _ => {}
            },
            None => {}
        }
        // Apply: every node on the way becomes dirty.
        let mut node = &mut self.root;
        for label in prefix {
            node.invalidate();
            node = match node {
                Node::Internal { children, .. } => children
                    .entry(label.clone())
                    .or_insert_with(Node::empty_internal),
                // Excluded by the validation above.
                Node::Leaf { .. } => return Err(PathError::ThroughLeaf),
            };
        }
        node.invalidate();
        match node {
            Node::Internal { children, .. } => {
                children.insert(
                    last.clone(),
                    Node::Leaf {
                        payload,
                        cache: None,
                    },
                );
                Ok(())
            }
            Node::Leaf { .. } => Err(PathError::ThroughLeaf),
        }
    }

    /// Stores the canonical encoding of `value` as the leaf at `path` (see
    /// [`MerkleTree::set_leaf`]).
    ///
    /// # Errors
    ///
    /// The same as [`MerkleTree::set_leaf`], including [`PathError::TooDeep`] for a path of more
    /// than [`MAX_PATH_DEPTH`] labels.
    pub fn set_leaf_value<T: CanonicalEncode + ?Sized>(
        &mut self,
        path: &ComponentPath,
        value: &T,
    ) -> Result<(), PathError> {
        self.set_leaf(path, encode_to_vec(value))
    }

    /// Removes the leaf or the whole subtree at `path`, marking its ancestors dirty.
    ///
    /// Returns `false` (and changes nothing) if `path` is the root or names no node. Internal
    /// nodes emptied by the removal are kept.
    pub fn remove(&mut self, path: &ComponentPath) -> bool {
        let Some((last, prefix)) = path.labels().split_last() else {
            return false;
        };
        let present = self
            .root
            .get(prefix)
            .and_then(|parent| parent.child(last))
            .is_some();
        if !present {
            return false;
        }
        let mut node = &mut self.root;
        for label in prefix {
            node.invalidate();
            node = match node {
                Node::Internal { children, .. } => match children.get_mut(label) {
                    Some(child) => child,
                    None => return false,
                },
                Node::Leaf { .. } => return false,
            };
        }
        node.invalidate();
        match node {
            Node::Internal { children, .. } => children.remove(last).is_some(),
            Node::Leaf { .. } => false,
        }
    }

    /// The root digest, computed incrementally: only dirty nodes are recomputed, and their
    /// digests are cached. Always equal to [`MerkleTree::full_root_digest`].
    pub fn root_digest(&mut self) -> Digest {
        self.root.digest_incremental(&mut self.hash_ops)
    }

    /// The root digest recomputed from scratch, ignoring every cache. Never mutates the tree
    /// and does not count towards [`MerkleTree::hash_operations`].
    #[must_use]
    pub fn full_root_digest(&self) -> Digest {
        self.root.digest_full()
    }

    /// The digest of the node at `path` (the root path gives the root digest), computed
    /// incrementally like [`MerkleTree::root_digest`]; `None` if there is no such node.
    pub fn digest_at(&mut self, path: &ComponentPath) -> Option<Digest> {
        let node = self.root.get_mut(path.labels())?;
        Some(node.digest_incremental(&mut self.hash_ops))
    }

    /// The payload of the leaf at `path`; `None` if `path` is absent or an internal node.
    #[must_use]
    pub fn leaf_payload(&self, path: &ComponentPath) -> Option<&[u8]> {
        match self.root.get(path.labels())? {
            Node::Leaf { payload, .. } => Some(payload),
            Node::Internal { .. } => None,
        }
    }

    /// The child labels of the internal node at `path`, ascending; `None` if `path` is absent or
    /// a leaf.
    #[must_use]
    pub fn child_labels(&self, path: &ComponentPath) -> Option<Vec<Label>> {
        match self.root.get(path.labels())? {
            Node::Internal { children, .. } => Some(children.keys().cloned().collect()),
            Node::Leaf { .. } => None,
        }
    }

    /// The cumulative number of leaf and internal digest computations performed by
    /// [`MerkleTree::root_digest`] and [`MerkleTree::digest_at`] on this tree (clones copy the
    /// count). [`MerkleTree::full_root_digest`] and [`diff_first_divergence`] are not counted.
    #[must_use]
    pub fn hash_operations(&self) -> u64 {
        self.hash_ops
    }
}

/// A read-only view of a node with its digest, built without mutating the tree.
///
/// A clean node uses its cache and builds no children (its descendants are clean too, so their
/// views are built on demand from their caches). A dirty internal node computes the views of
/// all its children once, so the whole walk stays linear in the size of the dirty part.
struct View<'a> {
    node: &'a Node,
    digest: Digest,
    kids: Option<BTreeMap<&'a Label, View<'a>>>,
}

impl<'a> View<'a> {
    fn of(node: &'a Node) -> View<'a> {
        if let Some(digest) = node.cache() {
            return View {
                node,
                digest,
                kids: None,
            };
        }
        match node {
            Node::Leaf { payload, .. } => View {
                node,
                digest: leaf_digest(payload),
                kids: None,
            },
            Node::Internal { children, .. } => {
                let kids: BTreeMap<&'a Label, View<'a>> = children
                    .iter()
                    .map(|(label, child)| (label, View::of(child)))
                    .collect();
                let digest =
                    node_digest_iter(kids.len(), kids.iter().map(|(label, v)| (*label, v.digest)));
                View {
                    node,
                    digest,
                    kids: Some(kids),
                }
            }
        }
    }

    /// Takes the view of the child `label` (whose node is `child`).
    fn take_child(&mut self, label: &Label, child: &'a Node) -> View<'a> {
        self.kids
            .as_mut()
            .and_then(|kids| kids.remove(label))
            .unwrap_or_else(|| View::of(child))
    }
}

/// The path of the first difference between `a` and `b` in depth-first, ascending-[`Label`]
/// order, or `None` if the trees are structurally identical (same labels, same node kinds,
/// same payloads). For divergence debugging (REQ-NFR-006).
///
/// At each pair of nodes at the same path, starting at the root:
/// - equal digests: identical subtrees, skipped;
/// - both internal: the union of child labels is walked in ascending order; a label present on
///   one side only returns that child's path; a label present on both sides whose subtrees
///   differ returns the result for that pair;
/// - both leaves: this path if the payloads differ;
/// - one leaf and one internal node: this path.
///
/// Neither tree is mutated; cached digests are used only on clean nodes, so stale caches are
/// never trusted.
#[must_use]
pub fn diff_first_divergence(a: &MerkleTree, b: &MerkleTree) -> Option<ComponentPath> {
    let mut path = Vec::new();
    diff_views(View::of(&a.root), View::of(&b.root), &mut path).then(|| ComponentPath::new(path))
}

/// Compares two views at `path`. Returns `true` with `path` set to the first difference, or
/// `false` with `path` restored, if the subtrees are identical.
fn diff_views<'a>(mut a: View<'a>, mut b: View<'a>, path: &mut Vec<Label>) -> bool {
    if a.digest == b.digest {
        return false;
    }
    match (a.node, b.node) {
        (Node::Leaf { payload: pa, .. }, Node::Leaf { payload: pb, .. }) => pa != pb,
        (Node::Internal { children: ca, .. }, Node::Internal { children: cb, .. }) => {
            let mut ia = ca.iter().peekable();
            let mut ib = cb.iter().peekable();
            loop {
                let order = match (ia.peek(), ib.peek()) {
                    (None, None) => return false,
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (Some((la, _)), Some((lb, _))) => la.cmp(lb),
                };
                match order {
                    Ordering::Less | Ordering::Greater => {
                        let lone = if order == Ordering::Less {
                            ia.next()
                        } else {
                            ib.next()
                        };
                        if let Some((label, _)) = lone {
                            path.push(label.clone());
                        }
                        return true;
                    }
                    Ordering::Equal => {
                        let (Some((label, na)), Some((_, nb))) = (ia.next(), ib.next()) else {
                            return false;
                        };
                        let va = a.take_child(label, na);
                        let vb = b.take_child(label, nb);
                        path.push(label.clone());
                        if diff_views(va, vb, path) {
                            return true;
                        }
                        path.pop();
                    }
                }
            }
        }
        _ => true,
    }
}
