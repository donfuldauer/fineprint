// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `fineprint_core::objects` type validation. Black-box: written from the
//! work-item brief only.
//!
//! Card types, supertypes and subtypes against the bundled `RulesVocabulary` (CR 205.2a,
//! 205.3c, 205.3d, 205.3k, 205.3m, 205.3r, 205.4a, 205.4b) (REQ-OBJ-001). Subtype names are
//! taken from the vocabulary, never hardcoded.

use fineprint_core::objects::{ObjectError, can_gain_subtype, validate_types};
use fineprint_dsl::vocab::RulesVocabulary;

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn vocab() -> &'static RulesVocabulary {
    RulesVocabulary::bundled().expect("bundled vocabulary loads")
}

/// A subtype of `of` that is not a subtype of `not_of`.
fn subtype_of_but_not(v: &RulesVocabulary, of: &str, not_of: &str) -> String {
    v.subtypes_of(of)
        .find(|s| !v.is_subtype_of(not_of, s))
        .unwrap_or_else(|| panic!("vocabulary has a {of} subtype that is not a {not_of} subtype"))
        .to_string()
}

const BAD_CARD_TYPE: &str = "Notacardtype";
const BAD_SUPERTYPE: &str = "Notasupertype";
const BAD_SUBTYPE: &str = "Notasubtype";

// ---------------------------------------------------------------------------------------------
// Case 13: validation
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-001, CR 205.2a: an unknown card type is rejected, carrying the offending string.
#[test]
fn types_205_2a_unknown_card_type_rejected() {
    let v = vocab();
    assert!(!v.is_card_type(BAD_CARD_TYPE));
    assert_eq!(
        validate_types(&[], &[BAD_CARD_TYPE], &[], v),
        Err(ObjectError::UnknownCardType(BAD_CARD_TYPE.to_string()))
    );
    assert_eq!(
        validate_types(&[], &["Creature", BAD_CARD_TYPE], &[], v),
        Err(ObjectError::UnknownCardType(BAD_CARD_TYPE.to_string()))
    );
}

/// REQ-OBJ-001, CR 205.4a: an unknown supertype is rejected, carrying the offending string.
#[test]
fn types_205_4a_unknown_supertype_rejected() {
    let v = vocab();
    assert!(!v.is_supertype(BAD_SUPERTYPE));
    assert_eq!(
        validate_types(&[BAD_SUPERTYPE], &["Creature"], &[], v),
        Err(ObjectError::UnknownSupertype(BAD_SUPERTYPE.to_string()))
    );
    assert_eq!(
        validate_types(&["Legendary", BAD_SUPERTYPE], &["Creature"], &[], v),
        Err(ObjectError::UnknownSupertype(BAD_SUPERTYPE.to_string()))
    );
}

/// REQ-OBJ-001, CR 205.3c, CR 205.3d: a land subtype on a creature-only object is not
/// correlated with any of its card types.
#[test]
fn types_205_3d_land_subtype_on_creature_rejected() {
    let v = vocab();
    let s = subtype_of_but_not(v, "Land", "Creature");
    assert_eq!(
        validate_types(&[], &["Creature"], &[s.as_str()], v),
        Err(ObjectError::SubtypeNotCorrelated { subtype: s.clone() })
    );
}

/// REQ-OBJ-001, CR 205.3c: the same land subtype on a land is accepted.
#[test]
fn types_205_3c_land_subtype_on_land_ok() {
    let v = vocab();
    let s = subtype_of_but_not(v, "Land", "Creature");
    assert_eq!(validate_types(&[], &["Land"], &[s.as_str()], v), Ok(()));
    for s in v.subtypes_of("Land") {
        assert_eq!(validate_types(&[], &["Land"], &[s], v), Ok(()), "{s}");
    }
}

/// REQ-OBJ-001, CR 205.3c: with several card types, each subtype is correlated to its own type.
#[test]
fn types_205_3c_land_creature_with_one_subtype_each_ok() {
    let v = vocab();
    let land = subtype_of_but_not(v, "Land", "Creature");
    let creature = subtype_of_but_not(v, "Creature", "Land");
    assert_eq!(
        validate_types(
            &[],
            &["Land", "Creature"],
            &[land.as_str(), creature.as_str()],
            v
        ),
        Ok(())
    );
    assert_eq!(
        validate_types(
            &[],
            &["Creature", "Land"],
            &[creature.as_str(), land.as_str()],
            v
        ),
        Ok(())
    );
}

/// REQ-OBJ-001, CR 205.3k: instants and sorceries share their subtype lists; validation agrees
/// with the vocabulary for every sorcery subtype on an instant.
#[test]
fn types_205_3k_instant_with_sorcery_subtype_follows_vocabulary() {
    let v = vocab();
    let sorcery: Vec<&str> = v.subtypes_of("Sorcery").collect();
    assert!(!sorcery.is_empty(), "vocabulary lists sorcery subtypes");
    let mut shared = 0usize;
    for s in sorcery {
        let expected = if v.is_subtype_of("Instant", s) {
            shared += 1;
            Ok(())
        } else {
            Err(ObjectError::SubtypeNotCorrelated {
                subtype: s.to_string(),
            })
        };
        assert_eq!(validate_types(&[], &["Instant"], &[s], v), expected, "{s}");
    }
    assert!(shared > 0, "CR 205.3k: at least one shared spell subtype");
}

/// REQ-OBJ-001, CR 205.3m: creatures and kindreds share their subtype lists; a creature subtype
/// on a Kindred object is accepted exactly when the vocabulary lists it for Kindred.
#[test]
fn types_205_3m_kindred_with_creature_subtype() {
    let v = vocab();
    let mut shared = 0usize;
    for s in v.subtypes_of("Creature") {
        if v.is_subtype_of("Kindred", s) {
            shared += 1;
            assert_eq!(validate_types(&[], &["Kindred"], &[s], v), Ok(()), "{s}");
            assert_eq!(
                validate_types(&[], &["Kindred", "Instant"], &[s], v),
                Ok(()),
                "{s}"
            );
        } else {
            assert_eq!(
                validate_types(&[], &["Kindred"], &[s], v),
                Err(ObjectError::SubtypeNotCorrelated {
                    subtype: s.to_string()
                }),
                "{s}"
            );
        }
    }
    assert!(shared > 0, "CR 205.3m: Kindred shares the creature types");
}

/// REQ-OBJ-001, CR 205.3r: phenomenon, scheme, vanguard and conspiracy objects have no
/// subtypes, so any subtype is not correlated.
#[test]
fn types_205_3r_no_subtype_card_types_reject_any_subtype() {
    let v = vocab();
    let candidates: Vec<String> = ["Creature", "Land", "Artifact", "Enchantment", "Sorcery"]
        .iter()
        .filter_map(|t| v.subtypes_of(t).next().map(str::to_string))
        .collect();
    assert!(!candidates.is_empty());
    for t in ["Phenomenon", "Scheme", "Vanguard", "Conspiracy"] {
        assert!(v.is_card_type(t), "{t} is a card type (CR 205.2a)");
        assert_eq!(v.subtypes_of(t).count(), 0, "{t} has no subtypes");
        assert_eq!(validate_types(&[], &[t], &[], v), Ok(()), "{t}");
        for s in &candidates {
            assert_eq!(
                validate_types(&[], &[t], &[s.as_str()], v),
                Err(ObjectError::SubtypeNotCorrelated { subtype: s.clone() }),
                "{t} {s}"
            );
        }
        assert_eq!(
            validate_types(&[], &[t], &[BAD_SUBTYPE], v),
            Err(ObjectError::SubtypeNotCorrelated {
                subtype: BAD_SUBTYPE.to_string()
            })
        );
    }
}

/// REQ-OBJ-001, CR 205.3d: a subtype with no card type at all is not correlated.
#[test]
fn types_205_3d_subtype_without_card_type_rejected() {
    let v = vocab();
    let s = v
        .subtypes_of("Creature")
        .next()
        .expect("creature subtypes exist")
        .to_string();
    assert_eq!(
        validate_types(&[], &[], &[s.as_str()], v),
        Err(ObjectError::SubtypeNotCorrelated { subtype: s.clone() })
    );
    assert_eq!(
        validate_types(&["Legendary"], &[], &[s.as_str()], v),
        Err(ObjectError::SubtypeNotCorrelated { subtype: s })
    );
}

/// REQ-OBJ-001, CR 205.4b: any supertype goes with any card type.
#[test]
fn types_205_4b_every_supertype_with_every_card_type_ok() {
    let v = vocab();
    assert!(!v.supertypes.is_empty());
    assert!(!v.card_types.is_empty());
    for st in &v.supertypes {
        for ct in &v.card_types {
            assert_eq!(
                validate_types(&[st.as_str()], &[ct.as_str()], &[], v),
                Ok(()),
                "{st} {ct}"
            );
        }
    }
    // All supertypes and all card types at once.
    let sts: Vec<&str> = v.supertypes.iter().map(String::as_str).collect();
    let cts: Vec<&str> = v.card_types.iter().map(String::as_str).collect();
    assert_eq!(validate_types(&sts, &cts, &[], v), Ok(()));
}

/// REQ-OBJ-001, CR 205.2a, CR 205.4a: matching is exact and case-sensitive.
#[test]
fn types_205_case_sensitive_matching() {
    let v = vocab();
    assert_eq!(
        validate_types(&[], &["land"], &[], v),
        Err(ObjectError::UnknownCardType("land".to_string()))
    );
    assert_eq!(
        validate_types(&["legendary"], &["Land"], &[], v),
        Err(ObjectError::UnknownSupertype("legendary".to_string()))
    );
    let s = v
        .subtypes_of("Land")
        .next()
        .expect("land subtypes exist")
        .to_string();
    let changed = if s.to_lowercase() == s {
        s.to_uppercase()
    } else {
        s.to_lowercase()
    };
    assert_ne!(changed, s);
    assert_eq!(
        validate_types(&[], &["Land"], &[changed.as_str()], v),
        Err(ObjectError::SubtypeNotCorrelated { subtype: changed })
    );
}

/// REQ-OBJ-001, CR 205: card types are checked before supertypes.
#[test]
fn types_205_order_card_type_before_supertype() {
    let v = vocab();
    assert_eq!(
        validate_types(&[BAD_SUPERTYPE], &[BAD_CARD_TYPE], &[], v),
        Err(ObjectError::UnknownCardType(BAD_CARD_TYPE.to_string()))
    );
    assert_eq!(
        validate_types(&[BAD_SUPERTYPE], &[BAD_CARD_TYPE], &[BAD_SUBTYPE], v),
        Err(ObjectError::UnknownCardType(BAD_CARD_TYPE.to_string()))
    );
    assert_eq!(
        validate_types(&[], &[BAD_CARD_TYPE], &[BAD_SUBTYPE], v),
        Err(ObjectError::UnknownCardType(BAD_CARD_TYPE.to_string()))
    );
}

/// REQ-OBJ-001, CR 205: supertypes are checked before subtypes.
#[test]
fn types_205_order_supertype_before_subtype() {
    let v = vocab();
    assert_eq!(
        validate_types(&[BAD_SUPERTYPE], &["Creature"], &[BAD_SUBTYPE], v),
        Err(ObjectError::UnknownSupertype(BAD_SUPERTYPE.to_string()))
    );
}

/// REQ-OBJ-001, CR 205: empty inputs are valid.
#[test]
fn types_205_empty_inputs_ok() {
    assert_eq!(validate_types(&[], &[], &[], vocab()), Ok(()));
}

/// REQ-OBJ-001, CR 205.3d: `ObjectError` implements `Display` and `Error`.
#[test]
fn types_205_3d_object_error_is_displayable_error() {
    let errors = [
        ObjectError::UnknownCardType(BAD_CARD_TYPE.to_string()),
        ObjectError::UnknownSupertype(BAD_SUPERTYPE.to_string()),
        ObjectError::SubtypeNotCorrelated {
            subtype: BAD_SUBTYPE.to_string(),
        },
    ];
    for e in &errors {
        let as_error: &dyn std::error::Error = e;
        assert!(!as_error.to_string().is_empty());
    }
}

/// REQ-OBJ-001, CR 205.3d: `can_gain_subtype` mirrors correlation: true exactly when the
/// subtype belongs to at least one listed card type.
#[test]
fn types_205_3d_can_gain_subtype_cases() {
    let v = vocab();
    let land = subtype_of_but_not(v, "Land", "Creature");
    let creature = subtype_of_but_not(v, "Creature", "Land");
    assert!(can_gain_subtype(&["Land"], &land, v));
    assert!(!can_gain_subtype(&["Creature"], &land, v));
    assert!(can_gain_subtype(&["Creature", "Land"], &land, v));
    assert!(can_gain_subtype(&["Creature", "Land"], &creature, v));
    assert!(!can_gain_subtype(&[], &land, v));
    assert!(!can_gain_subtype(&[], &creature, v));
    assert!(!can_gain_subtype(&["Land"], BAD_SUBTYPE, v));
    for t in ["Phenomenon", "Scheme", "Vanguard", "Conspiracy"] {
        assert!(!can_gain_subtype(&[t], &land, v), "{t}");
        assert!(!can_gain_subtype(&[t], &creature, v), "{t}");
    }
    for s in v.subtypes_of("Sorcery") {
        assert_eq!(
            can_gain_subtype(&["Instant"], s, v),
            v.is_subtype_of("Instant", s),
            "{s}"
        );
    }
    for s in v.subtypes_of("Creature") {
        assert_eq!(
            can_gain_subtype(&["Kindred"], s, v),
            v.is_subtype_of("Kindred", s),
            "{s}"
        );
    }
}

/// REQ-OBJ-001, CR 205.3c, CR 205.3d: `can_gain_subtype` and `validate_types` agree for every
/// (card type, subtype) pair the vocabulary knows.
#[test]
fn types_205_3d_can_gain_agrees_with_validate() {
    let v = vocab();
    let mut subtypes: Vec<&str> = Vec::new();
    for ct in &v.card_types {
        for s in v.subtypes_of(ct) {
            if !subtypes.contains(&s) {
                subtypes.push(s);
            }
        }
    }
    for ct in &v.card_types {
        for s in &subtypes {
            let gain = can_gain_subtype(&[ct.as_str()], s, v);
            assert_eq!(gain, v.is_subtype_of(ct, s), "{ct} {s}");
            assert_eq!(
                validate_types(&[], &[ct.as_str()], &[s], v).is_ok(),
                gain,
                "{ct} {s}"
            );
        }
    }
}
