// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `FaceDownProfile` and `FaceDownCause` (REQ-OBJ-003; CR 708.2, 708.2a,
//! 702.37a, 702.37b, 701.40a, 701.62a, 702.168a, 702.168b, 701.58a). Black-box: written from
//! the work-item brief only.

#[path = "spec_layers_facedown_support.rs"]
mod support;

use fineprint_core::layouts::{FaceDownCause, FaceDownProfile};
use fineprint_core::objects::Characteristics;
use support::*;

/// REQ-OBJ-003, CR 708.2a: the default profile, field by field: no name, no mana cost,
/// colorless, no color indicator, no supertypes, exactly the Creature card type, no subtypes,
/// no rules text, no abilities, 2/2, nothing else.
#[test]
fn req_obj_003_708_2a_default_profile_field_by_field() {
    let p = FaceDownProfile::default_2_2();
    let c = p.characteristics();
    assert!(c.names.is_empty(), "no name");
    assert_eq!(c.mana_cost, None, "no mana cost");
    assert_eq!(c.colors, ColorBitmask::NONE, "colorless");
    assert_eq!(c.color_indicator, None, "no color indicator");
    assert!(c.supertypes.is_empty(), "no supertypes");
    assert_eq!(c.card_types, set(&["Creature"]), "a creature");
    assert!(c.subtypes.is_empty(), "no subtypes");
    assert!(c.rules_text.is_empty(), "no text");
    assert!(c.abilities.is_empty(), "no abilities");
    assert_eq!(c.power.as_ref().and_then(literal_of), Some(2), "power 2");
    assert_eq!(
        c.toughness.as_ref().and_then(literal_of),
        Some(2),
        "toughness 2"
    );
    assert_eq!(c.loyalty, None);
    assert_eq!(c.defense, None);
    assert_eq!(c.hand_modifier, None);
    assert_eq!(c.life_modifier, None);
    assert_eq!(norm(c), expected_default());
    assert!(!p.has_ward_2());
}

/// REQ-OBJ-003, CR 702.168a, 702.168b, 701.58a: the ward variant is the default plus exactly
/// one ability, the ward {2} keyword with the pinned JSON fields and nothing else.
#[test]
fn req_obj_003_702_168a_ward_variant_adds_exactly_ward_2() {
    let p = FaceDownProfile::with_ward_2();
    let c = p.characteristics();
    assert_eq!(c.abilities.len(), 1, "exactly one ability");
    assert_eq!(ability_fields(&c.abilities[0]), expected_ward_fields());
    assert_eq!(
        keyword_of(&c.abilities[0]),
        Some(("facedown:ward".to_string(), "ward".to_string()))
    );
    assert_eq!(
        keywords(c),
        vec![("facedown:ward".to_string(), "ward".to_string())]
    );
    // Every other field is the default's.
    let without = Characteristics {
        abilities: Vec::new(),
        ..norm(c)
    };
    assert_eq!(without, expected_default());
    assert!(p.has_ward_2());
    assert_ne!(p, FaceDownProfile::default_2_2());
}

/// REQ-OBJ-003, CR 702.37a, 702.37b, 701.40a, 701.62a, 702.168a, 701.58a: the cause table:
/// morph, megamorph, manifest and manifest dread give the default; disguise and cloak give the
/// ward variant.
#[test]
fn req_obj_003_for_cause_table_all_six_causes() {
    let table = [
        (FaceDownCause::Morph, false),
        (FaceDownCause::Megamorph, false),
        (FaceDownCause::Manifest, false),
        (FaceDownCause::ManifestDread, false),
        (FaceDownCause::Disguise, true),
        (FaceDownCause::Cloak, true),
    ];
    for (cause, ward) in table {
        let p = FaceDownProfile::for_cause(cause);
        let want = if ward {
            FaceDownProfile::with_ward_2()
        } else {
            FaceDownProfile::default_2_2()
        };
        assert_eq!(p, want, "{cause:?}");
        assert_eq!(p.has_ward_2(), ward, "{cause:?}");
        let expected = if ward {
            expected_ward()
        } else {
            expected_default()
        };
        assert_eq!(norm(p.characteristics()), expected, "{cause:?}");
    }
    // The cause enum is Copy + Eq.
    let a = FaceDownCause::Cloak;
    let b = a;
    assert_eq!(a, b);
    assert_ne!(FaceDownCause::Morph, FaceDownCause::Megamorph);
}

/// REQ-OBJ-003, CR 708.2: a card effect's own list of characteristics (here a named 2/2
/// artifact creature with a subtype) is stored and returned exactly as given.
#[test]
fn req_obj_003_708_2_custom_profile_is_kept_as_given() {
    let listed = Characteristics {
        card_types: set(&["Artifact", "Creature"]),
        subtypes: set(&["Golem"]),
        ..creature("test.fd_custom_listed", 2, 2)
    };
    let p = FaceDownProfile::custom(listed.clone());
    assert_eq!(p.characteristics(), &listed);
    assert!(!p.has_ward_2());
    assert_eq!(p.clone(), p);
    assert_ne!(p, FaceDownProfile::default_2_2());

    // Any other value is stored verbatim too, including non-literal power.
    let odd = Characteristics {
        power: Some(non_literal_x()),
        ..secret_face_up()
    };
    assert_eq!(FaceDownProfile::custom(odd.clone()).characteristics(), &odd);
}

/// REQ-OBJ-003: profiles are deterministic values: building one twice gives equal values and
/// equal Debug text (REQ-CORE-006).
#[test]
fn req_obj_003_profiles_are_deterministic_values() {
    assert_eq!(
        FaceDownProfile::default_2_2(),
        FaceDownProfile::default_2_2()
    );
    assert_eq!(
        FaceDownProfile::with_ward_2(),
        FaceDownProfile::with_ward_2()
    );
    assert_eq!(
        format!("{:?}", FaceDownProfile::with_ward_2()),
        format!("{:?}", FaceDownProfile::with_ward_2())
    );
}
