// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Face-down profiles (REQ-OBJ-003, CR 708): the characteristics a face-down spell or permanent
//! has, and the reveal rule when one leaves the battlefield or the stack.
//!
//! A [`FaceDownProfile`] is the list of characteristics "listed by the ability or rules that
//! allowed the spell or permanent to be face down" (CR 708.2). It holds public characteristics
//! only: nothing of the face-up card is in it (REQ-CORE-004). Three shapes exist:
//!
//! * [`FaceDownProfile::default_2_2`]: the 2/2 creature with no text, no name, no subtypes and
//!   no mana cost (CR 708.2a), used by morph and megamorph (CR 702.37a, 702.37b), manifest
//!   (CR 701.40a) and manifest dread, which only manifests (CR 701.62a);
//! * [`FaceDownProfile::with_ward_2`]: the same with ward {2}, used by disguise (CR 702.168a,
//!   702.168b) and cloak (CR 701.58a);
//! * [`FaceDownProfile::custom`]: the characteristics a card effect lists itself (CR 708.2).
//!
//! [`FaceDownProfile::for_cause`] maps each [`FaceDownCause`] to its profile. The profile is
//! applied as a layer-1b effect by [`crate::layers::face_down_effect`] (CR 613.2b) and so is the
//! object's copiable values (CR 613.2c, 708.2).
//!
//! [`must_reveal_on_leave`] is the reveal rule of CR 708.9 for one move. The other reveals of
//! CR 708.9 (a player leaving the game, the end of the game) and the looks of CR 708.5 already
//! exist in [`crate::knowledge::Knowledge`] (`on_move`, `on_removed_from_hidden_zone`,
//! `on_player_leaves`, `on_game_end`); nothing is reimplemented here.
//!
//! **Not implemented here** (later units): the turn-face-up special action (CR 116.2b) and its
//! costs, "as ... is turned face up" power and toughness settings applied at turn-up
//! (CR 708.11), and wiring the reveal rule into zone moves.
//!
//! **Determinism.** No floats, clock or I/O; only ordered containers (REQ-CORE-006).

use std::collections::BTreeSet;

use fineprint_dsl::card::{AbilityAST, ColorBitmask, TextTokenStream, ValueExpressionAST};

use crate::objects::Characteristics;
use crate::zones::ZoneId;

/// The cost of the ward ability of the disguise and cloak profiles (CR 702.168a, 701.58a).
const WARD_COST: &str = "{2}";

/// The ward {2} keyword ability of the disguise and cloak profiles (CR 702.168a, 701.58a):
/// kind `KEYWORD`, ability id `facedown:ward`, keyword `ward`, cost `{2}` (the keyword shape of
/// [`crate::layers::keyword_counter_ability`] with a cost member).
fn ward_2_ability() -> AbilityAST {
    AbilityAST(
        [
            ("kind", "KEYWORD"),
            ("abilityId", "facedown:ward"),
            ("keyword", "ward"),
            ("cost", WARD_COST),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect(),
    )
}

/// What turned an object face down, for [`FaceDownProfile::for_cause`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceDownCause {
    /// Morph (CR 702.37a).
    Morph,
    /// Megamorph, a variant of morph (CR 702.37b).
    Megamorph,
    /// Manifest (CR 701.40a).
    Manifest,
    /// Manifest dread, which manifests one card (CR 701.62a).
    ManifestDread,
    /// Disguise (CR 702.168a).
    Disguise,
    /// Cloak (CR 701.58a).
    Cloak,
}

/// The characteristics of a face-down spell or permanent (CR 708.2): exactly those listed by
/// the rule or effect that turned it face down. They become the object's copiable values
/// through a layer-1b effect (CR 613.2b, 613.2c).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceDownProfile {
    characteristics: Characteristics,
}

impl FaceDownProfile {
    /// The default profile (CR 708.2a): a 2/2 creature with no text, no name, no subtypes and no
    /// mana cost. It is colorless with no color indicator, and has no supertypes, abilities,
    /// loyalty, defense or hand and life modifiers.
    #[must_use]
    pub fn default_2_2() -> FaceDownProfile {
        FaceDownProfile {
            characteristics: Characteristics {
                names: BTreeSet::new(),
                mana_cost: None,
                color_indicator: None,
                colors: ColorBitmask::NONE,
                supertypes: BTreeSet::new(),
                card_types: BTreeSet::from(["Creature".to_owned()]),
                subtypes: BTreeSet::new(),
                rules_text: TextTokenStream(Vec::new()),
                abilities: Vec::new(),
                power: Some(ValueExpressionAST(2.into())),
                toughness: Some(ValueExpressionAST(2.into())),
                loyalty: None,
                defense: None,
                hand_modifier: None,
                life_modifier: None,
            },
        }
    }

    /// The disguise and cloak profile (CR 702.168a, 702.168b, 701.58a): the default profile
    /// plus exactly one ability, ward {2}, as a keyword ability with kind `KEYWORD`, ability id
    /// `facedown:ward`, keyword `ward` and cost `{2}`.
    #[must_use]
    pub fn with_ward_2() -> FaceDownProfile {
        let mut profile = FaceDownProfile::default_2_2();
        profile.characteristics.abilities.push(ward_2_ability());
        profile
    }

    /// The characteristics a card effect lists for the face-down object (CR 708.2), stored as
    /// given. The engine does not inspect them (REQ-DSL-013).
    #[must_use]
    pub fn custom(characteristics: Characteristics) -> FaceDownProfile {
        FaceDownProfile { characteristics }
    }

    /// The profile for `cause`: the default for morph, megamorph, manifest and manifest dread
    /// (CR 702.37a, 702.37b, 701.40a, 701.62a), the ward {2} profile for disguise and cloak
    /// (CR 702.168a, 702.168b, 701.58a).
    #[must_use]
    pub fn for_cause(cause: FaceDownCause) -> FaceDownProfile {
        match cause {
            FaceDownCause::Morph
            | FaceDownCause::Megamorph
            | FaceDownCause::Manifest
            | FaceDownCause::ManifestDread => FaceDownProfile::default_2_2(),
            FaceDownCause::Disguise | FaceDownCause::Cloak => FaceDownProfile::with_ward_2(),
        }
    }

    /// The listed characteristics.
    #[must_use]
    pub fn characteristics(&self) -> &Characteristics {
        &self.characteristics
    }

    /// Whether the abilities contain the ward {2} keyword ability of
    /// [`FaceDownProfile::with_ward_2`].
    #[must_use]
    pub fn has_ward_2(&self) -> bool {
        let ward = ward_2_ability();
        self.characteristics.abilities.contains(&ward)
    }
}

/// Whether a face-down object moving from `from` to `to` must be revealed to all players as it
/// moves (CR 708.9): a face-down permanent leaving the battlefield for any other zone, or a
/// face-down spell leaving the stack for any zone other than the battlefield. `false` for a
/// face-up object and for a move within one zone.
#[must_use]
pub fn must_reveal_on_leave(face_down: bool, from: ZoneId, to: ZoneId) -> bool {
    if !face_down || from == to {
        return false;
    }
    match from {
        // CR 708.9: "moves from the battlefield to any other zone".
        ZoneId::Battlefield => true,
        // CR 708.9: "moves from the stack to any zone other than the battlefield".
        ZoneId::Stack => to != ZoneId::Battlefield,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    // Unit tests of face-down profiles: REQ-OBJ-003 (CR 708.2, 708.2a, 708.9, 702.37a-b,
    // 701.40a, 701.62a, 702.168a-b, 701.58a). Synthetic values only (R5).

    use super::*;
    use fineprint_base::ids::PlayerId;

    fn ward_json() -> AbilityAST {
        AbilityAST(
            [
                ("kind", "KEYWORD"),
                ("abilityId", "facedown:ward"),
                ("keyword", "ward"),
                ("cost", "{2}"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
        )
    }

    // REQ-OBJ-003; CR 708.2a: the default profile, field by field.
    #[test]
    fn req_obj_003_default_2_2_fields() {
        let c = FaceDownProfile::default_2_2().characteristics().clone();
        assert!(c.names.is_empty());
        assert_eq!(c.mana_cost, None);
        assert_eq!(c.color_indicator, None);
        assert_eq!(c.colors, ColorBitmask::NONE);
        assert!(c.supertypes.is_empty());
        assert_eq!(c.card_types, BTreeSet::from(["Creature".to_owned()]));
        assert!(c.subtypes.is_empty());
        assert!(c.rules_text.0.is_empty());
        assert!(c.abilities.is_empty());
        assert_eq!(c.power, Some(ValueExpressionAST(2.into())));
        assert_eq!(c.toughness, Some(ValueExpressionAST(2.into())));
        assert_eq!(c.loyalty, None);
        assert_eq!(c.defense, None);
        assert_eq!(c.hand_modifier, None);
        assert_eq!(c.life_modifier, None);
        assert!(!FaceDownProfile::default_2_2().has_ward_2());
    }

    // REQ-OBJ-003; CR 702.168a, 701.58a: the ward variant is the default plus exactly the ward
    // {2} keyword ability.
    #[test]
    fn req_obj_003_with_ward_2_adds_exactly_ward() {
        let ward = FaceDownProfile::with_ward_2();
        let mut c = ward.characteristics().clone();
        assert_eq!(c.abilities, vec![ward_json()]);
        assert!(ward.has_ward_2());
        c.abilities.clear();
        assert_eq!(&c, FaceDownProfile::default_2_2().characteristics());
    }

    // REQ-OBJ-003; CR 702.37a, 702.37b, 701.40a, 701.62a, 702.168a, 701.58a: the cause table.
    #[test]
    fn req_obj_003_for_cause_table() {
        let plain = FaceDownProfile::default_2_2();
        let ward = FaceDownProfile::with_ward_2();
        for (cause, want) in [
            (FaceDownCause::Morph, &plain),
            (FaceDownCause::Megamorph, &plain),
            (FaceDownCause::Manifest, &plain),
            (FaceDownCause::ManifestDread, &plain),
            (FaceDownCause::Disguise, &ward),
            (FaceDownCause::Cloak, &ward),
        ] {
            assert_eq!(&FaceDownProfile::for_cause(cause), want, "{cause:?}");
        }
    }

    // REQ-OBJ-003; CR 708.2: a custom profile is stored as given; has_ward_2 reads its
    // abilities.
    #[test]
    fn req_obj_003_custom_stored_as_given() {
        let mut c = FaceDownProfile::default_2_2().characteristics().clone();
        c.card_types.insert("Artifact".to_owned());
        c.subtypes.insert("Golem".to_owned());
        let profile = FaceDownProfile::custom(c.clone());
        assert_eq!(profile.characteristics(), &c);
        assert!(!profile.has_ward_2());
        c.abilities.push(ward_json());
        assert!(FaceDownProfile::custom(c.clone()).has_ward_2());
        // A ward keyword with another cost is not ward {2}.
        let mut other = ward_json();
        if let Some(m) = other.0.as_object_mut() {
            m.insert("cost".to_owned(), "{3}".into());
        }
        c.abilities = vec![other];
        assert!(!FaceDownProfile::custom(c).has_ward_2());
    }

    // REQ-OBJ-003, REQ-CORE-004; CR 708.9: the reveal truth table over every pair of zones.
    #[test]
    fn req_obj_003_must_reveal_on_leave_truth_table() {
        let pl = PlayerId::new(1);
        let zones = [
            ZoneId::Battlefield,
            ZoneId::Stack,
            ZoneId::Hand(pl),
            ZoneId::Graveyard(pl),
            ZoneId::Exile,
            ZoneId::Library(pl),
        ];
        for face_down in [false, true] {
            for from in zones {
                for to in zones {
                    let want = face_down
                        && from != to
                        && (from == ZoneId::Battlefield
                            || (from == ZoneId::Stack && to != ZoneId::Battlefield));
                    assert_eq!(
                        must_reveal_on_leave(face_down, from, to),
                        want,
                        "{face_down} {from:?} {to:?}"
                    );
                }
            }
        }
        // Spot checks of the rule text.
        assert!(must_reveal_on_leave(
            true,
            ZoneId::Battlefield,
            ZoneId::Exile
        ));
        assert!(must_reveal_on_leave(true, ZoneId::Stack, ZoneId::Hand(pl)));
        assert!(!must_reveal_on_leave(
            true,
            ZoneId::Stack,
            ZoneId::Battlefield
        ));
        assert!(!must_reveal_on_leave(true, ZoneId::Exile, ZoneId::Hand(pl)));
        assert!(!must_reveal_on_leave(true, ZoneId::Stack, ZoneId::Stack));
        assert!(!must_reveal_on_leave(
            false,
            ZoneId::Battlefield,
            ZoneId::Graveyard(pl)
        ));
    }
}
