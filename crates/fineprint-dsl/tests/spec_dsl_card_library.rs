// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the shared synthetic card library under `tests/fixtures/cards`
//! (REQ-TEST-001, REQ-NFR-004; REQ-ING-009 for the lint run). Every card is SYNTHETIC (R5).
//! The expected characteristics table is written by hand from each card's cost, not computed.

#[path = "../../../tests/fixtures/cards/loader.rs"]
mod card_library;

use std::collections::BTreeSet;

use fineprint_dsl::card::{CardDefinitionDSL, ColorBitmask};
use fineprint_dsl::lint::{
    LintContext, LintId, Severity, SourceCharacteristics, SourceFaceCharacteristics,
};
use fineprint_dsl::text::TextToken;

const FAMILIES: [&str; 6] = [
    "creatures",
    "lands",
    "keywords",
    "spells",
    "auras",
    "equipment",
];

const W: u8 = 1;
const U: u8 = 2;
const B: u8 = 4;
const R: u8 = 8;
const G: u8 = 16;

/// Hand-written expectations: (cardDefId, mana value, face colors, color identity), one face each.
/// Lands have no cost and no color; a colored mana land's identity is its produced color.
const EXPECTED: [(&str, u32, u8, u8); 30] = [
    ("test.vanilla_white_1_1", 1, W, W),
    ("test.vanilla_blue_1_3", 2, U, U),
    ("test.vanilla_black_2_1", 2, B, B),
    ("test.vanilla_red_3_1", 3, R, R),
    ("test.vanilla_green_2_2", 2, G, G),
    ("test.vanilla_colorless_2_2", 2, 0, 0),
    ("test.vanilla_gold_wu_2_2", 2, W | U, W | U),
    ("test.vanilla_green_4_4", 4, G, G),
    ("test.vanilla_red_5_5", 6, R, R),
    ("test.vanilla_white_0_4", 3, W, W),
    ("test.vanilla_black_1_1_no_cost", 0, 0, 0),
    ("test.mana_land_white", 0, 0, W),
    ("test.mana_land_blue", 0, 0, U),
    ("test.mana_land_black", 0, 0, B),
    ("test.mana_land_red", 0, 0, R),
    ("test.mana_land_green", 0, 0, G),
    ("test.mana_land_colorless", 0, 0, 0),
    ("test.flier_2_2", 2, W, W),
    ("test.first_strike_2_1", 2, R, R),
    ("test.trample_3_3", 4, G, G),
    ("test.haste_2_1", 2, R, R),
    ("test.lifelink_2_2", 3, W, W),
    ("test.deathtouch_1_1", 1, B, B),
    ("test.vigilance_2_3", 3, W, W),
    ("test.burn_3_any_target", 1, R, R),
    ("test.draw_two", 3, U, U),
    ("test.aura_plus_2_2", 2, W, W),
    ("test.aura_plus_0_3", 2, U, U),
    ("test.equipment_plus_1_1", 1, 0, 0),
    ("test.equipment_plus_2_0", 2, 0, 0),
];

/// REQ-ING-009; CR 903.4; remove under #252 (FP-172).
const COLORED_LANDS: [&str; 5] = [
    "test.mana_land_white",
    "test.mana_land_blue",
    "test.mana_land_black",
    "test.mana_land_red",
    "test.mana_land_green",
];

fn mask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).expect("bits <= 31")
}

/// REQ-TEST-001: at least 30 cards, and each of the six families is non-empty.
#[test]
fn req_test_001_library_has_at_least_30_cards_and_each_family() {
    let cards = card_library::load_all();
    assert!(cards.len() >= 30, "only {} cards", cards.len());
    for family in FAMILIES {
        assert!(
            cards.iter().any(|c| c.family == family),
            "family {family} is empty"
        );
    }
}

/// REQ-TEST-001: every file loads strictly, and its bytes re-hash to the declared hash.
#[test]
fn req_test_001_every_card_loads_strictly() {
    for path in card_library::library_paths() {
        let def = card_library::load_path(&path);
        let bytes = std::fs::read(&path).expect("read card file");
        let again = CardDefinitionDSL::from_json(&bytes).expect("strict reload");
        assert_eq!(
            again.compute_hash().expect("hash"),
            def.definition_hash,
            "{}",
            path.display()
        );
        assert_eq!(again, def, "{}", path.display());
    }
}

/// REQ-ING-009, REQ-TEST-001: every card has zero lint findings against the hand-written
/// characteristics; the table has exactly one entry per library card.
#[test]
fn req_ing_009_every_card_is_lint_clean() {
    let cards = card_library::load_all();
    let in_table: BTreeSet<&str> = EXPECTED.iter().map(|e| e.0).collect();
    assert_eq!(in_table.len(), EXPECTED.len(), "duplicate table entry");
    let in_library: BTreeSet<&str> = cards.iter().map(|c| c.def.card_def_id.as_str()).collect();
    assert_eq!(in_table, in_library, "table and library differ");

    let mut failures = Vec::new();
    for card in &cards {
        let (_, mv, colors, identity) = EXPECTED
            .iter()
            .find(|e| e.0 == card.def.card_def_id)
            .expect("table entry");
        let ctx = LintContext {
            source: Some(SourceCharacteristics {
                faces: vec![SourceFaceCharacteristics {
                    mana_value: Some(*mv),
                    colors: Some(mask(*colors)),
                }],
                // For the five colored lands the source comparison would add a second
                // warning of the same limitation; their identity is asserted directly below.
                color_identity: (!COLORED_LANDS.contains(&card.def.card_def_id.as_str()))
                    .then(|| mask(*identity)),
            }),
            known_token_ids: None,
        };
        assert_eq!(
            card.def.color_identity,
            mask(*identity),
            "{}: declared colorIdentity differs from the table",
            card.path.display()
        );
        let report = fineprint_dsl::lint::lint_definition(&card.def, &ctx);
        // REQ-ING-009; CR 903.4; remove under #252 (FP-172): the lint cannot yet derive
        // produced-mana symbols, so exactly the five colored mana lands carry one
        // FPL-COLOR-IDENTITY warning and nothing else.
        if COLORED_LANDS.contains(&card.def.card_def_id.as_str()) {
            let all: Vec<_> = report.errors().chain(report.warnings()).collect();
            let ok = all.len() == 1
                && all[0].id == LintId::COLOR_IDENTITY
                && all[0].severity == Severity::Warning;
            if !ok {
                failures.push(format!(
                    "{}: expected exactly one FPL-COLOR-IDENTITY warning, got {} finding(s): {}",
                    card.path.display(),
                    all.len(),
                    all.iter()
                        .map(|l| l.to_string())
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
        } else if !report.is_empty() {
            for lint in report.errors().chain(report.warnings()) {
                failures.push(format!("{}: {lint}", card.path.display()));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "lint findings:\n{}",
        failures.join("\n")
    );
}

/// REQ-NFR-004, REQ-TEST-001: naming convention and layout.
#[test]
fn req_nfr_004_naming_convention() {
    let mut seen = BTreeSet::new();
    for card in card_library::load_all() {
        let id = &card.def.card_def_id;
        let path = card.path.display();
        let rest = id
            .strip_prefix("test.")
            .unwrap_or_else(|| panic!("{path}: id {id} lacks the test. prefix"));
        assert!(
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{path}: id {id} has characters outside [a-z0-9_]"
        );
        let stem = card
            .path
            .file_stem()
            .expect("file stem")
            .to_string_lossy()
            .into_owned();
        assert_eq!(rest, stem, "{path}: id does not match the file stem");
        assert!(
            FAMILIES.contains(&card.family.as_str()),
            "{path}: unknown family {}",
            card.family
        );
        let family_dir = card.path.parent().and_then(|p| p.parent());
        assert_eq!(
            family_dir,
            Some(card_library::library_root().as_path()),
            "{path}: not directly under a family directory"
        );
        assert!(!card.def.faces.is_empty(), "{path}: no faces");
        for face in &card.def.faces {
            assert_eq!(&face.name, id, "{path}: face name differs from cardDefId");
        }
        assert!(seen.insert(id.clone()), "{path}: duplicate id {id}");
    }
}

/// REQ-NFR-004: no card-data member names; every LITERAL token text starts with "test ".
#[test]
fn req_nfr_004_no_card_data_members() {
    const FORBIDDEN: [&str; 5] = [
        "oracle_text",
        "oracleText",
        "flavor_text",
        "artist",
        "image_uris",
    ];
    for path in card_library::library_paths() {
        let text = std::fs::read_to_string(&path).expect("read card file");
        for word in FORBIDDEN {
            // Quoted, so the allowed member `oracleTextHash` does not match `oracleText`.
            assert!(
                !text.contains(&format!("\"{word}\"")),
                "{}: contains forbidden member name {word}",
                path.display()
            );
        }
    }
    for card in card_library::load_all() {
        for face in &card.def.faces {
            // #232 FP-164: the text position is the typed token stream.
            for token in face.text.tokens() {
                if let TextToken::Literal { text, .. } = token {
                    assert!(
                        text.starts_with("test "),
                        "{}: LITERAL token {text:?} does not start with \"test \"",
                        card.path.display()
                    );
                }
            }
        }
    }
}

/// Generator: rewrites `definitionHash` in place for every library file (README, "Adding a card").
/// Run: `cargo test -p fineprint-dsl --test spec_dsl_card_library -- --ignored seal_library`.
#[test]
#[ignore = "generator: rewrites library files in place"]
fn seal_library() {
    for path in card_library::library_paths() {
        let bytes = std::fs::read(&path).expect("read card file");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("plain JSON");
        value["definitionHash"] = serde_json::Value::String(String::new());
        let def: CardDefinitionDSL = serde_json::from_value(value)
            .unwrap_or_else(|e| panic!("{}: not a card definition: {e}", path.display()));
        let sealed = def.seal().expect("seal");
        let value = serde_json::to_value(&sealed).expect("value");
        let mut out = serde_json::to_string_pretty(&value).expect("pretty");
        out.push('\n');
        std::fs::write(&path, out).expect("write card file");
    }
}
