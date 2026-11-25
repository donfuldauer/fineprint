// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! [`Transactional`] for [`GameState`] (REQ-CORE-005, CR 734).
//!
//! `carry_durable` carries exactly the components a durably committed sub-transaction (a mana
//! ability that is not reversed, CR 734.1) can change in today's `GameState`: the PRNG, the
//! identifier generators and the players' life, counters and designations. Players are carried
//! as a delta: the restored player keeps its checkpoint values plus only the change from
//! `before` to `after` (life and counters additively, a count clamped at 0; a changed
//! designation takes `after`'s value or absence). The PRNG and the generators are carried as
//! values (the PRNG set to `after`'s; generators advanced, never rewound). It never carries the
//! turn position, priority, active player, turn order, continuous effects, timestamp ledger,
//! generating-ability links or history ledger, so the priority player stays the checkpoint's
//! (CR 734.2) and a rollback restores the checkpoint's effects, timestamps and links; the effect
//! and timestamp generators are advanced, so no effect id or timestamp issued by the durable
//! sub-transaction is issued again. Components added to `GameState` by later units must be
//! added here by those units.

use std::collections::BTreeSet;

use fineprint_base::hash::Digest;
use fineprint_base::ids::PlayerId;

use super::Transactional;
use crate::state::{GameState, PlayerState};

impl Transactional for GameState {
    fn state_hash(&self) -> Digest {
        GameState::state_hash(self)
    }

    // CR 734.2
    fn priority_player(&self) -> Option<PlayerId> {
        self.turn().priority_player()
    }

    // CR 734.1: the effect of a mana ability that is not reversed survives the rollback.
    fn carry_durable(mut restored: GameState, before: &GameState, after: &GameState) -> GameState {
        // The PRNG (a library shuffle draws from it).
        if before.prng() != after.prng() {
            *restored.prng_mut() = after.prng().clone();
        }

        // Identifier generators: advance, never rewind, so no id issued by the durable
        // sub-transaction is ever issued again.
        advance(
            &mut restored,
            before,
            after,
            |s| s.ids().card.last_issued(),
            |s| s.issue_card_id().is_ok(),
        );
        advance(
            &mut restored,
            before,
            after,
            |s| s.ids().object.last_issued(),
            |s| s.issue_object_id().is_ok(),
        );
        advance(
            &mut restored,
            before,
            after,
            |s| s.ids().ability_instance.last_issued(),
            |s| s.issue_ability_instance_id().is_ok(),
        );
        advance(
            &mut restored,
            before,
            after,
            |s| s.ids().effect.last_issued(),
            |s| s.issue_effect_id().is_ok(),
        );
        advance(
            &mut restored,
            before,
            after,
            |s| s.ids().timestamp.last_issued(),
            |s| s.issue_timestamp().is_ok(),
        );
        advance(
            &mut restored,
            before,
            after,
            |s| s.ids().link.last_issued(),
            |s| s.issue_link_id().is_ok(),
        );

        // Players present in `before`, `after` and `restored` whose state changed `before` ->
        // `after`: only that change is applied over the restored player (CR 734.1: the action's
        // own changes are reversed, the durable sub-transaction's are not).
        for player in after.players() {
            let id = player.id();
            let Some(prior) = before.player(id) else {
                continue;
            };
            if prior == player {
                continue;
            }
            carry_player(&mut restored, prior, player);
        }

        restored
    }
}

/// If generator `last` differs between `before` and `after`, issues from `restored` until its
/// `last` reaches `after`'s; stops (keeping what was done) if issuing fails. Never rewinds.
pub(super) fn advance(
    restored: &mut GameState,
    before: &GameState,
    after: &GameState,
    last: impl Fn(&GameState) -> u64,
    issue: impl Fn(&mut GameState) -> bool,
) {
    let target = last(after);
    if last(before) == target {
        return;
    }
    while last(restored) < target {
        if !issue(restored) {
            return;
        }
    }
}

/// Applies the change of one player from `before` to `after` over `restored`'s player of the
/// same id, if present: the life delta, the per-key counter delta (clamped at 0, which removes
/// the entry, because a count is never negative) and the `after` value (or absence) of every
/// designation that changed. Keys unchanged `before` -> `after` are not touched.
fn carry_player(restored: &mut GameState, before: &PlayerState, after: &PlayerState) {
    let Some(mut target) = restored.player_mut(after.id()) else {
        return;
    };
    if before.life() != after.life() {
        // Exact in i128 (integer, deterministic); only the final result is clamped to i64.
        let exact =
            i128::from(target.life()) + i128::from(after.life()) - i128::from(before.life());
        let clamped = exact.clamp(i128::from(i64::MIN), i128::from(i64::MAX));
        target.set_life(i64::try_from(clamped).unwrap_or(0));
    }
    // Counts are >= 0, so `to - from` cannot overflow and saturating_add then max(0) already
    // equals the exact clamped result.
    // Union of the counter kinds of `before` and `after` (ordered: BTreeMap keys).
    let kinds: BTreeSet<&String> = before
        .counters()
        .keys()
        .chain(after.counters().keys())
        .collect();
    for kind in kinds {
        let (from, to) = (before.counter(kind), after.counter(kind));
        if from == to {
            continue;
        }
        let amount = target
            .counter(kind)
            .saturating_add(to.saturating_sub(from))
            .max(0);
        target.set_counter(kind, amount);
    }
    let keys: BTreeSet<&String> = before
        .designations()
        .keys()
        .chain(after.designations().keys())
        .collect();
    for key in keys {
        let to = after.designation(key);
        if before.designation(key) == to {
            continue;
        }
        match to {
            Some(value) => target.set_designation(key, value.clone()),
            None => {
                target.clear_designation(key);
            }
        }
    }
}
