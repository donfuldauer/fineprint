// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Timestamp assignment (REQ-CORE-002, CR 613.7a-n). Every timestamp comes from the caller's
//! single monotonic `IdGenerator<Timestamp>`; the ledger owns no counter. Every failing
//! operation leaves the ledger, the registry and the generator unchanged.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{EffectId, IdGenerator, ObjectId, PlayerId, Timestamp};

use super::error::LayerError;
use super::registry::{ContinuousEffects, EffectOrigin};

/// Why an object receives a new timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampReason {
    /// It entered a zone (CR 613.7d).
    EntersZone,
    /// An Aura, Equipment or Fortification became attached (CR 613.7e).
    Attached,
    /// A permanent turned face up or face down (CR 613.7f).
    TurnedFaceUpOrDown,
    /// A double-faced permanent transformed or converted (CR 613.7g).
    TransformedOrConverted,
    /// A plane, phenomenon or scheme card turned face up (CR 613.7h), or a face-down conspiracy
    /// card turned face up (CR 613.7j).
    SpecialCardTurnedFaceUp,
    /// A vanguard card (CR 613.7i) or a conspiracy card (CR 613.7j) at the beginning of the game.
    GameStart,
    /// The object became part of a merged permanent (CR 613.7k, sticker clause).
    JoinedMergedPermanent,
}

/// What counters are on (CR 613.7c).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CounterHolder {
    /// An object.
    Object(ObjectId),
    /// A player.
    Player(PlayerId),
}

/// One object of a batch receiving timestamps simultaneously (CR 613.7m).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimultaneousStamp {
    /// The object.
    pub object: ObjectId,
    /// Its controller, or its owner if it has no controller.
    pub player: PlayerId,
    /// That player's chosen order among their own objects; ascending is earlier.
    pub choice: u32,
}

/// Borrowed view of every entry of a [`TimestampLedger`] (crate-internal; see
/// [`TimestampLedger::entries`]).
pub(crate) struct LedgerEntries<'a> {
    /// Object timestamps (CR 613.7d-j).
    pub(crate) objects: &'a BTreeMap<ObjectId, Timestamp>,
    /// Counter timestamps per holder and kind (CR 613.7c).
    pub(crate) counters: &'a BTreeMap<(CounterHolder, String), Timestamp>,
    /// Sticker timestamps per host and sticker (CR 613.7k).
    pub(crate) stickers: &'a BTreeMap<(ObjectId, u32), Timestamp>,
}

/// The timestamps of objects, counters and stickers (REQ-CORE-002, CR 613.7).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TimestampLedger {
    objects: BTreeMap<ObjectId, Timestamp>,
    counters: BTreeMap<(CounterHolder, String), Timestamp>,
    stickers: BTreeMap<(ObjectId, u32), Timestamp>,
}

impl TimestampLedger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> TimestampLedger {
        TimestampLedger::default()
    }

    /// The timestamp of object `id`, if it has one.
    #[must_use]
    pub fn object_timestamp(&self, id: ObjectId) -> Option<Timestamp> {
        self.objects.get(&id).copied()
    }

    /// The timestamp shared by every counter of `kind` on `holder` (CR 613.7c), if any.
    #[must_use]
    pub fn counter_timestamp(&self, holder: CounterHolder, kind: &str) -> Option<Timestamp> {
        self.counters.get(&(holder, kind.to_owned())).copied()
    }

    /// The timestamp of sticker `sticker` on `host` (CR 613.7k), if any.
    #[must_use]
    pub fn sticker_timestamp(&self, host: ObjectId, sticker: u32) -> Option<Timestamp> {
        self.stickers.get(&(host, sticker)).copied()
    }

    /// Read access to every stored timestamp (objects, counters, stickers), for the canonical
    /// state encoding and the wire form of the game state (crate-internal integration glue,
    /// docs/DECISIONS.md "#183 FP-144 — Ledger read access"). No ledger behaviour depends on it.
    pub(crate) fn entries(&self) -> LedgerEntries<'_> {
        LedgerEntries {
            objects: &self.objects,
            counters: &self.counters,
            stickers: &self.stickers,
        }
    }

    /// Gives `id` a new, strictly later timestamp (CR 613.7d, 613.7e, 613.7f, 613.7g, 613.7h,
    /// 613.7i, 613.7j; every reason stamps the same way), then gives every sticker on `id` a
    /// new timestamp immediately after it, keeping the stickers' relative order (CR 613.7k).
    /// Re-stamping an object that already has a timestamp is allowed.
    ///
    /// # Errors
    /// [`LayerError::IdExhausted`]; the ledger and `ids` are then unchanged.
    pub fn stamp_object(
        &mut self,
        ids: &mut IdGenerator<Timestamp>,
        id: ObjectId,
        reason: StampReason,
    ) -> Result<Timestamp, LayerError> {
        let _ = reason;
        let mut g = ids.clone();
        let mut next = self.clone();
        let ts = next.stamp_with_stickers(&mut g, id)?;
        *ids = g;
        *self = next;
        Ok(ts)
    }

    /// Stamps `id` and then its stickers (CR 613.7k) on `self`; the caller commits.
    fn stamp_with_stickers(
        &mut self,
        ids: &mut IdGenerator<Timestamp>,
        id: ObjectId,
    ) -> Result<Timestamp, LayerError> {
        let ts = ids.issue()?;
        self.objects.insert(id, ts);
        // CR 613.7k: the stickers get new timestamps immediately after the object's, in their
        // previous relative order (ties by sticker number).
        let mut stickers: Vec<(Timestamp, u32)> = self
            .stickers
            .range((id, u32::MIN)..=(id, u32::MAX))
            .map(|(&(_, s), &t)| (t, s))
            .collect();
        stickers.sort_unstable();
        for (_, s) in stickers {
            let t = ids.issue()?;
            self.stickers.insert((id, s), t);
        }
        Ok(ts)
    }

    /// Gives every object of `batch` a timestamp at the same moment (CR 613.7m, CR 101.4): in
    /// APNAP order (`apnap` lists the active player first, then turn order), each player's
    /// objects in that player's `choice` order, ties by [`ObjectId`]. Each object's stickers
    /// follow it (CR 613.7k). Returns the objects with their timestamps in that order; an empty
    /// batch issues nothing.
    ///
    /// # Errors
    /// [`LayerError::UnknownPlayer`] if a batch player is not in `apnap`,
    /// [`LayerError::DuplicateObject`] if an object appears twice, [`LayerError::IdExhausted`].
    /// The ledger and `ids` are then unchanged.
    pub fn stamp_objects_simultaneously(
        &mut self,
        ids: &mut IdGenerator<Timestamp>,
        apnap: &[PlayerId],
        batch: &[SimultaneousStamp],
        reason: StampReason,
    ) -> Result<Vec<(ObjectId, Timestamp)>, LayerError> {
        let _ = reason;
        let mut seen = BTreeSet::new();
        let mut order = Vec::with_capacity(batch.len());
        for entry in batch {
            let seat = apnap
                .iter()
                .position(|&p| p == entry.player)
                .ok_or(LayerError::UnknownPlayer(entry.player))?;
            if !seen.insert(entry.object) {
                return Err(LayerError::DuplicateObject(entry.object));
            }
            order.push((seat, entry.choice, entry.object));
        }
        // CR 613.7m, CR 101.4: APNAP order, then each player's own chosen order.
        order.sort_unstable();
        let mut g = ids.clone();
        let mut next = self.clone();
        let mut out = Vec::with_capacity(order.len());
        for (_, _, object) in order {
            let ts = next.stamp_with_stickers(&mut g, object)?;
            out.push((object, ts));
        }
        *ids = g;
        *self = next;
        Ok(out)
    }

    /// A counter of `kind` is put on `holder`: a new timestamp, which every counter of that
    /// kind on `holder` shares from now on (CR 613.7c).
    ///
    /// # Errors
    /// [`LayerError::IdExhausted`]; the ledger is then unchanged.
    pub fn put_counters(
        &mut self,
        ids: &mut IdGenerator<Timestamp>,
        holder: CounterHolder,
        kind: &str,
    ) -> Result<Timestamp, LayerError> {
        // CR 613.7c
        let ts = ids.issue()?;
        self.counters.insert((holder, kind.to_owned()), ts);
        Ok(ts)
    }

    /// Forgets the timestamp of the counters of `kind` on `holder` (the last one left).
    pub fn clear_counters(&mut self, holder: CounterHolder, kind: &str) {
        self.counters.remove(&(holder, kind.to_owned()));
    }

    /// A sticker is put on `host`: a new timestamp (CR 613.7k).
    ///
    /// # Errors
    /// [`LayerError::IdExhausted`]; the ledger is then unchanged.
    pub fn put_sticker(
        &mut self,
        ids: &mut IdGenerator<Timestamp>,
        host: ObjectId,
        sticker: u32,
    ) -> Result<Timestamp, LayerError> {
        // CR 613.7k
        let ts = ids.issue()?;
        self.stickers.insert((host, sticker), ts);
        Ok(ts)
    }

    /// Forgets the object `id`: its timestamp, its stickers' and the counters on it.
    pub fn forget_object(&mut self, id: ObjectId) {
        self.objects.remove(&id);
        self.stickers.retain(|&(host, _), _| host != id);
        self.counters
            .retain(|(holder, _), _| *holder != CounterHolder::Object(id));
    }
}

/// The timestamp of a continuous effect created by a resolving spell or ability: a new one
/// (CR 613.7b).
///
/// # Errors
/// [`LayerError::IdExhausted`].
pub fn stamp_resolved_effect(ids: &mut IdGenerator<Timestamp>) -> Result<Timestamp, LayerError> {
    // CR 613.7b
    Ok(ids.issue()?)
}

/// The timestamp of an effect of a static ability on an object with timestamp `object_ts`
/// (CR 613.7a): the object's for an ability it has of its own (`granted_at: None`), the later
/// of the object's and the granting effect's for a granted ability. For
/// [`EffectOrigin::Resolved`] it returns `object_ts` unchanged; resolved effects are stamped
/// with [`stamp_resolved_effect`] instead.
#[must_use]
pub fn static_effect_timestamp(object_ts: Timestamp, origin: &EffectOrigin) -> Timestamp {
    match origin {
        // CR 613.7a: the later of the two.
        EffectOrigin::Static {
            granted_at: Some(g),
        } => object_ts.max(*g),
        EffectOrigin::Static { granted_at: None } | EffectOrigin::Resolved => object_ts,
    }
}

/// Timestamps for an object's own static-ability effect and a resolved effect applying to it
/// that would be stamped simultaneously (CR 613.7n, CR 611.2e): `(static, resolved)`, the
/// static one strictly earlier.
///
/// # Errors
/// [`LayerError::IdExhausted`]; `ids` is then unchanged.
pub fn stamp_static_and_resolved(
    ids: &mut IdGenerator<Timestamp>,
) -> Result<(Timestamp, Timestamp), LayerError> {
    // CR 613.7n: the object's own static ability first.
    let mut g = ids.clone();
    let first = g.issue()?;
    let second = g.issue()?;
    *ids = g;
    Ok((first, second))
}

/// Re-stamps the static-ability effects of `object` after it moved from timestamp
/// `old_object_ts` to `new_object_ts` (CR 613.7a, second sentence).
///
/// A static effect of `object` from a granted ability (`granted_at: Some(_)`) whose current
/// timestamp is later than `old_object_ts` (it followed the granting effect, not the host) gets
/// a new timestamp from `ids` (later than `new_object_ts` when the caller issued that one from
/// the same generator), in ascending order of its old (timestamp, [`EffectId`]), so their
/// relative order stays the same; every other static effect of `object` (printed, or granted
/// with a timestamp not later than `old_object_ts`) gets `new_object_ts`. Resolved effects and
/// other objects' effects are never touched. The current timestamp, not `granted_at`, decides,
/// so repeated re-stamps keep the order.
///
/// # Errors
/// [`LayerError::IdExhausted`]; the registry and `ids` are then unchanged.
pub fn restamp_static_effects(
    effects: &mut ContinuousEffects,
    ids: &mut IdGenerator<Timestamp>,
    object: ObjectId,
    old_object_ts: Timestamp,
    new_object_ts: Timestamp,
) -> Result<(), LayerError> {
    let mut granted: Vec<(Timestamp, EffectId)> = Vec::new();
    let mut own: Vec<EffectId> = Vec::new();
    for (id, effect) in effects.iter() {
        if effect.source != Some(object) {
            continue;
        }
        match effect.origin {
            // CR 613.7a: judged on the current timestamp, so a second re-stamp still tells the
            // effects that followed the granting effect from those that shared the host's.
            EffectOrigin::Static {
                granted_at: Some(_),
            } if effect.timestamp > old_object_ts => granted.push((effect.timestamp, id)),
            EffectOrigin::Static { .. } => own.push(id),
            EffectOrigin::Resolved => {}
        }
    }
    // CR 613.7a: new timestamps, relative order unchanged.
    granted.sort_unstable();
    let mut g = ids.clone();
    let mut updates: Vec<(EffectId, Timestamp)> = Vec::with_capacity(granted.len() + own.len());
    for (_, id) in granted {
        updates.push((id, g.issue()?));
    }
    updates.extend(own.into_iter().map(|id| (id, new_object_ts)));
    for (id, ts) in updates {
        effects.set_timestamp(id, ts)?;
    }
    *ids = g;
    Ok(())
}
