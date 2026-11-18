// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_base::hash` (SHA-256, Merkle digests, incremental cost, divergence).
//! Black-box: written from the work-item brief only; a reference model recomputes every digest.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::encode_to_vec;
use fineprint_base::hash::{
    ComponentPath, Digest, Label, MerkleTree, PathError, diff_first_divergence, leaf_digest,
    node_digest, sha256,
};
use fineprint_base::prng::Pcg64Dxsm;

// ---------------------------------------------------------------- reference model

#[derive(Clone, Debug, PartialEq, Eq)]
enum M {
    Leaf(Vec<u8>),
    Node(BTreeMap<Label, M>),
}

fn label_bytes(l: &Label) -> Vec<u8> {
    match l {
        Label::Name(s) => {
            let mut v = vec![0u8];
            v.extend((s.len() as u32).to_le_bytes());
            v.extend(s.as_bytes());
            v
        }
        Label::Index(i) => {
            let mut v = vec![1u8];
            v.extend(i.to_le_bytes());
            v
        }
    }
}

fn m_digest(m: &M) -> Digest {
    match m {
        M::Leaf(p) => {
            let mut v = vec![0u8];
            v.extend(p);
            sha256(&v)
        }
        M::Node(c) => {
            let mut v = vec![1u8];
            v.extend((c.len() as u32).to_le_bytes());
            for (l, d) in c {
                v.extend(label_bytes(l));
                v.extend(m_digest(d).as_bytes());
            }
            sha256(&v)
        }
    }
}

fn m_new() -> M {
    M::Node(BTreeMap::new())
}

fn m_get<'a>(root: &'a M, path: &[Label]) -> Option<&'a M> {
    let mut cur = root;
    for l in path {
        match cur {
            M::Node(c) => cur = c.get(l)?,
            M::Leaf(_) => return None,
        }
    }
    Some(cur)
}

fn m_set(root: &mut M, path: &[Label], payload: Vec<u8>) -> Result<(), PathError> {
    if path.is_empty() {
        return Err(PathError::EmptyPath);
    }
    // validate first (atomic)
    {
        let mut cur = &*root;
        for (i, l) in path.iter().enumerate() {
            let last = i + 1 == path.len();
            let M::Node(c) = cur else { unreachable!() };
            match c.get(l) {
                Some(M::Node(_)) if last => return Err(PathError::IsInternal),
                Some(M::Leaf(_)) if !last => return Err(PathError::ThroughLeaf),
                Some(next) => cur = next,
                None => break,
            }
        }
    }
    let mut cur = root;
    for (i, l) in path.iter().enumerate() {
        let last = i + 1 == path.len();
        let M::Node(c) = cur else { unreachable!() };
        if last {
            c.insert(l.clone(), M::Leaf(payload));
            return Ok(());
        }
        cur = c.entry(l.clone()).or_insert_with(m_new);
    }
    unreachable!()
}

fn m_remove(root: &mut M, path: &[Label]) -> bool {
    let Some((last, parent)) = path.split_last() else {
        return false;
    };
    let mut cur = root;
    for l in parent {
        match cur {
            M::Node(c) => match c.get_mut(l) {
                Some(n) => cur = n,
                None => return false,
            },
            M::Leaf(_) => return false,
        }
    }
    match cur {
        M::Node(c) => c.remove(last).is_some(),
        M::Leaf(_) => false,
    }
}

fn m_leaves(m: &M, prefix: &mut Vec<Label>, out: &mut Vec<(Vec<Label>, Vec<u8>)>) {
    match m {
        M::Leaf(p) => out.push((prefix.clone(), p.clone())),
        M::Node(c) => {
            for (l, n) in c {
                prefix.push(l.clone());
                m_leaves(n, prefix, out);
                prefix.pop();
            }
        }
    }
}

fn m_all_paths(m: &M, prefix: &mut Vec<Label>, out: &mut Vec<Vec<Label>>) {
    out.push(prefix.clone());
    if let M::Node(c) = m {
        for (l, n) in c {
            prefix.push(l.clone());
            m_all_paths(n, prefix, out);
            prefix.pop();
        }
    }
}

fn m_has_empty_internal_below_root(m: &M, is_root: bool) -> bool {
    match m {
        M::Leaf(_) => false,
        M::Node(c) => {
            (c.is_empty() && !is_root)
                || c.values()
                    .any(|n| m_has_empty_internal_below_root(n, false))
        }
    }
}

/// Counts (leaves, internal nodes including the root).
fn m_counts(m: &M) -> (u64, u64) {
    match m {
        M::Leaf(_) => (1, 0),
        M::Node(c) => c.values().fold((0, 1), |(l, i), n| {
            let (a, b) = m_counts(n);
            (l + a, i + b)
        }),
    }
}

/// Oracle for the first divergence: depth-first, ascending label order.
fn oracle(a: &M, b: &M, path: &mut Vec<Label>) -> Option<Vec<Label>> {
    match (a, b) {
        (M::Leaf(x), M::Leaf(y)) => (x != y).then(|| path.clone()),
        (M::Node(ca), M::Node(cb)) => {
            let keys: BTreeSet<&Label> = ca.keys().chain(cb.keys()).collect();
            for k in keys {
                match (ca.get(k), cb.get(k)) {
                    (Some(x), Some(y)) => {
                        path.push(k.clone());
                        let r = oracle(x, y, path);
                        path.pop();
                        if r.is_some() {
                            return r;
                        }
                    }
                    _ => {
                        let mut p = path.clone();
                        p.push(k.clone());
                        return Some(p);
                    }
                }
            }
            None
        }
        _ => Some(path.clone()),
    }
}

fn m_oracle(a: &M, b: &M) -> Option<Vec<Label>> {
    oracle(a, b, &mut Vec::new())
}

// ---------------------------------------------------------------- helpers

struct Gen(Pcg64Dxsm);
impl Gen {
    fn new(seed: u64) -> Gen {
        Gen(Pcg64Dxsm::from_u64_seed(seed))
    }
    fn below(&mut self, n: u64) -> u64 {
        self.0.next_u64() % n
    }
    fn label(&mut self) -> Label {
        match self.below(6) {
            0 => Label::name("a"),
            1 => Label::name("b"),
            2 => Label::name("zone_a"),
            3 => Label::index(0),
            4 => Label::index(1),
            _ => Label::index(10),
        }
    }
    fn path(&mut self) -> Vec<Label> {
        let d = 1 + self.below(4) as usize;
        (0..d).map(|_| self.label()).collect()
    }
    fn payload(&mut self) -> Vec<u8> {
        let n = self.below(3) as usize;
        (0..n).map(|_| self.below(2) as u8).collect()
    }
}

fn cp(labels: &[Label]) -> ComponentPath {
    ComponentPath::new(labels.to_vec())
}

fn names(parts: &[&str]) -> Vec<Label> {
    parts.iter().map(|s| Label::name(s)).collect()
}

/// A tree and its model kept in lockstep.
#[derive(Clone)]
struct Pair {
    tree: MerkleTree,
    model: M,
}
impl Pair {
    fn new() -> Pair {
        Pair {
            tree: MerkleTree::new(),
            model: m_new(),
        }
    }
    fn set(&mut self, path: &[Label], payload: Vec<u8>) -> Result<(), PathError> {
        let r = self.tree.set_leaf(&cp(path), payload.clone());
        assert_eq!(
            r,
            m_set(&mut self.model, path, payload),
            "set_leaf result at {path:?}"
        );
        r
    }
    fn remove(&mut self, path: &[Label]) -> bool {
        let r = self.tree.remove(&cp(path));
        assert_eq!(
            r,
            m_remove(&mut self.model, path),
            "remove result at {path:?}"
        );
        r
    }
    fn random(g: &mut Gen) -> Pair {
        let mut p = Pair::new();
        for _ in 0..g.below(12) {
            let path = g.path();
            let payload = g.payload();
            let _ = p.set(&path, payload);
        }
        p
    }
    fn leaf_paths(&self) -> Vec<Vec<Label>> {
        let mut out = Vec::new();
        m_leaves(&self.model, &mut Vec::new(), &mut out);
        out.into_iter().map(|(p, _)| p).collect()
    }
}

type Snap = (Vec<Label>, Option<Vec<Label>>, Option<Vec<u8>>);

fn ops(t: &MerkleTree) -> u64 {
    t.hash_operations()
}

fn clean(t: &MerkleTree) -> MerkleTree {
    let mut c = t.clone();
    c.root_digest();
    c
}

/// Fixed fixture: leaves /a/x /a/y /b /c/d/e/f /c/d/g ; internals root, a, c, c/d, c/d/e.
fn fixture() -> Pair {
    let mut p = Pair::new();
    p.set(&names(&["a", "x"]), vec![1]).unwrap();
    p.set(&names(&["a", "y"]), vec![2]).unwrap();
    p.set(&names(&["b"]), vec![3]).unwrap();
    p.set(&names(&["c", "d", "e", "f"]), vec![4]).unwrap();
    p.set(&names(&["c", "d", "g"]), vec![5]).unwrap();
    p
}

fn assert_matches_model(p: &mut Pair) {
    let want = m_digest(&p.model);
    assert_eq!(p.tree.full_root_digest(), want, "full_root_digest vs model");
    assert_eq!(p.tree.root_digest(), want, "root_digest vs model");
    assert_eq!(p.tree.digest_at(&ComponentPath::root()), Some(want));
}

// ---------------------------------------------------------------- 1. SHA-256 vectors

/// REQ-NFR-006: NIST / FIPS 180-4 SHA-256 vectors and leaf-prefix equivalence.
#[test]
fn req_nfr_006_sha256_published_vectors() {
    assert_eq!(
        sha256(b"").to_hex(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256(b"abc").to_hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq").to_hex(),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    let million = vec![b'a'; 1_000_000];
    assert_eq!(
        sha256(&million).to_hex(),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
    assert_eq!(
        leaf_digest(&[]).to_hex(),
        "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"
    );
    assert_eq!(leaf_digest(&[]), sha256(&[0]));
}

/// REQ-NFR-006: leaf_digest(p) == sha256(0x00 || p); hex form is lowercase 64 chars.
#[test]
fn req_nfr_006_leaf_digest_prefix_and_hex_format() {
    let mut g = Gen::new(11);
    for len in [0usize, 1, 55, 56, 63, 64, 65, 119, 120, 10_000] {
        let p: Vec<u8> = (0..len).map(|_| g.below(256) as u8).collect();
        let mut pre = vec![0u8];
        pre.extend(&p);
        let d = leaf_digest(&p);
        assert_eq!(d, sha256(&pre));
        let h = d.to_hex();
        assert_eq!(h.len(), 64);
        assert!(
            h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert_eq!(format!("{d}"), h);
    }
}

// ---------------------------------------------------------------- 4. digest layout

/// REQ-NFR-006: node and leaf digests match hand-assembled preimages.
#[test]
fn req_nfr_006_digest_layout_exact() {
    assert_eq!(node_digest(&[]), sha256(&[1, 0, 0, 0, 0]));
    let c1 = leaf_digest(b"one");
    let c2 = leaf_digest(b"two");
    let c3 = leaf_digest(b"three");

    let mut pre = vec![1u8, 1, 0, 0, 0, 0, 4, 0, 0, 0];
    pre.extend(b"zone");
    pre.extend(c1.as_bytes());
    assert_eq!(node_digest(&[(Label::name("zone"), c1)]), sha256(&pre));

    let mut pre = vec![1u8, 1, 0, 0, 0, 1, 7, 0, 0, 0, 0, 0, 0, 0];
    pre.extend(c2.as_bytes());
    assert_eq!(node_digest(&[(Label::index(7), c2)]), sha256(&pre));

    let kids = [
        (Label::name(""), c1),
        (Label::name("a"), c2),
        (Label::name("b"), c1),
        (Label::index(0), c3),
        (Label::index(300), c2),
    ];
    let mut pre = vec![1u8, 5, 0, 0, 0];
    pre.extend([0, 0, 0, 0, 0]);
    pre.extend(c1.as_bytes());
    pre.extend([0, 1, 0, 0, 0, b'a']);
    pre.extend(c2.as_bytes());
    pre.extend([0, 1, 0, 0, 0, b'b']);
    pre.extend(c1.as_bytes());
    pre.extend([1, 0, 0, 0, 0, 0, 0, 0, 0]);
    pre.extend(c3.as_bytes());
    pre.extend([1, 0x2C, 1, 0, 0, 0, 0, 0, 0]);
    pre.extend(c2.as_bytes());
    assert_eq!(node_digest(&kids), sha256(&pre));
}

/// REQ-NFR-006: label and child digest both feed the node digest; leaf and node hashes are separated.
#[test]
fn req_nfr_006_digest_sensitivity_and_separation() {
    let c1 = leaf_digest(b"one");
    let c2 = leaf_digest(b"two");
    let base = node_digest(&[(Label::name("a"), c1)]);
    assert_ne!(base, node_digest(&[(Label::name("b"), c1)]));
    assert_ne!(base, node_digest(&[(Label::index(0), c1)]));
    assert_ne!(base, node_digest(&[(Label::name("a"), c2)]));
    assert_ne!(
        node_digest(&[(Label::name("1"), c1)]),
        node_digest(&[(Label::index(0x31), c1)])
    );

    let mut g = Gen::new(77);
    let mut node_set = vec![node_digest(&[]), base];
    for _ in 0..50 {
        let kids: Vec<(Label, Digest)> = (0..g.below(3))
            .map(|i| (Label::index(i), leaf_digest(&g.payload())))
            .collect();
        node_set.push(node_digest(&kids));
    }
    for _ in 0..300 {
        let p = g.payload();
        let l = leaf_digest(&p);
        assert!(!node_set.contains(&l));
    }
    assert_ne!(leaf_digest(&[0, 0, 0, 0]), node_digest(&[]));
    assert_ne!(leaf_digest(&[]), node_digest(&[]));
}

/// REQ-NFR-006: the tree's digests follow the reference model on a fixed fixture, per node.
#[test]
fn req_nfr_006_tree_digest_matches_reference_model() {
    let mut p = fixture();
    assert_matches_model(&mut p);
    let mut all = Vec::new();
    m_all_paths(&p.model, &mut Vec::new(), &mut all);
    for path in all {
        let want = m_get(&p.model, &path).map(m_digest);
        assert_eq!(p.tree.digest_at(&cp(&path)), want, "digest_at {path:?}");
    }
    assert_eq!(p.tree.digest_at(&cp(&names(&["nope"]))), None);
    assert_eq!(p.tree.digest_at(&cp(&names(&["b", "deeper"]))), None);
    // empty tree
    let mut t = MerkleTree::new();
    assert_eq!(t.root_digest(), node_digest(&[]));
    assert_eq!(t.full_root_digest(), sha256(&[1, 0, 0, 0, 0]));
    assert_eq!(MerkleTree::default().root_digest(), t.root_digest());
}

// ---------------------------------------------------------------- 5. incremental == full

/// REQ-NFR-006, REQ-LOOP-001: incremental root equals full recompute and the model after random edits.
#[test]
fn req_loop_001_incremental_equals_full_recompute_random() {
    let mut total = 0;
    for seed in [1u64, 2, 3, 4, 5, 0xDEAD_BEEF] {
        let mut g = Gen::new(seed);
        let mut p = Pair::new();
        for step in 0..1200 {
            total += 1;
            let op = g.below(10);
            match op {
                0..=4 => {
                    let path = g.path();
                    let payload = g.payload();
                    let _ = p.set(&path, payload);
                }
                5 => {
                    let path = g.path();
                    let v = g.below(4) as u32;
                    let r = p.tree.set_leaf_value(&cp(&path), &v);
                    let want = m_set(&mut p.model, &path, encode_to_vec(&v));
                    assert_eq!(r, want);
                }
                6 | 7 => {
                    // bias towards existing paths half the time
                    let path = if g.below(2) == 0 {
                        g.path()
                    } else {
                        let mut all = Vec::new();
                        m_all_paths(&p.model, &mut Vec::new(), &mut all);
                        let i = g.below(all.len() as u64) as usize;
                        all[i].clone()
                    };
                    p.remove(&path);
                }
                8 => {
                    let want = m_digest(&p.model);
                    assert_eq!(p.tree.root_digest(), want);
                }
                _ => {
                    let path = g.path();
                    let want = m_get(&p.model, &path).map(m_digest);
                    assert_eq!(p.tree.digest_at(&cp(&path)), want, "digest_at {path:?}");
                }
            }
            let want = m_digest(&p.model);
            assert_eq!(
                p.tree.full_root_digest(),
                want,
                "full at seed {seed} step {step}"
            );
            if step % 3 == 0 || op >= 8 {
                assert_eq!(
                    p.tree.root_digest(),
                    want,
                    "incremental at seed {seed} step {step}"
                );
            }
            // structural queries
            let probe = if g.below(2) == 0 {
                g.path()
            } else {
                let mut all = Vec::new();
                m_all_paths(&p.model, &mut Vec::new(), &mut all);
                let i = g.below(all.len() as u64) as usize;
                all[i].clone()
            };
            let node = m_get(&p.model, &probe);
            let want_payload = match node {
                Some(M::Leaf(b)) => Some(b.as_slice()),
                _ => None,
            };
            assert_eq!(p.tree.leaf_payload(&cp(&probe)), want_payload);
            let want_children = match node {
                Some(M::Node(c)) => Some(c.keys().cloned().collect::<Vec<_>>()),
                _ => None,
            };
            assert_eq!(p.tree.child_labels(&cp(&probe)), want_children);
        }
        assert_eq!(p.tree.root_digest(), m_digest(&p.model));
    }
    assert!(total >= 5000);
}

/// REQ-NFR-006, REQ-LOOP-001: a tree rebuilt from scratch in another insertion order has the same digest.
#[test]
fn req_loop_001_rebuild_in_different_order_equal() {
    let mut g = Gen::new(2024);
    for _ in 0..300 {
        let mut p = Pair::random(&mut g);
        // remove a few to exercise history
        for _ in 0..g.below(3) {
            let paths = p.leaf_paths();
            if !paths.is_empty() {
                let i = g.below(paths.len() as u64) as usize;
                p.remove(&paths[i]);
            }
        }
        let mut leaves = Vec::new();
        m_leaves(&p.model, &mut Vec::new(), &mut leaves);
        let mut order = leaves.clone();
        // deterministic shuffle
        for i in (1..order.len()).rev() {
            let j = g.below(i as u64 + 1) as usize;
            order.swap(i, j);
        }
        for ord in [order, leaves.iter().rev().cloned().collect::<Vec<_>>()] {
            let mut rebuilt = MerkleTree::new();
            for (path, payload) in &ord {
                rebuilt.set_leaf(&cp(path), payload.clone()).unwrap();
            }
            if !m_has_empty_internal_below_root(&p.model, true) {
                let mut orig = p.tree.clone();
                assert_eq!(rebuilt.root_digest(), orig.root_digest());
                assert_eq!(rebuilt.full_root_digest(), p.tree.full_root_digest());
                assert_eq!(diff_first_divergence(&rebuilt, &p.tree), None);
            }
        }
    }
}

// ---------------------------------------------------------------- 6. incremental cost

/// REQ-LOOP-001: exact hash_operations deltas for first, repeat and single-leaf updates.
#[test]
fn req_loop_001_cost_first_repeat_and_updates() {
    let mut p = fixture();
    let (l, i) = m_counts(&p.model);
    assert_eq!((l, i), (5, 5));
    assert_eq!(ops(&p.tree), 0);
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, l + i);
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 0);

    // depth 1
    p.set(&names(&["b"]), vec![9]).unwrap();
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 2);
    // depth 2
    p.set(&names(&["a", "x"]), vec![9]).unwrap();
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 3);
    // depth 4
    p.set(&names(&["c", "d", "e", "f"]), vec![9]).unwrap();
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 5);
    assert_matches_model(&mut p);

    // identical payload: nothing dirty
    p.set(&names(&["c", "d", "e", "f"]), vec![9]).unwrap();
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 0);
    let v = 5u8;
    p.tree.set_leaf_value(&cp(&names(&["b"])), &v).unwrap();
    m_set(&mut p.model, &names(&["b"]), encode_to_vec(&v)).unwrap();
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 2);
    p.tree.set_leaf_value(&cp(&names(&["b"])), &v).unwrap();
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 0);
    assert_matches_model(&mut p);
}

/// REQ-LOOP-001: removal costs the number of ancestors; absent removal costs nothing.
#[test]
fn req_loop_001_cost_removals() {
    let mut p = fixture();
    p.tree.root_digest();

    assert!(p.remove(&names(&["a", "y"]))); // d = 2
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 2);
    assert_matches_model(&mut p);

    assert!(p.remove(&names(&["c", "d"]))); // subtree, d = 2
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 2);
    assert_matches_model(&mut p);

    assert!(p.remove(&names(&["b"]))); // d = 1
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 1);
    assert_matches_model(&mut p);

    // failed removals change nothing
    assert!(!p.remove(&names(&["zzz"])));
    assert!(!p.remove(&names(&["a", "x", "deeper"])));
    assert!(!p.remove(&[]));
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 0);

    // deep removal d = 4 on a fresh fixture
    let mut p = fixture();
    p.tree.root_digest();
    assert!(p.remove(&names(&["c", "d", "e", "f"])));
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 4);
    assert_matches_model(&mut p);
}

/// REQ-LOOP-001: shared ancestors are recomputed once: cost = dirty leaves + distinct dirty internals.
#[test]
fn req_loop_001_cost_shared_ancestors() {
    let mut p = fixture();
    p.tree.root_digest();
    p.set(&names(&["a", "x"]), vec![8]).unwrap();
    p.set(&names(&["a", "y"]), vec![8]).unwrap();
    p.set(&names(&["c", "d", "g"]), vec![8]).unwrap();
    // dirty leaves 3; internals: a, c, c/d, root = 4
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 7);
    assert_matches_model(&mut p);

    // two siblings only: leaves 2 + internals {c/d/e, c/d, c, root} for one, shared tail
    p.set(&names(&["c", "d", "e", "f"]), vec![7]).unwrap();
    p.set(&names(&["c", "d", "g"]), vec![7]).unwrap();
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 2 + 4);

    // adding a new leaf below a new internal node: new leaf + new internal + ancestors
    let mut p = fixture();
    p.tree.root_digest();
    p.set(&names(&["n", "m"]), vec![1]).unwrap();
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 3); // leaf, n, root
    assert_matches_model(&mut p);

    // set then remove same leaf before hashing: only ancestors remain dirty
    let mut p = fixture();
    p.tree.root_digest();
    p.set(&names(&["a", "q"]), vec![1]).unwrap();
    assert!(p.remove(&names(&["a", "q"])));
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert!(ops(&p.tree) - before <= 2);
    assert_matches_model(&mut p);
}

/// REQ-LOOP-001: full_root_digest and diff_first_divergence never touch the counter; clones diverge independently.
#[test]
fn req_loop_001_cost_counter_isolation() {
    let mut p = fixture();
    p.tree.root_digest();
    p.set(&names(&["a", "x"]), vec![42]).unwrap();
    let before = ops(&p.tree);
    let _ = p.tree.full_root_digest();
    let other = fixture();
    let _ = diff_first_divergence(&p.tree, &other.tree);
    let _ = diff_first_divergence(&other.tree, &p.tree);
    assert_eq!(ops(&p.tree), before);
    assert_eq!(ops(&other.tree), 0);

    let mut c1 = p.tree.clone();
    assert_eq!(ops(&c1), ops(&p.tree));
    let base = ops(&p.tree);
    c1.root_digest();
    assert!(ops(&c1) > base);
    assert_eq!(ops(&p.tree), base);
    let mut c2 = p.tree.clone();
    c2.set_leaf(&cp(&names(&["b"])), vec![0xAB]).unwrap();
    c2.root_digest();
    assert_eq!(ops(&p.tree), base);
    assert_ne!(c2.full_root_digest(), p.tree.full_root_digest());

    // digest_at on a clean tree costs nothing
    let mut cl = clean(&p.tree);
    let b = ops(&cl);
    let _ = cl.digest_at(&cp(&names(&["c", "d"])));
    let _ = cl.digest_at(&ComponentPath::root());
    let _ = cl.digest_at(&cp(&names(&["missing"])));
    assert_eq!(ops(&cl), b);
}

// ---------------------------------------------------------------- 7. divergence

fn check_diff(a: &Pair, b: &Pair) -> Option<ComponentPath> {
    let want = m_oracle(&a.model, &b.model).map(ComponentPath::new);
    let got = diff_first_divergence(&a.tree, &b.tree);
    assert_eq!(got, want, "dirty trees");
    let (ca, cb) = (clean(&a.tree), clean(&b.tree));
    assert_eq!(diff_first_divergence(&ca, &cb), want, "clean trees");
    assert_eq!(diff_first_divergence(&ca, &b.tree), want, "mixed trees");
    assert_eq!(diff_first_divergence(&a.tree, &cb), want, "mixed trees 2");
    assert_eq!(
        diff_first_divergence(&b.tree, &a.tree),
        m_oracle(&b.model, &a.model).map(ComponentPath::new)
    );
    // symmetry
    assert_eq!(diff_first_divergence(&b.tree, &a.tree), want, "symmetry");
    want
}

/// REQ-LOOP-001: generated pairs; single-leaf change returns exactly that leaf path.
#[test]
fn req_loop_001_divergence_leaf_change_exact() {
    let mut g = Gen::new(900);
    let mut done = 0;
    while done < 700 {
        let a = Pair::random(&mut g);
        let leaves = a.leaf_paths();
        if leaves.is_empty() {
            continue;
        }
        let i = g.below(leaves.len() as u64) as usize;
        let mut b = a.clone();
        let mut newp = match m_get(&a.model, &leaves[i]) {
            Some(M::Leaf(p)) => p.clone(),
            _ => unreachable!(),
        };
        newp.push(0xEE);
        b.set(&leaves[i], newp).unwrap();
        let got = check_diff(&a, &b);
        assert_eq!(got, Some(cp(&leaves[i])));
        done += 1;
    }
}

/// REQ-LOOP-001: add / remove / kind-swap / double mutation pairs agree with the oracle.
#[test]
fn req_loop_001_divergence_other_mutations_match_oracle() {
    let mut g = Gen::new(901);
    let mut checked = 0;
    let mut some_count = 0;
    let mut none_count = 0;
    for _ in 0..2500 {
        let a = Pair::random(&mut g);
        let mut b = a.clone();
        let leaves = a.leaf_paths();
        match g.below(5) {
            0 => {
                // add
                let path = g.path();
                let payload = g.payload();
                let _ = b.set(&path, payload);
            }
            1 => {
                if let Some(first) = leaves.first() {
                    let i = g.below(leaves.len() as u64) as usize;
                    let _ = first;
                    b.remove(&leaves[i]);
                }
            }
            2 => {
                // leaf -> internal
                if !leaves.is_empty() {
                    let i = g.below(leaves.len() as u64) as usize;
                    b.remove(&leaves[i]);
                    let mut deeper = leaves[i].clone();
                    deeper.push(g.label());
                    b.set(&deeper, vec![1]).unwrap();
                }
            }
            3 => {
                // internal -> leaf (any internal non-root path)
                let mut all = Vec::new();
                m_all_paths(&a.model, &mut Vec::new(), &mut all);
                let internals: Vec<_> = all
                    .into_iter()
                    .filter(|p| !p.is_empty() && matches!(m_get(&a.model, p), Some(M::Node(_))))
                    .collect();
                if !internals.is_empty() {
                    let i = g.below(internals.len() as u64) as usize;
                    assert!(b.remove(&internals[i]));
                    b.set(&internals[i], vec![3]).unwrap();
                }
            }
            _ => {
                // two mutations at different places
                if !leaves.is_empty() {
                    let i = g.below(leaves.len() as u64) as usize;
                    b.remove(&leaves[i]);
                }
                let path = g.path();
                let payload = g.payload();
                let _ = b.set(&path, payload);
            }
        }
        match check_diff(&a, &b) {
            Some(_) => some_count += 1,
            None => none_count += 1,
        }
        checked += 1;
    }
    assert!(checked >= 2000);
    assert!(some_count > 500 && none_count < checked);
}

/// REQ-LOOP-001: identical or order-permuted trees report no divergence, regardless of caches.
#[test]
fn req_loop_001_divergence_none_cases() {
    let a = fixture();
    assert_eq!(diff_first_divergence(&a.tree, &a.tree.clone()), None);
    assert_eq!(
        diff_first_divergence(&MerkleTree::new(), &MerkleTree::new()),
        None
    );

    // same leaves, different insertion order
    let mut b = MerkleTree::new();
    let mut leaves = Vec::new();
    m_leaves(&a.model, &mut Vec::new(), &mut leaves);
    for (p, v) in leaves.iter().rev() {
        b.set_leaf(&cp(p), v.clone()).unwrap();
    }
    assert_eq!(diff_first_divergence(&a.tree, &b), None);
    assert_eq!(diff_first_divergence(&b, &a.tree), None);

    // one side hashed, the other not
    let mut b2 = b.clone();
    b2.root_digest();
    assert_eq!(diff_first_divergence(&a.tree, &b2), None);
    assert_eq!(diff_first_divergence(&b2, &a.tree), None);

    // stale cache: set then restore the value
    let mut c = a.tree.clone();
    c.root_digest();
    c.set_leaf(&cp(&names(&["b"])), vec![0xFF]).unwrap();
    c.set_leaf(&cp(&names(&["b"])), vec![3]).unwrap();
    assert_eq!(diff_first_divergence(&a.tree, &c), None);
}

/// REQ-LOOP-001: label-kind siblings, root-level differences and ordering of first difference.
#[test]
fn req_loop_001_divergence_specific_cases() {
    let mut a = MerkleTree::new();
    let mut b = MerkleTree::new();
    a.set_leaf(&cp(&[Label::name("0")]), vec![1]).unwrap();
    b.set_leaf(&cp(&[Label::index(0)]), vec![1]).unwrap();
    // Name sorts before Index: first label present on one side only is Name("0")
    assert_eq!(diff_first_divergence(&a, &b), Some(cp(&[Label::name("0")])));
    assert_eq!(diff_first_divergence(&b, &a), Some(cp(&[Label::name("0")])));

    // leaf vs nothing at root level
    let e = MerkleTree::new();
    let mut one = MerkleTree::new();
    one.set_leaf(&cp(&names(&["only"])), vec![]).unwrap();
    assert_eq!(diff_first_divergence(&e, &one), Some(cp(&names(&["only"]))));
    assert_eq!(diff_first_divergence(&one, &e), Some(cp(&names(&["only"]))));

    // earliest of two differences wins (Name before Index; "a" before "b")
    let mut x = MerkleTree::new();
    let mut y = MerkleTree::new();
    for t in [&mut x, &mut y] {
        t.set_leaf(&cp(&names(&["a", "k"])), vec![1]).unwrap();
        t.set_leaf(&cp(&[Label::index(5)]), vec![1]).unwrap();
    }
    y.set_leaf(&cp(&[Label::index(5)]), vec![2]).unwrap();
    assert_eq!(diff_first_divergence(&x, &y), Some(cp(&[Label::index(5)])));
    y.set_leaf(&cp(&names(&["a", "k"])), vec![2]).unwrap();
    assert_eq!(diff_first_divergence(&x, &y), Some(cp(&names(&["a", "k"]))));

    // leaf vs internal at the same path returns that path
    let mut l = MerkleTree::new();
    let mut n = MerkleTree::new();
    l.set_leaf(&cp(&names(&["p"])), vec![1]).unwrap();
    n.set_leaf(&cp(&names(&["p", "q"])), vec![1]).unwrap();
    assert_eq!(diff_first_divergence(&l, &n), Some(cp(&names(&["p"]))));
    assert_eq!(diff_first_divergence(&n, &l), Some(cp(&names(&["p"]))));

    // empty internal node vs missing node
    let mut with_empty = Pair::new();
    with_empty.set(&names(&["a", "x"]), vec![1]).unwrap();
    with_empty.remove(&names(&["a", "x"]));
    assert_eq!(
        diff_first_divergence(&with_empty.tree, &MerkleTree::new()),
        Some(cp(&names(&["a"])))
    );
}

// ---------------------------------------------------------------- 8. set_leaf / remove

/// REQ-NFR-006: the three set_leaf errors and their atomicity.
#[test]
fn req_nfr_006_set_leaf_errors_atomic() {
    let mut p = fixture();
    p.tree.root_digest();
    let full = p.tree.full_root_digest();
    let ops0 = ops(&p.tree);
    let mut all = Vec::new();
    m_all_paths(&p.model, &mut Vec::new(), &mut all);
    let snapshot = |t: &MerkleTree| -> Vec<Snap> {
        all.iter()
            .map(|path| {
                (
                    path.clone(),
                    t.child_labels(&cp(path)),
                    t.leaf_payload(&cp(path)).map(<[u8]>::to_vec),
                )
            })
            .collect()
    };
    let snap = snapshot(&p.tree);

    assert_eq!(
        p.tree.set_leaf(&ComponentPath::root(), vec![1]),
        Err(PathError::EmptyPath)
    );
    assert_eq!(
        p.tree.set_leaf(&cp(&names(&["b", "below"])), vec![1]),
        Err(PathError::ThroughLeaf)
    );
    assert_eq!(
        p.tree.set_leaf(&cp(&names(&["a", "x", "u", "v"])), vec![1]),
        Err(PathError::ThroughLeaf)
    );
    assert_eq!(
        p.tree.set_leaf(&cp(&names(&["a"])), vec![1]),
        Err(PathError::IsInternal)
    );
    assert_eq!(
        p.tree.set_leaf(&cp(&names(&["c", "d"])), vec![1]),
        Err(PathError::IsInternal)
    );
    assert_eq!(
        p.tree.set_leaf_value(&cp(&names(&["c"])), &7u8),
        Err(PathError::IsInternal)
    );
    assert_eq!(
        p.tree.set_leaf_value(&ComponentPath::root(), &7u8),
        Err(PathError::EmptyPath)
    );
    assert_eq!(
        p.tree.set_leaf_value(&cp(&names(&["b", "z"])), &7u8),
        Err(PathError::ThroughLeaf)
    );

    assert_eq!(ops(&p.tree), ops0);
    assert_eq!(p.tree.full_root_digest(), full);
    assert_eq!(snapshot(&p.tree), snap);
    assert_eq!(p.tree.root_digest(), full);
    assert_eq!(ops(&p.tree), ops0, "failed calls must not dirty anything");

    // failure part-way down a path that would create nodes must not leave stragglers
    let mut q = Pair::new();
    q.set(&names(&["l"]), vec![1]).unwrap();
    q.tree.root_digest();
    let before = q.tree.full_root_digest();
    assert_eq!(
        q.tree.set_leaf(&cp(&names(&["l", "m", "n"])), vec![0]),
        Err(PathError::ThroughLeaf)
    );
    assert_eq!(q.tree.full_root_digest(), before);
    assert_eq!(
        q.tree.child_labels(&ComponentPath::root()),
        Some(vec![Label::name("l")])
    );
}

/// REQ-NFR-006: set_leaf creates intermediates, replaces payloads, and exposes structure.
#[test]
fn req_nfr_006_set_leaf_structure_queries() {
    let mut t = MerkleTree::new();
    assert_eq!(t.child_labels(&ComponentPath::root()), Some(vec![]));
    assert_eq!(t.leaf_payload(&ComponentPath::root()), None);
    t.set_leaf(
        &cp(&[Label::name("players"), Label::index(2), Label::name("life")]),
        vec![20],
    )
    .unwrap();
    t.set_leaf(
        &cp(&[Label::name("players"), Label::index(0), Label::name("life")]),
        vec![19],
    )
    .unwrap();
    t.set_leaf(
        &cp(&[Label::name("players"), Label::name("count")]),
        vec![2],
    )
    .unwrap();
    assert_eq!(
        t.child_labels(&ComponentPath::root()),
        Some(vec![Label::name("players")])
    );
    assert_eq!(
        t.child_labels(&cp(&names(&["players"]))),
        Some(vec![Label::name("count"), Label::index(0), Label::index(2)])
    );
    assert_eq!(
        t.leaf_payload(&cp(&[
            Label::name("players"),
            Label::index(2),
            Label::name("life")
        ])),
        Some(&[20u8][..])
    );
    assert_eq!(t.leaf_payload(&cp(&names(&["players"]))), None);
    assert_eq!(
        t.child_labels(&cp(&[Label::name("players"), Label::name("count")])),
        None
    );
    assert_eq!(t.child_labels(&cp(&names(&["absent"]))), None);
    assert_eq!(t.leaf_payload(&cp(&names(&["absent"]))), None);
    // replacement
    t.set_leaf(
        &cp(&[Label::name("players"), Label::name("count")]),
        vec![3, 3],
    )
    .unwrap();
    assert_eq!(
        t.leaf_payload(&cp(&[Label::name("players"), Label::name("count")])),
        Some(&[3u8, 3][..])
    );
    // set_leaf_value payload is encode_to_vec
    t.set_leaf_value(&cp(&names(&["v"])), "hi").unwrap();
    assert_eq!(
        t.leaf_payload(&cp(&names(&["v"]))),
        Some(&[2u8, 0, 0, 0, b'h', b'i'][..])
    );
    t.set_leaf_value(&cp(&names(&["w"])), &vec![Some(1u16)])
        .unwrap();
    assert_eq!(
        t.leaf_payload(&cp(&names(&["w"]))),
        Some(&encode_to_vec(&vec![Some(1u16)])[..])
    );
}

/// REQ-NFR-006: remove semantics, empty internals retained, root path refused.
#[test]
fn req_nfr_006_remove_semantics() {
    let mut p = Pair::new();
    p.set(&names(&["a", "only"]), vec![1]).unwrap();
    p.set(&names(&["b"]), vec![2]).unwrap();
    p.tree.root_digest();
    assert!(p.remove(&names(&["a", "only"])));
    assert_eq!(p.tree.child_labels(&cp(&names(&["a"]))), Some(vec![]));
    assert_eq!(
        p.tree.child_labels(&ComponentPath::root()),
        Some(names(&["a", "b"]))
    );
    let mut reference_with_a = m_new();
    m_set(&mut reference_with_a, &names(&["b"]), vec![2]).unwrap();
    let M::Node(c) = &mut reference_with_a else {
        unreachable!()
    };
    c.insert(Label::name("a"), m_new());
    assert_eq!(p.tree.root_digest(), m_digest(&reference_with_a));
    assert_eq!(p.tree.full_root_digest(), m_digest(&reference_with_a));
    assert_eq!(
        p.tree.digest_at(&cp(&names(&["a"]))),
        Some(node_digest(&[]))
    );

    // false cases
    assert!(!p.tree.remove(&ComponentPath::root()));
    assert!(!p.tree.remove(&cp(&names(&["nope"]))));
    assert!(!p.tree.remove(&cp(&names(&["b", "below"]))));
    assert!(!p.tree.remove(&cp(&names(&["a", "only"]))));
    assert_eq!(p.tree.full_root_digest(), m_digest(&reference_with_a));

    // subtree removal and re-adding
    let mut f = fixture();
    assert!(f.remove(&names(&["c"])));
    assert_eq!(f.tree.leaf_payload(&cp(&names(&["c", "d", "g"]))), None);
    assert_eq!(f.tree.child_labels(&cp(&names(&["c"]))), None);
    assert_matches_model(&mut f);
    assert!(!f.remove(&names(&["c"])));
    f.set(&names(&["c", "d", "g"]), vec![5]).unwrap();
    assert_matches_model(&mut f);

    // removing every leaf leaves root plus empty internals, not the empty-tree digest
    let mut f = fixture();
    for path in f.leaf_paths() {
        f.remove(&path);
    }
    assert_eq!(
        f.tree.child_labels(&ComponentPath::root()),
        Some(names(&["a", "c"]))
    );
    assert_ne!(f.tree.root_digest(), MerkleTree::new().root_digest());
    assert_matches_model(&mut f);
    // remove a whole internal then the root has no children
    assert!(f.remove(&names(&["a"])));
    assert!(f.remove(&names(&["c"])));
    assert_eq!(f.tree.root_digest(), node_digest(&[]));
}

// ---------------------------------------------------------------- 9. Label / ComponentPath / Digest

/// REQ-NFR-006: label ordering and display.
#[test]
fn req_nfr_006_label_order_and_display() {
    assert!(Label::name("a") < Label::name("b"));
    assert!(Label::name("b") < Label::name("ba"));
    assert!(Label::name("") < Label::name("a"));
    assert!(Label::name("Z") < Label::name("a"));
    assert!(Label::name("B") < Label::name("a"));
    assert!(Label::name("zzzz") < Label::index(0));
    assert!(Label::name("\u{10ffff}") < Label::index(0));
    assert!(Label::index(2) < Label::index(10));
    assert!(Label::index(0) < Label::index(u64::MAX));
    assert_eq!(Label::name("x"), Label::Name("x".to_string()));
    assert_eq!(Label::index(4), Label::Index(4));
    assert_ne!(Label::name("4"), Label::index(4));
    assert_eq!(Label::name("zone_a").to_string(), "zone_a");
    assert_eq!(Label::index(10).to_string(), "10");
    assert_eq!(Label::index(u64::MAX).to_string(), "18446744073709551615");
    let mut v = vec![
        Label::index(10),
        Label::name("b"),
        Label::index(2),
        Label::name("B"),
        Label::name("a"),
    ];
    v.sort();
    assert_eq!(
        v,
        vec![
            Label::name("B"),
            Label::name("a"),
            Label::name("b"),
            Label::index(2),
            Label::index(10)
        ]
    );
}

/// REQ-NFR-006: ComponentPath construction, display and non-mutating child.
#[test]
fn req_nfr_006_component_path_api() {
    let root = ComponentPath::root();
    assert!(root.is_empty());
    assert_eq!(root.len(), 0);
    assert_eq!(root.to_string(), "/");
    assert_eq!(ComponentPath::default(), root);
    assert_eq!(root.labels(), &[] as &[Label]);
    let p1 = root.child(Label::name("players"));
    assert!(root.is_empty(), "child must not mutate the receiver");
    assert_eq!(p1.len(), 1);
    assert!(!p1.is_empty());
    let p2 = p1.child(Label::index(2));
    assert_eq!(p1.len(), 1);
    assert_eq!(p2.len(), 2);
    assert_eq!(p2.to_string(), "/players/2");
    assert_eq!(p1.to_string(), "/players");
    assert_eq!(p2.labels(), &[Label::name("players"), Label::index(2)]);
    assert_eq!(
        p2,
        ComponentPath::new(vec![Label::name("players"), Label::index(2)])
    );
    assert_ne!(p1, p2);
    assert!(p1 < p2);
    assert!(root < p1);
    let sib = p1.child(Label::index(3));
    assert_ne!(sib, p2);
    assert!(p2 < sib);
}

/// REQ-NFR-006: Digest accessors, hex, Display and ordering.
#[test]
fn req_nfr_006_digest_api() {
    let mut raw = [0u8; 32];
    for (i, b) in raw.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(9).wrapping_add(0xA0);
    }
    let d = Digest::from_bytes(raw);
    assert_eq!(d.as_bytes(), &raw);
    let hex = d.to_hex();
    assert_eq!(hex.len(), 64);
    assert!(hex.starts_with("a0a9b2"));
    assert_eq!(d.to_string(), hex);
    assert_eq!(Digest::from_bytes([0; 32]).to_hex(), "0".repeat(64));
    assert_eq!(Digest::from_bytes([0xFF; 32]).to_hex(), "f".repeat(64));
    let copy = d;
    assert_eq!(copy, d);
    let lo = Digest::from_bytes([0; 32]);
    let mut one_last = [0u8; 32];
    one_last[31] = 1;
    let mut one_first = [0u8; 32];
    one_first[0] = 1;
    assert!(lo < Digest::from_bytes(one_last));
    assert!(Digest::from_bytes(one_last) < Digest::from_bytes(one_first));
    assert_eq!(lo.cmp(&lo), std::cmp::Ordering::Equal);
    let mut set = BTreeSet::new();
    set.insert(d);
    set.insert(copy);
    assert_eq!(set.len(), 1);
    assert!(!format!("{d:?}").is_empty());
}

// ---------------------------------------------------------------- 10. PRNG leaf

/// REQ-LOOP-001, REQ-NFR-006: a PRNG stored as a leaf hashes its 32-byte canonical encoding.
#[test]
fn req_loop_001_prng_leaf() {
    let mut t = MerkleTree::new();
    let mut g = Pcg64Dxsm::from_u64_seed(0x5EED);
    let path = cp(&names(&["prng"]));
    t.set_leaf_value(&path, &g).unwrap();
    let payload = t.leaf_payload(&path).unwrap().to_vec();
    assert_eq!(payload.len(), 32);
    assert_eq!(payload, encode_to_vec(&g));
    let mut pre = vec![0u8];
    pre.extend(&payload);
    assert_eq!(t.digest_at(&path), Some(sha256(&pre)));
    let root0 = t.root_digest();
    let saved = g.clone();

    g.next_u64();
    t.set_leaf_value(&path, &g).unwrap();
    let root1 = t.root_digest();
    assert_ne!(root0, root1);
    assert_eq!(root1, t.full_root_digest());

    t.set_leaf_value(&path, &saved).unwrap();
    assert_eq!(t.root_digest(), root0);
    assert_eq!(t.full_root_digest(), root0);
}

// ---------------------------------------------------------------- hostile inputs (R12)

/// REQ-NFR-006: hostile labels, deep paths and payload sizes never panic and match the model.
#[test]
fn req_nfr_006_hostile_inputs() {
    let mut p = Pair::new();
    // empty labels
    p.set(&[Label::name("")], vec![]).unwrap();
    p.set(&[Label::name(""), Label::name("")], vec![1])
        .unwrap_err();
    p.set(
        &[Label::name("e"), Label::name(""), Label::name("")],
        vec![1],
    )
    .unwrap();
    // very long label
    let long = "L".repeat(5000);
    p.set(
        &[Label::Name(long.clone()), Label::index(u64::MAX)],
        vec![7],
    )
    .unwrap();
    // deep path of 64
    let deep: Vec<Label> = (0..64).map(Label::index).collect();
    p.set(&deep, vec![1, 2, 3]).unwrap();
    // payloads of 0 and 10_000 bytes
    p.set(&names(&["empty_payload"]), vec![]).unwrap();
    p.set(&names(&["big_payload"]), vec![0x5A; 10_000]).unwrap();
    assert_matches_model(&mut p);
    let mut all = Vec::new();
    m_all_paths(&p.model, &mut Vec::new(), &mut all);
    for path in &all {
        let want = m_get(&p.model, path).map(m_digest);
        assert_eq!(p.tree.digest_at(&cp(path)), want);
    }
    // incremental cost at depth 64 = 65
    p.tree.root_digest();
    p.set(&deep, vec![9]).unwrap();
    let before = ops(&p.tree);
    p.tree.root_digest();
    assert_eq!(ops(&p.tree) - before, 65);
    assert_matches_model(&mut p);
    // display of long and deep paths
    let shown = cp(&deep).to_string();
    assert!(shown.starts_with("/0/1/2/"));
    assert!(cp(&[Label::Name(long.clone())]).to_string().len() == 5001);
    // removal and diff on hostile trees
    let mut q = p.clone();
    assert!(q.remove(&deep));
    assert_eq!(check_diff(&p, &q), Some(cp(&deep)));

    assert!(q.remove(&[Label::Name(long)]));
    assert!(!q.remove(&deep));
    let _ = diff_first_divergence(&p.tree, &q.tree);
}
