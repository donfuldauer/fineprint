// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the ROOM layout of `fineprint_core::layouts` (REQ-OBJ-002). Black-box:
//! written from the work-item brief only.
//!
//! Only the cast half on the stack (CR 709.3b, 202.3d), both halves combined in every other zone
//! but the battlefield (CR 709.4, 709.4a-c, 202.3d), and on the battlefield only the unlocked
//! halves' name, mana cost and rules text over a shared type line (CR 709.5, 709.5a, 709.5c,
//! 202.2, 202.3, 202.3a). Doors on entering: CR 709.5d.

#[path = "spec_layouts_composite_support.rs"]
mod composite;

use composite::support::{self, ability, assert_chars, blank, cost_mv, mask, on_stack, set, text};
use composite::{
    bad_type, reversed, room_costless_right, room_def, room_indicator_left, room_left, room_right,
    room_sample, room_x_left,
};
use fineprint_core::layouts::{
    LayoutError, RoomDoors, RoomLayout, RoomQuery, base_characteristics, gating, layout_for,
    mana_value,
};
use fineprint_core::objects::{Characteristics, ManaValueContext, ObjectError, SplitHalf};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{AbilityAST, CardDefinitionDSL, ColorBitmask, FaceKind, LayoutBehavior};
use fineprint_dsl::mana::ManaCostDSL;

const W: u8 = 0b0_0001;
const U: u8 = 0b0_0010;
const R: u8 = 0b0_1000;
const G: u8 = 0b1_0000;

const LOCKED: RoomDoors = RoomDoors {
    left_unlocked: false,
    right_unlocked: false,
};
const LEFT: RoomDoors = RoomDoors {
    left_unlocked: true,
    right_unlocked: false,
};
const RIGHT: RoomDoors = RoomDoors {
    left_unlocked: false,
    right_unlocked: true,
};
const BOTH: RoomDoors = RoomDoors {
    left_unlocked: true,
    right_unlocked: true,
};
const ALL_DOORS: [RoomDoors; 4] = [LOCKED, LEFT, RIGHT, BOTH];
const ALL_CASTS: [Option<SplitHalf>; 3] = [None, Some(SplitHalf::Left), Some(SplitHalf::Right)];

fn q(zone: ZoneKind, cast: Option<SplitHalf>, doors: RoomDoors) -> RoomQuery {
    RoomQuery { zone, cast, doors }
}

fn chars_of(d: &CardDefinitionDSL, rq: &RoomQuery) -> Result<Characteristics, LayoutError> {
    RoomLayout.base_characteristics(d, rq, support::vocab())
}

fn mv_of(d: &CardDefinitionDSL, rq: &RoomQuery, ctx: ManaValueContext) -> Result<i64, LayoutError> {
    RoomLayout.mana_value(d, rq, ctx, support::vocab())
}

fn opt_cost(c: &str) -> Option<ManaCostDSL> {
    ManaCostDSL::parse_optional(c).expect("literal cost")
}

fn ab_l() -> AbilityAST {
    ability("test.room_left", 1)
}
fn ab_r1() -> AbilityAST {
    ability("test.room_right", 1)
}
fn ab_r2() -> AbilityAST {
    ability("test.room_right", 2)
}

/// One literal expected row: everything but the type line, which is passed separately.
struct Exp {
    names: &'static [&'static str],
    cost: &'static str,
    indicator: Option<ColorBitmask>,
    colors: u8,
    text: &'static [&'static str],
    abilities: Vec<AbilityAST>,
    mv: i64,
}

/// The shared type line of the sample halves combined (CR 709.5a).
fn union_types() -> (
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
) {
    (&["Legendary"], &["Artifact", "Enchantment"], &["Room"])
}

fn build(e: &Exp, types: (&[&str], &[&str], &[&str])) -> Characteristics {
    Characteristics {
        names: set(e.names),
        mana_cost: opt_cost(e.cost),
        color_indicator: e.indicator,
        colors: mask(e.colors),
        supertypes: set(types.0),
        card_types: set(types.1),
        subtypes: set(types.2),
        rules_text: text(e.text),
        abilities: e.abilities.clone(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn combined_sample() -> Exp {
    Exp {
        names: &["test.room_left", "test.room_right"],
        cost: "{1}{W}{3}{U}{U}",
        indicator: None,
        colors: W | U,
        text: &["test.room_left", "test.room_right", "second"],
        abilities: vec![ab_l(), ab_r1(), ab_r2()],
        mv: 7,
    }
}

// ---------------------------------------------------------------------------------------------
// stack
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 709.3b, 202.3d: the left half cast: only its own name, cost, text,
/// abilities and type line, whatever the doors; mana value of the left half only.
#[test]
fn room_709_3b_left_half_on_stack() {
    let d = room_sample();
    let expected = build(
        &Exp {
            names: &["test.room_left"],
            cost: "{1}{W}",
            indicator: None,
            colors: W,
            text: &["test.room_left"],
            abilities: vec![ab_l()],
            mv: 2,
        },
        (&["Legendary"], &["Enchantment"], &["Room"]),
    );
    for doors in ALL_DOORS {
        let rq = q(ZoneKind::Stack, Some(SplitHalf::Left), doors);
        assert_chars(
            &chars_of(&d, &rq).unwrap(),
            &expected,
            &format!("{doors:?}"),
        );
        assert_eq!(mv_of(&d, &rq, ManaValueContext::OffStack), Ok(2));
        assert_eq!(mv_of(&d, &rq, on_stack(4, 4, 4)), Ok(2));
    }
    // Faces in reverse order select the same half.
    let rq = q(ZoneKind::Stack, Some(SplitHalf::Left), LOCKED);
    assert_chars(&chars_of(&reversed(d), &rq).unwrap(), &expected, "reversed");
}

/// REQ-OBJ-002, CR 709.3b, 202.3d: the right half cast: only its own characteristics.
#[test]
fn room_709_3b_right_half_on_stack() {
    let d = room_sample();
    let expected = build(
        &Exp {
            names: &["test.room_right"],
            cost: "{3}{U}{U}",
            indicator: None,
            colors: U,
            text: &["test.room_right", "second"],
            abilities: vec![ab_r1(), ab_r2()],
            mv: 5,
        },
        (&[], &["Artifact", "Enchantment"], &["Room"]),
    );
    for doors in ALL_DOORS {
        let rq = q(ZoneKind::Stack, Some(SplitHalf::Right), doors);
        assert_chars(
            &chars_of(&d, &rq).unwrap(),
            &expected,
            &format!("{doors:?}"),
        );
        assert_eq!(mv_of(&d, &rq, ManaValueContext::OffStack), Ok(5));
    }
    let rq = q(ZoneKind::Stack, Some(SplitHalf::Right), LEFT);
    assert_chars(&chars_of(&reversed(d), &rq).unwrap(), &expected, "reversed");

    // The left half's color indicator does not reach the right half on the stack.
    let rq = q(ZoneKind::Stack, Some(SplitHalf::Right), LOCKED);
    let got = chars_of(&room_indicator_left(), &rq).unwrap();
    assert_eq!(got.color_indicator, None);
    assert_eq!(got.colors, ColorBitmask::BLUE);
}

/// REQ-OBJ-002, CR 709.3b: on the stack a room needs the half being cast.
#[test]
fn room_709_3b_stack_without_cast_half_is_invalid_cast() {
    let err = LayoutError::InvalidCast {
        layout: LayoutBehavior::Room,
        zone: ZoneKind::Stack,
    };
    for d in [room_sample(), room_costless_right(), room_indicator_left()] {
        for doors in ALL_DOORS {
            let rq = q(ZoneKind::Stack, None, doors);
            assert_eq!(chars_of(&d, &rq), Err(err.clone()));
            assert_eq!(mv_of(&d, &rq, ManaValueContext::OffStack), Err(err.clone()));
            assert_eq!(mv_of(&d, &rq, on_stack(1, 1, 1)), Err(err.clone()));
        }
    }
    // The default query is cast None.
    let rq = RoomQuery::new(ZoneKind::Stack);
    assert_eq!(chars_of(&room_sample(), &rq), Err(err));
}

/// REQ-OBJ-002, CR 202.3d: an X in the cast half counts the chosen value only on the stack;
/// elsewhere X is 0 whatever context is given.
#[test]
fn room_202_3d_x_cost_on_and_off_stack() {
    let d = room_x_left();
    let stack = q(ZoneKind::Stack, Some(SplitHalf::Left), LOCKED);
    assert_eq!(mv_of(&d, &stack, on_stack(3, 0, 0)), Ok(4));
    assert_eq!(mv_of(&d, &stack, on_stack(0, 0, 0)), Ok(1));
    assert_eq!(mv_of(&d, &stack, ManaValueContext::OffStack), Ok(1));
    // The right half cast: X of the left half is irrelevant.
    let stack_r = q(ZoneKind::Stack, Some(SplitHalf::Right), LOCKED);
    assert_eq!(mv_of(&d, &stack_r, on_stack(3, 0, 0)), Ok(5));
    // Hand: combined {X}{R}{3}{U}{U}, X = 0.
    for ctx in [ManaValueContext::OffStack, on_stack(3, 2, 1)] {
        let hand = q(ZoneKind::Hand, Some(SplitHalf::Left), LOCKED);
        assert_eq!(mv_of(&d, &hand, ctx), Ok(6), "{ctx:?}");
        let bf_left = q(ZoneKind::Battlefield, Some(SplitHalf::Left), LEFT);
        assert_eq!(mv_of(&d, &bf_left, ctx), Ok(1), "{ctx:?}");
        let bf_both = q(ZoneKind::Battlefield, None, BOTH);
        assert_eq!(mv_of(&d, &bf_both, ctx), Ok(6), "{ctx:?}");
    }
    let hand = q(ZoneKind::Hand, None, LOCKED);
    assert_eq!(
        chars_of(&d, &hand).unwrap().mana_cost,
        opt_cost("{X}{R}{3}{U}{U}")
    );
}

// ---------------------------------------------------------------------------------------------
// every other zone: combined
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 709.4, 709.4a, 709.4b, 709.4c, 202.3d: in hand, library, graveyard, exile and
/// command the room has both halves combined; cast, doors and context are ignored.
#[test]
fn room_709_4_combined_off_stack_and_battlefield() {
    let rows: Vec<(CardDefinitionDSL, Exp)> = vec![
        (room_sample(), combined_sample()),
        (
            room_costless_right(),
            Exp {
                cost: "{1}{W}",
                colors: W,
                mv: 2,
                ..combined_sample()
            },
        ),
        (
            room_indicator_left(),
            Exp {
                indicator: Some(ColorBitmask::GREEN),
                colors: W | U | G,
                ..combined_sample()
            },
        ),
    ];
    let zones = [
        ZoneKind::Hand,
        ZoneKind::Library,
        ZoneKind::Graveyard,
        ZoneKind::Exile,
        ZoneKind::Command,
    ];
    for (d, e) in &rows {
        let expected = build(e, union_types());
        for zone in zones {
            for cast in ALL_CASTS {
                for doors in ALL_DOORS {
                    let rq = q(zone, cast, doors);
                    let ctx = format!("{} {zone:?} {cast:?} {doors:?}", d.faces[1].name);
                    assert_chars(&chars_of(d, &rq).unwrap(), &expected, &ctx);
                    for mvc in [ManaValueContext::OffStack, on_stack(5, 5, 5)] {
                        assert_eq!(mv_of(d, &rq, mvc), Ok(e.mv), "{ctx} {mvc:?}");
                    }
                }
            }
            // Reversed face order gives the same combined view (left symbols still first).
            let rq = q(zone, None, LOCKED);
            assert_chars(
                &chars_of(&reversed(d.clone()), &rq).unwrap(),
                &expected,
                "rev",
            );
        }
    }
}

/// REQ-OBJ-002, CR 709.4b: with neither half costed the combined cost is None and MV 0.
#[test]
fn room_709_4b_no_cost_on_either_half() {
    let d = room_def(
        fineprint_dsl::card::CardFaceDSL {
            mana_cost: None,
            ..room_left()
        },
        fineprint_dsl::card::CardFaceDSL {
            mana_cost: None,
            ..room_right()
        },
    );
    for zone in [ZoneKind::Hand, ZoneKind::Battlefield] {
        let rq = q(zone, None, BOTH);
        let got = chars_of(&d, &rq).unwrap();
        assert_eq!(got.mana_cost, None, "{zone:?}");
        assert_eq!(got.colors, ColorBitmask::NONE, "{zone:?}");
        assert_eq!(mv_of(&d, &rq, ManaValueContext::OffStack), Ok(0));
    }
}

// ---------------------------------------------------------------------------------------------
// battlefield: doors
// ---------------------------------------------------------------------------------------------

fn locked_row() -> Exp {
    Exp {
        names: &[],
        cost: "",
        indicator: None,
        colors: 0,
        text: &[],
        abilities: vec![],
        mv: 0,
    }
}

fn left_row() -> Exp {
    Exp {
        names: &["test.room_left"],
        cost: "{1}{W}",
        indicator: None,
        colors: W,
        text: &["test.room_left"],
        abilities: vec![ab_l()],
        mv: 2,
    }
}

fn right_row() -> Exp {
    Exp {
        names: &["test.room_right"],
        cost: "{3}{U}{U}",
        indicator: None,
        colors: U,
        text: &["test.room_right", "second"],
        abilities: vec![ab_r1(), ab_r2()],
        mv: 5,
    }
}

/// REQ-OBJ-002, CR 709.5, 709.5a, 709.5c, 202.2, 202.3, 202.3a: on the battlefield only the
/// unlocked halves have their name, mana cost and rules text; the shared type line is always
/// present; colors and mana value come from the unlocked halves' cost (colorless, 0 when both
/// are locked) plus both halves' color indicators. Cast and context are ignored.
#[test]
fn room_709_5_battlefield_door_table() {
    let g = Some(ColorBitmask::GREEN);
    let rows: Vec<(&str, CardDefinitionDSL, RoomDoors, Exp)> = vec![
        // colored halves
        ("colored", room_sample(), LOCKED, locked_row()),
        ("colored", room_sample(), LEFT, left_row()),
        ("colored", room_sample(), RIGHT, right_row()),
        ("colored", room_sample(), BOTH, combined_sample()),
        // costless right half
        ("costless", room_costless_right(), LOCKED, locked_row()),
        ("costless", room_costless_right(), LEFT, left_row()),
        (
            "costless",
            room_costless_right(),
            RIGHT,
            Exp {
                cost: "",
                colors: 0,
                mv: 0,
                ..right_row()
            },
        ),
        (
            "costless",
            room_costless_right(),
            BOTH,
            Exp {
                cost: "{1}{W}",
                colors: W,
                mv: 2,
                ..combined_sample()
            },
        ),
        // green color indicator on the left half: it colors the room even while locked
        (
            "indicator",
            room_indicator_left(),
            LOCKED,
            Exp {
                indicator: g,
                colors: G,
                ..locked_row()
            },
        ),
        (
            "indicator",
            room_indicator_left(),
            LEFT,
            Exp {
                indicator: g,
                colors: W | G,
                ..left_row()
            },
        ),
        (
            "indicator",
            room_indicator_left(),
            RIGHT,
            Exp {
                indicator: g,
                colors: U | G,
                ..right_row()
            },
        ),
        (
            "indicator",
            room_indicator_left(),
            BOTH,
            Exp {
                indicator: g,
                colors: W | U | G,
                ..combined_sample()
            },
        ),
    ];
    for (label, d, doors, e) in &rows {
        let expected = build(e, union_types());
        for cast in ALL_CASTS {
            let rq = q(ZoneKind::Battlefield, cast, *doors);
            let ctx = format!("{label} {doors:?} {cast:?}");
            assert_chars(&chars_of(d, &rq).unwrap(), &expected, &ctx);
            for mvc in [ManaValueContext::OffStack, on_stack(6, 6, 6)] {
                assert_eq!(mv_of(d, &rq, mvc), Ok(e.mv), "{ctx} {mvc:?}");
            }
        }
        let rq = q(ZoneKind::Battlefield, None, *doors);
        assert_chars(
            &chars_of(&reversed(d.clone()), &rq).unwrap(),
            &expected,
            &format!("{label} {doors:?} reversed"),
        );
    }
}

/// REQ-OBJ-002, CR 709.5, 709.5a: both doors locked still leaves the shared type line.
#[test]
fn room_709_5a_locked_keeps_type_line() {
    let rq = RoomQuery::new(ZoneKind::Battlefield);
    assert_eq!(rq.doors, RoomDoors::LOCKED);
    assert_eq!(rq.cast, None);
    let got = chars_of(&room_sample(), &rq).unwrap();
    assert_eq!(got.names, set(&[]));
    assert_eq!(got.mana_cost, None);
    assert_eq!(got.colors, ColorBitmask::NONE);
    assert_eq!(got.supertypes, set(&["Legendary"]));
    assert_eq!(got.card_types, set(&["Artifact", "Enchantment"]));
    assert_eq!(got.subtypes, set(&["Room"]));
    assert!(got.abilities.is_empty());
    assert!(got.rules_text.0.is_empty());
}

// ---------------------------------------------------------------------------------------------
// doors
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 709.5d: the cast half enters unlocked; neither cast enters with neither.
#[test]
fn room_709_5d_on_enter() {
    assert_eq!(RoomDoors::on_enter(Some(SplitHalf::Left)), LEFT);
    assert_eq!(RoomDoors::on_enter(Some(SplitHalf::Right)), RIGHT);
    assert_eq!(RoomDoors::on_enter(None), LOCKED);
    assert_eq!(RoomDoors::default(), LOCKED);
    assert_eq!(RoomDoors::LOCKED, LOCKED);
}

/// REQ-OBJ-002, CR 709.5c: `with_unlocked` and `is_unlocked` are a plain setter and getter.
#[test]
fn room_709_5c_with_unlocked_is_unlocked_round_trip() {
    for doors in ALL_DOORS {
        assert_eq!(doors.is_unlocked(SplitHalf::Left), doors.left_unlocked);
        assert_eq!(doors.is_unlocked(SplitHalf::Right), doors.right_unlocked);
        for value in [false, true] {
            let l = doors.with_unlocked(SplitHalf::Left, value);
            assert_eq!(l.left_unlocked, value);
            assert_eq!(l.right_unlocked, doors.right_unlocked);
            assert_eq!(l.is_unlocked(SplitHalf::Left), value);
            let r = doors.with_unlocked(SplitHalf::Right, value);
            assert_eq!(r.right_unlocked, value);
            assert_eq!(r.left_unlocked, doors.left_unlocked);
            assert_eq!(r.is_unlocked(SplitHalf::Right), value);
        }
    }
    let both = RoomDoors::LOCKED
        .with_unlocked(SplitHalf::Left, true)
        .with_unlocked(SplitHalf::Right, true);
    assert_eq!(both, BOTH);
    // Copy: the original is unchanged.
    let a = LEFT;
    let _b = a.with_unlocked(SplitHalf::Left, false);
    assert_eq!(a, LEFT);
}

/// REQ-OBJ-002, CR 709.5c: the query builders set the cast half and the doors.
#[test]
fn room_query_builders() {
    let rq = RoomQuery::new(ZoneKind::Exile)
        .with_cast(SplitHalf::Right)
        .with_doors(BOTH);
    assert_eq!(rq.zone, ZoneKind::Exile);
    assert_eq!(rq.cast, Some(SplitHalf::Right));
    assert_eq!(rq.doors, BOTH);
    assert_eq!(RoomLayout.behavior(), LayoutBehavior::Room);
}

// ---------------------------------------------------------------------------------------------
// errors
// ---------------------------------------------------------------------------------------------

fn obj_err() -> LayoutError {
    LayoutError::Object(ObjectError::UnknownCardType("NotACardType".to_owned()))
}

fn bad_faces() -> LayoutError {
    LayoutError::BadFaces {
        layout: LayoutBehavior::Room,
    }
}

/// The malformed face sets: missing half, extra face, duplicate kind, no faces.
fn malformed_rooms() -> Vec<CardDefinitionDSL> {
    let mk = |faces| support::def(LayoutBehavior::Room, faces);
    vec![
        mk(vec![room_left()]),
        mk(vec![room_right()]),
        mk(vec![]),
        mk(vec![
            room_left(),
            room_right(),
            blank(FaceKind::Main, "test.extra"),
        ]),
        mk(vec![room_left(), room_right(), room_right()]),
        mk(vec![room_left(), room_left()]),
        mk(vec![room_left(), blank(FaceKind::Back, "test.not_a_half")]),
    ]
}

/// REQ-OBJ-002, CR 709.5: a definition of another layout is `LayoutMismatch`, before anything.
#[test]
fn room_wrong_layout_is_mismatch() {
    for found in [
        LayoutBehavior::Split,
        LayoutBehavior::Normal,
        LayoutBehavior::Meld,
    ] {
        let mut d = room_sample();
        d.layout = found;
        let err = LayoutError::LayoutMismatch {
            expected: LayoutBehavior::Room,
            found,
        };
        for zone in ZoneKind::ALL {
            let rq = q(zone, None, LOCKED);
            assert_eq!(chars_of(&d, &rq), Err(err.clone()), "{zone:?}");
            assert_eq!(mv_of(&d, &rq, ManaValueContext::OffStack), Err(err.clone()));
        }
        // Mismatch beats bad faces too.
        let d = support::def(found, vec![room_left()]);
        assert_eq!(chars_of(&d, &q(ZoneKind::Stack, None, LOCKED)), Err(err));
    }
}

/// REQ-OBJ-002, CR 709.5: the faces must be exactly LEFT_HALF and RIGHT_HALF.
#[test]
fn room_bad_faces() {
    for d in malformed_rooms() {
        for zone in ZoneKind::ALL {
            for cast in ALL_CASTS {
                let rq = q(zone, cast, BOTH);
                assert_eq!(chars_of(&d, &rq), Err(bad_faces()), "{zone:?} {cast:?}");
                assert_eq!(mv_of(&d, &rq, ManaValueContext::OffStack), Err(bad_faces()));
            }
        }
    }
    // Reversed order is fine.
    let rq = q(ZoneKind::Hand, None, LOCKED);
    assert!(chars_of(&reversed(room_sample()), &rq).is_ok());
}

/// REQ-OBJ-002, CR 709.4c, 709.5a: a half with a type line the vocabulary rejects is `Object`
/// wherever its type line is used; `InvalidCast` and `BadFaces` take precedence.
#[test]
fn room_type_line_error_and_precedence() {
    let d = room_def(bad_type(room_left()), room_right());
    for zone in [
        ZoneKind::Hand,
        ZoneKind::Library,
        ZoneKind::Graveyard,
        ZoneKind::Exile,
        ZoneKind::Command,
        ZoneKind::Battlefield,
    ] {
        for doors in ALL_DOORS {
            let rq = q(zone, None, doors);
            assert_eq!(chars_of(&d, &rq), Err(obj_err()), "{zone:?} {doors:?}");
            assert_eq!(
                mv_of(&d, &rq, ManaValueContext::OffStack),
                Err(obj_err()),
                "{zone:?} {doors:?}"
            );
        }
    }
    // The bad half cast.
    let rq = q(ZoneKind::Stack, Some(SplitHalf::Left), LOCKED);
    assert_eq!(chars_of(&d, &rq), Err(obj_err()));
    // InvalidCast beats Object.
    let rq = q(ZoneKind::Stack, None, LOCKED);
    assert_eq!(
        chars_of(&d, &rq),
        Err(LayoutError::InvalidCast {
            layout: LayoutBehavior::Room,
            zone: ZoneKind::Stack
        })
    );
    // BadFaces beats InvalidCast and Object.
    let bad = support::def(LayoutBehavior::Room, vec![bad_type(room_left())]);
    assert_eq!(chars_of(&bad, &rq), Err(bad_faces()));
    assert_eq!(
        chars_of(&bad, &q(ZoneKind::Hand, None, LOCKED)),
        Err(bad_faces())
    );
}

// ---------------------------------------------------------------------------------------------
// sweep
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 709.3b, 709.4, 709.5, 202.3d: over every zone, cast half, door state and
/// context, for valid and malformed definitions: `base_characteristics` and `mana_value` agree
/// on errors, are deterministic and never panic; when they succeed the mana value is that of the
/// returned cost (the context counting only on the stack).
#[test]
fn room_sweep_agreement_determinism() {
    let mut defs = vec![
        room_sample(),
        room_costless_right(),
        room_indicator_left(),
        room_x_left(),
        reversed(room_sample()),
        room_def(bad_type(room_left()), room_right()),
        room_def(room_left(), bad_type(room_right())),
    ];
    defs.extend(malformed_rooms());
    let mut wrong = room_sample();
    wrong.layout = LayoutBehavior::Split;
    defs.push(wrong);
    let mut checked = 0;
    for d in &defs {
        for zone in ZoneKind::ALL {
            for cast in ALL_CASTS {
                for doors in ALL_DOORS {
                    let rq = q(zone, cast, doors);
                    let c1 = chars_of(d, &rq);
                    let c2 = chars_of(d, &rq);
                    assert_eq!(c1, c2, "deterministic chars");
                    for ctx in [ManaValueContext::OffStack, on_stack(3, 2, 1)] {
                        let m1 = mv_of(d, &rq, ctx);
                        assert_eq!(m1, mv_of(d, &rq, ctx), "deterministic mv");
                        match (&c1, &m1) {
                            (Ok(c), Ok(m)) => {
                                let eff = if zone == ZoneKind::Stack {
                                    ctx
                                } else {
                                    ManaValueContext::OffStack
                                };
                                assert_eq!(*m, cost_mv(c.mana_cost.as_ref(), eff));
                            }
                            (Err(a), Err(b)) => assert_eq!(a, b),
                            other => panic!("disagree {zone:?} {cast:?} {doors:?}: {other:?}"),
                        }
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, defs.len() * 7 * 3 * 4 * 2);
}

/// REQ-OBJ-002: ROOM is not wired into `layout_for` or the free functions.
#[test]
fn room_not_wired_into_layout_for() {
    let err = LayoutError::UnsupportedLayout(LayoutBehavior::Room);
    assert_eq!(layout_for(LayoutBehavior::Room).err(), Some(err.clone()));
    let d = room_sample();
    let lq = fineprint_core::layouts::LayoutQuery::new(ZoneKind::Hand);
    assert_eq!(
        base_characteristics(&d, &lq, support::vocab()),
        Err(err.clone())
    );
    assert_eq!(
        mana_value(&d, &lq, ManaValueContext::OffStack, support::vocab()),
        Err(err.clone())
    );
    assert_eq!(gating(&d), Err(err));
}

/// Literal sanity: the support oracle for split combination is not used here; the expected
/// values above are literals. This checks the bit constants used by the tables.
#[test]
fn room_color_constants() {
    assert_eq!(mask(W), ColorBitmask::WHITE);
    assert_eq!(mask(U), ColorBitmask::BLUE);
    assert_eq!(mask(R), ColorBitmask::RED);
    assert_eq!(mask(G), ColorBitmask::GREEN);
}
