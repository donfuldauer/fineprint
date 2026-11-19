// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! State triggers (REQ-EVENT-002, CR 603.8, SRS 8.2 item 4): abilities that trigger as soon
//! as a game state matches their condition, and do not trigger again until the existing
//! instance has left the stack.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::ObjectId;
use fineprint_dsl::ability::Trigger;

use super::detect::{TriggerEnv, WaitingTrigger};
use super::event::EventField;
use super::lookback::StateView;

/// The state-trigger tracker (REQ-EVENT-002, CR 603.8): which state-trigger instances are
/// outstanding.
///
/// An instance is (source, ability id), as listed by [`TriggerEnv::registrations`]; separate
/// instances are independent (CR 113.2c). An instance is **outstanding** from the moment it
/// triggers until the caller reports with [`StateTriggers::instance_left_stack`] that it
/// resolved, was countered, was removed (including at placement for having no legal choice)
/// or otherwise left the stack; while outstanding it does not trigger again (CR 603.8).
/// Counting the time it waits to be put on the stack as outstanding is an inference from
/// CR 603.8 (docs/DECISIONS.md, "#31 FP-030 — Inferences and choices").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateTriggers {
    outstanding: BTreeSet<(ObjectId, String)>,
}

impl StateTriggers {
    /// No outstanding instance.
    #[must_use]
    pub fn new() -> StateTriggers {
        StateTriggers::default()
    }

    /// Checks every state trigger once (REQ-EVENT-002, CR 603.8).
    ///
    /// Lists [`TriggerEnv::registrations`] for [`StateView::PostEvent`] (an instance whose
    /// object is no longer listed, i.e. no longer in the same zone, does not trigger, CR 603.8),
    /// keeps the [`Trigger::State`] ones in (source, ability id) order and skips outstanding
    /// instances. An instance triggers iff its condition holds
    /// ([`TriggerEnv::condition_holds`] with no bindings) and, if it has an intervening "if"
    /// clause, that holds too (CR 603.4, the trigger-time check; the resolution-time check is
    /// the resolution unit's). A triggered instance is marked outstanding and returned as a
    /// [`WaitingTrigger`] with no look-back category, no event indices and no bindings. A false
    /// intervening "if" leaves the instance not outstanding.
    pub fn check<E: TriggerEnv + ?Sized>(&mut self, env: &E) -> Vec<WaitingTrigger> {
        let mut regs = env.registrations(StateView::PostEvent);
        regs.sort_by(|a, b| {
            (a.source, &a.ability.base.ability_id).cmp(&(b.source, &b.ability.base.ability_id))
        });
        let empty: BTreeMap<String, EventField> = BTreeMap::new();
        let mut out = Vec::new();
        for reg in &regs {
            let Trigger::State(pred) = &reg.ability.trigger else {
                continue;
            };
            let key = (reg.source, reg.ability.base.ability_id.clone());
            // CR 603.8: no new instance while the existing one has not left the stack.
            if self.outstanding.contains(&key) {
                continue;
            }
            // CR 603.8: triggers as soon as the game state matches the condition.
            if !env.condition_holds(StateView::PostEvent, reg, pred, &empty) {
                continue;
            }
            // CR 603.4: the intervening "if" clause is checked when the trigger event occurs.
            if let Some(cond) = &reg.ability.intervening_if
                && !env.condition_holds(StateView::PostEvent, reg, cond, &empty)
            {
                continue;
            }
            self.outstanding.insert(key.clone());
            out.push(WaitingTrigger {
                source: reg.source,
                controller: reg.controller,
                ability_id: key.1,
                look_back: None,
                event_indices: Vec::new(),
                bindings: BTreeMap::new(),
            });
        }
        out
    }

    /// Reports that the instance (`source`, `ability_id`) resolved, was countered, was removed
    /// (including at placement for having no legal choice) or otherwise left the stack
    /// (CR 603.8). Returns true iff it was outstanding. The next [`StateTriggers::check`]
    /// re-checks it: it triggers again only if its object is still listed (still in the same
    /// zone) and the condition still holds (CR 603.8).
    pub fn instance_left_stack(&mut self, source: ObjectId, ability_id: &str) -> bool {
        self.outstanding.remove(&(source, ability_id.to_owned()))
    }

    /// Drops the outstanding entry of an instance whose object left its zone (cleanup); true
    /// iff it was present.
    pub fn forget(&mut self, source: ObjectId, ability_id: &str) -> bool {
        self.outstanding.remove(&(source, ability_id.to_owned()))
    }

    /// Whether the instance (`source`, `ability_id`) is outstanding (waiting or on the stack).
    #[must_use]
    pub fn is_outstanding(&self, source: ObjectId, ability_id: &str) -> bool {
        self.outstanding.contains(&(source, ability_id.to_owned()))
    }
}
