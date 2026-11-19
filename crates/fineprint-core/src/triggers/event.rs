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
use crate::memory::LkiStore;
use crate::zones::{Moved, ZoneKind};

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
/// zero or more `TriggerEvent`s (damage dealt to a player is also its life loss, CR 120.3a);
/// an event with several occurrences (CR 603.2c) arrives as several executed events.
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
/// CR 603.10a, CR 701.6a). "Dies" is battlefield to graveyard. An exiled object exiled again
/// is only `EXILED`: it becomes a new object without changing zones (CR 400.8, CR 406.7).
fn zone_change_kinds(from: ZoneKind, to: ZoneKind, cause: ZoneChangeCause) -> Vec<EventKind> {
    let mut kinds = if from == ZoneKind::Exile && to == ZoneKind::Exile {
        // CR 400.8, CR 406.7
        vec![EventKind::Exiled]
    } else {
        moved_zone_kinds(from, to)
    };
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

/// The zone-derived kinds of a move from `from` to `to` that is not exile to exile (CR 603.6a,
/// CR 603.6c, CR 603.10a).
fn moved_zone_kinds(from: ZoneKind, to: ZoneKind) -> Vec<EventKind> {
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
    kinds
}

/// The counter fields: `amount`, and `kind` when it is a valid filter name or, failing that, a
/// zone name (the DSL decodes a zone-named filter value as a zone, so `HAND` is kept as
/// [`FilterValue::Zone`]); `ANY`, empty and other invalid kinds are omitted.
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
    let value = match FilterValue::name(kind) {
        Ok(name) => Some(name),
        Err(_) => Zone::from_wire(kind).map(FilterValue::Zone),
    };
    if let Some(value) = value {
        out = out.with_field("kind", EventField::Value(value));
    }
    out
}

/// The zone-change occurrence of a move `m` with `kinds`: pre-event and post-event ids and the
/// `from`/`to` zone fields.
fn moved_event(kinds: Vec<EventKind>, m: &Moved) -> TriggerEvent {
    TriggerEvent::new(kinds)
        .with_object(Some(m.old), Some(m.new))
        .with_field(
            "from",
            EventField::Value(FilterValue::Zone(dsl_zone(m.from.kind()))),
        )
        .with_field(
            "to",
            EventField::Value(FilterValue::Zone(dsl_zone(m.to.kind()))),
        )
}

/// Appends the `TriggerEvent`s of one executed event to `out` (none if no trigger can match
/// it). `lki` supplies the sacrificing controller (CR 701.21a).
fn trigger_events(executed: &ExecutedEvent, lki: &LkiStore, out: &mut Vec<TriggerEvent>) {
    let event = match executed.instance.event() {
        GameEvent::ZoneChange { cause, .. } => {
            let Outcome::ZoneChange(m) = &executed.outcome else {
                return;
            };
            // CR 400.7: a same-zone reposition keeps its id and is no zone change (exile and
            // command-zone re-entries get a new id, CR 400.8, CR 406.7, CR 400.10).
            if m.old == m.new {
                return;
            }
            let mut e = moved_event(zone_change_kinds(m.from.kind(), m.to.kind(), *cause), m);
            // CR 701.21a, CR 603.10a: the player who sacrificed it is its controller as it
            // last existed on the battlefield.
            match cause {
                ZoneChangeCause::Sacrifice => {
                    e.subject_player = lki.get(m.old).and_then(|r| r.controller());
                }
                // CR 701.9a, CR 701.17a: the player who discarded or milled the card owns the
                // hand or library it left.
                ZoneChangeCause::Discard | ZoneChangeCause::Mill => {
                    e.subject_player = m.from.owner();
                }
                _ => {}
            }
            e
        }
        // CR 121.1: a draw puts the top card of the library into the hand, a zone change too.
        GameEvent::Draw { player } => match &executed.outcome {
            Outcome::ZoneChange(m) if m.old != m.new => {
                let mut kinds = vec![EventKind::Drawn];
                kinds.extend(zone_change_kinds(
                    m.from.kind(),
                    m.to.kind(),
                    ZoneChangeCause::Other,
                ));
                moved_event(kinds, m).with_player(*player)
            }
            _ => TriggerEvent::new(vec![EventKind::Drawn]).with_player(*player),
        },
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
            out.push(
                TriggerEvent::new(vec![EventKind::DamageDealt])
                    .with_object(*source, *source)
                    .with_field("combat", EventField::Value(FilterValue::Bool(*combat)))
                    .with_int("amount", *amount)
                    .with_field("recipient", recipient.clone()),
            );
            // CR 120.3a, CR 120.4b/c: damage dealt to a player is then processed into that
            // much life loss, a second event. CR 120.8: 0 damage is no damage. Infect
            // (CR 120.3b) is not modelled: the executed event carries no infect flag and the
            // executor always applies life loss.
            match target {
                DamageTarget::Player(p) if *amount > 0 => {
                    TriggerEvent::new(vec![EventKind::LifeLost])
                        .with_player(*p)
                        .with_int("amount", *amount)
                        .with_object(*source, *source)
                        .with_field("recipient", recipient)
                }
                _ => return,
            }
        }
        GameEvent::LifeChange { player, delta } => {
            let kind = match delta.signum() {
                1 => EventKind::LifeGained,
                -1 => EventKind::LifeLost,
                _ => return,
            };
            TriggerEvent::new(vec![kind])
                .with_player(*player)
                .with_int("amount", delta.checked_abs().unwrap_or(i64::MAX))
        }
        GameEvent::AddCounters {
            target,
            kind,
            amount,
        } => counter_event(vec![EventKind::CountersPut], *target, kind, *amount),
        GameEvent::RemoveCounters {
            target,
            kind,
            amount,
        } => {
            let removed = match executed.outcome {
                Outcome::CountersRemoved { removed } => removed,
                _ => *amount,
            };
            counter_event(vec![EventKind::CountersRemoved], *target, kind, removed)
        }
        GameEvent::Tap { object } => {
            TriggerEvent::new(vec![EventKind::Tapped]).with_same_object(*object)
        }
        GameEvent::Untap { object } => {
            TriggerEvent::new(vec![EventKind::Untapped]).with_same_object(*object)
        }
        GameEvent::TurnFaceUp { object } => {
            TriggerEvent::new(vec![EventKind::TurnedFaceUp]).with_same_object(*object)
        }
        // CR 111.2: the creator is the owner; CR 701.7a, CR 603.6a: the token enters the
        // battlefield (`to` is the battlefield; from no zone, so no `from` and no ZONE_CHANGE).
        GameEvent::CreateToken {
            owner, controller, ..
        } => {
            let token = match executed.outcome {
                Outcome::TokenCreated { object } => Some(object),
                _ => None,
            };
            TriggerEvent::new(vec![EventKind::TokenCreated, EventKind::Enters])
                .with_object(None, token)
                .with_player(*owner)
                .with_field(
                    "to",
                    EventField::Value(FilterValue::Zone(Zone::Battlefield)),
                )
                .with_field(
                    "controller",
                    EventField::Player(controller.unwrap_or(*owner)),
                )
        }
        GameEvent::Attach { attachment, .. } => {
            TriggerEvent::new(vec![EventKind::Attached]).with_same_object(*attachment)
        }
        // CR 603.10c
        GameEvent::Unattach { attachment } => {
            TriggerEvent::new(vec![EventKind::Unattached]).with_same_object(*attachment)
        }
        // Turn structure, copy and face-down events have no event-matcher kind here.
        _ => return,
    };
    out.push(event);
}

/// Normalises one executed batch for trigger matching (REQ-EVENT-002, CR 603.2):
/// [`events_from_batch_with_lki`] with an empty [`LkiStore`], so a sacrifice has no subject
/// player.
#[must_use]
pub fn events_from_batch(batch: &ExecutedBatch) -> Vec<TriggerEvent> {
    events_from_batch_with_lki(batch, &LkiStore::new())
}

/// Normalises one executed batch for trigger matching (REQ-EVENT-002, CR 603.2): zero, one or
/// more [`TriggerEvent`]s per executed event, in batch order (the indices of the result are
/// the `event_indices` of [`WaitingTrigger`](super::WaitingTrigger)). Events with no mapping
/// (turn, phase and step beginnings, becoming a copy, turning face down, zero life change) and
/// same-zone repositions that keep their id (CR 400.7) produce none; damage dealt to a player
/// produces the damage occurrence followed by its life-loss occurrence (CR 120.3a). `lki` is
/// the store the batch captured into: a sacrifice's subject player is the sacrificed object's
/// last known controller (CR 701.21a); without a record it is `None`.
///
/// **Limit.** Infect (CR 120.3b) is not modelled: an executed damage event carries no infect
/// flag, so damage to a player always also yields `LIFE_LOST`.
#[must_use]
pub fn events_from_batch_with_lki(batch: &ExecutedBatch, lki: &LkiStore) -> Vec<TriggerEvent> {
    let mut out = Vec::new();
    for executed in &batch.events {
        trigger_events(executed, lki, &mut out);
    }
    out
}
