// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! References, player selectors, object selectors and impossible-instruction partitioning
//! (REQ-DSL-003, REQ-ARCH-002).

use std::cmp::Ordering as CmpOrdering;
use std::collections::BTreeSet;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_dsl::expr::{
    Direction, ObjectSelectorAST, OrderKey, PlayerGroup, PlayerSelectorAST, ReferenceAST,
    SelectorFilter, Zone,
};

use super::expr::{eval_predicate_object, eval_value};
use super::types::{
    ChosenValue, EvalContext, Ignored, InterpEnv, InterpError, ObjectView, Partition, Referent,
    Resolution,
};
use crate::memory::ChoiceValue;

/// The information the interpreter uses for an object: current information while it is in a
/// zone, otherwise its last known information (CR 608.2h, CR 113.7a, CR 608.2b).
pub(super) fn view_of(env: &dyn InterpEnv, id: ObjectId) -> Option<ObjectView> {
    env.object(id).or_else(|| env.lki(id))
}

/// Every player in APNAP order (CR 101.4): seat order rotated so the active player is first;
/// no rotation when there is no active player (or it is not seated).
pub(super) fn apnap_players(env: &dyn InterpEnv) -> Vec<PlayerId> {
    let mut players = env.players();
    if let Some(active) = env.active_player()
        && let Some(pos) = players.iter().position(|p| *p == active)
    {
        players.rotate_left(pos);
    }
    players
}

/// Adds a player referent if the player is in the game, else records it as gone (CR 101.3).
fn push_player(env: &dyn InterpEnv, out: &mut Resolution<Referent>, p: PlayerId) {
    if env.players().contains(&p) {
        out.items.push(Referent::Player(p));
    } else {
        out.ignored.push(Ignored::Gone(Referent::Player(p)));
    }
}

/// Adds an object referent if the object is in a zone now, else records it as gone (CR 101.3).
fn push_object(env: &dyn InterpEnv, out: &mut Resolution<Referent>, id: ObjectId) {
    if env.object(id).is_some() {
        out.items.push(Referent::Object(id));
    } else {
        out.ignored.push(Ignored::Gone(Referent::Object(id)));
    }
}

/// Adds any referent, checking it is still there (CR 101.3).
fn push_referent(env: &dyn InterpEnv, out: &mut Resolution<Referent>, r: Referent) {
    match r {
        Referent::Object(id) => push_object(env, out, id),
        Referent::Player(p) => push_player(env, out, p),
    }
}

/// Resolves a reference relative to the ability (REQ-DSL-003, SRS §14.4.0).
///
/// An impossible reference (nothing to name, or a named player or object that is gone) is
/// reported in [`Resolution::ignored`] and never an error (REQ-ARCH-002, CR 101.3). `SELF` and
/// `SOURCE_LKI` use last known information once the source has left (CR 608.2h, CR 113.7a,
/// CR 608.2b). `EACH_OPPONENT` is in APNAP order (CR 101.4).
///
/// # Errors
///
/// [`InterpError::UnboundVariable`] for `BOUND($var)` with no binding of `$var` (a malformed
/// ability, not an impossible instruction).
pub fn resolve_reference(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    r: &ReferenceAST,
) -> Result<Resolution<Referent>, InterpError> {
    let mut out = Resolution::default();
    let unresolved =
        |out: &mut Resolution<Referent>| out.ignored.push(Ignored::Unresolved(r.clone()));
    match r {
        // CR 608.2h, CR 113.7a: the source's last known information stands in once it left.
        ReferenceAST::SelfRef => match ctx.source {
            Some(id) if view_of(env, id).is_some() => out.items.push(Referent::Object(id)),
            _ => unresolved(&mut out),
        },
        // CR 608.2h, CR 608.2b: last known information first, current information otherwise.
        ReferenceAST::SourceLki => match ctx.source {
            Some(id) if env.lki(id).is_some() || env.object(id).is_some() => {
                out.items.push(Referent::Object(id));
            }
            _ => unresolved(&mut out),
        },
        ReferenceAST::Controller => push_player(env, &mut out, ctx.controller),
        ReferenceAST::Owner => match ctx.source.and_then(|id| view_of(env, id)) {
            Some(view) => push_player(env, &mut out, view.owner),
            None => unresolved(&mut out),
        },
        ReferenceAST::ActivePlayer => match env.active_player() {
            Some(p) => push_player(env, &mut out, p),
            None => unresolved(&mut out),
        },
        ReferenceAST::DefendingPlayer => match ctx.source.and_then(|id| env.defending_player(id)) {
            Some(p) => push_player(env, &mut out, p),
            None => unresolved(&mut out),
        },
        ReferenceAST::Target { slot } => {
            let chosen = usize::try_from(*slot)
                .ok()
                .and_then(|i| ctx.targets.get(i))
                .copied()
                .flatten();
            match chosen {
                Some(referent) => push_referent(env, &mut out, referent),
                None => unresolved(&mut out),
            }
        }
        ReferenceAST::Chosen { key } => match ctx.chosen.get(key) {
            Some(ChosenValue::Object(id)) => push_object(env, &mut out, *id),
            Some(ChosenValue::Player(p)) => push_player(env, &mut out, *p),
            Some(ChosenValue::Number(_)) | None => unresolved(&mut out),
        },
        ReferenceAST::Bound { var } => match ctx.bindings.get(var) {
            Some(list) => {
                for referent in list {
                    push_referent(env, &mut out, *referent);
                }
            }
            None => return Err(InterpError::UnboundVariable(var.clone())),
        },
        ReferenceAST::Linked { link_id } => match ctx.linked.get(link_id) {
            Some(list) if !list.is_empty() => {
                for referent in list {
                    push_referent(env, &mut out, *referent);
                }
            }
            _ => unresolved(&mut out),
        },
        ReferenceAST::Enchanted | ReferenceAST::Equipped | ReferenceAST::AttachedTo => {
            match ctx.source.and_then(|id| env.attached_to(id)) {
                Some(host) => push_object(env, &mut out, host),
                None => unresolved(&mut out),
            }
        }
        ReferenceAST::Remembered { key } => {
            let remembered = ctx
                .source
                .and_then(|id| env.memory(id))
                .and_then(|m| m.choice(key));
            match remembered {
                Some(ChoiceValue::Player(p)) => push_player(env, &mut out, *p),
                _ => unresolved(&mut out),
            }
        }
        ReferenceAST::ThatPlayer => match ctx.that_player {
            Some(p) => push_player(env, &mut out, p),
            None => unresolved(&mut out),
        },
        // CR 101.4: APNAP order.
        ReferenceAST::EachOpponent => {
            out.items = apnap_players(env)
                .into_iter()
                .filter(|p| *p != ctx.controller)
                .map(Referent::Player)
                .collect();
        }
    }
    Ok(out)
}

/// Resolves a player selector (REQ-DSL-003): a reference keeps its player referents (an object
/// referent is ignored as unresolved, CR 101.3); `ALL_PLAYERS` is every player in APNAP order
/// (CR 101.4); `TEAMMATES` is empty (no team formats yet).
///
/// # Errors
///
/// As [`resolve_reference`].
pub fn resolve_players(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    s: &PlayerSelectorAST,
) -> Result<Resolution<PlayerId>, InterpError> {
    match s {
        PlayerSelectorAST::Ref(r) => {
            let res = resolve_reference(env, ctx, r)?;
            let mut out = Resolution {
                items: Vec::new(),
                ignored: res.ignored,
            };
            for item in res.items {
                match item {
                    Referent::Player(p) => out.items.push(p),
                    Referent::Object(_) => out.ignored.push(Ignored::Unresolved(r.clone())),
                }
            }
            Ok(out)
        }
        PlayerSelectorAST::Players(PlayerGroup::AllPlayers) => Ok(Resolution {
            items: apnap_players(env),
            ignored: Vec::new(),
        }),
        PlayerSelectorAST::Players(PlayerGroup::Teammates) => Ok(Resolution::default()),
    }
}

/// Selects objects (REQ-DSL-003, SRS §14.4.0).
///
/// Reference form: the reference's object referents (a player referent is ignored as
/// unresolved, CR 101.3). Filter form: the objects in the listed zones (default: the
/// battlefield) that pass the predicate, sorted by the ordering key (ties, and the unordered
/// case, by ascending `ObjectId`), then cut to `count` (0 or less: none). The result never
/// depends on the order [`InterpEnv::objects_in`] returns.
///
/// # Errors
///
/// Any error from the predicate, the count expression or the reference.
pub fn select_objects(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    s: &ObjectSelectorAST,
) -> Result<Resolution<ObjectId>, InterpError> {
    match s {
        ObjectSelectorAST::Ref(r) => {
            let res = resolve_reference(env, ctx, r)?;
            let mut out = Resolution {
                items: Vec::new(),
                ignored: res.ignored,
            };
            for item in res.items {
                match item {
                    Referent::Object(id) => out.items.push(id),
                    Referent::Player(_) => out.ignored.push(Ignored::Unresolved(r.clone())),
                }
            }
            Ok(out)
        }
        ObjectSelectorAST::Filter(filter) => select_filtered(env, ctx, filter),
    }
}

/// The filter form of [`select_objects`].
fn select_filtered(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    filter: &SelectorFilter,
) -> Result<Resolution<ObjectId>, InterpError> {
    let default_zones = [Zone::Battlefield];
    let zones: &[Zone] = filter.zones.as_deref().unwrap_or(&default_zones);
    // De-duplicated candidates; the BTreeSet order is irrelevant because of the final sort.
    let mut ids = BTreeSet::new();
    for zone in zones {
        ids.extend(env.objects_in(*zone));
    }
    let mut views = Vec::new();
    for id in ids {
        let Some(view) = env.object(id) else {
            continue;
        };
        let keep = match &filter.predicate {
            Some(p) => eval_predicate_object(env, ctx, p, &view)?,
            None => true,
        };
        if keep {
            views.push(view);
        }
    }
    views.sort_by(|a, b| {
        let key = match filter.ordering {
            Some(ordering) => {
                let k = order_key(a, b, ordering.by);
                match ordering.direction {
                    Direction::Asc => k,
                    Direction::Desc => k.reverse(),
                }
            }
            None => CmpOrdering::Equal,
        };
        key.then(a.id.cmp(&b.id))
    });
    if let Some(count) = &filter.count {
        let n = eval_value(env, ctx, count)?;
        let keep = usize::try_from(n).unwrap_or(0);
        views.truncate(keep);
    }
    Ok(Resolution {
        items: views.into_iter().map(|v| v.id).collect(),
        ignored: Vec::new(),
    })
}

/// Compares two views by one ordering key (`None` power/toughness sorts before any number).
fn order_key(a: &ObjectView, b: &ObjectView, by: OrderKey) -> CmpOrdering {
    match by {
        OrderKey::Timestamp => a.timestamp.cmp(&b.timestamp),
        OrderKey::ManaValue => a.mana_value.cmp(&b.mana_value),
        OrderKey::Power => a.power.cmp(&b.power),
        OrderKey::Toughness => a.toughness.cmp(&b.toughness),
        OrderKey::ZonePosition => a.zone_position.cmp(&b.zone_position),
    }
}

/// Splits the parts of an instruction into those that can be performed and those that are
/// ignored, each in input order (REQ-ARCH-002, CR 101.3: "Any part of an instruction that is
/// impossible to perform is ignored. The rest of the instruction is still carried out.").
/// Calls `is_possible` exactly once per part, in order.
pub fn partition_possible<T>(
    parts: impl IntoIterator<Item = T>,
    mut is_possible: impl FnMut(&T) -> bool,
) -> Partition<T> {
    let mut out = Partition {
        performed: Vec::new(),
        ignored: Vec::new(),
    };
    for part in parts {
        if is_possible(&part) {
            out.performed.push(part);
        } else {
            out.ignored.push(part);
        }
    }
    out
}
