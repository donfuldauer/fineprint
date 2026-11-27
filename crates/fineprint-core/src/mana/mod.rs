// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Mana: the per-player mana pool with per-unit restrictions and riders, and the payment
//! planner that pays a mana component from a pool (REQ-CAST-001; CR 106, 107.4, 118.3a, 500.5,
//! 601.2h).
//!
//! # Units
//!
//! Every mana in a [`ManaPool`] is one [`ManaUnit`], tracked individually even when several
//! units have the same type (CR 106.6a: the restrictions and riders of the producing spell or
//! ability apply to every mana it produced). A unit carries its [`ManaType`] (CR 106.1b), its
//! source (CR 106.3), whether that source is a snow source (CR 107.4h), its spend restrictions
//! ([`ManaRestriction`]) and its riders ([`ManaRider`], CR 106.6).
//!
//! # Restrictions and purposes
//!
//! A payment is described by a [`SpendPurpose`]: a set of opaque, lowercase, generic tags such
//! as `"spell"`, `"ability"`, `"spell:artifact"`, `"ability:activated"` or `"x_cost"`. Tags are
//! never card names (R4, R5) and come from untrusted card data (R12): they are compared as exact
//! strings, and an unknown tag matches nothing. A unit is *allowed* for a purpose iff every one
//! of its restrictions has at least one of its tags in the purpose; an unrestricted unit is
//! allowed everywhere, and a restriction with no tag allows nothing (CR 106.6).
//!
//! # Riders
//!
//! The engine does not interpret riders (CR 106.6: an additional effect on the spell or ability
//! the mana is spent on, or a delayed trigger when it is spent). It reports, per spent unit, the
//! riders that unit carried; applying them is the host's job. One record per spent unit, so a
//! rider produced on several mana fires once per mana (CR 106.6a).
//!
//! # The payment planner
//!
//! [`plan_payment`] finds which units of a pool pay a [`ManaComponent`] for a purpose, or
//! `None` when no complete assignment exists (partial payments are not allowed, CR 601.2h):
//!
//! - a colored symbol needs a unit of exactly that color (CR 107.4a);
//! - `{C}` needs a colorless unit (CR 107.4c);
//! - `{S}` needs a unit of any type produced by a snow source (CR 107.4h);
//! - generic mana is paid by any unit (CR 107.4b);
//! - only units allowed for the purpose are used.
//!
//! It always finds a complete assignment when one exists (an exact counting feasibility check
//! guards every greedy step, so a flexible unit can never starve a colored symbol). A `{0}`
//! component is paid with the empty plan; the payment of `{0}` still has to be acknowledged by
//! the caller (CR 118.5).
//!
//! **Tie-break.** Among the valid assignments, the planner spends the allowed units in this
//! preference order: most restrictions first, then lowest pool index. It walks the units in that
//! order and spends a unit whenever some assignment of it (to its own type's symbols first, then
//! `{S}`, then generic) still leaves the rest of the cost payable by the units after it;
//! otherwise it skips the unit. Restricted mana is thus used up before flexible mana, and the
//! result depends only on the pool, the cost and the purpose (R3).
//!
//! [`spend`] removes exactly the planned units (CR 118.3a); the rest stays in the pool as
//! unspent mana (CR 106.4, 106.4a).
//!
//! # Lifecycle
//!
//! [`ManaPool::empty`] empties a pool as a step or phase ends (CR 500.5, 703.4q). The turn
//! structure calls it; this module only provides it.
//!
//! # Determinism
//!
//! Pools are ordered `Vec`s in insertion order; tags are `BTreeSet`s; nothing iterates a hash
//! container and there is no floating point (R3). Every value has a canonical encoding.

#![deny(missing_docs)]

mod plan;
mod pool;

#[cfg(test)]
mod unit_tests;

use std::collections::BTreeSet;

use fineprint_base::encode::{CanonicalEncode, put_variant};
use fineprint_base::ids::ObjectId;
use fineprint_dsl::mana::Color;

pub use crate::casting::ManaComponent;
pub use plan::{PaymentPlan, plan_payment, spend};
pub use pool::ManaPool;

/// One of the six types of mana (CR 106.1b).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ManaType {
    /// White mana.
    White,
    /// Blue mana.
    Blue,
    /// Black mana.
    Black,
    /// Red mana.
    Red,
    /// Green mana.
    Green,
    /// Colorless mana.
    Colorless,
}

impl ManaType {
    /// All six types in declaration order (W, U, B, R, G, colorless).
    pub const ALL: [ManaType; 6] = [
        ManaType::White,
        ManaType::Blue,
        ManaType::Black,
        ManaType::Red,
        ManaType::Green,
        ManaType::Colorless,
    ];

    /// The mana type of color `c` (CR 106.1b).
    #[must_use]
    pub fn from_color(c: Color) -> ManaType {
        match c {
            Color::White => ManaType::White,
            Color::Blue => ManaType::Blue,
            Color::Black => ManaType::Black,
            Color::Red => ManaType::Red,
            Color::Green => ManaType::Green,
        }
    }

    /// The color of this type, or `None` for colorless.
    #[must_use]
    pub fn color(self) -> Option<Color> {
        match self {
            ManaType::White => Some(Color::White),
            ManaType::Blue => Some(Color::Blue),
            ManaType::Black => Some(Color::Black),
            ManaType::Red => Some(Color::Red),
            ManaType::Green => Some(Color::Green),
            ManaType::Colorless => None,
        }
    }

    /// The lowercase identifier used in spell-history keys: `"white"`, `"blue"`, `"black"`,
    /// `"red"`, `"green"` or `"colorless"`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ManaType::White => "white",
            ManaType::Blue => "blue",
            ManaType::Black => "black",
            ManaType::Red => "red",
            ManaType::Green => "green",
            ManaType::Colorless => "colorless",
        }
    }

    /// The position of this type in [`ManaType::ALL`].
    pub(crate) fn index(self) -> usize {
        match self {
            ManaType::White => 0,
            ManaType::Blue => 1,
            ManaType::Black => 2,
            ManaType::Red => 3,
            ManaType::Green => 4,
            ManaType::Colorless => 5,
        }
    }
}

impl CanonicalEncode for ManaType {
    /// One tag byte: W 0, U 1, B 2, R 3, G 4, colorless 5 (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        let tag = match self {
            ManaType::White => 0,
            ManaType::Blue => 1,
            ManaType::Black => 2,
            ManaType::Red => 3,
            ManaType::Green => 4,
            ManaType::Colorless => 5,
        };
        put_variant(out, tag);
    }
}

/// A spend restriction on one mana unit (CR 106.6): the unit may be spent only on a payment
/// whose [`SpendPurpose`] carries at least one of these tags.
///
/// Tags are opaque, lowercase, generic identifiers (never card names, R4, R5). A restriction
/// with no tag allows nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ManaRestriction {
    tags: BTreeSet<String>,
}

impl ManaRestriction {
    /// A restriction "spend this mana only on a payment tagged with one of `tags`" (CR 106.6).
    #[must_use]
    pub fn only_for<'a>(tags: impl IntoIterator<Item = &'a str>) -> ManaRestriction {
        ManaRestriction {
            tags: tags.into_iter().map(str::to_owned).collect(),
        }
    }

    /// The tags, ascending.
    #[must_use]
    pub fn tags(&self) -> &BTreeSet<String> {
        &self.tags
    }

    /// Whether `purpose` carries at least one of the tags (CR 106.6).
    #[must_use]
    pub fn allows(&self, purpose: &SpendPurpose) -> bool {
        self.tags.iter().any(|t| purpose.tags.contains(t))
    }
}

impl CanonicalEncode for ManaRestriction {
    /// The tag set (REQ-NFR-006).
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        self.tags.encode_canonical(out);
    }
}

/// An opaque rider on one mana unit (CR 106.6): an additional effect on, or a delayed trigger
/// for, whatever the unit is spent on. The engine only reports which riders fired.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ManaRider {
    /// The rider's generic identifier, for example `"grants_haste"` (never a card name).
    pub id: String,
}

impl ManaRider {
    /// A rider with identifier `id`.
    #[must_use]
    pub fn new(id: &str) -> ManaRider {
        ManaRider { id: id.to_owned() }
    }
}

fineprint_base::impl_canonical_struct!(ManaRider { id });

/// One mana in a pool (CR 106.4), tracked individually with its restrictions and riders
/// (CR 106.6, 106.6a).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ManaUnit {
    /// The mana's type (CR 106.1b).
    pub mana_type: ManaType,
    /// The source of the mana (CR 106.3).
    pub source: ObjectId,
    /// Whether the source is a snow source, so the unit can pay `{S}` (CR 107.4h).
    pub source_is_snow: bool,
    /// Spend restrictions; all of them must be satisfied (CR 106.6).
    pub restrictions: Vec<ManaRestriction>,
    /// Riders reported when the unit is spent (CR 106.6).
    pub riders: Vec<ManaRider>,
}

impl ManaUnit {
    /// An unrestricted, rider-free, non-snow unit of `mana_type` from `source`.
    #[must_use]
    pub fn new(mana_type: ManaType, source: ObjectId) -> ManaUnit {
        ManaUnit {
            mana_type,
            source,
            source_is_snow: false,
            restrictions: Vec::new(),
            riders: Vec::new(),
        }
    }

    /// The same unit, produced by a snow source (CR 107.4h).
    #[must_use]
    pub fn snow(mut self) -> ManaUnit {
        self.source_is_snow = true;
        self
    }

    /// The same unit with one more spend restriction (CR 106.6).
    #[must_use]
    pub fn restricted(mut self, restriction: ManaRestriction) -> ManaUnit {
        self.restrictions.push(restriction);
        self
    }

    /// The same unit with one more rider (CR 106.6).
    #[must_use]
    pub fn with_rider(mut self, rider: ManaRider) -> ManaUnit {
        self.riders.push(rider);
        self
    }

    /// Whether the unit may be spent for `purpose`: every restriction allows it (CR 106.6).
    #[must_use]
    pub fn allowed_for(&self, purpose: &SpendPurpose) -> bool {
        self.restrictions.iter().all(|r| r.allows(purpose))
    }

    /// The spell-history source-type key of the unit: `"snow"` or `"nonsnow"`.
    #[must_use]
    pub fn source_kind(&self) -> &'static str {
        if self.source_is_snow {
            "snow"
        } else {
            "nonsnow"
        }
    }
}

fineprint_base::impl_canonical_struct!(ManaUnit {
    mana_type,
    source,
    source_is_snow,
    restrictions,
    riders,
});

/// What mana is being spent on, as a set of opaque generic tags (CR 106.6).
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpendPurpose {
    /// The tags, for example `"spell"` and `"spell:artifact"`.
    pub tags: BTreeSet<String>,
}

impl SpendPurpose {
    /// A purpose with these tags.
    #[must_use]
    pub fn new<'a>(tags: impl IntoIterator<Item = &'a str>) -> SpendPurpose {
        SpendPurpose {
            tags: tags.into_iter().map(str::to_owned).collect(),
        }
    }
}

fineprint_base::impl_canonical_struct!(SpendPurpose { tags });

/// A payment-method modifier offered while paying a cost (convoke-style; SRS 7.1 step 8,
/// CR 601.2h): a display label and the objects that may be used with it. The offers feed the
/// `methods` field of the `PAY_MANA_AND_COSTS` request. A method never changes the locked total
/// cost; it applies only after the total cost is determined (CR 702.51b, 702.126b).
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaymentMethodOffer {
    /// The display label of the method.
    pub label: String,
    /// The objects that may be used with the method, ascending.
    pub eligible: Vec<ObjectId>,
}

fineprint_base::impl_canonical_struct!(PaymentMethodOffer { label, eligible });
