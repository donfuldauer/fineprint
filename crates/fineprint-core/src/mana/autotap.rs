// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The auto-tap solver (REQ-API-002 `previewAutoTap`; CR 106.4, 106.6a, 107.4a-c, 107.4h,
//! 118.3, 118.3c, 601.2g, 601.2h, 605.3a-c). The input, the rank key and the budget are
//! documented on the [`mana`](super) module and on [`preview_auto_tap`].

use fineprint_base::ids::ObjectId;

use super::{
    ManaComponent, ManaPool, ManaRestriction, ManaRider, ManaType, ManaUnit, PaymentPlan,
    SpendPurpose, plan_payment,
};

/// Only the first `MAX_SOURCES` entries of the `sources` slice are considered by
/// [`preview_auto_tap`] (REQ-API-002, REQ-CORE-006).
pub const MAX_SOURCES: usize = 64;

/// The most solutions [`preview_auto_tap`] returns; `max_solutions` is clamped to this
/// (REQ-API-002).
pub const MAX_SOLUTIONS: usize = 32;

/// The number of candidate evaluations (calls of [`plan_payment`] on a candidate pool) one
/// [`preview_auto_tap`] call may make (REQ-API-002, REQ-CORE-006).
pub const SEARCH_BUDGET: usize = 200_000;

/// One alternative result of a mana ability (REQ-API-002): the mana one activation adds when the
/// ability produces this alternative (CR 106.4, 605.3a).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManaOutput {
    /// One mana unit per entry, added to the pool in this order (CR 106.4).
    pub types: Vec<ManaType>,
    /// Spend restrictions applied to every unit of this output (CR 106.6, 106.6a).
    pub restrictions: Vec<ManaRestriction>,
    /// Riders carried by every unit of this output (CR 106.6, 106.6a).
    pub riders: Vec<ManaRider>,
}

impl ManaOutput {
    /// An output of one unit per entry of `types`, with no restrictions and no riders
    /// (REQ-API-002; CR 106.4).
    #[must_use]
    pub fn new(types: &[ManaType]) -> ManaOutput {
        ManaOutput {
            types: types.to_vec(),
            restrictions: Vec::new(),
            riders: Vec::new(),
        }
    }

    /// The same output with one more spend restriction on every unit (REQ-API-002;
    /// CR 106.6a).
    #[must_use]
    pub fn restricted(mut self, r: ManaRestriction) -> ManaOutput {
        self.restrictions.push(r);
        self
    }

    /// The same output with one more rider on every unit (REQ-API-002; CR 106.6a).
    #[must_use]
    pub fn with_rider(mut self, r: ManaRider) -> ManaOutput {
        self.riders.push(r);
        self
    }
}

/// One activation the player could make now: one listed mana ability (REQ-API-002;
/// CR 605.3a), described by the host.
///
/// The host lists only abilities it can activate now whose only cost is an optional `{T}`. The
/// same `(source, ability_id)` may be listed more than once; each entry is one activation
/// (CR 605.3b, 605.3c).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManaSource {
    /// The object whose ability this is (CR 106.3).
    pub source: ObjectId,
    /// The ability's identifier, as answered in `PAY_MANA_AND_COSTS`.
    pub ability_id: String,
    /// Whether the source is a snow source, so its mana can pay `{S}` (CR 107.4h).
    pub source_is_snow: bool,
    /// Whether `{T}` is in the ability's cost: two used entries with `uses_tap` that share
    /// `source` cannot both be activated (CR 118.3).
    pub uses_tap: bool,
    /// The alternatives: one activation produces exactly one of them.
    pub outputs: Vec<ManaOutput>,
}

impl ManaSource {
    /// A non-snow entry without `{T}` (REQ-API-002).
    #[must_use]
    pub fn new(source: ObjectId, ability_id: &str, outputs: Vec<ManaOutput>) -> ManaSource {
        ManaSource {
            source,
            ability_id: ability_id.to_owned(),
            source_is_snow: false,
            uses_tap: false,
            outputs,
        }
    }

    /// The same entry from a snow source (REQ-API-002; CR 107.4h).
    #[must_use]
    pub fn snow(mut self) -> ManaSource {
        self.source_is_snow = true;
        self
    }

    /// The same entry with `{T}` in its cost (REQ-API-002; CR 118.3).
    #[must_use]
    pub fn taps(mut self) -> ManaSource {
        self.uses_tap = true;
        self
    }
}

/// One activation of a solution (REQ-API-002; CR 605.3a).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoTapActivation {
    /// The entry's position in the `sources` slice.
    pub index: usize,
    /// The entry's source, copied from it.
    pub source: ObjectId,
    /// The entry's ability identifier, copied from it.
    pub ability_id: String,
    /// The chosen alternative: an index into the entry's `outputs`.
    pub output: usize,
}

/// One way to pay a cost (REQ-API-002): the mana abilities to activate, in order, the units
/// they add, and the payment plan on the resulting pool (CR 601.2g, 601.2h).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoTapSolution {
    /// The activations, ascending by `index`; activate them in this order.
    pub activations: Vec<AutoTapActivation>,
    /// The units the activations add, in activation order, each output's `types` in order
    /// (CR 106.4, 106.6a).
    pub produced: Vec<ManaUnit>,
    /// The result of [`plan_payment`] on [`AutoTapSolution::pool_after`]; indices are into that
    /// pool (CR 601.2h).
    pub plan: PaymentPlan,
}

impl AutoTapSolution {
    /// The number of activations (REQ-API-002).
    #[must_use]
    pub fn sources_used(&self) -> usize {
        self.activations.len()
    }

    /// A clone of `pool` with [`AutoTapSolution::produced`] appended in order (REQ-API-002;
    /// CR 106.4): the pool the payment is planned on.
    #[must_use]
    pub fn pool_after(&self, pool: &ManaPool) -> ManaPool {
        let mut after = pool.clone();
        for u in &self.produced {
            after.add(u.clone());
        }
        after
    }
}

/// The units activating `entry` with `output` adds (CR 106.6a: every unit carries the output's
/// restrictions and riders; CR 107.4h: snow-ness comes from the source).
fn produce(entry: &ManaSource, output: &ManaOutput, out: &mut Vec<ManaUnit>) {
    for &t in &output.types {
        out.push(ManaUnit {
            mana_type: t,
            source: entry.source,
            source_is_snow: entry.source_is_snow,
            restrictions: output.restrictions.clone(),
            riders: output.riders.clone(),
        });
    }
}

/// The cost demand per slot: the six types, then `{S}`, then generic.
fn demand(cost: &ManaComponent) -> [u64; 8] {
    [
        cost.white,
        cost.blue,
        cost.black,
        cost.red,
        cost.green,
        cost.colorless,
        cost.snow,
        cost.generic,
    ]
}

/// Whether a unit of type `t` from a snow (or non-snow) source allowed for the purpose can pay
/// some symbol of `cost` (CR 107.4a-c, 107.4h). Units for which this is false are never spent by
/// [`plan_payment`].
fn could_pay(t: ManaType, snow: bool, d: &[u64; 8]) -> bool {
    let own = d.get(t.index()).copied().unwrap_or(0) > 0;
    let generic = d.get(7).copied().unwrap_or(0) > 0;
    let s = snow && d.get(6).copied().unwrap_or(0) > 0;
    own || generic || s
}

/// The number of units of `output` that could be spent on `cost` for `purpose` (CR 106.6).
fn useful_units(
    entry: &ManaSource,
    output: &ManaOutput,
    d: &[u64; 8],
    purpose: &SpendPurpose,
) -> u64 {
    if !output.restrictions.iter().all(|r| r.allows(purpose)) {
        return 0;
    }
    output
        .types
        .iter()
        .filter(|&&t| could_pay(t, entry.source_is_snow, d))
        .fold(0u64, |acc, _| acc.saturating_add(1))
}

/// A usable entry after pruning.
struct Entry {
    /// Position in the `sources` slice.
    index: usize,
    /// The outputs that could contribute a spent unit, ascending.
    outputs: Vec<usize>,
    /// The number of distinct types over all the entry's outputs (rank key K3).
    flex: usize,
}

/// A valid candidate with its rank key (K1 is the level).
struct Scored {
    wasted: usize,
    flex: usize,
    pairs: Vec<(usize, usize)>,
    solution: AutoTapSolution,
}

/// The search state of one [`preview_auto_tap`] call.
struct Search<'a> {
    pool: &'a ManaPool,
    sources: &'a [ManaSource],
    cost: &'a ManaComponent,
    purpose: &'a SpendPurpose,
    entries: Vec<Entry>,
    evaluations: usize,
    exhausted: bool,
    chosen: Vec<(usize, usize)>,
    tapped: Vec<ObjectId>,
    found: Vec<Scored>,
}

impl Search<'_> {
    /// The most entries that can still be picked from `entries[from..]` given the sources
    /// already tapped (CR 118.3: one `{T}` activation per source object).
    fn capacity(&self, from: usize) -> usize {
        let mut free: usize = 0;
        let mut sources: Vec<ObjectId> = Vec::new();
        for e in self.entries.get(from..).unwrap_or(&[]) {
            let Some(src) = self.sources.get(e.index) else {
                continue;
            };
            if !src.uses_tap {
                free = free.saturating_add(1);
            } else if !self.tapped.contains(&src.source) {
                sources.push(src.source);
            }
        }
        sources.sort_unstable();
        sources.dedup();
        free.saturating_add(sources.len())
    }

    /// Enumerates every candidate of `need` more activations from `entries[from..]`, in
    /// ascending `(index, output)` order, and evaluates it.
    fn walk(&mut self, from: usize, need: usize) {
        if self.exhausted {
            return;
        }
        if need == 0 {
            self.evaluate();
            return;
        }
        for pos in from..self.entries.len() {
            if self.exhausted || self.capacity(pos) < need {
                return;
            }
            let Some(e) = self.entries.get(pos) else {
                return;
            };
            let index = e.index;
            let outputs = e.outputs.clone();
            let Some(src) = self.sources.get(index) else {
                continue;
            };
            // CR 118.3: a `{T}` cost can be paid once per source object.
            let taps = src.uses_tap;
            let source = src.source;
            if taps && self.tapped.contains(&source) {
                continue;
            }
            if taps {
                self.tapped.push(source);
            }
            for o in outputs {
                self.chosen.push((pos, o));
                self.walk(pos.saturating_add(1), need - 1);
                self.chosen.pop();
                if self.exhausted {
                    break;
                }
            }
            if taps {
                self.tapped.pop();
            }
        }
    }

    /// Evaluates the current candidate with [`plan_payment`] (CR 601.2h) and keeps it when it is
    /// valid: payable, and every activation has a spent unit.
    fn evaluate(&mut self) {
        if self.evaluations >= SEARCH_BUDGET {
            self.exhausted = true;
            return;
        }
        self.evaluations = self.evaluations.saturating_add(1);
        let mut produced = Vec::new();
        let mut activations = Vec::with_capacity(self.chosen.len());
        let mut ranges = Vec::with_capacity(self.chosen.len());
        let mut pairs = Vec::with_capacity(self.chosen.len());
        let mut flex: usize = 0;
        for &(pos, o) in &self.chosen {
            let Some(e) = self.entries.get(pos) else {
                return;
            };
            let Some(src) = self.sources.get(e.index) else {
                return;
            };
            let Some(output) = src.outputs.get(o) else {
                return;
            };
            let start = self.pool.len().saturating_add(produced.len());
            produce(src, output, &mut produced);
            let end = self.pool.len().saturating_add(produced.len());
            ranges.push((start, end));
            activations.push(AutoTapActivation {
                index: e.index,
                source: src.source,
                ability_id: src.ability_id.clone(),
                output: o,
            });
            pairs.push((e.index, o));
            flex = flex.saturating_add(e.flex);
        }
        let solution = AutoTapSolution {
            activations,
            produced,
            plan: PaymentPlan::default(),
        };
        let after = solution.pool_after(self.pool);
        // CR 601.2h: only complete payments; the plan is exactly what payment will compute.
        let Some(plan) = plan_payment(&after, self.cost, self.purpose) else {
            return;
        };
        // No wasted activation: each one must contribute a spent unit.
        let contributes = ranges
            .iter()
            .all(|&(s, e)| plan.spent.iter().any(|&i| i >= s && i < e));
        if !contributes {
            return;
        }
        let spent_produced = plan.spent.iter().filter(|&&i| i >= self.pool.len()).count();
        let wasted = solution.produced.len().saturating_sub(spent_produced);
        self.found.push(Scored {
            wasted,
            flex,
            pairs,
            solution: AutoTapSolution { plan, ..solution },
        });
    }
}

/// Previews the ways to pay `cost` for `purpose` from `pool` and the listed mana abilities,
/// best first (REQ-API-002 `previewAutoTap`; CR 118.3, 118.3c, 601.2g, 601.2h, 605.3a-c).
///
/// **Input.** `pool` is the player's mana pool (CR 106.4). `sources` lists the mana abilities
/// the player could activate now (CR 605.3a), one [`ManaSource`] per activation; only the first
/// [`MAX_SOURCES`] entries are considered, and an entry with no outputs is never used. Each
/// entry is activated at most once, choosing one of its outputs; two used entries with
/// `uses_tap` cannot share a source (CR 118.3). Activating mana abilities is optional
/// (CR 118.3c); a solution may use none.
///
/// **Validity.** A candidate is returned only if [`plan_payment`] pays `cost` from
/// [`AutoTapSolution::pool_after`] (complete payments only, CR 601.2h) and every activation
/// contributes at least one spent unit. Every solution is therefore accepted when its
/// activations are answered, in order, as the `mana_abilities` of `PAY_MANA_AND_COSTS`
/// (`casting::pay_total_cost`). An unpayable cost gives an empty result; a `{0}` cost gives the
/// one solution with no activations.
///
/// **Rank.** Ascending by K1 the number of activations, K2 the produced units left unspent, K3
/// the sum over the activations of the number of distinct mana types the entry could make
/// (multi-color sources are kept for later), K4 the `(index, output)` sequence (stable
/// tie-break). The best `min(max_solutions, MAX_SOLUTIONS)` are returned; the result for a
/// smaller `max_solutions` is a prefix of the one for a larger.
///
/// **Search and budget.** Iterative deepening on K1; outputs that cannot contribute a spendable
/// unit and levels that counts prove unreachable are skipped (this never changes the result).
/// At most [`SEARCH_BUDGET`] candidates are evaluated with [`plan_payment`]; when the budget
/// runs out the valid candidates found so far are returned, ranked by the same key. The result
/// is exact (complete and optimal) whenever the search ends within the budget, which is
/// guaranteed for at most 10 entries with at most 2 outputs each. It is always sound.
///
/// Pure and deterministic (REQ-CORE-001, REQ-CORE-006): no I/O, clock, global state, hash
/// container or floating point; the inputs are not mutated; it does not panic on any input.
#[must_use]
pub fn preview_auto_tap(
    pool: &ManaPool,
    sources: &[ManaSource],
    cost: &ManaComponent,
    purpose: &SpendPurpose,
    max_solutions: usize,
) -> Vec<AutoTapSolution> {
    let limit = max_solutions.min(MAX_SOLUTIONS);
    if limit == 0 {
        return Vec::new();
    }
    let d = demand(cost);
    let total: u64 = d.iter().fold(0u64, |a, &x| a.saturating_add(x));
    let considered = sources.get(..MAX_SOURCES).unwrap_or(sources);

    // Prune: keep only outputs that could contribute a spent unit (CR 106.6, 107.4).
    let mut entries = Vec::new();
    let mut best_useful: Vec<u64> = Vec::new();
    for (index, src) in considered.iter().enumerate() {
        let mut outputs = Vec::new();
        let mut best: u64 = 0;
        for (o, output) in src.outputs.iter().enumerate() {
            let n = useful_units(src, output, &d, purpose);
            if n > 0 {
                outputs.push(o);
                best = best.max(n);
            }
        }
        if outputs.is_empty() {
            continue;
        }
        let mut seen = [false; 6];
        for t in src.outputs.iter().flat_map(|o| o.types.iter()) {
            if let Some(s) = seen.get_mut(t.index()) {
                *s = true;
            }
        }
        let flex = seen.iter().filter(|s| **s).count();
        entries.push(Entry {
            index,
            outputs,
            flex,
        });
        best_useful.push(best);
    }

    // Soundness pre-check: the pool with every usable output added is a superset of every
    // candidate pool; if it cannot pay, nothing can (CR 601.2h).
    let mut all = pool.clone();
    let mut scratch = Vec::new();
    for e in &entries {
        if let Some(src) = considered.get(e.index) {
            for &o in &e.outputs {
                if let Some(output) = src.outputs.get(o) {
                    produce(src, output, &mut scratch);
                }
            }
        }
    }
    for u in scratch {
        all.add(u);
    }
    if plan_payment(&all, cost, purpose).is_none() {
        return Vec::new();
    }

    // Level bound: each activation needs a spent unit and exactly `total` units are spent.
    let pool_useful = pool
        .units()
        .iter()
        .filter(|u| u.allowed_for(purpose) && could_pay(u.mana_type, u.source_is_snow, &d))
        .fold(0u64, |a, _| a.saturating_add(1));
    best_useful.sort_unstable_by(|a, b| b.cmp(a));
    let max_level = usize::try_from(total)
        .unwrap_or(usize::MAX)
        .min(entries.len());

    let mut search = Search {
        pool,
        sources: considered,
        cost,
        purpose,
        entries,
        evaluations: 0,
        exhausted: false,
        chosen: Vec::new(),
        tapped: Vec::new(),
        found: Vec::new(),
    };
    let mut result: Vec<AutoTapSolution> = Vec::new();
    let mut reach = pool_useful;
    for level in 0..=max_level {
        if level > 0 {
            let add = best_useful.get(level - 1).copied().unwrap_or(0);
            reach = reach.saturating_add(add);
        }
        // Counts prove this level unreachable: fewer spendable units than the cost needs.
        if reach < total {
            continue;
        }
        search.walk(0, level);
        let mut level_found = core::mem::take(&mut search.found);
        level_found.sort_by(|a, b| (a.wasted, a.flex, &a.pairs).cmp(&(b.wasted, b.flex, &b.pairs)));
        result.extend(level_found.into_iter().map(|s| s.solution));
        if result.len() >= limit || search.exhausted {
            break;
        }
    }
    result.truncate(limit);
    result
}

#[cfg(test)]
mod tests {
    // Unit and property tests of the auto-tap solver: REQ-API-002 (`previewAutoTap`),
    // REQ-CORE-001 and REQ-CORE-006 (determinism). CR 106.4, 106.6a, 107.4a-c, 107.4h, 118.3,
    // 118.3c, 601.2h, 605.3a-c. Synthetic sources, neutral tags and rider ids only (R5).

    use fineprint_base::prng::Pcg64Dxsm;

    use super::*;

    use ManaType::{Black, Blue, Colorless, Green, Red, White};

    fn o(n: u64) -> ObjectId {
        ObjectId::new(n)
    }

    fn pool_of(units: Vec<ManaUnit>) -> ManaPool {
        let mut p = ManaPool::new();
        for u in units {
            p.add(u);
        }
        p
    }

    fn cost(f: impl FnOnce(&mut ManaComponent)) -> ManaComponent {
        let mut c = ManaComponent::default();
        f(&mut c);
        c
    }

    fn any() -> SpendPurpose {
        SpendPurpose::new(["spell"])
    }

    /// A non-tap entry from object `id` with one output per slice.
    fn src(id: u64, outs: &[&[ManaType]]) -> ManaSource {
        ManaSource::new(
            o(id),
            "test.add_mana",
            outs.iter().map(|t| ManaOutput::new(t)).collect(),
        )
    }

    fn pairs(s: &AutoTapSolution) -> Vec<(usize, usize)> {
        s.activations.iter().map(|a| (a.index, a.output)).collect()
    }

    fn all_pairs(v: &[AutoTapSolution]) -> Vec<Vec<(usize, usize)>> {
        v.iter().map(pairs).collect()
    }

    /// The plan is what `plan_payment` computes on the pool after the activations (rule 3).
    fn assert_accepted(pool: &ManaPool, s: &AutoTapSolution, c: &ManaComponent, p: &SpendPurpose) {
        assert_eq!(
            plan_payment(&s.pool_after(pool), c, p),
            Some(s.plan.clone())
        );
    }

    // -----------------------------------------------------------------------------------------
    // Rule 1: produced units and pool_after
    // -----------------------------------------------------------------------------------------

    /// REQ-API-002, CR 106.4, 106.6a, 107.4h: an activation adds one unit per type of the
    /// chosen output, in order, each with the output's restrictions and riders and the
    /// source's snow-ness; `pool_after` appends them after the existing units.
    #[test]
    fn req_api_002_produced_units_and_pool_after() {
        let pool = pool_of(vec![ManaUnit::new(White, o(9))]);
        let r = ManaRestriction::only_for(["spell"]);
        let entry = ManaSource::new(
            o(5),
            "test.dual",
            vec![
                ManaOutput::new(&[Red, Green])
                    .restricted(r.clone())
                    .with_rider(ManaRider::new("test.rider")),
            ],
        )
        .snow()
        .taps();
        let c = cost(|c| {
            c.white = 1;
            c.red = 1;
            c.green = 1;
        });
        let sols = preview_auto_tap(&pool, &[entry], &c, &any(), 4);
        assert_eq!(sols.len(), 1);
        let s = &sols[0];
        let expect_unit = |t| ManaUnit {
            mana_type: t,
            source: o(5),
            source_is_snow: true,
            restrictions: vec![r.clone()],
            riders: vec![ManaRider::new("test.rider")],
        };
        assert_eq!(s.produced, vec![expect_unit(Red), expect_unit(Green)]);
        assert_eq!(
            s.activations,
            vec![AutoTapActivation {
                index: 0,
                source: o(5),
                ability_id: "test.dual".to_owned(),
                output: 0,
            }]
        );
        assert_eq!(s.sources_used(), 1);
        let after = s.pool_after(&pool);
        assert_eq!(after.len(), 3);
        assert_eq!(after.units()[0], ManaUnit::new(White, o(9)));
        assert_eq!(after.units()[1], expect_unit(Red));
        assert_eq!(after.units()[2], expect_unit(Green));
        assert_eq!(s.plan.spent, vec![0, 1, 2]);
        assert_accepted(&pool, s, &c, &any());
    }

    /// REQ-API-002: the builders set exactly the documented fields.
    #[test]
    fn req_api_002_builders() {
        let out = ManaOutput::new(&[Blue, Blue]);
        assert_eq!(out.types, vec![Blue, Blue]);
        assert!(out.restrictions.is_empty() && out.riders.is_empty());
        let s = ManaSource::new(o(3), "test.a", vec![out.clone()]);
        assert!(!s.source_is_snow && !s.uses_tap);
        assert_eq!(s.ability_id, "test.a");
        let s = s.snow().taps();
        assert!(s.source_is_snow && s.uses_tap);
        assert_eq!(s.outputs, vec![out]);
    }

    // -----------------------------------------------------------------------------------------
    // Rule 2: candidates
    // -----------------------------------------------------------------------------------------

    /// REQ-API-002, CR 118.3: two `{T}` entries of the same object cannot both be used.
    #[test]
    fn req_api_002_tap_conflict() {
        let a = src(1, &[&[Red]]).taps();
        let b = ManaSource::new(o(1), "test.other", vec![ManaOutput::new(&[Green])]).taps();
        let c = cost(|c| {
            c.red = 1;
            c.green = 1;
        });
        assert!(
            preview_auto_tap(&ManaPool::new(), &[a.clone(), b.clone()], &c, &any(), 8).is_empty()
        );
        // Without `{T}` on one of them, both may be activated.
        let b2 = ManaSource {
            uses_tap: false,
            ..b
        };
        let sols = preview_auto_tap(&ManaPool::new(), &[a, b2], &c, &any(), 8);
        assert_eq!(all_pairs(&sols), vec![vec![(0, 0), (1, 0)]]);
        // Different objects never conflict.
        let a = src(1, &[&[Red]]).taps();
        let b = src(2, &[&[Green]]).taps();
        let sols = preview_auto_tap(&ManaPool::new(), &[a, b], &c, &any(), 8);
        assert_eq!(all_pairs(&sols), vec![vec![(0, 0), (1, 0)]]);
    }

    /// REQ-API-002, CR 605.3b, 605.3c: a repeated listing of the same ability is one activation
    /// per listing.
    #[test]
    fn req_api_002_repeated_listing() {
        let e = src(1, &[&[Red]]);
        let c = cost(|c| c.red = 2);
        let sols = preview_auto_tap(&ManaPool::new(), &[e.clone(), e.clone()], &c, &any(), 8);
        assert_eq!(all_pairs(&sols), vec![vec![(0, 0), (1, 0)]]);
        assert_eq!(sols[0].activations[0].source, sols[0].activations[1].source);
        assert_eq!(
            sols[0].activations[0].ability_id,
            sols[0].activations[1].ability_id
        );
        // A `{T}` ability listed twice can be used once only.
        let t = e.taps();
        assert!(preview_auto_tap(&ManaPool::new(), &[t.clone(), t], &c, &any(), 8).is_empty());
    }

    /// REQ-API-002: entries with no outputs, and outputs with no types, are never used.
    #[test]
    fn req_api_002_empty_outputs_never_used() {
        let empty = ManaSource::new(o(1), "test.none", Vec::new());
        let no_types = src(2, &[&[]]);
        let real = src(3, &[&[], &[Green]]);
        let c = cost(|c| c.generic = 1);
        let sols = preview_auto_tap(&ManaPool::new(), &[empty, no_types, real], &c, &any(), 8);
        assert_eq!(all_pairs(&sols), vec![vec![(2, 1)]]);
    }

    /// REQ-API-002, REQ-CORE-006: entries at positions `>= MAX_SOURCES` are never used.
    #[test]
    fn req_api_002_entries_beyond_max_sources() {
        let mut sources: Vec<ManaSource> = (0..MAX_SOURCES as u64)
            .map(|i| src(i, &[&[White]]))
            .collect();
        sources.push(src(1000, &[&[Red]]));
        let c = cost(|c| c.red = 1);
        assert!(preview_auto_tap(&ManaPool::new(), &sources, &c, &any(), 8).is_empty());
        // The same entry inside the window is used.
        sources.swap(0, MAX_SOURCES);
        let sols = preview_auto_tap(&ManaPool::new(), &sources, &c, &any(), 8);
        assert_eq!(all_pairs(&sols), vec![vec![(0, 0)]]);
    }

    /// REQ-API-002: activations are listed ascending by index.
    #[test]
    fn req_api_002_activations_ascending() {
        let sources = vec![src(1, &[&[Green]]), src(2, &[&[Red]]), src(3, &[&[Blue]])];
        let c = cost(|c| {
            c.blue = 1;
            c.green = 1;
            c.red = 1;
        });
        let sols = preview_auto_tap(&ManaPool::new(), &sources, &c, &any(), 8);
        assert_eq!(all_pairs(&sols), vec![vec![(0, 0), (1, 0), (2, 0)]]);
        assert_eq!(
            sols[0]
                .produced
                .iter()
                .map(|u| u.mana_type)
                .collect::<Vec<_>>(),
            vec![Green, Red, Blue]
        );
        assert_accepted(&ManaPool::new(), &sols[0], &c, &any());
    }

    // -----------------------------------------------------------------------------------------
    // Rule 3: validity
    // -----------------------------------------------------------------------------------------

    /// REQ-API-002, CR 601.2h: an activation whose units the plan would not spend is wasted, so
    /// that candidate is not a solution.
    #[test]
    fn req_api_002_no_wasted_activation() {
        // The pool's unrestricted unit is spent first (lowest index), so tapping for G is waste.
        let pool = pool_of(vec![ManaUnit::new(Red, o(9))]);
        let c = cost(|c| c.generic = 1);
        let sols = preview_auto_tap(&pool, &[src(1, &[&[Green]])], &c, &any(), 8);
        assert_eq!(all_pairs(&sols), vec![Vec::new()]);
        // A restricted produced unit is spent before unrestricted pool mana (the planner's
        // tie-break), so here the activation contributes and is a second solution.
        let restricted = ManaSource::new(
            o(1),
            "test.r",
            vec![ManaOutput::new(&[Green]).restricted(ManaRestriction::only_for(["spell"]))],
        );
        let sols = preview_auto_tap(&pool, &[restricted], &c, &any(), 8);
        assert_eq!(all_pairs(&sols), vec![Vec::new(), vec![(0, 0)]]);
        assert_eq!(sols[1].plan.spent, vec![1]);
        for s in &sols {
            assert_accepted(&pool, s, &c, &any());
        }
    }

    /// REQ-API-002, CR 107.4h: a snow entry's mana pays `{S}`, a non-snow one does not.
    #[test]
    fn req_api_002_snow_payment() {
        let c = cost(|c| c.snow = 1);
        let sols = preview_auto_tap(
            &ManaPool::new(),
            &[src(1, &[&[Red]]), src(2, &[&[Red]]).snow()],
            &c,
            &any(),
            8,
        );
        assert_eq!(all_pairs(&sols), vec![vec![(1, 0)]]);
        assert!(sols[0].produced[0].source_is_snow);
    }

    // -----------------------------------------------------------------------------------------
    // Rule 4: unpayable costs
    // -----------------------------------------------------------------------------------------

    /// REQ-API-002, CR 601.2h, 107.4a-c, 107.4h, 106.6: unpayable costs give no solution.
    #[test]
    fn req_api_002_unpayable_costs() {
        let empty = ManaPool::new();
        let sources = vec![src(1, &[&[Red], &[Green]]), src(2, &[&[Blue]]).taps()];
        // Colored shortfall.
        let c = cost(|c| c.white = 1);
        assert!(preview_auto_tap(&empty, &sources, &c, &any(), 8).is_empty());
        // {C} shortfall: colored mana cannot pay {C}.
        let c = cost(|c| c.colorless = 1);
        assert!(preview_auto_tap(&empty, &sources, &c, &any(), 8).is_empty());
        // {S} shortfall: no snow source.
        let c = cost(|c| c.snow = 1);
        assert!(preview_auto_tap(&empty, &sources, &c, &any(), 8).is_empty());
        // Sum shortfall: two activations make two mana at most.
        let c = cost(|c| c.generic = 3);
        assert!(preview_auto_tap(&empty, &sources, &c, &any(), 8).is_empty());
        // One output per activation: R and G of the same entry cannot both be made.
        let c = cost(|c| {
            c.red = 1;
            c.green = 1;
        });
        assert!(preview_auto_tap(&empty, &sources, &c, &any(), 8).is_empty());
        // Restricted-only mana not allowed for the purpose, in the pool or produced.
        let r = ManaRestriction::only_for(["ability"]);
        let pool = pool_of(vec![ManaUnit::new(Red, o(9)).restricted(r.clone())]);
        let restricted = vec![ManaSource::new(
            o(1),
            "test.r",
            vec![ManaOutput::new(&[Red]).restricted(r)],
        )];
        let c = cost(|c| c.red = 1);
        assert!(preview_auto_tap(&pool, &restricted, &c, &any(), 8).is_empty());
        // A restriction with no tags allows nothing.
        let none = vec![ManaSource::new(
            o(1),
            "test.r",
            vec![ManaOutput::new(&[Red]).restricted(ManaRestriction::only_for([]))],
        )];
        assert!(preview_auto_tap(&empty, &none, &c, &any(), 8).is_empty());
    }

    /// REQ-API-002, REQ-CORE-006, R12: huge amounts return empty without hanging or panicking.
    #[test]
    fn req_api_002_huge_cost_is_empty() {
        let sources: Vec<ManaSource> = (0..80).map(|i| src(i, &[&[Red], &[Green]])).collect();
        let pool = pool_of(vec![ManaUnit::new(Red, o(9))]);
        for f in [
            |c: &mut ManaComponent| c.generic = u64::MAX,
            |c: &mut ManaComponent| c.red = u64::MAX,
            |c: &mut ManaComponent| c.snow = u64::MAX,
            |c: &mut ManaComponent| c.colorless = u64::MAX,
            |c: &mut ManaComponent| {
                c.white = u64::MAX;
                c.blue = u64::MAX;
                c.black = u64::MAX;
                c.red = u64::MAX;
                c.green = u64::MAX;
                c.colorless = u64::MAX;
                c.snow = u64::MAX;
                c.generic = u64::MAX;
            },
        ] {
            let c = cost(f);
            assert!(preview_auto_tap(&pool, &sources, &c, &any(), usize::MAX).is_empty());
        }
    }

    // -----------------------------------------------------------------------------------------
    // Rule 5: {0} and pool-only
    // -----------------------------------------------------------------------------------------

    /// REQ-API-002, CR 118.3c: `{0}` gives exactly the empty solution.
    #[test]
    fn req_api_002_zero_cost() {
        let sources = vec![src(1, &[&[Red]]), src(2, &[&[Green]])];
        let pool = pool_of(vec![ManaUnit::new(Red, o(9))]);
        let sols = preview_auto_tap(&pool, &sources, &ManaComponent::default(), &any(), 8);
        assert_eq!(sols.len(), 1);
        assert!(sols[0].activations.is_empty());
        assert!(sols[0].produced.is_empty());
        assert_eq!(sols[0].plan, PaymentPlan::default());
        assert_eq!(sols[0].sources_used(), 0);
        assert_eq!(sols[0].pool_after(&pool), pool);
        let sols = preview_auto_tap(&ManaPool::new(), &[], &ManaComponent::default(), &any(), 1);
        assert_eq!(sols.len(), 1);
    }

    /// REQ-API-002, CR 118.3c: when the pool alone pays, the first solution has no activation;
    /// activations whose mana the planner spends may follow.
    #[test]
    fn req_api_002_pool_only_first() {
        let pool = pool_of(vec![ManaUnit::new(Red, o(9))]);
        let r = ManaRestriction::only_for(["spell"]);
        let sources = vec![
            src(1, &[&[Red]]),
            ManaSource::new(o(2), "test.r", vec![ManaOutput::new(&[Red]).restricted(r)]),
        ];
        let c = cost(|c| c.red = 1);
        let sols = preview_auto_tap(&pool, &sources, &c, &any(), 8);
        assert_eq!(all_pairs(&sols), vec![Vec::new(), vec![(1, 0)]]);
        assert_eq!(sols[0].plan.spent, vec![0]);
    }

    // -----------------------------------------------------------------------------------------
    // Rule 6: rank and stable order
    // -----------------------------------------------------------------------------------------

    /// REQ-API-002: K1, fewer activations first, even at the cost of more waste.
    #[test]
    fn req_api_002_rank_k1_fewest_sources() {
        let sources = vec![
            src(1, &[&[Green]]),
            src(2, &[&[Green]]),
            src(3, &[&[Green, Green, Green]]),
        ];
        let c = cost(|c| c.green = 2);
        let sols = preview_auto_tap(&ManaPool::new(), &sources, &c, &any(), 8);
        // Level 2 follows, ranked by waste: {0, 1} wastes nothing, {0, 2} and {1, 2} waste two.
        assert_eq!(
            all_pairs(&sols),
            vec![
                vec![(2, 0)],
                vec![(0, 0), (1, 0)],
                vec![(0, 0), (2, 0)],
                vec![(1, 0), (2, 0)]
            ]
        );
        assert_eq!(sols[0].sources_used(), 1);
        assert_eq!(sols[1].sources_used(), 2);
    }

    /// REQ-API-002: K2, fewer produced-but-unspent units first.
    #[test]
    fn req_api_002_rank_k2_least_waste() {
        let sources = vec![src(1, &[&[Green, Green]]), src(2, &[&[Green]])];
        let c = cost(|c| c.green = 1);
        let sols = preview_auto_tap(&ManaPool::new(), &sources, &c, &any(), 8);
        assert_eq!(all_pairs(&sols), vec![vec![(1, 0)], vec![(0, 0)]]);
    }

    /// REQ-API-002: K3, an entry that could make more colors is tapped later (colors kept
    /// flexible); the count covers all outputs of the entry, used or not.
    #[test]
    fn req_api_002_rank_k3_keep_colors_flexible() {
        let sources = vec![
            src(1, &[&[Green], &[Red], &[Blue]]),
            src(2, &[&[Green], &[Green]]),
        ];
        let c = cost(|c| c.green = 1);
        let sols = preview_auto_tap(&ManaPool::new(), &sources, &c, &any(), 8);
        assert_eq!(
            all_pairs(&sols),
            vec![vec![(1, 0)], vec![(1, 1)], vec![(0, 0)]]
        );
    }

    /// REQ-API-002: K4, ties by the `(index, output)` sequence.
    #[test]
    fn req_api_002_rank_k4_stable_tie_break() {
        let sources = vec![
            src(1, &[&[Red]]),
            src(2, &[&[Black], &[Black]]),
            src(3, &[&[Red]]),
        ];
        let c = cost(|c| c.generic = 1);
        let sols = preview_auto_tap(&ManaPool::new(), &sources, &c, &any(), 8);
        // Entry 1 has flex 1 too (one distinct type).
        assert_eq!(
            all_pairs(&sols),
            vec![vec![(0, 0)], vec![(1, 0)], vec![(1, 1)], vec![(2, 0)]]
        );
    }

    /// REQ-API-002: `max_solutions` 0, 1 and above `MAX_SOLUTIONS`, and the prefix property.
    #[test]
    fn req_api_002_max_solutions_and_prefix() {
        let sources: Vec<ManaSource> = (0..10).map(|i| src(i, &[&[Green], &[Red]])).collect();
        let c = cost(|c| c.generic = 2);
        let pool = ManaPool::new();
        assert!(preview_auto_tap(&pool, &sources, &c, &any(), 0).is_empty());
        let one = preview_auto_tap(&pool, &sources, &c, &any(), 1);
        assert_eq!(one.len(), 1);
        let big = preview_auto_tap(&pool, &sources, &c, &any(), MAX_SOLUTIONS + 100);
        assert_eq!(big.len(), MAX_SOLUTIONS);
        assert_eq!(
            big,
            preview_auto_tap(&pool, &sources, &c, &any(), usize::MAX)
        );
        for n in 0..=MAX_SOLUTIONS {
            let r = preview_auto_tap(&pool, &sources, &c, &any(), n);
            assert_eq!(r.len(), n);
            assert_eq!(r[..], big[..n]);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Rule 7: search budget
    // -----------------------------------------------------------------------------------------

    /// REQ-API-002, REQ-CORE-006: a search that reaches `SEARCH_BUDGET` still returns only
    /// valid solutions, ranked, and the internal counter stops at the budget.
    #[test]
    fn req_api_002_search_budget_sound() {
        let sources: Vec<ManaSource> = (0..70).map(|i| src(i, &[&[Green]])).collect();
        let c = cost(|c| c.generic = 5);
        let pool = ManaPool::new();
        let sols = preview_auto_tap(&pool, &sources, &c, &any(), MAX_SOLUTIONS);
        assert_eq!(sols.len(), MAX_SOLUTIONS);
        assert_eq!(
            pairs(&sols[0]),
            vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]
        );
        for s in &sols {
            assert_eq!(s.sources_used(), 5);
            assert!(s.activations.iter().all(|a| a.index < MAX_SOURCES));
            check_accepted(&pool, &sources, s, &c, &any());
        }
        // The budget is what stopped it: drive the search directly.
        let mut search = Search {
            pool: &pool,
            sources: &sources[..MAX_SOURCES],
            cost: &c,
            purpose: &any(),
            entries: (0..MAX_SOURCES)
                .map(|index| Entry {
                    index,
                    outputs: vec![0],
                    flex: 1,
                })
                .collect(),
            evaluations: 0,
            exhausted: false,
            chosen: Vec::new(),
            tapped: Vec::new(),
            found: Vec::new(),
        };
        search.walk(0, 5);
        assert!(search.exhausted);
        assert_eq!(search.evaluations, SEARCH_BUDGET);
        assert_eq!(search.found.len(), SEARCH_BUDGET);
    }

    // -----------------------------------------------------------------------------------------
    // Rule 8: determinism and safety
    // -----------------------------------------------------------------------------------------

    /// REQ-CORE-001, REQ-CORE-006, R12: same inputs, same output; inputs are not mutated; odd
    /// inputs do not panic.
    #[test]
    fn req_api_002_determinism_and_odd_inputs() {
        let pool = pool_of(vec![ManaUnit::new(Red, o(u64::MAX)).snow()]);
        let sources = vec![
            ManaSource::new(o(u64::MAX), "", Vec::new()),
            src(u64::MAX, &[&[]]).taps(),
            src(u64::MAX, &[&[Colorless], &[Red, Red]]).taps(),
            src(0, &[&[Colorless]]).snow(),
            ManaSource::new(
                o(7),
                "test.x",
                vec![ManaOutput::new(&[White]).restricted(ManaRestriction::only_for([]))],
            ),
        ];
        let c = cost(|c| {
            c.colorless = 1;
            c.snow = 1;
            c.generic = 1;
        });
        let (pool0, sources0, c0, p0) = (pool.clone(), sources.clone(), c, any());
        let a = preview_auto_tap(&pool, &sources, &c, &p0, 32);
        let b = preview_auto_tap(&pool, &sources, &c, &p0, 32);
        assert_eq!(a, b);
        assert!(!a.is_empty());
        assert_eq!(pool, pool0);
        assert_eq!(sources, sources0);
        assert_eq!(c, c0);
        for s in &a {
            check_accepted(&pool, &sources, s, &c, &p0);
        }
        // Empty everything.
        assert!(
            preview_auto_tap(
                &ManaPool::new(),
                &[],
                &cost(|c| c.red = 1),
                &SpendPurpose::default(),
                8
            )
            .is_empty()
        );
        // Many entries, empty purpose: restricted pool mana is never spent.
        let many: Vec<ManaSource> = (0..200).map(|i| src(i, &[&[Red], &[]])).collect();
        let r = preview_auto_tap(
            &ManaPool::new(),
            &many,
            &cost(|c| c.red = 2),
            &SpendPurpose::default(),
            3,
        );
        assert_eq!(
            all_pairs(&r),
            vec![
                vec![(0, 0), (1, 0)],
                vec![(0, 0), (2, 0)],
                vec![(0, 0), (3, 0)]
            ]
        );
    }

    // -----------------------------------------------------------------------------------------
    // Rule 9: restrictions and riders
    // -----------------------------------------------------------------------------------------

    /// REQ-API-002, CR 106.6, 106.6a: an output not allowed for the purpose never appears; the
    /// riders of spent units are reported as `plan_payment` reports them.
    #[test]
    fn req_api_002_restrictions_and_riders() {
        let only_ability = ManaRestriction::only_for(["ability"]);
        let sources = vec![
            ManaSource::new(
                o(1),
                "test.split",
                vec![
                    ManaOutput::new(&[Red, Red]).restricted(only_ability),
                    ManaOutput::new(&[Red]).with_rider(ManaRider::new("test.rider")),
                ],
            ),
            src(2, &[&[Red]]),
        ];
        let c = cost(|c| c.red = 1);
        let sols = preview_auto_tap(&ManaPool::new(), &sources, &c, &any(), 8);
        // The restricted output (0, 0) never appears.
        assert_eq!(all_pairs(&sols), vec![vec![(0, 1)], vec![(1, 0)]]);
        assert!(sols[1].plan.riders_fired.is_empty());
        assert_eq!(
            sols[0].plan.riders_fired,
            vec![(0, vec![ManaRider::new("test.rider")])]
        );
        // With the purpose allowing it, the restricted output is usable.
        let ab = SpendPurpose::new(["ability"]);
        let sols = preview_auto_tap(&ManaPool::new(), &sources, &cost(|c| c.red = 2), &ab, 8);
        assert_eq!(all_pairs(&sols), vec![vec![(0, 0)], vec![(0, 1), (1, 0)]]);
    }

    // -----------------------------------------------------------------------------------------
    // Property tests
    // -----------------------------------------------------------------------------------------

    /// Each activation's produced units sit at consecutive indices after the pool; at least one
    /// of them is spent, and the plan is what `plan_payment` computes (rule 3).
    fn check_accepted(
        pool: &ManaPool,
        sources: &[ManaSource],
        s: &AutoTapSolution,
        c: &ManaComponent,
        p: &SpendPurpose,
    ) {
        assert_eq!(
            plan_payment(&s.pool_after(pool), c, p),
            Some(s.plan.clone())
        );
        let mut produced = Vec::new();
        let mut start = pool.len();
        let mut last: Option<usize> = None;
        for a in &s.activations {
            assert!(
                last.is_none_or(|l| l < a.index),
                "activations ascending and distinct"
            );
            last = Some(a.index);
            let e = &sources[a.index];
            assert_eq!(
                (a.source, a.ability_id.as_str()),
                (e.source, e.ability_id.as_str())
            );
            produce(e, &e.outputs[a.output], &mut produced);
            let end = pool.len() + produced.len();
            assert!(
                s.plan.spent.iter().any(|&i| i >= start && i < end),
                "wasted activation"
            );
            start = end;
        }
        assert_eq!(produced, s.produced);
        let tapped: Vec<ObjectId> = s
            .activations
            .iter()
            .filter(|a| sources[a.index].uses_tap)
            .map(|a| a.source)
            .collect();
        let mut dedup = tapped.clone();
        dedup.sort_unstable();
        dedup.dedup();
        assert_eq!(
            dedup.len(),
            tapped.len(),
            "two {{T}} activations of one object"
        );
    }

    fn below(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
        rng.next_bounded(n).unwrap_or(0)
    }

    fn rand_type(rng: &mut Pcg64Dxsm) -> ManaType {
        ManaType::ALL[below(rng, 6) as usize]
    }

    const TAGS: [&str; 3] = ["spell", "ability", "x_cost"];

    fn rand_restriction(rng: &mut Pcg64Dxsm) -> ManaRestriction {
        let n = below(rng, 3) as usize;
        let tags: Vec<&str> = (0..n).map(|_| TAGS[below(rng, 3) as usize]).collect();
        ManaRestriction::only_for(tags)
    }

    struct Case {
        pool: ManaPool,
        sources: Vec<ManaSource>,
        cost: ManaComponent,
        purpose: SpendPurpose,
    }

    fn rand_case(rng: &mut Pcg64Dxsm, max_entries: u64, max_outputs: u64) -> Case {
        let mut pool = ManaPool::new();
        for _ in 0..below(rng, 4) {
            let mut u = ManaUnit::new(rand_type(rng), o(100 + below(rng, 3)));
            if below(rng, 3) == 0 {
                u = u.snow();
            }
            if below(rng, 4) == 0 {
                u = u.restricted(rand_restriction(rng));
            }
            if below(rng, 4) == 0 {
                u = u.with_rider(ManaRider::new("test.pool_rider"));
            }
            pool.add(u);
        }
        let mut sources = Vec::new();
        for _ in 0..=below(rng, max_entries) {
            let mut outputs = Vec::new();
            for _ in 0..below(rng, max_outputs + 1) {
                let n = below(rng, 3);
                let types: Vec<ManaType> = (0..n).map(|_| rand_type(rng)).collect();
                let mut out = ManaOutput::new(&types);
                if below(rng, 4) == 0 {
                    out = out.restricted(rand_restriction(rng));
                }
                if below(rng, 4) == 0 {
                    out = out.with_rider(ManaRider::new("test.rider"));
                }
                outputs.push(out);
            }
            let id = below(rng, 5);
            let mut e = ManaSource::new(
                o(id),
                if below(rng, 2) == 0 {
                    "test.a"
                } else {
                    "test.b"
                },
                outputs,
            );
            if below(rng, 3) == 0 {
                e = e.snow();
            }
            if below(rng, 2) == 0 {
                e = e.taps();
            }
            sources.push(e);
        }
        let mut c = ManaComponent::default();
        for _ in 0..below(rng, 5) {
            match below(rng, 8) {
                0 => c.white += 1,
                1 => c.blue += 1,
                2 => c.black += 1,
                3 => c.red += 1,
                4 => c.green += 1,
                5 => c.colorless += 1,
                6 => c.snow += 1,
                _ => c.generic += 1,
            }
        }
        let n = below(rng, 3) as usize;
        let purpose = SpendPurpose::new((0..n).map(|_| TAGS[below(rng, 3) as usize]));
        Case {
            pool,
            sources,
            cost: c,
            purpose,
        }
    }

    /// K1, K2, K3, K4 of rule 6.
    type RankKey = (usize, usize, usize, Vec<(usize, usize)>);

    /// The reference: every candidate (each entry unused or with one output), no pruning,
    /// validity by rule 3, sorted by K1..K4, truncated.
    fn brute_force(case: &Case, max_solutions: usize) -> Vec<AutoTapSolution> {
        let n = case.sources.len().min(MAX_SOURCES);
        let mut choice = vec![0usize; n]; // 0 = unused, k = output k-1
        let mut found: Vec<(RankKey, AutoTapSolution)> = Vec::new();
        loop {
            let picked: Vec<(usize, usize)> = (0..n)
                .filter(|&i| choice[i] > 0)
                .map(|i| (i, choice[i] - 1))
                .collect();
            let mut taps: Vec<ObjectId> = picked
                .iter()
                .filter(|&&(i, _)| case.sources[i].uses_tap)
                .map(|&(i, _)| case.sources[i].source)
                .collect();
            let t = taps.len();
            taps.sort_unstable();
            taps.dedup();
            if taps.len() == t {
                let mut produced = Vec::new();
                let mut ranges = Vec::new();
                for &(i, k) in &picked {
                    let s = case.pool.len() + produced.len();
                    produce(&case.sources[i], &case.sources[i].outputs[k], &mut produced);
                    ranges.push((s, case.pool.len() + produced.len()));
                }
                let mut after = case.pool.clone();
                for u in &produced {
                    after.add(u.clone());
                }
                if let Some(plan) = plan_payment(&after, &case.cost, &case.purpose)
                    && ranges
                        .iter()
                        .all(|&(s, e)| plan.spent.iter().any(|&x| x >= s && x < e))
                {
                    let spent_prod = plan.spent.iter().filter(|&&x| x >= case.pool.len()).count();
                    let k3: usize = picked
                        .iter()
                        .map(|&(i, _)| {
                            let mut ts: Vec<ManaType> = case.sources[i]
                                .outputs
                                .iter()
                                .flat_map(|o| o.types.clone())
                                .collect();
                            ts.sort_unstable();
                            ts.dedup();
                            ts.len()
                        })
                        .sum();
                    let key = (
                        picked.len(),
                        produced.len() - spent_prod,
                        k3,
                        picked.clone(),
                    );
                    let activations = picked
                        .iter()
                        .map(|&(i, k)| AutoTapActivation {
                            index: i,
                            source: case.sources[i].source,
                            ability_id: case.sources[i].ability_id.clone(),
                            output: k,
                        })
                        .collect();
                    found.push((
                        key,
                        AutoTapSolution {
                            activations,
                            produced,
                            plan,
                        },
                    ));
                }
            }
            // Next choice vector (odometer).
            let mut i = 0;
            loop {
                if i == n {
                    found.sort_by(|a, b| a.0.cmp(&b.0));
                    return found
                        .into_iter()
                        .map(|f| f.1)
                        .take(max_solutions.min(MAX_SOLUTIONS))
                        .collect();
                }
                choice[i] += 1;
                if choice[i] <= case.sources[i].outputs.len() {
                    break;
                }
                choice[i] = 0;
                i += 1;
            }
        }
    }

    /// The pool with every output of every considered entry added (a superset of every
    /// candidate pool).
    fn everything(case: &Case) -> ManaPool {
        let mut all = case.pool.clone();
        let mut units = Vec::new();
        for e in case.sources.iter().take(MAX_SOURCES) {
            for out in &e.outputs {
                produce(e, out, &mut units);
            }
        }
        for u in units {
            all.add(u);
        }
        all
    }

    /// REQ-API-002 property (a): every returned solution is accepted when submitted, and
    /// property (b): no solution for a cost unpayable even with every output added.
    /// CR 601.2h, 118.3.
    #[test]
    fn req_api_002_prop_solutions_accepted_and_unpayable_empty() {
        let mut rng = Pcg64Dxsm::from_u64_seed(0x0000_0055_a070_0001);
        let mut nonempty = 0;
        let mut empty_unpayable = 0;
        for _ in 0..400 {
            let case = rand_case(&mut rng, 12, 3);
            let sols = preview_auto_tap(&case.pool, &case.sources, &case.cost, &case.purpose, 16);
            for s in &sols {
                check_accepted(&case.pool, &case.sources, s, &case.cost, &case.purpose);
            }
            if !sols.is_empty() {
                nonempty += 1;
            }
            if plan_payment(&everything(&case), &case.cost, &case.purpose).is_none() {
                assert!(sols.is_empty());
                empty_unpayable += 1;
            }
        }
        assert!(
            nonempty > 50 && empty_unpayable > 20,
            "{nonempty} {empty_unpayable}"
        );
    }

    /// REQ-API-002 property (c): for at most 8 entries with at most 2 outputs each the search
    /// is exact: the same ordered list as the brute-force reference, for several limits.
    #[test]
    fn req_api_002_prop_matches_brute_force() {
        let mut rng = Pcg64Dxsm::from_u64_seed(0x0000_0055_a070_0002);
        let mut nonempty = 0;
        for _ in 0..300 {
            let case = rand_case(&mut rng, 8, 2);
            for limit in [1, 5, MAX_SOLUTIONS] {
                let got =
                    preview_auto_tap(&case.pool, &case.sources, &case.cost, &case.purpose, limit);
                let want = brute_force(&case, limit);
                assert_eq!(got, want);
                if !got.is_empty() && limit == MAX_SOLUTIONS {
                    nonempty += 1;
                }
            }
        }
        assert!(nonempty > 50, "{nonempty}");
    }

    /// REQ-API-002, REQ-CORE-006: the prefix property and determinism on random inputs.
    #[test]
    fn req_api_002_prop_prefix_and_determinism() {
        let mut rng = Pcg64Dxsm::from_u64_seed(0x0000_0055_a070_0003);
        for _ in 0..100 {
            let case = rand_case(&mut rng, 10, 3);
            let full = preview_auto_tap(
                &case.pool,
                &case.sources,
                &case.cost,
                &case.purpose,
                MAX_SOLUTIONS,
            );
            assert_eq!(
                full,
                preview_auto_tap(
                    &case.pool,
                    &case.sources,
                    &case.cost,
                    &case.purpose,
                    MAX_SOLUTIONS
                )
            );
            for n in [0, 1, 2, 7] {
                let r = preview_auto_tap(&case.pool, &case.sources, &case.cost, &case.purpose, n);
                assert_eq!(r[..], full[..n.min(full.len())]);
            }
        }
    }
}
