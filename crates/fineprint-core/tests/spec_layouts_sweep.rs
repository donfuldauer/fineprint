// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Exhaustive and property tests for `fineprint_core::layouts` (REQ-OBJ-002). Black-box:
//! written from the work-item brief only.
//!
//! Every in-scope layout x every `ZoneKind` x every cast choice x every status x both mana value
//! contexts is compared with the brief's selection tables (encoded in the support oracle:
//! CR 202.3a-d, 709.3b, 709.4, 710.2, 711.6, 712.6a-e, 716.3b, 716.4, 719.3b, 719.4, 721.3b,
//! 721.4); results are deterministic and nothing panics; face sets are exact.

#[path = "spec_layouts_support.rs"]
mod support;

use fineprint_core::layouts::{
    Layout, LayoutError, LayoutQuery, LayoutStatus, base_characteristics, gating, layout_for,
    mana_value,
};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{FaceKind, LayoutBehavior};
use support::{Cast, IN_SCOPE, St, blank, contexts, def, oracle, query, required_kinds, sample};

/// REQ-OBJ-002, CR 202.3a-d, 709.3b, 709.4, 710.2, 711.6, 712.6a-e, 716.3b, 716.4, 719.3b,
/// 719.4, 721.3b, 721.4: the full table: for each in-scope layout, zone, cast choice, status and
/// context, the free functions and both layout objects give exactly the oracle's result.
#[test]
fn sweep_every_layout_zone_cast_status_matches_tables() {
    let v = support::vocab();
    let mut checked = 0u32;
    for layout in IN_SCOPE {
        let d = sample(layout);
        let obj = layout_for(layout).expect("in scope");
        for zone in ZoneKind::ALL {
            for cast in Cast::ALL {
                for st in St::all() {
                    let q = query(zone, cast, st);
                    let chars = base_characteristics(&d, &q, v);
                    assert_eq!(obj.base_characteristics(&d, &q, v), chars);
                    for ctx in contexts() {
                        let what = format!("{layout:?} {zone:?} {cast:?} {st:?} {ctx:?}");
                        let want = oracle(&d, zone, cast, st, ctx);
                        match &want {
                            Ok((c, mv)) => {
                                let got = chars.clone().expect(&what);
                                support::assert_chars(&got, c, &what);
                                assert_eq!(mana_value(&d, &q, ctx, v), Ok(*mv), "{what}");
                            }
                            Err(e) => {
                                assert_eq!(chars, Err(e.clone()), "{what}");
                                assert_eq!(mana_value(&d, &q, ctx, v), Err(e.clone()), "{what}");
                            }
                        }
                        assert_eq!(obj.mana_value(&d, &q, ctx, v), mana_value(&d, &q, ctx, v));
                        checked += 1;
                    }
                }
            }
        }
    }
    // 12 layouts x 7 zones x 10 casts x 8 statuses x 2 contexts.
    assert_eq!(checked, 12 * 7 * 10 * 8 * 2);
}

/// REQ-OBJ-002, CR 709.3b, 712.8b, 716.4, 719.4, 721.4: with cast None and the default status,
/// every in-scope layout gives characteristics in every zone, except SPLIT and DFC_MODAL on the
/// stack (InvalidCast); ADVENTURER, OMEN and PROTOTYPE accept None on the stack.
#[test]
fn sweep_cast_none_default_status_ok_except_split_and_modal_on_stack() {
    let v = support::vocab();
    for layout in IN_SCOPE {
        let d = sample(layout);
        for zone in ZoneKind::ALL {
            let q = LayoutQuery::new(zone);
            let got = base_characteristics(&d, &q, v);
            let must_fail = zone == ZoneKind::Stack
                && (layout == LayoutBehavior::Split || layout == LayoutBehavior::DfcModal);
            if must_fail {
                assert_eq!(
                    got,
                    Err(LayoutError::InvalidCast { layout, zone }),
                    "{layout:?} {zone:?}"
                );
            } else {
                assert!(got.is_ok(), "{layout:?} {zone:?}: {got:?}");
            }
            // `new` matches an explicit cast None with the default status.
            let explicit = query(zone, Cast::None, St::DEFAULT);
            assert_eq!(base_characteristics(&d, &explicit, v), got);
        }
    }
}

/// REQ-OBJ-002, CR 710.2, 712.11: `LayoutStatus::default()` is unflipped, front face up, not a copy of a back
/// face, and `LayoutQuery::new` uses it.
#[test]
fn sweep_layout_status_default() {
    let s = LayoutStatus::default();
    assert!(!s.flipped);
    assert!(matches!(s.face_up, fineprint_core::layouts::FaceUp::Front));
    assert!(!s.copying_back_face);
    let q = LayoutQuery::new(ZoneKind::Battlefield);
    assert_eq!(q.zone, ZoneKind::Battlefield);
    assert!(!q.status.flipped);
    assert!(matches!(
        q.status.face_up,
        fineprint_core::layouts::FaceUp::Front
    ));
    assert!(!q.status.copying_back_face);
    assert!(matches!(q.cast, fineprint_core::layouts::CastChoice::None));
}

/// REQ-OBJ-002, CR 202.3, 709.4, 712.6: determinism: repeated calls with equal inputs give equal
/// results (characteristics, mana value, gating), including on cloned definitions.
#[test]
fn sweep_determinism() {
    let v = support::vocab();
    for layout in IN_SCOPE {
        let d = sample(layout);
        let d2 = d.clone();
        for zone in ZoneKind::ALL {
            for cast in Cast::ALL {
                for st in St::all() {
                    let a = base_characteristics(&d, &query(zone, cast, st), v);
                    for _ in 0..3 {
                        assert_eq!(base_characteristics(&d2, &query(zone, cast, st), v), a);
                    }
                    for ctx in contexts() {
                        let m = mana_value(&d, &query(zone, cast, st), ctx, v);
                        assert_eq!(mana_value(&d2, &query(zone, cast, st), ctx, v), m);
                    }
                }
            }
        }
        assert_eq!(gating(&d), gating(&d2));
    }
}

/// REQ-OBJ-002, CR 709.4, 712.6a: `layout_for(b).behavior() == b` for the twelve layouts.
#[test]
fn sweep_layout_for_behavior_roundtrip() {
    for b in IN_SCOPE {
        let obj: &'static dyn Layout = layout_for(b).expect("in scope");
        assert_eq!(obj.behavior(), b);
    }
}

/// All multisets of face kinds of size 0 to `max`, as sorted vectors.
fn multisets(max: usize) -> Vec<Vec<FaceKind>> {
    let mut out = vec![Vec::new()];
    let mut frontier = vec![Vec::new()];
    for _ in 0..max {
        let mut next = Vec::new();
        for m in &frontier {
            let start = m.last().map_or(0, |last| {
                FaceKind::ALL
                    .iter()
                    .position(|k| k == last)
                    .expect("known kind")
            });
            for k in &FaceKind::ALL[start..] {
                let mut n: Vec<FaceKind> = m.clone();
                n.push(*k);
                next.push(n);
            }
        }
        out.extend(next.iter().cloned());
        frontier = next;
    }
    out
}

/// REQ-OBJ-002, CR 709.4, 710.2, 712.6a, 716.4, 719.4, 721.4: for every layout and every
/// multiset of up to three face kinds, the definition is accepted iff the kinds are exactly the
/// layout's face set; otherwise every entry point is `BadFaces`. No combination panics.
#[test]
fn sweep_face_sets_exact() {
    let v = support::vocab();
    let sets = multisets(3);
    // 1 + 10 + 55 + 220.
    assert_eq!(sets.len(), 286);
    for layout in IN_SCOPE {
        let mut required = required_kinds(layout);
        required.sort();
        let mut accepted = 0;
        for kinds in &sets {
            let faces = kinds
                .iter()
                .enumerate()
                .map(|(i, k)| blank(*k, &format!("test.sweep_{i}")))
                .collect();
            let d = def(layout, faces);
            let q = query(ZoneKind::Hand, Cast::None, St::DEFAULT);
            let got = base_characteristics(&d, &q, v);
            let mv = mana_value(
                &d,
                &q,
                fineprint_core::objects::ManaValueContext::OffStack,
                v,
            );
            let g = gating(&d);
            if *kinds == required {
                accepted += 1;
                assert!(got.is_ok(), "{layout:?} {kinds:?}: {got:?}");
                assert_eq!(mv, Ok(0));
                assert!(g.is_ok());
            } else {
                let err = LayoutError::BadFaces { layout };
                assert_eq!(got, Err(err.clone()), "{layout:?} {kinds:?}");
                assert_eq!(mv, Err(err.clone()), "{layout:?} {kinds:?}");
                assert_eq!(g, Err(err), "{layout:?} {kinds:?}");
            }
            // No panic on any zone, cast or status for this face set.
            for zone in ZoneKind::ALL {
                for cast in Cast::ALL {
                    for st in St::all() {
                        let q = query(zone, cast, st);
                        let _ = base_characteristics(&d, &q, v);
                        for ctx in contexts() {
                            let _ = mana_value(&d, &q, ctx, v);
                        }
                    }
                }
            }
        }
        assert_eq!(accepted, 1, "{layout:?}");
    }
}

/// REQ-OBJ-002, CR 709.4, 712.6a: no panic for any layout in `LayoutBehavior::ALL` (in scope or not) and any
/// query; out-of-scope layouts are `UnsupportedLayout` everywhere.
#[test]
fn sweep_no_panic_all_layouts() {
    let v = support::vocab();
    for layout in LayoutBehavior::ALL {
        let mut d = if IN_SCOPE.contains(&layout) {
            sample(layout)
        } else {
            def(layout, vec![blank(FaceKind::Main, "test.any")])
        };
        d.layout = layout;
        for zone in ZoneKind::ALL {
            for cast in Cast::ALL {
                for st in St::all() {
                    let q = query(zone, cast, st);
                    let got = base_characteristics(&d, &q, v);
                    if !IN_SCOPE.contains(&layout) {
                        assert_eq!(got, Err(LayoutError::UnsupportedLayout(layout)));
                    } else {
                        match got {
                            Ok(_) => {}
                            Err(LayoutError::InvalidCast { layout: l, zone: z }) => {
                                assert_eq!((l, z), (layout, zone));
                            }
                            Err(other) => panic!("{layout:?} {zone:?} {cast:?}: {other:?}"),
                        }
                    }
                    for ctx in contexts() {
                        let _ = mana_value(&d, &q, ctx, v);
                    }
                }
            }
        }
        let _ = gating(&d);
    }
}
