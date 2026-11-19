// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The waiting-trigger queue (REQ-EVENT-002, CR 603.3).

use fineprint_base::ids::PlayerId;

use super::detect::WaitingTrigger;

/// Triggers that have triggered and wait to be put on the stack the next time a player would
/// receive priority (REQ-EVENT-002, CR 603.3), in creation order.
///
/// The queue does not order by APNAP: the two-part ordering of CR 603.3b is the stack unit's
/// job, which takes each controller's triggers with [`WaitingTriggers::take_for_controller`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WaitingTriggers {
    items: Vec<WaitingTrigger>,
}

impl WaitingTriggers {
    /// An empty queue.
    #[must_use]
    pub fn new() -> WaitingTriggers {
        WaitingTriggers::default()
    }

    /// Appends `triggers` in the given order.
    pub fn push_all(&mut self, triggers: Vec<WaitingTrigger>) {
        self.items.extend(triggers);
    }

    /// The number of waiting triggers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether no trigger is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The waiting triggers, in creation order.
    pub fn iter(&self) -> impl Iterator<Item = &WaitingTrigger> {
        self.items.iter()
    }

    /// Removes and returns every waiting trigger, in creation order.
    pub fn take_all(&mut self) -> Vec<WaitingTrigger> {
        std::mem::take(&mut self.items)
    }

    /// Removes and returns the triggers controlled by `player`, in creation order; the others
    /// stay, in their order.
    pub fn take_for_controller(&mut self, player: PlayerId) -> Vec<WaitingTrigger> {
        let (taken, kept) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|t| t.controller == player);
        self.items = kept;
        taken
    }
}
