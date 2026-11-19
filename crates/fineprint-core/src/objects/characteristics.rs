// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The CR 109.3 characteristics of one face (REQ-OBJ-001).

use std::collections::BTreeSet;

use fineprint_dsl::card::{
    AbilityAST, CardFaceDSL, ColorBitmask, TextTokenStream, ValueExpressionAST,
};
use fineprint_dsl::mana::ManaCostDSL;
use fineprint_dsl::vocab::RulesVocabulary;

use super::colors::{CdaColors, base_colors};
use super::error::ObjectError;
use super::mana_value::{ManaValueContext, ManaValueSubject, mana_value};
use super::validation::validate_types;

/// The characteristics of an object or card face before continuous effects (CR 109.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Characteristics {
    /// Names; empty when an effect gives the object no name, two for a split card off the stack.
    pub names: BTreeSet<String>,
    /// Mana cost; `None` is "no mana cost", different from `{0}` (CR 202.3a).
    pub mana_cost: Option<ManaCostDSL>,
    /// Color indicator (CR 202.2e).
    pub color_indicator: Option<ColorBitmask>,
    /// Colors (CR 105.2, 202.2).
    pub colors: ColorBitmask,
    /// Supertypes (CR 205.4a).
    pub supertypes: BTreeSet<String>,
    /// Card types (CR 205.2a).
    pub card_types: BTreeSet<String>,
    /// Subtypes (CR 205.3c).
    pub subtypes: BTreeSet<String>,
    /// Rules text.
    pub rules_text: TextTokenStream,
    /// Abilities.
    pub abilities: Vec<AbilityAST>,
    /// Printed power: a number or a CDA expression (CR 208.1, 208.2).
    pub power: Option<ValueExpressionAST>,
    /// Printed toughness: a number or a CDA expression (CR 208.1, 208.2).
    pub toughness: Option<ValueExpressionAST>,
    /// Printed loyalty (CR 209.1).
    pub loyalty: Option<i64>,
    /// Printed defense (CR 210.1).
    pub defense: Option<i64>,
    /// Hand modifier (CR 211.1).
    pub hand_modifier: Option<i64>,
    /// Life modifier (CR 212.1).
    pub life_modifier: Option<i64>,
}

impl Characteristics {
    /// The characteristics printed on `face` (CR 109.3), after checking its type line
    /// ([`validate_types`], CR 205).
    ///
    /// `colors` comes from the cost and the color indicator only (CR 202.2, 202.2e); the colors
    /// of a characteristic-defining ability are applied by later work (CR 105.2).
    ///
    /// # Errors
    /// The [`ObjectError`] of [`validate_types`].
    pub fn from_face(
        face: &CardFaceDSL,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, ObjectError> {
        let supertypes: Vec<&str> = face.supertypes.iter().map(String::as_str).collect();
        let card_types: Vec<&str> = face.card_types.iter().map(String::as_str).collect();
        let subtypes: Vec<&str> = face.subtypes.iter().map(String::as_str).collect();
        // CR 205.2a, 205.3c, 205.4a: the type line must match the vocabulary.
        validate_types(&supertypes, &card_types, &subtypes, vocab)?;
        Ok(Characteristics {
            names: BTreeSet::from([face.name.clone()]),
            mana_cost: face.mana_cost.clone(),
            color_indicator: face.color_indicator,
            // CR 202.2, 202.2e: cost and indicator; CDA colors come later.
            colors: base_colors(
                face.mana_cost.as_ref(),
                face.color_indicator,
                CdaColors::None,
            ),
            supertypes: face.supertypes.iter().cloned().collect(),
            card_types: face.card_types.iter().cloned().collect(),
            subtypes: face.subtypes.iter().cloned().collect(),
            rules_text: face.text.clone(),
            abilities: face.abilities.clone(),
            power: face.power.clone(),
            toughness: face.toughness.clone(),
            loyalty: face.loyalty,
            defense: face.defense,
            hand_modifier: face.hand_modifier,
            life_modifier: face.life_modifier,
        })
    }

    /// The mana value of this object using its own mana cost (CR 202.3).
    #[must_use]
    pub fn mana_value(&self, ctx: ManaValueContext) -> i64 {
        mana_value(
            &ManaValueSubject::Plain {
                mana_cost: self.mana_cost.as_ref(),
            },
            ctx,
        )
    }

    /// Whether the object has power and toughness (CR 208.3, 208.5).
    ///
    /// A creature has power and toughness in every zone; a missing value counts as 0
    /// (CR 208.5). A noncreature has them only off the battlefield and only when a power or
    /// toughness is printed (CR 208.3); a noncreature permanent never has them.
    #[must_use]
    pub fn pt_applies(&self, on_battlefield: bool) -> bool {
        // CR 208.5: a creature always has power and toughness (0 when it has no value).
        // CR 208.3: a noncreature permanent has none; a noncreature object off the battlefield
        // has them only if printed.
        self.card_types.contains("Creature")
            || (!on_battlefield && (self.power.is_some() || self.toughness.is_some()))
    }
}
