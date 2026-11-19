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
//! [`Transactions::commit_durable`] survives the rollback of its parent and of every ancestor
//! (CR 734.1: the mana abilities that are not reversed with the action). Each open transaction
//! keeps an ordered, non-overlapping list of `(before, after)` steps that account for the live
//! state's history since its checkpoint: steps tagged durable (changes that must survive its
//! rollback) and steps tagged own (its own changes, which its rollback reverses but which a
//! durable commit of it keeps). After the rollback of an ancestor the state is that ancestor's
//! checkpoint with every durable effect beneath it applied exactly once, in time order, each
//! step on its own through [`Transactional::carry_durable`]. Steps are never merged into one
//! net pair, because a carry need not be linear (a counter clamps at zero), so the order of the
//! individual steps matters.
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
/// Ids are issued in increasing order and never reused by the [`Transactions`] that issued
/// them. An id is valid only on the [`Transactions`] instance that issued it: keep each handle
/// with its instance. Passing a handle issued by another instance is a caller error whose
/// result is unspecified: it may be [`TxnError::UnknownTxn`] or [`TxnError::NotInnermost`], or,
/// if its number equals that of an open transaction of this instance, it is treated as that
/// transaction. (Ids carry no instance identity: that would need global mutable state, which
/// determinism forbids.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TxnId(u64);

/// An error from [`Transactions`] (REQ-CORE-005).
///
/// A [`Transactions::commit`], [`Transactions::commit_durable`] or [`Transactions::rollback`]
/// call that returns an error changes nothing: neither the live state nor the open
/// transactions. This guarantee does not cover [`Transactions::into_state`], which consumes the
/// wrapper: on [`TxnError::OpenTransactions`] the live state and the open transactions are
/// dropped.
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
    /// `into_state` was called while transactions are still open. The wrapper, with its live
    /// state and open transactions, has been consumed and dropped.
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

/// One step of a frame's history: the live state went from `before` to `after`. `durable`
/// steps survive the frame's rollback (they are carried with [`Transactional::carry_durable`]);
/// own steps (`durable == false`) are changes the frame's rollback reverses, kept only so that a
/// durable commit of the frame can make them durable.
#[derive(Clone, Debug)]
struct Entry<S> {
    before: S,
    after: S,
    durable: bool,
}

/// Where a frame's `entries` stop accounting for the live state's history.
#[derive(Clone, Debug)]
enum Cursor<S> {
    /// At the frame's checkpoint (no entry yet accounts for anything).
    Checkpoint,
    /// At the `after` state of the frame's last entry.
    LastAfter,
    /// At this state (the live state just after a child's rollback).
    State(S),
}

/// One open transaction.
#[derive(Clone, Debug)]
struct Frame<S> {
    id: TxnId,
    /// The live state at `checkpoint()` time.
    checkpoint: S,
    /// Steps in time order, non-overlapping: applying them in order is the effect of the
    /// frame's window from its checkpoint up to `cursor`. Filled only when a child finishes
    /// with durable content; empty otherwise.
    entries: Vec<Entry<S>>,
    /// The live state up to which `entries` account for the history. From there to the
    /// current live state the history is continuous (no rollback in between).
    cursor: Cursor<S>,
}

impl<S: Clone> Frame<S> {
    /// The state at the frame's cursor, as an owned value; the cursor is left at
    /// [`Cursor::Checkpoint`] and the caller sets it again.
    fn take_cursor(&mut self) -> S {
        match core::mem::replace(&mut self.cursor, Cursor::Checkpoint) {
            Cursor::Checkpoint => self.checkpoint.clone(),
            Cursor::LastAfter => self
                .entries
                .last()
                .map_or_else(|| self.checkpoint.clone(), |e| e.after.clone()),
            Cursor::State(s) => s,
        }
    }
}

/// A live state with a stack of open, nested transactions (REQ-CORE-005, CR 734).
///
/// All mutation of the live state goes through [`Transactions::state_mut`]. Only the innermost
/// open transaction can be committed or rolled back. A [`TxnId`] is valid only on the instance
/// that issued it; using a foreign handle is a caller error with an unspecified result (see
/// [`TxnId`]).
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
            entries: Vec::new(),
            cursor: Cursor::Checkpoint,
        });
        id
    }

    /// Commits the innermost transaction `id`: the live state is kept, and the changes stay
    /// subject to the rollback of any ancestor. If durable effects lie beneath it, its steps
    /// move to the parent (if any) in time order, framed by own steps for the changes before
    /// its checkpoint and after its last step, so the parent's history stays complete.
    ///
    /// # Errors
    /// [`TxnError::UnknownTxn`] if `id` is not open, [`TxnError::NotInnermost`] if it is open
    /// but not the innermost; nothing changes then.
    pub fn commit(&mut self, id: TxnId) -> Result<(), TxnError> {
        let mut frame = self.pop_innermost(id)?;
        if frame.entries.is_empty() {
            // Nothing durable beneath: the live state is continuous from the parent's cursor.
            return Ok(());
        }
        let tail = frame.take_cursor();
        let Frame {
            checkpoint,
            entries,
            ..
        } = frame;
        let live = self.live.clone();
        if let Some(parent) = self.frames.last_mut() {
            let head = parent.take_cursor();
            parent.entries.push(Entry {
                before: head,
                after: checkpoint,
                durable: false,
            });
            parent.entries.extend(entries);
            parent.entries.push(Entry {
                before: tail,
                after: live,
                durable: false,
            });
            parent.cursor = Cursor::LastAfter;
        }
        Ok(())
    }

    /// Commits the innermost transaction `id` durably: the live state is kept and every change
    /// from its checkpoint to now survives the rollback of its parent and of every ancestor.
    /// With a parent, the parent receives, in time order: an own step for its changes before
    /// this checkpoint, then every step of this transaction made durable, then a durable step
    /// from this transaction's cursor to the live state now. Each change is thus carried
    /// exactly once, step by step. With no parent this is exactly [`Transactions::commit`].
    ///
    /// CR 734.1: mana abilities that are not reversed with the action are committed this way,
    /// as their own sub-transactions.
    ///
    /// # Errors
    /// [`TxnError::UnknownTxn`] if `id` is not open, [`TxnError::NotInnermost`] if it is open
    /// but not the innermost; nothing changes then.
    pub fn commit_durable(&mut self, id: TxnId) -> Result<(), TxnError> {
        let mut frame = self.pop_innermost(id)?;
        if self.frames.is_empty() {
            return Ok(());
        }
        // CR 734.1: the whole window (checkpoint -> now) is durable, kept as its individual
        // steps in time order; never merged into one net pair, since a carry need not be linear.
        let tail = frame.take_cursor();
        let Frame {
            checkpoint,
            entries,
            ..
        } = frame;
        let live = self.live.clone();
        if let Some(parent) = self.frames.last_mut() {
            let head = parent.take_cursor();
            parent.entries.push(Entry {
                before: head,
                after: checkpoint,
                durable: false,
            });
            parent
                .entries
                .extend(entries.into_iter().map(|e| Entry { durable: true, ..e }));
            parent.entries.push(Entry {
                before: tail,
                after: live,
                durable: true,
            });
            parent.cursor = Cursor::LastAfter;
        }
        Ok(())
    }

    /// Rolls back the innermost transaction `id`: the live state becomes its checkpoint state
    /// exactly, then its durable steps (the durable effects beneath it) are carried over, in
    /// time order, with [`Transactional::carry_durable`], each exactly once; its own steps are
    /// not. If it held durable steps they then move to the parent, if any, after an own step
    /// for the parent's changes before this checkpoint, so that a rollback of an outer ancestor
    /// carries them again (over that ancestor's checkpoint, again exactly once).
    ///
    /// CR 734.1: the action is reversed by restoring the checkpoint. CR 734.2: the player who
    /// had priority at the checkpoint has it again.
    ///
    /// # Errors
    /// [`TxnError::UnknownTxn`] if `id` is not open, [`TxnError::NotInnermost`] if it is open
    /// but not the innermost; nothing changes then.
    pub fn rollback(&mut self, id: TxnId) -> Result<RollbackReport, TxnError> {
        let frame = self.pop_innermost(id)?;
        let Frame {
            checkpoint,
            entries,
            ..
        } = frame;
        let durable: Vec<Entry<S>> = entries.into_iter().filter(|e| e.durable).collect();
        // CR 734.1: restore the checkpoint state exactly. The checkpoint is kept only when
        // durable steps must move to a parent.
        let forward = !durable.is_empty() && !self.frames.is_empty();
        let (mut restored, kept) = if forward {
            (checkpoint.clone(), Some(checkpoint))
        } else {
            (checkpoint, None)
        };
        // CR 734.1: durably committed sub-transactions are not reversed with the action.
        for e in &durable {
            restored = S::carry_durable(restored, &e.before, &e.after);
        }
        self.live = restored;
        if let Some(checkpoint) = kept {
            let live = self.live.clone();
            if let Some(parent) = self.frames.last_mut() {
                let head = parent.take_cursor();
                parent.entries.push(Entry {
                    before: head,
                    after: checkpoint,
                    durable: false,
                });
                parent.entries.extend(durable);
                parent.cursor = Cursor::State(live);
            }
        }
        // CR 734.2: the priority player is the checkpoint's (carry_durable keeps it).
        Ok(RollbackReport {
            priority_player: self.live.priority_player(),
            state_hash: self.live.state_hash(),
        })
    }

    /// The live state, consuming the wrapper.
    ///
    /// Close every open transaction first: this call is not covered by the no-change-on-error
    /// guarantee of [`TxnError`].
    ///
    /// # Errors
    /// [`TxnError::OpenTransactions`] if any transaction is still open. The wrapper has been
    /// consumed then: the live state and the open transactions are dropped.
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
}
