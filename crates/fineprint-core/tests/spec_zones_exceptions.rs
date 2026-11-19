// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::zones::exceptions`. Black-box: written from the work-item
//! brief only.
//!
//! The CR 400.7 exception registry: exactly twelve named entries, 400.7a to 400.7k then 400.7m
//! (there is no 400.7l), each currently reporting itself as not implemented (REQ-CORE-002).

use fineprint_base::ids::ObjectId;
use fineprint_core::zones::exceptions as ex;
use fineprint_core::zones::{EXCEPTIONS, Exception400_7, ExceptionEntry, ExceptionOutcome};

const EXPECTED_RULES: [&str; 12] = [
    "400.7a", "400.7b", "400.7c", "400.7d", "400.7e", "400.7f", "400.7g", "400.7h", "400.7i",
    "400.7j", "400.7k", "400.7m",
];

const EXPECTED_ORDER: [Exception400_7; 12] = [
    Exception400_7::A,
    Exception400_7::B,
    Exception400_7::C,
    Exception400_7::D,
    Exception400_7::E,
    Exception400_7::F,
    Exception400_7::G,
    Exception400_7::H,
    Exception400_7::I,
    Exception400_7::J,
    Exception400_7::K,
    Exception400_7::M,
];

// REQ-CORE-002, CR 400.7a-400.7k, 400.7m: ALL lists the twelve exceptions in order a..k, m.
#[test]
fn all_lists_twelve_exceptions_in_order() {
    assert_eq!(Exception400_7::ALL.len(), 12);
    assert_eq!(Exception400_7::ALL, EXPECTED_ORDER);
    for (i, a) in Exception400_7::ALL.iter().enumerate() {
        for b in Exception400_7::ALL.iter().skip(i + 1) {
            assert_ne!(a, b);
        }
    }
}

// REQ-CORE-002, CR 400.7a-400.7k, 400.7m: rule() gives exactly the CR numbers; there is no
// 400.7l.
#[test]
fn rule_strings_are_exact_and_skip_l() {
    let rules: Vec<&str> = Exception400_7::ALL.iter().map(|e| e.rule()).collect();
    assert_eq!(rules, EXPECTED_RULES);
    assert!(!rules.contains(&"400.7l"));
    for (i, a) in rules.iter().enumerate() {
        for b in rules.iter().skip(i + 1) {
            assert_ne!(a, b);
        }
    }
    for (e, r) in EXPECTED_ORDER.iter().zip(EXPECTED_RULES) {
        assert_eq!(e.rule(), r, "{e:?}");
    }
}

// REQ-CORE-002, CR 400.7: every exception has a unique, non-empty name.
#[test]
fn names_are_unique_and_non_empty() {
    let names: Vec<&str> = Exception400_7::ALL.iter().map(|e| e.name()).collect();
    for n in &names {
        assert!(!n.trim().is_empty());
    }
    for (i, a) in names.iter().enumerate() {
        for b in names.iter().skip(i + 1) {
            assert_ne!(a, b);
        }
    }
}

// REQ-CORE-002, CR 400.7a-400.7k, 400.7m: the registry has twelve entries in the order of ALL.
#[test]
fn registry_has_twelve_entries_matching_all() {
    assert_eq!(EXCEPTIONS.len(), 12);
    let order: Vec<Exception400_7> = EXCEPTIONS.iter().map(|e| e.exception).collect();
    assert_eq!(order, Exception400_7::ALL.to_vec());
    assert_eq!(order, EXPECTED_ORDER.to_vec());
    // The same registry is reachable through the exceptions module.
    let via_module: Vec<Exception400_7> = ex::EXCEPTIONS.iter().map(|e| e.exception).collect();
    assert_eq!(via_module, order);
    let _: &[ExceptionEntry; 12] = &EXCEPTIONS;
}

// REQ-CORE-002, CR 400.7a-400.7k, 400.7m: each entry's apply reports its own exception, for any
// pair of ids: Applies for CR 400.7d and 400.7j (REQ-CORE-007, #24 FP-023), NotImplemented for
// the other ten.
#[test]
fn every_entry_reports_not_implemented_for_itself() {
    let pairs = [
        (ObjectId::new(1), ObjectId::new(2)),
        (ObjectId::new(0), ObjectId::new(u64::MAX)),
        (ObjectId::new(5), ObjectId::new(5)),
    ];
    for entry in EXCEPTIONS.iter() {
        for (old, new) in pairs {
            // REQ-CORE-007: CR 400.7d and CR 400.7j are implemented (#24 FP-023).
            let expected = match entry.exception {
                Exception400_7::D | Exception400_7::J => ExceptionOutcome::Applies(entry.exception),
                other => ExceptionOutcome::NotImplemented(other),
            };
            assert_eq!(
                (entry.apply)(old, new),
                expected,
                "{}",
                entry.exception.rule()
            );
        }
    }
    assert_eq!(
        (EXCEPTIONS[3].apply)(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::Applies(Exception400_7::D)
    );
    assert_eq!(
        (EXCEPTIONS[11].apply)(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::NotImplemented(Exception400_7::M)
    );
    assert_eq!(
        (EXCEPTIONS[9].apply)(ObjectId::new(1), ObjectId::new(2)),
        ExceptionOutcome::Applies(Exception400_7::J)
    );
}
