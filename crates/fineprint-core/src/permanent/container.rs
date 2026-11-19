// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The permanent container (REQ-ZONE-001 item 3, REQ-CORE-006).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::encode::{CanonicalEncode, put_len};
use fineprint_base::ids::{ObjectId, PlayerId};

use super::error::PermanentError;
use super::types::{AttachTarget, AttackState, AttackTarget, CombatState, Permanent};
use crate::state::DesignationValue;

/// The non-characteristic state of every permanent on the battlefield (REQ-ZONE-001 item 3).
///
/// Keyed by [`ObjectId`] in a `BTreeMap`, so every iteration is in ascending id order
/// (REQ-CORE-006). Attachment links never form a cycle. An operation that returns an error
/// leaves the container unchanged. `clone()` gives an independent copy; equality compares
/// values.
///
/// **Omniscient state.** The accessors and the canonical encoding expose face-down status and
/// every designation; the container stays behind the redacted per-player view (REQ-CORE-004,
/// REQ-API-004).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Permanents {
    pub(super) permanents: BTreeMap<ObjectId, Permanent>,
}

impl Permanents {
    /// An empty container.
    #[must_use]
    pub fn new() -> Permanents {
        Permanents::default()
    }

    fn get_mut(&mut self, id: ObjectId) -> Result<&mut Permanent, PermanentError> {
        self.permanents
            .get_mut(&id)
            .ok_or(PermanentError::UnknownObject(id))
    }

    /// Records that object `id` entered the battlefield under `controller`'s control on turn
    /// `turn`: untapped, unflipped, face up and phased in (CR 110.5b), with no designations, no
    /// attachment and default combat state, controlled since `turn` (CR 110.1, CR 110.2).
    ///
    /// # Errors
    ///
    /// [`PermanentError::AlreadyPresent`] if `id` is already held.
    pub fn enter(
        &mut self,
        id: ObjectId,
        controller: PlayerId,
        turn: u64,
    ) -> Result<(), PermanentError> {
        if self.permanents.contains_key(&id) {
            return Err(PermanentError::AlreadyPresent(id));
        }
        // CR 110.1, CR 110.2, CR 110.5b: a new permanent with default status.
        self.permanents
            .insert(id, Permanent::new(id, controller, turn));
        Ok(())
    }

    /// Removes permanent `id` as it leaves the battlefield and returns its pre-removal value
    /// (CR 110.1, CR 400.7); it is removed from combat with it (CR 506.4). Then every permanent
    /// attached to it becomes unattached (CR 701.3d), its id is removed from every other
    /// permanent's blocking set, and every attacker attacking it becomes `Attacking(None)`
    /// (CR 506.4, CR 506.4c). Other permanents' `blocked` flags are not changed (CR 509.1h).
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn leave(&mut self, id: ObjectId) -> Result<Permanent, PermanentError> {
        // CR 400.7: the object stops being this permanent; nothing of it remains.
        let removed = self
            .permanents
            .remove(&id)
            .ok_or(PermanentError::UnknownObject(id))?;
        for other in self.permanents.values_mut() {
            // CR 701.3d: attachments to an object that leaves the battlefield end.
            if other.attached_to == Some(AttachTarget::Permanent(id)) {
                other.attached_to = None;
            }
            // CR 506.4: the leaving permanent is removed from combat; blockers no longer block
            // it. CR 509.1h: the attacker's `blocked` flag (here, its own) is untouched.
            other.combat.blocking.remove(&id);
        }
        self.redirect_attackers_of(id);
        Ok(removed)
    }

    /// CR 506.4, CR 506.4c: every permanent other than `id` attacking permanent `id` keeps
    /// attacking, but attacks nothing.
    fn redirect_attackers_of(&mut self, id: ObjectId) {
        let attacked = AttackState::Attacking(Some(AttackTarget::Permanent(id)));
        for (other_id, other) in self.permanents.iter_mut() {
            if *other_id != id && other.combat.attack == attacked {
                other.combat.attack = AttackState::Attacking(None);
            }
        }
    }

    /// The state of permanent `id`, if held.
    #[must_use]
    pub fn get(&self, id: ObjectId) -> Option<&Permanent> {
        self.permanents.get(&id)
    }

    /// Every held id, ascending.
    #[must_use]
    pub fn ids(&self) -> Vec<ObjectId> {
        self.permanents.keys().copied().collect()
    }

    /// The number of permanents held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.permanents.len()
    }

    /// Whether no permanent is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.permanents.is_empty()
    }

    /// Taps (`true`) or untaps (`false`) permanent `id`. Never touches combat state
    /// (CR 506.4b).
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn set_tapped(&mut self, id: ObjectId, v: bool) -> Result<(), PermanentError> {
        // CR 110.5, CR 506.4b.
        self.get_mut(id)?.status.tapped = v;
        Ok(())
    }

    /// Flips (`true`) or unflips (`false`) permanent `id` (CR 110.5).
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn set_flipped(&mut self, id: ObjectId, v: bool) -> Result<(), PermanentError> {
        // CR 110.5.
        self.get_mut(id)?.status.flipped = v;
        Ok(())
    }

    /// Turns permanent `id` face down (`true`) or face up (`false`) (CR 110.5).
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn set_face_down(&mut self, id: ObjectId, v: bool) -> Result<(), PermanentError> {
        // CR 110.5.
        self.get_mut(id)?.status.face_down = v;
        Ok(())
    }

    /// Phases permanent `id` out (`true`) or in (`false`) (CR 110.5). A phased-in permanent
    /// that phases out is also removed from combat, exactly as by
    /// [`Permanents::remove_from_combat`] (CR 702.26b, CR 506.4). Setting `true` when already
    /// phased out, or setting `false`, changes only the flag. Never changes the controller or
    /// the controlled-since turn (CR 702.26d).
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn set_phased_out(&mut self, id: ObjectId, v: bool) -> Result<(), PermanentError> {
        let permanent = self.get_mut(id)?;
        let phases_out = v && !permanent.status.phased_out;
        // CR 110.5; CR 702.26d: no control or zone change.
        permanent.status.phased_out = v;
        if phases_out {
            // CR 702.26b, CR 506.4: a permanent that phases out is removed from combat.
            self.remove_from_combat(id)?;
        }
        Ok(())
    }

    /// Gives permanent `id` designation `key` with `value`, replacing any previous value. Any
    /// string is a valid key (an open registry).
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn set_designation(
        &mut self,
        id: ObjectId,
        key: &str,
        value: DesignationValue,
    ) -> Result<(), PermanentError> {
        self.get_mut(id)?.designations.insert(key.to_owned(), value);
        Ok(())
    }

    /// Removes designation `key` from permanent `id`, returning its previous value.
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn clear_designation(
        &mut self,
        id: ObjectId,
        key: &str,
    ) -> Result<Option<DesignationValue>, PermanentError> {
        Ok(self.get_mut(id)?.designations.remove(key))
    }

    /// Gives control of permanent `id` to `new_controller` on turn `turn` (CR 613.1b, the
    /// result of layer 2). For a different player the controller becomes `new_controller`, the
    /// controlled-since turn becomes `turn` (stored as given, even if not above the old value),
    /// and the permanent is removed from combat exactly as by
    /// [`Permanents::remove_from_combat`] (CR 506.4). For the same player this is a complete
    /// no-op.
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn set_controller(
        &mut self,
        id: ObjectId,
        new_controller: PlayerId,
        turn: u64,
    ) -> Result<(), PermanentError> {
        let permanent = self.get_mut(id)?;
        if permanent.controller == new_controller {
            return Ok(());
        }
        // CR 613.1b, CR 302.6: a new controller restarts continuous-control tracking.
        permanent.controller = new_controller;
        permanent.controlled_since = turn;
        // CR 506.4: a permanent whose controller changes is removed from combat.
        self.remove_from_combat(id)
    }

    /// Attaches permanent `id` to `target`, replacing any previous link; attaching to the
    /// current target changes nothing (CR 701.3b). Player targets are not validated.
    ///
    /// Checks, in order: `id` is held; a `Permanent(x)` target is held; `x != id`; following
    /// attachment links from `x` through permanent targets never reaches `id`.
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] for an unheld `id` or target permanent;
    /// [`PermanentError::SelfAttach`] if the target is `id` itself;
    /// [`PermanentError::AttachmentCycle`] if the link would close a cycle. The container is
    /// unchanged on error.
    pub fn attach(&mut self, id: ObjectId, target: AttachTarget) -> Result<(), PermanentError> {
        if !self.permanents.contains_key(&id) {
            return Err(PermanentError::UnknownObject(id));
        }
        if let AttachTarget::Permanent(x) = target {
            if !self.permanents.contains_key(&x) {
                return Err(PermanentError::UnknownObject(x));
            }
            if x == id {
                return Err(PermanentError::SelfAttach(id));
            }
            if self.chain_reaches(x, id) {
                return Err(PermanentError::AttachmentCycle {
                    object: id,
                    target: x,
                });
            }
        }
        // CR 701.3b: attaching to the current target is no change; the assignment is then
        // idempotent.
        self.get_mut(id)?.attached_to = Some(target);
        Ok(())
    }

    /// Whether following attachment links from `start` through permanent targets reaches
    /// `goal`. The walk is bounded by `len() + 1` steps, so it terminates even on a malformed
    /// graph.
    fn chain_reaches(&self, start: ObjectId, goal: ObjectId) -> bool {
        let mut current = start;
        for _ in 0..=self.permanents.len() {
            if current == goal {
                return true;
            }
            match self.permanents.get(&current).and_then(|p| p.attached_to) {
                Some(AttachTarget::Permanent(next)) => current = next,
                _ => return false,
            }
        }
        false
    }

    /// Unattaches permanent `id`, returning what it was attached to.
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn detach(&mut self, id: ObjectId) -> Result<Option<AttachTarget>, PermanentError> {
        Ok(self.get_mut(id)?.attached_to.take())
    }

    /// What permanent `id` is attached to; `None` if it is unattached or not held.
    #[must_use]
    pub fn attached_to(&self, id: ObjectId) -> Option<AttachTarget> {
        self.permanents.get(&id).and_then(|p| p.attached_to)
    }

    /// Every permanent attached to `target`, ascending.
    #[must_use]
    pub fn attachments_of(&self, target: AttachTarget) -> Vec<ObjectId> {
        self.permanents
            .iter()
            .filter(|(_, p)| p.attached_to == Some(target))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Sets what permanent `id` is attacking (CR 508.1k). The target is not validated (attack
    /// legality, CR 506.3, belongs to the combat unit); `removed_from_combat` is not changed.
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn set_attack(&mut self, id: ObjectId, attack: AttackState) -> Result<(), PermanentError> {
        self.get_mut(id)?.combat.attack = attack;
        Ok(())
    }

    /// Replaces the set of attackers permanent `id` blocks (CR 509.1g); duplicates collapse.
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] naming `id`, or the first listed attacker that is not
    /// held. The container is unchanged on error.
    pub fn set_blocking(
        &mut self,
        id: ObjectId,
        attackers: &[ObjectId],
    ) -> Result<(), PermanentError> {
        if !self.permanents.contains_key(&id) {
            return Err(PermanentError::UnknownObject(id));
        }
        if let Some(missing) = attackers.iter().find(|a| !self.permanents.contains_key(a)) {
            return Err(PermanentError::UnknownObject(*missing));
        }
        let set: BTreeSet<ObjectId> = attackers.iter().copied().collect();
        self.get_mut(id)?.combat.blocking = set;
        Ok(())
    }

    /// Marks permanent `id` blocked (`true`) or unblocked (`false`) (CR 509.1h).
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn set_blocked(&mut self, id: ObjectId, v: bool) -> Result<(), PermanentError> {
        self.get_mut(id)?.combat.blocked = v;
        Ok(())
    }

    /// Removes permanent `id` from combat (CR 506.4): it stops attacking, blocks nothing, is not
    /// blocked, and is marked removed from combat. Every other permanent attacking `id` becomes
    /// `Attacking(None)` (CR 506.4c). Other permanents' blocking sets and `blocked` flags are
    /// not changed (CR 509.1h).
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn remove_from_combat(&mut self, id: ObjectId) -> Result<(), PermanentError> {
        // CR 506.4: it stops being an attacking, blocking, blocked or unblocked creature.
        let combat = &mut self.get_mut(id)?.combat;
        combat.attack = AttackState::NotAttacking;
        combat.blocking.clear();
        combat.blocked = false;
        combat.removed_from_combat = true;
        // CR 506.4, CR 506.4c.
        self.redirect_attackers_of(id);
        Ok(())
    }

    /// Resets permanent `id`'s combat state to the default (end of combat, CR 508.1k,
    /// CR 509.1g); no other permanent changes.
    ///
    /// # Errors
    ///
    /// [`PermanentError::UnknownObject`] if `id` is not held.
    pub fn clear_combat(&mut self, id: ObjectId) -> Result<(), PermanentError> {
        self.get_mut(id)?.combat = CombatState::default();
        Ok(())
    }

    /// Resets every permanent's combat state to the default (end of combat, CR 508.1k,
    /// CR 509.1g).
    pub fn clear_all_combat(&mut self) {
        for permanent in self.permanents.values_mut() {
            permanent.combat = CombatState::default();
        }
    }
}

impl CanonicalEncode for Permanents {
    /// [`put_len`] of the permanent count, then each permanent in ascending [`ObjectId`] order
    /// (its id, controller, status, designations, controlled-since turn, attachment and combat
    /// state) (REQ-NFR-006, REQ-CORE-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_len(out, self.permanents.len());
        for permanent in self.permanents.values() {
            permanent.encode_canonical(out);
        }
    }
}
