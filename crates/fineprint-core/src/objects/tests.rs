// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Unit tests of object characteristics (REQ-OBJ-001, REQ-NFR-008). Synthetic data only.

use std::collections::BTreeSet;

use fineprint_dsl::card::{
    AbilityAST, CardFaceDSL, ColorBitmask, FaceKind, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::mana::{Color, ManaCostDSL, ManaSymbol, Variable};
use fineprint_dsl::vocab::RulesVocabulary;

use super::*;

const OFF: ManaValueContext = ManaValueContext::OffStack;

fn cost(text: &str) -> ManaCostDSL {
    ManaCostDSL::parse(text).unwrap()
}

fn sym(text: &str) -> ManaSymbol {
    ManaSymbol::parse(text).unwrap()
}

fn on(x: u32, y: u32, z: u32) -> ManaValueContext {
    ManaValueContext::OnStack(VariableValues { x, y, z })
}

fn plain_mv(c: Option<&ManaCostDSL>, ctx: ManaValueContext) -> i64 {
    mana_value(&ManaValueSubject::Plain { mana_cost: c }, ctx)
}

fn vocab() -> &'static RulesVocabulary {
    RulesVocabulary::bundled().unwrap()
}

fn mask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).unwrap()
}

fn first_subtype(card_type: &str) -> String {
    vocab().subtypes_of(card_type).next().unwrap().to_owned()
}

fn face(name: &str, mana_cost: Option<ManaCostDSL>, card_types: &[&str]) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: FaceKind::Main,
        name: name.to_owned(),
        mana_cost,
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
        text: TextTokenStream(Vec::new()),
        abilities: Vec::new(),
    }
}

// ---------------------------------------------------------------- mana value

// REQ-OBJ-001, CR 202.3a: no mana cost is 0, {0} is 0, and the two differ in mana_cost.
#[test]
fn mv_202_3a_no_cost_and_zero_cost() {
    assert_eq!(plain_mv(None, OFF), 0);
    let zero = cost("{0}");
    assert_eq!(plain_mv(Some(&zero), OFF), 0);
    assert_eq!(plain_mv(None, on(5, 5, 5)), 0);
    let a = Characteristics::from_face(&face("test.no_cost", None, &["Land"]), vocab()).unwrap();
    let b = Characteristics::from_face(
        &face("test.no_cost", Some(zero.clone()), &["Land"]),
        vocab(),
    )
    .unwrap();
    assert_eq!(a.mana_value(OFF), b.mana_value(OFF));
    assert_eq!(a.mana_cost, None);
    assert_eq!(b.mana_cost, Some(zero));
    assert_ne!(a, b);
}

// REQ-OBJ-001, CR 202.3a, 708.2a: a face-down object has no mana cost, so MV 0.
#[test]
fn mv_202_3a_face_down_is_zero() {
    assert_eq!(plain_mv(None, OFF), 0);
}

// REQ-OBJ-001, CR 202.3b, 712.6b: a nonmodal back face uses the front face's cost.
#[test]
fn mv_202_3b_nonmodal_back_face_uses_front_cost() {
    let front = cost("{2}{G}");
    let s = ManaValueSubject::NonmodalBackFace {
        front_mana_cost: Some(&front),
    };
    assert_eq!(mana_value(&s, OFF), 3);
    // A front face with no mana cost gives 0; {0} front also 0.
    let none_front = ManaValueSubject::NonmodalBackFace {
        front_mana_cost: None,
    };
    assert_eq!(mana_value(&none_front, OFF), 0);
    // X on the front cost counts per context (CR 202.3e).
    let fx = cost("{X}{G}");
    let sx = ManaValueSubject::NonmodalBackFace {
        front_mana_cost: Some(&fx),
    };
    assert_eq!(mana_value(&sx, OFF), 1);
    assert_eq!(mana_value(&sx, on(4, 0, 0)), 5);
}

// REQ-OBJ-001, CR 202.3b: a copy of a nonmodal back face is 0 in every context.
#[test]
fn mv_202_3b_copy_of_back_face_is_zero() {
    let s = ManaValueSubject::CopyOfNonmodalBackFace;
    assert_eq!(mana_value(&s, OFF), 0);
    assert_eq!(mana_value(&s, on(9, 9, 9)), 0);
}

// REQ-OBJ-001, CR 202.3a, 712.6e: a modal DFC back face with no cost uses its own (absent) cost:
// 0; the nonmodal back face of the same front is the front's MV.
#[test]
fn mv_202_3a_modal_back_face_without_cost_is_zero() {
    let front = cost("{3}{U}");
    assert_eq!(plain_mv(None, OFF), 0);
    let nonmodal = ManaValueSubject::NonmodalBackFace {
        front_mana_cost: Some(&front),
    };
    assert_eq!(mana_value(&nonmodal, OFF), 4);
}

// REQ-OBJ-001, CR 202.3c, 713.5c: a melded permanent is the sum of its front costs.
#[test]
fn mv_202_3c_melded_sum() {
    let a = cost("{2}{W}");
    let b = cost("{4}{W}{W}");
    let s = ManaValueSubject::Melded {
        front_mana_costs: [Some(&a), Some(&b)],
    };
    assert_eq!(mana_value(&s, OFF), 9);
    let s1 = ManaValueSubject::Melded {
        front_mana_costs: [Some(&a), None],
    };
    assert_eq!(mana_value(&s1, OFF), 3);
    let s0 = ManaValueSubject::Melded {
        front_mana_costs: [None, None],
    };
    assert_eq!(mana_value(&s0, OFF), 0);
}

// REQ-OBJ-001, CR 202.3c: a copy of a melded permanent is 0.
#[test]
fn mv_202_3c_copy_of_melded_is_zero() {
    assert_eq!(mana_value(&ManaValueSubject::CopyOfMelded, OFF), 0);
    assert_eq!(mana_value(&ManaValueSubject::CopyOfMelded, on(3, 3, 3)), 0);
}

// REQ-OBJ-001, CR 202.3d, 709.4b, 709.3b, 709.4d: split card halves.
#[test]
fn mv_202_3d_split() {
    let l = cost("{1}{R}");
    let r = cost("{3}{U}{U}");
    let split = |state| ManaValueSubject::Split {
        left: Some(&l),
        right: Some(&r),
        state,
    };
    assert_eq!(mana_value(&split(SplitState::NotOnStack), OFF), 7);
    assert_eq!(
        mana_value(
            &split(SplitState::OnStackHalf(SplitHalf::Left)),
            on(0, 0, 0)
        ),
        2
    );
    assert_eq!(
        mana_value(
            &split(SplitState::OnStackHalf(SplitHalf::Right)),
            on(0, 0, 0)
        ),
        5
    );
    assert_eq!(mana_value(&split(SplitState::OnStackFused), on(0, 0, 0)), 7);
    // State decides the halves even with an OnStack context; no panic.
    assert_eq!(mana_value(&split(SplitState::NotOnStack), on(1, 1, 1)), 7);
    // A half with no cost counts 0.
    let half_none = ManaValueSubject::Split {
        left: None,
        right: Some(&r),
        state: SplitState::NotOnStack,
    };
    assert_eq!(mana_value(&half_none, OFF), 5);
}

// REQ-OBJ-001, CR 202.3d, 202.3e, 107.3g: a split card not on the stack treats X as 0 whatever
// context is supplied; on the stack the context applies (CodeRabbit r1 regression).
#[test]
fn mv_202_3d_split_not_on_stack_ignores_context() {
    let l = cost("{X}{R}");
    let r = cost("{1}{U}");
    let split = |state| ManaValueSubject::Split {
        left: Some(&l),
        right: Some(&r),
        state,
    };
    assert_eq!(mana_value(&split(SplitState::NotOnStack), OFF), 3);
    assert_eq!(mana_value(&split(SplitState::NotOnStack), on(3, 0, 0)), 3);
    assert_eq!(mana_value(&split(SplitState::OnStackFused), on(3, 0, 0)), 6);
    assert_eq!(
        mana_value(
            &split(SplitState::OnStackHalf(SplitHalf::Left)),
            on(3, 0, 0)
        ),
        4
    );
    assert_eq!(
        mana_value(
            &split(SplitState::OnStackHalf(SplitHalf::Right)),
            on(3, 0, 0)
        ),
        2
    );
    // The review's example: left {X}{R}, right {2}{U}, x = 3.
    let r2 = cost("{2}{U}");
    let split2 = |state| ManaValueSubject::Split {
        left: Some(&l),
        right: Some(&r2),
        state,
    };
    assert_eq!(mana_value(&split2(SplitState::NotOnStack), on(3, 0, 0)), 4);
    assert_eq!(
        mana_value(&split2(SplitState::OnStackFused), on(3, 0, 0)),
        7
    );
}

// REQ-OBJ-001, CR 202.3e, 107.3g, 107.3a, 107.3p, 707.10: X and Y off and on the stack.
#[test]
fn mv_202_3e_variables() {
    let xr = cost("{X}{R}");
    assert_eq!(plain_mv(Some(&xr), OFF), 1);
    assert_eq!(plain_mv(Some(&xr), on(3, 0, 0)), 4);
    // CR 707.10: a copy on the stack counts the same X (same OnStack values).
    let ctx = on(3, 0, 0);
    assert_eq!(plain_mv(Some(&xr), ctx), plain_mv(Some(&xr), ctx));
    let yr = cost("{Y}{R}");
    assert_eq!(plain_mv(Some(&yr), OFF), 1);
    assert_eq!(plain_mv(Some(&yr), on(0, 3, 0)), 4);
    assert_eq!(plain_mv(Some(&yr), on(3, 0, 0)), 1);
    // Z is treated like X and Y (DECISIONS.md #21).
    let zr = cost("{Z}{R}");
    assert_eq!(plain_mv(Some(&zr), OFF), 1);
    assert_eq!(plain_mv(Some(&zr), on(0, 0, 2)), 3);
    let xx = cost("{X}{X}{Y}");
    assert_eq!(plain_mv(Some(&xx), on(2, 5, 0)), 9);
}

// REQ-OBJ-001, CR 202.3f: hybrid uses the largest component.
#[test]
fn mv_202_3f_hybrid() {
    assert_eq!(plain_mv(Some(&cost("{1}{W/U}{W/U}")), OFF), 3);
    assert_eq!(plain_mv(Some(&cost("{2/B}{2/B}{2/B}")), OFF), 6);
}

// REQ-OBJ-001, CR 202.3g: Phyrexian symbols contribute 1 each.
#[test]
fn mv_202_3g_phyrexian() {
    assert_eq!(plain_mv(Some(&cost("{1}{W/P}{W/P}")), OFF), 3);
    assert_eq!(plain_mv(Some(&cost("{W/U/P}")), OFF), 1);
}

// REQ-OBJ-001, CR 202.3, 107.4c, 107.4h: {C} and {S} are 1 each.
#[test]
fn mv_202_3_colorless_and_snow() {
    assert_eq!(plain_mv(Some(&cost("{C}")), OFF), 1);
    assert_eq!(plain_mv(Some(&cost("{S}")), OFF), 1);
    assert_eq!(plain_mv(Some(&cost("{2}{C}{S}{G}")), OFF), 5);
}

// REQ-OBJ-001, CR 202.3, 202.3e-g: every symbol variant in both contexts.
#[test]
fn symbol_mana_value_every_variant() {
    let ctx = on(7, 8, 9);
    let cases: [(ManaSymbol, i64, i64); 12] = [
        (ManaSymbol::Generic(0), 0, 0),
        (ManaSymbol::Generic(5), 5, 5),
        (ManaSymbol::Colored(Color::Red), 1, 1),
        (ManaSymbol::Colorless, 1, 1),
        (ManaSymbol::Snow, 1, 1),
        (ManaSymbol::Hybrid(Color::White, Color::Blue), 1, 1),
        (ManaSymbol::GenericHybrid(2, Color::Black), 2, 2),
        (ManaSymbol::Phyrexian(Color::Green), 1, 1),
        (ManaSymbol::HybridPhyrexian(Color::White, Color::Blue), 1, 1),
        (ManaSymbol::Variable(Variable::X), 0, 7),
        (ManaSymbol::Variable(Variable::Y), 0, 8),
        (ManaSymbol::Variable(Variable::Z), 0, 9),
    ];
    for (s, off, onv) in cases {
        assert_eq!(symbol_mana_value(s, OFF), off, "{s:?} off stack");
        assert_eq!(symbol_mana_value(s, ctx), onv, "{s:?} on stack");
    }
}

// REQ-OBJ-001, CR 202.3e: large X values do not overflow (saturating i64 arithmetic).
#[test]
fn mv_large_values_do_not_overflow() {
    let c = cost("{X}{Y}{Z}{1000000}");
    let big = on(u32::MAX, u32::MAX, u32::MAX);
    assert_eq!(plain_mv(Some(&c), big), 3 * i64::from(u32::MAX) + 1_000_000);
}

// ---------------------------------------------------------------- colors

// REQ-OBJ-001, CR 105.2: color_mask and union_colors.
#[test]
fn color_mask_and_union() {
    let all: Vec<u8> = Color::ALL.iter().map(|c| color_mask(*c).bits()).collect();
    assert_eq!(all, vec![1, 2, 4, 8, 16]);
    let u = union_colors(ColorBitmask::WHITE, ColorBitmask::GREEN);
    assert_eq!(u.bits(), 17);
    assert_eq!(union_colors(mask(31), mask(31)).bits(), 31);
    assert_eq!(
        union_colors(ColorBitmask::NONE, ColorBitmask::NONE),
        ColorBitmask::NONE
    );
}

// REQ-OBJ-001, CR 202.2, 107.4c, 107.4e, 107.4f, 107.4h: colors of every symbol variant.
#[test]
fn symbol_colors_every_variant() {
    assert_eq!(symbol_colors(sym("{R}")), ColorBitmask::RED);
    assert_eq!(symbol_colors(sym("{W/U}")).bits(), 3);
    assert_eq!(symbol_colors(sym("{2/W}")), ColorBitmask::WHITE);
    assert_eq!(symbol_colors(sym("{G/P}")), ColorBitmask::GREEN);
    assert_eq!(symbol_colors(sym("{B/R/P}")).bits(), 12);
    for s in ["{0}", "{3}", "{C}", "{S}", "{X}", "{Y}", "{Z}"] {
        assert_eq!(symbol_colors(sym(s)), ColorBitmask::NONE, "{s}");
    }
}

// REQ-OBJ-001, CR 202.2, 202.2b, 202.2c, 202.2d: colors of whole costs.
#[test]
fn colors_of_costs() {
    let bg = colors_of_mana_cost(Some(&cost("{2/B}{2/B}{G/P}")));
    assert_eq!(bg, union_colors(ColorBitmask::BLACK, ColorBitmask::GREEN));
    assert_eq!(colors_of_mana_cost(Some(&cost("{C}"))), ColorBitmask::NONE);
    assert_eq!(colors_of_mana_cost(Some(&cost("{S}"))), ColorBitmask::NONE);
    assert_eq!(colors_of_mana_cost(Some(&cost("{0}"))), ColorBitmask::NONE);
    assert_eq!(colors_of_mana_cost(None), ColorBitmask::NONE);
    assert_eq!(colors_of_mana_cost(Some(&cost("{W/U/P}"))).bits(), 3);
}

// REQ-OBJ-001, CR 202.2e, 105.2: base colors union cost, indicator and CDA.
#[test]
fn base_colors_union() {
    let c = cost("{1}{R}");
    assert_eq!(
        base_colors(Some(&c), None, CdaColors::None),
        ColorBitmask::RED
    );
    assert_eq!(
        base_colors(Some(&c), Some(ColorBitmask::BLUE), CdaColors::None).bits(),
        10
    );
    assert_eq!(
        base_colors(None, None, CdaColors::Add(ColorBitmask::GREEN)),
        ColorBitmask::GREEN
    );
    assert_eq!(
        base_colors(
            Some(&c),
            Some(ColorBitmask::WHITE),
            CdaColors::Add(ColorBitmask::GREEN)
        )
        .bits(),
        25
    );
    assert_eq!(base_colors(None, None, CdaColors::None), ColorBitmask::NONE);
}

// REQ-OBJ-001, CR 604.3, 702.114a, 105.2, 613.3: a CDA can add to or override the printed
// colors; a colorless-defining CDA makes a colored-cost object colorless (CodeRabbit r1).
#[test]
fn base_colors_cda_add_and_set() {
    let c = cost("{1}{R}");
    let red = cost("{R}");
    assert_eq!(
        base_colors(Some(&c), None, CdaColors::Set(ColorBitmask::NONE)),
        ColorBitmask::NONE
    );
    assert_eq!(
        base_colors(
            Some(&c),
            Some(ColorBitmask::BLUE),
            CdaColors::Set(ColorBitmask::NONE)
        ),
        ColorBitmask::NONE
    );
    assert_eq!(
        base_colors(Some(&c), None, CdaColors::Set(ColorBitmask::GREEN)),
        ColorBitmask::GREEN
    );
    assert_eq!(
        base_colors(Some(&red), None, CdaColors::Add(ColorBitmask::GREEN)).bits(),
        24
    );
    assert_eq!(
        base_colors(Some(&c), None, CdaColors::None),
        ColorBitmask::RED
    );
    assert_eq!(
        base_colors(None, None, CdaColors::Add(ColorBitmask::NONE)),
        base_colors(None, None, CdaColors::None)
    );
    assert_eq!(
        base_colors(Some(&c), None, CdaColors::Add(ColorBitmask::NONE)),
        base_colors(Some(&c), None, CdaColors::None)
    );
    assert_eq!(CdaColors::default(), CdaColors::None);
}

// ---------------------------------------------------------------- color identity

// REQ-OBJ-001, CR 903.4, 702.114a: a devoid-style face keeps its cost colors in its identity;
// a colored CDA adds to them (CodeRabbit r1: the CDA change must not reach identity).
#[test]
fn identity_keeps_cost_colors_with_cda() {
    let c = cost("{1}{R}");
    let devoid_like = id_face(Some(&c), &[]);
    assert_eq!(color_identity(&[devoid_like], None), ColorBitmask::RED);
    let mut green_cda = id_face(Some(&c), &[]);
    green_cda.cda_colors = ColorBitmask::GREEN;
    assert_eq!(color_identity(&[green_cda], None).bits(), 24);
}

fn id_face<'a>(mana_cost: Option<&'a ManaCostDSL>, text: &'a [ManaSymbol]) -> IdentityFace<'a> {
    IdentityFace {
        mana_cost,
        color_indicator: None,
        cda_colors: ColorBitmask::NONE,
        rules_text_symbols: text,
    }
}

// REQ-OBJ-001, CR 903.4: cost symbols and rules-text symbols.
#[test]
fn identity_cost_and_text() {
    let c = cost("{1}{W}");
    let text = [sym("{U}"), sym("{2/B}"), sym("{C}")];
    assert_eq!(
        color_identity(&[id_face(Some(&c), &[])], None),
        ColorBitmask::WHITE
    );
    assert_eq!(color_identity(&[id_face(Some(&c), &text)], None).bits(), 7);
    assert_eq!(color_identity(&[id_face(None, &text)], None).bits(), 6);
}

// REQ-OBJ-001, CR 903.4: color indicator and CDA colors.
#[test]
fn identity_indicator_and_cda() {
    let mut f = id_face(None, &[]);
    f.color_indicator = Some(ColorBitmask::RED);
    assert_eq!(color_identity(&[f], None), ColorBitmask::RED);
    f.cda_colors = ColorBitmask::GREEN;
    assert_eq!(color_identity(&[f], None).bits(), 24);
}

// REQ-OBJ-001, CR 903.4d, 903.4e: the back face and alternative characteristics are included.
#[test]
fn identity_includes_other_faces() {
    let front = cost("{2}{G}");
    let alt = cost("{U}");
    let mut back = id_face(None, &[]);
    back.color_indicator = Some(ColorBitmask::BLACK);
    let faces = [id_face(Some(&front), &[]), back, id_face(Some(&alt), &[])];
    assert_eq!(color_identity(&faces, None).bits(), 16 | 4 | 2);
}

// REQ-OBJ-001, CR 903.4b: a pre-game color choice is included; empty list is NONE.
#[test]
fn identity_chosen_color_and_empty() {
    assert_eq!(color_identity(&[], None), ColorBitmask::NONE);
    assert_eq!(
        color_identity(&[], Some(ColorBitmask::BLUE)),
        ColorBitmask::BLUE
    );
    let c = cost("{R}");
    assert_eq!(
        color_identity(&[id_face(Some(&c), &[])], Some(ColorBitmask::WHITE)).bits(),
        9
    );
}

// ---------------------------------------------------------------- validation

// REQ-OBJ-001, CR 205.2a: an unknown card type is rejected, and it is checked first.
#[test]
fn validate_unknown_card_type() {
    let v = vocab();
    assert_eq!(
        validate_types(&[], &["test.no_such_type"], &[], v),
        Err(ObjectError::UnknownCardType("test.no_such_type".to_owned()))
    );
    // Case-sensitive.
    assert_eq!(
        validate_types(&["NotASupertype"], &["creature"], &["x"], v),
        Err(ObjectError::UnknownCardType("creature".to_owned()))
    );
}

// REQ-OBJ-001, CR 205.4a: an unknown supertype is rejected, checked before subtypes.
#[test]
fn validate_unknown_supertype() {
    let v = vocab();
    assert_eq!(
        validate_types(&["legendary"], &["Creature"], &["test.not_a_subtype"], v),
        Err(ObjectError::UnknownSupertype("legendary".to_owned()))
    );
}

// REQ-OBJ-001, CR 205.3c, 205.3d: a subtype of none of the card types is rejected.
#[test]
fn validate_subtype_not_correlated() {
    let v = vocab();
    let land_sub = first_subtype("Land");
    assert_eq!(
        validate_types(&[], &["Creature"], &[&land_sub], v),
        Err(ObjectError::SubtypeNotCorrelated {
            subtype: land_sub.clone()
        })
    );
    assert_eq!(
        validate_types(&[], &[], &[&land_sub], v),
        Err(ObjectError::SubtypeNotCorrelated { subtype: land_sub })
    );
    assert_eq!(validate_types(&[], &[], &[], v), Ok(()));
}

// REQ-OBJ-001, CR 205.3k, 205.3m: instants and sorceries share spell types; creatures and
// kindreds share creature types.
#[test]
fn validate_shared_subtype_lists() {
    let v = vocab();
    for s in v.subtypes_of("Instant") {
        assert_eq!(validate_types(&[], &["Sorcery"], &[s], v), Ok(()), "{s}");
    }
    for s in v.subtypes_of("Creature").take(20) {
        assert_eq!(
            validate_types(&[], &["Kindred", "Instant"], &[s], v),
            Ok(()),
            "{s}"
        );
    }
}

// REQ-OBJ-001, CR 205.3c: a multi-type object takes a subtype of each of its types.
#[test]
fn validate_multi_type() {
    let v = vocab();
    let land_sub = first_subtype("Land");
    let creature_sub = first_subtype("Creature");
    assert_eq!(
        validate_types(&[], &["Land", "Creature"], &[&land_sub, &creature_sub], v),
        Ok(())
    );
}

// REQ-OBJ-001, CR 205.4b: any supertype on any card type is accepted.
#[test]
fn validate_any_supertype_any_card_type() {
    let v = vocab();
    let supertypes = ["Basic", "Legendary", "Ongoing", "Snow", "World"];
    for st in supertypes {
        assert!(v.is_supertype(st));
    }
    for ct in [
        "Artifact", "Creature", "Instant", "Land", "Scheme", "Vanguard",
    ] {
        assert_eq!(validate_types(&supertypes, &[ct], &[], v), Ok(()), "{ct}");
    }
}

// REQ-OBJ-001, CR 205.3r: phenomenon, scheme, vanguard and conspiracy have no subtypes.
#[test]
fn validate_no_subtype_card_types() {
    let v = vocab();
    let creature_sub = first_subtype("Creature");
    for ct in ["Phenomenon", "Scheme", "Vanguard", "Conspiracy"] {
        assert_eq!(v.subtypes_of(ct).count(), 0, "{ct}");
        assert_eq!(
            validate_types(&[], &[ct], &[&creature_sub], v),
            Err(ObjectError::SubtypeNotCorrelated {
                subtype: creature_sub.clone()
            }),
            "{ct}"
        );
    }
}

// REQ-OBJ-001, CR 205.3d: can_gain_subtype both ways.
#[test]
fn can_gain_subtype_both_ways() {
    let v = vocab();
    let art = first_subtype("Artifact");
    assert!(can_gain_subtype(&["Artifact"], &art, v));
    assert!(can_gain_subtype(&["Creature", "Artifact"], &art, v));
    assert!(!can_gain_subtype(&["Creature"], &art, v));
    assert!(!can_gain_subtype(&[], &art, v));
}

// REQ-OBJ-001: ObjectError displays and is an Error.
#[test]
fn object_error_display() {
    let e: Box<dyn std::error::Error> = Box::new(ObjectError::UnknownCardType("a".into()));
    assert!(e.to_string().contains("card type"));
    assert!(
        ObjectError::UnknownSupertype("b".into())
            .to_string()
            .contains("supertype")
    );
    assert!(
        ObjectError::SubtypeNotCorrelated {
            subtype: "c".into()
        }
        .to_string()
        .contains("subtype")
    );
}

// ---------------------------------------------------------------- from_face

// REQ-OBJ-001, CR 109.3: every field is copied from the face.
#[test]
fn from_face_copies_all_fields() {
    let v = vocab();
    let creature_sub = first_subtype("Creature");
    let mut f = face(
        "test.vanilla_2_2",
        Some(cost("{1}{G}")),
        &["Creature", "Artifact"],
    );
    f.color_indicator = Some(ColorBitmask::BLUE);
    f.supertypes = vec!["Legendary".to_owned(), "Snow".to_owned()];
    f.subtypes = vec![creature_sub.clone()];
    f.power = Some(ValueExpressionAST(2.into()));
    f.toughness = Some(ValueExpressionAST(2.into()));
    f.loyalty = Some(3);
    f.defense = Some(4);
    f.hand_modifier = Some(-1);
    f.life_modifier = Some(5);
    f.text = TextTokenStream(vec!["test.token".into()]);
    f.abilities = vec![AbilityAST("test.ability".into())];
    let c = Characteristics::from_face(&f, v).unwrap();
    assert_eq!(c.names, BTreeSet::from(["test.vanilla_2_2".to_owned()]));
    assert_eq!(c.mana_cost, Some(cost("{1}{G}")));
    assert_eq!(c.color_indicator, Some(ColorBitmask::BLUE));
    // CR 202.2, 202.2e: cost colors plus the indicator.
    assert_eq!(c.colors.bits(), 16 | 2);
    assert_eq!(
        c.supertypes,
        BTreeSet::from(["Legendary".to_owned(), "Snow".to_owned()])
    );
    assert_eq!(
        c.card_types,
        BTreeSet::from(["Creature".to_owned(), "Artifact".to_owned()])
    );
    assert_eq!(c.subtypes, BTreeSet::from([creature_sub]));
    assert_eq!(c.power, f.power);
    assert_eq!(c.toughness, f.toughness);
    assert_eq!(c.loyalty, Some(3));
    assert_eq!(c.defense, Some(4));
    assert_eq!(c.hand_modifier, Some(-1));
    assert_eq!(c.life_modifier, Some(5));
    assert_eq!(c.rules_text, f.text);
    assert_eq!(c.abilities, f.abilities);
    assert_eq!(c.mana_value(OFF), 2);
    assert_eq!(c.mana_value(on(1, 1, 1)), 2);
}

// REQ-OBJ-001, CR 205.2a, 205.3c: from_face returns the validation error.
#[test]
fn from_face_propagates_errors() {
    let v = vocab();
    let bad_type = face("test.bad", None, &["Nonsense"]);
    assert_eq!(
        Characteristics::from_face(&bad_type, v),
        Err(ObjectError::UnknownCardType("Nonsense".to_owned()))
    );
    let mut bad_super = face("test.bad", None, &["Land"]);
    bad_super.supertypes = vec!["Nonsense".to_owned()];
    assert_eq!(
        Characteristics::from_face(&bad_super, v),
        Err(ObjectError::UnknownSupertype("Nonsense".to_owned()))
    );
    let mut bad_sub = face("test.bad", None, &["Land"]);
    bad_sub.subtypes = vec![first_subtype("Creature")];
    assert!(matches!(
        Characteristics::from_face(&bad_sub, v),
        Err(ObjectError::SubtypeNotCorrelated { .. })
    ));
}

// REQ-OBJ-001, CR 208.3: power and toughness apply per zone and type.
#[test]
fn pt_applies_cases() {
    let v = vocab();
    let mut noncreature = face("test.vehicle_like_3_3", Some(cost("{3}")), &["Artifact"]);
    noncreature.power = Some(ValueExpressionAST(3.into()));
    noncreature.toughness = Some(ValueExpressionAST(3.into()));
    let nc = Characteristics::from_face(&noncreature, v).unwrap();
    assert!(nc.pt_applies(false));
    assert!(!nc.pt_applies(true));

    let mut creature = face("test.vanilla_2_2", Some(cost("{1}{G}")), &["Creature"]);
    creature.power = Some(ValueExpressionAST(2.into()));
    creature.toughness = Some(ValueExpressionAST(2.into()));
    let cr = Characteristics::from_face(&creature, v).unwrap();
    assert!(cr.pt_applies(false));
    assert!(cr.pt_applies(true));

    let none = Characteristics::from_face(&face("test.rock", None, &["Artifact"]), v).unwrap();
    assert!(!none.pt_applies(false));
    assert!(!none.pt_applies(true));

    // Only one of power or toughness printed still counts as printed.
    let mut half = face("test.half_pt", None, &["Creature"]);
    half.toughness = Some(ValueExpressionAST(1.into()));
    let h = Characteristics::from_face(&half, v).unwrap();
    assert!(h.pt_applies(true));

    // CR 208.5 (CodeRabbit r1): a creature with no printed power or toughness still has them
    // (each 0) on and off the battlefield.
    let bare = Characteristics::from_face(&face("test.cda_like_creature", None, &["Creature"]), v)
        .unwrap();
    assert!(bare.pt_applies(false));
    assert!(bare.pt_applies(true));
}

// REQ-OBJ-001, CR 208.3, 208.5: creatures have P/T in every zone; noncreatures only off the
// battlefield and only when printed (CodeRabbit r1 regression).
#[test]
fn pt_applies_creature_status_separate_from_printed() {
    let v = vocab();
    let bare =
        Characteristics::from_face(&face("test.bare_creature", None, &["Creature"]), v).unwrap();
    assert!(bare.pt_applies(true));
    assert!(bare.pt_applies(false));

    let mut power_only = face("test.power_only_artifact", None, &["Artifact"]);
    power_only.power = Some(ValueExpressionAST(2.into()));
    let po = Characteristics::from_face(&power_only, v).unwrap();
    assert!(!po.pt_applies(true));
    assert!(po.pt_applies(false));

    let plain =
        Characteristics::from_face(&face("test.plain_enchantment", None, &["Enchantment"]), v)
            .unwrap();
    assert!(!plain.pt_applies(true));
    assert!(!plain.pt_applies(false));
}
