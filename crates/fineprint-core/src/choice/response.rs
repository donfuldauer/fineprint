// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Answers to pending choice requests (REQ-NET-003): one variant per response of the SRS 15.3
//! table.

use fineprint_base::encode::{CanonicalEncode, put_variant};
use fineprint_base::ids::{ObjectId, PlayerId};

use super::{ChoiceTarget, ChoiceType};

/// A chosen option value (REQ-NET-003): a number or a text such as a color or creature type.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChoiceValue {
    /// A number (canonical tag 0, then the `i64`).
    Number(i64),
    /// A text value (canonical tag 1, then the string).
    Text(String),
}

impl CanonicalEncode for ChoiceValue {
    /// Tag `0` + `i64`, or tag `1` + string.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        match self {
            ChoiceValue::Number(n) => {
                put_variant(out, 0);
                n.encode_canonical(out);
            }
            ChoiceValue::Text(s) => {
                put_variant(out, 1);
                s.encode_canonical(out);
            }
        }
    }
}

/// A player's answer to a [`PendingChoiceRequest`](super::PendingChoiceRequest)
/// (REQ-NET-003), one variant per response of the SRS 15.3 table.
///
/// Every variant belongs to exactly one [`ChoiceType`] ([`ChoiceResponse::choice_type`]).
/// Responses arrive from clients and are untrusted (R12): validate them with
/// [`PendingChoiceRequest::validate`](super::PendingChoiceRequest::validate).
///
/// The canonical encoding is a one-byte tag (given per variant) followed by the fields in
/// declaration order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChoiceResponse {
    /// `PASS_PRIORITY` (tag 0).
    PassPriority,
    /// `PLAY_LAND` (tag 1).
    PlayLand {
        /// The land to play.
        object: ObjectId,
        /// Which face (0-based).
        face_index: u32,
    },
    /// `INITIATE_CAST` (tag 2).
    InitiateCast {
        /// The card to cast.
        object: ObjectId,
        /// Which face (0-based).
        face_index: u32,
        /// The cast mode (normal or an alternative cast) as listed in the request.
        cast_mode: String,
    },
    /// `ACTIVATE` (tag 3).
    Activate {
        /// The source of the ability.
        object: ObjectId,
        /// The ability's id as listed in the request.
        ability_id: String,
    },
    /// `SPECIAL_ACTION` (tag 4): CR 116.2.
    SpecialAction {
        /// The special-action kind as listed in the request.
        kind: String,
        /// The object it involves.
        object: ObjectId,
    },
    /// `SELECT_MODES` (tag 5): indices into the request's mode list.
    SelectModes {
        /// The chosen mode indices (0-based); repeats are rejected unless the request allows them.
        modes: Vec<u32>,
    },
    /// `CONFIGURE_COSTS` (tag 6): indices refer to the request's
    /// [`ChoicePayload::ChooseCostsAndX`](super::ChoicePayload::ChooseCostsAndX) lists.
    ConfigureCosts {
        /// The chosen alternative cost (index), or `None` for the normal cost.
        alternative: Option<u32>,
        /// The chosen additional costs (indices, distinct).
        additional: Vec<u32>,
        /// The value of X, if the request has an X range.
        x: Option<u64>,
        /// One chosen option index per cost-symbol slot (hybrid/Phyrexian), in slot order.
        symbol_options: Vec<u32>,
        /// The creature chosen for harmonize, if any.
        harmonize: Option<ObjectId>,
    },
    /// `SELECT_TARGETS` (tag 7).
    SelectTargets {
        /// One list of chosen targets per target slot of the request, in slot order.
        targets: Vec<Vec<ChoiceTarget>>,
    },
    /// `SUBMIT_DIVISION` (tag 8).
    SubmitDivision {
        /// One `(recipient, amount)` pair per recipient of the request.
        assignments: Vec<(ChoiceTarget, u64)>,
    },
    /// `PAY_COSTS` (tag 9).
    PayCosts {
        /// `true`: use the request's auto-pay suggestion; the other fields must then be empty.
        auto_pay: bool,
        /// Payment-method uses: `(method index, objects used)`. Each method index appears at
        /// most once; list all objects for one method in a single entry.
        methods: Vec<(u32, Vec<ObjectId>)>,
        /// Mana abilities to activate: `(source, ability id)` as listed in the request.
        mana_abilities: Vec<(ObjectId, String)>,
    },
    /// `DECLINE_OPTIONAL_PAYMENT` (tag 10).
    DeclineOptionalPayment,
    /// `CANCEL_AND_ROLLBACK` (tag 11): CR 734.
    CancelAndRollback,
    /// `SUBMIT_ATTACKERS` (tag 12).
    SubmitAttackers {
        /// `(attacker, what it attacks)` pairs; each attacker at most once.
        attacks: Vec<(ObjectId, ChoiceTarget)>,
    },
    /// `SUBMIT_BLOCKERS` (tag 13).
    SubmitBlockers {
        /// `(blocker, attackers it blocks)` pairs; each blocker at most once.
        blocks: Vec<(ObjectId, Vec<ObjectId>)>,
    },
    /// `ORDER_TRIGGERS` (tag 14).
    OrderTriggers {
        /// A permutation of the request's trigger indices; the first is put on the stack first.
        order: Vec<u32>,
    },
    /// `YES` (tag 15).
    Yes,
    /// `NO` (tag 16).
    No,
    /// `SELECT_REPLACEMENT` (tag 17).
    SelectReplacement {
        /// The index of the chosen effect in the request's list.
        index: u32,
    },
    /// `SUBMIT_OBJECT_SELECTION` (tag 18).
    SubmitObjectSelection {
        /// The chosen objects (distinct).
        objects: Vec<ObjectId>,
    },
    /// `SUBMIT_ORDERING` (tag 19).
    SubmitOrdering {
        /// Cards put on top, the first listed ends up on top.
        top: Vec<ObjectId>,
        /// Cards put on the bottom, in order.
        bottom: Vec<ObjectId>,
        /// Cards put into the graveyard, in order.
        graveyard: Vec<ObjectId>,
    },
    /// `SUBMIT_PLAYER` (tag 20).
    SubmitPlayer {
        /// The chosen player.
        player: PlayerId,
    },
    /// `SUBMIT_CHOICE_VALUE` (tag 21).
    SubmitChoiceValue {
        /// The chosen value.
        value: ChoiceValue,
    },
    /// `SUBMIT_CARD_NAME` (tag 22).
    SubmitCardName {
        /// The named card name.
        name: String,
    },
    /// `SUBMIT_SET` (tag 23).
    SubmitSet {
        /// The chosen permanents (distinct).
        objects: Vec<ObjectId>,
    },
    /// `ACCEPT` (tag 24).
    Accept,
    /// `SHORTEN` (tag 25).
    Shorten {
        /// The index in the proposed sequence where the shortcut stops.
        at_index: u64,
    },
    /// `KEEP_HAND` (tag 26).
    KeepHand {
        /// The cards put on the bottom (distinct).
        bottom: Vec<ObjectId>,
    },
    /// `TAKE_MULLIGAN` (tag 27).
    TakeMulligan,
    /// `SUBMIT_PREGAME` (tag 28).
    SubmitPregame {
        /// The eligible cards whose pregame action is taken (distinct).
        objects: Vec<ObjectId>,
    },
}

impl ChoiceResponse {
    /// The one-byte canonical tag of this variant (see the variant docs).
    #[must_use]
    pub const fn tag(&self) -> u8 {
        match self {
            ChoiceResponse::PassPriority => 0,
            ChoiceResponse::PlayLand { .. } => 1,
            ChoiceResponse::InitiateCast { .. } => 2,
            ChoiceResponse::Activate { .. } => 3,
            ChoiceResponse::SpecialAction { .. } => 4,
            ChoiceResponse::SelectModes { .. } => 5,
            ChoiceResponse::ConfigureCosts { .. } => 6,
            ChoiceResponse::SelectTargets { .. } => 7,
            ChoiceResponse::SubmitDivision { .. } => 8,
            ChoiceResponse::PayCosts { .. } => 9,
            ChoiceResponse::DeclineOptionalPayment => 10,
            ChoiceResponse::CancelAndRollback => 11,
            ChoiceResponse::SubmitAttackers { .. } => 12,
            ChoiceResponse::SubmitBlockers { .. } => 13,
            ChoiceResponse::OrderTriggers { .. } => 14,
            ChoiceResponse::Yes => 15,
            ChoiceResponse::No => 16,
            ChoiceResponse::SelectReplacement { .. } => 17,
            ChoiceResponse::SubmitObjectSelection { .. } => 18,
            ChoiceResponse::SubmitOrdering { .. } => 19,
            ChoiceResponse::SubmitPlayer { .. } => 20,
            ChoiceResponse::SubmitChoiceValue { .. } => 21,
            ChoiceResponse::SubmitCardName { .. } => 22,
            ChoiceResponse::SubmitSet { .. } => 23,
            ChoiceResponse::Accept => 24,
            ChoiceResponse::Shorten { .. } => 25,
            ChoiceResponse::KeepHand { .. } => 26,
            ChoiceResponse::TakeMulligan => 27,
            ChoiceResponse::SubmitPregame { .. } => 28,
        }
    }

    /// The SRS 15.3 response name (`PASS_PRIORITY`, `SELECT_MODES`, ...).
    #[must_use]
    pub const fn wire_name(&self) -> &'static str {
        match self {
            ChoiceResponse::PassPriority => "PASS_PRIORITY",
            ChoiceResponse::PlayLand { .. } => "PLAY_LAND",
            ChoiceResponse::InitiateCast { .. } => "INITIATE_CAST",
            ChoiceResponse::Activate { .. } => "ACTIVATE",
            ChoiceResponse::SpecialAction { .. } => "SPECIAL_ACTION",
            ChoiceResponse::SelectModes { .. } => "SELECT_MODES",
            ChoiceResponse::ConfigureCosts { .. } => "CONFIGURE_COSTS",
            ChoiceResponse::SelectTargets { .. } => "SELECT_TARGETS",
            ChoiceResponse::SubmitDivision { .. } => "SUBMIT_DIVISION",
            ChoiceResponse::PayCosts { .. } => "PAY_COSTS",
            ChoiceResponse::DeclineOptionalPayment => "DECLINE_OPTIONAL_PAYMENT",
            ChoiceResponse::CancelAndRollback => "CANCEL_AND_ROLLBACK",
            ChoiceResponse::SubmitAttackers { .. } => "SUBMIT_ATTACKERS",
            ChoiceResponse::SubmitBlockers { .. } => "SUBMIT_BLOCKERS",
            ChoiceResponse::OrderTriggers { .. } => "ORDER_TRIGGERS",
            ChoiceResponse::Yes => "YES",
            ChoiceResponse::No => "NO",
            ChoiceResponse::SelectReplacement { .. } => "SELECT_REPLACEMENT",
            ChoiceResponse::SubmitObjectSelection { .. } => "SUBMIT_OBJECT_SELECTION",
            ChoiceResponse::SubmitOrdering { .. } => "SUBMIT_ORDERING",
            ChoiceResponse::SubmitPlayer { .. } => "SUBMIT_PLAYER",
            ChoiceResponse::SubmitChoiceValue { .. } => "SUBMIT_CHOICE_VALUE",
            ChoiceResponse::SubmitCardName { .. } => "SUBMIT_CARD_NAME",
            ChoiceResponse::SubmitSet { .. } => "SUBMIT_SET",
            ChoiceResponse::Accept => "ACCEPT",
            ChoiceResponse::Shorten { .. } => "SHORTEN",
            ChoiceResponse::KeepHand { .. } => "KEEP_HAND",
            ChoiceResponse::TakeMulligan => "TAKE_MULLIGAN",
            ChoiceResponse::SubmitPregame { .. } => "SUBMIT_PREGAME",
        }
    }

    /// The choice type this response answers (its row in the SRS 15.3 table).
    #[must_use]
    pub const fn choice_type(&self) -> ChoiceType {
        match self {
            ChoiceResponse::PassPriority
            | ChoiceResponse::PlayLand { .. }
            | ChoiceResponse::InitiateCast { .. }
            | ChoiceResponse::Activate { .. }
            | ChoiceResponse::SpecialAction { .. } => ChoiceType::PriorityAction,
            ChoiceResponse::SelectModes { .. } => ChoiceType::ChooseModes,
            ChoiceResponse::ConfigureCosts { .. } => ChoiceType::ChooseCostsAndX,
            ChoiceResponse::SelectTargets { .. } => ChoiceType::ChooseTargets,
            ChoiceResponse::SubmitDivision { .. } => ChoiceType::DivideValue,
            ChoiceResponse::PayCosts { .. }
            | ChoiceResponse::DeclineOptionalPayment
            | ChoiceResponse::CancelAndRollback => ChoiceType::PayManaAndCosts,
            ChoiceResponse::SubmitAttackers { .. } => ChoiceType::DeclareAttackers,
            ChoiceResponse::SubmitBlockers { .. } => ChoiceType::DeclareBlockers,
            ChoiceResponse::OrderTriggers { .. } => ChoiceType::OrderSimultaneousTriggers,
            ChoiceResponse::Yes | ChoiceResponse::No => ChoiceType::ChooseOptionalTrigger,
            ChoiceResponse::SelectReplacement { .. } => ChoiceType::ChooseReplacementEffect,
            ChoiceResponse::SubmitObjectSelection { .. } => ChoiceType::SelectObjectsFromZone,
            ChoiceResponse::SubmitOrdering { .. } => ChoiceType::OrderCards,
            ChoiceResponse::SubmitPlayer { .. } => ChoiceType::ChoosePlayer,
            ChoiceResponse::SubmitChoiceValue { .. } => ChoiceType::ChooseOptionOrValue,
            ChoiceResponse::SubmitCardName { .. } => ChoiceType::ChooseCardName,
            ChoiceResponse::SubmitSet { .. } => ChoiceType::ChooseUntapSet,
            ChoiceResponse::Accept | ChoiceResponse::Shorten { .. } => ChoiceType::ShortcutResponse,
            ChoiceResponse::KeepHand { .. } | ChoiceResponse::TakeMulligan => {
                ChoiceType::MulliganDecision
            }
            ChoiceResponse::SubmitPregame { .. } => ChoiceType::PregameAction,
        }
    }
}

impl CanonicalEncode for ChoiceResponse {
    /// [`ChoiceResponse::tag`], then the fields in declaration order.
    fn encode_canonical(&self, out: &mut Vec<u8>) {
        put_variant(out, self.tag());
        match self {
            ChoiceResponse::PassPriority
            | ChoiceResponse::DeclineOptionalPayment
            | ChoiceResponse::CancelAndRollback
            | ChoiceResponse::Yes
            | ChoiceResponse::No
            | ChoiceResponse::Accept
            | ChoiceResponse::TakeMulligan => {}
            ChoiceResponse::PlayLand { object, face_index } => {
                object.encode_canonical(out);
                face_index.encode_canonical(out);
            }
            ChoiceResponse::InitiateCast {
                object,
                face_index,
                cast_mode,
            } => {
                object.encode_canonical(out);
                face_index.encode_canonical(out);
                cast_mode.encode_canonical(out);
            }
            ChoiceResponse::Activate { object, ability_id } => {
                object.encode_canonical(out);
                ability_id.encode_canonical(out);
            }
            ChoiceResponse::SpecialAction { kind, object } => {
                kind.encode_canonical(out);
                object.encode_canonical(out);
            }
            ChoiceResponse::SelectModes { modes } => modes.encode_canonical(out),
            ChoiceResponse::ConfigureCosts {
                alternative,
                additional,
                x,
                symbol_options,
                harmonize,
            } => {
                alternative.encode_canonical(out);
                additional.encode_canonical(out);
                x.encode_canonical(out);
                symbol_options.encode_canonical(out);
                harmonize.encode_canonical(out);
            }
            ChoiceResponse::SelectTargets { targets } => targets.encode_canonical(out),
            ChoiceResponse::SubmitDivision { assignments } => assignments.encode_canonical(out),
            ChoiceResponse::PayCosts {
                auto_pay,
                methods,
                mana_abilities,
            } => {
                auto_pay.encode_canonical(out);
                methods.encode_canonical(out);
                mana_abilities.encode_canonical(out);
            }
            ChoiceResponse::SubmitAttackers { attacks } => attacks.encode_canonical(out),
            ChoiceResponse::SubmitBlockers { blocks } => blocks.encode_canonical(out),
            ChoiceResponse::OrderTriggers { order } => order.encode_canonical(out),
            ChoiceResponse::SelectReplacement { index } => index.encode_canonical(out),
            ChoiceResponse::SubmitObjectSelection { objects }
            | ChoiceResponse::SubmitSet { objects }
            | ChoiceResponse::SubmitPregame { objects } => objects.encode_canonical(out),
            ChoiceResponse::SubmitOrdering {
                top,
                bottom,
                graveyard,
            } => {
                top.encode_canonical(out);
                bottom.encode_canonical(out);
                graveyard.encode_canonical(out);
            }
            ChoiceResponse::SubmitPlayer { player } => player.encode_canonical(out),
            ChoiceResponse::SubmitChoiceValue { value } => value.encode_canonical(out),
            ChoiceResponse::SubmitCardName { name } => name.encode_canonical(out),
            ChoiceResponse::Shorten { at_index } => at_index.encode_canonical(out),
            ChoiceResponse::KeepHand { bottom } => bottom.encode_canonical(out),
        }
    }
}
