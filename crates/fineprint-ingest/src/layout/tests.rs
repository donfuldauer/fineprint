// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests for the layout mapping (REQ-ING-003). All records are synthetic.

use std::collections::BTreeSet;

use fineprint_dsl::card::{ColorBitmask, FaceKind, LayoutBehavior, SupportStatus};

use super::*;
use crate::import::normalize::{NormalizedCardRecord, NormalizedFace, RelatedPart};

fn face(name: &str, types: &[&str], subtypes: &[&str], text: Option<&str>) -> NormalizedFace {
    NormalizedFace {
        name: name.to_string(),
        mana_cost: None,
        mana_cost_text: None,
        supertypes: Vec::new(),
        card_types: types.iter().map(|s| (*s).to_string()).collect(),
        subtypes: subtypes.iter().map(|s| (*s).to_string()).collect(),
        oracle_text: text.map(str::to_string),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        colors: None,
        color_indicator: None,
        computed_colors: ColorBitmask::NONE,
    }
}

fn plain(n: usize) -> Vec<NormalizedFace> {
    (0..n)
        .map(|i| face(&format!("test.face{i}"), &["Creature"], &[], None))
        .collect()
}

fn record(layout: &str, faces: Vec<NormalizedFace>) -> NormalizedCardRecord {
    NormalizedCardRecord {
        card_def_id: "test.id".to_string(),
        name: "test.alpha".to_string(),
        layout: layout.to_string(),
        faces,
        color_identity: ColorBitmask::NONE,
        source_color_identity: None,
        mana_value: 0,
        keywords: Vec::new(),
        all_parts: Vec::new(),
        legalities: Default::default(),
        game_changer: false,
        digital_only: false,
        oracle_text_hash: String::new(),
        diagnostics: Vec::new(),
    }
}

fn part(component: &str, name: &str) -> RelatedPart {
    RelatedPart {
        id: None,
        component: Some(component.to_string()),
        name: Some(name.to_string()),
    }
}

fn mapping(r: &NormalizedCardRecord) -> LayoutMapping {
    match map_layout(r) {
        Ok(LayoutOutcome::Mapped(m)) => m,
        other => panic!("expected Mapped, got {other:?}"),
    }
}

/// REQ-ING-003: every row of the table, positive form.
#[test]
fn table_every_row() {
    use FaceKind::{Adventure, Back, Flipped, LeftHalf, Main, Omen, PrepareSpell, RightHalf};
    use LayoutBehavior as L;
    let station_text = "Station\n3+ | Test text.";
    let mut station = record(
        "normal",
        vec![face("s", &["Spacecraft"], &[], Some(station_text))],
    );
    station.keywords = vec!["Station".into()];
    let mut meld = record("meld", plain(1));
    meld.all_parts = vec![part("meld_part", "test.beta")];
    let room = record(
        "split",
        vec![
            face("a", &["Enchantment"], &["Room"], None),
            face("b", &["Enchantment"], &["Room"], None),
        ],
    );
    let omen = record(
        "adventure",
        vec![
            face("a", &["Creature"], &[], None),
            face("b", &["Instant"], &["Omen"], None),
        ],
    );
    let rows: Vec<(&str, NormalizedCardRecord, L, Vec<FaceKind>, bool)> = vec![
        (
            "normal",
            record("normal", plain(1)),
            L::Normal,
            vec![Main],
            false,
        ),
        ("station", station, L::Station, vec![Main], false),
        ("room", room, L::Room, vec![LeftHalf, RightHalf], false),
        (
            "split",
            record("split", plain(2)),
            L::Split,
            vec![LeftHalf, RightHalf],
            false,
        ),
        (
            "flip",
            record("flip", plain(2)),
            L::Flip,
            vec![Main, Flipped],
            false,
        ),
        (
            "transform",
            record("transform", plain(2)),
            L::DfcNonmodal,
            vec![Main, Back],
            false,
        ),
        (
            "modal_dfc",
            record("modal_dfc", plain(2)),
            L::DfcModal,
            vec![Main, Back],
            false,
        ),
        ("meld", meld, L::Meld, vec![Main], false),
        ("omen", omen, L::Omen, vec![Main, Omen], false),
        (
            "adventure",
            record("adventure", plain(2)),
            L::Adventurer,
            vec![Main, Adventure],
            false,
        ),
        (
            "prepare",
            record("prepare", plain(2)),
            L::Preparation,
            vec![Main, PrepareSpell],
            false,
        ),
        (
            "prototype",
            record("prototype", plain(1)),
            L::Prototype,
            vec![Main],
            false,
        ),
        (
            "mutate",
            record("mutate", plain(1)),
            L::Mutate,
            vec![Main],
            false,
        ),
        (
            "leveler",
            record("leveler", plain(1)),
            L::Leveler,
            vec![Main],
            false,
        ),
        (
            "class",
            record("class", plain(1)),
            L::Class,
            vec![Main],
            false,
        ),
        ("case", record("case", plain(1)), L::Case, vec![Main], false),
        ("saga", record("saga", plain(1)), L::Saga, vec![Main], false),
        (
            "battle1",
            record("battle", plain(1)),
            L::DfcNonmodal,
            vec![Main],
            false,
        ),
        (
            "battle2",
            record("battle", plain(2)),
            L::DfcNonmodal,
            vec![Main, Back],
            false,
        ),
        (
            "planar",
            record("planar", plain(1)),
            L::Planar,
            vec![Main],
            true,
        ),
        (
            "scheme",
            record("scheme", plain(1)),
            L::Scheme,
            vec![Main],
            true,
        ),
        (
            "vanguard",
            record("vanguard", plain(1)),
            L::Vanguard,
            vec![Main],
            true,
        ),
        (
            "token",
            record("token", plain(1)),
            L::Token,
            vec![Main],
            false,
        ),
        (
            "dft",
            record("double_faced_token", plain(2)),
            L::DfcToken,
            vec![Main, Back],
            false,
        ),
        (
            "emblem",
            record("emblem", plain(1)),
            L::Token,
            vec![Main],
            false,
        ),
    ];
    for (name, rec, behavior, kinds, gated) in rows {
        let m = mapping(&rec);
        assert_eq!(m.behavior, behavior, "{name}");
        assert_eq!(m.face_kinds, kinds, "{name}");
        assert_eq!(m.variant_gated, gated, "{name}");
        assert_eq!(m.face_kinds.len(), rec.faces.len(), "{name}");
    }
    assert_eq!(
        mapping(&record("token", plain(1))).token_kind,
        Some(TokenKind::Token)
    );
    assert_eq!(
        mapping(&record("double_faced_token", plain(2))).token_kind,
        Some(TokenKind::Token)
    );
    assert_eq!(
        mapping(&record("emblem", plain(1))).token_kind,
        Some(TokenKind::Emblem)
    );
    assert_eq!(mapping(&record("normal", plain(1))).token_kind, None);
    for (layout, reason) in [
        ("art_series", SkipReason::ArtSeries),
        ("reversible_card", SkipReason::ReversibleCard),
        ("front_card", SkipReason::FrontCard),
    ] {
        // Skipped outcomes do not check the face count.
        assert_eq!(
            map_layout(&record(layout, vec![])),
            Ok(LayoutOutcome::Skipped(reason))
        );
        assert!(!reason.as_str().is_empty());
    }
    for layout in ["augment", "host"] {
        let out = map_layout(&record(layout, vec![])).unwrap();
        assert_eq!(
            out,
            LayoutOutcome::Unsupported(UnsupportedReason::UnsetMechanic)
        );
    }
    assert!(
        UnsupportedReason::UnsetMechanic
            .as_str()
            .contains("ext.unset")
    );
    assert_eq!(
        UnsupportedReason::UnsetMechanic.support_status(),
        SupportStatus::Unsupported
    );
}

fn station_record(layout: &str, text: &str, keywords: &[&str]) -> NormalizedCardRecord {
    let mut r = record(layout, vec![face("s", &["Spacecraft"], &[], Some(text))]);
    r.keywords = keywords.iter().map(|s| (*s).to_string()).collect();
    r
}

fn behavior_of(r: &NormalizedCardRecord) -> LayoutBehavior {
    mapping(r).behavior
}

/// REQ-ING-003: STATION detection, positives and negatives.
#[test]
fn station_detection() {
    use LayoutBehavior::{DfcNonmodal, Normal, Station};
    // Text line begins with the word, no keywords entry.
    assert_eq!(
        behavior_of(&station_record("normal", "Station\n3+ | Test.", &[])),
        Station
    );
    // Keyword entry, no word line.
    assert_eq!(
        behavior_of(&station_record("normal", "Test.\n12+ |", &["Station"])),
        Station
    );
    assert_eq!(
        behavior_of(&station_record("normal", "Station x\n3+ | T", &[])),
        Station
    );
    assert_eq!(
        behavior_of(&station_record("normal", "Station\n123456+   | T", &[])),
        Station
    );
    // Negatives.
    assert_eq!(
        behavior_of(&station_record("normal", "Test.", &["Station"])),
        Normal
    );
    assert_eq!(
        behavior_of(&station_record("normal", "3+ | Test.", &[])),
        Normal
    );
    assert_eq!(
        behavior_of(&station_record(
            "normal",
            "Do a Station thing.\n3+ | T",
            &[]
        )),
        Normal
    );
    assert_eq!(
        behavior_of(&station_record("normal", "Station\n3 | T", &["Station"])),
        Normal
    );
    assert_eq!(
        behavior_of(&station_record("normal", "Station\n3+ T", &["Station"])),
        Normal
    );
    assert_eq!(
        behavior_of(&station_record(
            "normal",
            "Station\n1234567+ | T",
            &["Station"]
        )),
        Normal
    );
    assert_eq!(
        behavior_of(&station_record("normal", "Station\n+ | T", &["Station"])),
        Normal
    );
    assert_eq!(
        behavior_of(&station_record("normal", "Station\n x3+ | T", &["Station"])),
        Normal
    );
    assert_eq!(
        behavior_of(&station_record("normal", "Stationary\n3+ | T", &[])),
        Normal
    );
    // Only layout normal is considered.
    let mut t = station_record("transform", "Station\n3+ | Test.", &["Station"]);
    t.faces.push(face("b", &["Creature"], &[], None));
    assert_eq!(behavior_of(&t), DfcNonmodal);
}

/// REQ-ING-003: ROOM and OMEN detection.
#[test]
fn room_and_omen_detection() {
    use LayoutBehavior::{Adventurer, Omen, Room, Split};
    let split = |a: (&[&str], &[&str]), b: (&[&str], &[&str])| {
        record(
            "split",
            vec![face("a", a.0, a.1, None), face("b", b.0, b.1, None)],
        )
    };
    assert_eq!(
        behavior_of(&split(
            (&["Enchantment"], &["Room"]),
            (&["Enchantment"], &[])
        )),
        Room
    );
    assert_eq!(
        behavior_of(&split((&["Room"], &[]), (&["Instant"], &[]))),
        Room
    );
    assert_eq!(
        behavior_of(&split((&["Instant"], &[]), (&["Instant"], &["Room"]))),
        Room
    );
    assert_eq!(
        behavior_of(&split(
            (&["Instant"], &["Roomy"]),
            (&["Instant"], &["room"])
        )),
        Split
    );
    assert_eq!(
        behavior_of(&split((&["Instant"], &[]), (&["Instant"], &[]))),
        Split
    );
    let adv = |a: (&[&str], &[&str]), b: (&[&str], &[&str])| {
        record(
            "adventure",
            vec![face("a", a.0, a.1, None), face("b", b.0, b.1, None)],
        )
    };
    assert_eq!(
        behavior_of(&adv((&["Creature"], &[]), (&["Instant"], &["Omen"]))),
        Omen
    );
    assert_eq!(
        behavior_of(&adv((&["Creature"], &[]), (&["Omen"], &[]))),
        Omen
    );
    assert_eq!(
        behavior_of(&adv((&["Creature"], &["Omen"]), (&["Instant"], &[]))),
        Adventurer
    );
    assert_eq!(
        behavior_of(&adv((&["Creature"], &[]), (&["Instant"], &["Omens"]))),
        Adventurer
    );
    // A single-face adventure is a face-count error, not an Omen.
    let one = record("adventure", vec![face("a", &["Creature"], &["Omen"], None)]);
    assert_eq!(map_layout(&one), Err(LayoutError::FaceCount));
}

/// REQ-ING-003: MELD partner detection.
#[test]
fn meld() {
    let mut r = record("meld", plain(1));
    r.all_parts = vec![
        part("meld_result", "test.gamma"),
        part("meld_part", "test.beta"),
        part("meld_part", "test.alpha"),
        part("meld_part", "test.beta"),
        part("token", "test.zeta"),
        part("combo_piece", "test.eta"),
        RelatedPart {
            id: None,
            component: Some("meld_part".into()),
            name: None,
        },
        RelatedPart {
            id: None,
            component: None,
            name: Some("test.theta".into()),
        },
    ];
    let m = mapping(&r);
    assert_eq!(m.behavior, LayoutBehavior::Meld);
    assert_eq!(
        m.meld_partners,
        vec!["test.beta".to_string(), "test.gamma".to_string()]
    );
    // Missing partner, in each form.
    let mut none = record("meld", plain(1));
    assert_eq!(map_layout(&none), Err(LayoutError::MeldPartnerMissing));
    none.all_parts = vec![part("meld_part", "test.alpha"), part("token", "test.beta")];
    assert_eq!(map_layout(&none), Err(LayoutError::MeldPartnerMissing));
    assert_eq!(
        LayoutError::MeldPartnerMissing.support_status(),
        SupportStatus::Unsupported
    );
    // Meld with two faces is a face-count error.
    let mut two = record("meld", plain(2));
    two.all_parts = vec![part("meld_part", "test.beta")];
    assert_eq!(map_layout(&two), Err(LayoutError::FaceCount));
    // Other layouts have no partners.
    assert!(
        mapping(&record("normal", plain(1)))
            .meld_partners
            .is_empty()
    );
}

/// REQ-ING-003: face-count errors for every Mapped row.
#[test]
fn face_count_errors() {
    let layouts: [(&str, usize); 20] = [
        ("normal", 1),
        ("split", 2),
        ("flip", 2),
        ("transform", 2),
        ("modal_dfc", 2),
        ("adventure", 2),
        ("prepare", 2),
        ("prototype", 1),
        ("mutate", 1),
        ("leveler", 1),
        ("class", 1),
        ("case", 1),
        ("saga", 1),
        ("planar", 1),
        ("scheme", 1),
        ("vanguard", 1),
        ("token", 1),
        ("double_faced_token", 2),
        ("emblem", 1),
        ("battle", 1),
    ];
    for (layout, n) in layouts {
        let ok = n;
        assert!(map_layout(&record(layout, plain(ok))).is_ok(), "{layout}");
        for bad in [0usize, 3, 4] {
            assert_eq!(
                map_layout(&record(layout, plain(bad))),
                Err(LayoutError::FaceCount),
                "{layout} {bad}"
            );
        }
        if n == 1 {
            let two = map_layout(&record(layout, plain(2)));
            if layout == "battle" {
                assert!(two.is_ok());
            } else {
                assert_eq!(two, Err(LayoutError::FaceCount), "{layout}");
            }
        } else {
            assert_eq!(
                map_layout(&record(layout, plain(1))),
                Err(LayoutError::FaceCount),
                "{layout}"
            );
        }
    }
    let mut meld = record("meld", vec![]);
    meld.all_parts = vec![part("meld_part", "test.beta")];
    assert_eq!(map_layout(&meld), Err(LayoutError::FaceCount));
    assert_eq!(LayoutError::FaceCount.as_str(), "faceCount");
}

/// REQ-ING-003: unknown layouts are import errors; the batch continues; output is deterministic.
#[test]
fn unknown_layout_error() {
    for layout in [
        "",
        "attraction",
        "conspiracy",
        "Normal",
        "weird_secret_layout",
    ] {
        let e = map_layout(&record(layout, plain(1))).unwrap_err();
        assert_eq!(e, LayoutError::UnknownLayout);
        assert_eq!(e.support_status(), SupportStatus::Unsupported);
        assert_eq!(e.as_str(), "unknownLayout");
        assert!(layout.is_empty() || !e.as_str().contains(layout));
    }
    let mk = |id: &str, layout: &str| {
        let mut r = record(layout, plain(1));
        r.card_def_id = id.to_string();
        r
    };
    let a = vec![
        mk("c", "normal"),
        mk("b", "bogus"),
        mk("a", "saga"),
        mk("d", "nope"),
        mk("e", "art_series"),
    ];
    let mut b = a.clone();
    b.reverse();
    let out = map_records(&a);
    assert_eq!(out, map_records(&a));
    assert_eq!(out, map_records(&b));
    assert_eq!(
        out.outcomes.keys().cloned().collect::<Vec<_>>(),
        ["a", "c", "e"]
    );
    assert_eq!(
        out.errors,
        vec![
            ("b".to_string(), LayoutError::UnknownLayout),
            ("d".to_string(), LayoutError::UnknownLayout)
        ]
    );
    assert_eq!(map_records(&[]), LayoutImport::default());
}

/// REQ-ING-003: the reachable behaviors cover every table row; `Verified` is never produced.
#[test]
fn every_layout_behavior_covered() {
    let mut meld = record("meld", plain(1));
    meld.all_parts = vec![part("meld_part", "test.beta")];
    let mut reached = BTreeSet::new();
    let mut add = |r: &NormalizedCardRecord| {
        if let Ok(LayoutOutcome::Mapped(m)) = map_layout(r) {
            reached.insert(m.behavior.as_str());
        }
    };
    for (l, n) in [
        ("normal", 1),
        ("split", 2),
        ("flip", 2),
        ("transform", 2),
        ("modal_dfc", 2),
        ("adventure", 2),
        ("prepare", 2),
        ("prototype", 1),
        ("mutate", 1),
        ("leveler", 1),
        ("class", 1),
        ("case", 1),
        ("saga", 1),
        ("battle", 1),
        ("planar", 1),
        ("scheme", 1),
        ("vanguard", 1),
        ("token", 1),
        ("double_faced_token", 2),
        ("emblem", 1),
    ] {
        add(&record(l, plain(n)));
    }
    add(&meld);
    add(&station_record("normal", "Station\n3+ | T", &[]));
    add(&record(
        "split",
        vec![face("a", &["Room"], &[], None), face("b", &[], &[], None)],
    ));
    add(&record(
        "adventure",
        vec![face("a", &[], &[], None), face("b", &[], &["Omen"], None)],
    ));
    let expected: BTreeSet<&str> = [
        "NORMAL",
        "STATION",
        "SPLIT",
        "ROOM",
        "FLIP",
        "DFC_NONMODAL",
        "DFC_MODAL",
        "MELD",
        "ADVENTURER",
        "OMEN",
        "PREPARATION",
        "PROTOTYPE",
        "MUTATE",
        "LEVELER",
        "CLASS",
        "CASE",
        "SAGA",
        "PLANAR",
        "SCHEME",
        "VANGUARD",
        "TOKEN",
        "DFC_TOKEN",
    ]
    .into_iter()
    .collect();
    assert_eq!(reached, expected);
    // Every error and unsupported path reports Unsupported, never Verified.
    for e in [
        LayoutError::UnknownLayout,
        LayoutError::FaceCount,
        LayoutError::MeldPartnerMissing,
    ] {
        assert_ne!(e.support_status(), SupportStatus::Verified);
    }
    assert_ne!(
        UnsupportedReason::UnsetMechanic.support_status(),
        SupportStatus::Verified
    );
}

/// xorshift64, fixed seed (no clock, no crate).
struct Xs(u64);
impl Xs {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

/// REQ-ING-003, R12: bounded pseudo-random records never panic.
#[test]
fn bounded_random_no_panic() {
    let layouts = [
        "normal",
        "split",
        "flip",
        "transform",
        "modal_dfc",
        "meld",
        "adventure",
        "prepare",
        "prototype",
        "mutate",
        "leveler",
        "class",
        "case",
        "saga",
        "battle",
        "planar",
        "scheme",
        "vanguard",
        "token",
        "double_faced_token",
        "emblem",
        "augment",
        "host",
        "art_series",
        "reversible_card",
        "front_card",
        "",
        "\u{0}",
        "né\u{1F600}",
        "zzz",
    ];
    let frags = [
        "Station",
        "\n",
        "3+ |",
        "12+",
        " ",
        "|",
        "+",
        "999999999999+ |",
        "Room",
        "Omen",
        "é",
        "\u{0007}",
        "\r\n",
        "7",
        "\u{2014}",
    ];
    let types = ["Room", "Omen", "Creature", "", "Roomy", "é"];
    let mut rng = Xs(0x9E37_79B9_7F4A_7C15);
    for _ in 0..2000 {
        let nfaces = (rng.next() % 4) as usize;
        let faces = (0..nfaces)
            .map(|_| {
                let mut text = String::new();
                for _ in 0..(rng.next() % 8) {
                    text.push_str(rng.pick(&frags));
                }
                let t = rng.pick(&types);
                face("f", &[t], &[rng.pick(&types)], Some(&text))
            })
            .collect();
        let mut r = record(rng.pick(&layouts), faces);
        if rng.next().is_multiple_of(2) {
            r.keywords.push(rng.pick(&frags).to_string());
        }
        for _ in 0..(rng.next() % 3) {
            r.all_parts.push(part(
                rng.pick(&["meld_part", "meld_result", "token", ""]),
                rng.pick(&frags),
            ));
        }
        match map_layout(&r) {
            Ok(
                LayoutOutcome::Mapped(_)
                | LayoutOutcome::Skipped(_)
                | LayoutOutcome::Unsupported(_),
            )
            | Err(_) => {}
        }
    }
}

/// REQ-ING-003, R12: adversarial large and non-ASCII input.
#[test]
fn adversarial_input() {
    let big = "9".repeat(1 << 20);
    let r = station_record("normal", &big, &["Station"]);
    assert_eq!(behavior_of(&r), LayoutBehavior::Normal);
    let mut text = "Station\n".to_string();
    text.push_str(&"é1+ |\n".repeat(100_000));
    text.push_str(&"1".repeat(1 << 20));
    text.push_str("+ |");
    assert_eq!(
        behavior_of(&station_record("normal", &text, &[])),
        LayoutBehavior::Normal
    );
    assert_eq!(
        map_layout(&record("normal", vec![])),
        Err(LayoutError::FaceCount)
    );
    let long = "x".repeat(1 << 20);
    assert_eq!(
        map_layout(&record(&long, vec![])),
        Err(LayoutError::UnknownLayout)
    );
    assert!(
        !SkipReason::ArtSeries.as_str().is_empty()
            && !LayoutError::UnknownLayout.as_str().contains('x')
    );
}
