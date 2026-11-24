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

/// One referent of a reference, in input order: either resolved or ignored (CR 101.3). The
/// selectors read this ordered form so their `ignored` lists follow the referents' order.
enum Slot {
    /// A referent that is still there.
    Item(Referent),
    /// An impossible part of the reference.
    Ignored(Ignored),
}

/// Adds a player referent if the player is in the game, else records it as gone (CR 101.3).
fn push_player(env: &dyn InterpEnv, out: &mut Vec<Slot>, p: PlayerId) {
    if env.players().contains(&p) {
        out.push(Slot::Item(Referent::Player(p)));
    } else {
        out.push(Slot::Ignored(Ignored::Gone(Referent::Player(p))));
    }
}

/// Adds an object referent if the object is in a zone now, else records it as gone (CR 101.3).
fn push_object(env: &dyn InterpEnv, out: &mut Vec<Slot>, id: ObjectId) {
    if env.object(id).is_some() {
        out.push(Slot::Item(Referent::Object(id)));
    } else {
        out.push(Slot::Ignored(Ignored::Gone(Referent::Object(id))));
    }
}

/// Adds any referent, checking it is still there (CR 101.3).
fn push_referent(env: &dyn InterpEnv, out: &mut Vec<Slot>, r: Referent) {
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
/// CR 608.2b). `EACH_OPPONENT` is in APNAP order (CR 101.4). `ENCHANTED`, `EQUIPPED` and
/// `ATTACHED_TO` are the same lookup, [`InterpEnv::attached_to`] of the source: the host may be
/// an object or a player, and a host that is gone is ignored as [`Ignored::Gone`].
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
    for slot in resolve_slots(env, ctx, r)? {
        match slot {
            Slot::Item(referent) => out.items.push(referent),
            Slot::Ignored(ignored) => out.ignored.push(ignored),
        }
    }
    Ok(out)
}

/// The referents of a reference in input order, each resolved or ignored (REQ-DSL-003,
/// REQ-ARCH-002, CR 101.3): the ordered form behind [`resolve_reference`],
/// [`resolve_players`] and [`select_objects`].
fn resolve_slots(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    r: &ReferenceAST,
) -> Result<Vec<Slot>, InterpError> {
    let mut out = Vec::new();
    let unresolved = |out: &mut Vec<Slot>| out.push(Slot::Ignored(Ignored::Unresolved(r.clone())));
    match r {
        // CR 608.2h, CR 113.7a: the source's last known information stands in once it left.
        ReferenceAST::SelfRef => match ctx.source {
            Some(id) if view_of(env, id).is_some() => out.push(Slot::Item(Referent::Object(id))),
            _ => unresolved(&mut out),
        },
        // CR 608.2h, CR 608.2b: last known information first, current information otherwise.
        ReferenceAST::SourceLki => match ctx.source {
            Some(id) if env.lki(id).is_some() || env.object(id).is_some() => {
                out.push(Slot::Item(Referent::Object(id)));
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
        // CR 508.5: the player the source is attacking (or the controller of the planeswalker /
        // protector of the battle it is attacking); once the source is no longer attacking
        // (removed from combat, CR 506.4), the one it was attacking just before. The
        // environment retains that player; the interpreter does not check that the source is
        // still attacking. A source that never attacked gives `None` (modelling choice).
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
        // The three are the same lookup; the host may be an object or a player.
        ReferenceAST::Enchanted | ReferenceAST::Equipped | ReferenceAST::AttachedTo => {
            match ctx.source.and_then(|id| env.attached_to(id)) {
                Some(host) => push_referent(env, &mut out, host),
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
            out.extend(
                apnap_players(env)
                    .into_iter()
                    .filter(|p| *p != ctx.controller)
                    .map(|p| Slot::Item(Referent::Player(p))),
            );
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
            // CR 101.3: every ignored part is reported in the referents' input order.
            let mut out = Resolution::default();
            for slot in resolve_slots(env, ctx, r)? {
                match slot {
                    Slot::Item(Referent::Player(p)) => out.items.push(p),
                    Slot::Item(Referent::Object(_)) => {
                        out.ignored.push(Ignored::Unresolved(r.clone()));
                    }
                    Slot::Ignored(ignored) => out.ignored.push(ignored),
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
            // CR 101.3: every ignored part is reported in the referents' input order.
            let mut out = Resolution::default();
            for slot in resolve_slots(env, ctx, r)? {
                match slot {
                    Slot::Item(Referent::Object(id)) => out.items.push(id),
                    Slot::Item(Referent::Player(_)) => {
                        out.ignored.push(Ignored::Unresolved(r.clone()));
                    }
                    Slot::Ignored(ignored) => out.ignored.push(ignored),
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
        views.truncate(count_limit(n));
    }
    Ok(Resolution {
        items: views.into_iter().map(|v| v.id).collect(),
        ignored: Vec::new(),
    })
}

/// The number of objects a selector `count` of `n` keeps (REQ-DSL-003, REQ-CORE-006): 0 for
/// `n <= 0`, `usize::MAX` (keep all) for a positive `n` above `usize::MAX` (32-bit and wasm32
/// targets), otherwise `n`.
fn count_limit(n: i64) -> usize {
    count_limit_wide(i128::from(n))
}

/// [`count_limit`] over a wider input (REQ-CORE-006), so the overflow arm is reachable and
/// testable on every target: 0 for `n <= 0`, `usize::MAX` when `n` does not fit `usize`.
fn count_limit_wide(n: i128) -> usize {
    if n <= 0 {
        0
    } else {
        usize::try_from(n).unwrap_or(usize::MAX)
    }
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

#[cfg(test)]
mod count_tests {
    use super::{count_limit, count_limit_wide};

    /// REQ-CORE-006, REQ-DSL-003: selector count conversion: n <= 0 keeps none, a positive n
    /// keeps n, a positive n above `usize::MAX` keeps all (no panic, no wrap).
    #[test]
    fn count_limit_every_arm() {
        assert_eq!(count_limit(-1), 0);
        assert_eq!(count_limit(i64::MIN), 0);
        assert_eq!(count_limit(0), 0);
        assert_eq!(count_limit(1), 1);
        let max = usize::try_from(i64::MAX).unwrap_or(usize::MAX);
        assert_eq!(count_limit(i64::MAX), max);
        let above = i128::try_from(usize::MAX).unwrap() + 1;
        assert_eq!(count_limit_wide(above), usize::MAX);
        assert_eq!(count_limit_wide(i128::MAX), usize::MAX);
        assert_eq!(count_limit_wide(-5), 0);
    }
}
