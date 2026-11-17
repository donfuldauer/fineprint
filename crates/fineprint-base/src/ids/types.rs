// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The identifier newtypes (REQ-CORE-002).

use super::encode::CanonicalEncode;

/// Common interface of every identifier newtype (REQ-CORE-002).
pub trait Id: Copy + Ord + CanonicalEncode {
    /// Wraps a raw `u64`.
    fn from_raw(raw: u64) -> Self;
    /// Returns the raw `u64`.
    fn raw(self) -> u64;
}

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        ///
        /// Ordering is the numeric order of the inner `u64`.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(u64);

        impl $name {
            /// Wraps a raw `u64`.
            #[must_use]
            pub const fn new(raw: u64) -> $name {
                $name(raw)
            }

            /// Returns the raw `u64`.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl Id for $name {
            fn from_raw(raw: u64) -> Self {
                $name(raw)
            }

            fn raw(self) -> u64 {
                self.0
            }
        }

        impl CanonicalEncode for $name {
            /// The `u64` as exactly 8 bytes little-endian.
            fn encode_canonical(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.0.to_le_bytes());
            }
        }
    };
}

id_type!(
    /// The physical card; lives for the entire match (REQ-CORE-002).
    CardId
);
id_type!(
    /// One zone residency of an object; a new one is assigned whenever an object enters a zone
    /// (REQ-CORE-002).
    ObjectId
);
id_type!(
    /// One stack residency of an ability, or one pending trigger (REQ-CORE-002).
    AbilityInstanceId
);
id_type!(
    /// The lifetime of a continuous effect, replacement effect or delayed trigger
    /// (REQ-CORE-002).
    EffectId
);
id_type!(
    /// A value from the single monotonic timestamp counter (REQ-CORE-002).
    Timestamp
);
id_type!(
    /// The lifetime of a linked-ability pair (REQ-CORE-002).
    LinkId
);
id_type!(
    /// A player (REQ-CORE-002).
    PlayerId
);

#[cfg(test)]
mod tests {
    // REQ-CORE-002: identifier newtypes; REQ-CORE-006: canonical encoding.
    use super::*;
    use crate::num::testkit::Rng;

    fn check_type<I: Id + core::fmt::Debug>(new: fn(u64) -> I, get: fn(I) -> u64) {
        let edges = [0, 1, 2, u64::MAX - 1, u64::MAX];
        let mut rng = Rng::new(0x1D5_0005);
        let values: Vec<u64> = edges
            .iter()
            .copied()
            .chain((0..5_000).map(|_| rng.next_u64()))
            .collect();
        for w in values.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (x, y) = (new(a), new(b));
            assert_eq!(x.cmp(&y), a.cmp(&b));
            assert_eq!(x == y, a == b);
            assert_eq!(get(x), a);
            assert_eq!(I::from_raw(a), x);
            assert_eq!(x.raw(), a);
            let mut out = vec![0x55];
            x.encode_canonical(&mut out);
            let mut expected = vec![0x55];
            expected.extend_from_slice(&a.to_le_bytes());
            assert_eq!(out, expected);
        }
    }

    // REQ-CORE-002: property, ordering equals u64 ordering, accessors and encoding, all types.
    #[test]
    fn prop_every_id_type() {
        check_type(CardId::new, CardId::get);
        check_type(ObjectId::new, ObjectId::get);
        check_type(AbilityInstanceId::new, AbilityInstanceId::get);
        check_type(EffectId::new, EffectId::get);
        check_type(Timestamp::new, Timestamp::get);
        check_type(LinkId::new, LinkId::get);
        check_type(PlayerId::new, PlayerId::get);
    }

    // REQ-CORE-002, REQ-CORE-006: fixed byte example.
    #[test]
    fn encoding_example() {
        let mut out = Vec::new();
        ObjectId::new(0x0102_0304_0506_0708).encode_canonical(&mut out);
        assert_eq!(out, [8, 7, 6, 5, 4, 3, 2, 1]);
        Timestamp::new(1).encode_canonical(&mut out);
        assert_eq!(out.len(), 16);
        assert_eq!(&out[8..], &[1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(format!("{:?}", PlayerId::new(3)), "PlayerId(3)");
    }
}
