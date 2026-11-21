// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::format::FormatConfig`: wire form, `check`, robustness.
//! Black-box: written from the work-item brief only.
//!
//! Covers REQ-FMT-001 (format-defined rules are configuration; `firstTurnDrawRule`
//! CR_DEFAULT follows 103.8a-c).

#[path = "spec_format_support.rs"]
mod support;

use fineprint_core::format::*;
use fineprint_dsl::card::SupportStatus;
use serde_json::{Value, json};
use support::*;

fn srs_value() -> Value {
    serde_json::from_str(SRS_EXAMPLE).unwrap()
}

fn srs_config() -> FormatConfig {
    serde_json::from_str(SRS_EXAMPLE).unwrap()
}

/// REQ-FMT-001: the SRS example round-trips to an equal JSON value.
#[test]
fn srs_example_round_trips() {
    let cfg = srs_config();
    assert_eq!(serde_json::to_value(&cfg).unwrap(), srs_value());
    assert_eq!(cfg.format_id, "commander");
    assert_eq!(cfg.rules_baseline, RULES_BASELINE);
    assert_eq!(RULES_BASELINE, "CR-2026-11-06");
    assert_eq!(cfg.starting_life, 40);
    assert_eq!(cfg.deck.max_main, Some(100));
    assert_eq!(
        cfg.banlist_ref.as_deref(),
        Some("banlists/commander@2026-11-10")
    );
    assert_eq!(cfg.first_turn_draw_rule, FirstTurnDrawRule::CrDefault);
    assert!(cfg.commander.is_some());
    assert_eq!(cfg.check(), Ok(()));
    // Re-parse of the re-serialization is identical.
    let again: FormatConfig = serde_json::from_value(serde_json::to_value(&cfg).unwrap()).unwrap();
    assert_eq!(again, cfg);
}

/// REQ-FMT-001: unknown members rejected at top level and in each sub-object.
#[test]
fn unknown_members_rejected_everywhere() {
    let base = srs_value();
    let mut v = base.clone();
    v.as_object_mut().unwrap().insert("bogus".into(), json!(1));
    assert!(serde_json::from_value::<FormatConfig>(v).is_err());
    for sub in [
        "deck",
        "players",
        "mulligan",
        "commander",
        "cardPolicy",
        "digitalExtensions",
    ] {
        let mut v = base.clone();
        v[sub]
            .as_object_mut()
            .unwrap()
            .insert("bogus".into(), json!(1));
        assert!(
            serde_json::from_value::<FormatConfig>(v).is_err(),
            "sub-object {sub}"
        );
    }
}

/// REQ-FMT-001: a missing required (non-optional) member is rejected, at every level.
#[test]
fn missing_required_members_rejected() {
    let base = srs_value();
    let optional_top = ["commander", "banlistRef"];
    for key in base.as_object().unwrap().keys() {
        if optional_top.contains(&key.as_str()) {
            continue;
        }
        let mut v = base.clone();
        v.as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<FormatConfig>(v).is_err(),
            "missing {key}"
        );
    }
    let optional_nested = [
        ("deck", "maxMain"),
        ("players", "teams"),
        ("players", "rangeOfInfluence"),
    ];
    for sub in [
        "deck",
        "players",
        "mulligan",
        "commander",
        "cardPolicy",
        "digitalExtensions",
    ] {
        for key in base[sub].as_object().unwrap().keys() {
            if optional_nested.contains(&(sub, key.as_str())) {
                continue;
            }
            let mut v = base.clone();
            v[sub].as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<FormatConfig>(v).is_err(),
                "missing {sub}.{key}"
            );
        }
    }
}

/// REQ-FMT-001: wrongly typed members are rejected.
#[test]
fn wrong_types_rejected() {
    let mut v = srs_value();
    v["startingLife"] = json!("forty");
    assert!(serde_json::from_value::<FormatConfig>(v).is_err());
    let mut v = srs_value();
    v["deck"]["minMain"] = json!(-1);
    assert!(serde_json::from_value::<FormatConfig>(v).is_err());
    let mut v = srs_value();
    v["firstTurnDrawRule"] = json!("cr_default");
    assert!(serde_json::from_value::<FormatConfig>(v).is_err());
}

fn wire<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
    value: T,
    s: &str,
) {
    assert_eq!(serde_json::to_value(&value).unwrap(), json!(s));
    let back: T = serde_json::from_value(json!(s)).unwrap();
    assert_eq!(back, value);
}

/// REQ-FMT-001: every enum variant's wire string, both directions.
#[test]
fn enum_wire_strings() {
    wire(MulliganRule::London, "LONDON");
    wire(FirstTurnDrawRule::CrDefault, "CR_DEFAULT");
    wire(FirstTurnDrawRule::NeverSkip, "NEVER_SKIP");
    wire(FirstTurnDrawRule::AlwaysSkip, "ALWAYS_SKIP");
    wire(PolicyAction::Reject, "REJECT");
    wire(PolicyAction::Warn, "WARN");
    wire(TimeoutPolicy::AutoPass, "AUTO_PASS");
    wire(
        TimeoutPolicy::AutoPassThenDefaultChoice,
        "AUTO_PASS_THEN_DEFAULT_CHOICE",
    );
    wire(TimeoutPolicy::GameLoss, "GAME_LOSS");
    wire(SupportStatus::Verified, "VERIFIED");
    wire(SupportStatus::AutoParsed, "AUTO_PARSED");
    wire(SupportStatus::Partial, "PARTIAL");
    wire(SupportStatus::Unsupported, "UNSUPPORTED");
    assert!(serde_json::from_value::<PolicyAction>(json!("Warn")).is_err());
    assert!(serde_json::from_value::<TimeoutPolicy>(json!("NOPE")).is_err());
}

/// REQ-FMT-001: each variant is usable inside a whole config (round trip through JSON).
#[test]
fn enum_variants_round_trip_in_config() {
    for rule in ["CR_DEFAULT", "NEVER_SKIP", "ALWAYS_SKIP"] {
        for to in ["AUTO_PASS", "AUTO_PASS_THEN_DEFAULT_CHOICE", "GAME_LOSS"] {
            for st in ["VERIFIED", "AUTO_PARSED", "PARTIAL", "UNSUPPORTED"] {
                for pa in ["REJECT", "WARN"] {
                    let mut v = srs_value();
                    v["firstTurnDrawRule"] = json!(rule);
                    v["timeoutPolicy"] = json!(to);
                    v["cardPolicy"]["minSupportStatus"] = json!(st);
                    v["cardPolicy"]["onPartial"] = json!(pa);
                    let c: FormatConfig = serde_json::from_value(v.clone()).unwrap();
                    assert_eq!(serde_json::to_value(&c).unwrap(), v);
                }
            }
        }
    }
}

/// REQ-FMT-001: None members serialize as JSON null (always present).
#[test]
fn none_members_serialize_as_null() {
    let c = standard();
    let v = serde_json::to_value(&c).unwrap();
    assert!(v.as_object().unwrap().contains_key("commander"));
    assert_eq!(v["commander"], Value::Null);
    assert_eq!(v["deck"]["maxMain"], Value::Null);
    assert_eq!(v["players"]["teams"], Value::Null);
    assert_eq!(v["players"]["rangeOfInfluence"], Value::Null);
    let mut c2 = standard();
    c2.banlist_ref = None;
    let v2 = serde_json::to_value(&c2).unwrap();
    assert!(v2.as_object().unwrap().contains_key("banlistRef"));
    assert_eq!(v2["banlistRef"], Value::Null);
    let back: FormatConfig = serde_json::from_value(v2).unwrap();
    assert_eq!(back, c2);
}

/// REQ-FMT-001: serialization uses camelCase member names.
#[test]
fn camel_case_member_names() {
    let v = serde_json::to_value(standard()).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut want = vec![
        "formatId",
        "rulesBaseline",
        "deck",
        "players",
        "startingLife",
        "startingHandSize",
        "mulligan",
        "firstTurnDrawRule",
        "commander",
        "cardPolicy",
        "timeoutPolicy",
        "deckListsOpen",
        "ante",
        "digitalExtensions",
        "banlistRef",
    ];
    want.sort_unstable();
    assert_eq!(keys, want);
}

fn valid() -> FormatConfig {
    standard()
}

/// REQ-FMT-001: a valid config is Ok.
#[test]
fn check_valid_ok() {
    assert_eq!(valid().check(), Ok(()));
    assert_eq!(srs_config().check(), Ok(()));
}

/// REQ-FMT-001: EmptyFormatId.
#[test]
fn check_empty_format_id() {
    let mut c = valid();
    c.format_id = String::new();
    assert_eq!(c.check(), Err(FormatConfigError::EmptyFormatId));
}

/// REQ-FMT-001: DeckSizeRange and its boundaries.
#[test]
fn check_deck_size_range() {
    let mut c = valid();
    c.deck.min_main = 60;
    c.deck.max_main = Some(59);
    assert_eq!(c.check(), Err(FormatConfigError::DeckSizeRange));
    c.deck.max_main = Some(60);
    assert_eq!(c.check(), Ok(()));
    c.deck.max_main = Some(61);
    assert_eq!(c.check(), Ok(()));
    c.deck.max_main = None;
    assert_eq!(c.check(), Ok(()));
    c.deck.min_main = 0;
    c.deck.max_main = Some(0);
    assert_eq!(c.check(), Ok(()));
}

/// REQ-FMT-001: ZeroMaxCopies.
#[test]
fn check_zero_max_copies() {
    let mut c = valid();
    c.deck.max_copies = 0;
    assert_eq!(c.check(), Err(FormatConfigError::ZeroMaxCopies));
    c.deck.max_copies = 1;
    assert_eq!(c.check(), Ok(()));
}

/// REQ-FMT-001: PlayerRange and its boundaries.
#[test]
fn check_player_range() {
    let mut c = valid();
    c.players.min = 0;
    c.players.max = 2;
    assert_eq!(c.check(), Err(FormatConfigError::PlayerRange));
    c.players.min = 3;
    c.players.max = 2;
    assert_eq!(c.check(), Err(FormatConfigError::PlayerRange));
    c.players.min = 2;
    c.players.max = 2;
    assert_eq!(c.check(), Ok(()));
    c.players.min = 1;
    c.players.max = 1;
    assert_eq!(c.check(), Ok(()));
    c.players.min = 0;
    c.players.max = 0;
    assert_eq!(c.check(), Err(FormatConfigError::PlayerRange));
}

/// REQ-FMT-001: BadBanlistRef carries the offending text.
#[test]
fn check_bad_banlist_ref() {
    for bad in [
        "",
        "standard",
        "banlists/standard",
        "banlists/@v",
        "banlists/a/b@v",
    ] {
        let mut c = valid();
        c.banlist_ref = Some(bad.to_string());
        assert_eq!(
            c.check(),
            Err(FormatConfigError::BadBanlistRef(bad.to_string())),
            "{bad:?}"
        );
    }
    let mut c = valid();
    c.banlist_ref = Some("banlists/x@1".into());
    assert_eq!(c.check(), Ok(()));
    c.banlist_ref = None;
    assert_eq!(c.check(), Ok(()));
}

/// REQ-FMT-001: check reports the FIRST failing check in the documented order.
#[test]
fn check_order() {
    let mut c = valid();
    c.format_id = String::new();
    c.deck.min_main = 10;
    c.deck.max_main = Some(5);
    c.deck.max_copies = 0;
    c.players.min = 0;
    c.banlist_ref = Some("bad".into());
    assert_eq!(c.check(), Err(FormatConfigError::EmptyFormatId));
    c.format_id = "x".into();
    assert_eq!(c.check(), Err(FormatConfigError::DeckSizeRange));
    c.deck.max_main = None;
    assert_eq!(c.check(), Err(FormatConfigError::ZeroMaxCopies));
    c.deck.max_copies = 4;
    assert_eq!(c.check(), Err(FormatConfigError::PlayerRange));
    c.players.min = 2;
    assert_eq!(
        c.check(),
        Err(FormatConfigError::BadBanlistRef("bad".into()))
    );
    c.banlist_ref = None;
    assert_eq!(c.check(), Ok(()));
}

/// REQ-FMT-001: FormatConfigError is Ord (variants usable as map keys / sorted).
#[test]
fn error_is_ordered_and_cloneable() {
    let mut v = vec![
        FormatConfigError::BadBanlistRef("b".into()),
        FormatConfigError::EmptyFormatId,
        FormatConfigError::PlayerRange,
    ];
    v.sort();
    assert_eq!(v.first(), Some(&FormatConfigError::EmptyFormatId));
    assert_eq!(v.clone(), v);
}

/// REQ-FMT-001: the format rules are data: a config can carry non-default values untouched.
#[test]
fn rules_are_configurable_not_hardcoded() {
    let mut c = standard();
    c.starting_life = -5;
    c.starting_hand_size = 0;
    c.first_turn_draw_rule = FirstTurnDrawRule::AlwaysSkip;
    c.timeout_policy = TimeoutPolicy::GameLoss;
    c.deck_lists_open = true;
    c.ante = true;
    c.digital_extensions = DigitalExtensions {
        enabled: true,
        set: vec!["test.ext".into()],
    };
    c.commander = Some(CommanderRules {
        enabled: true,
        damage_threshold: 7,
        tax_per_cast: 1,
        remove_perpetual_on_command_zone: false,
    });
    let back: FormatConfig = serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap();
    assert_eq!(back, c);
    assert_eq!(back.starting_life, -5);
}

fn mutate(rng: &mut Rng, src: &[u8]) -> Vec<u8> {
    let mut b = src.to_vec();
    match rng.below(4) {
        0 => {
            let n = rng.below(8) + 1;
            for _ in 0..n {
                let i = rng.below(b.len() as u64) as usize;
                b[i] = rng.next_u64() as u8;
            }
        }
        1 => {
            let i = rng.below(b.len() as u64) as usize;
            b.truncate(i);
        }
        2 => {
            let i = rng.below(b.len() as u64) as usize;
            b.insert(i, rng.next_u64() as u8);
        }
        _ => {
            let i = rng.below(b.len() as u64) as usize;
            b.remove(i);
        }
    }
    b
}

/// REQ-FMT-001 (R12): bounded random bytes and mutated SRS example never panic the parser.
#[test]
fn parser_survives_random_and_mutated_bytes() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..2000 {
        let n = rng.below(256) as usize;
        let bytes: Vec<u8> = (0..n).map(|_| rng.next_u64() as u8).collect();
        let _ = serde_json::from_slice::<FormatConfig>(&bytes);
    }
    for _ in 0..2000 {
        let bytes = mutate(&mut rng, SRS_EXAMPLE.as_bytes());
        if let Ok(c) = serde_json::from_slice::<FormatConfig>(&bytes) {
            let _ = c.check();
            let _ = serde_json::to_string(&c).unwrap();
        }
    }
}
