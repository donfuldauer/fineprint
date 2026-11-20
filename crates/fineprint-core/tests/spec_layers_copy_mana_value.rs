// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the mana value of copiable values and their copies (REQ-OBJ-002; CR 202.3a,
//! 202.3b, 202.3c, 202.3e, 713.5c): nonmodal back faces, melded permanents, {X} costs and no
//! mana cost. Black-box: written from the work-item brief only.

#[path = "spec_layers_copy_support.rs"]
mod support;

use fineprint_core::layers::{CopiableValues, ValueSource};
use fineprint_core::layouts::MeldedPair;
use fineprint_core::objects::{Characteristics, ManaValueContext, VariableValues};
use support::*;

fn on_stack(x: u32) -> ManaValueContext {
    ManaValueContext::OnStack(VariableValues { x, y: 0, z: 0 })
}

/// A synthetic back face: a 3/3 green creature (indicator) with no mana cost.
fn back_face() -> Characteristics {
    Characteristics {
        color_indicator: Some(ColorBitmask::GREEN),
        colors: ColorBitmask::GREEN,
        subtypes: set(&["Wolf"]),
        ..creature("test.dfc_back_3_3", 3, 3)
    }
}

/// REQ-OBJ-002, CR 202.3b: the back face of a nonmodal DFC has the mana value of its front
/// face's cost (X counted as 0); a copy of it has mana value 0, and so does a copy of that copy.
#[test]
fn back_face_202_3b_original_front_mv_copies_zero() {
    let orig = CopiableValues::nonmodal_back_face(back_face(), some_cost("{X}{2}{G}"));
    assert_eq!(orig.characteristics, back_face());
    assert_eq!(orig.source, ValueSource::Fixed { mana_value: 3 });
    assert_eq!(orig.mana_value(ManaValueContext::OffStack), 3);

    let copy = orig.copy_with(&own_b(), &[]);
    assert_eq!(copy.characteristics, back_face());
    assert_eq!(copy.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(copy.mana_value(ManaValueContext::OffStack), 0);
    assert_eq!(copy.mana_value(on_stack(4)), 0);

    let copy2 = copy.copy_with(&own_c(), &[]);
    assert_eq!(copy2.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(copy2.mana_value(ManaValueContext::OffStack), 0);
    assert_eq!(copy2.mana_value(on_stack(4)), 0);
}

/// REQ-OBJ-002, CR 202.3a, 202.3b: a back face whose front face has no mana cost has mana
/// value 0; its copy too.
#[test]
fn back_face_with_costless_front_is_zero() {
    let orig = CopiableValues::nonmodal_back_face(back_face(), None);
    assert_eq!(orig.mana_value(ManaValueContext::OffStack), 0);
    assert_eq!(
        orig.copy_with(&own_b(), &[])
            .mana_value(ManaValueContext::OffStack),
        0
    );
}

/// REQ-OBJ-002, CR 202.3b: the back face's mana value never uses its own cost, even when the
/// back-face characteristics carry one.
#[test]
fn back_face_ignores_its_own_cost() {
    let mut c = back_face();
    c.mana_cost = some_cost("{9}");
    let orig = CopiableValues::nonmodal_back_face(c, some_cost("{1}{G}"));
    assert_eq!(orig.mana_value(ManaValueContext::OffStack), 2);
}

/// REQ-OBJ-002, CR 202.3c, 713.5c: a melded permanent has the combined back face's
/// characteristics and the sum of its front faces' mana values; a copy of it has mana value
/// 0, and so does a copy of that copy.
#[test]
fn melded_202_3c_sum_of_fronts_copies_zero() {
    let (a, b) = (meld_a(), meld_b());
    let pair = MeldedPair::new(&a, &b).expect("meld pair");
    let melded = CopiableValues::melded(&pair, vocab()).expect("melded values");
    assert_eq!(
        melded.characteristics,
        pair.characteristics(vocab()).unwrap()
    );
    assert_eq!(melded.characteristics.names, set(&["test.meld_whole"]));
    // {2}{W} = 3 plus {X}{3}{U} = 4 (X = 0); the combined face's own {9}{B} is not used.
    assert_eq!(melded.mana_value(ManaValueContext::OffStack), 7);
    assert_eq!(pair.mana_value(false, vocab()), Ok(7));
    assert_ne!(melded.source, ValueSource::CopyOfBackFaceOrMelded);

    let copy = melded.copy_with(&own_b(), &[]);
    assert_eq!(copy.characteristics, melded.characteristics);
    assert_eq!(copy.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(copy.mana_value(ManaValueContext::OffStack), 0);
    assert_eq!(copy.mana_value(on_stack(5)), 0);
    assert_eq!(pair.mana_value(true, vocab()), Ok(0));

    let copy2 = copy.copy_with(&own_c(), &[]);
    assert_eq!(copy2.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(copy2.mana_value(ManaValueContext::OffStack), 0);
}

/// REQ-OBJ-002, CR 202.3c: the melded values do not depend on the order of the pair.
#[test]
fn melded_values_do_not_depend_on_pair_order() {
    let (a, b) = (meld_a(), meld_b());
    let ab = CopiableValues::melded(&MeldedPair::new(&a, &b).unwrap(), vocab()).unwrap();
    let ba = CopiableValues::melded(&MeldedPair::new(&b, &a).unwrap(), vocab()).unwrap();
    assert_eq!(ab.characteristics, ba.characteristics);
    assert_eq!(ab.source, ba.source);
}

/// REQ-OBJ-002, CR 202.3e: a plain value with {X} in its cost has X = 0 off the stack and the
/// chosen number on it; so does its copy, and the copy stays plain.
#[test]
fn plain_x_cost_202_3e_original_and_copy() {
    let c = Characteristics {
        mana_cost: some_cost("{X}{1}{G}"),
        colors: ColorBitmask::GREEN,
        ..creature("test.x_creature", 1, 1)
    };
    let orig = CopiableValues::plain(c);
    assert_eq!(orig.source, ValueSource::Plain);
    assert_eq!(orig.mana_value(ManaValueContext::OffStack), 2);
    assert_eq!(orig.mana_value(on_stack(3)), 5);
    let copy = orig.copy_with(&own_b(), &[]);
    assert_eq!(copy.source, ValueSource::Plain);
    assert_eq!(copy.mana_value(ManaValueContext::OffStack), 2);
    assert_eq!(copy.mana_value(on_stack(3)), 5);
    assert_eq!(copy.mana_value(on_stack(0)), 2);
}

/// REQ-OBJ-002, CR 202.3a: a copy of a plain object with no mana cost has mana value 0 (the
/// copier's own cost is not used).
#[test]
fn plain_no_cost_202_3a_copy_is_zero() {
    let orig = CopiableValues::plain(creature("test.costless_2_2", 2, 2));
    assert_eq!(orig.mana_value(ManaValueContext::OffStack), 0);
    let copy = orig.copy_with(&own_b(), &[]);
    assert_eq!(copy.characteristics.mana_cost, None);
    assert_eq!(copy.mana_value(ManaValueContext::OffStack), 0);
    assert_eq!(copy.mana_value(on_stack(2)), 0);
}

/// REQ-OBJ-002, CR 202.3b, 202.3c: a non-copy layer-1a change ("as enters" P/T) of a copy of a
/// back face keeps mana value 0; of a melded permanent keeps the sum.
#[test]
fn modified_keeps_the_mana_value_source() {
    let ex = [
        fineprint_core::layers::CopyException::SetPower(lit(1)),
        fineprint_core::layers::CopyException::SetToughness(lit(1)),
    ];
    let copy = CopiableValues::nonmodal_back_face(back_face(), some_cost("{2}{G}"))
        .copy_with(&own_b(), &[]);
    let m = copy.modified(&ex);
    assert_eq!(m.source, ValueSource::CopyOfBackFaceOrMelded);
    assert_eq!(m.mana_value(ManaValueContext::OffStack), 0);

    let (a, b) = (meld_a(), meld_b());
    let melded = CopiableValues::melded(&MeldedPair::new(&a, &b).unwrap(), vocab()).unwrap();
    let m = melded.modified(&ex);
    assert_eq!(m.source, melded.source);
    assert_eq!(m.mana_value(ManaValueContext::OffStack), 7);
}
