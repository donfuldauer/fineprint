// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The payment planner: which units of a pool pay a mana component (REQ-CAST-001; CR 107.4a-c,
//! 107.4h, 118.3a, 118.5, 601.2h). The tie-break is documented on the [`mana`](super) module.

use core::cmp::Reverse;

use super::{ManaComponent, ManaPool, ManaRider, ManaUnit, SpendPurpose};

/// The units of a pool that pay a cost (REQ-CAST-001).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaymentPlan {
    /// Indices into the pool's units of the units to spend, ascending.
    pub spent: Vec<usize>,
    /// For each spent unit that carries riders: its index and its riders (CR 106.6, 106.6a),
    /// ascending by index.
    pub riders_fired: Vec<(usize, Vec<ManaRider>)>,
}

/// The demand slots: the six types (W, U, B, R, G, colorless), then `{S}`, then generic.
const SNOW: usize = 6;
const GENERIC: usize = 7;

/// Per-type counts of the units still available: `[nonsnow, snow]` per mana type.
#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    nonsnow: [u64; 6],
    snow: [u64; 6],
}

impl Counts {
    /// The counter of `unit`'s type and snow-ness (`index()` is always below 6).
    fn bucket(&mut self, unit: &ManaUnit) -> Option<&mut u64> {
        let side = if unit.source_is_snow {
            &mut self.snow
        } else {
            &mut self.nonsnow
        };
        side.get_mut(unit.mana_type.index())
    }

    fn add(&mut self, unit: &ManaUnit) {
        if let Some(b) = self.bucket(unit) {
            *b = b.saturating_add(1);
        }
    }

    fn remove(&mut self, unit: &ManaUnit) {
        if let Some(b) = self.bucket(unit) {
            *b = b.saturating_sub(1);
        }
    }
}

/// Whether `demand` can be paid by the units counted in `counts` (CR 107.4a-c, 107.4h).
///
/// Exact: each type's symbols take that type's nonsnow units first, so the most snow units stay
/// free for `{S}`; generic then takes whatever remains.
fn feasible(demand: &[u64; 8], counts: &Counts) -> bool {
    let mut spare_snow: u64 = 0;
    let mut left: u64 = 0;
    for t in 0..6 {
        let d = demand.get(t).copied().unwrap_or(0);
        let ns = counts.nonsnow.get(t).copied().unwrap_or(0);
        let s = counts.snow.get(t).copied().unwrap_or(0);
        let have = ns.saturating_add(s);
        if d > have {
            return false;
        }
        let snow_used = d.saturating_sub(ns);
        spare_snow = spare_snow.saturating_add(s - snow_used);
        left = left.saturating_add(have - d);
    }
    let s_demand = demand.get(SNOW).copied().unwrap_or(0);
    if s_demand > spare_snow {
        return false;
    }
    left - s_demand >= demand.get(GENERIC).copied().unwrap_or(0)
}

/// The slots a unit can pay, in the planner's preference order: its own type, `{S}` (snow
/// sources only), generic.
fn slots(unit: &ManaUnit) -> impl Iterator<Item = usize> {
    let own = unit.mana_type.index();
    let snow = unit.source_is_snow.then_some(SNOW);
    core::iter::once(own)
        .chain(snow)
        .chain(core::iter::once(GENERIC))
}

/// Plans the payment of `cost` from `pool` for `purpose` (REQ-CAST-001; CR 601.2h, 118.3a).
///
/// Colored symbols need a unit of exactly that color (CR 107.4a), `{C}` a colorless unit
/// (CR 107.4c), `{S}` a unit from a snow source (CR 107.4h), and generic mana any unit
/// (CR 107.4b); only units allowed for `purpose` are used (CR 106.6). Returns a complete plan
/// whenever one exists and `None` otherwise (partial payments are not allowed, CR 601.2h). `{0}`
/// gives the empty plan (CR 118.5). The choice among valid plans follows the tie-break on the
/// [`mana`](super) module: most restrictions first, then lowest index.
#[must_use]
pub fn plan_payment(
    pool: &ManaPool,
    cost: &ManaComponent,
    purpose: &SpendPurpose,
) -> Option<PaymentPlan> {
    let units = pool.units();
    let mut demand: [u64; 8] = [
        cost.white,
        cost.blue,
        cost.black,
        cost.red,
        cost.green,
        cost.colorless,
        cost.snow,
        cost.generic,
    ];
    // CR 106.6: only allowed units take part, in the tie-break order.
    let mut order: Vec<usize> = (0..units.len())
        .filter(|&i| units.get(i).is_some_and(|u| u.allowed_for(purpose)))
        .collect();
    order.sort_by_key(|&i| (Reverse(units.get(i).map_or(0, |u| u.restrictions.len())), i));
    let mut counts = Counts::default();
    for &i in &order {
        if let Some(u) = units.get(i) {
            counts.add(u);
        }
    }
    // CR 601.2h: no partial payment.
    if !feasible(&demand, &counts) {
        return None;
    }
    let mut spent = Vec::new();
    for &i in &order {
        if demand.iter().all(|d| *d == 0) {
            break;
        }
        let Some(unit) = units.get(i) else { continue };
        counts.remove(unit);
        for slot in slots(unit) {
            let Some(d) = demand.get_mut(slot) else {
                continue;
            };
            if *d == 0 {
                continue;
            }
            *d -= 1;
            if feasible(&demand, &counts) {
                spent.push(i);
                break;
            }
            if let Some(d) = demand.get_mut(slot) {
                *d += 1;
            }
        }
    }
    // The feasibility invariant guarantees the demand is met; check it anyway.
    if demand.iter().any(|d| *d != 0) {
        return None;
    }
    spent.sort_unstable();
    let riders_fired = spent
        .iter()
        .filter_map(|&i| {
            units
                .get(i)
                .filter(|u| !u.riders.is_empty())
                .map(|u| (i, u.riders.clone()))
        })
        .collect();
    Some(PaymentPlan {
        spent,
        riders_fired,
    })
}

/// CR 118.3a: removes exactly the planned units from `pool` and returns them, in ascending index
/// order. Excess mana stays in the pool (CR 106.4a). Indices that are out of range or repeated
/// are ignored.
pub fn spend(pool: &mut ManaPool, plan: &PaymentPlan) -> Vec<ManaUnit> {
    pool.take(&plan.spent)
}
