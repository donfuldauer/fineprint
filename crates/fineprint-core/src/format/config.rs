// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The `FormatConfig` type of SRS §11.1 and its sub-types (REQ-FMT-001).

use fineprint_dsl::card::SupportStatus;
use serde::{Deserialize, Serialize};

use super::banlist::BanListRef;

/// The rules baseline every preset names (SRS §11.1).
pub const RULES_BASELINE: &str = "CR-2026-11-06";

/// Everything a match takes from its format (SRS §11.1, REQ-FMT-001).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormatConfig {
    /// Format identifier.
    pub format_id: String,
    /// Rules baseline identifier.
    pub rules_baseline: String,
    /// Deck construction limits.
    pub deck: DeckRules,
    /// Player count and multiplayer options.
    pub players: PlayerRules,
    /// Starting life total.
    pub starting_life: i64,
    /// Starting hand size.
    pub starting_hand_size: u32,
    /// Mulligan settings.
    pub mulligan: MulliganRules,
    /// First-turn draw rule (`CrDefault` follows CR 103.8a-c per SRS §11.1).
    pub first_turn_draw_rule: FirstTurnDrawRule,
    /// Commander settings; JSON null when absent.
    pub commander: Option<CommanderRules>,
    /// Card coverage policy (REQ-ING-008).
    pub card_policy: CardPolicy,
    /// What happens when a player times out.
    pub timeout_policy: TimeoutPolicy,
    /// Whether deck lists are open.
    pub deck_lists_open: bool,
    /// Whether ante is played.
    pub ante: bool,
    /// Digital-only extensions (REQ-FMT-004).
    pub digital_extensions: DigitalExtensions,
    /// `banlists/<name>@<version>`; null means no ban list.
    pub banlist_ref: Option<String>,
}

/// Deck construction limits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckRules {
    /// Minimum main-deck size.
    pub min_main: u32,
    /// Maximum main-deck size; null means no maximum.
    pub max_main: Option<u32>,
    /// Maximum sideboard size.
    pub max_sideboard: u32,
    /// Maximum copies of one card (main plus sideboard).
    pub max_copies: u32,
    /// Whether basic lands are exempt from the copy limit.
    pub basic_land_exempt: bool,
    /// Stored only; not enforced by `validate_deck` (commanders are deferred, REQ-FMT-002).
    pub enforce_color_identity: bool,
}

/// Player count and multiplayer options.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerRules {
    /// Minimum players.
    pub min: u32,
    /// Maximum players.
    pub max: u32,
    /// Number of teams; null when not a team format.
    pub teams: Option<u32>,
    /// Whether teammates share turns.
    pub shared_team_turns: bool,
    /// Range of influence; null when unlimited.
    pub range_of_influence: Option<u32>,
    /// Whether a creature may attack several players.
    pub attack_multiple_players: bool,
}

/// Mulligan settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MulliganRules {
    /// Mulligan procedure.
    pub rule: MulliganRule,
    /// Whether the first multiplayer mulligan is free.
    pub free_first_mulligan_multiplayer: bool,
}

/// Mulligan procedure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MulliganRule {
    /// The London mulligan (`"LONDON"`).
    London,
}

/// First-turn draw rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FirstTurnDrawRule {
    /// Follows CR 103.8a-c (SRS §11.1) (`"CR_DEFAULT"`).
    CrDefault,
    /// House rule: nobody skips the draw (`"NEVER_SKIP"`).
    NeverSkip,
    /// House rule: the starting player always skips (`"ALWAYS_SKIP"`).
    AlwaysSkip,
}

/// Commander settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommanderRules {
    /// Whether the commander variant is on.
    pub enabled: bool,
    /// Commander damage that loses the game.
    pub damage_threshold: u32,
    /// Additional generic cost per previous cast.
    pub tax_per_cast: u32,
    /// Whether perpetual effects are removed in the command zone.
    pub remove_perpetual_on_command_zone: bool,
}

/// What to do with a card whose support status is incomplete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PolicyAction {
    /// Reject the deck (`"REJECT"`).
    Reject,
    /// Accept with a warning (`"WARN"`).
    Warn,
}

/// Card coverage policy (REQ-ING-008). There is no "allow": the engine never silently runs an
/// incomplete card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CardPolicy {
    /// Minimum support status; only `Verified` rejects `AutoParsed` cards.
    pub min_support_status: SupportStatus,
    /// Action for `Partial` cards.
    pub on_partial: PolicyAction,
    /// Action for `Unsupported` cards.
    pub on_unsupported: PolicyAction,
    /// Whether digital-only cards are allowed (also needs the digital extension).
    pub allow_digital_only: bool,
}

impl CardPolicy {
    /// The casual policy: AUTO_PARSED, warn on partial, reject unsupported, no digital-only.
    pub fn casual() -> Self {
        CardPolicy {
            min_support_status: SupportStatus::AutoParsed,
            on_partial: PolicyAction::Warn,
            on_unsupported: PolicyAction::Reject,
            allow_digital_only: false,
        }
    }

    /// The tournament policy: VERIFIED, reject partial and unsupported, no digital-only.
    pub fn tournament() -> Self {
        CardPolicy {
            min_support_status: SupportStatus::Verified,
            on_partial: PolicyAction::Reject,
            on_unsupported: PolicyAction::Reject,
            allow_digital_only: false,
        }
    }
}

/// Timeout behaviour (house-rule values beyond the SRS example).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TimeoutPolicy {
    /// Pass automatically (`"AUTO_PASS"`).
    AutoPass,
    /// Pass, then take the default choice (`"AUTO_PASS_THEN_DEFAULT_CHOICE"`).
    AutoPassThenDefaultChoice,
    /// The timed-out player loses (`"GAME_LOSS"`).
    GameLoss,
}

/// Digital-only extension settings (REQ-FMT-004).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DigitalExtensions {
    /// Whether digital extensions are on.
    pub enabled: bool,
    /// Extension identifiers.
    pub set: Vec<String>,
}

/// A failed [`FormatConfig::check`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FormatConfigError {
    /// `format_id` is empty.
    EmptyFormatId,
    /// `max_main` is below `min_main`.
    DeckSizeRange,
    /// `max_copies` is zero.
    ZeroMaxCopies,
    /// `players.min` is zero or above `players.max`.
    PlayerRange,
    /// `banlist_ref` does not parse; carries the offending text.
    BadBanlistRef(String),
}

impl FormatConfig {
    /// Self-check; returns the first failing check in declaration order of
    /// [`FormatConfigError`].
    pub fn check(&self) -> Result<(), FormatConfigError> {
        if self.format_id.is_empty() {
            return Err(FormatConfigError::EmptyFormatId);
        }
        if self.deck.max_main.is_some_and(|m| m < self.deck.min_main) {
            return Err(FormatConfigError::DeckSizeRange);
        }
        if self.deck.max_copies < 1 {
            return Err(FormatConfigError::ZeroMaxCopies);
        }
        if self.players.min < 1 || self.players.min > self.players.max {
            return Err(FormatConfigError::PlayerRange);
        }
        if let Some(r) = &self.banlist_ref {
            BanListRef::parse(r)?;
        }
        Ok(())
    }

    /// Returns `self` with the tournament card policy (SRS §11.1).
    pub fn with_tournament_policy(mut self) -> Self {
        self.card_policy = CardPolicy::tournament();
        self
    }
}
