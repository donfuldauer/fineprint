// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the ACTIVATE actions offered by the legal-action generator (REQ-DSL-013,
//! REQ-ARCH-002; CR 117.1b, 117.1d, 602.5, 602.5a, 602.5b, 602.5d, 602.5e, 605.3a, 307.5,
//! 101.2). Black-box: written from the work-item brief only. Every name is invented (R5).

#[path = "spec_actions_support.rs"]
mod support;

use fineprint_core::actions::*;
use fineprint_core::events::{Phase, Step};
use fineprint_dsl::ability::{ActivationTiming, RuleKind};
use serde_json::json;
use support::*;

/// A labelled situation for P1.
struct Sit {
    name: &'static str,
    own: bool,
    main: bool,
    combat: bool,
    empty: bool,
}

/// Every combination of own/opponent turn, phase and stack, with what it is.
fn situations() -> Vec<(Sit, FakeHost)> {
    let mut out = Vec::new();
    for own in [true, false] {
        for (phase, step, main, combat) in [
            (Phase::Beginning, Some(Step::Upkeep), false, false),
            (Phase::PrecombatMain, None, true, false),
            (Phase::Combat, Some(Step::DeclareAttackers), false, true),
            (Phase::Combat, Some(Step::EndOfCombat), false, true),
            (Phase::PostcombatMain, None, true, false),
            (Phase::Ending, Some(Step::End), false, false),
        ] {
            for empty in [true, false] {
                let h = FakeHost::new()
                    .active(Some(if own { P1 } else { P2 }))
                    .phase(phase, step)
                    .stack(empty);
                out.push((
                    Sit {
                        name: "situation",
                        own,
                        main,
                        combat,
                        empty,
                    },
                    h,
                ));
            }
        }
    }
    out
}

/// What a timing requires, per the brief (None: any priority).
fn timing_ok(t: Option<ActivationTiming>, s: &Sit) -> Result<(), TimingRequirement> {
    match t {
        None | Some(ActivationTiming::Instant) => Ok(()),
        Some(ActivationTiming::Sorcery) => {
            if s.own && s.main && s.empty {
                Ok(())
            } else {
                Err(TimingRequirement::SorceryTiming)
            }
        }
        Some(ActivationTiming::YourTurn) => {
            if s.own {
                Ok(())
            } else {
                Err(TimingRequirement::OwnTurn)
            }
        }
        Some(ActivationTiming::OpponentTurn) => {
            if s.own {
                Err(TimingRequirement::OpponentTurn)
            } else {
                Ok(())
            }
        }
        Some(ActivationTiming::CombatOnly) => {
            if s.combat {
                Ok(())
            } else {
                Err(TimingRequirement::Combat)
            }
        }
    }
}

fn all_timings() -> Vec<Option<ActivationTiming>> {
    let mut v = vec![None];
    v.extend(ActivationTiming::ALL.iter().copied().map(Some));
    v
}

/// REQ-DSL-013: every ActivationTiming, true and false, in every situation: offered exactly when
/// the timing holds, else the matching Timing reason; the basis is Rule "CR 117.1b"
/// (CR 117.1b, 602.5d, 602.5e, 307.5). CombatOnly holds on the opponent's combat too; Sorcery
/// needs an empty stack.
#[test]
fn req_dsl_013_activation_timing_matrix() {
    // CR 117.1b, CR 602.5d, CR 602.5e, CR 307.5
    for t in all_timings() {
        let a = AbilityCandidate {
            timing: t,
            ..ability(10, "test.ability_a")
        };
        let mut seen_true = false;
        let mut seen_false = false;
        for (s, mut h) in situations() {
            h.abilities.push(a.clone());
            let resp = ability_resp(&a);
            match timing_ok(t, &s) {
                Ok(()) => {
                    seen_true = true;
                    let basis = assert_legal(&h, P1, &resp);
                    assert!(
                        basis.contains(&Allowance::Rule { cr: "CR 117.1b" }),
                        "{t:?} {}: {basis:?}",
                        s.name
                    );
                }
                Err(req) => {
                    seen_false = true;
                    assert_illegal(&h, P1, &resp, &[Reason::Timing(req)]);
                }
            }
        }
        assert!(seen_true, "{t:?} never true");
        if !matches!(t, None | Some(ActivationTiming::Instant)) {
            assert!(seen_false, "{t:?} never false");
        }
    }
}

/// REQ-DSL-013: the edge cases called out by the brief: CombatOnly on the opponent's combat is
/// offered; Sorcery with a non-empty stack is not (CR 602.5d, 307.5).
#[test]
fn req_dsl_013_combat_only_on_opponents_combat_and_sorcery_needs_empty_stack() {
    // CR 602.5d, CR 307.5
    let combat = timed(10, "test.ability_c", ActivationTiming::CombatOnly);
    let mut h = FakeHost::new()
        .active(Some(P2))
        .phase(Phase::Combat, Some(Step::DeclareBlockers))
        .stack(false);
    h.abilities.push(combat.clone());
    assert_legal(&h, P1, &ability_resp(&combat));

    let sorcery = timed(11, "test.ability_s", ActivationTiming::Sorcery);
    let mut h = FakeHost::new().stack(false);
    h.abilities.push(sorcery.clone());
    assert_illegal(
        &h,
        P1,
        &ability_resp(&sorcery),
        &[Reason::Timing(TimingRequirement::SorceryTiming)],
    );
}

/// REQ-DSL-013: a mana ability with no stated timing is offered whenever the player has
/// priority; an explicit timing restriction still binds it (CR 117.1d, 605.3a, 605.1, 602.5d).
/// Its basis is Rule "CR 605.3a" when legal.
#[test]
fn req_dsl_013_mana_ability_ignores_timing() {
    // CR 117.1d, CR 605.3a, CR 605.1, CR 602.5d
    for t in all_timings() {
        let a = AbilityCandidate {
            timing: t,
            is_mana_ability: true,
            ..ability(10, "test.mana_ability")
        };
        for (s, mut h) in situations() {
            h.abilities.push(a.clone());
            let resp = ability_resp(&a);
            // No stated timing: always legal. Otherwise the explicit timing applies.
            let required = if t.is_none() {
                Ok(())
            } else {
                timing_ok(t, &s)
            };
            match required {
                Ok(()) => {
                    let basis = assert_legal(&h, P1, &resp);
                    assert!(
                        basis.contains(&Allowance::Rule { cr: "CR 605.3a" }),
                        "{t:?}: {basis:?}"
                    );
                }
                Err(req) => assert_illegal(&h, P1, &resp, &[Reason::Timing(req)]),
            }
        }
    }
}

/// REQ-DSL-013: activation limits: used up -> LimitReached, one short -> offered (CR 602.5b).
#[test]
fn req_dsl_013_activation_limits() {
    // CR 602.5b
    let cases = [
        // (per_turn, per_game, uses_turn, uses_game, offered)
        (Some(1), None, 0, 0, true),
        (Some(1), None, 1, 1, false),
        (Some(2), None, 1, 9, true),
        (Some(2), None, 2, 2, false),
        (Some(2), None, 3, 3, false),
        (None, Some(3), 0, 2, true),
        (None, Some(3), 0, 3, false),
        (None, Some(1), 0, 1, false),
        (Some(2), Some(5), 1, 4, true),
        (Some(2), Some(5), 2, 4, false),
        (Some(2), Some(5), 1, 5, false),
    ];
    for (pt, pg, ut, ug, ok) in cases {
        let a = AbilityCandidate {
            limits: Some(limit(pt, pg)),
            uses_this_turn: ut,
            uses_this_game: ug,
            ..ability(10, "test.ability_l")
        };
        let mut h = FakeHost::new();
        h.abilities.push(a.clone());
        if ok {
            assert_legal(&h, P1, &ability_resp(&a));
        } else {
            assert_illegal(&h, P1, &ability_resp(&a), &[Reason::LimitReached]);
        }
    }
    // No limits: uses never matter.
    let a = AbilityCandidate {
        uses_this_turn: 50,
        uses_this_game: 500,
        ..ability(10, "test.ability_l")
    };
    let mut h = FakeHost::new();
    h.abilities.push(a.clone());
    assert_legal(&h, P1, &ability_resp(&a));
}

/// REQ-DSL-013: a summoning-sick source can't activate a {T}-cost ability, but can activate one
/// without it (CR 602.5a).
#[test]
fn req_dsl_013_summoning_sickness() {
    // CR 602.5a
    let sick_tap = AbilityCandidate {
        summoning_sick: true,
        requires_tap_symbol: true,
        ..ability(10, "test.ability_t")
    };
    let sick_no_tap = AbilityCandidate {
        summoning_sick: true,
        ..ability(11, "test.ability_n")
    };
    let fresh_tap = AbilityCandidate {
        requires_tap_symbol: true,
        ..ability(12, "test.ability_t")
    };
    let mut h = FakeHost::new();
    h.abilities = vec![sick_tap.clone(), sick_no_tap.clone(), fresh_tap.clone()];
    assert_illegal(&h, P1, &ability_resp(&sick_tap), &[Reason::SummoningSick]);
    assert_legal(&h, P1, &ability_resp(&sick_no_tap));
    assert_legal(&h, P1, &ability_resp(&fresh_tap));
}

/// REQ-DSL-013: an unmet activation condition -> ConditionNotMet (CR 602.5).
#[test]
fn req_dsl_013_condition_not_met() {
    // CR 602.5
    let a = AbilityCandidate {
        only_if_met: false,
        ..ability(10, "test.ability_c")
    };
    let mut h = FakeHost::new();
    h.abilities.push(a.clone());
    assert_illegal(&h, P1, &ability_resp(&a), &[Reason::ConditionNotMet]);
}

/// REQ-ARCH-002, REQ-DSL-013: an ACTIVATE prohibition filtered by "object" forbids only that
/// source's abilities, beating nothing else; every failing check is listed, Prohibited first
/// (CR 602.5, 101.2).
#[test]
fn req_arch_002_activate_prohibition_by_object_and_reason_order() {
    // CR 602.5, CR 101.2
    let bad = AbilityCandidate {
        timing: Some(ActivationTiming::CombatOnly),
        limits: Some(limit(Some(1), None)),
        uses_this_turn: 1,
        requires_tap_symbol: true,
        summoning_sick: true,
        only_if_met: false,
        ..ability(10, "test.ability_x")
    };
    let good = ability(11, "test.ability_x");
    let mut h = FakeHost::new().with_rules(&[r(
        4,
        RuleKind::Prohibition,
        json!({"action": "ACTIVATE", "object": 10}),
        &[P1],
        1,
    )]);
    h.abilities = vec![bad.clone(), good.clone()];
    assert_illegal(
        &h,
        P1,
        &ability_resp(&bad),
        &[
            Reason::Prohibited { by: eid(4) },
            Reason::Timing(TimingRequirement::Combat),
            Reason::LimitReached,
            Reason::SummoningSick,
            Reason::ConditionNotMet,
        ],
    );
    assert_eq!(offered(&h, P1), vec![ability_resp(&good)]);
}

/// REQ-DSL-013: abilities are offered in ascending (object, ability_id) order (CR 117.1b).
#[test]
fn req_dsl_013_ability_order() {
    // CR 117.1b
    let mut h = FakeHost::new();
    h.abilities = vec![
        ability(20, "test.b"),
        ability(5, "test.z"),
        ability(20, "test.a"),
        ability(5, "test.a"),
    ];
    assert_eq!(
        offered(&h, P1),
        vec![
            ability_resp(&ability(5, "test.a")),
            ability_resp(&ability(5, "test.z")),
            ability_resp(&ability(20, "test.a")),
            ability_resp(&ability(20, "test.b")),
        ]
    );
}
