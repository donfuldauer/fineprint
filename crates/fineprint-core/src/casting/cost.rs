// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Casting step 6: determine and lock in the total cost (REQ-CAST-001, REQ-LAYER-004;
//! CR 601.2f, 118.6, 118.6a, 118.7, 118.7a-g, 118.8, 118.8d, 118.9a, 118.9d).
//!
//! [`determine_total_cost`] is a pure function of its arguments: it reads the [`CostStages`] of
//! the casting player and returns a [`LockedCost`] that nothing can change afterwards.
//!
//! # Procedure (SRS 7.1 step 6)
//!
//! - **(a)** Start from the mana cost or the chosen alternative cost ([`CostBasis`], CR 601.2b,
//!   601.2f, 118.9a).
//! - **(b)** Add every additional cost (CR 118.8, 118.8d), then every `COST_INCREASE` entry in
//!   list order (CR 601.2f, 118.9d).
//! - **(c)** Apply every `COST_REDUCTION` entry in the order the player chose
//!   ([`ReductionStep`]), each one on the running mana component, symbol by symbol, component by
//!   component (CR 118.7a-g). No component goes below zero; an all-zero component is `{0}`
//!   (CR 601.2f, 118.7).
//! - **(d)** Apply the effects that directly affect the total cost (`COST_MINIMUM`, `COST_SET`)
//!   in the order of [`CostStages::total_effects`] (CR 601.2f).
//! - **(e)** Lock in: the result is a [`LockedCost`] with no mutation API (CR 601.2f).
//!
//! An unpayable mana cost (no mana cost, CR 118.6) stays unpayable through additional costs,
//! increases, reductions and total-cost effects (CR 118.6a); the non-mana parts and the
//! Phyrexian life payment still accumulate. An alternative cost such as "without paying its
//! mana cost" is a [`CostBasis::free`] basis, which is payable (CR 118.6a, 118.9d).
//!
//! # Rule-effect parameter conventions
//!
//! Entries are read from their typed `modification.cost` member; the opaque `params` are used
//! only for `"minimum"`. Every entry is untrusted (R12): an entry whose `modification.kind` does
//! not match its list, or that is malformed as described below, is ignored (no effect, no
//! error). An ignored reduction still occupies its slot in the player's order, and its
//! `hybrid_halves` are not checked.
//!
//! - `COST_INCREASE`: `cost` must be present. Every `PAY_MANA` adds its symbols, which must be
//!   generic, colored, colorless or snow (any other symbol: the whole entry is ignored); every
//!   other cost item is appended to the non-mana part.
//! - `COST_REDUCTION`: every `PAY_MANA` in `cost` reduces; other cost items are ignored. Allowed
//!   symbols: generic, colored, colorless, snow, hybrid, monocolored hybrid, colorless hybrid,
//!   Phyrexian and hybrid Phyrexian; a variable symbol makes the whole entry ignored. Each
//!   hybrid-type symbol (hybrid, monocolored hybrid, colorless hybrid, hybrid Phyrexian) takes
//!   the next element of the step's `hybrid_halves` (CR 118.7e).
//! - `COST_MINIMUM`: `params["minimum"]` must be a non-negative JSON integer `N`. If the mana
//!   component is payable and its total (every mana amount, the Phyrexian life excluded) is below
//!   `N`, generic mana is added to reach `N`.
//! - `COST_SET`: the sum of every `PAY_MANA` in `cost` (generic, colored, colorless or snow
//!   symbols only; at least one `PAY_MANA`) becomes the mana component; the non-mana part and
//!   the Phyrexian life are unchanged.
//!
//! # Determinism
//!
//! Integer arithmetic only (saturating `u64`), only `Vec` and `BTreeMap`, no state, no I/O (R3).

use std::collections::BTreeMap;

use fineprint_base::ids::EffectId;
use fineprint_dsl::ability::{CostAST, RuleKind};
use fineprint_dsl::mana::{Color, MAX_GENERIC, ManaCostDSL, ManaSymbol, Variable};

use crate::layers::{CostStages, RuleEntry};

/// The life paid for one Phyrexian mana symbol paid with life (CR 601.2b).
const PHYREXIAN_LIFE: u64 = 2;

/// The `params` member naming a `COST_MINIMUM` amount.
const MINIMUM: &str = "minimum";

/// The mana component of a cost, component by component (CR 118.7). The default value is `{0}`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ManaComponent {
    /// Generic mana.
    pub generic: u64,
    /// White mana.
    pub white: u64,
    /// Blue mana.
    pub blue: u64,
    /// Black mana.
    pub black: u64,
    /// Red mana.
    pub red: u64,
    /// Green mana.
    pub green: u64,
    /// Colorless mana `{C}`.
    pub colorless: u64,
    /// Snow mana `{S}`.
    pub snow: u64,
}

impl ManaComponent {
    /// The amount of mana of color `c`.
    #[must_use]
    pub fn colored(&self, c: Color) -> u64 {
        match c {
            Color::White => self.white,
            Color::Blue => self.blue,
            Color::Black => self.black,
            Color::Red => self.red,
            Color::Green => self.green,
        }
    }

    /// The amount of mana of color `c`, mutably.
    fn colored_mut(&mut self, c: Color) -> &mut u64 {
        match c {
            Color::White => &mut self.white,
            Color::Blue => &mut self.blue,
            Color::Black => &mut self.black,
            Color::Red => &mut self.red,
            Color::Green => &mut self.green,
        }
    }

    /// Every field, in canonical order (generic, W, U, B, R, G, colorless, snow).
    fn fields(&self) -> [u64; 8] {
        [
            self.generic,
            self.white,
            self.blue,
            self.black,
            self.red,
            self.green,
            self.colorless,
            self.snow,
        ]
    }

    /// The saturating sum of all eight fields.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.fields()
            .iter()
            .fold(0u64, |acc, n| acc.saturating_add(*n))
    }

    /// Whether every field is zero, that is, the component is `{0}` (CR 601.2f, 118.7).
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.fields().iter().all(|n| *n == 0)
    }

    /// The canonical mana cost of this component: `{generic}` (omitted when it is 0 and
    /// something else is present), then `W U B R G`, then `C`, then `S`, each colored, colorless
    /// and snow amount written as repeated symbols. An all-zero component gives `{0}`.
    ///
    /// # Errors
    /// [`CostError::Overflow`] if the generic amount is above [`MAX_GENERIC`], or if the colored,
    /// colorless and snow amounts together are above [`MAX_GENERIC`] symbols.
    pub fn to_mana_cost(&self) -> Result<ManaCostDSL, CostError> {
        let generic = u32::try_from(self.generic)
            .ok()
            .filter(|g| *g <= MAX_GENERIC)
            .ok_or(CostError::Overflow)?;
        let repeated = self.total().saturating_sub(self.generic);
        if repeated > u64::from(MAX_GENERIC) {
            return Err(CostError::Overflow);
        }
        let mut symbols = Vec::new();
        if generic > 0 || repeated == 0 {
            symbols.push(ManaSymbol::Generic(generic));
        }
        for c in Color::ALL {
            push_repeated(&mut symbols, ManaSymbol::Colored(c), self.colored(c));
        }
        push_repeated(&mut symbols, ManaSymbol::Colorless, self.colorless);
        push_repeated(&mut symbols, ManaSymbol::Snow, self.snow);
        ManaCostDSL::new(symbols).map_err(|_| CostError::Overflow)
    }

    /// Adds `other` component-wise, saturating.
    fn add(&mut self, other: &ManaComponent) {
        self.generic = self.generic.saturating_add(other.generic);
        self.white = self.white.saturating_add(other.white);
        self.blue = self.blue.saturating_add(other.blue);
        self.black = self.black.saturating_add(other.black);
        self.red = self.red.saturating_add(other.red);
        self.green = self.green.saturating_add(other.green);
        self.colorless = self.colorless.saturating_add(other.colorless);
        self.snow = self.snow.saturating_add(other.snow);
    }

    /// Adds one amount, saturating.
    fn add_amount(&mut self, amount: Amount) {
        match amount {
            Amount::Generic(n) => self.generic = self.generic.saturating_add(n),
            Amount::Colored(c) => {
                let slot = self.colored_mut(c);
                *slot = slot.saturating_add(1);
            }
            Amount::Colorless => self.colorless = self.colorless.saturating_add(1),
            Amount::Snow => self.snow = self.snow.saturating_add(1),
        }
    }

    /// Reduces by one amount (CR 118.7a-d, 118.7g, 107.4h). No field goes below zero.
    fn reduce_amount(&mut self, amount: Amount) {
        match amount {
            // CR 118.7a: a generic reduction affects only the generic component.
            Amount::Generic(n) => self.generic = self.generic.saturating_sub(n),
            // CR 118.7b-c: that color first; if the cost has none of it left, generic.
            Amount::Colored(c) => {
                let slot = self.colored_mut(c);
                if *slot > 0 {
                    *slot -= 1;
                } else {
                    self.generic = self.generic.saturating_sub(1);
                }
            }
            // CR 118.7b, 118.7d: colorless first, then generic.
            Amount::Colorless => {
                if self.colorless > 0 {
                    self.colorless -= 1;
                } else {
                    self.generic = self.generic.saturating_sub(1);
                }
            }
            // CR 118.7g: a snow reduction reduces generic; CR 107.4h: {S} is never reduced.
            Amount::Snow => self.generic = self.generic.saturating_sub(1),
        }
    }
}

/// Appends `count` copies of `symbol` (the caller bounds `count`).
fn push_repeated(symbols: &mut Vec<ManaSymbol>, symbol: ManaSymbol, count: u64) {
    let mut left = count;
    while left > 0 {
        symbols.push(symbol);
        left -= 1;
    }
}

/// One fixed amount of mana: what a non-hybrid symbol, or a chosen half, stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Amount {
    /// `n` generic mana.
    Generic(u64),
    /// One mana of a color.
    Colored(Color),
    /// One colorless mana.
    Colorless,
    /// One snow mana.
    Snow,
}

/// The amount a generic, colored, colorless or snow symbol stands for; `None` for any other
/// symbol.
fn fixed_amount(symbol: ManaSymbol) -> Option<Amount> {
    match symbol {
        ManaSymbol::Generic(n) => Some(Amount::Generic(u64::from(n))),
        ManaSymbol::Colored(c) => Some(Amount::Colored(c)),
        ManaSymbol::Colorless => Some(Amount::Colorless),
        ManaSymbol::Snow => Some(Amount::Snow),
        ManaSymbol::Variable(_)
        | ManaSymbol::Hybrid(..)
        | ManaSymbol::GenericHybrid(..)
        | ManaSymbol::Phyrexian(_)
        | ManaSymbol::HybridPhyrexian(..)
        | ManaSymbol::ColorlessHybrid(_) => None,
    }
}

/// The two halves of a hybrid-type symbol, in written order (CR 107.4e, 118.7e).
fn hybrid_halves(symbol: ManaSymbol) -> Option<(Amount, Amount)> {
    match symbol {
        ManaSymbol::Hybrid(a, b) | ManaSymbol::HybridPhyrexian(a, b) => {
            Some((Amount::Colored(a), Amount::Colored(b)))
        }
        ManaSymbol::GenericHybrid(n, c) => {
            Some((Amount::Generic(u64::from(n)), Amount::Colored(c)))
        }
        ManaSymbol::ColorlessHybrid(c) => Some((Amount::Colorless, Amount::Colored(c))),
        ManaSymbol::Generic(_)
        | ManaSymbol::Colored(_)
        | ManaSymbol::Colorless
        | ManaSymbol::Snow
        | ManaSymbol::Variable(_)
        | ManaSymbol::Phyrexian(_) => None,
    }
}

/// How one hybrid or Phyrexian symbol of a cost is paid (CR 601.2b) or, for a reduction, which
/// half of a hybrid symbol is applied (CR 118.7e).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolChoice {
    /// The first half as written (`{a/b}`: `a`; `{n/c}`: `n` generic; `{C/c}`: colorless;
    /// `{c/P}`: mana of color `c`).
    First,
    /// The second half as written (`{a/b}`: `b`; `{n/c}`: `c`; `{C/c}`: `c`). Not valid for
    /// `{c/P}`.
    Second,
    /// Pay 2 life instead of the mana of a Phyrexian symbol (CR 601.2b). Not valid for a
    /// non-Phyrexian symbol, nor in a reduction.
    Life,
}

/// The mana part of a cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManaPart {
    /// No mana cost: an unpayable cost (CR 118.6, 118.6a).
    Unpayable,
    /// A payable mana component (possibly `{0}`).
    Payable(ManaComponent),
}

/// A base, alternative or additional cost with its choices already made (CR 601.2b).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostBasis {
    /// The mana part.
    pub mana: ManaPart,
    /// Life paid for Phyrexian symbols paid with life (2 per symbol, CR 601.2b).
    pub phyrexian_life: u64,
    /// The non-mana cost items, in order.
    pub other: Vec<CostAST>,
}

impl CostBasis {
    /// Resolves a mana cost into a basis with the choices announced in CR 601.2b.
    ///
    /// `cost == None` gives an unpayable mana part (no mana cost, CR 118.6). `x` is the value
    /// announced for `{X}`: each `{X}` stands for `x` generic mana (CR 107.3a). `choices` holds
    /// one [`SymbolChoice`] per hybrid or Phyrexian symbol of `cost` (hybrid, monocolored hybrid,
    /// colorless hybrid, Phyrexian, hybrid Phyrexian), in written order (CR 601.2b, 107.4e-f).
    /// Amounts saturate at `u64::MAX`.
    ///
    /// # Errors
    /// [`CostError::MissingX`] if the cost has `{X}` and `x` is `None`;
    /// [`CostError::UnsupportedVariable`] for `{Y}` or `{Z}`; [`CostError::BadChoice`] if
    /// `choices` has too few or too many elements, or a choice is not valid for its symbol.
    pub fn from_mana_cost(
        cost: Option<&ManaCostDSL>,
        x: Option<u64>,
        choices: &[SymbolChoice],
        other: Vec<CostAST>,
    ) -> Result<CostBasis, CostError> {
        let Some(cost) = cost else {
            // CR 118.6: no mana cost is an unpayable cost; there is no symbol to choose for.
            if !choices.is_empty() {
                return Err(CostError::BadChoice);
            }
            return Ok(CostBasis {
                mana: ManaPart::Unpayable,
                phyrexian_life: 0,
                other,
            });
        };
        let mut mana = ManaComponent::default();
        let mut life: u64 = 0;
        let mut next = choices.iter();
        for symbol in cost.symbols() {
            match *symbol {
                // CR 107.3a: each {X} is the announced value.
                ManaSymbol::Variable(Variable::X) => {
                    let value = x.ok_or(CostError::MissingX)?;
                    mana.add_amount(Amount::Generic(value));
                }
                ManaSymbol::Variable(Variable::Y | Variable::Z) => {
                    return Err(CostError::UnsupportedVariable);
                }
                // CR 601.2b, 107.4f: a Phyrexian symbol is paid with its color or 2 life.
                ManaSymbol::Phyrexian(c) => match next.next() {
                    Some(SymbolChoice::First) => mana.add_amount(Amount::Colored(c)),
                    Some(SymbolChoice::Life) => life = life.saturating_add(PHYREXIAN_LIFE),
                    Some(SymbolChoice::Second) | None => return Err(CostError::BadChoice),
                },
                // CR 601.2b, 107.4e: the nonhybrid equivalent of a hybrid symbol; a hybrid
                // Phyrexian symbol may also be paid with 2 life.
                other_symbol => {
                    if let Some(amount) = fixed_amount(other_symbol) {
                        mana.add_amount(amount);
                    } else if let Some((first, second)) = hybrid_halves(other_symbol) {
                        let phyrexian = matches!(other_symbol, ManaSymbol::HybridPhyrexian(..));
                        match next.next() {
                            Some(SymbolChoice::First) => mana.add_amount(first),
                            Some(SymbolChoice::Second) => mana.add_amount(second),
                            Some(SymbolChoice::Life) if phyrexian => {
                                life = life.saturating_add(PHYREXIAN_LIFE);
                            }
                            Some(SymbolChoice::Life) | None => return Err(CostError::BadChoice),
                        }
                    }
                }
            }
        }
        if next.next().is_some() {
            return Err(CostError::BadChoice);
        }
        Ok(CostBasis {
            mana: ManaPart::Payable(mana),
            phyrexian_life: life,
            other,
        })
    }

    /// A basis whose mana part is `{0}` and payable: an alternative cost such as "without paying
    /// its mana cost" (CR 118.6a, 118.9, 118.9d).
    #[must_use]
    pub fn free(other: Vec<CostAST>) -> CostBasis {
        CostBasis {
            mana: ManaPart::Payable(ManaComponent::default()),
            phyrexian_life: 0,
            other,
        }
    }
}

/// One cost reduction applied at its position in the player's chosen order, with the half chosen
/// for each of its hybrid-type symbols in written order (CR 601.2f, 118.7e).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReductionStep {
    /// The id of a `COST_REDUCTION` entry of [`CostStages::reductions`].
    pub id: EffectId,
    /// One [`SymbolChoice::First`] or [`SymbolChoice::Second`] per hybrid-type symbol of the
    /// entry (hybrid, monocolored hybrid, colorless hybrid, hybrid Phyrexian), in written order.
    pub hybrid_halves: Vec<SymbolChoice>,
}

/// The locked-in total cost (CR 601.2f). It is immutable: its fields are private and no method
/// takes cost stages or effects, so later changes to cost effects cannot alter it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedCost {
    /// The mana part.
    mana: ManaPart,
    /// Life paid for Phyrexian symbols.
    phyrexian_life: u64,
    /// The non-mana cost items, in order.
    other: Vec<CostAST>,
}

impl LockedCost {
    /// The mana part of the total cost.
    #[must_use]
    pub fn mana(&self) -> &ManaPart {
        &self.mana
    }

    /// The life paid for Phyrexian symbols paid with life.
    #[must_use]
    pub fn phyrexian_life(&self) -> u64 {
        self.phyrexian_life
    }

    /// The non-mana cost items, in order (base, then additional costs, then increases).
    #[must_use]
    pub fn other(&self) -> &[CostAST] {
        &self.other
    }
}

/// Why a total cost could not be determined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CostError {
    /// The reduction order is not a permutation of the ids of [`CostStages::reductions`]
    /// (missing, extra, repeated or unknown id).
    BadReductionOrder,
    /// A hybrid or Phyrexian choice is missing, extra, or not valid for its symbol.
    BadChoice,
    /// The cost has `{X}` and no value was announced (CR 601.2b, 107.3a).
    MissingX,
    /// The cost has `{Y}` or `{Z}`, which this calculator does not resolve.
    UnsupportedVariable,
    /// An additional cost has an unpayable mana part.
    UnpayableAdditional,
    /// An amount does not fit a mana cost ([`ManaComponent::to_mana_cost`]).
    Overflow,
}

/// SRS 7.1 step 6 (a)-(e): determines and locks in the total cost of a spell (REQ-CAST-001;
/// CR 601.2f).
///
/// `basis` is the mana cost or the chosen alternative cost (CR 601.2b, 118.9a); `additional`
/// are the additional costs (CR 118.8); `stages` are the casting player's cost effects
/// (REQ-LAYER-004); `reductions` is the order the player applies the cost reductions in, one
/// step per entry of `stages.reductions` (CR 601.2f).
///
/// # Errors
/// [`CostError::UnpayableAdditional`] if an additional cost has an unpayable mana part;
/// [`CostError::BadReductionOrder`] if `reductions` is not a permutation of the reduction ids;
/// [`CostError::BadChoice`] if a step's `hybrid_halves` do not match its entry's hybrid-type
/// symbols.
pub fn determine_total_cost(
    basis: &CostBasis,
    additional: &[CostBasis],
    stages: &CostStages,
    reductions: &[ReductionStep],
) -> Result<LockedCost, CostError> {
    // CR 601.2f (a): the mana cost or the alternative cost (CR 118.9a, 118.9d).
    let mut mana = basis.mana;
    let mut life = basis.phyrexian_life;
    let mut other = basis.other.clone();

    // CR 601.2f (b), 118.8, 118.8d: additional costs. CR 118.6a: an unpayable cost stays
    // unpayable when an additional cost is imposed.
    for extra in additional {
        let ManaPart::Payable(add) = extra.mana else {
            return Err(CostError::UnpayableAdditional);
        };
        if let ManaPart::Payable(total) = &mut mana {
            total.add(&add);
        }
        life = life.saturating_add(extra.phyrexian_life);
        other.extend(extra.other.iter().cloned());
    }

    // CR 601.2f (b): cost increases, in list order. CR 118.6a: still unpayable.
    for entry in &stages.increases {
        if let Some((add, items)) = increase(entry) {
            if let ManaPart::Payable(total) = &mut mana {
                total.add(&add);
            }
            other.extend(items);
        }
    }

    // CR 601.2f (c): reductions in the player's order, component-wise (CR 118.7a-g).
    let ordered = order_reductions(&stages.reductions, reductions)?;
    for (entry, step) in ordered {
        let Some(symbols) = reduction_symbols(entry) else {
            continue;
        };
        let amounts = choose_halves(&symbols, &step.hybrid_halves)?;
        if let ManaPart::Payable(total) = &mut mana {
            for amount in amounts {
                total.reduce_amount(amount);
            }
        }
    }

    // CR 601.2f (d): effects that directly affect the total cost, in list order.
    for entry in &stages.total_effects {
        if let ManaPart::Payable(total) = &mut mana {
            apply_total_effect(entry, total);
        }
    }

    // CR 601.2f (e): locked in.
    Ok(LockedCost {
        mana,
        phyrexian_life: life,
        other,
    })
}

/// The mana and non-mana items a `COST_INCREASE` entry adds, or `None` if the entry is ignored
/// (CR 601.2f).
fn increase(entry: &RuleEntry) -> Option<(ManaComponent, Vec<CostAST>)> {
    if entry.modification.kind != RuleKind::CostIncrease {
        return None;
    }
    let items = entry.modification.cost.as_ref()?;
    let mut mana = ManaComponent::default();
    let mut other = Vec::new();
    for item in items {
        if let CostAST::PayMana { mana: cost } = item {
            for symbol in cost.symbols() {
                mana.add_amount(fixed_amount(*symbol)?);
            }
        } else {
            other.push(item.clone());
        }
    }
    Some((mana, other))
}

/// Pairs every step with its reduction entry, in the player's order, after checking that the
/// order is a permutation of the entries' ids (CR 601.2f).
fn order_reductions<'a>(
    entries: &'a [RuleEntry],
    order: &'a [ReductionStep],
) -> Result<Vec<(&'a RuleEntry, &'a ReductionStep)>, CostError> {
    let mut pending: BTreeMap<EffectId, &'a RuleEntry> = BTreeMap::new();
    for entry in entries {
        if pending.insert(entry.id, entry).is_some() {
            return Err(CostError::BadReductionOrder);
        }
    }
    let mut paired = Vec::with_capacity(order.len());
    for step in order {
        let entry = pending
            .remove(&step.id)
            .ok_or(CostError::BadReductionOrder)?;
        paired.push((entry, step));
    }
    if pending.is_empty() {
        Ok(paired)
    } else {
        Err(CostError::BadReductionOrder)
    }
}

/// One symbol of a reduction: a fixed amount, or a hybrid-type symbol whose half the player
/// chooses (CR 118.7e).
#[derive(Clone, Copy, Debug)]
enum ReductionSymbol {
    /// A fixed amount.
    Fixed(Amount),
    /// The two halves of a hybrid-type symbol, in written order.
    Hybrid(Amount, Amount),
}

/// The symbols of a `COST_REDUCTION` entry in written order, or `None` if the entry is ignored.
fn reduction_symbols(entry: &RuleEntry) -> Option<Vec<ReductionSymbol>> {
    if entry.modification.kind != RuleKind::CostReduction {
        return None;
    }
    let items = entry.modification.cost.as_ref()?;
    let mut symbols = Vec::new();
    for item in items {
        let CostAST::PayMana { mana } = item else {
            continue;
        };
        for symbol in mana.symbols() {
            let resolved = match *symbol {
                // CR 118.7f: a Phyrexian symbol reduces by one mana of its color.
                ManaSymbol::Phyrexian(c) => ReductionSymbol::Fixed(Amount::Colored(c)),
                ManaSymbol::Variable(_) => return None,
                other_symbol => {
                    if let Some(amount) = fixed_amount(other_symbol) {
                        ReductionSymbol::Fixed(amount)
                    } else {
                        let (first, second) = hybrid_halves(other_symbol)?;
                        ReductionSymbol::Hybrid(first, second)
                    }
                }
            };
            symbols.push(resolved);
        }
    }
    Some(symbols)
}

/// Resolves the hybrid-type symbols of a reduction with the player's halves (CR 118.7e).
fn choose_halves(
    symbols: &[ReductionSymbol],
    halves: &[SymbolChoice],
) -> Result<Vec<Amount>, CostError> {
    let mut next = halves.iter();
    let mut amounts = Vec::with_capacity(symbols.len());
    for symbol in symbols {
        let amount = match *symbol {
            ReductionSymbol::Fixed(amount) => amount,
            ReductionSymbol::Hybrid(first, second) => match next.next() {
                Some(SymbolChoice::First) => first,
                Some(SymbolChoice::Second) => second,
                Some(SymbolChoice::Life) | None => return Err(CostError::BadChoice),
            },
        };
        amounts.push(amount);
    }
    if next.next().is_some() {
        return Err(CostError::BadChoice);
    }
    Ok(amounts)
}

/// Applies one `COST_MINIMUM` or `COST_SET` entry to a payable mana component (CR 601.2f);
/// malformed entries are ignored.
fn apply_total_effect(entry: &RuleEntry, total: &mut ManaComponent) {
    match entry.modification.kind {
        RuleKind::CostMinimum => {
            let Some(minimum) = entry
                .modification
                .params
                .get(MINIMUM)
                .and_then(serde_json::Value::as_u64)
            else {
                return;
            };
            let current = total.total();
            if current < minimum {
                total.add_amount(Amount::Generic(minimum - current));
            }
        }
        RuleKind::CostSet => {
            if let Some(set) = set_cost(entry) {
                *total = set;
            }
        }
        _ => {}
    }
}

/// The mana component a `COST_SET` entry sets, or `None` if the entry is malformed.
fn set_cost(entry: &RuleEntry) -> Option<ManaComponent> {
    let items = entry.modification.cost.as_ref()?;
    let mut mana = ManaComponent::default();
    let mut any = false;
    for item in items {
        if let CostAST::PayMana { mana: cost } = item {
            any = true;
            for symbol in cost.symbols() {
                mana.add_amount(fixed_amount(*symbol)?);
            }
        }
    }
    any.then_some(mana)
}

#[cfg(test)]
mod tests {
    //! Unit tests of casting step 6 (REQ-CAST-001, REQ-LAYER-004; CR 601.2f, 118.6a, 118.7a-g).
    //! Every cost effect here is synthetic (R5).

    use super::order_reductions;
    use std::collections::{BTreeMap, BTreeSet};

    use fineprint_base::ids::{EffectId, PlayerId, Timestamp};
    use fineprint_dsl::ability::{CostAST, RuleKind, RuleModification};
    use fineprint_dsl::mana::{Color, MAX_GENERIC, ManaCostDSL};
    use serde_json::{Value, json};

    use super::{
        CostBasis, CostError, LockedCost, ManaComponent, ManaPart, ReductionStep, SymbolChoice,
        determine_total_cost,
    };
    use crate::layers::{CostStages, RuleEntry};

    use SymbolChoice::{First, Life, Second};

    fn mana(text: &str) -> ManaCostDSL {
        ManaCostDSL::parse(text).expect("synthetic mana cost")
    }

    fn pay(text: &str) -> CostAST {
        CostAST::PayMana { mana: mana(text) }
    }

    fn entry_with(
        id: u64,
        kind: RuleKind,
        cost: Option<Vec<CostAST>>,
        params: BTreeMap<String, Value>,
    ) -> RuleEntry {
        RuleEntry {
            id: EffectId::new(id),
            players: BTreeSet::from([PlayerId::new(1)]),
            controller: PlayerId::new(1),
            source: None,
            timestamp: Timestamp::new(id),
            modification: RuleModification { kind, cost, params },
        }
    }

    fn entry(id: u64, kind: RuleKind, cost: Vec<CostAST>) -> RuleEntry {
        entry_with(id, kind, Some(cost), BTreeMap::new())
    }

    fn minimum(id: u64, n: Value) -> RuleEntry {
        entry_with(
            id,
            RuleKind::CostMinimum,
            None,
            BTreeMap::from([("minimum".to_owned(), n)]),
        )
    }

    fn basis(text: &str) -> CostBasis {
        CostBasis::from_mana_cost(Some(&mana(text)), None, &[], Vec::new()).expect("basis")
    }

    fn step(id: u64) -> ReductionStep {
        ReductionStep {
            id: EffectId::new(id),
            hybrid_halves: Vec::new(),
        }
    }

    fn steps(ids: &[u64]) -> Vec<ReductionStep> {
        ids.iter().map(|id| step(*id)).collect()
    }

    fn payable(cost: &LockedCost) -> ManaComponent {
        match cost.mana() {
            ManaPart::Payable(m) => *m,
            ManaPart::Unpayable => panic!("expected a payable cost"),
        }
    }

    fn text(cost: &LockedCost) -> String {
        payable(cost).to_mana_cost().expect("fits").to_string()
    }

    fn reductions_stage(entries: Vec<RuleEntry>) -> CostStages {
        CostStages {
            reductions: entries,
            ..CostStages::default()
        }
    }

    const PERMUTATIONS: [[u64; 3]; 6] = [
        [1, 2, 3],
        [1, 3, 2],
        [2, 1, 3],
        [2, 3, 1],
        [3, 1, 2],
        [3, 2, 1],
    ];

    // ---- Requirement 1: from_mana_cost (CR 601.2b, 107.3a, 107.4) ----

    #[test]
    fn req_cast_001_from_mana_cost_fixed_symbols_cr_601_2b() {
        let b = basis("{3}{W}{U}{B}{R}{G}{C}{S}");
        let ManaPart::Payable(m) = b.mana else {
            panic!("payable")
        };
        assert_eq!(
            m,
            ManaComponent {
                generic: 3,
                white: 1,
                blue: 1,
                black: 1,
                red: 1,
                green: 1,
                colorless: 1,
                snow: 1,
            }
        );
        assert_eq!(b.phyrexian_life, 0);
        assert_eq!(m.total(), 10);
    }

    #[test]
    fn req_cast_001_from_mana_cost_x_cr_107_3a() {
        let b = CostBasis::from_mana_cost(Some(&mana("{X}{X}{R}")), Some(4), &[], Vec::new())
            .expect("x");
        assert_eq!(
            b.mana,
            ManaPart::Payable(ManaComponent {
                generic: 8,
                red: 1,
                ..Default::default()
            })
        );
        assert_eq!(
            CostBasis::from_mana_cost(Some(&mana("{X}{R}")), None, &[], Vec::new()),
            Err(CostError::MissingX)
        );
        assert_eq!(
            CostBasis::from_mana_cost(Some(&mana("{Y}")), Some(1), &[], Vec::new()),
            Err(CostError::UnsupportedVariable)
        );
        assert_eq!(
            CostBasis::from_mana_cost(Some(&mana("{Z}")), Some(1), &[], Vec::new()),
            Err(CostError::UnsupportedVariable)
        );
        // No {X}: x is not needed.
        assert!(CostBasis::from_mana_cost(Some(&mana("{1}")), None, &[], Vec::new()).is_ok());
    }

    #[test]
    fn req_cast_001_from_mana_cost_x_saturates_and_overflows_cr_107_3a() {
        let b = CostBasis::from_mana_cost(Some(&mana("{X}{X}")), Some(u64::MAX), &[], Vec::new())
            .expect("x");
        let ManaPart::Payable(m) = b.mana else {
            panic!("payable")
        };
        assert_eq!(m.generic, u64::MAX);
        assert_eq!(m.to_mana_cost(), Err(CostError::Overflow));
    }

    #[test]
    fn req_cast_001_from_mana_cost_hybrid_choices_cr_601_2b_107_4e() {
        let cost = mana("{W/U}{2/B}{C/G}");
        let b = CostBasis::from_mana_cost(Some(&cost), None, &[First, First, First], Vec::new())
            .expect("first");
        assert_eq!(
            b.mana,
            ManaPart::Payable(ManaComponent {
                white: 1,
                generic: 2,
                colorless: 1,
                ..Default::default()
            })
        );
        let b = CostBasis::from_mana_cost(Some(&cost), None, &[Second, Second, Second], Vec::new())
            .expect("second");
        assert_eq!(
            b.mana,
            ManaPart::Payable(ManaComponent {
                blue: 1,
                black: 1,
                green: 1,
                ..Default::default()
            })
        );
        // Life is valid only for a Phyrexian symbol (CR 601.2b).
        for text in ["{W/U}", "{2/B}", "{C/G}"] {
            assert_eq!(
                CostBasis::from_mana_cost(Some(&mana(text)), None, &[Life], Vec::new()),
                Err(CostError::BadChoice),
                "{text}"
            );
        }
    }

    #[test]
    fn req_cast_001_from_mana_cost_phyrexian_choices_cr_601_2b_107_4f() {
        let b = CostBasis::from_mana_cost(
            Some(&mana("{1}{B/P}{B/P}")),
            None,
            &[First, Life],
            Vec::new(),
        )
        .expect("phyrexian");
        assert_eq!(
            b.mana,
            ManaPart::Payable(ManaComponent {
                generic: 1,
                black: 1,
                ..Default::default()
            })
        );
        assert_eq!(b.phyrexian_life, 2);
        assert_eq!(
            CostBasis::from_mana_cost(Some(&mana("{B/P}")), None, &[Second], Vec::new()),
            Err(CostError::BadChoice)
        );
        let cost = mana("{R/G/P}");
        let first = CostBasis::from_mana_cost(Some(&cost), None, &[First], Vec::new()).expect("a");
        let second =
            CostBasis::from_mana_cost(Some(&cost), None, &[Second], Vec::new()).expect("b");
        let life = CostBasis::from_mana_cost(Some(&cost), None, &[Life], Vec::new()).expect("life");
        assert_eq!(
            first.mana,
            ManaPart::Payable(ManaComponent {
                red: 1,
                ..Default::default()
            })
        );
        assert_eq!(
            second.mana,
            ManaPart::Payable(ManaComponent {
                green: 1,
                ..Default::default()
            })
        );
        assert_eq!(life.mana, ManaPart::Payable(ManaComponent::default()));
        assert_eq!(life.phyrexian_life, 2);
    }

    #[test]
    fn req_cast_001_from_mana_cost_choice_count_cr_601_2b() {
        let cost = mana("{W/U}{G/P}");
        assert_eq!(
            CostBasis::from_mana_cost(Some(&cost), None, &[First], Vec::new()),
            Err(CostError::BadChoice)
        );
        assert_eq!(
            CostBasis::from_mana_cost(Some(&cost), None, &[First, First, First], Vec::new()),
            Err(CostError::BadChoice)
        );
        assert_eq!(
            CostBasis::from_mana_cost(Some(&mana("{1}")), None, &[First], Vec::new()),
            Err(CostError::BadChoice)
        );
        assert_eq!(
            CostBasis::from_mana_cost(None, None, &[First], Vec::new()),
            Err(CostError::BadChoice)
        );
    }

    #[test]
    fn req_cast_001_no_mana_cost_is_unpayable_cr_118_6() {
        let b = CostBasis::from_mana_cost(None, Some(3), &[], vec![CostAST::TapSource])
            .expect("unpayable");
        assert_eq!(b.mana, ManaPart::Unpayable);
        assert_eq!(b.other, vec![CostAST::TapSource]);
        let free = CostBasis::free(vec![CostAST::Forage]);
        assert_eq!(free.mana, ManaPart::Payable(ManaComponent::default()));
        assert_eq!(free.other, vec![CostAST::Forage]);
    }

    #[test]
    fn req_cast_001_to_mana_cost_canonical_order() {
        assert_eq!(
            ManaComponent::default()
                .to_mana_cost()
                .expect("zero")
                .to_string(),
            "{0}"
        );
        assert!(ManaComponent::default().is_zero());
        let m = ManaComponent {
            generic: 2,
            white: 1,
            blue: 2,
            black: 1,
            red: 1,
            green: 1,
            colorless: 2,
            snow: 1,
        };
        assert_eq!(
            m.to_mana_cost().expect("fits").to_string(),
            "{2}{W}{U}{U}{B}{R}{G}{C}{C}{S}"
        );
        let no_generic = ManaComponent {
            green: 2,
            ..Default::default()
        };
        assert_eq!(
            no_generic.to_mana_cost().expect("fits").to_string(),
            "{G}{G}"
        );
        assert!(!no_generic.is_zero());
        assert_eq!(no_generic.colored(Color::Green), 2);
        assert_eq!(no_generic.colored(Color::White), 0);
    }

    #[test]
    fn req_cast_001_to_mana_cost_overflow() {
        let at_limit = ManaComponent {
            generic: u64::from(MAX_GENERIC),
            ..Default::default()
        };
        assert!(at_limit.to_mana_cost().is_ok());
        let over = ManaComponent {
            generic: u64::from(MAX_GENERIC) + 1,
            ..Default::default()
        };
        assert_eq!(over.to_mana_cost(), Err(CostError::Overflow));
        let many = ManaComponent {
            red: u64::MAX,
            ..Default::default()
        };
        assert_eq!(many.to_mana_cost(), Err(CostError::Overflow));
        let saturated = ManaComponent {
            red: u64::MAX,
            blue: u64::MAX,
            ..Default::default()
        };
        assert_eq!(saturated.total(), u64::MAX);
    }

    // ---- Requirement 2: steps (a)-(b) (CR 601.2f, 118.8, 118.6a) ----

    #[test]
    fn req_cast_001_additional_and_increases_cr_601_2f_118_8() {
        let base =
            CostBasis::from_mana_cost(Some(&mana("{1}{W}")), None, &[], vec![CostAST::TapSource])
                .expect("base");
        let kicker =
            CostBasis::from_mana_cost(Some(&mana("{G/P}")), None, &[Life], vec![CostAST::Forage])
                .expect("additional");
        let stages = CostStages {
            increases: vec![
                entry(
                    1,
                    RuleKind::CostIncrease,
                    vec![pay("{2}{S}"), CostAST::UntapSource],
                ),
                entry(2, RuleKind::CostIncrease, vec![pay("{C}")]),
            ],
            ..CostStages::default()
        };
        let locked = determine_total_cost(&base, &[kicker], &stages, &[]).expect("total");
        assert_eq!(text(&locked), "{3}{W}{C}{S}");
        assert_eq!(locked.phyrexian_life(), 2);
        assert_eq!(
            locked.other(),
            &[CostAST::TapSource, CostAST::Forage, CostAST::UntapSource]
        );
    }

    #[test]
    fn req_cast_001_unpayable_additional_rejected() {
        let unpayable = CostBasis::from_mana_cost(None, None, &[], Vec::new()).expect("none");
        assert_eq!(
            determine_total_cost(&basis("{1}"), &[unpayable], &CostStages::default(), &[]),
            Err(CostError::UnpayableAdditional)
        );
    }

    #[test]
    fn req_cast_001_unpayable_stays_unpayable_cr_118_6a() {
        let base =
            CostBasis::from_mana_cost(None, None, &[], vec![CostAST::TapSource]).expect("none");
        let extra = CostBasis::from_mana_cost(
            Some(&mana("{1}{B/P}")),
            None,
            &[Life],
            vec![CostAST::Forage],
        )
        .expect("extra");
        let stages = CostStages {
            increases: vec![entry(
                1,
                RuleKind::CostIncrease,
                vec![pay("{2}"), CostAST::UntapSource],
            )],
            reductions: vec![entry(2, RuleKind::CostReduction, vec![pay("{W/U}")])],
            total_effects: vec![
                minimum(3, json!(5)),
                entry(4, RuleKind::CostSet, vec![pay("{1}")]),
            ],
            ..CostStages::default()
        };
        let order = vec![ReductionStep {
            id: EffectId::new(2),
            hybrid_halves: vec![First],
        }];
        let locked = determine_total_cost(&base, &[extra], &stages, &order).expect("total");
        assert_eq!(locked.mana(), &ManaPart::Unpayable);
        assert_eq!(locked.phyrexian_life(), 2);
        assert_eq!(
            locked.other(),
            &[CostAST::TapSource, CostAST::Forage, CostAST::UntapSource]
        );
    }

    #[test]
    fn req_cast_001_free_alternative_cost_is_payable_cr_118_6a_118_9d() {
        // An alternative cost applied to an unpayable cost may be paid; increases apply to it.
        let stages = CostStages {
            increases: vec![entry(1, RuleKind::CostIncrease, vec![pay("{2}")])],
            reductions: vec![entry(2, RuleKind::CostReduction, vec![pay("{1}")])],
            ..CostStages::default()
        };
        let locked = determine_total_cost(&CostBasis::free(Vec::new()), &[], &stages, &steps(&[2]))
            .expect("free");
        assert_eq!(text(&locked), "{1}");
    }

    // ---- Requirement 3: parameter conventions; malformed entries ignored (R12) ----

    #[test]
    fn req_cast_001_increase_malformed_entries_ignored() {
        let stages = CostStages {
            increases: vec![
                entry_with(1, RuleKind::CostIncrease, None, BTreeMap::new()),
                entry(
                    2,
                    RuleKind::CostIncrease,
                    vec![pay("{W/U}"), CostAST::TapSource],
                ),
                entry(3, RuleKind::CostIncrease, vec![pay("{X}")]),
                entry(4, RuleKind::CostReduction, vec![pay("{5}")]),
                entry(5, RuleKind::CostIncrease, vec![pay("{1}")]),
            ],
            ..CostStages::default()
        };
        let locked = determine_total_cost(&basis("{R}"), &[], &stages, &[]).expect("total");
        assert_eq!(text(&locked), "{1}{R}");
        assert!(locked.other().is_empty());
    }

    #[test]
    fn req_cast_001_reduction_malformed_entries_ignored_but_hold_slot() {
        let stages = reductions_stage(vec![
            entry_with(1, RuleKind::CostReduction, None, BTreeMap::new()),
            entry(2, RuleKind::CostReduction, vec![pay("{X}")]),
            entry(3, RuleKind::CostIncrease, vec![pay("{1}")]),
            entry(
                4,
                RuleKind::CostReduction,
                vec![CostAST::TapSource, pay("{1}")],
            ),
        ]);
        // The ignored entries still need their slot in the order.
        assert_eq!(
            determine_total_cost(&basis("{3}"), &[], &stages, &steps(&[4])),
            Err(CostError::BadReductionOrder)
        );
        // Ignored entries' halves are not checked.
        let mut order = steps(&[3, 1, 4, 2]);
        order[0].hybrid_halves = vec![Life];
        let locked = determine_total_cost(&basis("{3}"), &[], &stages, &order).expect("total");
        assert_eq!(text(&locked), "{2}");
    }

    #[test]
    fn req_cast_001_total_effects_malformed_ignored() {
        let stages = CostStages {
            total_effects: vec![
                minimum(1, json!(-1)),
                minimum(2, json!("7")),
                minimum(3, json!(2.5)),
                entry_with(4, RuleKind::CostMinimum, None, BTreeMap::new()),
                entry(5, RuleKind::CostSet, vec![CostAST::TapSource]),
                entry(6, RuleKind::CostSet, vec![pay("{W/U}")]),
                entry_with(7, RuleKind::CostSet, None, BTreeMap::new()),
                entry(8, RuleKind::CostIncrease, vec![pay("{9}")]),
                entry_with(
                    9,
                    RuleKind::CostReduction,
                    None,
                    BTreeMap::from([("minimum".to_owned(), json!(9))]),
                ),
            ],
            ..CostStages::default()
        };
        let locked = determine_total_cost(&basis("{1}{G}"), &[], &stages, &[]).expect("total");
        assert_eq!(text(&locked), "{1}{G}");
    }

    #[test]
    fn req_cast_001_minimum_adds_generic_cr_601_2f() {
        let stages = CostStages {
            reductions: vec![entry(1, RuleKind::CostReduction, vec![pay("{5}")])],
            total_effects: vec![minimum(2, json!(2))],
            ..CostStages::default()
        };
        let locked =
            determine_total_cost(&basis("{2}{U}"), &[], &stages, &steps(&[1])).expect("total");
        assert_eq!(text(&locked), "{1}{U}");
        // Already at or above the minimum: unchanged.
        let locked = determine_total_cost(
            &basis("{2}{U}"),
            &[],
            &CostStages {
                total_effects: vec![minimum(2, json!(3))],
                ..CostStages::default()
            },
            &[],
        )
        .expect("total");
        assert_eq!(text(&locked), "{2}{U}");
    }

    #[test]
    fn req_cast_001_set_replaces_mana_only_cr_601_2f() {
        let base = CostBasis::from_mana_cost(
            Some(&mana("{4}{B/P}")),
            None,
            &[Life],
            vec![CostAST::TapSource],
        )
        .expect("base");
        let stages = CostStages {
            total_effects: vec![entry(
                1,
                RuleKind::CostSet,
                vec![pay("{1}"), CostAST::Forage, pay("{R}")],
            )],
            ..CostStages::default()
        };
        let locked = determine_total_cost(&base, &[], &stages, &[]).expect("total");
        assert_eq!(text(&locked), "{1}{R}");
        assert_eq!(locked.phyrexian_life(), 2);
        assert_eq!(locked.other(), &[CostAST::TapSource]);
    }

    // ---- Requirement 4: reductions (CR 601.2f, 118.7a-g, 107.4h) ----

    #[test]
    fn req_cast_001_generic_reduction_only_generic_cr_118_7a_107_4h() {
        let stages = reductions_stage(vec![entry(1, RuleKind::CostReduction, vec![pay("{9}")])]);
        let locked = determine_total_cost(&basis("{2}{W}{C}{S}"), &[], &stages, &steps(&[1]))
            .expect("total");
        assert_eq!(text(&locked), "{W}{C}{S}");
    }

    #[test]
    fn req_cast_001_color_the_cost_lacks_reduces_generic_cr_118_7b() {
        let stages = reductions_stage(vec![
            entry(1, RuleKind::CostReduction, vec![pay("{G}")]),
            entry(2, RuleKind::CostReduction, vec![pay("{C}")]),
        ]);
        let locked =
            determine_total_cost(&basis("{3}{W}"), &[], &stages, &steps(&[1, 2])).expect("total");
        assert_eq!(text(&locked), "{1}{W}");
    }

    #[test]
    fn req_cast_001_colored_excess_reduces_generic_cr_118_7c_118_7d() {
        let stages = reductions_stage(vec![
            entry(1, RuleKind::CostReduction, vec![pay("{W}{W}{W}")]),
            entry(2, RuleKind::CostReduction, vec![pay("{C}{C}")]),
        ]);
        let locked = determine_total_cost(&basis("{4}{W}{C}"), &[], &stages, &steps(&[1, 2]))
            .expect("total");
        assert_eq!(text(&locked), "{1}");
    }

    #[test]
    fn req_cast_001_snow_reduces_generic_cr_118_7g() {
        let stages = reductions_stage(vec![entry(1, RuleKind::CostReduction, vec![pay("{S}{S}")])]);
        let locked =
            determine_total_cost(&basis("{1}{S}"), &[], &stages, &steps(&[1])).expect("total");
        assert_eq!(text(&locked), "{S}");
    }

    #[test]
    fn req_cast_001_phyrexian_reduction_cr_118_7f() {
        let stages = reductions_stage(vec![entry(
            1,
            RuleKind::CostReduction,
            vec![pay("{B/P}{B/P}")],
        )]);
        let locked =
            determine_total_cost(&basis("{2}{B}"), &[], &stages, &steps(&[1])).expect("total");
        assert_eq!(text(&locked), "{1}");
    }

    #[test]
    fn req_cast_001_hybrid_half_both_ways_cr_118_7e() {
        let stages = reductions_stage(vec![entry(
            1,
            RuleKind::CostReduction,
            vec![pay("{W/U}{2/R}{C/G}{B/G/P}")],
        )]);
        let base = basis("{4}{W}{U}{R}{G}{B}{C}");
        let with = |halves: Vec<SymbolChoice>| {
            determine_total_cost(
                &base,
                &[],
                &stages,
                &[ReductionStep {
                    id: EffectId::new(1),
                    hybrid_halves: halves,
                }],
            )
        };
        let first = with(vec![First, First, First, First]).expect("first");
        assert_eq!(text(&first), "{2}{U}{R}{G}");
        let second = with(vec![Second, Second, Second, Second]).expect("second");
        // {B/G/P} second half: green is already gone, so it reduces generic (CR 118.7c).
        assert_eq!(text(&second), "{3}{W}{B}{C}");
        assert_eq!(with(vec![First, First, First]), Err(CostError::BadChoice));
        assert_eq!(
            with(vec![First, First, First, First, First]),
            Err(CostError::BadChoice)
        );
        assert_eq!(
            with(vec![First, First, First, Life]),
            Err(CostError::BadChoice)
        );
        assert_eq!(
            with(vec![Life, First, First, First]),
            Err(CostError::BadChoice)
        );
    }

    #[test]
    fn req_cast_001_reduction_floor_at_zero_cr_601_2f_118_7() {
        let stages = reductions_stage(vec![entry(
            1,
            RuleKind::CostReduction,
            vec![pay("{9}{W}{W}{C}")],
        )]);
        let locked =
            determine_total_cost(&basis("{1}{W}"), &[], &stages, &steps(&[1])).expect("total");
        assert!(payable(&locked).is_zero());
        assert_eq!(text(&locked), "{0}");
    }

    #[test]
    fn req_cast_001_bad_reduction_order_cr_601_2f() {
        let stages = reductions_stage(vec![
            entry(1, RuleKind::CostReduction, vec![pay("{1}")]),
            entry(2, RuleKind::CostReduction, vec![pay("{1}")]),
        ]);
        let base = basis("{3}");
        for bad in [vec![1], vec![1, 2, 3], vec![1, 1], vec![1, 3], vec![]] {
            assert_eq!(
                determine_total_cost(&base, &[], &stages, &steps(&bad)),
                Err(CostError::BadReductionOrder),
                "order {bad:?}"
            );
        }
        // A reduction order with no reductions at all.
        assert_eq!(
            determine_total_cost(&base, &[], &CostStages::default(), &steps(&[1])),
            Err(CostError::BadReductionOrder)
        );
        // Duplicate ids in the stages can never be matched by a permutation.
        let dup = reductions_stage(vec![
            entry(1, RuleKind::CostReduction, vec![pay("{1}")]),
            entry(1, RuleKind::CostReduction, vec![pay("{1}")]),
        ]);
        assert_eq!(
            determine_total_cost(&base, &[], &dup, &steps(&[1, 1])),
            Err(CostError::BadReductionOrder)
        );
    }

    #[test]
    fn req_cast_001_order_reductions_pairing_cr_601_2f() {
        let entries = vec![
            entry(1, RuleKind::CostReduction, vec![pay("{1}")]),
            entry(2, RuleKind::CostReduction, vec![pay("{1}")]),
            entry(3, RuleKind::CostReduction, vec![pay("{1}")]),
        ];
        // A valid permutation keeps the player's order.
        let order = steps(&[3, 1, 2]);
        let paired = order_reductions(&entries, &order).expect("permutation");
        let ids: Vec<(u64, u64)> = paired
            .iter()
            .map(|(e, s)| (e.id.get(), s.id.get()))
            .collect();
        assert_eq!(ids, vec![(3, 3), (1, 1), (2, 2)]);
        // Missing step, repeated step, unknown id, leftover entry.
        for bad in [vec![1, 2], vec![1, 1, 2], vec![1, 2, 9], vec![1, 2, 3, 3]] {
            assert!(order_reductions(&entries, &steps(&bad)).is_err(), "{bad:?}");
        }
        // Duplicate entry id.
        let dup = vec![
            entry(1, RuleKind::CostReduction, vec![pay("{1}")]),
            entry(1, RuleKind::CostReduction, vec![pay("{1}")]),
        ];
        assert!(order_reductions(&dup, &steps(&[1, 1])).is_err());
        assert!(order_reductions(&dup, &steps(&[1])).is_err());
    }

    #[test]
    fn req_cast_001_every_order_floor_not_reached_cr_601_2f() {
        // SRS 7.1 step 6 in every order: {5}{W}{U}{C} - {2} - {U}{G} - {C/W}(First) = {1}{W}.
        let stages = reductions_stage(vec![
            entry(1, RuleKind::CostReduction, vec![pay("{2}")]),
            entry(2, RuleKind::CostReduction, vec![pay("{U}{G}")]),
            entry(3, RuleKind::CostReduction, vec![pay("{C/W}")]),
        ]);
        for order in PERMUTATIONS {
            let mut s = steps(&order);
            for st in &mut s {
                if st.id == EffectId::new(3) {
                    st.hybrid_halves = vec![First];
                }
            }
            let locked =
                determine_total_cost(&basis("{5}{W}{U}{C}"), &[], &stages, &s).expect("total");
            assert_eq!(text(&locked), "{2}{W}", "order {order:?}");
        }
    }

    #[test]
    fn req_cast_001_every_order_floor_reached_cr_601_2f() {
        // {2}{R}: reductions {3}, {R}{R}, {S} reach {0} in every order; a minimum of 1 then
        // applies after the floor.
        let stages = CostStages {
            reductions: vec![
                entry(1, RuleKind::CostReduction, vec![pay("{3}")]),
                entry(2, RuleKind::CostReduction, vec![pay("{R}{R}")]),
                entry(3, RuleKind::CostReduction, vec![pay("{S}")]),
            ],
            total_effects: vec![minimum(4, json!(1))],
            ..CostStages::default()
        };
        for order in PERMUTATIONS {
            let locked = determine_total_cost(&basis("{2}{R}"), &[], &stages, &steps(&order))
                .expect("total");
            assert_eq!(text(&locked), "{1}", "order {order:?}");
        }
        let no_min = CostStages {
            total_effects: Vec::new(),
            ..stages
        };
        for order in PERMUTATIONS {
            let locked = determine_total_cost(&basis("{2}{R}"), &[], &no_min, &steps(&order))
                .expect("total");
            assert_eq!(text(&locked), "{0}", "order {order:?}");
        }
    }

    // ---- Requirement 5: total effects in list order (CR 601.2f) ----

    #[test]
    fn req_cast_001_minimum_then_set_and_set_then_minimum_cr_601_2f() {
        let min = minimum(1, json!(4));
        let set = entry(2, RuleKind::CostSet, vec![pay("{1}")]);
        let min_then_set = CostStages {
            total_effects: vec![min.clone(), set.clone()],
            ..CostStages::default()
        };
        let set_then_min = CostStages {
            total_effects: vec![set, min],
            ..CostStages::default()
        };
        let a = determine_total_cost(&basis("{G}"), &[], &min_then_set, &[]).expect("a");
        let b = determine_total_cost(&basis("{G}"), &[], &set_then_min, &[]).expect("b");
        assert_eq!(text(&a), "{1}");
        assert_eq!(text(&b), "{4}");
    }

    // ---- Requirement 6: lock-in (CR 601.2f) ----

    #[test]
    fn req_cast_001_locked_cost_unaffected_by_later_stages_cr_601_2f() {
        let before = CostStages {
            increases: vec![entry(1, RuleKind::CostIncrease, vec![pay("{1}")])],
            ..CostStages::default()
        };
        let first = determine_total_cost(&basis("{W}"), &[], &before, &[]).expect("first");
        let snapshot = first.clone();
        let after = CostStages {
            increases: vec![entry(1, RuleKind::CostIncrease, vec![pay("{5}")])],
            total_effects: vec![entry(2, RuleKind::CostSet, vec![pay("{9}")])],
            ..CostStages::default()
        };
        let second = determine_total_cost(&basis("{W}"), &[], &after, &[]).expect("second");
        assert_eq!(first, snapshot);
        assert_eq!(text(&first), "{1}{W}");
        assert_eq!(text(&second), "{9}");
    }

    // ---- Requirement 7: determinism and order-independence of additional costs ----

    /// A small deterministic generator for the property tests (no clock, no OS randomness).
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
    }

    const SYMBOLS: [&str; 8] = ["{1}", "{3}", "{W}", "{U}", "{B}", "{C}", "{S}", "{G}"];

    fn random_cost(rng: &mut Lcg) -> ManaCostDSL {
        let len = 1 + rng.next() % 4;
        let mut text = String::new();
        for _ in 0..len {
            text.push_str(SYMBOLS[(rng.next() % 8) as usize]);
        }
        mana(&text)
    }

    #[test]
    fn req_cast_001_property_additional_order_and_determinism() {
        let mut rng = Lcg(7);
        let others = [CostAST::TapSource, CostAST::Forage, CostAST::UntapSource];
        for _ in 0..200 {
            let base =
                CostBasis::from_mana_cost(Some(&random_cost(&mut rng)), None, &[], Vec::new())
                    .expect("base");
            let extras: Vec<CostBasis> = (0..3)
                .map(|i| {
                    let life = rng.next().is_multiple_of(2);
                    let cost = mana(if life { "{1}{B/P}" } else { "{2}" });
                    let choices: &[SymbolChoice] = if life { &[Life] } else { &[] };
                    CostBasis::from_mana_cost(Some(&cost), None, choices, vec![others[i].clone()])
                        .expect("extra")
                })
                .collect();
            let stages = reductions_stage(vec![
                entry(
                    1,
                    RuleKind::CostReduction,
                    vec![pay(SYMBOLS[(rng.next() % 8) as usize])],
                ),
                entry(
                    2,
                    RuleKind::CostReduction,
                    vec![pay(SYMBOLS[(rng.next() % 8) as usize])],
                ),
            ]);
            let order = steps(&[1, 2]);
            let forward = determine_total_cost(&base, &extras, &stages, &order).expect("forward");
            let again = determine_total_cost(&base, &extras, &stages, &order).expect("again");
            assert_eq!(forward, again);
            let reversed: Vec<CostBasis> = extras.iter().rev().cloned().collect();
            let backward =
                determine_total_cost(&base, &reversed, &stages, &order).expect("backward");
            assert_eq!(forward.mana(), backward.mana());
            assert_eq!(forward.phyrexian_life(), backward.phyrexian_life());
            let rev_other: Vec<CostAST> = forward.other().iter().rev().cloned().collect();
            assert_eq!(backward.other(), rev_other.as_slice());
        }
    }

    #[test]
    fn req_cast_001_property_never_below_zero_and_reduction_bounded() {
        // CR 601.2f: the mana component can't go below {0}; each field only shrinks.
        let mut rng = Lcg(11);
        for _ in 0..300 {
            let base = basis(&random_cost(&mut rng).to_string());
            let ManaPart::Payable(start) = base.mana else {
                panic!("payable")
            };
            let stages = reductions_stage(vec![
                entry(
                    1,
                    RuleKind::CostReduction,
                    vec![pay(&random_cost(&mut rng).to_string())],
                ),
                entry(
                    2,
                    RuleKind::CostReduction,
                    vec![pay(&random_cost(&mut rng).to_string())],
                ),
                entry(
                    3,
                    RuleKind::CostReduction,
                    vec![pay(&random_cost(&mut rng).to_string())],
                ),
            ]);
            let mut results = Vec::new();
            for order in PERMUTATIONS {
                let locked =
                    determine_total_cost(&base, &[], &stages, &steps(&order)).expect("total");
                let m = payable(&locked);
                assert!(
                    m.generic <= start.generic && m.white <= start.white && m.blue <= start.blue
                );
                assert!(
                    m.black <= start.black
                        && m.green <= start.green
                        && m.colorless <= start.colorless
                );
                assert_eq!(m.snow, start.snow, "CR 107.4h: snow is never reduced");
                assert!(m.to_mana_cost().is_ok());
                results.push(m);
            }
            // Without hybrid halves the result does not depend on the order.
            assert!(results.windows(2).all(|w| w[0] == w[1]));
        }
    }

    #[test]
    fn req_cast_001_all_errors_reachable() {
        let all = [
            CostError::BadReductionOrder,
            CostError::BadChoice,
            CostError::MissingX,
            CostError::UnsupportedVariable,
            CostError::UnpayableAdditional,
            CostError::Overflow,
        ];
        let reached = [
            determine_total_cost(&basis("{1}"), &[], &CostStages::default(), &steps(&[9])).err(),
            CostBasis::from_mana_cost(Some(&mana("{W/U}")), None, &[], Vec::new()).err(),
            CostBasis::from_mana_cost(Some(&mana("{X}")), None, &[], Vec::new()).err(),
            CostBasis::from_mana_cost(Some(&mana("{Y}")), None, &[], Vec::new()).err(),
            determine_total_cost(
                &basis("{1}"),
                &[CostBasis::from_mana_cost(None, None, &[], Vec::new()).expect("none")],
                &CostStages::default(),
                &[],
            )
            .err(),
            ManaComponent {
                generic: u64::MAX,
                ..Default::default()
            }
            .to_mana_cost()
            .err(),
        ];
        assert_eq!(reached.map(|e| e.expect("error")), all);
    }
}
