// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! [`Fixed2`]: scale-2 fixed point for half-values (REQ-CORE-006).

use core::fmt;

use super::int::{Int, OverflowError};
use crate::ids::CanonicalEncode;

/// A scale-2 fixed-point number (REQ-CORE-006): the inner value counts hundredths, so raw `150`
/// is `1.50`. The derived ordering is numeric order. `Default` is zero.
///
/// The single rounding rule for every operation that can drop precision is **round half away
/// from zero**: the exact rational result is rounded to the nearest hundredth, and an exact tie
/// moves to the value with the larger magnitude. As with [`Int`], there is no operator
/// overloading and nothing panics or wraps.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Fixed2(i64);

/// Divides `num` by `den` (`den != 0`) rounding half away from zero.
fn div_round_half_away(num: i128, den: i128) -> i128 {
    let q = num / den;
    let r = num % den;
    // |r| < |den| <= 2^70 in every caller, so doubling cannot overflow i128.
    if r.unsigned_abs() * 2 >= den.unsigned_abs() {
        if (num < 0) == (den < 0) { q + 1 } else { q - 1 }
    } else {
        q
    }
}

impl Fixed2 {
    /// Number of raw units in one whole: 100.
    pub const SCALE: i64 = 100;
    /// `0.00`.
    pub const ZERO: Fixed2 = Fixed2(0);
    /// `1.00` (raw 100).
    pub const ONE: Fixed2 = Fixed2(100);
    /// `0.50` (raw 50).
    pub const HALF: Fixed2 = Fixed2(50);
    /// The smallest value (raw `i64::MIN`).
    pub const MIN: Fixed2 = Fixed2(i64::MIN);
    /// The largest value (raw `i64::MAX`).
    pub const MAX: Fixed2 = Fixed2(i64::MAX);

    /// Builds a value from its count of hundredths.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Fixed2 {
        Fixed2(raw)
    }

    /// The count of hundredths.
    #[must_use]
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// `n` as a fixed-point value (`n * 100` hundredths), or `None` on overflow.
    #[must_use]
    pub fn from_int(n: Int) -> Option<Fixed2> {
        n.get().checked_mul(Self::SCALE).map(Fixed2)
    }

    /// `n` halves (`n * 50` hundredths), or `None` on overflow. Two halves make one.
    #[must_use]
    pub fn from_halves(n: Int) -> Option<Fixed2> {
        n.get().checked_mul(Self::SCALE / 2).map(Fixed2)
    }

    /// Like [`Fixed2::from_int`] but reports overflow as an [`OverflowError`].
    ///
    /// # Errors
    /// [`OverflowError`] when `n * 100` does not fit `i64`.
    pub fn try_from_int(n: Int) -> Result<Fixed2, OverflowError> {
        Self::from_int(n).ok_or(OverflowError)
    }

    /// Like [`Fixed2::from_halves`] but reports overflow as an [`OverflowError`].
    ///
    /// # Errors
    /// [`OverflowError`] when `n * 50` does not fit `i64`.
    pub fn try_from_halves(n: Int) -> Result<Fixed2, OverflowError> {
        Self::from_halves(n).ok_or(OverflowError)
    }

    /// Exact sum, or `None` on overflow.
    #[must_use]
    pub fn checked_add(self, rhs: Fixed2) -> Option<Fixed2> {
        self.0.checked_add(rhs.0).map(Fixed2)
    }

    /// Exact difference, or `None` on overflow.
    #[must_use]
    pub fn checked_sub(self, rhs: Fixed2) -> Option<Fixed2> {
        self.0.checked_sub(rhs.0).map(Fixed2)
    }

    /// Negation, or `None` for [`Fixed2::MIN`].
    #[must_use]
    pub fn checked_neg(self) -> Option<Fixed2> {
        self.0.checked_neg().map(Fixed2)
    }

    /// Absolute value, or `None` for [`Fixed2::MIN`].
    #[must_use]
    pub fn checked_abs(self) -> Option<Fixed2> {
        self.0.checked_abs().map(Fixed2)
    }

    /// Sum clamped to [`Fixed2::MIN`] / [`Fixed2::MAX`].
    #[must_use]
    pub fn saturating_add(self, rhs: Fixed2) -> Fixed2 {
        Fixed2(self.0.saturating_add(rhs.0))
    }

    /// Difference clamped to [`Fixed2::MIN`] / [`Fixed2::MAX`].
    #[must_use]
    pub fn saturating_sub(self, rhs: Fixed2) -> Fixed2 {
        Fixed2(self.0.saturating_sub(rhs.0))
    }

    /// Negation; [`Fixed2::MIN`] maps to [`Fixed2::MAX`].
    #[must_use]
    pub fn saturating_neg(self) -> Fixed2 {
        Fixed2(self.0.saturating_neg())
    }

    /// Absolute value; [`Fixed2::MIN`] maps to [`Fixed2::MAX`].
    #[must_use]
    pub fn saturating_abs(self) -> Fixed2 {
        Fixed2(self.0.saturating_abs())
    }

    /// `self * n` exactly (raw times `n`), or `None` on overflow.
    #[must_use]
    pub fn checked_mul_int(self, n: Int) -> Option<Fixed2> {
        self.0.checked_mul(n.get()).map(Fixed2)
    }

    /// Fixed-point product `raw_a * raw_b / 100` computed in `i128` and rounded half away from
    /// zero; `None` if the rounded result does not fit `i64`. Example: `0.05 * 0.10 = 0.01`.
    #[must_use]
    pub fn checked_mul(self, rhs: Fixed2) -> Option<Fixed2> {
        let exact = i128::from(self.0) * i128::from(rhs.0);
        let rounded = div_round_half_away(exact, i128::from(Self::SCALE));
        i64::try_from(rounded).ok().map(Fixed2)
    }

    /// Fixed-point quotient `raw_a * 100 / raw_b` computed in `i128` and rounded half away from
    /// zero; `None` if `rhs` is zero or the rounded result does not fit `i64`.
    /// Example: `2.00 / 3.00 = 0.67`.
    #[must_use]
    pub fn checked_div(self, rhs: Fixed2) -> Option<Fixed2> {
        if rhs.0 == 0 {
            return None;
        }
        let num = i128::from(self.0) * i128::from(Self::SCALE);
        let rounded = div_round_half_away(num, i128::from(rhs.0));
        i64::try_from(rounded).ok().map(Fixed2)
    }

    /// Integer part rounded toward zero.
    #[must_use]
    pub fn trunc_int(self) -> Int {
        Int::new(self.0 / Self::SCALE)
    }

    /// Integer part rounded toward negative infinity.
    #[must_use]
    pub fn floor_int(self) -> Int {
        let q = self.0 / Self::SCALE;
        Int::new(if self.0 % Self::SCALE < 0 { q - 1 } else { q })
    }

    /// Integer part rounded toward positive infinity.
    #[must_use]
    pub fn ceil_int(self) -> Int {
        let q = self.0 / Self::SCALE;
        Int::new(if self.0 % Self::SCALE > 0 { q + 1 } else { q })
    }

    /// Nearest integer, ties away from zero.
    #[must_use]
    pub fn round_int(self) -> Int {
        let q = self.0 / Self::SCALE;
        let r = self.0 % Self::SCALE;
        Int::new(if r >= Self::SCALE / 2 {
            q + 1
        } else if r <= -(Self::SCALE / 2) {
            q - 1
        } else {
            q
        })
    }
}

impl fmt::Display for Fixed2 {
    /// Sign, integer part, `.`, exactly two digits: `1.50`, `-0.05`, `0.00`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let mag = self.0.unsigned_abs();
        let scale = Self::SCALE.unsigned_abs();
        write!(f, "{sign}{}.{:02}", mag / scale, mag % scale)
    }
}

impl CanonicalEncode for Fixed2 {
    /// The raw `i64` as 8 bytes little-endian.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    // REQ-CORE-006: scale-2 fixed point for half values.
    use super::*;
    use crate::num::testkit::{EDGES, Rng, pairs};

    fn f(raw: i64) -> Fixed2 {
        Fixed2::from_raw(raw)
    }

    /// Independent oracle: round(|n| / |d|) half up on the magnitude, then reapply the sign.
    fn oracle_round(n: i128, d: i128) -> i128 {
        let neg = (n < 0) != (d < 0);
        let (a, b) = (n.unsigned_abs(), d.unsigned_abs());
        let mag = i128::try_from((2 * a + b) / (2 * b)).unwrap();
        if neg { -mag } else { mag }
    }

    fn fits(v: i128) -> Option<Fixed2> {
        i64::try_from(v).ok().map(Fixed2::from_raw)
    }

    // REQ-CORE-006
    #[test]
    fn constants_and_constructors() {
        assert_eq!(Fixed2::SCALE, 100);
        assert_eq!(Fixed2::ZERO.raw(), 0);
        assert_eq!(Fixed2::ONE.raw(), 100);
        assert_eq!(Fixed2::HALF.raw(), 50);
        assert_eq!(Fixed2::MIN.raw(), i64::MIN);
        assert_eq!(Fixed2::MAX.raw(), i64::MAX);
        assert_eq!(Fixed2::default(), Fixed2::ZERO);
        assert_eq!(Fixed2::from_int(Int::new(3)), Some(f(300)));
        assert_eq!(Fixed2::from_int(Int::new(-3)), Some(f(-300)));
        assert_eq!(Fixed2::from_int(Int::MAX), None);
        assert_eq!(Fixed2::from_halves(Int::new(2)), Some(Fixed2::ONE));
        assert_eq!(Fixed2::from_halves(Int::new(-1)), Some(f(-50)));
        assert_eq!(Fixed2::from_halves(Int::MIN), None);
        assert_eq!(Fixed2::try_from_int(Int::new(1)), Ok(Fixed2::ONE));
        assert_eq!(Fixed2::try_from_int(Int::MIN), Err(OverflowError));
        assert_eq!(Fixed2::try_from_halves(Int::new(1)), Ok(Fixed2::HALF));
        assert_eq!(Fixed2::try_from_halves(Int::MAX), Err(OverflowError));
        assert_eq!(Fixed2::HALF.checked_add(Fixed2::HALF), Some(Fixed2::ONE));
        assert!(f(-1) < Fixed2::ZERO && Fixed2::HALF < Fixed2::ONE);
    }

    // REQ-CORE-006
    #[test]
    fn add_sub_neg_abs_examples() {
        assert_eq!(Fixed2::MAX.checked_add(f(1)), None);
        assert_eq!(Fixed2::MIN.checked_sub(f(1)), None);
        assert_eq!(Fixed2::MAX.saturating_add(f(1)), Fixed2::MAX);
        assert_eq!(Fixed2::MIN.saturating_sub(f(1)), Fixed2::MIN);
        assert_eq!(Fixed2::MIN.checked_neg(), None);
        assert_eq!(Fixed2::MIN.checked_abs(), None);
        assert_eq!(f(-5).checked_abs(), Some(f(5)));
        assert_eq!(f(5).checked_neg(), Some(f(-5)));
        assert_eq!(Fixed2::MIN.saturating_neg(), Fixed2::MAX);
        assert_eq!(Fixed2::MIN.saturating_abs(), Fixed2::MAX);
        assert_eq!(f(150).checked_mul_int(Int::new(-3)), Some(f(-450)));
        assert_eq!(Fixed2::MAX.checked_mul_int(Int::new(2)), None);
    }

    // REQ-CORE-006: rounding examples from the brief.
    #[test]
    fn mul_div_examples() {
        assert_eq!(f(150).checked_mul(f(150)), Some(f(225)));
        assert_eq!(f(5).checked_mul(f(10)), Some(f(1)));
        assert_eq!(f(-5).checked_mul(f(10)), Some(f(-1)));
        assert_eq!(f(1).checked_mul(f(49)), Some(f(0)));
        assert_eq!(f(1).checked_mul(f(50)), Some(f(1)));
        assert_eq!(Fixed2::MAX.checked_mul(f(200)), None);
        assert_eq!(Fixed2::MAX.checked_mul(Fixed2::ONE), Some(Fixed2::MAX));
        assert_eq!(f(100).checked_div(f(300)), Some(f(33)));
        assert_eq!(f(200).checked_div(f(300)), Some(f(67)));
        assert_eq!(f(1).checked_div(f(200)), Some(f(1)));
        assert_eq!(f(-1).checked_div(f(200)), Some(f(-1)));
        assert_eq!(f(1).checked_div(f(-200)), Some(f(-1)));
        assert_eq!(f(1).checked_div(f(4)), Some(f(25)));
        assert_eq!(f(1).checked_div(Fixed2::ZERO), None);
        assert_eq!(Fixed2::MAX.checked_div(f(1)), None);
        assert_eq!(Fixed2::MIN.checked_div(f(-100)), None);
    }

    // REQ-CORE-006: Int conversions, including raw MIN and MAX.
    #[test]
    fn int_conversion_examples() {
        let cases: [(i64, i64, i64, i64, i64); 6] = [
            (50, 0, 0, 1, 1),
            (-50, 0, -1, 0, -1),
            (-199, -1, -2, -1, -2),
            (250, 2, 2, 3, 3),
            (
                i64::MIN,
                -92_233_720_368_547_758,
                -92_233_720_368_547_759,
                -92_233_720_368_547_758,
                -92_233_720_368_547_758,
            ),
            (
                i64::MAX,
                92_233_720_368_547_758,
                92_233_720_368_547_758,
                92_233_720_368_547_759,
                92_233_720_368_547_758,
            ),
        ];
        for (raw, t, fl, c, r) in cases {
            let x = f(raw);
            assert_eq!(x.trunc_int(), Int::new(t), "trunc {raw}");
            assert_eq!(x.floor_int(), Int::new(fl), "floor {raw}");
            assert_eq!(x.ceil_int(), Int::new(c), "ceil {raw}");
            assert_eq!(x.round_int(), Int::new(r), "round {raw}");
        }
    }

    // REQ-CORE-006
    #[test]
    fn display_examples() {
        assert_eq!(f(150).to_string(), "1.50");
        assert_eq!(f(5).to_string(), "0.05");
        assert_eq!(f(-5).to_string(), "-0.05");
        assert_eq!(f(0).to_string(), "0.00");
        assert_eq!(f(-250).to_string(), "-2.50");
        assert_eq!(Fixed2::MIN.to_string(), "-92233720368547758.08");
        assert_eq!(Fixed2::MAX.to_string(), "92233720368547758.07");
    }

    // REQ-CORE-006, REQ-CORE-002: canonical encoding hook.
    #[test]
    fn canonical_encoding() {
        let mut out = vec![1, 2];
        f(-2).encode_canonical(&mut out);
        assert_eq!(out, [1, 2, 0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    // REQ-CORE-006: property, arithmetic against i128 oracles.
    #[test]
    fn prop_arithmetic_matches_oracle() {
        for (a, b) in pairs(&mut Rng::new(0xF1C5_0002), 6_000) {
            let (x, y) = (f(a), f(b));
            let (wa, wb) = (i128::from(a), i128::from(b));
            assert_eq!(x.checked_add(y), fits(wa + wb));
            assert_eq!(x.checked_sub(y), fits(wa - wb));
            let clamp = |v: i128| {
                f(i64::try_from(v.clamp(i128::from(i64::MIN), i128::from(i64::MAX))).unwrap())
            };
            assert_eq!(x.saturating_add(y), clamp(wa + wb));
            assert_eq!(x.saturating_sub(y), clamp(wa - wb));
            assert_eq!(x.checked_mul_int(Int::new(b)), fits(wa * wb));
            assert_eq!(
                x.checked_mul(y),
                fits(oracle_round(wa * wb, 100)),
                "{a} * {b}"
            );
            assert_eq!(x.checked_mul(y), y.checked_mul(x));
            if b == 0 {
                assert_eq!(x.checked_div(y), None);
            } else {
                assert_eq!(
                    x.checked_div(y),
                    fits(oracle_round(wa * 100, wb)),
                    "{a} / {b}"
                );
            }
        }
    }

    // REQ-CORE-006: property, exact ties round away from zero.
    #[test]
    fn prop_ties_round_away_from_zero() {
        let mut rng = Rng::new(0x71E5_0003);
        for _ in 0..5_000 {
            // a = 10k + 5 and b = 0.10 make the exact product a / 10 = k + 0.5 hundredths: a tie.
            let k = rng.next_i64() >> 20;
            let a = k * 10 + 5;
            let expected = if a > 0 { k + 1 } else { k };
            assert_eq!(f(a).checked_mul(f(10)), Some(f(expected)), "{a}");
            assert_eq!(f(-a).checked_mul(f(10)), Some(f(-expected)), "-{a}");
            // a / 200 hundredths with a = 2m + 1 is m + 0.5: a tie in division.
            let m = rng.next_i64() >> 20;
            let n = 2 * m + 1;
            let q = if n > 0 { m + 1 } else { m };
            assert_eq!(f(n).checked_div(f(200)), Some(f(q)), "{n}");
        }
    }

    // REQ-CORE-006: property, conversion ordering invariants.
    #[test]
    fn prop_int_conversions() {
        let mut rng = Rng::new(0xC0DE_0004);
        let values = EDGES
            .iter()
            .copied()
            .chain((0..6_000).map(|_| rng.next_i64()));
        for raw in values {
            let x = f(raw);
            let w = i128::from(raw);
            let (t, fl, c, r) = (x.trunc_int(), x.floor_int(), x.ceil_int(), x.round_int());
            assert!(fl <= r && r <= c, "{raw}");
            assert!(fl <= t && t <= c, "{raw}");
            let gap = i128::from(c.get()) - i128::from(fl.get());
            assert!(gap == 0 || gap == 1, "{raw}");
            assert_eq!(gap == 0, raw % 100 == 0);
            assert_eq!(i128::from(t.get()), w / 100);
            assert_eq!(i128::from(fl.get()), w.div_euclid(100));
            assert_eq!(i128::from(r.get()), oracle_round(w, 100));
            assert_eq!(Fixed2::from_raw(raw).raw(), raw);
            // Display round trip of the digits
            let s = x.to_string();
            let digits: String = s.chars().filter(char::is_ascii_digit).collect();
            assert_eq!(
                digits.trim_start_matches('0'),
                w.unsigned_abs().to_string().trim_start_matches('0')
            );
            assert_eq!(s.starts_with('-'), raw < 0);
        }
    }
}
