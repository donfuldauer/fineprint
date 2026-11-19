// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Errors of the choice protocol (REQ-NET-003, REQ-DSL-011).

use core::fmt;

use super::RequestId;

/// An error of request generation, answer validation or the session driver (REQ-NET-003).
///
/// A [`ChoiceSession`](super::ChoiceSession) call that returns an error changes nothing: not the
/// status, not the accepted answers, not the hash.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChoiceError {
    /// The answer does not satisfy the pending request.
    Invalid {
        /// The request the answer was submitted for.
        request_id: RequestId,
        /// A non-empty explanation naming the violated constraint.
        explanation: String,
    },
    /// The answer names a request that is not the pending one.
    StaleRequest {
        /// The id the caller submitted.
        submitted: RequestId,
        /// The pending request's id (`None` when nothing is pending).
        pending: Option<RequestId>,
    },
    /// An answer was submitted after the procedure finished.
    NotPending,
    /// Re-execution reached the answered ask number `index` (0-based) with a request whose
    /// digest differs from the one the answer was accepted for, or the procedure did not reach
    /// that ask at all.
    Divergence {
        /// The position of the recorded answer that no longer fits.
        index: usize,
    },
    /// The procedure failed or misused the protocol (for example it ignored a suspension).
    Procedure(String),
    /// The choice spec is malformed (its own validation failed, or the resolved candidates have
    /// the wrong kind or shape).
    InvalidSpec(String),
    /// The choice kind needs resolved candidates (or a resolved division) and none were given,
    /// or it needs none and some were given.
    MissingCandidates,
    /// No answer can satisfy the resolved bounds.
    Unsatisfiable(String),
    /// No default answer exists (a `CARD_NAME` choice without a provided default).
    NoDefault,
    /// The provided default answer fails validation.
    InvalidDefault(String),
    /// [`ChoiceContext::ask`](super::ChoiceContext::ask) got a request whose id is not
    /// [`ChoiceContext::next_request_id`](super::ChoiceContext::next_request_id).
    RequestIdMismatch {
        /// The id the request must carry.
        expected: RequestId,
        /// The id it carried.
        got: RequestId,
    },
}

impl fmt::Display for ChoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChoiceError::Invalid {
                request_id,
                explanation,
            } => write!(f, "answer to request {request_id} rejected: {explanation}"),
            ChoiceError::StaleRequest { submitted, pending } => match pending {
                Some(p) => write!(f, "request {submitted} is not pending (request {p} is)"),
                None => write!(f, "request {submitted} is not pending (nothing is)"),
            },
            ChoiceError::NotPending => f.write_str("no request is pending: the procedure is done"),
            ChoiceError::Divergence { index } => write!(
                f,
                "re-execution diverged at recorded answer {index}: the procedure asked something else"
            ),
            ChoiceError::Procedure(m) => write!(f, "procedure error: {m}"),
            ChoiceError::InvalidSpec(m) => write!(f, "invalid choice spec: {m}"),
            ChoiceError::MissingCandidates => f.write_str(
                "the resolved candidates do not match what the choice kind needs (missing or extra)",
            ),
            ChoiceError::Unsatisfiable(m) => write!(f, "unsatisfiable choice: {m}"),
            ChoiceError::NoDefault => f.write_str("the choice has no default answer"),
            ChoiceError::InvalidDefault(m) => write!(f, "invalid default answer: {m}"),
            ChoiceError::RequestIdMismatch { expected, got } => {
                write!(f, "request id {got} asked where {expected} was expected")
            }
        }
    }
}

impl std::error::Error for ChoiceError {}
