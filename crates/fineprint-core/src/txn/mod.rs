// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Checkpoints, rollback and nested sub-transactions (REQ-CORE-005, CR 734), plus the separate
//! engine-level snapshot store for sandbox rewind (REQ-API-003).
//!
//! **Rules rollback.** [`Transactions`] wraps a [`Transactional`] state. Before an action the
//! engine calls [`Transactions::checkpoint`]; if the action turns out to be illegal or can't be
//! completed it calls [`Transactions::rollback`], which restores the checkpoint state exactly
//! (same state hash) and keeps the player who had priority (CR 734.1, CR 734.2). Checkpoints
//! nest: a checkpoint taken while another is open is its sub-transaction.
//!
//! **Durable sub-transactions.** A sub-transaction committed with
//! [`Transactions::commit_durable`] survives the rollback of its parent and of every ancestor:
//! its effect (the change from its checkpoint to the moment of the durable commit) is carried
//! over every restored ancestor checkpoint by [`Transactional::carry_durable`] (CR 734.1: the
//! mana abilities that are not reversed with the action).
//!
//! **Sandbox rewind.** [`SnapshotStore`] is a separate bounded history of snapshots for
//! `RewindToSnapshot` (REQ-API-003). It is not governed by CR 734 and never interacts with
//! [`Transactions`].

mod game_state;
mod snapshot;

#[cfg(test)]
mod tests;

use core::fmt;

use fineprint_base::hash::Digest;
use fineprint_base::ids::PlayerId;

pub use snapshot::{SnapshotError, SnapshotStore};

/// A state a [`Transactions`] can wrap (REQ-CORE-005).
pub trait Transactional: Clone {
    /// The canonical hash of the whole state (for `GameState`: `state_hash()`).
    fn state_hash(&self) -> Digest;

    /// The player who currently has priority, if any (CR 734.2).
    fn priority_player(&self) -> Option<PlayerId>;

    /// Called when a transaction is rolled back and some descendant sub-transactions were
    /// committed durably. `restored` is the checkpoint state (with earlier durable effects of the
    /// same rollback already applied). `before` is the state at the moment the durable
    /// sub-transaction began, `after` the state at the moment it was committed durably.
    /// Return `restored` with the effect of that sub-transaction (the change `before` -> `after`)
    /// applied. The result's `priority_player()` must equal `restored.priority_player()`.
    #[must_use]
    fn carry_durable(restored: Self, before: &Self, after: &Self) -> Self;
}

/// The identifier of one transaction of a [`Transactions`] (REQ-CORE-005).
///
/// Opaque. Ids are issued in increasing order and never reused within one [`Transactions`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TxnId(u64);

/// An error from [`Transactions`] (REQ-CORE-005). A call that returns an error changes nothing:
/// neither the live state nor the open transactions.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TxnError {
    /// The id names no open transaction (never issued, or already committed or rolled back).
    UnknownTxn(TxnId),
    /// The id names an open transaction that is not the innermost one.
    NotInnermost {
        /// The id the caller passed.
        requested: TxnId,
        /// The innermost open transaction.
        innermost: TxnId,
    },
    /// `into_state` was called while transactions are still open.
    OpenTransactions {
        /// The number of open transactions.
        open: usize,
    },
}

impl fmt::Display for TxnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TxnError::UnknownTxn(id) => write!(f, "transaction {} is not open", id.0),
            TxnError::NotInnermost {
                requested,
                innermost,
            } => write!(
                f,
                "transaction {} is not the innermost open transaction ({} is)",
                requested.0, innermost.0
            ),
            TxnError::OpenTransactions { open } => {
                write!(f, "{open} transaction(s) still open")
            }
        }
    }
}

impl std::error::Error for TxnError {}

/// What a successful [`Transactions::rollback`] left behind (REQ-CORE-005).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollbackReport {
    /// The player who has priority after the rollback: the one who had it at the checkpoint (CR 734.2).
    pub priority_player: Option<PlayerId>,
    /// `state_hash()` of the state after the rollback.
    pub state_hash: Digest,
}

/// One durably committed sub-transaction: its checkpoint state and the state at its commit.
#[derive(Clone, Debug)]
struct DurableRecord<S> {
    before: S,
    after: S,
}

/// One open transaction.
#[derive(Clone, Debug)]
struct Frame<S> {
    id: TxnId,
    /// The live state at `checkpoint()` time.
    checkpoint: S,
    /// Durable records of descendants, in commit order.
    durable: Vec<DurableRecord<S>>,
}

/// A live state with a stack of open, nested transactions (REQ-CORE-005, CR 734).
///
/// All mutation of the live state goes through [`Transactions::state_mut`]. Only the innermost
/// open transaction can be committed or rolled back.
#[derive(Clone, Debug)]
pub struct Transactions<S: Transactional> {
    live: S,
    frames: Vec<Frame<S>>,
    /// The raw value of the next [`TxnId`] to issue.
    next_id: u64,
}

impl<S: Transactional> Transactions<S> {
    /// Wraps `state` with no open transaction (depth 0).
    #[must_use]
    pub fn new(state: S) -> Self {
        Transactions {
            live: state,
            frames: Vec::new(),
            next_id: 1,
        }
    }

    /// The live state.
    #[must_use]
    pub fn state(&self) -> &S {
        &self.live
    }

    /// Mutable access to the live state; all mutation goes through here.
    pub fn state_mut(&mut self) -> &mut S {
        &mut self.live
    }

    /// The number of open transactions.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Begins a (sub-)transaction: takes a snapshot (one clone) of the live state and nests it
    /// inside the current innermost open transaction, if any.
    ///
    /// CR 734.1: the engine takes this checkpoint before a player begins to cast a spell,
    /// activate an ability or take a special action.
    pub fn checkpoint(&mut self) -> TxnId {
        let id = TxnId(self.next_id);
        // Ids increase strictly; 2^64 checkpoints are out of reach, so the saturation never
        // happens in practice and nothing here can panic.
        self.next_id = self.next_id.saturating_add(1);
        self.frames.push(Frame {
            id,
            checkpoint: self.live.clone(),
            durable: Vec::new(),
        });
        id
    }

    /// Commits the innermost transaction `id`: the live state is kept, and the changes stay
    /// subject to the rollback of any ancestor. Durable records of its descendants move to the
    /// parent, if any.
    ///
    /// # Errors
    /// [`TxnError::UnknownTxn`] if `id` is not open, [`TxnError::NotInnermost`] if it is open
    /// but not the innermost; nothing changes then.
    pub fn commit(&mut self, id: TxnId) -> Result<(), TxnError> {
        let frame = self.pop_innermost(id)?;
        self.hand_to_parent(frame.durable);
        Ok(())
    }

    /// Commits the innermost transaction `id` durably: the live state is kept and the change
    /// from its checkpoint to now survives the rollback of its parent and of every ancestor.
    /// With no parent this is exactly [`Transactions::commit`].
    ///
    /// CR 734.1: mana abilities that are not reversed with the action are committed this way,
    /// as their own sub-transactions.
    ///
    /// # Errors
    /// [`TxnError::UnknownTxn`] if `id` is not open, [`TxnError::NotInnermost`] if it is open
    /// but not the innermost; nothing changes then.
    pub fn commit_durable(&mut self, id: TxnId) -> Result<(), TxnError> {
        let frame = self.pop_innermost(id)?;
        let Frame {
            checkpoint,
            mut durable,
            ..
        } = frame;
        if !self.frames.is_empty() {
            durable.push(DurableRecord {
                before: checkpoint,
                after: self.live.clone(),
            });
        }
        self.hand_to_parent(durable);
        Ok(())
    }

    /// Rolls back the innermost transaction `id`: the live state becomes its checkpoint state
    /// exactly, then the effects of durable sub-transactions committed beneath it are carried
    /// over, in commit order, with [`Transactional::carry_durable`]. The records then move to the
    /// parent, if any, so that a rollback of an outer ancestor carries them again.
    ///
    /// CR 734.1: the action is reversed by restoring the checkpoint. CR 734.2: the player who
    /// had priority at the checkpoint has it again.
    ///
    /// # Errors
    /// [`TxnError::UnknownTxn`] if `id` is not open, [`TxnError::NotInnermost`] if it is open
    /// but not the innermost; nothing changes then.
    pub fn rollback(&mut self, id: TxnId) -> Result<RollbackReport, TxnError> {
        let frame = self.pop_innermost(id)?;
        // CR 734.1: restore the checkpoint state exactly.
        let mut restored = frame.checkpoint;
        // CR 734.1: durably committed sub-transactions are not reversed with the action.
        for record in &frame.durable {
            restored = S::carry_durable(restored, &record.before, &record.after);
        }
        self.live = restored;
        self.hand_to_parent(frame.durable);
        // CR 734.2: the priority player is the checkpoint's (carry_durable keeps it).
        Ok(RollbackReport {
            priority_player: self.live.priority_player(),
            state_hash: self.live.state_hash(),
        })
    }

    /// The live state, consuming the wrapper.
    ///
    /// # Errors
    /// [`TxnError::OpenTransactions`] if any transaction is still open.
    pub fn into_state(self) -> Result<S, TxnError> {
        if self.frames.is_empty() {
            Ok(self.live)
        } else {
            Err(TxnError::OpenTransactions {
                open: self.frames.len(),
            })
        }
    }

    /// Pops the innermost frame if it is `id`; otherwise an error and nothing changes.
    fn pop_innermost(&mut self, id: TxnId) -> Result<Frame<S>, TxnError> {
        match self.frames.last() {
            Some(top) if top.id == id => {}
            Some(top) if self.frames.iter().any(|f| f.id == id) => {
                return Err(TxnError::NotInnermost {
                    requested: id,
                    innermost: top.id,
                });
            }
            _ => return Err(TxnError::UnknownTxn(id)),
        }
        self.frames.pop().ok_or(TxnError::UnknownTxn(id))
    }

    /// Appends `records` (in order) to the innermost remaining frame; dropped at the root.
    fn hand_to_parent(&mut self, records: Vec<DurableRecord<S>>) {
        if let Some(parent) = self.frames.last_mut() {
            parent.durable.extend(records);
        }
    }
}
