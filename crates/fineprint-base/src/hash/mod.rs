// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! SHA-256 digests and the Merkle hashing framework of the canonical state hash (REQ-NFR-006,
//! REQ-LOOP-001).
//!
//! **Digests (byte-exact).**
//! - leaf digest = SHA-256( `0x00` || payload ) ([`leaf_digest`](crate::hash::leaf_digest));
//! - internal digest = SHA-256( `0x01` || [`put_len`](crate::encode::put_len)(child count) ||
//!   for each child in ascending [`Label`](crate::hash::Label) order: canonical label bytes || child digest (32 raw
//!   bytes) ) ([`node_digest`](crate::hash::node_digest)).
//!
//! The distinct `0x00` / `0x01` prefixes keep a leaf from ever colliding with an internal node.
//!
//! **Tree.** A [`MerkleTree`](crate::hash::MerkleTree) is an internal root node whose children are keyed by [`Label`](crate::hash::Label)s;
//! a [`ComponentPath`](crate::hash::ComponentPath) names a node. [`MerkleTree::root_digest`](crate::hash::MerkleTree::root_digest) is incremental: it recomputes
//! only the nodes made dirty since the previous call, and always equals the from-scratch
//! [`MerkleTree::full_root_digest`](crate::hash::MerkleTree::full_root_digest). [`diff_first_divergence`](crate::hash::diff_first_divergence) locates the first differing
//! component of two trees for divergence debugging.
//!
//! The module uses integer arithmetic and ordered containers only, never panics and performs no
//! I/O (REQ-CORE-006).

mod tree;

#[cfg(test)]
mod tests;

use core::fmt;

use sha2::{Digest as _, Sha256};

use crate::encode::{CanonicalEncode, put_bytes, put_len, put_variant};

pub use tree::{MerkleTree, PathError, diff_first_divergence};

/// The byte that starts every leaf preimage.
const LEAF_PREFIX: u8 = 0x00;
/// The byte that starts every internal-node preimage.
const NODE_PREFIX: u8 = 0x01;

/// A 32-byte SHA-256 digest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Digest([u8; 32]);

impl Digest {
    /// Wraps 32 raw digest bytes.
    #[must_use]
    pub const fn from_bytes(b: [u8; 32]) -> Digest {
        Digest(b)
    }

    /// Returns the 32 raw digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the digest as 64 lowercase hexadecimal characters.
    #[must_use]
    pub fn to_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(64);
        for byte in self.0 {
            s.push(char::from(HEX[usize::from(byte >> 4)]));
            s.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        s
    }
}

impl fmt::Display for Digest {
    /// Same as [`Digest::to_hex`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl CanonicalEncode for Digest {
    /// The 32 raw bytes, no prefix.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0);
    }
}

/// Plain SHA-256 of `data`.
#[must_use]
pub fn sha256(data: &[u8]) -> Digest {
    let mut h = Sha256::new();
    h.update(data);
    Digest(h.finalize().into())
}

/// The digest of a leaf: SHA-256( `0x00` || `payload` ).
#[must_use]
pub fn leaf_digest(payload: &[u8]) -> Digest {
    let mut h = Sha256::new();
    h.update([LEAF_PREFIX]);
    h.update(payload);
    Digest(h.finalize().into())
}

/// The digest of an internal node: SHA-256( `0x01` || `put_len(children.len())` || for each
/// child: canonical label bytes || child digest ).
///
/// `children` must already be in ascending [`Label`] order (the caller's responsibility; the
/// bytes are hashed in the order given).
#[must_use]
pub fn node_digest(children: &[(Label, Digest)]) -> Digest {
    node_digest_iter(children.len(), children.iter().map(|(l, d)| (l, *d)))
}

/// [`node_digest`] over `count` borrowed `(label, digest)` pairs, in the order given.
fn node_digest_iter<'a>(
    count: usize,
    children: impl Iterator<Item = (&'a Label, Digest)>,
) -> Digest {
    let mut buf = Vec::with_capacity(5 + count.saturating_mul(48));
    buf.push(NODE_PREFIX);
    put_len(&mut buf, count);
    for (label, digest) in children {
        label.encode_canonical(&mut buf);
        buf.extend_from_slice(&digest.0);
    }
    sha256(&buf)
}

/// One step of a [`ComponentPath`]: a named component or a numeric index.
///
/// The variant order is part of the hash contract: every `Name` sorts before every `Index`;
/// names compare bytewise and indexes numerically.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Label {
    /// A named component, for example `players`.
    Name(String),
    /// A numeric index, for example a seat or a position.
    Index(u64),
}

impl Label {
    /// A [`Label::Name`] holding `s`.
    #[must_use]
    pub fn name(s: &str) -> Label {
        Label::Name(s.to_owned())
    }

    /// A [`Label::Index`] holding `i`.
    #[must_use]
    pub fn index(i: u64) -> Label {
        Label::Index(i)
    }
}

impl fmt::Display for Label {
    /// A name prints as itself; an index prints in decimal.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Label::Name(s) => f.write_str(s),
            Label::Index(i) => write!(f, "{i}"),
        }
    }
}

impl CanonicalEncode for Label {
    /// `Name(s)`: `0x00` then [`put_bytes`] of the UTF-8 bytes; `Index(i)`: `0x01` then `i` as
    /// 8 bytes little-endian.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Label::Name(s) => {
                put_variant(out, 0);
                put_bytes(out, s.as_bytes());
            }
            Label::Index(i) => {
                put_variant(out, 1);
                i.encode_canonical(out);
            }
        }
    }
}

/// The path from the root of a [`MerkleTree`] to one of its nodes; the empty path is the root.
///
/// Paths order depth-first: a path sorts before its descendants, and siblings sort by
/// ascending [`Label`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct ComponentPath(Vec<Label>);

impl ComponentPath {
    /// The empty path, naming the root.
    #[must_use]
    pub fn root() -> ComponentPath {
        ComponentPath(Vec::new())
    }

    /// The path made of `labels`, outermost first.
    #[must_use]
    pub fn new(labels: Vec<Label>) -> ComponentPath {
        ComponentPath(labels)
    }

    /// A new path: `self` with `label` appended.
    #[must_use]
    pub fn child(&self, label: Label) -> ComponentPath {
        let mut labels = Vec::with_capacity(self.0.len().saturating_add(1));
        labels.extend_from_slice(&self.0);
        labels.push(label);
        ComponentPath(labels)
    }

    /// The labels, outermost first.
    #[must_use]
    pub fn labels(&self) -> &[Label] {
        &self.0
    }

    /// The number of labels (the depth of the named node).
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` for the root path.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for ComponentPath {
    /// The root prints as `/`; any other path as `/` followed by its labels joined by `/`, for
    /// example `/players/2`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("/");
        }
        for label in &self.0 {
            write!(f, "/{label}")?;
        }
        Ok(())
    }
}
