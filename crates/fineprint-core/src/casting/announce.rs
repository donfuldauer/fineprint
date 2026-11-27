// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The announce procedure: steps 0-5 of casting a spell or activating an ability (REQ-CAST-001;
//! CR 601.2, 601.2a-601.2e, 601.3, 602.2, 734).

use std::collections::BTreeSet;

use fineprint_base::hash::Digest;
use fineprint_base::ids::{EffectId, ObjectId, PlayerId};
use fineprint_dsl::ability::RuleKind;
use serde_json::Value;

use crate::actions::{ACTION_ACTIVATE, ACTION_CAST};
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason,
    ChoiceResponse, ChoiceTarget, ChoiceType, ENUMERATION_LIMIT, Interrupt, PendingChoiceRequest,
    TargetSlot, ValidatorContract, ValidatorRule,
};
use crate::layers::{RuleEntry, RuleSet};
use crate::txn::{Transactional, Transactions, TxnId};

/// What is being cast or activated, as named by the `INITIATE_CAST` or `ACTIVATE` response
/// (REQ-CAST-001).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CastKind {
    /// Casting a spell (CR 601.2). The face and the cast mode are the layout choices made before
    /// the card is put on the stack (CR 709.3, 712.8b, 716.3a, 719.3a, 721.3a, 708.4).
    Spell {
        /// Which face (0-based).
        face_index: u32,
        /// The cast mode (normal or an alternative cast) as listed in the priority request.
        cast_mode: String,
    },
    /// Activating an ability (CR 602.2).
    Ability {
        /// The ability's id as listed in the priority request.
        ability_id: String,
    },
}

/// The action a player proposes (REQ-CAST-001): the `INITIATE_CAST` / `ACTIVATE` response plus
/// the player who answered it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CastRequest {
    /// The player casting the spell or activating the ability.
    pub caster: PlayerId,
    /// The card to cast, or the source of the ability.
    pub object: ObjectId,
    /// Spell or ability, with its layout choices.
    pub kind: CastKind,
}

/// The modes of a modal spell or ability (CR 601.2b, 700.2), in printed order.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ModeSpec {
    /// One display label per mode.
    pub labels: Vec<String>,
    /// The least number of modes to choose.
    pub min: u32,
    /// The greatest number of modes to choose.
    pub max: u32,
    /// Whether the same mode may be chosen more than once (CR 700.2d).
    pub repeats_allowed: bool,
    /// Per-mode cost labels (spree, escalate, tiered); empty when the modes have no cost.
    pub mode_costs: Vec<String>,
}

/// The cost options announced in CR 601.2b, as display labels and ranges.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CostSpec {
    /// The alternative costs; at most one may be chosen.
    pub alternative_costs: Vec<String>,
    /// The additional costs; any distinct subset may be chosen.
    pub additional_costs: Vec<String>,
    /// The allowed values of X (inclusive), if the cost has an X (CR 107.3).
    pub x_range: Option<(u64, u64)>,
    /// One option list per hybrid or Phyrexian symbol (CR 601.2b).
    pub symbol_slots: Vec<Vec<String>>,
    /// The creatures that may be tapped for harmonize.
    pub harmonize_candidates: Vec<ObjectId>,
}

impl CostSpec {
    /// Whether the spec offers anything to choose.
    fn offers_choice(&self) -> bool {
        !self.alternative_costs.is_empty()
            || !self.additional_costs.is_empty()
            || self.x_range.is_some()
            || !self.symbol_slots.is_empty()
            || !self.harmonize_candidates.is_empty()
    }
}

/// The cost choices made in CR 601.2b (the `CONFIGURE_COSTS` answer); indices refer to the
/// [`CostSpec`] lists.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CostChoice {
    /// The chosen alternative cost, or `None` for the normal cost.
    pub alternative: Option<u32>,
    /// The chosen additional costs (distinct indices).
    pub additional: Vec<u32>,
    /// The value of X, if the cost has an X.
    pub x: Option<u64>,
    /// One chosen option per symbol slot, in slot order.
    pub symbol_options: Vec<u32>,
    /// The creature chosen for harmonize, if any.
    pub harmonize: Option<ObjectId>,
}

/// One instance of the word "target" (CR 601.2c, 115.3): its bounds and the candidates that
/// match its target predicate (ascending, before prohibitions).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TargetSlotSpec {
    /// The least number of targets (0: the slot may stay empty, CR 115.6).
    pub min: u64,
    /// The greatest number of targets.
    pub max: u64,
    /// The candidates matching the target predicate, before targeting prohibitions.
    pub candidates: Vec<ChoiceTarget>,
}

/// The fully specified proposal of steps 2-4 (CR 601.2b-601.2d).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Proposal {
    /// The chosen modes, as ORIGINAL indices into [`ModeSpec::labels`], ascending; empty when
    /// the spell or ability is not modal.
    pub modes: Vec<u32>,
    /// The cost choices.
    pub costs: CostChoice,
    /// One list of chosen targets per target slot, in slot order.
    pub targets: Vec<Vec<ChoiceTarget>>,
    /// The division among the targets, in recipient order, if something is divided.
    pub division: Option<Vec<(ChoiceTarget, u64)>>,
}

/// The host seam of the announce procedure (REQ-CAST-001, REQ-DSL-013): the state facts and
/// query points only the game state knows. It is card-agnostic: the procedure never branches on
/// a card id or name (R4).
pub trait CastState: Transactional {
    /// The rule set of pass B (CR 613.11).
    fn rule_set(&self) -> &RuleSet;

    /// Whether `a` and `b` are opponents.
    fn are_opponents(&self, a: PlayerId, b: PlayerId) -> bool;

    /// The controller of a target candidate; for a player target, that player.
    fn target_controller(&self, t: ChoiceTarget) -> PlayerId;

    /// The modes of the spell or ability; `None` when it is not modal (CR 601.2b, 700.2).
    fn mode_spec(&self, req: &CastRequest) -> Option<ModeSpec>;

    /// The cost options once `modes` (original indices) are chosen (CR 601.2b).
    fn cost_spec(&self, req: &CastRequest, modes: &[u32]) -> CostSpec;

    /// The target slots once `modes` and `costs` are chosen (CR 601.2c, 700.2c): one entry per
    /// instance of the word "target".
    fn target_slots(
        &self,
        req: &CastRequest,
        modes: &[u32],
        costs: &CostChoice,
    ) -> Vec<TargetSlotSpec>;

    /// The amount to divide among the targets, if any (CR 601.2d); `None`: nothing to divide.
    fn division_total(&self, req: &CastRequest, proposal: &Proposal) -> Option<u64>;

    /// CR 601.2a / 602.2a: puts the spell or ability on the stack as the topmost object with a
    /// new `ObjectId` and the caster as its controller, and returns that id. The real host moves
    /// the card out of its zone. No legality check happens here.
    fn put_on_stack(&mut self, req: &CastRequest) -> ObjectId;

    /// Whether the source has `quality` for targeting purposes: for a spell, the stack object
    /// as cast (the chosen face); for an ability, its source, with last known information if it
    /// left its zone (CR 113.7a, 702.11d, 702.16a).
    fn source_has_quality(&self, req: &CastRequest, stack_object: ObjectId, quality: &str) -> bool;

    /// Whether the prohibition `entry` applies to the fully specified proposal (CR 601.2e).
    fn prohibition_applies(
        &self,
        entry: &RuleEntry,
        req: &CastRequest,
        stack_object: ObjectId,
        proposal: &Proposal,
    ) -> bool;

    /// CR 601.3a: whether some choice of the proposal (a mode, a cost, X) could make `entry`
    /// stop applying.
    fn prohibition_could_lapse(&self, entry: &RuleEntry, req: &CastRequest) -> bool;

    /// Whether the chosen targets are legal together across slots; `true` by default.
    fn proposal_combination_legal(
        &self,
        req: &CastRequest,
        stack_object: ObjectId,
        proposal: &Proposal,
    ) -> bool {
        let _ = (req, stack_object, proposal);
        true
    }
}

/// The step of the SRS 7.1 table at which the procedure stopped (REQ-CAST-001).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CastStep {
    /// The begin check (CR 601.3), before step 0.
    Begin,
    /// Step 1: announce (CR 601.2a, 602.2a).
    Announce,
    /// Step 2: modes and costs (CR 601.2b).
    Choices,
    /// Step 3: targets (CR 601.2c).
    Targets,
    /// Step 4: division (CR 601.2d).
    Division,
    /// Step 5: the legality check of the fully specified proposal (CR 601.2e).
    Legality,
}

/// Why the action is illegal (REQ-CAST-001).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StopReason {
    /// A prohibition forbids beginning the action (CR 601.3).
    Prohibited {
        /// The prohibition.
        by: EffectId,
    },
    /// No mode can be chosen, or fewer than the minimum number (CR 700.2a).
    NoLegalMode,
    /// A required target slot has fewer legal candidates than its minimum (CR 601.2c).
    MissingTarget {
        /// The slot index.
        slot: usize,
    },
    /// The amount is smaller than the number of targets, so each can't receive at least one
    /// (CR 601.2d).
    DivisionImpossible {
        /// The amount to divide.
        total: u64,
        /// The number of distinct recipients.
        recipients: usize,
    },
    /// A prohibition applies to the fully specified proposal (CR 601.2e).
    ProposalProhibited {
        /// The prohibition.
        by: EffectId,
    },
    /// The chosen targets are illegal together ([`CastState::proposal_combination_legal`]).
    CombinationIllegal,
}

/// An illegal action, reversed (CR 734.1, 734.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RolledBack {
    /// The step that failed.
    pub step: CastStep,
    /// Why.
    pub reason: StopReason,
    /// The state hash after the rollback: the hash before the call.
    pub state_hash: Digest,
    /// The player with priority after the rollback: the one before the call (CR 734.2).
    pub priority_player: Option<PlayerId>,
}

/// The result of [`announce`] (REQ-CAST-001).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnounceOutcome {
    /// CR 601.3: refused before step 0 changed anything; the state is untouched.
    NotStarted {
        /// Why.
        reason: StopReason,
    },
    /// Steps 0-5 succeeded: the object is on the stack with this proposal.
    Announced {
        /// The spell or ability on the stack.
        stack_object: ObjectId,
        /// The fully specified proposal.
        proposal: Proposal,
    },
    /// A step was illegal and the whole action was reversed.
    RolledBack(RolledBack),
}

/// Runs steps 0-5 of casting a spell or activating an ability (REQ-CAST-001, CR 601.2,
/// 602.2): the begin check, the checkpoint, the announcement, modes and costs, targets,
/// division and the legality check of the fully specified proposal.
///
/// Usable directly as a [`ChoiceSession`](crate::choice::ChoiceSession) procedure. An illegal
/// step returns `Ok(AnnounceOutcome::RolledBack(..))` with the state restored exactly (same
/// hash, same priority player, CR 734.1, 734.2). On success the object stays on the stack and
/// `*state` keeps the changes. Library moves, shuffles and reveals are not reversible
/// (CR 734.1) and are the host's concern.
///
/// # Errors
/// [`Interrupt::Pending`] when a player must decide (propagate it), and [`Interrupt::Error`]
/// for a protocol error (an answer of the wrong shape, a malformed host spec). `*state` is
/// unchanged on error.
pub fn announce<S: CastState>(
    state: &mut S,
    ctx: &mut ChoiceContext<'_>,
    request: &CastRequest,
) -> Result<AnnounceOutcome, Interrupt> {
    // CR 601.3, 601.3a: the begin check, before step 0 changes anything.
    for entry in action_prohibitions(state.rule_set(), request) {
        if !state.prohibition_could_lapse(entry, request) {
            return Ok(AnnounceOutcome::NotStarted {
                reason: StopReason::Prohibited { by: entry.id },
            });
        }
    }
    // CR 734: step 0, the checkpoint.
    let mut txn = Transactions::new(state.clone());
    let id = txn.checkpoint();
    let result = run(&mut txn, ctx, request);
    match result {
        Ok(Ok((stack_object, proposal))) => {
            txn.commit(id).map_err(txn_error)?;
            *state = txn.into_state().map_err(txn_error)?;
            Ok(AnnounceOutcome::Announced {
                stack_object,
                proposal,
            })
        }
        Ok(Err((step, reason))) => rolled_back(state, txn, id, step, reason),
        Err(interrupt) => Err(interrupt),
    }
}

/// CR 734.1, 734.2: reverses the action and reports the restored hash and priority player.
fn rolled_back<S: CastState>(
    state: &mut S,
    mut txn: Transactions<S>,
    id: TxnId,
    step: CastStep,
    reason: StopReason,
) -> Result<AnnounceOutcome, Interrupt> {
    let report = txn.rollback(id).map_err(txn_error)?;
    *state = txn.into_state().map_err(txn_error)?;
    Ok(AnnounceOutcome::RolledBack(RolledBack {
        step,
        reason,
        state_hash: report.state_hash,
        priority_player: report.priority_player,
    }))
}

/// An illegal step: where and why.
type Stop = (CastStep, StopReason);

/// Steps 1-5 on the live state of `txn`: the stack object and proposal, or the failed step.
fn run<S: CastState>(
    txn: &mut Transactions<S>,
    ctx: &mut ChoiceContext<'_>,
    req: &CastRequest,
) -> Result<Result<(ObjectId, Proposal), Stop>, Interrupt> {
    // CR 601.2a, 602.2a: step 1, the object goes on the stack; no check happens here.
    let stack_object = txn.state_mut().put_on_stack(req);
    let state = txn.state();

    // CR 601.2b, 700.2: step 2, the modes.
    let modes = match state.mode_spec(req) {
        Some(spec) => match ask_modes(state, ctx, req, stack_object, &spec)? {
            Some(modes) => modes,
            None => return Ok(Err((CastStep::Choices, StopReason::NoLegalMode))),
        },
        None => Vec::new(),
    };

    // CR 601.2b: step 2, the costs and X.
    let spec = state.cost_spec(req, &modes);
    let costs = if spec.offers_choice() {
        ask_costs(ctx, req, stack_object, &spec)?
    } else {
        CostChoice::default()
    };

    // CR 601.2c: step 3, the targets.
    let mut slots = Vec::new();
    for (i, slot) in state.target_slots(req, &modes, &costs).iter().enumerate() {
        let legal = legal_candidates(state, req, stack_object, slot);
        if usize_to_u64(legal.len()) < slot.min {
            return Ok(Err((
                CastStep::Targets,
                StopReason::MissingTarget { slot: i },
            )));
        }
        if slot.max < slot.min {
            return Err(procedure_error(format!(
                "target slot {i} has max {} below min {}",
                slot.max, slot.min
            )));
        }
        slots.push(TargetSlot {
            min: slot.min,
            max: slot.max.min(usize_to_u64(legal.len())),
            legal,
            current: Vec::new(),
        });
    }
    let targets = if slots.is_empty() {
        Vec::new()
    } else {
        ask_targets(ctx, req, stack_object, slots)?
    };

    // CR 601.2d: step 4, the division.
    let mut proposal = Proposal {
        modes,
        costs,
        targets,
        division: None,
    };
    if let Some(total) = state.division_total(req, &proposal) {
        let mut seen = BTreeSet::new();
        let recipients: Vec<ChoiceTarget> = proposal
            .targets
            .iter()
            .flatten()
            .copied()
            .filter(|t| seen.insert(*t))
            .collect();
        if !recipients.is_empty() {
            if total < usize_to_u64(recipients.len()) {
                return Ok(Err((
                    CastStep::Division,
                    StopReason::DivisionImpossible {
                        total,
                        recipients: recipients.len(),
                    },
                )));
            }
            proposal.division = Some(ask_division(ctx, req, stack_object, total, &recipients)?);
        }
    }

    // CR 601.2e, 101.2: step 5, the fully specified proposal; no 601.3a leniency here.
    for entry in action_prohibitions(state.rule_set(), req) {
        if state.prohibition_applies(entry, req, stack_object, &proposal) {
            return Ok(Err((
                CastStep::Legality,
                StopReason::ProposalProhibited { by: entry.id },
            )));
        }
    }
    if !state.proposal_combination_legal(req, stack_object, &proposal) {
        return Ok(Err((CastStep::Legality, StopReason::CombinationIllegal)));
    }
    Ok(Ok((stack_object, proposal)))
}

// ---------------------------------------------------------------------------------------------
// Prohibitions on the action (CR 601.3, 601.2e)
// ---------------------------------------------------------------------------------------------

/// The `"zone"` filter parameter (#49 convention).
const PARAM_ZONE: &str = "zone";
/// The `"object"` filter parameter (#49 convention).
const PARAM_OBJECT: &str = "object";

/// The `PROHIBITION` entries applying to `req.caster` whose `"action"` is the request's action
/// and whose `"zone"` / `"object"` filters admit `req.object`, in application order
/// (CR 601.3, 101.2). The host reports no zone, so an entry with a `"zone"` parameter matches
/// nothing (R12: any malformed `"object"` matches nothing).
fn action_prohibitions<'a>(
    rules: &'a RuleSet,
    req: &'a CastRequest,
) -> impl Iterator<Item = &'a RuleEntry> + 'a {
    let action = match req.kind {
        CastKind::Spell { .. } => ACTION_CAST,
        CastKind::Ability { .. } => ACTION_ACTIVATE,
    };
    rules
        .applying(RuleKind::Prohibition, req.caster)
        .filter(move |e| {
            let params = &e.modification.params;
            e.action() == Some(action)
                && !params.contains_key(PARAM_ZONE)
                && params
                    .get(PARAM_OBJECT)
                    .is_none_or(|v| v.as_u64() == Some(req.object.get()))
        })
}

// ---------------------------------------------------------------------------------------------
// Targeting rules (CR 115, 702.11, 702.16, 702.18, 101.2)
// ---------------------------------------------------------------------------------------------

/// A `TARGETING_RULE` keyword.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Keyword {
    Hexproof,
    Shroud,
    Protection,
    Other,
}

/// What an applicable `TARGETING_RULE` entry does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetingEffect {
    Prohibit,
    IgnoreHexproof,
}

/// A parsed `TARGETING_RULE` entry: its keyword and effect; `None` from [`parse_targeting`]
/// when the entry is malformed (it matches nothing, R12).
#[derive(Clone, Copy)]
struct Targeting {
    keyword: Keyword,
    effect: TargetingEffect,
}

/// Reads an optional string parameter: `Ok(None)` absent, `Err(())` of the wrong type.
fn opt_str<'a>(entry: &'a RuleEntry, key: &str) -> Result<Option<&'a str>, ()> {
    match entry.modification.params.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(()),
    }
}

/// Reads an optional unsigned integer parameter: `Ok(None)` absent, `Err(())` otherwise.
fn opt_u64(entry: &RuleEntry, key: &str) -> Result<Option<u64>, ()> {
    match entry.modification.params.get(key) {
        None => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or(()),
    }
}

/// Parses the keyword and effect of a `TARGETING_RULE` entry (R12: unknown values give `None`).
fn parse_targeting(entry: &RuleEntry) -> Option<Targeting> {
    let keyword = match opt_str(entry, "keyword").ok()?? {
        "HEXPROOF" => Keyword::Hexproof,
        "SHROUD" => Keyword::Shroud,
        "PROTECTION" => Keyword::Protection,
        "OTHER" => Keyword::Other,
        _ => return None,
    };
    let effect = match opt_str(entry, "effect").ok()?.unwrap_or("PROHIBIT") {
        "PROHIBIT" => TargetingEffect::Prohibit,
        "IGNORE_HEXPROOF" => TargetingEffect::IgnoreHexproof,
        _ => return None,
    };
    Some(Targeting { keyword, effect })
}

/// Whether the target filter of `entry` (`"target_object"` / `"target_player"`, `"sources"`,
/// `"quality"`, `"source_player"`) admits `target` for this cast (CR 702.11b-d, 702.16a-b,
/// 702.16k, 702.18a). Any malformed parameter admits nothing (R12).
fn filter_admits<S: CastState>(
    state: &S,
    entry: &RuleEntry,
    req: &CastRequest,
    stack_object: ObjectId,
    target: ChoiceTarget,
) -> bool {
    let protected = match (
        opt_u64(entry, "target_object"),
        opt_u64(entry, "target_player"),
    ) {
        (Ok(Some(o)), Ok(None)) => ChoiceTarget::Object(ObjectId::new(o)),
        (Ok(None), Ok(Some(p))) => ChoiceTarget::Player(PlayerId::new(p)),
        _ => return false,
    };
    if protected != target {
        return false;
    }
    // CR 702.11b-c: hexproof applies only to spells and abilities an opponent controls.
    match opt_str(entry, "sources") {
        Ok(None | Some("ANY")) => {}
        Ok(Some("OPPONENTS")) => {
            if !state.are_opponents(req.caster, state.target_controller(target)) {
                return false;
            }
        }
        _ => return false,
    }
    // CR 702.11d, 702.16a-b: "from [quality]" applies only to sources with the quality.
    match opt_str(entry, "quality") {
        Ok(None) => {}
        Ok(Some(q)) => {
            if !state.source_has_quality(req, stack_object, q) {
                return false;
            }
        }
        Err(()) => return false,
    }
    // CR 702.16k: protection from a player.
    match opt_u64(entry, "source_player") {
        Ok(None) => true,
        Ok(Some(p)) => p == req.caster.get(),
        Err(()) => false,
    }
}

/// Whether `target` is a legal target for this cast as far as the `TARGETING_RULE` entries say
/// (CR 101.2, 702.11e): no applicable `PROHIBIT` entry remains after `IGNORE_HEXPROOF`
/// permissions suppress the hexproof ones. The result is independent of entry order.
fn targeting_allows<S: CastState>(
    state: &S,
    req: &CastRequest,
    stack_object: ObjectId,
    target: ChoiceTarget,
) -> bool {
    let entries = state.rule_set().entries(RuleKind::TargetingRule);
    let applicable: Vec<(&RuleEntry, Targeting)> = entries
        .iter()
        .filter_map(|e| parse_targeting(e).map(|t| (e, t)))
        .filter(|(e, _)| filter_admits(state, e, req, stack_object, target))
        .collect();
    // CR 702.11e: "as though it didn't have hexproof" (only for the caster it modifies).
    let ignore_hexproof = applicable
        .iter()
        .any(|(e, t)| t.effect == TargetingEffect::IgnoreHexproof && e.applies_to(req.caster));
    // CR 101.2: any remaining "can't" wins.
    !applicable.iter().any(|(_, t)| {
        t.effect == TargetingEffect::Prohibit
            && !(ignore_hexproof && t.keyword == Keyword::Hexproof)
    })
}

/// The legal candidates of a slot, ascending without duplicates (R3): the predicate
/// candidates, minus the stack object itself (CR 115.5), minus those a targeting rule forbids
/// (CR 115.2-115.4 are the host's predicate; CR 702.11, 702.16, 702.18). Ward is not checked
/// here (it is a triggered ability).
fn legal_candidates<S: CastState>(
    state: &S,
    req: &CastRequest,
    stack_object: ObjectId,
    slot: &TargetSlotSpec,
) -> Vec<ChoiceTarget> {
    let set: BTreeSet<ChoiceTarget> = slot.candidates.iter().copied().collect();
    set.into_iter()
        .filter(|&t| t != ChoiceTarget::Object(stack_object))
        .filter(|&t| targeting_allows(state, req, stack_object, t))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The asks (CR 601.2b-601.2d)
// ---------------------------------------------------------------------------------------------

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

/// A request with the fields every ask of this procedure shares (`hidden: false`, no reveal).
#[allow(
    clippy::too_many_arguments,
    reason = "a plain constructor of the request struct's fields"
)]
fn request(
    ctx: &ChoiceContext<'_>,
    req: &CastRequest,
    choice_type: ChoiceType,
    cr: &str,
    stack_object: ObjectId,
    (min, max): (u64, u64),
    default_on_timeout: ChoiceResponse,
    options: ChoiceOptions,
    payload: ChoicePayload,
) -> PendingChoiceRequest {
    PendingChoiceRequest {
        request_id: ctx.next_request_id(),
        chooser: req.caster,
        choice_type,
        reason: ChoiceReason {
            cr: cr.to_owned(),
            source: Some(stack_object),
        },
        min,
        max,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout,
        options,
        payload,
    }
}

/// Whether mode `mode` (original index) can be chosen: every target slot it requires has at
/// least its minimum of legal candidates (CR 700.2a). The slots are those of the mode alone with
/// no cost chosen.
fn mode_legal<S: CastState>(
    state: &S,
    req: &CastRequest,
    stack_object: ObjectId,
    mode: u32,
) -> bool {
    state
        .target_slots(req, &[mode], &CostChoice::default())
        .iter()
        .all(|s| usize_to_u64(legal_candidates(state, req, stack_object, s).len()) >= s.min)
}

/// CR 601.2b, 700.2a, 700.2d: asks the modes among the legal ones and returns the chosen
/// ORIGINAL indices, ascending; `None` when too few modes can be chosen.
fn ask_modes<S: CastState>(
    state: &S,
    ctx: &mut ChoiceContext<'_>,
    req: &CastRequest,
    stack_object: ObjectId,
    spec: &ModeSpec,
) -> Result<Option<Vec<u32>>, Interrupt> {
    // CR 700.2a: an illegal mode can't be chosen; the request lists only the legal ones.
    let available: Vec<u32> = (0..spec.labels.len())
        .filter_map(|i| u32::try_from(i).ok())
        .filter(|&m| mode_legal(state, req, stack_object, m))
        .collect();
    let k = usize_to_u64(available.len());
    let min = u64::from(spec.min);
    let max = if spec.repeats_allowed {
        u64::from(spec.max)
    } else {
        u64::from(spec.max).min(k)
    };
    if available.is_empty() || (!spec.repeats_allowed && k < min) || max < min {
        return Ok(None);
    }
    let pick = |list: &[String]| -> Vec<String> {
        available
            .iter()
            .filter_map(|&m| list.get(u32_to_usize(m)).cloned())
            .collect()
    };
    let labels = pick(&spec.labels);
    let mode_costs = pick(&spec.mode_costs);
    let default: Vec<u32> = (0..min)
        .map(|i| u32::try_from(i % k).unwrap_or(0))
        .collect();
    let options = if mode_answers_capped(k, min, max, spec.repeats_allowed) < ENUMERATION_LIMIT {
        ChoiceOptions::Enumerated(
            (0..available.len())
                .filter_map(|i| u32::try_from(i).ok())
                .map(ChoiceOption::Mode)
                .collect(),
        )
    } else {
        ChoiceOptions::Validator(ValidatorContract {
            min,
            max,
            rule: ValidatorRule::Modes {
                count: k,
                repeats_allowed: spec.repeats_allowed,
            },
        })
    };
    let r = request(
        ctx,
        req,
        ChoiceType::ChooseModes,
        "CR 601.2b",
        stack_object,
        (min, max),
        ChoiceResponse::SelectModes { modes: default },
        options,
        ChoicePayload::ChooseModes {
            labels,
            repeats_allowed: spec.repeats_allowed,
            mode_costs,
        },
    );
    match ask_checked(ctx, r)? {
        ChoiceResponse::SelectModes { modes } => {
            // CR 700.2d: a repeated mode counts once per choice, in printed order.
            let mut chosen = Vec::with_capacity(modes.len());
            for m in modes {
                let original = available
                    .get(u32_to_usize(m))
                    .copied()
                    .ok_or_else(|| procedure_error(format!("mode index {m} is out of range")))?;
                chosen.push(original);
            }
            chosen.sort_unstable();
            Ok(Some(chosen))
        }
        other => Err(unexpected(&other, ChoiceType::ChooseModes)),
    }
}

/// CR 601.2b: asks the alternative cost (at most one), the additional costs, X, the symbol
/// options and the harmonize creature.
fn ask_costs(
    ctx: &mut ChoiceContext<'_>,
    req: &CastRequest,
    stack_object: ObjectId,
    spec: &CostSpec,
) -> Result<CostChoice, Interrupt> {
    if let Some((lo, hi)) = spec.x_range
        && lo > hi
    {
        return Err(procedure_error(format!("the X range {lo}..={hi} is empty")));
    }
    if spec.symbol_slots.iter().any(Vec::is_empty) {
        return Err(procedure_error(
            "a cost-symbol slot has no option".to_owned(),
        ));
    }
    let default = ChoiceResponse::ConfigureCosts {
        alternative: None,
        additional: Vec::new(),
        x: spec.x_range.map(|(lo, _)| lo),
        symbol_options: vec![0; spec.symbol_slots.len()],
        harmonize: None,
    };
    let options = if cost_answers_capped(spec) < ENUMERATION_LIMIT {
        // The building blocks of an answer (decision "#52 FP-050 — Choice request shapes").
        let mut items: Vec<ChoiceOption> = Vec::new();
        items.extend(
            spec.alternative_costs
                .iter()
                .cloned()
                .map(ChoiceOption::Text),
        );
        items.extend(
            spec.additional_costs
                .iter()
                .cloned()
                .map(ChoiceOption::Text),
        );
        if let Some((lo, hi)) = spec.x_range {
            items.extend(
                (lo..=hi).map(|x| ChoiceOption::Number(i64::try_from(x).unwrap_or(i64::MAX))),
            );
        }
        for slot in &spec.symbol_slots {
            items.extend(slot.iter().cloned().map(ChoiceOption::Text));
        }
        items.extend(
            spec.harmonize_candidates
                .iter()
                .copied()
                .map(ChoiceOption::Object),
        );
        ChoiceOptions::Enumerated(items)
    } else {
        ChoiceOptions::Validator(ValidatorContract {
            min: 1,
            max: 1,
            rule: ValidatorRule::Payload,
        })
    };
    let r = request(
        ctx,
        req,
        ChoiceType::ChooseCostsAndX,
        "CR 601.2b",
        stack_object,
        (1, 1),
        default,
        options,
        ChoicePayload::ChooseCostsAndX {
            alternative_costs: spec.alternative_costs.clone(),
            additional_costs: spec.additional_costs.clone(),
            x_range: spec.x_range,
            symbol_slots: spec.symbol_slots.clone(),
            harmonize_candidates: spec.harmonize_candidates.clone(),
        },
    );
    match ask_checked(ctx, r)? {
        ChoiceResponse::ConfigureCosts {
            alternative,
            mut additional,
            x,
            symbol_options,
            harmonize,
        } => {
            additional.sort_unstable();
            Ok(CostChoice {
                alternative,
                additional,
                x,
                symbol_options,
                harmonize,
            })
        }
        other => Err(unexpected(&other, ChoiceType::ChooseCostsAndX)),
    }
}

/// CR 601.2c, 115.3, 115.6: asks one target list per slot; within a slot no repeats, across
/// slots the same target may be chosen again.
fn ask_targets(
    ctx: &mut ChoiceContext<'_>,
    req: &CastRequest,
    stack_object: ObjectId,
    slots: Vec<TargetSlot>,
) -> Result<Vec<Vec<ChoiceTarget>>, Interrupt> {
    let min = slots.iter().fold(0u64, |acc, s| acc.saturating_add(s.min));
    let max = slots.iter().fold(0u64, |acc, s| acc.saturating_add(s.max));
    let default: Vec<Vec<ChoiceTarget>> = slots
        .iter()
        .map(|s| s.legal.iter().take(u64_to_usize(s.min)).copied().collect())
        .collect();
    let options = if target_answers_capped(&slots) < ENUMERATION_LIMIT {
        let all: BTreeSet<ChoiceTarget> =
            slots.iter().flat_map(|s| s.legal.iter().copied()).collect();
        ChoiceOptions::Enumerated(all.into_iter().map(target_option).collect())
    } else {
        ChoiceOptions::Validator(ValidatorContract {
            min,
            max,
            rule: ValidatorRule::Payload,
        })
    };
    let r = request(
        ctx,
        req,
        ChoiceType::ChooseTargets,
        "CR 601.2c",
        stack_object,
        (min, max),
        ChoiceResponse::SelectTargets { targets: default },
        options,
        ChoicePayload::ChooseTargets {
            slots,
            may_keep_illegal: false,
        },
    );
    match ask_checked(ctx, r)? {
        ChoiceResponse::SelectTargets { targets } => Ok(targets
            .into_iter()
            .map(|mut slot| {
                slot.sort_unstable();
                slot
            })
            .collect()),
        other => Err(unexpected(&other, ChoiceType::ChooseTargets)),
    }
}

/// CR 601.2d: asks the division of `total` among `recipients` (distinct, in slot order), at
/// least one each; returns the shares in recipient order.
fn ask_division(
    ctx: &mut ChoiceContext<'_>,
    req: &CastRequest,
    stack_object: ObjectId,
    total: u64,
    recipients: &[ChoiceTarget],
) -> Result<Vec<(ChoiceTarget, u64)>, Interrupt> {
    let n = usize_to_u64(recipients.len());
    let extra = total.saturating_sub(n);
    let default: Vec<(ChoiceTarget, u64)> = recipients
        .iter()
        .enumerate()
        .map(|(i, &t)| {
            (
                t,
                if i == 0 {
                    1u64.saturating_add(extra)
                } else {
                    1
                },
            )
        })
        .collect();
    let r = request(
        ctx,
        req,
        ChoiceType::DivideValue,
        "CR 601.2d",
        stack_object,
        (n, n),
        ChoiceResponse::SubmitDivision {
            assignments: default,
        },
        ChoiceOptions::Enumerated(recipients.iter().copied().map(target_option).collect()),
        ChoicePayload::DivideValue {
            total,
            recipients: recipients.iter().map(|&t| (t, 1)).collect(),
        },
    );
    match ask_checked(ctx, r)? {
        ChoiceResponse::SubmitDivision { assignments } => Ok(recipients
            .iter()
            .map(|&t| {
                let share = assignments
                    .iter()
                    .find(|(a, _)| *a == t)
                    .map_or(0, |(_, s)| *s);
                (t, share)
            })
            .collect()),
        other => Err(unexpected(&other, ChoiceType::DivideValue)),
    }
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

fn target_option(t: ChoiceTarget) -> ChoiceOption {
    match t {
        ChoiceTarget::Player(p) => ChoiceOption::Player(p),
        ChoiceTarget::Object(o) => ChoiceOption::Object(o),
    }
}

fn procedure_error(text: String) -> Interrupt {
    Interrupt::Error(ChoiceError::Procedure(text))
}

fn unexpected(answer: &ChoiceResponse, asked: ChoiceType) -> Interrupt {
    procedure_error(format!(
        "{} does not answer {}",
        answer.wire_name(),
        asked.as_str()
    ))
}

fn txn_error(e: crate::txn::TxnError) -> Interrupt {
    procedure_error(format!("transaction error: {e}"))
}

fn usize_to_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn u64_to_usize(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

fn u32_to_usize(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// `min(C(n, k), ENUMERATION_LIMIT)`.
fn binomial_capped(n: u64, k: u64) -> u64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut c: u128 = 1;
    for i in 0..k {
        c = c * u128::from(n - i) / u128::from(i + 1);
        if c >= u128::from(ENUMERATION_LIMIT) {
            return ENUMERATION_LIMIT;
        }
    }
    u64::try_from(c).unwrap_or(ENUMERATION_LIMIT)
}

/// `min(number of valid CHOOSE_MODES answers, ENUMERATION_LIMIT)` among `k` modes: sets of
/// `min..=max` distinct modes, or multisets when repeats are allowed (CR 700.2d).
fn mode_answers_capped(k: u64, min: u64, max: u64, repeats: bool) -> u64 {
    let mut sum: u64 = 0;
    let mut j = min;
    while j <= max && sum < ENUMERATION_LIMIT {
        let count = if repeats {
            // Multisets of size j from k kinds: C(k + j - 1, j).
            if k == 0 {
                u64::from(j == 0)
            } else {
                binomial_capped(k.saturating_add(j).saturating_sub(1), j)
            }
        } else {
            binomial_capped(k, j)
        };
        sum = sum.saturating_add(count);
        j += 1;
    }
    sum.min(ENUMERATION_LIMIT)
}

/// `min(number of valid CONFIGURE_COSTS answers, ENUMERATION_LIMIT)`.
fn cost_answers_capped(spec: &CostSpec) -> u64 {
    let mut product: u64 = usize_to_u64(spec.alternative_costs.len()).saturating_add(1);
    let mut mul = |f: u64| {
        product = product.saturating_mul(f).min(ENUMERATION_LIMIT);
    };
    for _ in &spec.additional_costs {
        mul(2);
    }
    if let Some((lo, hi)) = spec.x_range {
        mul(hi.saturating_sub(lo).saturating_add(1));
    }
    for slot in &spec.symbol_slots {
        mul(usize_to_u64(slot.len()));
    }
    mul(usize_to_u64(spec.harmonize_candidates.len()).saturating_add(1));
    product
}

/// `min(number of valid CHOOSE_TARGETS answers, ENUMERATION_LIMIT)`: per slot, the subsets of
/// its legal targets of a size in `min..=max`, multiplied over the slots.
fn target_answers_capped(slots: &[TargetSlot]) -> u64 {
    let mut product: u64 = 1;
    for slot in slots {
        let n = usize_to_u64(slot.legal.len());
        let mut sum: u64 = 0;
        let mut k = slot.min;
        while k <= slot.max && sum < ENUMERATION_LIMIT {
            sum = sum.saturating_add(binomial_capped(n, k));
            k += 1;
        }
        product = product.saturating_mul(sum.min(ENUMERATION_LIMIT));
        if product >= ENUMERATION_LIMIT {
            return ENUMERATION_LIMIT;
        }
    }
    product
}

/// The capped counters, exposed to the unit tests.
#[cfg(test)]
pub(super) mod tests_support {
    /// `min(C(n, k), ENUMERATION_LIMIT)`.
    pub(crate) fn binomial(n: u64, k: u64) -> u64 {
        super::binomial_capped(n, k)
    }

    /// The capped CHOOSE_MODES answer count.
    pub(crate) fn modes(k: u64, min: u64, max: u64, repeats: bool) -> u64 {
        super::mode_answers_capped(k, min, max, repeats)
    }
}
