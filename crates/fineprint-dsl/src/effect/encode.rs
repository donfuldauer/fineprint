// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Node-level validity and encoding of [`Instruction`] (REQ-DSL-010, REQ-NFR-006). The check is
//! shallow: nested instructions, costs, choices, effects and modes check themselves when they
//! are decoded or written.

use serde::ser::SerializeMap;

use super::util::{check_non_empty, check_opaque_map, put_opt};
use super::{Instruction, TokenSource};
use crate::expr::json::{JResult, Members, check_bound_var, check_free_str, ser_check};

impl Instruction {
    /// Checks the node-level rules a hand-built value could break: STR members non-empty,
    /// every instruction and cost list non-empty, opaque objects valid, `captures` and `bind`
    /// names, the `CHOOSE_MODE` bounds and distinct mode ids, and the `NATIVE_EFFECT` rules.
    pub(crate) fn check(&self) -> JResult<()> {
        use Instruction as I;
        match self {
            I::PlayPermissionGrant {
                also_as: Some(s), ..
            } => check_free_str(s, "alsoAs"),
            I::CounterSpell {
                unless: Some(c), ..
            } => check_non_empty(c, "unless"),
            I::PutCounters { kind, .. }
            | I::RemoveCounters { kind, .. }
            | I::MoveCounters { kind, .. }
            | I::CreatePredefinedToken { kind, .. } => check_free_str(kind, "kind"),
            I::CreateToken {
                token, modifiers, ..
            } => {
                token.check()?;
                match modifiers {
                    Some(m) => check_opaque_map(m, true, "modifiers"),
                    None => Ok(()),
                }
            }
            I::CreateCopyToken {
                except: Some(e), ..
            } => check_opaque_map(e, true, "except"),
            I::CastFrom {
                alt_cost: Some(c), ..
            } => check_non_empty(c, "altCost"),
            I::TurnFaceUp {
                profile: Some(p), ..
            }
            | I::TurnFaceDown {
                profile: Some(p), ..
            } => check_free_str(p, "profile"),
            I::SetDesignation { kind, value, .. } => {
                check_free_str(kind, "kind")?;
                value.check()
            }
            I::Waterbend { cost } => check_non_empty(cost, "cost"),
            I::CreateContinuousEffect { effects, .. } => check_non_empty(effects, "effects"),
            I::CreateDelayedTrigger {
                effect, captures, ..
            } => {
                check_non_empty(effect, "effect")?;
                if let Some(c) = captures {
                    if c.is_empty() {
                        return Err("captures must not be an empty object".to_owned());
                    }
                    c.keys().try_for_each(|k| check_bound_var(k))?;
                }
                Ok(())
            }
            I::ReflexiveTrigger {
                if_action_taken,
                effect,
            } => {
                check_non_empty(if_action_taken, "ifActionTaken")?;
                check_non_empty(effect, "effect")
            }
            I::If {
                then, otherwise, ..
            } => {
                check_non_empty(then, "then")?;
                match otherwise {
                    Some(e) => check_non_empty(e, "else"),
                    None => Ok(()),
                }
            }
            I::ForEach { bind, body, .. } => {
                if let Some(b) = bind {
                    check_bound_var(b)?;
                }
                check_non_empty(body, "body")
            }
            I::Repeat { body, .. } | I::RepeatWhile { body, .. } | I::May { body, .. } => {
                check_non_empty(body, "body")
            }
            I::Unless { cost, body, .. } => {
                check_non_empty(cost, "cost")?;
                check_non_empty(body, "body")
            }
            I::ChooseMode {
                min, max, modes, ..
            } => {
                check_non_empty(modes, "modes")?;
                for (i, mode) in modes.iter().enumerate() {
                    if modes[..i].iter().any(|m| m.id == mode.id) {
                        return Err(format!("mode id {:?} is listed twice", mode.id));
                    }
                }
                if min > max {
                    return Err(format!("CHOOSE_MODE min {min} is greater than max {max}"));
                }
                if u64::from(*max) > modes.len() as u64 {
                    return Err(format!(
                        "CHOOSE_MODE max {max} exceeds the {} modes",
                        modes.len()
                    ));
                }
                Ok(())
            }
            I::Vote { store, .. } => check_free_str(store, "store"),
            I::Remember { key, .. } => check_free_str(key, "key"),
            I::NativeEffect(n) => n.check(),
            _ => Ok(()),
        }
    }
}

impl Members for Instruction {
    fn members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        if let Instruction::NativeEffect(n) = self {
            return n.members(m);
        }
        ser_check(self.check())?;
        m.serialize_entry("op", self.name())?;
        if self.is_nested() {
            self.nested_members(m)
        } else {
            self.flat_members(m)
        }
    }
}

impl Instruction {
    /// Whether this op holds instruction lists (directly or through modes): the only ops on the
    /// recursion path of a nested instruction tree, written by a function of their own so that
    /// one nesting level costs a small stack frame even in unoptimised builds.
    const fn is_nested(&self) -> bool {
        matches!(
            self,
            Instruction::CreateDelayedTrigger { .. }
                | Instruction::ReflexiveTrigger { .. }
                | Instruction::If { .. }
                | Instruction::ForEach { .. }
                | Instruction::Repeat { .. }
                | Instruction::RepeatWhile { .. }
                | Instruction::May { .. }
                | Instruction::Unless { .. }
                | Instruction::ChooseMode { .. }
        )
    }

    /// Writes the members (other than `op`) of an op for which [`Instruction::is_nested`]
    /// holds; any other op is written by [`Instruction::flat_members`].
    fn nested_members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        use Instruction as I;
        match self {
            I::CreateDelayedTrigger {
                trigger,
                effect,
                captures,
                one_shot,
            } => {
                m.serialize_entry("trigger", trigger)?;
                m.serialize_entry("effect", effect)?;
                put_opt(m, "captures", captures.as_ref())?;
                m.serialize_entry("oneShot", one_shot)
            }
            I::ReflexiveTrigger {
                if_action_taken,
                effect,
            } => {
                m.serialize_entry("ifActionTaken", if_action_taken)?;
                m.serialize_entry("effect", effect)
            }
            I::If {
                cond,
                then,
                otherwise,
            } => {
                m.serialize_entry("cond", cond)?;
                m.serialize_entry("then", then)?;
                put_opt(m, "else", otherwise.as_ref())
            }
            I::ForEach {
                iterator,
                bind,
                body,
            } => {
                m.serialize_entry("iterator", iterator)?;
                put_opt(m, "bind", bind.as_ref())?;
                m.serialize_entry("body", body)
            }
            I::Repeat { times, body } => {
                m.serialize_entry("times", times)?;
                m.serialize_entry("body", body)
            }
            I::RepeatWhile {
                cond,
                max_iter,
                body,
            } => {
                m.serialize_entry("cond", cond)?;
                m.serialize_entry("maxIter", &max_iter.get())?;
                m.serialize_entry("body", body)
            }
            I::May { player, body } => {
                m.serialize_entry("player", player)?;
                m.serialize_entry("body", body)
            }
            I::Unless { payer, cost, body } => {
                m.serialize_entry("payer", payer)?;
                m.serialize_entry("cost", cost)?;
                m.serialize_entry("body", body)
            }
            I::ChooseMode {
                chooser,
                min,
                max,
                modes,
            } => {
                m.serialize_entry("chooser", chooser)?;
                m.serialize_entry("min", min)?;
                m.serialize_entry("max", max)?;
                m.serialize_entry("modes", modes)
            }
            _ => self.flat_members(m),
        }
    }

    /// Writes the members (other than `op`) of an op for which [`Instruction::is_nested`] does
    /// not hold; nested ops are handed to [`Instruction::nested_members`].
    #[inline(never)]
    fn flat_members<M: SerializeMap>(&self, m: &mut M) -> Result<(), M::Error> {
        use Instruction as I;
        match self {
            I::Draw { player, count } | I::Mill { player, count } => {
                m.serialize_entry("player", player)?;
                m.serialize_entry("count", count)
            }
            I::Discard {
                player,
                count,
                choice,
            } => {
                m.serialize_entry("player", player)?;
                m.serialize_entry("count", count)?;
                put_opt(m, "choice", choice.as_ref())
            }
            I::Move {
                objects,
                to_zone,
                position,
                face_down,
                tapped,
                under_control_of,
                transformed,
                choice,
            } => {
                m.serialize_entry("objects", objects)?;
                m.serialize_entry("toZone", to_zone)?;
                put_opt(m, "position", position.as_ref())?;
                put_opt(m, "faceDown", face_down.as_ref())?;
                put_opt(m, "tapped", tapped.as_ref())?;
                put_opt(m, "underControlOf", under_control_of.as_ref())?;
                put_opt(m, "transformed", transformed.as_ref())?;
                put_opt(m, "choice", choice.as_ref())
            }
            I::SearchLibrary {
                player,
                selector,
                count,
                to_zone,
                position,
                choice,
            } => {
                m.serialize_entry("player", player)?;
                m.serialize_entry("selector", selector)?;
                m.serialize_entry("count", count)?;
                m.serialize_entry("toZone", to_zone)?;
                put_opt(m, "position", position.as_ref())?;
                put_opt(m, "choice", choice.as_ref())
            }
            I::Shuffle { player } => m.serialize_entry("player", player),
            I::Reveal {
                objects,
                to,
                choice,
            } => {
                m.serialize_entry("objects", objects)?;
                put_opt(m, "to", to.as_ref())?;
                put_opt(m, "choice", choice.as_ref())
            }
            I::LookAt { objects, player } => {
                m.serialize_entry("objects", objects)?;
                m.serialize_entry("player", player)
            }
            I::PutOnTopOrBottom {
                objects,
                player,
                choice,
            } => {
                m.serialize_entry("objects", objects)?;
                m.serialize_entry("player", player)?;
                put_opt(m, "choice", choice.as_ref())
            }
            I::ExileUntil {
                objects,
                duration,
                face_down,
            } => {
                m.serialize_entry("objects", objects)?;
                m.serialize_entry("duration", duration)?;
                put_opt(m, "faceDown", face_down.as_ref())
            }
            I::PlayPermissionGrant {
                objects,
                duration,
                who,
                also_as,
            } => {
                m.serialize_entry("objects", objects)?;
                m.serialize_entry("duration", duration)?;
                m.serialize_entry("who", who)?;
                put_opt(m, "alsoAs", also_as.as_ref())
            }
            I::DealDamage {
                source,
                recipients,
                amount,
                divided,
                choice,
            } => {
                m.serialize_entry("source", source)?;
                m.serialize_entry("recipients", recipients)?;
                m.serialize_entry("amount", amount)?;
                put_opt(m, "divided", divided.as_ref())?;
                put_opt(m, "choice", choice.as_ref())
            }
            I::Fight { first, second } | I::ExchangeControl { first, second } => {
                m.serialize_entry("first", first)?;
                m.serialize_entry("second", second)
            }
            I::Bite { source, target } => {
                m.serialize_entry("source", source)?;
                m.serialize_entry("target", target)
            }
            I::GainLife { player, amount }
            | I::LoseLife { player, amount }
            | I::SetLife { player, amount } => {
                m.serialize_entry("player", player)?;
                m.serialize_entry("amount", amount)
            }
            I::Prevent {
                duration,
                source,
                recipients,
                amount,
                combat_only,
            } => {
                m.serialize_entry("duration", duration)?;
                put_opt(m, "source", source.as_ref())?;
                put_opt(m, "recipients", recipients.as_ref())?;
                put_opt(m, "amount", amount.as_ref())?;
                put_opt(m, "combatOnly", combat_only.as_ref())
            }
            I::Destroy {
                objects,
                no_regeneration,
            } => {
                m.serialize_entry("objects", objects)?;
                put_opt(m, "noRegeneration", no_regeneration.as_ref())
            }
            I::Sacrifice {
                player,
                selector,
                count,
                choice,
            } => {
                m.serialize_entry("player", player)?;
                m.serialize_entry("selector", selector)?;
                m.serialize_entry("count", count)?;
                put_opt(m, "choice", choice.as_ref())
            }
            I::Regenerate { objects }
            | I::Transform { objects }
            | I::Convert { objects }
            | I::PhaseOut { objects }
            | I::Goad { objects }
            | I::Meld { objects }
            | I::Manifest { objects }
            | I::Cloak { objects }
            | I::Explore { objects }
            | I::Connive { objects } => m.serialize_entry("objects", objects),
            I::CounterSpell { objects, unless } => {
                m.serialize_entry("objects", objects)?;
                put_opt(m, "unless", unless.as_ref())
            }
            I::PutCounters {
                objects,
                kind,
                count,
            }
            | I::RemoveCounters {
                objects,
                kind,
                count,
            } => {
                m.serialize_entry("objects", objects)?;
                m.serialize_entry("kind", kind)?;
                m.serialize_entry("count", count)
            }
            I::MoveCounters {
                from,
                to,
                kind,
                count,
            } => {
                m.serialize_entry("from", from)?;
                m.serialize_entry("to", to)?;
                m.serialize_entry("kind", kind)?;
                m.serialize_entry("count", count)
            }
            I::Proliferate { choice } => put_opt(m, "choice", choice.as_ref()),
            I::CreateToken {
                token,
                count,
                modifiers,
                controller,
            } => {
                match token {
                    TokenSource::Def(d) => m.serialize_entry("tokenDef", d)?,
                    TokenSource::Inline(i) => m.serialize_entry("inline", i)?,
                }
                m.serialize_entry("count", count)?;
                put_opt(m, "modifiers", modifiers.as_ref())?;
                put_opt(m, "controller", controller.as_ref())
            }
            I::CreatePredefinedToken { kind, count } => {
                m.serialize_entry("kind", kind)?;
                put_opt(m, "count", count.as_ref())
            }
            I::CreateCopyToken { of, except } => {
                m.serialize_entry("of", of)?;
                put_opt(m, "except", except.as_ref())
            }
            I::Populate | I::Venture | I::RingTempts => Ok(()),
            I::Amass { n }
            | I::Incubate { n }
            | I::EmpowerJace { n }
            | I::Discover { n }
            | I::Blight { n } => m.serialize_entry("n", n),
            I::CopySpellOrAbility {
                objects,
                new_targets,
            } => {
                m.serialize_entry("objects", objects)?;
                put_opt(m, "newTargets", new_targets.as_ref())
            }
            I::ChangeTargets { objects, choice } => {
                m.serialize_entry("objects", objects)?;
                put_opt(m, "choice", choice.as_ref())
            }
            I::CastFrom {
                objects,
                without_paying,
                alt_cost,
            } => {
                m.serialize_entry("objects", objects)?;
                put_opt(m, "withoutPaying", without_paying.as_ref())?;
                put_opt(m, "altCost", alt_cost.as_ref())
            }
            I::TapOrUntap { objects, action } => {
                m.serialize_entry("objects", objects)?;
                m.serialize_entry("action", action)
            }
            I::TurnFaceUp { objects, profile } | I::TurnFaceDown { objects, profile } => {
                m.serialize_entry("objects", objects)?;
                put_opt(m, "profile", profile.as_ref())
            }
            I::Attach { objects, to } => {
                m.serialize_entry("objects", objects)?;
                m.serialize_entry("to", to)
            }
            I::GainControl {
                objects,
                duration,
                player,
            } => {
                m.serialize_entry("objects", objects)?;
                m.serialize_entry("duration", duration)?;
                put_opt(m, "player", player.as_ref())
            }
            I::SetDesignation { kind, value, to } => {
                m.serialize_entry("kind", kind)?;
                m.serialize_entry("value", value)?;
                put_opt(m, "to", to.as_ref())
            }
            I::IncreaseSpeed { player, amount } => {
                m.serialize_entry("player", player)?;
                put_opt(m, "amount", amount.as_ref())
            }
            I::MutateMerge { objects, onto } => {
                m.serialize_entry("objects", objects)?;
                m.serialize_entry("onto", onto)
            }
            I::Waterbend { cost } => m.serialize_entry("cost", cost),
            I::CreateContinuousEffect {
                effects,
                duration,
                lock_affected_set,
            } => {
                m.serialize_entry("effects", effects)?;
                m.serialize_entry("duration", duration)?;
                put_opt(m, "lockAffectedSet", lock_affected_set.as_ref())
            }
            I::Vote {
                voters,
                choice,
                store,
            } => {
                m.serialize_entry("voters", voters)?;
                m.serialize_entry("choice", choice)?;
                m.serialize_entry("store", store)
            }
            I::Remember { key, into, source } => {
                m.serialize_entry("key", key)?;
                m.serialize_entry("into", into)?;
                source.members(m)
            }

            I::NativeEffect(n) => n.members(m),
            I::CreateDelayedTrigger { .. }
            | I::ReflexiveTrigger { .. }
            | I::If { .. }
            | I::ForEach { .. }
            | I::Repeat { .. }
            | I::RepeatWhile { .. }
            | I::May { .. }
            | I::Unless { .. }
            | I::ChooseMode { .. } => self.nested_members(m),
        }
    }
}
