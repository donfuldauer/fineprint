// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the per-part drop of characteristic-defining abilities by copy exceptions
//! (REQ-OBJ-002; CR 707.9a, 707.9b, 707.9c, 707.9d, 604.3a, 109.3, 205.1b): a layer-4 CDA that
//! declares which type-line parts it defines (`"defines"` in its modification) is not copied
//! only when a copy exception sets, retains or does not copy one of those parts; `Add*`
//! ("in addition to its other types") exceptions keep every type CDA; a CDA defining several
//! parts goes as a whole as soon as one of them is touched (recorded policy, docs/DECISIONS.md).
//! Black-box: written from the work-item brief only, through the public API
//! (`CopiableValues::plain(..).copy_with(..)`). Synthetic data only (R5); pseudo-random inputs
//! come from a hand-written LCG (no `rand`, R3).

use std::collections::BTreeSet;

use fineprint_core::layers::{CopiableValues, CopiedCharacteristic, CopyException};
use fineprint_core::objects::Characteristics;
use fineprint_dsl::card::{AbilityAST, ColorBitmask, ValueExpressionAST};
use serde_json::{Value, json};

#[path = "typed_ast_support/mod.rs"]
mod typed_ast_support;
use typed_ast_support::{ability_of, synthetic_ability, tx};

// ---------------------------------------------------------------------------------------------
// helpers (local copies; R1: no shared helper is edited)
// ---------------------------------------------------------------------------------------------

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn mask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).expect("valid mask")
}

/// A literal power/toughness `Literal(n)` (#232 FP-164: formerly the bare-integer placeholder).
fn lit(n: i64) -> ValueExpressionAST {
    ValueExpressionAST::Literal(n)
}

/// `v` as a typed ability; panics when it is not valid.
fn ab(v: Value) -> AbilityAST {
    ability_of(v)
}

/// `v` as a typed ability, if it decodes (#232 FP-164).
fn try_ab(v: Value) -> Option<AbilityAST> {
    serde_json::from_value(v).ok()
}

/// A CHAR effect of `layer` on its own object with the given modification and `isCDA` member.
fn char_effect(layer: &str, modification: Value, is_cda: Option<bool>) -> Value {
    let mut e = json!({
        "type": "CHAR",
        "layer": layer,
        "affected": {"ref": "SELF"},
        "modification": modification,
    });
    if let Some(b) = is_cda {
        e.as_object_mut()
            .unwrap()
            .insert("isCDA".to_string(), json!(b));
    }
    e
}

/// A STATIC ability `id` with the given effects.
fn static_with(id: &str, effects: Vec<Value>) -> AbilityAST {
    ab(json!({"abilityId": id, "kind": "STATIC", "effects": effects}))
}

/// The JSON of a layer-4 CDA whose modification carries `defines` as the raw JSON value given
/// (`None`: the member is absent, which means all three type-line parts).
fn cda4_json(id: &str, defines: Option<Value>) -> Value {
    let mut m = json!({"op": "ANY_OP"});
    if let Some(d) = defines {
        m.as_object_mut().unwrap().insert("defines".to_string(), d);
    }
    json!({"abilityId": id, "kind": "STATIC", "effects": [char_effect("4", m, Some(true))]})
}

/// A layer-4 CDA whose modification carries `defines` (see [`cda4_json`]).
fn cda4_raw(id: &str, defines: Option<Value>) -> AbilityAST {
    ab(cda4_json(id, defines))
}

/// A layer-4 CDA defining exactly `parts` (each one of `SUPERTYPES`, `CARD_TYPES`,
/// `SUBTYPES`).
fn cda4(id: &str, parts: &[&str]) -> AbilityAST {
    cda4_raw(id, Some(json!(parts)))
}

/// A layer-4 CDA without `defines` (old behaviour: all three type-line parts).
fn cda4_all(id: &str) -> AbilityAST {
    cda4_raw(id, None)
}

/// A layer-5 (colors) CDA.
fn cda5(id: &str) -> AbilityAST {
    static_with(
        id,
        vec![char_effect("5", json!({"op": "ANY_OP"}), Some(true))],
    )
}

/// A layer-7a (power/toughness) CDA.
fn cda7a(id: &str) -> AbilityAST {
    static_with(
        id,
        vec![char_effect("7a", json!({"op": "ANY_OP"}), Some(true))],
    )
}

/// A plain opaque test ability (#232 FP-164: a valid inert ability instead of `{id: 1}`).
fn plain(id: &str) -> AbilityAST {
    synthetic_ability(id)
}

/// The copied original: a legendary artifact creature Elf with the given abilities.
fn source(abilities: Vec<AbilityAST>) -> Characteristics {
    Characteristics {
        names: set(&["test.cda_source"]),
        mana_cost: None,
        color_indicator: Some(ColorBitmask::RED),
        colors: mask(2 | 8),
        supertypes: set(&["Legendary"]),
        card_types: set(&["Artifact", "Creature"]),
        subtypes: set(&["Elf"]),
        rules_text: tx(vec![
            json!({"id": "w0", "kind": "LITERAL", "text": "test.cda_source"}),
        ]),
        abilities,
        power: Some(lit(3)),
        toughness: Some(lit(4)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// The copier's own values: a snow enchantment Shrine.
fn own() -> Characteristics {
    Characteristics {
        names: set(&["test.cda_copier"]),
        mana_cost: None,
        color_indicator: Some(ColorBitmask::BLACK),
        colors: mask(1 | 4),
        supertypes: set(&["Snow"]),
        card_types: set(&["Enchantment"]),
        subtypes: set(&["Shrine"]),
        rules_text: tx(vec![
            json!({"id": "w0", "kind": "LITERAL", "text": "test.cda_copier"}),
        ]),
        abilities: vec![plain("test.cda_copier.own")],
        power: Some(lit(1)),
        toughness: Some(lit(1)),
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

fn copy(src: &Characteristics, ex: &[CopyException]) -> Characteristics {
    CopiableValues::plain(src.clone())
        .copy_with(&own(), ex)
        .characteristics
}

/// `c` with its abilities removed (to compare everything but the abilities).
fn no_abilities(c: &Characteristics) -> Characteristics {
    Characteristics {
        abilities: Vec::new(),
        ..c.clone()
    }
}

/// A type-line part (CR 109.3: supertypes, card types and subtypes are separate
/// characteristics).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Sup,
    Card,
    Sub,
}

const PARTS: [Part; 3] = [Part::Sup, Part::Card, Part::Sub];

/// `SetX` of `part` (CR 707.9b: provides a specific set of values).
fn set_ex(part: Part) -> CopyException {
    match part {
        Part::Sup => CopyException::SetSupertypes(set(&["Basic"])),
        Part::Card => CopyException::SetCardTypes(set(&["Land"])),
        Part::Sub => CopyException::SetSubtypes(set(&["Spirit"])),
    }
}

/// `NotCopied(X)` of `part` (CR 707.9c: the characteristic is not copied).
fn not_copied_ex(part: Part) -> CopyException {
    CopyException::NotCopied(match part {
        Part::Sup => CopiedCharacteristic::Supertypes,
        Part::Card => CopiedCharacteristic::CardTypes,
        Part::Sub => CopiedCharacteristic::Subtypes,
    })
}

/// `AddX` of `part` (CR 707.9d last sentence: "in addition to its other types").
fn add_ex(part: Part) -> CopyException {
    match part {
        Part::Sup => CopyException::AddSupertypes(set(&["Snow"])),
        Part::Card => CopyException::AddCardTypes(set(&["Enchantment"])),
        Part::Sub => CopyException::AddSubtypes(set(&["Spirit"])),
    }
}

/// The six type-line exceptions that touch `part`'s CDAs, labelled.
fn touching(part: Part) -> [(String, CopyException); 2] {
    [
        (format!("Set({part:?})"), set_ex(part)),
        (format!("NotCopied({part:?})"), not_copied_ex(part)),
    ]
}

/// Checks that copying `src` (whose abilities are `plain(a), cda, plain(b)`) with `ex` keeps or
/// drops `cda` as `dropped` says, keeps the surrounding abilities in order, and leaves every
/// other characteristic as the same copy of a source without the CDA would have it.
fn check_one(cda: &AbilityAST, ex: &[CopyException], dropped: bool, label: &str) {
    let before = plain("test.before");
    let after = plain("test.after");
    let src = source(vec![before.clone(), cda.clone(), after.clone()]);
    let got = copy(&src, ex);
    let want_abilities = if dropped {
        vec![before.clone(), after.clone()]
    } else {
        vec![before.clone(), cda.clone(), after.clone()]
    };
    assert_eq!(got.abilities, want_abilities, "{label}");
    // Dropping a CDA does not change the exception results (characteristics still follow
    // the exceptions).
    let reference = copy(&source(vec![before, after]), ex);
    assert_eq!(no_abilities(&got), no_abilities(&reference), "{label}");
}

// ---------------------------------------------------------------------------------------------
// the per-part matrix
// ---------------------------------------------------------------------------------------------

/// One row of the matrix: the CDA and whether an exception on supertypes, card types or
/// subtypes (in that order) drops it.
struct Row {
    label: &'static str,
    cda: AbilityAST,
    drop_by: [bool; 3],
}

fn matrix() -> Vec<Row> {
    const D: bool = true;
    const K: bool = false;
    vec![
        Row {
            label: "[SUBTYPES]",
            cda: cda4("test.cda.sub", &["SUBTYPES"]),
            drop_by: [K, K, D],
        },
        Row {
            label: "[SUPERTYPES]",
            cda: cda4("test.cda.sup", &["SUPERTYPES"]),
            drop_by: [D, K, K],
        },
        Row {
            label: "[CARD_TYPES]",
            cda: cda4("test.cda.card", &["CARD_TYPES"]),
            drop_by: [K, D, K],
        },
        Row {
            label: "[CARD_TYPES,SUBTYPES]",
            cda: cda4("test.cda.card_sub", &["CARD_TYPES", "SUBTYPES"]),
            drop_by: [K, D, D],
        },
        Row {
            label: "[SUBTYPES,CARD_TYPES]",
            cda: cda4("test.cda.sub_card", &["SUBTYPES", "CARD_TYPES"]),
            drop_by: [K, D, D],
        },
        Row {
            label: "[SUPERTYPES,SUBTYPES]",
            cda: cda4("test.cda.sup_sub", &["SUPERTYPES", "SUBTYPES"]),
            drop_by: [D, K, D],
        },
        Row {
            label: "[SUPERTYPES,CARD_TYPES]",
            cda: cda4("test.cda.sup_card", &["SUPERTYPES", "CARD_TYPES"]),
            drop_by: [D, D, K],
        },
        Row {
            label: "[SUPERTYPES,CARD_TYPES,SUBTYPES]",
            cda: cda4("test.cda.all3", &["SUPERTYPES", "CARD_TYPES", "SUBTYPES"]),
            drop_by: [D, D, D],
        },
        Row {
            label: "[SUBTYPES,SUPERTYPES,CARD_TYPES]",
            cda: cda4(
                "test.cda.all3_reordered",
                &["SUBTYPES", "SUPERTYPES", "CARD_TYPES"],
            ),
            drop_by: [D, D, D],
        },
        Row {
            label: "no defines",
            cda: cda4_all("test.cda.none"),
            drop_by: [D, D, D],
        },
    ]
}

/// REQ-OBJ-002, CR 707.9d, 707.9c, 707.9b, 109.3: table-driven. For every CDA row and every
/// type-line part, both `SetX` and `NotCopied(X)` drop the CDA exactly when it defines that
/// part; a CDA defining several parts is dropped as a whole when any one is touched (recorded
/// policy); a layer-4 CDA without `defines` defines all three parts.
#[test]
fn req_obj_002_cda_drop_matrix_707_9d() {
    for row in matrix() {
        for (i, part) in PARTS.into_iter().enumerate() {
            for (exl, ex) in touching(part) {
                check_one(
                    &row.cda,
                    &[ex],
                    row.drop_by[i],
                    &format!("{} under {exl}", row.label),
                );
            }
        }
    }
}

/// REQ-OBJ-002, CR 707.9d (acceptance criterion): a subtype-only layer-4 CDA survives a
/// `SetSupertypes` exception, and the supertypes are set.
#[test]
fn req_obj_002_subtype_only_cda_survives_set_supertypes() {
    let cda = cda4("test.cda.sub", &["SUBTYPES"]);
    let got = copy(
        &source(vec![cda.clone()]),
        &[CopyException::SetSupertypes(set(&["Basic"]))],
    );
    assert_eq!(got.abilities, vec![cda]);
    assert_eq!(got.supertypes, set(&["Basic"]));
    assert_eq!(got.subtypes, set(&["Elf"]));
}

/// REQ-OBJ-002, CR 707.9d (acceptance criterion; recorded policy: a CDA is not copied as a
/// whole as soon as it defines any characteristic the exception touches): a
/// `["CARD_TYPES","SUBTYPES"]` CDA under a `SetSubtypes` exception is dropped entirely (its card
/// type part goes too), under `SetCardTypes` likewise, and survives `SetSupertypes`.
#[test]
fn req_obj_002_multi_part_cda_partial_exception_drops_whole() {
    let cda = cda4("test.cda.card_sub", &["CARD_TYPES", "SUBTYPES"]);
    let src = source(vec![cda.clone()]);
    let got = copy(&src, &[CopyException::SetSubtypes(set(&["Spirit"]))]);
    assert!(got.abilities.is_empty(), "{:?}", got.abilities);
    assert_eq!(got.subtypes, set(&["Spirit"]));
    // The card types themselves are still the copied ones (the exception is on subtypes).
    assert_eq!(got.card_types, set(&["Artifact", "Creature"]));
    let got = copy(&src, &[CopyException::SetCardTypes(set(&["Land"]))]);
    assert!(got.abilities.is_empty(), "{:?}", got.abilities);
    let got = copy(&src, &[CopyException::SetSupertypes(set(&["Basic"]))]);
    assert_eq!(got.abilities, vec![cda]);
}

/// REQ-OBJ-002, CR 707.9d: with several exceptions in one copy, the dropped CDAs are the union
/// of what each exception drops.
#[test]
fn req_obj_002_several_exceptions_drop_the_union() {
    let sub = cda4("test.cda.sub", &["SUBTYPES"]);
    let sup = cda4("test.cda.sup", &["SUPERTYPES"]);
    let card = cda4("test.cda.card", &["CARD_TYPES"]);
    let src = source(vec![sub.clone(), sup.clone(), card.clone()]);
    for ex in [
        vec![
            CopyException::SetSupertypes(set(&["Basic"])),
            CopyException::SetSubtypes(set(&["Spirit"])),
        ],
        vec![
            CopyException::SetSubtypes(set(&["Spirit"])),
            CopyException::SetSupertypes(set(&["Basic"])),
        ],
        vec![not_copied_ex(Part::Sup), set_ex(Part::Sub)],
        vec![set_ex(Part::Sup), not_copied_ex(Part::Sub)],
    ] {
        let got = copy(&src, &ex);
        assert_eq!(got.abilities, vec![card.clone()], "{ex:?}");
    }
    let got = copy(&src, &[set_ex(Part::Card), not_copied_ex(Part::Sub)]);
    assert_eq!(got.abilities, vec![sup.clone()]);
    let got = copy(
        &src,
        &[set_ex(Part::Card), set_ex(Part::Sub), set_ex(Part::Sup)],
    );
    assert!(got.abilities.is_empty());
    // Set and NotCopied of one part together drop just that part's CDAs.
    let got = copy(&src, &[set_ex(Part::Sub), not_copied_ex(Part::Sub)]);
    assert_eq!(got.abilities, vec![sup, card]);
}

// ---------------------------------------------------------------------------------------------
// "in addition to its other types" and names
// ---------------------------------------------------------------------------------------------

fn all_type_cdas() -> Vec<AbilityAST> {
    matrix().into_iter().map(|r| r.cda).collect()
}

fn add_and_names_pool() -> Vec<CopyException> {
    vec![
        add_ex(Part::Sup),
        add_ex(Part::Card),
        add_ex(Part::Sub),
        CopyException::SetNames(set(&["test.renamed"])),
    ]
}

/// REQ-OBJ-002, CR 707.9d last sentence, 205.1b (acceptance criterion): every combination of
/// `AddSupertypes`, `AddCardTypes`, `AddSubtypes` and `SetNames` (every non-empty subset, and
/// every order of all four) keeps every type CDA, including the one without `defines`.
#[test]
fn req_obj_002_add_exceptions_keep_type_cdas_707_9d() {
    let cdas = all_type_cdas();
    let src = source(cdas.clone());
    let pool = add_and_names_pool();
    for bits in 1u32..16 {
        let ex: Vec<CopyException> = (0..4)
            .filter(|i| bits & (1 << i) != 0)
            .map(|i| pool[i].clone())
            .collect();
        let got = copy(&src, &ex);
        assert_eq!(got.abilities, cdas, "{ex:?}");
    }
    for perm in permutations(4) {
        let ex: Vec<CopyException> = perm.iter().map(|&i| pool[i].clone()).collect();
        let got = copy(&src, &ex);
        assert_eq!(got.abilities, cdas, "{ex:?}");
        // The Add* exceptions still add (CR 205.1b).
        assert_eq!(got.supertypes, set(&["Legendary", "Snow"]));
        assert_eq!(
            got.card_types,
            set(&["Artifact", "Creature", "Enchantment"])
        );
        assert_eq!(got.subtypes, set(&["Elf", "Spirit"]));
        assert_eq!(got.names, set(&["test.renamed"]));
    }
}

/// REQ-OBJ-002, CR 707.9d: `AddSubtypes` together with `SetSubtypes` (either order) still drops
/// a subtype CDA (the `Set` applies) and keeps a supertype CDA; likewise for every part.
#[test]
fn req_obj_002_add_with_set_still_drops() {
    for part in PARTS {
        for row in matrix() {
            let i = PARTS.iter().position(|p| *p == part).unwrap();
            for ex in [
                vec![add_ex(part), set_ex(part)],
                vec![set_ex(part), add_ex(part)],
                vec![add_ex(part), not_copied_ex(part)],
            ] {
                check_one(
                    &row.cda,
                    &ex,
                    row.drop_by[i],
                    &format!("{} under {ex:?}", row.label),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// other layers and non-CDAs
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 707.9d: layer-5 (colors) and layer-7a (power/toughness) CDAs are not
/// dropped by any type-line exception, and type CDAs are not dropped by `SetColors` or
/// `SetPower`; `SetColors` drops the layer-5 CDA and `SetPower` the layer-7a CDA (unchanged
/// behaviour).
#[test]
fn req_obj_002_other_layer_cdas_and_type_cdas_are_independent() {
    let c5 = cda5("test.cda.colors");
    let c7 = cda7a("test.cda.pt");
    let types = all_type_cdas();
    let mut abilities = vec![c5.clone(), c7.clone()];
    abilities.extend(types.iter().cloned());
    let src = source(abilities);
    for part in PARTS {
        for (label, ex) in touching(part)
            .into_iter()
            .chain([(format!("Add({part:?})"), add_ex(part))])
        {
            let got = copy(&src, &[ex]);
            assert!(got.abilities.contains(&c5), "{label}");
            assert!(got.abilities.contains(&c7), "{label}");
        }
    }
    let got = copy(&src, &[CopyException::SetColors(ColorBitmask::WHITE)]);
    let mut want = vec![c7.clone()];
    want.extend(types.iter().cloned());
    assert_eq!(got.abilities, want);
    assert_eq!(got.colors, ColorBitmask::WHITE);

    let got = copy(&src, &[CopyException::SetPower(lit(7))]);
    let mut want = vec![c5.clone()];
    want.extend(types.iter().cloned());
    assert_eq!(got.abilities, want);
    assert_eq!(got.power, Some(lit(7)));
}

/// REQ-OBJ-002, CR 707.9d ("doesn't copy a certain characteristic"; color is a defined
/// characteristic): `NotCopied(Colors)` drops the layer-5 CDA and no type or power CDA.
#[test]
fn req_obj_002_not_copied_colors_drops_color_cda_only() {
    let c5 = cda5("test.cda.colors");
    let c7 = cda7a("test.cda.pt");
    let types = all_type_cdas();
    let mut abilities = vec![c5, c7.clone()];
    abilities.extend(types.iter().cloned());
    let got = copy(
        &source(abilities),
        &[CopyException::NotCopied(CopiedCharacteristic::Colors)],
    );
    let mut want = vec![c7];
    want.extend(types);
    assert_eq!(got.abilities, want);
}

/// REQ-OBJ-002, CR 604.3a, 707.9d: only characteristic-defining abilities are dropped. A
/// layer-4 effect with `isCDA` false or absent, a non-STATIC ability carrying a CDA-shaped
/// effect, a STATIC ability without `effects`, and plain test abilities are never dropped by
/// any type-line exception.
#[test]
fn req_obj_002_non_cdas_are_never_dropped() {
    let m_sub = json!({"op": "ANY_OP", "defines": ["SUBTYPES"]});
    let m_none = json!({"op": "ANY_OP"});
    for former in [
        json!({"abilityId": "test.triggered", "kind": "TRIGGERED",
            "effects": [char_effect("4", m_none.clone(), Some(true))]}),
        json!({"abilityId": "test.static.noeffects", "kind": "STATIC"}),
        json!({"kind": "KEYWORD", "abilityId": "test.kw", "keyword": "test_kw"}),
    ] {
        assert!(try_ab(former.clone()).is_none(), "{former}");
    }
    let keep = vec![
        static_with(
            "test.l4.false",
            vec![char_effect("4", m_sub.clone(), Some(false))],
        ),
        static_with(
            "test.l4.absent",
            vec![char_effect("4", m_sub.clone(), None)],
        ),
        static_with(
            "test.l4.false.nodefines",
            vec![char_effect("4", m_none.clone(), Some(false))],
        ),
        static_with(
            "test.l4.absent.nodefines",
            vec![char_effect("4", m_none.clone(), None)],
        ),
        // #232 FP-164: valid typed abilities with the same intent replace the former
        // placeholders (a TRIGGERED ability holding CHAR effects, a STATIC ability without
        // effects, a lower-case keyword), which the typed ability refuses.
        ab(json!({
            "abilityId": "test.triggered",
            "kind": "TRIGGERED",
            "trigger": {"mode": "STATE", "state": {"pred": "IS_CREATURE"}},
            "effect": [],
        })),
        static_with(
            "test.static.noeffects",
            vec![json!({"type": "RULE", "kind": "SKIP"})],
        ),
        plain("test.plain"),
        ab(json!({"kind": "KEYWORD", "abilityId": "test.kw", "keyword": "TEST_KW"})),
    ];
    let src = source(keep.clone());
    let mut all: Vec<Vec<CopyException>> = Vec::new();
    for part in PARTS {
        all.push(vec![set_ex(part)]);
        all.push(vec![not_copied_ex(part)]);
        all.push(vec![add_ex(part)]);
    }
    all.push(PARTS.iter().map(|p| set_ex(*p)).collect());
    all.push(PARTS.iter().map(|p| not_copied_ex(*p)).collect());
    for ex in all {
        assert_eq!(copy(&src, &ex).abilities, keep, "{ex:?}");
    }
}

/// REQ-OBJ-002, CR 707.9a: an ability gained as part of the copying process is part of the
/// copy's copiable values and is never dropped, even if it is a type CDA and an exception on
/// its part comes before or after it.
#[test]
fn req_obj_002_gained_ability_is_never_dropped_707_9a() {
    for gained in all_type_cdas() {
        for part in PARTS {
            for touch in [set_ex(part), not_copied_ex(part)] {
                for ex in [
                    vec![CopyException::GainAbility(gained.clone()), touch.clone()],
                    vec![touch.clone(), CopyException::GainAbility(gained.clone())],
                ] {
                    let got = copy(&source(vec![plain("test.p")]), &ex);
                    assert_eq!(
                        got.abilities,
                        vec![plain("test.p"), gained.clone()],
                        "{ex:?}"
                    );
                }
            }
        }
    }
}

/// REQ-OBJ-002, CR 707.9d: the surviving abilities keep their original relative order.
#[test]
fn req_obj_002_surviving_order_is_preserved() {
    let a = plain("test.a");
    let sub = cda4("test.cda.sub", &["SUBTYPES"]);
    let b = plain("test.b");
    let sup = cda4("test.cda.sup", &["SUPERTYPES"]);
    let card = cda4("test.cda.card", &["CARD_TYPES"]);
    let c = plain("test.c");
    let none = cda4_all("test.cda.none");
    let d = cda5("test.cda.colors");
    let src = source(vec![
        a.clone(),
        sub.clone(),
        b.clone(),
        sup.clone(),
        card.clone(),
        c.clone(),
        none.clone(),
        d.clone(),
    ]);
    let got = copy(&src, &[set_ex(Part::Sup)]);
    assert_eq!(
        got.abilities,
        vec![
            a.clone(),
            sub.clone(),
            b.clone(),
            card.clone(),
            c.clone(),
            d.clone()
        ]
    );
    let got = copy(&src, &[not_copied_ex(Part::Card)]);
    assert_eq!(
        got.abilities,
        vec![
            a.clone(),
            sub.clone(),
            b.clone(),
            sup.clone(),
            c.clone(),
            d.clone()
        ]
    );
    let got = copy(&src, &[set_ex(Part::Sub)]);
    assert_eq!(got.abilities, vec![a, b, sup, card, c, d]);
}

// ---------------------------------------------------------------------------------------------
// malformed defines (R12)
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 707.9d; R12: a malformed `defines` that bypassed decode (built as raw JSON)
/// never panics in `copy_with` and is treated as the no-`defines` fallback: dropped by any of
/// the six type-line exceptions, kept by `Add*` and `SetNames`. #232 FP-164: abilities are typed
/// now, so a `defines` the typed ability refuses can no longer bypass decode; for those the
/// refusal is asserted instead, and any that still decodes keeps the fallback check.
#[test]
fn req_obj_002_malformed_defines_fall_back_to_all_parts() {
    let malformed = [
        json!([]),
        json!("SUBTYPES"),
        json!(["NOPE"]),
        json!(["subtypes"]),
        json!(["SUBTYPES", "SUBTYPES"]),
        json!(["SUBTYPES", 1]),
        json!([["SUBTYPES"]]),
        json!(1),
        json!(true),
        json!({"SUBTYPES": true}),
        json!(null),
    ];
    for (n, d) in malformed.into_iter().enumerate() {
        let Some(cda) = try_ab(cda4_json(&format!("test.cda.bad{n}"), Some(d.clone()))) else {
            continue;
        };
        for part in PARTS {
            for (exl, ex) in touching(part) {
                check_one(&cda, &[ex], true, &format!("defines {d} under {exl}"));
            }
            check_one(
                &cda,
                &[add_ex(part)],
                false,
                &format!("defines {d} under Add({part:?})"),
            );
        }
        check_one(
            &cda,
            &[CopyException::SetNames(set(&["test.renamed"]))],
            false,
            &format!("defines {d} under SetNames"),
        );
    }
}

/// R12: abilities that are not well-formed at all (wrong JSON shapes for `effects`, `layer`,
/// `modification`) never panic `copy_with`. #232 FP-164: they are refused as typed abilities, so
/// only the shapes that still decode reach `copy_with`.
#[test]
fn req_obj_002_odd_ability_shapes_do_not_panic() {
    let odd: Vec<Option<AbilityAST>> = vec![
        try_ab(json!(null)),
        try_ab(json!([])),
        try_ab(json!("STATIC")),
        try_ab(json!({"kind": "STATIC", "effects": null})),
        try_ab(json!({"kind": "STATIC", "effects": {}})),
        try_ab(json!({"kind": "STATIC", "effects": [null, 1, "x"]})),
        try_ab(json!({"kind": "STATIC", "effects": [{"type": "CHAR", "layer": 4, "isCDA": true}]})),
        try_ab(
            json!({"kind": "STATIC", "effects": [{"type": "CHAR", "layer": "4", "isCDA": "true",
            "modification": "ANY_OP"}]}),
        ),
        try_ab(
            json!({"kind": "STATIC", "effects": [{"type": "CHAR", "layer": "4", "isCDA": true,
            "modification": {"defines": ["SUBTYPES"]}}]}),
        ),
    ];
    let src = source(odd.into_iter().flatten().collect());
    for part in PARTS {
        for ex in [set_ex(part), not_copied_ex(part), add_ex(part)] {
            let _ = copy(&src, &[ex]);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// copy of a copy, determinism
// ---------------------------------------------------------------------------------------------

/// REQ-OBJ-002, CR 707.9d, 707.9b: a copy of a copy. A first copy with `SetSupertypes` keeps
/// the subtype and card-type CDAs (and drops the supertype one); copying that result with
/// `SetSubtypes` then drops the subtype CDA and keeps the card-type CDA, both through
/// `CopiableValues::plain` of the first result and through `copy_with` on the first values.
/// Every step is deterministic (same inputs, same outputs).
#[test]
fn req_obj_002_copy_of_a_copy_and_determinism() {
    let sub = cda4("test.cda.sub", &["SUBTYPES"]);
    let sup = cda4("test.cda.sup", &["SUPERTYPES"]);
    let card = cda4("test.cda.card", &["CARD_TYPES"]);
    let multi = cda4("test.cda.sup_sub", &["SUPERTYPES", "SUBTYPES"]);
    let src = source(vec![sub.clone(), sup, card.clone(), multi]);
    let first_ex = [CopyException::SetSupertypes(set(&["Basic"]))];
    let v1 = CopiableValues::plain(src.clone()).copy_with(&own(), &first_ex);
    let v1_again = CopiableValues::plain(src).copy_with(&own(), &first_ex);
    assert_eq!(v1, v1_again);
    assert_eq!(
        v1.characteristics.abilities,
        vec![sub.clone(), card.clone()]
    );
    assert_eq!(v1.characteristics.supertypes, set(&["Basic"]));

    // No exception on the second copy: everything retained stays.
    let v2 = CopiableValues::plain(v1.characteristics.clone()).copy_with(&own(), &[]);
    assert_eq!(
        v2.characteristics.abilities,
        vec![sub.clone(), card.clone()]
    );

    let second_ex = [CopyException::SetSubtypes(set(&["Spirit"]))];
    let via_plain = CopiableValues::plain(v1.characteristics.clone()).copy_with(&own(), &second_ex);
    let via_plain_again =
        CopiableValues::plain(v1.characteristics.clone()).copy_with(&own(), &second_ex);
    assert_eq!(via_plain, via_plain_again);
    assert_eq!(via_plain.characteristics.abilities, vec![card.clone()]);
    assert_eq!(via_plain.characteristics.subtypes, set(&["Spirit"]));
    assert_eq!(via_plain.characteristics.supertypes, set(&["Basic"]));

    let chained = v1.copy_with(&own(), &second_ex);
    let chained_again = v1_again.copy_with(&own(), &second_ex);
    assert_eq!(chained, chained_again);
    assert_eq!(chained.characteristics.abilities, vec![card]);
    assert_eq!(chained.characteristics.subtypes, set(&["Spirit"]));

    // A CDA gained in the first copy is part of the copiable values and the second copy's
    // exception on its part does not drop it either (CR 707.9a).
    let gained = cda4("test.cda.gained_sub", &["SUBTYPES"]);
    let g1 = CopiableValues::plain(source(vec![]))
        .copy_with(&own(), &[CopyException::GainAbility(gained.clone())]);
    assert_eq!(g1.characteristics.abilities, vec![gained.clone()]);
    // Copied (not gained) by the second copy: now it is an ordinary copied CDA.
    let g2 = CopiableValues::plain(g1.characteristics.clone()).copy_with(&own(), &[]);
    assert_eq!(g2.characteristics.abilities, vec![gained]);
}

// ---------------------------------------------------------------------------------------------
// property test (LCG, R3)
// ---------------------------------------------------------------------------------------------

/// A 64-bit LCG (Knuth MMIX constants), local copy.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Lcg {
        Lcg(seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407))
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

fn permutations(n: usize) -> Vec<Vec<usize>> {
    if n == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for p in permutations(n - 1) {
        for i in 0..=p.len() {
            let mut q = p.clone();
            q.insert(i, n - 1);
            out.push(q);
        }
    }
    out
}

/// What a pooled ability defines, for the model: `None` is "not a CDA that type-line
/// exceptions drop".
#[derive(Clone)]
struct Pooled {
    ability: AbilityAST,
    defines: Option<Vec<Part>>,
}

fn ability_pool() -> Vec<Pooled> {
    let mut pool: Vec<Pooled> = Vec::new();
    let names = [
        (&["SUPERTYPES"][..], vec![Part::Sup]),
        (&["CARD_TYPES"][..], vec![Part::Card]),
        (&["SUBTYPES"][..], vec![Part::Sub]),
        (&["CARD_TYPES", "SUBTYPES"][..], vec![Part::Card, Part::Sub]),
        (&["SUPERTYPES", "SUBTYPES"][..], vec![Part::Sup, Part::Sub]),
        (
            &["SUPERTYPES", "CARD_TYPES", "SUBTYPES"][..],
            PARTS.to_vec(),
        ),
    ];
    for (i, (wire, parts)) in names.iter().enumerate() {
        pool.push(Pooled {
            ability: cda4(&format!("test.pool.cda{i}"), wire),
            defines: Some(parts.clone()),
        });
    }
    pool.push(Pooled {
        ability: cda4_all("test.pool.none"),
        defines: Some(PARTS.to_vec()),
    });
    // #232 FP-164: a malformed `defines` only enters the pool if the typed ability holds it.
    if let Some(ability) = try_ab(cda4_json("test.pool.bad", Some(json!(["NOPE"])))) {
        pool.push(Pooled {
            ability,
            defines: Some(PARTS.to_vec()),
        });
    }
    pool.push(Pooled {
        ability: cda5("test.pool.colors"),
        defines: None,
    });
    pool.push(Pooled {
        ability: static_with(
            "test.pool.l4_not_cda",
            vec![char_effect(
                "4",
                json!({"op": "ANY_OP", "defines": ["SUBTYPES"]}),
                Some(false),
            )],
        ),
        defines: None,
    });
    pool.push(Pooled {
        ability: plain("test.pool.plain"),
        defines: None,
    });
    pool
}

/// The exception pool and the part each touches (`None`: touches no type-line part).
fn exception_pool() -> Vec<(CopyException, Option<Part>)> {
    let mut out = Vec::new();
    for part in PARTS {
        out.push((set_ex(part), Some(part)));
        out.push((not_copied_ex(part), Some(part)));
        out.push((add_ex(part), None));
    }
    out.push((CopyException::SetNames(set(&["test.renamed"])), None));
    out
}

/// REQ-OBJ-002, CR 707.9d, 707.9a: property (200 seeds). For a random ability list (with
/// repeats) and a random sequence of type-line, `Add*`, `SetNames` and `GainAbility`
/// exceptions, the copied abilities are exactly the source abilities that are not type CDAs
/// defining a touched part, in their original order, followed by the gained abilities in
/// exception order; the other characteristics equal those of the same copy of the source
/// without abilities; and the result is deterministic.
#[test]
fn req_obj_002_property_drop_is_the_union_of_touched_parts() {
    let pool = ability_pool();
    let ex_pool = exception_pool();
    for seed in 0..200u64 {
        let mut rng = Lcg::new(seed);
        let n_ab = rng.below(7) as usize;
        let picked: Vec<Pooled> = (0..n_ab)
            .map(|_| pool[rng.below(pool.len() as u64) as usize].clone())
            .collect();
        let n_ex = rng.below(5) as usize;
        let mut ex: Vec<CopyException> = Vec::new();
        let mut touched: Vec<Part> = Vec::new();
        let mut gained: Vec<AbilityAST> = Vec::new();
        for _ in 0..n_ex {
            if rng.below(6) == 0 {
                let g = pool[rng.below(pool.len() as u64) as usize].ability.clone();
                gained.push(g.clone());
                ex.push(CopyException::GainAbility(g));
            } else {
                let (e, p) = ex_pool[rng.below(ex_pool.len() as u64) as usize].clone();
                if let Some(p) = p {
                    touched.push(p);
                }
                ex.push(e);
            }
        }
        let src = source(picked.iter().map(|p| p.ability.clone()).collect());
        let got = copy(&src, &ex);
        let mut want: Vec<AbilityAST> = picked
            .iter()
            .filter(|p| match &p.defines {
                Some(parts) => !parts.iter().any(|q| touched.contains(q)),
                None => true,
            })
            .map(|p| p.ability.clone())
            .collect();
        want.extend(gained);
        assert_eq!(got.abilities, want, "seed {seed}: {ex:?}");
        let reference = copy(&source(Vec::new()), &ex);
        assert_eq!(
            no_abilities(&got),
            no_abilities(&reference),
            "seed {seed}: {ex:?}"
        );
        assert_eq!(copy(&src, &ex), got, "seed {seed}: determinism");
    }
}
