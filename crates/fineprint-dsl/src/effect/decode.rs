// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Decoding of [`Instruction`] from buffered JSON (REQ-DSL-010, REQ-NFR-006). Recursion happens
//! only on the buffered, depth-checked value.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use serde_json::Value;

use super::util::{non_empty_vec, opaque_map, opt, opt_bool, req};
use super::{Instruction, NativeEffectAST, RememberSource, TokenSource, instr_list};
use crate::ability::CostAST;
use crate::expr::ObjectSelectorAST;
use crate::expr::json::{
    FromJson, JResult, Obj, bool_of, check_bound_var, free_str, int_of, str_of, u32_of,
};

/// Decodes the required non-empty instruction list `key` (rule A4).
fn instrs(o: &mut Obj<'_>, key: &str) -> JResult<Vec<Instruction>> {
    instr_list(o.req(key)?, key)
}

/// Decodes the required non-empty cost list `key` (rule A4).
fn costs(o: &mut Obj<'_>, key: &str) -> JResult<Vec<CostAST>> {
    non_empty_vec(o.req(key)?, key)
}

/// Decodes the optional non-empty cost list `key` (rule A4).
fn opt_costs(o: &mut Obj<'_>, key: &str) -> JResult<Option<Vec<CostAST>>> {
    o.opt(key)?.map(|c| non_empty_vec(c, key)).transpose()
}

/// Decodes the required STR member `key` (rule A3).
fn string(o: &mut Obj<'_>, key: &str) -> JResult<String> {
    free_str(o.req(key)?, key)
}

/// Decodes the optional STR member `key` (rule A3).
fn opt_string(o: &mut Obj<'_>, key: &str) -> JResult<Option<String>> {
    o.opt(key)?.map(|s| free_str(s, key)).transpose()
}

/// Decodes the optional non-empty opaque object `key` (rule A5).
fn opt_opaque(o: &mut Obj<'_>, key: &str) -> JResult<Option<BTreeMap<String, Value>>> {
    o.opt(key)?.map(|m| opaque_map(m, true, key)).transpose()
}

/// Decodes `maxIter`: an integer 1..=4294967295 (REQ-DSL-010 loop safety).
fn max_iter(v: &Value) -> JResult<NonZeroU32> {
    let i = int_of(v, "maxIter")?;
    u32::try_from(i)
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| format!("maxIter {i} is outside 1..=4294967295"))
}

/// Decodes `captures`: a non-empty object mapping `$` variables to object selectors.
fn captures(v: &Value) -> JResult<BTreeMap<String, ObjectSelectorAST>> {
    let map = v
        .as_object()
        .ok_or_else(|| "captures must be an object".to_owned())?;
    if map.is_empty() {
        return Err("captures must not be an empty object".to_owned());
    }
    map.iter()
        .map(|(k, s)| {
            check_bound_var(k)?;
            Ok((k.clone(), ObjectSelectorAST::from_json(s)?))
        })
        .collect()
}

/// A decoder of one op's members.
type TakeFn = fn(&mut Obj<'_>) -> JResult<Instruction>;

/// The decoder of an op whose members hold instruction lists (directly or through modes), if
/// `op` is one. These are the only ops on the recursion path of a nested instruction tree; each
/// has its own small function so that one nesting level costs a small stack frame even in
/// unoptimised builds.
fn nested_decoder(op: &str) -> Option<TakeFn> {
    Some(match op {
        "CREATE_DELAYED_TRIGGER" => take_delayed_trigger,
        "REFLEXIVE_TRIGGER" => take_reflexive_trigger,
        "IF" => take_if,
        "FOR_EACH" => take_for_each,
        "REPEAT" => take_repeat,
        "REPEAT_WHILE" => take_repeat_while,
        "MAY" => take_may,
        "UNLESS" => take_unless,
        "CHOOSE_MODE" => take_choose_mode,
        _ => return None,
    })
}

/// Decodes the members of the instruction whose `op` is `op` (everything but `op`).
fn take_op(o: &mut Obj<'_>, op: &str) -> JResult<Instruction> {
    match nested_decoder(op) {
        Some(take) => take(o),
        None => take_flat(o, op),
    }
}

/// `CREATE_DELAYED_TRIGGER` (CR 603.7).
fn take_delayed_trigger(o: &mut Obj<'_>) -> JResult<Instruction> {
    Ok(Instruction::CreateDelayedTrigger {
        trigger: req(o, "trigger")?,
        effect: instrs(o, "effect")?,
        captures: o.opt("captures")?.map(captures).transpose()?,
        one_shot: bool_of(o.req("oneShot")?, "oneShot")?,
    })
}

/// `REFLEXIVE_TRIGGER` (CR 603.12).
fn take_reflexive_trigger(o: &mut Obj<'_>) -> JResult<Instruction> {
    Ok(Instruction::ReflexiveTrigger {
        if_action_taken: instrs(o, "ifActionTaken")?,
        effect: instrs(o, "effect")?,
    })
}

/// `IF`.
fn take_if(o: &mut Obj<'_>) -> JResult<Instruction> {
    Ok(Instruction::If {
        cond: req(o, "cond")?,
        then: instrs(o, "then")?,
        otherwise: match o.opt("else")? {
            Some(e) => Some(instr_list(e, "else")?),
            None => None,
        },
    })
}

/// `FOR_EACH`.
fn take_for_each(o: &mut Obj<'_>) -> JResult<Instruction> {
    let iterator = req(o, "iterator")?;
    let bind = match o.opt("bind")? {
        Some(b) => {
            let s = str_of(b, "bind")?;
            check_bound_var(s)?;
            Some(s.to_owned())
        }
        None => None,
    };
    Ok(Instruction::ForEach {
        iterator,
        bind,
        body: instrs(o, "body")?,
    })
}

/// `REPEAT`.
fn take_repeat(o: &mut Obj<'_>) -> JResult<Instruction> {
    Ok(Instruction::Repeat {
        times: req(o, "times")?,
        body: instrs(o, "body")?,
    })
}

/// `REPEAT_WHILE` (`maxIter` required).
fn take_repeat_while(o: &mut Obj<'_>) -> JResult<Instruction> {
    Ok(Instruction::RepeatWhile {
        cond: req(o, "cond")?,
        max_iter: max_iter(o.req("maxIter")?)?,
        body: instrs(o, "body")?,
    })
}

/// `MAY`.
fn take_may(o: &mut Obj<'_>) -> JResult<Instruction> {
    Ok(Instruction::May {
        player: req(o, "player")?,
        body: instrs(o, "body")?,
    })
}

/// `UNLESS`: "\[A\] unless \[B\]" counts as mandatory for loop detection (CR 733.6).
fn take_unless(o: &mut Obj<'_>) -> JResult<Instruction> {
    Ok(Instruction::Unless {
        payer: req(o, "payer")?,
        cost: costs(o, "cost")?,
        body: instrs(o, "body")?,
    })
}

/// `CHOOSE_MODE`.
fn take_choose_mode(o: &mut Obj<'_>) -> JResult<Instruction> {
    Ok(Instruction::ChooseMode {
        chooser: req(o, "chooser")?,
        min: u32_of(o.req("min")?, "min")?,
        max: u32_of(o.req("max")?, "max")?,
        modes: non_empty_vec(o.req("modes")?, "modes")?,
    })
}

/// Decodes the members of an op outside the set that [`nested_decoder`] handles.
#[inline(never)]
fn take_flat(o: &mut Obj<'_>, op: &str) -> JResult<Instruction> {
    use Instruction as I;
    Ok(match op {
        // Zones
        "DRAW" => I::Draw {
            player: req(o, "player")?,
            count: req(o, "count")?,
        },
        "MILL" => I::Mill {
            player: req(o, "player")?,
            count: req(o, "count")?,
        },
        "DISCARD" => I::Discard {
            player: req(o, "player")?,
            count: req(o, "count")?,
            choice: opt(o, "choice")?,
        },
        "MOVE" => I::Move {
            objects: req(o, "objects")?,
            to_zone: req(o, "toZone")?,
            position: opt(o, "position")?,
            face_down: opt_bool(o, "faceDown")?,
            tapped: opt_bool(o, "tapped")?,
            under_control_of: opt(o, "underControlOf")?,
            transformed: opt_bool(o, "transformed")?,
            choice: opt(o, "choice")?,
        },
        "SEARCH_LIBRARY" => I::SearchLibrary {
            player: req(o, "player")?,
            selector: req(o, "selector")?,
            count: req(o, "count")?,
            to_zone: req(o, "toZone")?,
            position: opt(o, "position")?,
            choice: opt(o, "choice")?,
        },
        "SHUFFLE" => I::Shuffle {
            player: req(o, "player")?,
        },
        "REVEAL" => I::Reveal {
            objects: req(o, "objects")?,
            to: opt(o, "to")?,
            choice: opt(o, "choice")?,
        },
        "LOOK_AT" => I::LookAt {
            objects: req(o, "objects")?,
            player: req(o, "player")?,
        },
        "PUT_ON_TOP_OR_BOTTOM" => I::PutOnTopOrBottom {
            objects: req(o, "objects")?,
            player: req(o, "player")?,
            choice: opt(o, "choice")?,
        },
        "EXILE_UNTIL" => I::ExileUntil {
            objects: req(o, "objects")?,
            duration: req(o, "duration")?,
            face_down: opt_bool(o, "faceDown")?,
        },
        "PLAY_PERMISSION_GRANT" => I::PlayPermissionGrant {
            objects: req(o, "objects")?,
            duration: req(o, "duration")?,
            who: req(o, "who")?,
            also_as: opt_string(o, "alsoAs")?,
        },
        // Damage and life
        "DEAL_DAMAGE" => I::DealDamage {
            source: req(o, "source")?,
            recipients: req(o, "recipients")?,
            amount: req(o, "amount")?,
            divided: opt_bool(o, "divided")?,
            choice: opt(o, "choice")?,
        },
        "FIGHT" => I::Fight {
            first: req(o, "first")?,
            second: req(o, "second")?,
        },
        "BITE" => I::Bite {
            source: req(o, "source")?,
            target: req(o, "target")?,
        },
        "GAIN_LIFE" => I::GainLife {
            player: req(o, "player")?,
            amount: req(o, "amount")?,
        },
        "LOSE_LIFE" => I::LoseLife {
            player: req(o, "player")?,
            amount: req(o, "amount")?,
        },
        "SET_LIFE" => I::SetLife {
            player: req(o, "player")?,
            amount: req(o, "amount")?,
        },
        "PREVENT" => I::Prevent {
            duration: req(o, "duration")?,
            source: opt(o, "source")?,
            recipients: opt(o, "recipients")?,
            amount: opt(o, "amount")?,
            combat_only: opt_bool(o, "combatOnly")?,
        },
        "DESTROY" => I::Destroy {
            objects: req(o, "objects")?,
            no_regeneration: opt_bool(o, "noRegeneration")?,
        },
        "SACRIFICE" => I::Sacrifice {
            player: req(o, "player")?,
            selector: req(o, "selector")?,
            count: req(o, "count")?,
            choice: opt(o, "choice")?,
        },
        "REGENERATE" => I::Regenerate {
            objects: req(o, "objects")?,
        },
        "COUNTER_SPELL" => I::CounterSpell {
            objects: req(o, "objects")?,
            unless: opt_costs(o, "unless")?,
        },
        // Counters and tokens
        "PUT_COUNTERS" => I::PutCounters {
            objects: req(o, "objects")?,
            kind: string(o, "kind")?,
            count: req(o, "count")?,
        },
        "REMOVE_COUNTERS" => I::RemoveCounters {
            objects: req(o, "objects")?,
            kind: string(o, "kind")?,
            count: req(o, "count")?,
        },
        "MOVE_COUNTERS" => I::MoveCounters {
            from: req(o, "from")?,
            to: req(o, "to")?,
            kind: string(o, "kind")?,
            count: req(o, "count")?,
        },
        "PROLIFERATE" => I::Proliferate {
            choice: opt(o, "choice")?,
        },
        "CREATE_TOKEN" => I::CreateToken {
            token: TokenSource::take_from(o)?,
            count: req(o, "count")?,
            modifiers: opt_opaque(o, "modifiers")?,
            controller: opt(o, "controller")?,
        },
        "CREATE_PREDEFINED_TOKEN" => I::CreatePredefinedToken {
            kind: string(o, "kind")?,
            count: opt(o, "count")?,
        },
        "CREATE_COPY_TOKEN" => I::CreateCopyToken {
            of: req(o, "of")?,
            except: opt_opaque(o, "except")?,
        },
        "POPULATE" => I::Populate,
        "AMASS" => I::Amass { n: req(o, "n")? },
        "INCUBATE" => I::Incubate { n: req(o, "n")? },
        // Stack
        "COPY_SPELL_OR_ABILITY" => I::CopySpellOrAbility {
            objects: req(o, "objects")?,
            new_targets: opt_bool(o, "newTargets")?,
        },
        "CHANGE_TARGETS" => I::ChangeTargets {
            objects: req(o, "objects")?,
            choice: opt(o, "choice")?,
        },
        "CAST_FROM" => I::CastFrom {
            objects: req(o, "objects")?,
            without_paying: opt_bool(o, "withoutPaying")?,
            alt_cost: opt_costs(o, "altCost")?,
        },
        // State and designations
        "TAP_OR_UNTAP" => I::TapOrUntap {
            objects: req(o, "objects")?,
            action: req(o, "action")?,
        },
        "TRANSFORM" => I::Transform {
            objects: req(o, "objects")?,
        },
        "CONVERT" => I::Convert {
            objects: req(o, "objects")?,
        },
        "TURN_FACE_UP" => I::TurnFaceUp {
            objects: req(o, "objects")?,
            profile: opt_string(o, "profile")?,
        },
        "TURN_FACE_DOWN" => I::TurnFaceDown {
            objects: req(o, "objects")?,
            profile: opt_string(o, "profile")?,
        },
        "PHASE_OUT" => I::PhaseOut {
            objects: req(o, "objects")?,
        },
        "ATTACH" => I::Attach {
            objects: req(o, "objects")?,
            to: req(o, "to")?,
        },
        "GAIN_CONTROL" => I::GainControl {
            objects: req(o, "objects")?,
            duration: req(o, "duration")?,
            player: opt(o, "player")?,
        },
        "EXCHANGE_CONTROL" => I::ExchangeControl {
            first: req(o, "first")?,
            second: req(o, "second")?,
        },
        "GOAD" => I::Goad {
            objects: req(o, "objects")?,
        },
        "SET_DESIGNATION" => I::SetDesignation {
            kind: string(o, "kind")?,
            value: req(o, "value")?,
            to: opt(o, "to")?,
        },
        "VENTURE" => I::Venture,
        "RING_TEMPTS" => I::RingTempts,
        "INCREASE_SPEED" => I::IncreaseSpeed {
            player: req(o, "player")?,
            amount: opt(o, "amount")?,
        },
        "EMPOWER_JACE" => I::EmpowerJace { n: req(o, "n")? },
        "MELD" => I::Meld {
            objects: req(o, "objects")?,
        },
        "MUTATE_MERGE" => I::MutateMerge {
            objects: req(o, "objects")?,
            onto: req(o, "onto")?,
        },
        "MANIFEST" => I::Manifest {
            objects: req(o, "objects")?,
        },
        "CLOAK" => I::Cloak {
            objects: req(o, "objects")?,
        },
        "EXPLORE" => I::Explore {
            objects: req(o, "objects")?,
        },
        "CONNIVE" => I::Connive {
            objects: req(o, "objects")?,
        },
        "DISCOVER" => I::Discover { n: req(o, "n")? },
        "BLIGHT" => I::Blight { n: req(o, "n")? },
        "WATERBEND" => I::Waterbend {
            cost: costs(o, "cost")?,
        },
        // Continuous effects. No sameAsLayer cross-check here (docs/DECISIONS.md "#14 FP-013 —
        // No lock-in cross-check inside CREATE_CONTINUOUS_EFFECT"); absent lockAffectedSet
        // means true (CR 611.2c).
        "CREATE_CONTINUOUS_EFFECT" => I::CreateContinuousEffect {
            effects: non_empty_vec(o.req("effects")?, "effects")?,
            duration: req(o, "duration")?,
            lock_affected_set: opt_bool(o, "lockAffectedSet")?,
        },
        "VOTE" => I::Vote {
            voters: req(o, "voters")?,
            choice: req(o, "choice")?,
            store: string(o, "store")?,
        },
        "REMEMBER" => I::Remember {
            key: string(o, "key")?,
            into: req(o, "into")?,
            source: RememberSource::take_from(o)?,
        },
        "NATIVE_EFFECT" => I::NativeEffect(NativeEffectAST::take_from(o)?),
        other => return Err(format!("unknown instruction op {other:?}")),
    })
}

impl FromJson for Instruction {
    fn from_json(v: &Value) -> JResult<Self> {
        let mut o = Obj::new(v, "instruction")?;
        let op = str_of(o.req("op")?, "op")?;
        let out = take_op(&mut o, op)?;
        o.finish()?;
        out.check()?;
        Ok(out)
    }
}
