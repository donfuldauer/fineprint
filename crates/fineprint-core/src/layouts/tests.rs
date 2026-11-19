// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests of base-characteristic selection per layout (REQ-OBJ-002, REQ-OBJ-001,
//! REQ-NFR-008). Synthetic data only (R5).

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

/// The twelve supported layouts.
const SUPPORTED: [LayoutBehavior; 12] = [
    LayoutBehavior::Normal,
    LayoutBehavior::Split,
    LayoutBehavior::Flip,
    LayoutBehavior::DfcNonmodal,
    LayoutBehavior::DfcModal,
    LayoutBehavior::Adventurer,
    LayoutBehavior::Omen,
    LayoutBehavior::Prototype,
    LayoutBehavior::Leveler,
    LayoutBehavior::Saga,
    LayoutBehavior::Class,
    LayoutBehavior::Case,
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

/// The zones other than the stack and the battlefield.
const HIDDEN_OR_OTHER: [ZoneKind; 5] = [
    ZoneKind::Library,
    ZoneKind::Hand,
    ZoneKind::Graveyard,
    ZoneKind::Exile,
    ZoneKind::Command,
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

fn mask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).unwrap()
}

fn subtype_of(card_type: &str, n: usize) -> String {
    vocab().subtypes_of(card_type).nth(n).unwrap().to_owned()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn status(flipped: bool, face_up: FaceUp, copying_back_face: bool) -> LayoutStatus {
    LayoutStatus {
        flipped,
        face_up,
        copying_back_face,
    }
}

/// Every status combination.
fn statuses() -> Vec<LayoutStatus> {
    let mut out = Vec::new();
    for flipped in [false, true] {
        for face_up in [FaceUp::Front, FaceUp::Back] {
            for copying in [false, true] {
                out.push(status(flipped, face_up, copying));
            }
        }
    }
    out
}

/// A synthetic face whose text and abilities are distinguishable by `tag`.
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
        text: TextTokenStream(vec![format!("test.text.{tag}").into()]),
        abilities: vec![AbilityAST(format!("test.ability.{tag}").into())],
    }
}

fn with_pt(mut f: CardFaceDSL, p: i64, t: i64) -> CardFaceDSL {
    f.power = Some(ValueExpressionAST(p.into()));
    f.toughness = Some(ValueExpressionAST(t.into()));
    f
}

fn def(layout: LayoutBehavior, faces: Vec<CardFaceDSL>) -> CardDefinitionDSL {
    CardDefinitionDSL {
        card_def_id: "test.layout_card".to_owned(),
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

/// The face kinds each supported layout requires.
fn kinds(layout: LayoutBehavior) -> Vec<FaceKind> {
    match layout {
        LayoutBehavior::Split => vec![FaceKind::LeftHalf, FaceKind::RightHalf],
        LayoutBehavior::Flip => vec![FaceKind::Main, FaceKind::Flipped],
        LayoutBehavior::DfcNonmodal | LayoutBehavior::DfcModal => {
            vec![FaceKind::Main, FaceKind::Back]
        }
        LayoutBehavior::Adventurer => vec![FaceKind::Main, FaceKind::Adventure],
        LayoutBehavior::Omen => vec![FaceKind::Main, FaceKind::Omen],
        LayoutBehavior::Prototype => vec![FaceKind::Main, FaceKind::Prototype],
        _ => vec![FaceKind::Main],
    }
}

/// A valid definition of `layout`: first face a 4/4 creature `{X}{1}{G}`, second face a 1/1
/// creature `{2}{R}` (for split: an instant `{X}{1}{G}` and a sorcery `{2}{R}`).
fn valid(layout: LayoutBehavior) -> CardDefinitionDSL {
    let split = layout == LayoutBehavior::Split;
    let faces = kinds(layout)
        .into_iter()
        .enumerate()
        .map(|(i, k)| {
            let first = i == 0;
            let (tag, c) = if first {
                ("first", "{X}{1}{G}")
            } else {
                ("second", "{2}{R}")
            };
            let types: &[&str] = match (split, first) {
                (true, true) => &["Instant"],
                (true, false) => &["Sorcery"],
                (false, _) => &["Creature"],
            };
            let f = face(k, tag, Some(c), types);
            match (split, first) {
                (true, _) => f,
                (false, true) => with_pt(f, 4, 4),
                (false, false) => with_pt(f, 1, 1),
            }
        })
        .collect();
    def(layout, faces)
}

fn face_of(d: &CardDefinitionDSL, kind: FaceKind) -> &CardFaceDSL {
    d.faces.iter().find(|f| f.face_kind == kind).unwrap()
}

fn printed(d: &CardDefinitionDSL, kind: FaceKind) -> Characteristics {
    Characteristics::from_face(face_of(d, kind), vocab()).unwrap()
}

fn q(zone: ZoneKind) -> LayoutQuery {
    LayoutQuery::new(zone)
}

fn base(d: &CardDefinitionDSL, query: LayoutQuery) -> Result<Characteristics, LayoutError> {
    base_characteristics(d, &query, vocab())
}

fn mv(
    d: &CardDefinitionDSL,
    query: LayoutQuery,
    ctx: ManaValueContext,
) -> Result<i64, LayoutError> {
    mana_value(d, &query, ctx, vocab())
}

fn invalid(layout: LayoutBehavior, zone: ZoneKind) -> LayoutError {
    LayoutError::InvalidCast { layout, zone }
}

fn non_stack_zones() -> impl Iterator<Item = ZoneKind> {
    ZoneKind::ALL.into_iter().filter(|z| *z != ZoneKind::Stack)
}

// ---------------------------------------------------------------- API surface

// REQ-OBJ-002: the query builder and the default status.
#[test]
fn query_builder_and_default_status() {
    let s = LayoutStatus::default();
    assert_eq!(s, status(false, FaceUp::Front, false));
    let query = LayoutQuery::new(ZoneKind::Hand);
    assert_eq!(query.zone, ZoneKind::Hand);
    assert_eq!(query.cast, CastChoice::None);
    assert_eq!(query.status, s);
    let other = status(true, FaceUp::Back, true);
    let query = query.with_cast(CastChoice::Fused).with_status(other);
    assert_eq!(query.cast, CastChoice::Fused);
    assert_eq!(query.status, other);
    assert_eq!(query.zone, ZoneKind::Hand);
}

// REQ-OBJ-002: layout_for maps each of the twelve layouts to its own object.
#[test]
fn layout_for_supported() {
    for b in SUPPORTED {
        assert_eq!(layout_for(b).unwrap().behavior(), b);
    }
}

// REQ-OBJ-002: every other behavior, ROOM and MELD included, is UnsupportedLayout, from
// layout_for and from the free functions.
#[test]
fn layout_for_unsupported_exhaustive() {
    let mut unsupported = 0;
    for b in LayoutBehavior::ALL {
        if SUPPORTED.contains(&b) {
            continue;
        }
        unsupported += 1;
        assert_eq!(layout_for(b).err(), Some(LayoutError::UnsupportedLayout(b)));
        let d = def(b, vec![face(FaceKind::Main, "u", None, &["Creature"])]);
        let e = LayoutError::UnsupportedLayout(b);
        assert_eq!(base(&d, q(ZoneKind::Hand)), Err(e.clone()));
        assert_eq!(mv(&d, q(ZoneKind::Hand), OFF), Err(e.clone()));
        assert_eq!(gating(&d), Err(e));
    }
    assert_eq!(unsupported, LayoutBehavior::ALL.len() - SUPPORTED.len());
    assert!(layout_for(LayoutBehavior::Room).is_err());
    assert!(layout_for(LayoutBehavior::Meld).is_err());
}

// REQ-OBJ-002: a layout object given another layout's definition is LayoutMismatch, before
// any face or cast check.
#[test]
fn layout_mismatch_every_pair() {
    for expected in SUPPORTED {
        let layout = layout_for(expected).unwrap();
        for found in LayoutBehavior::ALL {
            if found == expected {
                continue;
            }
            // Faces deliberately wrong too: mismatch takes precedence over BadFaces.
            let d = def(found, Vec::new());
            let e = LayoutError::LayoutMismatch { expected, found };
            let query = q(ZoneKind::Stack).with_cast(CastChoice::Omen);
            assert_eq!(
                layout.base_characteristics(&d, &query, vocab()),
                Err(e.clone())
            );
            assert_eq!(layout.mana_value(&d, &query, OFF, vocab()), Err(e.clone()));
            assert_eq!(layout.gating(&d), Err(e));
        }
    }
}

// REQ-OBJ-002: faces must be exactly the required kinds, each once, in any order; BadFaces
// takes precedence over InvalidCast.
#[test]
fn bad_faces_every_layout() {
    for layout in SUPPORTED {
        let good = valid(layout);
        let e = LayoutError::BadFaces { layout };
        let mut shapes: Vec<Vec<CardFaceDSL>> = vec![Vec::new()];
        // One face missing (or the only face replaced by a wrong kind).
        let mut missing = good.faces.clone();
        missing.pop();
        if missing.is_empty() {
            missing.push(face(FaceKind::Back, "wrong", None, &["Creature"]));
        }
        shapes.push(missing);
        // An extra face of an unrelated kind.
        let mut extra = good.faces.clone();
        extra.push(face(FaceKind::MeldResult, "extra", None, &["Creature"]));
        shapes.push(extra);
        // A duplicated required kind.
        let mut dup = good.faces.clone();
        dup.push(good.faces[0].clone());
        shapes.push(dup);
        // Same count, one kind duplicated in place of another.
        if good.faces.len() == 2 {
            shapes.push(vec![good.faces[0].clone(), good.faces[0].clone()]);
            shapes.push(vec![good.faces[1].clone(), good.faces[1].clone()]);
        }
        // Same count, every kind wrong.
        shapes.push(
            good.faces
                .iter()
                .map(|f| CardFaceDSL {
                    face_kind: FaceKind::PrepareSpell,
                    ..f.clone()
                })
                .collect(),
        );
        for faces in shapes {
            let d = def(layout, faces);
            for zone in ZoneKind::ALL {
                for cast in CASTS {
                    let query = q(zone).with_cast(cast);
                    assert_eq!(base(&d, query), Err(e.clone()), "{layout} {zone:?}");
                    assert_eq!(mv(&d, query, OFF), Err(e.clone()));
                }
            }
            assert_eq!(gating(&d), Err(e.clone()));
        }
        // Reversed order is accepted: faces are looked up by kind, never by index.
        let mut reversed = good.clone();
        reversed.faces.reverse();
        for zone in ZoneKind::ALL {
            for cast in CASTS {
                let query = q(zone).with_cast(cast);
                assert_eq!(base(&reversed, query), base(&good, query));
                assert_eq!(mv(&reversed, query, on(1)), mv(&good, query, on(1)));
            }
        }
        assert_eq!(gating(&reversed), gating(&good));
    }
}

// REQ-OBJ-002: InvalidCast takes precedence over a type-line error, BadFaces over
// InvalidCast.
#[test]
fn error_precedence() {
    let mut d = valid(LayoutBehavior::Split);
    for f in &mut d.faces {
        f.card_types = vec!["test.not_a_type".to_owned()];
    }
    let stack = q(ZoneKind::Stack);
    let object = LayoutError::Object(ObjectError::UnknownCardType("test.not_a_type".to_owned()));
    assert_eq!(
        base(&d, stack),
        Err(invalid(LayoutBehavior::Split, ZoneKind::Stack))
    );
    assert_eq!(base(&d, q(ZoneKind::Hand)), Err(object.clone()));
    assert_eq!(mv(&d, q(ZoneKind::Hand), OFF), Err(object));
    d.faces.pop();
    assert_eq!(
        base(&d, stack),
        Err(LayoutError::BadFaces {
            layout: LayoutBehavior::Split
        })
    );
}

// REQ-OBJ-002, CR 205.4a: a type-line error of a selected face is LayoutError::Object, for
// every layout, from both base characteristics and mana value.
#[test]
fn object_error_every_layout() {
    for layout in SUPPORTED {
        let mut d = valid(layout);
        for f in &mut d.faces {
            f.supertypes = vec!["test.not_a_supertype".to_owned()];
        }
        let e = LayoutError::Object(ObjectError::UnknownSupertype(
            "test.not_a_supertype".to_owned(),
        ));
        assert_eq!(base(&d, q(ZoneKind::Hand)), Err(e.clone()), "{layout}");
        assert_eq!(mv(&d, q(ZoneKind::Hand), OFF), Err(e), "{layout}");
    }
}

// REQ-OBJ-002: LayoutError converts from ObjectError, displays, and exposes its source.
#[test]
fn layout_error_display_and_source() {
    use std::error::Error as _;
    let obj = ObjectError::UnknownCardType("test.t".to_owned());
    let e: LayoutError = obj.clone().into();
    assert_eq!(e, LayoutError::Object(obj.clone()));
    assert_eq!(e.to_string(), obj.to_string());
    assert!(e.source().is_some());
    let others = [
        LayoutError::UnsupportedLayout(LayoutBehavior::Room),
        LayoutError::LayoutMismatch {
            expected: LayoutBehavior::Split,
            found: LayoutBehavior::Normal,
        },
        LayoutError::BadFaces {
            layout: LayoutBehavior::Flip,
        },
        invalid(LayoutBehavior::DfcModal, ZoneKind::Stack),
    ];
    for e in others {
        assert!(!e.to_string().is_empty());
        assert!(e.source().is_none());
    }
}

// ---------------------------------------------------------------- printed layouts

// REQ-OBJ-002, CR 711.5, 711.6, 715.2, 717.2b, 720.3b: NORMAL, LEVELER, SAGA, CLASS and CASE
// have their printed characteristics in every zone; cast and status are ignored.
#[test]
fn printed_layouts_every_zone_cast_status() {
    for layout in [
        LayoutBehavior::Normal,
        LayoutBehavior::Leveler,
        LayoutBehavior::Saga,
        LayoutBehavior::Class,
        LayoutBehavior::Case,
    ] {
        let d = valid(layout);
        let main = printed(&d, FaceKind::Main);
        for zone in ZoneKind::ALL {
            for cast in CASTS {
                for s in statuses() {
                    let query = q(zone).with_cast(cast).with_status(s);
                    assert_eq!(base(&d, query).unwrap(), main, "{layout} {zone:?} {cast:?}");
                    // CR 202.3, 202.3e: {X}{1}{G} is 2 with X = 0; with X = 3 it is 5 on the
                    // stack only, 2 in every other zone (CR 107.3g).
                    assert_eq!(mv(&d, query, OFF), Ok(2));
                    let with_x = if zone == ZoneKind::Stack { 5 } else { 2 };
                    assert_eq!(mv(&d, query, on(3)), Ok(with_x));
                }
            }
        }
    }
}

// REQ-OBJ-002, CR 711.5, 711.6, 711.2a, 711.2b: a leveler carries its uppermost P/T in every
// zone (bands are layer-7b static abilities, never applied here); gating exposes the uppermost
// P/T and the printed abilities in order.
#[test]
fn leveler_uppermost_pt_and_gating() {
    let mut d = valid(LayoutBehavior::Leveler);
    d.faces[0].abilities = vec![
        AbilityAST("test.level_up".into()),
        AbilityAST("test.band_1".into()),
        AbilityAST("test.band_2".into()),
    ];
    for zone in ZoneKind::ALL {
        let c = base(&d, q(zone)).unwrap();
        assert_eq!(c.power, Some(ValueExpressionAST(4.into())));
        assert_eq!(c.toughness, Some(ValueExpressionAST(4.into())));
        assert_eq!(c.abilities, d.faces[0].abilities);
    }
    assert_eq!(
        gating(&d),
        Ok(Some(Gating::Leveler {
            uppermost_power: Some(ValueExpressionAST(4.into())),
            uppermost_toughness: Some(ValueExpressionAST(4.into())),
            abilities: d.faces[0].abilities.clone(),
        }))
    );
}

// REQ-OBJ-002, CR 715.2, 717.2a, 720.3b: Saga, Class and Case gating is the printed abilities
// in definition order.
#[test]
fn saga_class_case_gating() {
    for layout in [
        LayoutBehavior::Saga,
        LayoutBehavior::Class,
        LayoutBehavior::Case,
    ] {
        let mut d = valid(layout);
        d.faces[0].abilities = vec![
            AbilityAST("test.second".into()),
            AbilityAST("test.first".into()),
        ];
        let abilities = d.faces[0].abilities.clone();
        let expected = match layout {
            LayoutBehavior::Saga => Gating::Saga { abilities },
            LayoutBehavior::Class => Gating::Class { abilities },
            _ => Gating::Case { abilities },
        };
        assert_eq!(gating(&d), Ok(Some(expected)), "{layout}");
        assert_eq!(layout_for(layout).unwrap().gating(&d), gating(&d));
    }
}

// REQ-OBJ-002: gating is None for every layout other than leveler, Saga, Class and Case.
#[test]
fn gating_none_for_other_layouts() {
    for layout in [
        LayoutBehavior::Normal,
        LayoutBehavior::Split,
        LayoutBehavior::Flip,
        LayoutBehavior::DfcNonmodal,
        LayoutBehavior::DfcModal,
        LayoutBehavior::Adventurer,
        LayoutBehavior::Omen,
        LayoutBehavior::Prototype,
    ] {
        assert_eq!(gating(&valid(layout)), Ok(None), "{layout}");
    }
}

// ---------------------------------------------------------------- SPLIT

fn split_def() -> CardDefinitionDSL {
    let mut left = face(
        FaceKind::LeftHalf,
        "split_a",
        Some("{X}{1}{G}"),
        &["Instant"],
    );
    left.subtypes = vec![subtype_of("Instant", 0)];
    left.supertypes = vec!["Legendary".to_owned()];
    left.color_indicator = Some(ColorBitmask::WHITE);
    let mut right = face(FaceKind::RightHalf, "split_b", Some("{2}{R}"), &["Sorcery"]);
    right.color_indicator = Some(ColorBitmask::BLUE);
    def(LayoutBehavior::Split, vec![right, left])
}

// REQ-OBJ-002, CR 709.4, 709.4a, 709.4b, 709.4c, 202.3d: off the stack (the battlefield
// included, CR 304.4, 307.4) a split card has both halves combined, whatever the cast choice.
#[test]
fn split_off_stack_combined() {
    let d = split_def();
    let l = printed(&d, FaceKind::LeftHalf);
    let r = printed(&d, FaceKind::RightHalf);
    for zone in non_stack_zones() {
        for cast in CASTS {
            let c = base(&d, q(zone).with_cast(cast)).unwrap();
            assert_eq!(c.names, set(&["test.split_a", "test.split_b"]));
            assert_eq!(c.mana_cost, Some(cost("{X}{1}{G}{2}{R}")));
            assert_eq!(c.color_indicator, Some(mask(1 | 2)));
            // G and R from the cost, W and U from the indicators.
            assert_eq!(c.colors, mask(1 | 2 | 8 | 16));
            assert_eq!(c.supertypes, set(&["Legendary"]));
            assert_eq!(c.card_types, set(&["Instant", "Sorcery"]));
            assert_eq!(c.subtypes, l.subtypes);
            let mut text = l.rules_text.0.clone();
            text.extend(r.rules_text.0.clone());
            assert_eq!(c.rules_text, TextTokenStream(text));
            let mut abilities = l.abilities.clone();
            abilities.extend(r.abilities.clone());
            assert_eq!(c.abilities, abilities);
            assert_eq!(c.power, None);
            assert_eq!(c.toughness, None);
            assert_eq!(c.loyalty, None);
            assert_eq!(c.defense, None);
            assert_eq!(c.hand_modifier, None);
            assert_eq!(c.life_modifier, None);
            // CR 202.3d, 202.3e: combined, X is 0 off the stack even with an OnStack context.
            assert_eq!(mv(&d, q(zone).with_cast(cast), OFF), Ok(5));
            assert_eq!(mv(&d, q(zone).with_cast(cast), on(7)), Ok(5));
        }
    }
}

// REQ-OBJ-002, CR 709.3b, 202.3d: on the stack only the half cast exists; its own cost, with
// X counted at its announced value.
#[test]
fn split_stack_half() {
    let d = split_def();
    let stack = q(ZoneKind::Stack);
    let left = stack.with_cast(CastChoice::SplitHalf(SplitHalf::Left));
    let right = stack.with_cast(CastChoice::SplitHalf(SplitHalf::Right));
    assert_eq!(base(&d, left).unwrap(), printed(&d, FaceKind::LeftHalf));
    assert_eq!(base(&d, right).unwrap(), printed(&d, FaceKind::RightHalf));
    assert_eq!(mv(&d, left, on(4)), Ok(6));
    assert_eq!(mv(&d, left, OFF), Ok(2));
    assert_eq!(mv(&d, right, on(4)), Ok(3));
}

// REQ-OBJ-002, CR 709.4d, 702.102b, 202.3d: a fused split spell has both halves combined and
// the combined mana value, X counted.
#[test]
fn split_stack_fused() {
    let d = split_def();
    let fused = q(ZoneKind::Stack).with_cast(CastChoice::Fused);
    assert_eq!(base(&d, fused), base(&d, q(ZoneKind::Hand)));
    assert_eq!(mv(&d, fused, on(4)), Ok(9));
    assert_eq!(mv(&d, fused, OFF), Ok(5));
}

// REQ-OBJ-002, CR 709.3b: on the stack a split card needs a half or fused; anything else is
// InvalidCast.
#[test]
fn split_stack_invalid_casts() {
    let d = split_def();
    for cast in CASTS {
        if matches!(cast, CastChoice::SplitHalf(_) | CastChoice::Fused) {
            continue;
        }
        let query = q(ZoneKind::Stack).with_cast(cast);
        let e = invalid(LayoutBehavior::Split, ZoneKind::Stack);
        assert_eq!(base(&d, query), Err(e.clone()), "{cast:?}");
        assert_eq!(mv(&d, query, OFF), Err(e));
    }
}

// REQ-OBJ-002, CR 709.4a, 709.4b, 202.3a: equal names give one name; a half without a cost
// contributes nothing; neither half with a cost is no mana cost; no indicators stay None.
#[test]
fn split_combined_edge_cases() {
    let a = face(FaceKind::LeftHalf, "same", None, &["Instant"]);
    let b = face(FaceKind::RightHalf, "same", Some("{U}"), &["Instant"]);
    let d = def(LayoutBehavior::Split, vec![a.clone(), b]);
    let c = base(&d, q(ZoneKind::Graveyard)).unwrap();
    assert_eq!(c.names, set(&["test.same"]));
    assert_eq!(c.mana_cost, Some(cost("{U}")));
    assert_eq!(c.color_indicator, None);
    assert_eq!(c.colors, ColorBitmask::BLUE);
    assert_eq!(c.card_types, set(&["Instant"]));
    assert_eq!(mv(&d, q(ZoneKind::Graveyard), OFF), Ok(1));
    let left_only = q(ZoneKind::Stack).with_cast(CastChoice::SplitHalf(SplitHalf::Left));
    assert_eq!(mv(&d, left_only, on(3)), Ok(0));

    let none_b = face(FaceKind::RightHalf, "other", None, &["Sorcery"]);
    let d = def(LayoutBehavior::Split, vec![a, none_b]);
    let c = base(&d, q(ZoneKind::Exile)).unwrap();
    assert_eq!(c.mana_cost, None);
    assert_eq!(c.colors, ColorBitmask::NONE);
    assert_eq!(mv(&d, q(ZoneKind::Exile), OFF), Ok(0));

    // One indicator only.
    let mut l = face(FaceKind::LeftHalf, "ind", Some("{1}"), &["Instant"]);
    l.color_indicator = Some(ColorBitmask::BLACK);
    let r = face(FaceKind::RightHalf, "nind", Some("{1}"), &["Instant"]);
    let d = def(LayoutBehavior::Split, vec![l, r]);
    let c = base(&d, q(ZoneKind::Hand)).unwrap();
    assert_eq!(c.color_indicator, Some(ColorBitmask::BLACK));
    assert_eq!(c.colors, ColorBitmask::BLACK);
    assert_eq!(c.mana_cost, Some(cost("{1}{1}")));
}

// ---------------------------------------------------------------- FLIP

fn flip_def() -> CardDefinitionDSL {
    let mut main = with_pt(
        face(FaceKind::Main, "flip_top", Some("{2}{U}"), &["Creature"]),
        1,
        2,
    );
    main.subtypes = vec![subtype_of("Creature", 0)];
    main.color_indicator = Some(ColorBitmask::GREEN);
    let mut flipped = with_pt(
        face(FaceKind::Flipped, "flip_bottom", None, &["Creature"]),
        5,
        6,
    );
    flipped.subtypes = vec![subtype_of("Creature", 1)];
    flipped.supertypes = vec!["Legendary".to_owned()];
    flipped.loyalty = Some(3);
    flipped.defense = Some(4);
    flipped.hand_modifier = Some(5);
    flipped.life_modifier = Some(6);
    main.loyalty = Some(7);
    main.defense = Some(8);
    main.hand_modifier = Some(-1);
    main.life_modifier = Some(-2);
    def(LayoutBehavior::Flip, vec![flipped, main])
}

// REQ-OBJ-002, CR 710.2, 710.4, 110.5: every zone off the battlefield has the normal
// characteristics, flipped status ignored; so does an unflipped permanent.
#[test]
fn flip_unflipped_and_off_battlefield() {
    let d = flip_def();
    let main = printed(&d, FaceKind::Main);
    for zone in ZoneKind::ALL {
        for cast in CASTS {
            for s in statuses() {
                if zone == ZoneKind::Battlefield && s.flipped {
                    continue;
                }
                let query = q(zone).with_cast(cast).with_status(s);
                assert_eq!(base(&d, query).unwrap(), main, "{zone:?} {s:?}");
                assert_eq!(mv(&d, query, OFF), Ok(3));
            }
        }
    }
}

// REQ-OBJ-002, CR 710.1b, 710.1c, 710.2: a flipped permanent uses the flipped name, text, type
// line and P/T; mana cost, color indicator, colors and mana value stay the normal ones, and so
// do loyalty, defense and the hand and life modifiers, which CR 710.1b and 710.2 do not swap.
#[test]
fn flip_flipped_on_battlefield() {
    let d = flip_def();
    let main = printed(&d, FaceKind::Main);
    let alt = printed(&d, FaceKind::Flipped);
    for cast in CASTS {
        let query = q(ZoneKind::Battlefield).with_cast(cast).with_status(status(
            true,
            FaceUp::Front,
            false,
        ));
        let c = base(&d, query).unwrap();
        assert_eq!(c.names, set(&["test.flip_bottom"]));
        assert_eq!(c.rules_text, alt.rules_text);
        assert_eq!(c.abilities, alt.abilities);
        assert_eq!(c.supertypes, set(&["Legendary"]));
        assert_eq!(c.card_types, alt.card_types);
        assert_eq!(c.subtypes, alt.subtypes);
        assert_eq!(c.power, Some(ValueExpressionAST(5.into())));
        assert_eq!(c.toughness, Some(ValueExpressionAST(6.into())));
        assert_eq!(c.loyalty, Some(7));
        assert_eq!(c.defense, Some(8));
        assert_eq!(c.hand_modifier, Some(-1));
        assert_eq!(c.life_modifier, Some(-2));
        assert_eq!(c.mana_cost, main.mana_cost);
        assert_eq!(c.color_indicator, Some(ColorBitmask::GREEN));
        assert_eq!(c.colors, main.colors);
        assert_eq!(mv(&d, query, OFF), Ok(3));
    }
}

// ---------------------------------------------------------------- DFC_NONMODAL

fn nonmodal_def(front_cost: Option<&str>) -> CardDefinitionDSL {
    let front = with_pt(
        face(FaceKind::Main, "front", front_cost, &["Creature"]),
        2,
        2,
    );
    let mut back = with_pt(face(FaceKind::Back, "back", None, &["Creature"]), 4, 4);
    back.color_indicator = Some(ColorBitmask::RED);
    def(LayoutBehavior::DfcNonmodal, vec![back, front])
}

// REQ-OBJ-002, CR 712.6a, 202.3b, 712.6d: off the stack and battlefield only the front face,
// cast and status ignored; the copy-of-a-back-face rule applies only to a spell or permanent, so
// the front face's cost applies here even with `copying_back_face` set.
#[test]
fn nonmodal_other_zones_front() {
    let d = nonmodal_def(Some("{X}{1}{G}"));
    let front = printed(&d, FaceKind::Main);
    for zone in HIDDEN_OR_OTHER {
        for cast in CASTS {
            for s in statuses() {
                let query = q(zone).with_cast(cast).with_status(s);
                assert_eq!(base(&d, query).unwrap(), front);
                assert_eq!(mv(&d, query, OFF), Ok(2), "{zone:?} {s:?}");
                // CR 107.3g: X is 0 off the stack whatever the context.
                assert_eq!(mv(&d, query, on(3)), Ok(2), "{zone:?} {s:?}");
            }
        }
    }
}

// REQ-OBJ-002, CR 712.6b, 712.8a, 202.3b: on the stack the front face normally; cast
// transformed, the back face with the front face's mana value (X counted).
#[test]
fn nonmodal_stack() {
    let d = nonmodal_def(Some("{X}{1}{G}"));
    let stack = q(ZoneKind::Stack);
    assert_eq!(base(&d, stack).unwrap(), printed(&d, FaceKind::Main));
    assert_eq!(mv(&d, stack, on(2)), Ok(4));
    let transformed = stack.with_cast(CastChoice::Transformed);
    assert_eq!(base(&d, transformed).unwrap(), printed(&d, FaceKind::Back));
    assert_eq!(mv(&d, transformed, on(2)), Ok(4));
    assert_eq!(mv(&d, transformed, OFF), Ok(2));
    // CR 202.3b, 712.6d: a copy of the back face is 0.
    let copy = transformed.with_status(status(false, FaceUp::Front, true));
    assert_eq!(mv(&d, copy, on(2)), Ok(0));
    assert_eq!(base(&d, copy).unwrap(), printed(&d, FaceKind::Back));
}

// REQ-OBJ-002, CR 712.6b: any cast other than None or Transformed is InvalidCast on the stack.
#[test]
fn nonmodal_stack_invalid_casts() {
    let d = nonmodal_def(Some("{1}"));
    for cast in CASTS {
        if matches!(cast, CastChoice::None | CastChoice::Transformed) {
            continue;
        }
        let query = q(ZoneKind::Stack).with_cast(cast);
        let e = invalid(LayoutBehavior::DfcNonmodal, ZoneKind::Stack);
        assert_eq!(base(&d, query), Err(e.clone()));
        assert_eq!(mv(&d, query, OFF), Err(e));
    }
}

// REQ-OBJ-002, CR 712.6c, 712.6d, 202.3b: on the battlefield the face up; back face MV is the
// front cost's; a copy of the back face is 0; cast ignored.
#[test]
fn nonmodal_battlefield() {
    let d = nonmodal_def(Some("{3}{G}"));
    for cast in CASTS {
        let bf = q(ZoneKind::Battlefield).with_cast(cast);
        let front = bf.with_status(status(false, FaceUp::Front, false));
        let back = bf.with_status(status(false, FaceUp::Back, false));
        let copy = bf.with_status(status(false, FaceUp::Back, true));
        assert_eq!(base(&d, front).unwrap(), printed(&d, FaceKind::Main));
        assert_eq!(base(&d, back).unwrap(), printed(&d, FaceKind::Back));
        assert_eq!(base(&d, back).unwrap().colors, ColorBitmask::RED);
        assert_eq!(mv(&d, front, OFF), Ok(4));
        assert_eq!(mv(&d, back, OFF), Ok(4));
        assert_eq!(mv(&d, copy, OFF), Ok(0));
        assert_eq!(base(&d, copy).unwrap(), printed(&d, FaceKind::Back));
    }
}

// REQ-OBJ-002, CR 202.3a, 202.3b: front cost {0} and no front cost both give the back face
// mana value 0; the two front faces differ in mana_cost.
#[test]
fn nonmodal_front_zero_versus_no_cost() {
    let zero = nonmodal_def(Some("{0}"));
    let none = nonmodal_def(None);
    let back = q(ZoneKind::Battlefield).with_status(status(false, FaceUp::Back, false));
    assert_eq!(mv(&zero, back, OFF), Ok(0));
    assert_eq!(mv(&none, back, OFF), Ok(0));
    assert_eq!(base(&zero, back).unwrap().mana_cost, None);
    assert_eq!(
        base(&zero, q(ZoneKind::Hand)).unwrap().mana_cost,
        Some(cost("{0}"))
    );
    assert_eq!(base(&none, q(ZoneKind::Hand)).unwrap().mana_cost, None);
    assert_eq!(mv(&zero, q(ZoneKind::Hand), OFF), Ok(0));
}

// ---------------------------------------------------------------- DFC_MODAL

fn modal_def() -> CardDefinitionDSL {
    let front = face(
        FaceKind::Main,
        "modal_front",
        Some("{X}{2}{B}"),
        &["Sorcery"],
    );
    let back = face(FaceKind::Back, "modal_back", None, &["Land"]);
    def(LayoutBehavior::DfcModal, vec![front, back])
}

// REQ-OBJ-002, CR 712.6a: off the stack and battlefield only the front face; the copying
// flag is ignored for modal cards.
#[test]
fn modal_other_zones_front() {
    let d = modal_def();
    for zone in HIDDEN_OR_OTHER {
        for cast in CASTS {
            for s in statuses() {
                let query = q(zone).with_cast(cast).with_status(s);
                assert_eq!(base(&d, query).unwrap(), printed(&d, FaceKind::Main));
                assert_eq!(mv(&d, query, OFF), Ok(3));
            }
        }
    }
}

// REQ-OBJ-002, CR 712.8b, 712.6e, 202.3a: on the stack the chosen face, its own mana value;
// a back face with no mana cost is 0 (never the front cost).
#[test]
fn modal_stack_chosen_face() {
    let d = modal_def();
    let front = q(ZoneKind::Stack).with_cast(CastChoice::ModalFace(FaceUp::Front));
    let back = q(ZoneKind::Stack).with_cast(CastChoice::ModalFace(FaceUp::Back));
    assert_eq!(base(&d, front).unwrap(), printed(&d, FaceKind::Main));
    assert_eq!(base(&d, back).unwrap(), printed(&d, FaceKind::Back));
    assert_eq!(mv(&d, front, on(2)), Ok(5));
    assert_eq!(mv(&d, back, on(2)), Ok(0));
    let copying_back = back.with_status(status(false, FaceUp::Front, true));
    assert_eq!(mv(&d, copying_back, on(2)), Ok(0));
    let copying_front = front.with_status(status(false, FaceUp::Back, true));
    assert_eq!(mv(&d, copying_front, on(2)), Ok(5));
}

// REQ-OBJ-002, CR 712.8b: on the stack a modal face must be chosen; None, Transformed and
// every other choice are InvalidCast.
#[test]
fn modal_stack_invalid_casts() {
    let d = modal_def();
    for cast in CASTS {
        if matches!(cast, CastChoice::ModalFace(_)) {
            continue;
        }
        let query = q(ZoneKind::Stack).with_cast(cast);
        let e = invalid(LayoutBehavior::DfcModal, ZoneKind::Stack);
        assert_eq!(base(&d, query), Err(e.clone()), "{cast:?}");
        assert_eq!(mv(&d, query, OFF), Err(e));
    }
}

// REQ-OBJ-002, CR 712.6e, 712.9, 712.10, 712.11: on the battlefield the face up, each with its
// own mana value; cast and copying flag ignored.
#[test]
fn modal_battlefield() {
    let d = modal_def();
    for cast in CASTS {
        for copying in [false, true] {
            let bf = q(ZoneKind::Battlefield).with_cast(cast);
            let front = bf.with_status(status(false, FaceUp::Front, copying));
            let back = bf.with_status(status(false, FaceUp::Back, copying));
            assert_eq!(base(&d, front).unwrap(), printed(&d, FaceKind::Main));
            assert_eq!(base(&d, back).unwrap(), printed(&d, FaceKind::Back));
            assert_eq!(mv(&d, front, OFF), Ok(3));
            assert_eq!(mv(&d, back, OFF), Ok(0));
        }
    }
}

// ---------------------------------------------------------------- ADVENTURER / OMEN

fn alt_def(layout: LayoutBehavior, alt_kind: FaceKind) -> CardDefinitionDSL {
    let main = with_pt(
        face(FaceKind::Main, "alt_main", Some("{3}{G}"), &["Creature"]),
        3,
        3,
    );
    let mut alt = face(alt_kind, "alt_spell", Some("{X}{U}"), &["Instant"]);
    alt.subtypes = vec![subtype_of("Instant", 0)];
    def(layout, vec![alt, main])
}

// REQ-OBJ-002, CR 716.3b, 716.4, 721.3b, 721.4: on the stack as the alternative only the
// alternative characteristics; on the stack not as it, and in every other zone, only the
// normal ones; other casts on the stack are InvalidCast.
#[test]
fn adventurer_and_omen() {
    for (layout, kind, alt_cast) in [
        (
            LayoutBehavior::Adventurer,
            FaceKind::Adventure,
            CastChoice::Adventure,
        ),
        (LayoutBehavior::Omen, FaceKind::Omen, CastChoice::Omen),
    ] {
        let d = alt_def(layout, kind);
        let main = printed(&d, FaceKind::Main);
        let alt = printed(&d, kind);
        let stack = q(ZoneKind::Stack);
        let as_alt = stack.with_cast(alt_cast);
        assert_eq!(base(&d, as_alt).unwrap(), alt, "{layout}");
        assert_eq!(base(&d, as_alt).unwrap().power, None);
        assert_eq!(base(&d, as_alt).unwrap().colors, ColorBitmask::BLUE);
        assert_eq!(mv(&d, as_alt, on(3)), Ok(4));
        assert_eq!(mv(&d, as_alt, OFF), Ok(1));
        assert_eq!(base(&d, stack).unwrap(), main);
        assert_eq!(mv(&d, stack, on(3)), Ok(4));
        for cast in CASTS {
            if cast == alt_cast || cast == CastChoice::None {
                continue;
            }
            let e = invalid(layout, ZoneKind::Stack);
            assert_eq!(base(&d, stack.with_cast(cast)), Err(e.clone()));
            assert_eq!(mv(&d, stack.with_cast(cast), OFF), Err(e));
        }
        for zone in non_stack_zones() {
            for cast in CASTS {
                for s in statuses() {
                    let query = q(zone).with_cast(cast).with_status(s);
                    assert_eq!(base(&d, query).unwrap(), main);
                    assert_eq!(mv(&d, query, OFF), Ok(4));
                }
            }
        }
    }
}

// ---------------------------------------------------------------- PROTOTYPE

fn prototype_def() -> CardDefinitionDSL {
    let mut main = with_pt(
        face(
            FaceKind::Main,
            "proto_main",
            Some("{6}"),
            &["Artifact", "Creature"],
        ),
        6,
        6,
    );
    main.subtypes = vec![subtype_of("Creature", 0)];
    let proto = with_pt(
        face(FaceKind::Prototype, "proto_small", Some("{X}{1}{R}"), &[]),
        2,
        1,
    );
    def(LayoutBehavior::Prototype, vec![proto, main])
}

// REQ-OBJ-002, CR 719.3b, 719.5: a prototyped spell and the permanent it becomes have the
// prototype P/T, mana cost and that cost's colors; name, types and abilities stay normal.
#[test]
fn prototype_prototyped_stack_and_battlefield() {
    let d = prototype_def();
    let main = printed(&d, FaceKind::Main);
    for zone in [ZoneKind::Stack, ZoneKind::Battlefield] {
        for s in statuses() {
            let query = q(zone).with_cast(CastChoice::Prototype).with_status(s);
            let c = base(&d, query).unwrap();
            assert_eq!(c.mana_cost, Some(cost("{X}{1}{R}")));
            assert_eq!(c.power, Some(ValueExpressionAST(2.into())));
            assert_eq!(c.toughness, Some(ValueExpressionAST(1.into())));
            assert_eq!(c.colors, ColorBitmask::RED);
            assert_eq!(
                c.colors,
                base_colors(c.mana_cost.as_ref(), main.color_indicator, CdaColors::None)
            );
            assert_eq!(c.names, main.names);
            assert_eq!(c.card_types, main.card_types);
            assert_eq!(c.subtypes, main.subtypes);
            assert_eq!(c.supertypes, main.supertypes);
            assert_eq!(c.abilities, main.abilities);
            assert_eq!(c.rules_text, main.rules_text);
            assert_eq!(c.color_indicator, main.color_indicator);
            // CR 202.3e, 107.3m: X counts on the stack, 0 for the permanent.
            let with_x = if zone == ZoneKind::Stack { 4 } else { 2 };
            assert_eq!(mv(&d, query, on(2)), Ok(with_x));
            assert_eq!(mv(&d, query, OFF), Ok(2));
        }
    }
}

// REQ-OBJ-002, CR 719.3b: the prototype colors keep the Main face's color indicator.
#[test]
fn prototype_colors_keep_indicator() {
    let mut d = prototype_def();
    for f in &mut d.faces {
        if f.face_kind == FaceKind::Main {
            f.color_indicator = Some(ColorBitmask::WHITE);
        }
    }
    let query = q(ZoneKind::Battlefield).with_cast(CastChoice::Prototype);
    assert_eq!(base(&d, query).unwrap().colors, mask(1 | 8));
}

// REQ-OBJ-002, CR 719.4: not cast prototyped (stack and battlefield), and every other zone,
// only the normal characteristics.
#[test]
fn prototype_normal() {
    let d = prototype_def();
    let main = printed(&d, FaceKind::Main);
    for zone in [ZoneKind::Stack, ZoneKind::Battlefield] {
        let query = q(zone);
        assert_eq!(base(&d, query).unwrap(), main);
        assert_eq!(mv(&d, query, on(2)), Ok(6));
    }
    for zone in HIDDEN_OR_OTHER {
        for cast in CASTS {
            let query = q(zone).with_cast(cast);
            assert_eq!(base(&d, query).unwrap(), main);
            assert_eq!(mv(&d, query, OFF), Ok(6));
        }
    }
}

// REQ-OBJ-002, CR 719.3b, 719.4: on the stack or battlefield only Prototype or None.
#[test]
fn prototype_invalid_casts() {
    let d = prototype_def();
    for zone in [ZoneKind::Stack, ZoneKind::Battlefield] {
        for cast in CASTS {
            if matches!(cast, CastChoice::None | CastChoice::Prototype) {
                continue;
            }
            let e = invalid(LayoutBehavior::Prototype, zone);
            assert_eq!(base(&d, q(zone).with_cast(cast)), Err(e.clone()));
            assert_eq!(mv(&d, q(zone).with_cast(cast), OFF), Err(e));
        }
    }
}

// ---------------------------------------------------------------- exhaustive properties

/// Definition shapes for one supported layout: valid, empty, a bad type line, duplicated
/// faces, and every other layout's valid faces.
fn shapes(layout: LayoutBehavior) -> Vec<CardDefinitionDSL> {
    let good = valid(layout);
    let mut out = vec![good.clone(), def(layout, Vec::new())];
    let mut bad_type = good.clone();
    for f in &mut bad_type.faces {
        f.card_types = vec!["test.bad".to_owned()];
    }
    out.push(bad_type);
    let mut extra = good.clone();
    extra.faces.extend(good.faces.clone());
    out.push(extra);
    for other in SUPPORTED {
        let mut d = valid(other);
        d.layout = layout;
        out.push(d);
    }
    out
}

fn is_defined_error(e: &LayoutError, layout: LayoutBehavior) -> bool {
    match e {
        LayoutError::BadFaces { layout: l } | LayoutError::InvalidCast { layout: l, .. } => {
            *l == layout
        }
        LayoutError::Object(_) => true,
        LayoutError::UnsupportedLayout(_)
        | LayoutError::LayoutMismatch { .. }
        | LayoutError::NotMeldPair
        | LayoutError::AlreadyPrepared
        | LayoutError::TooFewComponents { .. }
        | LayoutError::InvalidStationSymbols { .. } => false,
    }
}

// REQ-OBJ-002, REQ-CORE-006: for every supported layout, definition shape, zone, cast and
// status, the functions return Ok or a defined error, never panic, agree with each other, and
// are deterministic across repeated calls.
#[test]
fn exhaustive_total_and_deterministic() {
    let mut checked = 0u32;
    for layout in SUPPORTED {
        let object = layout_for(layout).unwrap();
        for d in shapes(layout) {
            let g = gating(&d);
            assert_eq!(g, object.gating(&d));
            assert_eq!(g, gating(&d));
            for zone in ZoneKind::ALL {
                for cast in CASTS {
                    for s in statuses() {
                        let query = q(zone).with_cast(cast).with_status(s);
                        let b = base(&d, query);
                        assert_eq!(b, base(&d, query));
                        assert_eq!(b, object.base_characteristics(&d, &query, vocab()));
                        for ctx in [OFF, on(0), on(u32::MAX)] {
                            let m = mv(&d, query, ctx);
                            assert_eq!(m, mv(&d, query, ctx));
                            assert_eq!(m, object.mana_value(&d, &query, ctx, vocab()));
                            match (&b, &m) {
                                (Ok(_), Ok(v)) => assert!(*v >= 0),
                                (Err(e1), Err(e2)) => {
                                    assert_eq!(e1, e2);
                                    assert!(is_defined_error(e1, layout), "{e1:?}");
                                }
                                _ => panic!("base and mana value disagree: {b:?} {m:?}"),
                            }
                        }
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 0);
}

// ---------------------------------------------------------------- X off the stack (CR 202.3e)

/// A stack cast choice of `layout` that selects the first face (`{X}{1}{G}` in [`valid`]).
fn first_face_cast(layout: LayoutBehavior) -> CastChoice {
    match layout {
        LayoutBehavior::Split => CastChoice::SplitHalf(SplitHalf::Left),
        LayoutBehavior::DfcModal => CastChoice::ModalFace(FaceUp::Front),
        _ => CastChoice::None,
    }
}

// REQ-OBJ-001, REQ-OBJ-002, CR 202.3e, 107.3g, 107.3m: for every layout, an `OnStack(X = 3)`
// context counts X only on the stack; in every other zone X is 0 whatever context the caller
// supplies. Checked through the trait impl directly and through `layouts::mana_value`.
#[test]
fn x_counts_only_on_the_stack_every_layout() {
    for layout in SUPPORTED {
        let d = valid(layout);
        let object = layout_for(layout).unwrap();
        for zone in ZoneKind::ALL {
            let query = q(zone).with_cast(first_face_cast(layout));
            // {X}{1}{G}: 5 with X = 3 on the stack, 2 everywhere else; a split card off the
            // stack adds its right half's {2}{R} (CR 202.3d).
            let off = match (layout, zone) {
                (_, ZoneKind::Stack) => 2,
                (LayoutBehavior::Split, _) => 5,
                _ => 2,
            };
            let expected = if zone == ZoneKind::Stack { 5 } else { off };
            assert_eq!(mv(&d, query, on(3)), Ok(expected), "{layout} {zone:?}");
            assert_eq!(
                object.mana_value(&d, &query, on(3), vocab()),
                Ok(expected),
                "{layout} {zone:?}"
            );
            assert_eq!(mv(&d, query, OFF), Ok(off), "{layout} {zone:?}");
        }
    }
}

// REQ-OBJ-001, REQ-OBJ-002, CR 107.3p, 202.3e: Y (and Z, treated alike) follow X: counted on
// the stack, 0 in every other zone.
#[test]
fn y_and_z_count_only_on_the_stack() {
    let mut d = valid(LayoutBehavior::Normal);
    d.faces[0].mana_cost = Some(cost("{X}{Y}{Z}{1}"));
    let ctx = ManaValueContext::OnStack(VariableValues { x: 1, y: 2, z: 4 });
    for zone in ZoneKind::ALL {
        let expected = if zone == ZoneKind::Stack { 8 } else { 1 };
        assert_eq!(mv(&d, q(zone), ctx), Ok(expected), "{zone:?}");
        assert_eq!(
            NormalLayout.mana_value(&d, &q(zone), ctx, vocab()),
            Ok(expected)
        );
    }
}

// REQ-OBJ-001, REQ-OBJ-002, CR 202.3b, 712.6d, 712.6a: `copying_back_face` gives mana value 0
// only to a spell on the stack or a permanent on the battlefield; in every other zone the
// flag is ignored and the front face's cost applies.
#[test]
fn nonmodal_copying_back_face_zone_matrix() {
    let d = nonmodal_def(Some("{3}{G}"));
    let copying = status(false, FaceUp::Back, true);
    for zone in ZoneKind::ALL {
        let query = q(zone).with_status(copying);
        let expected = if matches!(zone, ZoneKind::Stack | ZoneKind::Battlefield) {
            0
        } else {
            4
        };
        assert_eq!(mv(&d, query, OFF), Ok(expected), "{zone:?}");
        assert_eq!(
            DfcNonmodalLayout.mana_value(&d, &query, OFF, vocab()),
            Ok(expected),
            "{zone:?}"
        );
    }
}
