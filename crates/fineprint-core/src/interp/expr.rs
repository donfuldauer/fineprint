// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Predicates and integer value expressions (REQ-DSL-003, REQ-CORE-006).

use std::collections::BTreeSet;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_dsl::expr::{
    Cmp, DistinctAttr, HistoryQuery as HistoryAST, MathOp, ObjectAttr, PlayerAttr,
    PlayerSelectorAST, PredicateAST, ReferenceAST, ValueExpressionAST, WordRef, Zone,
};

use super::resolve::{resolve_players, resolve_reference, select_objects, view_of};
use super::types::{EvalContext, InterpEnv, InterpError, ObjectView, PlayerStat, Referent};
use crate::events::{HistoryQuery as LedgerQuery, HistoryScope, HistoryStat};
use crate::memory::ChoiceValue;

/// The subject a predicate is tested on.
#[derive(Clone, Copy)]
enum Subject<'a> {
    Object(&'a ObjectView),
    Player(PlayerId),
}

/// Compares two integers with a DSL comparison operator (REQ-DSL-003).
#[must_use]
pub fn compare(cmp: Cmp, lhs: i64, rhs: i64) -> bool {
    match cmp {
        Cmp::Eq => lhs == rhs,
        Cmp::Ne => lhs != rhs,
        Cmp::Lt => lhs < rhs,
        Cmp::Le => lhs <= rhs,
        Cmp::Gt => lhs > rhs,
        Cmp::Ge => lhs >= rhs,
    }
}

/// Tests a predicate on an object (REQ-DSL-003, SRS §14.4.0), over the view's evaluated
/// characteristics and status, never the raw card.
///
/// # Errors
///
/// An error of an evaluated child, value expression, reference or text-token word
/// ([`InterpError::UnknownTextToken`] when a word token cannot be read, SRS §14.5).
pub fn eval_predicate_object(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    p: &PredicateAST,
    subject: &ObjectView,
) -> Result<bool, InterpError> {
    eval_pred(env, ctx, p, Subject::Object(subject))
}

/// Tests a predicate on a player (REQ-DSL-003, SRS §14.4.0). Object-only tests are false.
///
/// # Errors
///
/// As [`eval_predicate_object`].
pub fn eval_predicate_player(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    p: &PredicateAST,
    subject: PlayerId,
) -> Result<bool, InterpError> {
    eval_pred(env, ctx, p, Subject::Player(subject))
}

/// Resolves a word: a fixed name, or the current value of a rewritable word token of the
/// ability's source (SRS §14.5).
fn resolve_word(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    w: &WordRef,
) -> Result<String, InterpError> {
    match w {
        WordRef::Name(s) => Ok(s.clone()),
        WordRef::TextToken(id) => ctx
            .source
            .and_then(|src| env.text_token_value(src, id))
            .ok_or_else(|| InterpError::UnknownTextToken(id.clone())),
    }
}

/// The colour bit of a colour word (ASCII case-insensitive letter or name), if any.
fn color_bit(word: &str) -> Option<u8> {
    let w = word.to_ascii_lowercase();
    match w.as_str() {
        "w" | "white" => Some(1),
        "u" | "blue" => Some(2),
        "b" | "black" => Some(4),
        "r" | "red" => Some(8),
        "g" | "green" => Some(16),
        _ => None,
    }
}

fn eval_pred(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    p: &PredicateAST,
    subject: Subject<'_>,
) -> Result<bool, InterpError> {
    // Object-only tests: false for a player subject.
    let obj = |f: &dyn Fn(&ObjectView) -> bool| match subject {
        Subject::Object(s) => f(s),
        Subject::Player(_) => false,
    };
    Ok(match p {
        PredicateAST::And(children) => {
            for c in children {
                if !eval_pred(env, ctx, c, subject)? {
                    return Ok(false);
                }
            }
            true
        }
        PredicateAST::Or(children) => {
            for c in children {
                if eval_pred(env, ctx, c, subject)? {
                    return Ok(true);
                }
            }
            false
        }
        PredicateAST::Not(c) => !eval_pred(env, ctx, c, subject)?,
        PredicateAST::IsCreature => obj(&|s| s.characteristics.card_types.contains("Creature")),
        PredicateAST::IsLand => obj(&|s| s.characteristics.card_types.contains("Land")),
        // CR 110.1
        PredicateAST::IsPermanent => obj(&|s| s.zone == Zone::Battlefield),
        PredicateAST::IsPlayer => matches!(subject, Subject::Player(_)),
        PredicateAST::IsCardOrToken => matches!(subject, Subject::Object(_)),
        PredicateAST::IsToken => obj(&|s| s.is_token),
        PredicateAST::IsColorless => obj(&|s| s.characteristics.colors.bits() == 0),
        PredicateAST::IsMulticolored => obj(&|s| s.characteristics.colors.bits().count_ones() >= 2),
        // The environment supplies the status flags. FaceUp and Untapped are simply "not face
        // down" and "not tapped": they are true for any object without that flag, including
        // objects outside the battlefield.
        // CR 110.5, 110.5b
        PredicateAST::FaceUp => obj(&|s| !s.face_down),
        // CR 110.5, 110.5b
        PredicateAST::FaceDown => obj(&|s| s.face_down),
        // CR 110.5, 701.26a, 701.26b
        PredicateAST::Tapped => obj(&|s| s.tapped),
        // CR 110.5, 701.26a, 701.26b
        PredicateAST::Untapped => obj(&|s| !s.tapped),
        // CR 508.1k, 506.4
        PredicateAST::Attacking => obj(&|s| s.attacking),
        // CR 509.1g, 506.4
        PredicateAST::Blocking => obj(&|s| s.blocking),
        PredicateAST::EnteredThisTurn => obj(&|s| s.entered_this_turn),
        PredicateAST::WasCast => obj(&|s| s.was_cast),
        PredicateAST::HasCardType { value }
        | PredicateAST::HasSubtype { value }
        | PredicateAST::HasSupertype { value } => match subject {
            Subject::Object(s) => {
                let word = resolve_word(env, ctx, value)?;
                let set = match p {
                    PredicateAST::HasCardType { .. } => &s.characteristics.card_types,
                    PredicateAST::HasSubtype { .. } => &s.characteristics.subtypes,
                    _ => &s.characteristics.supertypes,
                };
                set.contains(&word)
            }
            Subject::Player(_) => false,
        },
        PredicateAST::HasColor { value } => match subject {
            Subject::Object(s) => {
                let word = resolve_word(env, ctx, value)?;
                color_bit(&word).is_some_and(|bit| s.characteristics.colors.bits() & bit != 0)
            }
            Subject::Player(_) => false,
        },
        PredicateAST::HasKeyword { keyword } => obj(&|s| s.keywords.contains(keyword)),
        PredicateAST::HasDesignation { kind } => obj(&|s| s.designations.contains(kind)),
        PredicateAST::HasCounter { kind, compare: c } => match subject {
            Subject::Object(s) => {
                let n = s.counters.get(kind).copied().unwrap_or(0);
                match c {
                    None => n >= 1,
                    Some((cmp, v)) => compare(*cmp, n, eval_value(env, ctx, v)?),
                }
            }
            Subject::Player(_) => false,
        },
        PredicateAST::ControlledBy { player } => match subject {
            Subject::Object(s) => {
                let who = resolve_players(env, ctx, player)?.items;
                s.controller.is_some_and(|c| who.contains(&c))
            }
            Subject::Player(_) => false,
        },
        PredicateAST::OwnedBy { player } => match subject {
            Subject::Object(s) => resolve_players(env, ctx, player)?.items.contains(&s.owner),
            Subject::Player(_) => false,
        },
        PredicateAST::ManaValue { cmp, value } => match subject {
            Subject::Object(s) => compare(*cmp, s.mana_value, eval_value(env, ctx, value)?),
            Subject::Player(_) => false,
        },
        PredicateAST::Power { cmp, value } | PredicateAST::Toughness { cmp, value } => {
            match subject {
                Subject::Object(s) => {
                    let rhs = eval_value(env, ctx, value)?;
                    let own = if matches!(p, PredicateAST::Power { .. }) {
                        s.power
                    } else {
                        s.toughness
                    };
                    own.is_some_and(|n| compare(*cmp, n, rhs))
                }
                Subject::Player(_) => false,
            }
        }
        PredicateAST::SameAs { reference } => {
            let me = match subject {
                Subject::Object(s) => Referent::Object(s.id),
                Subject::Player(pl) => Referent::Player(pl),
            };
            resolve_reference(env, ctx, reference)?.items.contains(&me)
        }
        PredicateAST::Controls { selector, player } => {
            let who: Vec<PlayerId> = match (player, subject) {
                (Some(sel), _) => resolve_players(env, ctx, sel)?.items,
                (None, Subject::Player(pl)) => vec![pl],
                (None, Subject::Object(_)) => vec![ctx.controller],
            };
            let selected = select_objects(env, ctx, selector)?.items;
            selected.into_iter().any(|id| {
                env.object(id)
                    .and_then(|v| v.controller)
                    .is_some_and(|c| who.contains(&c))
            })
        }
        PredicateAST::PlayerHasMaxSpeed { player } => {
            let who = resolve_players(env, ctx, player)?.items;
            !who.is_empty() && who.iter().all(|pl| env.has_max_speed(*pl))
        }
        PredicateAST::Compare { cmp, lhs, rhs } => {
            let l = eval_value(env, ctx, lhs)?;
            let r = eval_value(env, ctx, rhs)?;
            compare(*cmp, l, r)
        }
        // SRS §14.5: the token's CURRENT value (it may have been rewritten).
        PredicateAST::TextTokenMatch { text_token, value } => match ctx.source {
            None => false,
            Some(src) => match env.text_token_value(src, text_token) {
                Some(v) => v == *value,
                None => return Err(InterpError::UnknownTextToken(text_token.clone())),
            },
        },
    })
}

/// A length as an `i64` (saturating).
fn len_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// Saturating sum.
fn sat_sum(values: impl IntoIterator<Item = i64>) -> i64 {
    values.into_iter().fold(0_i64, i64::saturating_add)
}

/// Evaluates an integer value expression (REQ-DSL-003, REQ-CORE-006, SRS §14.4.0). Integer
/// only; additions, subtractions and multiplications saturate. A reference that resolves to
/// nothing, or to something gone, contributes 0 (REQ-ARCH-002, CR 101.3).
///
/// # Errors
///
/// [`InterpError::MissingX`] for `X` without a value; [`InterpError::DivisionByZero`] for a
/// division by zero; [`InterpError::UnboundVariable`] and [`InterpError::UnknownTextToken`]
/// from references and words.
pub fn eval_value(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    v: &ValueExpressionAST,
) -> Result<i64, InterpError> {
    match v {
        ValueExpressionAST::Literal(n) => Ok(*n),
        ValueExpressionAST::X => ctx.x.ok_or(InterpError::MissingX),
        ValueExpressionAST::Count(sel) => Ok(len_i64(select_objects(env, ctx, sel)?.items.len())),
        ValueExpressionAST::Attr { reference, attr } => {
            let items = resolve_reference(env, ctx, reference)?.items;
            // CR 608.2h, CR 113.7a, CR 608.2b: SOURCE_LKI reads the source's captured last
            // known information first; every other reference reads current information first.
            let source_lki = *reference == ReferenceAST::SourceLki;
            Ok(sat_sum(items.into_iter().map(|item| match item {
                Referent::Object(id) => {
                    let view = if source_lki && ctx.source == Some(id) {
                        lki_first_view(env, id)
                    } else {
                        view_of(env, id)
                    };
                    view.map_or(0, |view| object_attr(&view, attr))
                }
                Referent::Player(_) => 0,
            })))
        }
        ValueExpressionAST::PlayerAttr { reference, attr } => {
            let players: Vec<PlayerId> = resolve_reference(env, ctx, reference)?
                .items
                .into_iter()
                .filter_map(|r| match r {
                    Referent::Player(p) => Some(p),
                    Referent::Object(_) => None,
                })
                .collect();
            let stat = match attr {
                PlayerAttr::Life => PlayerStat::Life,
                PlayerAttr::Speed => PlayerStat::Speed,
                PlayerAttr::HandSize => PlayerStat::HandSize,
                PlayerAttr::LibrarySize => PlayerStat::LibrarySize,
                PlayerAttr::GraveyardSize => PlayerStat::GraveyardSize,
                PlayerAttr::Counters(kind) => PlayerStat::Counters(kind.clone()),
                PlayerAttr::Devotion(words) => {
                    let colors = words
                        .iter()
                        .map(|w| resolve_word(env, ctx, w))
                        .collect::<Result<Vec<String>, InterpError>>()?;
                    return Ok(sat_sum(players.iter().map(|p| env.devotion(*p, &colors))));
                }
            };
            Ok(sat_sum(
                players
                    .iter()
                    .map(|p| env.player_stat(*p, &stat).unwrap_or(0)),
            ))
        }
        ValueExpressionAST::Math { op, lhs, rhs } => {
            let l = eval_value(env, ctx, lhs)?;
            let r = eval_value(env, ctx, rhs)?;
            math(*op, l, r)
        }
        ValueExpressionAST::Distinct { attr, selector } => {
            let views: Vec<ObjectView> = select_objects(env, ctx, selector)?
                .items
                .into_iter()
                .filter_map(|id| view_of(env, id))
                .collect();
            Ok(distinct(*attr, &views))
        }
        ValueExpressionAST::History(q) => eval_history(env, ctx, q),
        ValueExpressionAST::Memory(key) => Ok(ctx
            .source
            .and_then(|src| env.memory(src))
            .and_then(|m| m.choice(key))
            .map_or(0, |c| match c {
                ChoiceValue::Number(n) => *n,
                ChoiceValue::Mode(m) => i64::from(*m),
                _ => 0,
            })),
    }
}

/// The source's captured last known information, falling back to its current information when
/// none was captured (REQ-DSL-003, CR 608.2h, CR 113.7a, CR 608.2b): the view `SOURCE_LKI` reads.
fn lki_first_view(env: &dyn InterpEnv, id: ObjectId) -> Option<ObjectView> {
    env.lki(id).or_else(|| env.object(id))
}

/// One characteristic of an object as an integer (a missing power or toughness counts as 0,
/// CR 208.5).
fn object_attr(view: &ObjectView, attr: &ObjectAttr) -> i64 {
    match attr {
        // CR 208.5: a missing value counts as 0.
        ObjectAttr::Power => view.power.unwrap_or(0),
        ObjectAttr::Toughness => view.toughness.unwrap_or(0),
        ObjectAttr::Loyalty => view.loyalty.unwrap_or(0),
        ObjectAttr::Defense => view.defense.unwrap_or(0),
        ObjectAttr::ManaValue => view.mana_value,
        ObjectAttr::ColorCount => i64::from(view.characteristics.colors.bits().count_ones()),
        ObjectAttr::Counters(kind) => view.counters.get(kind).copied().unwrap_or(0),
    }
}

/// `MATH(op, lhs, rhs)` over integers (REQ-CORE-006): saturating add/sub/mul; floor and
/// ceiling division (saturating for `i64::MIN / -1`).
fn math(op: MathOp, l: i64, r: i64) -> Result<i64, InterpError> {
    Ok(match op {
        MathOp::Add => l.saturating_add(r),
        MathOp::Sub => l.saturating_sub(r),
        MathOp::Mul => l.saturating_mul(r),
        MathOp::DivFloor | MathOp::DivCeil => {
            if r == 0 {
                return Err(InterpError::DivisionByZero);
            }
            let (Some(q), Some(rem)) = (l.checked_div(r), l.checked_rem(r)) else {
                // Only i64::MIN / -1 overflows: the exact quotient is above i64::MAX.
                return Ok(i64::MAX);
            };
            let exact = rem == 0;
            let negative = (rem < 0) != (r < 0);
            match op {
                MathOp::DivFloor if !exact && negative => q.saturating_sub(1),
                MathOp::DivCeil if !exact && !negative => q.saturating_add(1),
                _ => q,
            }
        }
        MathOp::Min => l.min(r),
        MathOp::Max => l.max(r),
    })
}

/// The number of distinct values of `attr` among the views.
fn distinct(attr: DistinctAttr, views: &[ObjectView]) -> i64 {
    match attr {
        DistinctAttr::CardType
        | DistinctAttr::Subtype
        | DistinctAttr::Supertype
        | DistinctAttr::Name => {
            let set: BTreeSet<&String> = views
                .iter()
                .flat_map(|v| {
                    let c = &v.characteristics;
                    match attr {
                        DistinctAttr::CardType => &c.card_types,
                        DistinctAttr::Subtype => &c.subtypes,
                        DistinctAttr::Supertype => &c.supertypes,
                        _ => &c.names,
                    }
                })
                .collect();
            len_i64(set.len())
        }
        DistinctAttr::Color => {
            let bits = views
                .iter()
                .fold(0_u8, |acc, v| acc | v.characteristics.colors.bits());
            i64::from((bits & 0b1_1111).count_ones())
        }
        DistinctAttr::ManaValue => len_i64(
            views
                .iter()
                .map(|v| v.mana_value)
                .collect::<BTreeSet<i64>>()
                .len(),
        ),
        DistinctAttr::Power => len_i64(
            views
                .iter()
                .filter_map(|v| v.power)
                .collect::<BTreeSet<i64>>()
                .len(),
        ),
        DistinctAttr::Toughness => len_i64(
            views
                .iter()
                .filter_map(|v| v.toughness)
                .collect::<BTreeSet<i64>>()
                .len(),
        ),
    }
}

/// The players a player selector resolves to (none resolved: empty).
fn players_of(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    sel: &PlayerSelectorAST,
) -> Result<Vec<PlayerId>, InterpError> {
    Ok(resolve_players(env, ctx, sel)?.items)
}

/// `HISTORY(query)`, answered from the history ledger for the current turn and from the
/// environment's per-turn lists (REQ-DSL-003).
fn eval_history(
    env: &dyn InterpEnv,
    ctx: &EvalContext,
    q: &HistoryAST,
) -> Result<i64, InterpError> {
    let turn = env.turn_number();
    let ledger = |stat: HistoryStat, player: Option<PlayerId>| {
        env.history().query(&LedgerQuery {
            stat,
            scope: HistoryScope::Turn(turn),
            player,
        })
    };
    let per_player = |stat: HistoryStat, sel: &PlayerSelectorAST| -> Result<i64, InterpError> {
        Ok(sat_sum(
            players_of(env, ctx, sel)?
                .into_iter()
                .map(|p| ledger(stat, Some(p))),
        ))
    };
    match q {
        HistoryAST::SpellsCastThisTurn {
            player,
            condition: None,
        } => match player {
            None => Ok(ledger(HistoryStat::SpellsCast, None)),
            Some(sel) => per_player(HistoryStat::SpellsCast, sel),
        },
        HistoryAST::SpellsCastThisTurn {
            player,
            condition: Some(pred),
        } => {
            // No player selector: every caster counts, as in the unconditioned ledger query.
            let casters: Option<Vec<PlayerId>> = match player {
                None => None,
                Some(sel) => Some(players_of(env, ctx, sel)?),
            };
            let mut n = 0_i64;
            for (caster, view) in env.spells_cast_this_turn() {
                if casters.as_ref().is_none_or(|c| c.contains(&caster))
                    && eval_predicate_object(env, ctx, pred, &view)?
                {
                    n = n.saturating_add(1);
                }
            }
            Ok(n)
        }
        HistoryAST::LifeLostThisTurn { player } => per_player(HistoryStat::LifeLost, player),
        HistoryAST::LifeGainedThisTurn { player } => per_player(HistoryStat::LifeGained, player),
        HistoryAST::CardsDrawnThisTurn { player } => per_player(HistoryStat::CardsDrawn, player),
        HistoryAST::CreaturesDiedThisTurn { condition } => {
            let mut n = 0_i64;
            for view in env.creatures_died_this_turn() {
                let holds = match condition {
                    Some(pred) => eval_predicate_object(env, ctx, pred, &view)?,
                    None => true,
                };
                if holds {
                    n = n.saturating_add(1);
                }
            }
            Ok(n)
        }
        HistoryAST::ManaSpentToCast { reference, color } => {
            let color = match color {
                Some(w) => Some(resolve_word(env, ctx, w)?),
                None => None,
            };
            let items = resolve_reference(env, ctx, reference)?.items;
            Ok(sat_sum(items.into_iter().filter_map(|r| match r {
                Referent::Object(id) => Some(env.mana_spent_to_cast(id, color.as_deref())),
                Referent::Player(_) => None,
            })))
        }
    }
}
