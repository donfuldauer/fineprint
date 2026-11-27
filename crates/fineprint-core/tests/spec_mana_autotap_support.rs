// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared builders, a solution checker and a brute-force reference ("oracle") for the auto-tap
//! solver spec tests (REQ-API-002; CR 106.4, 106.6, 106.6a, 107.4a-c, 107.4h, 118.3, 118.3a,
//! 601.2g, 601.2h, 605.3b).
//!
//! Black-box: written from the work-item brief only, against the pinned public API of
//! `fineprint_core::mana::preview_auto_tap`. The oracle enumerates every candidate literally
//! (every subset of entries, every output choice) and checks it with the merged `plan_payment`
//! only, so the tests compare the solver with the brief rather than with itself.
//!
//! Each `spec_mana_autotap*.rs` test file includes this file with `#[path]`. Nothing here names a
//! real card (R5): sources are small numbers, ability ids are `test.*`, tags are neutral.

#![allow(dead_code)]

use fineprint_base::ids::ObjectId;
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::casting::ManaComponent;
use fineprint_core::mana::{
    AutoTapActivation, AutoTapSolution, MAX_SOLUTIONS, MAX_SOURCES, ManaOutput, ManaPool,
    ManaRestriction, ManaRider, ManaSource, ManaType, ManaUnit, SpendPurpose, plan_payment,
    preview_auto_tap,
};

pub const W: ManaType = ManaType::White;
pub const U: ManaType = ManaType::Blue;
pub const B: ManaType = ManaType::Black;
pub const R: ManaType = ManaType::Red;
pub const G: ManaType = ManaType::Green;
pub const C: ManaType = ManaType::Colorless;

pub const TYPES: [ManaType; 6] = [W, U, B, R, G, C];

/// Neutral purpose tags (R5).
pub const TAGS: [&str; 3] = ["spell", "ability", "test.tag_x"];

pub fn obj(n: u64) -> ObjectId {
    ObjectId::new(n)
}

pub fn pool_of(units: &[ManaUnit]) -> ManaPool {
    let mut p = ManaPool::default();
    for u in units {
        p.add(u.clone());
    }
    p
}

pub fn purpose(tags: &[&str]) -> SpendPurpose {
    SpendPurpose::new(tags.iter().copied())
}

pub fn any_purpose() -> SpendPurpose {
    SpendPurpose::new(Vec::<&str>::new())
}

/// A mana component: `generic`, then W, U, B, R, G, `{C}` and `{S}` counts.
#[allow(clippy::too_many_arguments)]
pub fn mc(
    generic: u64,
    white: u64,
    blue: u64,
    black: u64,
    red: u64,
    green: u64,
    colorless: u64,
    snow: u64,
) -> ManaComponent {
    ManaComponent {
        generic,
        white,
        blue,
        black,
        red,
        green,
        colorless,
        snow,
    }
}

/// The `{0}` cost.
pub fn zero() -> ManaComponent {
    ManaComponent::default()
}

/// `{R}` repeated `n` times.
pub fn reds(n: u64) -> ManaComponent {
    mc(0, 0, 0, 0, n, 0, 0, 0)
}

/// A single-output entry producing `types`.
pub fn entry(source: u64, id: &str, types: &[ManaType]) -> ManaSource {
    ManaSource::new(obj(source), id, vec![ManaOutput::new(types)])
}

/// A multi-output entry: one alternative per slice.
pub fn entry_alts(source: u64, id: &str, alts: &[&[ManaType]]) -> ManaSource {
    ManaSource::new(
        obj(source),
        id,
        alts.iter().map(|t| ManaOutput::new(t)).collect(),
    )
}

/// The units one activation of `src` choosing `output` adds (REQ-API-002; CR 106.6a): one unit
/// per `types` entry, with the entry's source and snow-ness and the output's restrictions and
/// riders on every unit.
pub fn expected_units(src: &ManaSource, output: usize) -> Vec<ManaUnit> {
    let Some(out) = src.outputs.get(output) else {
        return Vec::new();
    };
    out.types
        .iter()
        .map(|&t| ManaUnit {
            mana_type: t,
            source: src.source,
            source_is_snow: src.source_is_snow,
            restrictions: out.restrictions.clone(),
            riders: out.riders.clone(),
        })
        .collect()
}

/// The pool with `extra` appended.
pub fn appended(pool: &ManaPool, extra: &[ManaUnit]) -> ManaPool {
    let mut p = pool.clone();
    for u in extra {
        p.add(u.clone());
    }
    p
}

/// The rank key of the brief (REQ-API-002): K1 activations, K2 wasted produced units, K3
/// flexibility lost, K4 the `(index, output)` sequence.
pub type RankKey = (usize, usize, usize, Vec<(usize, usize)>);

pub fn distinct_types(src: &ManaSource) -> usize {
    TYPES
        .iter()
        .filter(|t| src.outputs.iter().any(|o| o.types.contains(t)))
        .count()
}

pub fn rank_key(pool: &ManaPool, sources: &[ManaSource], s: &AutoTapSolution) -> RankKey {
    let k1 = s.activations.len();
    let spent_produced = s.plan.spent.iter().filter(|&&i| i >= pool.len()).count();
    let k2 = s.produced.len().saturating_sub(spent_produced);
    let k3 = s
        .activations
        .iter()
        .map(|a| sources.get(a.index).map_or(0, distinct_types))
        .sum();
    let k4 = s.activations.iter().map(|a| (a.index, a.output)).collect();
    (k1, k2, k3, k4)
}

/// Asserts every per-solution property of requirements 1-3 (REQ-API-002; CR 106.6a, 118.3,
/// 601.2h, 605.3b).
pub fn assert_valid(
    pool: &ManaPool,
    sources: &[ManaSource],
    cost: &ManaComponent,
    purpose: &SpendPurpose,
    s: &AutoTapSolution,
) {
    // Requirement 2: candidate rules.
    for w in s.activations.windows(2) {
        assert!(w[0].index < w[1].index, "activations ascend: {s:?}");
    }
    let mut produced = Vec::new();
    for a in &s.activations {
        assert!(a.index < MAX_SOURCES, "only the first 64 entries: {a:?}");
        let src = sources.get(a.index).expect("index in range");
        assert_eq!(a.source, src.source, "source copied");
        assert_eq!(a.ability_id, src.ability_id, "ability id copied");
        assert!(a.output < src.outputs.len(), "output in range: {a:?}");
        produced.extend(expected_units(src, a.output));
    }
    for (i, a) in s.activations.iter().enumerate() {
        for b in s.activations.iter().skip(i + 1) {
            let sa = &sources[a.index];
            let sb = &sources[b.index];
            assert!(
                !(sa.uses_tap && sb.uses_tap && sa.source == sb.source),
                "CR 118.3: two {{T}} entries of one source: {s:?}"
            );
        }
    }
    assert_eq!(s.sources_used(), s.activations.len());
    // Requirement 1: produced units and pool_after.
    assert_eq!(s.produced, produced, "produced units");
    let after = appended(pool, &produced);
    assert_eq!(s.pool_after(pool), after, "pool_after");
    // Requirement 3: validity and no wasted activation.
    assert_eq!(
        plan_payment(&after, cost, purpose).as_ref(),
        Some(&s.plan),
        "the plan is plan_payment on pool_after"
    );
    let mut start = pool.len();
    for a in &s.activations {
        let n = expected_units(&sources[a.index], a.output).len();
        let end = start + n;
        assert!(
            s.plan.spent.iter().any(|&i| i >= start && i < end),
            "activation {a:?} contributes a spent unit: {s:?}"
        );
        start = end;
    }
}

/// Asserts the result is sorted by the rank key with no duplicate candidate.
pub fn assert_ranked(pool: &ManaPool, sources: &[ManaSource], sols: &[AutoTapSolution]) {
    for w in sols.windows(2) {
        let a = rank_key(pool, sources, &w[0]);
        let b = rank_key(pool, sources, &w[1]);
        assert!(a < b, "rank order: {a:?} then {b:?}");
    }
}

/// Asserts every solution valid, the list ranked and at most `MAX_SOLUTIONS` long.
pub fn assert_all_valid(
    pool: &ManaPool,
    sources: &[ManaSource],
    cost: &ManaComponent,
    purpose: &SpendPurpose,
    sols: &[AutoTapSolution],
) {
    assert!(sols.len() <= MAX_SOLUTIONS);
    for s in sols {
        assert_valid(pool, sources, cost, purpose, s);
    }
    assert_ranked(pool, sources, sols);
}

/// The brute-force reference of requirement 10: every subset of the considered entries and every
/// output choice, the `uses_tap` rule, validity per requirement 3 via `plan_payment` only, sorted
/// by K1-K4 and truncated to `min(max_solutions, MAX_SOLUTIONS)`. Exponential: small inputs only.
pub fn reference(
    pool: &ManaPool,
    sources: &[ManaSource],
    cost: &ManaComponent,
    purpose: &SpendPurpose,
    max_solutions: usize,
) -> Vec<AutoTapSolution> {
    let considered = &sources[..sources.len().min(MAX_SOURCES)];
    // choice[i] = 0 skip, k + 1 = output k.
    let mut choice = vec![0usize; considered.len()];
    let mut out: Vec<(RankKey, AutoTapSolution)> = Vec::new();
    loop {
        if let Some(s) = candidate(pool, considered, cost, purpose, &choice) {
            out.push((rank_key(pool, considered, &s), s));
        }
        // Odometer increment.
        let mut i = 0;
        loop {
            if i == choice.len() {
                out.sort_by(|a, b| a.0.cmp(&b.0));
                return out
                    .into_iter()
                    .map(|(_, s)| s)
                    .take(max_solutions.min(MAX_SOLUTIONS))
                    .collect();
            }
            if choice[i] < considered[i].outputs.len() {
                choice[i] += 1;
                break;
            }
            choice[i] = 0;
            i += 1;
        }
    }
}

fn candidate(
    pool: &ManaPool,
    sources: &[ManaSource],
    cost: &ManaComponent,
    purpose: &SpendPurpose,
    choice: &[usize],
) -> Option<AutoTapSolution> {
    let mut activations = Vec::new();
    let mut produced = Vec::new();
    let mut ranges = Vec::new();
    let mut tapped: Vec<ObjectId> = Vec::new();
    for (index, &c) in choice.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let src = &sources[index];
        if src.uses_tap {
            if tapped.contains(&src.source) {
                return None;
            }
            tapped.push(src.source);
        }
        let units = expected_units(src, c - 1);
        let start = pool.len() + produced.len();
        ranges.push((start, start + units.len()));
        produced.extend(units);
        activations.push(AutoTapActivation {
            index,
            source: src.source,
            ability_id: src.ability_id.clone(),
            output: c - 1,
        });
    }
    let after = appended(pool, &produced);
    let plan = plan_payment(&after, cost, purpose)?;
    let contributes = ranges
        .iter()
        .all(|&(a, b)| plan.spent.iter().any(|&i| i >= a && i < b));
    contributes.then_some(AutoTapSolution {
        activations,
        produced,
        plan,
    })
}

/// Calls the solver, checks every returned solution and determinism, and returns the result.
pub fn solve(
    pool: &ManaPool,
    sources: &[ManaSource],
    cost: &ManaComponent,
    purpose: &SpendPurpose,
    max_solutions: usize,
) -> Vec<AutoTapSolution> {
    let (p0, s0, c0, u0) = (pool.clone(), sources.to_vec(), *cost, purpose.clone());
    let sols = preview_auto_tap(pool, sources, cost, purpose, max_solutions);
    assert_eq!(pool, &p0, "pool unchanged");
    assert_eq!(sources, s0.as_slice(), "sources unchanged");
    assert_eq!(cost, &c0, "cost unchanged");
    assert_eq!(purpose, &u0, "purpose unchanged");
    assert!(sols.len() <= max_solutions.min(MAX_SOLUTIONS));
    assert_all_valid(pool, sources, cost, purpose, &sols);
    sols
}

/// `(index, output)` per activation, per solution: a compact view of a result.
pub fn shape(sols: &[AutoTapSolution]) -> Vec<Vec<(usize, usize)>> {
    sols.iter()
        .map(|s| s.activations.iter().map(|a| (a.index, a.output)).collect())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Seeded generators (Pcg64Dxsm, literal seeds only; R3)
// ---------------------------------------------------------------------------------------------

pub struct Rng(pub Pcg64Dxsm);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(Pcg64Dxsm::from_u64_seed(seed))
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.0.next_bounded(n).unwrap_or(0)
    }

    pub fn idx(&mut self, n: usize) -> usize {
        usize::try_from(self.below(n as u64)).unwrap_or(0)
    }

    /// True with probability 1/n.
    pub fn one_in(&mut self, n: u64) -> bool {
        self.below(n) == 0
    }

    pub fn mana_type(&mut self) -> ManaType {
        TYPES[self.idx(TYPES.len())]
    }

    /// A random (possibly empty) subset of `TAGS`.
    pub fn tags(&mut self) -> Vec<&'static str> {
        TAGS.iter().copied().filter(|_| self.one_in(2)).collect()
    }

    pub fn restriction(&mut self) -> ManaRestriction {
        let t = self.tags();
        ManaRestriction::only_for(t)
    }

    pub fn rider(&mut self) -> ManaRider {
        ManaRider::new(["test.rider_a", "test.rider_b"][self.idx(2)])
    }

    /// A pool of 0-3 units, some snow, restricted or with riders.
    pub fn pool(&mut self) -> ManaPool {
        let mut p = ManaPool::default();
        for k in 0..self.below(4) {
            let mut u = ManaUnit::new(self.mana_type(), obj(1000 + k));
            if self.one_in(4) {
                u = u.snow();
            }
            if self.one_in(4) {
                let r = self.restriction();
                u = u.restricted(r);
            }
            if self.one_in(4) {
                let r = self.rider();
                u = u.with_rider(r);
            }
            p.add(u);
        }
        p
    }

    /// One output of 0-2 types (rarely 0), sometimes restricted or with a rider.
    pub fn output(&mut self) -> ManaOutput {
        let n = if self.one_in(12) { 0 } else { 1 + self.idx(2) };
        let types: Vec<ManaType> = (0..n).map(|_| self.mana_type()).collect();
        let mut o = ManaOutput::new(&types);
        if self.one_in(5) {
            let r = self.restriction();
            o = o.restricted(r);
        }
        if self.one_in(5) {
            let r = self.rider();
            o = o.with_rider(r);
        }
        o
    }

    /// Up to `max_entries` entries with 0-2 outputs each (rarely 0); sources collide often so
    /// the `{T}` rule matters; one in four entries repeats an earlier listing exactly.
    pub fn sources(&mut self, max_entries: usize) -> Vec<ManaSource> {
        let n = self.idx(max_entries + 1);
        let mut v: Vec<ManaSource> = Vec::new();
        for _ in 0..n {
            if !v.is_empty() && self.one_in(4) {
                let k = self.idx(v.len());
                let again = v[k].clone();
                v.push(again);
                continue;
            }
            let outputs_n = if self.one_in(15) { 0 } else { 1 + self.idx(2) };
            let outputs = (0..outputs_n).map(|_| self.output()).collect();
            let id = ["test.ability_a", "test.ability_b"][self.idx(2)];
            let mut s = ManaSource::new(obj(1 + self.below(4)), id, outputs);
            if self.one_in(4) {
                s = s.snow();
            }
            if self.one_in(2) {
                s = s.taps();
            }
            v.push(s);
        }
        v
    }

    /// A small mixed cost: generic 0-2 and 0-1 of each other symbol, at most 4 symbols.
    pub fn cost(&mut self) -> ManaComponent {
        let mut c = ManaComponent {
            generic: self.below(3),
            ..ManaComponent::default()
        };
        let mut left = 4u64.saturating_sub(c.generic);
        for slot in 0..7 {
            if left == 0 {
                break;
            }
            if self.one_in(4) {
                left -= 1;
                match slot {
                    0 => c.white += 1,
                    1 => c.blue += 1,
                    2 => c.black += 1,
                    3 => c.red += 1,
                    4 => c.green += 1,
                    5 => c.colorless += 1,
                    _ => c.snow += 1,
                }
            }
        }
        c
    }

    pub fn purpose(&mut self) -> SpendPurpose {
        let t = self.tags();
        SpendPurpose::new(t)
    }
}

/// A pool holding `pool` plus every output of every considered entry (ignoring `{T}`).
pub fn everything(pool: &ManaPool, sources: &[ManaSource]) -> ManaPool {
    let mut p = pool.clone();
    for s in sources.iter().take(MAX_SOURCES) {
        for k in 0..s.outputs.len() {
            for u in expected_units(s, k) {
                p.add(u);
            }
        }
    }
    p
}
