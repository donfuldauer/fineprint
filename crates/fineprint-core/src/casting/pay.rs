// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Steps 7-9 of casting a spell: the mana-ability window, paying the locked total cost and the
//! commit (REQ-CAST-001, REQ-CORE-005; CR 601.2g-601.2i, 605, 118.3, 734).
//!
//! # The procedure
//!
//! [`pay_total_cost`] is a deterministic, suspendable procedure written against the narrow,
//! card-agnostic host seam [`PayState`] (R4). The caller owns the checkpoint of the whole cast
//! (taken before announce, CR 734) and passes its [`TxnId`]; it must be the innermost open
//! transaction. The procedure always closes it: committed when the spell becomes cast, rolled
//! back otherwise (`announce` commits its own checkpoint on success, so the whole-cast
//! checkpoint has to come from the caller; see `docs/DECISIONS.md`, "#54 FP-052").
//!
//! 1. **Step 7, mana-ability window** (CR 601.2g, 605.3a, 117.1d; contract CR-015): if the
//!    locked mana part is payable and not `{0}`, the player is asked `PAY_MANA_AND_COSTS` one or
//!    more times: each request lists the mana abilities that can be activated now, the
//!    payment-method offers and whether an auto-pay plan exists, all computed on a state in which
//!    the activations of the earlier answers of this window have been applied. An answer in which
//!    at least one listed ability is actually activated (available at that moment, not already
//!    in progress, accepted by the host; tried in the listed order, CR 605.3c) is followed by a
//!    new request; its payment methods and its auto-pay flag are discarded. The window ends with
//!    the first answer that is auto-pay, lists no ability, or lists abilities none of which was
//!    activated; that answer's payment methods are used. At most 256 requests are asked: the
//!    256th lists no ability, so it ends the window. `CANCEL_AND_ROLLBACK` reverses the cast (CR 734.1). An unpayable mana part
//!    (CR 118.6) makes the attempt illegal without an ask. Nothing in the real state changes
//!    while the window is open: the rounds are simulated on a scratch copy of the state, and
//!    when the window has ended the concatenation of every listed ability is activated for real,
//!    in order, without the stack (CR 605.3b, 605.4a, 405.6c), exactly as if one answer had
//!    listed them all. Activating mana abilities is optional (CR 118.3c): an activation that is
//!    not available at its turn, or that the host refuses, is simply not activated.
//! 2. **Step 8, payment** (CR 601.2h): the chosen payment methods are applied to the mana still
//!    to pay (they never change the locked cost, CR 702.51b, 702.126b). Then the first pass pays
//!    every cost that is not late ([`PayState::cost_is_late`] false): the mana from the pool
//!    ([`plan_payment`], [`spend`], CR 118.3a), the Phyrexian life (CR 118.3b) and the other
//!    items in the locked order; then the second pass pays the late items in the locked order.
//!    Partial payment is never allowed: the mana plan is computed and the life paid (an atomic
//!    host call) before any mana is spent, and any failure rolls the whole cast back.
//! 3. **Step 9, commit** (CR 601.2i): the spell history is recorded ([`PayState::record_cast`]),
//!    the cast triggers go to the waiting queue ([`PayState::queue_cast_triggers`]; they are put
//!    on the stack the next time a player would receive priority, CR 603.3), and the cast
//!    checkpoint is committed.
//!
//! # Reversal and durable sub-transactions (CR 734.1)
//!
//! Each mana-ability activation and each non-mana cost item is paid inside its own nested
//! checkpoint. If the host reports it as library-touching ([`ManaActivation::library_touching`]:
//! it, or a triggered mana ability it caused, moved cards to a library, moved cards from a
//! library to a zone other than the stack, shuffled a library or revealed library cards), the
//! nested checkpoint is committed durably and survives the rollback of the cast (CR 734.1, last
//! sentence). Otherwise it is committed normally and is reversed with the cast: CR 734.1 lets
//! each player reverse the legal mana abilities they activated, and the engine's default is to
//! reverse them (SRS 2.5).
//!
//! **Dependent activations.** CR 734.1 forbids reversing an activation whose mana (or the mana of
//! the triggered mana abilities it caused) was spent on another mana ability that is not
//! reversed. Before the real activations, the procedure runs them once on a scratch copy of the
//! state and follows each unit of the paying player's pool: when a durable activation consumed a
//! unit produced by an earlier activation, the earlier activation is committed durably too
//! (transitively). The probe covers the whole concatenated list of the window, so a dependency
//! across two answers of the window is followed too. Which pool units an activation removed comes
//! from the host ([`PayState::last_activation_removed`], contract CR-016); a host that does not
//! report it (or reports an invalid list) gets the documented inference fallback: the removed
//! units are the multiset difference of the pool before and after, each credited to the earliest
//! equal unit (the lowest-index tie-break of [`plan_payment`]). Units equal in every field cannot
//! be told apart otherwise, so the fallback is sound only for the hosts described on
//! [`PayState::activate_mana_ability`].
//!
//! **No re-activation before resolution** (CR 605.3c). A mana ability is offered in a request
//! only if it is in [`PayState::available_mana_abilities`] on the window's state at that request,
//! and it is activated only if it is available at the moment of its activation (in the
//! simulation and again on the real state). The procedure keeps the set of activations it has
//! begun and not yet finished: beginning one that is already in progress is refused with
//! [`PayError::Illegal`]. Activations resolve immediately (CR 605.3b), so the set holds at most
//! one activation at a time.
//!
//! # Spell-history conventions
//!
//! `costs_paid` gets, after the entries of the caller's base history and in payment order:
//! `"method.<label>"` for each payment method used, `"mana"` for the mana part, `"life"` for
//! Phyrexian life, then each non-mana item's DSL node name in lowercase (for example
//! `"tap_source"`). `mana_spent` adds one per spent unit under the key `(type, source kind)`:
//! the type is `"white"`, `"blue"`, `"black"`, `"red"`, `"green"` or `"colorless"`
//! ([`ManaType::as_str`](crate::mana::ManaType::as_str)), the source kind `"snow"` or
//! `"nonsnow"` ([`ManaUnit::source_kind`]).
//!
//! # Determinism and replay
//!
//! Only `Vec`, `BTreeMap` and `BTreeSet`; each request id comes from
//! [`ChoiceContext::next_request_id`]; nothing in the real state changes before the window has
//! ended (its rounds run on a scratch copy), so a suspended run leaves the transaction stack and
//! the state untouched and a [`ChoiceSession`](crate::choice::ChoiceSession) replay of the same
//! answers reaches the same final state (R3).

use std::collections::BTreeSet;

use fineprint_base::hash::Digest;
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_dsl::ability::CostAST;

use super::cost::{LockedCost, ManaComponent, ManaPart};
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOptions, ChoicePayload, ChoiceReason, ChoiceResponse,
    ChoiceType, Interrupt, PendingChoiceRequest, ValidatorContract, ValidatorRule,
};
use crate::mana::{
    ManaPool, ManaRider, ManaUnit, PaymentMethodOffer, SpendPurpose, plan_payment, spend,
};
use crate::memory::SpellHistory;
use crate::txn::{Transactions, TxnError, TxnId};

/// Why a host call of the payment procedure failed (REQ-CAST-001).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PayError {
    /// The cost cannot be paid (CR 118.3).
    Unpayable,
    /// The action is not allowed now (for example a mana ability activated again before it
    /// resolved, CR 605.3c).
    Illegal(String),
}

/// What one mana-ability activation or one paid cost item did (REQ-CAST-001, CR 734.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ManaActivation {
    /// Whether it, or a triggered mana ability it caused, moved cards to a library, moved cards
    /// from a library to a zone other than the stack, shuffled a library or revealed library
    /// cards: such an action is not reversed with the cast (CR 734.1).
    pub library_touching: bool,
}

/// The host seam of the payment procedure (REQ-CAST-001, REQ-DSL-013): the state facts and
/// operations only the game state knows. It is card-agnostic: the procedure never branches on a
/// card id or name (R4).
pub trait PayState: crate::txn::Transactional {
    /// `player`'s mana pool (CR 106.4).
    fn mana_pool(&self, player: PlayerId) -> &ManaPool;

    /// `player`'s mana pool, mutably.
    fn mana_pool_mut(&mut self, player: PlayerId) -> &mut ManaPool;

    /// The mana abilities `player` can activate now, as `(source, ability id)`, ascending
    /// (CR 605.3a).
    fn available_mana_abilities(&self, player: PlayerId) -> Vec<(ObjectId, String)>;

    /// Activates a mana ability (CR 605.3): pays its cost, adds its mana to the pool and
    /// immediately resolves every triggered mana ability it caused (CR 605.3b, 605.4a, 405.6c).
    ///
    /// **Host requirement (CR 734.1, contract CR-016).** The procedure must know which pool
    /// units an activation removed, to keep an activation whose mana a durable activation spent.
    /// Without [`PayState::last_activation_removed`] it infers the removal from the pool before
    /// and after the call: the removed units are the multiset difference, each credited to the
    /// earliest equal unit, the survivors keep their order and the added units come after them.
    /// That inference is sound exactly when (1) the host removes, among equal units, the
    /// lowest-index one (the tie-break of [`plan_payment`]) and (2) the activation does not both
    /// remove a unit and add an equal one. A host that removes, among equal units, other than the
    /// lowest-index one, or whose activation both removes a unit and adds an equal one, MUST
    /// override [`PayState::last_activation_removed`].
    ///
    /// # Errors
    /// [`PayError`] if it cannot be activated; the state must then be unchanged (the procedure
    /// rolls its nested checkpoint back anyway).
    fn activate_mana_ability(
        &mut self,
        player: PlayerId,
        source: ObjectId,
        ability_id: &str,
    ) -> Result<ManaActivation, PayError>;

    /// Which units of `player`'s mana pool the last successful
    /// [`activate_mana_ability`](PayState::activate_mana_ability) call removed (CR 734.1,
    /// REQ-CORE-005, contract CR-016): its cost payment and the triggered mana abilities it
    /// caused, together, as indices into the pool as it was immediately BEFORE the call, strictly
    /// ascending, without duplicates. The units that were not removed keep their relative order
    /// and the units the call added come after them, in the order added. A unit the call removed
    /// and an equal unit the call added are reported as removed and added respectively (the pool
    /// may be identical before and after).
    ///
    /// The value is transient bookkeeping of the last call: it must not enter the state hash or
    /// any canonical encoding, it must survive `Clone`, and the procedure reads it only directly
    /// after a successful call on the same value (the scratch copy of the dependent-activation
    /// probe). `None` (the default) means the host does not report; the procedure then infers the
    /// removal as described on [`activate_mana_ability`](PayState::activate_mana_ability). A
    /// report is used only when it is valid (every index below the pool length before the call,
    /// strictly ascending, and no more units surviving than the pool holds after the call);
    /// otherwise the inference is used.
    fn last_activation_removed(&self, _player: PlayerId) -> Option<Vec<usize>> {
        None
    }

    /// Whether `item` involves random elements or moves objects from the library to a public
    /// zone, so it is paid in the second pass of CR 601.2h.
    fn cost_is_late(&self, player: PlayerId, item: &CostAST) -> bool;

    /// Pays one non-mana cost item of the spell `stack_object` (CR 601.2h).
    ///
    /// # Errors
    /// [`PayError::Unpayable`] if it cannot be paid (CR 118.3); the state must then be
    /// unchanged (the procedure rolls its nested checkpoint back anyway).
    fn pay_cost_item(
        &mut self,
        player: PlayerId,
        stack_object: ObjectId,
        item: &CostAST,
    ) -> Result<ManaActivation, PayError>;

    /// Pays `amount` life (CR 118.3b, the Phyrexian life of the locked cost).
    ///
    /// # Errors
    /// [`PayError`] if the player cannot pay it; the state must then be unchanged.
    fn pay_life(&mut self, player: PlayerId, amount: u64) -> Result<(), PayError>;

    /// The payment-method offers for the spell (the payment-method modifier hook, SRS 7.1 step
    /// 8). Empty when there is none.
    fn payment_methods(
        &self,
        player: PlayerId,
        stack_object: ObjectId,
        cost: &LockedCost,
    ) -> Vec<PaymentMethodOffer>;

    /// Applies a payment method with the chosen `objects` (tapping or exiling them) and returns
    /// the mana still to pay. The procedure clamps the result to `remaining`, component-wise.
    ///
    /// # Errors
    /// [`PayError`] if the method cannot be used so; the cast is then rolled back.
    fn apply_payment_method(
        &mut self,
        player: PlayerId,
        stack_object: ObjectId,
        offer: &PaymentMethodOffer,
        objects: &[ObjectId],
        remaining: &ManaComponent,
    ) -> Result<ManaComponent, PayError>;

    /// CR 601.2i: the spell becomes cast; records `history` in its Memory (SRS 2.6).
    fn record_cast(&mut self, player: PlayerId, stack_object: ObjectId, history: SpellHistory);

    /// CR 601.2i, 603.3: puts the abilities that trigger on the cast or on the spell being put
    /// onto the stack into the waiting queue (not onto the stack).
    fn queue_cast_triggers(&mut self, player: PlayerId, stack_object: ObjectId);

    /// The tags describing what the mana is spent on (CR 106.6).
    fn purpose(&self, player: PlayerId, stack_object: ObjectId) -> SpendPurpose;
}

/// Why the payment stopped and the cast was reversed (REQ-CAST-001).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PayStopReason {
    /// The locked cost's mana part is unpayable (`ManaPart::Unpayable`, CR 118.6). Only that
    /// case: attempting to pay it is illegal and nothing is asked.
    UnpayableCost,
    /// The cost could not be paid in full at step 8 (CR 601.2h, 118.3, 734.1): the pool, after
    /// the mana abilities and payment methods, can't pay the mana part; the Phyrexian life can't
    /// be paid (CR 118.3b); a non-mana item can't be paid; or a payment method the host refused
    /// could not be completed.
    UnpayableMana,
    /// The player cancelled the cast (`CANCEL_AND_ROLLBACK`, CR 734.1).
    Cancelled,
}

/// A cast reversed by the payment procedure (CR 734.1, 734.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayRolledBack {
    /// Why.
    pub reason: PayStopReason,
    /// The state hash after the rollback: the checkpoint's, with the durable sub-transactions
    /// carried over.
    pub state_hash: Digest,
    /// The player with priority after the rollback: the one at the checkpoint (CR 734.2).
    pub priority_player: Option<PlayerId>,
}

/// The result of [`pay_total_cost`] (REQ-CAST-001).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PayOutcome {
    /// The cost was paid and the spell became cast (CR 601.2i).
    Committed {
        /// The mana units spent, in pool order.
        spent: Vec<ManaUnit>,
        /// One rider list per spent unit, parallel to `spent` (CR 106.6, 106.6a); the host
        /// applies them.
        riders_fired: Vec<Vec<ManaRider>>,
    },
    /// The cost could not be paid; the cast was reversed (CR 601.2h, 734.1).
    RolledBack(PayRolledBack),
    /// The player cancelled; the cast was reversed (CR 734.1).
    Cancelled(PayRolledBack),
}

/// The CR 605.3c guard: the activations begun and not yet finished in this procedure run.
#[derive(Debug, Default)]
pub(crate) struct ActivationGuard {
    in_progress: BTreeSet<(ObjectId, String)>,
}

impl ActivationGuard {
    /// CR 605.3c: begins an activation; refused if the same ability is already in progress.
    pub(crate) fn begin(&mut self, source: ObjectId, ability_id: &str) -> Result<(), PayError> {
        if self.in_progress.insert((source, ability_id.to_owned())) {
            Ok(())
        } else {
            Err(PayError::Illegal(format!(
                "the mana ability {ability_id:?} of object {} is already being activated (CR 605.3c)",
                source.get()
            )))
        }
    }

    /// The activation resolved (or failed): it may be activated again.
    pub(crate) fn finish(&mut self, source: ObjectId, ability_id: &str) {
        self.in_progress.remove(&(source, ability_id.to_owned()));
    }
}

/// What the answer that ended the step-7 window chose: the payment methods, as
/// `(offer index, objects)`.
struct Answer {
    methods: Vec<(u32, Vec<ObjectId>)>,
}

/// The most `PAY_MANA_AND_COSTS` requests of one step-7 window (contract CR-015): the last one
/// lists no mana ability, so it ends the window.
const MAX_WINDOW_REQUESTS: usize = 256;

/// Runs steps 7-9 of casting a spell (REQ-CAST-001, REQ-CORE-005; CR 601.2g-601.2i, 605, 734):
/// the mana-ability window, the payment of `locked` and the commit, as described on the module.
///
/// `cast_checkpoint` must be the innermost open transaction of `txn` (the checkpoint the caller
/// took before announcing the spell `stack_object`). On return with `Ok`, it is closed:
/// committed for [`PayOutcome::Committed`], rolled back for [`PayOutcome::RolledBack`] and
/// [`PayOutcome::Cancelled`] (the state hash is then the checkpoint's, with the durable
/// sub-transactions carried over, and the priority player is kept, CR 734.1, 734.2).
/// `base_history` is the spell history so far (additional costs, X, face); the payment adds the
/// costs paid and the mana spent.
///
/// Stop reasons: an unpayable mana part (CR 118.6) rolls back with
/// [`PayStopReason::UnpayableCost`]; every other failure to pay the cost in full at step 8
/// (mana, Phyrexian life, a non-mana item, or a refused payment method; CR 601.2h, 118.3, 734.1)
/// rolls the whole cast back with [`PayStopReason::UnpayableMana`].
///
/// # Errors
/// [`Interrupt::Pending`] when the player must decide (propagate it; nothing has changed), and
/// [`Interrupt::Error`] for a precondition or protocol error: `cast_checkpoint` is not the
/// innermost open transaction (nothing has changed), or an answer of the wrong shape.
#[allow(
    clippy::too_many_arguments,
    reason = "the pinned public API (REQ-CAST-001, issue #54)"
)]
pub fn pay_total_cost<S: PayState>(
    txn: &mut Transactions<S>,
    cast_checkpoint: TxnId,
    ctx: &mut ChoiceContext<'_>,
    player: PlayerId,
    stack_object: ObjectId,
    locked: &LockedCost,
    base_history: SpellHistory,
) -> Result<PayOutcome, Interrupt> {
    if !is_innermost(txn, cast_checkpoint) {
        return Err(procedure_error(
            "pay_total_cost: the cast checkpoint is not the innermost open transaction".to_owned(),
        ));
    }
    // CR 118.6: attempting to pay an unpayable cost is illegal.
    let mana = match locked.mana() {
        ManaPart::Unpayable => {
            return rolled_back(txn, cast_checkpoint, PayStopReason::UnpayableCost)
                .map(PayOutcome::RolledBack);
        }
        ManaPart::Payable(m) => *m,
    };

    // CR 601.2g, 605.3a: step 7, the window in which the player may activate mana abilities
    // (contract CR-015). Its rounds run on a scratch copy: nothing in the real state changes
    // before the window has ended.
    let mut offers = Vec::new();
    let mut methods = Vec::new();
    if !mana.is_zero() {
        let mut scratch = txn.state().clone();
        let mut listed: Vec<(ObjectId, String)> = Vec::new();
        for round in 1..=MAX_WINDOW_REQUESTS {
            offers = scratch.payment_methods(player, stack_object, locked);
            // The last request lists no ability, so it ends the window.
            let abilities = if round < MAX_WINDOW_REQUESTS {
                let mut a = scratch.available_mana_abilities(player);
                a.sort();
                a.dedup();
                a
            } else {
                Vec::new()
            };
            let purpose = scratch.purpose(player, stack_object);
            let auto_pay = plan_payment(scratch.mana_pool(player), &mana, &purpose).is_some();
            let request = pay_request(
                ctx,
                player,
                stack_object,
                locked_total(locked),
                &offers,
                abilities,
                auto_pay,
            );
            match ask_checked(ctx, request)? {
                ChoiceResponse::PayCosts { auto_pay: true, .. } => break,
                ChoiceResponse::PayCosts {
                    methods: chosen,
                    mana_abilities,
                    ..
                } => {
                    let ran = simulate_round(&mut scratch, player, &mana_abilities);
                    listed.extend(mana_abilities);
                    if !ran {
                        // The window ends with this answer's payment methods.
                        methods = chosen;
                        break;
                    }
                    // CR 605.3a: something was activated, so the player may activate more; this
                    // answer's methods are discarded.
                }
                // CR 734.1: the player cancels the cast; nothing has changed yet.
                ChoiceResponse::CancelAndRollback => {
                    return rolled_back(txn, cast_checkpoint, PayStopReason::Cancelled)
                        .map(PayOutcome::Cancelled);
                }
                other => {
                    return Err(procedure_error(format!(
                        "{} does not answer this PAY_MANA_AND_COSTS request",
                        other.wire_name()
                    )));
                }
            }
        }
        // CR 605.3b, 605.4a, 734.1: every listed ability, in order, each in its own nested
        // checkpoint, as if one answer had listed them all.
        activate_all(txn, player, &listed)?;
        // The real state now equals the scratch copy's final state (R3), so the offers match the
        // request that ended the window.
        offers = txn.state().payment_methods(player, stack_object, locked);
    }
    let answer = Answer { methods };

    // CR 601.2h, SRS 7.1 step 8: payment methods apply to the mana still to pay.
    let mut costs_paid: Vec<String> = Vec::new();
    let mut remaining = mana;
    for (index, objects) in &answer.methods {
        if objects.is_empty() {
            continue;
        }
        let Some(offer) = usize::try_from(*index).ok().and_then(|i| offers.get(i)) else {
            return Err(procedure_error(format!(
                "payment method {index} is not offered"
            )));
        };
        let result =
            txn.state_mut()
                .apply_payment_method(player, stack_object, offer, objects, &remaining);
        match result {
            Ok(after) => remaining = clamp(&after, &remaining),
            // CR 601.2h, 734.1: the chosen way of paying can't be completed; no partial payment.
            Err(_) => {
                return rolled_back(txn, cast_checkpoint, PayStopReason::UnpayableMana)
                    .map(PayOutcome::RolledBack);
            }
        }
        costs_paid.push(format!("method.{}", offer.label));
    }

    // CR 601.2h: which items are paid in the second pass, decided before paying anything.
    let (first, late): (Vec<&CostAST>, Vec<&CostAST>) = locked
        .other()
        .iter()
        .partition(|item| !txn.state().cost_is_late(player, item));

    // CR 601.2h, 118.3: no partial payment; the mana plan exists before anything is spent.
    let purpose = txn.state().purpose(player, stack_object);
    let Some(plan) = plan_payment(txn.state().mana_pool(player), &remaining, &purpose) else {
        return rolled_back(txn, cast_checkpoint, PayStopReason::UnpayableMana)
            .map(PayOutcome::RolledBack);
    };
    // CR 118.3b: the life is paid (atomically) before any mana is spent; 0 life needs nothing.
    let life = locked.phyrexian_life();
    if life > 0 && txn.state_mut().pay_life(player, life).is_err() {
        return rolled_back(txn, cast_checkpoint, PayStopReason::UnpayableMana)
            .map(PayOutcome::RolledBack);
    }
    // CR 118.3a: the planned units leave the pool; the rest stays (CR 106.4a).
    let spent = spend(txn.state_mut().mana_pool_mut(player), &plan);
    costs_paid.push("mana".to_owned());
    if life > 0 {
        costs_paid.push("life".to_owned());
    }
    // CR 601.2h: first the items without random elements or library-to-public-zone moves, then
    // the rest.
    for item in first.into_iter().chain(late) {
        if !pay_item(txn, player, stack_object, item)? {
            return rolled_back(txn, cast_checkpoint, PayStopReason::UnpayableMana)
                .map(PayOutcome::RolledBack);
        }
        costs_paid.push(item.name().to_ascii_lowercase());
    }

    // CR 601.2i: step 9, the spell becomes cast.
    let mut history = base_history;
    history.costs_paid.extend(costs_paid);
    for unit in &spent {
        let key = (
            unit.mana_type.as_str().to_owned(),
            unit.source_kind().to_owned(),
        );
        let n = history.mana_spent.entry(key).or_insert(0);
        *n = n.saturating_add(1);
    }
    let state = txn.state_mut();
    state.record_cast(player, stack_object, history);
    // CR 603.3: the cast triggers wait for the next time a player would receive priority.
    state.queue_cast_triggers(player, stack_object);
    txn.commit(cast_checkpoint).map_err(txn_error)?;
    let riders_fired = spent.iter().map(|u| u.riders.clone()).collect();
    Ok(PayOutcome::Committed {
        spent,
        riders_fired,
    })
}

/// Whether `id` is the innermost open transaction of `txn` (probed on a copy, so `txn` is not
/// touched).
fn is_innermost<S: PayState>(txn: &Transactions<S>, id: TxnId) -> bool {
    let mut probe = txn.clone();
    probe.commit(id).is_ok()
}

/// CR 734.1, 734.2: reverses the cast and reports the restored hash and priority player.
fn rolled_back<S: PayState>(
    txn: &mut Transactions<S>,
    cast_checkpoint: TxnId,
    reason: PayStopReason,
) -> Result<PayRolledBack, Interrupt> {
    let report = txn.rollback(cast_checkpoint).map_err(txn_error)?;
    Ok(PayRolledBack {
        reason,
        state_hash: report.state_hash,
        priority_player: report.priority_player,
    })
}

/// `after`, component-wise no more than `remaining` (a payment method never adds mana to pay).
fn clamp(after: &ManaComponent, remaining: &ManaComponent) -> ManaComponent {
    ManaComponent {
        generic: after.generic.min(remaining.generic),
        white: after.white.min(remaining.white),
        blue: after.blue.min(remaining.blue),
        black: after.black.min(remaining.black),
        red: after.red.min(remaining.red),
        green: after.green.min(remaining.green),
        colorless: after.colorless.min(remaining.colorless),
        snow: after.snow.min(remaining.snow),
    }
}

/// CR 601.2h, 734.1: pays one non-mana item inside its own nested checkpoint, committed durably
/// when it touched a library. `Ok(false)` when it can't be paid (the nested checkpoint is rolled
/// back).
fn pay_item<S: PayState>(
    txn: &mut Transactions<S>,
    player: PlayerId,
    stack_object: ObjectId,
    item: &CostAST,
) -> Result<bool, Interrupt> {
    let nested = txn.checkpoint();
    match txn.state_mut().pay_cost_item(player, stack_object, item) {
        Ok(done) => {
            close(txn, nested, done.library_touching)?;
            Ok(true)
        }
        Err(_) => {
            txn.rollback(nested).map_err(txn_error)?;
            Ok(false)
        }
    }
}

/// Commits a nested checkpoint, durably if `durable` (CR 734.1).
fn close<S: PayState>(
    txn: &mut Transactions<S>,
    nested: TxnId,
    durable: bool,
) -> Result<(), Interrupt> {
    if durable {
        txn.commit_durable(nested).map_err(txn_error)
    } else {
        txn.commit(nested).map_err(txn_error)
    }
}

/// What the scratch run learned about one listed activation.
#[derive(Clone, Copy, Debug, Default)]
struct Probe {
    /// Whether it was activated (available and accepted by the host).
    activated: bool,
    /// Whether it must survive the cast's rollback (CR 734.1).
    durable: bool,
}

/// Whether `(source, ability)` is in `player`'s available mana abilities of `state` now
/// (CR 605.3a).
fn is_available<S: PayState>(state: &S, player: PlayerId, source: ObjectId, ability: &str) -> bool {
    state
        .available_mana_abilities(player)
        .iter()
        .any(|(s, a)| *s == source && a == ability)
}

/// CR 601.2g, 605.3a-c: one round of the step-7 window on the scratch copy `scratch`, with the
/// rules [`activate_all`] applies on the real state: each listed ability, in order, is activated
/// only if it is available at that moment, not already in progress (CR 605.3c) and accepted by the
/// host; a refused activation leaves `scratch` as it was. Returns whether at least one ran.
fn simulate_round<S: PayState>(
    scratch: &mut S,
    player: PlayerId,
    list: &[(ObjectId, String)],
) -> bool {
    let mut guard = ActivationGuard::default();
    let mut ran = false;
    for (source, ability) in list {
        if !is_available(scratch, player, *source, ability) {
            continue;
        }
        if guard.begin(*source, ability).is_err() {
            continue;
        }
        let before = scratch.clone();
        match scratch.activate_mana_ability(player, *source, ability) {
            Ok(_) => ran = true,
            Err(_) => *scratch = before,
        }
        guard.finish(*source, ability);
    }
    ran
}

/// CR 734.1: runs the listed activations on a scratch copy of the state and decides which must
/// be durable: the library-touching ones and, transitively, every earlier activation whose mana
/// a durable activation consumed. The removed units come from
/// [`PayState::last_activation_removed`] when the host reports them (contract CR-016).
fn probe_activations<S: PayState>(
    state: &S,
    player: PlayerId,
    list: &[(ObjectId, String)],
) -> Vec<Probe> {
    let mut scratch = state.clone();
    let mut probes = vec![Probe::default(); list.len()];
    // The producer (list index) of each unit of the paying player's pool, parallel to it.
    let mut origin: Vec<Option<usize>> = vec![None; scratch.mana_pool(player).len()];
    // For each activation, the producers of the units it consumed.
    let mut consumed: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); list.len()];
    for (k, (source, ability)) in list.iter().enumerate() {
        if !is_available(&scratch, player, *source, ability) {
            continue;
        }
        let before = scratch.clone();
        match scratch.activate_mana_ability(player, *source, ability) {
            Ok(done) => {
                let removed = scratch.last_activation_removed(player);
                let (gone, next) = follow_units(
                    before.mana_pool(player).units(),
                    scratch.mana_pool(player).units(),
                    &origin,
                    removed.as_deref(),
                    k,
                );
                if let (Some(c), Some(p)) = (consumed.get_mut(k), probes.get_mut(k)) {
                    c.extend(gone);
                    p.activated = true;
                    p.durable = done.library_touching;
                }
                origin = next;
            }
            Err(_) => scratch = before,
        }
    }
    // Backwards: a durable activation makes the producers of the mana it consumed durable.
    for k in (0..list.len()).rev() {
        if probes.get(k).is_some_and(|p| p.durable) {
            let producers: Vec<usize> = consumed
                .get(k)
                .map(|c| c.iter().copied().collect())
                .unwrap_or_default();
            for i in producers {
                if let Some(p) = probes.get_mut(i) {
                    p.durable = true;
                }
            }
        }
    }
    probes
}

/// Whether `removed` is a usable report of the units an activation removed from a pool of
/// `before` units that holds `after` units afterwards (contract CR-016): strictly ascending,
/// every index below `before`, and no more survivors than `after`.
fn valid_removal(removed: &[usize], before: usize, after: usize) -> bool {
    removed.windows(2).all(|w| matches!(w, [a, b] if a < b))
        && removed.iter().all(|&i| i < before)
        && before.saturating_sub(removed.len()) <= after
}

/// The inference fallback of contract CR-016: for each distinct unit value, the
/// `count_before - count_after` (when positive) earliest equal units of `before` are removed
/// (the lowest-index tie-break of [`plan_payment`]). Returns one flag per `before` unit.
fn infer_removal(before: &[ManaUnit], after: &[ManaUnit]) -> Vec<bool> {
    let mut removed = vec![false; before.len()];
    let mut decided = vec![false; before.len()];
    for (i, unit) in before.iter().enumerate() {
        if decided.get(i).copied().unwrap_or(true) {
            continue;
        }
        let equal: Vec<usize> = (i..before.len())
            .filter(|&j| before.get(j) == Some(unit))
            .collect();
        let remaining = after.iter().filter(|u| *u == unit).count();
        let gone = equal.len().saturating_sub(remaining);
        for (n, &j) in equal.iter().enumerate() {
            if let Some(d) = decided.get_mut(j) {
                *d = true;
            }
            if n < gone
                && let Some(r) = removed.get_mut(j)
            {
                *r = true;
            }
        }
    }
    removed
}

/// CR 734.1: aligns the pool `before` an activation with the pool `after` it (contract CR-016).
/// The removed units are the host's report `removed` when it is valid, otherwise the inference
/// of [`infer_removal`]. The units not removed keep their order and pair one-to-one with the
/// first units of `after`; the rest of `after` is new and credited to activation `k`. Returns the
/// producers of the removed units and the producer list of `after`. Never panics.
fn follow_units(
    before: &[ManaUnit],
    after: &[ManaUnit],
    origin: &[Option<usize>],
    removed: Option<&[usize]>,
    k: usize,
) -> (Vec<usize>, Vec<Option<usize>>) {
    let taken = match removed.filter(|v| valid_removal(v, before.len(), after.len())) {
        Some(v) => {
            let mut flags = vec![false; before.len()];
            for &i in v {
                if let Some(f) = flags.get_mut(i) {
                    *f = true;
                }
            }
            flags
        }
        None => infer_removal(before, after),
    };
    let mut gone = Vec::new();
    let mut next = Vec::with_capacity(after.len());
    for (i, gone_now) in taken.iter().enumerate() {
        let from = origin.get(i).copied().flatten();
        if *gone_now {
            if let Some(p) = from {
                gone.push(p);
            }
        } else {
            next.push(from);
        }
    }
    // The survivors are the first units of `after`; the rest is new.
    next.truncate(after.len());
    next.resize(after.len(), Some(k));
    (gone, next)
}

/// CR 605.3a-c, 605.4a, 734.1: activates the listed mana abilities in order, each inside its own
/// nested checkpoint, committed durably when [`probe_activations`] says so. An ability that is
/// not available now, or that the host refuses, is not activated (CR 118.3c).
fn activate_all<S: PayState>(
    txn: &mut Transactions<S>,
    player: PlayerId,
    list: &[(ObjectId, String)],
) -> Result<(), Interrupt> {
    if list.is_empty() {
        return Ok(());
    }
    let probes = probe_activations(txn.state(), player, list);
    let mut guard = ActivationGuard::default();
    for ((source, ability), probe) in list.iter().zip(probes) {
        // CR 605.3a: only an ability that can be activated now.
        if !is_available(txn.state(), player, *source, ability) {
            continue;
        }
        // CR 605.3c: not again before it has resolved.
        if guard.begin(*source, ability).is_err() {
            continue;
        }
        let nested = txn.checkpoint();
        let result = txn
            .state_mut()
            .activate_mana_ability(player, *source, ability);
        guard.finish(*source, ability);
        match result {
            Ok(done) => close(txn, nested, done.library_touching || probe.durable)?,
            Err(_) => {
                txn.rollback(nested).map_err(txn_error)?;
            }
        }
    }
    Ok(())
}

/// The display form of the locked total: the mana cost, then `"N life"`, then the non-mana
/// items' DSL node names, separated by `", "`.
fn locked_total(locked: &LockedCost) -> String {
    let mut parts: Vec<String> = Vec::new();
    match locked.mana() {
        ManaPart::Payable(m) => parts.push(
            m.to_mana_cost()
                .map_or_else(|_| format!("{m:?}"), |c| c.to_string()),
        ),
        ManaPart::Unpayable => parts.push("unpayable".to_owned()),
    }
    if locked.phyrexian_life() > 0 {
        parts.push(format!("{} life", locked.phyrexian_life()));
    }
    parts.extend(locked.other().iter().map(|c| c.name().to_owned()));
    parts.join(", ")
}

/// The `PAY_MANA_AND_COSTS` request of step 7 (CR 601.2g). Not optional, cancellable; the
/// timeout default is the auto-pay suggestion when there is one, otherwise cancelling.
fn pay_request(
    ctx: &ChoiceContext<'_>,
    player: PlayerId,
    stack_object: ObjectId,
    locked_total: String,
    offers: &[PaymentMethodOffer],
    mana_abilities: Vec<(ObjectId, String)>,
    auto_pay: bool,
) -> PendingChoiceRequest {
    let default_on_timeout = if auto_pay {
        ChoiceResponse::PayCosts {
            auto_pay: true,
            methods: Vec::new(),
            mana_abilities: Vec::new(),
        }
    } else {
        ChoiceResponse::CancelAndRollback
    };
    PendingChoiceRequest {
        request_id: ctx.next_request_id(),
        chooser: player,
        choice_type: ChoiceType::PayManaAndCosts,
        reason: ChoiceReason {
            cr: "CR 601.2g".to_owned(),
            source: Some(stack_object),
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout,
        options: ChoiceOptions::Validator(ValidatorContract {
            min: 1,
            max: 1,
            rule: ValidatorRule::Payload,
        }),
        payload: ChoicePayload::PayManaAndCosts {
            locked_total,
            methods: offers
                .iter()
                .map(|o| (o.label.clone(), o.eligible.clone()))
                .collect(),
            mana_abilities,
            auto_pay,
            optional: false,
            cancellable: true,
        },
    }
}

/// Asks `request` and checks the answer against it again (R12: never trust an answer).
fn ask_checked(
    ctx: &mut ChoiceContext<'_>,
    request: PendingChoiceRequest,
) -> Result<ChoiceResponse, Interrupt> {
    let check = request.clone();
    let answer = ctx.ask(request)?;
    check.validate(&answer).map_err(|explanation| {
        Interrupt::Error(ChoiceError::Invalid {
            request_id: check.request_id,
            explanation,
        })
    })?;
    Ok(answer)
}

fn procedure_error(text: String) -> Interrupt {
    Interrupt::Error(ChoiceError::Procedure(text))
}

fn txn_error(e: TxnError) -> Interrupt {
    procedure_error(format!("transaction error: {e}"))
}

#[cfg(test)]
#[path = "../mana/pay_unit_tests.rs"]
mod tests;
