// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Shared helpers for the ROOM, MELD, STATION, PREPARATION and MUTATE/merged layout spec tests
//! (REQ-OBJ-002). Black-box: written from the work-item brief only.
//!
//! Each `spec_layouts_{room,meld,station,prepare,merged}.rs` file includes this file with
//! `#[path]`; on its own it is an empty test target. It re-exports the #22 helpers
//! (`spec_layouts_support.rs`, used read-only) as `support` and adds synthetic definitions
//! (R5: invented, neutrally named) for the five composite layouts.

#![allow(dead_code)]

#[path = "spec_layouts_support.rs"]
pub mod support;

use fineprint_dsl::card::{CardDefinitionDSL, CardFaceDSL, ColorBitmask, FaceKind, LayoutBehavior};

use support::{ability, blank, creature, def, printed, spell, strings, text};

/// `d` with `card_def_id` = `id` and `meld_partner` = `partner`.
pub fn with_ids(mut d: CardDefinitionDSL, id: &str, partner: Option<&str>) -> CardDefinitionDSL {
    d.card_def_id = id.to_owned();
    d.meld_partner = partner.map(str::to_owned);
    d
}

/// `d` with its faces in reverse order (faces are looked up by kind, never by index).
pub fn reversed(mut d: CardDefinitionDSL) -> CardDefinitionDSL {
    d.faces.reverse();
    d
}

/// A face whose type line the vocabulary rejects (`ObjectError::UnknownCardType`).
pub fn bad_type(face: CardFaceDSL) -> CardFaceDSL {
    CardFaceDSL {
        card_types: strings(&["NotACardType"]),
        subtypes: Vec::new(),
        ..face
    }
}

// ---------------------------------------------------------------------------------------------
// ROOM
// ---------------------------------------------------------------------------------------------

/// The left door: `{1}{W}`, one ability, on the shared type line legendary artifact
/// enchantment Room (CR 709.5a: both halves carry the one shared type line).
pub fn room_left() -> CardFaceDSL {
    CardFaceDSL {
        supertypes: strings(&["Legendary"]),
        card_types: strings(&["Artifact", "Enchantment"]),
        subtypes: strings(&["Room"]),
        ..spell(
            FaceKind::LeftHalf,
            "test.room_left",
            "{1}{W}",
            "Enchantment",
        )
    }
}

/// The right door: `{3}{U}{U}`, two abilities and a two-token text, on the same shared type
/// line as the left door (CR 709.5a).
pub fn room_right() -> CardFaceDSL {
    CardFaceDSL {
        supertypes: strings(&["Legendary"]),
        card_types: strings(&["Artifact", "Enchantment"]),
        subtypes: strings(&["Room"]),
        text: text(&["test.room_right", "second"]),
        abilities: vec![ability("test.room_right", 1), ability("test.room_right", 2)],
        ..spell(
            FaceKind::RightHalf,
            "test.room_right",
            "{3}{U}{U}",
            "Enchantment",
        )
    }
}

pub fn room_def(left: CardFaceDSL, right: CardFaceDSL) -> CardDefinitionDSL {
    with_ids(
        def(LayoutBehavior::Room, vec![left, right]),
        "test.room_pair",
        None,
    )
}

/// The sample room: both halves colored, no color indicator.
pub fn room_sample() -> CardDefinitionDSL {
    room_def(room_left(), room_right())
}

/// A room whose right half has no mana cost.
pub fn room_costless_right() -> CardDefinitionDSL {
    room_def(
        room_left(),
        CardFaceDSL {
            mana_cost: None,
            ..room_right()
        },
    )
}

/// A room whose left half has a green color indicator.
pub fn room_indicator_left() -> CardDefinitionDSL {
    room_def(
        CardFaceDSL {
            color_indicator: Some(ColorBitmask::GREEN),
            ..room_left()
        },
        room_right(),
    )
}

/// A room whose left half costs `{X}{R}`.
pub fn room_x_left() -> CardDefinitionDSL {
    room_def(
        CardFaceDSL {
            mana_cost: support::some_cost("{X}{R}"),
            ..room_left()
        },
        room_right(),
    )
}

// ---------------------------------------------------------------------------------------------
// MELD
// ---------------------------------------------------------------------------------------------

pub const MELD_A: &str = "test.meld_a";
pub const MELD_B: &str = "test.meld_b";
pub const MELD_C: &str = "test.meld_c";

/// Front face of meld card A: a `{2}{W}` 2/2 Human.
pub fn meld_a_front() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Human"]),
        ..creature(FaceKind::Main, "test.meld_a_front", "{2}{W}", 2, 2)
    }
}

/// Front face of meld card B: a `{X}{3}{U}` 1/4 Wizard.
pub fn meld_b_front() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Wizard"]),
        ..creature(FaceKind::Main, "test.meld_b_front", "{X}{3}{U}", 1, 4)
    }
}

/// The combined back face: legendary artifact creature Golem, its own `{9}{B}` cost, a red
/// indicator, 9/9, distinctive name, text and abilities.
pub fn meld_result() -> CardFaceDSL {
    CardFaceDSL {
        supertypes: strings(&["Legendary"]),
        card_types: strings(&["Artifact", "Creature"]),
        subtypes: strings(&["Golem"]),
        color_indicator: Some(ColorBitmask::RED),
        loyalty: Some(3),
        abilities: vec![ability("test.meld_whole", 1), ability("test.meld_whole", 2)],
        ..creature(FaceKind::MeldResult, "test.meld_whole", "{9}{B}", 9, 9)
    }
}

/// Meld card A (carries the MELD_RESULT face), partnered with B.
pub fn meld_a() -> CardDefinitionDSL {
    with_ids(
        def(LayoutBehavior::Meld, vec![meld_a_front(), meld_result()]),
        MELD_A,
        Some(MELD_B),
    )
}

/// Meld card B (MAIN only), partnered with A.
pub fn meld_b() -> CardDefinitionDSL {
    with_ids(
        def(LayoutBehavior::Meld, vec![meld_b_front()]),
        MELD_B,
        Some(MELD_A),
    )
}

// ---------------------------------------------------------------------------------------------
// STATION
// ---------------------------------------------------------------------------------------------

/// A station card: `{X}{2}{U}` artifact Spacecraft with printed 4/5 and two unstriated abilities.
pub fn station_main() -> CardFaceDSL {
    CardFaceDSL {
        mana_cost: support::some_cost("{X}{2}{U}"),
        card_types: strings(&["Artifact"]),
        subtypes: strings(&["Spacecraft"]),
        power: Some(printed(4)),
        toughness: Some(printed(5)),
        text: text(&["test.station_hull"]),
        abilities: vec![ability("station", 1), ability("test.station_hull", 1)],
        ..blank(FaceKind::Main, "test.station_hull")
    }
}

pub fn station_def() -> CardDefinitionDSL {
    with_ids(
        def(LayoutBehavior::Station, vec![station_main()]),
        "test.station_hull",
        None,
    )
}

// ---------------------------------------------------------------------------------------------
// PREPARATION
// ---------------------------------------------------------------------------------------------

/// The normal characteristics: a `{X}{2}{G}` 3/3 Human creature.
pub fn prep_main() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Human"]),
        ..creature(FaceKind::Main, "test.prep_body", "{X}{2}{G}", 3, 3)
    }
}

/// The prepare spell: a `{1}{R}{R}` legendary artifact creature Golem 7/1 with a blue indicator,
/// so any leak of its name, cost, types or P/T shows.
pub fn prep_spell() -> CardFaceDSL {
    CardFaceDSL {
        supertypes: strings(&["Legendary"]),
        card_types: strings(&["Artifact", "Creature"]),
        subtypes: strings(&["Golem"]),
        color_indicator: Some(ColorBitmask::BLUE),
        ..creature(FaceKind::PrepareSpell, "test.prep_inset", "{1}{R}{R}", 7, 1)
    }
}

pub fn prep_def() -> CardDefinitionDSL {
    with_ids(
        def(LayoutBehavior::Preparation, vec![prep_main(), prep_spell()]),
        "test.prep_body",
        None,
    )
}

// ---------------------------------------------------------------------------------------------
// MUTATE
// ---------------------------------------------------------------------------------------------

/// A mutating creature: `{2}{B}{B}` 4/4 Wizard with a shared ability and its own.
pub fn mutant_main() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Wizard"]),
        abilities: vec![ability("test.shared", 1), ability("test.mutant", 1)],
        ..creature(FaceKind::Main, "test.mutant", "{2}{B}{B}", 4, 4)
    }
}

pub fn mutant_def() -> CardDefinitionDSL {
    with_ids(
        def(LayoutBehavior::Mutate, vec![mutant_main()]),
        "test.mutant",
        None,
    )
}

/// A NORMAL `{1}{U}` 1/1 Human with the shared ability and its own.
pub fn merge_base_main() -> CardFaceDSL {
    CardFaceDSL {
        subtypes: strings(&["Human"]),
        abilities: vec![ability("test.shared", 1), ability("test.merge_base", 1)],
        ..creature(FaceKind::Main, "test.merge_base", "{1}{U}", 1, 1)
    }
}

pub fn merge_base_def() -> CardDefinitionDSL {
    with_ids(
        def(LayoutBehavior::Normal, vec![merge_base_main()]),
        "test.merge_base",
        None,
    )
}
