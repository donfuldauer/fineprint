// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! ROOM (CR 709.5): a split permanent card with a shared type line whose halves (doors) are
//! locked or unlocked on the battlefield (REQ-OBJ-002).
//!
//! The existence of both halves is part of the object's copiable values in every zone, the
//! stack included (CR 709.5, 709.5b). That needs no extra API: the definition always keeps both
//! faces, and only the selection below depends on the zone, the cast half and the doors.
//!
//! The unlock special action (CR 709.5e) and effects that lock or unlock a door are actions and
//! are not implemented here; [`RoomDoors`] is plain state supplied by the caller.

use std::collections::BTreeSet;

use fineprint_dsl::card::{
    CardDefinitionDSL, CardFaceDSL, FaceKind, LayoutBehavior, TextTokenStream,
};
use fineprint_dsl::vocab::RulesVocabulary;

use super::faces::{chars, effective_ctx, faces};
use super::split::combined;
use super::types::{LayoutError, LayoutQuery};
use crate::objects::{CdaColors, Characteristics, ManaValueContext, SplitHalf, base_colors};
use crate::zones::ZoneKind;

/// ROOM (CR 709.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoomLayout;

const LAYOUT: LayoutBehavior = LayoutBehavior::Room;

/// The unlocked designations of a Room permanent (CR 709.5c, 709.5j: a door is a half).
///
/// `RoomDoors::default()` is [`RoomDoors::LOCKED`]: neither half unlocked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoomDoors {
    /// The permanent has the "left half unlocked" designation.
    pub left_unlocked: bool,
    /// The permanent has the "right half unlocked" designation.
    pub right_unlocked: bool,
}

impl RoomDoors {
    /// Neither half unlocked (CR 709.5c: both halves locked).
    pub const LOCKED: RoomDoors = RoomDoors {
        left_unlocked: false,
        right_unlocked: false,
    };

    /// The designations a Room permanent enters the battlefield with: only the half cast as a
    /// spell is unlocked; `None` (neither half cast) enters with neither designation
    /// (CR 709.5d).
    #[must_use]
    pub fn on_enter(cast: Option<SplitHalf>) -> RoomDoors {
        // CR 709.5d: the cast half's unlocked designation, or none.
        match cast {
            None => RoomDoors::LOCKED,
            Some(half) => RoomDoors::LOCKED.with_unlocked(half, true),
        }
    }

    /// Whether `half` is unlocked (CR 709.5c).
    #[must_use]
    pub fn is_unlocked(self, half: SplitHalf) -> bool {
        match half {
            SplitHalf::Left => self.left_unlocked,
            SplitHalf::Right => self.right_unlocked,
        }
    }

    /// These doors with `half` set to `unlocked`: a plain setter with no cost, priority or
    /// timing logic (the unlock special action, CR 709.5e, is not modelled).
    #[must_use]
    pub fn with_unlocked(self, half: SplitHalf, unlocked: bool) -> RoomDoors {
        match half {
            SplitHalf::Left => RoomDoors {
                left_unlocked: unlocked,
                ..self
            },
            SplitHalf::Right => RoomDoors {
                right_unlocked: unlocked,
                ..self
            },
        }
    }
}

/// Where a Room object is, which half was cast (consulted on the stack only) and its doors
/// (consulted on the battlefield only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoomQuery {
    /// The zone the object is in.
    pub zone: ZoneKind,
    /// The half being cast; `None` when not cast.
    pub cast: Option<SplitHalf>,
    /// The unlocked designations.
    pub doors: RoomDoors,
}

impl RoomQuery {
    /// A query for `zone`, not cast, with both doors locked.
    #[must_use]
    pub fn new(zone: ZoneKind) -> RoomQuery {
        RoomQuery {
            zone,
            cast: None,
            doors: RoomDoors::LOCKED,
        }
    }

    /// This query with `half` as the half cast.
    #[must_use]
    pub fn with_cast(self, half: SplitHalf) -> RoomQuery {
        RoomQuery {
            cast: Some(half),
            ..self
        }
    }

    /// This query with `doors` as the unlocked designations.
    #[must_use]
    pub fn with_doors(self, doors: RoomDoors) -> RoomQuery {
        RoomQuery { doors, ..self }
    }
}

/// A half's characteristics with its name, mana cost, rules text and abilities removed: a
/// locked half on the battlefield (CR 709.5). Its type line and color indicator stay.
fn locked(half: Characteristics) -> Characteristics {
    Characteristics {
        names: Default::default(),
        mana_cost: None,
        colors: base_colors(None, half.color_indicator, CdaColors::None),
        rules_text: TextTokenStream::default(),
        abilities: Vec::new(),
        ..half
    }
}

/// The entries of one type-line field as a set: [`Characteristics`] and the combined view
/// keep types as sets, so order and repeats carry no meaning (DECISIONS.md #23).
fn type_set(items: &[String]) -> BTreeSet<&str> {
    items.iter().map(String::as_str).collect()
}

/// Whether the two halves have one shared type line: equal supertypes, card types and
/// subtypes (CR 709.5a).
fn shared_type_line(left: &CardFaceDSL, right: &CardFaceDSL) -> bool {
    // CR 709.5a: each half shares the types and subtypes on the card's one shared type line.
    type_set(&left.supertypes) == type_set(&right.supertypes)
        && type_set(&left.card_types) == type_set(&right.card_types)
        && type_set(&left.subtypes) == type_set(&right.subtypes)
}

impl RoomLayout {
    /// The layout behavior this object handles: [`LayoutBehavior::Room`].
    #[must_use]
    pub fn behavior(&self) -> LayoutBehavior {
        LAYOUT
    }

    /// The left and right halves, and on the stack the half being cast.
    ///
    /// Every zone runs this, so no zone skips the face-set and shared-type-line checks.
    fn select<'a>(
        def: &'a CardDefinitionDSL,
        query: &RoomQuery,
    ) -> Result<(&'a CardFaceDSL, &'a CardFaceDSL, Option<SplitHalf>), LayoutError> {
        let [left, right] = faces(LAYOUT, def, [FaceKind::LeftHalf, FaceKind::RightHalf])?;
        // CR 709.5a: a Room has one shared type line; halves whose type lines differ are a
        // malformed definition, in every zone (before the cast half is checked).
        if !shared_type_line(left, right) {
            return Err(LayoutError::BadFaces { layout: LAYOUT });
        }
        if query.zone != ZoneKind::Stack {
            return Ok((left, right, None));
        }
        match query.cast {
            // CR 709.3: the half being cast is chosen before the card goes on the stack.
            Some(half) => Ok((left, right, Some(half))),
            None => Err(LayoutError::InvalidCast {
                layout: LAYOUT,
                zone: query.zone,
            }),
        }
    }

    /// The base characteristics of a Room object with definition `def` for `query`, before any
    /// layer effect.
    ///
    /// - Stack: only the cast half (CR 709.3b). Its type line is the shared type line, which
    ///   both halves carry (CR 709.5a; 709.5b does not stop that on the stack).
    /// - Battlefield: the name, mana cost, rules text and abilities of each *unlocked* half;
    ///   the type line is always both halves' (the shared type line, CR 709.5, 709.5a). Colors
    ///   come from the remaining mana cost and the halves' color indicators (DECISIONS.md #23).
    /// - Every other zone: both halves combined (CR 709.4, 709.4a, 709.4b, 709.4c).
    ///
    /// Power, toughness, loyalty, defense and the modifiers are `None` off the stack.
    ///
    /// # Errors
    /// [`LayoutError::LayoutMismatch`], [`LayoutError::BadFaces`] (faces other than exactly
    /// one left half and one right half, or the halves' type lines differ: supertypes, card
    /// types or subtypes, compared as sets, CR 709.5a), [`LayoutError::InvalidCast`] (on the stack with no
    /// cast half), then [`LayoutError::Object`], in that order.
    pub fn base_characteristics(
        &self,
        def: &CardDefinitionDSL,
        query: &RoomQuery,
        vocab: &RulesVocabulary,
    ) -> Result<Characteristics, LayoutError> {
        let (left, right, cast) = Self::select(def, query)?;
        match cast {
            // CR 709.3b: on the stack only the half being cast exists.
            Some(SplitHalf::Left) => return chars(left, vocab),
            Some(SplitHalf::Right) => return chars(right, vocab),
            None => {}
        }
        let (l, r) = (chars(left, vocab)?, chars(right, vocab)?);
        if query.zone != ZoneKind::Battlefield {
            // CR 709.4, 709.4a, 709.4b, 709.4c: both halves combined.
            return Ok(combined(l, r));
        }
        // CR 709.5: a half without its unlocked designation has no name, mana cost or rules
        // text; CR 709.5a: both halves share the type line, so the types always stay.
        let l = if query.doors.is_unlocked(SplitHalf::Left) {
            l
        } else {
            locked(l)
        };
        let r = if query.doors.is_unlocked(SplitHalf::Right) {
            r
        } else {
            locked(r)
        };
        let mut out = combined(l, r);
        // CR 202.2: the colors of the remaining mana cost (left then right), plus the color
        // indicators, which CR 709.5 does not remove.
        out.colors = base_colors(out.mana_cost.as_ref(), out.color_indicator, CdaColors::None);
        Ok(out)
    }

    /// The mana value of that object.
    ///
    /// On the stack: the cast half's mana cost, with `ctx` for X, Y and Z (CR 202.3d, 202.3e).
    /// Off the battlefield and stack: both halves' costs combined (CR 202.3d). On the
    /// battlefield: the unlocked halves' costs, 0 with neither unlocked (CR 709.5, 202.3,
    /// 202.3a). Off the stack X, Y and Z count 0 whatever `ctx` says (CR 202.3e; DECISIONS.md
    /// #22 Layouts API).
    ///
    /// # Errors
    /// As [`RoomLayout::base_characteristics`]: the same selection runs first.
    pub fn mana_value(
        &self,
        def: &CardDefinitionDSL,
        query: &RoomQuery,
        ctx: ManaValueContext,
        vocab: &RulesVocabulary,
    ) -> Result<i64, LayoutError> {
        let selected = self.base_characteristics(def, query, vocab)?;
        // CR 202.3: the total mana in the selected mana cost (the cast half, the combined cost,
        // or the unlocked halves' symbols); CR 202.3a: no mana cost is 0.
        Ok(selected.mana_value(effective_ctx(&LayoutQuery::new(query.zone), ctx)))
    }
}
