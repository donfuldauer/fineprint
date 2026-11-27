// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Casting a spell and activating an ability, steps 0-5 of the SRS 7.1 table: checkpoint,
//! announce, choices, targets, division and the legality check of the fully specified proposal
//! (REQ-CAST-001, REQ-NET-003, REQ-CORE-005, REQ-DSL-013; CR 601.2, 601.2a-601.2e, 601.3,
//! 602.2, 734).
//!
//! [`announce`] runs the steps as one deterministic, suspendable procedure (a
//! [`ChoiceSession`](crate::choice::ChoiceSession) procedure) written against the narrow,
//! card-agnostic host seam [`CastState`]. The ward and becomes-target triggers are not part of
//! this module.
//!
//! Steps 7-9 (CR 601.2g-601.2i), the mana-ability window, paying the locked total cost in the
//! CR 601.2h order with the payment-method hook, and the commit (spell history, cast triggers
//! queued), are [`pay_total_cost`], written against the host seam [`PayState`]. It runs inside
//! the caller's whole-cast checkpoint and commits library-touching mana abilities and cost items
//! as durable sub-transactions that survive the rollback of an unpayable cost (CR 734.1).
//!
//! Step 6, determining and locking in the total cost (CR 601.2f), is the pure calculator
//! [`determine_total_cost`]: from a [`CostBasis`] (the mana cost or the chosen alternative cost),
//! it adds the additional costs and the `COST_INCREASE` entries, applies the `COST_REDUCTION`
//! entries in the player's order ([`ReductionStep`]) component by component (CR 118.7a-g) with
//! the mana component floored at `{0}`, then the `COST_MINIMUM` and `COST_SET` entries in
//! [`CostStages::total_effects`](crate::layers::CostStages::total_effects) order, and returns an
//! immutable [`LockedCost`]. An unpayable cost stays unpayable (CR 118.6a). Its parameter
//! conventions are documented on the `cost` module and in `docs/DECISIONS.md` ("#53 FP-051").
//!
//! # Steps
//!
//! 1. **Begin check** (CR 601.3, 601.3a): before anything changes, every applicable
//!    `PROHIBITION` on the action (`"CAST"` for a spell, `"ACTIVATE"` for an ability) refuses the
//!    action with [`AnnounceOutcome::NotStarted`], unless the host says a choice of the proposal
//!    could make it stop applying ([`CastState::prohibition_could_lapse`]). The permissions
//!    (timing, zone of origin) are not re-checked: the legal-action generator offered the action.
//! 2. **Step 0** (CR 734): a checkpoint is taken with [`Transactions`](crate::txn::Transactions).
//! 3. **Announce** (CR 601.2a, 602.2a): [`CastState::put_on_stack`]. Layout choices (split
//!    half, face, cast mode) are already in [`CastKind`] (CR 709.3, 712.8b, 716.3a, 719.3a,
//!    721.3a, 708.4).
//! 4. **Choices** (CR 601.2b, 700.2a, 700.2d): `CHOOSE_MODES` (a mode is offered when some
//!    cost configuration gives it its targets, or when probing every configuration would exceed
//!    the per-mode budget, CR-012, CR-013), then `CHOOSE_COSTS_AND_X`; when the spell has an
//!    alternative or additional cost, a second `CHOOSE_COSTS_AND_X` is sent only when
//!    [`CastState::symbol_slots`] returns slots for the costs selected in the first, and asks
//!    those hybrid and Phyrexian symbols (CR 118.9; CR-011). A selection whose costs have no such
//!    symbol (for example a symbol-free alternative cost) gets one request only, so replay and UI
//!    callers must not expect a fixed two-request sequence.
//! 5. **Targets** (CR 601.2c, 115.2-115.6): `CHOOSE_TARGETS`.
//! 6. **Division** (CR 601.2d): `DIVIDE_VALUE`.
//! 7. **Legality** (CR 601.2e): every applicable prohibition is checked against the fully
//!    specified proposal, with no 601.3a leniency, then the cross-slot combination.
//!
//! Any illegal step rolls the whole action back (CR 734.1): the state hash after the rollback
//! equals the hash before the call and the priority player is kept (CR 734.2). Moves of cards to
//! a library, moves from a library to a zone other than the stack, library shuffles and library
//! reveals are not reversible (CR 734.1) and are the host's concern; the move of a spell cast
//! from a library to the stack (CR 601.2a) is reversible, and a failed cast returns the spell to
//! the library it came from (CR 734.1).
//!
//! # Prohibition parameter conventions
//!
//! The `"action"`, `"zone"` and `"object"` parameters of `PROHIBITION` entries follow the
//! [`actions`](crate::actions) conventions: `"object"` (unsigned JSON integer) must be the raw id
//! of [`CastRequest::object`] (for an ability, its source). [`CastState`] reports no zone, so an
//! entry with a `"zone"` parameter matches nothing here.
//!
//! # `TARGETING_RULE` parameter convention
//!
//! The params of a `TARGETING_RULE` [`RuleEntry`](crate::layers::RuleEntry) are opaque,
//! untrusted JSON of any shape (R12). A wrong type or an unknown value makes the entry match
//! nothing. The entry's `players` set is not used for scope, except for `IGNORE_HEXPROOF`.
//!
//! - `"keyword"` (string, required): `"HEXPROOF"` (CR 702.11), `"SHROUD"` (CR 702.18),
//!   `"PROTECTION"` (CR 702.16) or `"OTHER"` ("can't be the target of").
//! - `"effect"` (string, optional, default `"PROHIBIT"`): `"PROHIBIT"`, or `"IGNORE_HEXPROOF"`,
//!   a permission to target "as though it didn't have hexproof" (CR 702.11e).
//! - `"target_object"` / `"target_player"` (unsigned JSON integer, exactly one present): the
//!   raw id of the protected object or player. Neither or both: the entry matches nothing.
//! - `"sources"` (string, optional, default `"ANY"`): `"OPPONENTS"` applies only when the
//!   caster is an opponent of the target's controller (hexproof, CR 702.11b-c); `"ANY"` applies
//!   whoever the caster is (shroud, protection, CR 702.18a, 702.16b).
//! - `"quality"` (string, optional): applies only when the spell, or the ability's source, has
//!   the quality ([`CastState::source_has_quality`]; CR 702.11d, 702.16a-b). Protection from
//!   everything omits it (CR 702.16j).
//! - `"source_player"` (unsigned JSON integer, optional): applies only when that player is the
//!   caster (protection from a player, CR 702.16k).
//!
//! An applicable `PROHIBIT` entry makes the candidate an illegal target (CR 101.2: "can't"
//! beats "can"). An applicable `IGNORE_HEXPROOF` entry (one that also modifies the rules for the
//! caster, [`RuleEntry::applies_to`](crate::layers::RuleEntry::applies_to)) suppresses only the
//! `PROHIBIT` entries whose keyword is `"HEXPROOF"` (including hexproof from a quality); shroud,
//! protection and `"OTHER"` still apply (CR 702.11e, 101.2). The result does not depend on the
//! order of the entries.
//!
//! # Determinism
//!
//! Only `Vec`, `BTreeMap` and `BTreeSet`; target candidates and modes are in ascending order;
//! request ids come from [`ChoiceContext::next_request_id`](crate::choice::ChoiceContext::next_request_id)
//! in ask order (modes, costs, targets, division; a step that needs no ask is skipped) (R3).

#![deny(missing_docs)]

mod announce;
mod cost;
mod pay;

#[cfg(test)]
mod announce_tests;

pub use crate::mana::PaymentMethodOffer;
pub use announce::{
    AnnounceOutcome, CastKind, CastRequest, CastState, CastStep, CostChoice, CostSpec, ModeSpec,
    Proposal, RolledBack, StopReason, TargetSlotSpec, announce,
};
pub use cost::{
    CostBasis, CostError, LockedCost, ManaComponent, ManaPart, ReductionStep, SymbolChoice,
    determine_total_cost,
};
pub use pay::{
    ManaActivation, PayError, PayOutcome, PayRolledBack, PayState, PayStopReason, pay_total_cost,
};
