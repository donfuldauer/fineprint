// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unbiased sampling on top of [`Pcg64Dxsm`] (REQ-CORE-001).
//!
//! Every algorithm here is part of the frozen output: the number of draws each call consumes,
//! and the value it returns, must not change within an engine major version.

use super::pcg64::Pcg64Dxsm;

impl Pcg64Dxsm {
    /// An unbiased integer in `[0, bound)` by Lemire's multiply-with-rejection method
    /// (REQ-CORE-001). Returns `None` for `bound == 0` without consuming a draw; `bound == 1`
    /// returns `Some(0)` and consumes one draw.
    pub fn next_bounded(&mut self, bound: u64) -> Option<u64> {
        if bound == 0 {
            return None;
        }
        let mut m = u128::from(self.next_u64()) * u128::from(bound);
        // Truncation to the low 64 bits is the algorithm.
        let mut l = m as u64;
        if l < bound {
            let threshold = bound.wrapping_neg() % bound;
            while l < threshold {
                m = u128::from(self.next_u64()) * u128::from(bound);
                l = m as u64;
            }
        }
        Some((m >> 64) as u64)
    }

    /// Shuffles `items` in place with the Fisher-Yates algorithm, from the last index down
    /// (REQ-CORE-001): for each `i` from `len - 1` down to `1`, swap `items[i]` with
    /// `items[next_bounded(i + 1)]`. Slices of length 0 or 1 consume no draws.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let Ok(i_u64) = u64::try_from(i) else {
                return;
            };
            // The bound is always >= 2, so `None` cannot occur; leave the slice and return.
            let Some(j) = self.next_bounded(i_u64.wrapping_add(1)) else {
                return;
            };
            // j <= i, so it always fits in usize.
            let Ok(j) = usize::try_from(j) else {
                return;
            };
            items.swap(i, j);
        }
    }

    /// A uniformly random element of `items` (REQ-CORE-001); `None` for an empty slice, which
    /// consumes no draw.
    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        let len = u64::try_from(items.len()).ok()?;
        let index = usize::try_from(self.next_bounded(len)?).ok()?;
        items.get(index)
    }

    /// A uniformly random `k`-element subset of `0..n`, as indices in ascending order, by
    /// selection sampling (REQ-CORE-001). Index `i` is chosen when
    /// `next_bounded(n - i) < k - chosen`; sampling stops as soon as `k` indices are chosen.
    ///
    /// Returns `None` when `k > n`. Returns `None` when the result vector for `k` indices cannot
    /// be reserved (capacity overflow or allocation failure); the call never panics or aborts on
    /// a capacity computation overflow. In every `None` case no draw is consumed. `k == 0`
    /// returns an empty set without consuming a draw.
    pub fn random_subset(&mut self, n: usize, k: usize) -> Option<Vec<usize>> {
        if k > n {
            return None;
        }
        // Reserve before the first draw so a failed reservation leaves the generator untouched.
        let mut chosen: Vec<usize> = Vec::new();
        chosen.try_reserve_exact(k).ok()?;
        for i in 0..n {
            let remaining_needed = k - chosen.len();
            if remaining_needed == 0 {
                break;
            }
            let remaining_pool = u64::try_from(n - i).ok()?;
            let needed = u64::try_from(remaining_needed).ok()?;
            if self.next_bounded(remaining_pool)? < needed {
                chosen.push(i);
            }
        }
        Some(chosen)
    }
}
