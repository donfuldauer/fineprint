// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The per-player mana pool (REQ-CAST-001; CR 106.4, 106.6a, 500.5, 703.4q).

use fineprint_base::encode::CanonicalEncode;

use super::{ManaType, ManaUnit};

/// One player's mana pool (CR 106.4): the units in insertion order, each tracked individually
/// with its restrictions and riders (CR 106.6a). The order is deterministic (R3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ManaPool {
    units: Vec<ManaUnit>,
}

impl ManaPool {
    /// An empty pool.
    #[must_use]
    pub fn new() -> ManaPool {
        ManaPool::default()
    }

    /// CR 106.4: adds one mana to the pool, after the units already there.
    pub fn add(&mut self, unit: ManaUnit) {
        self.units.push(unit);
    }

    /// The units, in insertion order.
    #[must_use]
    pub fn units(&self) -> &[ManaUnit] {
        &self.units
    }

    /// The number of units.
    #[must_use]
    pub fn len(&self) -> usize {
        self.units.len()
    }

    /// Whether the pool holds no mana.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// The number of units of type `t`.
    #[must_use]
    pub fn count(&self, t: ManaType) -> usize {
        self.units.iter().filter(|u| u.mana_type == t).count()
    }

    /// CR 500.5, 703.4q: the pool empties as a step or phase ends; returns the units lost, in
    /// pool order.
    pub fn empty(&mut self) -> Vec<ManaUnit> {
        core::mem::take(&mut self.units)
    }

    /// Removes the units at `indices` (ascending, distinct, in range) and returns them in
    /// ascending index order. Out-of-range and repeated indices are ignored.
    pub(crate) fn take(&mut self, indices: &[usize]) -> Vec<ManaUnit> {
        let mut wanted: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|i| *i < self.units.len())
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
        let mut taken = Vec::with_capacity(wanted.len());
        let mut kept = Vec::with_capacity(self.units.len().saturating_sub(wanted.len()));
        let mut next = wanted.iter().peekable();
        for (i, unit) in core::mem::take(&mut self.units).into_iter().enumerate() {
            if next.peek() == Some(&&i) {
                next.next();
                taken.push(unit);
            } else {
                kept.push(unit);
            }
        }
        self.units = kept;
        taken
    }
}

impl CanonicalEncode for ManaPool {
    /// The units in pool order (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.units.encode_canonical(out);
    }
}
