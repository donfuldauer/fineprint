// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the format module (REQ-FMT-001, REQ-ING-008, REQ-FMT-004, REQ-NFR-008).
//! Synthetic card ids only (R5).

use std::collections::BTreeMap;

use fineprint_dsl::card::{Legality, SupportStatus};

use super::*;

const SRS_JSON: &str = r#"{
  "formatId": "commander",
  "rulesBaseline": "CR-2026-11-06",
  "deck": { "minMain": 100, "maxMain": 100, "maxSideboard": 0, "maxCopies": 1,
            "basicLandExempt": true, "enforceColorIdentity": true },
  "players": { "min": 2, "max": 6, "teams": null, "sharedTeamTurns": false,
               "rangeOfInfluence": null, "attackMultiplePlayers": true },
  "startingLife": 40,
  "startingHandSize": 7,
  "mulligan": { "rule": "LONDON", "freeFirstMulliganMultiplayer": true },
  "firstTurnDrawRule": "CR_DEFAULT",
  "commander": { "enabled": true, "damageThreshold": 21, "taxPerCast": 2,
                 "removePerpetualOnCommandZone": true },
  "cardPolicy": { "minSupportStatus": "AUTO_PARSED", "onPartial": "WARN",
                  "onUnsupported": "REJECT", "allowDigitalOnly": false },
  "timeoutPolicy": "AUTO_PASS_THEN_DEFAULT_CHOICE",
  "deckListsOpen": false,
  "ante": false,
  "digitalExtensions": { "enabled": false, "set": [] },
  "banlistRef": "banlists/commander@2026-11-10"
}"#;

struct Catalog(BTreeMap<String, CardFacts>);
impl CardCatalog for Catalog {
    fn card_facts(&self, id: &str) -> Option<CardFacts> {
        self.0.get(id).copied()
    }
}
struct Lists(Vec<BanList>);
impl BanListSource for Lists {
    fn ban_list(&self, r: &BanListRef) -> Option<&BanList> {
        self.0
            .iter()
            .find(|l| l.name == r.name && l.version == r.version)
    }
}

fn facts(s: SupportStatus) -> CardFacts {
    CardFacts {
        support_status: s,
        digital_only: false,
        is_basic_land: false,
    }
}

fn catalog() -> Catalog {
    let mut m = BTreeMap::new();
    for id in ["test.card_a", "test.card_b", "test.card_c"] {
        m.insert(id.to_string(), facts(SupportStatus::Verified));
    }
    m.insert(
        "test.basic".into(),
        CardFacts {
            is_basic_land: true,
            ..facts(SupportStatus::Verified)
        },
    );
    m.insert(
        "test.digital".into(),
        CardFacts {
            digital_only: true,
            ..facts(SupportStatus::Verified)
        },
    );
    m.insert("test.unsup".into(), facts(SupportStatus::Unsupported));
    m.insert("test.partial".into(), facts(SupportStatus::Partial));
    m.insert("test.auto".into(), facts(SupportStatus::AutoParsed));
    Catalog(m)
}

fn list(entries: &[(&str, Legality)]) -> BanList {
    BanList {
        name: "std".into(),
        version: PRESET_BANLIST_VERSION.into(),
        entries: entries.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
    }
}

fn lists() -> Lists {
    Lists(vec![list(&[
        ("test.card_b", Legality::Banned),
        ("test.card_c", Legality::Restricted),
        ("test.notlegal", Legality::NotLegal),
        ("test.legal", Legality::Legal),
    ])])
}

fn fmt() -> FormatConfig {
    let mut f = preset("modern").unwrap();
    f.banlist_ref = Some(format!("banlists/std@{PRESET_BANLIST_VERSION}"));
    f.deck.min_main = 4;
    f
}

fn e(id: &str, n: u32) -> DeckEntry {
    DeckEntry {
        card_def_id: id.into(),
        count: n,
    }
}

fn deck(main: &[(&str, u32)], side: &[(&str, u32)]) -> DeckSubmission {
    DeckSubmission {
        main: main.iter().map(|(i, n)| e(i, *n)).collect(),
        sideboard: side.iter().map(|(i, n)| e(i, *n)).collect(),
    }
}

fn run(d: &DeckSubmission, f: &FormatConfig) -> DeckValidationResult {
    validate_deck(d, f, &catalog(), &lists())
}

fn codes(i: &[DeckIssue]) -> Vec<DeckIssueCode> {
    i.iter().map(|x| x.code).collect()
}

#[test]
fn req_fmt_001_srs_example_round_trips() {
    let v: serde_json::Value = serde_json::from_str(SRS_JSON).unwrap();
    let c: FormatConfig = serde_json::from_str(SRS_JSON).unwrap();
    assert_eq!(serde_json::to_value(&c).unwrap(), v);
    assert_eq!(c.commander.as_ref().unwrap().damage_threshold, 21);
    assert!(c.check().is_ok());
}

#[test]
fn req_fmt_001_nulls_serialize_and_unknown_members_rejected() {
    let p = preset("standard").unwrap();
    let v = serde_json::to_value(&p).unwrap();
    assert!(v["commander"].is_null());
    assert!(v["deck"]["maxMain"].is_null());
    assert!(v["players"]["teams"].is_null());
    assert!(v["players"]["rangeOfInfluence"].is_null());
    let mut none = p.clone();
    none.banlist_ref = None;
    assert!(serde_json::to_value(&none).unwrap()["banlistRef"].is_null());
    let mut w = v.clone();
    w["extra"] = serde_json::json!(1);
    assert!(serde_json::from_value::<FormatConfig>(w).is_err());
    let mut w = v;
    w["deck"]["extra"] = serde_json::json!(1);
    assert!(serde_json::from_value::<FormatConfig>(w).is_err());
}

#[test]
fn req_fmt_001_wire_strings() {
    let s = |v: &dyn Erased| v.json();
    assert_eq!(s(&FirstTurnDrawRule::CrDefault), "\"CR_DEFAULT\"");
    assert_eq!(s(&FirstTurnDrawRule::NeverSkip), "\"NEVER_SKIP\"");
    assert_eq!(s(&FirstTurnDrawRule::AlwaysSkip), "\"ALWAYS_SKIP\"");
    assert_eq!(s(&MulliganRule::London), "\"LONDON\"");
    assert_eq!(s(&PolicyAction::Reject), "\"REJECT\"");
    assert_eq!(s(&PolicyAction::Warn), "\"WARN\"");
    assert_eq!(s(&TimeoutPolicy::AutoPass), "\"AUTO_PASS\"");
    assert_eq!(
        s(&TimeoutPolicy::AutoPassThenDefaultChoice),
        "\"AUTO_PASS_THEN_DEFAULT_CHOICE\""
    );
    assert_eq!(s(&TimeoutPolicy::GameLoss), "\"GAME_LOSS\"");
    assert_eq!(s(&IssueSeverity::Error), "\"ERROR\"");
    assert_eq!(s(&IssueSeverity::Warning), "\"WARNING\"");
}

trait Erased {
    fn json(&self) -> String;
}
impl<T: serde::Serialize> Erased for T {
    fn json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[test]
fn req_fmt_001_issue_code_strings() {
    use DeckIssueCode::*;
    for (c, s) in [
        (InvalidFormatConfig, "INVALID_FORMAT_CONFIG"),
        (InvalidCount, "INVALID_COUNT"),
        (MainTooSmall, "MAIN_TOO_SMALL"),
        (MainTooLarge, "MAIN_TOO_LARGE"),
        (SideboardTooLarge, "SIDEBOARD_TOO_LARGE"),
        (TooManyCopies, "TOO_MANY_COPIES"),
        (Banned, "BANNED"),
        (NotLegal, "NOT_LEGAL"),
        (BanListUnavailable, "BAN_LIST_UNAVAILABLE"),
        (DigitalOnlyCard, "DIGITAL_ONLY_CARD"),
        (UnknownCard, "UNKNOWN_CARD"),
        (UnsupportedCard, "UNSUPPORTED_CARD"),
        (PartialCard, "PARTIAL_CARD"),
        (BelowMinSupport, "BELOW_MIN_SUPPORT"),
    ] {
        assert_eq!(c.as_str(), s);
        assert_eq!(serde_json::to_string(&c).unwrap(), format!("\"{s}\""));
    }
}

#[test]
fn req_fmt_001_check_each_error_and_order() {
    let ok = preset("standard").unwrap();
    assert_eq!(ok.check(), Ok(()));
    let mut f = ok.clone();
    f.format_id.clear();
    assert_eq!(f.check(), Err(FormatConfigError::EmptyFormatId));
    let mut f = ok.clone();
    f.deck.max_main = Some(59);
    assert_eq!(f.check(), Err(FormatConfigError::DeckSizeRange));
    f.deck.max_main = Some(60);
    assert_eq!(f.check(), Ok(()));
    let mut f = ok.clone();
    f.deck.max_copies = 0;
    assert_eq!(f.check(), Err(FormatConfigError::ZeroMaxCopies));
    let mut f = ok.clone();
    f.players.min = 0;
    assert_eq!(f.check(), Err(FormatConfigError::PlayerRange));
    f.players.min = 3;
    assert_eq!(f.check(), Err(FormatConfigError::PlayerRange));
    let mut f = ok.clone();
    f.banlist_ref = Some("nope".into());
    assert_eq!(
        f.check(),
        Err(FormatConfigError::BadBanlistRef("nope".into()))
    );
    // First failing check wins.
    let mut f = ok;
    f.format_id.clear();
    f.deck.max_main = Some(1);
    f.deck.max_copies = 0;
    f.players.min = 0;
    f.banlist_ref = Some("x".into());
    assert_eq!(f.check(), Err(FormatConfigError::EmptyFormatId));
    f.format_id = "x".into();
    assert_eq!(f.check(), Err(FormatConfigError::DeckSizeRange));
    f.deck.max_main = None;
    assert_eq!(f.check(), Err(FormatConfigError::ZeroMaxCopies));
    f.deck.max_copies = 1;
    assert_eq!(f.check(), Err(FormatConfigError::PlayerRange));
    f.players.min = 1;
    assert!(matches!(
        f.check(),
        Err(FormatConfigError::BadBanlistRef(_))
    ));
}

#[test]
fn req_fmt_001_banlist_ref_parse_table() {
    for s in ["banlists/a@1", "banlists/std@2026-11-10"] {
        let r = BanListRef::parse(s).unwrap();
        assert_eq!(r.to_string(), s);
    }
    let r = BanListRef::parse("banlists/std@v1").unwrap();
    assert_eq!((r.name.as_str(), r.version.as_str()), ("std", "v1"));
    for s in [
        "",
        "std@1",
        "banlist/std@1",
        "banlists/@1",
        "banlists/std@",
        "banlists/std",
        "banlists/a/b@1",
        "banlists/a@1/2",
        "banlists/a@1@2",
        "banlists/a@1/",
    ] {
        assert_eq!(
            BanListRef::parse(s),
            Err(FormatConfigError::BadBanlistRef(s.to_string())),
            "{s}"
        );
    }
}

#[test]
fn req_fmt_001_presets() {
    assert_eq!(
        PRESET_IDS,
        ["standard", "pioneer", "modern", "legacy", "vintage"]
    );
    assert!(preset("unknown").is_none());
    assert!(preset("").is_none());
    for id in PRESET_IDS {
        let p = preset(id).unwrap();
        assert_eq!(p.format_id, id);
        assert_eq!(p.rules_baseline, RULES_BASELINE);
        assert_eq!(p.deck.min_main, 60);
        assert_eq!(p.deck.max_main, None);
        assert_eq!(p.deck.max_sideboard, 15);
        assert_eq!(p.deck.max_copies, 4);
        assert!(p.deck.basic_land_exempt);
        assert!(!p.deck.enforce_color_identity);
        assert_eq!((p.players.min, p.players.max), (2, 2));
        assert_eq!(p.players.teams, None);
        assert!(!p.players.shared_team_turns);
        assert_eq!(p.players.range_of_influence, None);
        assert!(p.players.attack_multiple_players);
        assert_eq!(p.starting_life, 20);
        assert_eq!(p.starting_hand_size, 7);
        assert_eq!(p.mulligan.rule, MulliganRule::London);
        assert!(!p.mulligan.free_first_mulligan_multiplayer);
        assert_eq!(p.first_turn_draw_rule, FirstTurnDrawRule::CrDefault);
        assert!(p.commander.is_none());
        assert_eq!(p.card_policy, CardPolicy::casual());
        assert_eq!(p.timeout_policy, TimeoutPolicy::AutoPassThenDefaultChoice);
        assert!(!p.deck_lists_open && !p.ante);
        assert!(!p.digital_extensions.enabled && p.digital_extensions.set.is_empty());
        assert_eq!(
            p.banlist_ref.as_deref(),
            Some(format!("banlists/{id}@2026-11-10").as_str())
        );
        assert_eq!(p.check(), Ok(()));
    }
    let ids: std::collections::BTreeSet<_> = PRESET_IDS.iter().collect();
    assert_eq!(ids.len(), 5);
}

#[test]
fn req_fmt_001_card_policy_constructors_and_tournament() {
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
    let f = preset("legacy").unwrap().with_tournament_policy();
    assert_eq!(f.card_policy, t);
    assert_eq!(f.format_id, "legacy");
}

#[test]
fn req_fmt_001_banlist_serde() {
    let l = list(&[("test.card_a", Legality::Banned)]);
    let j = serde_json::to_value(&l).unwrap();
    assert_eq!(j["entries"]["test.card_a"], "BANNED");
    assert_eq!(serde_json::from_value::<BanList>(j).unwrap(), l);
}

fn ok_main() -> Vec<(&'static str, u32)> {
    vec![("test.card_a", 4)]
}

#[test]
fn req_fmt_001_main_min_and_max() {
    let f = fmt();
    assert!(run(&deck(&ok_main(), &[]), &f).valid);
    let r = run(&deck(&[("test.card_a", 3)], &[]), &f);
    assert_eq!(codes(&r.errors), [DeckIssueCode::MainTooSmall]);
    assert!(!r.valid);
    assert_eq!(r.errors[0].card_def_id, None);
    // maxMain None: huge decks are fine on size.
    let big: Vec<DeckEntry> = (0..300).map(|i| e(&format!("test.x{i:03}"), 1)).collect();
    let mut cat = catalog();
    for x in &big {
        cat.0
            .insert(x.card_def_id.clone(), facts(SupportStatus::Verified));
    }
    let d = DeckSubmission {
        main: big,
        sideboard: vec![],
    };
    assert!(validate_deck(&d, &f, &cat, &lists()).valid);
    let mut f2 = f;
    f2.deck.max_main = Some(299);
    let r = validate_deck(&d, &f2, &cat, &lists());
    assert_eq!(codes(&r.errors), [DeckIssueCode::MainTooLarge]);
    f2.deck.max_main = Some(300);
    assert!(validate_deck(&d, &f2, &cat, &lists()).valid);
}

#[test]
fn req_fmt_001_sideboard_max() {
    let f = fmt();
    let side: Vec<(&str, u32)> = vec![("test.card_a", 4), ("test.card_b", 0)];
    let _ = side;
    let ok = deck(&ok_main(), &[("test.basic", 15)]);
    assert!(run(&ok, &f).valid);
    let bad = deck(&ok_main(), &[("test.basic", 16)]);
    assert_eq!(
        codes(&run(&bad, &f).errors),
        [DeckIssueCode::SideboardTooLarge]
    );
}

#[test]
fn req_fmt_001_copies() {
    let f = fmt();
    // Combined main+sideboard.
    assert!(
        run(&deck(&[("test.card_a", 2)], &[("test.card_a", 2)]), &f)
            .errors
            .len()
            == 1
    );
    let r = run(&deck(&[("test.card_a", 4)], &[("test.card_a", 1)]), &f);
    assert_eq!(codes(&r.errors), [DeckIssueCode::TooManyCopies]);
    assert_eq!(r.errors[0].card_def_id.as_deref(), Some("test.card_a"));
    // Exactly the limit across zones is fine (main min 4 satisfied by basics).
    let ok = deck(
        &[("test.card_a", 2), ("test.basic", 2)],
        &[("test.card_a", 2)],
    );
    assert!(run(&ok, &f).valid);
    // Basic-land exemption on and off.
    let many = deck(&[("test.basic", 30)], &[]);
    assert!(run(&many, &f).valid);
    let mut off = f.clone();
    off.deck.basic_land_exempt = false;
    assert_eq!(
        codes(&run(&many, &off).errors),
        [DeckIssueCode::TooManyCopies]
    );
    // Unknown card is never exempt.
    let r = run(&deck(&[("test.mystery", 5)], &[]), &f);
    assert_eq!(
        codes(&r.errors),
        [DeckIssueCode::TooManyCopies, DeckIssueCode::UnknownCard]
    );
}

#[test]
fn req_fmt_001_ban_list() {
    let f = fmt();
    let r = run(&deck(&[("test.card_a", 3), ("test.card_b", 1)], &[]), &f);
    assert_eq!(codes(&r.errors), [DeckIssueCode::Banned]);
    let mut cat = catalog();
    cat.0
        .insert("test.notlegal".into(), facts(SupportStatus::Verified));
    cat.0
        .insert("test.legal".into(), facts(SupportStatus::Verified));
    let d = deck(&[("test.notlegal", 1), ("test.legal", 3)], &[]);
    let r = validate_deck(&d, &f, &cat, &lists());
    assert_eq!(codes(&r.errors), [DeckIssueCode::NotLegal]);
    // Restricted: 1 ok, 2 errors; absent = legal.
    assert!(run(&deck(&[("test.card_c", 1), ("test.card_a", 3)], &[]), &f).valid);
    let r = run(&deck(&[("test.card_c", 2), ("test.card_a", 3)], &[]), &f);
    assert_eq!(codes(&r.errors), [DeckIssueCode::TooManyCopies]);
    let r = run(
        &deck(
            &[("test.card_c", 1), ("test.card_a", 3)],
            &[("test.card_c", 1)],
        ),
        &f,
    );
    assert_eq!(codes(&r.errors), [DeckIssueCode::TooManyCopies]);
    // No ban list: skipped.
    let mut none = f.clone();
    none.banlist_ref = None;
    assert!(run(&deck(&[("test.card_b", 4)], &[]), &none).valid);
    // Missing list, and name / version mismatch.
    struct Wrong(BanList);
    impl BanListSource for Wrong {
        fn ban_list(&self, _: &BanListRef) -> Option<&BanList> {
            Some(&self.0)
        }
    }
    let mut missing = f.clone();
    missing.banlist_ref = Some("banlists/other@1".into());
    let r = run(&deck(&ok_main(), &[]), &missing);
    assert_eq!(codes(&r.errors), [DeckIssueCode::BanListUnavailable]);
    assert_eq!(r.errors[0].card_def_id, None);
    let mut other = list(&[]);
    other.name = "different".into();
    let r = validate_deck(&deck(&ok_main(), &[]), &f, &catalog(), &Wrong(other));
    assert_eq!(codes(&r.errors), [DeckIssueCode::BanListUnavailable]);
    let mut other = list(&[]);
    other.version = "9".into();
    let r = validate_deck(&deck(&ok_main(), &[]), &f, &catalog(), &Wrong(other));
    assert_eq!(codes(&r.errors), [DeckIssueCode::BanListUnavailable]);
}

#[test]
fn req_fmt_004_req_ing_008_digital_only() {
    let d = deck(&[("test.digital", 4)], &[]);
    let mut f = fmt();
    // Extension off.
    f.card_policy.allow_digital_only = true;
    assert_eq!(codes(&run(&d, &f).errors), [DeckIssueCode::DigitalOnlyCard]);
    // Extension on, policy off.
    f.digital_extensions.enabled = true;
    f.card_policy.allow_digital_only = false;
    assert_eq!(codes(&run(&d, &f).errors), [DeckIssueCode::DigitalOnlyCard]);
    // Both on.
    f.card_policy.allow_digital_only = true;
    assert!(run(&d, &f).valid);
}

#[test]
fn req_ing_008_support_policy() {
    let casual = fmt();
    let tourn = fmt().with_tournament_policy();
    let main = |id: &'static str| deck(&[(id, 1), ("test.basic", 3)], &[]);
    // Unsupported rejected by default.
    let r = run(&main("test.unsup"), &casual);
    assert_eq!(codes(&r.errors), [DeckIssueCode::UnsupportedCard]);
    // Unsupported with WARN: warning, still valid.
    let mut w = casual.clone();
    w.card_policy.on_unsupported = PolicyAction::Warn;
    let r = run(&main("test.unsup"), &w);
    assert!(r.valid);
    assert_eq!(codes(&r.warnings), [DeckIssueCode::UnsupportedCard]);
    // Partial warns under casual, rejected under tournament.
    let r = run(&main("test.partial"), &casual);
    assert!(r.valid);
    assert_eq!(codes(&r.warnings), [DeckIssueCode::PartialCard]);
    let r = run(&main("test.partial"), &tourn);
    assert_eq!(codes(&r.errors), [DeckIssueCode::PartialCard]);
    // AutoParsed.
    assert!(run(&main("test.auto"), &casual).valid);
    let r = run(&main("test.auto"), &tourn);
    assert_eq!(codes(&r.errors), [DeckIssueCode::BelowMinSupport]);
    // Verified ok under both.
    assert!(run(&main("test.card_a"), &casual).valid);
    assert!(run(&main("test.card_a"), &tourn).valid);
    // Min status other than Verified adds nothing.
    let mut p = casual.clone();
    p.card_policy.min_support_status = SupportStatus::Unsupported;
    assert!(run(&main("test.auto"), &p).valid);
    // Unknown id.
    let r = run(&main("test.mystery"), &casual);
    assert_eq!(codes(&r.errors), [DeckIssueCode::UnknownCard]);
}

#[test]
fn req_fmt_001_invalid_count_format_and_merge() {
    let f = fmt();
    let r = run(
        &deck(
            &[("test.card_a", 0), ("test.basic", 4)],
            &[("test.card_a", 0)],
        ),
        &f,
    );
    assert_eq!(
        codes(&r.errors),
        [DeckIssueCode::InvalidCount, DeckIssueCode::InvalidCount]
    );
    assert!(!r.valid);
    // Invalid config: exactly one error.
    let mut bad = f.clone();
    bad.deck.max_copies = 0;
    let r = run(&deck(&ok_main(), &[]), &bad);
    assert_eq!(codes(&r.errors), [DeckIssueCode::InvalidFormatConfig]);
    assert_eq!(r.errors.len(), 1);
    assert!(r.errors[0].card_def_id.is_none());
    // Duplicates merge.
    let d = DeckSubmission {
        main: vec![
            e("test.card_a", 2),
            e("test.card_a", 2),
            e("test.card_a", 1),
        ],
        sideboard: vec![],
    };
    let r = run(&d, &f);
    assert_eq!(codes(&r.errors), [DeckIssueCode::TooManyCopies]);
    assert_eq!(r.errors.len(), 1);
}

#[test]
fn req_fmt_001_ordering_determinism_and_huge_counts() {
    let f = fmt();
    let d = deck(
        &[("test.zzz", 5), ("test.mystery", 5), ("test.card_a", 5)],
        &[],
    );
    let r = run(&d, &f);
    let ids: Vec<_> = r
        .errors
        .iter()
        .filter_map(|i| i.card_def_id.as_deref())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
    assert_eq!(r, run(&d, &f));
    let rev = deck(
        &[("test.card_a", 5), ("test.mystery", 5), ("test.zzz", 5)],
        &[],
    );
    assert_eq!(r, run(&rev, &f));
    // Overflow safety: many u32::MAX entries.
    let huge = DeckSubmission {
        main: (0..5)
            .map(|_| e("test.card_a", u32::MAX))
            .chain((0..3).map(|_| e("test.basic", u32::MAX)))
            .collect(),
        sideboard: vec![e("test.card_a", u32::MAX); 4],
    };
    let mut cap = f;
    cap.deck.max_main = Some(10);
    let r = run(&huge, &cap);
    assert!(!r.valid);
    assert!(codes(&r.errors).contains(&DeckIssueCode::MainTooLarge));
    assert!(codes(&r.errors).contains(&DeckIssueCode::SideboardTooLarge));
}

struct Xs(u64);
impl Xs {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn req_fmt_001_nfr_008_property_valid_matches_errors() {
    let ids = [
        "test.card_a",
        "test.card_b",
        "test.card_c",
        "test.basic",
        "test.digital",
        "test.unsup",
        "test.partial",
        "test.auto",
        "test.mystery",
    ];
    let mut rng = Xs(0x9E37_79B9_7F4A_7C15);
    for _ in 0..500 {
        let mut mk = |n: u64| -> Vec<DeckEntry> {
            (0..rng.next() % n)
                .map(|_| {
                    let id = ids[(rng.next() % ids.len() as u64) as usize];
                    let c = match rng.next() % 5 {
                        0 => 0,
                        1 => u32::MAX,
                        _ => (rng.next() % 6) as u32,
                    };
                    e(id, c)
                })
                .collect()
        };
        let d = DeckSubmission {
            main: mk(12),
            sideboard: mk(5),
        };
        let mut f = fmt();
        if rng.next().is_multiple_of(2) {
            f = f.with_tournament_policy();
        }
        f.digital_extensions.enabled = rng.next().is_multiple_of(2);
        let r = run(&d, &f);
        assert_eq!(r.valid, r.errors.is_empty());
        assert!(r.errors.iter().all(|i| i.severity == IssueSeverity::Error));
        assert!(
            r.warnings
                .iter()
                .all(|i| i.severity == IssueSeverity::Warning)
        );
        assert_eq!(r, run(&d, &f));
    }
}

#[test]
fn req_fmt_001_req_nfr_008_deserialization_survives_random_bytes() {
    let mut rng = Xs(0x1234_5678_9ABC_DEF1);
    let seeds = [
        SRS_JSON.as_bytes().to_vec(),
        br#"{"main":[{"cardDefId":"a","count":1}],"sideboard":[]}"#.to_vec(),
        br#"{"name":"a","version":"1","entries":{"x":"BANNED"}}"#.to_vec(),
    ];
    for i in 0..2000usize {
        let mut b = if i % 2 == 0 {
            (0..(rng.next() % 64)).map(|_| rng.next() as u8).collect()
        } else {
            seeds[i % 3].clone()
        };
        if i % 2 == 1 && !b.is_empty() {
            for _ in 0..3 {
                let at = (rng.next() % b.len() as u64) as usize;
                b[at] = rng.next() as u8;
            }
        }
        let _ = serde_json::from_slice::<FormatConfig>(&b);
        let _ = serde_json::from_slice::<BanList>(&b);
        let _ = serde_json::from_slice::<DeckSubmission>(&b);
    }
}
