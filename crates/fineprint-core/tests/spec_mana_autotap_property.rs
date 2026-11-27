// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Seeded property tests for the auto-tap solver `preview_auto_tap` (REQ-API-002,
//! REQ-CORE-006; CR 106.4, 106.6, 106.6a, 107.4a, 107.4b, 107.4c, 107.4h, 118.3, 118.3a,
//! 601.2g, 601.2h, 605.3a, 605.3b, 605.3c): validity, exactness against a brute-force
//! reference, the unpayable property, and acceptance of every returned solution by the payment
//! procedure `pay_total_cost` on a fake host. Black-box: written from the work-item brief only.
//! Randomness is `Pcg64Dxsm::from_u64_seed` over literal seeds only (R3). Every name is invented
//! and neutral (R5).

#[path = "spec_mana_autotap_support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use fineprint_base::hash::{Digest, leaf_digest};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::casting::{
    CostBasis, LockedCost, ManaActivation, ManaComponent, ManaPart, PayError, PayOutcome, PayState,
    PaymentMethodOffer, determine_total_cost, pay_total_cost,
};
use fineprint_core::choice::{ChoiceContext, ChoiceResponse, ChoiceSession, Interrupt, Status};
use fineprint_core::layers::CostStages;
use fineprint_core::mana::{
    AutoTapSolution, MAX_SOLUTIONS, ManaPool, ManaSource, ManaUnit, SpendPurpose, plan_payment,
    preview_auto_tap,
};
use fineprint_core::memory::SpellHistory;
use fineprint_core::txn::{Transactional, Transactions};
use fineprint_dsl::ability::CostAST;
use fineprint_dsl::mana::ManaCostDSL;
use support::*;

const SEEDS: [u64; 3] = [0x5eed_0001, 0x5eed_0002, 0x5eed_0003];
const P1: PlayerId = PlayerId::new(1);
const SPELL: ObjectId = ObjectId::new(100);

// ---------------------------------------------------------------------------------------------
// Requirement 3 / 12: validity and the unpayable property
// ---------------------------------------------------------------------------------------------

/// REQ-API-002, REQ-CORE-006 (requirements 3, 9, 12): over 3 seeds x 400 random cases (up to 12
/// entries) every returned solution is valid and ranked, the call is deterministic and pure, and
/// when even every output of every entry (ignoring `{T}`) cannot pay, the result is empty
/// (CR 601.2h, 106.6, 107.4a-c, 107.4h, 118.3).
#[test]
fn req_api_002_req_core_006_property_valid_and_unpayable() {
    // CR 601.2h, CR 106.6, CR 107.4a, CR 107.4b, CR 107.4c, CR 107.4h, CR 118.3
    let mut unpayable_cases = 0usize;
    let mut payable_cases = 0usize;
    for seed in SEEDS {
        let mut g = Rng::new(seed);
        for _ in 0..400 {
            let pool = g.pool();
            let sources = g.sources(12);
            let cost = g.cost();
            let p = g.purpose();
            let max = [MAX_SOLUTIONS, 1, 5, 100][g.idx(4)];
            let sols = solve(&pool, &sources, &cost, &p, max);
            assert_eq!(sols, preview_auto_tap(&pool, &sources, &cost, &p, max));
            if plan_payment(&everything(&pool, &sources), &cost, &p).is_none() {
                unpayable_cases += 1;
                assert!(sols.is_empty(), "unpayable but solved: {sols:?}");
            } else if !sols.is_empty() {
                payable_cases += 1;
            }
        }
    }
    assert!(
        unpayable_cases > 50,
        "unpayable cases exercised: {unpayable_cases}"
    );
    assert!(
        payable_cases > 50,
        "payable cases exercised: {payable_cases}"
    );
}

// ---------------------------------------------------------------------------------------------
// Requirement 10: exactness against the reference
// ---------------------------------------------------------------------------------------------

/// REQ-API-002 (requirement 10): for at most 8 entries with at most 2 outputs each, the result
/// equals the brute-force reference (every subset, every output, the `{T}` rule, validity via
/// `plan_payment`, sorted by K1-K4, truncated), over 3 seeds x 300 cases; an empty result means
/// no valid candidate exists (CR 601.2h, 118.3, 605.3b).
#[test]
fn req_api_002_property_exact_against_reference() {
    // CR 601.2h, CR 118.3, CR 605.3b
    let mut nonempty = 0usize;
    let mut multi = 0usize;
    for seed in SEEDS {
        let mut g = Rng::new(seed ^ 0xa11);
        for case in 0..300 {
            let pool = g.pool();
            let sources = g.sources(8);
            let cost = g.cost();
            let p = g.purpose();
            let max = [MAX_SOLUTIONS, 0, 1, 2, 7, 1000][g.idx(6)];
            let got = solve(&pool, &sources, &cost, &p, max);
            let want = reference(&pool, &sources, &cost, &p, max);
            assert_eq!(
                got, want,
                "seed {seed:#x} case {case}: pool {pool:?} sources {sources:?} cost {cost:?} purpose {p:?} max {max}"
            );
            if reference(&pool, &sources, &cost, &p, MAX_SOLUTIONS).is_empty() {
                assert!(preview_auto_tap(&pool, &sources, &cost, &p, MAX_SOLUTIONS).is_empty());
            } else {
                nonempty += 1;
            }
            multi += usize::from(got.len() > 1);
        }
    }
    assert!(nonempty > 100, "non-empty cases exercised: {nonempty}");
    assert!(multi > 30, "multi-solution cases exercised: {multi}");
}

/// REQ-API-002 (requirement 7): the prefix property over random inputs: for every `n < m` in
/// 0..=34, the result for `n` is a prefix of the result for `m` (CR 601.2h).
#[test]
fn req_api_002_property_prefix() {
    // CR 601.2h
    for seed in SEEDS {
        let mut g = Rng::new(seed ^ 0xbee);
        for _ in 0..40 {
            let pool = g.pool();
            let sources = g.sources(8);
            let cost = g.cost();
            let p = g.purpose();
            let all: Vec<Vec<AutoTapSolution>> = (0..=34)
                .map(|n| preview_auto_tap(&pool, &sources, &cost, &p, n))
                .collect();
            for n in 0..all.len() {
                assert!(all[n].len() <= n);
                for m in n + 1..all.len() {
                    assert_eq!(all[n].as_slice(), &all[m][..all[n].len()]);
                    if all[n].len() < n {
                        assert_eq!(all[m].len(), all[n].len(), "a short list is complete");
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Requirement 11: acceptance by the payment procedure
// ---------------------------------------------------------------------------------------------

/// One scripted activation: the units it adds and whether its cost includes `{T}`.
type Scripted = (Vec<ManaUnit>, bool);

/// The fake host: one player's pool, the spend purpose, and per `(source, ability id)` a queue
/// of scripted activations (the units it adds, whether its cost includes `{T}`), consumed in
/// order so a repeated listing can add a different chosen output each time.
#[derive(Clone, Debug)]
struct Host {
    pool: ManaPool,
    empty: ManaPool,
    purpose: SpendPurpose,
    scripts: BTreeMap<(ObjectId, String), VecDeque<Scripted>>,
    /// Sources tapped by a `{T}` activation (CR 118.3).
    tapped: BTreeSet<ObjectId>,
    activated: Vec<(ObjectId, String)>,
    cast: bool,
}

impl Host {
    fn new(pool: &ManaPool, purpose: &SpendPurpose) -> Host {
        Host {
            pool: pool.clone(),
            empty: ManaPool::default(),
            purpose: purpose.clone(),
            scripts: BTreeMap::new(),
            tapped: BTreeSet::new(),
            activated: Vec::new(),
            cast: false,
        }
    }

    /// Scripts the solution's activations: each adds exactly the units its chosen output
    /// describes.
    fn scripted(mut self, sources: &[ManaSource], s: &AutoTapSolution) -> Host {
        for a in &s.activations {
            let src = &sources[a.index];
            self.scripts
                .entry((a.source, a.ability_id.clone()))
                .or_default()
                .push_back((expected_units(src, a.output), src.uses_tap));
        }
        self
    }

    fn blocked(&self, key: &(ObjectId, String)) -> bool {
        self.scripts
            .get(key)
            .and_then(VecDeque::front)
            .is_none_or(|(_, taps)| *taps && self.tapped.contains(&key.0))
    }
}

impl Transactional for Host {
    fn state_hash(&self) -> Digest {
        leaf_digest(format!("{self:?}").as_bytes())
    }

    fn priority_player(&self) -> Option<PlayerId> {
        Some(P1)
    }

    fn carry_durable(restored: Host, _before: &Host, _after: &Host) -> Host {
        restored
    }
}

impl PayState for Host {
    fn mana_pool(&self, player: PlayerId) -> &ManaPool {
        if player == P1 {
            &self.pool
        } else {
            &self.empty
        }
    }

    fn mana_pool_mut(&mut self, player: PlayerId) -> &mut ManaPool {
        if player == P1 {
            &mut self.pool
        } else {
            &mut self.empty
        }
    }

    fn available_mana_abilities(&self, _player: PlayerId) -> Vec<(ObjectId, String)> {
        self.scripts
            .keys()
            .filter(|k| !self.blocked(k))
            .cloned()
            .collect()
    }

    fn activate_mana_ability(
        &mut self,
        player: PlayerId,
        source: ObjectId,
        ability_id: &str,
    ) -> Result<ManaActivation, PayError> {
        let key = (source, ability_id.to_owned());
        if self.blocked(&key) {
            return Err(PayError::Unpayable);
        }
        let Some((units, taps)) = self.scripts.get_mut(&key).and_then(VecDeque::pop_front) else {
            return Err(PayError::Illegal("test: unknown mana ability".to_owned()));
        };
        if taps {
            self.tapped.insert(source);
        }
        self.activated.push(key);
        let pool = self.mana_pool_mut(player);
        for u in units {
            pool.add(u);
        }
        Ok(ManaActivation {
            library_touching: false,
        })
    }

    fn cost_is_late(&self, _player: PlayerId, _item: &CostAST) -> bool {
        false
    }

    fn pay_cost_item(
        &mut self,
        _player: PlayerId,
        _stack_object: ObjectId,
        _item: &CostAST,
    ) -> Result<ManaActivation, PayError> {
        Err(PayError::Illegal("test: no cost items".to_owned()))
    }

    fn pay_life(&mut self, _player: PlayerId, amount: u64) -> Result<(), PayError> {
        if amount == 0 {
            Ok(())
        } else {
            Err(PayError::Unpayable)
        }
    }

    fn payment_methods(
        &self,
        _player: PlayerId,
        _stack_object: ObjectId,
        _cost: &LockedCost,
    ) -> Vec<PaymentMethodOffer> {
        Vec::new()
    }

    fn apply_payment_method(
        &mut self,
        _player: PlayerId,
        _stack_object: ObjectId,
        _offer: &PaymentMethodOffer,
        _objects: &[ObjectId],
        _remaining: &ManaComponent,
    ) -> Result<ManaComponent, PayError> {
        Err(PayError::Illegal("test: no payment methods".to_owned()))
    }

    fn record_cast(&mut self, _player: PlayerId, _stack_object: ObjectId, _history: SpellHistory) {
        self.cast = true;
    }

    fn queue_cast_triggers(&mut self, _player: PlayerId, _stack_object: ObjectId) {}

    fn purpose(&self, _player: PlayerId, _stack_object: ObjectId) -> SpendPurpose {
        self.purpose.clone()
    }
}

/// The cost as mana symbols: generic first, then W, U, B, R, G, `{C}`, `{S}`; `{0}` when zero.
fn cost_text(c: &ManaComponent) -> String {
    let mut s = String::new();
    if c.generic > 0 {
        s.push_str(&format!("{{{}}}", c.generic));
    }
    for (n, sym) in [
        (c.white, "{W}"),
        (c.blue, "{U}"),
        (c.black, "{B}"),
        (c.red, "{R}"),
        (c.green, "{G}"),
        (c.colorless, "{C}"),
        (c.snow, "{S}"),
    ] {
        for _ in 0..n {
            s.push_str(sym);
        }
    }
    if s.is_empty() {
        s.push_str("{0}");
    }
    s
}

/// The locked total cost whose mana part is exactly `cost` (CR 601.2f), as `spec_casting_pay.rs`
/// derives it.
fn locked(cost: &ManaComponent) -> LockedCost {
    let mc = ManaCostDSL::parse(&cost_text(cost)).expect("parses");
    let basis = CostBasis::from_mana_cost(Some(&mc), None, &[], Vec::new()).expect("basis");
    let lc = determine_total_cost(&basis, &[], &CostStages::default(), &[]).expect("locked");
    assert_eq!(lc.mana(), &ManaPart::Payable(*cost), "the locked mana part");
    lc
}

/// Submits `s` to `pay_total_cost` on a fresh fake host: activates the solution's mana abilities
/// in order in one answer, then (CR-015) answers the follow-up request with a plain payment.
/// Returns the outcome (`None` for a procedure error) and the final host.
fn submit(
    pool: &ManaPool,
    sources: &[ManaSource],
    cost: &ManaComponent,
    purpose: &SpendPurpose,
    s: &AutoTapSolution,
) -> (Option<PayOutcome>, Host) {
    let host = Host::new(pool, purpose).scripted(sources, s);
    let lc = locked(cost);
    let procedure = move |st: &mut Host,
                          ctx: &mut ChoiceContext<'_>|
          -> Result<Option<PayOutcome>, Interrupt> {
        let mut txn = Transactions::new(st.clone());
        let cp = txn.checkpoint();
        let r = pay_total_cost(&mut txn, cp, ctx, P1, SPELL, &lc, SpellHistory::default());
        let out = match r {
            Err(Interrupt::Pending(p)) => return Err(Interrupt::Pending(p)),
            Err(_) => None,
            Ok(o) => Some(o),
        };
        *st = txn.state().clone();
        Ok(out)
    };
    let mut session = ChoiceSession::start(host, procedure).expect("starts");
    if let Some(req) = session.pending() {
        let id = req.request_id;
        let answer = ChoiceResponse::PayCosts {
            auto_pay: false,
            methods: Vec::new(),
            mana_abilities: s
                .activations
                .iter()
                .map(|a| (a.source, a.ability_id.clone()))
                .collect(),
        };
        session.submit(id, answer).expect("a valid answer");
        // CR-015 (CR 601.2g, 605.3a): an answer that activated at least one listed ability is
        // followed by a new PAY_MANA_AND_COSTS request; the plain answer ends the window and pays.
        if !s.activations.is_empty() {
            let id = session
                .pending()
                .expect("CR-015: a second request after the activating answer")
                .request_id;
            let plain = ChoiceResponse::PayCosts {
                auto_pay: false,
                methods: Vec::new(),
                mana_abilities: Vec::new(),
            };
            session.submit(id, plain).expect("a valid answer");
        }
    } else {
        // CR 601.2g: nothing to ask only when there is nothing to pay.
        assert!(cost.is_zero(), "no ask for {cost:?}");
        assert!(s.activations.is_empty());
    }
    match session.status() {
        Status::Done { state, output } => (output.clone(), state.clone()),
        Status::Pending(r) => panic!("still pending: {:?}", r.payload),
    }
}

/// REQ-API-002 (requirement 11, the issue's acceptance property "every returned solution is
/// accepted when submitted"): over 3 seeds x 300 random cases (pools with restricted, snow and
/// plain units, random entries, mixed costs, random purposes), every returned solution, submitted
/// to `pay_total_cost` by listing its activations in order, commits and spends exactly the units
/// `plan.spent` names in `pool_after`, in that order (CR 601.2g, 601.2h, 605.3a, 605.3b, 605.3c,
/// 118.3, 118.3a, 106.6, 106.6a).
#[test]
fn req_api_002_property_every_solution_accepted_when_submitted() {
    // CR 601.2g, CR 601.2h, CR 605.3a, CR 605.3b, CR 605.3c, CR 118.3, CR 118.3a, CR 106.6,
    // CR 106.6a
    let mut submitted = 0usize;
    let mut with_activations = 0usize;
    for seed in SEEDS {
        let mut g = Rng::new(seed ^ 0xacc);
        for case in 0..300 {
            let pool = g.pool();
            let sources = g.sources(7);
            let cost = g.cost();
            let p = g.purpose();
            let sols = solve(&pool, &sources, &cost, &p, MAX_SOLUTIONS);
            for s in &sols {
                let (outcome, host) = submit(&pool, &sources, &cost, &p, s);
                let after = s.pool_after(&pool);
                let want: Vec<ManaUnit> = s
                    .plan
                    .spent
                    .iter()
                    .map(|&i| after.units()[i].clone())
                    .collect();
                match outcome {
                    Some(PayOutcome::Committed {
                        spent,
                        riders_fired,
                    }) => {
                        assert_eq!(spent, want, "seed {seed:#x} case {case}: {s:?}");
                        let riders: Vec<_> = want.iter().map(|u| u.riders.clone()).collect();
                        assert_eq!(riders_fired, riders, "riders parallel to spent");
                    }
                    other => panic!(
                        "seed {seed:#x} case {case}: not committed {other:?}: pool {pool:?} sources {sources:?} cost {cost:?} solution {s:?}"
                    ),
                }
                assert!(host.cast, "the spell became cast");
                let listed: Vec<(ObjectId, String)> = s
                    .activations
                    .iter()
                    .map(|a| (a.source, a.ability_id.clone()))
                    .collect();
                assert_eq!(host.activated, listed, "each listed ability activated once");
                submitted += 1;
                with_activations += usize::from(!s.activations.is_empty());
            }
        }
    }
    assert!(submitted > 300, "solutions submitted: {submitted}");
    assert!(
        with_activations > 200,
        "with activations: {with_activations}"
    );
}

/// REQ-API-002 (requirement 11): the cost text round-trips through the locked cost for every
/// symbol kind, so the acceptance property pays exactly the cost given to the solver.
#[test]
fn req_api_002_locked_cost_matches_component() {
    // CR 107.4a, CR 107.4b, CR 107.4c, CR 107.4h
    for c in [
        mc(0, 0, 0, 0, 0, 0, 0, 0),
        mc(3, 1, 1, 1, 1, 1, 1, 1),
        mc(0, 2, 0, 0, 0, 0, 2, 2),
        mc(12, 0, 0, 0, 0, 0, 0, 0),
    ] {
        let _ = locked(&c);
    }
}
