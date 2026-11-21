// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `validate_deck`: size, copies, sideboard, ban list, digital-only cards and card support
//! policy (REQ-FMT-001, REQ-ING-008, REQ-FMT-004). Pure and deterministic; only reads
//! support status (R11).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_dsl::card::{Legality, SupportStatus};
use serde::{Deserialize, Serialize};

use super::banlist::{BanListRef, BanListSource, CardCatalog};
use super::config::{FormatConfig, PolicyAction};

/// One line of a deck list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckEntry {
    /// Card definition id.
    pub card_def_id: String,
    /// Number of copies.
    pub count: u32,
}

/// A submitted deck.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckSubmission {
    /// Main deck.
    pub main: Vec<DeckEntry>,
    /// Sideboard.
    pub sideboard: Vec<DeckEntry>,
}

/// Severity of an issue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueSeverity {
    /// Makes the deck invalid.
    Error,
    /// Does not affect validity.
    Warning,
}

/// Machine-readable issue code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeckIssueCode {
    /// `format.check()` failed.
    InvalidFormatConfig,
    /// An entry with count 0.
    InvalidCount,
    /// Main deck below the minimum.
    MainTooSmall,
    /// Main deck above the maximum.
    MainTooLarge,
    /// Sideboard above the maximum.
    SideboardTooLarge,
    /// More copies than allowed.
    TooManyCopies,
    /// Card is banned.
    Banned,
    /// Card is not legal in the format.
    NotLegal,
    /// The referenced ban list could not be resolved.
    BanListUnavailable,
    /// Digital-only card not permitted.
    DigitalOnlyCard,
    /// Card id not in the database.
    UnknownCard,
    /// Card is UNSUPPORTED.
    UnsupportedCard,
    /// Card is PARTIAL.
    PartialCard,
    /// Card is below the minimum support status.
    BelowMinSupport,
}

impl DeckIssueCode {
    /// The wire string of this code.
    pub fn as_str(self) -> &'static str {
        match self {
            DeckIssueCode::InvalidFormatConfig => "INVALID_FORMAT_CONFIG",
            DeckIssueCode::InvalidCount => "INVALID_COUNT",
            DeckIssueCode::MainTooSmall => "MAIN_TOO_SMALL",
            DeckIssueCode::MainTooLarge => "MAIN_TOO_LARGE",
            DeckIssueCode::SideboardTooLarge => "SIDEBOARD_TOO_LARGE",
            DeckIssueCode::TooManyCopies => "TOO_MANY_COPIES",
            DeckIssueCode::Banned => "BANNED",
            DeckIssueCode::NotLegal => "NOT_LEGAL",
            DeckIssueCode::BanListUnavailable => "BAN_LIST_UNAVAILABLE",
            DeckIssueCode::DigitalOnlyCard => "DIGITAL_ONLY_CARD",
            DeckIssueCode::UnknownCard => "UNKNOWN_CARD",
            DeckIssueCode::UnsupportedCard => "UNSUPPORTED_CARD",
            DeckIssueCode::PartialCard => "PARTIAL_CARD",
            DeckIssueCode::BelowMinSupport => "BELOW_MIN_SUPPORT",
        }
    }
}

/// One validation finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckIssue {
    /// Issue code.
    pub code: DeckIssueCode,
    /// Severity.
    pub severity: IssueSeverity,
    /// The card concerned, if any.
    pub card_def_id: Option<String>,
    /// Human text built from ids and numbers only.
    pub message: String,
}

/// Result of [`validate_deck`]; `valid` is `errors.is_empty()`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckValidationResult {
    /// True when there are no errors.
    pub valid: bool,
    /// Error-severity issues in emission order.
    pub errors: Vec<DeckIssue>,
    /// Warning-severity issues in emission order.
    pub warnings: Vec<DeckIssue>,
}

fn issue(
    code: DeckIssueCode,
    severity: IssueSeverity,
    id: Option<&str>,
    message: String,
) -> DeckIssue {
    DeckIssue {
        code,
        severity,
        card_def_id: id.map(str::to_string),
        message,
    }
}

fn action_severity(a: PolicyAction) -> IssueSeverity {
    match a {
        PolicyAction::Reject => IssueSeverity::Error,
        PolicyAction::Warn => IssueSeverity::Warning,
    }
}

/// Merges duplicate ids with saturating u64 sums; zero-count entries raise InvalidCount.
fn merge(entries: &[DeckEntry], out: &mut Vec<DeckIssue>) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    let mut zero = BTreeSet::new();
    for e in entries {
        if e.count == 0 {
            zero.insert(e.card_def_id.as_str());
            continue;
        }
        let c = m.entry(e.card_def_id.clone()).or_insert(0u64);
        *c = c.saturating_add(u64::from(e.count));
    }
    for id in zero {
        out.push(issue(
            DeckIssueCode::InvalidCount,
            IssueSeverity::Error,
            Some(id),
            format!("entry for {id} has count 0"),
        ));
    }
    m
}

/// Validates `deck` against `format`: size, sideboard, copies, ban list, digital-only cards
/// and card support policy. Color identity and partners are deferred (REQ-FMT-002).
pub fn validate_deck(
    deck: &DeckSubmission,
    format: &FormatConfig,
    catalog: &dyn CardCatalog,
    banlists: &dyn BanListSource,
) -> DeckValidationResult {
    let mut all: Vec<DeckIssue> = Vec::new();
    if format.check().is_err() {
        all.push(issue(
            DeckIssueCode::InvalidFormatConfig,
            IssueSeverity::Error,
            None,
            "format configuration failed its self-check".to_string(),
        ));
        return finish(all);
    }
    // Step 1.
    let main = merge(&deck.main, &mut all);
    let side = merge(&deck.sideboard, &mut all);
    // Step 2.
    let main_total = main.values().fold(0u64, |a, c| a.saturating_add(*c));
    let side_total = side.values().fold(0u64, |a, c| a.saturating_add(*c));
    if main_total < u64::from(format.deck.min_main) {
        all.push(issue(
            DeckIssueCode::MainTooSmall,
            IssueSeverity::Error,
            None,
            format!(
                "main deck has {main_total} cards, minimum {}",
                format.deck.min_main
            ),
        ));
    }
    if let Some(m) = format.deck.max_main.filter(|m| main_total > u64::from(*m)) {
        all.push(issue(
            DeckIssueCode::MainTooLarge,
            IssueSeverity::Error,
            None,
            format!("main deck has {main_total} cards, maximum {m}"),
        ));
    }
    if side_total > u64::from(format.deck.max_sideboard) {
        all.push(issue(
            DeckIssueCode::SideboardTooLarge,
            IssueSeverity::Error,
            None,
            format!(
                "sideboard has {side_total} cards, maximum {}",
                format.deck.max_sideboard
            ),
        ));
    }
    // Step 3.
    let mut ban_list = None;
    if let Some(s) = &format.banlist_ref {
        let found = BanListRef::parse(s).ok().and_then(|r| {
            banlists
                .ban_list(&r)
                .filter(|l| l.name == r.name && l.version == r.version)
        });
        match found {
            Some(l) => ban_list = Some(l),
            None => all.push(issue(
                DeckIssueCode::BanListUnavailable,
                IssueSeverity::Error,
                None,
                format!("ban list {s} is unavailable"),
            )),
        }
    }
    // Step 4.
    let ids: BTreeSet<&String> = main.keys().chain(side.keys()).collect();
    let policy = &format.card_policy;
    for id in ids {
        let id = id.as_str();
        let mut restricted = false;
        if let Some(l) = ban_list {
            match l.entries.get(id) {
                Some(Legality::Banned) => all.push(issue(
                    DeckIssueCode::Banned,
                    IssueSeverity::Error,
                    Some(id),
                    format!("{id} is banned"),
                )),
                Some(Legality::NotLegal) => all.push(issue(
                    DeckIssueCode::NotLegal,
                    IssueSeverity::Error,
                    Some(id),
                    format!("{id} is not legal in this format"),
                )),
                Some(Legality::Restricted) => restricted = true,
                Some(Legality::Legal) | None => {}
            }
        }
        let facts = catalog.card_facts(id);
        let total = main
            .get(id)
            .copied()
            .unwrap_or(0)
            .saturating_add(side.get(id).copied().unwrap_or(0));
        let limit = if restricted {
            1
        } else {
            u64::from(format.deck.max_copies)
        };
        let exempt = format.deck.basic_land_exempt && facts.is_some_and(|f| f.is_basic_land);
        if !exempt && total > limit {
            all.push(issue(
                DeckIssueCode::TooManyCopies,
                IssueSeverity::Error,
                Some(id),
                format!("{total} copies of {id}, limit {limit}"),
            ));
        }
        let Some(f) = facts else {
            all.push(issue(
                DeckIssueCode::UnknownCard,
                IssueSeverity::Error,
                Some(id),
                format!("{id} is not in the card database"),
            ));
            continue;
        };
        if f.digital_only && !(format.digital_extensions.enabled && policy.allow_digital_only) {
            all.push(issue(
                DeckIssueCode::DigitalOnlyCard,
                IssueSeverity::Error,
                Some(id),
                format!("{id} is digital-only"),
            ));
        }
        match f.support_status {
            SupportStatus::Unsupported => all.push(issue(
                DeckIssueCode::UnsupportedCard,
                action_severity(policy.on_unsupported),
                Some(id),
                format!("{id} has no usable definition"),
            )),
            SupportStatus::Partial => all.push(issue(
                DeckIssueCode::PartialCard,
                action_severity(policy.on_partial),
                Some(id),
                format!("{id} has a partial definition"),
            )),
            SupportStatus::AutoParsed if policy.min_support_status == SupportStatus::Verified => {
                all.push(issue(
                    DeckIssueCode::BelowMinSupport,
                    IssueSeverity::Error,
                    Some(id),
                    format!("{id} is below the minimum support status"),
                ))
            }
            SupportStatus::AutoParsed | SupportStatus::Verified => {}
        }
    }
    finish(all)
}

/// Step 5: split by severity, preserving emission order.
fn finish(all: Vec<DeckIssue>) -> DeckValidationResult {
    let (errors, warnings): (Vec<_>, Vec<_>) = all
        .into_iter()
        .partition(|i| i.severity == IssueSeverity::Error);
    DeckValidationResult {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}
