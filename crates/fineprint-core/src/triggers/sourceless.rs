// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Player-scoped inherent triggered abilities with no source (REQ-EVENT-002, SRS 8.2 item 6):
//! the monarch (CR 726.2), the initiative (CR 727.2), rad counters (CR 729.1) and speed
//! (CR 702.179d). Each is an exception to CR 113.8: the ability has no source object and is
//! controlled by a player.

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::PlayerId;

use super::event::EventField;
use super::place::{PendingTrigger, TriggerOrigin};

/// The designation (or player counter, or speed) under which sourceless triggers are
/// registered per player (REQ-EVENT-002; CR 726.2, CR 727.2, CR 729.1, CR 702.179d).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourcelessKind {
    /// Being the monarch (CR 726.2).
    Monarch,
    /// Having the initiative (CR 727.2).
    Initiative,
    /// Having rad counters (CR 729.1).
    RadCounters,
    /// Having 1 or more speed (CR 702.179d).
    Speed,
}

/// One inherent sourceless triggered ability (REQ-EVENT-002): two for the monarch (CR 726.2),
/// three for the initiative (CR 727.2), one for rad counters (CR 729.1) and one for speed
/// (CR 702.179d).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourcelessAbility {
    /// "At the beginning of the monarch's end step, that player draws a card" (CR 726.2).
    MonarchEndStep,
    /// "Whenever a creature deals combat damage to the monarch, its controller becomes the
    /// monarch" (CR 726.2).
    MonarchCombatDamage,
    /// "At the beginning of the upkeep of the player who has the initiative, that player
    /// ventures into Undercity" (CR 727.2).
    InitiativeUpkeep,
    /// "Whenever one or more creatures a player controls deal combat damage to the player who
    /// has the initiative, the controller of those creatures takes the initiative" (CR 727.2).
    InitiativeCombatDamage,
    /// "Whenever a player takes the initiative, that player ventures into Undercity"
    /// (CR 727.2).
    InitiativeTaken,
    /// The rad-counter ability at the beginning of each player's precombat main phase
    /// (CR 729.1).
    RadCounters,
    /// The speed ability (CR 702.179d).
    Speed,
}

impl SourcelessAbility {
    /// The designation this ability belongs to.
    #[must_use]
    pub fn kind(self) -> SourcelessKind {
        match self {
            SourcelessAbility::MonarchEndStep | SourcelessAbility::MonarchCombatDamage => {
                SourcelessKind::Monarch
            }
            SourcelessAbility::InitiativeUpkeep
            | SourcelessAbility::InitiativeCombatDamage
            | SourcelessAbility::InitiativeTaken => SourcelessKind::Initiative,
            SourcelessAbility::RadCounters => SourcelessKind::RadCounters,
            SourcelessAbility::Speed => SourcelessKind::Speed,
        }
    }

    /// The stable ability id of the pending trigger (`sourceless.<kind>[.<which>]`).
    #[must_use]
    pub fn ability_id(self) -> &'static str {
        match self {
            SourcelessAbility::MonarchEndStep => "sourceless.monarch.end_step",
            SourcelessAbility::MonarchCombatDamage => "sourceless.monarch.combat_damage",
            SourcelessAbility::InitiativeUpkeep => "sourceless.initiative.upkeep",
            SourcelessAbility::InitiativeCombatDamage => "sourceless.initiative.combat_damage",
            SourcelessAbility::InitiativeTaken => "sourceless.initiative.taken",
            SourcelessAbility::RadCounters => "sourceless.rad_counters",
            SourcelessAbility::Speed => "sourceless.speed",
        }
    }
}

/// The player-scoped registrations of sourceless triggers (REQ-EVENT-002, SRS 8.2 item 6):
/// which player holds which designation.
///
/// **Holder and controller per ability** (what the caller passes to
/// [`SourcelessTriggers::fire`]):
///
/// - [`SourcelessAbility::MonarchEndStep`], [`SourcelessAbility::MonarchCombatDamage`]: holder
///   and controller are the player who was the monarch when the ability triggered (CR 726.2);
/// - [`SourcelessAbility::InitiativeUpkeep`], [`SourcelessAbility::InitiativeCombatDamage`],
///   [`SourcelessAbility::InitiativeTaken`]: holder and controller are the player who had the
///   initiative when the ability triggered (CR 727.2);
/// - [`SourcelessAbility::Speed`]: holder and controller are the player with the speed
///   (CR 702.179d);
/// - [`SourcelessAbility::RadCounters`]: the holder is the player with the rad counters whose
///   precombat main phase begins, and the controller is the **active** player (CR 729.1).
///
/// **Not tracked here** (the caller or the integration unit does it): that only one player is
/// the monarch or has the initiative at a time (CR 726.3, CR 727.3; call
/// [`SourcelessTriggers::unregister`] for the previous holder); the intervening "if" clauses
/// ("if that player has one or more rad counters", CR 729.1; "if your speed is less than 4",
/// CR 702.179d, checked per CR 603.4); and "This ability triggers only once each turn" of the
/// speed ability (CR 702.179d).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourcelessTriggers {
    registered: BTreeSet<(PlayerId, SourcelessKind)>,
}

impl SourcelessTriggers {
    /// No registration.
    #[must_use]
    pub fn new() -> SourcelessTriggers {
        SourcelessTriggers::default()
    }

    /// Registers `kind` for `holder`; true iff it was not registered yet.
    pub fn register(&mut self, holder: PlayerId, kind: SourcelessKind) -> bool {
        self.registered.insert((holder, kind))
    }

    /// Removes the registration of `kind` for `holder`; true iff it was present.
    pub fn unregister(&mut self, holder: PlayerId, kind: SourcelessKind) -> bool {
        self.registered.remove(&(holder, kind))
    }

    /// Whether `kind` is registered for `holder`.
    #[must_use]
    pub fn is_registered(&self, holder: PlayerId, kind: SourcelessKind) -> bool {
        self.registered.contains(&(holder, kind))
    }

    /// The players `kind` is registered for, ascending.
    #[must_use]
    pub fn players_with(&self, kind: SourcelessKind) -> Vec<PlayerId> {
        self.registered
            .iter()
            .filter(|(_, k)| *k == kind)
            .map(|(p, _)| *p)
            .collect()
    }

    /// The pending trigger of `ability` for `holder`, controlled by `controller`, or `None` if
    /// `ability.kind()` is not registered for `holder` (CR 113.8 exception: no source; CR
    /// 726.2, CR 727.2, CR 729.1, CR 702.179d for the controller; see the type docs for who
    /// the caller passes). The trigger binds `"player"` to the holder and has no option, no
    /// targets and no modes.
    #[must_use]
    pub fn fire(
        &self,
        holder: PlayerId,
        controller: PlayerId,
        ability: SourcelessAbility,
    ) -> Option<PendingTrigger> {
        if !self.is_registered(holder, ability.kind()) {
            return None;
        }
        let mut bindings = BTreeMap::new();
        bindings.insert("player".to_owned(), EventField::Player(holder));
        Some(PendingTrigger {
            origin: TriggerOrigin::Sourceless(ability),
            controller,
            source: None,
            ability_id: ability.ability_id().to_owned(),
            bindings,
            optional: false,
            responds_to_trigger: false,
            targets: Vec::new(),
            modes: None,
        })
    }
}
