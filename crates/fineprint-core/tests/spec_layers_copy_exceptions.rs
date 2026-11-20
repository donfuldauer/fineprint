// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for exceptions to the copying process (REQ-OBJ-002; CR 707.9a-d): one test per
//! `CopyException` variant, ordering, `NotCopied` for every `CopiedCharacteristic`, and the
//! exceptions as part of the copy's copiable values. Black-box: written from the work-item brief
//! only.

#[path = "spec_layers_copy_support.rs"]
mod support;

use fineprint_core::layers::{
    ContinuousEffects, CopiableValues, CopyException, ValueSource, copiable_characteristics,
};
use fineprint_core::objects::Characteristics;
use support::*;

/// `rich_a()` with a CDA-like (non-literal) power and toughness.
fn cda_a() -> Characteristics {
    Characteristics {
        power: Some(cda_like("count")),
        toughness: Some(cda_like("count_plus_one")),
        ..rich_a()
    }
}

fn copy(ex: &[CopyException]) -> Characteristics {
    CopiableValues::plain(rich_a())
        .copy_with(&own_b(), ex)
        .characteristics
}

/// REQ-OBJ-002, CR 707.2: with no exception the copy is exactly the original's copiable values,
/// and none of the copier's own values survive.
#[test]
fn exceptions_empty_slice_is_a_plain_copy() {
    let v = CopiableValues::plain(rich_a()).copy_with(&own_b(), &[]);
    assert_eq!(v.characteristics, rich_a());
    assert_eq!(v.source, ValueSource::Plain);
}

/// REQ-OBJ-002, CR 707.9a: a gained ability is appended to the copied abilities.
#[test]
fn exception_707_9a_gain_ability_appends() {
    let g = json_ability("test.gained", 1);
    let got = copy(&[CopyException::GainAbility(g.clone())]);
    let mut want = rich_a();
    want.abilities.push(g);
    assert_eq!(got, want);
}

/// REQ-OBJ-002, CR 707.9a: the gained ability is part of the copy's copiable values, so a
/// later copy of the copy has it too.
#[test]
fn exception_707_9a_gained_ability_is_copied_by_a_later_copy() {
    let g = json_ability("test.gained", 1);
    let vb = CopiableValues::plain(rich_a())
        .copy_with(&own_b(), &[CopyException::GainAbility(g.clone())]);
    let vc = vb.copy_with(&own_c(), &[]);
    assert!(vc.characteristics.abilities.contains(&g));
    assert_eq!(vc.characteristics, vb.characteristics);
}

/// REQ-OBJ-002, CR 707.9b: `SetNames` replaces the names.
#[test]
fn exception_707_9b_set_names() {
    let got = copy(&[CopyException::SetNames(set(&["test.renamed"]))]);
    let mut want = rich_a();
    want.names = set(&["test.renamed"]);
    assert_eq!(got, want);
    // An empty set of names is a copy with no name.
    let got = copy(&[CopyException::SetNames(set(&[]))]);
    assert!(got.names.is_empty());
}

/// REQ-OBJ-002, CR 707.9b: `SetManaCost` replaces the mana cost (`None` is no mana cost). The
/// colors are not asserted (the brief does not say whether they follow; see DECISIONS.md).
#[test]
fn exception_707_9b_set_mana_cost() {
    for c in [some_cost("{4}{B}"), None] {
        let mut got = copy(&[CopyException::SetManaCost(c.clone())]);
        assert_eq!(got.mana_cost, c);
        let mut want = rich_a();
        want.mana_cost = c;
        got.colors = want.colors;
        assert_eq!(got, want);
    }
}

/// REQ-OBJ-002, CR 707.9b, 707.9d: `SetColors` sets the colors and the copied color indicator
/// is not copied.
#[test]
fn exception_707_9d_set_colors_clears_the_copied_color_indicator() {
    let got = copy(&[CopyException::SetColors(ColorBitmask::WHITE)]);
    let mut want = rich_a();
    want.colors = ColorBitmask::WHITE;
    want.color_indicator = None;
    assert_eq!(got, want);
}

/// REQ-OBJ-002, CR 707.9b: `SetSupertypes`, `SetCardTypes`, `SetSubtypes` replace.
#[test]
fn exception_707_9b_set_types_replace() {
    let got = copy(&[CopyException::SetSupertypes(set(&["Snow"]))]);
    let mut want = rich_a();
    want.supertypes = set(&["Snow"]);
    assert_eq!(got, want);

    let got = copy(&[CopyException::SetCardTypes(set(&["Enchantment"]))]);
    let mut want = rich_a();
    want.card_types = set(&["Enchantment"]);
    assert_eq!(got, want);

    let got = copy(&[CopyException::SetSubtypes(set(&["Spirit"]))]);
    let mut want = rich_a();
    want.subtypes = set(&["Spirit"]);
    assert_eq!(got, want);
}

/// REQ-OBJ-002, CR 707.9b, 707.9d ("in addition to its other types"): `AddSupertypes`,
/// `AddCardTypes`, `AddSubtypes` keep the copied set and add to it.
#[test]
fn exception_707_9d_add_types_keep_the_copied_set() {
    let got = copy(&[CopyException::AddSupertypes(set(&["Snow"]))]);
    let mut want = rich_a();
    want.supertypes = set(&["Legendary", "Snow"]);
    assert_eq!(got, want);

    let got = copy(&[CopyException::AddCardTypes(set(&["Enchantment"]))]);
    let mut want = rich_a();
    want.card_types = set(&["Artifact", "Creature", "Enchantment"]);
    assert_eq!(got, want);

    let got = copy(&[CopyException::AddSubtypes(set(&["Spirit"]))]);
    let mut want = rich_a();
    want.subtypes = set(&["Elf", "Spirit"]);
    assert_eq!(got, want);

    // Adding a type the original already has changes nothing.
    assert_eq!(copy(&[CopyException::AddSubtypes(set(&["Elf"]))]), rich_a());
}

/// REQ-OBJ-002, CR 707.9b, 707.9d: `SetPower` / `SetToughness` replace a CDA-like expression of
/// the original with exactly the given literal (the CDA expression is not copied).
#[test]
fn exception_707_9d_set_power_and_toughness_replace_a_cda_expression() {
    let a = CopiableValues::plain(cda_a());
    let got = a
        .copy_with(&own_b(), &[CopyException::SetPower(lit(4))])
        .characteristics;
    assert_eq!(got.power, Some(lit(4)));
    assert_eq!(got.toughness, Some(cda_like("count_plus_one")));
    let mut want = cda_a();
    want.power = Some(lit(4));
    assert_eq!(got, want);

    let got = a
        .copy_with(&own_b(), &[CopyException::SetToughness(lit(5))])
        .characteristics;
    assert_eq!(got.toughness, Some(lit(5)));
    assert_eq!(got.power, Some(cda_like("count")));

    let got = a
        .copy_with(
            &own_b(),
            &[
                CopyException::SetPower(lit(0)),
                CopyException::SetToughness(lit(1)),
            ],
        )
        .characteristics;
    assert_eq!(got.power, Some(lit(0)));
    assert_eq!(got.toughness, Some(lit(1)));
    // Without an exception the CDA-like expressions are copied as they are.
    let plain = a.copy_with(&own_b(), &[]).characteristics;
    assert_eq!(plain, cda_a());
}

/// REQ-OBJ-002, CR 707.9c: `NotCopied(c)`, for every characteristic `c`: the copier keeps its
/// own value for that characteristic; everything else is copied. `NotCopied(Colors)` also keeps
/// the copier's color indicator (CR 707.9d).
#[test]
fn exception_707_9c_not_copied_every_characteristic() {
    for which in ALL_COPIED {
        let got = copy(&[CopyException::NotCopied(which)]);
        let mut want = rich_a();
        take_field(which, &mut want, &own_b());
        assert_eq!(got, want, "NotCopied({which:?})");
    }
}

/// REQ-OBJ-002, CR 707.9d: `NotCopied(Colors)` keeps the copier's colors and color indicator,
/// even when the copier has none.
#[test]
fn exception_707_9d_not_copied_colors_keeps_own_indicator() {
    let got = copy(&[CopyException::NotCopied(
        fineprint_core::layers::CopiedCharacteristic::Colors,
    )]);
    assert_eq!(got.colors, own_b().colors);
    assert_eq!(got.color_indicator, own_b().color_indicator);

    let got = CopiableValues::plain(rich_a())
        .copy_with(
            &own_c(),
            &[CopyException::NotCopied(
                fineprint_core::layers::CopiedCharacteristic::Colors,
            )],
        )
        .characteristics;
    assert_eq!(got.colors, ColorBitmask::NONE);
    assert_eq!(got.color_indicator, None);
}

/// REQ-OBJ-002, CR 707.9c: `NotCopied` of a power with a CDA-like copier value keeps it.
#[test]
fn exception_707_9c_not_copied_power_keeps_own_expression() {
    let got = CopiableValues::plain(rich_a())
        .copy_with(
            &cda_a(),
            &[CopyException::NotCopied(
                fineprint_core::layers::CopiedCharacteristic::Power,
            )],
        )
        .characteristics;
    assert_eq!(got.power, Some(cda_like("count")));
    assert_eq!(got.toughness, Some(lit(4)));
}

/// REQ-OBJ-002, CR 707.9b: exceptions apply in slice order: two on one field, the last wins.
#[test]
fn exceptions_apply_in_order_last_wins() {
    let got = copy(&[
        CopyException::SetNames(set(&["test.first"])),
        CopyException::SetNames(set(&["test.second"])),
    ]);
    assert_eq!(got.names, set(&["test.second"]));
    let got = copy(&[
        CopyException::SetPower(lit(1)),
        CopyException::SetPower(lit(8)),
    ]);
    assert_eq!(got.power, Some(lit(8)));
    let got = copy(&[
        CopyException::SetColors(ColorBitmask::WHITE),
        CopyException::SetColors(ColorBitmask::GREEN),
    ]);
    assert_eq!(got.colors, ColorBitmask::GREEN);
    // A NotCopied after a Set restores the copier's own value; the reverse keeps the Set.
    let got = copy(&[
        CopyException::SetNames(set(&["test.first"])),
        CopyException::NotCopied(fineprint_core::layers::CopiedCharacteristic::Names),
    ]);
    assert_eq!(got.names, own_b().names);
    let got = copy(&[
        CopyException::NotCopied(fineprint_core::layers::CopiedCharacteristic::Names),
        CopyException::SetNames(set(&["test.first"])),
    ]);
    assert_eq!(got.names, set(&["test.first"]));
}

/// REQ-OBJ-002, CR 707.9b, 707.9d: `SetCardTypes` then `AddCardTypes` is the union of the two;
/// `AddCardTypes` then `SetCardTypes` is only the set value.
#[test]
fn exceptions_set_then_add_versus_add_then_set() {
    let got = copy(&[
        CopyException::SetCardTypes(set(&["Enchantment"])),
        CopyException::AddCardTypes(set(&["Creature"])),
    ]);
    assert_eq!(got.card_types, set(&["Creature", "Enchantment"]));
    let got = copy(&[
        CopyException::AddCardTypes(set(&["Creature"])),
        CopyException::SetCardTypes(set(&["Enchantment"])),
    ]);
    assert_eq!(got.card_types, set(&["Enchantment"]));
    // Same for subtypes and supertypes.
    let got = copy(&[
        CopyException::SetSubtypes(set(&["Spirit"])),
        CopyException::AddSubtypes(set(&["Elf"])),
    ]);
    assert_eq!(got.subtypes, set(&["Elf", "Spirit"]));
    let got = copy(&[
        CopyException::AddSupertypes(set(&["Snow"])),
        CopyException::SetSupertypes(set(&["Basic"])),
    ]);
    assert_eq!(got.supertypes, set(&["Basic"]));
}

/// REQ-OBJ-002, CR 707.9a, 707.9b: the exceptions' values are part of the copy's copiable
/// values: after the copy effect is registered, `copiable_characteristics` of the copier shows
/// them, and a copy of the copier has them.
#[test]
fn exceptions_707_9b_are_part_of_the_copiable_values() {
    let mut env = TestEnv::new();
    env.put(oid(1), rich_a());
    env.put(oid(2), own_b());
    env.put(oid(3), own_c());
    let g = json_ability("test.gained", 1);
    let ex = [
        CopyException::GainAbility(g.clone()),
        CopyException::SetNames(set(&["test.renamed"])),
        CopyException::AddCardTypes(set(&["Enchantment"])),
        CopyException::SetColors(ColorBitmask::BLACK),
        CopyException::SetPower(lit(1)),
        CopyException::NotCopied(fineprint_core::layers::CopiedCharacteristic::Loyalty),
    ];
    let vb = CopiableValues::plain(rich_a()).copy_with(&own_b(), &ex);
    let mut want = rich_a();
    want.abilities.push(g);
    want.names = set(&["test.renamed"]);
    want.card_types = set(&["Artifact", "Creature", "Enchantment"]);
    want.colors = ColorBitmask::BLACK;
    want.color_indicator = None;
    want.power = Some(lit(1));
    want.loyalty = own_b().loyalty;
    assert_eq!(vb.characteristics, want);

    let mut effects = ContinuousEffects::new();
    effects
        .register(eid(1), copy_eff(&vb, 10, &[oid(2)]))
        .unwrap();
    let read_back = copiable_characteristics(&effects, &env, oid(2)).unwrap();
    assert_eq!(norm(&read_back), norm(&want));

    let vc = CopiableValues::plain(read_back).copy_with(&own_c(), &[]);
    effects
        .register(eid(2), copy_eff(&vc, 20, &[oid(3)]))
        .unwrap();
    assert_eq!(eval_chars(&effects, &env, oid(3)), norm(&want));
}
