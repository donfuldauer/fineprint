// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the shape of the SBA registry (REQ-SBA-001, REQ-TEST-001; CR 704.5a-704.5i,
//! 704.5j, 704.5k, 704.5m, 704.5n, 704.5p, 704.5q, 704.5r, 704.5s, 704.5t, 704.5c, 704.6b).
//! Black-box: written from the work-item brief only. The CR has no 704.5l and no 704.5o.

#[path = "spec_sba_support.rs"]
mod support;

use fineprint_core::sba::{GameVariant, SbaAction, SbaRegistry};
use support::*;

// REQ-SBA-001, REQ-TEST-001, CR 704.5a-704.5t: the standard registry holds exactly the entries
// 704.5a..704.5i, then 704.5j, 704.5k, 704.5m, 704.5n, 704.5p, 704.5q, 704.5r, 704.5s, 704.5t
// in that order, each with `cr_ref` "CR <id>".
#[test]
fn req_sba_001_registry_ids_cr_refs_and_order() {
    let r = SbaRegistry::standard();
    let ids: Vec<&str> = r.entries().iter().map(|e| e.id).collect();
    assert_eq!(ids, ENTRY_IDS.to_vec());
    let refs: Vec<&str> = r.entries().iter().map(|e| e.cr_ref).collect();
    let expected: Vec<String> = ENTRY_IDS.iter().map(|id| format!("CR {id}")).collect();
    assert_eq!(refs, expected);
}

// REQ-SBA-001, CR 704.5a-704.5t: `get` finds every entry by id and nothing else; 704.5l and
// 704.5o do not exist in the CR and are absent.
#[test]
fn req_sba_001_registry_get() {
    let r = SbaRegistry::standard();
    for id in ENTRY_IDS {
        let e = r.get(id).unwrap_or_else(|| panic!("{id} present"));
        assert_eq!(e.id, id);
        assert_eq!(e.cr_ref, format!("CR {id}"));
    }
    for absent in [
        "704.5l",
        "704.5o",
        "704.5",
        "",
        "CR 704.5a",
        "704.6b",
        "704.5A",
        "704.5J",
        "704.5u",
        "CR 704.5j",
    ] {
        assert!(r.get(absent).is_none(), "{absent:?} must be absent");
    }
}

// REQ-SBA-001, CR 704.5c, 704.6b: 704.5c is ignored in Two-Headed Giant and active in a
// standard game; every other entry (704.5a included) is active in both.
#[test]
fn req_sba_001_registry_active_in_variant_gate() {
    let r = SbaRegistry::standard();
    for e in r.entries() {
        assert!(
            e.active_in(GameVariant::Standard),
            "{} active in Standard",
            e.id
        );
        assert_eq!(
            e.active_in(GameVariant::TwoHeadedGiant),
            e.id != "704.5c",
            "{} in Two-Headed Giant",
            e.id
        );
    }
    assert!(
        r.get("704.5a")
            .expect("present")
            .active_in(GameVariant::TwoHeadedGiant)
    );
}

// REQ-SBA-001: two standard registries are equal in shape (deterministic construction).
#[test]
fn req_sba_001_registry_standard_is_deterministic() {
    let a = SbaRegistry::standard();
    let b = SbaRegistry::standard();
    let shape = |r: &SbaRegistry| -> Vec<(&str, &str, bool, bool)> {
        r.entries()
            .iter()
            .map(|e| {
                (
                    e.id,
                    e.cr_ref,
                    e.active_in(GameVariant::Standard),
                    e.active_in(GameVariant::TwoHeadedGiant),
                )
            })
            .collect()
    };
    assert_eq!(shape(&a), shape(&b));
}

// REQ-SBA-001, CR 704.5j, 704.5k, 704.5m, 704.5n, 704.5p, 704.5q, 704.5r, 704.5s, 704.5t: the
// nine new entries follow 704.5i in order, cite "CR <id>", are active in both variants and
// carry the agreed actions.
#[test]
fn req_sba_001_registry_new_entries_actions_and_gate() {
    let r = SbaRegistry::standard();
    let ids: Vec<&str> = r.entries().iter().map(|e| e.id).collect();
    let i = ids
        .iter()
        .position(|id| *id == "704.5i")
        .expect("704.5i present");
    assert_eq!(&ids[i + 1..], &NEW_ENTRY_IDS[..]);
    for id in NEW_ENTRY_IDS {
        let e = r.get(id).unwrap_or_else(|| panic!("{id} present"));
        assert_eq!(e.cr_ref, format!("CR {id}"));
        assert!(e.active_in(GameVariant::Standard), "{id} in Standard");
        assert!(
            e.active_in(GameVariant::TwoHeadedGiant),
            "{id} in Two-Headed Giant"
        );
        let ok = match id {
            "704.5j" => matches!(e.action, SbaAction::LegendChoice),
            "704.5k" | "704.5m" => matches!(e.action, SbaAction::PutIntoGraveyard),
            "704.5n" | "704.5p" => matches!(e.action, SbaAction::Unattach),
            "704.5q" | "704.5r" => matches!(e.action, SbaAction::RemoveCounters),
            "704.5s" => matches!(e.action, SbaAction::Sacrifice),
            "704.5t" => matches!(e.action, SbaAction::RemoveFromGame),
            other => panic!("unexpected id {other}"),
        };
        assert!(ok, "{id} has the agreed action");
    }
}
