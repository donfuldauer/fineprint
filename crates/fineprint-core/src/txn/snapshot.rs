// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Engine-level snapshot store for sandbox rewind (REQ-CORE-005 last bullet, REQ-API-003).
//!
//! A separate feature from rules rollback: it is not governed by CR 734 and never interacts
//! with [`Transactions`](super::Transactions). Whether a match may use it (FormatConfig) is
//! decided by the API layer, not here.

use core::fmt;
use std::collections::VecDeque;

/// An error from [`SnapshotStore`] (REQ-API-003). A call that returns an error changes nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SnapshotError {
    /// A snapshot store needs a capacity of at least 1.
    ZeroCapacity,
    /// `rewind(n)` with n >= len().
    NotEnoughSnapshots {
        /// The `n` passed to `rewind`.
        requested: usize,
        /// The number of snapshots held.
        available: usize,
    },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::ZeroCapacity => {
                f.write_str("a snapshot store needs a capacity of at least 1")
            }
            SnapshotError::NotEnoughSnapshots {
                requested,
                available,
            } => write!(
                f,
                "cannot rewind {requested} decision tick(s): only {available} snapshot(s) held"
            ),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// A bounded history of state snapshots, one per decision tick, for sandbox
/// `RewindToSnapshot` (REQ-API-003). When full, recording drops the oldest snapshot.
#[derive(Clone, Debug)]
pub struct SnapshotStore<S: Clone> {
    /// Oldest first, latest last; never longer than `capacity`.
    snapshots: VecDeque<S>,
    capacity: usize,
}

impl<S: Clone> SnapshotStore<S> {
    /// An empty store holding at most `capacity` snapshots.
    ///
    /// # Errors
    /// [`SnapshotError::ZeroCapacity`] if `capacity` is 0.
    pub fn new(capacity: usize) -> Result<Self, SnapshotError> {
        if capacity == 0 {
            return Err(SnapshotError::ZeroCapacity);
        }
        Ok(SnapshotStore {
            snapshots: VecDeque::new(),
            capacity,
        })
    }

    /// Appends a snapshot (a clone) of `state` as the latest; if the store is full, the oldest
    /// snapshot is dropped first.
    pub fn record(&mut self, state: &S) {
        if self.snapshots.len() >= self.capacity {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(state.clone());
    }

    /// The number of snapshots held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    /// `true` iff no snapshot is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    /// The largest number of snapshots held.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The latest snapshot, if any.
    #[must_use]
    pub fn latest(&self) -> Option<&S> {
        self.snapshots.back()
    }

    /// Undo `n` decision ticks: return a clone of the snapshot recorded `n` records before the latest
    /// (n == 0 returns the latest) and discard the `n` newest, so that snapshot becomes the latest.
    ///
    /// # Errors
    /// If n >= len(): [`SnapshotError::NotEnoughSnapshots`] `{ requested: n, available: len() }`
    /// and the store is unchanged.
    pub fn rewind(&mut self, n: usize) -> Result<S, SnapshotError> {
        let available = self.snapshots.len();
        if n >= available {
            return Err(SnapshotError::NotEnoughSnapshots {
                requested: n,
                available,
            });
        }
        self.snapshots.truncate(available - n);
        self.snapshots
            .back()
            .cloned()
            .ok_or(SnapshotError::NotEnoughSnapshots {
                requested: n,
                available,
            })
    }
}
