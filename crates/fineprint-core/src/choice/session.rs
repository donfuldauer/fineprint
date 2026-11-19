// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The suspend/resume driver (REQ-NET-003, REQ-CORE-001): deterministic re-execution of a
//! synchronous procedure from a base checkpoint with the recorded answer list (docs/DECISIONS.md,
//! "#28 FP-027 — Suspension design: deterministic re-execution with a recorded answer list").

use core::fmt;

use fineprint_base::hash::Digest;

use super::{ChoiceError, ChoiceResponse, PendingChoiceRequest, RequestId};
use crate::txn::Transactional;

/// Why a procedure stopped before producing its output (REQ-NET-003).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Interrupt {
    /// A player must decide: the request returned by [`ChoiceContext::ask`]. Propagate it with
    /// `?` unchanged.
    Pending(Box<PendingChoiceRequest>),
    /// The procedure failed.
    Error(ChoiceError),
}

impl From<ChoiceError> for Interrupt {
    fn from(e: ChoiceError) -> Interrupt {
        Interrupt::Error(e)
    }
}

impl fmt::Display for Interrupt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Interrupt::Pending(r) => write!(
                f,
                "suspended on request {} ({})",
                r.request_id, r.choice_type
            ),
            Interrupt::Error(e) => write!(f, "{e}"),
        }
    }
}

/// The ask channel of one procedure run (REQ-NET-003).
///
/// It replays the recorded answers in order and suspends at the first unanswered ask. A
/// procedure gets it from a [`ChoiceSession`]; it holds no global state (R3).
#[derive(Debug)]
pub struct ChoiceContext<'a> {
    answers: &'a [ChoiceResponse],
    digests: &'a [Digest],
    /// The number of asks made so far in this run.
    position: usize,
    /// The digest of the request this run suspended on, once it has.
    suspended: Option<Digest>,
    /// A protocol misuse seen in this run (reported even if the procedure swallows it).
    misuse: Option<ChoiceError>,
}

impl<'a> ChoiceContext<'a> {
    fn new(answers: &'a [ChoiceResponse], digests: &'a [Digest]) -> ChoiceContext<'a> {
        ChoiceContext {
            answers,
            digests,
            position: 0,
            suspended: None,
            misuse: None,
        }
    }
}

impl ChoiceContext<'_> {
    /// The id the next [`ChoiceContext::ask`] must carry: the 0-based position of that ask in
    /// this run's ask sequence (REQ-NET-003).
    #[must_use]
    pub fn next_request_id(&self) -> RequestId {
        RequestId::new(u64::try_from(self.position).unwrap_or(u64::MAX))
    }

    /// Asks a player to decide (REQ-NET-003).
    ///
    /// If this ask already has a recorded answer, returns it (after checking that `request`
    /// equals the request that answer was accepted for); otherwise returns
    /// [`Interrupt::Pending`], which the procedure must propagate with `?`.
    ///
    /// # Errors
    /// [`Interrupt::Pending`] (suspend here), or [`Interrupt::Error`] with
    /// [`ChoiceError::RequestIdMismatch`] (the id is not [`ChoiceContext::next_request_id`]),
    /// [`ChoiceError::Divergence`] (the request differs from the recorded one) or
    /// [`ChoiceError::Procedure`] (asking again after a suspension).
    pub fn ask(&mut self, request: PendingChoiceRequest) -> Result<ChoiceResponse, Interrupt> {
        if self.suspended.is_some() {
            let e = ChoiceError::Procedure(
                "ask called after the run suspended: propagate Interrupt::Pending".to_owned(),
            );
            self.misuse.get_or_insert_with(|| e.clone());
            return Err(Interrupt::Error(e));
        }
        let expected = self.next_request_id();
        if request.request_id != expected {
            let e = ChoiceError::RequestIdMismatch {
                expected,
                got: request.request_id,
            };
            self.misuse.get_or_insert_with(|| e.clone());
            return Err(Interrupt::Error(e));
        }
        let index = self.position;
        match (self.answers.get(index), self.digests.get(index)) {
            (Some(answer), Some(digest)) => {
                if request.digest() != *digest {
                    let e = ChoiceError::Divergence { index };
                    self.misuse.get_or_insert_with(|| e.clone());
                    return Err(Interrupt::Error(e));
                }
                self.position += 1;
                Ok(answer.clone())
            }
            _ => {
                self.suspended = Some(request.digest());
                Err(Interrupt::Pending(Box::new(request)))
            }
        }
    }
}

/// Where a [`ChoiceSession`] stands (REQ-NET-003).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "the variant layout is the pinned public API (FP-027); values are short-lived"
)]
pub enum Status<S, O> {
    /// Waiting for an answer to this request. The visible state is the base state.
    Pending(PendingChoiceRequest),
    /// The procedure finished.
    Done {
        /// The final state.
        state: S,
        /// The procedure's output.
        output: O,
    },
}

/// A suspendable run of one procedure over a base state (REQ-NET-003, REQ-CORE-001).
///
/// The session keeps the base state, the accepted answers and, per answer, the digest of the
/// request it answered. Every accepted answer re-runs the procedure from a fresh clone of the
/// base with all answers, so the same base and the same answers always give the same final
/// state and hash, whether the answers came one by one through [`ChoiceSession::submit`] or all
/// at once through [`ChoiceSession::replay`]. A call that returns an error changes nothing.
pub struct ChoiceSession<S, O, F>
where
    S: Transactional,
    F: Fn(&mut S, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    base: S,
    procedure: F,
    answers: Vec<ChoiceResponse>,
    digests: Vec<Digest>,
    status: Status<S, O>,
}

impl<S, O, F> fmt::Debug for ChoiceSession<S, O, F>
where
    S: Transactional + fmt::Debug,
    O: fmt::Debug,
    F: Fn(&mut S, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChoiceSession")
            .field("base", &self.base)
            .field("answers", &self.answers)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

/// One run of `procedure` from a fresh clone of `base` with `answers`.
fn run<S, O, F>(
    base: &S,
    procedure: &F,
    answers: &[ChoiceResponse],
    digests: &[Digest],
) -> Result<Status<S, O>, ChoiceError>
where
    S: Transactional,
    F: Fn(&mut S, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    let mut state = base.clone();
    let mut ctx = ChoiceContext::new(answers, digests);
    let result = procedure(&mut state, &mut ctx);
    if let Some(e) = ctx.misuse {
        return Err(e);
    }
    match result {
        Ok(output) => {
            if ctx.suspended.is_some() {
                return Err(ChoiceError::Procedure(
                    "the procedure finished after a suspension: propagate Interrupt::Pending"
                        .to_owned(),
                ));
            }
            if ctx.position < answers.len() {
                // A recorded answer was never asked for again.
                return Err(ChoiceError::Divergence {
                    index: ctx.position,
                });
            }
            Ok(Status::Done { state, output })
        }
        Err(Interrupt::Pending(request)) => {
            if ctx.suspended != Some(request.digest()) {
                return Err(ChoiceError::Procedure(
                    "the procedure returned a pending request that ask did not return".to_owned(),
                ));
            }
            Ok(Status::Pending(*request))
        }
        Err(Interrupt::Error(e)) => Err(e),
    }
}

impl<S, O, F> ChoiceSession<S, O, F>
where
    S: Transactional,
    F: Fn(&mut S, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    /// Runs `procedure` on a clone of `base` with no answers (REQ-NET-003).
    ///
    /// # Errors
    /// Any error of the run ([`ChoiceError::Procedure`], [`ChoiceError::RequestIdMismatch`], or
    /// an error the procedure returned).
    pub fn start(base: S, procedure: F) -> Result<Self, ChoiceError> {
        let status = run(&base, &procedure, &[], &[])?;
        Ok(ChoiceSession {
            base,
            procedure,
            answers: Vec::new(),
            digests: Vec::new(),
            status,
        })
    }

    /// Starts a session and submits `answers` in order, each to the request pending at that
    /// point (REQ-NET-003). Equivalent to [`ChoiceSession::start`] followed by one
    /// [`ChoiceSession::submit`] per answer.
    ///
    /// # Errors
    /// The first error: of the start, of an answer ([`ChoiceError::Invalid`]), or
    /// [`ChoiceError::NotPending`] if answers remain after the procedure finished.
    pub fn replay(base: S, procedure: F, answers: &[ChoiceResponse]) -> Result<Self, ChoiceError> {
        let mut session = ChoiceSession::start(base, procedure)?;
        for answer in answers {
            let id = session
                .pending()
                .map(|r| r.request_id)
                .ok_or(ChoiceError::NotPending)?;
            session.submit(id, answer.clone())?;
        }
        Ok(session)
    }

    /// The current status.
    #[must_use]
    pub fn status(&self) -> &Status<S, O> {
        &self.status
    }

    /// The pending request, if the procedure is suspended.
    #[must_use]
    pub fn pending(&self) -> Option<&PendingChoiceRequest> {
        match &self.status {
            Status::Pending(r) => Some(r),
            Status::Done { .. } => None,
        }
    }

    /// Answers the pending request `id` with `response` and resumes (REQ-NET-003). Read the
    /// outcome through [`ChoiceSession::status`].
    ///
    /// # Errors
    /// [`ChoiceError::NotPending`] (the procedure is done), [`ChoiceError::StaleRequest`]
    /// (`id` is not the pending request), [`ChoiceError::Invalid`] (the answer fails
    /// [`PendingChoiceRequest::validate`]), or an error of the re-run (for example
    /// [`ChoiceError::Divergence`]). On any error the session is unchanged.
    pub fn submit(&mut self, id: RequestId, response: ChoiceResponse) -> Result<(), ChoiceError> {
        let pending = match &self.status {
            Status::Pending(r) => r,
            Status::Done { .. } => return Err(ChoiceError::NotPending),
        };
        if pending.request_id != id {
            return Err(ChoiceError::StaleRequest {
                submitted: id,
                pending: Some(pending.request_id),
            });
        }
        pending
            .validate(&response)
            .map_err(|explanation| ChoiceError::Invalid {
                request_id: id,
                explanation,
            })?;
        let mut answers = self.answers.clone();
        let mut digests = self.digests.clone();
        answers.push(response);
        digests.push(pending.digest());
        let status = run(&self.base, &self.procedure, &answers, &digests)?;
        self.answers = answers;
        self.digests = digests;
        self.status = status;
        Ok(())
    }

    /// Answers the pending request `id` with its `default_on_timeout` (REQ-NET-003): exactly
    /// `submit(id, pending.default_on_timeout.clone())`. The engine never reads a clock; calling
    /// this is how the caller applies a timeout (R3, R7).
    ///
    /// # Errors
    /// As [`ChoiceSession::submit`].
    pub fn submit_default(&mut self, id: RequestId) -> Result<(), ChoiceError> {
        let default = match &self.status {
            Status::Pending(r) => r.default_on_timeout.clone(),
            Status::Done { .. } => return Err(ChoiceError::NotPending),
        };
        self.submit(id, default)
    }

    /// The accepted answers, in order.
    #[must_use]
    pub fn answers(&self) -> &[ChoiceResponse] {
        &self.answers
    }

    /// The base state the procedure runs from (the visible state while pending).
    #[must_use]
    pub fn base(&self) -> &S {
        &self.base
    }

    /// When done, the final state's hash; while pending, the base state's hash.
    #[must_use]
    pub fn state_hash(&self) -> Digest {
        match &self.status {
            Status::Done { state, .. } => state.state_hash(),
            Status::Pending(_) => self.base.state_hash(),
        }
    }
}
