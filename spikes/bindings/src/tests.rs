// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the FP-005 spike crate. Every test names REQ-NFR-002 and/or REQ-NFR-003.

use super::*;

/// The shared fixed-input file that every caller (C, .NET, Node, browsers) reads (REQ-NFR-003).
const VECTORS: &str = include_str!("../vectors.txt");

/// Sentinel used to prove that error paths write nothing to `out`.
const UNTOUCHED: [u8; FP_SPIKE_SHA256_LEN] = [0xA5; FP_SPIKE_SHA256_LEN];

struct Vector {
    name: &'static str,
    input: Vec<u8>,
    expected: &'static str,
}

fn decode_hex(s: &str) -> Vec<u8> {
    if s == "-" {
        return Vec::new();
    }
    assert!(s.len().is_multiple_of(2), "odd-length hex input");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex input"))
        .collect()
}

fn vectors() -> Vec<Vector> {
    VECTORS
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next().expect("name");
            let input = decode_hex(fields.next().expect("input"));
            let expected = fields.next().expect("expected");
            assert!(fields.next().is_none(), "extra field in vectors.txt");
            Vector {
                name,
                input,
                expected,
            }
        })
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    use core::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        write!(s, "{b:02x}").expect("write to String");
        s
    })
}

/// Reference digest computed through sha2's incremental API, one byte at a time, independently
/// of the one-shot path the library uses.
fn reference(data: &[u8]) -> [u8; FP_SPIKE_SHA256_LEN] {
    let mut h = Sha256::new();
    for b in data {
        h.update([*b]);
    }
    h.finalize().into()
}

fn ffi(data: &[u8]) -> (i32, [u8; FP_SPIKE_SHA256_LEN]) {
    let mut out = UNTOUCHED;
    // SAFETY: `data` is a live slice of `data.len()` bytes; `out` is a local 32-byte array.
    let rc = unsafe { fp_spike_sha256(data.as_ptr(), data.len(), out.as_mut_ptr()) };
    (rc, out)
}

/// Deterministic pseudo-random bytes (64-bit LCG, integer only; R3).
fn pseudo_random_bytes(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed;
    (0..len)
        .map(|_| {
            x = x
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            x.to_be_bytes()[0]
        })
        .collect()
}

#[test]
fn req_nfr_003_vectors_file_has_the_required_inputs() {
    let v = vectors();
    let names: Vec<&str> = v.iter().map(|v| v.name).collect();
    assert_eq!(names, ["empty", "abc", "seq200"]);
    assert_eq!(
        v[0].expected,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert!(v[0].input.is_empty());
    assert_eq!(
        v[1].expected,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(v[1].input, b"abc");
    let seq: Vec<u8> = (0..=199u8).collect();
    assert_eq!(v[2].input, seq);
}

#[test]
fn req_nfr_002_req_nfr_003_known_vectors_safe_api() {
    for v in vectors() {
        assert_eq!(sha256_hex(&v.input), v.expected, "vector {}", v.name);
        assert_eq!(to_hex(&sha256(&v.input)), v.expected, "vector {}", v.name);
    }
}

#[test]
fn req_nfr_002_req_nfr_003_known_vectors_ffi() {
    for v in vectors() {
        let (rc, out) = ffi(&v.input);
        assert_eq!(rc, FP_SPIKE_OK, "vector {}", v.name);
        assert_eq!(to_hex(&out), v.expected, "vector {}", v.name);
    }
}

#[test]
fn req_nfr_003_multi_block_inputs_cross_the_64_byte_boundary() {
    for len in [55, 56, 57, 63, 64, 65, 119, 120, 127, 128, 129, 192, 200] {
        let data = pseudo_random_bytes(len, 0x5eed);
        let (rc, out) = ffi(&data);
        assert_eq!(rc, FP_SPIKE_OK, "len {len}");
        assert_eq!(out, reference(&data), "len {len}");
        assert_eq!(sha256(&data), reference(&data), "len {len}");
    }
}

#[test]
fn req_nfr_002_null_data_with_zero_len_is_the_empty_input() {
    let mut out = UNTOUCHED;
    // SAFETY: null data with len 0 is allowed by the contract; `out` is a local 32-byte array.
    let rc = unsafe { fp_spike_sha256(core::ptr::null(), 0, out.as_mut_ptr()) };
    assert_eq!(rc, FP_SPIKE_OK);
    assert_eq!(
        to_hex(&out),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn req_nfr_002_non_null_data_with_zero_len_is_the_empty_input() {
    let (rc, out) = ffi(&[]);
    assert_eq!(rc, FP_SPIKE_OK);
    assert_eq!(out, sha256(&[]));
    let backing = [1u8, 2, 3];
    let mut out = UNTOUCHED;
    // SAFETY: len 0 means `data` is never read; `out` is a local 32-byte array.
    let rc = unsafe { fp_spike_sha256(backing.as_ptr(), 0, out.as_mut_ptr()) };
    assert_eq!(rc, FP_SPIKE_OK);
    assert_eq!(out, sha256(&[]));
}

#[test]
fn req_nfr_002_null_out_returns_nonzero() {
    let data = b"abc";
    // SAFETY: `out` is null, which the function rejects before reading or writing anything.
    let rc = unsafe { fp_spike_sha256(data.as_ptr(), data.len(), core::ptr::null_mut()) };
    assert_eq!(rc, FP_SPIKE_ERR_NULL_OUT);
    assert_ne!(rc, FP_SPIKE_OK);
    // SAFETY: as above; both pointers null.
    let rc = unsafe { fp_spike_sha256(core::ptr::null(), 0, core::ptr::null_mut()) };
    assert_eq!(rc, FP_SPIKE_ERR_NULL_OUT);
}

#[test]
fn req_nfr_002_null_data_with_positive_len_returns_nonzero_and_writes_nothing() {
    let mut out = UNTOUCHED;
    // SAFETY: null data with len > 0 is rejected before any read; `out` is a local array.
    let rc = unsafe { fp_spike_sha256(core::ptr::null(), 3, out.as_mut_ptr()) };
    assert_eq!(rc, FP_SPIKE_ERR_NULL_DATA);
    assert_ne!(rc, FP_SPIKE_OK);
    assert_eq!(out, UNTOUCHED);
}

#[test]
fn req_nfr_002_len_above_isize_max_returns_nonzero_and_writes_nothing() {
    let backing = [0u8; 1];
    for len in [isize::MAX.unsigned_abs() + 1, usize::MAX] {
        let mut out = UNTOUCHED;
        // SAFETY: the oversized len is rejected before `data` is read; `out` is a local array.
        let rc = unsafe { fp_spike_sha256(backing.as_ptr(), len, out.as_mut_ptr()) };
        assert_eq!(rc, FP_SPIKE_ERR_LEN, "len {len}");
        assert_eq!(out, UNTOUCHED, "len {len}");
        // SAFETY: as above, with null data.
        let rc = unsafe { fp_spike_sha256(core::ptr::null(), len, out.as_mut_ptr()) };
        assert_eq!(rc, FP_SPIKE_ERR_LEN, "len {len}");
        assert_eq!(out, UNTOUCHED, "len {len}");
    }
}

#[test]
fn req_nfr_002_error_codes_are_distinct_and_nonzero() {
    let codes = [
        FP_SPIKE_ERR_NULL_OUT,
        FP_SPIKE_ERR_NULL_DATA,
        FP_SPIKE_ERR_LEN,
    ];
    for (i, a) in codes.iter().enumerate() {
        assert_ne!(*a, FP_SPIKE_OK);
        for b in &codes[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

/// Bounded random-bytes run (R12): every length 0..=300 with varied content hashes through the
/// FFI function without panicking and matches the sha2 reference computed in the test.
#[test]
fn req_nfr_002_req_nfr_003_bounded_random_lengths_match_reference() {
    for len in 0..=300usize {
        for seed in [1u64, 0xdead_beef, u64::try_from(len).expect("small")] {
            let data = pseudo_random_bytes(len, seed);
            let (rc, out) = ffi(&data);
            assert_eq!(rc, FP_SPIKE_OK, "len {len} seed {seed}");
            assert_eq!(out, reference(&data), "len {len} seed {seed}");
            assert_eq!(sha256_hex(&data), to_hex(&out), "len {len} seed {seed}");
        }
    }
}

#[test]
fn req_nfr_003_hex_is_lowercase_and_64_chars() {
    let h = sha256_hex(b"abc");
    assert_eq!(h.len(), 64);
    assert!(
        h.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    );
}
