// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Simultaneous graveyard entry: the owner arranges the cards (REQ-ZONE-001, REQ-EVENT-001,
//! CR 404.3).
//!
//! A batch is final when it reaches the executor (replacements were applied before it, CR 614.6),
//! so the cards one batch puts into one graveyard are known from the planning run. Every owner
//! with two or more such cards gets one `ORDER_CARDS` request
//! ([`GraveyardOrderGroup::request`]); [`EventContext::execute_batch_ordered`](super::EventContext::execute_batch_ordered)
//! then executes the batch with the chosen orders. See docs/DECISIONS.md, "#208 FP-154 — Owner
//! orders simultaneous graveyard entry (CR 404.3)".

use std::collections::{BTreeMap, BTreeSet};

use fineprint_base::ids::{IdGenerator, ObjectId, PlayerId};

use super::error::EventError;
use super::state::ObjectMarkers;
use super::types::{ExecutedEvent, GameEvent, Outcome};
use crate::choice::{
    ChoiceOption, ChoiceOptions, ChoicePayload, ChoiceReason, ChoiceResponse, ChoiceType,
    ENUMERATION_LIMIT, OrderDestination, PendingChoiceRequest, RequestId, ValidatorContract,
    ValidatorRule,
};
use crate::zones::{MoveRequest, Position, ZoneId, ZoneKind, Zones};

/// One owner's simultaneous graveyard entry (REQ-ZONE-001, CR 404.3): two or more nontoken
/// cards one batch puts on top of that owner's graveyard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraveyardOrderGroup {
    /// The owner of the cards and of the graveyard (CR 400.3), who arranges them.
    pub owner: PlayerId,
    /// Pre-move ObjectIds of the cards, ascending.
    pub cards: Vec<ObjectId>,
    /// Resulting top-to-bottom order of these cards if nobody chooses: exactly what
    /// [`execute_batch`](super::EventContext::execute_batch) produces (moves put on top in
    /// submission order, so the last-submitted card ends on top). This is the "submission order
    /// of arrival" and the timeout default.
    pub default_order: Vec<ObjectId>,
}

/// The order an owner chose for a [`GraveyardOrderGroup`] (REQ-ZONE-001, CR 404.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraveyardOrder {
    /// The owner of the group.
    pub owner: PlayerId,
    /// The group's pre-move ObjectIds; index 0 = topmost of these cards in the owner's
    /// graveyard after the batch.
    pub top_to_bottom: Vec<ObjectId>,
}

impl GraveyardOrderGroup {
    /// The `ORDER_CARDS` request for this group (SRS 15.3, CR 404.3): chooser the owner, the
    /// cards ascending, the one destination `Graveyard` (answered with
    /// `SUBMIT_ORDERING.graveyard`, top first), public (a graveyard is public, CR 404.2), and
    /// the options in the shape [`generate_request`](crate::choice::generate_request) gives an
    /// `ORDER` choice: the cards as enumerated options while the number of orders `n!` is below
    /// [`ENUMERATION_LIMIT`], a [`ValidatorRule::Permutation`] contract otherwise. The timeout
    /// default is [`GraveyardOrderGroup::default_order`]. A pure function of the group and
    /// `request_id`, so replay compares equal digests.
    #[must_use]
    pub fn request(&self, request_id: RequestId) -> PendingChoiceRequest {
        let n = u64::try_from(self.cards.len()).unwrap_or(u64::MAX);
        let options = if factorial_capped(n) < ENUMERATION_LIMIT {
            ChoiceOptions::Enumerated(
                self.cards
                    .iter()
                    .map(|&o| ChoiceOption::Object(o))
                    .collect(),
            )
        } else {
            ChoiceOptions::Validator(ValidatorContract {
                min: n,
                max: n,
                rule: ValidatorRule::Permutation {
                    items: self.cards.clone(),
                },
            })
        };
        PendingChoiceRequest {
            request_id,
            chooser: self.owner,
            choice_type: ChoiceType::OrderCards,
            reason: ChoiceReason {
                cr: "CR 404.3".to_owned(),
                source: None,
            },
            min: n,
            max: n,
            hidden: false,
            reveal_to: Vec::new(),
            default_on_timeout: ChoiceResponse::SubmitOrdering {
                top: Vec::new(),
                bottom: Vec::new(),
                graveyard: self.default_order.clone(),
            },
            options,
            payload: ChoicePayload::OrderCards {
                cards: self.cards.clone(),
                destinations: vec![OrderDestination::Graveyard],
            },
        }
    }

    /// Maps an answer to [`GraveyardOrderGroup::request`] to the chosen order (CR 404.3): the
    /// `graveyard` list, top first.
    ///
    /// # Errors
    /// [`EventError::InvalidGraveyardOrder`] naming the owner unless `response` is a
    /// `SubmitOrdering` with empty `top` and `bottom` lists and a `graveyard` list that is a
    /// permutation of [`GraveyardOrderGroup::cards`].
    pub fn order_from_response(
        &self,
        response: &ChoiceResponse,
    ) -> Result<GraveyardOrder, EventError> {
        let invalid = EventError::InvalidGraveyardOrder { owner: self.owner };
        match response {
            ChoiceResponse::SubmitOrdering {
                top,
                bottom,
                graveyard,
            } if top.is_empty() && bottom.is_empty() && is_permutation(graveyard, &self.cards) => {
                Ok(GraveyardOrder {
                    owner: self.owner,
                    top_to_bottom: graveyard.clone(),
                })
            }
            _ => Err(invalid),
        }
    }
}

/// `min(n!, ENUMERATION_LIMIT)`, never overflowing (the count `generate_request` uses for an
/// `ORDER` choice).
fn factorial_capped(n: u64) -> u64 {
    let mut f: u64 = 1;
    let mut i: u64 = 2;
    while i <= n {
        f = f.saturating_mul(i);
        if f >= ENUMERATION_LIMIT {
            return ENUMERATION_LIMIT;
        }
        i += 1;
    }
    f
}

/// Whether `list` holds exactly the (distinct) ids of `cards`, each once.
fn is_permutation(list: &[ObjectId], cards: &[ObjectId]) -> bool {
    let mut a = list.to_vec();
    let mut b = cards.to_vec();
    a.sort_unstable();
    b.sort_unstable();
    a == b
}

/// CR 404.3, CR 400.3, CR 111.6, CR 108.2b: the groups of a batch, from the executed events of
/// a run and the state right after that run (before it is restored). Counted: executed
/// `ZoneChange` events (not draws) requested at `Position::Top`, that changed zones
/// (`old != new`: a graveyard-to-graveyard reposition is not "put into"), into a graveyard,
/// of a nontoken card that is still in that graveyard at the end of the batch and that no later
/// event of the batch moved again. One group per owner (the graveyard's player) with at least
/// two such cards, ascending by owner.
pub(super) fn groups_of(
    zones: &Zones,
    markers: &ObjectMarkers,
    executed: &[ExecutedEvent],
) -> Vec<GraveyardOrderGroup> {
    // Ids a later event of the batch moved or repositioned (its `old`).
    let moved_from: BTreeSet<ObjectId> = executed
        .iter()
        .filter_map(|e| match e.outcome {
            Outcome::ZoneChange(m) => Some(m.old),
            _ => None,
        })
        .collect();
    // owner -> (new id -> old id)
    let mut entries: BTreeMap<PlayerId, BTreeMap<ObjectId, ObjectId>> = BTreeMap::new();
    for e in executed {
        let (GameEvent::ZoneChange { request, .. }, Outcome::ZoneChange(m)) =
            (e.instance.event(), e.outcome)
        else {
            continue;
        };
        let ZoneId::Graveyard(owner) = m.to else {
            continue;
        };
        if request.position != Position::Top || m.old == m.new || moved_from.contains(&m.new) {
            continue;
        }
        let Some(record) = zones.object(m.new) else {
            continue;
        };
        // CR 111.7, CR 111.6, CR 108.2b: a token is not a card; it takes no part.
        if record.zone != m.to || markers.is_token(record.card) {
            continue;
        }
        entries.entry(owner).or_default().insert(m.new, m.old);
    }
    entries
        .into_iter()
        .filter(|(_, map)| map.len() >= 2)
        .map(|(owner, map)| {
            let mut cards: Vec<ObjectId> = map.values().copied().collect();
            cards.sort_unstable();
            let default_order = zones
                .contents(ZoneId::Graveyard(owner))
                .into_iter()
                .filter_map(|id| map.get(&id).copied())
                .collect();
            GraveyardOrderGroup {
                owner,
                cards,
                default_order,
            }
        })
        .collect()
}

/// Checks `orders` against `groups` (REQ-EVENT-001): each order names an owner with a group,
/// no owner twice, and lists a permutation of that group's cards. Returns the orders sorted by
/// owner. The entries are checked in ascending owner order, so the reported owner does not
/// depend on the order of `orders`.
///
/// # Errors
/// [`EventError::InvalidGraveyardOrder`] naming the lowest offending owner.
pub(super) fn check_orders(
    groups: &[GraveyardOrderGroup],
    orders: &[GraveyardOrder],
) -> Result<Vec<GraveyardOrder>, EventError> {
    let mut sorted = orders.to_vec();
    sorted.sort_by_key(|o| o.owner);
    let mut seen = BTreeSet::new();
    for o in &sorted {
        let invalid = EventError::InvalidGraveyardOrder { owner: o.owner };
        if !seen.insert(o.owner) {
            return Err(invalid);
        }
        let group = groups
            .iter()
            .find(|g| g.owner == o.owner)
            .ok_or_else(|| invalid.clone())?;
        if !is_permutation(&o.top_to_bottom, &group.cards) {
            return Err(invalid);
        }
    }
    Ok(sorted)
}

/// CR 404.3, CR 404.1: puts the cards of `order` (named by their pre-move ids; `renamed` maps
/// pre-move to post-move ids from the real run) in `order.top_to_bottom` order into the
/// positions ("slots") the group's cards occupy in the batch's submission-order result: the
/// topmost slot gets `top_to_bottom[0]`, and so on. Every other object of the graveyard keeps
/// exactly its submission-order position (cards already there stay below, a `Bottom` or
/// `Index` move keeps its requested place, a token keeps its place until it ceases to exist,
/// CR 111.7). So the group is contiguous unless another object of the batch landed between its
/// cards, and the default order changes nothing. Only same-zone repositions are used (CR 400.7:
/// no zone change, no new ids, no events).
///
/// # Errors
/// [`EventError::UnknownObject`] if a card is not in the owner's graveyard (unreachable after a
/// successful check), or a zone error.
pub(super) fn arrange(
    zones: &mut Zones,
    ids: &mut IdGenerator<ObjectId>,
    order: &GraveyardOrder,
    renamed: &BTreeMap<ObjectId, ObjectId>,
) -> Result<(), EventError> {
    let zone = ZoneId::Graveyard(order.owner);
    let block: Vec<ObjectId> = order
        .top_to_bottom
        .iter()
        .map(|old| {
            renamed
                .get(old)
                .copied()
                .ok_or(EventError::UnknownObject(*old))
        })
        .collect::<Result<_, _>>()?;
    let members: BTreeSet<ObjectId> = block.iter().copied().collect();
    let current = zones.contents(zone);
    if let Some(&missing) = block.iter().find(|id| !current.contains(id)) {
        return Err(EventError::UnknownObject(missing));
    }
    // The group's slots, top first.
    let slots: Vec<usize> = current
        .iter()
        .enumerate()
        .filter(|(_, id)| members.contains(id))
        .map(|(i, _)| i)
        .collect();
    if slots.iter().zip(&block).all(|(&i, id)| current[i] == *id) {
        return Ok(());
    }
    // First every card of the group to the bottom, then each to its slot, top first: when
    // card `i` goes to `slots[i]`, everything above that slot is already final (CR 400.5: the
    // other objects keep their relative order).
    for &id in &block {
        reposition(zones, ids, id, Position::Bottom)?;
    }
    for (&slot, &id) in slots.iter().zip(&block) {
        reposition(zones, ids, id, Position::Index(slot))?;
    }
    Ok(())
}

/// CR 400.7, CR 400.5: a same-zone reposition in a graveyard (same object, same face).
fn reposition(
    zones: &mut Zones,
    ids: &mut IdGenerator<ObjectId>,
    id: ObjectId,
    position: Position,
) -> Result<(), EventError> {
    let face_down = zones
        .object(id)
        .ok_or(EventError::UnknownObject(id))?
        .face_down;
    zones.move_object(
        ids,
        id,
        MoveRequest {
            to: ZoneKind::Graveyard,
            position,
            face_down,
            controller: None,
        },
    )?;
    Ok(())
}
