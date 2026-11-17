// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_base::num` and the numeric canonical encodings.
//! Black-box: written from the work-item brief only.

use fineprint_base::ids::CanonicalEncode;
use fineprint_base::num::{Fixed2, Int, OverflowError};

/// Deterministic splitmix64 generator with a fixed seed (no time, no env).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn i64(&mut self) -> i64 {
        match self.next() % 6 {
            0 => self.next() as i64,
            1 => (self.next() % 2001) as i64 - 1000,
            2 => EDGES[(self.next() % EDGES.len() as u64) as usize],
            3 => i64::MAX - (self.next() % 1000) as i64,
            4 => i64::MIN + (self.next() % 1000) as i64,
            _ => (self.next() as i64) >> 31,
        }
    }
}

const EDGES: [i64; 17] = [
    0,
    1,
    -1,
    2,
    -2,
    50,
    -50,
    99,
    -99,
    100,
    -100,
    101,
    -101,
    i64::MIN,
    i64::MIN + 1,
    i64::MAX - 1,
    i64::MAX,
];

const ITERS: usize = 6_000;

/// Edge cross-product followed by `ITERS` random pairs.
fn pairs(seed: u64) -> Vec<(i64, i64)> {
    let mut v = Vec::new();
    for &a in &EDGES {
        for &b in &EDGES {
            v.push((a, b));
        }
    }
    let mut rng = Rng(seed);
    for _ in 0..ITERS {
        let a = rng.i64();
        let b = rng.i64();
        v.push((a, b));
    }
    v
}

fn singles(seed: u64) -> Vec<i64> {
    let mut v = EDGES.to_vec();
    let mut rng = Rng(seed);
    for _ in 0..ITERS {
        v.push(rng.i64());
    }
    v
}

fn i(x: i64) -> Int {
    Int::new(x)
}

fn f(raw: i64) -> Fixed2 {
    Fixed2::from_raw(raw)
}

fn fit(x: i128) -> Option<i64> {
    i64::try_from(x).ok()
}

fn clamp(x: i128) -> i64 {
    x.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Independent round-half-away-from-zero division oracle. Works on u128
/// magnitudes and never doubles the numerator, so it cannot overflow for any
/// i64 x i64 product. Equivalent to q = (2n + d) / (2d) on magnitudes.
fn div_round(n: i128, d: i128) -> i128 {
    assert!(d != 0);
    let neg = (n < 0) != (d < 0);
    let an = n.unsigned_abs();
    let ad = d.unsigned_abs();
    let mut q = an / ad;
    let r = an % ad;
    // r < ad, so r >= ad - r is the exact-tie-or-above test (2r >= ad).
    if r >= ad - r {
        q += 1;
    }
    let q = q as i128;
    if neg { -q } else { q }
}

// ---------------------------------------------------------------- Int

// REQ-CORE-006
#[test]
fn spec_int_consts_and_accessors() {
    assert_eq!(Int::ZERO.get(), 0);
    assert_eq!(Int::ONE.get(), 1);
    assert_eq!(Int::MIN.get(), i64::MIN);
    assert_eq!(Int::MAX.get(), i64::MAX);
    assert_eq!(Int::default(), Int::ZERO);
    for x in singles(1) {
        assert_eq!(Int::new(x).get(), x);
    }
}

// REQ-CORE-006
#[test]
fn spec_int_from_conversions_and_display() {
    for x in singles(2) {
        let n: Int = x.into();
        assert_eq!(n, Int::new(x));
        let back: i64 = n.into();
        assert_eq!(back, x);
        assert_eq!(n.to_string(), x.to_string());
    }
    assert_eq!(Int::new(-12).to_string(), "-12");
    assert_eq!(Int::new(0).to_string(), "0");
}

// REQ-CORE-006
#[test]
fn spec_int_ordering_matches_i64() {
    for (a, b) in pairs(3) {
        assert_eq!(i(a).cmp(&i(b)), a.cmp(&b));
        assert_eq!(i(a) < i(b), a < b);
        assert_eq!(i(a) == i(b), a == b);
        assert_eq!(i(a).partial_cmp(&i(b)), a.partial_cmp(&b));
    }
}

// REQ-CORE-006
#[test]
fn spec_int_checked_add_sub_mul_match_oracle() {
    for (a, b) in pairs(4) {
        let (x, y) = (a as i128, b as i128);
        assert_eq!(i(a).checked_add(i(b)), fit(x + y).map(Int::new), "{a}+{b}");
        assert_eq!(i(a).checked_sub(i(b)), fit(x - y).map(Int::new), "{a}-{b}");
        assert_eq!(i(a).checked_mul(i(b)), fit(x * y).map(Int::new), "{a}*{b}");
    }
}

// REQ-CORE-006
#[test]
fn spec_int_saturating_add_sub_mul_match_oracle() {
    for (a, b) in pairs(5) {
        let (x, y) = (a as i128, b as i128);
        assert_eq!(i(a).saturating_add(i(b)).get(), clamp(x + y), "{a}+{b}");
        assert_eq!(i(a).saturating_sub(i(b)).get(), clamp(x - y), "{a}-{b}");
        assert_eq!(i(a).saturating_mul(i(b)).get(), clamp(x * y), "{a}*{b}");
    }
}

// REQ-CORE-006
#[test]
fn spec_int_overflow_boundaries() {
    assert_eq!(Int::MAX.checked_add(Int::ONE), None);
    assert_eq!(Int::MIN.checked_sub(Int::ONE), None);
    assert_eq!(Int::MIN.checked_mul(i(-1)), None);
    assert_eq!(Int::MAX.checked_add(Int::ZERO), Some(Int::MAX));
    assert_eq!(Int::MAX.saturating_add(Int::ONE), Int::MAX);
    assert_eq!(Int::MIN.saturating_sub(Int::ONE), Int::MIN);
    assert_eq!(Int::MIN.saturating_mul(i(-1)), Int::MAX);
    assert_eq!(Int::MAX.saturating_mul(i(-2)), Int::MIN);
}

// REQ-CORE-006
#[test]
fn spec_int_neg_abs() {
    for x in singles(6) {
        let n = i(x);
        let neg = if x == i64::MIN { None } else { Some(i(-x)) };
        let abs = if x == i64::MIN {
            None
        } else {
            Some(i(x.abs()))
        };
        assert_eq!(n.checked_neg(), neg, "neg {x}");
        assert_eq!(n.checked_abs(), abs, "abs {x}");
        assert_eq!(n.saturating_neg(), neg.unwrap_or(Int::MAX));
        assert_eq!(n.saturating_abs(), abs.unwrap_or(Int::MAX));
    }
    assert_eq!(Int::MIN.saturating_neg(), Int::MAX);
    assert_eq!(Int::MIN.saturating_abs(), Int::MAX);
    assert_eq!(Int::MIN.checked_neg(), None);
    assert_eq!(Int::MIN.checked_abs(), None);
}

// REQ-CORE-006
#[test]
fn spec_int_checked_div_rem_truncating() {
    for (a, b) in pairs(7) {
        let (x, y) = (a as i128, b as i128);
        let (q, r) = if b == 0 {
            (None, None)
        } else {
            (fit(x / y).map(Int::new), fit(x % y).map(Int::new))
        };
        // MIN / -1 overflows i64 and MIN % -1 is specified as None too.
        let r = if a == i64::MIN && b == -1 { None } else { r };
        assert_eq!(i(a).checked_div(i(b)), q, "{a}/{b}");
        assert_eq!(i(a).checked_rem(i(b)), r, "{a}%{b}");
    }
    assert_eq!(Int::ONE.checked_div(Int::ZERO), None);
    assert_eq!(Int::ONE.checked_rem(Int::ZERO), None);
    assert_eq!(Int::MIN.checked_div(i(-1)), None);
    assert_eq!(Int::MIN.checked_rem(i(-1)), None);
    assert_eq!(i(-7).checked_div(i(2)), Some(i(-3)));
    assert_eq!(i(-7).checked_rem(i(2)), Some(i(-1)));
    assert_eq!(i(7).checked_div(i(-2)), Some(i(-3)));
    assert_eq!(i(7).checked_rem(i(-2)), Some(i(1)));
}

// REQ-CORE-006
#[test]
fn spec_int_algebraic_properties() {
    for (a, b) in pairs(8) {
        assert_eq!(i(a).checked_add(i(b)), i(b).checked_add(i(a)));
        assert_eq!(i(a).checked_mul(i(b)), i(b).checked_mul(i(a)));
        assert_eq!(i(a).saturating_add(i(b)), i(b).saturating_add(i(a)));
        assert_eq!(i(a).saturating_mul(i(b)), i(b).saturating_mul(i(a)));
        if let Some(s) = i(a).checked_add(i(b)) {
            assert_eq!(s.checked_sub(i(b)), Some(i(a)));
        }
    }
}

// REQ-CORE-006
#[test]
fn spec_int_signum_floor_zero() {
    for x in singles(9) {
        assert_eq!(i(x).signum(), i(x.signum()));
        assert_eq!(i(x).floor_zero(), i(x.max(0)));
    }
    assert_eq!(i(-5).signum(), Int::new(-1));
    assert_eq!(Int::ZERO.signum(), Int::ZERO);
    assert_eq!(i(5).signum(), Int::ONE);
    assert_eq!(i(-5).floor_zero(), Int::ZERO);
    assert_eq!(i(5).floor_zero(), i(5));
}

// REQ-CORE-006
#[test]
fn spec_overflow_error_traits() {
    fn is_error<E: std::error::Error>(_: &E) {}
    let e = OverflowError;
    is_error(&e);
    let copy = e;
    assert_eq!(e, copy);
    assert!(!e.to_string().is_empty());
    assert!(!format!("{e:?}").is_empty());
}

// ------------------------------------------------------------- Fixed2

// REQ-CORE-006
#[test]
fn spec_fixed2_consts() {
    assert_eq!(Fixed2::SCALE, 100);
    assert_eq!(Fixed2::ZERO.raw(), 0);
    assert_eq!(Fixed2::ONE.raw(), 100);
    assert_eq!(Fixed2::HALF.raw(), 50);
    assert_eq!(Fixed2::MIN.raw(), i64::MIN);
    assert_eq!(Fixed2::MAX.raw(), i64::MAX);
    assert_eq!(Fixed2::default(), Fixed2::ZERO);
    assert_eq!(Fixed2::HALF.checked_add(Fixed2::HALF), Some(Fixed2::ONE));
    assert_eq!(Fixed2::from_halves(i(2)), Some(Fixed2::ONE));
    assert_eq!(Fixed2::from_halves(i(1)), Some(Fixed2::HALF));
    for x in singles(10) {
        assert_eq!(f(x).raw(), x);
    }
}

// REQ-CORE-006
#[test]
fn spec_fixed2_from_int_and_halves() {
    assert_eq!(Fixed2::from_int(Int::MAX), None);
    assert_eq!(Fixed2::from_int(Int::MIN), None);
    assert_eq!(Fixed2::from_int(i(5)), Some(f(500)));
    assert_eq!(Fixed2::from_int(i(-5)), Some(f(-500)));
    for x in singles(11) {
        let n = x as i128;
        assert_eq!(Fixed2::from_int(i(x)), fit(n * 100).map(f), "{x}");
        assert_eq!(Fixed2::from_halves(i(x)), fit(n * 50).map(f), "{x}");
    }
}

// REQ-CORE-006
#[test]
fn spec_fixed2_add_sub_neg_abs() {
    for (a, b) in pairs(12) {
        let (x, y) = (a as i128, b as i128);
        assert_eq!(f(a).checked_add(f(b)), fit(x + y).map(f));
        assert_eq!(f(a).checked_sub(f(b)), fit(x - y).map(f));
        assert_eq!(f(a).saturating_add(f(b)).raw(), clamp(x + y));
        assert_eq!(f(a).saturating_sub(f(b)).raw(), clamp(x - y));
        assert_eq!(f(a).checked_add(f(b)), f(b).checked_add(f(a)));
    }
    for x in singles(13) {
        let neg = if x == i64::MIN { None } else { Some(f(-x)) };
        let abs = if x == i64::MIN {
            None
        } else {
            Some(f(x.abs()))
        };
        assert_eq!(f(x).checked_neg(), neg);
        assert_eq!(f(x).checked_abs(), abs);
        assert_eq!(f(x).saturating_neg(), neg.unwrap_or(Fixed2::MAX));
        assert_eq!(f(x).saturating_abs(), abs.unwrap_or(Fixed2::MAX));
    }
    assert_eq!(Fixed2::MIN.saturating_neg(), Fixed2::MAX);
    assert_eq!(Fixed2::MIN.saturating_abs(), Fixed2::MAX);
}

// REQ-CORE-006
#[test]
fn spec_fixed2_checked_mul_int() {
    for (a, b) in pairs(14) {
        let exact = (a as i128) * (b as i128);
        assert_eq!(f(a).checked_mul_int(i(b)), fit(exact).map(f), "{a}*{b}");
    }
    assert_eq!(f(150).checked_mul_int(i(3)), Some(f(450)));
    assert_eq!(f(150).checked_mul_int(i(-2)), Some(f(-300)));
    assert_eq!(Fixed2::MAX.checked_mul_int(i(2)), None);
}

// REQ-CORE-006
#[test]
fn spec_fixed2_mul_examples() {
    assert_eq!(f(150).checked_mul(f(150)), Some(f(225)));
    assert_eq!(f(5).checked_mul(f(10)), Some(f(1)));
    assert_eq!(f(-5).checked_mul(f(10)), Some(f(-1)));
    assert_eq!(f(1).checked_mul(f(49)), Some(f(0)));
    assert_eq!(f(1).checked_mul(f(50)), Some(f(1)));
    assert_eq!(f(-1).checked_mul(f(50)), Some(f(-1)));
    assert_eq!(f(-1).checked_mul(f(-50)), Some(f(1)));
    assert_eq!(f(-1).checked_mul(f(49)), Some(f(0)));
    assert_eq!(Fixed2::MAX.checked_mul(Fixed2::MAX), None);
    assert_eq!(Fixed2::MIN.checked_mul(Fixed2::MIN), None);
    assert_eq!(Fixed2::MAX.checked_mul(Fixed2::ONE), Some(Fixed2::MAX));
    assert_eq!(Fixed2::MIN.checked_mul(Fixed2::ONE), Some(Fixed2::MIN));
}

// REQ-CORE-006
#[test]
fn spec_fixed2_mul_matches_oracle() {
    for (a, b) in pairs(15) {
        let exact = div_round((a as i128) * (b as i128), 100);
        assert_eq!(f(a).checked_mul(f(b)), fit(exact).map(f), "{a}*{b}");
        assert_eq!(f(a).checked_mul(f(b)), f(b).checked_mul(f(a)));
    }
}

// REQ-CORE-006
#[test]
fn spec_fixed2_div_examples() {
    assert_eq!(f(100).checked_div(f(300)), Some(f(33)));
    assert_eq!(f(200).checked_div(f(300)), Some(f(67)));
    assert_eq!(f(1).checked_div(f(200)), Some(f(1)));
    assert_eq!(f(-1).checked_div(f(200)), Some(f(-1)));
    assert_eq!(f(1).checked_div(f(-200)), Some(f(-1)));
    assert_eq!(f(-1).checked_div(f(-200)), Some(f(1)));
    assert_eq!(f(1).checked_div(f(4)), Some(f(25)));
    assert_eq!(f(100).checked_div(f(0)), None);
    assert_eq!(Fixed2::ZERO.checked_div(Fixed2::ZERO), None);
    assert_eq!(Fixed2::MAX.checked_div(Fixed2::HALF), None);
    assert_eq!(Fixed2::MAX.checked_div(Fixed2::ONE), Some(Fixed2::MAX));
}

// REQ-CORE-006
#[test]
fn spec_fixed2_div_matches_oracle() {
    for (a, b) in pairs(16) {
        let expect = if b == 0 {
            None
        } else {
            fit(div_round((a as i128) * 100, b as i128)).map(f)
        };
        assert_eq!(f(a).checked_div(f(b)), expect, "{a}/{b}");
    }
}

fn conv_expect(raw: i64) -> (i64, i64, i64, i64) {
    let r = raw as i128;
    let trunc = r / 100;
    let floor = r.div_euclid(100);
    let ceil = -((-r).div_euclid(100));
    let round = div_round(r, 100);
    (trunc as i64, floor as i64, ceil as i64, round as i64)
}

// REQ-CORE-006
#[test]
fn spec_fixed2_int_conversion_examples() {
    let cases: [(i64, [i64; 4]); 4] = [
        (50, [0, 0, 1, 1]),
        (-50, [0, -1, 0, -1]),
        (-199, [-1, -2, -1, -2]),
        (250, [2, 2, 3, 3]),
    ];
    for (raw, [t, fl, c, r]) in cases {
        assert_eq!(f(raw).trunc_int(), i(t), "trunc {raw}");
        assert_eq!(f(raw).floor_int(), i(fl), "floor {raw}");
        assert_eq!(f(raw).ceil_int(), i(c), "ceil {raw}");
        assert_eq!(f(raw).round_int(), i(r), "round {raw}");
    }
}

// REQ-CORE-006
#[test]
fn spec_fixed2_int_conversions_match_oracle_and_invariants() {
    for raw in singles(17) {
        let (t, fl, c, r) = conv_expect(raw);
        let v = f(raw);
        assert_eq!(v.trunc_int().get(), t, "trunc {raw}");
        assert_eq!(v.floor_int().get(), fl, "floor {raw}");
        assert_eq!(v.ceil_int().get(), c, "ceil {raw}");
        assert_eq!(v.round_int().get(), r, "round {raw}");
        let (fl, c, r) = (v.floor_int().get(), v.ceil_int().get(), v.round_int().get());
        assert!(fl <= r && r <= c, "{raw}");
        let d = (c as i128) - (fl as i128);
        assert!(d == 0 || d == 1, "{raw}");
        assert_eq!(d == 0, raw % 100 == 0, "{raw}");
    }
    assert_eq!(Fixed2::MIN.trunc_int().get(), -92_233_720_368_547_758);
    assert_eq!(Fixed2::MIN.floor_int().get(), -92_233_720_368_547_759);
    assert_eq!(Fixed2::MIN.ceil_int().get(), -92_233_720_368_547_758);
    assert_eq!(Fixed2::MIN.round_int().get(), -92_233_720_368_547_758);
    assert_eq!(Fixed2::MAX.trunc_int().get(), 92_233_720_368_547_758);
    assert_eq!(Fixed2::MAX.floor_int().get(), 92_233_720_368_547_758);
    assert_eq!(Fixed2::MAX.ceil_int().get(), 92_233_720_368_547_759);
    assert_eq!(Fixed2::MAX.round_int().get(), 92_233_720_368_547_758);
}

// REQ-CORE-006
#[test]
fn spec_fixed2_display() {
    assert_eq!(f(150).to_string(), "1.50");
    assert_eq!(f(5).to_string(), "0.05");
    assert_eq!(f(-5).to_string(), "-0.05");
    assert_eq!(f(0).to_string(), "0.00");
    assert_eq!(f(100).to_string(), "1.00");
    assert_eq!(f(-150).to_string(), "-1.50");
    assert_eq!(Fixed2::MIN.to_string(), "-92233720368547758.08");
    assert_eq!(Fixed2::MAX.to_string(), "92233720368547758.07");
    for raw in singles(18) {
        let r = raw as i128;
        let sign = if r < 0 { "-" } else { "" };
        let expect = format!("{sign}{}.{:02}", r.abs() / 100, r.abs() % 100);
        assert_eq!(f(raw).to_string(), expect);
    }
}

// REQ-CORE-006
#[test]
fn spec_fixed2_ordering_matches_raw() {
    for (a, b) in pairs(19) {
        assert_eq!(f(a).cmp(&f(b)), a.cmp(&b));
        assert_eq!(f(a) == f(b), a == b);
    }
    assert!(f(-1) < Fixed2::ZERO && Fixed2::ZERO < Fixed2::HALF && Fixed2::HALF < Fixed2::ONE);
}

// ----------------------------------------------------------- Encoding

fn enc<T: CanonicalEncode>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode_canonical(&mut out);
    out
}

// REQ-CORE-002, REQ-CORE-006
#[test]
fn spec_int_canonical_encoding() {
    assert_eq!(enc(&Int::new(-1)), vec![0xFF; 8]);
    assert_eq!(enc(&Int::new(0)), vec![0; 8]);
    assert_eq!(enc(&Int::new(1)), vec![1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(enc(&Int::MIN), vec![0, 0, 0, 0, 0, 0, 0, 0x80]);
    assert_eq!(
        enc(&Int::MAX),
        vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F]
    );
    for x in singles(20) {
        let b = enc(&i(x));
        assert_eq!(b.len(), 8);
        assert_eq!(i64::from_le_bytes(b.try_into().unwrap()), x);
    }
}

// REQ-CORE-002, REQ-CORE-006
#[test]
fn spec_fixed2_canonical_encoding() {
    assert_eq!(
        enc(&f(-2)),
        vec![0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
    );
    assert_eq!(enc(&Fixed2::ONE), vec![100, 0, 0, 0, 0, 0, 0, 0]);
    for x in singles(21) {
        let b = enc(&f(x));
        assert_eq!(b.len(), 8);
        assert_eq!(i64::from_le_bytes(b.try_into().unwrap()), x);
    }
}

// REQ-CORE-002, REQ-CORE-006
#[test]
fn spec_numeric_encoding_appends() {
    let mut out = vec![9u8, 9, 9];
    i(1).encode_canonical(&mut out);
    f(2).encode_canonical(&mut out);
    assert_eq!(out.len(), 3 + 16);
    assert_eq!(&out[..3], &[9, 9, 9]);
    assert_eq!(&out[3..11], &[1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&out[11..], &[2, 0, 0, 0, 0, 0, 0, 0]);
}
