// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Match configuration: [`FormatConfig`] (SRS §11.1), versioned ban-list references, the
//! two-player constructed presets as data and [`validate_deck`] (REQ-FMT-001, REQ-ING-008,
//! REQ-FMT-004).
//!
//! Every match starts from a [`FormatConfig`]; settings the rules leave to the format are
//! carried here and never hardcoded. Ban lists and card facts are supplied by the caller
//! through [`BanListSource`] and [`CardCatalog`]; no card data lives in this crate.

mod banlist;
mod config;
mod presets;
mod validate;

#[cfg(test)]
mod tests;

pub use banlist::{BanList, BanListRef, BanListSource, CardCatalog, CardFacts};
pub use config::{
    CardPolicy, CommanderRules, DeckRules, DigitalExtensions, FirstTurnDrawRule, FormatConfig,
    FormatConfigError, MulliganRule, MulliganRules, PlayerRules, PolicyAction, RULES_BASELINE,
    TimeoutPolicy,
};
pub use presets::{PRESET_BANLIST_VERSION, PRESET_IDS, preset};
pub use validate::{
    DeckEntry, DeckIssue, DeckIssueCode, DeckSubmission, DeckValidationResult, IssueSeverity,
    validate_deck,
};
