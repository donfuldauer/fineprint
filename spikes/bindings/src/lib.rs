// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! FP-005 (#6) binding-toolchain spike (REQ-NFR-002, REQ-NFR-003).
//!
//! A toy SHA-256 function exposed three ways so the binding toolchains can be proved end to end
//! before the engine depends on them:
//!
//! * a C ABI function, [`fp_spike_sha256`], with a cbindgen-generated header
//!   (`include/fineprint_spike.h`), called from C and from .NET through P/Invoke;
//! * a wasm-bindgen function, `sha256_hex`, compiled only for `wasm32` targets, called from
//!   Node.js and from headless browsers;
//! * the safe Rust functions [`sha256`] and [`sha256_hex`] that both wrap.
//!
//! The hash is pure and deterministic (R3): it reads nothing but its input.
//!
//! Unsafe code is permitted in this spike crate only, and only in [`fp_spike_sha256`].

use sha2::{Digest, Sha256};

/// Length in bytes of a SHA-256 digest; the size of the `out` buffer of [`fp_spike_sha256`].
pub const FP_SPIKE_SHA256_LEN: usize = 32;

/// Return code: success; `out` holds the digest.
pub const FP_SPIKE_OK: i32 = 0;

/// Return code: `out` was null. Nothing was written.
pub const FP_SPIKE_ERR_NULL_OUT: i32 = 1;

/// Return code: `data` was null while `len` was greater than zero. Nothing was written.
pub const FP_SPIKE_ERR_NULL_DATA: i32 = 2;

/// Return code: `len` exceeds `isize::MAX`, so it cannot describe a valid buffer.
/// Nothing was written.
pub const FP_SPIKE_ERR_LEN: i32 = 3;

/// Returns the SHA-256 digest of `data`.
#[must_use]
pub fn sha256(data: &[u8]) -> [u8; FP_SPIKE_SHA256_LEN] {
    Sha256::digest(data).into()
}

/// Returns the SHA-256 digest of `data` as 64 lowercase hexadecimal characters.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = sha256(data);
    let mut s = String::with_capacity(FP_SPIKE_SHA256_LEN * 2);
    for byte in digest {
        s.push(char::from(HEX[usize::from(byte >> 4)]));
        s.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    s
}

/// Computes the SHA-256 digest of `len` bytes at `data` and writes it to the 32 bytes at `out`.
///
/// Returns [`FP_SPIKE_OK`] (0) on success, or a nonzero error code, in which case nothing is
/// written:
///
/// * [`FP_SPIKE_ERR_NULL_OUT`] if `out` is null;
/// * [`FP_SPIKE_ERR_NULL_DATA`] if `data` is null and `len` is greater than zero;
/// * [`FP_SPIKE_ERR_LEN`] if `len` is greater than `isize::MAX`.
///
/// A null `data` with `len == 0` is the empty input.
///
/// # Safety
///
/// The caller guarantees that:
///
/// * if `len > 0`, `data` points to `len` initialised bytes that are readable for the whole
///   call and are not written by anyone during it;
/// * if `out` is not null, it points to [`FP_SPIKE_SHA256_LEN`] (32) bytes that are writable
///   for the whole call and do not overlap `data`.
///
/// The function reads only `data[0..len]` and writes only `out[0..32]`. It never panics.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fp_spike_sha256(data: *const u8, len: usize, out: *mut u8) -> i32 {
    if out.is_null() {
        return FP_SPIKE_ERR_NULL_OUT;
    }
    if len > isize::MAX.unsigned_abs() {
        return FP_SPIKE_ERR_LEN;
    }
    let input: &[u8] = if len == 0 {
        &[]
    } else if data.is_null() {
        return FP_SPIKE_ERR_NULL_DATA;
    } else {
        // SAFETY: `data` is non-null and, by the caller's contract, points to `len` readable,
        // initialised bytes that are not mutated during the call; `len <= isize::MAX` was
        // checked above.
        unsafe { core::slice::from_raw_parts(data, len) }
    };
    let digest = sha256(input);
    // SAFETY: `out` is non-null and, by the caller's contract, points to 32 writable bytes that
    // do not overlap `data`; `digest` is a local array of exactly 32 bytes.
    unsafe { core::ptr::copy_nonoverlapping(digest.as_ptr(), out, FP_SPIKE_SHA256_LEN) };
    FP_SPIKE_OK
}

/// The wasm-bindgen surface, compiled only for `wasm32` so native builds need no wasm-bindgen.
#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::wasm_bindgen;

    /// Returns the SHA-256 digest of `data` as 64 lowercase hexadecimal characters.
    #[wasm_bindgen]
    #[must_use]
    pub fn sha256_hex(data: &[u8]) -> String {
        crate::sha256_hex(data)
    }
}

#[cfg(test)]
mod tests;
