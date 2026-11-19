// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! [`Transactional`] for [`GameState`] (REQ-CORE-005, CR 734).
//!
//! `carry_durable` carries exactly the components a durably committed sub-transaction (a mana
//! ability that is not reversed, CR 734.1) can change in today's `GameState`: the PRNG, the
//! identifier generators and the players' life, counters and designations. It never carries the
//! turn position, priority, active player, turn order, effect registry or history ledger, so the
//! priority player stays the checkpoint's (CR 734.2). Components added to `GameState` by later
//! units must be added here by those units.

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

        // Players present in both `restored` and `after` whose state changed `before` ->
        // `after`: life, counters and designations become `after`'s.
        for player in after.players() {
            let id = player.id();
            if before.player(id) == Some(player) {
                continue;
            }
            if restored.player(id).is_none_or(|r| r == player) {
                continue;
            }
            copy_player(&mut restored, player);
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

/// Makes the life, counters and designations of `restored`'s player `from.id()` equal `from`'s.
fn copy_player(restored: &mut GameState, from: &PlayerState) {
    let id = from.id();
    let Some(current) = restored.player(id) else {
        return;
    };
    let stale_counters: Vec<String> = current
        .counters()
        .keys()
        .filter(|k| !from.counters().contains_key(*k))
        .cloned()
        .collect();
    let stale_designations: Vec<String> = current
        .designations()
        .keys()
        .filter(|k| !from.designations().contains_key(*k))
        .cloned()
        .collect();
    let Some(mut target) = restored.player_mut(id) else {
        return;
    };
    target.set_life(from.life());
    for kind in &stale_counters {
        target.set_counter(kind, 0);
    }
    for (kind, amount) in from.counters() {
        target.set_counter(kind, *amount);
    }
    for key in &stale_designations {
        target.clear_designation(key);
    }
    for (key, value) in from.designations() {
        target.set_designation(key, value.clone());
    }
}
