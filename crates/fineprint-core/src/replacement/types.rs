// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The replacement and prevention effect model: event kinds, precedence tiers, matchers,
//! actions, shields, effects and the registry (REQ-EVENT-001, REQ-DSL-009).

use core::fmt;
use std::collections::BTreeMap;

use fineprint_base::encode::{CanonicalEncode, put_variant};
use fineprint_base::ids::{EffectId, ObjectId, PlayerId};
use fineprint_dsl::ability::TierHint;

use crate::choice::{ChoiceError, Interrupt, PendingChoiceRequest};
use crate::events::{CounterTarget, DamageTarget, EventError, GameEvent, ZoneChangeCause};
use crate::permanent::AttachTarget;
use crate::zones::ZoneKind;

/// The kind of a [`GameEvent`], as a replacement effect's matcher names it (REQ-EVENT-001).
///
/// Named `BusEventKind` to avoid a clash with the DSL's own `EventKind`. Canonical tag: the
/// declaration index (`ZoneChange` 0 ... `TurnFaceDown` 16), equal to the [`GameEvent`] tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BusEventKind {
    /// [`GameEvent::ZoneChange`].
    ZoneChange,
    /// [`GameEvent::Draw`].
    Draw,
    /// [`GameEvent::Damage`].
    Damage,
    /// [`GameEvent::LifeChange`].
    LifeChange,
    /// [`GameEvent::AddCounters`].
    AddCounters,
    /// [`GameEvent::RemoveCounters`].
    RemoveCounters,
    /// [`GameEvent::Tap`].
    Tap,
    /// [`GameEvent::Untap`].
    Untap,
    /// [`GameEvent::CreateToken`].
    CreateToken,
    /// [`GameEvent::TurnBegins`].
    TurnBegins,
    /// [`GameEvent::PhaseBegins`].
    PhaseBegins,
    /// [`GameEvent::StepBegins`].
    StepBegins,
    /// [`GameEvent::Attach`].
    Attach,
    /// [`GameEvent::Unattach`].
    Unattach,
    /// [`GameEvent::BecomeCopy`].
    BecomeCopy,
    /// [`GameEvent::TurnFaceUp`].
    TurnFaceUp,
    /// [`GameEvent::TurnFaceDown`].
    TurnFaceDown,
}

impl BusEventKind {
    /// The kind of `event`.
    #[must_use]
    pub fn of(event: &GameEvent) -> BusEventKind {
        match event {
            GameEvent::ZoneChange { .. } => BusEventKind::ZoneChange,
            GameEvent::Draw { .. } => BusEventKind::Draw,
            GameEvent::Damage { .. } => BusEventKind::Damage,
            GameEvent::LifeChange { .. } => BusEventKind::LifeChange,
            GameEvent::AddCounters { .. } => BusEventKind::AddCounters,
            GameEvent::RemoveCounters { .. } => BusEventKind::RemoveCounters,
            GameEvent::Tap { .. } => BusEventKind::Tap,
            GameEvent::Untap { .. } => BusEventKind::Untap,
            GameEvent::CreateToken { .. } => BusEventKind::CreateToken,
            GameEvent::TurnBegins { .. } => BusEventKind::TurnBegins,
            GameEvent::PhaseBegins { .. } => BusEventKind::PhaseBegins,
            GameEvent::StepBegins { .. } => BusEventKind::StepBegins,
            GameEvent::Attach { .. } => BusEventKind::Attach,
            GameEvent::Unattach { .. } => BusEventKind::Unattach,
            GameEvent::BecomeCopy { .. } => BusEventKind::BecomeCopy,
            GameEvent::TurnFaceUp { .. } => BusEventKind::TurnFaceUp,
            GameEvent::TurnFaceDown { .. } => BusEventKind::TurnFaceDown,
        }
    }

    /// The wire name (`"ZONE_CHANGE"`, `"DRAW"`, ...), used as the `event` of a
    /// `CHOOSE_REPLACEMENT_EFFECT` payload.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            BusEventKind::ZoneChange => "ZONE_CHANGE",
            BusEventKind::Draw => "DRAW",
            BusEventKind::Damage => "DAMAGE",
            BusEventKind::LifeChange => "LIFE_CHANGE",
            BusEventKind::AddCounters => "ADD_COUNTERS",
            BusEventKind::RemoveCounters => "REMOVE_COUNTERS",
            BusEventKind::Tap => "TAP",
            BusEventKind::Untap => "UNTAP",
            BusEventKind::CreateToken => "CREATE_TOKEN",
            BusEventKind::TurnBegins => "TURN_BEGINS",
            BusEventKind::PhaseBegins => "PHASE_BEGINS",
            BusEventKind::StepBegins => "STEP_BEGINS",
            BusEventKind::Attach => "ATTACH",
            BusEventKind::Unattach => "UNATTACH",
            BusEventKind::BecomeCopy => "BECOME_COPY",
            BusEventKind::TurnFaceUp => "TURN_FACE_UP",
            BusEventKind::TurnFaceDown => "TURN_FACE_DOWN",
        }
    }

    fn tag(self) -> u8 {
        self as u8
    }
}

impl CanonicalEncode for BusEventKind {
    /// The one-byte declaration index.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_variant(out, self.tag());
    }
}

/// The precedence tier of a replacement effect (REQ-EVENT-001, CR 616.1a-e). The derive order is
/// the precedence: the first variant is the highest.
///
/// Canonical tag: the declaration index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// CR 616.1a: self-replacement effects (CR 614.15).
    SelfReplacement,
    /// CR 616.1b: effects changing under whose control the object enters.
    ControlEtb,
    /// CR 616.1c: effects making the object enter as a copy.
    CopyEtb,
    /// CR 616.1d: effects making a card enter with its back face up.
    BackFaceEtb,
    /// CR 616.1e: any other applicable effect.
    Other,
}

impl CanonicalEncode for Tier {
    /// The one-byte declaration index.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_variant(out, *self as u8);
    }
}

/// The tier of a DSL replacement ability (REQ-DSL-009, CR 616.1): `Some(true)` self-replacement
/// (CR 614.15 -> CR 616.1a) wins over a hint; otherwise the hint's same-named tier
/// (CR 616.1b-d); otherwise [`Tier::Other`] (CR 616.1e).
#[must_use]
pub fn tier_from_ability(is_self_replacement: Option<bool>, hint: Option<TierHint>) -> Tier {
    if is_self_replacement == Some(true) {
        // CR 614.15, CR 616.1a
        return Tier::SelfReplacement;
    }
    match hint {
        Some(TierHint::ControlEtb) => Tier::ControlEtb,
        Some(TierHint::CopyEtb) => Tier::CopyEtb,
        Some(TierHint::BackFaceEtb) => Tier::BackFaceEtb,
        None => Tier::Other,
    }
}

/// What events a replacement effect applies to (REQ-EVENT-001). Every `Some` field must hold
/// (logical AND), evaluated against the current state and the event as it is now (after earlier
/// applications). `kind: None` never matches. A field that does not apply to the event's kind
/// makes the matcher not match.
///
/// Affected object / player per kind: `ZoneChange`, `Tap`, `Untap`, `TurnFaceUp`,
/// `TurnFaceDown`, `BecomeCopy`: the object; `Attach`/`Unattach`: the attachment; `Damage`,
/// `AddCounters`, `RemoveCounters`: the target (object or player); `Draw`, `LifeChange`,
/// `TurnBegins`: the player; `CreateToken`: the owner (player); `PhaseBegins`/`StepBegins`:
/// neither.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventMatcher {
    /// The event kind; `None` never matches.
    pub kind: Option<BusEventKind>,
    /// The event's affected object equals this.
    pub affected_object: Option<ObjectId>,
    /// The event's affected player equals this.
    pub affected_player: Option<PlayerId>,
    /// `ZoneChange` only: the kind of the zone the object is in now.
    pub from: Option<ZoneKind>,
    /// `ZoneChange`: the current destination; `CreateToken` counts as the battlefield.
    pub to: Option<ZoneKind>,
    /// `ZoneChange` only: the cause.
    pub cause: Option<ZoneChangeCause>,
    /// `Damage` only: whether it is combat damage.
    pub combat: Option<bool>,
    /// `AddCounters`/`RemoveCounters` only: the counter kind.
    pub counter_kind: Option<String>,
    /// The `Damage`/`AddCounters`/`RemoveCounters` amount, or the `LifeChange` magnitude, is
    /// below this.
    pub amount_below: Option<i64>,
    /// That object currently has at least one counter of that kind (counter-backed effects).
    pub requires_counter: Option<(ObjectId, String)>,
}

impl EventMatcher {
    /// A matcher for every event of `kind`.
    #[must_use]
    pub fn new(kind: BusEventKind) -> EventMatcher {
        EventMatcher {
            kind: Some(kind),
            ..EventMatcher::default()
        }
    }
}

/// Appends an `Option` zone kind: `0x00`, or `0x01` then the zone-kind tag.
fn encode_zone_kind(zone: Option<ZoneKind>, out: &mut Vec<u8>) {
    match zone {
        None => put_variant(out, 0),
        Some(z) => {
            put_variant(out, 1);
            put_variant(out, z.tag());
        }
    }
}

impl CanonicalEncode for EventMatcher {
    /// The fields in declaration order; zone kinds as `Option` of their one-byte tag.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.kind.encode_canonical(out);
        self.affected_object.encode_canonical(out);
        self.affected_player.encode_canonical(out);
        encode_zone_kind(self.from, out);
        encode_zone_kind(self.to, out);
        self.cause.encode_canonical(out);
        self.combat.encode_canonical(out);
        self.counter_kind.encode_canonical(out);
        self.amount_below.encode_canonical(out);
        self.requires_counter.encode_canonical(out);
    }
}

/// How an amount modification changes an amount (REQ-EVENT-001). The result saturates and is
/// never below 0; `Mul` requires a factor of at least 0.
///
/// Canonical tags: `Add` 0, `Mul` 1, `Set` 2, then the operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmountOp {
    /// Adds `n` (may be negative).
    Add(i64),
    /// Multiplies by `n` (at least 0).
    Mul(i64),
    /// Sets the amount to `n`.
    Set(i64),
}

impl AmountOp {
    /// `amount` after this operation: saturating, at least 0.
    #[must_use]
    pub fn apply(self, amount: i64) -> i64 {
        match self {
            AmountOp::Add(n) => amount.saturating_add(n),
            AmountOp::Mul(n) => amount.saturating_mul(n),
            AmountOp::Set(n) => n,
        }
        .max(0)
    }
}

impl CanonicalEncode for AmountOp {
    /// The tag listed on the type, then the operand.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        let (tag, n) = match self {
            AmountOp::Add(n) => (0, n),
            AmountOp::Mul(n) => (1, n),
            AmountOp::Set(n) => (2, n),
        };
        put_variant(out, tag);
        n.encode_canonical(out);
    }
}

/// A rewrite of an event (REQ-EVENT-001, CR 614.6).
///
/// Canonical tags: `SetDestination` 0, `SetController` 1, `SetFaceDown` 2, `Amount` 3,
/// `EntersAsCopyOf` 4, then the operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modification {
    /// `ZoneChange`: the destination zone kind; the rest of the request is kept, except that
    /// the controller is cleared for a zone other than the battlefield and the stack.
    SetDestination(ZoneKind),
    /// `ZoneChange`/`CreateToken`: the controller it enters under.
    SetController(PlayerId),
    /// `ZoneChange`: whether it enters face down.
    SetFaceDown(bool),
    /// `Damage`/`AddCounters`/`RemoveCounters`: the amount; `LifeChange`: the magnitude (sign
    /// kept). A result of 0 drops the event.
    Amount(AmountOp),
    /// `ZoneChange` to the battlefield or `CreateToken` (CR 614.1c): the new object becomes a
    /// copy as part of its entry; the `BecomeCopy` is executed right after the entry, as its
    /// own batch, without replacement collection.
    EntersAsCopyOf(ObjectId),
}

impl CanonicalEncode for Modification {
    /// The tag listed on the type, then the operand.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Modification::SetDestination(z) => {
                put_variant(out, 0);
                put_variant(out, z.tag());
            }
            Modification::SetController(p) => {
                put_variant(out, 1);
                p.encode_canonical(out);
            }
            Modification::SetFaceDown(b) => {
                put_variant(out, 2);
                b.encode_canonical(out);
            }
            Modification::Amount(op) => {
                put_variant(out, 3);
                op.encode_canonical(out);
            }
            Modification::EntersAsCopyOf(o) => {
                put_variant(out, 4);
                o.encode_canonical(out);
            }
        }
    }
}

/// What a replacement or prevention effect does to the event (REQ-EVENT-001, CR 614.6,
/// CR 615.6).
///
/// Canonical tags: `Modify` 0, `Skip` 1, `Instead` 2, `Prevent` 3, then the fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Rewrites the event.
    Modify(Modification),
    /// The event does not happen.
    Skip,
    /// The event does not happen; these child events are submitted to the bus as the
    /// modified event, keeping its applied effects (CR 614.5, CR 614.6).
    Instead(Vec<GameEvent>),
    /// `Damage` only (CR 615.6): prevents all the damage (or, with [`Shield::Pool`], up to the
    /// pool, CR 615.7); the `then` child events are new events submitted to the bus
    /// (CR 615.5).
    Prevent {
        /// Child events submitted after the batch executes.
        then: Vec<GameEvent>,
    },
}

impl CanonicalEncode for Action {
    /// The tag listed on the type, then the fields.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Action::Modify(m) => {
                put_variant(out, 0);
                m.encode_canonical(out);
            }
            Action::Skip => put_variant(out, 1),
            Action::Instead(events) => {
                put_variant(out, 2);
                events.encode_canonical(out);
            }
            Action::Prevent { then } => {
                put_variant(out, 3);
                then.encode_canonical(out);
            }
        }
    }
}

/// A use limit (REQ-EVENT-001, CR 615.7, CR 615.8): the remaining uses or the remaining
/// damage pool.
///
/// Canonical tags: `Uses` 0 then the `u32`; `Pool` 1 then the `i64`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shield {
    /// One use per application; the effect is removed from the registry at 0.
    Uses(u32),
    /// With [`Action::Prevent`] only: prevents up to this much damage in all; removed at 0.
    Pool(i64),
}

impl CanonicalEncode for Shield {
    /// The tag listed on the type, then the remaining amount.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            Shield::Uses(n) => {
                put_variant(out, 0);
                n.encode_canonical(out);
            }
            Shield::Pool(n) => {
                put_variant(out, 1);
                n.encode_canonical(out);
            }
        }
    }
}

/// One replacement or prevention effect in force (REQ-EVENT-001, CR 614.6, CR 615.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacementEffect {
    /// Its id; the registry orders by it.
    pub id: EffectId,
    /// The object that generates it, if any.
    pub source: Option<ObjectId>,
    /// The display description offered to the chooser.
    pub description: String,
    /// Its precedence tier (CR 616.1a-e).
    pub tier: Tier,
    /// What events it applies to.
    pub matcher: EventMatcher,
    /// Whether it applies to an event contained in another (CR 616.1g).
    pub contained: bool,
    /// What it does.
    pub action: Action,
    /// Whether it may apply again to an event it already modified (an exception to CR 614.5,
    /// as CR 903.9b).
    pub overrides_614_5: bool,
    /// Its remaining use limit, if any.
    pub shield: Option<Shield>,
}

fineprint_base::impl_canonical_struct!(ReplacementEffect {
    id,
    source,
    description,
    tier,
    matcher,
    contained,
    action,
    overrides_614_5,
    shield
});

const SHIELD: &str = "shield";
const STUN: &str = "stun";
const FINALITY: &str = "finality";

fn remove_one(object: ObjectId, kind: &str) -> GameEvent {
    GameEvent::RemoveCounters {
        target: CounterTarget::Object(object),
        kind: kind.to_owned(),
        amount: 1,
    }
}

impl ReplacementEffect {
    /// An effect of tier [`Tier::Other`], no source, description `"effect <id>"`, not contained,
    /// no 614.5 override, no shield.
    #[must_use]
    pub fn new(id: EffectId, matcher: EventMatcher, action: Action) -> ReplacementEffect {
        ReplacementEffect {
            id,
            source: None,
            description: format!("effect {}", id.get()),
            tier: Tier::Other,
            matcher,
            contained: false,
            action,
            overrides_614_5: false,
            shield: None,
        }
    }

    /// This effect with tier `tier`.
    #[must_use]
    pub fn with_tier(self, tier: Tier) -> Self {
        ReplacementEffect { tier, ..self }
    }

    /// This effect with source `source`.
    #[must_use]
    pub fn with_source(self, source: ObjectId) -> Self {
        ReplacementEffect {
            source: Some(source),
            ..self
        }
    }

    /// This effect with description `description`.
    #[must_use]
    pub fn with_description(self, description: impl Into<String>) -> Self {
        ReplacementEffect {
            description: description.into(),
            ..self
        }
    }

    /// This effect with the contained flag `contained` (CR 616.1g).
    #[must_use]
    pub fn with_contained(self, contained: bool) -> Self {
        ReplacementEffect { contained, ..self }
    }

    /// This effect with the CR 614.5 override flag `overrides` (CR 903.9b-style).
    #[must_use]
    pub fn with_overrides_614_5(self, overrides: bool) -> Self {
        ReplacementEffect {
            overrides_614_5: overrides,
            ..self
        }
    }

    /// This effect with shield `shield`.
    #[must_use]
    pub fn with_shield(self, shield: Shield) -> Self {
        ReplacementEffect {
            shield: Some(shield),
            ..self
        }
    }

    /// The destroy half of a shield counter (CR 122.1c): if `object` would be destroyed as the
    /// result of an effect (cause `Destroy`), instead remove a shield counter from it. A
    /// state-based destruction (cause `StateBasedAction`, CR 701.8b, CR 704.1) is not replaced.
    #[must_use]
    pub fn shield_counter_destroy(id: EffectId, object: ObjectId) -> Self {
        // CR 122.1c
        let matcher = EventMatcher {
            affected_object: Some(object),
            cause: Some(ZoneChangeCause::Destroy),
            requires_counter: Some((object, SHIELD.to_owned())),
            ..EventMatcher::new(BusEventKind::ZoneChange)
        };
        ReplacementEffect::new(
            id,
            matcher,
            Action::Instead(vec![remove_one(object, SHIELD)]),
        )
        .with_source(object)
        .with_description("shield counter: destruction")
    }

    /// The damage half of a shield counter (CR 122.1c, CR 615.1): if damage would be dealt to
    /// `object`, prevent it and remove a shield counter from it.
    #[must_use]
    pub fn shield_counter_damage(id: EffectId, object: ObjectId) -> Self {
        // CR 122.1c, CR 615.1
        let matcher = EventMatcher {
            affected_object: Some(object),
            requires_counter: Some((object, SHIELD.to_owned())),
            ..EventMatcher::new(BusEventKind::Damage)
        };
        ReplacementEffect::new(
            id,
            matcher,
            Action::Prevent {
                then: vec![remove_one(object, SHIELD)],
            },
        )
        .with_source(object)
        .with_description("shield counter: damage")
    }

    /// A stun counter (CR 122.1d): if `object` would become untapped, instead remove a stun
    /// counter from it. An untap of an untapped permanent is no event (CR 701.26b), so the bus
    /// offers no effect for it and the counter stays.
    #[must_use]
    pub fn stun_counter(id: EffectId, object: ObjectId) -> Self {
        // CR 122.1d
        let matcher = EventMatcher {
            affected_object: Some(object),
            requires_counter: Some((object, STUN.to_owned())),
            ..EventMatcher::new(BusEventKind::Untap)
        };
        ReplacementEffect::new(id, matcher, Action::Instead(vec![remove_one(object, STUN)]))
            .with_source(object)
            .with_description("stun counter")
    }

    /// A finality counter (CR 122.1h): if `object` would be put into a graveyard from the
    /// battlefield, exile it instead.
    #[must_use]
    pub fn finality_counter(id: EffectId, object: ObjectId) -> Self {
        // CR 122.1h
        let matcher = EventMatcher {
            affected_object: Some(object),
            from: Some(ZoneKind::Battlefield),
            to: Some(ZoneKind::Graveyard),
            requires_counter: Some((object, FINALITY.to_owned())),
            ..EventMatcher::new(BusEventKind::ZoneChange)
        };
        ReplacementEffect::new(
            id,
            matcher,
            Action::Modify(Modification::SetDestination(ZoneKind::Exile)),
        )
        .with_source(object)
        .with_description("finality counter")
    }

    /// Why this effect cannot be registered, if it cannot.
    fn invalid_reason(&self) -> Option<String> {
        let id = self.id.get();
        let Some(kind) = self.matcher.kind else {
            return Some(format!("effect {id}: the matcher has no event kind"));
        };
        let is_damage = kind == BusEventKind::Damage;
        if matches!(self.action, Action::Prevent { .. }) && !is_damage {
            return Some(format!(
                "effect {id}: prevention applies only to DAMAGE, not {}",
                kind.name()
            ));
        }
        match self.shield {
            Some(Shield::Uses(0)) => {
                return Some(format!("effect {id}: a use shield needs at least 1 use"));
            }
            Some(Shield::Pool(n)) => {
                if n <= 0 {
                    return Some(format!("effect {id}: a pool shield needs at least 1"));
                }
                if !is_damage || !matches!(self.action, Action::Prevent { .. }) {
                    return Some(format!(
                        "effect {id}: a pool shield needs a DAMAGE matcher and a PREVENT action"
                    ));
                }
            }
            _ => {}
        }
        if let Action::Modify(m) = self.action {
            let zone_or_token =
                matches!(kind, BusEventKind::ZoneChange | BusEventKind::CreateToken);
            let fits = match m {
                Modification::SetDestination(_) | Modification::SetFaceDown(_) => {
                    kind == BusEventKind::ZoneChange
                }
                Modification::SetController(_) | Modification::EntersAsCopyOf(_) => zone_or_token,
                Modification::Amount(op) => {
                    if let AmountOp::Mul(n) = op
                        && n < 0
                    {
                        return Some(format!("effect {id}: a negative factor {n}"));
                    }
                    matches!(
                        kind,
                        BusEventKind::Damage
                            | BusEventKind::LifeChange
                            | BusEventKind::AddCounters
                            | BusEventKind::RemoveCounters
                    )
                }
            };
            if !fits {
                return Some(format!(
                    "effect {id}: the modification {m:?} does not apply to {}",
                    kind.name()
                ));
            }
        }
        None
    }
}

/// The replacement and prevention effects in force, by ascending [`EffectId`] (REQ-EVENT-001).
///
/// The caller keeps it equal to the effects in force; the bus consumes shields and removes
/// exhausted effects. Canonical encoding: the `BTreeMap` encoding (count, then each id and
/// effect ascending, including the remaining shield).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplacementRegistry {
    effects: BTreeMap<EffectId, ReplacementEffect>,
}

impl ReplacementRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        ReplacementRegistry::default()
    }

    /// Registers `effect`.
    ///
    /// # Errors
    /// [`BusError::DuplicateEffect`] if its id is registered; [`BusError::InvalidEffect`] if the
    /// matcher has no kind, a prevention or pool shield is not on a `DAMAGE` matcher, a pool
    /// shield is not on a `Prevent` action, a modification does not fit the kind, a shield is
    /// empty, or a factor is negative. The registry is then unchanged.
    pub fn add(&mut self, effect: ReplacementEffect) -> Result<(), BusError> {
        if self.effects.contains_key(&effect.id) {
            return Err(BusError::DuplicateEffect(effect.id));
        }
        if let Some(reason) = effect.invalid_reason() {
            return Err(BusError::InvalidEffect(reason));
        }
        self.effects.insert(effect.id, effect);
        Ok(())
    }

    /// Removes and returns the effect `id`, if registered.
    pub fn remove(&mut self, id: EffectId) -> Option<ReplacementEffect> {
        self.effects.remove(&id)
    }

    /// The effect `id`, if registered.
    #[must_use]
    pub fn get(&self, id: EffectId) -> Option<&ReplacementEffect> {
        self.effects.get(&id)
    }

    /// Every registered id, ascending.
    #[must_use]
    pub fn ids(&self) -> Vec<EffectId> {
        self.effects.keys().copied().collect()
    }

    /// The number of registered effects.
    #[must_use]
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Whether no effect is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// The effects, ascending id.
    pub(super) fn iter(&self) -> impl Iterator<Item = &ReplacementEffect> + '_ {
        self.effects.values()
    }

    pub(super) fn get_mut(&mut self, id: EffectId) -> Option<&mut ReplacementEffect> {
        self.effects.get_mut(&id)
    }
}

impl CanonicalEncode for ReplacementRegistry {
    /// The count, then each id and effect ascending (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.effects.encode_canonical(out);
    }
}

/// An error of the bus or of registering an effect (REQ-EVENT-001). On any error from
/// [`process_batch`](super::process_batch) the registry and every component of the event
/// context are restored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BusError {
    /// An effect with this id is already registered.
    DuplicateEffect(EffectId),
    /// The effect cannot be registered (the explanation names why).
    InvalidEffect(String),
    /// The event executor failed, or the chooser's object is unknown.
    Event(EventError),
    /// The choice protocol failed.
    Choice(ChoiceError),
    /// Child events nested deeper than [`MAX_CHILD_DEPTH`](super::MAX_CHILD_DEPTH).
    DepthExceeded,
    /// More than [`MAX_REPEATS`](super::MAX_REPEATS) applications to one event.
    RepeatLimit,
    /// More than [`MAX_BUS_EVENTS`](super::MAX_BUS_EVENTS) event instances taken up in one
    /// call (the submitted batch and every child event).
    WorkLimit,
}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BusError::DuplicateEffect(id) => write!(f, "effect {} is already registered", id.get()),
            BusError::InvalidEffect(m) => write!(f, "invalid effect: {m}"),
            BusError::Event(e) => write!(f, "event error: {e}"),
            BusError::Choice(e) => write!(f, "choice error: {e}"),
            BusError::DepthExceeded => write!(
                f,
                "child events nested deeper than {}",
                super::MAX_CHILD_DEPTH
            ),
            BusError::RepeatLimit => write!(
                f,
                "more than {} replacement applications to one event",
                super::MAX_REPEATS
            ),
            BusError::WorkLimit => write!(
                f,
                "more than {} events taken up by one bus call",
                super::MAX_BUS_EVENTS
            ),
        }
    }
}

impl std::error::Error for BusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BusError::Event(e) => Some(e),
            BusError::Choice(e) => Some(e),
            _ => None,
        }
    }
}

impl From<EventError> for BusError {
    fn from(e: EventError) -> BusError {
        BusError::Event(e)
    }
}

/// Why [`process_batch`](super::process_batch) stopped before finishing (REQ-EVENT-001).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BusInterrupt {
    /// A player must choose a replacement effect; propagate it to the session.
    Pending(Box<PendingChoiceRequest>),
    /// The bus failed; nothing changed.
    Error(BusError),
}

impl fmt::Display for BusInterrupt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BusInterrupt::Pending(r) => write!(f, "suspended on request {}", r.request_id),
            BusInterrupt::Error(e) => write!(f, "{e}"),
        }
    }
}

impl From<Interrupt> for BusInterrupt {
    fn from(i: Interrupt) -> BusInterrupt {
        match i {
            Interrupt::Pending(r) => BusInterrupt::Pending(r),
            Interrupt::Error(e) => BusInterrupt::Error(BusError::Choice(e)),
        }
    }
}

impl From<BusInterrupt> for Interrupt {
    fn from(i: BusInterrupt) -> Interrupt {
        match i {
            BusInterrupt::Pending(r) => Interrupt::Pending(r),
            BusInterrupt::Error(e) => Interrupt::Error(ChoiceError::Procedure(e.to_string())),
        }
    }
}

impl From<BusError> for BusInterrupt {
    fn from(e: BusError) -> BusInterrupt {
        BusInterrupt::Error(e)
    }
}

impl From<EventError> for BusInterrupt {
    fn from(e: EventError) -> BusInterrupt {
        BusInterrupt::Error(BusError::Event(e))
    }
}

/// Helpers the bus uses to read an event's affected object and player.
pub(super) fn affected_object(event: &GameEvent) -> Option<ObjectId> {
    match event {
        GameEvent::ZoneChange { object, .. }
        | GameEvent::Tap { object }
        | GameEvent::Untap { object }
        | GameEvent::TurnFaceUp { object }
        | GameEvent::TurnFaceDown { object }
        | GameEvent::BecomeCopy { object, .. } => Some(*object),
        GameEvent::Attach { attachment, .. } | GameEvent::Unattach { attachment } => {
            Some(*attachment)
        }
        GameEvent::Damage {
            target: DamageTarget::Object(o),
            ..
        }
        | GameEvent::AddCounters {
            target: CounterTarget::Object(o),
            ..
        }
        | GameEvent::RemoveCounters {
            target: CounterTarget::Object(o),
            ..
        } => Some(*o),
        _ => None,
    }
}

/// CR 101.3, CR 614.6, CR 400.7: the objects `event` names in a role that needs the object as
/// it is now, so that the event is an instruction that can't be carried out once such an
/// object has left its zone (see `Bus::impossible`):
///
/// - the affected object of every event ([`affected_object`]);
/// - the host of an `Attach` to a permanent (CR 701.3b: an attachment to an object it can't be
///   attached to doesn't happen; a moved object is a new object, CR 400.7);
/// - the copied object of a `BecomeCopy` (engine limitation: CR 608.2h would copy its last
///   known information, but the events layer's standalone `BecomeCopy` takes only an object
///   in a zone; see DECISIONS "Descendant scheduling and the roles of the
///   impossible-instruction check (round 6)").
///
/// Not named here: the source of `Damage` (CR 608.2h: the object as it most recently existed
/// deals the damage) and players (never retired).
pub(super) fn required_objects(event: &GameEvent) -> Vec<ObjectId> {
    let mut objects: Vec<ObjectId> = affected_object(event).into_iter().collect();
    match event {
        // CR 701.3b
        GameEvent::Attach {
            target: AttachTarget::Permanent(host),
            ..
        } => objects.push(*host),
        // CR 608.2h (limitation, see above)
        GameEvent::BecomeCopy { copied, .. } => objects.push(*copied),
        _ => {}
    }
    objects
}

/// The affected player of `event` (see [`EventMatcher`]).
pub(super) fn affected_player(event: &GameEvent) -> Option<PlayerId> {
    match event {
        GameEvent::Draw { player }
        | GameEvent::LifeChange { player, .. }
        | GameEvent::TurnBegins { player } => Some(*player),
        GameEvent::CreateToken { owner, .. } => Some(*owner),
        GameEvent::Damage {
            target: DamageTarget::Player(p),
            ..
        }
        | GameEvent::AddCounters {
            target: CounterTarget::Player(p),
            ..
        }
        | GameEvent::RemoveCounters {
            target: CounterTarget::Player(p),
            ..
        } => Some(*p),
        _ => None,
    }
}
