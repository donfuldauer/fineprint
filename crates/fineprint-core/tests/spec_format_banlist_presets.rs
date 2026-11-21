// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for ban-list references, ban-list data and the constructed presets.
//! Black-box: written from the work-item brief only.
//!
//! Covers REQ-FMT-001 (ban lists and legalities are versioned data; presets are configs).

#[path = "spec_format_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_core::format::*;
use fineprint_dsl::card::{Legality, SupportStatus};
use serde_json::json;
use support::*;

/// REQ-FMT-001: BanListRef::parse accept / reject table.
#[test]
fn banlist_ref_parse_table() {
    let ok = [
        ("banlists/standard@2026-11-10", "standard", "2026-11-10"),
        ("banlists/a@b", "a", "b"),
        ("banlists/x.y_z@1.2.3", "x.y_z", "1.2.3"),
    ];
    for (s, n, v) in ok {
        let r = BanListRef::parse(s).unwrap();
        assert_eq!((r.name.as_str(), r.version.as_str()), (n, v), "{s}");
        assert_eq!(r.to_string(), s);
    }
    let bad = [
        "",
        "banlists",
        "banlists/",
        "banlists/x",
        "banlists/@v",
        "banlists/x@",
        "banlists/@",
        "x@v",
        "/x@v",
        "banlist/x@v",
        "banlists/a/b@v",
        "banlists/a@b@c",
        "banlists/a@b/c",
        "banlists/x@v/",
        "banlists//x@v",
    ];
    for s in bad {
        assert_eq!(
            BanListRef::parse(s),
            Err(FormatConfigError::BadBanlistRef(s.to_string())),
            "{s:?}"
        );
    }
}

/// REQ-FMT-001: Display and parse round trip.
#[test]
fn banlist_ref_display_round_trip() {
    let r = BanListRef {
        name: "modern".into(),
        version: "2026-11-10".into(),
    };
    let s = r.to_string();
    assert_eq!(s, "banlists/modern@2026-11-10");
    assert_eq!(BanListRef::parse(&s), Ok(r));
}

/// REQ-FMT-001: BanList serde round trip, camelCase, `entries` map.
#[test]
fn banlist_serde_round_trip() {
    let list = ban_list_for(
        "standard",
        &[
            (BANNED, Legality::Banned),
            (RESTRICTED, Legality::Restricted),
            (NOT_LEGAL, Legality::NotLegal),
            (CARD_A, Legality::Legal),
        ],
    );
    let v = serde_json::to_value(&list).unwrap();
    assert_eq!(
        v,
        json!({
            "name": "standard",
            "version": PRESET_BANLIST_VERSION,
            "entries": {
                BANNED: "BANNED",
                RESTRICTED: "RESTRICTED",
                NOT_LEGAL: "NOT_LEGAL",
                CARD_A: "LEGAL"
            }
        })
    );
    let back: BanList = serde_json::from_value(v).unwrap();
    assert_eq!(back, list);
    let text = serde_json::to_string(&list).unwrap();
    assert_eq!(serde_json::from_str::<BanList>(&text).unwrap(), list);
    let empty = BanList {
        name: "n".into(),
        version: "v".into(),
        entries: BTreeMap::new(),
    };
    assert_eq!(
        serde_json::from_value::<BanList>(serde_json::to_value(&empty).unwrap()).unwrap(),
        empty
    );
}

/// REQ-FMT-001: bad ban-list JSON rejected.
#[test]
fn banlist_bad_json_rejected() {
    assert!(
        serde_json::from_value::<BanList>(
            json!({"name":"n","version":"v","entries":{"a":"MAYBE"}})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<BanList>(json!({"name":"n","version":"v"})).is_err());
}

/// REQ-FMT-001: a BanListSource is usable through a trait object; absent ref -> None.
#[test]
fn ban_list_source_trait_object() {
    let lists = banlists();
    let src: &dyn BanListSource = &lists;
    let r = BanListRef::parse(&format!("banlists/standard@{PRESET_BANLIST_VERSION}")).unwrap();
    let got = src.ban_list(&r).unwrap();
    assert_eq!(got.name, "standard");
    assert_eq!(got.entries.get(BANNED), Some(&Legality::Banned));
    let missing = BanListRef::parse("banlists/nope@1").unwrap();
    assert!(src.ban_list(&missing).is_none());
}

/// REQ-FMT-001: CardPolicy constructors.
#[test]
fn card_policy_constructors() {
    let c = CardPolicy::casual();
    assert_eq!(c.min_support_status, SupportStatus::AutoParsed);
    assert_eq!(c.on_partial, PolicyAction::Warn);
    assert_eq!(c.on_unsupported, PolicyAction::Reject);
    assert!(!c.allow_digital_only);
    let t = CardPolicy::tournament();
    assert_eq!(t.min_support_status, SupportStatus::Verified);
    assert_eq!(t.on_partial, PolicyAction::Reject);
    assert_eq!(t.on_unsupported, PolicyAction::Reject);
    assert!(!t.allow_digital_only);
}

/// REQ-FMT-001: all five presets exist and match the pinned table field by field.
#[test]
fn presets_match_table() {
    assert_eq!(
        PRESET_IDS,
        ["standard", "pioneer", "modern", "legacy", "vintage"]
    );
    assert_eq!(PRESET_BANLIST_VERSION, "2026-11-10");
    for id in PRESET_IDS {
        let p = preset(id).unwrap_or_else(|| panic!("preset {id}"));
        assert_eq!(p.format_id, id);
        assert_eq!(p.rules_baseline, RULES_BASELINE);
        assert_eq!(
            p.deck,
            DeckRules {
                min_main: 60,
                max_main: None,
                max_sideboard: 15,
                max_copies: 4,
                basic_land_exempt: true,
                enforce_color_identity: false
            }
        );
        assert_eq!(
            p.players,
            PlayerRules {
                min: 2,
                max: 2,
                teams: None,
                shared_team_turns: false,
                range_of_influence: None,
                attack_multiple_players: true
            }
        );
        assert_eq!(p.starting_life, 20);
        assert_eq!(p.starting_hand_size, 7);
        assert_eq!(
            p.mulligan,
            MulliganRules {
                rule: MulliganRule::London,
                free_first_mulligan_multiplayer: false
            }
        );
        assert_eq!(p.first_turn_draw_rule, FirstTurnDrawRule::CrDefault);
        assert_eq!(p.commander, None);
        assert_eq!(p.card_policy, CardPolicy::casual());
        assert_eq!(p.timeout_policy, TimeoutPolicy::AutoPassThenDefaultChoice);
        assert!(!p.deck_lists_open);
        assert!(!p.ante);
        assert_eq!(
            p.digital_extensions,
            DigitalExtensions {
                enabled: false,
                set: vec![]
            }
        );
        let want_ref = format!("banlists/{id}@{PRESET_BANLIST_VERSION}");
        assert_eq!(p.banlist_ref.as_deref(), Some(want_ref.as_str()));
        let parsed = BanListRef::parse(&want_ref).unwrap();
        assert_eq!(
            (parsed.name.as_str(), parsed.version.as_str()),
            (id, PRESET_BANLIST_VERSION)
        );
        assert_eq!(p.check(), Ok(()));
    }
}

/// REQ-FMT-001: unknown preset ids are None (case-sensitive).
#[test]
fn unknown_preset_ids() {
    for id in [
        "",
        "commander",
        "Standard",
        "STANDARD",
        "standard ",
        "brawl",
    ] {
        assert!(preset(id).is_none(), "{id:?}");
    }
}

/// REQ-FMT-001: presets serialize and parse back unchanged.
#[test]
fn presets_serde_round_trip() {
    for id in PRESET_IDS {
        let p = preset(id).unwrap();
        let back: FormatConfig = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }
}

/// REQ-FMT-001, REQ-ING-008: with_tournament_policy changes only card_policy.
#[test]
fn with_tournament_policy_changes_only_card_policy() {
    for id in PRESET_IDS {
        let base = preset(id).unwrap();
        let t = base.clone().with_tournament_policy();
        assert_eq!(t.card_policy, CardPolicy::tournament());
        let mut expect = base.clone();
        expect.card_policy = CardPolicy::tournament();
        assert_eq!(t, expect);
        assert_ne!(t, base);
        assert_eq!(t.check(), Ok(()));
    }
}
