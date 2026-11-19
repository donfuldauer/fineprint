// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `fineprint_core::choice` spec tests (REQ-NET-003, REQ-DSL-011,
//! REQ-CORE-001, REQ-CORE-006). Black-box: written from the work-item brief only.
//!
//! Each `spec_choice_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. All data is synthetic (R5): labels and names are neutral test strings.
//!
//! The oracle functions below restate the brief's kind -> request mapping table independently
//! of the implementation; property tests compare `PendingChoiceRequest::validate` against them.

#![allow(dead_code)]

use std::collections::BTreeSet;

use fineprint_base::hash::{Digest, sha256};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::choice::*;
use fineprint_core::txn::Transactional;
use fineprint_dsl::choice::{ChoiceKind, ChoiceSpec};
use fineprint_dsl::expr::{PlayerSelectorAST, ReferenceAST};

/// The brief's threshold: fewer than this many valid answers -> enumerated options.
pub const LIMIT: u64 = 10_000;

/// An object id from its raw value.
pub fn oid(n: u64) -> ObjectId {
    ObjectId::new(n)
}

/// A player id from its raw value. `GameState::new` issues players `1..=player_count`.
pub fn pid(n: u64) -> PlayerId {
    PlayerId::new(n)
}

/// Object ids from raw values, in the given order.
pub fn oids(ns: &[u64]) -> Vec<ObjectId> {
    ns.iter().copied().map(oid).collect()
}

/// A minimal spec: the controller chooses, every optional member absent.
pub fn spec(kind: ChoiceKind) -> ChoiceSpec {
    ChoiceSpec::new(PlayerSelectorAST::Ref(ReferenceAST::Controller), kind)
}

/// The synthetic reason every generated request carries.
pub fn reason() -> ChoiceReason {
    ChoiceReason {
        cr: "test.reason_a".to_owned(),
        source: Some(oid(900)),
    }
}

/// A resolved choice with player 1 choosing, nothing revealed, no division and no default.
pub fn resolved(candidates: ResolvedCandidates, min: u64, max: u64) -> ResolvedChoice {
    ResolvedChoice {
        chooser: pid(1),
        candidates,
        min,
        max,
        reveal_to: Vec::new(),
        division: None,
        default: None,
        reason: reason(),
    }
}

/// `ResolvedCandidates::Objects` from raw ids, in the given order.
pub fn objects(ns: &[u64]) -> ResolvedCandidates {
    ResolvedCandidates::Objects(oids(ns))
}

/// `ResolvedCandidates::Players` from raw ids, in the given order.
pub fn players(ns: &[u64]) -> ResolvedCandidates {
    ResolvedCandidates::Players(ns.iter().copied().map(pid).collect())
}

/// `ResolvedCandidates::Strings` from labels, in the given order.
pub fn strings(labels: &[String]) -> ResolvedCandidates {
    ResolvedCandidates::Strings(labels.to_vec())
}

/// `n` distinct neutral labels `test.<prefix>_00000`, ...
pub fn labels(prefix: &str, n: usize) -> Vec<String> {
    (0..n).map(|i| format!("test.{prefix}_{i:05}")).collect()
}

/// Raw ids `1..=n`.
pub fn range_ids(n: u64) -> Vec<u64> {
    (1..=n).collect()
}

/// A resolved DIVISION choice (no candidates, `division` set). The resolved `min`/`max` are
/// not meaningful for a division; the tests pass 1/1 (DECISIONS: "#28 FP-027 — Spec-test
/// choices", item 2).
pub fn division(total: u64, recipients: Vec<(ChoiceTarget, u64)>) -> ResolvedChoice {
    let mut r = resolved(ResolvedCandidates::None, 1, 1);
    r.division = Some(ResolvedDivision { total, recipients });
    r
}

/// Generates with id 0 from a minimal spec of `kind`.
pub fn gen_kind(kind: ChoiceKind, r: ResolvedChoice) -> Result<Generated, ChoiceError> {
    generate_request(RequestId::new(0), &spec(kind), r)
}

/// Unwraps `Generated::Ask`.
pub fn expect_ask(g: Result<Generated, ChoiceError>) -> PendingChoiceRequest {
    match g {
        Ok(Generated::Ask(req)) => req,
        Ok(Generated::Random) => panic!("expected Ask, got Random"),
        Err(e) => panic!("expected Ask, got Err({e:?})"),
    }
}

/// Generates with id 0 from a minimal spec of `kind` and unwraps the request.
pub fn ask_kind(kind: ChoiceKind, r: ResolvedChoice) -> PendingChoiceRequest {
    expect_ask(gen_kind(kind, r))
}

/// The enumerated options of `req`; panics on a validator contract.
pub fn items(req: &PendingChoiceRequest) -> Vec<ChoiceOption> {
    match &req.options {
        ChoiceOptions::Enumerated(items) => items.clone(),
        ChoiceOptions::Validator(_) => panic!("expected Enumerated, got Validator"),
    }
}

/// Whether `req` carries a validator contract.
pub fn is_validator(req: &PendingChoiceRequest) -> bool {
    matches!(req.options, ChoiceOptions::Validator(_))
}

/// Asserts `req` accepts `resp`.
pub fn accept(req: &PendingChoiceRequest, resp: &ChoiceResponse) {
    if let Err(e) = req.validate(resp) {
        panic!(
            "{:?} rejected valid answer {resp:?}: {e}",
            req.choice_type.as_str()
        );
    }
}

/// Asserts `req` rejects `resp` with a non-empty explanation, and returns it.
pub fn reject(req: &PendingChoiceRequest, resp: &ChoiceResponse) -> String {
    match req.validate(resp) {
        Ok(()) => panic!(
            "{:?} accepted invalid answer {resp:?}",
            req.choice_type.as_str()
        ),
        Err(e) => {
            assert!(
                !e.trim().is_empty(),
                "rejection of {resp:?} has an empty explanation"
            );
            e
        }
    }
}

/// `SubmitObjectSelection` from raw ids.
pub fn sel(ns: &[u64]) -> ChoiceResponse {
    ChoiceResponse::SubmitObjectSelection { objects: oids(ns) }
}

/// `SubmitChoiceValue { Number(v) }`.
pub fn num(v: i64) -> ChoiceResponse {
    ChoiceResponse::SubmitChoiceValue {
        value: ChoiceValue::Number(v),
    }
}

/// `SubmitChoiceValue { Text(s) }`.
pub fn text(s: &str) -> ChoiceResponse {
    ChoiceResponse::SubmitChoiceValue {
        value: ChoiceValue::Text(s.to_owned()),
    }
}

/// `SubmitOrdering` from raw ids.
pub fn ordering(top: &[u64], bottom: &[u64], graveyard: &[u64]) -> ChoiceResponse {
    ChoiceResponse::SubmitOrdering {
        top: oids(top),
        bottom: oids(bottom),
        graveyard: oids(graveyard),
    }
}

/// `SubmitDivision` from pairs.
pub fn div(assignments: &[(ChoiceTarget, u64)]) -> ChoiceResponse {
    ChoiceResponse::SubmitDivision {
        assignments: assignments.to_vec(),
    }
}

/// `ChoiceTarget::Object` from a raw id.
pub fn to(n: u64) -> ChoiceTarget {
    ChoiceTarget::Object(oid(n))
}

/// `ChoiceTarget::Player` from a raw id.
pub fn tp(n: u64) -> ChoiceTarget {
    ChoiceTarget::Player(pid(n))
}

/// One sample of every `ChoiceResponse` variant whose fields the brief pins (the variants with
/// unpinned fields cannot be constructed). Payloads name ids 1..=3 and player 1, so a payload
/// would often be acceptable to a request were the variant right.
pub fn pinned_samples() -> Vec<ChoiceResponse> {
    use ChoiceResponse as R;
    vec![
        R::PassPriority,
        R::PlayLand {
            object: oid(1),
            face_index: 0,
        },
        R::InitiateCast {
            object: oid(1),
            face_index: 0,
            cast_mode: "test.cast_mode".to_owned(),
        },
        R::Activate {
            object: oid(1),
            ability_id: "test.ability_a".to_owned(),
        },
        R::SpecialAction {
            kind: "test.special".to_owned(),
            object: oid(1),
        },
        R::SelectModes { modes: vec![0] },
        R::SubmitDivision {
            assignments: vec![(to(1), 1)],
        },
        R::DeclineOptionalPayment,
        R::CancelAndRollback,
        R::Yes,
        R::No,
        R::SubmitObjectSelection {
            objects: oids(&[1]),
        },
        R::SubmitOrdering {
            top: oids(&[1, 2, 3]),
            bottom: Vec::new(),
            graveyard: Vec::new(),
        },
        R::SubmitPlayer { player: pid(1) },
        R::SubmitChoiceValue {
            value: ChoiceValue::Number(1),
        },
        R::SubmitChoiceValue {
            value: ChoiceValue::Text("test.color_00000".to_owned()),
        },
        R::SubmitCardName {
            name: "test.name_a".to_owned(),
        },
        R::SubmitSet {
            objects: oids(&[1]),
        },
        R::Accept,
        R::Shorten { at_index: 0 },
        R::KeepHand { bottom: Vec::new() },
        R::TakeMulligan,
    ]
}

/// Whether two responses are the same variant.
pub fn same_variant(a: &ChoiceResponse, b: &ChoiceResponse) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

// ---------------------------------------------------------------------------------------------
// Independent oracle: the brief's mapping table restated.
// ---------------------------------------------------------------------------------------------

/// The candidates as a sorted id list (OBJECTS, ORDER).
fn obj_cands(r: &ResolvedChoice) -> Vec<ObjectId> {
    match &r.candidates {
        ResolvedCandidates::Objects(v) => {
            let mut v = v.clone();
            v.sort();
            v
        }
        _ => Vec::new(),
    }
}

/// The candidates as a sorted player list (PLAYER).
fn player_cands(r: &ResolvedChoice) -> Vec<PlayerId> {
    match &r.candidates {
        ResolvedCandidates::Players(v) => {
            let mut v = v.clone();
            v.sort();
            v
        }
        _ => Vec::new(),
    }
}

/// The candidates as strings, given order (MODE, COLOR, CREATURE_TYPE, PILE).
fn str_cands(r: &ResolvedChoice) -> Vec<String> {
    match &r.candidates {
        ResolvedCandidates::Strings(v) => v.clone(),
        _ => Vec::new(),
    }
}

/// `max` clamped to `n` (OBJECTS, MODE).
pub fn clamped_max(r: &ResolvedChoice, n: usize) -> u64 {
    r.max.min(n as u64)
}

/// Whether `xs` has no repeated element.
fn distinct<T: Ord>(xs: &[T]) -> bool {
    let set: BTreeSet<&T> = xs.iter().collect();
    set.len() == xs.len()
}

/// The CARD_NAME structural check from the brief's table.
pub fn oracle_card_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
}

/// Whether `resp` is a valid answer to the request generated from (`kind`, `r`), per the
/// brief's table.
pub fn oracle_valid(kind: ChoiceKind, r: &ResolvedChoice, resp: &ChoiceResponse) -> bool {
    use ChoiceResponse as R;
    match (kind, resp) {
        (ChoiceKind::Objects, R::SubmitObjectSelection { objects }) => {
            let cands = obj_cands(r);
            let n = objects.len() as u64;
            distinct(objects)
                && objects.iter().all(|o| cands.contains(o))
                && n >= r.min
                && n <= clamped_max(r, cands.len())
        }
        (ChoiceKind::Mode, R::SelectModes { modes }) => {
            let n = str_cands(r).len();
            let k = modes.len() as u64;
            distinct(modes)
                && modes.iter().all(|&m| (m as usize) < n)
                && k >= r.min
                && k <= clamped_max(r, n)
        }
        (ChoiceKind::Number, R::SubmitChoiceValue { value }) => match value {
            ChoiceValue::Number(v) => *v >= 0 && (*v as u64) >= r.min && (*v as u64) <= r.max,
            ChoiceValue::Text(_) => false,
        },
        (
            ChoiceKind::Color | ChoiceKind::CreatureType | ChoiceKind::Pile,
            R::SubmitChoiceValue { value },
        ) => match value {
            ChoiceValue::Text(s) => str_cands(r).contains(s),
            ChoiceValue::Number(_) => false,
        },
        (ChoiceKind::CardName, R::SubmitCardName { name }) => oracle_card_name(name),
        (ChoiceKind::Player, R::SubmitPlayer { player }) => player_cands(r).contains(player),
        (ChoiceKind::YesNo, R::Yes | R::No) => true,
        (
            ChoiceKind::Order,
            R::SubmitOrdering {
                top,
                bottom,
                graveyard,
            },
        ) => {
            let mut t = top.clone();
            t.sort();
            bottom.is_empty() && graveyard.is_empty() && t == obj_cands(r)
        }
        (ChoiceKind::Division, R::SubmitDivision { assignments }) => {
            let Some(d) = &r.division else {
                return false;
            };
            if assignments.len() != d.recipients.len() {
                return false;
            }
            let targets: Vec<ChoiceTarget> = assignments.iter().map(|(t, _)| *t).collect();
            if !distinct(&targets) {
                return false;
            }
            let mut sum: u128 = 0;
            for (t, amount) in assignments {
                let Some((_, floor)) = d.recipients.iter().find(|(rt, _)| rt == t) else {
                    return false;
                };
                if amount < floor {
                    return false;
                }
                sum += u128::from(*amount);
            }
            sum == u128::from(d.total)
        }
        _ => false,
    }
}

/// C(n, k) saturating at `u64::MAX` (exact below it).
pub fn binom(n: u64, k: u64) -> u64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut acc: u128 = 1;
    for i in 0..k {
        acc = acc * u128::from(n - i) / u128::from(i + 1);
        if acc > u128::from(u64::MAX) {
            return u64::MAX;
        }
    }
    acc as u64
}

/// The brief's N (number of valid answers), saturating.
pub fn oracle_count(kind: ChoiceKind, r: &ResolvedChoice) -> u64 {
    match kind {
        ChoiceKind::Objects | ChoiceKind::Mode => {
            let n = match kind {
                ChoiceKind::Objects => obj_cands(r).len(),
                _ => str_cands(r).len(),
            } as u64;
            let mut total: u64 = 0;
            for k in r.min..=clamped_max(r, n as usize) {
                total = total.saturating_add(binom(n, k));
            }
            total
        }
        ChoiceKind::Number => (r.max - r.min).saturating_add(1),
        ChoiceKind::Color | ChoiceKind::CreatureType | ChoiceKind::Pile => {
            str_cands(r).len() as u64
        }
        ChoiceKind::CardName => u64::MAX,
        ChoiceKind::Player => player_cands(r).len() as u64,
        ChoiceKind::YesNo => 2,
        ChoiceKind::Order => {
            let mut f: u64 = 1;
            for i in 1..=obj_cands(r).len() as u64 {
                f = f.saturating_mul(i);
            }
            f
        }
        ChoiceKind::Division => {
            let d = r.division.as_ref().expect("division");
            let floors: u64 = d.recipients.iter().map(|(_, f)| *f).sum();
            let rem = d.total - floors;
            let k = d.recipients.len() as u64;
            binom(rem.saturating_add(k - 1), k - 1)
        }
    }
}

/// The brief's enumerated items for (`kind`, `r`).
pub fn oracle_items(kind: ChoiceKind, r: &ResolvedChoice) -> Vec<ChoiceOption> {
    match kind {
        ChoiceKind::Objects | ChoiceKind::Order => {
            obj_cands(r).into_iter().map(ChoiceOption::Object).collect()
        }
        ChoiceKind::Mode => (0..str_cands(r).len() as u32)
            .map(ChoiceOption::Mode)
            .collect(),
        ChoiceKind::Number => (r.min..=r.max)
            .map(|v| ChoiceOption::Number(v as i64))
            .collect(),
        ChoiceKind::Color | ChoiceKind::CreatureType | ChoiceKind::Pile => {
            str_cands(r).into_iter().map(ChoiceOption::Text).collect()
        }
        ChoiceKind::Player => player_cands(r)
            .into_iter()
            .map(ChoiceOption::Player)
            .collect(),
        ChoiceKind::YesNo => vec![ChoiceOption::Yes, ChoiceOption::No],
        ChoiceKind::Division => r
            .division
            .as_ref()
            .expect("division")
            .recipients
            .iter()
            .map(|(t, _)| match t {
                ChoiceTarget::Object(o) => ChoiceOption::Object(*o),
                ChoiceTarget::Player(p) => ChoiceOption::Player(*p),
            })
            .collect(),
        ChoiceKind::CardName => Vec::new(),
    }
}

/// The expected `choice_type` for each kind (brief's table).
pub fn expected_type(kind: ChoiceKind) -> ChoiceType {
    match kind {
        ChoiceKind::Objects => ChoiceType::SelectObjectsFromZone,
        ChoiceKind::Mode => ChoiceType::ChooseModes,
        ChoiceKind::Number | ChoiceKind::Color | ChoiceKind::CreatureType | ChoiceKind::Pile => {
            ChoiceType::ChooseOptionOrValue
        }
        ChoiceKind::CardName => ChoiceType::ChooseCardName,
        ChoiceKind::Player => ChoiceType::ChoosePlayer,
        ChoiceKind::YesNo => ChoiceType::ChooseOptionalTrigger,
        ChoiceKind::Order => ChoiceType::OrderCards,
        ChoiceKind::Division => ChoiceType::DivideValue,
    }
}

// ---------------------------------------------------------------------------------------------
// Seeded generators (R3: the only randomness is `Pcg64Dxsm::from_u64_seed`).
// ---------------------------------------------------------------------------------------------

/// A uniform value in `0..bound` (`bound > 0`).
pub fn below(rng: &mut Pcg64Dxsm, bound: u64) -> u64 {
    rng.next_bounded(bound).expect("bound > 0")
}

/// A uniform value in `lo..=hi`.
pub fn between(rng: &mut Pcg64Dxsm, lo: u64, hi: u64) -> u64 {
    lo + below(rng, hi - lo + 1)
}

/// A random small resolved choice of `kind` whose answer space is below the threshold.
pub fn random_small(rng: &mut Pcg64Dxsm, kind: ChoiceKind) -> ResolvedChoice {
    // Distinct ids drawn from 1..=60.
    let ids = |rng: &mut Pcg64Dxsm, n: u64| -> Vec<u64> {
        let mut pool: Vec<u64> = (1..=60).collect();
        rng.shuffle(&mut pool);
        pool.truncate(n as usize);
        // Given ascending: the sorting of unsorted candidates has its own property test
        // (`req_dsl_011_prop_candidate_order`); the other properties should not depend on it.
        pool.sort_unstable();
        pool
    };
    match kind {
        ChoiceKind::Objects => {
            let n = between(rng, 1, 8);
            let min = between(rng, 0, n);
            let max = between(rng, min, n + 2);
            let v = ids(rng, n);
            resolved(objects(&v), min, max)
        }
        ChoiceKind::Mode => {
            let n = between(rng, 1, 8);
            let min = between(rng, 0, n);
            let max = between(rng, min, n + 2);
            resolved(strings(&labels("mode", n as usize)), min, max)
        }
        ChoiceKind::Number => {
            let min = between(rng, 0, 40);
            let max = between(rng, min, min + 40);
            resolved(ResolvedCandidates::None, min, max)
        }
        ChoiceKind::Color | ChoiceKind::CreatureType | ChoiceKind::Pile => {
            let n = between(rng, 1, 10);
            let mut l = labels("value", n as usize);
            rng.shuffle(&mut l);
            resolved(strings(&l), 1, 1)
        }
        ChoiceKind::Player => {
            let n = between(rng, 1, 6);
            let v = ids(rng, n);
            resolved(players(&v), 1, 1)
        }
        ChoiceKind::YesNo | ChoiceKind::CardName => resolved(ResolvedCandidates::None, 1, 1),
        ChoiceKind::Order => {
            let n = between(rng, 1, 5);
            let v = ids(rng, n);
            resolved(objects(&v), n, n)
        }
        ChoiceKind::Division => {
            let r = between(rng, 1, 3);
            let v = ids(rng, r);
            let mut recipients = Vec::new();
            let mut floors = 0;
            for (i, raw) in v.iter().enumerate() {
                let t = if i % 2 == 0 { to(*raw) } else { tp(*raw) };
                let f = below(rng, 3);
                floors += f;
                recipients.push((t, f));
            }
            let total = floors + below(rng, 20);
            division(total, recipients)
        }
    }
}

/// Kinds whose spaces `random_small` keeps below the threshold (all but CARD_NAME).
pub const SMALL_KINDS: [ChoiceKind; 10] = [
    ChoiceKind::Objects,
    ChoiceKind::Mode,
    ChoiceKind::Number,
    ChoiceKind::Color,
    ChoiceKind::CreatureType,
    ChoiceKind::Pile,
    ChoiceKind::Player,
    ChoiceKind::YesNo,
    ChoiceKind::Order,
    ChoiceKind::Division,
];

/// A valid answer built from the request's enumerated items and bounds (property i).
pub fn answer_from_items(
    rng: &mut Pcg64Dxsm,
    kind: ChoiceKind,
    req: &PendingChoiceRequest,
    r: &ResolvedChoice,
) -> ChoiceResponse {
    let its = items(req);
    let pick = |rng: &mut Pcg64Dxsm, its: &[ChoiceOption]| -> ChoiceOption {
        its[below(rng, its.len() as u64) as usize].clone()
    };
    match kind {
        ChoiceKind::Objects => {
            let k = between(rng, req.min, req.max);
            let mut v = its.clone();
            rng.shuffle(&mut v);
            v.truncate(k as usize);
            ChoiceResponse::SubmitObjectSelection {
                objects: v
                    .into_iter()
                    .map(|o| match o {
                        ChoiceOption::Object(o) => o,
                        other => panic!("OBJECTS item {other:?}"),
                    })
                    .collect(),
            }
        }
        ChoiceKind::Mode => {
            let k = between(rng, req.min, req.max);
            let mut v = its.clone();
            rng.shuffle(&mut v);
            v.truncate(k as usize);
            ChoiceResponse::SelectModes {
                modes: v
                    .into_iter()
                    .map(|o| match o {
                        ChoiceOption::Mode(m) => m,
                        other => panic!("MODE item {other:?}"),
                    })
                    .collect(),
            }
        }
        ChoiceKind::Number => match pick(rng, &its) {
            ChoiceOption::Number(v) => num(v),
            other => panic!("NUMBER item {other:?}"),
        },
        ChoiceKind::Color | ChoiceKind::CreatureType | ChoiceKind::Pile => match pick(rng, &its) {
            ChoiceOption::Text(s) => text(&s),
            other => panic!("string item {other:?}"),
        },
        ChoiceKind::Player => match pick(rng, &its) {
            ChoiceOption::Player(p) => ChoiceResponse::SubmitPlayer { player: p },
            other => panic!("PLAYER item {other:?}"),
        },
        ChoiceKind::YesNo => match pick(rng, &its) {
            ChoiceOption::Yes => ChoiceResponse::Yes,
            ChoiceOption::No => ChoiceResponse::No,
            other => panic!("YES_NO item {other:?}"),
        },
        ChoiceKind::Order => {
            let mut v = its.clone();
            rng.shuffle(&mut v);
            ChoiceResponse::SubmitOrdering {
                top: v
                    .into_iter()
                    .map(|o| match o {
                        ChoiceOption::Object(o) => o,
                        other => panic!("ORDER item {other:?}"),
                    })
                    .collect(),
                bottom: Vec::new(),
                graveyard: Vec::new(),
            }
        }
        ChoiceKind::Division => {
            // Items name the recipients; floors and total come from the resolved division.
            let d = r.division.as_ref().expect("division");
            let mut amounts: Vec<u64> = d.recipients.iter().map(|(_, f)| *f).collect();
            let floors: u64 = amounts.iter().sum();
            for _ in 0..d.total - floors {
                let i = below(rng, amounts.len() as u64) as usize;
                amounts[i] += 1;
            }
            let assignments = its
                .iter()
                .zip(amounts)
                .map(|(o, a)| match o {
                    ChoiceOption::Object(o) => (ChoiceTarget::Object(*o), a),
                    ChoiceOption::Player(p) => (ChoiceTarget::Player(*p), a),
                    other => panic!("DIVISION item {other:?}"),
                })
                .collect();
            ChoiceResponse::SubmitDivision { assignments }
        }
        ChoiceKind::CardName => ChoiceResponse::SubmitCardName {
            name: "test.name_a".to_owned(),
        },
    }
}

// ---------------------------------------------------------------------------------------------
// Toy Transactional (requirement 8): a counter plus a log.
// ---------------------------------------------------------------------------------------------

/// A toy state: a counter and an append-only log; `state_hash` = sha256 of its bytes.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Toy {
    pub counter: i64,
    pub log: Vec<u8>,
}

impl Toy {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(self.counter.to_le_bytes());
        out.extend((self.log.len() as u64).to_le_bytes());
        out.extend(&self.log);
        out
    }
}

impl Transactional for Toy {
    fn state_hash(&self) -> Digest {
        sha256(&self.bytes())
    }

    fn priority_player(&self) -> Option<PlayerId> {
        Some(pid(1))
    }

    fn carry_durable(restored: Self, _before: &Self, _after: &Self) -> Self {
        restored
    }
}

// ---------------------------------------------------------------------------------------------
// Session helpers.
// ---------------------------------------------------------------------------------------------

/// What a rejected call must leave unchanged: pending request, answers and state hash.
pub type Observed = (Option<PendingChoiceRequest>, Vec<ChoiceResponse>, Digest);

/// The observable state of a session.
pub fn observe<S, O, F>(s: &ChoiceSession<S, O, F>) -> Observed
where
    S: Transactional,
    F: Fn(&mut S, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    (s.pending().cloned(), s.answers().to_vec(), s.state_hash())
}

/// Submits each answer to the request pending at the time; panics on any rejection.
pub fn submit_all<S, O, F>(s: &mut ChoiceSession<S, O, F>, answers: &[ChoiceResponse])
where
    S: Transactional,
    F: Fn(&mut S, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    for a in answers {
        let id = s.pending().expect("a pending request").request_id;
        if let Err(e) = s.submit(id, a.clone()) {
            panic!("answer {a:?} to {id:?} rejected: {e:?}");
        }
    }
}

/// Whether the session is `Done`.
pub fn is_done<S, O, F>(s: &ChoiceSession<S, O, F>) -> bool
where
    S: Transactional,
    F: Fn(&mut S, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    matches!(s.status(), Status::Done { .. })
}

/// The `Done` state's hash; panics while pending.
pub fn done_hash<S, O, F>(s: &ChoiceSession<S, O, F>) -> Digest
where
    S: Transactional,
    F: Fn(&mut S, &mut ChoiceContext<'_>) -> Result<O, Interrupt>,
{
    match s.status() {
        Status::Done { state, .. } => state.state_hash(),
        Status::Pending(_) => panic!("expected Done"),
    }
}

/// Generates and asks; the procedure-side idiom from the brief.
pub fn ask_in(
    ctx: &mut ChoiceContext<'_>,
    kind: ChoiceKind,
    r: ResolvedChoice,
) -> Result<ChoiceResponse, Interrupt> {
    let id = ctx.next_request_id();
    match generate_request(id, &spec(kind), r)? {
        Generated::Ask(req) => ctx.ask(req),
        Generated::Random => Err(ChoiceError::Procedure("unexpected Random".to_owned()).into()),
    }
}
