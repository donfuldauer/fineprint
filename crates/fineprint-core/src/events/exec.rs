// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Event execution primitives: the execution context, batch atomicity, LKI capture and the
//! per-event semantics (REQ-EVENT-001, REQ-DSL-003).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{CardId, IdGenerator, ObjectId, PlayerId};

use super::error::EventError;
use super::history::HistoryStat;
use super::order::{GraveyardOrder, GraveyardOrderGroup, arrange, check_orders, groups_of};
use super::state::{EventState, ObjectMarkers};
use super::types::{
    CounterTarget, DamageTarget, EventBatch, EventInstance, ExecutedBatch, ExecutedEvent,
    GameEvent, Outcome, Phase, Step, UnattachRecord,
};
use crate::memory::{CharacteristicsProvider, LkiStore, MemoryTable, capture_lki_on_move};
use crate::objects::Characteristics;
use crate::permanent::{AttachTarget, Permanents};
use crate::state::GameState;
use crate::zones::{MoveRequest, Moved, Position, ZoneId, ZoneKind, Zones};

/// Everything event execution reads and writes (REQ-EVENT-001). Built with a struct literal.
///
/// `ids` is the object-id generator the [`Zones`] container uses (the game state's own object
/// generator is not reachable from here; the integration unit joins them). `provider` supplies
/// the evaluated characteristics that last-known-information capture snapshots.
///
/// **The provider must be the evaluated one** (REQ-LAYER-001, REQ-CORE-003; CR 613.1-613.4,
/// CR 608.2h). `provider` MUST answer with the characteristics evaluated through layers 1a-7d,
/// for example a [`LayeredProvider`](crate::layers::LayeredProvider); a base-characteristic
/// provider is for unit fakes only. The context holds `game` mutably, so the provider cannot
/// borrow the live game state; build it over a snapshot taken before the context (and so
/// before the batch runs, CR 603.10a, CR 704.8):
///
/// ```text
/// let snap = game.clone();
/// let env = GameEnvironment::new(&snap, &world);
/// let provider = LayeredProvider::new(snap.effects(), &env);
/// let mut ctx = EventContext { game: &mut game, /* ... */ provider: &provider };
/// ```
///
/// An object created by an earlier event of the same batch is unknown to the snapshot, as to
/// any injected provider, so its capture fails with
/// [`MemoryError::NoCharacteristics`](crate::memory::MemoryError::NoCharacteristics). Only the
/// provider's characteristics are read: LKI counters come from the object markers and
/// attachments from `permanents` (see `LkiProvider`).
pub struct EventContext<'a> {
    /// The game state: players, life, player counters, turn number, timestamps.
    pub game: &'a mut GameState,
    /// The zones.
    pub zones: &'a mut Zones,
    /// The object-id generator `zones` uses.
    pub ids: &'a mut IdGenerator<ObjectId>,
    /// The battlefield permanents.
    pub permanents: &'a mut Permanents,
    /// The per-object Memory table.
    pub memories: &'a mut MemoryTable,
    /// The last-known-information store.
    pub lki: &'a mut LkiStore,
    /// The events-owned state: markers, log, history, phase/step, flags.
    pub events: &'a mut EventState,
    /// The characteristics source for LKI capture: the layer-evaluated characteristics (layers
    /// 1a-7d, e.g. [`LayeredProvider`](crate::layers::LayeredProvider) over a snapshot of the
    /// state); see the type documentation for the recipe.
    pub provider: &'a dyn CharacteristicsProvider,
}

/// A clone of every component of an [`EventContext`], for all-or-nothing execution.
#[derive(Clone)]
struct Snapshot {
    game: GameState,
    zones: Zones,
    ids: IdGenerator<ObjectId>,
    permanents: Permanents,
    memories: MemoryTable,
    lki: LkiStore,
    events: EventState,
}

/// How a run of a batch handles last known information.
enum LkiMode<'s> {
    /// Planning run: capture nothing.
    Off,
    /// Real run: these ids were captured before any move of the batch; any other departing
    /// object is captured just before its own move.
    On(&'s BTreeSet<ObjectId>),
}

/// The provider LKI capture uses (CR 603.10, CR 608.2h): characteristics from the caller's
/// provider, which must be the layer-evaluated one (REQ-LAYER-001, CR 613.1-613.4; see
/// [`EventContext`]); counters only from [`ObjectMarkers`], the single source of counters on objects
/// (CR 122.1), which `AddCounters` and `RemoveCounters` also read and write; the objects
/// attached here only from the permanent container, which `Attach` and `Unattach` write
/// (CR 701.3a, CR 701.3d). The caller's provider's `counters` and `attached_here` are not
/// consulted, so LKI always matches what the events themselves did (see DECISIONS). Layers do
/// not change counter amounts, so these raw counters are also the evaluated ones.
struct LkiProvider<'a> {
    inner: &'a dyn CharacteristicsProvider,
    markers: &'a ObjectMarkers,
    permanents: &'a Permanents,
}

impl CharacteristicsProvider for LkiProvider<'_> {
    fn characteristics(&self, id: ObjectId) -> Option<Characteristics> {
        self.inner.characteristics(id)
    }

    /// CR 122.1: the counters the events module holds, and nothing else.
    fn counters(&self, id: ObjectId) -> BTreeMap<String, i64> {
        self.markers.counters(id)
    }

    /// CR 701.3a: the permanents attached to `id` in the permanent container, ascending.
    fn attached_here(&self, id: ObjectId) -> Vec<ObjectId> {
        self.permanents.attachments_of(AttachTarget::Permanent(id))
    }
}

impl EventContext<'_> {
    /// Executes one event as a batch of one (REQ-EVENT-001). Returns `None` if no event
    /// occurred (a no-op: nothing changed, nothing logged).
    ///
    /// # Errors
    /// As [`EventContext::execute_batch`]; nothing is changed.
    pub fn execute(
        &mut self,
        event: impl Into<EventInstance>,
    ) -> Result<Option<ExecutedEvent>, EventError> {
        let batch = EventBatch::new(vec![event.into()]);
        let executed = self.execute_batch(&batch)?;
        Ok(executed.events.into_iter().next())
    }

    /// Executes a set of simultaneous events all-or-nothing (REQ-EVENT-001, CR 704.3,
    /// CR 703.4m).
    ///
    /// Last known information is captured for every object the batch moves out of its zone
    /// before any move of the batch (CR 704.8, CR 603.10a); an object that only exists because
    /// of an earlier event of the same batch is captured just before its own move. The events
    /// are then executed in submission order. If at least one event occurred, one
    /// [`ExecutedBatch`] is appended to the log; a batch in which nothing occurred is returned
    /// with no events and is not logged.
    ///
    /// # Errors
    /// The first error of any event (see [`EventError`]). Every component of the context
    /// (game, zones, ids, permanents, memories, LKI, event state) is then restored to its
    /// value before the call.
    pub fn execute_batch(&mut self, batch: &EventBatch) -> Result<ExecutedBatch, EventError> {
        self.execute_batch_with_entry_copies(batch, &[])
    }

    /// Executes a set of simultaneous events exactly as [`EventContext::execute_batch`] does,
    /// except that each listed entering event puts its object onto the battlefield as a copy
    /// (REQ-EVENT-001, CR 707.5, CR 614.1c).
    ///
    /// Each pair of `copies` is (index into `batch.events()` of an entering event, the copied
    /// object). In the real run, immediately after the event at a listed index is applied, if
    /// it put an object onto the battlefield (a zone change to the battlefield from another
    /// zone, or a token creation), that new object's copy link is set to the copied object
    /// before the next event of the batch runs and before the batch is logged: CR 707.5, it
    /// enters as a copy and does not enter and then become one. The link is part of that
    /// event's execution; the executed batch holds no extra event. A listed event that does
    /// not put an object onto the battlefield sets nothing. The copied object is accepted if
    /// it is in a zone at that moment or has last known information (it left earlier in this
    /// batch, captured before the batch's moves, or in an earlier call): CR 608.2h. Only the
    /// bus uses this entry point; a standalone `BecomeCopy` event stays strict.
    ///
    /// # Errors
    /// As [`EventContext::execute_batch`], and [`EventError::UnknownObject`] for a copied
    /// object that has neither a zone record nor last known information when its entering
    /// event runs. Every component of the context is then restored to its value before the
    /// call.
    pub fn execute_batch_with_entry_copies(
        &mut self,
        batch: &EventBatch,
        copies: &[(usize, ObjectId)],
    ) -> Result<ExecutedBatch, EventError> {
        self.execute_planned(batch, copies, &[])
    }

    /// The simultaneous graveyard entries of `batch` that its owners may arrange
    /// (REQ-ZONE-001, REQ-EVENT-001, CR 404.3), ascending by owner PlayerId; empty if there is
    /// none. Ask one `ORDER_CARDS` request per group ([`GraveyardOrderGroup::request`]), in
    /// this order (ascending PlayerId: the CR gives no sequencing and the piles are
    /// independent), then call [`EventContext::execute_batch_ordered`].
    ///
    /// `batch` is final (replacements were applied before it, CR 614.6). The groups come from a
    /// planning run of the batch exactly as [`EventContext::execute_batch`] would execute it;
    /// the group rule is in the docs/DECISIONS.md entry "#208 FP-154": two or more nontoken
    /// cards (CR 111.6, CR 108.2b) put by `ZoneChange` events at `Position::Top` from another
    /// zone into the same graveyard (CR 400.3: their owner's), and still there at the end of
    /// the batch. Draws, `Bottom` and `Index` moves, tokens and graveyard repositions take no
    /// part. Every component of the context is left unchanged.
    ///
    /// # Errors
    /// As [`EventContext::execute_batch`] (the batch would fail); nothing is changed.
    pub fn graveyard_order_groups(
        &mut self,
        batch: &EventBatch,
    ) -> Result<Vec<GraveyardOrderGroup>, EventError> {
        let snapshot = self.snapshot();
        let result = self
            .run(batch, &LkiMode::Off, &[])
            .map(|planned| groups_of(self.zones, self.events.markers(), &planned));
        self.restore(snapshot);
        result
    }

    /// Executes `batch` exactly as [`EventContext::execute_batch`] does (same events, same
    /// outcomes and object ids, same log entry, LKI captured before any move, all-or-nothing),
    /// except that the cards of each [`GraveyardOrderGroup`] end in the order `orders` gives
    /// for its owner (REQ-ZONE-001, REQ-EVENT-001, CR 404.3). A group with no entry keeps its
    /// [`GraveyardOrderGroup::default_order`]; the order of the entries of `orders` is
    /// irrelevant. With no orders this is `execute_batch`.
    ///
    /// **Placement (CR 404.3, CR 404.1).** The group's cards take the positions ("slots") they
    /// occupy in the `execute_batch` result, in the chosen order, `top_to_bottom[0]` in the
    /// topmost slot. Every other object of the graveyard keeps its `execute_batch` position:
    /// cards already there stay below, a `Bottom` or `Index` move keeps its requested place, a
    /// token keeps its place (it ceases to exist at the next state-based-action check,
    /// CR 111.7). So the group ends contiguous on top of the graveyard unless another object of
    /// the batch landed between its cards (then that object stays where it landed), and an
    /// order equal to `default_order` changes nothing. The arrangement uses same-zone
    /// repositions only (CR 400.7): no event, no new object id.
    ///
    /// # Errors
    /// [`EventError::InvalidGraveyardOrder`] if an entry names an owner with no group, an owner
    /// twice, or a list that is not a permutation of that group's cards (the lowest such
    /// owner), and every error of [`EventContext::execute_batch`]. Every component of the
    /// context is then restored to its value before the call.
    pub fn execute_batch_ordered(
        &mut self,
        batch: &EventBatch,
        orders: &[GraveyardOrder],
    ) -> Result<ExecutedBatch, EventError> {
        self.execute_planned(batch, &[], orders)
    }

    /// The common body of the batch entry points: plan, check `orders` against the planned
    /// groups (CR 404.3), restore, run for real with LKI captured first (CR 704.8), arrange
    /// the ordered groups, log.
    fn execute_planned(
        &mut self,
        batch: &EventBatch,
        copies: &[(usize, ObjectId)],
        orders: &[GraveyardOrder],
    ) -> Result<ExecutedBatch, EventError> {
        let snapshot = self.snapshot();
        // Planning run: find which objects the batch moves out of their zones, in the exact
        // order and with the exact cards the real run will move (draws depend on library
        // order, which earlier events of the batch can change).
        let planned = match self.run(batch, &LkiMode::Off, &[]) {
            Ok(events) => events,
            Err(e) => {
                self.restore(snapshot);
                return Err(e);
            }
        };
        // CR 404.3: the chosen orders must fit the batch's final groups.
        let orders = if orders.is_empty() {
            Vec::new()
        } else {
            let groups = groups_of(self.zones, self.events.markers(), &planned);
            match check_orders(&groups, orders) {
                Ok(sorted) => sorted,
                Err(e) => {
                    self.restore(snapshot);
                    return Err(e);
                }
            }
        };
        self.restore(snapshot.clone());
        let result = self
            .run_captured(batch, &planned, copies)
            .and_then(|events| {
                self.arrange_all(&events, &orders)?;
                Ok(events)
            });
        match result {
            Ok(events) => {
                drop(snapshot);
                let executed = ExecutedBatch {
                    index: self.events.log().next_index(),
                    turn: self.game.turn().turn_number(),
                    events,
                };
                if !executed.events.is_empty() {
                    self.events.log_mut().push(executed.clone());
                }
                Ok(executed)
            }
            Err(e) => {
                self.restore(snapshot);
                Err(e)
            }
        }
    }

    /// CR 404.3: puts each ordered group of the real run's `events` in its chosen order.
    fn arrange_all(
        &mut self,
        events: &[ExecutedEvent],
        orders: &[GraveyardOrder],
    ) -> Result<(), EventError> {
        if orders.is_empty() {
            return Ok(());
        }
        let renamed: BTreeMap<ObjectId, ObjectId> = events
            .iter()
            .filter_map(|e| match e.outcome {
                Outcome::ZoneChange(m) => Some((m.old, m.new)),
                _ => None,
            })
            .collect();
        for order in orders {
            arrange(self.zones, self.ids, order, &renamed)?;
        }
        Ok(())
    }

    /// The real run: captures LKI for every planned departure of an object that exists now
    /// (CR 704.8: before any of the batch's moves), then executes the batch.
    fn run_captured(
        &mut self,
        batch: &EventBatch,
        planned: &[ExecutedEvent],
        copies: &[(usize, ObjectId)],
    ) -> Result<Vec<ExecutedEvent>, EventError> {
        let mut captured = BTreeSet::new();
        for executed in planned {
            if let Outcome::ZoneChange(moved) = executed.outcome
                && moved.old != moved.new
                && self.zones.object(moved.old).is_some()
                && self.capture(moved.old, &departure_request(executed))?
            {
                captured.insert(moved.old);
            }
        }
        self.run(batch, &LkiMode::On(&captured), copies)
    }

    /// Captures the LKI of `id` before `req` moves it (CR 603.10, CR 608.2h), with the
    /// evaluated characteristics of the context's provider (REQ-LAYER-001).
    fn capture(&mut self, id: ObjectId, req: &MoveRequest) -> Result<bool, EventError> {
        let provider = LkiProvider {
            inner: self.provider,
            markers: self.events.markers(),
            permanents: self.permanents,
        };
        Ok(capture_lki_on_move(
            self.zones,
            self.permanents,
            self.memories,
            &provider,
            self.lki,
            id,
            req,
        )?)
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            game: self.game.clone(),
            zones: self.zones.clone(),
            ids: self.ids.clone(),
            permanents: self.permanents.clone(),
            memories: self.memories.clone(),
            lki: self.lki.clone(),
            events: self.events.clone(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        *self.game = s.game;
        *self.zones = s.zones;
        *self.ids = s.ids;
        *self.permanents = s.permanents;
        *self.memories = s.memories;
        *self.lki = s.lki;
        *self.events = s.events;
    }

    /// Executes every event of `batch` in order; returns the events that occurred. `copies`
    /// (used only by the real run) lists the entering events that enter as a copy.
    fn run(
        &mut self,
        batch: &EventBatch,
        mode: &LkiMode<'_>,
        copies: &[(usize, ObjectId)],
    ) -> Result<Vec<ExecutedEvent>, EventError> {
        let mut executed = Vec::new();
        for (index, instance) in batch.events().iter().enumerate() {
            let mut unattached = Vec::new();
            if let Some((outcome, lki_captured)) =
                self.apply(instance.event(), mode, &mut unattached)?
            {
                if let LkiMode::On(_) = mode {
                    for (_, copied) in copies.iter().filter(|(i, _)| *i == index) {
                        self.enter_as_copy(outcome, *copied)?;
                    }
                }
                executed.push(ExecutedEvent {
                    instance: instance.clone(),
                    outcome,
                    lki_captured,
                    unattached,
                });
            }
        }
        Ok(executed)
    }

    /// Executes one event; `None` for a no-op. The flag is whether LKI was stored. Every
    /// attachment link the event removes is appended to `unattached` (CR 701.3d); a no-op
    /// appends nothing.
    fn apply(
        &mut self,
        event: &GameEvent,
        mode: &LkiMode<'_>,
        unattached: &mut Vec<UnattachRecord>,
    ) -> Result<Option<(Outcome, bool)>, EventError> {
        let applied = |o: Option<Outcome>| o.map(|o| (o, false));
        match event {
            GameEvent::ZoneChange {
                object, request, ..
            } => self.zone_change(*object, *request, mode, unattached),
            GameEvent::Draw { player } => self.draw(*player, mode, unattached),
            GameEvent::Damage {
                source_controller,
                target,
                amount,
                ..
            } => self
                .damage(*source_controller, *target, *amount)
                .map(applied),
            GameEvent::LifeChange { player, delta } => {
                self.life_change(*player, *delta).map(applied)
            }
            GameEvent::AddCounters {
                target,
                kind,
                amount,
            } => self.add_counters(*target, kind, *amount).map(applied),
            GameEvent::RemoveCounters {
                target,
                kind,
                amount,
            } => self.remove_counters(*target, kind, *amount).map(applied),
            GameEvent::Tap { object } => self.set_tapped(*object, true).map(applied),
            GameEvent::Untap { object } => self.set_tapped(*object, false).map(applied),
            GameEvent::CreateToken {
                owner,
                card,
                controller,
            } => self.create_token(*owner, *card, *controller).map(applied),
            GameEvent::TurnBegins { player } => self.turn_begins(*player).map(applied),
            GameEvent::PhaseBegins { phase } => Ok(applied(Some(self.phase_begins(*phase)))),
            GameEvent::StepBegins { step } => Ok(applied(Some(self.step_begins(*step)))),
            GameEvent::Attach { attachment, target } => {
                self.attach(*attachment, *target, unattached).map(applied)
            }
            GameEvent::Unattach { attachment } => {
                self.unattach(*attachment, None, unattached).map(applied)
            }
            GameEvent::UnattachFrom { attachment, host } => self
                .unattach(*attachment, Some(*host), unattached)
                .map(applied),
            GameEvent::BecomeCopy { object, copied } => {
                self.become_copy(*object, *copied).map(applied)
            }
            GameEvent::TurnFaceUp { object } => self.set_face_down(*object, false).map(applied),
            GameEvent::TurnFaceDown { object } => self.set_face_down(*object, true).map(applied),
        }
    }

    fn turn(&self) -> u64 {
        self.game.turn().turn_number()
    }

    fn require_player(&self, player: PlayerId) -> Result<(), EventError> {
        if self.game.player(player).is_some() {
            Ok(())
        } else {
            Err(EventError::UnknownPlayer(player))
        }
    }

    /// `Ok` iff `id` is a permanent held by the permanent container: `UnknownObject` if no
    /// zone holds it, `NotOnBattlefield` if it is in a zone but not a held permanent.
    fn require_permanent(&self, id: ObjectId) -> Result<(), EventError> {
        if self.zones.object(id).is_none() {
            return Err(EventError::UnknownObject(id));
        }
        if self.permanents.get(id).is_none() {
            return Err(EventError::NotOnBattlefield(id));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Zone changes and draws
    // -----------------------------------------------------------------------------------------

    /// CR 400.7: a zone change makes a new object; CR 111.8: a token that left the battlefield
    /// can't move; CR 122.2: counters cease to exist on a zone change.
    fn zone_change(
        &mut self,
        object: ObjectId,
        request: MoveRequest,
        mode: &LkiMode<'_>,
        unattached: &mut Vec<UnattachRecord>,
    ) -> Result<Option<(Outcome, bool)>, EventError> {
        let record = *self
            .zones
            .object(object)
            .ok_or(EventError::UnknownObject(object))?;
        // CR 111.8: a token that has left the battlefield remains in its current zone.
        if self.events.markers().is_token(record.card) && record.zone != ZoneId::Battlefield {
            return Ok(None);
        }
        let (moved, lki_captured) = self.move_with_lki(object, request, mode, unattached)?;
        Ok(Some((Outcome::ZoneChange(moved), lki_captured)))
    }

    /// CR 121.1, CR 121.2: one draw puts the top card of the library into the hand. CR 121.4,
    /// CR 704.5b: an attempt to draw from an empty library is flagged for the state-based
    /// action check and is no event.
    fn draw(
        &mut self,
        player: PlayerId,
        mode: &LkiMode<'_>,
        unattached: &mut Vec<UnattachRecord>,
    ) -> Result<Option<(Outcome, bool)>, EventError> {
        self.require_player(player)?;
        let Some(top) = self
            .zones
            .contents(ZoneId::Library(player))
            .first()
            .copied()
        else {
            // CR 121.4, CR 704.5b
            self.events.flag_empty_library_draw(player);
            return Ok(None);
        };
        let card = self
            .zones
            .object(top)
            .ok_or(EventError::UnknownObject(top))?
            .card;
        if self.events.markers().is_token(card) {
            // CR 111.8: a token that has left the battlefield remains in its current zone; the
            // blocked move is no draw (and the library is not empty, so no CR 121.4 flag).
            return Ok(None);
        }
        let (moved, lki_captured) =
            self.move_with_lki(top, MoveRequest::new(ZoneKind::Hand), mode, unattached)?;
        let turn = self.turn();
        self.events
            .history_mut()
            .add(turn, HistoryStat::CardsDrawn, player, 1);
        Ok(Some((Outcome::ZoneChange(moved), lki_captured)))
    }

    /// Moves `object` with the zone-move primitive and applies CR 400.7 to every component:
    /// LKI (captured before the move, CR 603.10, CR 704.8), Memory, the permanent container
    /// (CR 110.1) and the object markers (CR 122.2).
    ///
    /// CR 701.3d: when a permanent leaves the battlefield, every attachment link it was part of
    /// ends; they are appended to `unattached`, read before the permanent container drops them:
    /// first the permanent's own link (under its old id), then one record per permanent
    /// attached to it, ascending id. A same-zone reposition ends no link.
    fn move_with_lki(
        &mut self,
        object: ObjectId,
        request: MoveRequest,
        mode: &LkiMode<'_>,
        unattached: &mut Vec<UnattachRecord>,
    ) -> Result<(Moved, bool), EventError> {
        let lki_captured = match mode {
            LkiMode::Off => false,
            LkiMode::On(pre) if pre.contains(&object) => true,
            // Not pre-captured: either it does not leave its zone (Ok(false)) or it only came
            // into existence during this batch; capture it just before its own move.
            LkiMode::On(_) => self.capture(object, &request)?,
        };
        let moved = self.zones.move_object(self.ids, object, request)?;
        if moved.old == moved.new {
            // CR 400.7: a same-zone reposition is not a zone change: no LKI belongs to it (an
            // id pre-captured for a later departure in the batch is reported by that event).
            // The zone record now carries the request's face status; keep the permanent's
            // status equal to it (CR 701.27b, CR 708.8).
            self.sync_face_down(moved.new)?;
            return Ok((moved, false));
        }
        self.memories.apply_zone_change(&moved);
        if moved.from == ZoneId::Battlefield && self.permanents.get(moved.old).is_some() {
            // CR 701.3d: its own link ends, as do the links of everything attached to it.
            if let Some(host) = self.permanents.attached_to(moved.old) {
                unattached.push(UnattachRecord {
                    attachment: moved.old,
                    host,
                });
            }
            let host = AttachTarget::Permanent(moved.old);
            for attachment in self.permanents.attachments_of(host) {
                unattached.push(UnattachRecord { attachment, host });
            }
            // CR 110.1: it stops being a permanent (this drops the links read above).
            self.permanents.leave(moved.old)?;
        }
        if moved.to == ZoneId::Battlefield {
            let controller = self
                .zones
                .object(moved.new)
                .ok_or(EventError::UnknownObject(moved.new))?
                .controller;
            // CR 110.1: it becomes a new permanent.
            self.permanents.enter(moved.new, controller, self.turn())?;
            // A permanent put onto the battlefield face down is face down (CR 708.8).
            self.sync_face_down(moved.new)?;
        }
        // CR 122.2, CR 400.7: counters, marked damage and copy links cease to exist.
        self.events.markers_mut().forget(moved.old);
        Ok((moved, lki_captured))
    }

    // -----------------------------------------------------------------------------------------
    // Damage, life and counters
    // -----------------------------------------------------------------------------------------

    /// CR 120.8: 0 damage is no damage. CR 119.2, CR 120.3a: damage to a player is life
    /// loss. CR 120.3e: damage to a permanent is marked on it.
    fn damage(
        &mut self,
        source_controller: Option<PlayerId>,
        target: DamageTarget,
        amount: i64,
    ) -> Result<Option<Outcome>, EventError> {
        if amount < 0 {
            return Err(EventError::InvalidAmount(amount));
        }
        if amount == 0 {
            // CR 120.8
            return Ok(None);
        }
        let turn = self.turn();
        match target {
            DamageTarget::Player(p) => {
                self.adjust_life(p, -amount)?;
                let history = self.events.history_mut();
                // CR 119.2, CR 120.3a
                history.add(turn, HistoryStat::LifeLost, p, amount);
                history.add(turn, HistoryStat::DamageDealtToPlayer, p, amount);
            }
            DamageTarget::Object(o) => {
                self.require_permanent(o)?;
                // CR 120.3e
                let marked = self
                    .events
                    .markers()
                    .damage(o)
                    .checked_add(amount)
                    .ok_or(EventError::Overflow)?;
                self.events.markers_mut().set_damage(o, marked);
            }
        }
        if let Some(controller) = source_controller {
            self.events
                .history_mut()
                .add(turn, HistoryStat::DamageDealtBy, controller, amount);
        }
        Ok(Some(Outcome::Applied))
    }

    /// Adds `delta` to `player`'s life total with checked arithmetic (CR 119.3); a total
    /// below 0 is allowed (CR 119.6 is a state-based action).
    fn adjust_life(&mut self, player: PlayerId, delta: i64) -> Result<(), EventError> {
        let life = self
            .game
            .player(player)
            .ok_or(EventError::UnknownPlayer(player))?
            .life()
            .checked_add(delta)
            .ok_or(EventError::Overflow)?;
        if let Some(mut p) = self.game.player_mut(player) {
            p.set_life(life);
        }
        Ok(())
    }

    /// CR 119.3: life gain and loss. CR 119.9, CR 119.10: gaining 0 life is no event (losing 0
    /// is treated the same, see DECISIONS).
    fn life_change(&mut self, player: PlayerId, delta: i64) -> Result<Option<Outcome>, EventError> {
        if delta == 0 {
            // CR 119.9, CR 119.10
            return Ok(None);
        }
        self.adjust_life(player, delta)?;
        let turn = self.turn();
        let history = self.events.history_mut();
        if delta > 0 {
            history.add(turn, HistoryStat::LifeGained, player, delta);
        } else {
            // `delta` is negative; `i64::MIN` saturates to `i64::MAX`.
            history.add(turn, HistoryStat::LifeLost, player, delta.saturating_neg());
        }
        Ok(Some(Outcome::Applied))
    }

    /// The counters of `kind` on `target` now (`target` must exist).
    fn held_counters(&self, target: CounterTarget, kind: &str) -> Result<i64, EventError> {
        match target {
            CounterTarget::Player(p) => Ok(self
                .game
                .player(p)
                .ok_or(EventError::UnknownPlayer(p))?
                .counter(kind)),
            CounterTarget::Object(o) => {
                if self.zones.object(o).is_none() {
                    return Err(EventError::UnknownObject(o));
                }
                Ok(self.events.markers().counter(o, kind))
            }
        }
    }

    fn set_counters(&mut self, target: CounterTarget, kind: &str, amount: i64) {
        match target {
            CounterTarget::Player(p) => {
                if let Some(mut player) = self.game.player_mut(p) {
                    player.set_counter(kind, amount);
                }
            }
            CounterTarget::Object(o) => self.events.markers_mut().set_counter(o, kind, amount),
        }
    }

    /// CR 122.1: counters are put on objects and players. CR 122.3, CR 704.5q: +1/+1 and
    /// -1/-1 counters are not annihilated here (a state-based action).
    fn add_counters(
        &mut self,
        target: CounterTarget,
        kind: &str,
        amount: i64,
    ) -> Result<Option<Outcome>, EventError> {
        if amount < 0 {
            return Err(EventError::InvalidAmount(amount));
        }
        if amount == 0 {
            return Ok(None);
        }
        let held = self.held_counters(target, kind)?;
        let total = held.checked_add(amount).ok_or(EventError::Overflow)?;
        self.set_counters(target, kind, total);
        Ok(Some(Outcome::Applied))
    }

    /// CR 122.1: removes up to `amount` counters of `kind`; nothing held is no event.
    fn remove_counters(
        &mut self,
        target: CounterTarget,
        kind: &str,
        amount: i64,
    ) -> Result<Option<Outcome>, EventError> {
        if amount < 0 {
            return Err(EventError::InvalidAmount(amount));
        }
        if amount == 0 {
            return Ok(None);
        }
        let held = self.held_counters(target, kind)?;
        let removed = amount.min(held);
        if removed <= 0 {
            return Ok(None);
        }
        self.set_counters(target, kind, held - removed);
        Ok(Some(Outcome::CountersRemoved { removed }))
    }

    // -----------------------------------------------------------------------------------------
    // Permanent status, tokens, turn structure, attachments, copies, face
    // -----------------------------------------------------------------------------------------

    /// CR 701.26a: only untapped permanents can be tapped; CR 701.26b: only tapped permanents
    /// can be untapped.
    fn set_tapped(
        &mut self,
        object: ObjectId,
        tapped: bool,
    ) -> Result<Option<Outcome>, EventError> {
        self.require_permanent(object)?;
        let current = self
            .permanents
            .get(object)
            .is_some_and(|p| p.status().tapped);
        if current == tapped {
            return Ok(None);
        }
        self.permanents.set_tapped(object, tapped)?;
        Ok(Some(Outcome::Applied))
    }

    /// CR 111.2: the creator owns the token and it enters under their control; CR 701.7a: it
    /// is put onto the battlefield.
    fn create_token(
        &mut self,
        owner: PlayerId,
        card: CardId,
        controller: Option<PlayerId>,
    ) -> Result<Option<Outcome>, EventError> {
        self.require_player(owner)?;
        if let Some(c) = controller {
            self.require_player(c)?;
        }
        let request = MoveRequest {
            to: ZoneKind::Battlefield,
            position: Position::Top,
            face_down: false,
            controller,
        };
        let object = self.zones.create_object(self.ids, card, owner, request)?;
        let entered_controller = self
            .zones
            .object(object)
            .ok_or(EventError::UnknownObject(object))?
            .controller;
        self.permanents
            .enter(object, entered_controller, self.turn())?;
        self.events.markers_mut().mark_token(card);
        let turn = self.turn();
        self.events
            .history_mut()
            .add(turn, HistoryStat::TokensCreated, owner, 1);
        Ok(Some(Outcome::TokenCreated { object }))
    }

    /// CR 500.1: a new turn; the active player becomes `player`; no phase or step has begun.
    fn turn_begins(&mut self, player: PlayerId) -> Result<Option<Outcome>, EventError> {
        self.require_player(player)?;
        let turn = self.turn().checked_add(1).ok_or(EventError::Overflow)?;
        self.game.set_active_player(Some(player))?;
        self.game.set_turn_number(turn);
        self.events.set_phase(None);
        self.events.set_step(None);
        Ok(Some(Outcome::TurnBegan { turn }))
    }

    /// CR 500.6, CR 603.2b: the phase begins (its log entry is what "at the beginning of"
    /// triggers match).
    fn phase_begins(&mut self, phase: Phase) -> Outcome {
        self.events.set_phase(Some(phase));
        self.events.set_step(None);
        Outcome::Applied
    }

    /// CR 500.6, CR 603.2b: the step begins, in its phase (CR 500.1).
    fn step_begins(&mut self, step: Step) -> Outcome {
        self.events.set_phase(Some(step.phase()));
        self.events.set_step(Some(step));
        Outcome::Applied
    }

    /// CR 701.3a: attach; CR 701.3b: to the current host it does nothing; CR 701.3c: a new
    /// timestamp. CR 701.3d: attaching to a different host ends the link to the previous one,
    /// which is appended to `unattached`; a first attach ends no link.
    fn attach(
        &mut self,
        attachment: ObjectId,
        target: AttachTarget,
        unattached: &mut Vec<UnattachRecord>,
    ) -> Result<Option<Outcome>, EventError> {
        self.require_permanent(attachment)?;
        match target {
            AttachTarget::Permanent(host) => self.require_permanent(host)?,
            AttachTarget::Player(p) => self.require_player(p)?,
        }
        let previous = self.permanents.attached_to(attachment);
        if previous == Some(target) {
            // CR 701.3b
            return Ok(None);
        }
        self.permanents.attach(attachment, target)?;
        if let Some(host) = previous {
            // CR 701.3d: it ceases to be attached to its previous host.
            unattached.push(UnattachRecord { attachment, host });
        }
        // CR 701.3c
        let timestamp = self.game.issue_timestamp()?;
        Ok(Some(Outcome::Attached { timestamp }))
    }

    /// CR 701.3d: unattach; the cleared link is appended to `unattached`. With `only_from`
    /// (`UnattachFrom`), the link is cleared only if it is to exactly that host. An unattached
    /// object, or one attached to another host than `only_from`, is no event.
    fn unattach(
        &mut self,
        attachment: ObjectId,
        only_from: Option<AttachTarget>,
        unattached: &mut Vec<UnattachRecord>,
    ) -> Result<Option<Outcome>, EventError> {
        if self.zones.object(attachment).is_none() {
            return Err(EventError::UnknownObject(attachment));
        }
        let Some(host) = self.permanents.attached_to(attachment) else {
            return Ok(None);
        };
        if only_from.is_some_and(|from| from != host) {
            return Ok(None);
        }
        self.permanents.detach(attachment)?;
        unattached.push(UnattachRecord { attachment, host });
        Ok(Some(Outcome::Applied))
    }

    /// CR 707.2, CR 613.1a: the copy link the layer system reads; CR 707.4: no zone change.
    fn become_copy(
        &mut self,
        object: ObjectId,
        copied: ObjectId,
    ) -> Result<Option<Outcome>, EventError> {
        self.require_permanent(object)?;
        if self.zones.object(copied).is_none() {
            return Err(EventError::UnknownObject(copied));
        }
        if self.events.markers().copy_of(object) == Some(copied) {
            return Ok(None);
        }
        self.events.markers_mut().set_copy(object, copied);
        Ok(Some(Outcome::Applied))
    }

    /// CR 707.5, CR 614.1c: the object `outcome` put onto the battlefield enters as a copy of
    /// `copied`. CR 608.2h: `copied` is taken from its current information while it is in a
    /// zone, else from its last known information; with neither it is unknown. An outcome
    /// that put nothing onto the battlefield sets nothing.
    fn enter_as_copy(&mut self, outcome: Outcome, copied: ObjectId) -> Result<(), EventError> {
        let entered = match outcome {
            Outcome::ZoneChange(moved)
                if moved.to == ZoneId::Battlefield && moved.from != ZoneId::Battlefield =>
            {
                moved.new
            }
            Outcome::TokenCreated { object } => object,
            _ => return Ok(()),
        };
        if self.zones.object(copied).is_none() && self.lki.get(copied).is_none() {
            return Err(EventError::UnknownObject(copied));
        }
        // CR 707.2: the copy link the layer system reads.
        self.events.markers_mut().set_copy(entered, copied);
        Ok(())
    }

    /// Sets the face status of `object`, if it is a held permanent on the battlefield, to its
    /// zone record's, so the two never disagree (CR 708.8).
    fn sync_face_down(&mut self, object: ObjectId) -> Result<(), EventError> {
        let Some(permanent) = self.permanents.get(object) else {
            return Ok(());
        };
        let record = self
            .zones
            .object(object)
            .ok_or(EventError::UnknownObject(object))?;
        if record.zone == ZoneId::Battlefield && permanent.status().face_down != record.face_down {
            self.permanents.set_face_down(object, record.face_down)?;
        }
        Ok(())
    }

    /// CR 701.27b: turning face up or down is its own action on the same object (no zone
    /// change, CR 400.7); CR 708.8.
    ///
    /// Updates both the zone record's `face_down` (which the redacted projection and LKI
    /// capture read, REQ-CORE-004) and the permanent's status, inside the batch, so a later
    /// failing event rolls both back. The zone container has no direct setter, so the record
    /// is updated by a same-zone battlefield reposition that keeps the id and passes the
    /// current controller explicitly (see DECISIONS). The event is a no-op only if both
    /// already equal `face_down`; if they disagree, the event repairs them and occurs.
    fn set_face_down(
        &mut self,
        object: ObjectId,
        face_down: bool,
    ) -> Result<Option<Outcome>, EventError> {
        self.require_permanent(object)?;
        let record = *self
            .zones
            .object(object)
            .ok_or(EventError::UnknownObject(object))?;
        if record.zone != ZoneId::Battlefield {
            return Err(EventError::NotOnBattlefield(object));
        }
        let status = self
            .permanents
            .get(object)
            .is_some_and(|p| p.status().face_down);
        if status == face_down && record.face_down == face_down {
            return Ok(None);
        }
        let request = MoveRequest {
            to: ZoneKind::Battlefield,
            position: Position::Top,
            face_down,
            controller: Some(record.controller),
        };
        self.zones.move_object(self.ids, object, request)?;
        self.permanents.set_face_down(object, face_down)?;
        Ok(Some(Outcome::Applied))
    }
}

/// The request with which a planned departure leaves its zone: the event's own request, or
/// the hand for a draw.
fn departure_request(executed: &ExecutedEvent) -> MoveRequest {
    match executed.instance.event() {
        GameEvent::ZoneChange { request, .. } => *request,
        _ => MoveRequest::new(ZoneKind::Hand),
    }
}
