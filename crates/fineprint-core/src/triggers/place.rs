// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Putting waiting triggers on the stack (REQ-EVENT-002, REQ-ARCH-002; CR 603.3, CR 603.3b-d,
//! CR 700.2b-c, CR 101.4), and the "may" choice an optional trigger makes when it resolves
//! (CR 603.5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_dsl::ability::{EventKind, ModeSetAST, TargetSlotAST, Trigger, TriggeredAbilityAST};

use super::detect::WaitingTrigger;
use super::event::EventField;
use super::sourceless::SourcelessAbility;
use crate::choice::{
    ChoiceContext, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason,
    ChoiceResponse, ChoiceTarget, ChoiceType, ENUMERATION_LIMIT, Interrupt, PendingChoiceRequest,
    TargetSlot, ValidatorContract, ValidatorRule,
};

/// Where a pending trigger came from (REQ-EVENT-002).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TriggerOrigin {
    /// An event trigger found by [`detect`](super::detect) (CR 603.2).
    Event,
    /// A state trigger (CR 603.8).
    State,
    /// A delayed triggered ability (CR 603.7).
    Delayed,
    /// A reflexive triggered ability (CR 603.12).
    Reflexive,
    /// An inherent sourceless ability (CR 113.8 exception; CR 726.2, CR 727.2, CR 729.1,
    /// CR 702.179d).
    Sourceless(SourcelessAbility),
}

/// A triggered ability that has triggered and waits to be put on the stack, of any origin
/// (REQ-EVENT-002, CR 603.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingTrigger {
    /// Where it came from.
    pub origin: TriggerOrigin,
    /// Its controller (CR 113.8, CR 603.3a; CR 603.7d-g for a delayed trigger).
    pub controller: PlayerId,
    /// Its source; `None` only for a sourceless trigger.
    pub source: Option<ObjectId>,
    /// The ability instance's id.
    pub ability_id: String,
    /// Bound variable name to value.
    pub bindings: BTreeMap<String, EventField>,
    /// Whether the effect is optional ("may", CR 603.5); the choice is made on resolution.
    pub optional: bool,
    /// True iff its trigger condition is another ability triggering: it goes on the stack in
    /// the second part of CR 603.3b.
    pub responds_to_trigger: bool,
    /// The ability-level target slots.
    pub targets: Vec<TargetSlotAST>,
    /// The modes; each mode may have its own target slots (CR 700.2c).
    pub modes: Option<ModeSetAST>,
}

/// Whether `ability`'s trigger condition is another ability triggering (CR 603.3b).
fn responds_to_trigger(ability: &TriggeredAbilityAST) -> bool {
    matches!(&ability.trigger, Trigger::Event(m) if m.kind == EventKind::Triggered)
}

impl PendingTrigger {
    /// A pending trigger with option, targets, modes and the CR 603.3b part read from
    /// `ability`.
    pub(super) fn new(
        origin: TriggerOrigin,
        controller: PlayerId,
        source: Option<ObjectId>,
        ability_id: String,
        bindings: BTreeMap<String, EventField>,
        ability: &TriggeredAbilityAST,
    ) -> PendingTrigger {
        PendingTrigger {
            origin,
            controller,
            source,
            ability_id,
            bindings,
            optional: ability.optional == Some(true),
            responds_to_trigger: responds_to_trigger(ability),
            targets: ability.targets.clone().unwrap_or_default(),
            modes: ability.modes.clone(),
        }
    }

    /// The pending trigger of a detected trigger `w` (REQ-EVENT-002). `optional`, `targets`
    /// and `modes` are copied from `ability` (absent: `false`, none, `None`);
    /// `responds_to_trigger` is true iff `ability`'s trigger is an event matcher of kind
    /// [`EventKind::Triggered`] (CR 603.3b). The ability id, source, controller and bindings
    /// are `w`'s.
    #[must_use]
    pub fn from_waiting(
        w: WaitingTrigger,
        ability: &TriggeredAbilityAST,
        origin: TriggerOrigin,
    ) -> PendingTrigger {
        PendingTrigger::new(
            origin,
            w.controller,
            Some(w.source),
            w.ability_id,
            w.bindings,
            ability,
        )
    }

    /// The text shown for this trigger in an `ORDER_SIMULTANEOUS_TRIGGERS` payload:
    /// `object <source> ability <id>`, or `player <controller> ability <id>` without a source.
    #[must_use]
    pub fn description(&self) -> String {
        match self.source {
            Some(o) => format!("object {} ability {}", o.get(), self.ability_id),
            None => format!(
                "player {} ability {}",
                self.controller.get(),
                self.ability_id
            ),
        }
    }
}

/// A trigger put on the stack, with its choices (REQ-EVENT-002, CR 603.3c-d).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackedTrigger {
    /// The trigger.
    pub trigger: PendingTrigger,
    /// The chosen modes, as original indices into `trigger.modes`, ascending; empty if it is not
    /// modal (CR 603.3c, CR 700.2b).
    pub modes: Vec<u32>,
    /// One target list per target slot: the ability-level slots first, in order, then the slots
    /// of each chosen mode, in ascending mode order (CR 603.3d, CR 700.2c).
    pub targets: Vec<Vec<ChoiceTarget>>,
}

/// Why a trigger was removed instead of being put on the stack (REQ-EVENT-002).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemovedReason {
    /// No legal choice could be made: a required target slot cannot be filled, or no mode can
    /// be chosen (CR 603.3c, CR 603.3d, CR 700.2b). Maps to `TriggerRemovedEvent` with reason
    /// `NO_LEGAL_TARGETS` (SRS 15.4).
    NoLegalTargets,
}

/// A trigger removed during placement (REQ-EVENT-002, CR 603.3c-d).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedTrigger {
    /// The trigger.
    pub trigger: PendingTrigger,
    /// Why it was removed.
    pub reason: RemovedReason,
}

/// The outcome of [`place_triggers`] (REQ-EVENT-002).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TriggerPlacement {
    /// The triggers put on the stack, in that order: index 0 first (deepest); the last one is
    /// the topmost and resolves first (CR 603.3).
    pub stacked: Vec<StackedTrigger>,
    /// The removed triggers, in the order they were found to have no legal choice.
    pub removed: Vec<RemovedTrigger>,
}

/// The game state as seen by trigger placement (REQ-EVENT-002, REQ-ARCH-002). The integration
/// unit implements it on the real state; tests implement it with fakes.
pub trait PlacementEnv {
    /// The legal targets of slot `slot` of `trigger` as it is about to be put on the stack.
    /// `mode: None` names the ability-level slot `trigger.targets[slot]`; `mode: Some(i)` names
    /// slot `slot` of original mode `i` (`trigger.modes.modes[i].targets`). `stacked_so_far` is
    /// what this placement has already put on the stack. Ascending, no duplicates.
    fn legal_targets(
        &self,
        trigger: &PendingTrigger,
        mode: Option<usize>,
        slot: usize,
        stacked_so_far: &[StackedTrigger],
    ) -> Vec<ChoiceTarget>;
}

fn procedure_error(text: String) -> Interrupt {
    Interrupt::Error(ChoiceError::Procedure(text))
}

fn to_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn to_usize(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// `min(n!, ENUMERATION_LIMIT)`.
fn permutations_capped(n: usize) -> u64 {
    let mut f: u64 = 1;
    for i in 2..=to_u64(n) {
        f = f.saturating_mul(i);
        if f >= ENUMERATION_LIMIT {
            return ENUMERATION_LIMIT;
        }
    }
    f
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

/// `min(number of valid CHOOSE_TARGETS answers, ENUMERATION_LIMIT)`: per slot, the subsets of
/// its legal targets of a size in `min..=max`, multiplied over the slots.
fn target_answers_capped(slots: &[TargetSlot]) -> u64 {
    let mut product: u64 = 1;
    for slot in slots {
        let n = to_u64(slot.legal.len());
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

/// The legal targets `env` gives, sorted ascending without duplicates (R3).
fn legal_for<E: PlacementEnv + ?Sized>(
    env: &E,
    trigger: &PendingTrigger,
    mode: Option<usize>,
    slot: usize,
    stacked: &[StackedTrigger],
) -> Vec<ChoiceTarget> {
    let set: BTreeSet<ChoiceTarget> = env
        .legal_targets(trigger, mode, slot, stacked)
        .into_iter()
        .collect();
    set.into_iter().collect()
}

/// One slot to fill: the AST slot and its legal targets.
struct SlotChoice<'a> {
    ast: &'a TargetSlotAST,
    legal: Vec<ChoiceTarget>,
}

impl SlotChoice<'_> {
    /// CR 603.3d: a required slot with fewer legal targets than its minimum has no legal
    /// choice.
    fn unfillable(&self) -> bool {
        self.legal.len() < to_usize(self.ast.min)
    }

    fn request_slot(&self) -> TargetSlot {
        TargetSlot {
            min: u64::from(self.ast.min),
            max: u64::from(self.ast.max).min(to_u64(self.legal.len())),
            legal: self.legal.clone(),
            current: Vec::new(),
        }
    }
}

/// Asks the order of one player's triggers of one part (CR 603.3b, CR 101.4c). Returns the
/// indices into `group` in the order they go on the stack.
fn ask_order(
    ctx: &mut ChoiceContext<'_>,
    player: PlayerId,
    group: &[PendingTrigger],
) -> Result<Vec<usize>, Interrupt> {
    let n = group.len();
    let count = to_u64(n);
    let identity: Vec<u32> = (0..n).filter_map(|i| u32::try_from(i).ok()).collect();
    let options = if permutations_capped(n) < ENUMERATION_LIMIT {
        ChoiceOptions::Enumerated(
            (0..n)
                .map(|i| ChoiceOption::Number(i64::try_from(i).unwrap_or(i64::MAX)))
                .collect(),
        )
    } else {
        ChoiceOptions::Validator(ValidatorContract {
            min: count,
            max: count,
            rule: ValidatorRule::Payload,
        })
    };
    let request = PendingChoiceRequest {
        request_id: ctx.next_request_id(),
        chooser: player,
        choice_type: ChoiceType::OrderSimultaneousTriggers,
        reason: ChoiceReason {
            cr: "CR 603.3b".to_owned(),
            source: None,
        },
        min: count,
        max: count,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::OrderTriggers { order: identity },
        options,
        payload: ChoicePayload::OrderSimultaneousTriggers {
            triggers: group.iter().map(PendingTrigger::description).collect(),
        },
    };
    match ctx.ask(request)? {
        ChoiceResponse::OrderTriggers { order } => {
            let mut seen = BTreeSet::new();
            let mut out = Vec::with_capacity(n);
            for i in order {
                let i = to_usize(i);
                if i >= n || !seen.insert(i) {
                    return Err(procedure_error(format!(
                        "ORDER_TRIGGERS answer is not a permutation of {n} triggers"
                    )));
                }
                out.push(i);
            }
            if out.len() != n {
                return Err(procedure_error(format!(
                    "ORDER_TRIGGERS answer is not a permutation of {n} triggers"
                )));
            }
            Ok(out)
        }
        other => Err(procedure_error(format!(
            "{} does not answer ORDER_SIMULTANEOUS_TRIGGERS",
            other.wire_name()
        ))),
    }
}

/// Asks the modes of `trigger` among its `legal` modes (original indices) (CR 603.3c,
/// CR 700.2b). Returns the chosen original indices, ascending.
fn ask_modes(
    ctx: &mut ChoiceContext<'_>,
    trigger: &PendingTrigger,
    set: &ModeSetAST,
    legal: &[usize],
) -> Result<Vec<usize>, Interrupt> {
    let k = legal.len();
    let min = u64::from(set.min);
    let max = u64::from(set.max).min(to_u64(k));
    let labels: Vec<String> = legal
        .iter()
        .filter_map(|&i| set.modes.get(i).map(|m| m.id.clone()))
        .collect();
    let default: Vec<u32> = (0..set.min).collect();
    let request = PendingChoiceRequest {
        request_id: ctx.next_request_id(),
        chooser: trigger.controller,
        choice_type: ChoiceType::ChooseModes,
        reason: ChoiceReason {
            cr: "CR 603.3c".to_owned(),
            source: trigger.source,
        },
        min,
        max,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SelectModes { modes: default },
        options: ChoiceOptions::Enumerated(
            (0..k)
                .filter_map(|i| u32::try_from(i).ok())
                .map(ChoiceOption::Mode)
                .collect(),
        ),
        payload: ChoicePayload::ChooseModes {
            labels,
            repeats_allowed: false,
            mode_costs: Vec::new(),
        },
    };
    match ctx.ask(request)? {
        ChoiceResponse::SelectModes { modes } => {
            let mut chosen = BTreeSet::new();
            for m in modes {
                let original = legal
                    .get(to_usize(m))
                    .copied()
                    .ok_or_else(|| procedure_error(format!("mode index {m} is out of range")))?;
                chosen.insert(original);
            }
            Ok(chosen.into_iter().collect())
        }
        other => Err(procedure_error(format!(
            "{} does not answer CHOOSE_MODES",
            other.wire_name()
        ))),
    }
}

/// Asks the targets of `trigger` for `slots` (CR 603.3d).
fn ask_targets(
    ctx: &mut ChoiceContext<'_>,
    trigger: &PendingTrigger,
    slots: &[SlotChoice<'_>],
) -> Result<Vec<Vec<ChoiceTarget>>, Interrupt> {
    let request_slots: Vec<TargetSlot> = slots.iter().map(SlotChoice::request_slot).collect();
    let min = request_slots
        .iter()
        .fold(0u64, |acc, s| acc.saturating_add(s.min));
    let max = request_slots
        .iter()
        .fold(0u64, |acc, s| acc.saturating_add(s.max));
    let default: Vec<Vec<ChoiceTarget>> = request_slots
        .iter()
        .map(|s| {
            s.legal
                .iter()
                .take(usize::try_from(s.min).unwrap_or(usize::MAX))
                .copied()
                .collect()
        })
        .collect();
    let options = if target_answers_capped(&request_slots) < ENUMERATION_LIMIT {
        let all: BTreeSet<ChoiceTarget> = request_slots
            .iter()
            .flat_map(|s| s.legal.iter().copied())
            .collect();
        ChoiceOptions::Enumerated(
            all.into_iter()
                .map(|t| match t {
                    ChoiceTarget::Player(p) => ChoiceOption::Player(p),
                    ChoiceTarget::Object(o) => ChoiceOption::Object(o),
                })
                .collect(),
        )
    } else {
        ChoiceOptions::Validator(ValidatorContract {
            min,
            max,
            rule: ValidatorRule::Payload,
        })
    };
    let request = PendingChoiceRequest {
        request_id: ctx.next_request_id(),
        chooser: trigger.controller,
        choice_type: ChoiceType::ChooseTargets,
        reason: ChoiceReason {
            cr: "CR 603.3d".to_owned(),
            source: trigger.source,
        },
        min,
        max,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::SelectTargets { targets: default },
        options,
        payload: ChoicePayload::ChooseTargets {
            slots: request_slots,
            may_keep_illegal: false,
        },
    };
    match ctx.ask(request)? {
        ChoiceResponse::SelectTargets { targets } => Ok(targets),
        other => Err(procedure_error(format!(
            "{} does not answer CHOOSE_TARGETS",
            other.wire_name()
        ))),
    }
}

/// Records `trigger` as removed for having no legal choice (CR 603.3c, CR 603.3d).
fn remove(placement: &mut TriggerPlacement, trigger: PendingTrigger) -> Result<(), Interrupt> {
    placement.removed.push(RemovedTrigger {
        trigger,
        reason: RemovedReason::NoLegalTargets,
    });
    Ok(())
}

/// Puts one trigger on the stack with its modes and targets, or finds it has no legal choice
/// (CR 603.3c, CR 603.3d, CR 700.2b, CR 700.2c).
fn put_one<E: PlacementEnv + ?Sized>(
    env: &E,
    ctx: &mut ChoiceContext<'_>,
    trigger: PendingTrigger,
    placement: &mut TriggerPlacement,
) -> Result<(), Interrupt> {
    // (a) Ability-level slots: CR 603.3d, an unfillable required slot removes the ability.
    let mut slots: Vec<SlotChoice<'_>> = Vec::new();
    for (i, ast) in trigger.targets.iter().enumerate() {
        let legal = legal_for(env, &trigger, None, i, &placement.stacked);
        slots.push(SlotChoice { ast, legal });
    }
    if slots.iter().any(SlotChoice::unfillable) {
        return remove(placement, trigger);
    }
    // (b) Modes: CR 603.3c, CR 700.2b; a mode whose own required targets cannot be chosen can't
    // be chosen. CR 700.2c: a mode's targets are needed only if that mode is chosen.
    let mut chosen_modes: Vec<usize> = Vec::new();
    let mut mode_slots: BTreeMap<usize, Vec<Vec<ChoiceTarget>>> = BTreeMap::new();
    if let Some(set) = &trigger.modes {
        let mut legal_modes = Vec::new();
        for (i, mode) in set.modes.iter().enumerate() {
            let asts = mode.targets.as_deref().unwrap_or_default();
            let mut lists = Vec::with_capacity(asts.len());
            let mut ok = true;
            for (j, ast) in asts.iter().enumerate() {
                let legal = legal_for(env, &trigger, Some(i), j, &placement.stacked);
                if legal.len() < to_usize(ast.min) {
                    ok = false;
                }
                lists.push(legal);
            }
            if ok {
                legal_modes.push(i);
                mode_slots.insert(i, lists);
            }
        }
        if legal_modes.is_empty() || legal_modes.len() < to_usize(set.min) {
            return remove(placement, trigger);
        }
        chosen_modes = ask_modes(ctx, &trigger, set, &legal_modes)?;
        if chosen_modes.is_empty() {
            // CR 603.3c: if no mode is chosen, the ability is removed from the stack.
            return remove(placement, trigger);
        }
    }
    // (c) Targets: CR 603.3d, the ability-level slots then each chosen mode's slots.
    let modes_set = trigger.modes.as_ref();
    for &i in &chosen_modes {
        let asts = modes_set
            .and_then(|s| s.modes.get(i))
            .and_then(|m| m.targets.as_deref())
            .unwrap_or_default();
        let lists = mode_slots.remove(&i).unwrap_or_default();
        for (ast, legal) in asts.iter().zip(lists) {
            slots.push(SlotChoice { ast, legal });
        }
    }
    let targets = if slots.is_empty() {
        Vec::new()
    } else {
        ask_targets(ctx, &trigger, &slots)?
    };
    // (d) CR 603.3: it becomes the topmost object on the stack. No optional ask (CR 603.5).
    let modes = chosen_modes
        .into_iter()
        .filter_map(|i| u32::try_from(i).ok())
        .collect();
    placement.stacked.push(StackedTrigger {
        trigger,
        modes,
        targets,
    });
    Ok(())
}

/// Puts the waiting triggers on the stack (REQ-EVENT-002, REQ-ARCH-002; CR 603.3b-d,
/// CR 700.2b-c, CR 101.4).
///
/// `apnap` lists the players in APNAP order (the active player first, then turn order,
/// CR 101.4); `waiting` is in creation order. The two-part process of CR 603.3b: first every
/// trigger whose condition is not another ability triggering, then the rest; within each part
/// the players in `apnap` order, a player without a trigger in that part skipped. A player with
/// two or more triggers in a part orders them with one `ORDER_SIMULTANEOUS_TRIGGERS` request
/// (the first answer index goes on the stack first); with fewer, nothing is asked (a recorded
/// inference). Then each trigger in that order, as it goes on the stack: an unfillable required
/// ability-level target slot removes it (CR 603.3d); if modal, only modes whose own required
/// slots can be filled are offered (CR 603.3c, CR 700.2b), too few removes it, otherwise one
/// `CHOOSE_MODES` request; then one `CHOOSE_TARGETS` request for the ability-level slots and the
/// chosen modes' slots (CR 700.2c), if there are any. The optional "may" is not asked here
/// (CR 603.5; see [`ask_optional_trigger`]). The state-based-action loop of CR 603.3b is the
/// caller's.
///
/// A pure function of (`env`, `apnap`, `waiting`, the answers): it re-runs from scratch after
/// each accepted answer (the choice module's suspension model).
///
/// # Errors
/// [`Interrupt::Pending`] (a player must decide), or [`Interrupt::Error`] with
/// [`ChoiceError::Procedure`] if a trigger's controller is not in `apnap`, `apnap` lists a
/// player twice, or an answer has the wrong shape; or any error of [`ChoiceContext::ask`].
pub fn place_triggers<E: PlacementEnv + ?Sized>(
    env: &E,
    ctx: &mut ChoiceContext<'_>,
    apnap: &[PlayerId],
    waiting: Vec<PendingTrigger>,
) -> Result<TriggerPlacement, Interrupt> {
    let mut seen = BTreeSet::new();
    for p in apnap {
        if !seen.insert(*p) {
            return Err(procedure_error(format!(
                "player {} is listed twice in the APNAP order",
                p.get()
            )));
        }
    }
    if let Some(t) = waiting.iter().find(|t| !seen.contains(&t.controller)) {
        return Err(procedure_error(format!(
            "trigger controller player {} is not in the APNAP order",
            t.controller.get()
        )));
    }
    let mut placement = TriggerPlacement::default();
    // CR 603.3b: part 1, then part 2 (abilities that trigger on another ability triggering).
    for part in [false, true] {
        // CR 101.4: players in APNAP order.
        for &player in apnap {
            let group: Vec<PendingTrigger> = waiting
                .iter()
                .filter(|t| t.controller == player && t.responds_to_trigger == part)
                .cloned()
                .collect();
            if group.is_empty() {
                continue;
            }
            // CR 603.3b: each player puts their triggers on the stack in any order they choose.
            let order = if group.len() >= 2 {
                ask_order(ctx, player, &group)?
            } else {
                vec![0]
            };
            let mut slots: Vec<Option<PendingTrigger>> = group.into_iter().map(Some).collect();
            for i in order {
                if let Some(trigger) = slots.get_mut(i).and_then(Option::take) {
                    put_one(env, ctx, trigger, &mut placement)?;
                }
            }
        }
    }
    Ok(placement)
}

/// The "may" choice of an optional triggered ability, made as it resolves (REQ-EVENT-002,
/// CR 603.5); the resolution unit calls it.
///
/// Not optional: `Ok(true)` with no request. Otherwise one `CHOOSE_OPTIONAL_TRIGGER` request
/// to the controller (reason `CR 603.5`, source the trigger's source, the trigger's
/// [`PendingTrigger::description`], options `YES` and `NO`, default `NO`: declining is the safe
/// default); `Ok(true)` iff the answer is `YES`.
///
/// # Errors
/// [`Interrupt::Pending`], or any error of [`ChoiceContext::ask`].
pub fn ask_optional_trigger(
    ctx: &mut ChoiceContext<'_>,
    trigger: &PendingTrigger,
) -> Result<bool, Interrupt> {
    if !trigger.optional {
        return Ok(true);
    }
    let request = PendingChoiceRequest {
        request_id: ctx.next_request_id(),
        chooser: trigger.controller,
        choice_type: ChoiceType::ChooseOptionalTrigger,
        reason: ChoiceReason {
            cr: "CR 603.5".to_owned(),
            source: trigger.source,
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::No,
        options: ChoiceOptions::Enumerated(vec![ChoiceOption::Yes, ChoiceOption::No]),
        payload: ChoicePayload::ChooseOptionalTrigger {
            description: trigger.description(),
        },
    };
    Ok(matches!(ctx.ask(request)?, ChoiceResponse::Yes))
}
