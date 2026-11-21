// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `fineprint_core::format` spec tests (REQ-FMT-001, REQ-ING-008,
//! REQ-FMT-004). Black-box: written from the work-item brief only.
//!
//! Each `spec_format_*.rs` file includes this file with `#[path]`. All data is synthetic (R5).

#![allow(dead_code)]

use std::collections::BTreeMap;

use fineprint_core::format::*;
use fineprint_dsl::card::{Legality, SupportStatus};

/// The SRS section 11.1 example, verbatim.
pub const SRS_EXAMPLE: &str = r#"
{ "formatId": "commander", "rulesBaseline": "CR-2026-11-06",
  "deck": { "minMain": 100, "maxMain": 100, "maxSideboard": 0, "maxCopies": 1, "basicLandExempt": true, "enforceColorIdentity": true },
  "players": { "min": 2, "max": 6, "teams": null, "sharedTeamTurns": false, "rangeOfInfluence": null, "attackMultiplePlayers": true },
  "startingLife": 40, "startingHandSize": 7,
  "mulligan": { "rule": "LONDON", "freeFirstMulliganMultiplayer": true },
  "firstTurnDrawRule": "CR_DEFAULT",
  "commander": { "enabled": true, "damageThreshold": 21, "taxPerCast": 2, "removePerpetualOnCommandZone": true },
  "cardPolicy": { "minSupportStatus": "AUTO_PARSED", "onPartial": "WARN", "onUnsupported": "REJECT", "allowDigitalOnly": false },
  "timeoutPolicy": "AUTO_PASS_THEN_DEFAULT_CHOICE", "deckListsOpen": false, "ante": false,
  "digitalExtensions": { "enabled": false, "set": [] },
  "banlistRef": "banlists/commander@2026-11-10" }
"#;

/// Deterministic xorshift64 (constant seeds only; no clock).
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Uniform-ish value in `0..n` (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
    pub fn flip(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

/// BTreeMap-backed catalog.
#[derive(Default)]
pub struct TestCatalog(pub BTreeMap<String, CardFacts>);

impl TestCatalog {
    pub fn add(&mut self, id: &str, status: SupportStatus, digital_only: bool, basic: bool) {
        self.0.insert(
            id.to_string(),
            CardFacts {
                support_status: status,
                digital_only,
                is_basic_land: basic,
            },
        );
    }
}

impl CardCatalog for TestCatalog {
    fn card_facts(&self, card_def_id: &str) -> Option<CardFacts> {
        self.0.get(card_def_id).copied()
    }
}

/// BTreeMap-backed ban-list source keyed by the textual reference.
#[derive(Default)]
pub struct TestBanLists(pub BTreeMap<String, BanList>);

impl BanListSource for TestBanLists {
    fn ban_list(&self, r: &BanListRef) -> Option<&BanList> {
        self.0.get(&r.to_string())
    }
}

pub const BANNED: &str = "test.banned_card";
pub const NOT_LEGAL: &str = "test.notlegal_card";
pub const RESTRICTED: &str = "test.restricted_card";
pub const CARD_A: &str = "test.card_a";
pub const CARD_B: &str = "test.card_b";
pub const BASIC: &str = "test.basic_land";
pub const DIGITAL: &str = "test.digital_card";
pub const PARTIAL: &str = "test.partial_card";
pub const UNSUPPORTED: &str = "test.unsupported_card";
pub const AUTO: &str = "test.auto_card";
pub const UNKNOWN: &str = "test.not_in_database";
/// Unsupported, digital-only and listed Banned: exercises the per-card issue order.
pub const MULTI: &str = "test.multi_issue";

/// Number of generated filler cards `test.card_00 ..`.
pub const FILLER: u32 = 40;

pub fn filler_id(i: u32) -> String {
    format!("test.card_{i:02}")
}

pub fn catalog() -> TestCatalog {
    let mut c = TestCatalog::default();
    c.add(CARD_A, SupportStatus::Verified, false, false);
    c.add(CARD_B, SupportStatus::Verified, false, false);
    c.add(BASIC, SupportStatus::Verified, false, true);
    c.add(DIGITAL, SupportStatus::Verified, true, false);
    c.add(PARTIAL, SupportStatus::Partial, false, false);
    c.add(UNSUPPORTED, SupportStatus::Unsupported, false, false);
    c.add(AUTO, SupportStatus::AutoParsed, false, false);
    c.add(BANNED, SupportStatus::Verified, false, false);
    c.add(NOT_LEGAL, SupportStatus::Verified, false, false);
    c.add(RESTRICTED, SupportStatus::Verified, false, false);
    c.add(MULTI, SupportStatus::Unsupported, true, false);
    for i in 0..FILLER {
        c.add(&filler_id(i), SupportStatus::Verified, false, false);
    }
    c
}

pub fn ban_list_for(id: &str, entries: &[(&str, Legality)]) -> BanList {
    BanList {
        name: id.to_string(),
        version: PRESET_BANLIST_VERSION.to_string(),
        entries: entries.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
    }
}

/// Ban lists matching the presets' references; "standard" carries the synthetic entries.
pub fn banlists() -> TestBanLists {
    let mut b = TestBanLists::default();
    for id in PRESET_IDS {
        let entries: Vec<(&str, Legality)> = if id == "standard" {
            vec![
                (BANNED, Legality::Banned),
                (NOT_LEGAL, Legality::NotLegal),
                (RESTRICTED, Legality::Restricted),
                (MULTI, Legality::Banned),
                (CARD_B, Legality::Legal),
            ]
        } else {
            vec![]
        };
        b.0.insert(
            format!("banlists/{id}@{PRESET_BANLIST_VERSION}"),
            ban_list_for(id, &entries),
        );
    }
    b
}

pub fn entry(id: &str, count: u32) -> DeckEntry {
    DeckEntry {
        card_def_id: id.to_string(),
        count,
    }
}

pub fn entries(list: &[(&str, u32)]) -> Vec<DeckEntry> {
    list.iter().map(|(i, c)| entry(i, *c)).collect()
}

pub fn deck(main: &[(&str, u32)], side: &[(&str, u32)]) -> DeckSubmission {
    DeckSubmission {
        main: entries(main),
        sideboard: entries(side),
    }
}

/// 60 copies of the basic land.
pub fn legal_deck() -> DeckSubmission {
    deck(&[(BASIC, 60)], &[])
}

pub fn standard() -> FormatConfig {
    preset("standard").unwrap()
}

pub fn run(d: &DeckSubmission, f: &FormatConfig) -> DeckValidationResult {
    validate_deck(d, f, &catalog(), &banlists())
}

/// Wire strings of the issue codes, in order (DeckIssueCode is compared via `as_str`).
pub fn codes(issues: &[DeckIssue]) -> Vec<String> {
    issues.iter().map(|i| i.code.as_str().to_string()).collect()
}

pub fn has(issues: &[DeckIssue], code: &str, id: Option<&str>) -> bool {
    issues
        .iter()
        .any(|i| i.code.as_str() == code && i.card_def_id.as_deref() == id)
}

pub fn count(issues: &[DeckIssue], code: &str) -> usize {
    issues.iter().filter(|i| i.code.as_str() == code).count()
}
