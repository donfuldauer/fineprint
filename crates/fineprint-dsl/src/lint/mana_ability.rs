// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `FPL-MANA-ABILITY` (REQ-ING-009): a declared `isManaAbility: true` must be consistent with
//! CR 605.1a (activated) and CR 605.1b (triggered). Only typed abilities are checked; `None` and
//! `false` are never findings, because this DSL baseline has no add-mana instruction, so "could
//! add mana" cannot be decided from structure (docs/DECISIONS.md, "#16 FP-015 — Mana-ability
//! check baseline").

use serde_json::Value;

use super::walk::{AbilityFacts, CardFacts};
use super::{Lint, LintId, Severity, push};
use crate::ability::{
    AbilityAST, ActivatedAbilityAST, CostAST, EventKind, ModeSetAST, TargetSlotAST, Trigger,
    TriggeredAbilityAST,
};
use crate::card::CardDefinitionDSL;
use crate::expr::Zone;

/// Instruction ops that move a card to or from a library wherever they appear (CR 605.1a).
const LIBRARY_OPS: [&str; 4] = ["SEARCH_LIBRARY", "MILL", "DRAW", "PUT_ON_TOP_OR_BOTTOM"];

/// Checks every typed ability that declares `isManaAbility: true`.
pub(super) fn check(_def: &CardDefinitionDSL, facts: &CardFacts<'_>, out: &mut Vec<Lint>) {
    for a in &facts.abilities {
        let Ok(typed) = &a.typed else { continue };
        if typed.base().is_mana_ability != Some(true) {
            continue;
        }
        let mut fail = |message: String| {
            push(
                out,
                LintId::MANA_ABILITY,
                Severity::Error,
                a.location(),
                message,
            );
        };
        match typed {
            AbilityAST::Activated(act) => activated(act, a, &mut fail),
            AbilityAST::Triggered(trig) => triggered(trig, &mut fail),
            // Keyword macros expand later; their abilities are checked after expansion.
            AbilityAST::Keyword(_) => {}
            // CR 605.1: only activated and triggered abilities can be mana abilities.
            other => fail(format!(
                "isManaAbility is true on a {} ability; only activated and triggered abilities \
                 can be mana abilities (CR 605.1)",
                other.kind()
            )),
        }
    }
}

/// Whether the ability requires a target: any target slot, top-level or in a mode
/// (CR 115.6: it counts even when zero targets may be chosen).
fn requires_target(targets: Option<&Vec<TargetSlotAST>>, modes: Option<&ModeSetAST>) -> bool {
    targets.is_some_and(|t| !t.is_empty())
        || modes.is_some_and(|m| {
            m.modes
                .iter()
                .any(|mode| mode.targets.as_ref().is_some_and(|t| !t.is_empty()))
        })
}

/// Whether the ability has no instruction at all (top-level `effect` and every mode's).
fn no_effect<T>(effect: &[T], modes: Option<&ModeSetAST>) -> bool {
    effect.is_empty() && modes.is_none_or(|m| m.modes.iter().all(|mode| mode.effect.is_empty()))
}

/// Whether a cost node (COMPOSITE included) satisfies `pred`, walked iteratively.
fn any_cost(costs: &[CostAST], pred: impl Fn(&CostAST) -> bool) -> bool {
    let mut stack: Vec<&CostAST> = costs.iter().collect();
    while let Some(c) = stack.pop() {
        if pred(c) {
            return true;
        }
        if let CostAST::Composite { costs } = c {
            stack.extend(costs.iter());
        }
    }
    false
}

/// Whether the raw JSON under `roots` holds an instruction that moves a card to or from a
/// library: an op of [`LIBRARY_OPS`], or a `MOVE` whose `toZone` is `LIBRARY` or whose
/// `objects` selector names the `LIBRARY` zone (CR 605.1a).
fn moves_library_card(roots: &[Option<&Value>]) -> bool {
    let mut stack: Vec<&Value> = roots.iter().flatten().copied().collect();
    while let Some(v) = stack.pop() {
        match v {
            Value::Object(map) => {
                match map.get("op").and_then(Value::as_str) {
                    Some(op) if LIBRARY_OPS.contains(&op) => return true,
                    Some("MOVE")
                        if map.get("toZone").and_then(Value::as_str)
                            == Some(Zone::Library.as_str())
                            || map.get("objects").is_some_and(names_library_zone) =>
                    {
                        return true;
                    }
                    _ => {}
                }
                stack.extend(map.values());
            }
            Value::Array(items) => stack.extend(items.iter()),
            _ => {}
        }
    }
    false
}

/// Whether a selector (raw JSON) lists `LIBRARY` in a `zones` member at any depth, i.e. its
/// objects are taken from a library.
fn names_library_zone(selector: &Value) -> bool {
    let mut stack = vec![selector];
    while let Some(v) = stack.pop() {
        match v {
            Value::Object(map) => {
                if map
                    .get("zones")
                    .and_then(Value::as_array)
                    .is_some_and(|zones| {
                        zones
                            .iter()
                            .any(|z| z.as_str() == Some(Zone::Library.as_str()))
                    })
                {
                    return true;
                }
                stack.extend(map.values());
            }
            Value::Array(items) => stack.extend(items.iter()),
            _ => {}
        }
    }
    false
}

/// CR 605.1a criteria for an activated ability declared a mana ability.
fn activated(act: &ActivatedAbilityAST, a: &AbilityFacts<'_>, fail: &mut impl FnMut(String)) {
    // CR 605.1a, CR 115.6: it doesn't require a target.
    if requires_target(act.targets.as_ref(), act.modes.as_ref()) {
        fail("isManaAbility is true but the ability requires a target (CR 605.1a)".to_owned());
    }
    // CR 605.1a: it's not a loyalty ability.
    if act.is_loyalty == Some(true) || any_cost(&act.cost, |c| matches!(c, CostAST::Loyalty { .. }))
    {
        fail("isManaAbility is true but the ability is a loyalty ability (CR 605.1a)".to_owned());
    }
    // CR 605.1a: its cost and effect don't move any card to or from a library.
    let library_cost = any_cost(&act.cost, |c| {
        matches!(
            c,
            CostAST::ExileFromZone {
                zone: Zone::Library,
                ..
            }
        )
    });
    if library_cost || moves_library_card(&[a.raw.get("effect"), a.raw.get("modes")]) {
        fail(
            "isManaAbility is true but its cost or effect moves a card to or from a library \
             (CR 605.1a)"
                .to_owned(),
        );
    }
    // CR 605.1a: it could add mana; with no instruction at all it cannot.
    if no_effect(&act.effect, act.modes.as_ref()) {
        fail(
            "isManaAbility is true but the ability has no effect to add mana (CR 605.1a)"
                .to_owned(),
        );
    }
}

/// CR 605.1b criteria for a triggered ability declared a mana ability. The loyalty and library
/// criteria of CR 605.1a do not apply.
fn triggered(trig: &TriggeredAbilityAST, fail: &mut impl FnMut(String)) {
    // CR 605.1b, CR 115.6: it doesn't require a target.
    if requires_target(trig.targets.as_ref(), trig.modes.as_ref()) {
        fail("isManaAbility is true but the ability requires a target (CR 605.1b)".to_owned());
    }
    // CR 605.1b: it triggers from the activation or resolution of an activated mana ability.
    // The event set has no "mana added" event, so only ACTIVATED and RESOLVED qualify.
    let from_activation = matches!(
        &trig.trigger,
        Trigger::Event(m) if matches!(m.kind, EventKind::Activated | EventKind::Resolved)
    );
    if !from_activation {
        fail(
            "isManaAbility is true but the trigger is not an ACTIVATED or RESOLVED event \
             (CR 605.1b)"
                .to_owned(),
        );
    }
    // CR 605.1b: it could add mana; with no instruction at all it cannot.
    if no_effect(&trig.effect, trig.modes.as_ref()) {
        fail(
            "isManaAbility is true but the ability has no effect to add mana (CR 605.1b)"
                .to_owned(),
        );
    }
}
