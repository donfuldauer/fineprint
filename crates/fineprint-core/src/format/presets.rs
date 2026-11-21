// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Two-player constructed presets as data (REQ-FMT-001).

use super::config::{
    CardPolicy, DeckRules, DigitalExtensions, FirstTurnDrawRule, FormatConfig, MulliganRule,
    MulliganRules, PlayerRules, RULES_BASELINE, TimeoutPolicy,
};

/// Placeholder preset ids; the card lists behind them are data the client supplies.
pub const PRESET_IDS: [&str; 5] = ["standard", "pioneer", "modern", "legacy", "vintage"];

/// Version of the ban lists the presets reference (the SRS example version).
pub const PRESET_BANLIST_VERSION: &str = "2026-11-10";

/// The preset for `id`, or `None` when `id` is not in [`PRESET_IDS`]. Uses the casual card
/// policy.
pub fn preset(id: &str) -> Option<FormatConfig> {
    if !PRESET_IDS.contains(&id) {
        return None;
    }
    Some(FormatConfig {
        format_id: id.to_string(),
        rules_baseline: RULES_BASELINE.to_string(),
        deck: DeckRules {
            min_main: 60,
            max_main: None,
            max_sideboard: 15,
            max_copies: 4,
            basic_land_exempt: true,
            enforce_color_identity: false,
        },
        players: PlayerRules {
            min: 2,
            max: 2,
            teams: None,
            shared_team_turns: false,
            range_of_influence: None,
            attack_multiple_players: true,
        },
        starting_life: 20,
        starting_hand_size: 7,
        mulligan: MulliganRules {
            rule: MulliganRule::London,
            free_first_mulligan_multiplayer: false,
        },
        first_turn_draw_rule: FirstTurnDrawRule::CrDefault,
        commander: None,
        card_policy: CardPolicy::casual(),
        timeout_policy: TimeoutPolicy::AutoPassThenDefaultChoice,
        deck_lists_open: false,
        ante: false,
        digital_extensions: DigitalExtensions {
            enabled: false,
            set: Vec::new(),
        },
        banlist_ref: Some(format!("banlists/{id}@{PRESET_BANLIST_VERSION}")),
    })
}
