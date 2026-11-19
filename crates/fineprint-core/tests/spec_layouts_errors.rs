// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the error cases of `fineprint_core::layouts` (REQ-OBJ-002). Black-box: written
//! from the work-item brief only.
//!
//! Face sets per layout (`BadFaces`), layout mismatch, unsupported layouts (ROOM, MELD and every
//! other layout outside the twelve), type lines the vocabulary rejects (CR 205.2a, 205.3c,
//! 205.4a) and the error precedence LayoutMismatch, BadFaces, InvalidCast, Object.

#[path = "spec_layouts_support.rs"]
mod support;

use fineprint_core::layouts::{LayoutError, base_characteristics, gating, layout_for, mana_value};
use fineprint_core::objects::{ManaValueContext, ObjectError};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{CardFaceDSL, FaceKind, LayoutBehavior};
use support::{
    Cast, IN_SCOPE, St, blank, def, query, required_kinds, sample, split_left, split_right, strings,
};

fn hand() -> fineprint_core::layouts::LayoutQuery {
    query(ZoneKind::Hand, Cast::None, St::DEFAULT)
}

fn faces_of(kinds: &[FaceKind]) -> Vec<CardFaceDSL> {
    kinds
        .iter()
        .enumerate()
        .map(|(i, k)| blank(*k, &format!("test.face_{i}")))
        .collect()
}

/// Asserts every entry point reports `BadFaces { layout }` for `d`.
fn assert_bad_faces(d: &fineprint_dsl::card::CardDefinitionDSL, what: &str) {
    let v = support::vocab();
    let layout = d.layout;
    let err = LayoutError::BadFaces { layout };
    let obj = layout_for(layout).expect("supported");
    for zone in ZoneKind::ALL {
        for cast in Cast::ALL {
            let q = query(zone, cast, St::DEFAULT);
            assert_eq!(
                base_characteristics(d, &q, v),
                Err(err.clone()),
                "{what} {zone:?} {cast:?}"
            );
            assert_eq!(
                obj.base_characteristics(d, &q, v),
                Err(err.clone()),
                "{what}"
            );
            assert_eq!(
                mana_value(d, &q, ManaValueContext::OffStack, v),
                Err(err.clone()),
                "{what}"
            );
        }
    }
    assert_eq!(gating(d), Err(err.clone()), "{what}");
    assert_eq!(obj.gating(d), Err(err), "{what}");
}

/// REQ-OBJ-002, CR 709.4, 710.2, 712.6a, 716.4, 719.4, 721.4: an empty face list is `BadFaces`
/// for every in-scope layout.
#[test]
fn errors_bad_faces_empty() {
    for layout in IN_SCOPE {
        assert_bad_faces(&def(layout, Vec::new()), &format!("{layout:?} empty"));
    }
}

/// REQ-OBJ-002, CR 709.4, 710.2, 712.6a, 716.4, 719.4, 721.4: a missing face (one required kind
/// dropped) is `BadFaces`.
#[test]
fn errors_bad_faces_missing_face() {
    for layout in IN_SCOPE {
        let kinds = required_kinds(layout);
        for drop in 0..kinds.len() {
            let mut k = kinds.clone();
            k.remove(drop);
            assert_bad_faces(
                &def(layout, faces_of(&k)),
                &format!("{layout:?} without {:?}", kinds[drop]),
            );
        }
    }
}

/// REQ-OBJ-002, CR 709.4, 710.2, 712.6a, 716.4, 719.4, 721.4: an extra face of another kind is `BadFaces`, for every extra kind.
#[test]
fn errors_bad_faces_extra_kind() {
    for layout in IN_SCOPE {
        let kinds = required_kinds(layout);
        for extra in FaceKind::ALL.into_iter().filter(|k| !kinds.contains(k)) {
            let mut k = kinds.clone();
            k.push(extra);
            assert_bad_faces(
                &def(layout, faces_of(&k)),
                &format!("{layout:?} plus {extra:?}"),
            );
        }
    }
}

/// REQ-OBJ-002, CR 709.4, 710.2, 712.6a, 716.4, 719.4, 721.4: a required kind given twice is `BadFaces`, even with the full set present.
#[test]
fn errors_bad_faces_duplicate_kind() {
    for layout in IN_SCOPE {
        let kinds = required_kinds(layout);
        for dup in kinds.iter().copied() {
            let mut k = kinds.clone();
            k.push(dup);
            assert_bad_faces(
                &def(layout, faces_of(&k)),
                &format!("{layout:?} duplicate {dup:?}"),
            );
        }
        // Replace one required kind by a copy of another (same size, wrong set).
        if kinds.len() == 2 {
            assert_bad_faces(
                &def(layout, faces_of(&[kinds[0], kinds[0]])),
                &format!("{layout:?} twice {:?}", kinds[0]),
            );
            assert_bad_faces(
                &def(layout, faces_of(&[kinds[1], kinds[1]])),
                &format!("{layout:?} twice {:?}", kinds[1]),
            );
        }
    }
}

/// REQ-OBJ-002, CR 709.4, 710.2, 712.6a, 716.4, 719.4, 721.4: faces of the wrong kinds (another layout's set, same size) are `BadFaces`.
#[test]
fn errors_bad_faces_wrong_kinds() {
    for layout in IN_SCOPE {
        let kinds = required_kinds(layout);
        for other in IN_SCOPE {
            let other_kinds = required_kinds(other);
            let mut a = kinds.clone();
            let mut b = other_kinds.clone();
            a.sort();
            b.sort();
            if a == b {
                continue;
            }
            assert_bad_faces(
                &def(layout, faces_of(&other_kinds)),
                &format!("{layout:?} with faces of {other:?}"),
            );
        }
        // A face of a kind no in-scope layout uses.
        let mut k = kinds.clone();
        k[0] = FaceKind::MeldResult;
        assert_bad_faces(
            &def(layout, faces_of(&k)),
            &format!("{layout:?} meld result"),
        );
        let mut k = kinds;
        k[0] = FaceKind::PrepareSpell;
        assert_bad_faces(&def(layout, faces_of(&k)), &format!("{layout:?} prepare"));
    }
}

/// REQ-OBJ-002, CR 709.4, 712.6a, 716.4: every exact face set, in any order, is accepted (faces looked up by kind).
#[test]
fn errors_exact_face_sets_any_order_accepted() {
    let v = support::vocab();
    for layout in IN_SCOPE {
        let kinds = required_kinds(layout);
        let mut reversed = kinds.clone();
        reversed.reverse();
        for k in [kinds, reversed] {
            let d = def(layout, faces_of(&k));
            assert!(
                base_characteristics(&d, &hand(), v).is_ok(),
                "{layout:?} {k:?}"
            );
            assert!(gating(&d).is_ok(), "{layout:?} {k:?}");
        }
        // The sample definitions with faces in reverse order give the sample's results.
        let fwd = sample(layout);
        let mut rev = fwd.clone();
        rev.faces.reverse();
        for zone in ZoneKind::ALL {
            for cast in Cast::ALL {
                let q = query(zone, cast, St::DEFAULT);
                assert_eq!(
                    base_characteristics(&rev, &q, v),
                    base_characteristics(&fwd, &q, v),
                    "{layout:?} {zone:?} {cast:?}"
                );
            }
        }
        assert_eq!(gating(&rev), gating(&fwd));
    }
}

/// REQ-OBJ-002, SRS §4.2, CR 709.4 (ROOM is out of scope): `layout_for` refuses every layout outside the twelve (ROOM and MELD
/// named explicitly), and the free functions refuse a definition with such a layout.
#[test]
fn errors_unsupported_layouts() {
    let v = support::vocab();
    for named in [LayoutBehavior::Room, LayoutBehavior::Meld] {
        assert_eq!(
            layout_for(named).err(),
            Some(LayoutError::UnsupportedLayout(named))
        );
    }
    let others: Vec<LayoutBehavior> = LayoutBehavior::ALL
        .into_iter()
        .filter(|b| !IN_SCOPE.contains(b))
        .collect();
    assert_eq!(others.len(), LayoutBehavior::ALL.len() - 12);
    assert!(others.contains(&LayoutBehavior::Room));
    assert!(others.contains(&LayoutBehavior::Meld));
    for b in others {
        let err = LayoutError::UnsupportedLayout(b);
        assert_eq!(layout_for(b).err(), Some(err.clone()), "{b:?}");
        let d = def(b, vec![blank(FaceKind::Main, "test.unsupported")]);
        assert_eq!(
            base_characteristics(&d, &hand(), v),
            Err(err.clone()),
            "{b:?}"
        );
        assert_eq!(
            mana_value(&d, &hand(), ManaValueContext::OffStack, v),
            Err(err.clone())
        );
        assert_eq!(gating(&d), Err(err), "{b:?}");
    }
    for b in IN_SCOPE {
        assert_eq!(layout_for(b).expect("in scope").behavior(), b);
    }
}

/// REQ-OBJ-002, CR 709.4, 710.2, 712.6a: a layout object given a definition of another layout is `LayoutMismatch`
/// for characteristics, mana value and gating.
#[test]
fn errors_layout_mismatch() {
    let v = support::vocab();
    for expected in IN_SCOPE {
        let obj = layout_for(expected).expect("in scope");
        for found in LayoutBehavior::ALL.into_iter().filter(|f| *f != expected) {
            let mut d = sample(expected);
            d.layout = found;
            let err = LayoutError::LayoutMismatch { expected, found };
            for zone in ZoneKind::ALL {
                let q = query(zone, Cast::None, St::DEFAULT);
                assert_eq!(obj.base_characteristics(&d, &q, v), Err(err.clone()));
                assert_eq!(
                    obj.mana_value(&d, &q, ManaValueContext::OffStack, v),
                    Err(err.clone())
                );
            }
            assert_eq!(obj.gating(&d), Err(err), "{expected:?} {found:?}");
        }
    }
}

/// REQ-OBJ-002, CR 709.3b, 205.2a: error precedence LayoutMismatch before BadFaces before InvalidCast before
/// Object.
#[test]
fn errors_precedence() {
    let v = support::vocab();
    let bad_type = |kind, n: &str| CardFaceDSL {
        card_types: strings(&["NotACardType"]),
        ..blank(kind, n)
    };
    // Mismatch beats BadFaces: a Split object given a NORMAL definition with no faces.
    let d = def(LayoutBehavior::Normal, Vec::new());
    let obj = layout_for(LayoutBehavior::Split).expect("split");
    assert_eq!(
        obj.base_characteristics(&d, &hand(), v),
        Err(LayoutError::LayoutMismatch {
            expected: LayoutBehavior::Split,
            found: LayoutBehavior::Normal,
        })
    );
    // BadFaces beats InvalidCast: a SPLIT definition with one half, on the stack with cast None.
    let d = def(
        LayoutBehavior::Split,
        vec![bad_type(FaceKind::LeftHalf, "test.lonely_half")],
    );
    let stack_none = query(ZoneKind::Stack, Cast::None, St::DEFAULT);
    assert_eq!(
        base_characteristics(&d, &stack_none, v),
        Err(LayoutError::BadFaces {
            layout: LayoutBehavior::Split
        })
    );
    // InvalidCast beats Object: valid faces but a bad type line, invalid cast.
    let d = def(
        LayoutBehavior::Split,
        vec![
            bad_type(FaceKind::LeftHalf, "test.bad_left"),
            bad_type(FaceKind::RightHalf, "test.bad_right"),
        ],
    );
    assert_eq!(
        base_characteristics(&d, &stack_none, v),
        Err(LayoutError::InvalidCast {
            layout: LayoutBehavior::Split,
            zone: ZoneKind::Stack,
        })
    );
    // And with a valid cast the Object error shows.
    let left = query(ZoneKind::Stack, Cast::Left, St::DEFAULT);
    assert_eq!(
        base_characteristics(&d, &left, v),
        Err(LayoutError::Object(ObjectError::UnknownCardType(
            "NotACardType".to_owned()
        )))
    );
}

/// A named way to break a face's type line, and the error it causes.
type Breaker = (&'static str, fn(&mut CardFaceDSL), ObjectError);

/// REQ-OBJ-002, CR 205.2a, 205.3c, 205.4a: a selected face whose type line the vocabulary
/// rejects is `Object(..)`, for every in-scope layout and for each kind of type-line error.
#[test]
fn errors_object_bad_type_line() {
    let v = support::vocab();
    let breakers: [Breaker; 3] = [
        (
            "card type",
            |f| f.card_types = strings(&["NotACardType"]),
            ObjectError::UnknownCardType("NotACardType".to_owned()),
        ),
        (
            "supertype",
            |f| f.supertypes = strings(&["NotASupertype"]),
            ObjectError::UnknownSupertype("NotASupertype".to_owned()),
        ),
        (
            "subtype",
            |f| {
                f.card_types = strings(&["Instant"]);
                f.subtypes = strings(&["Saga"]);
            },
            ObjectError::SubtypeNotCorrelated {
                subtype: "Saga".to_owned(),
            },
        ),
    ];
    for layout in IN_SCOPE {
        for (what, breaker, obj_err) in &breakers {
            let mut d = sample(layout);
            // Break the face every zone but the stack and battlefield selects: Main, or the
            // left half for SPLIT (combined there).
            let target = if layout == LayoutBehavior::Split {
                FaceKind::LeftHalf
            } else {
                FaceKind::Main
            };
            for f in d.faces.iter_mut().filter(|f| f.face_kind == target) {
                breaker(f);
            }
            let err = LayoutError::Object(obj_err.clone());
            assert_eq!(
                base_characteristics(&d, &hand(), v),
                Err(err.clone()),
                "{layout:?} {what}"
            );
            assert_eq!(
                layout_for(layout)
                    .expect("in scope")
                    .base_characteristics(&d, &hand(), v),
                Err(err),
                "{layout:?} {what}"
            );
        }
    }
    // The right half of a split card on its own: Object when cast, and in the combined view.
    let mut right = split_right();
    right.card_types = strings(&["NotACardType"]);
    right.subtypes = Vec::new();
    let d = def(LayoutBehavior::Split, vec![split_left(), right]);
    let err = Err(LayoutError::Object(ObjectError::UnknownCardType(
        "NotACardType".to_owned(),
    )));
    assert_eq!(
        base_characteristics(&d, &query(ZoneKind::Stack, Cast::Right, St::DEFAULT), v),
        err.clone()
    );
    assert_eq!(base_characteristics(&d, &hand(), v), err);
}
