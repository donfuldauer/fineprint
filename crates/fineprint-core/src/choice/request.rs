// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Pending choice requests and answer validation (REQ-NET-003, SRS 15.3).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::{CanonicalEncode, encode_to_vec, put_variant};
use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{ObjectId, PlayerId};

use super::{
    ChoiceResponse, ChoiceTarget, ChoiceType, ChoiceValue, MAX_CARD_NAME_BYTES, RequestId,
};

/// Why a request is asked (REQ-NET-003): a CR reference and the source object.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChoiceReason {
    /// The CR reference, for example `"CR 601.2b"`.
    pub cr: String,
    /// The object whose rule or ability asks, if any.
    pub source: Option<ObjectId>,
}

fineprint_base::impl_canonical_struct!(ChoiceReason { cr, source });

/// One pre-validated option of an enumerated request (REQ-NET-003).
///
/// For most types an option is one whole answer (a player, a value, `Yes`); for
/// `SELECT_OBJECTS_FROM_ZONE`, `CHOOSE_MODES`, `ORDER_CARDS` and `DIVIDE_VALUE` the options are
/// the building blocks an answer is made of (the candidates, the modes, the cards, the
/// recipients), and the request's `min`/`max` and payload say how they combine.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChoiceOption {
    /// An object (canonical tag 0).
    Object(ObjectId),
    /// A player (tag 1).
    Player(PlayerId),
    /// A text value (tag 2).
    Text(String),
    /// A number (tag 3).
    Number(i64),
    /// A mode index into the request's mode list (tag 4).
    Mode(u32),
    /// Yes (tag 5).
    Yes,
    /// No (tag 6).
    No,
}

impl CanonicalEncode for ChoiceOption {
    /// The tag given per variant, then the value.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            ChoiceOption::Object(o) => {
                put_variant(out, 0);
                o.encode_canonical(out);
            }
            ChoiceOption::Player(p) => {
                put_variant(out, 1);
                p.encode_canonical(out);
            }
            ChoiceOption::Text(s) => {
                put_variant(out, 2);
                s.encode_canonical(out);
            }
            ChoiceOption::Number(n) => {
                put_variant(out, 3);
                n.encode_canonical(out);
            }
            ChoiceOption::Mode(m) => {
                put_variant(out, 4);
                m.encode_canonical(out);
            }
            ChoiceOption::Yes => put_variant(out, 5),
            ChoiceOption::No => put_variant(out, 6),
        }
    }
}

/// The answer shape a [`ValidatorContract`] checks (REQ-NET-003). Each variant carries the
/// domain a remote client needs to pre-check an answer; the engine still validates every
/// answer itself.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValidatorRule {
    /// Tag 0: a set of distinct objects from `candidates`, `min..=max` of them.
    Subset {
        /// The objects an answer may contain (ascending ids).
        candidates: Vec<ObjectId>,
    },
    /// Tag 1: `min..=max` mode indices below `count`, distinct unless `repeats_allowed`.
    Modes {
        /// The number of modes.
        count: u64,
        /// Whether a mode may be chosen more than once.
        repeats_allowed: bool,
    },
    /// Tag 2: one integer in `min..=max`.
    IntegerRange,
    /// Tag 3: exactly one of `values`.
    OneOf {
        /// The allowed values.
        values: Vec<String>,
    },
    /// Tag 4: exactly one of `players`.
    OneOfPlayers {
        /// The allowed players (ascending ids).
        players: Vec<PlayerId>,
    },
    /// Tag 5: a permutation of `items` (all of them, each once), in `top`.
    Permutation {
        /// The cards to order (ascending ids).
        items: Vec<ObjectId>,
    },
    /// Tag 6: one amount per recipient, each at least its minimum, summing to `total`.
    Division {
        /// The amount to divide.
        total: u64,
        /// `(recipient, minimum)` pairs.
        recipients: Vec<(ChoiceTarget, u64)>,
    },
    /// Tag 7: a card name, checked for structure only: non-empty after trimming, at most
    /// `max_bytes` UTF-8 bytes, no control characters. Whether the name exists is the raising
    /// procedure's check (the engine does not hold the card database).
    CardName {
        /// The longest accepted name in bytes ([`MAX_CARD_NAME_BYTES`]).
        max_bytes: u64,
    },
    /// Tag 8: the domain is the request's [`ChoicePayload`] (types raised by procedures).
    Payload,
}

impl CanonicalEncode for ValidatorRule {
    /// The tag given per variant, then the fields in declaration order.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            ValidatorRule::Subset { candidates } => {
                put_variant(out, 0);
                candidates.encode_canonical(out);
            }
            ValidatorRule::Modes {
                count,
                repeats_allowed,
            } => {
                put_variant(out, 1);
                count.encode_canonical(out);
                repeats_allowed.encode_canonical(out);
            }
            ValidatorRule::IntegerRange => put_variant(out, 2),
            ValidatorRule::OneOf { values } => {
                put_variant(out, 3);
                values.encode_canonical(out);
            }
            ValidatorRule::OneOfPlayers { players } => {
                put_variant(out, 4);
                players.encode_canonical(out);
            }
            ValidatorRule::Permutation { items } => {
                put_variant(out, 5);
                items.encode_canonical(out);
            }
            ValidatorRule::Division { total, recipients } => {
                put_variant(out, 6);
                total.encode_canonical(out);
                recipients.encode_canonical(out);
            }
            ValidatorRule::CardName { max_bytes } => {
                put_variant(out, 7);
                max_bytes.encode_canonical(out);
            }
            ValidatorRule::Payload => put_variant(out, 8),
        }
    }
}

/// The validator contract of a request with [`ENUMERATION_LIMIT`](super::ENUMERATION_LIMIT) or
/// more valid answers (REQ-NET-003, SRS 15.3): the client submits an answer and the engine
/// accepts it or rejects it with an explanation.
///
/// `min` and `max` equal the request's `min` and `max`: a procedure that tightens the request
/// bounds must update the validator contract bounds in the same request;
/// [`ChoiceContext::ask`](super::ChoiceContext::ask) rejects a mismatch.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValidatorContract {
    /// The request's `min` (a count, or the low bound of an [`ValidatorRule::IntegerRange`]).
    pub min: u64,
    /// The request's `max` (a count, or the high bound of an [`ValidatorRule::IntegerRange`]).
    pub max: u64,
    /// What a valid answer looks like, with its domain.
    pub rule: ValidatorRule,
}

fineprint_base::impl_canonical_struct!(ValidatorContract { min, max, rule });

/// The options of a request (REQ-NET-003, SRS 15.3).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChoiceOptions {
    /// Tag 0: fewer than [`ENUMERATION_LIMIT`](super::ENUMERATION_LIMIT) valid answers: the
    /// pre-validated options (see [`ChoiceOption`] for what an item stands for).
    Enumerated(Vec<ChoiceOption>),
    /// Tag 1: [`ENUMERATION_LIMIT`](super::ENUMERATION_LIMIT) or more valid answers.
    Validator(ValidatorContract),
}

impl CanonicalEncode for ChoiceOptions {
    /// Tag `0` + the option list, or tag `1` + the contract.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            ChoiceOptions::Enumerated(items) => {
                put_variant(out, 0);
                items.encode_canonical(out);
            }
            ChoiceOptions::Validator(c) => {
                put_variant(out, 1);
                c.encode_canonical(out);
            }
        }
    }
}

/// Where an `ORDER_CARDS` answer may put cards (REQ-NET-003, CR 731.3a for graveyard order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrderDestination {
    /// The top of the library (tag 0).
    Top,
    /// The bottom of the library (tag 1).
    Bottom,
    /// The graveyard (tag 2).
    Graveyard,
}

impl CanonicalEncode for OrderDestination {
    /// One byte: the tag given per variant.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_variant(
            out,
            match self {
                OrderDestination::Top => 0,
                OrderDestination::Bottom => 1,
                OrderDestination::Graveyard => 2,
            },
        );
    }
}

/// One target slot of a `CHOOSE_TARGETS` request (CR 601.2c).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetSlot {
    /// Least number of targets in this slot.
    pub min: u64,
    /// Greatest number of targets in this slot.
    pub max: u64,
    /// The legal targets.
    pub legal: Vec<ChoiceTarget>,
    /// The slot's current targets (for a copy that may keep them, CR 707.10c); empty otherwise.
    pub current: Vec<ChoiceTarget>,
}

fineprint_base::impl_canonical_struct!(TargetSlot {
    min,
    max,
    legal,
    current
});

/// The value domain of a `CHOOSE_OPTION_OR_VALUE` request.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AllowedValues {
    /// Tag 0: any integer in the request's `min..=max` (answered with [`ChoiceValue::Number`]).
    Range,
    /// Tag 1: one of these texts (answered with [`ChoiceValue::Text`]).
    Texts(Vec<String>),
}

impl CanonicalEncode for AllowedValues {
    /// Tag `0`, or tag `1` + the list.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            AllowedValues::Range => put_variant(out, 0),
            AllowedValues::Texts(t) => {
                put_variant(out, 1);
                t.encode_canonical(out);
            }
        }
    }
}

/// The type-specific payload of a request: the "Payload" column of the SRS 15.3 table
/// (REQ-NET-003). One variant per [`ChoiceType`], in table order; the canonical tag of a variant
/// equals [`ChoiceType::tag`] of its type. Index-based answers (modes, costs, triggers,
/// replacement effects, shortcut steps) index into the lists here.
///
/// Descriptions are opaque display strings supplied by the raising procedure; it must not put
/// information hidden from the chooser into them (R12).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChoicePayload {
    /// `PRIORITY_ACTION`.
    PriorityAction {
        /// Whether passing priority is allowed.
        can_pass: bool,
        /// Whether a shortcut may be proposed (its response arrives with the shortcut unit).
        can_propose_shortcut: bool,
        /// Every legal action, each as the exact response that takes it (`PLAY_LAND`,
        /// `INITIATE_CAST`, `ACTIVATE`, `SPECIAL_ACTION`).
        actions: Vec<ChoiceResponse>,
    },
    /// `CHOOSE_MODES` (CR 601.2b).
    ChooseModes {
        /// One label per mode.
        labels: Vec<String>,
        /// Whether a mode may be chosen more than once.
        repeats_allowed: bool,
        /// Per-mode cost descriptions (spree, tiered); empty when the modes have no own cost.
        mode_costs: Vec<String>,
    },
    /// `CHOOSE_COSTS_AND_X` (CR 601.2b).
    ChooseCostsAndX {
        /// Alternative costs (at most one may be chosen).
        alternative_costs: Vec<String>,
        /// Additional costs (any distinct subset).
        additional_costs: Vec<String>,
        /// The allowed X range `(low, high)` inclusive, if the cost has X.
        x_range: Option<(u64, u64)>,
        /// Per cost-symbol slot (hybrid/Phyrexian), its options; one must be chosen per slot.
        symbol_slots: Vec<Vec<String>>,
        /// Creatures that may be chosen for harmonize.
        harmonize_candidates: Vec<ObjectId>,
    },
    /// `CHOOSE_TARGETS` (CR 601.2c, CR 707.10c).
    ChooseTargets {
        /// The target slots, in order.
        slots: Vec<TargetSlot>,
        /// For copies: a slot may keep its current targets even if illegal.
        may_keep_illegal: bool,
    },
    /// `DIVIDE_VALUE` (CR 601.2d, CR 510.1c, CR 510.1d).
    DivideValue {
        /// The amount to divide.
        total: u64,
        /// `(recipient, minimum)` pairs, in order.
        recipients: Vec<(ChoiceTarget, u64)>,
    },
    /// `PAY_MANA_AND_COSTS` (CR 601.2g, CR 601.2h).
    PayManaAndCosts {
        /// The locked total cost, as a display string.
        locked_total: String,
        /// Payment-method candidates (convoke, delve, improvise, ...): `(display label, the
        /// objects that may be used with that method)`. A `PAY_COSTS` answer names a method by
        /// its index here and may use only that method's objects.
        methods: Vec<(String, Vec<ObjectId>)>,
        /// Mana abilities that may be activated: `(source, ability id)`.
        mana_abilities: Vec<(ObjectId, String)>,
        /// Whether an auto-pay suggestion exists (answered with `PayCosts { auto_pay: true, .. }`).
        auto_pay: bool,
        /// Whether the payment is optional (`DECLINE_OPTIONAL_PAYMENT` allowed).
        optional: bool,
        /// Whether the action may be cancelled (`CANCEL_AND_ROLLBACK` allowed).
        cancellable: bool,
    },
    /// `DECLARE_ATTACKERS` (CR 508.1).
    DeclareAttackers {
        /// Each eligible attacker with the legal attack targets for it.
        attackers: Vec<(ObjectId, Vec<ChoiceTarget>)>,
        /// The maximum number of requirements that can be satisfied.
        max_requirements: u64,
    },
    /// `DECLARE_BLOCKERS` (CR 509.1).
    DeclareBlockers {
        /// Each eligible blocker, the attackers it can block, and how many it may block.
        blockers: Vec<(ObjectId, Vec<ObjectId>, u64)>,
    },
    /// `ORDER_SIMULTANEOUS_TRIGGERS` (CR 603.3b).
    OrderSimultaneousTriggers {
        /// This player's waiting triggers, as descriptions.
        triggers: Vec<String>,
    },
    /// `CHOOSE_OPTIONAL_TRIGGER`.
    ChooseOptionalTrigger {
        /// What is optional.
        description: String,
    },
    /// `CHOOSE_REPLACEMENT_EFFECT` (CR 616.1).
    ChooseReplacementEffect {
        /// The event being replaced.
        event: String,
        /// The applicable effects of the current tier.
        effects: Vec<String>,
    },
    /// `SELECT_OBJECTS_FROM_ZONE`.
    SelectObjectsFromZone {
        /// The candidates (ascending ids).
        candidates: Vec<ObjectId>,
    },
    /// `ORDER_CARDS` (CR 731.3a).
    OrderCards {
        /// The cards to order (ascending ids).
        cards: Vec<ObjectId>,
        /// Where cards may go.
        destinations: Vec<OrderDestination>,
    },
    /// `CHOOSE_PLAYER` (CR 800.4g).
    ChoosePlayer {
        /// The legal players (ascending ids).
        players: Vec<PlayerId>,
    },
    /// `CHOOSE_OPTION_OR_VALUE`.
    ChooseOptionOrValue {
        /// What is chosen (`NUMBER`, `COLOR`, `CREATURE_TYPE`, `PILE`, or another kind).
        option_type: String,
        /// The allowed values.
        allowed: AllowedValues,
    },
    /// `CHOOSE_CARD_NAME` (CR 712.16, CR 723.5): the domain is the validator contract.
    ChooseCardName,
    /// `CHOOSE_UNTAP_SET` (CR 502.3).
    ChooseUntapSet {
        /// The permanents that may be untapped.
        permanents: Vec<ObjectId>,
    },
    /// `SHORTCUT_RESPONSE` (CR 733.2b).
    ShortcutResponse {
        /// The proposed sequence, as descriptions.
        sequence: Vec<String>,
    },
    /// `MULLIGAN_DECISION` (CR 103.5).
    MulliganDecision {
        /// The hand.
        hand: Vec<ObjectId>,
        /// Mulligans taken so far.
        mulligans_taken: u64,
        /// How many cards a kept hand puts on the bottom.
        to_bottom: u64,
    },
    /// `PREGAME_ACTION` (CR 103.6).
    PregameAction {
        /// The eligible cards.
        eligible: Vec<ObjectId>,
    },
}

impl ChoicePayload {
    /// The choice type this payload belongs to.
    #[must_use]
    pub const fn choice_type(&self) -> ChoiceType {
        match self {
            ChoicePayload::PriorityAction { .. } => ChoiceType::PriorityAction,
            ChoicePayload::ChooseModes { .. } => ChoiceType::ChooseModes,
            ChoicePayload::ChooseCostsAndX { .. } => ChoiceType::ChooseCostsAndX,
            ChoicePayload::ChooseTargets { .. } => ChoiceType::ChooseTargets,
            ChoicePayload::DivideValue { .. } => ChoiceType::DivideValue,
            ChoicePayload::PayManaAndCosts { .. } => ChoiceType::PayManaAndCosts,
            ChoicePayload::DeclareAttackers { .. } => ChoiceType::DeclareAttackers,
            ChoicePayload::DeclareBlockers { .. } => ChoiceType::DeclareBlockers,
            ChoicePayload::OrderSimultaneousTriggers { .. } => {
                ChoiceType::OrderSimultaneousTriggers
            }
            ChoicePayload::ChooseOptionalTrigger { .. } => ChoiceType::ChooseOptionalTrigger,
            ChoicePayload::ChooseReplacementEffect { .. } => ChoiceType::ChooseReplacementEffect,
            ChoicePayload::SelectObjectsFromZone { .. } => ChoiceType::SelectObjectsFromZone,
            ChoicePayload::OrderCards { .. } => ChoiceType::OrderCards,
            ChoicePayload::ChoosePlayer { .. } => ChoiceType::ChoosePlayer,
            ChoicePayload::ChooseOptionOrValue { .. } => ChoiceType::ChooseOptionOrValue,
            ChoicePayload::ChooseCardName => ChoiceType::ChooseCardName,
            ChoicePayload::ChooseUntapSet { .. } => ChoiceType::ChooseUntapSet,
            ChoicePayload::ShortcutResponse { .. } => ChoiceType::ShortcutResponse,
            ChoicePayload::MulliganDecision { .. } => ChoiceType::MulliganDecision,
            ChoicePayload::PregameAction { .. } => ChoiceType::PregameAction,
        }
    }
}

impl CanonicalEncode for ChoicePayload {
    /// [`ChoiceType::tag`] of [`ChoicePayload::choice_type`], then the fields in declaration
    /// order.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_variant(out, self.choice_type().tag());
        match self {
            ChoicePayload::PriorityAction {
                can_pass,
                can_propose_shortcut,
                actions,
            } => {
                can_pass.encode_canonical(out);
                can_propose_shortcut.encode_canonical(out);
                actions.encode_canonical(out);
            }
            ChoicePayload::ChooseModes {
                labels,
                repeats_allowed,
                mode_costs,
            } => {
                labels.encode_canonical(out);
                repeats_allowed.encode_canonical(out);
                mode_costs.encode_canonical(out);
            }
            ChoicePayload::ChooseCostsAndX {
                alternative_costs,
                additional_costs,
                x_range,
                symbol_slots,
                harmonize_candidates,
            } => {
                alternative_costs.encode_canonical(out);
                additional_costs.encode_canonical(out);
                x_range.encode_canonical(out);
                symbol_slots.encode_canonical(out);
                harmonize_candidates.encode_canonical(out);
            }
            ChoicePayload::ChooseTargets {
                slots,
                may_keep_illegal,
            } => {
                slots.encode_canonical(out);
                may_keep_illegal.encode_canonical(out);
            }
            ChoicePayload::DivideValue { total, recipients } => {
                total.encode_canonical(out);
                recipients.encode_canonical(out);
            }
            ChoicePayload::PayManaAndCosts {
                locked_total,
                methods,
                mana_abilities,
                auto_pay,
                optional,
                cancellable,
            } => {
                locked_total.encode_canonical(out);
                methods.encode_canonical(out);
                mana_abilities.encode_canonical(out);
                auto_pay.encode_canonical(out);
                optional.encode_canonical(out);
                cancellable.encode_canonical(out);
            }
            ChoicePayload::DeclareAttackers {
                attackers,
                max_requirements,
            } => {
                attackers.encode_canonical(out);
                max_requirements.encode_canonical(out);
            }
            ChoicePayload::DeclareBlockers { blockers } => blockers.encode_canonical(out),
            ChoicePayload::OrderSimultaneousTriggers { triggers } => {
                triggers.encode_canonical(out);
            }
            ChoicePayload::ChooseOptionalTrigger { description } => {
                description.encode_canonical(out);
            }
            ChoicePayload::ChooseReplacementEffect { event, effects } => {
                event.encode_canonical(out);
                effects.encode_canonical(out);
            }
            ChoicePayload::SelectObjectsFromZone { candidates } => {
                candidates.encode_canonical(out);
            }
            ChoicePayload::OrderCards {
                cards,
                destinations,
            } => {
                cards.encode_canonical(out);
                destinations.encode_canonical(out);
            }
            ChoicePayload::ChoosePlayer { players } => players.encode_canonical(out),
            ChoicePayload::ChooseOptionOrValue {
                option_type,
                allowed,
            } => {
                option_type.encode_canonical(out);
                allowed.encode_canonical(out);
            }
            ChoicePayload::ChooseCardName => {}
            ChoicePayload::ChooseUntapSet { permanents } => permanents.encode_canonical(out),
            ChoicePayload::ShortcutResponse { sequence } => sequence.encode_canonical(out),
            ChoicePayload::MulliganDecision {
                hand,
                mulligans_taken,
                to_bottom,
            } => {
                hand.encode_canonical(out);
                mulligans_taken.encode_canonical(out);
                to_bottom.encode_canonical(out);
            }
            ChoicePayload::PregameAction { eligible } => eligible.encode_canonical(out),
        }
    }
}

/// A decision the engine waits for (REQ-NET-003, SRS 15.3).
///
/// Generated from a [`ChoiceSpec`](fineprint_dsl::choice::ChoiceSpec) by
/// [`generate_request`](super::generate_request), or built directly by the procedure that
/// raises one of the other types. `min`/`max` are counts (how many items an answer contains),
/// except for an integer choice (`AllowedValues::Range`), where they bound the value.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PendingChoiceRequest {
    /// The request's id: its position in the procedure's ask sequence.
    pub request_id: RequestId,
    /// The player who decides.
    pub chooser: PlayerId,
    /// The SRS 15.3 row.
    pub choice_type: ChoiceType,
    /// Why it is asked.
    pub reason: ChoiceReason,
    /// The `ChoiceSpec`-derived lower bound (after clamping). A procedure that tightens it
    /// must update a [`ValidatorContract`]'s `min` in the same request;
    /// [`ChoiceContext::ask`](super::ChoiceContext::ask) rejects a mismatch.
    pub min: u64,
    /// The `ChoiceSpec`-derived upper bound (after clamping). A procedure that tightens it
    /// must update a [`ValidatorContract`]'s `max` in the same request;
    /// [`ChoiceContext::ask`](super::ChoiceContext::ask) rejects a mismatch.
    pub max: u64,
    /// Whether the choice made is hidden from other players.
    pub hidden: bool,
    /// The players the choice is revealed to.
    pub reveal_to: Vec<PlayerId>,
    /// The answer used when the caller times the chooser out; it always validates.
    pub default_on_timeout: ChoiceResponse,
    /// The pre-validated options or the validator contract.
    pub options: ChoiceOptions,
    /// The type-specific payload.
    pub payload: ChoicePayload,
}

fineprint_base::impl_canonical_struct!(PendingChoiceRequest {
    request_id,
    chooser,
    choice_type,
    reason,
    min,
    max,
    hidden,
    reveal_to,
    default_on_timeout,
    options,
    payload
});

type Check = Result<(), String>;

/// `len` must lie in `min..=max` (`what` names the counted items).
fn check_count(len: usize, min: u64, max: u64, what: &str) -> Check {
    let len = u64::try_from(len).unwrap_or(u64::MAX);
    if len < min {
        return Err(format!("{len} {what} chosen, at least {min} required"));
    }
    if len > max {
        return Err(format!("{len} {what} chosen, at most {max} allowed"));
    }
    Ok(())
}

/// Every item must be in `allowed` and appear at most once.
fn check_distinct_subset<T: Ord + Copy, D: Fn(T) -> String>(
    items: &[T],
    allowed: &[T],
    describe: D,
    domain: &str,
) -> Check {
    let allowed: BTreeSet<T> = allowed.iter().copied().collect();
    let mut seen = BTreeSet::new();
    for &item in items {
        if !allowed.contains(&item) {
            return Err(format!("{} is not one of the {domain}", describe(item)));
        }
        if !seen.insert(item) {
            return Err(format!("{} is chosen more than once", describe(item)));
        }
    }
    Ok(())
}

fn object(o: ObjectId) -> String {
    format!("object {}", o.get())
}

fn index(i: u32) -> String {
    format!("index {i}")
}

/// Structural card-name check (decision "#28 FP-027 — Card-name validation is structural").
fn check_card_name(name: &str) -> Check {
    if name.trim().is_empty() {
        return Err("the card name is empty".to_owned());
    }
    let len = u64::try_from(name.len()).unwrap_or(u64::MAX);
    if len > MAX_CARD_NAME_BYTES {
        return Err(format!(
            "the card name is {len} bytes long, at most {MAX_CARD_NAME_BYTES} allowed"
        ));
    }
    if name.chars().any(char::is_control) {
        return Err("the card name contains a control character".to_owned());
    }
    Ok(())
}

/// A permutation of `0..n` as `u32` indices.
fn check_index_permutation(order: &[u32], n: usize, what: &str) -> Check {
    if order.len() != n {
        return Err(format!(
            "{} {what} ordered, all {n} must be ordered exactly once",
            order.len()
        ));
    }
    let mut seen = BTreeSet::new();
    for &i in order {
        if usize::try_from(i).map_or(true, |i| i >= n) {
            return Err(format!(
                "{} is out of range: there are {n} {what}",
                index(i)
            ));
        }
        if !seen.insert(i) {
            return Err(format!("{} is ordered more than once", index(i)));
        }
    }
    Ok(())
}

fn index_below(i: u32, n: usize, what: &str) -> Check {
    if usize::try_from(i).map_or(true, |i| i >= n) {
        return Err(format!(
            "{} is out of range: there are {n} {what}",
            index(i)
        ));
    }
    Ok(())
}

impl PendingChoiceRequest {
    /// The SHA-256 of the canonical encoding (used to detect divergence on re-execution).
    #[must_use]
    pub fn digest(&self) -> Digest {
        sha256(&encode_to_vec(self))
    }

    /// Checks that `response` is a valid answer to this request (REQ-NET-003). Pure; never
    /// panics on any response (R12).
    ///
    /// The response variant must belong to [`PendingChoiceRequest::choice_type`]; the payload
    /// must belong to it too. Then, per type:
    ///
    /// - `SELECT_OBJECTS_FROM_ZONE`: distinct candidates, `min..=max` of them;
    /// - `CHOOSE_MODES`: indices below the mode count, distinct unless repeats are allowed,
    ///   `min..=max` of them;
    /// - `CHOOSE_OPTION_OR_VALUE`: a number in `min..=max` (range) or one of the texts;
    /// - `CHOOSE_CARD_NAME`: structure only (non-empty after trimming, at most
    ///   [`MAX_CARD_NAME_BYTES`] bytes, no control characters);
    /// - `CHOOSE_PLAYER`: one of the players; `CHOOSE_OPTIONAL_TRIGGER`: `YES` or `NO`;
    /// - `ORDER_CARDS`: the three lists together are a permutation of the cards; lists for
    ///   destinations the request does not offer are empty;
    /// - `DIVIDE_VALUE`: exactly one pair per recipient, each at least its minimum, summing to
    ///   the total;
    /// - `PRIORITY_ACTION`: pass only if allowed; any other action exactly as listed;
    /// - `CHOOSE_COSTS_AND_X`: indices in range, additional costs distinct, X in range (absent
    ///   without a range), one option per symbol slot, harmonize from the candidates;
    /// - `CHOOSE_TARGETS`: one list per slot, `min..=max` distinct legal targets each (current
    ///   targets also allowed when the copy may keep them);
    /// - `PAY_MANA_AND_COSTS`: method indices in range, each method's objects distinct and
    ///   eligible for that method, mana abilities as listed, auto-pay only if offered and alone,
    ///   decline only if optional, cancel only if cancellable;
    /// - `DECLARE_ATTACKERS`: distinct listed attackers, each with one of its targets;
    /// - `DECLARE_BLOCKERS`: distinct listed blockers, each blocking 1..=capacity distinct
    ///   attackers it can block;
    /// - `ORDER_SIMULTANEOUS_TRIGGERS`: a permutation of the trigger indices;
    /// - `CHOOSE_REPLACEMENT_EFFECT`: an index below the effect count;
    /// - `CHOOSE_UNTAP_SET`: distinct listed permanents, `min..=max` of them;
    /// - `SHORTCUT_RESPONSE`: `ACCEPT`, or `SHORTEN` at an index in the sequence;
    /// - `MULLIGAN_DECISION`: `TAKE_MULLIGAN`, or `KEEP_HAND` with exactly the required number
    ///   of distinct cards from the hand;
    /// - `PREGAME_ACTION`: distinct eligible cards.
    ///
    /// Deferred to the raising procedure: card-name membership in the database, whether a
    /// payment covers the cost, attack/block requirements and restrictions (menace, requirement
    /// maximisation), trample lethal floors, the legality of combined target sets. For a name
    /// absent from the database the raising procedure returns an explained error
    /// (`Interrupt::Error(ChoiceError::Invalid { request_id, explanation })`); it must not
    /// re-ask, because [`ChoiceSession::submit`](super::ChoiceSession::submit) would record the
    /// answer as accepted and replace the request.
    ///
    /// # Errors
    /// A non-empty explanation naming the violated constraint. It mentions only the answer's own
    /// items and the request's public bounds.
    pub fn validate(&self, response: &ChoiceResponse) -> Result<(), String> {
        if response.choice_type() != self.choice_type {
            return Err(format!(
                "{} does not answer a {} request",
                response.wire_name(),
                self.choice_type.as_str()
            ));
        }
        if self.payload.choice_type() != self.choice_type {
            return Err(format!(
                "malformed request: its payload belongs to {}, not {}",
                self.payload.choice_type().as_str(),
                self.choice_type.as_str()
            ));
        }
        let (min, max) = (self.min, self.max);
        match (&self.payload, response) {
            (
                ChoicePayload::SelectObjectsFromZone { candidates },
                ChoiceResponse::SubmitObjectSelection { objects },
            ) => {
                check_count(objects.len(), min, max, "objects")?;
                check_distinct_subset(objects, candidates, object, "candidates")
            }
            (
                ChoicePayload::ChooseModes {
                    labels,
                    repeats_allowed,
                    ..
                },
                ChoiceResponse::SelectModes { modes },
            ) => {
                check_count(modes.len(), min, max, "modes")?;
                let mut seen = BTreeSet::new();
                for &m in modes {
                    index_below(m, labels.len(), "modes")?;
                    if !seen.insert(m) && !repeats_allowed {
                        return Err(format!("mode {m} is chosen more than once"));
                    }
                }
                Ok(())
            }
            (
                ChoicePayload::ChooseOptionOrValue { allowed, .. },
                ChoiceResponse::SubmitChoiceValue { value },
            ) => match (allowed, value) {
                (AllowedValues::Range, ChoiceValue::Number(v)) => {
                    let v128 = i128::from(*v);
                    if v128 < i128::from(min) || v128 > i128::from(max) {
                        Err(format!("{v} is outside the allowed range {min}..={max}"))
                    } else {
                        Ok(())
                    }
                }
                (AllowedValues::Range, ChoiceValue::Text(_)) => {
                    Err("a number is required, not a text".to_owned())
                }
                (AllowedValues::Texts(texts), ChoiceValue::Text(t)) => {
                    if texts.iter().any(|x| x == t) {
                        Ok(())
                    } else {
                        Err(format!("{t:?} is not one of the allowed values"))
                    }
                }
                (AllowedValues::Texts(_), ChoiceValue::Number(_)) => {
                    Err("one of the allowed texts is required, not a number".to_owned())
                }
            },
            (ChoicePayload::ChooseCardName, ChoiceResponse::SubmitCardName { name }) => {
                check_card_name(name)
            }
            (ChoicePayload::ChoosePlayer { players }, ChoiceResponse::SubmitPlayer { player }) => {
                if players.contains(player) {
                    Ok(())
                } else {
                    Err(format!(
                        "player {} is not one of the legal players",
                        player.get()
                    ))
                }
            }
            (
                ChoicePayload::ChooseOptionalTrigger { .. },
                ChoiceResponse::Yes | ChoiceResponse::No,
            ) => Ok(()),
            (
                ChoicePayload::OrderCards {
                    cards,
                    destinations,
                },
                ChoiceResponse::SubmitOrdering {
                    top,
                    bottom,
                    graveyard,
                },
            ) => {
                for (dest, list, name) in [
                    (OrderDestination::Top, top, "top"),
                    (OrderDestination::Bottom, bottom, "bottom"),
                    (OrderDestination::Graveyard, graveyard, "graveyard"),
                ] {
                    if !list.is_empty() && !destinations.contains(&dest) {
                        return Err(format!("cards may not be put on the {name} list here"));
                    }
                }
                let total = top
                    .len()
                    .saturating_add(bottom.len())
                    .saturating_add(graveyard.len());
                if total != cards.len() {
                    return Err(format!(
                        "{total} cards placed, all {} cards must be placed exactly once",
                        cards.len()
                    ));
                }
                let all: Vec<ObjectId> =
                    top.iter().chain(bottom).chain(graveyard).copied().collect();
                check_distinct_subset(&all, cards, object, "cards to order")
            }
            (
                ChoicePayload::DivideValue { total, recipients },
                ChoiceResponse::SubmitDivision { assignments },
            ) => validate_division(*total, recipients, assignments),
            (
                ChoicePayload::PriorityAction {
                    can_pass, actions, ..
                },
                r,
            ) => {
                if matches!(r, ChoiceResponse::PassPriority) {
                    if *can_pass {
                        Ok(())
                    } else {
                        Err("passing priority is not allowed here".to_owned())
                    }
                } else if actions.contains(r) {
                    Ok(())
                } else {
                    Err(format!(
                        "this {} is not one of the legal actions",
                        r.wire_name()
                    ))
                }
            }
            (
                ChoicePayload::ChooseCostsAndX {
                    alternative_costs,
                    additional_costs,
                    x_range,
                    symbol_slots,
                    harmonize_candidates,
                },
                ChoiceResponse::ConfigureCosts {
                    alternative,
                    additional,
                    x,
                    symbol_options,
                    harmonize,
                },
            ) => {
                if let Some(a) = alternative {
                    index_below(*a, alternative_costs.len(), "alternative costs")?;
                }
                let mut seen = BTreeSet::new();
                for &a in additional {
                    index_below(a, additional_costs.len(), "additional costs")?;
                    if !seen.insert(a) {
                        return Err(format!("additional cost {a} is chosen more than once"));
                    }
                }
                match (x_range, x) {
                    (None, None) => {}
                    (None, Some(_)) => return Err("this cost has no X".to_owned()),
                    (Some(_), None) => return Err("a value for X is required".to_owned()),
                    (Some((lo, hi)), Some(v)) => {
                        if v < lo || v > hi {
                            return Err(format!("X = {v} is outside the range {lo}..={hi}"));
                        }
                    }
                }
                if symbol_options.len() != symbol_slots.len() {
                    return Err(format!(
                        "{} cost-symbol options given, {} required (one per slot)",
                        symbol_options.len(),
                        symbol_slots.len()
                    ));
                }
                for (&o, slot) in symbol_options.iter().zip(symbol_slots) {
                    index_below(o, slot.len(), "options in a cost-symbol slot")?;
                }
                if let Some(h) = harmonize
                    && !harmonize_candidates.contains(h)
                {
                    return Err(format!("{} cannot be chosen for harmonize", object(*h)));
                }
                Ok(())
            }
            (
                ChoicePayload::ChooseTargets {
                    slots,
                    may_keep_illegal,
                },
                ChoiceResponse::SelectTargets { targets },
            ) => {
                if targets.len() != slots.len() {
                    return Err(format!(
                        "{} target lists given, {} required (one per slot)",
                        targets.len(),
                        slots.len()
                    ));
                }
                for (i, (chosen, slot)) in targets.iter().zip(slots).enumerate() {
                    check_count(chosen.len(), slot.min, slot.max, "targets")
                        .map_err(|e| format!("slot {i}: {e}"))?;
                    let mut allowed = slot.legal.clone();
                    if *may_keep_illegal {
                        allowed.extend(slot.current.iter().copied());
                    }
                    check_distinct_subset(chosen, &allowed, |t| t.to_string(), "legal targets")
                        .map_err(|e| format!("slot {i}: {e}"))?;
                }
                Ok(())
            }
            (
                ChoicePayload::PayManaAndCosts {
                    methods: method_list,
                    mana_abilities: ability_list,
                    auto_pay: auto_offered,
                    optional,
                    cancellable,
                    ..
                },
                r,
            ) => match r {
                ChoiceResponse::DeclineOptionalPayment if !optional => {
                    Err("this payment is not optional".to_owned())
                }
                ChoiceResponse::CancelAndRollback if !cancellable => {
                    Err("this action cannot be cancelled".to_owned())
                }
                ChoiceResponse::PayCosts {
                    auto_pay,
                    methods,
                    mana_abilities,
                } => {
                    if *auto_pay {
                        if !auto_offered {
                            return Err("no auto-pay suggestion is offered".to_owned());
                        }
                        if !methods.is_empty() || !mana_abilities.is_empty() {
                            return Err(
                                "auto-pay must not be combined with explicit payments".to_owned()
                            );
                        }
                    }
                    for (m, objects) in methods {
                        index_below(*m, method_list.len(), "payment methods")?;
                        let eligible = usize::try_from(*m)
                            .ok()
                            .and_then(|i| method_list.get(i))
                            .map_or(&[][..], |(_, eligible)| eligible.as_slice());
                        check_distinct_subset(
                            objects,
                            eligible,
                            object,
                            &format!("objects eligible for payment method {m}"),
                        )?;
                    }
                    for ability in mana_abilities {
                        if !ability_list.contains(ability) {
                            return Err(format!(
                                "the mana ability {:?} of {} is not available",
                                ability.1,
                                object(ability.0)
                            ));
                        }
                    }
                    Ok(())
                }
                _ => Ok(()),
            },
            (
                ChoicePayload::DeclareAttackers { attackers, .. },
                ChoiceResponse::SubmitAttackers { attacks },
            ) => {
                let mut seen = BTreeSet::new();
                for (a, target) in attacks {
                    let Some((_, legal)) = attackers.iter().find(|(id, _)| id == a) else {
                        return Err(format!("{} is not an eligible attacker", object(*a)));
                    };
                    if !seen.insert(*a) {
                        return Err(format!("{} attacks more than once", object(*a)));
                    }
                    if !legal.contains(target) {
                        return Err(format!("{} cannot attack {target}", object(*a)));
                    }
                }
                Ok(())
            }
            (
                ChoicePayload::DeclareBlockers { blockers },
                ChoiceResponse::SubmitBlockers { blocks },
            ) => {
                let mut seen = BTreeSet::new();
                for (b, blocked) in blocks {
                    let Some((_, can_block, capacity)) = blockers.iter().find(|(id, _, _)| id == b)
                    else {
                        return Err(format!("{} is not an eligible blocker", object(*b)));
                    };
                    if !seen.insert(*b) {
                        return Err(format!("{} is listed as a blocker twice", object(*b)));
                    }
                    check_count(blocked.len(), 1, *capacity, "attackers blocked")
                        .map_err(|e| format!("{}: {e}", object(*b)))?;
                    check_distinct_subset(blocked, can_block, object, "attackers it can block")
                        .map_err(|e| format!("{}: {e}", object(*b)))?;
                }
                Ok(())
            }
            (
                ChoicePayload::OrderSimultaneousTriggers { triggers },
                ChoiceResponse::OrderTriggers { order },
            ) => check_index_permutation(order, triggers.len(), "triggers"),
            (
                ChoicePayload::ChooseReplacementEffect { effects, .. },
                ChoiceResponse::SelectReplacement { index },
            ) => index_below(*index, effects.len(), "applicable effects"),
            (
                ChoicePayload::ChooseUntapSet { permanents },
                ChoiceResponse::SubmitSet { objects },
            ) => {
                check_count(objects.len(), min, max, "permanents")?;
                check_distinct_subset(objects, permanents, object, "permanents offered")
            }
            (ChoicePayload::ShortcutResponse { sequence }, r) => match r {
                ChoiceResponse::Shorten { at_index } => {
                    let n = u64::try_from(sequence.len()).unwrap_or(u64::MAX);
                    if *at_index < n {
                        Ok(())
                    } else {
                        Err(format!(
                            "index {at_index} is outside the proposed sequence of {n} steps"
                        ))
                    }
                }
                _ => Ok(()),
            },
            (
                ChoicePayload::MulliganDecision {
                    hand, to_bottom, ..
                },
                r,
            ) => match r {
                ChoiceResponse::KeepHand { bottom } => {
                    check_count(bottom.len(), *to_bottom, *to_bottom, "cards to the bottom")?;
                    check_distinct_subset(bottom, hand, object, "cards in hand")
                }
                _ => Ok(()),
            },
            (
                ChoicePayload::PregameAction { eligible },
                ChoiceResponse::SubmitPregame { objects },
            ) => check_distinct_subset(objects, eligible, object, "eligible cards"),
            // The type checks above make every remaining pairing impossible; reject rather
            // than accept if a future variant is added without a rule.
            _ => Err(format!(
                "{} cannot be checked against a {} request",
                response.wire_name(),
                self.choice_type.as_str()
            )),
        }
    }
}

/// `DIVIDE_VALUE`: exactly one pair per recipient, each at least its minimum, summing to
/// `total` (CR 601.2d).
fn validate_division(
    total: u64,
    recipients: &[(ChoiceTarget, u64)],
    assignments: &[(ChoiceTarget, u64)],
) -> Check {
    if assignments.len() != recipients.len() {
        return Err(format!(
            "{} assignments given, exactly one per recipient ({}) required",
            assignments.len(),
            recipients.len()
        ));
    }
    let floors: BTreeMap<ChoiceTarget, u64> = recipients.iter().copied().collect();
    let mut seen = BTreeSet::new();
    let mut sum: u64 = 0;
    for (target, amount) in assignments {
        let Some(floor) = floors.get(target) else {
            return Err(format!("{target} is not a recipient"));
        };
        if !seen.insert(*target) {
            return Err(format!("{target} is assigned more than once"));
        }
        if amount < floor {
            return Err(format!(
                "{target} is assigned {amount}, at least {floor} required"
            ));
        }
        sum = sum
            .checked_add(*amount)
            .ok_or_else(|| format!("the assignments sum to more than the total {total}"))?;
    }
    if sum != total {
        return Err(format!(
            "the assignments sum to {sum}, they must sum to {total}"
        ));
    }
    Ok(())
}
