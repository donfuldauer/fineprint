// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the digests and the Merkle framework (REQ-NFR-006, REQ-LOOP-001).
//!
//! Properties use a hand-rolled deterministic harness (PCG64-DXSM with literal seeds, at least
//! 5 000 iterations each). The reference functions below walk the private node structure
//! directly, without digests, so they are independent of the code under test.

use std::collections::BTreeMap;

use super::tree::Node;
use super::{
    ComponentPath, Digest, Label, MerkleTree, PathError, diff_first_divergence, leaf_digest,
    node_digest, sha256,
};
use crate::encode::{CanonicalEncode, encode_to_vec};
use crate::prng::Pcg64Dxsm;

const ITERATIONS: usize = 5_000;

fn n(s: &str) -> Label {
    Label::name(s)
}

fn ix(i: u64) -> Label {
    Label::index(i)
}

fn path(labels: &[Label]) -> ComponentPath {
    ComponentPath::new(labels.to_vec())
}

fn hex_digest(hex: &str) -> Digest {
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap_or(0);
    }
    Digest::from_bytes(out)
}

// ---------------------------------------------------------------------------------------------
// Digests
// ---------------------------------------------------------------------------------------------

#[test]
fn req_nfr_006_sha256_published_vectors() {
    let cases = [
        (
            "",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            "abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
    ];
    for (input, hex) in cases {
        let d = sha256(input.as_bytes());
        assert_eq!(d.to_hex(), hex);
        assert_eq!(d.to_string(), hex);
        assert_eq!(d, hex_digest(hex));
    }
}

#[test]
fn req_nfr_006_leaf_digest_of_empty_payload() {
    let d = leaf_digest(&[]);
    assert_eq!(
        d.to_hex(),
        "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"
    );
    assert_eq!(d, sha256(&[0x00]));
    assert_eq!(leaf_digest(&[7, 8]), sha256(&[0x00, 7, 8]));
}

#[test]
fn req_nfr_006_digest_bytes_hex_and_encoding() {
    let mut raw = [0u8; 32];
    for (i, b) in raw.iter_mut().enumerate() {
        *b = u8::try_from(i * 8).unwrap_or(0);
    }
    raw[31] = 0xab;
    let d = Digest::from_bytes(raw);
    assert_eq!(d.as_bytes(), &raw);
    let hex = d.to_hex();
    assert_eq!(hex.len(), 64);
    assert!(
        hex.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    );
    assert!(hex.starts_with("000810"));
    assert!(hex.ends_with("ab"));
    assert_eq!(format!("{d}"), hex);
    assert_eq!(encode_to_vec(&d), raw.to_vec());
    assert!(!format!("{d:?}").is_empty());
}

#[test]
fn req_nfr_006_node_digest_layout_is_byte_exact() {
    assert_eq!(node_digest(&[]), sha256(&[1, 0, 0, 0, 0]));
    let da = leaf_digest(&[1, 2]);
    let d3 = node_digest(&[]);
    let mut pre = vec![1u8, 2, 0, 0, 0];
    pre.extend_from_slice(&[0, 1, 0, 0, 0, b'a']);
    pre.extend_from_slice(da.as_bytes());
    pre.extend_from_slice(&[1, 3, 0, 0, 0, 0, 0, 0, 0]);
    pre.extend_from_slice(d3.as_bytes());
    let expected = sha256(&pre);
    assert_eq!(node_digest(&[(n("a"), da), (ix(3), d3)]), expected);

    let mut t = MerkleTree::new();
    assert_eq!(t.root_digest(), node_digest(&[]));
    t.set_leaf(&path(&[n("a")]), vec![1, 2]).unwrap();
    t.set_leaf(&path(&[ix(3), n("tmp")]), vec![]).unwrap();
    assert!(t.remove(&path(&[ix(3), n("tmp")])));
    assert_eq!(t.root_digest(), expected);
    assert_eq!(t.full_root_digest(), expected);
}

#[test]
fn req_nfr_006_node_digest_depends_on_child_labels() {
    let d = leaf_digest(&[5]);
    assert_ne!(node_digest(&[(n("a"), d)]), node_digest(&[(n("b"), d)]));
    assert_ne!(node_digest(&[(ix(0), d)]), node_digest(&[(n("0"), d)]));
    assert_ne!(node_digest(&[(ix(1), d)]), node_digest(&[(ix(2), d)]));
    // A leaf never equals an internal node over the same bytes.
    assert_ne!(leaf_digest(&[1, 0, 0, 0, 0]), node_digest(&[]));
}

// ---------------------------------------------------------------------------------------------
// Labels and paths
// ---------------------------------------------------------------------------------------------

#[test]
fn req_nfr_006_label_ordering_display_and_encoding() {
    assert!(n("zzz") < ix(0), "every Name sorts before every Index");
    assert!(n("a") < n("b"));
    assert!(n("") < n("a"));
    assert!(n("B") < n("a"), "names compare bytewise");
    assert!(ix(2) < ix(10), "indexes compare numerically");
    assert_eq!(n("players"), Label::Name("players".to_owned()));
    assert_eq!(ix(4), Label::Index(4));
    assert_eq!(n("zone").to_string(), "zone");
    assert_eq!(ix(42).to_string(), "42");
    assert_eq!(encode_to_vec(&n("ab")), [0, 2, 0, 0, 0, b'a', b'b']);
    assert_eq!(encode_to_vec(&ix(0x0102)), [1, 2, 1, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn req_nfr_006_component_path_api_and_display() {
    let root = ComponentPath::root();
    assert!(root.is_empty());
    assert_eq!(root.len(), 0);
    assert_eq!(root, ComponentPath::default());
    assert_eq!(root.to_string(), "/");
    let p = root.child(n("players")).child(ix(2));
    assert_eq!(p.to_string(), "/players/2");
    assert_eq!(p.len(), 2);
    assert!(!p.is_empty());
    assert_eq!(p.labels(), &[n("players"), ix(2)]);
    assert_eq!(p, path(&[n("players"), ix(2)]));
    // Depth-first order: a path sorts before its descendants and after earlier siblings.
    assert!(path(&[n("a")]) < path(&[n("a"), ix(0)]));
    assert!(path(&[n("a"), ix(9)]) < path(&[n("b")]));
}

#[test]
fn req_nfr_006_path_error_display() {
    for e in [
        PathError::EmptyPath,
        PathError::ThroughLeaf,
        PathError::IsInternal,
    ] {
        assert!(!e.to_string().is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// Tree API and errors
// ---------------------------------------------------------------------------------------------

#[test]
fn req_loop_001_tree_accessors() {
    let mut t = MerkleTree::default();
    assert_eq!(t.hash_operations(), 0);
    assert_eq!(t.child_labels(&ComponentPath::root()), Some(vec![]));
    t.set_leaf_value(&path(&[ix(1), n("x")]), &0x0102u16)
        .unwrap();
    t.set_leaf(&path(&[n("b")]), vec![9]).unwrap();
    t.set_leaf_value(&path(&[n("a")]), "s").unwrap();
    assert_eq!(
        t.child_labels(&ComponentPath::root()),
        Some(vec![n("a"), n("b"), ix(1)])
    );
    assert_eq!(t.child_labels(&path(&[ix(1)])), Some(vec![n("x")]));
    assert_eq!(t.child_labels(&path(&[n("b")])), None);
    assert_eq!(t.child_labels(&path(&[n("nope")])), None);
    assert_eq!(t.leaf_payload(&path(&[ix(1), n("x")])), Some(&[2u8, 1][..]));
    assert_eq!(
        t.leaf_payload(&path(&[n("a")])),
        Some(&[1u8, 0, 0, 0, b's'][..])
    );
    assert_eq!(t.leaf_payload(&path(&[ix(1)])), None);
    assert_eq!(t.leaf_payload(&path(&[n("b"), n("under")])), None);
    assert_eq!(t.leaf_payload(&ComponentPath::root()), None);

    let root = t.digest_at(&ComponentPath::root());
    assert_eq!(root, Some(t.full_root_digest()));
    assert_eq!(t.digest_at(&path(&[n("b")])), Some(leaf_digest(&[9])));
    assert_eq!(
        t.digest_at(&path(&[ix(1)])),
        Some(node_digest(&[(n("x"), leaf_digest(&[2, 1]))]))
    );
    assert_eq!(t.digest_at(&path(&[n("missing")])), None);
    assert_eq!(t.digest_at(&path(&[n("b"), n("under")])), None);

    assert!(!t.remove(&ComponentPath::root()));
    assert!(!t.remove(&path(&[n("missing")])));
    assert!(!t.remove(&path(&[n("b"), n("under")])));
    assert!(!t.remove(&path(&[n("missing"), n("deeper")])));
    assert!(t.remove(&path(&[ix(1)])));
    assert_eq!(
        t.child_labels(&ComponentPath::root()),
        Some(vec![n("a"), n("b")])
    );
    assert_eq!(t.root_digest(), t.full_root_digest());
    assert!(!format!("{t:?}").is_empty());
}

/// Every non-root node with its payload (`None` for an internal node).
type NodeList = Vec<(ComponentPath, Option<Vec<u8>>)>;

/// Everything observable about a tree, for atomicity checks.
fn snapshot(t: &MerkleTree) -> (NodeList, Digest, u64) {
    (all_nodes(t), t.full_root_digest(), t.hash_operations())
}

#[test]
fn req_loop_001_set_leaf_errors_leave_tree_unchanged() {
    let mut t = MerkleTree::new();
    t.set_leaf(&path(&[n("leaf")]), vec![1]).unwrap();
    t.set_leaf(&path(&[n("group"), ix(0)]), vec![2]).unwrap();
    t.root_digest();
    let before = snapshot(&t);
    let cases = [
        (ComponentPath::root(), PathError::EmptyPath),
        (path(&[n("leaf"), n("x")]), PathError::ThroughLeaf),
        (path(&[n("leaf"), n("x"), n("y")]), PathError::ThroughLeaf),
        (path(&[n("group")]), PathError::IsInternal),
    ];
    for (p, err) in &cases {
        assert_eq!(t.set_leaf(p, vec![3]), Err(*err), "{p}");
        assert_eq!(t.set_leaf_value(p, &7u8), Err(*err), "{p}");
        assert_eq!(snapshot(&t), before, "{p}");
        // Nothing became dirty: a root digest costs zero operations.
        let ops = t.hash_operations();
        t.root_digest();
        assert_eq!(t.hash_operations(), ops, "{p}");
    }
    // Same on a dirty tree: the error must not create intermediates or change the digest.
    let mut d = MerkleTree::new();
    d.set_leaf(&path(&[n("a"), n("b")]), vec![1]).unwrap();
    let before = snapshot(&d);
    assert_eq!(
        d.set_leaf(&path(&[n("a"), n("b"), n("c"), n("d")]), vec![]),
        Err(PathError::ThroughLeaf)
    );
    assert_eq!(snapshot(&d), before);
    let mut fresh = d.clone();
    assert_eq!(d.root_digest(), fresh.root_digest());
    assert_eq!(d.hash_operations(), fresh.hash_operations());
}

// ---------------------------------------------------------------------------------------------
// Incremental cost
// ---------------------------------------------------------------------------------------------

#[test]
fn req_loop_001_incremental_cost_is_exact() {
    let mut t = MerkleTree::new();
    t.set_leaf(&path(&[n("a")]), vec![1]).unwrap();
    t.set_leaf(&path(&[n("b"), n("c")]), vec![2]).unwrap();
    t.set_leaf(&path(&[n("d"), ix(0), n("f")]), vec![3])
        .unwrap();
    t.set_leaf(&path(&[n("d"), ix(0), n("g")]), vec![4])
        .unwrap();
    // 4 leaves + 4 internal nodes (root, b, d, d/0).
    t.root_digest();
    assert_eq!(t.hash_operations(), 8);
    let mut ops = t.hash_operations();
    let mut step = |t: &mut MerkleTree, expected: u64| {
        let d = t.root_digest();
        assert_eq!(t.hash_operations() - ops, expected);
        assert_eq!(d, t.full_root_digest());
        ops = t.hash_operations();
    };
    step(&mut t, 0);
    t.set_leaf(&path(&[n("a")]), vec![10]).unwrap();
    step(&mut t, 2);
    t.set_leaf(&path(&[n("b"), n("c")]), vec![20]).unwrap();
    step(&mut t, 3);
    t.set_leaf(&path(&[n("d"), ix(0), n("f")]), vec![30])
        .unwrap();
    step(&mut t, 4);
    // No-op update: same payload.
    t.set_leaf(&path(&[n("d"), ix(0), n("f")]), vec![30])
        .unwrap();
    step(&mut t, 0);
    // Remove a leaf at depth 3: its 3 ancestors.
    assert!(t.remove(&path(&[n("d"), ix(0), n("g")])));
    step(&mut t, 3);
    // Remove a whole subtree at depth 1: only the root.
    assert!(t.remove(&path(&[n("d")])));
    step(&mut t, 1);
    step(&mut t, 0);
    // A new leaf under new intermediates: leaf + 2 new internal + root.
    t.set_leaf(&path(&[n("x"), n("y"), n("z")]), vec![])
        .unwrap();
    step(&mut t, 4);
    // Two updates in one subtree share their ancestors.
    t.set_leaf(&path(&[n("x"), n("y"), n("z")]), vec![1])
        .unwrap();
    t.set_leaf(&path(&[n("x"), n("y"), n("w")]), vec![1])
        .unwrap();
    step(&mut t, 5);
}

#[test]
fn req_loop_001_digest_at_counts_only_its_subtree_and_clones_copy_the_count() {
    let mut t = MerkleTree::new();
    t.set_leaf(&path(&[n("p"), n("q")]), vec![1]).unwrap();
    t.set_leaf(&path(&[n("r")]), vec![2]).unwrap();
    assert_eq!(
        t.digest_at(&path(&[n("p")])),
        Some(node_digest(&[(n("q"), leaf_digest(&[1]))]))
    );
    assert_eq!(t.hash_operations(), 2);
    t.root_digest();
    assert_eq!(
        t.hash_operations(),
        4,
        "p is cached: only r and the root remain"
    );
    let mut c = t.clone();
    assert_eq!(c.hash_operations(), 4);
    c.set_leaf(&path(&[n("r")]), vec![3]).unwrap();
    c.root_digest();
    assert_eq!(c.hash_operations(), 6);
    assert_eq!(t.hash_operations(), 4);
    assert_ne!(c.root_digest(), t.root_digest());
    // full_root_digest and diff are not counted.
    let _ = t.full_root_digest();
    let _ = diff_first_divergence(&t, &c);
    assert_eq!(t.hash_operations(), 4);
    assert_eq!(c.hash_operations(), 6);
}

// ---------------------------------------------------------------------------------------------
// Reference helpers (walk the private nodes directly; no digests)
// ---------------------------------------------------------------------------------------------

/// Every non-root node: leaves with `Some(payload)`, internal nodes with `None`, depth-first.
fn all_nodes(t: &MerkleTree) -> NodeList {
    fn walk(node: &Node, p: &ComponentPath, out: &mut NodeList) {
        if let Node::Internal { children, .. } = node {
            for (label, child) in children {
                let cp = p.child(label.clone());
                match child {
                    Node::Leaf { payload, .. } => out.push((cp, Some(payload.clone()))),
                    Node::Internal { .. } => {
                        out.push((cp.clone(), None));
                        walk(child, &cp, out);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&t.root, &ComponentPath::root(), &mut out);
    out
}

fn leaves(t: &MerkleTree) -> Vec<ComponentPath> {
    all_nodes(t)
        .into_iter()
        .filter(|(_, p)| p.is_some())
        .map(|(p, _)| p)
        .collect()
}

/// Rebuilds `t` from scratch, inserting its leaves (and its empty internal nodes) in reverse
/// depth-first order.
fn rebuild(t: &MerkleTree) -> MerkleTree {
    let nodes = all_nodes(t);
    let mut empty_internal = Vec::new();
    for (i, (p, _)) in nodes.iter().enumerate() {
        let has_child = nodes
            .get(i + 1)
            .is_some_and(|(q, _)| q.len() > p.len() && q.labels().starts_with(p.labels()));
        if nodes[i].1.is_none() && !has_child {
            empty_internal.push(p.clone());
        }
    }
    let mut r = MerkleTree::new();
    for (p, payload) in nodes.iter().rev() {
        if let Some(payload) = payload {
            r.set_leaf(p, payload.clone()).unwrap();
        }
    }
    for p in empty_internal.iter().rev() {
        let tmp = p.child(n("__tmp"));
        r.set_leaf(&tmp, vec![]).unwrap();
        assert!(r.remove(&tmp));
    }
    r
}

/// The B4 definition, applied literally on the node structure.
fn reference_diff(a: &MerkleTree, b: &MerkleTree) -> Option<ComponentPath> {
    fn go(a: &Node, b: &Node, p: &ComponentPath) -> Option<ComponentPath> {
        match (a, b) {
            (Node::Leaf { payload: x, .. }, Node::Leaf { payload: y, .. }) => {
                (x != y).then(|| p.clone())
            }
            (Node::Internal { children: ca, .. }, Node::Internal { children: cb, .. }) => {
                let labels: std::collections::BTreeSet<&Label> =
                    ca.keys().chain(cb.keys()).collect();
                for label in labels {
                    let cp = p.child(label.clone());
                    match (ca.get(label), cb.get(label)) {
                        (Some(x), Some(y)) => {
                            if let Some(r) = go(x, y, &cp) {
                                return Some(r);
                            }
                        }
                        _ => return Some(cp),
                    }
                }
                None
            }
            _ => Some(p.clone()),
        }
    }
    go(&a.root, &b.root, &ComponentPath::root())
}

// ---------------------------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------------------------

fn below(rng: &mut Pcg64Dxsm, k: u64) -> u64 {
    rng.next_bounded(k).unwrap_or(0)
}

fn gen_label(rng: &mut Pcg64Dxsm) -> Label {
    match below(rng, 6) {
        0 => n("a"),
        1 => n("b"),
        2 => n(""),
        3 => ix(0),
        4 => ix(1),
        _ => ix(7),
    }
}

fn gen_path(rng: &mut Pcg64Dxsm) -> ComponentPath {
    let depth = 1 + below(rng, 3);
    ComponentPath::new((0..depth).map(|_| gen_label(rng)).collect())
}

fn gen_payload(rng: &mut Pcg64Dxsm) -> Vec<u8> {
    (0..below(rng, 3))
        .map(|_| u8::try_from(below(rng, 3)).unwrap_or(0))
        .collect()
}

/// A random tree with `min + below(spread)` leaf insertions (some may fail), sometimes a
/// removal, sometimes cleaned.
fn gen_tree(rng: &mut Pcg64Dxsm, min: u64, spread: u64) -> MerkleTree {
    let leaves = min + below(rng, spread);
    let mut t = MerkleTree::new();
    for _ in 0..leaves {
        let _ = t.set_leaf(&gen_path(rng), gen_payload(rng));
    }
    if below(rng, 3) == 0 {
        let p = gen_path(rng);
        let _ = t.remove(&p);
    }
    if below(rng, 2) == 0 {
        t.root_digest();
    }
    t
}

fn pick<T: Clone>(rng: &mut Pcg64Dxsm, items: &[T]) -> Option<T> {
    rng.choose(items).cloned()
}

// ---------------------------------------------------------------------------------------------
// Property: incremental == full recompute
// ---------------------------------------------------------------------------------------------

#[test]
fn req_loop_001_req_nfr_006_property_incremental_equals_full_recompute() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xfeed_0010);
    let mut steps = 0usize;
    while steps < ITERATIONS {
        let mut trees = vec![MerkleTree::new()];
        for _ in 0..40 {
            steps += 1;
            if below(&mut rng, 10) == 0 && trees.len() < 4 {
                let i = usize::try_from(below(&mut rng, trees.len() as u64)).unwrap_or(0);
                trees.push(trees[i].clone());
            }
            for t in &mut trees {
                match below(&mut rng, 6) {
                    0..=2 => {
                        let p = gen_path(&mut rng);
                        let before = snapshot(t);
                        if t.set_leaf(&p, gen_payload(&mut rng)).is_err() {
                            assert_eq!(snapshot(t), before);
                        }
                    }
                    3 => {
                        let target = match below(&mut rng, 2) {
                            0 => pick(&mut rng, &leaves(t)).unwrap_or_else(|| gen_path(&mut rng)),
                            _ => ComponentPath::root().child(gen_label(&mut rng)),
                        };
                        let existed = all_nodes(t).iter().any(|(q, _)| *q == target);
                        assert_eq!(t.remove(&target), existed);
                    }
                    4 => {
                        let d = t.root_digest();
                        assert_eq!(t.root_digest(), d);
                    }
                    _ => {}
                }
                let full = t.full_root_digest();
                assert_eq!(t.root_digest(), full);
                assert_eq!(rebuild(t).root_digest(), full);
                assert_eq!(rebuild(t).full_root_digest(), full);
            }
        }
    }
}

#[test]
fn req_loop_001_insertion_order_does_not_change_the_digest() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xfeed_0011);
    for _ in 0..ITERATIONS / 5 {
        let entries: BTreeMap<u64, Vec<u8>> = (0..below(&mut rng, 8))
            .map(|_| (below(&mut rng, 16), gen_payload(&mut rng)))
            .collect();
        let mut fwd = MerkleTree::new();
        let mut rev = MerkleTree::new();
        for (k, v) in &entries {
            fwd.set_leaf_value(&path(&[n("seat"), ix(*k)]), v).unwrap();
        }
        for (k, v) in entries.iter().rev() {
            rev.set_leaf_value(&path(&[n("seat"), ix(*k)]), v).unwrap();
        }
        assert_eq!(fwd.root_digest(), rev.root_digest());
        assert_eq!(diff_first_divergence(&fwd, &rev), None);
    }
}

#[test]
fn req_loop_001_set_leaf_value_uses_canonical_encoding() {
    let mut a = MerkleTree::new();
    let mut b = MerkleTree::new();
    let value = (7u8, Some("x".to_owned()), vec![1i64, -1]);
    a.set_leaf_value(&path(&[n("v")]), &value).unwrap();
    b.set_leaf(&path(&[n("v")]), encode_to_vec(&value)).unwrap();
    assert_eq!(a.root_digest(), b.root_digest());
    let rng = Pcg64Dxsm::from_u64_seed(3);
    let mut prng_bytes = Vec::new();
    rng.encode_canonical(&mut prng_bytes);
    a.set_leaf_value(&path(&[n("prng")]), &rng).unwrap();
    assert_eq!(
        a.leaf_payload(&path(&[n("prng")])),
        Some(prng_bytes.as_slice())
    );
    assert_eq!(prng_bytes.len(), 32);
}

// ---------------------------------------------------------------------------------------------
// Divergence
// ---------------------------------------------------------------------------------------------

#[test]
fn req_nfr_006_diff_examples() {
    let mut a = MerkleTree::new();
    a.set_leaf(&path(&[n("players"), ix(1), n("life")]), vec![20])
        .unwrap();
    a.set_leaf(&path(&[n("players"), ix(2), n("life")]), vec![20])
        .unwrap();
    a.set_leaf(&path(&[n("stack")]), vec![]).unwrap();
    let mut b = a.clone();
    assert_eq!(diff_first_divergence(&a, &b), None);
    b.set_leaf(&path(&[n("players"), ix(2), n("life")]), vec![19])
        .unwrap();
    let p = diff_first_divergence(&a, &b).unwrap();
    assert_eq!(p.to_string(), "/players/2/life");
    b.set_leaf(&path(&[n("stack")]), vec![1]).unwrap();
    assert_eq!(
        diff_first_divergence(&a, &b).unwrap().to_string(),
        "/players/2/life"
    );
    b.set_leaf(&path(&[n("players"), ix(0), n("life")]), vec![1])
        .unwrap();
    assert_eq!(
        diff_first_divergence(&b, &a).unwrap().to_string(),
        "/players/0"
    );
    // Leaf versus internal at the same path.
    let mut c = a.clone();
    assert!(c.remove(&path(&[n("stack")])));
    c.set_leaf(&path(&[n("stack"), ix(0)]), vec![]).unwrap();
    assert_eq!(diff_first_divergence(&a, &c).unwrap().to_string(), "/stack");
    // Empty trees, and an emptied internal node versus a missing one.
    assert_eq!(
        diff_first_divergence(&MerkleTree::new(), &MerkleTree::new()),
        None
    );
    let mut e = MerkleTree::new();
    e.set_leaf(&path(&[n("z"), n("y")]), vec![]).unwrap();
    assert!(e.remove(&path(&[n("z"), n("y")])));
    assert_eq!(
        diff_first_divergence(&e, &MerkleTree::new())
            .unwrap()
            .to_string(),
        "/z"
    );
}

#[test]
fn req_nfr_006_diff_ignores_cache_state() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xfeed_0020);
    for _ in 0..ITERATIONS / 5 {
        let mut a = gen_tree(&mut rng, 1, 8);
        let mut b = rebuild(&a);
        // Stale everywhere versus clean, both ways, and identical caches.
        assert_eq!(diff_first_divergence(&a, &b), None);
        a.root_digest();
        assert_eq!(diff_first_divergence(&a, &b), None);
        assert_eq!(diff_first_divergence(&b, &a), None);
        b.root_digest();
        assert_eq!(diff_first_divergence(&a, &b), None);
        // A partly dirty tree: mutate and restore one leaf.
        if let Some(p) = pick(&mut rng, &leaves(&a)) {
            let old = a.leaf_payload(&p).unwrap().to_vec();
            let mut changed = old.clone();
            changed.push(0xee);
            a.set_leaf(&p, changed).unwrap();
            assert_eq!(diff_first_divergence(&a, &b), Some(p.clone()));
            a.root_digest();
            a.set_leaf(&p, old).unwrap();
            assert_eq!(diff_first_divergence(&a, &b), None);
            assert_eq!(diff_first_divergence(&b, &a), None);
        }
    }
}

/// The first prefix of `p` that does not exist in `t`.
fn first_missing_prefix(t: &MerkleTree, p: &ComponentPath) -> Option<ComponentPath> {
    let existing: Vec<ComponentPath> = all_nodes(t).into_iter().map(|(q, _)| q).collect();
    (1..=p.len())
        .map(|k| ComponentPath::new(p.labels()[..k].to_vec()))
        .find(|q| !existing.contains(q))
}

/// Applies one random mutation to `b` and returns the path `diff_first_divergence` must report.
fn mutate(rng: &mut Pcg64Dxsm, b: &mut MerkleTree) -> Option<ComponentPath> {
    let leaf_paths = leaves(b);
    match below(rng, 4) {
        0 => {
            let p = pick(rng, &leaf_paths)?;
            let mut payload = b.leaf_payload(&p)?.to_vec();
            payload.push(0xaa);
            b.set_leaf(&p, payload).ok()?;
            Some(p)
        }
        1 => {
            let p = gen_path(rng);
            let expected = first_missing_prefix(b, &p)?;
            b.set_leaf(&p, gen_payload(rng)).ok()?;
            Some(expected)
        }
        2 => {
            let p = pick(rng, &leaf_paths)?;
            assert!(b.remove(&p));
            Some(p)
        }
        _ => {
            let p = pick(rng, &leaf_paths)?;
            assert!(b.remove(&p));
            b.set_leaf(&p.child(gen_label(rng)), gen_payload(rng))
                .ok()?;
            Some(p)
        }
    }
}

#[test]
fn req_nfr_006_property_diff_finds_the_exact_single_mutation() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xfeed_0030);
    let mut checked = 0usize;
    while checked < ITERATIONS {
        let a = gen_tree(&mut rng, 0, 8);
        let mut b = a.clone();
        if below(&mut rng, 2) == 0 {
            b.root_digest();
        }
        let Some(expected) = mutate(&mut rng, &mut b) else {
            continue;
        };
        if below(&mut rng, 2) == 0 {
            b.root_digest();
        }
        assert_eq!(diff_first_divergence(&a, &b), Some(expected.clone()));
        assert_eq!(diff_first_divergence(&b, &a), Some(expected.clone()));
        assert_eq!(reference_diff(&a, &b), Some(expected));
        checked += 1;
    }
}

#[test]
fn req_nfr_006_property_diff_reports_the_first_of_two_mutations() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xfeed_0031);
    let mut checked = 0usize;
    while checked < ITERATIONS {
        let a = gen_tree(&mut rng, 2, 8);
        let leaf_paths = leaves(&a);
        let (Some(p), Some(q)) = (pick(&mut rng, &leaf_paths), pick(&mut rng, &leaf_paths)) else {
            continue;
        };
        if p == q {
            continue;
        }
        let mut b = a.clone();
        for target in [&q, &p] {
            let mut payload = b.leaf_payload(target).unwrap().to_vec();
            payload.push(0x55);
            b.set_leaf(target, payload).unwrap();
        }
        let expected = p.clone().min(q.clone());
        assert_eq!(diff_first_divergence(&a, &b), Some(expected.clone()));
        assert_eq!(diff_first_divergence(&b, &a), Some(expected));
        checked += 1;
    }
}

#[test]
fn req_nfr_006_property_diff_matches_reference_on_random_pairs() {
    let mut rng = Pcg64Dxsm::from_u64_seed(0xfeed_0032);
    let mut differing = 0usize;
    for _ in 0..ITERATIONS {
        let a = gen_tree(&mut rng, 0, 6);
        let b = gen_tree(&mut rng, 0, 6);
        let got = diff_first_divergence(&a, &b);
        assert_eq!(got, reference_diff(&a, &b));
        assert_eq!(got.is_none(), a.full_root_digest() == b.full_root_digest());
        differing += usize::from(got.is_some());
    }
    assert!(differing > ITERATIONS / 2);
}
