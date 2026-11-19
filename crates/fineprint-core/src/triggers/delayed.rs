// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Delayed triggered abilities (REQ-EVENT-002, CR 603.7, SRS 8.2 item 5) and reflexive
//! triggered abilities (CR 603.12).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_dsl::ability::{ActivationLimit, TriggeredAbilityAST};

use super::detect::{TriggerEnv, TriggerRegistration, WaitingTrigger};
use super::event::EventField;
use super::lookback::StateView;
use super::place::{PendingTrigger, TriggerOrigin};

/// The identifier of one delayed triggered ability within one [`DelayedTriggers`]
/// (REQ-EVENT-002): `0, 1, 2, …` in registration order, never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DelayedTriggerId(u64);

impl DelayedTriggerId {
    /// The raw number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The ability id its registration carries: `delayed#<n>`.
    fn ability_id(self) -> String {
        format!("{DELAYED_PREFIX}{}", self.0)
    }
}

const DELAYED_PREFIX: &str = "delayed#";

/// One registered delayed triggered ability.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Delayed {
    source: ObjectId,
    controller: PlayerId,
    ability: TriggeredAbilityAST,
    captured: BTreeMap<String, EventField>,
    one_shot: bool,
    /// The trigger-limit counters of this instance (CR 603.2h), counted by
    /// [`DelayedTriggers::take_triggered`] after the intervening "if" passed (CR 603.4).
    counts: LimitCounts,
}

/// The trigger-limit counters of one delayed triggered ability (CR 603.2h).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LimitCounts {
    /// The turn `this_turn` was last counted for; `None` before the first triggering.
    turn: Option<u64>,
    /// Triggerings during `turn`.
    this_turn: u32,
    /// Triggerings this game.
    this_game: u32,
}

impl LimitCounts {
    /// Counts one triggering in `turn` if `limit` allows it (CR 603.2h); false, with nothing
    /// counted, if the limit is reached.
    fn try_count(&mut self, limit: &ActivationLimit, turn: u64) -> bool {
        let this_turn = if self.turn == Some(turn) {
            self.this_turn
        } else {
            0
        };
        if limit.per_turn.is_some_and(|n| this_turn >= n)
            || limit.per_game.is_some_and(|n| self.this_game >= n)
        {
            return false;
        }
        self.turn = Some(turn);
        self.this_turn = this_turn.saturating_add(1);
        self.this_game = self.this_game.saturating_add(1);
        true
    }
}

/// The registered delayed triggered abilities (REQ-EVENT-002, CR 603.7).
///
/// A delayed triggered ability is created by an effect (CR 603.7a) and captures its bindings
/// when it is created (SRS 8.2 item 5). It does not trigger before it is registered
/// (CR 603.7a). It persists if its source leaves its zone: the CR is silent, so this is a
/// recorded decision (docs/DECISIONS.md, "#31 FP-030 — Inferences and choices").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DelayedTriggers {
    entries: BTreeMap<u64, Delayed>,
    next: u64,
}

impl DelayedTriggers {
    /// No delayed trigger.
    #[must_use]
    pub fn new() -> DelayedTriggers {
        DelayedTriggers::default()
    }

    /// Registers a delayed triggered ability with the bindings `captured` now (REQ-EVENT-002,
    /// CR 603.7, CR 603.7a).
    ///
    /// The **caller** supplies `source` and `controller` per CR 603.7d-g: created by a spell,
    /// the source is that spell and the controller the player who controlled it as it resolved
    /// (CR 603.7d); by an activated or triggered ability, the source is that ability's source
    /// and the controller the player who controlled that ability as it resolved (CR 603.7e);
    /// through a replacement effect of a static ability, the object with that static ability
    /// and its controller when the replacement effect was applied (CR 603.7f); after an action a
    /// static ability allowed, the object with that ability and its controller when the action
    /// was taken (CR 603.7g). `one_shot` is true iff the ability has **no** stated duration: it
    /// then triggers only once, the next time its trigger event occurs (CR 603.7b).
    pub fn register(
        &mut self,
        source: ObjectId,
        controller: PlayerId,
        ability: TriggeredAbilityAST,
        captured: BTreeMap<String, EventField>,
        one_shot: bool,
    ) -> DelayedTriggerId {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        self.entries.insert(
            id,
            Delayed {
                source,
                controller,
                ability,
                captured,
                one_shot,
                counts: LimitCounts::default(),
            },
        );
        DelayedTriggerId(id)
    }

    /// The registrations for [`TriggerEnv::registrations`](super::TriggerEnv::registrations),
    /// ordered by id: each with its stored source, controller and ability, the ability id
    /// replaced by `delayed#<n>` so every delayed instance is distinct (docs/DECISIONS.md,
    /// "#30 FP-029 CR round 1" item 1).
    ///
    /// Each listed ability has its intervening "if" and its trigger limit removed
    /// (`intervening_if` and `limit` are `None`). [`detect`](super::detect) checks an
    /// intervening "if" with the event's bindings only, but a delayed trigger's condition must
    /// see the bindings it captured when it was created (CR 603.4, CR 603.7).
    /// [`DelayedTriggers::take_triggered`] checks the stored condition instead, with the
    /// captured bindings applied, and only then the stored limit (CR 603.2h), so that an event
    /// that fails the condition neither uses up the limit nor suppresses a later event that
    /// passes it. Detect therefore never keeps limit counters in Memory for a delayed ability.
    #[must_use]
    pub fn registrations(&self) -> Vec<TriggerRegistration> {
        self.entries
            .iter()
            .map(|(&id, d)| registration(id, d, false))
            .collect()
    }

    /// The registered delayed trigger a `(source, ability id)` pair names, if any.
    fn lookup(&self, source: ObjectId, ability_id: &str) -> Option<u64> {
        let n: u64 = ability_id.strip_prefix(DELAYED_PREFIX)?.parse().ok()?;
        if DelayedTriggerId(n).ability_id() != ability_id {
            return None;
        }
        self.entries
            .get(&n)
            .filter(|d| d.source == source)
            .map(|_| n)
    }

    /// The pending triggers of the delayed abilities among `waiting` (REQ-EVENT-002, CR 603.4,
    /// CR 603.7, CR 603.7b), in input order.
    ///
    /// Each element whose (source, ability id) is a delayed registration is considered with
    /// the event bindings overlaid by the captured bindings (the captured value wins on a name
    /// clash). If the stored ability has an intervening "if", it is checked here with
    /// [`TriggerEnv::condition_holds`] and those overlaid bindings, in the view detection used
    /// for that element ([`StateView::PreEvent`] if `look_back` is set, else
    /// [`StateView::PostEvent`]), for a registration whose ability id is `delayed#<n>` (as in
    /// [`DelayedTriggers::registrations`], which leaves the condition to this method). A false
    /// condition drops the element: it does not trigger (CR 603.4), and nothing is counted.
    ///
    /// Then, if the stored ability has a `limit`, the element is checked against this
    /// registration's own counters for `turn`, the turn number the limit counters are kept for
    /// (the same value the caller passes to [`detect`](super::detect)): it is dropped if the
    /// per-turn or per-game count has reached the limit, otherwise counted (CR 603.2h). The
    /// limit is enforced here, after the condition, rather than by detect, so that an element
    /// that fails its condition neither consumes the limit nor suppresses a later passing
    /// element. Elements are counted in input order. A dropped element does not consume a
    /// one-shot registration.
    ///
    /// Each remaining element yields a [`PendingTrigger`] with origin
    /// [`TriggerOrigin::Delayed`], the `delayed#<n>` ability id, the overlaid bindings, and
    /// option, targets and modes as [`PendingTrigger::from_waiting`]. A one-shot registration
    /// yields at most one pending trigger, for the first element in input order that passes
    /// its condition, and is then removed; a dropped element does not consume it (CR 603.7b; the
    /// controller's choice of event is replaced by first-in-creation-order, a recorded
    /// decision). A registration with a stated duration yields one per passing element and
    /// stays. Other elements are ignored.
    pub fn take_triggered<E: TriggerEnv + ?Sized>(
        &mut self,
        env: &E,
        turn: u64,
        waiting: &[WaitingTrigger],
    ) -> Vec<PendingTrigger> {
        let mut fired_once: BTreeSet<u64> = BTreeSet::new();
        let mut out = Vec::new();
        for w in waiting {
            let Some(n) = self.lookup(w.source, &w.ability_id) else {
                continue;
            };
            let Some(d) = self.entries.get(&n) else {
                continue;
            };
            if d.one_shot && fired_once.contains(&n) {
                // CR 603.7b: a delayed trigger with no stated duration triggers only once.
                continue;
            }
            let mut bindings = w.bindings.clone();
            for (name, value) in &d.captured {
                bindings.insert(name.clone(), value.clone());
            }
            // CR 603.4: the intervening "if" is checked when the trigger event occurs, here
            // with the captured bindings (CR 603.7).
            if let Some(pred) = &d.ability.intervening_if {
                let view = if w.look_back.is_some() {
                    StateView::PreEvent
                } else {
                    StateView::PostEvent
                };
                let reg = registration(n, d, true);
                if !env.condition_holds(view, &reg, pred, &bindings) {
                    continue;
                }
            }
            // CR 603.2h: the trigger limit, counted only for an element whose condition held.
            if let Some(limit) = d.ability.limit
                && let Some(entry) = self.entries.get_mut(&n)
                && !entry.counts.try_count(&limit, turn)
            {
                continue;
            }
            let Some(d) = self.entries.get(&n) else {
                continue;
            };
            if d.one_shot {
                fired_once.insert(n);
            }
            let mut pending =
                PendingTrigger::from_waiting(w.clone(), &d.ability, TriggerOrigin::Delayed);
            pending.controller = d.controller;
            pending.bindings = bindings;
            out.push(pending);
        }
        for n in fired_once {
            self.entries.remove(&n);
        }
        out
    }

    /// Fires the delayed trigger `id` directly (a phase or step delayed trigger; the turn unit
    /// calls it when the step begins), with its captured bindings. `None` if `id` is unknown.
    /// A one-shot trigger is removed (CR 603.7b).
    pub fn fire(&mut self, id: DelayedTriggerId) -> Option<PendingTrigger> {
        let d = self.entries.get(&id.0)?;
        let pending = PendingTrigger::new(
            TriggerOrigin::Delayed,
            d.controller,
            Some(d.source),
            id.ability_id(),
            d.captured.clone(),
            &d.ability,
        );
        if d.one_shot {
            self.entries.remove(&id.0);
        }
        Some(pending)
    }

    /// Removes the delayed trigger `id`; true iff it was registered.
    pub fn cancel(&mut self, id: DelayedTriggerId) -> bool {
        self.entries.remove(&id.0).is_some()
    }

    /// The number of registered delayed triggers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no delayed trigger is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The registration of delayed trigger `id`: its stored source, controller and ability with the
/// ability id `delayed#<n>`; the intervening "if" kept iff `keep_condition`. The `limit` is
/// always removed: [`DelayedTriggers::take_triggered`] enforces it (CR 603.2h).
fn registration(id: u64, d: &Delayed, keep_condition: bool) -> TriggerRegistration {
    let mut ability = d.ability.clone();
    ability.base.ability_id = DelayedTriggerId(id).ability_id();
    ability.limit = None;
    if !keep_condition {
        ability.intervening_if = None;
    }
    TriggerRegistration {
        source: d.source,
        controller: d.controller,
        ability,
    }
}

/// The reflexive triggers of one resolution (REQ-EVENT-002, CR 603.12, CR 603.12a).
///
/// `occurrences` is how many times the trigger event occurred earlier during the resolution of
/// the spell or ability that created the reflexive ability. It triggers once per occurrence
/// (CR 603.12a), or at most once if `repeated_payment` (a cost paid one or more times,
/// CR 603.12a). Each pending trigger has origin [`TriggerOrigin::Reflexive`], the ability's own
/// id and `bindings`; `source` and `controller` follow CR 603.7d-e as for a delayed trigger
/// (CR 603.12). Zero occurrences yield nothing.
#[must_use]
pub fn reflexive_triggers(
    source: ObjectId,
    controller: PlayerId,
    ability: &TriggeredAbilityAST,
    bindings: BTreeMap<String, EventField>,
    occurrences: u32,
    repeated_payment: bool,
) -> Vec<PendingTrigger> {
    let count = if repeated_payment {
        occurrences.min(1)
    } else {
        occurrences
    };
    (0..count)
        .map(|_| {
            PendingTrigger::new(
                TriggerOrigin::Reflexive,
                controller,
                Some(source),
                ability.base.ability_id.clone(),
                bindings.clone(),
                ability,
            )
        })
        .collect()
}
