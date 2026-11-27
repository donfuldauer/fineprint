// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Alignment of the sample scenarios with the synthetic card library (REQ-TEST-001 item 1,
//! REQ-NFR-004): every `cardDefId` a sample uses exists under `tests/fixtures/cards/`, and the
//! sample's definition hash equals the library file's `definitionHash`.
//! Black-box: written from the work-item brief (FP-049) only; plain file reads.

#[path = "spec_scenario_support.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;
use support::*;

/// `cardDefId` -> `definitionHash` of every library file.
fn library() -> BTreeMap<String, String> {
    fn walk(dir: &Path, out: &mut BTreeMap<String, String>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "json") {
                let v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
                let id = v["cardDefId"].as_str().unwrap().to_string();
                let h = v["definitionHash"].as_str().unwrap().to_string();
                assert!(
                    out.insert(id.clone(), h).is_none(),
                    "duplicate library id {id}"
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(
        &workspace_root()
            .join("tests")
            .join("fixtures")
            .join("cards"),
        &mut out,
    );
    out
}

/// Every `card` (object form) or string card reference in a sample's setup.
fn cards_used(file: &Value) -> Vec<String> {
    let card_of = |r: &Value| -> Option<String> {
        match r {
            Value::String(s) => Some(s.clone()),
            o => o.get("card").and_then(Value::as_str).map(str::to_string),
        }
    };
    let mut out = Vec::new();
    for key in ["battlefield", "stack", "sharedExile"] {
        out.extend(
            file[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(card_of),
        );
    }
    for p in file["players"].as_array().into_iter().flatten() {
        if let Some(zones) = p.get("zones").and_then(Value::as_object) {
            for refs in zones.values() {
                out.extend(refs.as_array().into_iter().flatten().filter_map(card_of));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// REQ-TEST-001.1, REQ-NFR-004: every card used by a sample is a library card and its hash in
/// the sample equals the library's; every hash listed belongs to a library card.
#[test]
fn req_test_001_dsl_alignment_samples_use_library_hashes() {
    let lib = library();
    assert!(!lib.is_empty());
    let mut used_any = 0;
    for name in SAMPLES {
        let file: Value = serde_json::from_slice(&sample_bytes(name)).unwrap();
        let hashes = file["cardDatabase"]["definitionHashes"]
            .as_object()
            .unwrap();
        for card in cards_used(&file) {
            assert!(card.starts_with("test."), "{name}: {card} is not synthetic");
            let lib_hash = lib
                .get(&card)
                .unwrap_or_else(|| panic!("{name}: {card} not in library"));
            assert_eq!(
                hashes.get(&card).and_then(Value::as_str),
                Some(lib_hash.as_str()),
                "{name}: {card}"
            );
            used_any += 1;
        }
        for (card, h) in hashes {
            assert_eq!(
                lib.get(card).map(String::as_str),
                h.as_str(),
                "{name}: listed {card}"
            );
        }
    }
    assert!(used_any > 0);
}

/// REQ-TEST-001.1, REQ-NFR-004: the same holds for the wrong twins and the support module's
/// hash constants.
#[test]
fn req_test_001_dsl_alignment_twins_and_constants() {
    let lib = library();
    for (_, wrong, _) in WRONG_TWINS {
        let file: Value = serde_json::from_slice(&negative_bytes(wrong)).unwrap();
        for (card, h) in file["cardDatabase"]["definitionHashes"]
            .as_object()
            .unwrap()
        {
            assert_eq!(
                lib.get(card).map(String::as_str),
                h.as_str(),
                "{wrong}: {card}"
            );
        }
    }
    for (card, h) in [
        ("test.vanilla_green_2_2", GREEN_2_2_HASH),
        ("test.vanilla_white_1_1", WHITE_1_1_HASH),
        ("test.flier_2_2", FLIER_2_2_HASH),
        ("test.mana_land_green", LAND_GREEN_HASH),
        ("test.aura_plus_2_2", AURA_2_2_HASH),
        ("test.draw_two", DRAW_TWO_HASH),
    ] {
        assert_eq!(lib.get(card).map(String::as_str), Some(h), "{card}");
    }
}
