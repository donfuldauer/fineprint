// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The game-event model: applied-effect sets, events, batches and execution results
//! (REQ-EVENT-001, REQ-CORE-006).

use std::collections::BTreeSet;

use fineprint_base::encode::{CanonicalEncode, put_variant};
use fineprint_base::ids::{CardId, EffectId, ObjectId, PlayerId, Timestamp};

use crate::permanent::AttachTarget;
use crate::zones::{MoveRequest, Moved, Position, ZoneId};

/// The replacement and prevention effects that have already modified an event (REQ-EVENT-001,
/// CR 614.5: a replacement effect gets only one opportunity to affect an event or any modified
/// events that may replace it).
///
/// Iterates in ascending [`EffectId`] order. Canonical encoding: the `BTreeSet` encoding (count,
/// then each id ascending).
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct AppliedEffects(BTreeSet<EffectId>);

impl AppliedEffects {
    /// An empty set.
    #[must_use]
    pub fn new() -> AppliedEffects {
        AppliedEffects::default()
    }

    /// Adds `effect`; returns `true` iff it was not already in the set.
    pub fn insert(&mut self, effect: EffectId) -> bool {
        self.0.insert(effect)
    }

    /// Whether `effect` is in the set.
    #[must_use]
    pub fn contains(&self, effect: EffectId) -> bool {
        self.0.contains(&effect)
    }

    /// The number of effects in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The effects, ascending.
    pub fn iter(&self) -> impl Iterator<Item = EffectId> + '_ {
        self.0.iter().copied()
    }
}

impl CanonicalEncode for AppliedEffects {
    /// The count, then each effect id ascending (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.0.encode_canonical(out);
    }
}

/// Why an object changes zones (REQ-EVENT-001). A label for later units (trigger matching,
/// "sacrifice"/"discard"/"mill" wording); it does not change execution.
///
/// Canonical tags: `Other` 0, `Destroy` 1, `Sacrifice` 2, `Discard` 3, `Mill` 4, `Exile` 5,
/// `CounterSpell` 6, `StateBasedAction` 7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ZoneChangeCause {
    /// Any other cause.
    Other,
    /// Destruction (CR 701.8).
    Destroy,
    /// Sacrifice (CR 701.21a).
    Sacrifice,
    /// Discard (CR 701.9a).
    Discard,
    /// Mill (CR 701.17a).
    Mill,
    /// Exile (CR 701.13a).
    Exile,
    /// Countering a spell (CR 701.6a).
    CounterSpell,
    /// A state-based action (CR 704.3).
    StateBasedAction,
}

impl ZoneChangeCause {
    fn tag(self) -> u8 {
        match self {
            ZoneChangeCause::Other => 0,
            ZoneChangeCause::Destroy => 1,
            ZoneChangeCause::Sacrifice => 2,
            ZoneChangeCause::Discard => 3,
            ZoneChangeCause::Mill => 4,
            ZoneChangeCause::Exile => 5,
            ZoneChangeCause::CounterSpell => 6,
            ZoneChangeCause::StateBasedAction => 7,
        }
    }
}

impl CanonicalEncode for ZoneChangeCause {
    /// The one-byte tag listed on the type.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_variant(out, self.tag());
    }
}

/// What damage is dealt to (REQ-EVENT-001, CR 120.1).
///
/// Canonical tags: `Player` 0 then the player id; `Object` 1 then the object id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DamageTarget {
    /// A player.
    Player(PlayerId),
    /// A permanent (CR 120.1a legality is the caller's check).
    Object(ObjectId),
}

impl CanonicalEncode for DamageTarget {
    /// The tag listed on the type, then the id.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            DamageTarget::Player(p) => {
                put_variant(out, 0);
                p.encode_canonical(out);
            }
            DamageTarget::Object(o) => {
                put_variant(out, 1);
                o.encode_canonical(out);
            }
        }
    }
}

/// What counters are put on or removed from (REQ-EVENT-001, CR 122.1).
///
/// Canonical tags: `Player` 0 then the player id; `Object` 1 then the object id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CounterTarget {
    /// A player.
    Player(PlayerId),
    /// An object in any zone.
    Object(ObjectId),
}

impl CanonicalEncode for CounterTarget {
    /// The tag listed on the type, then the id.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            CounterTarget::Player(p) => {
                put_variant(out, 0);
                p.encode_canonical(out);
            }
            CounterTarget::Object(o) => {
                put_variant(out, 1);
                o.encode_canonical(out);
            }
        }
    }
}

/// A phase of the turn (REQ-EVENT-001, CR 500.1).
///
/// Canonical tags: `Beginning` 0, `PrecombatMain` 1, `Combat` 2, `PostcombatMain` 3, `Ending` 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// The beginning phase.
    Beginning,
    /// The precombat main phase.
    PrecombatMain,
    /// The combat phase.
    Combat,
    /// The postcombat main phase.
    PostcombatMain,
    /// The ending phase.
    Ending,
}

impl Phase {
    fn tag(self) -> u8 {
        match self {
            Phase::Beginning => 0,
            Phase::PrecombatMain => 1,
            Phase::Combat => 2,
            Phase::PostcombatMain => 3,
            Phase::Ending => 4,
        }
    }
}

impl CanonicalEncode for Phase {
    /// The one-byte tag listed on the type.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_variant(out, self.tag());
    }
}

/// A step of a phase (REQ-EVENT-001, CR 500.1).
///
/// Canonical tags: `Untap` 0, `Upkeep` 1, `Draw` 2, `BeginningOfCombat` 3, `DeclareAttackers` 4,
/// `DeclareBlockers` 5, `FirstStrikeDamage` 6, `CombatDamage` 7, `EndOfCombat` 8, `End` 9,
/// `Cleanup` 10.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    /// The untap step.
    Untap,
    /// The upkeep step.
    Upkeep,
    /// The draw step.
    Draw,
    /// The beginning of combat step.
    BeginningOfCombat,
    /// The declare attackers step.
    DeclareAttackers,
    /// The declare blockers step.
    DeclareBlockers,
    /// The first-strike combat damage step.
    FirstStrikeDamage,
    /// The combat damage step.
    CombatDamage,
    /// The end of combat step.
    EndOfCombat,
    /// The end step.
    End,
    /// The cleanup step.
    Cleanup,
}

impl Step {
    /// The phase this step belongs to (CR 500.1): untap, upkeep and draw are in the beginning
    /// phase; beginning of combat through end of combat in the combat phase; end and cleanup in
    /// the ending phase.
    #[must_use]
    pub fn phase(self) -> Phase {
        // CR 500.1
        match self {
            Step::Untap | Step::Upkeep | Step::Draw => Phase::Beginning,
            Step::BeginningOfCombat
            | Step::DeclareAttackers
            | Step::DeclareBlockers
            | Step::FirstStrikeDamage
            | Step::CombatDamage
            | Step::EndOfCombat => Phase::Combat,
            Step::End | Step::Cleanup => Phase::Ending,
        }
    }

    fn tag(self) -> u8 {
        match self {
            Step::Untap => 0,
            Step::Upkeep => 1,
            Step::Draw => 2,
            Step::BeginningOfCombat => 3,
            Step::DeclareAttackers => 4,
            Step::DeclareBlockers => 5,
            Step::FirstStrikeDamage => 6,
            Step::CombatDamage => 7,
            Step::EndOfCombat => 8,
            Step::End => 9,
            Step::Cleanup => 10,
        }
    }
}

impl CanonicalEncode for Step {
    /// The one-byte tag listed on the type.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_variant(out, self.tag());
    }
}

/// One atomic change to the game (REQ-EVENT-001, SRS 8.1).
///
/// Every atomic change is one `GameEvent`, executed through
/// [`EventContext`](super::EventContext). Replacement and prevention (which turn an event into
/// another before it is executed) and trigger matching (which reads the executed-event log) are
/// later units.
///
/// Canonical tags (one byte, then the fields in declaration order): `ZoneChange` 0, `Draw` 1,
/// `Damage` 2, `LifeChange` 3, `AddCounters` 4, `RemoveCounters` 5, `Tap` 6, `Untap` 7,
/// `CreateToken` 8, `TurnBegins` 9, `PhaseBegins` 10, `StepBegins` 11, `Attach` 12,
/// `Unattach` 13, `BecomeCopy` 14, `TurnFaceUp` 15, `TurnFaceDown` 16. A [`MoveRequest`] is the
/// zone-kind tag, the position (`Top` 0, `Bottom` 1, `Index` 2 then the index as `u64`), the
/// face-down flag and the `Option` controller.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GameEvent {
    /// An object moves to another zone, or is repositioned within its zone (CR 400.7).
    /// Sacrifice, destroy, discard, mill, exile and countering a spell are zone changes with a
    /// [`ZoneChangeCause`].
    ZoneChange {
        /// The object.
        object: ObjectId,
        /// The destination.
        request: MoveRequest,
        /// Why it moves (recorded only).
        cause: ZoneChangeCause,
    },
    /// The player draws exactly one card (CR 121.1, CR 121.2).
    Draw {
        /// The drawing player.
        player: PlayerId,
    },
    /// Damage is dealt (CR 120).
    Damage {
        /// The source object, if any.
        source: Option<ObjectId>,
        /// The source's controller, if any (feeds the "damage dealt by" ledger stat).
        source_controller: Option<PlayerId>,
        /// The recipient.
        target: DamageTarget,
        /// The amount (CR 120.8: 0 is no damage).
        amount: i64,
        /// Whether it is combat damage (CR 120.2a).
        combat: bool,
    },
    /// A player gains (`delta > 0`) or loses (`delta < 0`) life (CR 119.3).
    LifeChange {
        /// The player.
        player: PlayerId,
        /// The change.
        delta: i64,
    },
    /// Counters are put on an object or player (CR 122.1).
    AddCounters {
        /// The recipient.
        target: CounterTarget,
        /// The counter kind (an open string).
        kind: String,
        /// How many.
        amount: i64,
    },
    /// Counters are removed from an object or player (CR 122.1).
    RemoveCounters {
        /// The holder.
        target: CounterTarget,
        /// The counter kind.
        kind: String,
        /// How many at most.
        amount: i64,
    },
    /// A permanent is tapped (CR 701.26a).
    Tap {
        /// The permanent.
        object: ObjectId,
    },
    /// A permanent is untapped (CR 701.26b).
    Untap {
        /// The permanent.
        object: ObjectId,
    },
    /// One token is created on the battlefield (CR 111.2, CR 701.7a).
    CreateToken {
        /// Its owner (the player who creates it, CR 111.2).
        owner: PlayerId,
        /// A fresh card id issued by the caller for the token.
        card: CardId,
        /// Its controller; `None` means the owner.
        controller: Option<PlayerId>,
    },
    /// A turn begins (CR 500.1).
    TurnBegins {
        /// The active player of the new turn.
        player: PlayerId,
    },
    /// A phase begins (CR 500.6).
    PhaseBegins {
        /// The phase.
        phase: Phase,
    },
    /// A step begins (CR 500.6).
    StepBegins {
        /// The step.
        step: Step,
    },
    /// A permanent is attached to a permanent or player (CR 701.3a).
    Attach {
        /// The permanent being attached.
        attachment: ObjectId,
        /// What it is attached to.
        target: AttachTarget,
    },
    /// A permanent becomes unattached (CR 701.3d).
    Unattach {
        /// The permanent.
        attachment: ObjectId,
    },
    /// A permanent becomes a copy of another object (CR 707.4).
    BecomeCopy {
        /// The permanent that becomes a copy.
        object: ObjectId,
        /// The object it copies.
        copied: ObjectId,
    },
    /// A face-down permanent is turned face up (CR 701.27b, CR 708.8).
    TurnFaceUp {
        /// The permanent.
        object: ObjectId,
    },
    /// A permanent is turned face down (CR 701.27b).
    TurnFaceDown {
        /// The permanent.
        object: ObjectId,
    },
}

/// Appends a move request: zone-kind tag, position, face-down flag, `Option` controller.
pub(super) fn encode_request(req: &MoveRequest, out: &mut Vec<u8>) {
    put_variant(out, req.to.tag());
    match req.position {
        Position::Top => put_variant(out, 0),
        Position::Bottom => put_variant(out, 1),
        Position::Index(n) => {
            put_variant(out, 2);
            // `usize` is at most 64 bits on every supported target, so this is lossless.
            (n as u64).encode_canonical(out);
        }
    }
    req.face_down.encode_canonical(out);
    req.controller.encode_canonical(out);
}

/// Appends a zone id: the zone-kind tag, then the `Option` owner of a per-player zone.
pub(super) fn encode_zone_id(zone: ZoneId, out: &mut Vec<u8>) {
    put_variant(out, zone.kind().tag());
    zone.owner().encode_canonical(out);
}

impl CanonicalEncode for GameEvent {
    /// The tag listed on the type, then the fields in declaration order (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            GameEvent::ZoneChange {
                object,
                request,
                cause,
            } => {
                put_variant(out, 0);
                object.encode_canonical(out);
                encode_request(request, out);
                cause.encode_canonical(out);
            }
            GameEvent::Draw { player } => {
                put_variant(out, 1);
                player.encode_canonical(out);
            }
            GameEvent::Damage {
                source,
                source_controller,
                target,
                amount,
                combat,
            } => {
                put_variant(out, 2);
                source.encode_canonical(out);
                source_controller.encode_canonical(out);
                target.encode_canonical(out);
                amount.encode_canonical(out);
                combat.encode_canonical(out);
            }
            GameEvent::LifeChange { player, delta } => {
                put_variant(out, 3);
                player.encode_canonical(out);
                delta.encode_canonical(out);
            }
            GameEvent::AddCounters {
                target,
                kind,
                amount,
            } => {
                put_variant(out, 4);
                target.encode_canonical(out);
                kind.encode_canonical(out);
                amount.encode_canonical(out);
            }
            GameEvent::RemoveCounters {
                target,
                kind,
                amount,
            } => {
                put_variant(out, 5);
                target.encode_canonical(out);
                kind.encode_canonical(out);
                amount.encode_canonical(out);
            }
            GameEvent::Tap { object } => {
                put_variant(out, 6);
                object.encode_canonical(out);
            }
            GameEvent::Untap { object } => {
                put_variant(out, 7);
                object.encode_canonical(out);
            }
            GameEvent::CreateToken {
                owner,
                card,
                controller,
            } => {
                put_variant(out, 8);
                owner.encode_canonical(out);
                card.encode_canonical(out);
                controller.encode_canonical(out);
            }
            GameEvent::TurnBegins { player } => {
                put_variant(out, 9);
                player.encode_canonical(out);
            }
            GameEvent::PhaseBegins { phase } => {
                put_variant(out, 10);
                phase.encode_canonical(out);
            }
            GameEvent::StepBegins { step } => {
                put_variant(out, 11);
                step.encode_canonical(out);
            }
            GameEvent::Attach { attachment, target } => {
                put_variant(out, 12);
                attachment.encode_canonical(out);
                target.encode_canonical(out);
            }
            GameEvent::Unattach { attachment } => {
                put_variant(out, 13);
                attachment.encode_canonical(out);
            }
            GameEvent::BecomeCopy { object, copied } => {
                put_variant(out, 14);
                object.encode_canonical(out);
                copied.encode_canonical(out);
            }
            GameEvent::TurnFaceUp { object } => {
                put_variant(out, 15);
                object.encode_canonical(out);
            }
            GameEvent::TurnFaceDown { object } => {
                put_variant(out, 16);
                object.encode_canonical(out);
            }
        }
    }
}

/// An event together with the replacement and prevention effects that already modified it
/// (REQ-EVENT-001, CR 614.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventInstance {
    event: GameEvent,
    applied: AppliedEffects,
}

impl EventInstance {
    /// `event` with an empty applied-effects set.
    #[must_use]
    pub fn new(event: GameEvent) -> EventInstance {
        EventInstance {
            event,
            applied: AppliedEffects::new(),
        }
    }

    /// This instance with its applied-effects set replaced by `applied`.
    #[must_use]
    pub fn with_applied(self, applied: AppliedEffects) -> EventInstance {
        EventInstance { applied, ..self }
    }

    /// The event.
    #[must_use]
    pub fn event(&self) -> &GameEvent {
        &self.event
    }

    /// The effects that already modified the event (CR 614.5).
    #[must_use]
    pub fn applied_effects(&self) -> &AppliedEffects {
        &self.applied
    }
}

impl From<GameEvent> for EventInstance {
    fn from(event: GameEvent) -> EventInstance {
        EventInstance::new(event)
    }
}

impl CanonicalEncode for EventInstance {
    /// The event, then the applied-effects set (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.event.encode_canonical(out);
        self.applied.encode_canonical(out);
    }
}

/// One set of simultaneous events, in a fixed submission order (REQ-EVENT-001, CR 704.3,
/// CR 703.4m). Executed all-or-nothing by
/// [`EventContext::execute_batch`](super::EventContext::execute_batch).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventBatch {
    events: Vec<EventInstance>,
}

impl EventBatch {
    /// A batch of `events`, in this order.
    #[must_use]
    pub fn new(events: Vec<EventInstance>) -> EventBatch {
        EventBatch { events }
    }

    /// A batch of `events`, each with an empty applied-effects set.
    #[must_use]
    pub fn from_events(events: Vec<GameEvent>) -> EventBatch {
        EventBatch::new(events.into_iter().map(EventInstance::new).collect())
    }

    /// The events, in submission order.
    #[must_use]
    pub fn events(&self) -> &[EventInstance] {
        &self.events
    }

    /// The number of events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the batch holds no event.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// What executing one event produced (REQ-EVENT-001).
///
/// Canonical tags: `ZoneChange` 0 then old id, new id, from zone, to zone (each zone a
/// zone-kind tag and `Option` owner); `TokenCreated` 1 then the object id; `CountersRemoved` 2
/// then the amount; `Attached` 3 then the timestamp; `TurnBegan` 4 then the turn number;
/// `Applied` 5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A zone change or draw: the old and new ids and both zones (CR 400.7). `old == new` for a
    /// same-zone reposition.
    ZoneChange(Moved),
    /// A token was created as this object.
    TokenCreated {
        /// The token's object id.
        object: ObjectId,
    },
    /// Counters were removed.
    CountersRemoved {
        /// The number actually removed.
        removed: i64,
    },
    /// An attachment happened and received this new timestamp (CR 701.3c).
    Attached {
        /// The new timestamp.
        timestamp: Timestamp,
    },
    /// A turn began.
    TurnBegan {
        /// The new turn number.
        turn: u64,
    },
    /// The event was applied; nothing further to report.
    Applied,
}

impl CanonicalEncode for Outcome {
    /// The tag listed on the type, then its fields (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Outcome::ZoneChange(m) => {
                put_variant(out, 0);
                m.old.encode_canonical(out);
                m.new.encode_canonical(out);
                encode_zone_id(m.from, out);
                encode_zone_id(m.to, out);
            }
            Outcome::TokenCreated { object } => {
                put_variant(out, 1);
                object.encode_canonical(out);
            }
            Outcome::CountersRemoved { removed } => {
                put_variant(out, 2);
                removed.encode_canonical(out);
            }
            Outcome::Attached { timestamp } => {
                put_variant(out, 3);
                timestamp.encode_canonical(out);
            }
            Outcome::TurnBegan { turn } => {
                put_variant(out, 4);
                turn.encode_canonical(out);
            }
            Outcome::Applied => put_variant(out, 5),
        }
    }
}

/// One event that actually occurred (REQ-EVENT-001): what trigger detection reads (CR 603.2,
/// CR 603.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutedEvent {
    /// The event as submitted, with its applied effects.
    pub instance: EventInstance,
    /// What it produced.
    pub outcome: Outcome,
    /// Whether last known information was stored for the departing object (the record is in
    /// the `LkiStore` under the old id).
    pub lki_captured: bool,
}

fineprint_base::impl_canonical_struct!(ExecutedEvent {
    instance,
    outcome,
    lki_captured
});

/// One executed batch of simultaneous events (REQ-EVENT-001).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutedBatch {
    /// The position of this batch in the log, counting logged batches from 0.
    pub index: u64,
    /// The game turn number when the batch finished executing.
    pub turn: u64,
    /// The events that actually occurred, in submission order.
    pub events: Vec<ExecutedEvent>,
}

fineprint_base::impl_canonical_struct!(ExecutedBatch {
    index,
    turn,
    events
});
