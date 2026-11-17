// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! [`Int`]: checked signed 64-bit integer (REQ-CORE-006).

use core::fmt;

use crate::ids::CanonicalEncode;

/// A signed 64-bit rules number (REQ-CORE-006).
///
/// There is deliberately no operator overloading: every arithmetic operation names its overflow
/// policy, either `checked_*` (returns `None` when the exact result does not fit `i64`) or
/// `saturating_*` (clamps to [`Int::MIN`] / [`Int::MAX`]). No method panics or wraps.
/// `Default` is zero.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Int(i64);

impl Int {
    /// Zero.
    pub const ZERO: Int = Int(0);
    /// One.
    pub const ONE: Int = Int(1);
    /// The smallest value, `i64::MIN`.
    pub const MIN: Int = Int(i64::MIN);
    /// The largest value, `i64::MAX`.
    pub const MAX: Int = Int(i64::MAX);

    /// Wraps a raw `i64`.
    #[must_use]
    pub const fn new(v: i64) -> Int {
        Int(v)
    }

    /// Returns the raw `i64`.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    /// `self + rhs`, or `None` if the exact sum is outside `i64`.
    #[must_use]
    pub fn checked_add(self, rhs: Int) -> Option<Int> {
        self.0.checked_add(rhs.0).map(Int)
    }

    /// `self - rhs`, or `None` if the exact difference is outside `i64`.
    #[must_use]
    pub fn checked_sub(self, rhs: Int) -> Option<Int> {
        self.0.checked_sub(rhs.0).map(Int)
    }

    /// `self * rhs`, or `None` if the exact product is outside `i64`.
    #[must_use]
    pub fn checked_mul(self, rhs: Int) -> Option<Int> {
        self.0.checked_mul(rhs.0).map(Int)
    }

    /// `-self`, or `None` for [`Int::MIN`].
    #[must_use]
    pub fn checked_neg(self) -> Option<Int> {
        self.0.checked_neg().map(Int)
    }

    /// `|self|`, or `None` for [`Int::MIN`].
    #[must_use]
    pub fn checked_abs(self) -> Option<Int> {
        self.0.checked_abs().map(Int)
    }

    /// Truncating division (toward zero, like Rust `/`). `None` when `rhs` is zero or when
    /// `self` is [`Int::MIN`] and `rhs` is `-1`.
    #[must_use]
    pub fn checked_div(self, rhs: Int) -> Option<Int> {
        self.0.checked_div(rhs.0).map(Int)
    }

    /// Remainder of truncating division (sign follows `self`, like Rust `%`). `None` when `rhs`
    /// is zero or when `self` is [`Int::MIN`] and `rhs` is `-1`.
    #[must_use]
    pub fn checked_rem(self, rhs: Int) -> Option<Int> {
        self.0.checked_rem(rhs.0).map(Int)
    }

    /// `self + rhs`, clamped to [`Int::MIN`] / [`Int::MAX`].
    #[must_use]
    pub fn saturating_add(self, rhs: Int) -> Int {
        Int(self.0.saturating_add(rhs.0))
    }

    /// `self - rhs`, clamped to [`Int::MIN`] / [`Int::MAX`].
    #[must_use]
    pub fn saturating_sub(self, rhs: Int) -> Int {
        Int(self.0.saturating_sub(rhs.0))
    }

    /// `self * rhs`, clamped to [`Int::MIN`] / [`Int::MAX`] by the sign of the exact product.
    #[must_use]
    pub fn saturating_mul(self, rhs: Int) -> Int {
        Int(self.0.saturating_mul(rhs.0))
    }

    /// `-self`; [`Int::MIN`] maps to [`Int::MAX`].
    #[must_use]
    pub fn saturating_neg(self) -> Int {
        Int(self.0.saturating_neg())
    }

    /// `|self|`; [`Int::MIN`] maps to [`Int::MAX`].
    #[must_use]
    pub fn saturating_abs(self) -> Int {
        Int(self.0.saturating_abs())
    }

    /// `-1`, `0` or `1` according to the sign of `self`.
    #[must_use]
    pub fn signum(self) -> Int {
        Int(self.0.signum())
    }

    /// `max(self, 0)`.
    #[must_use]
    pub fn floor_zero(self) -> Int {
        Int(self.0.max(0))
    }
}

impl From<i64> for Int {
    /// Wraps a raw `i64` (same as [`Int::new`]).
    fn from(v: i64) -> Int {
        Int(v)
    }
}

impl From<Int> for i64 {
    /// Unwraps the raw `i64` (same as [`Int::get`]).
    fn from(v: Int) -> i64 {
        v.0
    }
}

impl fmt::Display for Int {
    /// Plain decimal, e.g. `-12`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl CanonicalEncode for Int {
    /// The two's-complement `i64` as 8 bytes little-endian.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0.to_le_bytes());
    }
}

/// An arithmetic result did not fit the target type (REQ-CORE-006).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OverflowError;

impl fmt::Display for OverflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("integer overflow")
    }
}

impl std::error::Error for OverflowError {}

#[cfg(test)]
mod tests {
    // REQ-CORE-006: checked i64 arithmetic with an explicit overflow policy.
    use super::*;
    use crate::num::testkit::{EDGES, Rng, pairs};

    fn fits(v: i128) -> Option<Int> {
        i64::try_from(v).ok().map(Int::new)
    }

    fn clamp(v: i128) -> Int {
        Int::new(i64::try_from(v.clamp(i128::from(i64::MIN), i128::from(i64::MAX))).unwrap())
    }

    // REQ-CORE-006
    #[test]
    fn constants_and_accessors() {
        assert_eq!(Int::ZERO.get(), 0);
        assert_eq!(Int::ONE.get(), 1);
        assert_eq!(Int::MIN.get(), i64::MIN);
        assert_eq!(Int::MAX.get(), i64::MAX);
        assert_eq!(Int::default(), Int::ZERO);
        assert_eq!(Int::new(-7).get(), -7);
        assert_eq!(Int::from(5_i64), Int::new(5));
        assert_eq!(i64::from(Int::new(9)), 9);
        assert!(Int::new(-1) < Int::ZERO && Int::ZERO < Int::ONE);
    }

    // REQ-CORE-006
    #[test]
    fn display_is_plain_decimal() {
        assert_eq!(Int::new(-12).to_string(), "-12");
        assert_eq!(Int::ZERO.to_string(), "0");
        assert_eq!(Int::MIN.to_string(), "-9223372036854775808");
    }

    // REQ-CORE-006
    #[test]
    fn overflow_error_display_and_error() {
        let e = OverflowError;
        assert_eq!(e.to_string(), "integer overflow");
        let dynerr: &dyn std::error::Error = &e;
        assert!(dynerr.source().is_none());
        assert_eq!(e, e.clone());
        assert_eq!(format!("{e:?}"), "OverflowError");
    }

    // REQ-CORE-006
    #[test]
    fn edge_examples() {
        assert_eq!(Int::MAX.checked_add(Int::ONE), None);
        assert_eq!(Int::MIN.checked_sub(Int::ONE), None);
        assert_eq!(Int::MIN.checked_mul(Int::new(-1)), None);
        assert_eq!(Int::MIN.checked_neg(), None);
        assert_eq!(Int::MIN.checked_abs(), None);
        assert_eq!(Int::new(-3).checked_abs(), Some(Int::new(3)));
        assert_eq!(Int::MIN.saturating_neg(), Int::MAX);
        assert_eq!(Int::MIN.saturating_abs(), Int::MAX);
        assert_eq!(Int::MAX.saturating_add(Int::ONE), Int::MAX);
        assert_eq!(Int::MIN.saturating_sub(Int::ONE), Int::MIN);
        assert_eq!(Int::MIN.saturating_mul(Int::new(-1)), Int::MAX);
        assert_eq!(Int::MAX.saturating_mul(Int::new(-2)), Int::MIN);
        assert_eq!(Int::new(-7).checked_div(Int::new(2)), Some(Int::new(-3)));
        assert_eq!(Int::new(-7).checked_rem(Int::new(2)), Some(Int::new(-1)));
        assert_eq!(Int::new(7).checked_div(Int::ZERO), None);
        assert_eq!(Int::new(7).checked_rem(Int::ZERO), None);
        assert_eq!(Int::MIN.checked_div(Int::new(-1)), None);
        assert_eq!(Int::MIN.checked_rem(Int::new(-1)), None);
        assert_eq!(Int::new(-5).signum(), Int::new(-1));
        assert_eq!(Int::ZERO.signum(), Int::ZERO);
        assert_eq!(Int::new(5).signum(), Int::ONE);
        assert_eq!(Int::new(-5).floor_zero(), Int::ZERO);
        assert_eq!(Int::new(5).floor_zero(), Int::new(5));
    }

    // REQ-CORE-006, REQ-CORE-002: canonical encoding hook.
    #[test]
    fn canonical_encoding() {
        let mut out = vec![0xAA];
        Int::new(-1).encode_canonical(&mut out);
        assert_eq!(out, [0xAA, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        let mut out = Vec::new();
        Int::new(0x0102_0304_0506_0708).encode_canonical(&mut out);
        assert_eq!(out, [8, 7, 6, 5, 4, 3, 2, 1]);
    }

    // REQ-CORE-006: property, binary ops against the i128 oracle.
    #[test]
    fn prop_binary_ops_match_i128_oracle() {
        for (a, b) in pairs(&mut Rng::new(0x1A2B_3C4D), 6_000) {
            let (x, y) = (Int::new(a), Int::new(b));
            let (wa, wb) = (i128::from(a), i128::from(b));
            assert_eq!(x.checked_add(y), fits(wa + wb));
            assert_eq!(x.checked_sub(y), fits(wa - wb));
            assert_eq!(x.checked_mul(y), fits(wa * wb));
            assert_eq!(x.saturating_add(y), clamp(wa + wb));
            assert_eq!(x.saturating_sub(y), clamp(wa - wb));
            assert_eq!(x.saturating_mul(y), clamp(wa * wb));
            // commutativity
            assert_eq!(x.checked_add(y), y.checked_add(x));
            assert_eq!(x.checked_mul(y), y.checked_mul(x));
            assert_eq!(x.saturating_add(y), y.saturating_add(x));
            assert_eq!(x.saturating_mul(y), y.saturating_mul(x));
            // a + b - b = a when no overflow
            if let Some(s) = x.checked_add(y) {
                assert_eq!(s.checked_sub(y), Some(x));
            }
            // truncating division
            if b == 0 || (a == i64::MIN && b == -1) {
                assert_eq!(x.checked_div(y), None);
                assert_eq!(x.checked_rem(y), None);
            } else {
                let q = wa / wb;
                let r = wa % wb;
                assert_eq!(x.checked_div(y), fits(q));
                assert_eq!(x.checked_rem(y), fits(r));
                assert_eq!(q * wb + r, wa);
                assert!(r == 0 || (r < 0) == (wa < 0));
            }
        }
    }

    // REQ-CORE-006: property, unary ops.
    #[test]
    fn prop_unary_ops() {
        let mut rng = Rng::new(0x5EED_0001);
        let values = EDGES
            .iter()
            .copied()
            .chain((0..6_000).map(|_| rng.next_i64()));
        for a in values {
            let x = Int::new(a);
            let w = i128::from(a);
            assert_eq!(x.checked_neg(), fits(-w));
            assert_eq!(x.checked_abs(), fits(w.abs()));
            assert_eq!(x.checked_neg().is_none(), a == i64::MIN);
            assert_eq!(x.checked_abs().is_none(), a == i64::MIN);
            assert_eq!(x.saturating_neg(), clamp(-w));
            assert_eq!(x.saturating_abs(), clamp(w.abs()));
            assert_eq!(x.signum().get(), a.signum());
            assert_eq!(x.floor_zero().get(), a.max(0));
            assert_eq!(x.to_string(), a.to_string());
        }
    }
}
