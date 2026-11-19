// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The look-back categories of CR 603.10a-g and their derivation from an event matcher
//! (REQ-EVENT-002).

use fineprint_dsl::ability::{EventKind, EventMatcher, FilterValue};
use fineprint_dsl::expr::Zone;

/// The look-back categories of CR 603.10a-g (REQ-EVENT-002). A trigger in one of these
/// categories is matched against the state immediately before the event (CR 603.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LookBackKind {
    /// A leaves-the-battlefield ability (CR 603.10a, CR 603.6c).
    LeavesBattlefield,
    /// An ability that triggers when a player sacrifices a permanent (CR 603.10a).
    Sacrificed,
    /// An ability that triggers when a card leaves a graveyard (CR 603.10a).
    LeavesGraveyard,
    /// An ability that triggers when an object all players can see is put into a hand or
    /// library (CR 603.10a).
    ToHandOrLibrary,
    /// An ability that triggers when a permanent phases out (CR 603.10b).
    PhasedOut,
    /// An ability that triggers specifically when an object becomes unattached (CR 603.10c).
    Unattached,
    /// An ability that triggers when a player loses control of an object, or an opponent gains
    /// control of it from that player (CR 603.10d).
    ControlLost,
    /// An ability that triggers when a spell is countered (CR 603.10e).
    Countered,
    /// An ability that triggers when a player loses the game (CR 603.10f).
    PlayerLost,
    /// An ability that triggers when a player planeswalks away from a plane (CR 603.10g).
    Planeswalked,
}

impl LookBackKind {
    /// The CR rule that makes this category look back: `"603.10a"` for the four zone-change
    /// categories, then `"603.10b"` to `"603.10g"`.
    #[must_use]
    pub fn rule(self) -> &'static str {
        match self {
            LookBackKind::LeavesBattlefield
            | LookBackKind::Sacrificed
            | LookBackKind::LeavesGraveyard
            | LookBackKind::ToHandOrLibrary => "603.10a",
            LookBackKind::PhasedOut => "603.10b",
            LookBackKind::Unattached => "603.10c",
            LookBackKind::ControlLost => "603.10d",
            LookBackKind::Countered => "603.10e",
            LookBackKind::PlayerLost => "603.10f",
            LookBackKind::Planeswalked => "603.10g",
        }
    }
}

/// Which game state a trigger is matched against (REQ-EVENT-002, CR 603.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateView {
    /// Immediately before the event: look-back triggers (CR 603.10a-g).
    PreEvent,
    /// Immediately after the event: every other trigger (CR 603.10, CR 603.6a).
    PostEvent,
}

/// The zone named by the `key` filter of `matcher`, if it is a [`FilterValue::Zone`].
fn zone_filter(matcher: &EventMatcher, key: &str) -> Option<Zone> {
    match matcher.filters.as_ref()?.get(key)? {
        FilterValue::Zone(z) => Some(*z),
        _ => None,
    }
}

/// Derives the look-back category of `matcher` from its kind and its `from`/`to` zone filters,
/// or `None` for a trigger matched against the post-event state (REQ-EVENT-002, CR 603.10,
/// SRS 8.2). The definition never sets it; `subject`, `bind` and `batch` do not affect it.
#[must_use]
pub fn look_back_kind(matcher: &EventMatcher) -> Option<LookBackKind> {
    let from = zone_filter(matcher, "from");
    let to = zone_filter(matcher, "to");
    match matcher.kind {
        // CR 603.10a, CR 603.6c: leaves-the-battlefield abilities.
        EventKind::LeavesBattlefield | EventKind::Dies => Some(LookBackKind::LeavesBattlefield),
        // CR 603.10a: a player sacrifices a permanent.
        EventKind::Sacrificed => Some(LookBackKind::Sacrificed),
        // CR 603.10a: a card leaves a graveyard.
        EventKind::LeavesGraveyard => Some(LookBackKind::LeavesGraveyard),
        // CR 603.10a: the zone filters decide the category.
        EventKind::ZoneChange => match (from, to) {
            (Some(Zone::Battlefield), _) => Some(LookBackKind::LeavesBattlefield),
            (Some(Zone::Graveyard), _) => Some(LookBackKind::LeavesGraveyard),
            (_, Some(Zone::Hand | Zone::Library)) => Some(LookBackKind::ToHandOrLibrary),
            _ => None,
        },
        // CR 603.6c: "from anywhere" is never a leaves-the-battlefield ability.
        EventKind::PutIntoGraveyard => {
            (from == Some(Zone::Battlefield)).then_some(LookBackKind::LeavesBattlefield)
        }
        // CR 603.10a
        EventKind::Exiled => match from {
            Some(Zone::Battlefield) => Some(LookBackKind::LeavesBattlefield),
            Some(Zone::Graveyard) => Some(LookBackKind::LeavesGraveyard),
            _ => None,
        },
        // CR 603.10a: a return to hand is always put into a hand.
        EventKind::ReturnedToHand => match from {
            Some(Zone::Battlefield) => Some(LookBackKind::LeavesBattlefield),
            Some(Zone::Graveyard) => Some(LookBackKind::LeavesGraveyard),
            _ => Some(LookBackKind::ToHandOrLibrary),
        },
        // CR 603.10b
        EventKind::PhasedOut => Some(LookBackKind::PhasedOut),
        // CR 603.10c
        EventKind::Unattached => Some(LookBackKind::Unattached),
        // CR 603.10d
        EventKind::ControlChanged => Some(LookBackKind::ControlLost),
        // CR 603.10e
        EventKind::Countered => Some(LookBackKind::Countered),
        // CR 603.10f
        EventKind::PlayerLost => Some(LookBackKind::PlayerLost),
        // CR 603.10g
        EventKind::Planeswalked => Some(LookBackKind::Planeswalked),
        // CR 603.10, CR 603.6a: everything else uses the post-event state (crew, saddle and
        // station activations included: CR 702.122e, 702.171c, 702.184a are not in the list).
        _ => None,
    }
}
