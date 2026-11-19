// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the `fineprint_core::layouts` spec tests (REQ-OBJ-002). Black-box:
//! written from the work-item brief only.
//!
//! Each `spec_layouts_*.rs` test file includes this file with `#[path]`; on its own it is an
//! empty test target. It holds synthetic definitions (R5: invented, neutrally named), a
//! catalogue of every cast choice and layout status, and an oracle that encodes the brief's
//! selection tables (CR 709, 710, 711.6, 712.6, 716, 719, 721; mana value CR 202.3a-d).
//!
//! The oracle is used by the exhaustive sweeps; every behaviour it encodes is also asserted
//! with literal expected values in the per-layout files, so a mistake in the oracle cannot hide
//! a mistake in the module.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use fineprint_core::layouts::{CastChoice, FaceUp, LayoutError, LayoutQuery, LayoutStatus};
use fineprint_core::objects::{
    CdaColors, Characteristics, ManaValueContext, ManaValueSubject, SplitHalf, VariableValues,
    base_colors, mana_value, union_colors,
};
use fineprint_core::zones::ZoneKind;
use fineprint_dsl::card::{
    AbilityAST, CardDefinitionDSL, CardFaceDSL, ColorBitmask, DSL_VERSION, FaceKind,
    LayoutBehavior, Origin, ProvenanceDSL, SupportStatus, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::mana::ManaCostDSL;
use fineprint_dsl::vocab::RulesVocabulary;

// ---------------------------------------------------------------------------------------------
// primitive builders
// ---------------------------------------------------------------------------------------------

pub fn vocab() -> &'static RulesVocabulary {
    RulesVocabulary::bundled().expect("bundled vocabulary loads")
}

pub fn cost(text: &str) -> ManaCostDSL {
    ManaCostDSL::parse(text).expect("synthetic cost parses")
}

pub fn some_cost(text: &str) -> Option<ManaCostDSL> {
    Some(cost(text))
}

pub fn mask(bits: u8) -> ColorBitmask {
    ColorBitmask::new(bits).expect("bits in 0..=31")
}

/// Round-trips a serialisable value through MessagePack into one of the DSL placeholder types
/// (which wrap a JSON value), so no direct `serde_json` dependency is needed.
fn via_msgpack<In: serde::Serialize, Out: serde::de::DeserializeOwned>(value: &In) -> Out {
    let bytes = rmp_serde::to_vec(value).expect("encode");
    rmp_serde::from_slice(&bytes).expect("decode")
}

pub fn printed(n: i64) -> ValueExpressionAST {
    via_msgpack(&n)
}

pub fn text(tokens: &[&str]) -> TextTokenStream {
    via_msgpack(&tokens.to_vec())
}

/// A synthetic ability: the JSON object `{key: n}`.
pub fn ability(key: &str, n: i64) -> AbilityAST {
    let mut m = BTreeMap::new();
    m.insert(key.to_string(), n);
    via_msgpack(&m)
}

pub fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

pub fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

/// A blank face of `kind`: no cost, no types, nothing printed.
pub fn blank(kind: FaceKind, face_name: &str) -> CardFaceDSL {
    CardFaceDSL {
        face_kind: kind,
        name: face_name.to_string(),
        mana_cost: None,
        color_indicator: None,
        supertypes: Vec::new(),
        card_types: Vec::new(),
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

/// A synthetic definition with `layout` and `faces` (the hash is not needed: no `from_json`).
pub fn def(layout: LayoutBehavior, faces: Vec<CardFaceDSL>) -> CardDefinitionDSL {
    CardDefinitionDSL {
        card_def_id: "test.layout_subject".to_owned(),
        dsl_version: DSL_VERSION.to_owned(),
        definition_hash: String::new(),
        source: ProvenanceDSL {
            oracle_text_hash: "0".repeat(64),
            origin: Origin::Parser,
            parser_version: Some("0.1.0".to_owned()),
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

/// The characteristics `Characteristics::from_face` gives for `face` (valid synthetic faces).
pub fn chars(face: &CardFaceDSL) -> Characteristics {
    Characteristics::from_face(face, vocab()).expect("valid synthetic face")
}

/// The face of `kind` in `d` (faces are looked up by kind).
pub fn face_of(d: &CardDefinitionDSL, kind: FaceKind) -> &CardFaceDSL {
    d.faces
        .iter()
        .find(|f| f.face_kind == kind)
        .expect("face of that kind")
}

pub fn on_stack(x: u32, y: u32, z: u32) -> ManaValueContext {
    ManaValueContext::OnStack(VariableValues { x, y, z })
}

/// The mana value of a cost used as the object's own cost (CR 202.3, 202.3a).
pub fn cost_mv(c: Option<&ManaCostDSL>, ctx: ManaValueContext) -> i64 {
    mana_value(&ManaValueSubject::Plain { mana_cost: c }, ctx)
}

/// Both mana value contexts the sweeps use.
pub fn contexts() -> [ManaValueContext; 2] {
    [ManaValueContext::OffStack, on_stack(3, 2, 1)]
}

// ---------------------------------------------------------------------------------------------
// cast choices and statuses (local mirrors, so no derive of the API types is assumed)
// ---------------------------------------------------------------------------------------------

/// Mirror of `CastChoice`, with `ModalFace` split by face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cast {
    None,
    Left,
    Right,
    Fused,
    Adventure,
    Omen,
    Prototype,
    Transformed,
    ModalFront,
    ModalBack,
}

impl Cast {
    pub const ALL: [Cast; 10] = [
        Cast::None,
        Cast::Left,
        Cast::Right,
        Cast::Fused,
        Cast::Adventure,
        Cast::Omen,
        Cast::Prototype,
        Cast::Transformed,
        Cast::ModalFront,
        Cast::ModalBack,
    ];

    pub fn api(self) -> CastChoice {
        match self {
            Cast::None => CastChoice::None,
            Cast::Left => CastChoice::SplitHalf(SplitHalf::Left),
            Cast::Right => CastChoice::SplitHalf(SplitHalf::Right),
            Cast::Fused => CastChoice::Fused,
            Cast::Adventure => CastChoice::Adventure,
            Cast::Omen => CastChoice::Omen,
            Cast::Prototype => CastChoice::Prototype,
            Cast::Transformed => CastChoice::Transformed,
            Cast::ModalFront => CastChoice::ModalFace(FaceUp::Front),
            Cast::ModalBack => CastChoice::ModalFace(FaceUp::Back),
        }
    }

    /// Every cast choice except those listed.
    pub fn all_except(keep_out: &[Cast]) -> Vec<Cast> {
        Cast::ALL
            .into_iter()
            .filter(|c| !keep_out.contains(c))
            .collect()
    }
}

/// Mirror of `LayoutStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct St {
    pub flipped: bool,
    pub back_up: bool,
    pub copying_back_face: bool,
}

impl St {
    pub const DEFAULT: St = St {
        flipped: false,
        back_up: false,
        copying_back_face: false,
    };

    /// All eight combinations.
    pub fn all() -> Vec<St> {
        let mut out = Vec::new();
        for flipped in [false, true] {
            for back_up in [false, true] {
                for copying_back_face in [false, true] {
                    out.push(St {
                        flipped,
                        back_up,
                        copying_back_face,
                    });
                }
            }
        }
        out
    }

    pub fn flipped() -> St {
        St {
            flipped: true,
            ..St::DEFAULT
        }
    }

    pub fn back_up() -> St {
        St {
            back_up: true,
            ..St::DEFAULT
        }
    }

    pub fn copying() -> St {
        St {
            copying_back_face: true,
            ..St::DEFAULT
        }
    }

    pub fn api(self) -> LayoutStatus {
        LayoutStatus {
            flipped: self.flipped,
            face_up: if self.back_up {
                FaceUp::Back
            } else {
                FaceUp::Front
            },
            copying_back_face: self.copying_back_face,
        }
    }
}

/// The query for `zone`, `cast` and `st`.
pub fn query(zone: ZoneKind, cast: Cast, st: St) -> LayoutQuery {
    LayoutQuery::new(zone)
        .with_cast(cast.api())
        .with_status(st.api())
}

pub fn non_stack_zones() -> Vec<ZoneKind> {
    ZoneKind::ALL
        .into_iter()
        .filter(|z| *z != ZoneKind::Stack)
        .collect()
}

/// Zones other than the stack and the battlefield.
pub fn hidden_or_other_zones() -> Vec<ZoneKind> {
    ZoneKind::ALL
        .into_iter()
        .filter(|z| *z != ZoneKind::Stack && *z != ZoneKind::Battlefield)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// the twelve layouts and their synthetic sample definitions
// ---------------------------------------------------------------------------------------------

pub const IN_SCOPE: [LayoutBehavior; 12] = [
    LayoutBehavior::Normal,
    LayoutBehavior::Split,
    LayoutBehavior::Flip,
    LayoutBehavior::Leveler,
    LayoutBehavior::DfcNonmodal,
    LayoutBehavior::DfcModal,
    LayoutBehavior::Saga,
    LayoutBehavior::Adventurer,
    LayoutBehavior::Class,
    LayoutBehavior::Prototype,
    LayoutBehavior::Case,
    LayoutBehavior::Omen,
];

/// The required face kinds of each in-scope layout (brief "Face sets").
pub fn required_kinds(layout: LayoutBehavior) -> Vec<FaceKind> {
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

pub fn creature(kind: FaceKind, face_name: &str, c: &str, p: i64, t: i64) -> CardFaceDSL {
    CardFaceDSL {
        mana_cost: ManaCostDSL::parse_optional(c).expect("synthetic cost"),
        card_types: strings(&["Creature"]),
        power: Some(printed(p)),
        toughness: Some(printed(t)),
        text: text(&[face_name]),
        abilities: vec![ability(face_name, 1)],
        ..blank(kind, face_name)
    }
}

pub fn spell(kind: FaceKind, face_name: &str, c: &str, card_type: &str) -> CardFaceDSL {
    CardFaceDSL {
        mana_cost: ManaCostDSL::parse_optional(c).expect("synthetic cost"),
        card_types: strings(&[card_type]),
        text: text(&[face_name]),
        abilities: vec![ability(face_name, 1)],
        ..blank(kind, face_name)
    }
}

/// NORMAL: a 2/2 Human creature with `{X}` in its cost.
pub fn normal_main() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Human"]),
        ..creature(FaceKind::Main, "test.normal_main", "{X}{1}{G}", 2, 2)
    }
}

pub fn split_left() -> CardFaceDSL {
    spell(FaceKind::LeftHalf, "test.split_left", "{X}{R}", "Instant")
}

pub fn split_right() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Arcane"]),
        ..spell(
            FaceKind::RightHalf,
            "test.split_right",
            "{2}{U}{U}",
            "Sorcery",
        )
    }
}

pub fn flip_main() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Human", "Wizard"]),
        loyalty: Some(1),
        ..creature(FaceKind::Main, "test.flip_top", "{2}{B}", 1, 3)
    }
}

/// The flipped face declares its own cost and indicator; both must be ignored (CR 710.1c).
pub fn flip_flipped() -> CardFaceDSL {
    CardFaceDSL {
        supertypes: strings(&["Legendary"]),
        subtypes: strings(&["Wizard"]),
        color_indicator: Some(ColorBitmask::GREEN),
        defense: Some(3),
        hand_modifier: Some(-1),
        life_modifier: Some(4),
        ..creature(FaceKind::Flipped, "test.flip_bottom", "{9}{W}", 4, 4)
    }
}

pub fn dfc_front() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Human"]),
        ..creature(FaceKind::Main, "test.dfc_front", "{3}{G}", 2, 3)
    }
}

/// A back face with no cost and a red color indicator.
pub fn dfc_back() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Wizard"]),
        color_indicator: Some(ColorBitmask::RED),
        ..creature(FaceKind::Back, "test.dfc_back", "", 5, 5)
    }
}

pub fn mdfc_front() -> CardFaceDSL {
    spell(FaceKind::Main, "test.mdfc_front", "{2}{W}", "Sorcery")
}

/// A land back face with no cost.
pub fn mdfc_back() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Forest"]),
        ..spell(FaceKind::Back, "test.mdfc_back", "", "Land")
    }
}

pub fn adventurer_main() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Human"]),
        ..creature(FaceKind::Main, "test.adv_creature", "{3}{G}", 4, 3)
    }
}

pub fn adventure_face() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Adventure"]),
        ..spell(
            FaceKind::Adventure,
            "test.adv_spell",
            "{X}{1}{R}",
            "Instant",
        )
    }
}

pub fn omen_main() -> CardFaceDSL {
    creature(FaceKind::Main, "test.omen_creature", "{4}{U}", 3, 5)
}

pub fn omen_face() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Omen"]),
        ..spell(FaceKind::Omen, "test.omen_spell", "{X}{B}", "Sorcery")
    }
}

/// A colorless-cost artifact creature with a blue color indicator.
pub fn proto_main() -> CardFaceDSL {
    CardFaceDSL {
        card_types: strings(&["Artifact", "Creature"]),
        color_indicator: Some(ColorBitmask::BLUE),
        loyalty: Some(2),
        ..creature(FaceKind::Main, "test.proto_full", "{7}", 6, 6)
    }
}

/// The prototype face: cost and P/T only matter; its own name and types must be ignored.
pub fn proto_face() -> CardFaceDSL {
    CardFaceDSL {
        mana_cost: some_cost("{X}{2}{R}{R}"),
        power: Some(printed(3)),
        toughness: Some(printed(2)),
        card_types: strings(&["Artifact"]),
        text: text(&["test.proto_inset"]),
        abilities: vec![ability("test.proto_inset", 1)],
        ..blank(FaceKind::Prototype, "test.proto_inset")
    }
}

pub fn leveler_main() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Human"]),
        abilities: vec![
            ability("level_up", 1),
            ability("band_2_4", 2),
            ability("band_5", 5),
        ],
        ..creature(FaceKind::Main, "test.leveler", "{1}{W}", 1, 1)
    }
}

pub fn saga_main() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Saga"]),
        abilities: vec![
            ability("chapter", 1),
            ability("chapter", 2),
            ability("chapter", 3),
        ],
        ..spell(FaceKind::Main, "test.saga", "{2}{R}", "Enchantment")
    }
}

pub fn class_main() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Class"]),
        abilities: vec![ability("level_1", 1), ability("level_2", 2)],
        ..spell(FaceKind::Main, "test.class", "{W}", "Enchantment")
    }
}

pub fn case_main() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Case"]),
        abilities: vec![ability("to_solve", 1), ability("solved", 2)],
        ..spell(FaceKind::Main, "test.case", "{X}{3}{U}", "Enchantment")
    }
}

/// The sample definition of an in-scope layout.
pub fn sample(layout: LayoutBehavior) -> CardDefinitionDSL {
    let faces = match layout {
        LayoutBehavior::Normal => vec![normal_main()],
        LayoutBehavior::Split => vec![split_left(), split_right()],
        LayoutBehavior::Flip => vec![flip_main(), flip_flipped()],
        LayoutBehavior::Leveler => vec![leveler_main()],
        LayoutBehavior::DfcNonmodal => vec![dfc_front(), dfc_back()],
        LayoutBehavior::DfcModal => vec![mdfc_front(), mdfc_back()],
        LayoutBehavior::Saga => vec![saga_main()],
        LayoutBehavior::Adventurer => vec![adventurer_main(), adventure_face()],
        LayoutBehavior::Class => vec![class_main()],
        LayoutBehavior::Prototype => vec![proto_main(), proto_face()],
        LayoutBehavior::Case => vec![case_main()],
        LayoutBehavior::Omen => vec![omen_main(), omen_face()],
        other => panic!("not an in-scope layout: {other:?}"),
    };
    def(layout, faces)
}

// ---------------------------------------------------------------------------------------------
// combined split characteristics and the oracle
// ---------------------------------------------------------------------------------------------

/// The combined characteristics of two split halves, per the brief's combination rule
/// (CR 709.4, 709.4a-d, 702.102b).
pub fn split_combined(left: &CardFaceDSL, right: &CardFaceDSL) -> Characteristics {
    let l = chars(left);
    let r = chars(right);
    let mut symbols = Vec::new();
    if let Some(c) = &left.mana_cost {
        symbols.extend_from_slice(c.symbols());
    }
    if let Some(c) = &right.mana_cost {
        symbols.extend_from_slice(c.symbols());
    }
    let mana_cost = if symbols.is_empty() {
        None
    } else {
        Some(ManaCostDSL::new(symbols).expect("combined cost"))
    };
    let color_indicator = match (left.color_indicator, right.color_indicator) {
        (None, None) => None,
        (a, b) => Some(union_colors(
            a.unwrap_or(ColorBitmask::NONE),
            b.unwrap_or(ColorBitmask::NONE),
        )),
    };
    let mut rules_text = l.rules_text.0.clone();
    rules_text.extend(r.rules_text.0.iter().cloned());
    let mut abilities = l.abilities.clone();
    abilities.extend(r.abilities.iter().cloned());
    Characteristics {
        names: l.names.union(&r.names).cloned().collect(),
        mana_cost,
        color_indicator,
        colors: union_colors(l.colors, r.colors),
        supertypes: l.supertypes.union(&r.supertypes).cloned().collect(),
        card_types: l.card_types.union(&r.card_types).cloned().collect(),
        subtypes: l.subtypes.union(&r.subtypes).cloned().collect(),
        rules_text: TextTokenStream(rules_text),
        abilities,
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        hand_modifier: None,
        life_modifier: None,
    }
}

/// Flipped characteristics: name, text box, type line and P/T are the Flipped face's
/// (CR 710.1b, 710.2); cost, indicator and colors stay the Main face's (CR 710.1c); loyalty,
/// defense and the hand and life modifiers are not among the characteristics CR 710.1b and 710.2
/// replace, so they stay the Main face's too.
pub fn flip_flipped_chars(main: &CardFaceDSL, flipped: &CardFaceDSL) -> Characteristics {
    let m = chars(main);
    Characteristics {
        mana_cost: m.mana_cost,
        color_indicator: m.color_indicator,
        colors: m.colors,
        loyalty: m.loyalty,
        defense: m.defense,
        hand_modifier: m.hand_modifier,
        life_modifier: m.life_modifier,
        ..chars(flipped)
    }
}

/// Prototyped characteristics: Main's, with cost and P/T from the prototype face and colors from
/// the prototype cost plus Main's indicator (CR 719.3b, 719.5).
pub fn prototyped_chars(main: &CardFaceDSL, proto: &CardFaceDSL) -> Characteristics {
    let m = chars(main);
    Characteristics {
        colors: base_colors(
            proto.mana_cost.as_ref(),
            main.color_indicator,
            CdaColors::None,
        ),
        mana_cost: proto.mana_cost.clone(),
        power: proto.power.clone(),
        toughness: proto.toughness.clone(),
        ..m
    }
}

fn invalid(layout: LayoutBehavior, zone: ZoneKind) -> LayoutError {
    LayoutError::InvalidCast { layout, zone }
}

/// What the brief says `base_characteristics` and `mana_value` return for a valid definition of
/// an in-scope layout (CR 202.3a-d, 709, 710, 711.6, 712.6, 716, 719, 721).
pub fn oracle(
    d: &CardDefinitionDSL,
    zone: ZoneKind,
    cast: Cast,
    st: St,
    ctx: ManaValueContext,
) -> Result<(Characteristics, i64), LayoutError> {
    let layout = d.layout;
    let stack = zone == ZoneKind::Stack;
    let battlefield = zone == ZoneKind::Battlefield;
    // CR 202.3e, 107.3g, 107.3m: X (and Y, Z: CR 107.3p) is 0 in every zone but the stack,
    // whatever context the caller supplies.
    let ctx = if stack {
        ctx
    } else {
        ManaValueContext::OffStack
    };
    let main_plain = |c: Characteristics| {
        let mv = cost_mv(c.mana_cost.as_ref(), ctx);
        (c, mv)
    };
    match layout {
        LayoutBehavior::Normal
        | LayoutBehavior::Leveler
        | LayoutBehavior::Saga
        | LayoutBehavior::Class
        | LayoutBehavior::Case => Ok(main_plain(chars(face_of(d, FaceKind::Main)))),
        LayoutBehavior::Split => {
            let left = face_of(d, FaceKind::LeftHalf);
            let right = face_of(d, FaceKind::RightHalf);
            let lmv = |c| cost_mv(left.mana_cost.as_ref(), c);
            let rmv = |c| cost_mv(right.mana_cost.as_ref(), c);
            if !stack {
                let off = ManaValueContext::OffStack;
                return Ok((split_combined(left, right), lmv(off) + rmv(off)));
            }
            match cast {
                Cast::Left => Ok((chars(left), lmv(ctx))),
                Cast::Right => Ok((chars(right), rmv(ctx))),
                Cast::Fused => Ok((split_combined(left, right), lmv(ctx) + rmv(ctx))),
                _ => Err(invalid(layout, zone)),
            }
        }
        LayoutBehavior::Flip => {
            let main = face_of(d, FaceKind::Main);
            let c = if battlefield && st.flipped {
                flip_flipped_chars(main, face_of(d, FaceKind::Flipped))
            } else {
                chars(main)
            };
            Ok((c, cost_mv(main.mana_cost.as_ref(), ctx)))
        }
        LayoutBehavior::DfcNonmodal => {
            let front = face_of(d, FaceKind::Main);
            let back = face_of(d, FaceKind::Back);
            let selected = if stack {
                match cast {
                    Cast::None => front,
                    Cast::Transformed => back,
                    _ => return Err(invalid(layout, zone)),
                }
            } else if battlefield && st.back_up {
                back
            } else {
                front
            };
            // CR 202.3b, 712.6d: only a spell or permanent copying a back face has mana value
            // 0; in every other zone only the front face applies (CR 712.6a).
            let mv = if st.copying_back_face && (stack || battlefield) {
                0
            } else {
                cost_mv(front.mana_cost.as_ref(), ctx)
            };
            Ok((chars(selected), mv))
        }
        LayoutBehavior::DfcModal => {
            let front = face_of(d, FaceKind::Main);
            let back = face_of(d, FaceKind::Back);
            let selected = if stack {
                match cast {
                    Cast::ModalFront => front,
                    Cast::ModalBack => back,
                    _ => return Err(invalid(layout, zone)),
                }
            } else if battlefield && st.back_up {
                back
            } else {
                front
            };
            Ok(main_plain(chars(selected)))
        }
        LayoutBehavior::Adventurer | LayoutBehavior::Omen => {
            let (alt_kind, alt_cast) = if layout == LayoutBehavior::Adventurer {
                (FaceKind::Adventure, Cast::Adventure)
            } else {
                (FaceKind::Omen, Cast::Omen)
            };
            let main = face_of(d, FaceKind::Main);
            let selected = if stack {
                if cast == alt_cast {
                    face_of(d, alt_kind)
                } else if cast == Cast::None {
                    main
                } else {
                    return Err(invalid(layout, zone));
                }
            } else {
                main
            };
            Ok(main_plain(chars(selected)))
        }
        LayoutBehavior::Prototype => {
            let main = face_of(d, FaceKind::Main);
            let c = if stack || battlefield {
                match cast {
                    Cast::Prototype => prototyped_chars(main, face_of(d, FaceKind::Prototype)),
                    Cast::None => chars(main),
                    _ => return Err(invalid(layout, zone)),
                }
            } else {
                chars(main)
            };
            Ok(main_plain(c))
        }
        other => Err(LayoutError::UnsupportedLayout(other)),
    }
}

/// Asserts two characteristics are equal field by field, then as a whole.
pub fn assert_chars(actual: &Characteristics, expected: &Characteristics, ctx: &str) {
    assert_eq!(actual.names, expected.names, "names: {ctx}");
    assert_eq!(actual.mana_cost, expected.mana_cost, "mana_cost: {ctx}");
    assert_eq!(
        actual.color_indicator, expected.color_indicator,
        "color_indicator: {ctx}"
    );
    assert_eq!(actual.colors, expected.colors, "colors: {ctx}");
    assert_eq!(actual.supertypes, expected.supertypes, "supertypes: {ctx}");
    assert_eq!(actual.card_types, expected.card_types, "card_types: {ctx}");
    assert_eq!(actual.subtypes, expected.subtypes, "subtypes: {ctx}");
    assert_eq!(actual.rules_text, expected.rules_text, "rules_text: {ctx}");
    assert_eq!(actual.abilities, expected.abilities, "abilities: {ctx}");
    assert_eq!(actual.power, expected.power, "power: {ctx}");
    assert_eq!(actual.toughness, expected.toughness, "toughness: {ctx}");
    assert_eq!(actual.loyalty, expected.loyalty, "loyalty: {ctx}");
    assert_eq!(actual.defense, expected.defense, "defense: {ctx}");
    assert_eq!(
        actual.hand_modifier, expected.hand_modifier,
        "hand_modifier: {ctx}"
    );
    assert_eq!(
        actual.life_modifier, expected.life_modifier,
        "life_modifier: {ctx}"
    );
    assert_eq!(actual, expected, "whole: {ctx}");
}
