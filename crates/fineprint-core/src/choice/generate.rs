// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Automatic generation of pending choice requests from choice specs (REQ-DSL-011, SRS
//! §14.4.6; REQ-NET-003, SRS 15.3).
//!
//! A [`ChoiceSpec`]'s selectors and value expressions are unevaluated; the procedure that raises
//! the choice evaluates them and passes the results as a [`ResolvedChoice`]. Generation maps the
//! spec's kind to a [`ChoiceType`], checks the resolved candidates and bounds, counts the
//! distinct valid answers (saturating, capped at [`ENUMERATION_LIMIT`]) and picks enumerated
//! options or a validator contract. Nothing here depends on a card (R4).

use std::collections::BTreeSet;

use fineprint_base::encode::{CanonicalEncode, put_variant};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_dsl::choice::{ChoiceKind, ChoiceSpec};

use super::{
    AllowedValues, ChoiceError, ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason,
    ChoiceResponse, ChoiceTarget, ChoiceType, ChoiceValue, ENUMERATION_LIMIT, MAX_CARD_NAME_BYTES,
    OrderDestination, PendingChoiceRequest, RequestId, ValidatorContract, ValidatorRule,
};

/// The evaluated candidates of a choice (REQ-DSL-011).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolvedCandidates {
    /// Tag 0: no candidates (`NUMBER`, `CARD_NAME`, `YES_NO`, `DIVISION`).
    None,
    /// Tag 1: distinct objects, in any order: generation sorts them ascending (`OBJECTS`,
    /// `ORDER`).
    Objects(Vec<ObjectId>),
    /// Tag 2: distinct players, in any order: generation sorts them ascending (`PLAYER`).
    Players(Vec<PlayerId>),
    /// Tag 3: labels or values, distinct and non-empty, in the given order (`MODE`, `COLOR`,
    /// `CREATURE_TYPE`, `PILE`).
    Strings(Vec<String>),
}

impl CanonicalEncode for ResolvedCandidates {
    /// The tag given per variant, then the list.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            ResolvedCandidates::None => put_variant(out, 0),
            ResolvedCandidates::Objects(v) => {
                put_variant(out, 1);
                v.encode_canonical(out);
            }
            ResolvedCandidates::Players(v) => {
                put_variant(out, 2);
                v.encode_canonical(out);
            }
            ResolvedCandidates::Strings(v) => {
                put_variant(out, 3);
                v.encode_canonical(out);
            }
        }
    }
}

/// The evaluated division of a `DIVISION` choice (CR 601.2d, CR 510.1c, CR 510.1d).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolvedDivision {
    /// The amount to divide.
    pub total: u64,
    /// `(recipient, minimum)` pairs, distinct recipients, in order (the first recipient gets the
    /// remainder in the default answer).
    pub recipients: Vec<(ChoiceTarget, u64)>,
}

fineprint_base::impl_canonical_struct!(ResolvedDivision { total, recipients });

/// A choice spec's evaluated values (REQ-DSL-011): what the raising procedure computed from the
/// spec's selectors and value expressions.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolvedChoice {
    /// The player who decides (from `chooser`).
    pub chooser: PlayerId,
    /// The evaluated candidates.
    pub candidates: ResolvedCandidates,
    /// The evaluated `min` (0 for an "up to" spec).
    pub min: u64,
    /// The evaluated `max`.
    pub max: u64,
    /// The evaluated `revealTo` players.
    pub reveal_to: Vec<PlayerId>,
    /// The evaluated division (`DIVISION` only).
    pub division: Option<ResolvedDivision>,
    /// A default answer that overrides the generated one (it must validate).
    pub default: Option<ChoiceResponse>,
    /// Why the choice is asked.
    pub reason: ChoiceReason,
}

fineprint_base::impl_canonical_struct!(ResolvedChoice {
    chooser,
    candidates,
    min,
    max,
    reveal_to,
    division,
    default,
    reason
});

/// The result of [`generate_request`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(
    clippy::large_enum_variant,
    reason = "the variant layout is the pinned public API (FP-027); values are short-lived"
)]
pub enum Generated {
    /// Tag 0: a player decides: ask this request.
    Ask(PendingChoiceRequest),
    /// Tag 1: the spec says `random`: no request; the procedure draws from the state's PRNG.
    Random,
}

impl CanonicalEncode for Generated {
    /// Tag `0` + the request, or tag `1`.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Generated::Ask(r) => {
                put_variant(out, 0);
                r.encode_canonical(out);
            }
            Generated::Random => put_variant(out, 1),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Counting (capped at ENUMERATION_LIMIT, never overflowing)
// ---------------------------------------------------------------------------------------------

const LIMIT: u128 = ENUMERATION_LIMIT as u128;

/// `min(C(n, k), ENUMERATION_LIMIT)`, exact below the limit.
fn binomial_capped(n: u128, k: u128) -> u64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut c: u128 = 1;
    let mut i: u128 = 0;
    while i < k {
        // C(n, i+1) = C(n, i) * (n - i) / (i + 1), exact; c < LIMIT and n - i < 2^66, so the
        // product fits in u128.
        c = c * (n - i) / (i + 1);
        if c >= LIMIT {
            return ENUMERATION_LIMIT;
        }
        i += 1;
    }
    // c < LIMIT here.
    u64::try_from(c).unwrap_or(ENUMERATION_LIMIT)
}

/// `min(sum_{k=min..=max} C(n, k), ENUMERATION_LIMIT)`.
fn subsets_capped(n: u64, min: u64, max: u64) -> u64 {
    let mut sum: u64 = 0;
    let mut k = min;
    while k <= max {
        sum = sum.saturating_add(binomial_capped(u128::from(n), u128::from(k)));
        // Each term is at least 1 for k <= n, so this loop runs at most LIMIT times.
        if sum >= ENUMERATION_LIMIT {
            return ENUMERATION_LIMIT;
        }
        match k.checked_add(1) {
            Some(next) => k = next,
            None => break,
        }
    }
    sum
}

/// `min(n!, ENUMERATION_LIMIT)`.
fn factorial_capped(n: u64) -> u64 {
    let mut f: u64 = 1;
    let mut i: u64 = 2;
    while i <= n {
        f = f.saturating_mul(i);
        if f >= ENUMERATION_LIMIT {
            return ENUMERATION_LIMIT;
        }
        i += 1;
    }
    f
}

fn len_u64(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

// ---------------------------------------------------------------------------------------------
// Candidate checks
// ---------------------------------------------------------------------------------------------

/// Sorts `items` ascending (R3: a stable key) and rejects duplicates.
fn sorted_distinct<T: Ord + Copy>(items: &[T], what: &str) -> Result<Vec<T>, ChoiceError> {
    let mut v = items.to_vec();
    v.sort_unstable();
    if v.windows(2).any(|w| w[0] == w[1]) {
        return Err(ChoiceError::InvalidSpec(format!(
            "{what} candidates must be distinct"
        )));
    }
    Ok(v)
}

/// Object candidates, sorted ascending and distinct.
fn want_objects(c: &ResolvedCandidates) -> Result<Vec<ObjectId>, ChoiceError> {
    match c {
        ResolvedCandidates::Objects(v) => sorted_distinct(v, "object"),
        ResolvedCandidates::None => Err(ChoiceError::MissingCandidates),
        _ => Err(ChoiceError::InvalidSpec(
            "this choice kind needs object candidates".to_owned(),
        )),
    }
}

/// Player candidates, sorted ascending and distinct.
fn want_players(c: &ResolvedCandidates) -> Result<Vec<PlayerId>, ChoiceError> {
    match c {
        ResolvedCandidates::Players(v) => sorted_distinct(v, "player"),
        ResolvedCandidates::None => Err(ChoiceError::MissingCandidates),
        _ => Err(ChoiceError::InvalidSpec(
            "this choice kind needs player candidates".to_owned(),
        )),
    }
}

fn want_strings(c: &ResolvedCandidates) -> Result<&[String], ChoiceError> {
    match c {
        ResolvedCandidates::Strings(v) => {
            let mut seen = BTreeSet::new();
            for s in v {
                if s.is_empty() {
                    return Err(ChoiceError::InvalidSpec(
                        "string candidates must be non-empty".to_owned(),
                    ));
                }
                if !seen.insert(s.as_str()) {
                    return Err(ChoiceError::InvalidSpec(format!(
                        "string candidate {s:?} is listed twice"
                    )));
                }
            }
            Ok(v)
        }
        ResolvedCandidates::None => Err(ChoiceError::MissingCandidates),
        _ => Err(ChoiceError::InvalidSpec(
            "this choice kind needs string candidates".to_owned(),
        )),
    }
}

/// Kinds without candidates: any given candidates are extra (decision "#28 FP-027 —
/// Generation: candidate handling, errors and request bounds").
fn want_none(c: &ResolvedCandidates) -> Result<(), ChoiceError> {
    match c {
        ResolvedCandidates::None => Ok(()),
        _ => Err(ChoiceError::MissingCandidates),
    }
}

/// `OBJECTS`/`MODE` bounds: `min <= n`; `max` clamped to `n`.
fn subset_bounds(n: u64, min: u64, max: u64) -> Result<u64, ChoiceError> {
    if min > n {
        return Err(ChoiceError::Unsatisfiable(format!(
            "at least {min} must be chosen from {n} candidates"
        )));
    }
    Ok(max.min(n))
}

fn exactly_one_of(n: usize) -> Result<(), ChoiceError> {
    if n == 0 {
        return Err(ChoiceError::Unsatisfiable(
            "exactly one must be chosen from no candidates".to_owned(),
        ));
    }
    Ok(())
}

/// Everything kind-specific about a generated request.
struct Shape {
    choice_type: ChoiceType,
    min: u64,
    max: u64,
    /// Distinct valid answers, capped at `ENUMERATION_LIMIT`.
    count: u64,
    /// Builds the enumerated options (only called when `count < ENUMERATION_LIMIT`).
    items: Box<dyn FnOnce() -> Vec<ChoiceOption>>,
    rule: ValidatorRule,
    payload: ChoicePayload,
    default: Option<ChoiceResponse>,
}

fn mode_index(i: u64) -> Result<u32, ChoiceError> {
    u32::try_from(i).map_err(|_| ChoiceError::InvalidSpec("too many modes".to_owned()))
}

fn shape(kind: ChoiceKind, r: &ResolvedChoice) -> Result<Shape, ChoiceError> {
    if kind != ChoiceKind::Division && r.division.is_some() {
        return Err(ChoiceError::MissingCandidates);
    }
    let (min, max) = (r.min, r.max);
    Ok(match kind {
        // CR 601.2c-style object selection from a zone: a subset of the candidates.
        ChoiceKind::Objects => {
            let cands = want_objects(&r.candidates)?;
            let n = len_u64(cands.len());
            let max = subset_bounds(n, min, max)?;
            let default = cands
                .iter()
                .take(usize::try_from(min).unwrap_or(usize::MAX))
                .copied()
                .collect();
            let items_src = cands.clone();
            Shape {
                choice_type: ChoiceType::SelectObjectsFromZone,
                min,
                max,
                count: subsets_capped(n, min, max),
                items: Box::new(move || items_src.into_iter().map(ChoiceOption::Object).collect()),
                rule: ValidatorRule::Subset {
                    candidates: cands.clone(),
                },
                payload: ChoicePayload::SelectObjectsFromZone { candidates: cands },
                default: Some(ChoiceResponse::SubmitObjectSelection { objects: default }),
            }
        }
        // CR 601.2b: choose modes.
        ChoiceKind::Mode => {
            let labels = want_strings(&r.candidates)?.to_vec();
            let n = len_u64(labels.len());
            mode_index(n)?;
            let max = subset_bounds(n, min, max)?;
            let mut default = Vec::new();
            for i in 0..min {
                default.push(mode_index(i)?);
            }
            Shape {
                choice_type: ChoiceType::ChooseModes,
                min,
                max,
                count: subsets_capped(n, min, max),
                items: Box::new(move || {
                    (0..n)
                        .filter_map(|i| u32::try_from(i).ok())
                        .map(ChoiceOption::Mode)
                        .collect()
                }),
                rule: ValidatorRule::Modes {
                    count: n,
                    repeats_allowed: false,
                },
                payload: ChoicePayload::ChooseModes {
                    labels,
                    repeats_allowed: false,
                    mode_costs: Vec::new(),
                },
                default: Some(ChoiceResponse::SelectModes { modes: default }),
            }
        }
        ChoiceKind::Number => {
            want_none(&r.candidates)?;
            let top = u64::try_from(i64::MAX).unwrap_or(u64::MAX);
            if min > top {
                return Err(ChoiceError::Unsatisfiable(format!(
                    "the number range starts at {min}, above the largest representable number"
                )));
            }
            let max = max.min(top);
            let (lo, hi) = (
                i64::try_from(min).unwrap_or(i64::MAX),
                i64::try_from(max).unwrap_or(i64::MAX),
            );
            Shape {
                choice_type: ChoiceType::ChooseOptionOrValue,
                min,
                max,
                count: (max - min).saturating_add(1).min(ENUMERATION_LIMIT),
                items: Box::new(move || (lo..=hi).map(ChoiceOption::Number).collect()),
                rule: ValidatorRule::IntegerRange,
                payload: ChoicePayload::ChooseOptionOrValue {
                    option_type: kind.as_str().to_owned(),
                    allowed: AllowedValues::Range,
                },
                default: Some(ChoiceResponse::SubmitChoiceValue {
                    value: ChoiceValue::Number(lo),
                }),
            }
        }
        ChoiceKind::Color | ChoiceKind::CreatureType | ChoiceKind::Pile => {
            let values = want_strings(&r.candidates)?.to_vec();
            exactly_one_of(values.len())?;
            let first = values[0].clone();
            let items_src = values.clone();
            Shape {
                choice_type: ChoiceType::ChooseOptionOrValue,
                min: 1,
                max: 1,
                count: len_u64(values.len()).min(ENUMERATION_LIMIT),
                items: Box::new(move || items_src.into_iter().map(ChoiceOption::Text).collect()),
                rule: ValidatorRule::OneOf {
                    values: values.clone(),
                },
                payload: ChoicePayload::ChooseOptionOrValue {
                    option_type: kind.as_str().to_owned(),
                    allowed: AllowedValues::Texts(values),
                },
                default: Some(ChoiceResponse::SubmitChoiceValue {
                    value: ChoiceValue::Text(first),
                }),
            }
        }
        // CR 712.16, CR 723.5: any name in the database; always a validator contract.
        ChoiceKind::CardName => {
            want_none(&r.candidates)?;
            Shape {
                choice_type: ChoiceType::ChooseCardName,
                min: 1,
                max: 1,
                count: ENUMERATION_LIMIT,
                items: Box::new(Vec::new),
                rule: ValidatorRule::CardName {
                    max_bytes: MAX_CARD_NAME_BYTES,
                },
                payload: ChoicePayload::ChooseCardName,
                default: None,
            }
        }
        ChoiceKind::Player => {
            let players = want_players(&r.candidates)?;
            exactly_one_of(players.len())?;
            let lowest = players[0];
            let items_src = players.clone();
            Shape {
                choice_type: ChoiceType::ChoosePlayer,
                min: 1,
                max: 1,
                count: len_u64(players.len()).min(ENUMERATION_LIMIT),
                items: Box::new(move || items_src.into_iter().map(ChoiceOption::Player).collect()),
                rule: ValidatorRule::OneOfPlayers {
                    players: players.clone(),
                },
                payload: ChoicePayload::ChoosePlayer { players },
                default: Some(ChoiceResponse::SubmitPlayer { player: lowest }),
            }
        }
        ChoiceKind::YesNo => {
            want_none(&r.candidates)?;
            Shape {
                choice_type: ChoiceType::ChooseOptionalTrigger,
                min: 1,
                max: 1,
                count: 2,
                items: Box::new(|| vec![ChoiceOption::Yes, ChoiceOption::No]),
                rule: ValidatorRule::Payload,
                payload: ChoicePayload::ChooseOptionalTrigger {
                    description: String::new(),
                },
                default: Some(ChoiceResponse::No),
            }
        }
        // CR 731.3a-style ordering: a permutation of all candidates, on top.
        ChoiceKind::Order => {
            let cards = want_objects(&r.candidates)?;
            let n = len_u64(cards.len());
            let items_src = cards.clone();
            Shape {
                choice_type: ChoiceType::OrderCards,
                min: n,
                max: n,
                count: factorial_capped(n),
                items: Box::new(move || items_src.into_iter().map(ChoiceOption::Object).collect()),
                rule: ValidatorRule::Permutation {
                    items: cards.clone(),
                },
                payload: ChoicePayload::OrderCards {
                    cards: cards.clone(),
                    destinations: vec![OrderDestination::Top],
                },
                default: Some(ChoiceResponse::SubmitOrdering {
                    top: cards,
                    bottom: Vec::new(),
                    graveyard: Vec::new(),
                }),
            }
        }
        // CR 601.2d, CR 510.1c, CR 510.1d: divide a total among recipients.
        ChoiceKind::Division => {
            want_none(&r.candidates)?;
            let division = r.division.as_ref().ok_or(ChoiceError::MissingCandidates)?;
            let recipients = division.recipients.clone();
            let count_r = len_u64(recipients.len());
            if recipients.is_empty() {
                return Err(ChoiceError::Unsatisfiable(
                    "a division needs at least one recipient".to_owned(),
                ));
            }
            let distinct: BTreeSet<ChoiceTarget> = recipients.iter().map(|(t, _)| *t).collect();
            if distinct.len() != recipients.len() {
                return Err(ChoiceError::InvalidSpec(
                    "division recipients must be distinct".to_owned(),
                ));
            }
            let floors = recipients
                .iter()
                .try_fold(0u64, |acc, (_, f)| acc.checked_add(*f));
            let rem = match floors {
                Some(sum) if sum <= division.total => division.total - sum,
                _ => {
                    return Err(ChoiceError::Unsatisfiable(format!(
                        "the minimums exceed the total {}",
                        division.total
                    )));
                }
            };
            let total = division.total;
            let mut default: Vec<(ChoiceTarget, u64)> = recipients.clone();
            default[0].1 += rem; // floor_0 + rem <= total, no overflow
            let items_src = recipients.clone();
            Shape {
                choice_type: ChoiceType::DivideValue,
                min: count_r,
                max: count_r,
                count: binomial_capped(
                    u128::from(rem) + u128::from(count_r) - 1,
                    u128::from(count_r) - 1,
                ),
                items: Box::new(move || {
                    items_src
                        .into_iter()
                        .map(|(t, _)| match t {
                            ChoiceTarget::Player(p) => ChoiceOption::Player(p),
                            ChoiceTarget::Object(o) => ChoiceOption::Object(o),
                        })
                        .collect()
                }),
                rule: ValidatorRule::Division {
                    total,
                    recipients: recipients.clone(),
                },
                payload: ChoicePayload::DivideValue { total, recipients },
                default: Some(ChoiceResponse::SubmitDivision {
                    assignments: default,
                }),
            }
        }
    })
}

/// Generates the pending request for `spec` with its evaluated values `resolved`
/// (REQ-DSL-011, REQ-NET-003).
///
/// Steps: `spec.validate()`; `random: true` gives [`Generated::Random`]; `min > max` is
/// unsatisfiable; the kind is mapped to its choice type (`OBJECTS` ->
/// `SELECT_OBJECTS_FROM_ZONE`, `MODE` -> `CHOOSE_MODES`, `NUMBER`/`COLOR`/`CREATURE_TYPE`/`PILE`
/// -> `CHOOSE_OPTION_OR_VALUE`, `CARD_NAME` -> `CHOOSE_CARD_NAME`, `PLAYER` -> `CHOOSE_PLAYER`,
/// `YES_NO` -> `CHOOSE_OPTIONAL_TRIGGER`, `ORDER` -> `ORDER_CARDS`, `DIVISION` ->
/// `DIVIDE_VALUE`); candidates are checked; the distinct valid answers are counted and the
/// options enumerated below [`ENUMERATION_LIMIT`], a validator contract otherwise; the default
/// is `resolved.default` (which must validate) or the deterministic generated default.
///
/// `hidden` is `spec.hidden == Some(true)`; `reveal_to`, `chooser` and `reason` come from
/// `resolved`. Request bounds: `OBJECTS`/`MODE` keep `min` and clamp `max` to the candidate
/// count; `NUMBER` bounds the value (clamped to `i64::MAX`); `ORDER` and `DIVISION` use the
/// item/recipient count for both; the other kinds use 1 and 1.
///
/// # Errors
/// [`ChoiceError::InvalidSpec`] (spec validation, malformed or wrong-kind candidates),
/// [`ChoiceError::MissingCandidates`] (candidates or division missing or extra),
/// [`ChoiceError::Unsatisfiable`], [`ChoiceError::InvalidDefault`] or
/// [`ChoiceError::NoDefault`] (`CARD_NAME` without a provided default).
pub fn generate_request(
    id: RequestId,
    spec: &ChoiceSpec,
    resolved: ResolvedChoice,
) -> Result<Generated, ChoiceError> {
    spec.validate().map_err(ChoiceError::InvalidSpec)?;
    if spec.random == Some(true) {
        return Ok(Generated::Random);
    }
    if resolved.min > resolved.max {
        return Err(ChoiceError::Unsatisfiable(format!(
            "min {} is greater than max {}",
            resolved.min, resolved.max
        )));
    }
    let shape = shape(spec.kind, &resolved)?;
    let options =
        if shape.count < ENUMERATION_LIMIT && shape.choice_type != ChoiceType::ChooseCardName {
            ChoiceOptions::Enumerated((shape.items)())
        } else {
            ChoiceOptions::Validator(ValidatorContract {
                min: shape.min,
                max: shape.max,
                rule: shape.rule,
            })
        };
    let provided = resolved.default.is_some();
    let default = match resolved.default {
        Some(d) => d,
        None => shape.default.ok_or(ChoiceError::NoDefault)?,
    };
    let request = PendingChoiceRequest {
        request_id: id,
        chooser: resolved.chooser,
        choice_type: shape.choice_type,
        reason: resolved.reason,
        min: shape.min,
        max: shape.max,
        hidden: spec.hidden == Some(true),
        reveal_to: resolved.reveal_to,
        default_on_timeout: default,
        options,
        payload: shape.payload,
    };
    if let Err(e) = request.validate(&request.default_on_timeout) {
        return Err(if provided {
            ChoiceError::InvalidDefault(e)
        } else {
            // Unreachable for a well-formed shape; reported rather than asserted.
            ChoiceError::InvalidDefault(format!("generated default: {e}"))
        });
    }
    Ok(Generated::Ask(request))
}
