// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::format::validate_deck`. Black-box: written from the
//! work-item brief only.
//!
//! Covers REQ-FMT-001 (deck rules, ban lists as data), REQ-ING-008 (support-status policy) and
//! REQ-FMT-004 (digital-only cards).

#[path = "spec_format_support.rs"]
mod support;

use fineprint_core::format::*;
use fineprint_dsl::card::SupportStatus;
use support::*;

fn assert_ok(r: &DeckValidationResult) {
    assert!(r.valid, "errors: {:?}", codes(&r.errors));
    assert!(r.errors.is_empty());
}

// ---------------------------------------------------------------- codes / basics

/// REQ-FMT-001: DeckIssueCode wire strings.
#[test]
fn issue_code_strings() {
    let table = [
        (DeckIssueCode::InvalidFormatConfig, "INVALID_FORMAT_CONFIG"),
        (DeckIssueCode::InvalidCount, "INVALID_COUNT"),
        (DeckIssueCode::MainTooSmall, "MAIN_TOO_SMALL"),
        (DeckIssueCode::MainTooLarge, "MAIN_TOO_LARGE"),
        (DeckIssueCode::SideboardTooLarge, "SIDEBOARD_TOO_LARGE"),
        (DeckIssueCode::TooManyCopies, "TOO_MANY_COPIES"),
        (DeckIssueCode::Banned, "BANNED"),
        (DeckIssueCode::NotLegal, "NOT_LEGAL"),
        (DeckIssueCode::BanListUnavailable, "BAN_LIST_UNAVAILABLE"),
        (DeckIssueCode::DigitalOnlyCard, "DIGITAL_ONLY_CARD"),
        (DeckIssueCode::UnknownCard, "UNKNOWN_CARD"),
        (DeckIssueCode::UnsupportedCard, "UNSUPPORTED_CARD"),
        (DeckIssueCode::PartialCard, "PARTIAL_CARD"),
        (DeckIssueCode::BelowMinSupport, "BELOW_MIN_SUPPORT"),
    ];
    for (c, s) in table {
        assert_eq!(c.as_str(), s);
    }
}

/// REQ-FMT-001: a plain legal deck is valid with no issues.
#[test]
fn legal_deck_is_valid() {
    let r = run(&legal_deck(), &standard());
    assert_ok(&r);
    assert!(r.warnings.is_empty());
}

// ---------------------------------------------------------------- deck size

/// REQ-FMT-001: main deck too small / exactly min.
#[test]
fn main_size_min() {
    let r = run(&deck(&[(BASIC, 59)], &[]), &standard());
    assert!(!r.valid);
    assert_eq!(codes(&r.errors), ["MAIN_TOO_SMALL"]);
    assert_eq!(r.errors[0].card_def_id, None);
    assert_eq!(r.errors[0].severity, IssueSeverity::Error);
    assert_ok(&run(&deck(&[(BASIC, 60)], &[]), &standard()));
    // Empty deck.
    let r = run(&deck(&[], &[]), &standard());
    assert_eq!(codes(&r.errors), ["MAIN_TOO_SMALL"]);
}

/// REQ-FMT-001: main total spans many entries.
#[test]
fn main_size_sums_entries() {
    let mut main = Vec::new();
    for i in 0..15 {
        main.push((filler_id(i), 4u32));
    }
    let main_ref: Vec<(&str, u32)> = main.iter().map(|(s, c)| (s.as_str(), *c)).collect();
    assert_ok(&run(&deck(&main_ref, &[]), &standard()));
    let r = run(&deck(&main_ref[..14], &[]), &standard());
    assert_eq!(codes(&r.errors), ["MAIN_TOO_SMALL"]);
}

/// REQ-FMT-001: max_main Some exceeded; None unbounded.
#[test]
fn main_size_max() {
    let mut c = standard();
    c.deck.min_main = 40;
    c.deck.max_main = Some(60);
    assert_ok(&run(&deck(&[(BASIC, 60)], &[]), &c));
    let r = run(&deck(&[(BASIC, 61)], &[]), &c);
    assert_eq!(codes(&r.errors), ["MAIN_TOO_LARGE"]);
    assert_eq!(r.errors[0].card_def_id, None);
    // Unbounded.
    assert_ok(&run(&deck(&[(BASIC, 1000)], &[]), &standard()));
}

/// REQ-FMT-001: sideboard 15 ok, 16 too large.
#[test]
fn sideboard_size() {
    assert_ok(&run(&deck(&[(BASIC, 60)], &[(BASIC, 15)]), &standard()));
    let r = run(&deck(&[(BASIC, 60)], &[(BASIC, 16)]), &standard());
    assert_eq!(codes(&r.errors), ["SIDEBOARD_TOO_LARGE"]);
    assert_eq!(r.errors[0].card_def_id, None);
    // Spread over entries.
    let r = run(
        &deck(&[(BASIC, 60)], &[(CARD_A, 4), (CARD_B, 4), (BASIC, 8)]),
        &standard(),
    );
    assert_eq!(codes(&r.errors), ["SIDEBOARD_TOO_LARGE"]);
}

/// REQ-FMT-001: sideboard limit comes from the config.
#[test]
fn sideboard_limit_is_configurable() {
    let mut c = standard();
    c.deck.max_sideboard = 0;
    assert_ok(&run(&legal_deck(), &c));
    let r = run(&deck(&[(BASIC, 60)], &[(CARD_A, 1)]), &c);
    assert_eq!(codes(&r.errors), ["SIDEBOARD_TOO_LARGE"]);
}

// ---------------------------------------------------------------- copies

/// REQ-FMT-001: copies 4 ok, 5 too many.
#[test]
fn copies_limit() {
    let base = [(BASIC, 56)];
    let mut main = base.to_vec();
    main.push((CARD_A, 4));
    assert_ok(&run(&deck(&main, &[]), &standard()));
    main[1] = (CARD_A, 5);
    main[0] = (BASIC, 55);
    let r = run(&deck(&main, &[]), &standard());
    assert_eq!(codes(&r.errors), ["TOO_MANY_COPIES"]);
    assert_eq!(r.errors[0].card_def_id.as_deref(), Some(CARD_A));
}

/// REQ-FMT-001: copies are counted across main and sideboard combined.
#[test]
fn copies_combined_zones() {
    let r = run(
        &deck(&[(BASIC, 57), (CARD_A, 3)], &[(CARD_A, 2)]),
        &standard(),
    );
    assert_eq!(codes(&r.errors), ["TOO_MANY_COPIES"]);
    assert!(has(&r.errors, "TOO_MANY_COPIES", Some(CARD_A)));
    let r = run(
        &deck(&[(BASIC, 57), (CARD_A, 3)], &[(CARD_A, 1)]),
        &standard(),
    );
    assert_ok(&r);
}

/// REQ-FMT-001: max_copies is configurable.
#[test]
fn copies_configurable() {
    let mut c = standard();
    c.deck.max_copies = 1;
    let r = run(&deck(&[(BASIC, 58), (CARD_A, 2)], &[]), &c);
    assert_eq!(codes(&r.errors), ["TOO_MANY_COPIES"]);
    c.deck.max_copies = 2;
    assert_ok(&run(&deck(&[(BASIC, 58), (CARD_A, 2)], &[]), &c));
}

/// REQ-FMT-001: basic land exempt from the copy limit; not when exemption off; non-basic still limited.
#[test]
fn basic_land_exemption() {
    assert_ok(&run(&deck(&[(BASIC, 60)], &[]), &standard()));
    let mut c = standard();
    c.deck.basic_land_exempt = false;
    let r = run(&deck(&[(BASIC, 60)], &[]), &c);
    assert_eq!(codes(&r.errors), ["TOO_MANY_COPIES"]);
    assert_eq!(r.errors[0].card_def_id.as_deref(), Some(BASIC));
    // Non-basic still limited while exempt.
    let r = run(&deck(&[(BASIC, 55), (CARD_A, 5)], &[]), &standard());
    assert_eq!(codes(&r.errors), ["TOO_MANY_COPIES"]);
    // Basic spread over both zones is still exempt.
    assert_ok(&run(&deck(&[(BASIC, 60)], &[(BASIC, 15)]), &standard()));
}

// ---------------------------------------------------------------- ban list

/// REQ-FMT-001: Banned card -> Banned error.
#[test]
fn banned_card() {
    let r = run(&deck(&[(BASIC, 59), (BANNED, 1)], &[]), &standard());
    assert!(!r.valid);
    assert_eq!(codes(&r.errors), ["BANNED"]);
    assert_eq!(r.errors[0].card_def_id.as_deref(), Some(BANNED));
    // Banned in the sideboard too.
    let r = run(&deck(&[(BASIC, 60)], &[(BANNED, 1)]), &standard());
    assert_eq!(codes(&r.errors), ["BANNED"]);
}

/// REQ-FMT-001: NotLegal card -> NotLegal error.
#[test]
fn not_legal_card() {
    let r = run(&deck(&[(BASIC, 59), (NOT_LEGAL, 1)], &[]), &standard());
    assert_eq!(codes(&r.errors), ["NOT_LEGAL"]);
    assert_eq!(r.errors[0].card_def_id.as_deref(), Some(NOT_LEGAL));
}

/// REQ-FMT-001: Restricted: 1 copy ok, 2 copies error (even when max_copies is 4).
#[test]
fn restricted_card() {
    assert_ok(&run(
        &deck(&[(BASIC, 59), (RESTRICTED, 1)], &[]),
        &standard(),
    ));
    let r = run(&deck(&[(BASIC, 58), (RESTRICTED, 2)], &[]), &standard());
    assert_eq!(codes(&r.errors), ["TOO_MANY_COPIES"]);
    assert_eq!(r.errors[0].card_def_id.as_deref(), Some(RESTRICTED));
    // Combined across zones.
    let r = run(
        &deck(&[(BASIC, 59), (RESTRICTED, 1)], &[(RESTRICTED, 1)]),
        &standard(),
    );
    assert_eq!(codes(&r.errors), ["TOO_MANY_COPIES"]);
}

/// REQ-FMT-001: explicit Legal entry and absent entry are both fine.
#[test]
fn legal_and_absent_entries() {
    assert_ok(&run(&deck(&[(BASIC, 56), (CARD_B, 4)], &[]), &standard()));
    assert_ok(&run(&deck(&[(BASIC, 56), (CARD_A, 4)], &[]), &standard()));
}

/// REQ-FMT-001: banlist_ref None skips ban checks.
#[test]
fn banlist_ref_none_skips() {
    let mut c = standard();
    c.banlist_ref = None;
    let r = run(&deck(&[(BASIC, 58), (BANNED, 1), (NOT_LEGAL, 1)], &[]), &c);
    assert_ok(&r);
    assert!(r.warnings.is_empty());
}

/// REQ-FMT-001: missing list -> exactly one BanListUnavailable, no Banned error.
#[test]
fn ban_list_missing() {
    let mut c = standard();
    c.banlist_ref = Some("banlists/absent@1".into());
    let r = run(&deck(&[(BASIC, 58), (BANNED, 1), (NOT_LEGAL, 1)], &[]), &c);
    assert!(!r.valid);
    assert_eq!(codes(&r.errors), ["BAN_LIST_UNAVAILABLE"]);
    assert_eq!(r.errors[0].card_def_id, None);
}

/// REQ-FMT-001: name or version mismatch -> exactly one BanListUnavailable.
#[test]
fn ban_list_mismatch() {
    let c = standard();
    let key = format!("banlists/standard@{PRESET_BANLIST_VERSION}");
    for (name, version) in [
        ("other", PRESET_BANLIST_VERSION),
        ("standard", "1999-01-01"),
    ] {
        let mut lists = banlists();
        let mut l = ban_list_for(
            "standard",
            &[(BANNED, fineprint_dsl::card::Legality::Banned)],
        );
        l.name = name.into();
        l.version = version.into();
        lists.0.insert(key.clone(), l);
        let r = validate_deck(
            &deck(&[(BASIC, 59), (BANNED, 1)], &[]),
            &c,
            &catalog(),
            &lists,
        );
        assert_eq!(
            codes(&r.errors),
            ["BAN_LIST_UNAVAILABLE"],
            "{name}@{version}"
        );
    }
}

/// REQ-FMT-001: a malformed banlist_ref is an invalid config, not a ban-list problem.
#[test]
fn malformed_banlist_ref_is_invalid_config() {
    let mut c = standard();
    c.banlist_ref = Some("nonsense".into());
    let r = run(&legal_deck(), &c);
    assert_eq!(codes(&r.errors), ["INVALID_FORMAT_CONFIG"]);
}

/// REQ-FMT-001: ban lists are data: a different list changes the outcome with no code change.
#[test]
fn ban_data_drives_outcome() {
    let d = deck(&[(BASIC, 59), (CARD_A, 1)], &[]);
    assert_ok(&run(&d, &standard()));
    let mut lists = banlists();
    let key = format!("banlists/standard@{PRESET_BANLIST_VERSION}");
    lists.0.insert(
        key,
        ban_list_for(
            "standard",
            &[(CARD_A, fineprint_dsl::card::Legality::Banned)],
        ),
    );
    let r = validate_deck(&d, &standard(), &catalog(), &lists);
    assert_eq!(codes(&r.errors), ["BANNED"]);
}

// ---------------------------------------------------------------- digital only

/// REQ-FMT-004: digital-only card fails when extensions are disabled.
#[test]
fn digital_extension_disabled() {
    let d = deck(&[(BASIC, 59), (DIGITAL, 1)], &[]);
    let mut c = standard();
    c.card_policy.allow_digital_only = true; // policy permissive, extension off
    let r = run(&d, &c);
    assert!(!r.valid);
    assert_eq!(codes(&r.errors), ["DIGITAL_ONLY_CARD"]);
    assert_eq!(r.errors[0].card_def_id.as_deref(), Some(DIGITAL));
    // Default preset: both off.
    assert_eq!(codes(&run(&d, &standard()).errors), ["DIGITAL_ONLY_CARD"]);
}

/// REQ-FMT-004: extension enabled but allow_digital_only false -> still an error.
#[test]
fn digital_policy_disallows() {
    let d = deck(&[(BASIC, 59), (DIGITAL, 1)], &[]);
    let mut c = standard();
    c.digital_extensions.enabled = true;
    c.card_policy.allow_digital_only = false;
    assert_eq!(codes(&run(&d, &c).errors), ["DIGITAL_ONLY_CARD"]);
}

/// REQ-FMT-004: both permissive -> accepted.
#[test]
fn digital_both_permissive() {
    let d = deck(&[(BASIC, 59), (DIGITAL, 1)], &[]);
    let mut c = standard();
    c.digital_extensions.enabled = true;
    c.card_policy.allow_digital_only = true;
    let r = run(&d, &c);
    assert_ok(&r);
    assert!(r.warnings.is_empty());
}

/// REQ-FMT-004: non-digital cards are unaffected by the digital settings.
#[test]
fn non_digital_unaffected() {
    assert_ok(&run(&legal_deck(), &standard()));
}

// ---------------------------------------------------------------- support policy

/// REQ-ING-008: Unsupported rejected by default (casual) and under tournament policy.
#[test]
fn unsupported_rejected_by_default() {
    let d = deck(&[(BASIC, 59), (UNSUPPORTED, 1)], &[]);
    let r = run(&d, &standard());
    assert!(!r.valid);
    assert_eq!(codes(&r.errors), ["UNSUPPORTED_CARD"]);
    assert_eq!(r.errors[0].card_def_id.as_deref(), Some(UNSUPPORTED));
    assert!(r.warnings.is_empty());
    let r = run(&d, &standard().with_tournament_policy());
    assert_eq!(codes(&r.errors), ["UNSUPPORTED_CARD"]);
}

/// REQ-ING-008: Unsupported with on_unsupported Warn -> warning, valid.
#[test]
fn unsupported_warn_policy() {
    let mut c = standard();
    c.card_policy.on_unsupported = PolicyAction::Warn;
    let r = run(&deck(&[(BASIC, 59), (UNSUPPORTED, 1)], &[]), &c);
    assert!(r.valid);
    assert!(r.errors.is_empty());
    assert_eq!(codes(&r.warnings), ["UNSUPPORTED_CARD"]);
    assert_eq!(r.warnings[0].severity, IssueSeverity::Warning);
}

/// REQ-ING-008: Partial warns under casual, valid.
#[test]
fn partial_warns_casual() {
    let r = run(&deck(&[(BASIC, 59), (PARTIAL, 1)], &[]), &standard());
    assert!(r.valid);
    assert!(r.errors.is_empty());
    assert_eq!(codes(&r.warnings), ["PARTIAL_CARD"]);
    assert_eq!(r.warnings[0].card_def_id.as_deref(), Some(PARTIAL));
    assert_eq!(r.warnings[0].severity, IssueSeverity::Warning);
}

/// REQ-ING-008: Partial is rejected under tournament policy.
#[test]
fn partial_rejected_tournament() {
    let r = run(
        &deck(&[(BASIC, 59), (PARTIAL, 1)], &[]),
        &standard().with_tournament_policy(),
    );
    assert!(!r.valid);
    assert_eq!(codes(&r.errors), ["PARTIAL_CARD"]);
    assert!(r.warnings.is_empty());
}

/// REQ-ING-008: AutoParsed ok under casual, BelowMinSupport under tournament.
#[test]
fn auto_parsed_policy() {
    let d = deck(&[(BASIC, 59), (AUTO, 1)], &[]);
    let r = run(&d, &standard());
    assert_ok(&r);
    assert!(r.warnings.is_empty());
    let r = run(&d, &standard().with_tournament_policy());
    assert!(!r.valid);
    assert_eq!(codes(&r.errors), ["BELOW_MIN_SUPPORT"]);
    assert_eq!(r.errors[0].card_def_id.as_deref(), Some(AUTO));
}

/// REQ-ING-008: Verified allowed everywhere.
#[test]
fn verified_allowed_everywhere() {
    let d = deck(&[(BASIC, 56), (CARD_A, 4)], &[]);
    for c in [standard(), standard().with_tournament_policy()] {
        let r = run(&d, &c);
        assert_ok(&r);
        assert!(r.warnings.is_empty());
    }
}

/// REQ-ING-008: min_support_status values other than Verified add no extra rejection.
#[test]
fn other_min_support_values_add_no_rejection() {
    for st in [
        SupportStatus::AutoParsed,
        SupportStatus::Partial,
        SupportStatus::Unsupported,
    ] {
        let mut c = standard();
        c.card_policy.min_support_status = st;
        let r = run(&deck(&[(BASIC, 58), (AUTO, 1), (CARD_A, 1)], &[]), &c);
        assert_ok(&r);
        assert!(r.warnings.is_empty());
    }
}

/// REQ-ING-008: a tournament-like Verified minimum with permissive partial gives warning + error mix.
#[test]
fn mixed_policy_partial_warn_auto_error() {
    let mut c = standard();
    c.card_policy.min_support_status = SupportStatus::Verified;
    c.card_policy.on_partial = PolicyAction::Warn;
    let r = run(&deck(&[(BASIC, 58), (AUTO, 1), (PARTIAL, 1)], &[]), &c);
    assert_eq!(codes(&r.errors), ["BELOW_MIN_SUPPORT"]);
    assert_eq!(codes(&r.warnings), ["PARTIAL_CARD"]);
    assert!(!r.valid);
}

/// REQ-ING-008: unknown card id -> UnknownCard.
#[test]
fn unknown_card() {
    let r = run(&deck(&[(BASIC, 59), (UNKNOWN, 1)], &[]), &standard());
    assert!(!r.valid);
    assert_eq!(codes(&r.errors), ["UNKNOWN_CARD"]);
    assert_eq!(r.errors[0].card_def_id.as_deref(), Some(UNKNOWN));
}

// ---------------------------------------------------------------- counts, config, merging

/// REQ-FMT-001: a count-0 entry is InvalidCount for that id and otherwise ignored.
#[test]
fn zero_count_entry() {
    let r = run(&deck(&[(BASIC, 60), (CARD_A, 0)], &[]), &standard());
    assert!(!r.valid);
    assert_eq!(codes(&r.errors), ["INVALID_COUNT"]);
    assert_eq!(r.errors[0].card_def_id.as_deref(), Some(CARD_A));
    // Ignored otherwise: an unknown or banned id with count 0 yields only InvalidCount.
    let r = run(
        &deck(&[(BASIC, 60), (UNKNOWN, 0), (BANNED, 0)], &[]),
        &standard(),
    );
    assert_eq!(count(&r.errors, "INVALID_COUNT"), 2);
    assert_eq!(r.errors.len(), 2);
    // Ignored for totals: 60 real cards plus a zero entry is not too small.
    let r = run(&deck(&[(BASIC, 60), (CARD_A, 0)], &[]), &standard());
    assert_eq!(count(&r.errors, "MAIN_TOO_SMALL"), 0);
}

/// REQ-FMT-001: InvalidCount once per zone it appears in; duplicate zeros do not pile up merged counts.
#[test]
fn zero_count_per_zone() {
    let r = run(
        &deck(&[(BASIC, 60), (CARD_A, 0)], &[(CARD_A, 0)]),
        &standard(),
    );
    assert_eq!(count(&r.errors, "INVALID_COUNT"), 2);
    assert_eq!(r.errors.len(), 2);
    let r = run(&deck(&[(BASIC, 60)], &[(CARD_A, 0)]), &standard());
    assert_eq!(codes(&r.errors), ["INVALID_COUNT"]);
}

/// REQ-FMT-001: a zero entry beside a positive entry of the same id: positive one counts.
#[test]
fn zero_beside_positive_entry() {
    let r = run(
        &deck(&[(BASIC, 59), (CARD_A, 0), (CARD_A, 1)], &[]),
        &standard(),
    );
    assert_eq!(count(&r.errors, "INVALID_COUNT"), 1);
    assert_eq!(count(&r.errors, "MAIN_TOO_SMALL"), 0);
}

/// REQ-FMT-001: an invalid config gives exactly one InvalidFormatConfig error, nothing else.
#[test]
fn invalid_config_single_error() {
    let mut c = standard();
    c.deck.max_copies = 0;
    // A deck that would otherwise produce many issues.
    let d = deck(&[(UNKNOWN, 0), (BANNED, 99)], &[(CARD_A, 99)]);
    let r = run(&d, &c);
    assert!(!r.valid);
    assert_eq!(r.errors.len(), 1);
    assert!(r.warnings.is_empty());
    assert_eq!(r.errors[0].code.as_str(), "INVALID_FORMAT_CONFIG");
    assert_eq!(r.errors[0].card_def_id, None);
    assert_eq!(r.errors[0].severity, IssueSeverity::Error);
    for mutate in [
        |c: &mut FormatConfig| c.format_id.clear(),
        |c: &mut FormatConfig| {
            c.deck.min_main = 5;
            c.deck.max_main = Some(4)
        },
        |c: &mut FormatConfig| c.players.min = 0,
    ] {
        let mut c = standard();
        mutate(&mut c);
        assert_eq!(codes(&run(&d, &c).errors), ["INVALID_FORMAT_CONFIG"]);
    }
}

/// REQ-FMT-001: duplicate entries per id are merged within each zone.
#[test]
fn duplicate_entries_merged() {
    // 3 + 2 of one id in main -> 5 copies.
    let r = run(
        &deck(&[(BASIC, 55), (CARD_A, 3), (CARD_A, 2)], &[]),
        &standard(),
    );
    assert_eq!(codes(&r.errors), ["TOO_MANY_COPIES"]);
    assert_eq!(count(&r.errors, "TOO_MANY_COPIES"), 1);
    // Sideboard duplicates merged: 8 + 8 = 16.
    let r = run(
        &deck(&[(BASIC, 60)], &[(BASIC, 8), (BASIC, 8)]),
        &standard(),
    );
    assert_eq!(codes(&r.errors), ["SIDEBOARD_TOO_LARGE"]);
    // Merged main totals reach the minimum: 30 + 30.
    assert_ok(&run(&deck(&[(BASIC, 30), (BASIC, 30)], &[]), &standard()));
    // Ban issues are reported once per distinct id even with duplicates.
    let r = run(
        &deck(&[(BASIC, 58), (BANNED, 1), (BANNED, 1)], &[]),
        &standard(),
    );
    assert_eq!(count(&r.errors, "BANNED"), 1);
}

/// REQ-FMT-001: warnings never affect validity.
#[test]
fn warnings_never_invalidate() {
    let mut c = standard();
    c.card_policy.on_unsupported = PolicyAction::Warn;
    let r = run(
        &deck(
            &[(BASIC, 57), (PARTIAL, 1), (UNSUPPORTED, 1), (CARD_A, 1)],
            &[],
        ),
        &c,
    );
    assert!(r.valid);
    assert!(r.errors.is_empty());
    assert_eq!(r.warnings.len(), 2);
    assert!(
        r.warnings
            .iter()
            .all(|w| w.severity == IssueSeverity::Warning)
    );
}

// ---------------------------------------------------------------- ordering and determinism

/// REQ-FMT-001: deterministic; entry order within zones does not matter.
#[test]
fn deterministic_and_order_independent() {
    let a = deck(
        &[
            (BASIC, 50),
            (BANNED, 1),
            (UNKNOWN, 2),
            (CARD_A, 6),
            (PARTIAL, 1),
        ],
        &[(CARD_B, 2), (NOT_LEGAL, 1), (DIGITAL, 1)],
    );
    let b = deck(
        &[
            (PARTIAL, 1),
            (CARD_A, 6),
            (UNKNOWN, 2),
            (BANNED, 1),
            (BASIC, 50),
        ],
        &[(DIGITAL, 1), (NOT_LEGAL, 1), (CARD_B, 2)],
    );
    let r1 = run(&a, &standard());
    assert_eq!(r1, run(&a, &standard()));
    assert_eq!(r1, run(&b, &standard()));
    assert!(!r1.valid);
}

/// REQ-FMT-001: deck-level issues precede per-card issues; per-card issues ascend by id.
#[test]
fn issue_ordering() {
    let mut c = standard();
    c.deck.max_sideboard = 0;
    // ids ascending: banned_card < card_a < card_b < not_in_database < notlegal_card
    let d = deck(
        &[(CARD_B, 5), (BANNED, 1), (CARD_A, 5), (UNKNOWN, 1)],
        &[(NOT_LEGAL, 1)],
    );
    let r = run(&d, &c);
    assert_eq!(
        codes(&r.errors),
        [
            "MAIN_TOO_SMALL",
            "SIDEBOARD_TOO_LARGE",
            "BANNED",
            "TOO_MANY_COPIES",
            "TOO_MANY_COPIES",
            "UNKNOWN_CARD",
            "NOT_LEGAL",
        ]
    );
    let ids: Vec<Option<&str>> = r.errors.iter().map(|e| e.card_def_id.as_deref()).collect();
    assert_eq!(
        ids,
        [
            None,
            None,
            Some(BANNED),
            Some(CARD_A),
            Some(CARD_B),
            Some(UNKNOWN),
            Some(NOT_LEGAL)
        ]
    );
}

/// REQ-FMT-001: InvalidCount first, then deck-level, then BanListUnavailable, then per card.
#[test]
fn ban_list_unavailable_position() {
    let mut c = standard();
    c.banlist_ref = Some("banlists/absent@1".into());
    c.deck.max_sideboard = 0;
    let d = deck(&[(CARD_A, 0), (UNKNOWN, 1)], &[(CARD_B, 1)]);
    let r = run(&d, &c);
    assert_eq!(
        codes(&r.errors),
        [
            "INVALID_COUNT",
            "MAIN_TOO_SMALL",
            "SIDEBOARD_TOO_LARGE",
            "BAN_LIST_UNAVAILABLE",
            "UNKNOWN_CARD"
        ]
    );
}

/// REQ-FMT-001, REQ-ING-008, REQ-FMT-004: per-card order is ban, copies, unknown/digital, support.
#[test]
fn per_card_check_order() {
    // MULTI: Banned in list, Unsupported + digital-only in catalog, 5 copies (limit 4).
    let d = deck(&[(BASIC, 55), (MULTI, 5)], &[]);
    let r = run(&d, &standard());
    assert_eq!(
        codes(&r.errors),
        [
            "BANNED",
            "TOO_MANY_COPIES",
            "DIGITAL_ONLY_CARD",
            "UNSUPPORTED_CARD"
        ]
    );
    assert!(
        r.errors
            .iter()
            .all(|e| e.card_def_id.as_deref() == Some(MULTI))
    );
    // Unknown card: copies check runs before the unknown check; nothing after it.
    let r = run(&deck(&[(BASIC, 55), (UNKNOWN, 5)], &[]), &standard());
    assert_eq!(codes(&r.errors), ["TOO_MANY_COPIES", "UNKNOWN_CARD"]);
    // Digital error precedes the support warning for the same card.
    let mut c = standard();
    c.card_policy.on_unsupported = PolicyAction::Warn;
    let r = run(&deck(&[(BASIC, 59), (MULTI, 1)], &[]), &c);
    assert_eq!(codes(&r.errors), ["BANNED", "DIGITAL_ONLY_CARD"]);
    assert_eq!(codes(&r.warnings), ["UNSUPPORTED_CARD"]);
}

/// REQ-FMT-001: warnings are in emission (ascending id) order.
#[test]
fn warnings_ascending_order() {
    let mut c = standard();
    c.card_policy.on_unsupported = PolicyAction::Warn;
    let r = run(
        &deck(&[(BASIC, 58), (UNSUPPORTED, 1), (PARTIAL, 1)], &[]),
        &c,
    );
    assert!(r.valid);
    // test.partial_card < test.unsupported_card
    assert_eq!(codes(&r.warnings), ["PARTIAL_CARD", "UNSUPPORTED_CARD"]);
}

/// REQ-FMT-001: u32::MAX counts across entries and ids neither panic nor overflow.
#[test]
fn extreme_counts() {
    let mut c = standard();
    c.deck.max_main = Some(100);
    let big = u32::MAX;
    let d = deck(
        &[
            (CARD_A, big),
            (CARD_A, big),
            (CARD_B, big),
            (BASIC, big),
            (BASIC, big),
        ],
        &[(CARD_A, big), (CARD_B, big), (CARD_B, big), (BASIC, big)],
    );
    let r = run(&d, &c);
    assert!(!r.valid);
    assert_eq!(count(&r.errors, "MAIN_TOO_LARGE"), 1);
    assert_eq!(count(&r.errors, "SIDEBOARD_TOO_LARGE"), 1);
    assert!(has(&r.errors, "TOO_MANY_COPIES", Some(CARD_A)));
    assert!(has(&r.errors, "TOO_MANY_COPIES", Some(CARD_B)));
    assert!(!has(&r.errors, "TOO_MANY_COPIES", Some(BASIC)));
    // Many entries of one id: still no panic.
    let many: Vec<(&str, u32)> = (0..64).map(|_| (CARD_A, big)).collect();
    let r = run(&deck(&many, &many), &standard());
    assert_eq!(count(&r.errors, "TOO_MANY_COPIES"), 1);
    // Exempt basic land at huge counts stays valid with no maximum.
    let r = run(&deck(&[(BASIC, big), (BASIC, big)], &[]), &standard());
    assert_ok(&r);
}

// ---------------------------------------------------------------- robustness

/// REQ-FMT-001 (R12): random bytes into BanList and DeckSubmission parsers never panic.
#[test]
fn parsers_survive_random_bytes() {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    for _ in 0..2000 {
        let n = rng.below(256) as usize;
        let bytes: Vec<u8> = (0..n).map(|_| rng.next_u64() as u8).collect();
        let _ = serde_json::from_slice::<BanList>(&bytes);
        if let Ok(d) = serde_json::from_slice::<DeckSubmission>(&bytes) {
            let _ = run(&d, &standard());
        }
    }
    // Mutated well-formed documents.
    let deck_doc = br#"{"main":[{"cardDefId":"test.card_a","count":4}],"sideboard":[]}"#.to_vec();
    let list_doc = br#"{"name":"n","version":"v","entries":{"test.card_a":"BANNED"}}"#.to_vec();
    for doc in [deck_doc, list_doc] {
        for _ in 0..2000 {
            let mut b = doc.clone();
            for _ in 0..(rng.below(4) + 1) {
                let i = rng.below(b.len() as u64) as usize;
                match rng.below(3) {
                    0 => b[i] = rng.next_u64() as u8,
                    1 => b.truncate(i),
                    _ => b.insert(i, rng.next_u64() as u8),
                }
                if b.is_empty() {
                    break;
                }
            }
            let _ = serde_json::from_slice::<BanList>(&b);
            if let Ok(d) = serde_json::from_slice::<DeckSubmission>(&b) {
                let _ = run(&d, &standard());
            }
        }
    }
}

/// REQ-FMT-001: DeckSubmission wire form is camelCase.
#[test]
fn deck_submission_wire_form() {
    let d = deck(&[(CARD_A, 2)], &[(CARD_B, 1)]);
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(
        v,
        serde_json::json!({
            "main": [{"cardDefId": "test.card_a", "count": 2}],
            "sideboard": [{"cardDefId": "test.card_b", "count": 1}]
        })
    );
    assert_eq!(serde_json::from_value::<DeckSubmission>(v).unwrap(), d);
}

// ---------------------------------------------------------------- properties

fn random_config(rng: &mut Rng) -> FormatConfig {
    let mut c = preset(PRESET_IDS[rng.below(5) as usize]).unwrap();
    c.deck.min_main = rng.below(80) as u32;
    c.deck.max_main = if rng.flip() {
        None
    } else {
        Some(rng.below(120) as u32)
    };
    c.deck.max_sideboard = rng.below(20) as u32;
    c.deck.max_copies = rng.below(6) as u32; // 0 makes the config invalid
    c.deck.basic_land_exempt = rng.flip();
    c.players.min = rng.below(3) as u32;
    c.players.max = rng.below(4) as u32;
    c.banlist_ref = match rng.below(4) {
        0 => None,
        1 => Some("banlists/absent@1".to_string()),
        2 => Some("garbage".to_string()),
        _ => c.banlist_ref,
    };
    c.card_policy = if rng.flip() {
        CardPolicy::casual()
    } else {
        CardPolicy::tournament()
    };
    c.card_policy.allow_digital_only = rng.flip();
    c.digital_extensions.enabled = rng.flip();
    if rng.flip() {
        c.card_policy.on_unsupported = PolicyAction::Warn;
    }
    c
}

fn random_ids() -> Vec<String> {
    let mut ids: Vec<String> = [
        CARD_A,
        CARD_B,
        BASIC,
        DIGITAL,
        PARTIAL,
        UNSUPPORTED,
        AUTO,
        BANNED,
        NOT_LEGAL,
        RESTRICTED,
        UNKNOWN,
        MULTI,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    ids.extend((0..5).map(filler_id));
    ids
}

fn random_entries(rng: &mut Rng, ids: &[String]) -> Vec<DeckEntry> {
    let n = rng.below(12);
    (0..n)
        .map(|_| {
            let id = &ids[rng.below(ids.len() as u64) as usize];
            let count = match rng.below(6) {
                0 => 0,
                1 => rng.below(70) as u32,
                2 => u32::MAX - rng.below(3) as u32,
                _ => rng.below(6) as u32,
            };
            entry(id, count)
        })
        .collect()
}

/// REQ-FMT-001, REQ-ING-008, REQ-FMT-004: structural properties over random decks and configs.
#[test]
fn property_result_shape() {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    let ids = random_ids();
    let cat = catalog();
    let lists = banlists();
    for _ in 0..1500 {
        let c = random_config(&mut rng);
        let d = DeckSubmission {
            main: random_entries(&mut rng, &ids),
            sideboard: random_entries(&mut rng, &ids),
        };
        let r = validate_deck(&d, &c, &cat, &lists);
        assert_eq!(r.valid, r.errors.is_empty());
        assert!(r.errors.iter().all(|e| e.severity == IssueSeverity::Error));
        assert!(
            r.warnings
                .iter()
                .all(|w| w.severity == IssueSeverity::Warning)
        );
        assert_eq!(r, validate_deck(&d, &c, &cat, &lists));
        if c.check().is_err() {
            assert_eq!(codes(&r.errors), ["INVALID_FORMAT_CONFIG"]);
            assert!(r.warnings.is_empty());
        } else {
            assert_eq!(count(&r.errors, "INVALID_FORMAT_CONFIG"), 0);
        }
        // Reversing entry order never changes the result.
        let mut rev = d.clone();
        rev.main.reverse();
        rev.sideboard.reverse();
        assert_eq!(r, validate_deck(&rev, &c, &cat, &lists));
    }
}

/// REQ-FMT-001, REQ-ING-008: a deck within every limit, all Verified, no ban entries is valid.
#[test]
fn property_compliant_deck_is_valid() {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let cat = catalog();
    let lists = banlists(); // standard list entries only name non-filler ids
    for _ in 0..500 {
        let mut c = standard();
        c.deck.max_copies = 1 + rng.below(4) as u32;
        c.deck.max_sideboard = rng.below(16) as u32;
        c.deck.basic_land_exempt = rng.flip();
        if rng.flip() {
            c.card_policy = CardPolicy::tournament();
        }
        let mut next = 0u32;
        let max_copies = c.deck.max_copies;
        let mut take = |rng: &mut Rng, budget: u32| -> Vec<DeckEntry> {
            let mut out = Vec::new();
            let mut left = budget;
            while left > 0 && next < FILLER {
                let n = (1 + rng.below(max_copies as u64) as u32).min(left);
                out.push(entry(&filler_id(next), n));
                next += 1;
                left -= n;
            }
            out
        };
        let budget = 1 + rng.below(30) as u32;
        let main = take(&mut rng, budget);
        let side = take(&mut rng, c.deck.max_sideboard);
        let total: u32 = main.iter().map(|e| e.count).sum();
        c.deck.min_main = 1 + rng.below(total as u64) as u32;
        c.deck.max_main = if rng.flip() {
            None
        } else {
            Some(total + rng.below(5) as u32)
        };
        assert_eq!(c.check(), Ok(()));
        let d = DeckSubmission {
            main,
            sideboard: side,
        };
        let r = validate_deck(&d, &c, &cat, &lists);
        assert!(r.valid, "{:?}", r);
        assert!(r.errors.is_empty());
        assert!(r.warnings.is_empty());
    }
}
