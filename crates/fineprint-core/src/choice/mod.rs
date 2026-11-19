// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The choice protocol (REQ-NET-003, REQ-DSL-011): pending choice requests, responses, their
//! generation from [`ChoiceSpec`](fineprint_dsl::choice::ChoiceSpec)s, answer validation and the
//! deterministic suspend/resume driver.
//!
//! # Suspension design
//!
//! Synchronous rules code stops when a player must decide and resumes later by **deterministic
//! re-execution from a base checkpoint with a recorded answer list** (docs/DECISIONS.md, "#28
//! FP-027 — Suspension design: deterministic re-execution with a recorded answer list"). A rules
//! procedure is a plain function `Fn(&mut S, &mut ChoiceContext<'_>) -> Result<O, Interrupt>`
//! that calls [`ChoiceContext::ask`](crate::choice::ChoiceContext::ask) with `?`. A [`ChoiceSession`](crate::choice::ChoiceSession) keeps the base state (one
//! cheap clone) and the ordered list of accepted answers; every accepted answer re-runs the
//! procedure from a fresh clone of the base with the whole list. Whatever the procedure changed
//! before it suspended is discarded with that clone, so while a request is pending the visible
//! state is exactly the base.
//!
//! Consequences for procedures:
//!
//! - a procedure must be a deterministic function of the state and the answers (R3): it reads
//!   and writes nothing outside `S`, and draws randomness only from the state's PRNG (each run
//!   starts from the base PRNG, so draws replay identically);
//! - every decision goes through [`ChoiceContext::ask`](crate::choice::ChoiceContext::ask), with the request id
//!   [`ChoiceContext::next_request_id`](crate::choice::ChoiceContext::next_request_id);
//! - a procedure with `k` asks costs `O(k^2)` procedure steps over a whole session (each answer
//!   re-runs the prefix); `k` is small for every rules procedure.
//!
//! If re-execution reaches an answered ask whose request differs from the one the answer was
//! accepted for (compared by [`PendingChoiceRequest::digest`](crate::choice::PendingChoiceRequest::digest)), the session reports
//! [`ChoiceError::Divergence`](crate::choice::ChoiceError::Divergence); it never silently applies an answer to another question.
//!
//! # Option sets
//!
//! A request lists pre-validated options ([`ChoiceOptions::Enumerated`](crate::choice::ChoiceOptions::Enumerated)) when its number of
//! distinct valid answers is below [`ENUMERATION_LIMIT`](crate::choice::ENUMERATION_LIMIT), and a [`ValidatorContract`](crate::choice::ValidatorContract) otherwise
//! (SRS 15.3). `CHOOSE_CARD_NAME` always carries a validator contract.
//!
//! # Time
//!
//! The engine never reads a clock (R3, R7): a timeout is the caller deciding to call
//! [`ChoiceSession::submit_default`](crate::choice::ChoiceSession::submit_default).

#![deny(missing_docs)]

mod error;
mod generate;
mod request;
mod response;
mod session;

#[cfg(test)]
mod tests;

use core::fmt;

use fineprint_base::encode::{CanonicalEncode, put_variant};
use fineprint_base::ids::{ObjectId, PlayerId};

pub use error::ChoiceError;
pub use generate::{
    Generated, ResolvedCandidates, ResolvedChoice, ResolvedDivision, generate_request,
};
pub use request::{
    AllowedValues, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason, OrderDestination,
    PendingChoiceRequest, TargetSlot, ValidatorContract, ValidatorRule,
};
pub use response::{ChoiceResponse, ChoiceValue};
pub use session::{ChoiceContext, ChoiceSession, Interrupt, Status};

/// The option-count threshold of SRS 15.3 (REQ-NET-003): a request whose number of distinct
/// valid answers is below this lists them as [`ChoiceOptions::Enumerated`]; at or above it the
/// request carries a [`ChoiceOptions::Validator`] contract.
pub const ENUMERATION_LIMIT: u64 = 10_000;

/// The longest card name, in UTF-8 bytes, a `CHOOSE_CARD_NAME` answer may have (structural
/// check only; see [`ValidatorRule::CardName`]).
pub const MAX_CARD_NAME_BYTES: u64 = 256;

/// The identifier of one request inside one procedure run (REQ-NET-003).
///
/// It is the 0-based position of the ask in the procedure's ask sequence, so it is
/// deterministic and needs no global counter (R3): the first ask is `RequestId::new(0)`, the
/// next `RequestId::new(1)`, and so on. Ids are meaningful only within one [`ChoiceSession`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(u64);

impl RequestId {
    /// Wraps a raw position.
    #[must_use]
    pub const fn new(raw: u64) -> RequestId {
        RequestId(raw)
    }

    /// Returns the raw position.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl CanonicalEncode for RequestId {
    /// The `u64` as 8 bytes little-endian.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.0.encode_canonical(out);
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

macro_rules! choice_types {
    ($( $(#[$doc:meta])* $variant:ident => $wire:literal = $tag:literal ),+ $(,)?) => {
        /// The SRS 15.3 choice taxonomy (REQ-NET-003): one variant per table row, in table
        /// order. The canonical encoding is one byte, the 0-based row index (shown per variant).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum ChoiceType {
            $( $(#[$doc])* $variant, )+
        }

        impl ChoiceType {
            /// Every choice type, in SRS 15.3 table order.
            pub const ALL: [ChoiceType; 20] = [ $( ChoiceType::$variant ),+ ];

            /// The exact SRS 15.3 name (`PRIORITY_ACTION`, ...).
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( ChoiceType::$variant => $wire, )+
                }
            }

            /// The choice type whose SRS name is exactly `text` (case-sensitive), if any.
            #[must_use]
            pub fn from_wire(text: &str) -> Option<ChoiceType> {
                ChoiceType::ALL.into_iter().find(|t| t.as_str() == text)
            }

            /// The one-byte canonical tag (the table row index).
            #[must_use]
            pub const fn tag(self) -> u8 {
                match self {
                    $( ChoiceType::$variant => $tag, )+
                }
            }
        }
    };
}

choice_types! {
    /// `PRIORITY_ACTION` (tag 0): the heartbeat's priority decision.
    PriorityAction => "PRIORITY_ACTION" = 0,
    /// `CHOOSE_MODES` (tag 1): CR 601.2b, trigger insertion.
    ChooseModes => "CHOOSE_MODES" = 1,
    /// `CHOOSE_COSTS_AND_X` (tag 2): CR 601.2b.
    ChooseCostsAndX => "CHOOSE_COSTS_AND_X" = 2,
    /// `CHOOSE_TARGETS` (tag 3): CR 601.2c, trigger insertion, CR 707.10c new targets.
    ChooseTargets => "CHOOSE_TARGETS" = 3,
    /// `DIVIDE_VALUE` (tag 4): CR 601.2d, CR 510.1c, CR 510.1d.
    DivideValue => "DIVIDE_VALUE" = 4,
    /// `PAY_MANA_AND_COSTS` (tag 5): CR 601.2g, CR 601.2h, attack and block costs, ward, "unless".
    PayManaAndCosts => "PAY_MANA_AND_COSTS" = 5,
    /// `DECLARE_ATTACKERS` (tag 6): CR 508.1.
    DeclareAttackers => "DECLARE_ATTACKERS" = 6,
    /// `DECLARE_BLOCKERS` (tag 7): CR 509.1.
    DeclareBlockers => "DECLARE_BLOCKERS" = 7,
    /// `ORDER_SIMULTANEOUS_TRIGGERS` (tag 8): CR 603.3b.
    OrderSimultaneousTriggers => "ORDER_SIMULTANEOUS_TRIGGERS" = 8,
    /// `CHOOSE_OPTIONAL_TRIGGER` (tag 9): "you may" triggers and "may" instructions.
    ChooseOptionalTrigger => "CHOOSE_OPTIONAL_TRIGGER" = 9,
    /// `CHOOSE_REPLACEMENT_EFFECT` (tag 10): CR 616.1.
    ChooseReplacementEffect => "CHOOSE_REPLACEMENT_EFFECT" = 10,
    /// `SELECT_OBJECTS_FROM_ZONE` (tag 11): tutor, discard, sacrifice, legend rule, mulligan bottom.
    SelectObjectsFromZone => "SELECT_OBJECTS_FROM_ZONE" = 11,
    /// `ORDER_CARDS` (tag 12): scry, surveil, "in any order", graveyard order (CR 731.3a).
    OrderCards => "ORDER_CARDS" = 12,
    /// `CHOOSE_PLAYER` (tag 13): "choose an opponent", battle protector, CR 800.4g reassignment.
    ChoosePlayer => "CHOOSE_PLAYER" = 13,
    /// `CHOOSE_OPTION_OR_VALUE` (tag 14): color, creature type, number, sector, vote, anchor word.
    ChooseOptionOrValue => "CHOOSE_OPTION_OR_VALUE" = 14,
    /// `CHOOSE_CARD_NAME` (tag 15): "choose a card name" (CR 712.16, CR 723.5).
    ChooseCardName => "CHOOSE_CARD_NAME" = 15,
    /// `CHOOSE_UNTAP_SET` (tag 16): CR 502.3, when an effect allows choosing.
    ChooseUntapSet => "CHOOSE_UNTAP_SET" = 16,
    /// `SHORTCUT_RESPONSE` (tag 17): CR 733.2b.
    ShortcutResponse => "SHORTCUT_RESPONSE" = 17,
    /// `MULLIGAN_DECISION` (tag 18): CR 103.5.
    MulliganDecision => "MULLIGAN_DECISION" = 18,
    /// `PREGAME_ACTION` (tag 19): CR 103.6.
    PregameAction => "PREGAME_ACTION" = 19,
}

impl CanonicalEncode for ChoiceType {
    /// One byte: [`ChoiceType::tag`].
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_variant(out, self.tag());
    }
}

impl fmt::Display for ChoiceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A player or an object that receives something: a division share, an attack, a target
/// (REQ-NET-003). Players order before objects; each kind orders by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChoiceTarget {
    /// A player (canonical tag 0, then the id).
    Player(PlayerId),
    /// An object (canonical tag 1, then the id).
    Object(ObjectId),
}

impl CanonicalEncode for ChoiceTarget {
    /// Tag `0` + player id, or tag `1` + object id.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            ChoiceTarget::Player(p) => {
                put_variant(out, 0);
                p.encode_canonical(out);
            }
            ChoiceTarget::Object(o) => {
                put_variant(out, 1);
                o.encode_canonical(out);
            }
        }
    }
}

impl fmt::Display for ChoiceTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChoiceTarget::Player(p) => write!(f, "player {}", p.get()),
            ChoiceTarget::Object(o) => write!(f, "object {}", o.get()),
        }
    }
}
