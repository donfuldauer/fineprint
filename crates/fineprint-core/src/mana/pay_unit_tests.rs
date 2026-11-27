// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the payment procedure `casting::pay_total_cost` (steps 7-9 of casting; compiled
// as the `tests` module of `casting/pay.rs`): REQ-CAST-001 (mana-ability window CR 601.2g,
// 605.3a-c, 605.4a; payment order and no partial payment CR 601.2h, 118.3; payment-method hook;
// commit CR 601.2i, 603.3), REQ-CORE-005 (rollback to the identical hash, priority kept,
// durable library-touching sub-transactions, CR 734.1, 734.2), REQ-NET-003 / REQ-CORE-001
// (replay determinism). A synthetic in-test host with neutral ids only (R5).

use std::collections::BTreeSet;

use fineprint_base::encode::CanonicalEncode;
use fineprint_base::hash::{Digest, leaf_digest};
use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_dsl::ability::CostAST;

use super::*;
use crate::casting::{CostBasis, determine_total_cost};
use crate::choice::{ChoiceContext, ChoiceError, ChoiceResponse, ChoiceSession, Status};
use crate::layers::CostStages;
use crate::mana::{ManaRestriction, ManaType};
use crate::txn::Transactional;

// ---------------------------------------------------------------------------------------------
// The synthetic host
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

const STACK: u64 = 100;

/// A synthetic mana ability.
#[derive(Clone, Debug, Default)]
struct FakeAbility {
    source: u64,
    id: &'static str,
    /// Cost: tap the source.
    taps: bool,
    /// Cost: mana paid from the pool (spend purpose `"ability"`).
    cost: ManaComponent,
    /// The mana it adds.
    produces: Vec<ManaUnit>,
    /// A triggered mana ability it causes mills this many cards (library-touching, CR 605.1b).
    mills: usize,
    /// The host refuses the activation.
    refuses: bool,
}

/// How the host pays a non-mana item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemRule {
    /// First pass, paid.
    Plain,
    /// Second pass (CR 601.2h), paid.
    Late,
    /// Second pass, mills a card (library-touching, CR 734.1).
    LateLibrary,
    /// First pass, can't be paid.
    Unpayable,
    /// Second pass, can't be paid.
    LateUnpayable,
}

/// How the payment method behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MethodMode {
    /// Each object pays this much generic (saturating at the remaining generic).
    Reduce(u64),
    /// Returns more mana than remaining (the procedure must clamp it).
    Overshoot,
    /// Refuses.
    Fail,
}

#[derive(Clone, Debug)]
struct Table {
    pools: [ManaPool; 2],
    abilities: Vec<FakeAbility>,
    tapped: BTreeSet<u64>,
    library: Vec<u64>,
    graveyard: Vec<u64>,
    life: u64,
    prio: Option<PlayerId>,
    stack: Vec<u64>,
    items: Vec<(CostAST, ItemRule)>,
    methods: Vec<PaymentMethodOffer>,
    method_mode: MethodMode,
    purpose_tags: Vec<&'static str>,
    log: Vec<String>,
    history: Option<SpellHistory>,
    cast: bool,
    queued: Vec<u64>,
}

fn slot(player: PlayerId) -> usize {
    usize::from(player.get() != 1)
}

impl Table {
    fn new() -> Table {
        Table {
            pools: [ManaPool::new(), ManaPool::new()],
            abilities: Vec::new(),
            tapped: BTreeSet::new(),
            library: vec![1, 2, 3, 4],
            graveyard: Vec::new(),
            life: 20,
            prio: Some(p(1)),
            stack: Vec::new(),
            items: Vec::new(),
            methods: Vec::new(),
            method_mode: MethodMode::Reduce(1),
            purpose_tags: vec!["spell"],
            log: Vec::new(),
            history: None,
            cast: false,
            queued: Vec::new(),
        }
    }

    fn with(mut self, a: FakeAbility) -> Table {
        self.abilities.push(a);
        self
    }

    fn pool(&self) -> &ManaPool {
        &self.pools[0]
    }

    fn mill(&mut self, n: usize) {
        for _ in 0..n {
            if let Some(c) = self.library.pop() {
                self.graveyard.push(c);
            }
        }
    }
}

fn red(source: u64) -> ManaUnit {
    ManaUnit::new(ManaType::Red, o(source))
}

fn green(source: u64) -> ManaUnit {
    ManaUnit::new(ManaType::Green, o(source))
}

fn black(source: u64) -> ManaUnit {
    ManaUnit::new(ManaType::Black, o(source))
}

/// Taps source 10 for one red.
fn tap_red() -> FakeAbility {
    FakeAbility {
        source: 10,
        id: "tap_red",
        taps: true,
        produces: vec![red(10)],
        ..FakeAbility::default()
    }
}

/// Taps source 11 for one green carrying a rider.
fn tap_green() -> FakeAbility {
    FakeAbility {
        source: 11,
        id: "tap_green",
        taps: true,
        produces: vec![green(11).with_rider(ManaRider::new("rider_x"))],
        ..FakeAbility::default()
    }
}

/// Taps source 12 for one black; a triggered mana ability mills a card (library-touching).
fn dig() -> FakeAbility {
    FakeAbility {
        source: 12,
        id: "dig",
        taps: true,
        produces: vec![black(12)],
        mills: 1,
        ..FakeAbility::default()
    }
}

/// Taps source 13 and pays {R} from the pool for one green; its triggered mana ability mills.
fn convert() -> FakeAbility {
    FakeAbility {
        source: 13,
        id: "convert",
        taps: true,
        cost: ManaComponent {
            red: 1,
            ..ManaComponent::default()
        },
        produces: vec![green(13)],
        mills: 1,
        ..FakeAbility::default()
    }
}

/// Removes from `from` one unit equal to each unit of `remove` (multiset difference).
fn without(from: &[ManaUnit], remove: &[ManaUnit]) -> (Vec<ManaUnit>, Vec<ManaUnit>) {
    let mut left: Vec<ManaUnit> = remove.to_vec();
    let mut kept = Vec::new();
    for u in from {
        if let Some(pos) = left.iter().position(|g| g == u) {
            left.remove(pos);
        } else {
            kept.push(u.clone());
        }
    }
    (kept, left)
}

impl Transactional for Table {
    fn state_hash(&self) -> Digest {
        let mut out = Vec::new();
        self.pools[0].encode_canonical(&mut out);
        self.pools[1].encode_canonical(&mut out);
        let tapped: Vec<u64> = self.tapped.iter().copied().collect();
        tapped.encode_canonical(&mut out);
        self.library.encode_canonical(&mut out);
        self.graveyard.encode_canonical(&mut out);
        self.life.encode_canonical(&mut out);
        self.prio
            .map_or(0, PlayerId::get)
            .encode_canonical(&mut out);
        self.stack.encode_canonical(&mut out);
        self.log.encode_canonical(&mut out);
        self.history.encode_canonical(&mut out);
        self.cast.encode_canonical(&mut out);
        self.queued.encode_canonical(&mut out);
        leaf_digest(&out)
    }

    fn priority_player(&self) -> Option<PlayerId> {
        self.prio
    }

    /// CR 734.1: carries the library, graveyard, tapped set and pool changes of a durable step.
    fn carry_durable(mut restored: Table, before: &Table, after: &Table) -> Table {
        if before.library != after.library {
            restored.library = after.library.clone();
        }
        if before.graveyard != after.graveyard {
            restored.graveyard = after.graveyard.clone();
        }
        for t in after.tapped.difference(&before.tapped) {
            restored.tapped.insert(*t);
        }
        for t in before.tapped.difference(&after.tapped) {
            restored.tapped.remove(t);
        }
        for i in 0..2 {
            // added = after - before; gone = before - after (multisets).
            let (added, _) = without(after.pools[i].units(), before.pools[i].units());
            let (gone, _) = without(before.pools[i].units(), after.pools[i].units());
            let (kept, _) = without(restored.pools[i].units(), &gone);
            let mut pool = ManaPool::new();
            for u in kept.into_iter().chain(added) {
                pool.add(u);
            }
            restored.pools[i] = pool;
        }
        restored
    }
}

impl PayState for Table {
    fn mana_pool(&self, player: PlayerId) -> &ManaPool {
        &self.pools[slot(player)]
    }

    fn mana_pool_mut(&mut self, player: PlayerId) -> &mut ManaPool {
        &mut self.pools[slot(player)]
    }

    fn available_mana_abilities(&self, _player: PlayerId) -> Vec<(ObjectId, String)> {
        let mut v: Vec<(ObjectId, String)> = self
            .abilities
            .iter()
            .filter(|a| !(a.taps && self.tapped.contains(&a.source)))
            .map(|a| (o(a.source), a.id.to_owned()))
            .collect();
        v.sort();
        v
    }

    fn activate_mana_ability(
        &mut self,
        player: PlayerId,
        source: ObjectId,
        ability_id: &str,
    ) -> Result<ManaActivation, PayError> {
        let Some(a) = self
            .abilities
            .iter()
            .find(|a| o(a.source) == source && a.id == ability_id)
            .cloned()
        else {
            return Err(PayError::Illegal("no such ability".to_owned()));
        };
        if a.refuses || (a.taps && self.tapped.contains(&a.source)) {
            return Err(PayError::Illegal("can't be activated".to_owned()));
        }
        let purpose = SpendPurpose::new(["ability"]);
        let Some(plan) = plan_payment(self.mana_pool(player), &a.cost, &purpose) else {
            return Err(PayError::Unpayable);
        };
        spend(self.mana_pool_mut(player), &plan);
        if a.taps {
            self.tapped.insert(a.source);
        }
        for u in &a.produces {
            self.mana_pool_mut(player).add(u.clone());
        }
        // CR 605.4a: the triggered mana ability resolves immediately.
        self.mill(a.mills);
        self.log.push(format!("activate {}", a.id));
        Ok(ManaActivation {
            library_touching: a.mills > 0,
        })
    }

    fn cost_is_late(&self, _player: PlayerId, item: &CostAST) -> bool {
        self.items.iter().any(|(c, r)| {
            c == item
                && matches!(
                    r,
                    ItemRule::Late | ItemRule::LateLibrary | ItemRule::LateUnpayable
                )
        })
    }

    fn pay_cost_item(
        &mut self,
        _player: PlayerId,
        _stack_object: ObjectId,
        item: &CostAST,
    ) -> Result<ManaActivation, PayError> {
        let rule = self
            .items
            .iter()
            .find(|(c, _)| c == item)
            .map_or(ItemRule::Plain, |(_, r)| *r);
        match rule {
            ItemRule::Unpayable | ItemRule::LateUnpayable => Err(PayError::Unpayable),
            ItemRule::Plain | ItemRule::Late => {
                self.log.push(item.name().to_owned());
                Ok(ManaActivation::default())
            }
            ItemRule::LateLibrary => {
                self.mill(1);
                self.log.push(item.name().to_owned());
                Ok(ManaActivation {
                    library_touching: true,
                })
            }
        }
    }

    fn pay_life(&mut self, _player: PlayerId, amount: u64) -> Result<(), PayError> {
        if amount > self.life {
            return Err(PayError::Unpayable);
        }
        self.life -= amount;
        self.log.push(format!("life {amount}"));
        Ok(())
    }

    fn payment_methods(
        &self,
        _player: PlayerId,
        _stack_object: ObjectId,
        _cost: &LockedCost,
    ) -> Vec<PaymentMethodOffer> {
        self.methods.clone()
    }

    fn apply_payment_method(
        &mut self,
        _player: PlayerId,
        _stack_object: ObjectId,
        offer: &PaymentMethodOffer,
        objects: &[ObjectId],
        remaining: &ManaComponent,
    ) -> Result<ManaComponent, PayError> {
        let mut out = *remaining;
        match self.method_mode {
            MethodMode::Fail => return Err(PayError::Illegal("refused".to_owned())),
            MethodMode::Reduce(each) => {
                let n = u64::try_from(objects.len()).unwrap();
                out.generic = out.generic.saturating_sub(n.saturating_mul(each));
            }
            MethodMode::Overshoot => {
                out.generic = out.generic.saturating_add(5);
                out.red = out.red.saturating_add(1);
            }
        }
        for obj in objects {
            self.tapped.insert(obj.get());
        }
        self.log.push(format!("method {}", offer.label));
        Ok(out)
    }

    fn record_cast(&mut self, _player: PlayerId, _stack_object: ObjectId, history: SpellHistory) {
        self.history = Some(history);
        self.cast = true;
    }

    fn queue_cast_triggers(&mut self, _player: PlayerId, stack_object: ObjectId) {
        self.queued.push(stack_object.get());
    }

    fn purpose(&self, _player: PlayerId, _stack_object: ObjectId) -> SpendPurpose {
        SpendPurpose::new(self.purpose_tags.iter().copied())
    }
}

// ---------------------------------------------------------------------------------------------
// Drivers
// ---------------------------------------------------------------------------------------------

fn mana(f: impl FnOnce(&mut ManaComponent)) -> ManaComponent {
    let mut c = ManaComponent::default();
    f(&mut c);
    c
}

fn locked(m: ManaComponent, life: u64, other: Vec<CostAST>) -> LockedCost {
    determine_total_cost(
        &CostBasis {
            mana: ManaPart::Payable(m),
            phyrexian_life: life,
            other,
        },
        &[],
        &CostStages::default(),
        &[],
    )
    .expect("locked")
}

fn unpayable() -> LockedCost {
    determine_total_cost(
        &CostBasis {
            mana: ManaPart::Unpayable,
            phyrexian_life: 0,
            other: Vec::new(),
        },
        &[],
        &CostStages::default(),
        &[],
    )
    .expect("locked")
}

fn pay(abilities: &[(u64, &str)], methods: Vec<(u32, Vec<ObjectId>)>) -> ChoiceResponse {
    ChoiceResponse::PayCosts {
        auto_pay: false,
        methods,
        mana_abilities: abilities
            .iter()
            .map(|(s, a)| (o(*s), (*a).to_owned()))
            .collect(),
    }
}

/// The whole-cast procedure under test: checkpoint, the spell put on the stack (standing in for
/// announce), then the payment with `base` history.
fn procedure(
    s: &mut Table,
    ctx: &mut ChoiceContext<'_>,
    cost: &LockedCost,
    base: &SpellHistory,
) -> Result<PayOutcome, Interrupt> {
    let mut txn = Transactions::new(s.clone());
    let cp = txn.checkpoint();
    txn.state_mut().stack.push(STACK);
    let out = pay_total_cost(&mut txn, cp, ctx, p(1), o(STACK), cost, base.clone())?;
    *s = txn
        .into_state()
        .map_err(|e| Interrupt::Error(ChoiceError::Procedure(e.to_string())))?;
    Ok(out)
}

fn run_with(
    t: &Table,
    cost: &LockedCost,
    base: &SpellHistory,
    answers: &[ChoiceResponse],
) -> (Table, PayOutcome) {
    let cost = cost.clone();
    let base = base.clone();
    let session = ChoiceSession::replay(
        t.clone(),
        move |s: &mut Table, ctx: &mut ChoiceContext<'_>| procedure(s, ctx, &cost, &base),
        answers,
    )
    .expect("session");
    match session.status() {
        Status::Done { state, output } => (state.clone(), output.clone()),
        Status::Pending(r) => panic!("still pending: {r:?}"),
    }
}

fn run(t: &Table, cost: &LockedCost, answers: &[ChoiceResponse]) -> (Table, PayOutcome) {
    run_with(t, cost, &SpellHistory::default(), answers)
}

/// The first request the procedure asks.
fn first_request(t: &Table, cost: &LockedCost) -> PendingChoiceRequest {
    let cost = cost.clone();
    let session = ChoiceSession::start(
        t.clone(),
        move |s: &mut Table, ctx: &mut ChoiceContext<'_>| {
            procedure(s, ctx, &cost, &SpellHistory::default())
        },
    )
    .expect("session");
    session.pending().expect("pending").clone()
}

fn rolled(out: &PayOutcome) -> PayRolledBack {
    match out {
        PayOutcome::RolledBack(r) => *r,
        other => panic!("not rolled back: {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Commit (CR 601.2g, 601.2h, 601.2i)
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, CR 601.2g, 605.3a: the PAY_MANA_AND_COSTS request lists the available mana
/// abilities, no methods when there is no offer, auto-pay only when the pool already pays, is
/// not optional and is cancellable.
#[test]
fn req_cast_001_pay_request_shape() {
    let t = Table::new().with(tap_green()).with(tap_red());
    let cost = locked(
        mana(|c| {
            c.generic = 1;
            c.green = 1;
        }),
        2,
        vec![CostAST::TapSource],
    );
    let r = first_request(&t, &cost);
    assert_eq!(r.request_id.get(), 0);
    assert_eq!(r.chooser, p(1));
    assert_eq!(r.choice_type, ChoiceType::PayManaAndCosts);
    assert_eq!(r.reason.cr, "CR 601.2g");
    assert_eq!(r.reason.source, Some(o(STACK)));
    assert_eq!(r.default_on_timeout, ChoiceResponse::CancelAndRollback);
    match r.payload {
        ChoicePayload::PayManaAndCosts {
            locked_total,
            methods,
            mana_abilities,
            auto_pay,
            optional,
            cancellable,
        } => {
            assert_eq!(locked_total, "{1}{G}, 2 life, TAP_SOURCE");
            assert!(methods.is_empty());
            assert_eq!(
                mana_abilities,
                vec![
                    (o(10), "tap_red".to_owned()),
                    (o(11), "tap_green".to_owned())
                ]
            );
            assert!(!auto_pay);
            assert!(!optional);
            assert!(cancellable);
        }
        other => panic!("payload {other:?}"),
    }
    // With the pool already holding enough mana, auto-pay is offered (and is the default).
    let mut t2 = t.clone();
    t2.pools[0].add(green(1));
    t2.pools[0].add(red(1));
    let r = first_request(&t2, &cost);
    assert!(matches!(
        r.payload,
        ChoicePayload::PayManaAndCosts { auto_pay: true, .. }
    ));
    assert!(matches!(
        r.default_on_timeout,
        ChoiceResponse::PayCosts { auto_pay: true, .. }
    ));
    // The display survives a total that has no mana-cost form.
    let huge = locked(mana(|c| c.generic = u64::MAX), 0, Vec::new());
    let r = first_request(&t, &huge);
    assert!(matches!(
        r.payload,
        ChoicePayload::PayManaAndCosts { ref locked_total, .. } if !locked_total.is_empty()
    ));
}

/// REQ-CAST-001, CR 605.3b, 601.2h, 601.2i, 603.3, 106.6a: mana abilities activated without the
/// stack pay the cost; the spell becomes cast with its history; cast triggers are queued; the
/// riders of the spent units are reported per unit.
#[test]
fn req_cast_001_committed_payment() {
    let t = Table::new().with(tap_red()).with(tap_green());
    let cost = locked(
        mana(|c| {
            c.generic = 1;
            c.green = 1;
        }),
        0,
        Vec::new(),
    );
    let mut base = SpellHistory::default();
    base.costs_paid.push("additional.kicker".to_owned());
    base.x_value = Some(2);
    let (s, out) = run_with(
        &t,
        &cost,
        &base,
        &[pay(&[(10, "tap_red"), (11, "tap_green")], Vec::new())],
    );
    let PayOutcome::Committed {
        spent,
        riders_fired,
    } = out
    else {
        panic!("not committed: {out:?}");
    };
    assert_eq!(
        spent,
        vec![red(10), green(11).with_rider(ManaRider::new("rider_x"))]
    );
    assert_eq!(
        riders_fired,
        vec![Vec::new(), vec![ManaRider::new("rider_x")]]
    );
    assert!(s.pool().is_empty());
    assert_eq!(s.tapped, BTreeSet::from([10, 11]));
    assert!(s.cast);
    assert_eq!(s.queued, vec![STACK]);
    assert_eq!(s.stack, vec![STACK]);
    assert_eq!(s.log, vec!["activate tap_red", "activate tap_green"]);
    let h = s.history.expect("history");
    assert_eq!(h.costs_paid, vec!["additional.kicker", "mana"]);
    assert_eq!(h.x_value, Some(2));
    assert_eq!(
        h.mana_spent.get(&("red".to_owned(), "nonsnow".to_owned())),
        Some(&1)
    );
    assert_eq!(
        h.mana_spent
            .get(&("green".to_owned(), "nonsnow".to_owned())),
        Some(&1)
    );
    assert_eq!(h.mana_spent.len(), 2);
}

/// REQ-CAST-001, CR 601.2h, 106.4a: auto-pay spends from the pool; excess mana stays; snow
/// mana is recorded under "snow".
#[test]
fn req_cast_001_auto_pay_and_snow_history() {
    let mut t = Table::new();
    t.pools[0].add(green(1).snow());
    t.pools[0].add(red(2));
    t.pools[0].add(red(3));
    let cost = locked(
        mana(|c| {
            c.snow = 1;
            c.red = 1;
        }),
        0,
        Vec::new(),
    );
    let auto = ChoiceResponse::PayCosts {
        auto_pay: true,
        methods: Vec::new(),
        mana_abilities: Vec::new(),
    };
    let (s, out) = run(&t, &cost, &[auto]);
    assert!(matches!(out, PayOutcome::Committed { .. }));
    assert_eq!(s.pool().units(), &[red(3)]);
    let h = s.history.expect("history");
    assert_eq!(
        h.mana_spent.get(&("green".to_owned(), "snow".to_owned())),
        Some(&1)
    );
    assert_eq!(
        h.mana_spent.get(&("red".to_owned(), "nonsnow".to_owned())),
        Some(&1)
    );
}

/// REQ-CAST-001, CR 106.6: restricted mana is spent only on an allowed purpose; otherwise the
/// cast rolls back.
#[test]
fn req_cast_001_restricted_mana_in_payment() {
    let mut t = Table::new();
    t.pools[0].add(red(1).restricted(ManaRestriction::only_for(["spell:artifact"])));
    let cost = locked(mana(|c| c.generic = 1), 0, Vec::new());
    let (_, out) = run(&t, &cost, &[pay(&[], Vec::new())]);
    assert_eq!(rolled(&out).reason, PayStopReason::UnpayableMana);
    t.purpose_tags = vec!["spell", "spell:artifact"];
    let (s, out) = run(&t, &cost, &[pay(&[], Vec::new())]);
    assert!(matches!(out, PayOutcome::Committed { .. }));
    assert!(s.pool().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Rollback (CR 734.1, 734.2)
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, REQ-CORE-005, CR 601.2h, 734.1, 734.2: an unpayable mana part rolls back to
/// the identical hash, the priority player is kept, the spell leaves the stack, nothing is
/// queued, and the non-library mana ability is reversed.
#[test]
fn req_core_005_unpayable_mana_rolls_back_identically() {
    let t = Table::new().with(tap_red());
    let cost = locked(mana(|c| c.green = 1), 0, Vec::new());
    let (s, out) = run(&t, &cost, &[pay(&[(10, "tap_red")], Vec::new())]);
    let r = rolled(&out);
    assert_eq!(r.reason, PayStopReason::UnpayableMana);
    assert_eq!(r.state_hash, t.state_hash());
    assert_eq!(s.state_hash(), t.state_hash());
    assert_eq!(r.priority_player, Some(p(1)));
    assert!(s.stack.is_empty());
    assert!(s.queued.is_empty());
    assert!(!s.cast);
    assert!(s.tapped.is_empty());
    assert!(s.pool().is_empty());
}

/// REQ-CAST-001, CR 118.6: an unpayable mana part makes the attempt illegal without an ask.
#[test]
fn req_cast_001_unpayable_cost_no_ask() {
    let t = Table::new().with(tap_red());
    let (s, out) = run(&t, &unpayable(), &[]);
    let r = rolled(&out);
    assert_eq!(r.reason, PayStopReason::UnpayableCost);
    assert_eq!(s.state_hash(), t.state_hash());
}

/// REQ-CORE-005, REQ-CAST-001, CR 734.1 (issue #54 acceptance criterion 1): the cost is
/// unpayable and the cast rolls back; the library-touching activation survives (card milled,
/// its mana and its tap kept) while the plain activation is reversed.
#[test]
fn req_core_005_library_touching_activation_survives_rollback() {
    let t = Table::new().with(tap_red()).with(dig());
    let cost = locked(mana(|c| c.generic = 5), 0, Vec::new());
    let (s, out) = run(
        &t,
        &cost,
        &[pay(&[(10, "tap_red"), (12, "dig")], Vec::new())],
    );
    let r = rolled(&out);
    assert_eq!(r.reason, PayStopReason::UnpayableMana);
    assert_eq!(r.priority_player, Some(p(1)));
    assert_eq!(r.state_hash, s.state_hash());
    assert_ne!(r.state_hash, t.state_hash());
    assert_eq!(s.library, vec![1, 2, 3]);
    assert_eq!(s.graveyard, vec![4]);
    assert_eq!(s.tapped, BTreeSet::from([12]));
    assert_eq!(s.pool().units(), &[black(12)]);
    assert!(s.stack.is_empty());
    assert!(s.queued.is_empty());
    assert!(!s.cast);
    // The other activation order gives the same survivors.
    let (s2, _) = run(
        &t,
        &cost,
        &[pay(&[(12, "dig"), (10, "tap_red")], Vec::new())],
    );
    assert_eq!(s2.tapped, BTreeSet::from([12]));
    assert_eq!(s2.pool().units(), &[black(12)]);
    assert_eq!(s2.graveyard, vec![4]);
}

/// REQ-CORE-005, CR 734.1: an activation whose mana was spent on a durable (not reversed) mana
/// ability is not reversed either; one whose mana was not consumed is.
#[test]
fn req_core_005_dependent_activation_kept() {
    let t = Table::new().with(tap_red()).with(convert());
    let cost = locked(mana(|c| c.generic = 9), 0, Vec::new());
    let (s, out) = run(
        &t,
        &cost,
        &[pay(&[(10, "tap_red"), (13, "convert")], Vec::new())],
    );
    assert_eq!(rolled(&out).reason, PayStopReason::UnpayableMana);
    // Both survive: the red was consumed by the durable conversion.
    assert_eq!(s.tapped, BTreeSet::from([10, 13]));
    assert_eq!(s.pool().units(), &[green(13)]);
    assert_eq!(s.graveyard, vec![4]);
    // Control: with a pre-existing red listed first in the pool, the conversion consumes that
    // one, so the tap-red activation's own mana is not consumed and it is reversed.
    let mut t2 = t.clone();
    t2.pools[0].add(red(1));
    let (s2, _) = run(
        &t2,
        &cost,
        &[pay(&[(10, "tap_red"), (13, "convert")], Vec::new())],
    );
    assert!(s2.tapped.contains(&13));
    assert!(!s2.tapped.contains(&10));
    assert_eq!(s2.pool().units(), &[green(13)]);
}

/// REQ-CAST-001, CR 734.1: cancelling rolls the cast back to the identical state.
#[test]
fn req_cast_001_cancel_rolls_back() {
    let t = Table::new().with(tap_red());
    let cost = locked(mana(|c| c.red = 1), 0, Vec::new());
    let (s, out) = run(&t, &cost, &[ChoiceResponse::CancelAndRollback]);
    let PayOutcome::Cancelled(r) = out else {
        panic!("not cancelled: {out:?}");
    };
    assert_eq!(r.reason, PayStopReason::Cancelled);
    assert_eq!(r.state_hash, t.state_hash());
    assert_eq!(r.priority_player, Some(p(1)));
    assert_eq!(s.state_hash(), t.state_hash());
}

/// REQ-CAST-001, R12: DECLINE_OPTIONAL_PAYMENT is not offered, so it is rejected; so is an
/// unlisted mana ability.
#[test]
fn req_cast_001_decline_rejected() {
    let t = Table::new();
    let cost = locked(mana(|c| c.red = 1), 0, Vec::new());
    let mut session = ChoiceSession::start(t, move |s: &mut Table, ctx: &mut ChoiceContext<'_>| {
        procedure(s, ctx, &cost, &SpellHistory::default())
    })
    .expect("start");
    let id = session.pending().expect("pending").request_id;
    assert!(
        session
            .submit(id, ChoiceResponse::DeclineOptionalPayment)
            .is_err()
    );
    assert!(
        session
            .submit(id, pay(&[(99, "nothing")], Vec::new()))
            .is_err()
    );
}

// ---------------------------------------------------------------------------------------------
// Payment order and no partial payment (CR 601.2h, 118.3)
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, CR 601.2h: costs that are not late are paid first, whatever the locked order;
/// a {0} mana part needs no ask (CR 118.5); costs_paid follows the payment order.
#[test]
fn req_cast_001_first_pass_before_late() {
    let mut t = Table::new();
    t.items = vec![
        (CostAST::Forage, ItemRule::Late),
        (CostAST::TapSource, ItemRule::Plain),
    ];
    let cost = locked(
        ManaComponent::default(),
        0,
        vec![CostAST::Forage, CostAST::TapSource],
    );
    let (s, out) = run(&t, &cost, &[]);
    assert!(matches!(out, PayOutcome::Committed { .. }));
    assert_eq!(s.log, vec!["TAP_SOURCE", "FORAGE"]);
    assert_eq!(
        s.history.expect("history").costs_paid,
        vec!["mana", "tap_source", "forage"]
    );
}

/// REQ-CAST-001, CR 601.2h, 118.3b: life that can't be paid rolls back with nothing spent;
/// payable life is paid and recorded.
#[test]
fn req_cast_001_life_all_or_nothing() {
    let mut t = Table::new();
    t.pools[0].add(red(1));
    t.life = 3;
    let cost = locked(mana(|c| c.red = 1), 4, Vec::new());
    let (s, out) = run(&t, &cost, &[pay(&[], Vec::new())]);
    assert_eq!(rolled(&out).reason, PayStopReason::UnpayableMana);
    assert_eq!(s.state_hash(), t.state_hash());
    assert_eq!(s.pool().units(), &[red(1)]);
    t.life = 4;
    let (s, out) = run(&t, &cost, &[pay(&[], Vec::new())]);
    assert!(matches!(out, PayOutcome::Committed { .. }));
    assert_eq!(s.life, 0);
    assert_eq!(s.history.expect("history").costs_paid, vec!["mana", "life"]);
}

/// REQ-CAST-001, CR 601.2h, 118.3: an item that can't be paid after others were paid rolls
/// everything back to the identical state (no partial payment).
#[test]
fn req_cast_001_item_all_or_nothing() {
    let mut t = Table::new();
    t.pools[0].add(red(1));
    t.items = vec![
        (CostAST::TapSource, ItemRule::Plain),
        (CostAST::UntapSource, ItemRule::Unpayable),
    ];
    let cost = locked(
        mana(|c| c.red = 1),
        2,
        vec![CostAST::TapSource, CostAST::UntapSource],
    );
    let (s, out) = run(&t, &cost, &[pay(&[], Vec::new())]);
    let r = rolled(&out);
    assert_eq!(r.reason, PayStopReason::UnpayableMana);
    assert_eq!(r.state_hash, t.state_hash());
    assert_eq!(s.state_hash(), t.state_hash());
}

/// REQ-CORE-005, CR 601.2h, 734.1: a late cost item that touched a library survives the
/// rollback caused by a later item that can't be paid.
#[test]
fn req_core_005_library_cost_item_survives() {
    let mut t = Table::new();
    t.items = vec![
        (CostAST::Forage, ItemRule::LateLibrary),
        (CostAST::UntapSource, ItemRule::LateUnpayable),
        (CostAST::TapSource, ItemRule::Plain),
    ];
    let cost = locked(
        ManaComponent::default(),
        0,
        vec![CostAST::Forage, CostAST::UntapSource, CostAST::TapSource],
    );
    let (s, out) = run(&t, &cost, &[]);
    assert_eq!(rolled(&out).reason, PayStopReason::UnpayableMana);
    assert_eq!(s.library, vec![1, 2, 3]);
    assert_eq!(s.graveyard, vec![4]);
    assert!(s.stack.is_empty());
    assert!(s.log.is_empty());
    assert!(!s.cast);
}

// ---------------------------------------------------------------------------------------------
// Mana abilities (CR 605.3c, 118.3c)
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001, CR 605.3c: the procedure's guard refuses beginning an activation that is in
/// progress, and allows it again once finished.
#[test]
fn req_cast_001_reactivation_guard() {
    let mut g = ActivationGuard::default();
    assert_eq!(g.begin(o(10), "tap_red"), Ok(()));
    assert!(matches!(
        g.begin(o(10), "tap_red"),
        Err(PayError::Illegal(_))
    ));
    assert_eq!(g.begin(o(11), "tap_red"), Ok(()));
    g.finish(o(10), "tap_red");
    assert_eq!(g.begin(o(10), "tap_red"), Ok(()));
}

/// REQ-CAST-001, CR 605.3a, 605.3c, 118.3c: an ability listed twice is activated again only if
/// it is available again; a tap ability is not, a repeatable one is; a refused activation is
/// skipped and leaves no trace.
#[test]
fn req_cast_001_listed_twice_and_refused() {
    let repeat = FakeAbility {
        source: 14,
        id: "repeat",
        produces: vec![red(14)],
        ..FakeAbility::default()
    };
    let refused = FakeAbility {
        source: 15,
        id: "refused",
        refuses: true,
        produces: vec![red(15)],
        ..FakeAbility::default()
    };
    let t = Table::new().with(tap_red()).with(repeat).with(refused);
    let rr = locked(mana(|c| c.red = 2), 0, Vec::new());
    let (s, out) = run(
        &t,
        &rr,
        &[pay(
            &[(10, "tap_red"), (10, "tap_red"), (15, "refused")],
            Vec::new(),
        )],
    );
    assert_eq!(rolled(&out).reason, PayStopReason::UnpayableMana);
    assert_eq!(s.state_hash(), t.state_hash());
    let (s, out) = run(
        &t,
        &rr,
        &[pay(&[(14, "repeat"), (14, "repeat")], Vec::new())],
    );
    assert!(matches!(out, PayOutcome::Committed { .. }));
    assert!(s.pool().is_empty());
    assert_eq!(s.log, vec!["activate repeat", "activate repeat"]);
}

/// REQ-CAST-001, CR 734.1: the scratch-run unit tracking credits new units to the activation and
/// reports the producers of removed units.
#[test]
fn req_cast_001_follow_units() {
    let before = vec![red(1), green(2), red(1)];
    let origin = vec![None, Some(0), Some(1)];
    let after = vec![red(1), red(1), black(3)];
    let (gone, next) = follow_units(&before, &after, &origin, 2);
    assert_eq!(gone, vec![0]);
    assert_eq!(next, vec![None, Some(1), Some(2)]);
}

// ---------------------------------------------------------------------------------------------
// Payment methods (SRS 7.1 step 8)
// ---------------------------------------------------------------------------------------------

fn offer() -> PaymentMethodOffer {
    PaymentMethodOffer {
        label: "method_tap".to_owned(),
        eligible: vec![o(20), o(21)],
    }
}

/// REQ-CAST-001, CR 601.2h, 702.51b: a payment method pays generic mana by tapping objects; the
/// locked cost is unchanged; the method is listed in the request and recorded.
#[test]
fn req_cast_001_payment_method_reduces_remaining() {
    let mut t = Table::new();
    t.methods = vec![offer()];
    t.pools[0].add(green(1));
    let cost = locked(
        mana(|c| {
            c.generic = 2;
            c.green = 1;
        }),
        0,
        Vec::new(),
    );
    let before = cost.clone();
    let r = first_request(&t, &cost);
    assert!(matches!(
        r.payload,
        ChoicePayload::PayManaAndCosts { ref methods, .. }
            if *methods == vec![("method_tap".to_owned(), vec![o(20), o(21)])]
    ));
    let (s, out) = run(&t, &cost, &[pay(&[], vec![(0, vec![o(20), o(21)])])]);
    assert!(matches!(out, PayOutcome::Committed { .. }));
    assert_eq!(cost, before);
    assert_eq!(s.tapped, BTreeSet::from([20, 21]));
    assert!(s.pool().is_empty());
    assert_eq!(
        s.history.expect("history").costs_paid,
        vec!["method.method_tap", "mana"]
    );
    // Using the method with no object is not using it.
    let (_, out) = run(&t, &cost, &[pay(&[], vec![(0, Vec::new())])]);
    assert_eq!(rolled(&out).reason, PayStopReason::UnpayableMana);
}

/// REQ-CAST-001: a method can never leave more to pay than before (clamped component-wise) and
/// never produces a negative amount; a refused method rolls the cast back.
#[test]
fn req_cast_001_payment_method_clamped() {
    let mut t = Table::new();
    t.methods = vec![offer()];
    t.pools[0].add(green(1));
    let cost = locked(
        mana(|c| {
            c.generic = 1;
            c.green = 1;
        }),
        0,
        Vec::new(),
    );
    // Reducing more than remains saturates at zero.
    t.method_mode = MethodMode::Reduce(5);
    let (s, out) = run(&t, &cost, &[pay(&[], vec![(0, vec![o(20)])])]);
    assert!(matches!(out, PayOutcome::Committed { .. }));
    assert!(s.pool().is_empty());
    // Returning more than remaining is clamped: the pool must still pay {1}{G} and can't.
    t.method_mode = MethodMode::Overshoot;
    let (s, out) = run(&t, &cost, &[pay(&[], vec![(0, vec![o(20)])])]);
    assert_eq!(rolled(&out).reason, PayStopReason::UnpayableMana);
    assert_eq!(s.state_hash(), t.state_hash());
    t.pools[0].add(red(2));
    let (s, out) = run(&t, &cost, &[pay(&[], vec![(0, vec![o(20)])])]);
    assert!(matches!(out, PayOutcome::Committed { .. }));
    assert!(s.pool().is_empty());
    // A refused method rolls back.
    t.method_mode = MethodMode::Fail;
    let (s, out) = run(&t, &cost, &[pay(&[], vec![(0, vec![o(20)])])]);
    assert_eq!(rolled(&out).reason, PayStopReason::UnpayableMana);
    assert_eq!(s.state_hash(), t.state_hash());
    assert_eq!(
        clamp(
            &mana(|c| c.generic = 9),
            &mana(|c| {
                c.generic = 1;
                c.snow = 1;
            })
        ),
        mana(|c| c.generic = 1)
    );
}

// ---------------------------------------------------------------------------------------------
// Replay, determinism and the precondition (REQ-NET-003, REQ-CORE-001)
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003, REQ-CORE-001: ask, suspend (state untouched), resume gives the same outcome and
/// hash as a replay; ten runs agree, for a commit and for a rollback with a durable step.
#[test]
fn req_net_003_pay_replay_round_trip() {
    let t = Table::new().with(tap_red()).with(dig());
    let cost = locked(mana(|c| c.generic = 2), 0, Vec::new());
    let answer = pay(&[(12, "dig"), (10, "tap_red")], Vec::new());
    let c2 = cost.clone();
    let mut session = ChoiceSession::start(
        t.clone(),
        move |s: &mut Table, ctx: &mut ChoiceContext<'_>| {
            procedure(s, ctx, &c2, &SpellHistory::default())
        },
    )
    .expect("start");
    assert_eq!(session.state_hash(), t.state_hash());
    let id = session.pending().expect("pending").request_id;
    session.submit(id, answer.clone()).expect("submit");
    let Status::Done { state, output } = session.status().clone() else {
        panic!("not done");
    };
    assert!(matches!(output, PayOutcome::Committed { .. }));
    let (replayed, out) = run(&t, &cost, std::slice::from_ref(&answer));
    assert_eq!(out, output);
    assert_eq!(replayed.state_hash(), state.state_hash());
    for _ in 0..10 {
        let (again, out) = run(&t, &cost, std::slice::from_ref(&answer));
        assert_eq!(out, output);
        assert_eq!(again.state_hash(), state.state_hash());
    }
    let big = locked(mana(|c| c.generic = 7), 0, Vec::new());
    let (first, out1) = run(&t, &big, std::slice::from_ref(&answer));
    assert!(matches!(out1, PayOutcome::RolledBack(_)));
    for _ in 0..10 {
        let (again, out) = run(&t, &big, std::slice::from_ref(&answer));
        assert_eq!(out, out1);
        assert_eq!(again.state_hash(), first.state_hash());
    }
}

/// REQ-CAST-001: the cast checkpoint must be the innermost open transaction; otherwise a
/// procedure error and nothing changes.
#[test]
fn req_cast_001_precondition_innermost() {
    let t = Table::new().with(tap_red());
    let cost = locked(mana(|c| c.red = 1), 0, Vec::new());
    let result = ChoiceSession::start(t, move |s: &mut Table, ctx: &mut ChoiceContext<'_>| {
        let mut txn = Transactions::new(s.clone());
        let cp = txn.checkpoint();
        let inner = txn.checkpoint();
        let hash = txn.state().state_hash();
        let out = pay_total_cost(
            &mut txn,
            cp,
            ctx,
            p(1),
            o(STACK),
            &cost,
            SpellHistory::default(),
        );
        assert!(matches!(
            out,
            Err(Interrupt::Error(ChoiceError::Procedure(_)))
        ));
        assert_eq!(txn.depth(), 2);
        assert_eq!(txn.state().state_hash(), hash);
        txn.rollback(inner).expect("inner");
        txn.rollback(cp).expect("cp");
        // A closed checkpoint is not open at all.
        let out2 = pay_total_cost(
            &mut txn,
            cp,
            ctx,
            p(1),
            o(STACK),
            &cost,
            SpellHistory::default(),
        );
        assert!(matches!(
            out2,
            Err(Interrupt::Error(ChoiceError::Procedure(_)))
        ));
        out
    });
    assert!(matches!(result, Err(ChoiceError::Procedure(_))));
}

/// REQ-CAST-001, CR 601.2h, 118.3, 734.1: a late item that can't be paid, after a non-late item
/// and mana were paid, rolls back with `UnpayableMana`; the state equals the checkpoint.
#[test]
fn req_cast_001_late_item_unpayable_after_first_pass() {
    let mut t = Table::new();
    t.pools[0].add(red(1));
    t.items = vec![
        (CostAST::TapSource, ItemRule::Plain),
        (CostAST::UntapSource, ItemRule::LateUnpayable),
    ];
    let cost = locked(
        mana(|c| c.red = 1),
        0,
        vec![CostAST::TapSource, CostAST::UntapSource],
    );
    let (s, out) = run(&t, &cost, &[pay(&[], Vec::new())]);
    let r = rolled(&out);
    assert_eq!(r.reason, PayStopReason::UnpayableMana);
    assert_eq!(r.state_hash, t.state_hash());
    assert_eq!(s.state_hash(), t.state_hash());
    assert_eq!(s.pool().units(), &[red(1)]);
    assert!(!s.cast);
}

/// REQ-CAST-001, CR 601.2h, 734.1: a library-touching item paid before a later unpayable item
/// keeps its effect after the rollback, which reports `UnpayableMana`.
#[test]
fn req_cast_001_library_item_survives_later_unpayable_item() {
    let mut t = Table::new();
    t.items = vec![
        (CostAST::Forage, ItemRule::LateLibrary),
        (CostAST::UntapSource, ItemRule::LateUnpayable),
    ];
    let cost = locked(
        ManaComponent::default(),
        0,
        vec![CostAST::Forage, CostAST::UntapSource],
    );
    let (s, out) = run(&t, &cost, &[]);
    assert_eq!(rolled(&out).reason, PayStopReason::UnpayableMana);
    assert_eq!(s.graveyard, vec![4]);
    assert!(!s.cast);
}

/// REQ-CAST-001, CR 601.2h, 734.1: a payment method the host refuses rolls the cast back with
/// `UnpayableMana` and nothing is spent.
#[test]
fn req_cast_001_refused_payment_method_spends_nothing() {
    let mut t = Table::new();
    t.methods = vec![offer()];
    t.method_mode = MethodMode::Fail;
    t.pools[0].add(green(1));
    let cost = locked(
        mana(|c| {
            c.generic = 1;
            c.green = 1;
        }),
        0,
        Vec::new(),
    );
    let (s, out) = run(&t, &cost, &[pay(&[], vec![(0, vec![o(20)])])]);
    assert_eq!(rolled(&out).reason, PayStopReason::UnpayableMana);
    assert_eq!(s.state_hash(), t.state_hash());
    assert_eq!(s.pool().units(), &[green(1)]);
    assert!(s.tapped.is_empty());
}
