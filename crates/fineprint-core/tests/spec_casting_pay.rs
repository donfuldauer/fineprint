// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for SRS 7.1 steps 7-9 of casting: the mana-ability window, paying the locked
//! total cost and the commit, through `casting::pay_total_cost` on a fake host (REQ-CAST-001,
//! REQ-CORE-005; CR 601.2e-i, 603.3, 605.1a-b, 605.3a-c, 605.4a, 106.6, 118.3, 118.3a-c, 118.6,
//! 702.51b, 702.126b, 734.1, 734.2). Black-box: written from the work-item brief only. Every
//! name is invented and neutral (R5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::hash::{Digest, leaf_digest};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::casting::{
    CostBasis, LockedCost, ManaActivation, ManaComponent, PayError, PayOutcome, PayState,
    PayStopReason, PaymentMethodOffer, SymbolChoice, determine_total_cost, pay_total_cost,
};
use fineprint_core::choice::{
    ChoiceContext, ChoicePayload, ChoiceResponse, ChoiceSession, ChoiceType, Interrupt,
    PendingChoiceRequest, Status,
};
use fineprint_core::layers::CostStages;
use fineprint_core::mana::{
    ManaPool, ManaRestriction, ManaRider, ManaType, ManaUnit, SpendPurpose,
};
use fineprint_core::memory::SpellHistory;
use fineprint_core::txn::{Transactional, Transactions};
use fineprint_dsl::ability::CostAST;
use fineprint_dsl::mana::ManaCostDSL;

// ---------------------------------------------------------------------------------------------
// The fake host
// ---------------------------------------------------------------------------------------------

const P1: PlayerId = PlayerId::new(1);
const SPELL: ObjectId = ObjectId::new(100);

fn obj(n: u64) -> ObjectId {
    ObjectId::new(n)
}

/// A scripted mana ability.
#[derive(Clone, Debug, Default)]
struct Script {
    /// Units added when it resolves.
    adds: Vec<ManaUnit>,
    /// A unit of this type is removed from the pool as its cost (a mana ability paid with mana).
    costs: Option<ManaType>,
    /// Reported as library-touching.
    library_touching: bool,
    /// Added to `library_mills` when it resolves.
    mills: u64,
}

/// How a non-mana cost item behaves when paid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemMode {
    Ok,
    OkLibrary,
    Unpayable,
}

/// How the payment-method hook changes the remaining mana.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MethodMode {
    /// One generic less per object used (saturating).
    PerObject,
    /// Tries to make the remaining mana larger (a misbehaving method).
    Inflate,
}

/// The fake host. Every field takes part in the hash; `marks` and `library_mills` are the
/// fields `carry_durable` carries.
#[derive(Clone, Debug)]
struct FakeState {
    prio: Option<PlayerId>,
    pools: BTreeMap<PlayerId, ManaPool>,
    empty_pool: ManaPool,
    life: BTreeMap<PlayerId, u64>,
    waiting_triggers: Vec<String>,
    history: Option<SpellHistory>,
    /// Every host call that changes something, in order.
    calls: Vec<String>,
    activations: BTreeMap<(ObjectId, String), u32>,
    /// A log of effects; carried by `carry_durable` (appended suffix).
    marks: Vec<String>,
    /// A counter of library-touching effects; carried by `carry_durable` (delta).
    library_mills: u64,
    abilities: BTreeMap<(ObjectId, String), Script>,
    items: Vec<(CostAST, bool, ItemMode, String)>,
    purpose_tags: Vec<String>,
    offers: Vec<(String, Vec<ObjectId>)>,
    method_mode: MethodMode,
    used_for_methods: Vec<ObjectId>,
    cast_triggers: Vec<String>,
}

impl FakeState {
    fn new() -> FakeState {
        let mut pools = BTreeMap::new();
        pools.insert(P1, ManaPool::default());
        let mut life = BTreeMap::new();
        life.insert(P1, 20);
        FakeState {
            prio: Some(P1),
            pools,
            empty_pool: ManaPool::default(),
            life,
            waiting_triggers: Vec::new(),
            history: None,
            calls: Vec::new(),
            activations: BTreeMap::new(),
            marks: Vec::new(),
            library_mills: 0,
            abilities: BTreeMap::new(),
            items: Vec::new(),
            purpose_tags: vec!["spell".to_owned()],
            offers: Vec::new(),
            method_mode: MethodMode::PerObject,
            used_for_methods: Vec::new(),
            cast_triggers: vec!["test.cast_trigger_a".to_owned()],
        }
    }

    fn with_units(mut self, units: &[ManaUnit]) -> FakeState {
        let pool = self.pools.entry(P1).or_default();
        for u in units {
            pool.add(u.clone());
        }
        self
    }

    fn with_ability(mut self, source: u64, id: &str, script: Script) -> FakeState {
        self.abilities.insert((obj(source), id.to_owned()), script);
        self
    }

    fn with_item(mut self, item: CostAST, late: bool, mode: ItemMode, label: &str) -> FakeState {
        self.items.push((item, late, mode, label.to_owned()));
        self
    }

    fn pool(&self) -> &ManaPool {
        &self.pools[&P1]
    }

    fn activation_count(&self, source: u64, id: &str) -> u32 {
        self.activations
            .get(&(obj(source), id.to_owned()))
            .copied()
            .unwrap_or(0)
    }

    /// The state without the carried fields, for "the rest equals the checkpoint".
    fn non_durable(&self) -> FakeState {
        let mut s = self.clone();
        s.marks.clear();
        s.library_mills = 0;
        s
    }

    fn item(&self, item: &CostAST) -> Option<&(CostAST, bool, ItemMode, String)> {
        self.items.iter().find(|(i, ..)| i == item)
    }
}

fn remove_one(pool: &mut ManaPool, t: ManaType) -> bool {
    let units = pool.units().to_vec();
    let Some(k) = units.iter().position(|u| u.mana_type == t) else {
        return false;
    };
    let mut rebuilt = ManaPool::default();
    for (i, u) in units.into_iter().enumerate() {
        if i != k {
            rebuilt.add(u);
        }
    }
    *pool = rebuilt;
    true
}

impl Transactional for FakeState {
    fn state_hash(&self) -> Digest {
        leaf_digest(format!("{self:?}").as_bytes())
    }

    fn priority_player(&self) -> Option<PlayerId> {
        self.prio
    }

    fn carry_durable(mut restored: FakeState, before: &FakeState, after: &FakeState) -> FakeState {
        restored.library_mills = restored
            .library_mills
            .saturating_add(after.library_mills.saturating_sub(before.library_mills));
        restored
            .marks
            .extend_from_slice(after.marks.get(before.marks.len()..).unwrap_or_default());
        restored
    }
}

impl PayState for FakeState {
    fn mana_pool(&self, player: PlayerId) -> &ManaPool {
        self.pools.get(&player).unwrap_or(&self.empty_pool)
    }

    fn mana_pool_mut(&mut self, player: PlayerId) -> &mut ManaPool {
        self.calls.push("pool_mut".to_owned());
        self.pools.entry(player).or_default()
    }

    fn available_mana_abilities(&self, _player: PlayerId) -> Vec<(ObjectId, String)> {
        self.abilities.keys().cloned().collect()
    }

    fn activate_mana_ability(
        &mut self,
        player: PlayerId,
        source: ObjectId,
        ability_id: &str,
    ) -> Result<ManaActivation, PayError> {
        let key = (source, ability_id.to_owned());
        let Some(script) = self.abilities.get(&key).cloned() else {
            return Err(PayError::Illegal("test: unknown mana ability".to_owned()));
        };
        if let Some(t) = script.costs
            && !remove_one(self.pools.entry(player).or_default(), t)
        {
            return Err(PayError::Unpayable);
        }
        *self.activations.entry(key).or_insert(0) += 1;
        self.calls.push(format!("activate:{ability_id}"));
        self.marks.push(format!("activate:{ability_id}"));
        self.library_mills += script.mills;
        let pool = self.pools.entry(player).or_default();
        for u in script.adds {
            pool.add(u);
        }
        Ok(ManaActivation {
            library_touching: script.library_touching,
        })
    }

    fn cost_is_late(&self, _player: PlayerId, item: &CostAST) -> bool {
        self.item(item).is_some_and(|(_, late, ..)| *late)
    }

    fn pay_cost_item(
        &mut self,
        _player: PlayerId,
        _stack_object: ObjectId,
        item: &CostAST,
    ) -> Result<ManaActivation, PayError> {
        let Some((_, _, mode, label)) = self.item(item).cloned() else {
            return Err(PayError::Illegal("test: unknown cost item".to_owned()));
        };
        match mode {
            ItemMode::Unpayable => Err(PayError::Unpayable),
            ItemMode::Ok | ItemMode::OkLibrary => {
                self.calls.push(format!("item:{label}"));
                self.marks.push(format!("item:{label}"));
                let library_touching = mode == ItemMode::OkLibrary;
                if library_touching {
                    self.library_mills += 1;
                }
                Ok(ManaActivation { library_touching })
            }
        }
    }

    fn pay_life(&mut self, player: PlayerId, amount: u64) -> Result<(), PayError> {
        let life = self.life.entry(player).or_insert(0);
        if *life < amount {
            return Err(PayError::Unpayable);
        }
        *life -= amount;
        self.calls.push(format!("life:{amount}"));
        Ok(())
    }

    fn payment_methods(
        &self,
        _player: PlayerId,
        _stack_object: ObjectId,
        _cost: &LockedCost,
    ) -> Vec<PaymentMethodOffer> {
        self.offers
            .iter()
            .map(|(label, eligible)| PaymentMethodOffer {
                label: label.clone(),
                eligible: eligible.clone(),
            })
            .collect()
    }

    fn apply_payment_method(
        &mut self,
        _player: PlayerId,
        _stack_object: ObjectId,
        offer: &PaymentMethodOffer,
        objects: &[ObjectId],
        remaining: &ManaComponent,
    ) -> Result<ManaComponent, PayError> {
        self.calls.push(format!("method:{}", offer.label));
        self.used_for_methods.extend_from_slice(objects);
        let mut r = *remaining;
        match self.method_mode {
            MethodMode::PerObject => {
                r.generic = r.generic.saturating_sub(objects.len() as u64);
            }
            MethodMode::Inflate => {
                r.generic = r.generic.saturating_add(5);
            }
        }
        Ok(r)
    }

    fn record_cast(&mut self, _player: PlayerId, _stack_object: ObjectId, history: SpellHistory) {
        self.calls.push("record_cast".to_owned());
        self.history = Some(history);
    }

    fn queue_cast_triggers(&mut self, _player: PlayerId, _stack_object: ObjectId) {
        self.calls.push("queue_cast_triggers".to_owned());
        let t = self.cast_triggers.clone();
        self.waiting_triggers.extend(t);
    }

    fn purpose(&self, _player: PlayerId, _stack_object: ObjectId) -> SpendPurpose {
        SpendPurpose::new(self.purpose_tags.iter().map(String::as_str))
    }
}

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

fn red(src: u64) -> ManaUnit {
    ManaUnit::new(ManaType::Red, obj(src))
}

fn green(src: u64) -> ManaUnit {
    ManaUnit::new(ManaType::Green, obj(src))
}

fn locked_with(cost: &str, choices: &[SymbolChoice], other: Vec<CostAST>) -> LockedCost {
    let mc = ManaCostDSL::parse(cost).expect("parses");
    let basis = CostBasis::from_mana_cost(Some(&mc), None, choices, other).expect("basis");
    determine_total_cost(&basis, &[], &CostStages::default(), &[]).expect("locked")
}

fn locked(cost: &str) -> LockedCost {
    locked_with(cost, &[], Vec::new())
}

fn locked_unpayable() -> LockedCost {
    let basis = CostBasis::from_mana_cost(None, None, &[], Vec::new()).expect("basis");
    determine_total_cost(&basis, &[], &CostStages::default(), &[]).expect("locked")
}

fn base_history() -> SpellHistory {
    SpellHistory {
        x_value: Some(3),
        cast_face: Some("test.face_a".to_owned()),
        ..SpellHistory::default()
    }
}

/// What one run of the procedure left behind.
struct Run {
    result: Result<PayOutcome, Interrupt>,
    pre: FakeState,
    pre_hash: Digest,
    /// Open transactions after the call.
    depth: usize,
}

type Proc = Box<dyn Fn(&mut FakeState, &mut ChoiceContext<'_>) -> Result<Run, Interrupt>>;

/// The procedure: a cast checkpoint, then `pay_total_cost`. `Interrupt::Pending` propagates;
/// `Interrupt::Error` is captured in the output so the test can inspect the state.
fn procedure(lc: LockedCost) -> Proc {
    Box::new(move |state, ctx| {
        let pre = state.clone();
        let pre_hash = pre.state_hash();
        let mut txn = Transactions::new(state.clone());
        let cp = txn.checkpoint();
        let result = pay_total_cost(&mut txn, cp, ctx, P1, SPELL, &lc, base_history());
        let result = match result {
            Err(Interrupt::Pending(p)) => return Err(Interrupt::Pending(p)),
            other => other,
        };
        let depth = txn.depth();
        *state = txn.state().clone();
        Ok(Run {
            result,
            pre,
            pre_hash,
            depth,
        })
    })
}

type Session = ChoiceSession<FakeState, Run, Proc>;

fn start(state: FakeState, lc: &LockedCost) -> Session {
    ChoiceSession::start(state, procedure(lc.clone())).expect("starts")
}

/// Starts and answers each pending request in turn.
fn drive(state: FakeState, lc: &LockedCost, answers: &[ChoiceResponse]) -> Session {
    let mut s = start(state, lc);
    for a in answers {
        let id = s.pending().expect("a pending request").request_id;
        s.submit(id, a.clone()).expect("valid answer");
    }
    s
}

fn done(s: &Session) -> (&FakeState, &Run) {
    match s.status() {
        Status::Done { state, output } => (state, output),
        Status::Pending(r) => panic!("still pending: {:?}", r.payload),
    }
}

fn pay_with(abilities: &[(u64, &str)]) -> ChoiceResponse {
    ChoiceResponse::PayCosts {
        auto_pay: false,
        methods: Vec::new(),
        mana_abilities: abilities
            .iter()
            .map(|(s, id)| (obj(*s), (*id).to_owned()))
            .collect(),
    }
}

fn pay_plain() -> ChoiceResponse {
    pay_with(&[])
}

struct Ask {
    locked_total: String,
    methods: Vec<(String, Vec<ObjectId>)>,
    mana_abilities: Vec<(ObjectId, String)>,
    auto_pay: bool,
    optional: bool,
    cancellable: bool,
}

fn ask_of(r: &PendingChoiceRequest) -> Ask {
    assert_eq!(r.choice_type, ChoiceType::PayManaAndCosts);
    assert_eq!(r.chooser, P1);
    match &r.payload {
        ChoicePayload::PayManaAndCosts {
            locked_total,
            methods,
            mana_abilities,
            auto_pay,
            optional,
            cancellable,
        } => Ask {
            locked_total: locked_total.clone(),
            methods: methods.clone(),
            mana_abilities: mana_abilities.clone(),
            auto_pay: *auto_pay,
            optional: *optional,
            cancellable: *cancellable,
        },
        other => panic!("unexpected payload {other:?}"),
    }
}

/// Asserts a rollback/cancel left exactly the pre-cast state (apart from carried fields when
/// `durable` is set), kept priority, queued no trigger and recorded no history.
fn assert_restored(state: &FakeState, run: &Run) {
    assert_eq!(run.depth, 0, "the cast checkpoint is closed");
    assert_eq!(state.prio, Some(P1), "CR 734.2: priority kept");
    assert_eq!(
        state.waiting_triggers, run.pre.waiting_triggers,
        "CR 734.1: no triggers"
    );
    assert!(state.history.is_none(), "no history recorded");
    assert_eq!(
        state.non_durable().state_hash(),
        run.pre.non_durable().state_hash()
    );
}

fn rolled_back(run: &Run) -> (&PayStopReason, Digest, Option<PlayerId>) {
    match &run.result {
        Ok(PayOutcome::RolledBack(rb)) => (&rb.reason, rb.state_hash, rb.priority_player),
        other => panic!("expected RolledBack, got {:?}", other.as_ref().err()),
    }
}

// ---------------------------------------------------------------------------------------------
// D1. Committed payment
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: {1}{R} is paid from the pool: history recorded (CR 601.2i), `record_cast` then
/// `queue_cast_triggers` once each, the spent units reported, the excess left in the pool
/// (CR 118.3a), the state changed (CR 601.2h, 601.2i, 603.3).
#[test]
fn req_cast_001_committed_payment() {
    // CR 601.2h, CR 601.2i, CR 603.3, CR 118.3a
    let st = FakeState::new().with_units(&[red(1), green(2), green(2)]);
    let lc = locked("{1}{R}");
    let s = drive(st, &lc, &[pay_plain()]);
    let (state, run) = done(&s);
    let Ok(PayOutcome::Committed { spent, .. }) = &run.result else {
        panic!("expected Committed");
    };
    assert_eq!(spent, &vec![red(1), green(2)]);
    assert_eq!(state.pool().units(), &[green(2)], "excess mana stays");
    assert_eq!(run.depth, 0, "the cast checkpoint is committed");
    let rc: Vec<&String> = state
        .calls
        .iter()
        .filter(|c| *c == "record_cast" || *c == "queue_cast_triggers")
        .collect();
    assert_eq!(rc, vec!["record_cast", "queue_cast_triggers"]);
    assert_eq!(
        state.waiting_triggers,
        vec!["test.cast_trigger_a".to_owned()]
    );
    let h = state.history.as_ref().expect("history recorded");
    assert_eq!(h.x_value, Some(3), "built from base_history");
    assert_eq!(h.cast_face.as_deref(), Some("test.face_a"));
    assert!(!h.costs_paid.is_empty(), "the mana payment is a paid cost");
    assert_eq!(h.mana_spent.values().sum::<i64>(), 2);
    assert_eq!(h.mana_spent.len(), 2, "red and green are separate colors");
    assert_ne!(state.state_hash(), run.pre_hash);
}

/// REQ-CAST-001: mana spent is summed per (color, source type): two red units from one source
/// give one entry of 2 (CR 601.2i).
#[test]
fn req_cast_001_mana_spent_is_summed() {
    // CR 601.2i
    let st = FakeState::new().with_units(&[red(1), red(1)]);
    let s = drive(st, &locked("{R}{R}"), &[pay_plain()]);
    let (state, run) = done(&s);
    assert!(matches!(run.result, Ok(PayOutcome::Committed { .. })));
    let h = state.history.as_ref().expect("history");
    assert_eq!(h.mana_spent.len(), 1);
    assert_eq!(h.mana_spent.values().copied().collect::<Vec<_>>(), vec![2]);
    assert!(state.pool().is_empty());
}

// ---------------------------------------------------------------------------------------------
// D2. The mana-ability window
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: with an empty pool the request lists the available mana abilities and offers
/// no auto-pay; activating one adds R and pays; it is activated exactly once; auto-pay is offered
/// when the pool already suffices (CR 601.2g, 605.3a, 605.3b, 118.3c).
#[test]
fn req_cast_001_mana_ability_window() {
    // CR 601.2g, CR 605.3a, CR 605.3b, CR 118.3c
    let st = FakeState::new()
        .with_ability(
            5,
            "test.ability_b",
            Script {
                adds: vec![green(5)],
                ..Script::default()
            },
        )
        .with_ability(
            3,
            "test.ability_a",
            Script {
                adds: vec![red(3)],
                ..Script::default()
            },
        );
    let lc = locked("{R}");
    let s = start(st.clone(), &lc);
    let ask = ask_of(s.pending().expect("asks"));
    assert_eq!(
        ask.mana_abilities,
        vec![
            (obj(3), "test.ability_a".to_owned()),
            (obj(5), "test.ability_b".to_owned())
        ]
    );
    assert_eq!(ask.mana_abilities, st.available_mana_abilities(P1));
    assert!(!ask.auto_pay, "the empty pool does not suffice");
    assert!(!ask.optional);
    assert!(ask.cancellable);
    assert!(ask.methods.is_empty(), "no offers: no methods");
    assert!(!ask.locked_total.is_empty());

    let s = drive(st.clone(), &lc, &[pay_with(&[(3, "test.ability_a")])]);
    let (state, run) = done(&s);
    let Ok(PayOutcome::Committed { spent, .. }) = &run.result else {
        panic!("expected Committed");
    };
    assert_eq!(spent, &vec![red(3)]);
    assert_eq!(state.activation_count(3, "test.ability_a"), 1);
    assert_eq!(state.activation_count(5, "test.ability_b"), 0);
    assert!(state.pool().is_empty());

    // Auto-pay is offered when the pool already suffices, and pays from it.
    let st2 = st.clone().with_units(&[red(9)]);
    let s2 = start(st2.clone(), &lc);
    assert!(ask_of(s2.pending().expect("asks")).auto_pay);
    let auto = ChoiceResponse::PayCosts {
        auto_pay: true,
        methods: Vec::new(),
        mana_abilities: Vec::new(),
    };
    let s2 = drive(st2, &lc, &[auto]);
    let (state2, run2) = done(&s2);
    assert!(matches!(run2.result, Ok(PayOutcome::Committed { .. })));
    assert!(state2.pool().is_empty());
    assert_eq!(state2.activation_count(3, "test.ability_a"), 0);

    // Different locked totals display differently.
    let other = start(st, &locked("{R}{R}"));
    assert_ne!(
        ask_of(other.pending().expect("asks")).locked_total,
        ask.locked_total
    );
}

/// REQ-CAST-001: `{0}` asks nothing and commits (CR 118.5, 601.2g).
#[test]
fn req_cast_001_zero_mana_no_ask() {
    // CR 118.5, CR 601.2g
    let st = FakeState::new().with_units(&[red(1)]);
    let s = start(st, &locked("{0}"));
    let (state, run) = done(&s);
    let Ok(PayOutcome::Committed { spent, .. }) = &run.result else {
        panic!("expected Committed");
    };
    assert!(spent.is_empty());
    assert_eq!(state.pool().units(), &[red(1)]);
    assert!(state.history.is_some());
}

// ---------------------------------------------------------------------------------------------
// D3. ACCEPTANCE: an unpayable cost rolls back
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, REQ-CORE-005 (acceptance): {R}{R} with one R rolls back: UnpayableMana, the
/// pre-cast hash, priority kept, no triggers, no history, and a non-library mana ability
/// activated in the window is reversed by default (CR 601.2h, 601.2e, 734.1, 734.2, 118.3).
#[test]
fn req_cast_001_unpayable_cost_rolls_back() {
    // CR 601.2h, CR 601.2e, CR 734.1, CR 734.2, CR 118.3
    let st = FakeState::new().with_units(&[red(1)]).with_ability(
        4,
        "test.ability_a",
        Script {
            adds: vec![green(4)],
            ..Script::default()
        },
    );
    let s = drive(
        st.clone(),
        &locked("{R}{R}"),
        &[pay_with(&[(4, "test.ability_a")])],
    );
    let (state, run) = done(&s);
    let (reason, hash, prio) = rolled_back(run);
    assert!(matches!(reason, PayStopReason::UnpayableMana), "{reason:?}");
    assert_eq!(hash, run.pre_hash);
    assert_eq!(
        state.state_hash(),
        run.pre_hash,
        "state equals the pre-cast state"
    );
    assert_eq!(state.state_hash(), st.state_hash());
    assert_eq!(prio, Some(P1));
    assert_restored(state, run);
    assert_eq!(state.activation_count(4, "test.ability_a"), 0, "reversed");
    assert_eq!(state.pool().units(), &[red(1)]);
    assert_eq!(state.library_mills, 0);
    assert!(state.marks.is_empty());

    // Without any activation, too.
    let s = drive(st.clone(), &locked("{R}{R}"), &[pay_plain()]);
    let (state, run) = done(&s);
    let (reason, hash, _) = rolled_back(run);
    assert!(matches!(reason, PayStopReason::UnpayableMana));
    assert_eq!(hash, st.state_hash());
    assert_eq!(state.state_hash(), st.state_hash());
}

// ---------------------------------------------------------------------------------------------
// D4. ACCEPTANCE: library-touching survives the rollback
// ---------------------------------------------------------------------------------------------

fn library_state(library_touching: bool) -> FakeState {
    FakeState::new()
        .with_units(&[red(1)])
        .with_ability(
            6,
            "test.ability_lib",
            Script {
                adds: vec![green(6)],
                library_touching,
                mills: 2,
                ..Script::default()
            },
        )
        .with_ability(
            7,
            "test.ability_plain",
            Script {
                adds: vec![green(7)],
                ..Script::default()
            },
        )
}

/// REQ-CAST-001, REQ-CORE-005 (acceptance): a library-touching mana-ability effect survives the
/// rollback of an unpayable cast (carried as a durable sub-transaction), the non-library mana
/// ability's effect and mana are gone, and the rest equals the checkpoint; with
/// `library_touching: false` the same effect is rolled back (CR 734.1, 605.1b, 601.2h).
#[test]
fn req_cast_001_library_touching_survives_rollback() {
    // CR 734.1 "Players may not reverse actions that moved cards to a library ...", CR 605.1b,
    // CR 601.2h
    let answer = pay_with(&[(7, "test.ability_plain"), (6, "test.ability_lib")]);
    let lc = locked("{R}{R}");

    let st = library_state(true);
    let s = drive(st.clone(), &lc, std::slice::from_ref(&answer));
    let (state, run) = done(&s);
    let (reason, _, prio) = rolled_back(run);
    assert!(matches!(reason, PayStopReason::UnpayableMana));
    assert_eq!(prio, Some(P1));
    assert_eq!(state.library_mills, 2, "the library effect is carried");
    assert_eq!(state.marks, vec!["activate:test.ability_lib".to_owned()]);
    assert_eq!(
        state.pool().units(),
        &[red(1)],
        "the non-library mana is gone"
    );
    assert_eq!(state.activation_count(7, "test.ability_plain"), 0);
    assert_restored(state, run);

    // Contrast: not library-touching, so reversed with the action.
    let st = library_state(false);
    let s = drive(st.clone(), &lc, &[answer]);
    let (state, run) = done(&s);
    let (_, hash, _) = rolled_back(run);
    assert_eq!(state.library_mills, 0);
    assert!(state.marks.is_empty());
    assert_eq!(state.state_hash(), st.state_hash());
    assert_eq!(hash, st.state_hash());
}

/// REQ-CAST-001, REQ-CORE-005 (acceptance): a library-touching late cost item that was paid
/// survives the rollback caused by a later unpayable item; a non-library item does not
/// (CR 601.2h second pass, 734.1).
#[test]
fn req_cast_001_library_touching_cost_item_survives_rollback() {
    // CR 601.2h "Then they pay all remaining costs in any order.", CR 734.1
    for (mode, survives) in [(ItemMode::OkLibrary, true), (ItemMode::Ok, false)] {
        let st = FakeState::new()
            .with_units(&[red(1)])
            .with_item(CostAST::Forage, true, mode, "test.item_lib")
            .with_item(
                CostAST::UntapSource,
                true,
                ItemMode::Unpayable,
                "test.item_bad",
            );
        let lc = locked_with("{R}", &[], vec![CostAST::Forage, CostAST::UntapSource]);
        let s = drive(st.clone(), &lc, &[pay_plain()]);
        let (state, run) = done(&s);
        let (reason, _, prio) = rolled_back(run);
        assert!(matches!(reason, PayStopReason::UnpayableMana), "{reason:?}");
        assert_eq!(prio, Some(P1));
        assert_restored(state, run);
        assert_eq!(
            state.pool().units(),
            &[red(1)],
            "the mana payment is undone"
        );
        if survives {
            assert_eq!(state.library_mills, 1);
            assert_eq!(state.marks, vec!["item:test.item_lib".to_owned()]);
        } else {
            assert_eq!(state.library_mills, 0);
            assert!(state.marks.is_empty());
            assert_eq!(state.state_hash(), st.state_hash());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// D5. Dependent activation (CR 734.1)
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, REQ-CORE-005: ability X (non-library) makes R that pays the cost of ability Y
/// (library-touching); after the unpayable-cost rollback Y's effect is retained and no mana
/// appears from nothing (CR 734.1; see docs/DECISIONS.md on the scope of this assertion).
#[test]
fn req_cast_001_dependent_activation_keeps_library_effect() {
    // CR 734.1 "... unless mana from those abilities ... was spent on another mana ability that
    // wasn't reversed."
    let st = FakeState::new()
        .with_ability(
            8,
            "test.ability_x",
            Script {
                adds: vec![red(8)],
                ..Script::default()
            },
        )
        .with_ability(
            9,
            "test.ability_y",
            Script {
                adds: vec![green(9)],
                costs: Some(ManaType::Red),
                library_touching: true,
                mills: 1,
            },
        );
    let s = drive(
        st.clone(),
        &locked("{R}{R}"),
        &[pay_with(&[(8, "test.ability_x"), (9, "test.ability_y")])],
    );
    let (state, run) = done(&s);
    let (reason, _, prio) = rolled_back(run);
    assert!(matches!(reason, PayStopReason::UnpayableMana));
    assert_eq!(prio, Some(P1));
    assert_eq!(state.library_mills, 1, "Y's library effect is retained");
    assert!(state.marks.contains(&"activate:test.ability_y".to_owned()));
    assert_eq!(state.pool(), st.pool(), "no mana appears from nothing");
    assert!(state.waiting_triggers.is_empty());
    assert!(state.history.is_none());
}

// ---------------------------------------------------------------------------------------------
// D6. Restricted mana and riders
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001 (acceptance): restricted mana can only be spent where allowed: with purpose
/// {"spell"} a unit restricted to {"ability"} is unused (and the payment rolls back if it is
/// needed); with purpose {"ability"} it is spent; riders of spent units are reported
/// (CR 106.6, 601.2h).
#[test]
fn req_cast_001_restricted_mana_only_where_allowed() {
    // CR 106.6, CR 601.2h
    let restricted = red(1)
        .restricted(ManaRestriction::only_for(["ability"]))
        .with_rider(ManaRider {
            id: "test.rider_restricted".to_owned(),
        });
    let plain = red(2).with_rider(ManaRider {
        id: "test.rider_plain".to_owned(),
    });
    let st = FakeState::new().with_units(&[restricted.clone(), plain.clone()]);

    // {R} under "spell": the plain unit is spent, the restricted one stays.
    let s = drive(st.clone(), &locked("{R}"), &[pay_plain()]);
    let (state, run) = done(&s);
    let Ok(PayOutcome::Committed {
        spent,
        riders_fired,
    }) = &run.result
    else {
        panic!("expected Committed");
    };
    assert_eq!(spent, &vec![plain.clone()]);
    assert_eq!(state.pool().units(), std::slice::from_ref(&restricted));
    let fired = format!("{riders_fired:?}");
    assert!(fired.contains("test.rider_plain"), "{fired}");
    assert!(!fired.contains("test.rider_restricted"), "{fired}");

    // {R}{R} under "spell": the restricted unit is needed, so the payment rolls back.
    let s = drive(st.clone(), &locked("{R}{R}"), &[pay_plain()]);
    let (state, run) = done(&s);
    let (reason, hash, _) = rolled_back(run);
    assert!(matches!(reason, PayStopReason::UnpayableMana));
    assert_eq!(hash, st.state_hash());
    assert_eq!(state.state_hash(), st.state_hash());

    // {R}{R} under "ability": both are spent, both riders reported.
    let mut st2 = st.clone();
    st2.purpose_tags = vec!["ability".to_owned()];
    let s = drive(st2, &locked("{R}{R}"), &[pay_plain()]);
    let (state, run) = done(&s);
    let Ok(PayOutcome::Committed {
        spent,
        riders_fired,
    }) = &run.result
    else {
        panic!("expected Committed");
    };
    assert_eq!(spent.len(), 2);
    assert!(state.pool().is_empty());
    let fired = format!("{riders_fired:?}");
    assert!(fired.contains("test.rider_plain") && fired.contains("test.rider_restricted"));
}

// ---------------------------------------------------------------------------------------------
// D7. Payment order and no partial payment
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: late costs are paid only after every non-late cost (mana, life, other items);
/// if a late item is unpayable after the others were payable, nothing stays paid (CR 601.2h).
#[test]
fn req_cast_001_payment_order_and_no_partial_payment() {
    // CR 601.2h "First, they pay all costs that don't involve random elements or moving objects
    // from the library to a public zone, in any order. Then they pay all remaining costs in any
    // order. Partial payments are not allowed."
    let build = |late_mode: ItemMode| {
        FakeState::new()
            .with_units(&[red(1)])
            // Listed first, but late.
            .with_item(CostAST::Forage, true, late_mode, "test.item_late")
            .with_item(CostAST::TapSource, false, ItemMode::Ok, "test.item_early_a")
            .with_item(
                CostAST::UntapSource,
                false,
                ItemMode::Ok,
                "test.item_early_b",
            )
    };
    let lc = locked_with(
        "{R}{R/P}",
        &[SymbolChoice::Life],
        vec![CostAST::Forage, CostAST::TapSource, CostAST::UntapSource],
    );
    assert_eq!(lc.phyrexian_life(), 2);

    let s = drive(build(ItemMode::Ok), &lc, &[pay_plain()]);
    let (state, run) = done(&s);
    assert!(matches!(run.result, Ok(PayOutcome::Committed { .. })));
    let pos = |name: &str| {
        state
            .calls
            .iter()
            .position(|c| c == name)
            .unwrap_or_else(|| panic!("{name} not called: {:?}", state.calls))
    };
    let late = pos("item:test.item_late");
    assert!(pos("item:test.item_early_a") < late, "{:?}", state.calls);
    assert!(pos("item:test.item_early_b") < late, "{:?}", state.calls);
    assert!(pos("life:2") < late, "{:?}", state.calls);
    let last_pool = state
        .calls
        .iter()
        .rposition(|c| c == "pool_mut")
        .expect("mana spent from the pool");
    assert!(
        last_pool < late,
        "mana is a non-late cost: {:?}",
        state.calls
    );
    assert!(pos("record_cast") > late);
    assert_eq!(state.life[&P1], 18);
    assert!(state.pool().is_empty());

    // The late item is unpayable: nothing is spent, life unchanged.
    let st = build(ItemMode::Unpayable);
    let s = drive(st.clone(), &lc, &[pay_plain()]);
    let (state, run) = done(&s);
    let (reason, hash, _) = rolled_back(run);
    assert!(matches!(reason, PayStopReason::UnpayableMana), "{reason:?}");
    assert_eq!(hash, st.state_hash());
    assert_eq!(state.state_hash(), st.state_hash());
    assert_eq!(state.life[&P1], 20);
    assert_eq!(state.pool().units(), &[red(1)]);
    assert_restored(state, run);
}

// ---------------------------------------------------------------------------------------------
// D8. Life
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: Phyrexian life is paid with `pay_life`; a player who cannot pay rolls back with
/// nothing paid (CR 118.3, 118.3b, 601.2h).
#[test]
fn req_cast_001_life_payment() {
    // CR 118.3, CR 118.3b, CR 601.2h
    // Mana {0} plus 2 life: nothing to ask; life is paid.
    let lc = locked_with("{R/P}", &[SymbolChoice::Life], Vec::new());
    let s = start(FakeState::new(), &lc);
    let (state, run) = done(&s);
    assert!(matches!(run.result, Ok(PayOutcome::Committed { .. })));
    assert_eq!(state.life[&P1], 18);
    assert!(state.calls.contains(&"life:2".to_owned()));

    // Not enough life: rolled back, nothing paid.
    let mut poor = FakeState::new().with_units(&[red(1)]);
    poor.life.insert(P1, 1);
    let lc = locked_with("{R}{R/P}", &[SymbolChoice::Life], Vec::new());
    let s = drive(poor.clone(), &lc, &[pay_plain()]);
    let (state, run) = done(&s);
    let (reason, hash, prio) = rolled_back(run);
    assert!(matches!(reason, PayStopReason::UnpayableMana), "{reason:?}");
    assert_eq!(hash, poor.state_hash());
    assert_eq!(prio, Some(P1));
    assert_eq!(state.life[&P1], 1);
    assert_eq!(state.pool().units(), &[red(1)]);
    assert_restored(state, run);
}

// ---------------------------------------------------------------------------------------------
// D9. Unpayable mana part
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: a locked cost with no mana cost is unpayable: no ask, RolledBack(UnpayableCost),
/// state identical (CR 118.6, 601.2h, 734.1).
#[test]
fn req_cast_001_unpayable_mana_part() {
    // CR 118.6 "attempting to pay an unpayable cost is an illegal action.", CR 601.2h, CR 734.1
    let st =
        FakeState::new()
            .with_units(&[red(1)])
            .with_ability(3, "test.ability_a", Script::default());
    let s = start(st.clone(), &locked_unpayable());
    assert!(s.pending().is_none(), "no ask");
    let (state, run) = done(&s);
    let (reason, hash, prio) = rolled_back(run);
    assert!(matches!(reason, PayStopReason::UnpayableCost), "{reason:?}");
    assert_eq!(hash, st.state_hash());
    assert_eq!(prio, Some(P1));
    assert_eq!(state.state_hash(), st.state_hash());
    assert_restored(state, run);
}

// ---------------------------------------------------------------------------------------------
// D10. Cancel
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, REQ-CORE-005: `CancelAndRollback` cancels: state equals the checkpoint,
/// priority kept, no triggers (CR 734.1, 734.2).
#[test]
fn req_cast_001_cancel_rolls_back() {
    // CR 734.1, CR 734.2
    let st = FakeState::new().with_units(&[red(1)]);
    let s = drive(
        st.clone(),
        &locked("{R}"),
        &[ChoiceResponse::CancelAndRollback],
    );
    let (state, run) = done(&s);
    let Ok(PayOutcome::Cancelled(rb)) = &run.result else {
        panic!("expected Cancelled");
    };
    assert_eq!(rb.state_hash, st.state_hash());
    assert_eq!(rb.priority_player, Some(P1));
    assert_eq!(state.state_hash(), st.state_hash());
    assert_restored(state, run);
}

/// REQ-CAST-001: the payment is not optional, so `DeclineOptionalPayment` is refused (CR 601.2h).
#[test]
fn req_cast_001_decline_is_refused() {
    // CR 601.2h
    let st = FakeState::new().with_units(&[red(1)]);
    let mut s = start(st, &locked("{R}"));
    let id = s.pending().expect("asks").request_id;
    assert!(
        s.submit(id, ChoiceResponse::DeclineOptionalPayment)
            .is_err()
    );
    assert!(s.pending().is_some(), "still waiting for a valid answer");
}

// ---------------------------------------------------------------------------------------------
// D11. Re-activation guard
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: a response listing the same mana ability twice is refused without a double
/// activation; no state is left half-applied (CR 605.3c).
#[test]
fn req_cast_001_duplicate_activation_refused() {
    // CR 605.3c "Once a player begins to activate a mana ability, that ability can't be
    // activated again until it has resolved."
    let st = FakeState::new().with_ability(
        3,
        "test.ability_a",
        Script {
            adds: vec![red(3)],
            ..Script::default()
        },
    );
    let lc = locked("{R}{R}");
    let mut s = start(st.clone(), &lc);
    let id = s.pending().expect("asks").request_id;
    let dup = pay_with(&[(3, "test.ability_a"), (3, "test.ability_a")]);
    match s.submit(id, dup) {
        // Refused before the procedure ran on it.
        Err(_) => assert!(s.pending().is_some()),
        Ok(()) => {
            let (state, run) = done(&s);
            assert!(
                !matches!(run.result, Ok(PayOutcome::Committed { .. })),
                "a double activation paid the cost"
            );
            assert!(state.activation_count(3, "test.ability_a") <= 1);
            match &run.result {
                Ok(_) => {
                    assert_eq!(state.state_hash(), st.state_hash());
                    assert_restored(state, run);
                }
                Err(Interrupt::Error(_)) => {
                    // Nothing half-applied: the cast checkpoint is closed and the state is the
                    // pre-cast one, or it is still open for the caller to roll back.
                    assert!(run.depth <= 1);
                    if run.depth == 0 {
                        assert_eq!(state.state_hash(), st.state_hash());
                    }
                    assert!(state.waiting_triggers.is_empty());
                    assert!(state.history.is_none());
                }
                Err(Interrupt::Pending(_)) => unreachable!("propagated"),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// D12. Payment-method hook
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: a payment method applies in step 8, one generic less per object used; the locked
/// cost is unchanged; the offers appear as the request's methods (CR 601.2h, 702.51b, 702.126b,
/// 601.2f).
#[test]
fn req_cast_001_payment_method_hook() {
    // CR 601.2h, CR 702.51b, CR 702.126b, CR 601.2f "If effects would change the total cost
    // after this time, they have no effect."
    let mut st = FakeState::new().with_units(&[red(1), green(2)]);
    st.offers = vec![("test.method_a".to_owned(), vec![obj(11), obj(12)])];
    let lc = locked("{1}{R}");
    let before = lc.clone();
    let s = start(st.clone(), &lc);
    let ask = ask_of(s.pending().expect("asks"));
    assert_eq!(
        ask.methods,
        vec![("test.method_a".to_owned(), vec![obj(11), obj(12)])]
    );
    let answer = ChoiceResponse::PayCosts {
        auto_pay: false,
        methods: vec![(0, vec![obj(11)])],
        mana_abilities: Vec::new(),
    };
    let s = drive(st.clone(), &lc, &[answer]);
    let (state, run) = done(&s);
    let Ok(PayOutcome::Committed { spent, .. }) = &run.result else {
        panic!("expected Committed");
    };
    assert_eq!(spent, &vec![red(1)], "the method paid the generic part");
    assert_eq!(state.pool().units(), &[green(2)]);
    assert_eq!(state.used_for_methods, vec![obj(11)]);
    assert_eq!(lc, before, "the locked cost is unchanged");

    // More objects than generic mana: clamped at zero (never underflows).
    let answer = ChoiceResponse::PayCosts {
        auto_pay: false,
        methods: vec![(0, vec![obj(11), obj(12)])],
        mana_abilities: Vec::new(),
    };
    let s = drive(st.clone(), &locked("{R}"), &[answer]);
    let (state, run) = done(&s);
    let Ok(PayOutcome::Committed { spent, .. }) = &run.result else {
        panic!("expected Committed");
    };
    assert_eq!(spent, &vec![red(1)]);
    assert_eq!(state.pool().units(), &[green(2)]);
}

/// REQ-CAST-001: a method that tries to make the remaining mana larger than before is clamped or
/// refused: never more mana is spent than the locked total (CR 601.2f, 601.2h).
#[test]
fn req_cast_001_payment_method_cannot_inflate() {
    // CR 601.2f, CR 601.2h
    let mut st = FakeState::new().with_units(&[red(1), green(2), green(3), green(4), green(5)]);
    st.offers = vec![("test.method_a".to_owned(), vec![obj(11)])];
    st.method_mode = MethodMode::Inflate;
    let answer = ChoiceResponse::PayCosts {
        auto_pay: false,
        methods: vec![(0, vec![obj(11)])],
        mana_abilities: Vec::new(),
    };
    let s = drive(st.clone(), &locked("{1}{R}"), &[answer]);
    let (state, run) = done(&s);
    match &run.result {
        Ok(PayOutcome::Committed { spent, .. }) => {
            assert!(spent.len() <= 2, "spent {spent:?}");
            assert!(state.pool().len() >= 3);
        }
        Ok(PayOutcome::RolledBack(rb) | PayOutcome::Cancelled(rb)) => {
            assert_eq!(rb.state_hash, st.state_hash());
            assert_eq!(state.state_hash(), st.state_hash());
        }
        Err(_) => assert!(state.waiting_triggers.is_empty()),
    }
}

// ---------------------------------------------------------------------------------------------
// D13. Precondition
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, REQ-CORE-005: a cast checkpoint that is not the innermost open one is an error
/// and changes nothing (CR 734.1).
#[test]
fn req_cast_001_checkpoint_must_be_innermost() {
    // CR 734.1
    let st = FakeState::new().with_units(&[red(1)]);
    let lc = locked("{R}");
    let proc_ = move |state: &mut FakeState,
                      ctx: &mut ChoiceContext<'_>|
          -> Result<(bool, Digest, Digest, usize), Interrupt> {
        let mut txn = Transactions::new(state.clone());
        let outer = txn.checkpoint();
        let _inner = txn.checkpoint();
        let before = txn.state().state_hash();
        let r = pay_total_cost(&mut txn, outer, ctx, P1, SPELL, &lc, base_history());
        let is_error = matches!(r, Err(Interrupt::Error(_)));
        Ok((is_error, before, txn.state().state_hash(), txn.depth()))
    };
    let s = ChoiceSession::start(st, proc_).expect("starts");
    let Status::Done { output, .. } = s.status() else {
        panic!("must not ask");
    };
    let (is_error, before, after, depth) = *output;
    assert!(is_error, "Err(Interrupt::Error(_)) expected");
    assert_eq!(before, after, "state unchanged");
    assert_eq!(depth, 2, "no transaction closed");
}

// ---------------------------------------------------------------------------------------------
// D14. Determinism and replay
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: the same recorded answers give the same outcome and state hash in 10 runs, and
/// suspend-then-resume equals a single replay (R3; CR 601.2g, 601.2h).
#[test]
fn req_cast_001_replay_is_deterministic() {
    // CR 601.2g, CR 601.2h
    let st = FakeState::new()
        .with_units(&[green(2), red(1)])
        .with_ability(
            3,
            "test.ability_a",
            Script {
                adds: vec![red(3).with_rider(ManaRider {
                    id: "test.rider_a".to_owned(),
                })],
                ..Script::default()
            },
        );
    let lc = locked("{1}{R}{R}");
    let answers = vec![pay_with(&[(3, "test.ability_a")])];
    let mut hashes = BTreeSet::new();
    let mut outcomes = BTreeSet::new();
    for _ in 0..10 {
        let s = ChoiceSession::replay(st.clone(), procedure(lc.clone()), &answers).expect("replay");
        let (state, run) = done(&s);
        hashes.insert(state.state_hash());
        outcomes.insert(format!("{:?}", run.result.as_ref().ok()));
        assert!(matches!(run.result, Ok(PayOutcome::Committed { .. })));
    }
    assert_eq!(hashes.len(), 1);
    assert_eq!(outcomes.len(), 1);

    // Suspend at the ask, then resume.
    let mut s = start(st.clone(), &lc);
    let pending = s.pending().expect("suspended").clone();
    assert_eq!(pending.choice_type, ChoiceType::PayManaAndCosts);
    s.submit(pending.request_id, answers[0].clone())
        .expect("resume");
    let (state, run) = done(&s);
    assert!(hashes.contains(&state.state_hash()));
    assert!(outcomes.contains(&format!("{:?}", run.result.as_ref().ok())));
    assert_eq!(s.answers(), answers.as_slice());
}

// ---------------------------------------------------------------------------------------------
// D15. Triggers
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001: no trigger is queued on a rolled-back or cancelled outcome (CR 734.1 "No
/// abilities trigger"); exactly the host's cast triggers on commit (CR 601.2i, 603.3).
#[test]
fn req_cast_001_triggers_only_on_commit() {
    // CR 734.1, CR 601.2i, CR 603.3
    let mut st = FakeState::new().with_units(&[red(1)]);
    st.cast_triggers = vec![
        "test.cast_trigger_a".to_owned(),
        "test.cast_trigger_b".to_owned(),
    ];
    let cases: Vec<(LockedCost, Vec<ChoiceResponse>, bool)> = vec![
        (locked("{R}"), vec![pay_plain()], true),
        (locked("{R}{R}"), vec![pay_plain()], false),
        (
            locked("{R}"),
            vec![ChoiceResponse::CancelAndRollback],
            false,
        ),
        (locked_unpayable(), Vec::new(), false),
    ];
    for (lc, answers, commits) in cases {
        let s = drive(st.clone(), &lc, &answers);
        let (state, run) = done(&s);
        if commits {
            assert!(matches!(run.result, Ok(PayOutcome::Committed { .. })));
            assert_eq!(
                state.waiting_triggers,
                vec![
                    "test.cast_trigger_a".to_owned(),
                    "test.cast_trigger_b".to_owned()
                ]
            );
            assert_eq!(
                state
                    .calls
                    .iter()
                    .filter(|c| *c == "queue_cast_triggers")
                    .count(),
                1
            );
        } else {
            assert!(!matches!(run.result, Ok(PayOutcome::Committed { .. })));
            assert!(state.waiting_triggers.is_empty());
            assert!(!state.calls.iter().any(|c| c == "queue_cast_triggers"));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// R12: untrusted ability ids
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001 (R12): empty and very long mana-ability ids are passed through without a panic
/// (CR 605.3a).
#[test]
fn req_cast_001_odd_ability_ids() {
    // CR 605.3a
    let long = "a".repeat(50_000);
    let st = FakeState::new()
        .with_ability(
            3,
            "",
            Script {
                adds: vec![red(3)],
                ..Script::default()
            },
        )
        .with_ability(
            4,
            &long,
            Script {
                adds: vec![red(4)],
                ..Script::default()
            },
        );
    let lc = locked("{R}{R}");
    let s = start(st.clone(), &lc);
    let ask = ask_of(s.pending().expect("asks"));
    assert_eq!(ask.mana_abilities.len(), 2);
    let s = drive(st, &lc, &[pay_with(&[(3, ""), (4, long.as_str())])]);
    let (state, run) = done(&s);
    assert!(matches!(run.result, Ok(PayOutcome::Committed { .. })));
    assert_eq!(state.activation_count(3, ""), 1);
    assert_eq!(state.activation_count(4, &long), 1);
}
