// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared builders and an independent reference model ("oracle") for the casting total-cost
//! spec tests (REQ-CAST-001; CR 601.2f, 118.6a, 118.7, 118.7a-g, 107.4h, 118.8d, 118.9d, 107.3a).
//!
//! Black-box: written from the work-item brief only (its S1-S7 text), against the pinned public
//! API of `fineprint_core::casting`. The oracle is a tiny reference implementation of the brief's
//! component-wise reduction model, kept deliberately literal so the tests compare the product
//! with the brief rather than with itself.
//!
//! Each `spec_casting_cost*.rs` test file includes this file with `#[path]`. Nothing here names a
//! real card (R5): costs are bare mana symbols, ids are small numbers.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, PlayerId, Timestamp};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_core::casting::{
    CostBasis, CostError, LockedCost, ManaComponent, ManaPart, ReductionStep, SymbolChoice,
    determine_total_cost,
};
use fineprint_core::layers::{CostStages, RuleEntry};
use fineprint_dsl::ability::{CostAST, RuleKind, RuleModification};
use fineprint_dsl::expr::ValueExpressionAST;
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol, Variable};
use serde_json::{Value, json};

/// The player every entry applies to.
pub const P1: PlayerId = PlayerId::new(1);

/// The largest generic amount `to_mana_cost` accepts (pinned API: `MAX_GENERIC`).
pub const MAX_GENERIC: u64 = 1_000_000;

// ---------------------------------------------------------------------------------------------
// builders
// ---------------------------------------------------------------------------------------------

pub fn eid(n: u64) -> EffectId {
    EffectId::new(n)
}

/// Parses mana text; panics on a typo in a test.
pub fn mc(text: &str) -> ManaCostDSL {
    ManaCostDSL::parse(text).expect("test mana text parses")
}

/// `PAY_MANA` with the given text.
pub fn pay(text: &str) -> CostAST {
    CostAST::PayMana { mana: mc(text) }
}

/// A distinguishable non-mana cost item (`PAY_ENERGY{n}`).
pub fn energy(n: i64) -> CostAST {
    CostAST::PayEnergy {
        amount: ValueExpressionAST::Literal(n),
    }
}

/// A rule entry with an explicit kind, typed cost and params.
pub fn entry(id: u64, kind: RuleKind, cost: Option<Vec<CostAST>>, params: Value) -> RuleEntry {
    let mut modification = RuleModification::new(kind);
    modification.cost = cost;
    if let Value::Object(map) = params {
        modification.params = map.into_iter().collect::<BTreeMap<_, _>>();
    }
    RuleEntry {
        id: eid(id),
        players: BTreeSet::from([P1]),
        controller: P1,
        source: None,
        timestamp: Timestamp::new(id),
        modification,
    }
}

/// `COST_INCREASE` with the given cost items.
pub fn inc(id: u64, cost: Vec<CostAST>) -> RuleEntry {
    entry(id, RuleKind::CostIncrease, Some(cost), json!({}))
}

/// `COST_REDUCTION` paying-mana text.
pub fn red(id: u64, text: &str) -> RuleEntry {
    entry(
        id,
        RuleKind::CostReduction,
        Some(vec![pay(text)]),
        json!({}),
    )
}

/// `COST_MINIMUM` with `"minimum": value`.
pub fn min_entry(id: u64, value: Value) -> RuleEntry {
    entry(id, RuleKind::CostMinimum, None, json!({ "minimum": value }))
}

/// `COST_SET` to the given mana text.
pub fn set_entry(id: u64, text: &str) -> RuleEntry {
    entry(id, RuleKind::CostSet, Some(vec![pay(text)]), json!({}))
}

pub fn stages(
    increases: Vec<RuleEntry>,
    reductions: Vec<RuleEntry>,
    total_effects: Vec<RuleEntry>,
) -> CostStages {
    CostStages {
        grants: Vec::new(),
        increases,
        reductions,
        total_effects,
    }
}

/// A reduction step with no hybrid halves.
pub fn step(id: u64) -> ReductionStep {
    ReductionStep {
        id: eid(id),
        hybrid_halves: Vec::new(),
    }
}

/// A reduction step with hybrid halves.
pub fn step_h(id: u64, halves: &[SymbolChoice]) -> ReductionStep {
    ReductionStep {
        id: eid(id),
        hybrid_halves: halves.to_vec(),
    }
}

/// Steps in the stages' list order, with no halves.
pub fn steps_in_order(st: &CostStages) -> Vec<ReductionStep> {
    st.reductions
        .iter()
        .map(|e| ReductionStep {
            id: e.id,
            hybrid_halves: Vec::new(),
        })
        .collect()
}

/// A payable basis from mana text with no choices, no X and no other items.
pub fn basis(text: &str) -> CostBasis {
    CostBasis::from_mana_cost(Some(&mc(text)), None, &[], Vec::new()).expect("basis builds")
}

/// A component from mana text containing only generic, colored, colorless and snow symbols.
pub fn comp(text: &str) -> ManaComponent {
    match basis(text).mana {
        ManaPart::Payable(m) => m,
        ManaPart::Unpayable => panic!("payable text"),
    }
}

/// The payable component of a locked cost (panics when Unpayable).
pub fn payable(locked: &LockedCost) -> ManaComponent {
    match locked.mana() {
        ManaPart::Payable(m) => *m,
        ManaPart::Unpayable => panic!("expected a payable mana component"),
    }
}

/// Runs the calculator and unwraps.
pub fn total(
    b: &CostBasis,
    additional: &[CostBasis],
    st: &CostStages,
    order: &[ReductionStep],
) -> LockedCost {
    determine_total_cost(b, additional, st, order).expect("total cost determined")
}

/// The canonical text of a payable result.
pub fn text_of(locked: &LockedCost) -> String {
    payable(locked)
        .to_mana_cost()
        .expect("component prints")
        .to_string()
}

/// Every permutation of `items` (lexicographic over indices), deterministic.
pub fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    let n = items.len();
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = vec![idx.iter().map(|&i| items[i].clone()).collect()];
    loop {
        // next lexicographic permutation
        let Some(k) = (0..n.saturating_sub(1))
            .rev()
            .find(|&k| idx[k] < idx[k + 1])
        else {
            return out;
        };
        let l = (k + 1..n).rev().find(|&l| idx[k] < idx[l]).expect("exists");
        idx.swap(k, l);
        idx[k + 1..].reverse();
        out.push(idx.iter().map(|&i| items[i].clone()).collect());
    }
}

// ---------------------------------------------------------------------------------------------
// the oracle (literal transcription of brief S1-S5)
// ---------------------------------------------------------------------------------------------

pub fn col_mut(m: &mut ManaComponent, c: Color) -> &mut u64 {
    match c {
        Color::White => &mut m.white,
        Color::Blue => &mut m.blue,
        Color::Black => &mut m.black,
        Color::Red => &mut m.red,
        Color::Green => &mut m.green,
    }
}

pub fn col(m: &ManaComponent, c: Color) -> u64 {
    match c {
        Color::White => m.white,
        Color::Blue => m.blue,
        Color::Black => m.black,
        Color::Red => m.red,
        Color::Green => m.green,
    }
}

/// Component-wise sum (checked; `None` on overflow).
pub fn add(a: &ManaComponent, b: &ManaComponent) -> Option<ManaComponent> {
    Some(ManaComponent {
        generic: a.generic.checked_add(b.generic)?,
        white: a.white.checked_add(b.white)?,
        blue: a.blue.checked_add(b.blue)?,
        black: a.black.checked_add(b.black)?,
        red: a.red.checked_add(b.red)?,
        green: a.green.checked_add(b.green)?,
        colorless: a.colorless.checked_add(b.colorless)?,
        snow: a.snow.checked_add(b.snow)?,
    })
}

pub fn oracle_total_of(m: &ManaComponent) -> u64 {
    [
        m.generic,
        m.white,
        m.blue,
        m.black,
        m.red,
        m.green,
        m.colorless,
        m.snow,
    ]
    .iter()
    .fold(0u64, |a, b| a.saturating_add(*b))
}

/// Whether a symbol consumes one choice in `from_mana_cost` (S1).
pub fn is_choice_symbol(s: ManaSymbol) -> bool {
    matches!(
        s,
        ManaSymbol::Hybrid(..)
            | ManaSymbol::GenericHybrid(..)
            | ManaSymbol::ColorlessHybrid(_)
            | ManaSymbol::Phyrexian(_)
            | ManaSymbol::HybridPhyrexian(..)
    )
}

/// Whether a symbol consumes one hybrid half in a reduction (S4).
pub fn is_half_symbol(s: ManaSymbol) -> bool {
    matches!(
        s,
        ManaSymbol::Hybrid(..)
            | ManaSymbol::GenericHybrid(..)
            | ManaSymbol::ColorlessHybrid(_)
            | ManaSymbol::HybridPhyrexian(..)
    )
}

/// Oracle for `CostBasis::from_mana_cost` (S1). `Err` holds every error the inputs justify (the
/// product may report any one of them).
pub fn oracle_basis(
    cost: Option<&ManaCostDSL>,
    x: Option<u64>,
    choices: &[SymbolChoice],
    other: Vec<CostAST>,
) -> Result<CostBasis, Vec<CostError>> {
    let Some(cost) = cost else {
        if !choices.is_empty() {
            return Err(vec![CostError::BadChoice]);
        }
        return Ok(CostBasis {
            mana: ManaPart::Unpayable,
            phyrexian_life: 0,
            other,
        });
    };
    let mut errs = Vec::new();
    let needed = cost
        .symbols()
        .iter()
        .filter(|s| is_choice_symbol(**s))
        .count();
    if needed != choices.len() {
        errs.push(CostError::BadChoice);
    }
    let mut m = ManaComponent::default();
    let mut life = 0u64;
    let mut ci = 0usize;
    for &s in cost.symbols() {
        let choice = if is_choice_symbol(s) {
            let c = choices.get(ci).copied();
            ci += 1;
            c
        } else {
            None
        };
        let bump = |slot: &mut u64, n: u64, errs: &mut Vec<CostError>| match slot.checked_add(n) {
            Some(v) => *slot = v,
            None => errs.push(CostError::Overflow),
        };
        match (s, choice) {
            (ManaSymbol::Generic(n), _) => bump(&mut m.generic, u64::from(n), &mut errs),
            (ManaSymbol::Colored(c), _) => bump(col_mut(&mut m, c), 1, &mut errs),
            (ManaSymbol::Colorless, _) => bump(&mut m.colorless, 1, &mut errs),
            (ManaSymbol::Snow, _) => bump(&mut m.snow, 1, &mut errs),
            (ManaSymbol::Variable(Variable::X), _) => match x {
                Some(v) => bump(&mut m.generic, v, &mut errs),
                None => errs.push(CostError::MissingX),
            },
            (ManaSymbol::Variable(_), _) => errs.push(CostError::UnsupportedVariable),
            (ManaSymbol::Hybrid(a, _), Some(SymbolChoice::First))
            | (ManaSymbol::Hybrid(_, a), Some(SymbolChoice::Second))
            | (ManaSymbol::GenericHybrid(_, a), Some(SymbolChoice::Second))
            | (ManaSymbol::ColorlessHybrid(a), Some(SymbolChoice::Second))
            | (ManaSymbol::Phyrexian(a), Some(SymbolChoice::First))
            | (ManaSymbol::HybridPhyrexian(a, _), Some(SymbolChoice::First))
            | (ManaSymbol::HybridPhyrexian(_, a), Some(SymbolChoice::Second)) => {
                bump(col_mut(&mut m, a), 1, &mut errs);
            }
            (ManaSymbol::GenericHybrid(n, _), Some(SymbolChoice::First)) => {
                bump(&mut m.generic, u64::from(n), &mut errs);
            }
            (ManaSymbol::ColorlessHybrid(_), Some(SymbolChoice::First)) => {
                bump(&mut m.colorless, 1, &mut errs);
            }
            (
                ManaSymbol::Phyrexian(_) | ManaSymbol::HybridPhyrexian(..),
                Some(SymbolChoice::Life),
            ) => {
                life += 2;
            }
            _ => errs.push(CostError::BadChoice),
        }
    }
    if errs.is_empty() {
        Ok(CostBasis {
            mana: ManaPart::Payable(m),
            phyrexian_life: life,
            other,
        })
    } else {
        Err(errs)
    }
}

/// The non-mana outcome of the oracle calculator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expected {
    pub mana: ManaPart,
    pub phyrexian_life: u64,
    pub other: Vec<CostAST>,
}

impl Expected {
    pub fn matches(&self, locked: &LockedCost) -> bool {
        *locked.mana() == self.mana
            && locked.phyrexian_life() == self.phyrexian_life
            && locked.other() == self.other.as_slice()
    }
}

/// One half of a symbol as applied by a reduction (S4).
#[derive(Clone, Copy, Debug)]
enum Half {
    Generic(u64),
    Colored(Color),
    Colorless,
    Snow,
}

/// Applies one reduction half to the running component (S4; CR 118.7a-d, 118.7g, 107.4h).
fn reduce_half(m: &mut ManaComponent, h: Half) {
    match h {
        Half::Generic(n) => m.generic = m.generic.saturating_sub(n),
        Half::Colored(c) => {
            let slot = col_mut(m, c);
            if *slot > 0 {
                *slot -= 1;
            } else {
                m.generic = m.generic.saturating_sub(1);
            }
        }
        Half::Colorless => {
            if m.colorless > 0 {
                m.colorless -= 1;
            } else {
                m.generic = m.generic.saturating_sub(1);
            }
        }
        Half::Snow => m.generic = m.generic.saturating_sub(1),
    }
}

/// The reduction symbols of a well-formed `COST_REDUCTION` entry, or `None` when ignored (S3).
pub fn reduction_symbols(e: &RuleEntry) -> Option<Vec<ManaSymbol>> {
    if e.modification.kind != RuleKind::CostReduction {
        return None;
    }
    // A `COST_REDUCTION` with no `cost` member is malformed and ignored, as a `COST_INCREASE`
    // with none is (DECISIONS "#53 FP-051 tester - COST_REDUCTION without a cost member").
    let cost = e.modification.cost.as_ref()?;
    let mut out = Vec::new();
    for item in cost {
        if let CostAST::PayMana { mana } = item {
            for &s in mana.symbols() {
                if matches!(s, ManaSymbol::Variable(_)) {
                    return None;
                }
                out.push(s);
            }
        }
    }
    Some(out)
}

/// The correct number of hybrid halves for a reduction entry (0 when ignored).
pub fn halves_needed(e: &RuleEntry) -> usize {
    reduction_symbols(e).map_or(0, |v| v.iter().filter(|s| is_half_symbol(**s)).count())
}

/// Applies one reduction entry; `Err(())` on a hybrid-halves problem (BadChoice).
fn apply_reduction(
    m: &mut ManaComponent,
    e: &RuleEntry,
    halves: &[SymbolChoice],
) -> Result<(), ()> {
    let Some(symbols) = reduction_symbols(e) else {
        return Ok(());
    };
    let needed = symbols.iter().filter(|s| is_half_symbol(**s)).count();
    if needed != halves.len() {
        return Err(());
    }
    let mut hi = 0usize;
    for s in symbols {
        let half = if is_half_symbol(s) {
            let c = halves[hi];
            hi += 1;
            match (s, c) {
                (ManaSymbol::Hybrid(a, _), SymbolChoice::First)
                | (ManaSymbol::Hybrid(_, a), SymbolChoice::Second)
                | (ManaSymbol::GenericHybrid(_, a), SymbolChoice::Second)
                | (ManaSymbol::ColorlessHybrid(a), SymbolChoice::Second)
                | (ManaSymbol::HybridPhyrexian(a, _), SymbolChoice::First)
                | (ManaSymbol::HybridPhyrexian(_, a), SymbolChoice::Second) => Half::Colored(a),
                (ManaSymbol::GenericHybrid(n, _), SymbolChoice::First) => {
                    Half::Generic(u64::from(n))
                }
                (ManaSymbol::ColorlessHybrid(_), SymbolChoice::First) => Half::Colorless,
                _ => return Err(()),
            }
        } else {
            match s {
                ManaSymbol::Generic(n) => Half::Generic(u64::from(n)),
                ManaSymbol::Colored(c) | ManaSymbol::Phyrexian(c) => Half::Colored(c),
                ManaSymbol::Colorless => Half::Colorless,
                ManaSymbol::Snow => Half::Snow,
                _ => unreachable!("filtered above"),
            }
        };
        reduce_half(m, half);
    }
    Ok(())
}

/// Sum of the `PayMana` items of `cost` restricted to generic/colored/colorless/snow; `None` if
/// another symbol occurs (S3).
fn plain_sum(cost: &[CostAST]) -> Option<(ManaComponent, usize)> {
    let mut m = ManaComponent::default();
    let mut pays = 0usize;
    for item in cost {
        if let CostAST::PayMana { mana } = item {
            pays += 1;
            for &s in mana.symbols() {
                match s {
                    ManaSymbol::Generic(n) => m.generic += u64::from(n),
                    ManaSymbol::Colored(c) => *col_mut(&mut m, c) += 1,
                    ManaSymbol::Colorless => m.colorless += 1,
                    ManaSymbol::Snow => m.snow += 1,
                    _ => return None,
                }
            }
        }
    }
    Some((m, pays))
}

/// Oracle for `determine_total_cost` (S2-S5). `Err` lists every error the inputs justify.
pub fn oracle_total(
    b: &CostBasis,
    additional: &[CostBasis],
    st: &CostStages,
    order: &[ReductionStep],
) -> Result<Expected, Vec<CostError>> {
    let mut errs = Vec::new();
    // (a) basis plus additional costs.
    let mut mana = b.mana;
    let mut life = b.phyrexian_life;
    let mut other = b.other.clone();
    for a in additional {
        match (a.mana, &mut mana) {
            (ManaPart::Unpayable, _) => errs.push(CostError::UnpayableAdditional),
            (ManaPart::Payable(am), ManaPart::Payable(m)) => {
                *m = add(m, &am).expect("small test values");
            }
            (ManaPart::Payable(_), ManaPart::Unpayable) => {}
        }
        life += a.phyrexian_life;
        other.extend(a.other.iter().cloned());
    }
    // (b) increases in list order.
    for e in &st.increases {
        if e.modification.kind != RuleKind::CostIncrease {
            continue;
        }
        let Some(cost) = e.modification.cost.as_ref().filter(|c| !c.is_empty()) else {
            continue;
        };
        let Some((m_add, _)) = plain_sum(cost) else {
            continue;
        };
        if let ManaPart::Payable(m) = &mut mana {
            *m = add(m, &m_add).expect("small test values");
        }
        other.extend(
            cost.iter()
                .filter(|c| !matches!(c, CostAST::PayMana { .. }))
                .cloned(),
        );
    }
    // (c) reduction order validation and application.
    let ids: Vec<EffectId> = st.reductions.iter().map(|e| e.id).collect();
    let given: Vec<EffectId> = order.iter().map(|s| s.id).collect();
    let given_set: BTreeSet<EffectId> = given.iter().copied().collect();
    let id_set: BTreeSet<EffectId> = ids.iter().copied().collect();
    let order_ok =
        given.len() == ids.len() && given_set.len() == given.len() && given_set == id_set;
    if !order_ok {
        errs.push(CostError::BadReductionOrder);
    }
    if order_ok {
        let mut work = match mana {
            ManaPart::Payable(m) => m,
            ManaPart::Unpayable => ManaComponent::default(),
        };
        for s in order {
            let e = st
                .reductions
                .iter()
                .find(|e| e.id == s.id)
                .expect("validated");
            if apply_reduction(&mut work, e, &s.hybrid_halves).is_err() {
                errs.push(CostError::BadChoice);
            }
        }
        if let ManaPart::Payable(m) = &mut mana {
            *m = work;
        }
    } else {
        // the product may also detect a halves problem; list it as a possible error.
        for s in order {
            if let Some(e) = st.reductions.iter().find(|e| e.id == s.id) {
                let mut scratch = ManaComponent::default();
                if apply_reduction(&mut scratch, e, &s.hybrid_halves).is_err() {
                    errs.push(CostError::BadChoice);
                }
            }
        }
    }
    // (d) total effects in list order.
    for e in &st.total_effects {
        let ManaPart::Payable(m) = &mut mana else {
            continue;
        };
        match e.modification.kind {
            RuleKind::CostMinimum => {
                let Some(n) = e.modification.params.get("minimum").and_then(Value::as_u64) else {
                    continue;
                };
                let t = oracle_total_of(m);
                if t < n {
                    m.generic += n - t;
                }
            }
            RuleKind::CostSet => {
                let Some(cost) = e.modification.cost.as_ref() else {
                    continue;
                };
                let Some((set_to, pays)) = plain_sum(cost) else {
                    continue;
                };
                if pays == 0 {
                    continue;
                }
                *m = set_to;
            }
            _ => {}
        }
    }
    if errs.is_empty() {
        Ok(Expected {
            mana,
            phyrexian_life: life,
            other,
        })
    } else {
        Err(errs)
    }
}

/// Asserts that the product agrees with the oracle (exact on success; on failure, the product's
/// error must be one the oracle justifies).
pub fn assert_agrees(
    b: &CostBasis,
    additional: &[CostBasis],
    st: &CostStages,
    order: &[ReductionStep],
    ctx: &str,
) -> Result<LockedCost, CostError> {
    let got = determine_total_cost(b, additional, st, order);
    let want = oracle_total(b, additional, st, order);
    match (&got, &want) {
        (Ok(l), Ok(w)) => assert!(w.matches(l), "{ctx}: product {l:?} != oracle {w:?}"),
        (Err(e), Err(ws)) => assert!(ws.contains(e), "{ctx}: product error {e:?} not in {ws:?}"),
        _ => panic!("{ctx}: product {got:?} vs oracle {want:?}\nstages {st:?}\norder {order:?}"),
    }
    got
}

/// The canonical text the brief pins for `to_mana_cost` (S: canonical symbols).
pub fn oracle_text(m: &ManaComponent) -> Option<String> {
    if m.generic > MAX_GENERIC {
        return None;
    }
    let rest = oracle_total_of(m) - m.generic;
    let mut s = String::new();
    if m.generic > 0 || rest == 0 {
        s.push_str(&format!("{{{}}}", m.generic));
    }
    for (n, letter) in [
        (m.white, "W"),
        (m.blue, "U"),
        (m.black, "B"),
        (m.red, "R"),
        (m.green, "G"),
        (m.colorless, "C"),
        (m.snow, "S"),
    ] {
        for _ in 0..n {
            s.push('{');
            s.push_str(letter);
            s.push('}');
        }
    }
    Some(s)
}

// ---------------------------------------------------------------------------------------------
// seeded random generation (R3: Pcg64Dxsm with fixed seeds only)
// ---------------------------------------------------------------------------------------------

pub struct Gen(pub Pcg64Dxsm);

impl Gen {
    pub fn new(seed: u64) -> Gen {
        Gen(Pcg64Dxsm::from_u64_seed(seed))
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.0.next_bounded(n).unwrap_or(0)
    }
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }
    pub fn color(&mut self) -> Color {
        Color::ALL[self.below(5) as usize]
    }
    pub fn two_colors(&mut self) -> (Color, Color) {
        let a = self.below(5) as usize;
        let b = (a + 1 + self.below(4) as usize) % 5;
        (Color::ALL[a], Color::ALL[b])
    }
    /// A plain symbol: generic, colored, colorless or snow.
    pub fn plain_symbol(&mut self) -> ManaSymbol {
        match self.below(4) {
            0 => ManaSymbol::Generic(self.below(5) as u32),
            1 => ManaSymbol::Colored(self.color()),
            2 => ManaSymbol::Colorless,
            _ => ManaSymbol::Snow,
        }
    }
    /// Any symbol except variables.
    pub fn any_fixed_symbol(&mut self) -> ManaSymbol {
        match self.below(9) {
            0..=3 => self.plain_symbol(),
            4 => {
                let (a, b) = self.two_colors();
                ManaSymbol::Hybrid(a, b)
            }
            5 => ManaSymbol::GenericHybrid(1 + self.below(3) as u32, self.color()),
            6 => ManaSymbol::ColorlessHybrid(self.color()),
            7 => ManaSymbol::Phyrexian(self.color()),
            _ => {
                let (a, b) = self.two_colors();
                ManaSymbol::HybridPhyrexian(a, b)
            }
        }
    }
    pub fn cost_of(&mut self, len: u64, f: impl Fn(&mut Gen) -> ManaSymbol) -> ManaCostDSL {
        let n = 1 + self.below(len);
        let symbols: Vec<ManaSymbol> = (0..n).map(|_| f(self)).collect();
        ManaCostDSL::new(symbols).expect("valid generated symbols")
    }
    pub fn other_item(&mut self) -> CostAST {
        match self.below(4) {
            0 => CostAST::TapSource,
            1 => CostAST::UntapSource,
            2 => CostAST::Forage,
            _ => energy(self.below(9) as i64),
        }
    }
    /// A valid choice for a choice symbol of `from_mana_cost`.
    pub fn valid_choice(&mut self, s: ManaSymbol) -> SymbolChoice {
        match s {
            ManaSymbol::Phyrexian(_) => {
                if self.chance(1, 2) {
                    SymbolChoice::First
                } else {
                    SymbolChoice::Life
                }
            }
            ManaSymbol::HybridPhyrexian(..) => match self.below(3) {
                0 => SymbolChoice::First,
                1 => SymbolChoice::Second,
                _ => SymbolChoice::Life,
            },
            _ => {
                if self.chance(1, 2) {
                    SymbolChoice::First
                } else {
                    SymbolChoice::Second
                }
            }
        }
    }
    /// A valid half for a reduction hybrid symbol.
    pub fn valid_half(&mut self) -> SymbolChoice {
        if self.chance(1, 2) {
            SymbolChoice::First
        } else {
            SymbolChoice::Second
        }
    }
    pub fn any_choice(&mut self) -> SymbolChoice {
        [
            SymbolChoice::First,
            SymbolChoice::Second,
            SymbolChoice::Life,
        ][self.below(3) as usize]
    }
    /// A payable basis built through the product's `from_mana_cost` with valid choices.
    pub fn payable_basis(&mut self) -> CostBasis {
        let cost = self.cost_of(5, Gen::any_fixed_symbol);
        let choices: Vec<SymbolChoice> = cost
            .symbols()
            .iter()
            .filter(|s| is_choice_symbol(**s))
            .map(|s| self.valid_choice(*s))
            .collect();
        let other: Vec<CostAST> = (0..self.below(3)).map(|_| self.other_item()).collect();
        CostBasis::from_mana_cost(Some(&cost), None, &choices, other).expect("valid basis")
    }
}
