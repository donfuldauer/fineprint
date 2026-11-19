// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Executed events normalised for trigger matching (REQ-EVENT-002, CR 603.2).

use std::collections::BTreeMap;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_dsl::ability::{EventKind, FilterValue};
use fineprint_dsl::expr::Zone;

use crate::events::{
    CounterTarget, DamageTarget, ExecutedBatch, ExecutedEvent, GameEvent, Outcome, ZoneChangeCause,
};
use crate::zones::ZoneKind;

/// A value of an event field, usable in `filters` and `bind` (REQ-EVENT-002).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventField {
    /// An object (its id in the view the field is read in).
    Object(ObjectId),
    /// A player.
    Player(PlayerId),
    /// A plain value compared with a filter value of the same variant.
    Value(FilterValue),
}

/// One occurrence of an event, normalised for matching (REQ-EVENT-002). One executed event is
/// at most one `TriggerEvent`; an event with several occurrences (CR 603.2c) arrives as several
/// executed events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriggerEvent {
    /// Every [`EventKind`] this occurrence is an instance of (matching is by membership).
    pub kinds: Vec<EventKind>,
    /// The subject object's id in the pre-event state.
    pub subject_pre: Option<ObjectId>,
    /// The subject object's id in the post-event state (a new id after a zone change).
    pub subject_post: Option<ObjectId>,
    /// The subject player.
    pub subject_player: Option<PlayerId>,
    /// Named event fields for `filters` and `bind`.
    pub fields: BTreeMap<String, EventField>,
}

impl TriggerEvent {
    /// An occurrence of `kinds` with no subject and no field.
    #[must_use]
    pub fn new(kinds: Vec<EventKind>) -> TriggerEvent {
        TriggerEvent {
            kinds,
            subject_pre: None,
            subject_post: None,
            subject_player: None,
            fields: BTreeMap::new(),
        }
    }

    /// This occurrence with the subject object's pre-event id `pre` and post-event id `post`.
    #[must_use]
    pub fn with_object(self, pre: Option<ObjectId>, post: Option<ObjectId>) -> TriggerEvent {
        TriggerEvent {
            subject_pre: pre,
            subject_post: post,
            ..self
        }
    }

    /// This occurrence with the subject player `player`.
    #[must_use]
    pub fn with_player(self, player: PlayerId) -> TriggerEvent {
        TriggerEvent {
            subject_player: Some(player),
            ..self
        }
    }

    /// This occurrence with field `key` set to `value` (replacing any earlier value).
    #[must_use]
    pub fn with_field(mut self, key: &str, value: EventField) -> TriggerEvent {
        self.fields.insert(key.to_owned(), value);
        self
    }

    /// This occurrence with the subject object `object` in both views.
    fn with_same_object(self, object: ObjectId) -> TriggerEvent {
        self.with_object(Some(object), Some(object))
    }

    /// This occurrence with field `key` set to the integer `value`.
    fn with_int(self, key: &str, value: i64) -> TriggerEvent {
        self.with_field(key, EventField::Value(FilterValue::Int(value)))
    }
}

/// The DSL zone of an engine zone kind.
fn dsl_zone(kind: ZoneKind) -> Zone {
    match kind {
        ZoneKind::Library => Zone::Library,
        ZoneKind::Hand => Zone::Hand,
        ZoneKind::Battlefield => Zone::Battlefield,
        ZoneKind::Graveyard => Zone::Graveyard,
        ZoneKind::Stack => Zone::Stack,
        ZoneKind::Exile => Zone::Exile,
        ZoneKind::Command => Zone::Command,
    }
}

/// The event kinds of a zone change from `from` to `to` with `cause` (CR 603.6a, CR 603.6c,
/// CR 603.10a, CR 701.6a). "Dies" is battlefield to graveyard.
fn zone_change_kinds(from: ZoneKind, to: ZoneKind, cause: ZoneChangeCause) -> Vec<EventKind> {
    use ZoneKind::{Battlefield, Exile, Graveyard, Hand, Library};
    let mut kinds = vec![EventKind::ZoneChange];
    let mut add = |cond: bool, kind: EventKind| {
        if cond {
            kinds.push(kind);
        }
    };
    // CR 603.6a
    add(to == Battlefield && from != Battlefield, EventKind::Enters);
    // CR 603.6c
    add(
        from == Battlefield && to != Battlefield,
        EventKind::LeavesBattlefield,
    );
    add(from == Battlefield && to == Graveyard, EventKind::Dies);
    add(
        to == Graveyard && from != Graveyard,
        EventKind::PutIntoGraveyard,
    );
    // CR 603.10a
    add(
        from == Graveyard && to != Graveyard,
        EventKind::LeavesGraveyard,
    );
    add(to == Exile && from != Exile, EventKind::Exiled);
    add(
        to == Hand && !matches!(from, Library | Hand),
        EventKind::ReturnedToHand,
    );
    let by_cause = match cause {
        ZoneChangeCause::Sacrifice => Some(EventKind::Sacrificed),
        ZoneChangeCause::Destroy => Some(EventKind::Destroyed),
        ZoneChangeCause::Discard => Some(EventKind::Discarded),
        ZoneChangeCause::Mill => Some(EventKind::Milled),
        // CR 701.6a, CR 603.10e
        ZoneChangeCause::CounterSpell => Some(EventKind::Countered),
        ZoneChangeCause::Other | ZoneChangeCause::Exile | ZoneChangeCause::StateBasedAction => None,
    };
    if let Some(kind) = by_cause {
        kinds.push(kind);
    }
    kinds
}

/// The counter fields: `amount`, and `kind` when it is a valid filter name.
fn counter_event(
    kinds: Vec<EventKind>,
    target: CounterTarget,
    kind: &str,
    amount: i64,
) -> TriggerEvent {
    let mut out = TriggerEvent::new(kinds).with_int("amount", amount);
    out = match target {
        CounterTarget::Object(o) => out.with_same_object(o),
        CounterTarget::Player(p) => out.with_player(p),
    };
    if let Ok(name) = FilterValue::name(kind) {
        out = out.with_field("kind", EventField::Value(name));
    }
    out
}

/// The `TriggerEvent` of one executed event, or `None` if no trigger can match it.
fn trigger_event(executed: &ExecutedEvent) -> Option<TriggerEvent> {
    match executed.instance.event() {
        GameEvent::ZoneChange { cause, .. } => {
            let Outcome::ZoneChange(m) = executed.outcome else {
                return None;
            };
            let (from, to) = (m.from.kind(), m.to.kind());
            Some(
                TriggerEvent::new(zone_change_kinds(from, to, *cause))
                    .with_object(Some(m.old), Some(m.new))
                    .with_field("from", EventField::Value(FilterValue::Zone(dsl_zone(from))))
                    .with_field("to", EventField::Value(FilterValue::Zone(dsl_zone(to)))),
            )
        }
        GameEvent::Draw { player } => {
            Some(TriggerEvent::new(vec![EventKind::Drawn]).with_player(*player))
        }
        GameEvent::Damage {
            source,
            target,
            amount,
            combat,
            ..
        } => {
            let recipient = match target {
                DamageTarget::Player(p) => EventField::Player(*p),
                DamageTarget::Object(o) => EventField::Object(*o),
            };
            Some(
                TriggerEvent::new(vec![EventKind::DamageDealt])
                    .with_object(*source, *source)
                    .with_field("combat", EventField::Value(FilterValue::Bool(*combat)))
                    .with_int("amount", *amount)
                    .with_field("recipient", recipient),
            )
        }
        GameEvent::LifeChange { player, delta } => {
            let kind = match delta.signum() {
                1 => EventKind::LifeGained,
                -1 => EventKind::LifeLost,
                _ => return None,
            };
            Some(
                TriggerEvent::new(vec![kind])
                    .with_player(*player)
                    .with_int("amount", delta.checked_abs().unwrap_or(i64::MAX)),
            )
        }
        GameEvent::AddCounters {
            target,
            kind,
            amount,
        } => Some(counter_event(
            vec![EventKind::CountersPut],
            *target,
            kind,
            *amount,
        )),
        GameEvent::RemoveCounters {
            target,
            kind,
            amount,
        } => {
            let removed = match executed.outcome {
                Outcome::CountersRemoved { removed } => removed,
                _ => *amount,
            };
            Some(counter_event(
                vec![EventKind::CountersRemoved],
                *target,
                kind,
                removed,
            ))
        }
        GameEvent::Tap { object } => {
            Some(TriggerEvent::new(vec![EventKind::Tapped]).with_same_object(*object))
        }
        GameEvent::Untap { object } => {
            Some(TriggerEvent::new(vec![EventKind::Untapped]).with_same_object(*object))
        }
        GameEvent::TurnFaceUp { object } => {
            Some(TriggerEvent::new(vec![EventKind::TurnedFaceUp]).with_same_object(*object))
        }
        GameEvent::CreateToken {
            owner, controller, ..
        } => {
            let token = match executed.outcome {
                Outcome::TokenCreated { object } => Some(object),
                _ => None,
            };
            Some(
                TriggerEvent::new(vec![EventKind::TokenCreated])
                    .with_object(None, token)
                    .with_player(controller.unwrap_or(*owner)),
            )
        }
        GameEvent::Attach { attachment, .. } => {
            Some(TriggerEvent::new(vec![EventKind::Attached]).with_same_object(*attachment))
        }
        // CR 603.10c
        GameEvent::Unattach { attachment } => {
            Some(TriggerEvent::new(vec![EventKind::Unattached]).with_same_object(*attachment))
        }
        // Turn structure, copy and face-down events have no event-matcher kind here.
        _ => None,
    }
}

/// Normalises one executed batch for trigger matching (REQ-EVENT-002, CR 603.2): one
/// [`TriggerEvent`] per executed event that a trigger can match, in batch order. Events with no
/// mapping (turn, phase and step beginnings, becoming a copy, turning face down, zero life
/// change) produce none.
#[must_use]
pub fn events_from_batch(batch: &ExecutedBatch) -> Vec<TriggerEvent> {
    batch.events.iter().filter_map(trigger_event).collect()
}
