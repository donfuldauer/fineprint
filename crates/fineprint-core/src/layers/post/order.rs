// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The application order of one pass after the layers (CR 613.10, 613.11): timestamp order
//! (CR 613.7) with the optional dependency hook (CR 613.8, 613.8b).

use fineprint_base::ids::{EffectId, Timestamp};

use super::effects::PostLayerEnvironment;

/// Orders the effects `keys` of one pass and returns their ids in application order. Effects
/// whose [`PostLayerEnvironment::effect_exists`] is false are dropped (CR 613.8a(b)).
///
/// CR 613.7: an earlier timestamp applies first; equal timestamps by ascending [`EffectId`].
/// CR 613.8b: an effect that depends on unapplied effects waits; among the effects that depend
/// on no unapplied effect the earliest key applies. When every unapplied effect depends on
/// another, the effects form dependency loops: the earliest-key member of a loop with no way out
/// (a strongly connected component that depends on nothing outside itself) applies, and the
/// relation is looked at again after each application.
pub(super) fn application_order(
    keys: &[(Timestamp, EffectId)],
    env: &dyn PostLayerEnvironment,
) -> Vec<EffectId> {
    let mut sorted: Vec<(Timestamp, EffectId)> = keys
        .iter()
        .copied()
        .filter(|&(_, id)| env.effect_exists(id))
        .collect();
    // CR 613.7: timestamp order; EffectId breaks ties so registration order never matters.
    sorted.sort_unstable();
    sorted.dedup();
    let ids: Vec<EffectId> = sorted.iter().map(|&(_, id)| id).collect();
    let n = ids.len();
    // CR 613.8a: deps[i][j] is "i depends on j" (an effect never depends on itself).
    let deps: Vec<Vec<bool>> = ids
        .iter()
        .enumerate()
        .map(|(i, &a)| {
            ids.iter()
                .enumerate()
                .map(|(j, &b)| i != j && env.depends_on(a, b))
                .collect()
        })
        .collect();
    let mut applied = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while order.len() < n {
        let free = (0..n).find(|&i| !applied[i] && (0..n).all(|j| applied[j] || !deps[i][j]));
        // CR 613.8b: if no effect is free, a dependency loop is applied in timestamp order.
        let next = free
            .or_else(|| earliest_in_closed_loop(&deps, &applied))
            .or_else(|| (0..n).find(|&i| !applied[i]));
        let Some(i) = next else { break };
        applied[i] = true;
        order.push(ids[i]);
    }
    order
}

/// The earliest unapplied effect (lowest index, indices being in key order) that belongs to a
/// dependency loop with no way out: everything it reaches through unapplied dependencies reaches
/// it back (CR 613.8b).
fn earliest_in_closed_loop(deps: &[Vec<bool>], applied: &[bool]) -> Option<usize> {
    let n = deps.len();
    let reach: Vec<Vec<bool>> = (0..n).map(|i| reachable(deps, applied, i)).collect();
    (0..n).find(|&i| !applied[i] && (0..n).all(|j| !reach[i][j] || reach[j][i]))
}

/// Which unapplied effects `start` reaches through one or more unapplied dependencies.
fn reachable(deps: &[Vec<bool>], applied: &[bool], start: usize) -> Vec<bool> {
    let n = deps.len();
    let mut seen = vec![false; n];
    if applied[start] {
        return seen;
    }
    let mut stack = vec![start];
    while let Some(i) = stack.pop() {
        for j in 0..n {
            if deps[i][j] && !applied[j] && !seen[j] {
                seen[j] = true;
                stack.push(j);
            }
        }
    }
    seen
}
