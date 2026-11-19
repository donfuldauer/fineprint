// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Single-face layouts: NORMAL, LEVELER, SAGA, CLASS and CASE (REQ-OBJ-002).
//!
//! Each has one Main face whose printed characteristics are the base characteristics in every
//! zone; the cast choice and status are ignored. Level bands, chapters, class levels and the
//! solved designation are abilities and state, not characteristic selection (CR 711.2a,
//! 711.2b, 715.2, 717.2a, 717.2b, 720.3b), so they are exposed only as [`Gating`] data.

use fineprint_dsl::card::{CardDefinitionDSL, CardFaceDSL, FaceKind, LayoutBehavior};
use fineprint_dsl::vocab::RulesVocabulary;

use super::faces::{chars, faces};
use super::types::{Gating, Layout, LayoutError, LayoutQuery};
use crate::objects::{Characteristics, ManaValueContext};

/// The Main face of a single-face `layout` definition.
fn main_face(layout: LayoutBehavior, def: &CardDefinitionDSL) -> Result<&CardFaceDSL, LayoutError> {
    let [main] = faces(layout, def, [FaceKind::Main])?;
    Ok(main)
}

/// Base characteristics of a single-face layout: the Main face in every zone.
fn printed_chars(
    layout: LayoutBehavior,
    def: &CardDefinitionDSL,
    vocab: &RulesVocabulary,
) -> Result<Characteristics, LayoutError> {
    // CR 109.3: the printed characteristics; CR 711.5, 711.6: a leveler's Main face carries
    // the uppermost power/toughness box, which is its P/T off the battlefield and below the
    // first level band; band P/T is a static-ability effect (CR 711.2a, 711.2b), not applied
    // here.
    chars(main_face(layout, def)?, vocab)
}

/// Implements [`Layout`] for a single-face layout; `$gating` maps the Main face to its gating.
macro_rules! printed_layout {
    ($(#[$meta:meta])* $name:ident, $behavior:expr, $gating:expr) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name;

        impl Layout for $name {
            fn behavior(&self) -> LayoutBehavior {
                $behavior
            }

            fn base_characteristics(
                &self,
                def: &CardDefinitionDSL,
                _query: &LayoutQuery,
                vocab: &RulesVocabulary,
            ) -> Result<Characteristics, LayoutError> {
                printed_chars($behavior, def, vocab)
            }

            fn mana_value(
                &self,
                def: &CardDefinitionDSL,
                _query: &LayoutQuery,
                ctx: ManaValueContext,
                vocab: &RulesVocabulary,
            ) -> Result<i64, LayoutError> {
                // CR 202.3: the object's own mana cost.
                Ok(printed_chars($behavior, def, vocab)?.mana_value(ctx))
            }

            fn gating(&self, def: &CardDefinitionDSL) -> Result<Option<Gating>, LayoutError> {
                let main = main_face($behavior, def)?;
                let gating: fn(&CardFaceDSL) -> Option<Gating> = $gating;
                Ok(gating(main))
            }
        }
    };
}

printed_layout!(
    /// NORMAL: the printed characteristics in every zone (CR 109.3).
    NormalLayout,
    LayoutBehavior::Normal,
    |_| None
);

printed_layout!(
    /// LEVELER (CR 711): the Main face with its uppermost P/T in every zone (CR 711.5, 711.6).
    LevelerLayout,
    LayoutBehavior::Leveler,
    // CR 711.2a, 711.2b: level bands are static abilities; CR 711.5, 711.6: the uppermost P/T.
    |main| Some(Gating::Leveler {
        uppermost_power: main.power.clone(),
        uppermost_toughness: main.toughness.clone(),
        abilities: main.abilities.clone(),
    })
);

printed_layout!(
    /// SAGA (CR 715): the printed characteristics in every zone; chapters are abilities
    /// (CR 715.2).
    SagaLayout,
    LayoutBehavior::Saga,
    // CR 715.2: chapter symbols are triggered chapter abilities.
    |main| Some(Gating::Saga {
        abilities: main.abilities.clone(),
    })
);

printed_layout!(
    /// CLASS (CR 717): the printed characteristics in every zone; levels are a designation and
    /// not copiable (CR 717.2b).
    ClassLayout,
    LayoutBehavior::Class,
    // CR 717.2a: class levels gate abilities; CR 717.2b: the level is a designation.
    |main| Some(Gating::Class {
        abilities: main.abilities.clone(),
    })
);

printed_layout!(
    /// CASE (CR 720): the printed characteristics in every zone; solved is a designation and
    /// not copiable (CR 720.3b).
    CaseLayout,
    LayoutBehavior::Case,
    // CR 720.3b: solved is a designation, neither an ability nor a copiable value.
    |main| Some(Gating::Case {
        abilities: main.abilities.clone(),
    })
);
