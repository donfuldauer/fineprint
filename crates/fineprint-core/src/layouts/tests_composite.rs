// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests of the composite layouts: ROOM, MELD, STATION, PREPARATION and MUTATE/merged
//! permanents (REQ-OBJ-002, REQ-NFR-008). Synthetic data only (R5).

use std::collections::{BTreeMap, BTreeSet};

use fineprint_dsl::card::{
    AbilityAST, CardDefinitionDSL, CardFaceDSL, ColorBitmask, DSL_VERSION, FaceKind,
    LayoutBehavior, Origin, ProvenanceDSL, SupportStatus, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::mana::ManaCostDSL;
use fineprint_dsl::vocab::RulesVocabulary;

use super::*;
use crate::objects::{
    CdaColors, Characteristics, ManaValueContext, ObjectError, SplitHalf, VariableValues,
    base_colors,
};
use crate::zones::ZoneKind;

// ---------------------------------------------------------------- fixtures

const OFF: ManaValueContext = ManaValueContext::OffStack;

/// The zones other than the stack and the battlefield.
const OTHER_ZONES: [ZoneKind; 5] = [
    ZoneKind::Library,
    ZoneKind::Hand,
    ZoneKind::Graveyard,
    ZoneKind::Exile,
    ZoneKind::Command,
];

/// Every cast choice value.
const CASTS: [CastChoice; 10] = [
    CastChoice::None,
    CastChoice::SplitHalf(SplitHalf::Left),
    CastChoice::SplitHalf(SplitHalf::Right),
    CastChoice::Fused,
    CastChoice::Adventure,
    CastChoice::Omen,
    CastChoice::Prototype,
    CastChoice::Transformed,
    CastChoice::ModalFace(FaceUp::Front),
    CastChoice::ModalFace(FaceUp::Back),
];

/// The four door states, in the order neither, left only, right only, both.
const DOORS: [RoomDoors; 4] = [
    RoomDoors {
        left_unlocked: false,
        right_unlocked: false,
    },
    RoomDoors {
        left_unlocked: true,
        right_unlocked: false,
    },
    RoomDoors {
        left_unlocked: false,
        right_unlocked: true,
    },
    RoomDoors {
        left_unlocked: true,
        right_unlocked: true,
    },
];

fn vocab() -> &'static RulesVocabulary {
    RulesVocabulary::bundled().unwrap()
}

fn cost(text: &str) -> ManaCostDSL {
    ManaCostDSL::parse(text).unwrap()
}

fn on(x: u32) -> ManaValueContext {
    ManaValueContext::OnStack(VariableValues { x, y: 0, z: 0 })
}

fn subtype_of(card_type: &str, n: usize) -> String {
    vocab().subtypes_of(card_type).nth(n).unwrap().to_owned()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn colors_of(text: &str) -> ColorBitmask {
    base_colors(Some(&cost(text)), None, CdaColors::None)
}

fn ability(tag: &str) -> AbilityAST {
    AbilityAST(format!("test.ability.{tag}").into())
}

fn abilities(tags: &[&str]) -> Vec<AbilityAST> {
    tags.iter().map(|t| ability(t)).collect()
}

fn texts(tags: &[&str]) -> TextTokenStream {
    TextTokenStream(
        tags.iter()
            .map(|t| format!("test.text.{t}").into())
            .collect(),
    )
}

fn pt(n: i64) -> ValueExpressionAST {
    ValueExpressionAST(n.into())
}

/// A synthetic face whose name, text and ability are distinguishable by `tag`.
fn face(kind: FaceKind, tag: &str, mana_cost: Option<&str>, card_types: &[&str]) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: kind,
        name: format!("test.{tag}"),
        mana_cost: mana_cost.map(cost),
        color_indicator: None,
        supertypes: Vec::new(),
        card_types: card_types.iter().map(|s| (*s).to_owned()).collect(),
        subtypes: Vec::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
        text: texts(&[tag]),
        abilities: vec![ability(tag)],
    }
}

fn with_pt(mut f: CardFaceDSL, p: i64, t: i64) -> CardFaceDSL {
    f.power = Some(pt(p));
    f.toughness = Some(pt(t));
    f
}

fn def(id: &str, layout: LayoutBehavior, faces: Vec<CardFaceDSL>) -> CardDefinitionDSL {
    CardDefinitionDSL {
        card_def_id: id.to_owned(),
        dsl_version: DSL_VERSION.to_owned(),
        definition_hash: String::new(),
        source: ProvenanceDSL {
            oracle_text_hash: "0".repeat(64),
            origin: Origin::Parser,
            parser_version: None,
            override_source: None,
            verified_by: None,
            rules_baseline: "test-baseline".to_owned(),
        },
        support_status: SupportStatus::AutoParsed,
        known_gaps: None,
        layout,
        faces,
        meld_partner: None,
        color_identity: ColorBitmask::NONE,
        commander_eligibility: None,
        legalities: BTreeMap::new(),
        digital_only: false,
        related_tokens: None,
    }
}

/// A face whose card type the vocabulary rejects.
fn bad_types(mut f: CardFaceDSL) -> CardFaceDSL {
    f.card_types = vec!["test.not_a_type".to_owned()];
    f
}

fn object_error() -> LayoutError {
    LayoutError::Object(ObjectError::UnknownCardType("test.not_a_type".to_owned()))
}

fn face_of(d: &CardDefinitionDSL, kind: FaceKind) -> &CardFaceDSL {
    d.faces.iter().find(|f| f.face_kind == kind).unwrap()
}

fn printed(d: &CardDefinitionDSL, kind: FaceKind) -> Characteristics {
    Characteristics::from_face(face_of(d, kind), vocab()).unwrap()
}

fn mismatch(expected: LayoutBehavior, found: LayoutBehavior) -> LayoutError {
    LayoutError::LayoutMismatch { expected, found }
}

fn bad_faces(layout: LayoutBehavior) -> LayoutError {
    LayoutError::BadFaces { layout }
}

fn statuses() -> Vec<LayoutStatus> {
    let mut out = Vec::new();
    for flipped in [false, true] {
        for face_up in [FaceUp::Front, FaceUp::Back] {
            for copying_back_face in [false, true] {
                out.push(LayoutStatus {
                    flipped,
                    face_up,
                    copying_back_face,
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------- errors and wiring

// REQ-OBJ-002: layout_for still refuses the five composite layouts (dedicated APIs only).
#[test]
fn layout_for_still_unsupported_for_composite_layouts() {
    for b in [
        LayoutBehavior::Room,
        LayoutBehavior::Meld,
        LayoutBehavior::Station,
        LayoutBehavior::Preparation,
        LayoutBehavior::Mutate,
    ] {
        assert_eq!(layout_for(b).err(), Some(LayoutError::UnsupportedLayout(b)));
    }
}

// REQ-OBJ-002: the new error variants display a message and have no source.
#[test]
fn new_error_variants_display() {
    use std::error::Error as _;
    let cases = [
        (LayoutError::NotMeldPair, "not a meld pair"),
        (LayoutError::AlreadyPrepared, "already prepared"),
        (LayoutError::TooFewComponents { count: 1 }, "got 1"),
        (LayoutError::InvalidStationSymbols { count: 3 }, "got 3"),
    ];
    for (e, needle) in cases {
        assert!(e.to_string().contains(needle), "{e}");
        assert!(e.source().is_none());
    }
}

// ---------------------------------------------------------------- ROOM (CR 709.3-709.5)

/// A Room: left half `{1}{W}`, right half `{X}{2}{B}`, both enchantments with one shared
/// subtype; `swap` lists the right half first.
fn room(swap: bool) -> CardDefinitionDSL {
    let sub = subtype_of("Enchantment", 0);
    let mut left = face(FaceKind::LeftHalf, "left", Some("{1}{W}"), &["Enchantment"]);
    let mut right = face(
        FaceKind::RightHalf,
        "right",
        Some("{X}{2}{B}"),
        &["Enchantment"],
    );
    left.subtypes = vec![sub.clone()];
    right.subtypes = vec![sub];
    let faces = if swap {
        vec![right, left]
    } else {
        vec![left, right]
    };
    def("test.room_pair", LayoutBehavior::Room, faces)
}

fn room_base(d: &CardDefinitionDSL, q: RoomQuery) -> Result<Characteristics, LayoutError> {
    RoomLayout.base_characteristics(d, &q, vocab())
}

fn room_mv(d: &CardDefinitionDSL, q: RoomQuery, ctx: ManaValueContext) -> Result<i64, LayoutError> {
    RoomLayout.mana_value(d, &q, ctx, vocab())
}

fn other(half: SplitHalf) -> SplitHalf {
    match half {
        SplitHalf::Left => SplitHalf::Right,
        SplitHalf::Right => SplitHalf::Left,
    }
}

// REQ-OBJ-002, CR 709.5c, 709.5d: door state, entering and the query builder.
#[test]
fn room_doors_and_query() {
    assert_eq!(RoomDoors::default(), RoomDoors::LOCKED);
    assert_eq!(RoomDoors::LOCKED, DOORS[0]);
    // CR 709.5d: only the cast half is unlocked; neither when not cast.
    assert_eq!(RoomDoors::on_enter(None), DOORS[0]);
    assert_eq!(RoomDoors::on_enter(Some(SplitHalf::Left)), DOORS[1]);
    assert_eq!(RoomDoors::on_enter(Some(SplitHalf::Right)), DOORS[2]);
    for d in DOORS {
        assert_eq!(d.is_unlocked(SplitHalf::Left), d.left_unlocked);
        assert_eq!(d.is_unlocked(SplitHalf::Right), d.right_unlocked);
        for half in [SplitHalf::Left, SplitHalf::Right] {
            for v in [false, true] {
                let n = d.with_unlocked(half, v);
                assert_eq!(n.is_unlocked(half), v);
                assert_eq!(n.is_unlocked(other(half)), d.is_unlocked(other(half)));
            }
        }
    }
    let q = RoomQuery::new(ZoneKind::Hand);
    assert_eq!(q.zone, ZoneKind::Hand);
    assert_eq!(q.cast, None);
    assert_eq!(q.doors, RoomDoors::LOCKED);
    let q = q.with_cast(SplitHalf::Right).with_doors(DOORS[3]);
    assert_eq!(q.cast, Some(SplitHalf::Right));
    assert_eq!(q.doors, DOORS[3]);
    assert_eq!(q.zone, ZoneKind::Hand);
    assert_eq!(RoomLayout.behavior(), LayoutBehavior::Room);
}

// REQ-OBJ-002, CR 709.3b, 202.3d: on the stack only the cast half exists, either half, with
// either face order in the definition; X counts on the stack.
#[test]
fn room_stack_only_cast_half() {
    for swap in [false, true] {
        let d = room(swap);
        for (half, kind, want_mv) in [
            (SplitHalf::Left, FaceKind::LeftHalf, 2),
            (SplitHalf::Right, FaceKind::RightHalf, 8),
        ] {
            let q = RoomQuery::new(ZoneKind::Stack).with_cast(half);
            for doors in DOORS {
                let q = q.with_doors(doors);
                assert_eq!(room_base(&d, q).unwrap(), printed(&d, kind));
                assert_eq!(room_mv(&d, q, on(5)).unwrap(), want_mv);
            }
        }
    }
}

// REQ-OBJ-002, CR 709.4, 709.4a, 709.4b, 709.4c, 202.3d: off the stack and battlefield, both
// halves combined; X counts 0 whatever the context; the cast half and doors are ignored.
#[test]
fn room_other_zones_combined() {
    for swap in [false, true] {
        let d = room(swap);
        for zone in OTHER_ZONES {
            for doors in DOORS {
                for cast in [None, Some(SplitHalf::Left), Some(SplitHalf::Right)] {
                    let q = RoomQuery { zone, cast, doors };
                    let c = room_base(&d, q).unwrap();
                    assert_eq!(c.names, set(&["test.left", "test.right"]));
                    assert_eq!(c.mana_cost, Some(cost("{1}{W}{X}{2}{B}")));
                    assert_eq!(c.colors, colors_of("{W}{B}"));
                    assert_eq!(c.card_types, set(&["Enchantment"]));
                    assert_eq!(c.subtypes.len(), 1);
                    assert_eq!(c.rules_text, texts(&["left", "right"]));
                    assert_eq!(c.abilities, abilities(&["left", "right"]));
                    assert_eq!(c.power, None);
                    assert_eq!(c.toughness, None);
                    assert_eq!(c.loyalty, None);
                    assert_eq!(c.defense, None);
                    assert_eq!(room_mv(&d, q, on(5)).unwrap(), 5);
                    assert_eq!(room_mv(&d, q, OFF).unwrap(), 5);
                }
            }
        }
    }
}

/// One battlefield row: doors, names, mana cost, colors, text tags, mana value.
type DoorRow = (
    RoomDoors,
    &'static [&'static str],
    Option<&'static str>,
    ColorBitmask,
    &'static [&'static str],
    i64,
);

// REQ-OBJ-002, CR 709.5, 709.5a, 202.3, 202.3a: on the battlefield only unlocked halves have
// their name, mana cost, text and abilities; the shared type line always stays.
#[test]
fn room_battlefield_door_states() {
    // (doors, names, cost, colors, tags, mana value)
    let rows: [DoorRow; 4] = [
        (DOORS[0], &[], None, ColorBitmask::NONE, &[], 0),
        (
            DOORS[1],
            &["test.left"],
            Some("{1}{W}"),
            colors_of("{W}"),
            &["left"],
            2,
        ),
        (
            DOORS[2],
            &["test.right"],
            Some("{X}{2}{B}"),
            colors_of("{B}"),
            &["right"],
            3,
        ),
        (
            DOORS[3],
            &["test.left", "test.right"],
            Some("{1}{W}{X}{2}{B}"),
            colors_of("{W}{B}"),
            &["left", "right"],
            5,
        ),
    ];
    for swap in [false, true] {
        let d = room(swap);
        for (doors, names, want_cost, colors, tags, want_mv) in rows {
            for cast in [None, Some(SplitHalf::Left), Some(SplitHalf::Right)] {
                let q = RoomQuery {
                    zone: ZoneKind::Battlefield,
                    cast,
                    doors,
                };
                let c = room_base(&d, q).unwrap();
                assert_eq!(c.names, set(names), "{doors:?}");
                assert_eq!(c.mana_cost, want_cost.map(cost));
                assert_eq!(c.colors, colors);
                assert_eq!(c.color_indicator, None);
                assert_eq!(c.card_types, set(&["Enchantment"]));
                assert_eq!(c.subtypes.len(), 1);
                assert_eq!(c.rules_text, texts(tags));
                assert_eq!(c.abilities, abilities(tags));
                assert_eq!(c.power, None);
                assert_eq!(room_mv(&d, q, on(7)).unwrap(), want_mv);
            }
        }
    }
}

// REQ-OBJ-002, CR 709.5, 202.2: a locked half's color indicator stays (DECISIONS.md #23).
#[test]
fn room_locked_half_keeps_color_indicator() {
    let mut d = room(false);
    let indicator = colors_of("{G}");
    d.faces[1].color_indicator = Some(indicator);
    let q = RoomQuery::new(ZoneKind::Battlefield);
    let c = room_base(&d, q).unwrap();
    assert_eq!(c.color_indicator, Some(indicator));
    assert_eq!(c.colors, indicator);
    let c = room_base(&d, q.with_doors(DOORS[1])).unwrap();
    assert_eq!(
        c.colors,
        base_colors(Some(&cost("{W}")), Some(indicator), CdaColors::None)
    );
}

// REQ-OBJ-002, CR 709.3, 709.5: errors in precedence order.
#[test]
fn room_errors() {
    let good = room(false);
    let q = RoomQuery::new(ZoneKind::Stack);
    let mut wrong = good.clone();
    wrong.layout = LayoutBehavior::Split;
    let e = mismatch(LayoutBehavior::Room, LayoutBehavior::Split);
    assert_eq!(room_base(&wrong, q).unwrap_err(), e);
    assert_eq!(room_mv(&wrong, q, OFF).unwrap_err(), e);
    let mut missing = good.clone();
    missing.faces.pop();
    let mut dup = good.clone();
    dup.faces[1].face_kind = FaceKind::LeftHalf;
    let mut extra = good.clone();
    extra
        .faces
        .push(face(FaceKind::Main, "x", None, &["Enchantment"]));
    for d in [&missing, &dup, &extra] {
        assert_eq!(
            room_base(d, q).unwrap_err(),
            bad_faces(LayoutBehavior::Room)
        );
        assert_eq!(
            room_mv(d, q, OFF).unwrap_err(),
            bad_faces(LayoutBehavior::Room)
        );
    }
    // CR 709.3: no cast half on the stack.
    let invalid = LayoutError::InvalidCast {
        layout: LayoutBehavior::Room,
        zone: ZoneKind::Stack,
    };
    assert_eq!(room_base(&good, q).unwrap_err(), invalid);
    // CR 709.5a: one shared type line, so a rejected type line is on both halves.
    let mut bad = good.clone();
    bad.faces[0] = bad_types(bad.faces[0].clone());
    bad.faces[1] = bad_types(bad.faces[1].clone());
    assert_eq!(room_base(&bad, q).unwrap_err(), invalid);
    assert_eq!(room_mv(&bad, q, OFF).unwrap_err(), invalid);
    for half in [SplitHalf::Left, SplitHalf::Right] {
        assert_eq!(
            room_base(&bad, q.with_cast(half)).unwrap_err(),
            object_error()
        );
    }
    // BadFaces (halves' type lines differ) beats InvalidCast and Object.
    let mut differ = good.clone();
    differ.faces[1] = bad_types(differ.faces[1].clone());
    assert_eq!(
        room_base(&differ, q).unwrap_err(),
        bad_faces(LayoutBehavior::Room)
    );
    assert_eq!(
        room_mv(&differ, q, OFF).unwrap_err(),
        bad_faces(LayoutBehavior::Room)
    );
    for zone in [ZoneKind::Hand, ZoneKind::Battlefield] {
        let q = RoomQuery::new(zone);
        assert_eq!(room_base(&bad, q).unwrap_err(), object_error());
        assert_eq!(room_mv(&bad, q, OFF).unwrap_err(), object_error());
    }
}

/// The `room(false)` definition with one half's type line changed: a differing card type, a
/// differing subtype or a differing supertype, on either half.
fn rooms_with_differing_type_lines() -> Vec<CardDefinitionDSL> {
    let mut out = Vec::new();
    for i in [0usize, 1] {
        let mut card_type = room(false);
        card_type.faces[i].card_types.push("Artifact".to_owned());
        let mut subtype = room(false);
        subtype.faces[i].subtypes = vec![subtype_of("Enchantment", 1)];
        let mut supertype = room(false);
        supertype.faces[i].supertypes = vec!["Legendary".to_owned()];
        out.extend([card_type, subtype, supertype]);
    }
    out
}

/// REQ-OBJ-002, CR 709.5a: a Room has one shared type line; halves whose supertypes, card
/// types or subtypes differ are `BadFaces` in every zone: the stack with either cast half,
/// the battlefield with every door state, and every other zone. Order and repeats inside a
/// type-line field do not matter (set comparison).
#[test]
fn room_differing_type_lines_rejected() {
    let bad = bad_faces(LayoutBehavior::Room);
    for d in rooms_with_differing_type_lines() {
        for zone in ZoneKind::ALL {
            for cast in [None, Some(SplitHalf::Left), Some(SplitHalf::Right)] {
                for doors in DOORS {
                    let q = RoomQuery { zone, cast, doors };
                    assert_eq!(room_base(&d, q), Err(bad.clone()), "{zone:?} {cast:?}");
                    assert_eq!(room_mv(&d, q, on(2)), Err(bad.clone()), "{zone:?}");
                    assert_eq!(room_mv(&d, q, OFF), Err(bad.clone()), "{zone:?}");
                }
            }
        }
    }
    // Same entries in another order, or repeated, are one shared type line.
    let mut d = room(false);
    d.faces[0].card_types = vec!["Enchantment".to_owned(), "Artifact".to_owned()];
    d.faces[1].card_types = vec![
        "Artifact".to_owned(),
        "Enchantment".to_owned(),
        "Artifact".to_owned(),
    ];
    let q = RoomQuery::new(ZoneKind::Stack).with_cast(SplitHalf::Right);
    assert_eq!(
        room_base(&d, q).unwrap().card_types,
        set(&["Artifact", "Enchantment"])
    );
}

// REQ-OBJ-002: sweep over zones x casts x door states: no panic, same result twice, and
// mana_value fails exactly when base_characteristics does.
#[test]
fn room_sweep_deterministic() {
    let mut bad = room(false);
    bad.faces[0] = bad_types(bad.faces[0].clone());
    bad.faces[1] = bad_types(bad.faces[1].clone());
    let mut differ = room(false);
    differ.faces[0] = bad_types(differ.faces[0].clone());
    let mut empty = room(false);
    empty.faces.clear();
    for d in [room(false), room(true), bad, differ, empty] {
        for zone in ZoneKind::ALL {
            for cast in [None, Some(SplitHalf::Left), Some(SplitHalf::Right)] {
                for doors in DOORS {
                    let q = RoomQuery { zone, cast, doors };
                    let a = room_base(&d, q);
                    assert_eq!(a, room_base(&d, q));
                    let m = room_mv(&d, q, on(2));
                    assert_eq!(m, room_mv(&d, q, on(2)));
                    assert_eq!(a.as_ref().err(), m.as_ref().err());
                }
            }
        }
    }
}

// ---------------------------------------------------------------- MELD (CR 713, 701.42)

/// Meld card A (`{2}{W}` 2/2, carries the combined 9/9 face) naming B as its partner.
fn meld_a() -> CardDefinitionDSL {
    let mut d = def(
        "test.meld_a",
        LayoutBehavior::Meld,
        vec![
            with_pt(
                face(FaceKind::Main, "meld_a", Some("{2}{W}"), &["Creature"]),
                2,
                2,
            ),
            with_pt(
                face(FaceKind::MeldResult, "melded", None, &["Creature"]),
                9,
                9,
            ),
        ],
    );
    d.meld_partner = Some("test.meld_b".to_owned());
    d
}

/// Meld card B (`{X}{3}{U}` 3/3, no combined face) naming A as its partner.
fn meld_b() -> CardDefinitionDSL {
    let mut d = def(
        "test.meld_b",
        LayoutBehavior::Meld,
        vec![with_pt(
            face(FaceKind::Main, "meld_b", Some("{X}{3}{U}"), &["Creature"]),
            3,
            3,
        )],
    );
    d.meld_partner = Some("test.meld_a".to_owned());
    d
}

// REQ-OBJ-002, CR 713.5a, 713.5b, 713.1b, 713.1c: an unmelded meld card has only its front
// face in every zone, whatever the cast choice or status.
#[test]
fn meld_unmelded_front_face_everywhere() {
    for d in [meld_a(), meld_b()] {
        let want = printed(&d, FaceKind::Main);
        for zone in ZoneKind::ALL {
            for cast in CASTS {
                for status in statuses() {
                    let q = LayoutQuery::new(zone).with_cast(cast).with_status(status);
                    assert_eq!(
                        MeldLayout.base_characteristics(&d, &q, vocab()).unwrap(),
                        want
                    );
                }
            }
        }
        assert_eq!(MeldLayout.gating(&d).unwrap(), None);
    }
    assert_eq!(MeldLayout.behavior(), LayoutBehavior::Meld);
    // CR 202.3: the front cost; X counts only on the stack.
    let b = meld_b();
    let mv = |zone, ctx| MeldLayout.mana_value(&b, &LayoutQuery::new(zone), ctx, vocab());
    assert_eq!(mv(ZoneKind::Stack, on(4)).unwrap(), 8);
    assert_eq!(mv(ZoneKind::Hand, on(4)).unwrap(), 4);
    assert_eq!(mv(ZoneKind::Battlefield, on(4)).unwrap(), 4);
}

// REQ-OBJ-002, CR 713.1, 713.5: face-set and layout errors of an unmelded meld card.
#[test]
fn meld_layout_errors() {
    let q = LayoutQuery::new(ZoneKind::Hand);
    let mut wrong = meld_a();
    wrong.layout = LayoutBehavior::DfcNonmodal;
    let e = mismatch(LayoutBehavior::Meld, LayoutBehavior::DfcNonmodal);
    assert_eq!(
        MeldLayout
            .base_characteristics(&wrong, &q, vocab())
            .unwrap_err(),
        e
    );
    assert_eq!(
        MeldLayout.mana_value(&wrong, &q, OFF, vocab()).unwrap_err(),
        e
    );
    assert_eq!(MeldLayout.gating(&wrong).unwrap_err(), e);
    let mut no_main = meld_a();
    no_main.faces.remove(0);
    let mut two_mains = meld_b();
    two_mains.faces.push(two_mains.faces[0].clone());
    let mut two_results = meld_a();
    two_results.faces.push(two_results.faces[1].clone());
    let mut back = meld_a();
    back.faces[1].face_kind = FaceKind::Back;
    let mut empty = meld_a();
    empty.faces.clear();
    let bf = bad_faces(LayoutBehavior::Meld);
    for d in [&no_main, &two_mains, &two_results, &back, &empty] {
        assert_eq!(
            MeldLayout.base_characteristics(d, &q, vocab()).unwrap_err(),
            bf
        );
        assert_eq!(MeldLayout.gating(d).unwrap_err(), bf);
        assert_eq!(MeldedPair::new(d, &meld_b()).unwrap_err(), bf);
    }
    // Only the selected (front) face is type-checked.
    let mut bad_result = meld_a();
    bad_result.faces[1] = bad_types(bad_result.faces[1].clone());
    assert!(
        MeldLayout
            .base_characteristics(&bad_result, &q, vocab())
            .is_ok()
    );
    let mut bad_front = meld_a();
    bad_front.faces[0] = bad_types(bad_front.faces[0].clone());
    assert_eq!(
        MeldLayout
            .base_characteristics(&bad_front, &q, vocab())
            .unwrap_err(),
        object_error()
    );
    assert_eq!(
        MeldLayout
            .mana_value(&bad_front, &q, OFF, vocab())
            .unwrap_err(),
        object_error()
    );
}

// REQ-OBJ-002, CR 713.5c, 202.3c, 713.8, 713.8e: the melded permanent, in either argument
// order.
#[test]
fn melded_pair_characteristics_and_counts() {
    let (a, b) = (meld_a(), meld_b());
    for (first, second) in [(&a, &b), (&b, &a)] {
        let pair = MeldedPair::new(first, second).unwrap();
        let c = pair.characteristics(vocab()).unwrap();
        // CR 713.5c: only the combined back face; nothing from either front.
        assert_eq!(c, printed(&a, FaceKind::MeldResult));
        assert_eq!(c.names, set(&["test.melded"]));
        assert_eq!(c.power, Some(pt(9)));
        assert_eq!(c.mana_cost, None);
        // CR 202.3c: {2}{W} = 3 plus {X}{3}{U} = 4 (X is 0); a copy is 0.
        assert_eq!(pair.mana_value(false, vocab()).unwrap(), 7);
        assert_eq!(pair.mana_value(true, vocab()).unwrap(), 0);
        // CR 713.8, 713.8e: one object, two cards.
        assert_eq!(pair.object_count(), 1);
        assert_eq!(pair.card_count(), 2);
        let [x, y] = pair.cards_on_leaving();
        assert_eq!(x.card_def_id, first.card_def_id);
        assert_eq!(y.card_def_id, second.card_def_id);
    }
}

// REQ-OBJ-002, CR 701.42b, 713.5c: MeldedPair::new errors in precedence order.
#[test]
fn melded_pair_errors() {
    let (a, b) = (meld_a(), meld_b());
    let mut normal = meld_b();
    normal.layout = LayoutBehavior::Normal;
    let mm = mismatch(LayoutBehavior::Meld, LayoutBehavior::Normal);
    let bf = bad_faces(LayoutBehavior::Meld);
    assert_eq!(MeldedPair::new(&normal, &a).unwrap_err(), mm);
    assert_eq!(MeldedPair::new(&a, &normal).unwrap_err(), mm);
    // Both layouts are checked before either face set.
    let mut empty = meld_a();
    empty.faces.clear();
    assert_eq!(MeldedPair::new(&empty, &normal).unwrap_err(), mm);
    assert_eq!(MeldedPair::new(&a, &empty).unwrap_err(), bf);
    // CR 701.42b: a one-way partner, no partner, and the same card twice.
    let mut one_way = meld_b();
    one_way.meld_partner = Some("test.meld_other".to_owned());
    let mut none = meld_b();
    none.meld_partner = None;
    for other in [&one_way, &none] {
        assert_eq!(
            MeldedPair::new(&a, other).unwrap_err(),
            LayoutError::NotMeldPair
        );
        assert_eq!(
            MeldedPair::new(other, &a).unwrap_err(),
            LayoutError::NotMeldPair
        );
    }
    let mut selfish = meld_a();
    selfish.meld_partner = Some("test.meld_a".to_owned());
    assert_eq!(
        MeldedPair::new(&selfish, &selfish).unwrap_err(),
        LayoutError::NotMeldPair
    );
    // NotMeldPair comes before the combined-face check.
    let mut a_plain = meld_a();
    a_plain.faces.truncate(1);
    assert_eq!(
        MeldedPair::new(&a_plain, &one_way).unwrap_err(),
        LayoutError::NotMeldPair
    );
    // Neither or both carry the combined face.
    let mut b_with = meld_b();
    b_with.faces.push(face_of(&a, FaceKind::MeldResult).clone());
    assert_eq!(MeldedPair::new(&a_plain, &b).unwrap_err(), bf);
    assert_eq!(MeldedPair::new(&a, &b_with).unwrap_err(), bf);
    // The combined face may sit on the second definition.
    let pair = MeldedPair::new(&a_plain, &b_with).unwrap();
    assert_eq!(
        pair.characteristics(vocab()).unwrap().names,
        set(&["test.melded"])
    );
    // A bad combined type line fails both queries the same way.
    let mut bad = meld_a();
    bad.faces[1] = bad_types(bad.faces[1].clone());
    let pair = MeldedPair::new(&bad, &b).unwrap();
    assert_eq!(pair.characteristics(vocab()).unwrap_err(), object_error());
    assert_eq!(pair.mana_value(false, vocab()).unwrap_err(), object_error());
    assert_eq!(pair.mana_value(true, vocab()).unwrap_err(), object_error());
}

// ---------------------------------------------------------------- STATION (CR 722)

/// A station card: an artifact `{X}{2}` with a printed 5/5 box.
fn station() -> CardDefinitionDSL {
    def(
        "test.station_card",
        LayoutBehavior::Station,
        vec![with_pt(
            face(FaceKind::Main, "station", Some("{X}{2}"), &["Artifact"]),
            5,
            5,
        )],
    )
}

fn symbol(threshold: i64, tag: &str, box_pt: Option<i64>) -> StationSymbol {
    StationSymbol {
        threshold,
        abilities: vec![ability(tag)],
        power_toughness: box_pt.map(|n| (pt(n), pt(n))),
    }
}

// REQ-OBJ-002, CR 722.2c, 722.4: printed characteristics, with no power or toughness off the
// battlefield (the stack included).
#[test]
fn station_pt_only_on_battlefield() {
    let d = station();
    let full = printed(&d, FaceKind::Main);
    for zone in ZoneKind::ALL {
        for cast in CASTS {
            let q = LayoutQuery::new(zone).with_cast(cast);
            let c = StationLayout.base_characteristics(&d, &q, vocab()).unwrap();
            if zone == ZoneKind::Battlefield {
                assert_eq!(c, full);
            } else {
                assert_eq!(c.power, None, "{zone:?}");
                assert_eq!(c.toughness, None);
                assert_eq!(
                    Characteristics {
                        power: full.power.clone(),
                        toughness: full.toughness.clone(),
                        ..c
                    },
                    full
                );
            }
        }
    }
    let mv = |zone| StationLayout.mana_value(&d, &LayoutQuery::new(zone), on(3), vocab());
    assert_eq!(mv(ZoneKind::Stack).unwrap(), 5);
    assert_eq!(mv(ZoneKind::Battlefield).unwrap(), 2);
    assert_eq!(StationLayout.gating(&d).unwrap(), None);
    assert_eq!(StationLayout.behavior(), LayoutBehavior::Station);
}

// REQ-OBJ-002, CR 722.1: station layout errors.
#[test]
fn station_errors() {
    let q = LayoutQuery::new(ZoneKind::Hand);
    let mut wrong = station();
    wrong.layout = LayoutBehavior::Normal;
    let e = mismatch(LayoutBehavior::Station, LayoutBehavior::Normal);
    assert_eq!(
        StationLayout
            .base_characteristics(&wrong, &q, vocab())
            .unwrap_err(),
        e
    );
    assert_eq!(StationLayout.gating(&wrong).unwrap_err(), e);
    let mut two = station();
    two.faces.push(two.faces[0].clone());
    let bf = bad_faces(LayoutBehavior::Station);
    assert_eq!(
        StationLayout
            .mana_value(&two, &q, OFF, vocab())
            .unwrap_err(),
        bf
    );
    assert_eq!(StationLayout.gating(&two).unwrap_err(), bf);
    let mut bad = station();
    bad.faces[0] = bad_types(bad.faces[0].clone());
    assert_eq!(
        StationLayout
            .mana_value(&bad, &q, OFF, vocab())
            .unwrap_err(),
        object_error()
    );
}

// REQ-OBJ-002, CR 722.1, 722.2a, 722.2b: symbols gated on charge counters on the battlefield.
#[test]
fn station_active_symbols() {
    // CR 722.1: one or two symbols, checked in every zone.
    for (n, zone) in [
        (0usize, ZoneKind::Battlefield),
        (3, ZoneKind::Battlefield),
        (3, ZoneKind::Hand),
    ] {
        let symbols: Vec<StationSymbol> = (0..n).map(|_| symbol(1, "s", None)).collect();
        assert_eq!(
            active_station_symbols(&symbols, zone, 100).unwrap_err(),
            LayoutError::InvalidStationSymbols { count: n }
        );
    }
    // One symbol {4+}: below, at and above the threshold.
    let one = [symbol(4, "one", Some(3))];
    for (counters, want) in [(-1, 0), (0, 0), (3, 0), (4, 1), (5, 1)] {
        let got = active_station_symbols(&one, ZoneKind::Battlefield, counters).unwrap();
        assert_eq!(got.len(), want, "{counters}");
    }
    // Two symbols {2+} and {8+}: both may apply, in printed order.
    let two = [symbol(2, "low", None), symbol(8, "high", Some(6))];
    let rows: [(i64, &[usize]); 7] = [
        (-5, &[]),
        (1, &[]),
        (2, &[0]),
        (3, &[0]),
        (7, &[0]),
        (8, &[0, 1]),
        (9, &[0, 1]),
    ];
    for (counters, want) in rows {
        let got = active_station_symbols(&two, ZoneKind::Battlefield, counters).unwrap();
        let want: Vec<&StationSymbol> = want.iter().map(|i| &two[*i]).collect();
        assert_eq!(got, want, "{counters}");
    }
    // Off the battlefield none applies, however many counters.
    for zone in ZoneKind::ALL {
        if zone != ZoneKind::Battlefield {
            assert!(active_station_symbols(&two, zone, 100).unwrap().is_empty());
            assert!(active_station_symbols(&one, zone, 100).unwrap().is_empty());
        }
    }
    // Negative counters never activate, even a non-positive threshold.
    let low = [symbol(0, "zero", None), symbol(-3, "neg", None)];
    assert!(
        active_station_symbols(&low, ZoneKind::Battlefield, -1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        active_station_symbols(&low, ZoneKind::Battlefield, 0)
            .unwrap()
            .len(),
        2
    );
    // CR 722.2b: a P/T box makes the permanent a creature.
    assert!(two[1].grants_creature());
    assert!(!two[0].grants_creature());
}

// ---------------------------------------------------------------- PREPARATION (CR 723)

/// A preparation card: creature `{X}{2}{G}` 3/3, prepare spell sorcery `{1}{G}`.
fn preparation() -> CardDefinitionDSL {
    def(
        "test.prepare_card",
        LayoutBehavior::Preparation,
        vec![
            with_pt(
                face(FaceKind::Main, "normal", Some("{X}{2}{G}"), &["Creature"]),
                3,
                3,
            ),
            face(
                FaceKind::PrepareSpell,
                "prepare",
                Some("{1}{G}"),
                &["Sorcery"],
            ),
        ],
    )
}

// REQ-OBJ-002, CR 723.4, 723.3: normal characteristics in every zone, the stack and the
// battlefield included; never the prepare spell.
#[test]
fn preparation_normal_everywhere() {
    for swap in [false, true] {
        let mut d = preparation();
        if swap {
            d.faces.reverse();
        }
        let want = printed(&d, FaceKind::Main);
        for zone in ZoneKind::ALL {
            for cast in CASTS {
                for status in statuses() {
                    let q = LayoutQuery::new(zone).with_cast(cast).with_status(status);
                    assert_eq!(
                        PreparationLayout
                            .base_characteristics(&d, &q, vocab())
                            .unwrap(),
                        want
                    );
                }
            }
        }
        let mv = |zone| PreparationLayout.mana_value(&d, &LayoutQuery::new(zone), on(2), vocab());
        assert_eq!(mv(ZoneKind::Stack).unwrap(), 5);
        assert_eq!(mv(ZoneKind::Battlefield).unwrap(), 3);
        assert_eq!(PreparationLayout.gating(&d).unwrap(), None);
    }
    assert_eq!(PreparationLayout.behavior(), LayoutBehavior::Preparation);
    assert_eq!(PREPARED_DESIGNATION, "prepared");
}

// REQ-OBJ-002, CR 723.3a, 723.3c: the prepare spell's characteristics and becoming prepared.
#[test]
fn preparation_prepare_spell_and_become_prepared() {
    let d = preparation();
    let want = printed(&d, FaceKind::PrepareSpell);
    assert_eq!(prepare_spell_characteristics(&d, vocab()).unwrap(), want);
    assert_eq!(
        become_prepared(&d, ZoneKind::Battlefield, false, vocab()).unwrap(),
        want
    );
    for zone in ZoneKind::ALL {
        if zone == ZoneKind::Battlefield {
            continue;
        }
        let e = LayoutError::InvalidCast {
            layout: LayoutBehavior::Preparation,
            zone,
        };
        // The zone check precedes the already-prepared check.
        for already in [false, true] {
            assert_eq!(become_prepared(&d, zone, already, vocab()).unwrap_err(), e);
        }
    }
    // CR 723.3a: not again while already prepared.
    assert_eq!(
        become_prepared(&d, ZoneKind::Battlefield, true, vocab()).unwrap_err(),
        LayoutError::AlreadyPrepared
    );
}

// REQ-OBJ-002, CR 723.3a: preparation errors in precedence order.
#[test]
fn preparation_errors() {
    let q = LayoutQuery::new(ZoneKind::Battlefield);
    let normal = def(
        "test.normal_card",
        LayoutBehavior::Normal,
        vec![face(FaceKind::Main, "normal", Some("{1}"), &["Creature"])],
    );
    let e = mismatch(LayoutBehavior::Preparation, LayoutBehavior::Normal);
    assert_eq!(
        become_prepared(&normal, ZoneKind::Hand, true, vocab()).unwrap_err(),
        e
    );
    assert_eq!(
        prepare_spell_characteristics(&normal, vocab()).unwrap_err(),
        e
    );
    assert_eq!(
        PreparationLayout
            .base_characteristics(&normal, &q, vocab())
            .unwrap_err(),
        e
    );
    assert_eq!(PreparationLayout.gating(&normal).unwrap_err(), e);
    let mut missing = preparation();
    missing.faces.pop();
    let bf = bad_faces(LayoutBehavior::Preparation);
    assert_eq!(
        become_prepared(&missing, ZoneKind::Hand, true, vocab()).unwrap_err(),
        bf
    );
    assert_eq!(
        prepare_spell_characteristics(&missing, vocab()).unwrap_err(),
        bf
    );
    assert_eq!(
        PreparationLayout
            .mana_value(&missing, &q, OFF, vocab())
            .unwrap_err(),
        bf
    );
    assert_eq!(PreparationLayout.gating(&missing).unwrap_err(), bf);
    // Only the selected face is type-checked.
    let mut bad_prepare = preparation();
    bad_prepare.faces[1] = bad_types(bad_prepare.faces[1].clone());
    assert!(
        PreparationLayout
            .base_characteristics(&bad_prepare, &q, vocab())
            .is_ok()
    );
    assert_eq!(
        prepare_spell_characteristics(&bad_prepare, vocab()).unwrap_err(),
        object_error()
    );
    assert_eq!(
        become_prepared(&bad_prepare, ZoneKind::Battlefield, false, vocab()).unwrap_err(),
        object_error()
    );
    assert_eq!(
        become_prepared(&bad_prepare, ZoneKind::Battlefield, true, vocab()).unwrap_err(),
        LayoutError::AlreadyPrepared
    );
    let mut bad_main = preparation();
    bad_main.faces[0] = bad_types(bad_main.faces[0].clone());
    assert_eq!(
        PreparationLayout
            .mana_value(&bad_main, &q, OFF, vocab())
            .unwrap_err(),
        object_error()
    );
    assert!(become_prepared(&bad_main, ZoneKind::Battlefield, false, vocab()).is_ok());
}

// ---------------------------------------------------------------- MUTATE / MERGED (CR 731)

fn mutate_card(tag: &str, c: &str, n: i64) -> CardDefinitionDSL {
    def(
        &format!("test.{tag}"),
        LayoutBehavior::Mutate,
        vec![with_pt(
            face(FaceKind::Main, tag, Some(c), &["Creature"]),
            n,
            n,
        )],
    )
}

fn normal_card(tag: &str, c: &str, n: i64) -> CardDefinitionDSL {
    def(
        &format!("test.{tag}"),
        LayoutBehavior::Normal,
        vec![with_pt(
            face(FaceKind::Main, tag, Some(c), &["Creature"]),
            n,
            n,
        )],
    )
}

fn comp(d: &CardDefinitionDSL, is_token: bool, face_up: bool) -> MergedComponent<'_> {
    MergedComponent {
        def: d,
        query: LayoutQuery::new(ZoneKind::Battlefield),
        is_token,
        face_up,
    }
}

fn ids(components: &[MergedComponent<'_>]) -> Vec<String> {
    components
        .iter()
        .map(|c| c.def.card_def_id.clone())
        .collect()
}

// REQ-OBJ-002, CR 702.140c, 202.3: a mutate card alone has its printed characteristics.
#[test]
fn mutate_layout_printed() {
    let d = mutate_card("mutant", "{X}{1}{G}", 4);
    for zone in ZoneKind::ALL {
        let q = LayoutQuery::new(zone);
        assert_eq!(
            MutateLayout.base_characteristics(&d, &q, vocab()).unwrap(),
            printed(&d, FaceKind::Main)
        );
    }
    let mv = |zone| MutateLayout.mana_value(&d, &LayoutQuery::new(zone), on(3), vocab());
    assert_eq!(mv(ZoneKind::Stack).unwrap(), 5);
    assert_eq!(mv(ZoneKind::Hand).unwrap(), 2);
    assert_eq!(MutateLayout.gating(&d).unwrap(), None);
    assert_eq!(MutateLayout.behavior(), LayoutBehavior::Mutate);
    let n = normal_card("plain", "{1}", 1);
    let e = mismatch(LayoutBehavior::Mutate, LayoutBehavior::Normal);
    assert_eq!(MutateLayout.gating(&n).unwrap_err(), e);
    let mut two = d.clone();
    two.faces.push(two.faces[0].clone());
    assert_eq!(
        MutateLayout
            .base_characteristics(&two, &LayoutQuery::new(ZoneKind::Hand), vocab())
            .unwrap_err(),
        bad_faces(LayoutBehavior::Mutate)
    );
}

// REQ-OBJ-002, CR 731.2: a merged permanent needs at least two components.
#[test]
fn merged_new_counts() {
    let a = mutate_card("top", "{1}{G}", 4);
    let b = normal_card("bottom", "{3}", 2);
    for n in [0usize, 1] {
        let comps: Vec<_> = [comp(&a, false, true)].into_iter().take(n).collect();
        assert_eq!(
            MergedPermanent::new(comps, true).unwrap_err(),
            LayoutError::TooFewComponents { count: n }
        );
    }
    let p = MergedPermanent::new(vec![comp(&a, false, true), comp(&b, false, true)], true).unwrap();
    assert_eq!(ids(p.components()), ["test.top", "test.bottom"]);
    assert_eq!(p.object_count(), 1);
}

// REQ-OBJ-002, CR 731.2a, 707.2, 702.140e: the top supplies the characteristics; a mutated
// permanent has every component's abilities, top to bottom, without deduplication.
#[test]
fn merged_top_supplies_characteristics() {
    let a = mutate_card("top", "{X}{1}{G}", 4);
    let b = normal_card("bottom", "{3}", 2);
    let top = printed(&a, FaceKind::Main);
    for mutated in [false, true] {
        let p = MergedPermanent::new(vec![comp(&a, false, true), comp(&b, false, true)], mutated)
            .unwrap();
        let c = p.base_characteristics(vocab()).unwrap();
        let want = if mutated {
            abilities(&["top", "bottom"])
        } else {
            abilities(&["top"])
        };
        assert_eq!(c.abilities, want);
        // Every other field, rules text included, is the top's.
        assert_eq!(
            Characteristics {
                abilities: top.abilities.clone(),
                ..c
            },
            top
        );
        // CR 202.3: the top's mana value on the battlefield, X counts 0.
        assert_eq!(p.mana_value(vocab()).unwrap(), 2);
    }
    let p = MergedPermanent::new(vec![comp(&a, false, true), comp(&a, true, true)], true).unwrap();
    assert_eq!(
        p.base_characteristics(vocab()).unwrap().abilities,
        abilities(&["top", "top"])
    );
}

// REQ-OBJ-002, CR 702.140c, 731.2, 731.2d, 731.2e, 731.3: merging over and under, in both
// orders; token and face-up status come from the top.
#[test]
fn merged_merge_over_and_under() {
    let a = mutate_card("a", "{1}", 1);
    let b = normal_card("b", "{2}", 2);
    let c = mutate_card("c", "{3}", 3);
    let base =
        MergedPermanent::new(vec![comp(&a, true, false), comp(&b, false, true)], true).unwrap();
    assert!(base.is_token());
    assert!(!base.face_up());
    let over = base.merge(comp(&c, false, true), MergeSide::Over);
    assert_eq!(ids(over.components()), ["test.c", "test.a", "test.b"]);
    assert!(!over.is_token());
    assert!(over.face_up());
    assert_eq!(over.mana_value(vocab()).unwrap(), 3);
    let under = base.merge(comp(&c, false, true), MergeSide::Under);
    assert_eq!(ids(under.components()), ["test.a", "test.b", "test.c"]);
    assert!(under.is_token());
    assert!(!under.face_up());
    // `under` has a face-down top: its characteristics, mana value and abilities belong to
    // layer 1b (CR 708.2) and are deferred to #39 FP-038, so they are not asserted here.
    // The original is unchanged.
    assert_eq!(ids(base.components()), ["test.a", "test.b"]);
    // Over then under, and under then over.
    let ou = base
        .merge(comp(&c, false, true), MergeSide::Over)
        .merge(comp(&b, false, true), MergeSide::Under);
    assert_eq!(
        ids(ou.components()),
        ["test.c", "test.a", "test.b", "test.b"]
    );
    let uo = base
        .merge(comp(&b, false, true), MergeSide::Under)
        .merge(comp(&c, false, true), MergeSide::Over);
    assert_eq!(ids(uo.components()), ids(ou.components()));
    // CR 731.3: every component leaves, top to bottom.
    assert_eq!(ids(&over.components_leaving()), ids(over.components()));
}

/// REQ-OBJ-002, CR 731.2e, 731.2d: a face-up top over a face-down lower component: the
/// permanent is face up and its token status follows the top, before and after merging under
/// and over. The face-down component's characteristics and abilities are deferred to layer 1b
/// (#39 FP-038, CR 708.2) and are not asserted here.
#[test]
fn merged_face_up_top_over_face_down_lower() {
    let a = mutate_card("a", "{1}", 1);
    let b = normal_card("b", "{2}", 2);
    let c = mutate_card("c", "{3}", 3);
    let p = MergedPermanent::new(vec![comp(&a, true, true), comp(&b, false, false)], true).unwrap();
    assert_eq!(ids(p.components()), ["test.a", "test.b"]);
    assert!(p.face_up());
    assert!(p.is_token());
    // CR 731.2e: a face-down component merged under leaves the face-up top in place.
    let under = p.merge(comp(&c, false, false), MergeSide::Under);
    assert_eq!(ids(under.components()), ["test.a", "test.b", "test.c"]);
    assert!(under.face_up());
    assert!(under.is_token());
    // A face-up non-token merged over becomes the top (CR 731.2d, 731.2e).
    let over = under.merge(comp(&c, false, true), MergeSide::Over);
    assert_eq!(
        ids(over.components()),
        ["test.c", "test.a", "test.b", "test.c"]
    );
    assert!(over.face_up());
    assert!(!over.is_token());
    // The original is unchanged.
    assert_eq!(ids(p.components()), ["test.a", "test.b"]);
    assert!(p.face_up());
}

// REQ-OBJ-002, CR 731.2a, 722.2c, 713.5b, 723.4: each component uses its own layout, always on
// the battlefield (the component query's zone is ignored).
#[test]
fn merged_component_layouts() {
    let st = station();
    let prep = preparation();
    let meld = meld_a();
    let bottom = mutate_card("bottom", "{1}", 1);
    for top in [&st, &prep, &meld] {
        let mut t = comp(top, false, true);
        t.query = LayoutQuery::new(ZoneKind::Hand).with_cast(CastChoice::Fused);
        let p = MergedPermanent::new(vec![t, comp(&bottom, false, true)], false).unwrap();
        assert_eq!(
            p.base_characteristics(vocab()).unwrap(),
            printed(top, FaceKind::Main)
        );
    }
    // The top's X counts 0 even if its query says stack.
    let x = mutate_card("x", "{X}{1}", 2);
    let mut t = comp(&x, false, true);
    t.query = LayoutQuery::new(ZoneKind::Stack);
    let p = MergedPermanent::new(vec![t, comp(&bottom, false, true)], true).unwrap();
    assert_eq!(p.mana_value(vocab()).unwrap(), 1);
}

// REQ-OBJ-002, CR 731.2a, 702.140e: errors come from the top, then (mutated only) from the
// other components, top to bottom.
#[test]
fn merged_errors() {
    let good = mutate_card("good", "{1}", 1);
    let room_def = room(false);
    let mut bad = mutate_card("bad", "{1}", 1);
    bad.faces[0] = bad_types(bad.faces[0].clone());
    let unsupported = LayoutError::UnsupportedLayout(LayoutBehavior::Room);
    for mutated in [false, true] {
        let p = MergedPermanent::new(
            vec![comp(&room_def, false, true), comp(&good, false, true)],
            mutated,
        )
        .unwrap();
        assert_eq!(p.base_characteristics(vocab()).unwrap_err(), unsupported);
        assert_eq!(p.mana_value(vocab()).unwrap_err(), unsupported);
        let p = MergedPermanent::new(
            vec![comp(&bad, false, true), comp(&room_def, false, true)],
            mutated,
        )
        .unwrap();
        assert_eq!(p.base_characteristics(vocab()).unwrap_err(), object_error());
        assert_eq!(p.mana_value(vocab()).unwrap_err(), object_error());
    }
    let lower = vec![
        comp(&good, false, true),
        comp(&room_def, false, true),
        comp(&bad, false, true),
    ];
    let p = MergedPermanent::new(lower.clone(), false).unwrap();
    assert!(p.base_characteristics(vocab()).is_ok());
    assert_eq!(p.mana_value(vocab()).unwrap(), 1);
    let p = MergedPermanent::new(lower, true).unwrap();
    assert_eq!(p.base_characteristics(vocab()).unwrap_err(), unsupported);
    assert_eq!(p.mana_value(vocab()).unwrap_err(), unsupported);
    let p = MergedPermanent::new(
        vec![
            comp(&good, false, true),
            comp(&bad, false, true),
            comp(&room_def, false, true),
        ],
        true,
    )
    .unwrap();
    assert_eq!(p.base_characteristics(vocab()).unwrap_err(), object_error());
    assert_eq!(p.mana_value(vocab()).unwrap_err(), object_error());
}

// REQ-OBJ-002: sweep over zones x query shapes for the four Layout impls: no panic, same
// result twice, and mana_value fails exactly when base_characteristics does.
#[test]
fn composite_layout_sweep_deterministic() {
    let mut defs = vec![
        meld_a(),
        meld_b(),
        station(),
        preparation(),
        mutate_card("m", "{X}", 1),
    ];
    let mut broken = Vec::new();
    for d in &defs {
        let mut b = d.clone();
        b.faces[0] = bad_types(b.faces[0].clone());
        broken.push(b);
        let mut e = d.clone();
        e.faces.clear();
        broken.push(e);
    }
    defs.extend(broken);
    let layouts: [&dyn Layout; 4] = [
        &MeldLayout,
        &StationLayout,
        &PreparationLayout,
        &MutateLayout,
    ];
    for layout in layouts {
        for d in &defs {
            for zone in ZoneKind::ALL {
                for cast in CASTS {
                    for status in statuses() {
                        let q = LayoutQuery::new(zone).with_cast(cast).with_status(status);
                        let a = layout.base_characteristics(d, &q, vocab());
                        assert_eq!(a, layout.base_characteristics(d, &q, vocab()));
                        let m = layout.mana_value(d, &q, on(1), vocab());
                        assert_eq!(m, layout.mana_value(d, &q, on(1), vocab()));
                        assert_eq!(a.as_ref().err(), m.as_ref().err());
                    }
                }
            }
            assert_eq!(layout.gating(d), layout.gating(d));
        }
    }
}
