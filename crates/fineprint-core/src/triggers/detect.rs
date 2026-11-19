// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Matching event-triggered abilities against one batch of events (REQ-EVENT-002, CR 603.2,
//! CR 603.10, CR 603.4), with per-ability trigger limits kept in Memory.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_dsl::ability::{
    ActivationLimit, EventBatch as DslEventBatch, EventMatcher, EventSubject, FilterValue, Trigger,
    TriggeredAbilityAST,
};
use fineprint_dsl::expr::{ObjectSelectorAST, PlayerGroup, PredicateAST};

use super::event::{EventField, TriggerEvent};
use super::lookback::{LookBackKind, StateView, look_back_kind};
use crate::memory::{ChoiceValue, Memory, MemoryTable};

/// A triggered ability as registered on an object (REQ-EVENT-002). Only abilities with a
/// [`Trigger::Event`] trigger are considered by [`detect`]; others are ignored.
///
/// **Ability-instance identity.** An ability instance is identified by (`source`, the
/// ability's `ability_id`) as supplied by [`TriggerEnv::registrations`]: `ONE_OR_MORE`
/// grouping and the `limit` counters are keyed by it. A producer that grants two independent
/// copies of one definition ability to one object must give each registration a distinct
/// `ability_id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriggerRegistration {
    /// The object the ability is on (its id in the view the registration was listed in).
    pub source: ObjectId,
    /// The ability's controller.
    pub controller: PlayerId,
    /// The ability.
    pub ability: TriggeredAbilityAST,
}

/// The game state as seen by trigger detection (REQ-EVENT-002). The integration unit implements
/// it on the real state; tests implement it with fakes. Every method answers for the given
/// [`StateView`]: the state immediately before or immediately after the batch (CR 603.10).
pub trait TriggerEnv {
    /// Active triggered abilities as they exist in `view` (pre-event: the abilities that
    /// existed immediately before the batch, CR 603.10).
    fn registrations(&self, view: StateView) -> Vec<TriggerRegistration>;

    /// Does `object` (its id in `view`) satisfy the event subject selector, for the ability
    /// `reg`, as seen in `view`?
    fn object_matches(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        object: ObjectId,
        selector: &ObjectSelectorAST,
    ) -> bool;

    /// Is `player` in the event subject group `group`, for the ability `reg`, as seen in
    /// `view`?
    fn player_matches(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        player: PlayerId,
        group: &PlayerGroup,
    ) -> bool;

    /// Does a [`FilterValue::Predicate`] filter hold? `field` is the event field under test
    /// (`None` if the event lacks it).
    fn predicate_holds(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        pred: &PredicateAST,
        field: Option<&EventField>,
    ) -> bool;

    /// Does the intervening "if" condition (CR 603.4) hold, with the event's `bindings`?
    fn condition_holds(
        &self,
        view: StateView,
        reg: &TriggerRegistration,
        pred: &PredicateAST,
        bindings: &BTreeMap<String, EventField>,
    ) -> bool;

    /// Was `object` (its pre-event id) an object all players can see (CR 603.10a, the hand or
    /// library clause)?
    fn is_visible_to_all(&self, object: ObjectId) -> bool;
}

/// A triggered ability that has triggered and waits to be put on the stack (REQ-EVENT-002,
/// CR 603.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitingTrigger {
    /// The ability's source (its id in the view it triggered in).
    pub source: ObjectId,
    /// The ability's controller.
    pub controller: PlayerId,
    /// The ability's id within its definition.
    pub ability_id: String,
    /// `Some(category)` iff it was detected in the pre-event view (CR 603.10a-g).
    pub look_back: Option<LookBackKind>,
    /// Indices into the `events` passed to [`detect`]: one for `EACH`; every matching event,
    /// ascending, for `ONE_OR_MORE`.
    pub event_indices: Vec<usize>,
    /// Bound variable name to captured value, from the first matching event.
    pub bindings: BTreeMap<String, EventField>,
}

/// One registration with an event matcher, and its look-back category.
struct EventReg<'a> {
    reg: &'a TriggerRegistration,
    matcher: &'a EventMatcher,
    look_back: Option<LookBackKind>,
}

/// The event-triggered registrations of `regs`, sorted by (source, ability id) (REQ-CORE-001).
fn event_regs(regs: &[TriggerRegistration]) -> Vec<EventReg<'_>> {
    let mut out: Vec<EventReg<'_>> = regs
        .iter()
        .filter_map(|reg| match &reg.ability.trigger {
            Trigger::Event(matcher) => Some(EventReg {
                reg,
                matcher,
                look_back: look_back_kind(matcher),
            }),
            Trigger::PhaseStep { .. } | Trigger::State(_) => None,
        })
        .collect();
    out.sort_by(|a, b| {
        (a.reg.source, &a.reg.ability.base.ability_id)
            .cmp(&(b.reg.source, &b.reg.ability.base.ability_id))
    });
    out
}

/// The view a matcher of category `look_back` is evaluated in for `event` (CR 603.10,
/// CR 603.10a: the hand or library clause applies only to an object all players can see).
fn view_for(
    env: &dyn TriggerEnv,
    look_back: Option<LookBackKind>,
    event: &TriggerEvent,
) -> StateView {
    match look_back {
        None => StateView::PostEvent,
        Some(LookBackKind::ToHandOrLibrary) => match event.subject_pre {
            Some(o) if env.is_visible_to_all(o) => StateView::PreEvent,
            _ => StateView::PostEvent,
        },
        Some(_) => StateView::PreEvent,
    }
}

/// The subject object's id in `view`.
fn subject_object(event: &TriggerEvent, view: StateView) -> Option<ObjectId> {
    match view {
        StateView::PreEvent => event.subject_pre,
        StateView::PostEvent => event.subject_post,
    }
}

/// Whether `event` matches `matcher`'s subject and filters in `view` (CR 603.2).
fn matches(
    env: &dyn TriggerEnv,
    view: StateView,
    reg: &TriggerRegistration,
    matcher: &EventMatcher,
    event: &TriggerEvent,
) -> bool {
    let subject_ok = match &matcher.subject {
        None => true,
        Some(EventSubject::Object(sel)) => {
            subject_object(event, view).is_some_and(|o| env.object_matches(view, reg, o, sel))
        }
        Some(EventSubject::Players(group)) => event
            .subject_player
            .is_some_and(|p| env.player_matches(view, reg, p, group)),
    };
    if !subject_ok {
        return false;
    }
    let Some(filters) = &matcher.filters else {
        return true;
    };
    filters.iter().all(|(key, want)| match want {
        FilterValue::Any => true,
        FilterValue::Predicate(p) => env.predicate_holds(view, reg, p, event.fields.get(key)),
        FilterValue::Bool(_)
        | FilterValue::Int(_)
        | FilterValue::Zone(_)
        | FilterValue::Name(_) => {
            matches!(event.fields.get(key), Some(EventField::Value(v)) if v == want)
        }
    })
}

/// The bindings `matcher.bind` captures from `event` in `view`: reserved field `object` is the
/// subject object's id in the view (a pre-event id under look-back), `player` the subject
/// player; any other field is read from `event.fields`. Absent values are not bound.
fn bindings(
    matcher: &EventMatcher,
    event: &TriggerEvent,
    view: StateView,
) -> BTreeMap<String, EventField> {
    let mut out = BTreeMap::new();
    let Some(bind) = &matcher.bind else {
        return out;
    };
    for (field, var) in bind {
        let value = match field.as_str() {
            "object" => subject_object(event, view).map(EventField::Object),
            "player" => event.subject_player.map(EventField::Player),
            _ => event.fields.get(field).cloned(),
        };
        if let Some(v) = value {
            out.insert(var.clone(), v);
        }
    }
    out
}

/// Memory key of the turn the per-turn counter belongs to.
fn turn_key(ability_id: &str) -> String {
    format!("trigger.limit.turn:{ability_id}")
}

/// Memory key of the number of triggerings in that turn.
fn turn_count_key(ability_id: &str) -> String {
    format!("trigger.limit.turn_count:{ability_id}")
}

/// Memory key of the number of triggerings this game.
fn game_count_key(ability_id: &str) -> String {
    format!("trigger.limit.game_count:{ability_id}")
}

/// The number stored under `key`, if any.
fn number(memory: Option<&Memory>, key: &str) -> Option<i64> {
    match memory?.choice(key)? {
        ChoiceValue::Number(n) => Some(*n),
        _ => None,
    }
}

/// The (this turn, this game) trigger counts of `ability_id` on `source`.
fn counts(memory: &MemoryTable, source: ObjectId, ability_id: &str, turn: i64) -> (i64, i64) {
    let mem = memory.get(source);
    let this_turn = if number(mem, &turn_key(ability_id)) == Some(turn) {
        number(mem, &turn_count_key(ability_id)).unwrap_or(0)
    } else {
        0
    };
    (
        this_turn,
        number(mem, &game_count_key(ability_id)).unwrap_or(0),
    )
}

/// Whether the counts are below `limit` (SRS 8.2 item 8; CR 603.2h covers the "Do this only
/// once each turn" form).
fn within(limit: &ActivationLimit, (this_turn, this_game): (i64, i64)) -> bool {
    limit.per_turn.is_none_or(|n| this_turn < i64::from(n))
        && limit.per_game.is_none_or(|n| this_game < i64::from(n))
}

/// Records one triggering of `ability_id` on `source` in `turn`.
fn record(
    memory: &mut MemoryTable,
    source: ObjectId,
    ability_id: &str,
    turn: i64,
    (this_turn, this_game): (i64, i64),
) {
    let mem = memory.get_mut_or_default(source);
    mem.set_choice(&turn_key(ability_id), ChoiceValue::Number(turn));
    mem.set_choice(
        &turn_count_key(ability_id),
        ChoiceValue::Number(this_turn.saturating_add(1)),
    );
    mem.set_choice(
        &game_count_key(ability_id),
        ChoiceValue::Number(this_game.saturating_add(1)),
    );
}

/// A matched ability instance with the view of its first matching event and its matching event
/// indices.
struct Candidate<'a> {
    view: StateView,
    reg: &'a EventReg<'a>,
    indices: Vec<usize>,
}

/// Detects which event-triggered abilities trigger on `events`, one batch of simultaneous
/// events in order (REQ-EVENT-002, SRS 8.2).
///
/// Equivalent to [`detect_with_pre_event_memory`] with `pre_memory` a copy of `memory` taken
/// before the call: correct whenever `memory` has not yet lost the Memory of a source that left
/// in the batch. See [`detect_with_pre_event_memory`] for the full contract.
#[must_use]
pub fn detect(
    env: &dyn TriggerEnv,
    memory: &mut MemoryTable,
    turn: u64,
    events: &[TriggerEvent],
) -> Vec<WaitingTrigger> {
    if events.is_empty() {
        return Vec::new();
    }
    let pre_memory = memory.clone();
    detect_with_pre_event_memory(env, &pre_memory, memory, turn, events)
}

/// Detects which event-triggered abilities trigger on `events`, one batch of simultaneous
/// events in order (REQ-EVENT-002, SRS 8.2), reading trigger limits from the Memory as it was
/// before the batch.
///
/// For each event and registration: the matcher's kind must be one of the event's kinds; the
/// view is the pre-event state for the look-back categories of CR 603.10a-g (for the hand or
/// library category only when the object was visible to all) and the post-event state
/// otherwise (CR 603.10), and the registration must be listed in that view; the subject and
/// filters must match in that view. `EACH` matchers yield one candidate per matching event and
/// `ONE_OR_MORE` matchers one per ability instance per call (CR 603.2c), whichever view each
/// event matched in; its view, bindings and look-back category are those of its first matching
/// event. An ability instance is (source, ability id) as listed by
/// [`TriggerEnv::registrations`] (see [`TriggerRegistration`]). Candidates are taken in order
/// of (first event index, source, ability id); each is dropped if its intervening "if"
/// condition is false at trigger time (CR 603.4), or if its `limit` is reached; otherwise it is
/// returned and its limit counters (keyed by ability id, for `turn`) are incremented.
///
/// **Memory contract.** `pre_memory` is the [`MemoryTable`] as it was immediately before the
/// batch: it still holds the Memory of a source that left a zone in the batch (CR 400.7), so a
/// departing source's used limit still counts (CR 603.2h, SRS 8.2 item 8). The counts of an
/// ability instance are its counts in `pre_memory` plus the candidates of that instance already
/// accepted in this call. `memory` is the current (post-batch) table: the incremented counters
/// are written to it only for a source that survives, i.e. whose (source, ability id) is listed
/// in the post-event registrations; a departing source gets no entry in `memory`.
#[must_use]
pub fn detect_with_pre_event_memory(
    env: &dyn TriggerEnv,
    pre_memory: &MemoryTable,
    memory: &mut MemoryTable,
    turn: u64,
    events: &[TriggerEvent],
) -> Vec<WaitingTrigger> {
    if events.is_empty() {
        return Vec::new();
    }
    let pre_list = env.registrations(StateView::PreEvent);
    let post_list = env.registrations(StateView::PostEvent);
    let pre = event_regs(&pre_list);
    let post = event_regs(&post_list);

    // CR 603.2, CR 603.10: match every event against the registrations of its view.
    let mut candidates: Vec<Candidate<'_>> = Vec::new();
    // CR 603.2c, CR 603.10a: ONE_OR_MORE groups by ability instance, across views.
    let mut grouped: BTreeMap<(ObjectId, &str), usize> = BTreeMap::new();
    for (i, event) in events.iter().enumerate() {
        for (view, regs) in [(StateView::PreEvent, &pre), (StateView::PostEvent, &post)] {
            for er in regs {
                if !event.kinds.contains(&er.matcher.kind)
                    || view_for(env, er.look_back, event) != view
                    || !matches(env, view, er.reg, er.matcher, event)
                {
                    continue;
                }
                // CR 603.2c: EACH triggers per occurrence; ONE_OR_MORE once per batch.
                if er.matcher.batch == Some(DslEventBatch::OneOrMore) {
                    let key = (er.reg.source, er.reg.ability.base.ability_id.as_str());
                    if let Some(&c) = grouped.get(&key) {
                        let indices = &mut candidates[c].indices;
                        if indices.last() != Some(&i) {
                            indices.push(i);
                        }
                        continue;
                    }
                    grouped.insert(key, candidates.len());
                }
                candidates.push(Candidate {
                    view,
                    reg: er,
                    indices: vec![i],
                });
            }
        }
    }
    // Stable: ties keep (event, pre before post, registration) order.
    candidates.sort_by(|a, b| {
        (
            a.indices[0],
            a.reg.reg.source,
            &a.reg.reg.ability.base.ability_id,
        )
            .cmp(&(
                b.indices[0],
                b.reg.reg.source,
                &b.reg.reg.ability.base.ability_id,
            ))
    });

    // Ability instances that still exist after the batch (their counters are kept).
    let survivors: BTreeSet<(ObjectId, &str)> = post_list
        .iter()
        .map(|r| (r.source, r.ability.base.ability_id.as_str()))
        .collect();
    // Triggerings accepted in this call, per ability instance.
    let mut tally: BTreeMap<(ObjectId, &str), i64> = BTreeMap::new();

    let turn = i64::try_from(turn).unwrap_or(i64::MAX);
    let mut out = Vec::new();
    for cand in candidates {
        let reg = cand.reg.reg;
        let ability = &reg.ability;
        let ability_id = &ability.base.ability_id;
        let binds = bindings(cand.reg.matcher, &events[cand.indices[0]], cand.view);
        // CR 603.4: the intervening "if" condition is checked when the event occurs.
        if let Some(pred) = &ability.intervening_if
            && !env.condition_holds(cand.view, reg, pred, &binds)
        {
            continue;
        }
        // SRS 8.2 item 8: limits per ability instance, tracked in Memory (CR 603.2h).
        if let Some(limit) = &ability.limit {
            let key = (reg.source, ability_id.as_str());
            let earlier = tally.get(&key).copied().unwrap_or(0);
            let (this_turn, this_game) = counts(pre_memory, reg.source, ability_id, turn);
            let c = (
                this_turn.saturating_add(earlier),
                this_game.saturating_add(earlier),
            );
            if !within(limit, c) {
                continue;
            }
            tally.insert(key, earlier.saturating_add(1));
            if survivors.contains(&key) {
                record(memory, reg.source, ability_id, turn, c);
            }
        }
        out.push(WaitingTrigger {
            source: reg.source,
            controller: reg.controller,
            ability_id: ability_id.clone(),
            look_back: match cand.view {
                StateView::PreEvent => cand.reg.look_back,
                StateView::PostEvent => None,
            },
            event_indices: cand.indices,
            bindings: binds,
        });
    }
    out
}
