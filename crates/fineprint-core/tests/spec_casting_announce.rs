// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the casting and activation procedure, steps 0-5 (announce): proposal on the
//! stack, begin check, modes, costs and X, targets, division and the legality check, each
//! failure rolling back to the identical state (REQ-CAST-001, REQ-NET-003, REQ-CORE-005,
//! REQ-DSL-013, REQ-TEST-001; CR 601.2, 601.2a-e, 601.3, 601.3a, 602.2, 734.1, 734.2, 115.3,
//! 115.5, 115.6, 101.2, 702.11b-e, 702.16b/j/k, 702.18a, 700.2a, 700.2d, 113.7a).
//!
//! Black-box: written from the work-item brief only, against the pinned public API of
//! `fineprint_core::casting`, driven through `ChoiceSession` with a procedure that calls
//! `announce` directly on the run's state. Synthetic objects and neutral labels only (R5).

#[path = "spec_casting_announce_support.rs"]
mod support;

use fineprint_core::casting::*;
use fineprint_core::choice::{
    ChoiceError, ChoicePayload, ChoiceResponse, ChoiceTarget, ChoiceType, PendingChoiceRequest,
    RequestId, Status,
};
use fineprint_core::txn::Transactional;
use fineprint_dsl::ability::RuleKind;
use serde_json::json;
use support::*;

// ---------------------------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------------------------

/// A prohibition on `req`'s action that the begin check lets through (601.3a leniency) and
/// step 5 applies.
fn late_prohibition(id: u64, req: &CastRequest) -> R {
    prohibition(
        id,
        json!({"action": action_of(req), "test.could_lapse": true, "test.applies": true}),
        &[req.caster],
        1,
    )
}

/// The full four-ask setup: two modes (mode 0 adds a two-target slot), X in 0..=5, division 3.
fn four_ask_fake() -> Fake {
    Fake {
        mode_spec: Some(modes_spec(2, 1, 1, false)),
        mode_slots: vec![
            vec![slot(1, 2, &[to(10), to(11)])],
            vec![slot(1, 1, &[to(12)])],
        ],
        cost_spec: x_costs(0, 5),
        division: Some(3),
        ..Fake::new()
    }
}

fn four_answers() -> Vec<ChoiceResponse> {
    vec![
        select_modes(&[0]),
        with_x(1),
        select_targets(&[&[to(10), to(11)]]),
        submit_division(&[(to(10), 2), (to(11), 1)]),
    ]
}

// ---------------------------------------------------------------------------------------------
// 1. Rollback hash identity
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001 (step 0/2), REQ-CORE-005; CR 601.2, 700.2a, 734.1, 734.2: no legal mode rolls
/// back to the identical hash with priority retained, for a spell and an ability.
#[test]
fn rollback_no_legal_mode_restores_hash_and_priority() {
    for priority in [Some(P1), Some(P2), None] {
        for (label, req) in both_kinds(P1) {
            let base = Fake {
                priority,
                mode_spec: Some(modes_spec(2, 1, 1, false)),
                mode_slots: vec![vec![slot(1, 1, &[to(10)])], vec![slot(1, 1, &[to(11)])]],
                ..Fake::new()
            }
            .with_rules(&[tr(1, shroud_obj(10), 1), tr(2, shroud_obj(11), 2)]);
            let run = drive(&base, &req, &[]);
            assert!(run.requests.is_empty(), "{label}: no mode request expected");
            assert_rolled_back(&run, &base, CastStep::Choices, StopReason::NoLegalMode);
        }
    }
}

/// REQ-CAST-001 (step 3), REQ-CORE-005; CR 601.2, 601.2c, 734.1, 734.2: a mandatory slot
/// without enough legal candidates rolls back (slot index reported), spell and ability.
#[test]
fn rollback_missing_target_restores_hash_and_priority() {
    for (label, req) in both_kinds(P1) {
        // Slot 0 is optional and fine; slot 1 needs two targets but only one is legal.
        let base = Fake {
            base_slots: vec![slot(0, 1, &[to(11)]), slot(2, 2, &[to(10), to(12)])],
            ..Fake::new()
        }
        .with_rules(&[tr(1, shroud_obj(10), 1)]);
        let run = drive(&base, &req, &[]);
        assert!(
            run.requests.is_empty(),
            "{label}: no target request expected"
        );
        assert_rolled_back(
            &run,
            &base,
            CastStep::Targets,
            StopReason::MissingTarget { slot: 1 },
        );
    }
}

/// REQ-CAST-001 (step 3), CR 601.2c, 115.5: a slot whose only candidate is the spell or
/// ability itself has no legal target.
#[test]
fn rollback_missing_target_when_only_candidate_is_itself() {
    for (label, req) in both_kinds(P1) {
        let base = Fake::one_slot(1, 1, &[to(FIRST_STACK_ID)]);
        let run = drive(&base, &req, &[]);
        assert!(run.requests.is_empty(), "{label}");
        assert_rolled_back(
            &run,
            &base,
            CastStep::Targets,
            StopReason::MissingTarget { slot: 0 },
        );
    }
}

/// REQ-CAST-001 (step 4), REQ-CORE-005; CR 601.2d, 734.1, 734.2: a total smaller than the
/// number of targets cannot be divided; rollback after the target ask.
#[test]
fn rollback_division_impossible_restores_hash_and_priority() {
    for (label, req) in both_kinds(P1) {
        let base = Fake {
            division: Some(1),
            ..Fake::one_slot(1, 2, &[to(10), to(11)])
        };
        let run = drive(&base, &req, &[select_targets(&[&[to(10), to(11)]])]);
        assert_eq!(run.requests.len(), 1, "{label}: only the target request");
        assert_rolled_back(
            &run,
            &base,
            CastStep::Division,
            StopReason::DivisionImpossible {
                total: 1,
                recipients: 2,
            },
        );
    }
}

/// REQ-CAST-001 (step 5), REQ-CORE-005; CR 601.2e, 601.3a, 734.1, 734.2: a prohibition that
/// applies to the completed proposal rolls back after every ask was answered.
#[test]
fn rollback_proposal_prohibited_restores_hash_and_priority() {
    for (label, req) in both_kinds(P1) {
        let base = four_ask_fake().with_rules(&[late_prohibition(7, &req)]);
        let run = drive(&base, &req, &four_answers());
        assert_eq!(run.requests.len(), 4, "{label}");
        assert_rolled_back(
            &run,
            &base,
            CastStep::Legality,
            StopReason::ProposalProhibited { by: eid(7) },
        );
    }
}

/// REQ-CAST-001 (step 5), REQ-CORE-005; CR 601.2e, 734.1, 734.2: an illegal combination
/// rolls back to the identical state.
#[test]
fn rollback_combination_illegal_restores_hash_and_priority() {
    for (label, req) in both_kinds(P1) {
        let base = Fake {
            combination_legal: false,
            ..four_ask_fake()
        };
        let run = drive(&base, &req, &four_answers());
        assert_eq!(run.requests.len(), 4, "{label}");
        assert_rolled_back(
            &run,
            &base,
            CastStep::Legality,
            StopReason::CombinationIllegal,
        );
    }
}

/// REQ-CORE-005; CR 734.1: a rollback restores the stack exactly even when the stack was not
/// empty before, and with a non-default priority holder (CR 734.2).
#[test]
fn rollback_restores_a_non_empty_stack() {
    for (_, req) in both_kinds(P2) {
        let mut base = Fake {
            priority: Some(P2),
            combination_legal: false,
            marker: 41,
            ..Fake::one_slot(0, 1, &[to(10)])
        };
        base.stack = vec![oid(90), oid(91)];
        let run = drive(&base, &req, &[select_targets(&[&[to(10)]])]);
        assert_rolled_back(
            &run,
            &base,
            CastStep::Legality,
            StopReason::CombinationIllegal,
        );
    }
}

/// REQ-CAST-001, CR 601.2a, 602.2a: a successful announce leaves the new object on top of the
/// stack (the contrast case for the rollback tests: the change is observable in the hash).
#[test]
fn announced_leaves_the_stack_object_and_changes_the_hash() {
    for (_, req) in both_kinds(P1) {
        let base = four_ask_fake();
        let run = drive(&base, &req, &four_answers());
        let (so, _) = announced(&run, &base);
        assert_eq!(so, oid(FIRST_STACK_ID));
        assert_ne!(run.final_hash, base.state_hash());
        assert_eq!(run.observed.after, run.final_hash);
    }
}

// ---------------------------------------------------------------------------------------------
// 2. Begin check
// ---------------------------------------------------------------------------------------------

/// REQ-CAST-001 (step 1), REQ-DSL-013; CR 601.3, 602.2: a matching prohibition stops the cast
/// before anything happens: no request, state untouched.
#[test]
fn begin_prohibition_not_started() {
    for (label, req) in both_kinds(P1) {
        let base = four_ask_fake().with_rules(&[prohibition(
            3,
            json!({"action": action_of(&req)}),
            &[P1],
            1,
        )]);
        let run = drive(&base, &req, &[]);
        assert_not_started(&run, &base, eid(3));
        let _ = label;
    }
}

/// REQ-CAST-001 (step 1); CR 601.3: "CAST" does not stop an activation and "ACTIVATE" does not
/// stop a cast; a prohibition for another player does not stop the caster.
#[test]
fn begin_prohibition_other_action_or_player_does_not_apply() {
    let cases = [
        (spell(P1), json!({"action": "ACTIVATE"}), vec![P1]),
        (ability(P1), json!({"action": "CAST"}), vec![P1]),
        (spell(P1), json!({"action": "CAST"}), vec![P2]),
        (ability(P1), json!({"action": "ACTIVATE"}), vec![P2]),
        (spell(P1), json!({"action": "PLAY_LAND"}), vec![P1]),
    ];
    for (req, params, players) in cases {
        let base = Fake::one_slot(1, 1, &[to(10)]).with_rules(&[prohibition(
            3,
            params.clone(),
            &players,
            1,
        )]);
        let run = drive(&base, &req, &[select_targets(&[&[to(10)]])]);
        let (_, p) = announced(&run, &base);
        assert_eq!(p.targets, vec![vec![to(10)]], "{params}");
    }
}

/// REQ-CAST-001 (step 1), R12; CR 601.3: the "object" filter is the raw id of the request's
/// object; a non-matching or malformed "object", a malformed "zone" or a non-string "action"
/// make the entry match nothing.
#[test]
fn begin_prohibition_filters() {
    for (label, req) in both_kinds(P1) {
        let action = action_of(&req);
        let raw = req.object.get();
        let stopping = [
            json!({"action": action, "object": raw}),
            json!({"action": action, "test.unrelated": 1}),
        ];
        for params in stopping {
            let base = Fake::one_slot(1, 1, &[to(10)]).with_rules(&[prohibition(
                3,
                params.clone(),
                &[P1],
                1,
            )]);
            assert_not_started(&drive(&base, &req, &[]), &base, eid(3));
        }
        let not_stopping = [
            json!({"action": action, "object": raw + 1}),
            json!({"action": action, "object": raw.to_string()}),
            json!({"action": action, "object": -1}),
            json!({"action": action, "object": 5.5}),
            json!({"action": action, "zone": 7}),
            json!({"action": action, "zone": "test.no_such_zone"}),
            json!({"action": 1}),
            json!({"action": [action]}),
            json!({"action": null}),
            json!({"action": action.to_lowercase()}),
            json!({}),
        ];
        for params in not_stopping {
            let base = Fake::one_slot(1, 1, &[to(10)]).with_rules(&[prohibition(
                3,
                params.clone(),
                &[P1],
                1,
            )]);
            let run = drive(&base, &req, &[select_targets(&[&[to(10)]])]);
            let _ = announced(&run, &base);
            let _ = (label, params);
        }
    }
}

/// REQ-CAST-001 (step 1); CR 601.3a: a prohibition that a proposal choice could lift lets the
/// cast begin; with no "test.applies" it then does not apply at step 5 either.
#[test]
fn begin_prohibition_could_lapse_begins() {
    for (_, req) in both_kinds(P1) {
        let base = Fake::one_slot(1, 1, &[to(10)]).with_rules(&[prohibition(
            3,
            json!({"action": action_of(&req), "test.could_lapse": true}),
            &[P1],
            1,
        )]);
        let run = drive(&base, &req, &[select_targets(&[&[to(10)]])]);
        let _ = announced(&run, &base);
    }
}

/// REQ-CAST-001 (steps 1 and 5), REQ-CORE-005; CR 601.3a (its example, with neutral data),
/// 601.2e, 734.2: "can't cast with an even mana value" against a cost with X: the cast may
/// begin; X = 2 (even) is rolled back at the legality check to the identical hash with priority
/// retained; X = 1 (odd) is announced. Spell ("CAST") and ability ("ACTIVATE").
#[test]
fn begin_even_mana_value_scenario() {
    for (label, req) in both_kinds(P1) {
        let base = Fake {
            cost_spec: x_costs(0, 9),
            ..Fake::new()
        }
        .with_rules(&[prohibition(
            8,
            json!({"action": action_of(&req), "test.parity": "even"}),
            &[P1],
            1,
        )]);
        // Begin allowed: the first request is the cost/X ask.
        let first = start(&base, &req);
        let p = first.pending().expect("the cast begins");
        assert_eq!(p.choice_type, ChoiceType::ChooseCostsAndX, "{label}");

        let even = drive(&base, &req, &[with_x(2)]);
        assert_rolled_back(
            &even,
            &base,
            CastStep::Legality,
            StopReason::ProposalProhibited { by: eid(8) },
        );
        let odd = drive(&base, &req, &[with_x(1)]);
        let (_, proposal) = announced(&odd, &base);
        assert_eq!(proposal.costs.x, Some(1));
    }
}

/// REQ-CAST-001 (step 5); CR 601.2e: step 5 has no 601.3a leniency: whatever could_lapse
/// says, an applicable prohibition rolls back.
#[test]
fn step5_has_no_leniency() {
    for (_, req) in both_kinds(P1) {
        let base = Fake::one_slot(0, 1, &[to(10)]).with_rules(&[late_prohibition(4, &req)]);
        let run = drive(&base, &req, &[select_targets(&[&[]])]);
        assert_rolled_back(
            &run,
            &base,
            CastStep::Legality,
            StopReason::ProposalProhibited { by: eid(4) },
        );
    }
}

/// REQ-CAST-001 (step 1), REQ-DSL-013; CR 101.2, 601.3: a prohibition beats a permission for
/// the same action whatever the timestamp order.
#[test]
fn begin_prohibition_beats_permission_both_orders() {
    for (_, req) in both_kinds(P1) {
        let action = action_of(&req);
        for (p_ts, n_ts) in [(1, 2), (2, 1)] {
            let base = Fake::one_slot(1, 1, &[to(10)]).with_rules(&[
                r(
                    2,
                    RuleKind::CastPermission,
                    json!({"action": action}),
                    &[P1],
                    p_ts,
                ),
                prohibition(3, json!({"action": action}), &[P1], n_ts),
            ]);
            assert_not_started(&drive(&base, &req, &[]), &base, eid(3));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 3. Targeting
// ---------------------------------------------------------------------------------------------

const CANDS: [u64; 3] = [10, 11, 12];

fn cands() -> Vec<ChoiceTarget> {
    CANDS.iter().map(|&n| to(n)).collect()
}

fn without(n: u64) -> Vec<ChoiceTarget> {
    CANDS.iter().filter(|&&m| m != n).map(|&m| to(m)).collect()
}

/// One optional slot over objects 10..=12 (all controlled by P2 unless stated).
fn tfake(items: &[R]) -> Fake {
    Fake::one_slot(0, 3, &cands()).with_rules(items)
}

/// REQ-CAST-001 (step 3); CR 702.11b: hexproof on an object hides it from an opponent's spell
/// or ability but not from its controller's.
#[test]
fn targeting_hexproof_object() {
    for caster in [P1, P2] {
        for (label, req) in both_kinds(caster) {
            let f = tfake(&[tr(1, hexproof_obj(10), 1)]);
            let expected = if caster == P2 { cands() } else { without(10) };
            assert_eq!(legal(&f, &req), expected, "{label} by {caster:?}");
        }
    }
}

/// REQ-CAST-001 (step 3); CR 702.11c: hexproof on a player hides that player from opponents
/// only.
#[test]
fn targeting_hexproof_player() {
    for caster in [P1, P2] {
        for (label, req) in both_kinds(caster) {
            let f =
                Fake::one_slot(0, 2, &[tp(1), tp(2)]).with_rules(&[tr(1, hexproof_player(2), 1)]);
            let expected = if caster == P2 {
                vec![tp(1), tp(2)]
            } else {
                vec![tp(1)]
            };
            assert_eq!(legal(&f, &req), expected, "{label} by {caster:?}");
        }
    }
}

/// REQ-CAST-001 (step 3); CR 702.18a: shroud hides an object or player from everyone,
/// including its controller.
#[test]
fn targeting_shroud() {
    for caster in [P1, P2] {
        for (label, req) in both_kinds(caster) {
            let f = tfake(&[tr(1, shroud_obj(11), 1)]);
            assert_eq!(legal(&f, &req), without(11), "{label} by {caster:?}");
            let f = Fake::one_slot(0, 2, &[tp(1), tp(2)]).with_rules(&[tr(1, shroud_player(2), 1)]);
            assert_eq!(legal(&f, &req), vec![tp(1)], "{label} by {caster:?}");
        }
    }
}

/// REQ-CAST-001 (step 3); CR 702.16b, 113.7a: protection from a quality hides the target only
/// from a spell, or an ability's source, with that quality; for both casters.
#[test]
fn targeting_protection_from_quality() {
    for caster in [P1, P2] {
        for (label, req) in both_kinds(caster) {
            let base = tfake(&[tr(1, protection_obj(12, QUALITY_A), 1)]);
            assert_eq!(legal(&base, &req), cands(), "{label}: no quality");
            let with_other = base.clone().with_qualities(&[QUALITY_B]);
            assert_eq!(legal(&with_other, &req), cands(), "{label}: other quality");
            let with_q = base.with_qualities(&[QUALITY_A]);
            assert_eq!(legal(&with_q, &req), without(12), "{label}: has quality");
        }
    }
}

/// REQ-CAST-001 (step 3); CR 702.16j: protection from everything (no quality) hides the target
/// from every spell and ability.
#[test]
fn targeting_protection_from_everything() {
    for caster in [P1, P2] {
        for (label, req) in both_kinds(caster) {
            for qs in [&[][..], &[QUALITY_A][..]] {
                let f = tfake(&[tr(1, protection_all_obj(10), 1)]).with_qualities(qs);
                assert_eq!(legal(&f, &req), without(10), "{label} by {caster:?}");
            }
        }
    }
}

/// REQ-CAST-001 (step 3); CR 702.16k: protection from a player hides the target only from
/// spells and abilities that player controls.
#[test]
fn targeting_protection_from_player() {
    for caster in [P1, P2] {
        for (label, req) in both_kinds(caster) {
            let f = tfake(&[tr(1, protection_from_player_obj(11, 1), 1)]);
            let expected = if caster == P1 { without(11) } else { cands() };
            assert_eq!(legal(&f, &req), expected, "{label} by {caster:?}");
        }
    }
}

/// REQ-CAST-001 (step 3); CR 702.11d: hexproof from a quality hides the target only from
/// opponents' spells/abilities whose source has the quality.
#[test]
fn targeting_hexproof_from_quality() {
    for caster in [P1, P2] {
        for (label, req) in both_kinds(caster) {
            for (qs, has) in [(&[][..], false), (&[QUALITY_A][..], true)] {
                let f = tfake(&[tr(1, hexproof_from_obj(10, QUALITY_A), 1)]).with_qualities(qs);
                let hidden = has && caster == P1;
                let expected = if hidden { without(10) } else { cands() };
                assert_eq!(
                    legal(&f, &req),
                    expected,
                    "{label} by {caster:?} quality={has}"
                );
            }
        }
    }
}

/// REQ-CAST-001 (step 3); CR 702.11e, 101.2: "as though it didn't have hexproof" (for the
/// caster) lifts hexproof and hexproof-from, but not shroud, protection or another "can't be
/// the target" effect; it does nothing for a player outside its `players` set or for another
/// target.
#[test]
fn targeting_ignore_hexproof() {
    for (label, req) in both_kinds(P1) {
        let f = |items: &[R]| tfake(items).with_qualities(&[QUALITY_A]);
        let ignore = tr_for(9, ignore_hexproof_obj(10), &[P1], 5);
        // Lifted.
        for prot in [hexproof_obj(10), hexproof_from_obj(10, QUALITY_A)] {
            assert_eq!(
                legal(&f(&[tr(1, prot.clone(), 1)]), &req),
                without(10),
                "{label}: precondition {prot}"
            );
            assert_eq!(
                legal(&f(&[tr(1, prot.clone(), 1), ignore.clone()]), &req),
                cands(),
                "{label}: lifted {prot}"
            );
            // Not the caster's permission.
            let foreign = tr_for(9, ignore_hexproof_obj(10), &[P2], 5);
            assert_eq!(
                legal(&f(&[tr(1, prot.clone(), 1), foreign]), &req),
                without(10),
                "{label}: foreign permission {prot}"
            );
            // Another target.
            let elsewhere = tr_for(9, ignore_hexproof_obj(11), &[P1], 5);
            assert_eq!(
                legal(&f(&[tr(1, prot.clone(), 1), elsewhere]), &req),
                without(10),
                "{label}: permission for another object {prot}"
            );
        }
        // Not lifted.
        for prot in [
            shroud_obj(10),
            protection_obj(10, QUALITY_A),
            protection_all_obj(10),
            protection_from_player_obj(10, 1),
            other_obj(10),
        ] {
            assert_eq!(
                legal(&f(&[tr(1, prot.clone(), 1), ignore.clone()]), &req),
                without(10),
                "{label}: not lifted {prot}"
            );
        }
        // Hexproof and shroud together: shroud still wins.
        assert_eq!(
            legal(
                &f(&[tr(1, hexproof_obj(10), 1), tr(2, shroud_obj(10), 2), ignore]),
                &req
            ),
            without(10),
            "{label}: hexproof + shroud"
        );
    }
}

/// REQ-CAST-001 (step 3); CR 101.2, 702.11e: a "can't" beats the permission in both timestamp
/// orders; the permission's effect on hexproof does not depend on the order either.
#[test]
fn targeting_prohibition_beats_permission_both_orders() {
    for (label, req) in both_kinds(P1) {
        for (a, b) in [(1, 2), (2, 1)] {
            for prot in [shroud_obj(10), protection_all_obj(10), other_obj(10)] {
                let f = tfake(&[
                    tr_for(9, ignore_hexproof_obj(10), &[P1], a),
                    tr(1, prot.clone(), b),
                ]);
                assert_eq!(legal(&f, &req), without(10), "{label} {prot} order {a}/{b}");
            }
            let f = tfake(&[
                tr_for(9, ignore_hexproof_obj(10), &[P1], a),
                tr(1, hexproof_obj(10), b),
            ]);
            assert_eq!(legal(&f, &req), cands(), "{label} hexproof order {a}/{b}");
        }
    }
}

/// REQ-CAST-001 (step 3); CR 101.2: an "OTHER" can't-be-targeted effect hides from everyone by
/// default ("sources" ANY); "sources": "OPPONENTS" restricts it to opponents.
#[test]
fn targeting_other_and_sources() {
    for caster in [P1, P2] {
        for (label, req) in both_kinds(caster) {
            assert_eq!(
                legal(&tfake(&[tr(1, other_obj(10), 1)]), &req),
                without(10),
                "{label} by {caster:?}"
            );
            let opp = json!({"keyword": "OTHER", "target_object": 10, "sources": "OPPONENTS"});
            let expected = if caster == P1 { without(10) } else { cands() };
            assert_eq!(
                legal(&tfake(&[tr(1, opp, 1)]), &req),
                expected,
                "{label} by {caster:?}"
            );
            // HEXPROOF with the default "ANY" sources has no controller test.
            let any = json!({"keyword": "HEXPROOF", "target_object": 10});
            assert_eq!(
                legal(&tfake(&[tr(1, any, 1)]), &req),
                without(10),
                "{label} by {caster:?}"
            );
        }
    }
}

/// REQ-CAST-001 (step 3): the entry's `players` set does not scope PROHIBIT targeting entries.
#[test]
fn targeting_players_set_is_not_scope() {
    for (label, req) in both_kinds(P1) {
        for players in [&[P1][..], &[P2][..], &[P1, P2][..]] {
            let f = tfake(&[tr_for(1, shroud_obj(10), players, 1)]);
            assert_eq!(legal(&f, &req), without(10), "{label} players {players:?}");
        }
    }
}

/// REQ-CAST-001 (step 3), R12: a malformed targeting entry (unknown or missing keyword, unknown
/// effect or sources, wrong types, neither or both target keys) matches nothing.
#[test]
fn targeting_malformed_entries_match_nothing() {
    let malformed = [
        json!({}),
        json!({"target_object": 10}),
        json!({"keyword": "WARD", "target_object": 10}),
        json!({"keyword": "shroud", "target_object": 10}),
        json!({"keyword": 1, "target_object": 10}),
        json!({"keyword": ["SHROUD"], "target_object": 10}),
        json!({"keyword": "SHROUD"}),
        json!({"keyword": "SHROUD", "target_object": 10, "target_player": 1}),
        json!({"keyword": "SHROUD", "target_object": "10"}),
        json!({"keyword": "SHROUD", "target_object": -10}),
        json!({"keyword": "SHROUD", "target_object": 10.0}),
        json!({"keyword": "SHROUD", "target_object": null}),
        json!({"keyword": "SHROUD", "target_object": 10, "effect": "BOGUS"}),
        json!({"keyword": "SHROUD", "target_object": 10, "effect": 0}),
        json!({"keyword": "SHROUD", "target_object": 10, "sources": "BOGUS"}),
        json!({"keyword": "SHROUD", "target_object": 10, "sources": true}),
        json!({"keyword": "SHROUD", "target_object": 10, "quality": 5}),
        json!({"keyword": "SHROUD", "target_object": 10, "source_player": "1"}),
        json!({"keyword": "SHROUD", "target_object": 10, "source_player": -1}),
        json!([1, 2, 3]),
        json!("SHROUD"),
    ];
    for (label, req) in both_kinds(P1) {
        for params in &malformed {
            let f = tfake(&[tr(1, params.clone(), 1)]).with_qualities(&[QUALITY_A]);
            assert_eq!(legal(&f, &req), cands(), "{label}: {params}");
        }
    }
}

/// REQ-CAST-001 (step 3), REQ-NET-003; CR 601.2c, 115.5: the spell or ability itself is never
/// a legal target, and the request carries the slot as pinned: `TargetSlot{min, max, legal,
/// current: []}`, `may_keep_illegal: false`.
#[test]
fn targeting_request_shape_and_self_exclusion() {
    for (label, req) in both_kinds(P1) {
        let f = Fake {
            base_slots: vec![
                slot(1, 2, &[to(10), to(FIRST_STACK_ID)]),
                slot(0, 1, &[tp(1), to(FIRST_STACK_ID), to(11)]),
            ],
            ..Fake::new()
        };
        let s = start(&f, &req);
        let p = s.pending().expect("target request");
        assert_eq!(p.choice_type, ChoiceType::ChooseTargets);
        let ChoicePayload::ChooseTargets {
            slots,
            may_keep_illegal,
        } = &p.payload
        else {
            panic!("{label}: {:?}", p.payload);
        };
        assert!(!may_keep_illegal, "{label}");
        // (spec min, spec max, legal); `max` is the spec's or that clamped to the legal count
        // (docs/DECISIONS.md "#52 FP-050 tester — TargetSlot max").
        let expected = [(1, 2, vec![to(10)]), (0, 1, vec![tp(1), to(11)])];
        assert_eq!(slots.len(), expected.len(), "{label}");
        for (got, (min, max, legal)) in slots.iter().zip(expected) {
            let clamped = max.min(legal.len() as u64);
            assert_eq!(got.min, min, "{label}");
            assert!(
                got.max == max || got.max == clamped,
                "{label}: max {}",
                got.max
            );
            assert_eq!(got.legal, legal, "{label}");
            assert_eq!(got.current, Vec::<ChoiceTarget>::new(), "{label}");
        }
        // Answering with the stack object is rejected.
        assert!(
            p.validate(&select_targets(&[&[to(FIRST_STACK_ID)], &[]]))
                .is_err()
        );
    }
}

/// REQ-CAST-001 (step 3); CR 115.3, 601.2c (its example): the same object may be chosen once
/// per instance of "target" (two slots) but not twice for one instance.
#[test]
fn targeting_same_object_across_slots_but_not_within() {
    for (label, req) in both_kinds(P1) {
        let two = Fake {
            base_slots: vec![slot(1, 1, &[to(10)]), slot(1, 1, &[to(10), to(11)])],
            ..Fake::new()
        };
        let run = drive(&two, &req, &[select_targets(&[&[to(10)], &[to(10)]])]);
        let (_, p) = announced(&run, &two);
        assert_eq!(p.targets, vec![vec![to(10)], vec![to(10)]], "{label}");

        let one = Fake::one_slot(2, 2, &[to(10), to(11)]);
        let p = pending_of(&one, &req, &[]);
        let dup = select_targets(&[&[to(10), to(10)]]);
        assert!(p.validate(&dup).is_err(), "{label}: duplicate accepted");
        let mut s = start(&one, &req);
        let id = s.pending().expect("pending").request_id;
        assert!(matches!(
            s.submit(id, dup),
            Err(ChoiceError::Invalid { .. })
        ));
        assert!(s.answers().is_empty(), "a rejected answer was recorded");
        let run = drive(&one, &req, &[select_targets(&[&[to(10), to(11)]])]);
        let (_, p) = announced(&run, &one);
        assert_eq!(p.targets, vec![vec![to(10), to(11)]], "{label}");
    }
}

/// REQ-CAST-001 (step 3); CR 115.6, 601.2c: a slot with `min == 0` accepts zero targets and
/// the cast is announced with an empty target list; an optional slot whose candidates are all
/// illegal does not roll back.
#[test]
fn targeting_zero_targets_when_optional() {
    for (label, req) in both_kinds(P1) {
        let f = Fake::one_slot(0, 2, &[to(10), to(11)]);
        let run = drive(&f, &req, &[select_targets(&[&[]])]);
        let (_, p) = announced(&run, &f);
        assert_eq!(p.targets, vec![Vec::<ChoiceTarget>::new()], "{label}");

        let hidden = Fake::one_slot(0, 1, &[to(10)]).with_rules(&[tr(1, shroud_obj(10), 1)]);
        let s = start(&hidden, &req);
        if let Some(p) = s.pending() {
            assert_eq!(target_slots_of(p)[0].legal, Vec::<ChoiceTarget>::new());
            let run = drive(&hidden, &req, &[select_targets(&[&[]])]);
            let _ = announced(&run, &hidden);
        } else {
            let Status::Done { output, .. } = s.status() else {
                unreachable!()
            };
            assert!(
                matches!(output.outcome, AnnounceOutcome::Announced { .. }),
                "{label}: {:?}",
                output.outcome
            );
        }
    }
}

/// REQ-CAST-001 (step 3): ward is not checked when targets are chosen: a ward-like entry
/// (unknown keyword) leaves the target legal and adds no request.
#[test]
fn targeting_ward_not_checked() {
    for (label, req) in both_kinds(P1) {
        let ward = json!({"keyword": "WARD", "target_object": 10, "sources": "OPPONENTS"});
        let plain = tfake(&[]);
        let warded = tfake(&[tr(1, ward, 1)]);
        assert_eq!(legal(&warded, &req), cands(), "{label}");
        let a = drive(&plain, &req, &[select_targets(&[&[to(10)]])]);
        let b = drive(&warded, &req, &[select_targets(&[&[to(10)]])]);
        assert_eq!(a.requests, b.requests, "{label}");
        assert_eq!(a.outcome(), b.outcome(), "{label}");
    }
}

/// REQ-CAST-001 (step 3); CR 601.2c: targets that only an additional cost requires appear
/// when (and only when) that cost was announced.
#[test]
fn targeting_slots_depend_on_costs() {
    for (label, req) in both_kinds(P1) {
        let f = Fake {
            cost_spec: CostSpec {
                additional_costs: vec!["test.extra_cost".to_owned()],
                ..no_costs()
            },
            additional_slots: vec![slot(1, 1, &[to(11)])],
            ..Fake::one_slot(1, 1, &[to(10)])
        };
        let with = CostChoice {
            additional: vec![0],
            ..no_choice()
        };
        let run = drive(
            &f,
            &req,
            &[configure(&with), select_targets(&[&[to(10)], &[to(11)]])],
        );
        let (_, p) = announced(&run, &f);
        assert_eq!(p.costs, with, "{label}");
        assert_eq!(p.targets, vec![vec![to(10)], vec![to(11)]], "{label}");
        let run = drive(
            &f,
            &req,
            &[configure(&no_choice()), select_targets(&[&[to(10)]])],
        );
        let (_, p) = announced(&run, &f);
        assert_eq!(p.targets, vec![vec![to(10)]], "{label}");
    }
}

// ---------------------------------------------------------------------------------------------
// 4. Modes
// ---------------------------------------------------------------------------------------------

fn modes_payload(p: &PendingChoiceRequest) -> (Vec<String>, bool) {
    match &p.payload {
        ChoicePayload::ChooseModes {
            labels,
            repeats_allowed,
            ..
        } => (labels.clone(), *repeats_allowed),
        other => panic!("expected ChooseModes, got {other:?}"),
    }
}

/// Three modes; mode 0's mandatory target is hidden by shroud, so only modes 1 and 2 are
/// offered.
fn modal_fake(min: u32, max: u32, repeats: bool) -> Fake {
    Fake {
        mode_spec: Some(modes_spec(3, min, max, repeats)),
        mode_slots: vec![
            vec![slot(1, 1, &[to(10)])],
            vec![slot(1, 1, &[to(11)])],
            vec![slot(0, 1, &[to(12)])],
        ],
        ..Fake::new()
    }
    .with_rules(&[tr(1, shroud_obj(10), 1)])
}

/// REQ-CAST-001 (step 2), REQ-NET-003; CR 700.2a, 601.2b: a mode whose mandatory target has
/// no legal candidate is not offered; the answer's indices are mapped back to the original
/// mode indices in `Proposal.modes`.
#[test]
fn modes_illegal_mode_not_offered_and_indices_mapped() {
    for (label, req) in both_kinds(P1) {
        let f = modal_fake(1, 1, false);
        let p = pending_of(&f, &req, &[]);
        assert_eq!(p.choice_type, ChoiceType::ChooseModes);
        let labels = mode_labels(3);
        assert_eq!(
            modes_payload(&p).0,
            vec![labels[1].clone(), labels[2].clone()],
            "{label}"
        );
        // Offered index 1 is original mode 2.
        let run = drive(
            &f,
            &req,
            &[select_modes(&[1]), select_targets(&[&[to(12)]])],
        );
        let (_, prop) = announced(&run, &f);
        assert_eq!(prop.modes, vec![2], "{label}");
        assert_eq!(prop.targets, vec![vec![to(12)]], "{label}");
        // Offered index 0 is original mode 1.
        let run = drive(
            &f,
            &req,
            &[select_modes(&[0]), select_targets(&[&[to(11)]])],
        );
        let (_, prop) = announced(&run, &f);
        assert_eq!(prop.modes, vec![1], "{label}");
        // Index 2 does not exist in the offered list.
        assert!(p.validate(&select_modes(&[2])).is_err(), "{label}");
    }
}

/// REQ-CAST-001 (step 2), CR 115.5, 700.2a: a mode whose only candidate is the spell itself is
/// illegal and not offered.
#[test]
fn modes_self_only_target_mode_not_offered() {
    for (label, req) in both_kinds(P1) {
        let f = Fake {
            mode_spec: Some(modes_spec(2, 1, 1, false)),
            mode_slots: vec![
                vec![slot(1, 1, &[to(FIRST_STACK_ID)])],
                vec![slot(1, 1, &[to(11)])],
            ],
            ..Fake::new()
        };
        let p = pending_of(&f, &req, &[]);
        assert_eq!(
            modes_payload(&p).0,
            vec![mode_labels(2)[1].clone()],
            "{label}"
        );
    }
}

/// REQ-CAST-001 (step 2); CR 700.2d: a mode can be chosen twice only when repeats are allowed;
/// a repeated mode may take the same target again (each instance is its own slot).
#[test]
fn modes_repeats() {
    for (label, req) in both_kinds(P1) {
        let no = modal_fake(1, 2, false);
        let p = pending_of(&no, &req, &[]);
        assert!(!modes_payload(&p).1);
        assert!(p.validate(&select_modes(&[0, 0])).is_err(), "{label}");
        assert!(p.validate(&select_modes(&[0, 1])).is_ok(), "{label}");

        let yes = modal_fake(1, 2, true);
        let p = pending_of(&yes, &req, &[]);
        assert!(modes_payload(&p).1);
        assert!(p.validate(&select_modes(&[0, 0])).is_ok(), "{label}");
        let run = drive(
            &yes,
            &req,
            &[
                select_modes(&[0, 0]),
                select_targets(&[&[to(11)], &[to(11)]]),
            ],
        );
        let (_, prop) = announced(&run, &yes);
        assert_eq!(prop.modes, vec![1, 1], "{label}");
        assert_eq!(prop.targets, vec![vec![to(11)], vec![to(11)]], "{label}");
    }
}

/// REQ-CAST-001 (step 2), REQ-CORE-005; CR 700.2a, 601.2, 734.1: all modes illegal, or fewer
/// legal modes than the minimum, rolls back at Choices with NoLegalMode.
#[test]
fn modes_no_legal_mode() {
    for (label, req) in both_kinds(P1) {
        // Fewer legal modes (2) than the minimum (3), no repeats.
        let f = modal_fake(3, 3, false);
        let run = drive(&f, &req, &[]);
        assert!(run.requests.is_empty(), "{label}");
        assert_rolled_back(&run, &f, CastStep::Choices, StopReason::NoLegalMode);
        // All illegal.
        let f = Fake {
            mode_spec: Some(modes_spec(2, 1, 1, false)),
            mode_slots: vec![
                vec![slot(1, 1, &[to(10)])],
                vec![slot(2, 2, &[to(10), to(11)])],
            ],
            ..Fake::new()
        }
        .with_rules(&[tr(1, other_obj(10), 1)]);
        let run = drive(&f, &req, &[]);
        assert_rolled_back(&run, &f, CastStep::Choices, StopReason::NoLegalMode);
    }
}

/// REQ-CAST-001 (step 2); CR 700.2a: mode legality uses the targeting prohibitions (hexproof is
/// per caster): the controller of the hexproof object is offered the mode, an opponent is not.
#[test]
fn modes_legality_depends_on_caster() {
    for caster in [P1, P2] {
        for (label, req) in both_kinds(caster) {
            let f = Fake {
                mode_spec: Some(modes_spec(2, 1, 1, false)),
                mode_slots: vec![vec![slot(1, 1, &[to(10)])], vec![]],
                ..Fake::new()
            }
            .with_rules(&[tr(1, hexproof_obj(10), 1)]);
            let p = pending_of(&f, &req, &[]);
            let n = modes_payload(&p).0.len();
            assert_eq!(n, if caster == P2 { 2 } else { 1 }, "{label} by {caster:?}");
            assert_eq!(p.chooser, caster);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 5. Costs
// ---------------------------------------------------------------------------------------------

fn rich_costs() -> CostSpec {
    CostSpec {
        alternative_costs: vec!["test.alt_a".to_owned(), "test.alt_b".to_owned()],
        additional_costs: vec!["test.add_a".to_owned(), "test.add_b".to_owned()],
        x_range: Some((1, 4)),
        symbol_slots: vec![
            vec!["test.sym_a".to_owned(), "test.sym_b".to_owned()],
            vec!["test.sym_c".to_owned(), "test.sym_d".to_owned()],
        ],
        harmonize_candidates: vec![oid(30), oid(31)],
    }
}

/// REQ-CAST-001 (step 2), REQ-NET-003; CR 601.2b: the cost ask carries the CostSpec;
/// invalid configurations (an alternative cost outside the list, the same additional cost
/// twice, X out of range or missing, a wrong number of symbol options) are rejected; a valid
/// one ends up in `Proposal.costs`.
#[test]
fn costs_validation_and_result() {
    for (label, req) in both_kinds(P1) {
        let f = Fake {
            cost_spec: rich_costs(),
            ..Fake::new()
        };
        let p = pending_of(&f, &req, &[]);
        assert_eq!(p.choice_type, ChoiceType::ChooseCostsAndX);
        let spec = rich_costs();
        assert_eq!(
            p.payload,
            ChoicePayload::ChooseCostsAndX {
                alternative_costs: spec.alternative_costs.clone(),
                additional_costs: spec.additional_costs.clone(),
                x_range: spec.x_range,
                symbol_slots: spec.symbol_slots.clone(),
                harmonize_candidates: spec.harmonize_candidates.clone(),
            },
            "{label}"
        );
        let valid = CostChoice {
            alternative: Some(1),
            additional: vec![0, 1],
            x: Some(3),
            symbol_options: vec![1, 0],
            harmonize: Some(oid(31)),
        };
        let bad = [
            CostChoice {
                alternative: Some(2),
                ..valid.clone()
            },
            CostChoice {
                additional: vec![0, 0],
                ..valid.clone()
            },
            CostChoice {
                additional: vec![2],
                ..valid.clone()
            },
            CostChoice {
                x: Some(5),
                ..valid.clone()
            },
            CostChoice {
                x: Some(0),
                ..valid.clone()
            },
            CostChoice {
                x: None,
                ..valid.clone()
            },
            CostChoice {
                symbol_options: vec![1],
                ..valid.clone()
            },
            CostChoice {
                symbol_options: vec![1, 0, 0],
                ..valid.clone()
            },
            CostChoice {
                symbol_options: vec![2, 0],
                ..valid.clone()
            },
        ];
        for b in &bad {
            assert!(
                p.validate(&configure(b)).is_err(),
                "{label}: accepted {b:?}"
            );
        }
        assert!(p.validate(&configure(&valid)).is_ok(), "{label}");
        let run = drive(&f, &req, &[configure(&valid)]);
        let (_, prop) = announced(&run, &f);
        assert_eq!(prop.costs, valid, "{label}");
        assert_eq!(prop.modes, Vec::<u32>::new());
        assert!(prop.targets.iter().all(Vec::is_empty));
        assert_eq!(prop.division, None);
    }
}

/// REQ-CAST-001 (step 2); CR 601.2b: only one alternative cost: the X-free spec with two
/// alternatives accepts each one alone and the normal cost.
#[test]
fn costs_single_alternative() {
    let f = Fake {
        cost_spec: CostSpec {
            alternative_costs: vec!["test.alt_a".to_owned(), "test.alt_b".to_owned()],
            ..no_costs()
        },
        ..Fake::new()
    };
    for (label, req) in both_kinds(P1) {
        for alt in [None, Some(0), Some(1)] {
            let c = CostChoice {
                alternative: alt,
                ..no_choice()
            };
            let run = drive(&f, &req, &[configure(&c)]);
            assert_eq!(announced(&run, &f).1.costs, c, "{label}");
        }
        let p = pending_of(&f, &req, &[]);
        assert!(
            p.validate(&with_x(1)).is_err(),
            "{label}: X without an X cost"
        );
    }
}

/// REQ-CAST-001 (step 2), REQ-NET-003; CR 601.2b: no cost ask when the CostSpec offers
/// nothing; the proposal's costs are then the empty choice.
#[test]
fn costs_no_ask_when_nothing_offered() {
    for (label, req) in both_kinds(P1) {
        let f = Fake::one_slot(1, 1, &[to(10)]);
        let run = drive(&f, &req, &[select_targets(&[&[to(10)]])]);
        assert_eq!(run.requests.len(), 1, "{label}");
        assert_eq!(run.requests[0].choice_type, ChoiceType::ChooseTargets);
        let (_, p) = announced(&run, &f);
        assert_eq!(p.costs, no_choice());
        // Each single feature makes the ask appear.
        let features = [
            CostSpec {
                alternative_costs: vec!["test.alt".to_owned()],
                ..no_costs()
            },
            CostSpec {
                additional_costs: vec!["test.add".to_owned()],
                ..no_costs()
            },
            x_costs(0, 0),
            CostSpec {
                symbol_slots: vec![vec!["test.sym".to_owned()]],
                ..no_costs()
            },
            CostSpec {
                harmonize_candidates: vec![oid(30)],
                ..no_costs()
            },
        ];
        for c in features {
            let f = Fake {
                cost_spec: c.clone(),
                ..Fake::new()
            };
            let p = pending_of(&f, &req, &[]);
            assert_eq!(p.choice_type, ChoiceType::ChooseCostsAndX, "{label}: {c:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 6. Division
// ---------------------------------------------------------------------------------------------

fn div_fake(total: u64) -> Fake {
    Fake {
        division: Some(total),
        ..Fake::one_slot(1, 2, &[to(10), to(11)])
    }
}

/// REQ-CAST-001 (step 4), REQ-NET-003 (DIVIDE_VALUE); CR 601.2d: 3 among two targets: 3/0,
/// 2/0 and 4/1 are rejected; 2/1 and 1/2 are accepted and returned in `Proposal.division`.
#[test]
fn division_three_among_two() {
    for (label, req) in both_kinds(P1) {
        let f = div_fake(3);
        let t = select_targets(&[&[to(10), to(11)]]);
        let p = pending_of(&f, &req, std::slice::from_ref(&t));
        assert_eq!(p.choice_type, ChoiceType::DivideValue);
        assert_eq!(
            p.payload,
            ChoicePayload::DivideValue {
                total: 3,
                recipients: vec![(to(10), 1), (to(11), 1)],
            },
            "{label}"
        );
        for (a, b) in [(3, 0), (2, 0), (4, 1), (0, 3), (1, 1)] {
            let resp = submit_division(&[(to(10), a), (to(11), b)]);
            assert!(p.validate(&resp).is_err(), "{label}: accepted {a}/{b}");
        }
        for (a, b) in [(2, 1), (1, 2)] {
            let d = [(to(10), a), (to(11), b)];
            let run = drive(&f, &req, &[t.clone(), submit_division(&d)]);
            let (_, prop) = announced(&run, &f);
            assert_eq!(prop.division, Some(d.to_vec()), "{label}");
        }
    }
}

/// REQ-CAST-001 (step 4), REQ-CORE-005; CR 601.2d: total 1 with two targets rolls back with
/// DivisionImpossible{total: 1, recipients: 2}; one target divides alone.
#[test]
fn division_impossible_and_single_recipient() {
    for (_, req) in both_kinds(P1) {
        let f = div_fake(1);
        let run = drive(&f, &req, &[select_targets(&[&[to(10), to(11)]])]);
        assert_rolled_back(
            &run,
            &f,
            CastStep::Division,
            StopReason::DivisionImpossible {
                total: 1,
                recipients: 2,
            },
        );
        let run = drive(
            &f,
            &req,
            &[
                select_targets(&[&[to(11)]]),
                submit_division(&[(to(11), 1)]),
            ],
        );
        let (_, prop) = announced(&run, &f);
        assert_eq!(prop.division, Some(vec![(to(11), 1)]));
    }
}

/// REQ-CAST-001 (step 4), REQ-NET-003; CR 601.2d: recipients are the chosen targets in slot
/// order across several slots.
#[test]
fn division_recipients_in_slot_order() {
    for (label, req) in both_kinds(P1) {
        let f = Fake {
            base_slots: vec![slot(1, 1, &[to(12)]), slot(1, 2, &[tp(2), to(10)])],
            division: Some(5),
            ..Fake::new()
        };
        let t = select_targets(&[&[to(12)], &[to(10), tp(2)]]);
        let p = pending_of(&f, &req, std::slice::from_ref(&t));
        let ChoicePayload::DivideValue { total, recipients } = &p.payload else {
            panic!("{label}: {:?}", p.payload);
        };
        assert_eq!(*total, 5);
        let names: Vec<ChoiceTarget> = recipients.iter().map(|(t, _)| *t).collect();
        let mut second = [to(10), tp(2)];
        let first = [to(12)];
        // Slot order is required; within a slot the answer order or the ascending order.
        let a: Vec<ChoiceTarget> = first.iter().chain(second.iter()).copied().collect();
        second.sort();
        let b: Vec<ChoiceTarget> = first.iter().chain(second.iter()).copied().collect();
        assert!(names == a || names == b, "{label}: {names:?}");
        assert!(recipients.iter().all(|(_, m)| *m == 1), "{label}");
    }
}

/// REQ-NET-003, REQ-CAST-001 (step 4): no DIVIDE_VALUE request when `division_total` is None
/// or no target was chosen.
#[test]
fn division_no_request_when_not_needed() {
    for (label, req) in both_kinds(P1) {
        let f = Fake::one_slot(1, 2, &[to(10), to(11)]);
        let run = drive(&f, &req, &[select_targets(&[&[to(10), to(11)]])]);
        assert_eq!(run.requests.len(), 1, "{label}");
        assert_eq!(announced(&run, &f).1.division, None);

        let f = Fake {
            division: Some(3),
            ..Fake::one_slot(0, 2, &[to(10), to(11)])
        };
        let run = drive(&f, &req, &[select_targets(&[&[]])]);
        assert_eq!(
            run.requests.len(),
            1,
            "{label}: zero targets, no division ask"
        );
        assert!(
            run.requests
                .iter()
                .all(|r| r.choice_type != ChoiceType::DivideValue)
        );
    }
}

// ---------------------------------------------------------------------------------------------
// 7. Suspension
// ---------------------------------------------------------------------------------------------

/// REQ-NET-003, REQ-CORE-001; CR 601.2b, 700.2, 601.2c, 601.2d: the four asks come in order
/// with ids 0..=3, the right choice type, the caster as chooser and `reason.cr` naming the
/// rule; the base hash is unchanged while pending.
#[test]
fn suspension_request_sequence() {
    for caster in [P1, P2] {
        for (label, req) in both_kinds(caster) {
            let base = Fake {
                priority: Some(caster),
                ..four_ask_fake()
            };
            let run = drive(&base, &req, &four_answers());
            let expected = [
                (ChoiceType::ChooseModes, &["601.2b", "700.2"][..]),
                (ChoiceType::ChooseCostsAndX, &["601.2b"][..]),
                (ChoiceType::ChooseTargets, &["601.2c"][..]),
                (ChoiceType::DivideValue, &["601.2d"][..]),
            ];
            assert_eq!(run.requests.len(), 4, "{label}");
            for (i, (p, (ty, crs))) in run.requests.iter().zip(expected).enumerate() {
                assert_eq!(p.request_id, RequestId::new(i as u64), "{label}");
                assert_eq!(p.choice_type, ty, "{label}");
                assert_eq!(p.chooser, caster, "{label}");
                assert!(
                    crs.iter().any(|c| p.reason.cr.contains(c)),
                    "{label}: request {i} reason.cr {:?} names none of {crs:?}",
                    p.reason.cr
                );
            }
            let (_, prop) = announced(&run, &base);
            assert_eq!(
                prop,
                Proposal {
                    modes: vec![0],
                    costs: CostChoice {
                        x: Some(1),
                        ..no_choice()
                    },
                    targets: vec![vec![to(10), to(11)]],
                    division: Some(vec![(to(10), 2), (to(11), 1)]),
                },
                "{label}"
            );
        }
    }
}

/// REQ-NET-003, REQ-CORE-001: skipped steps keep the ids dense (no modes, no costs: the target
/// ask is request 0 and the division ask request 1).
#[test]
fn suspension_ids_dense_when_steps_skipped() {
    for (_, req) in both_kinds(P1) {
        let f = div_fake(2);
        let run = drive(
            &f,
            &req,
            &[
                select_targets(&[&[to(10), to(11)]]),
                submit_division(&[(to(10), 1), (to(11), 1)]),
            ],
        );
        let ids: Vec<u64> = run.requests.iter().map(|p| p.request_id.get()).collect();
        assert_eq!(ids, vec![0, 1]);
        assert_eq!(run.requests[0].choice_type, ChoiceType::ChooseTargets);
        assert_eq!(run.requests[1].choice_type, ChoiceType::DivideValue);
    }
}

/// REQ-NET-003, REQ-CORE-001: replaying the recorded answers in one call gives the identical
/// final hash, state and outcome as submitting them one at a time.
#[test]
fn suspension_replay_identical() {
    for (_, req) in both_kinds(P1) {
        for base in [
            four_ask_fake(),
            four_ask_fake().with_rules(&[late_prohibition(7, &req)]),
        ] {
            let run = drive(&base, &req, &four_answers());
            let s = replay(&base, &req, &run.answers).expect("replay");
            assert_eq!(s.state_hash(), run.final_hash);
            let Status::Done { state, output } = s.status() else {
                panic!("replay did not finish");
            };
            assert_eq!(state, &run.state);
            assert_eq!(output, &run.observed);
        }
    }
}

/// REQ-NET-003, REQ-CORE-001: a different answer order, answers recorded on another state, a
/// stale request id or extra answers are reported as a ChoiceError, never ignored; a rejected
/// submission changes nothing.
#[test]
fn suspension_errors_reported() {
    for (label, req) in both_kinds(P1) {
        let base = four_ask_fake();
        let answers = four_answers();
        // Swapped order.
        let mut swapped = answers.clone();
        swapped.swap(0, 1);
        assert!(replay(&base, &req, &swapped).is_err(), "{label}: swapped");
        let mut swapped = answers.clone();
        swapped.swap(2, 3);
        assert!(
            replay(&base, &req, &swapped).is_err(),
            "{label}: swapped 2/3"
        );
        // Divergent state: the recorded targets are not legal there.
        let other = Fake {
            mode_slots: vec![
                vec![slot(1, 2, &[to(13), to(14)])],
                vec![slot(1, 1, &[to(12)])],
            ],
            ..four_ask_fake()
        };
        assert!(
            replay(&other, &req, &answers).is_err(),
            "{label}: divergent"
        );
        let hidden = four_ask_fake().with_rules(&[tr(1, shroud_obj(11), 1)]);
        assert!(
            replay(&hidden, &req, &answers).is_err(),
            "{label}: hidden target"
        );
        // Extra answer.
        let mut extra = answers.clone();
        extra.push(select_modes(&[0]));
        assert!(
            matches!(replay(&base, &req, &extra), Err(ChoiceError::NotPending)),
            "{label}: extra"
        );
        // Stale id and invalid answer leave the session unchanged.
        let mut s = start(&base, &req);
        let before = s.state_hash();
        assert!(matches!(
            s.submit(RequestId::new(1), select_modes(&[0])),
            Err(ChoiceError::StaleRequest { .. })
        ));
        assert!(matches!(
            s.submit(RequestId::new(0), with_x(1)),
            Err(ChoiceError::Invalid { .. })
        ));
        assert!(s.answers().is_empty());
        assert_eq!(s.state_hash(), before);
        assert_eq!(s.pending().map(|p| p.request_id), Some(RequestId::new(0)));
    }
}

/// REQ-NET-003, R12: every request's default answer is valid for it (the session would refuse
/// to publish it otherwise), so a timeout can always be applied.
#[test]
fn suspension_defaults_validate() {
    for (_, req) in both_kinds(P1) {
        let run = drive(&four_ask_fake(), &req, &four_answers());
        for p in &run.requests {
            assert!(
                p.validate(&p.default_on_timeout).is_ok(),
                "{:?}",
                p.choice_type
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 8. Order and determinism
// ---------------------------------------------------------------------------------------------

/// R3, REQ-CORE-001, REQ-CAST-001: ten runs on the same inputs give identical requests, hashes
/// and outcomes.
#[test]
fn determinism_ten_runs() {
    for (_, req) in both_kinds(P1) {
        for base in [
            four_ask_fake(),
            four_ask_fake().with_rules(&[tr(1, shroud_obj(12), 1), late_prohibition(7, &req)]),
        ] {
            let first = drive(&base, &req, &four_answers());
            for _ in 0..10 {
                assert_eq!(drive(&base, &req, &four_answers()), first);
            }
        }
    }
}

/// R3, REQ-CAST-001 (step 3): inserting the same rule entries in another order (same
/// timestamps) or giving the targeting entries other timestamps does not change the requests
/// or the outcome.
#[test]
fn determinism_entry_order() {
    let entries = [
        tr(1, shroud_obj(10), 1),
        tr(2, hexproof_obj(11), 2),
        tr_for(3, ignore_hexproof_obj(11), &[P1], 3),
        tr(4, protection_obj(12, QUALITY_A), 4),
        tr(5, other_obj(13), 5),
    ];
    let base = Fake::one_slot(0, 4, &[to(10), to(11), to(12), to(13), to(14)])
        .with_qualities(&[QUALITY_A]);
    for (_, req) in both_kinds(P1) {
        let answer = [select_targets(&[&[to(11), to(14)]])];
        let reference = drive(&base.clone().with_rules(&entries), &req, &answer);
        assert_eq!(
            target_slots_of(&reference.requests[0])[0].legal,
            vec![to(11), to(14)]
        );
        let mut reversed = entries.to_vec();
        reversed.reverse();
        let run = drive(&base.clone().with_rules(&reversed), &req, &answer);
        assert_eq!(run, reference);
        // Other timestamps: same requests and outcome (the hash of the rule set differs).
        let restamped: Vec<R> = entries
            .iter()
            .map(|e| R {
                ts: 10 - e.ts,
                ..e.clone()
            })
            .collect();
        let run = drive(&base.clone().with_rules(&restamped), &req, &answer);
        assert_eq!(run.requests, reference.requests);
        assert_eq!(run.outcome(), reference.outcome());
    }
}

/// R3, REQ-CAST-001 (steps 2-3): permuting the order in which the fake lists candidates does
/// not change the requests or the outcome: `legal` is ascending (players before objects).
#[test]
fn determinism_candidate_order() {
    let orders: [&[ChoiceTarget]; 3] = [
        &[tp(1), tp(2), to(10), to(11), to(12)],
        &[to(12), tp(2), to(10), tp(1), to(11)],
        &[to(11), to(10), to(12), tp(2), tp(1)],
    ];
    for (_, req) in both_kinds(P1) {
        let mut seen = Vec::new();
        for cands in orders {
            let f = Fake::one_slot(1, 2, cands).with_rules(&[tr(1, shroud_obj(11), 1)]);
            let run = drive(&f, &req, &[select_targets(&[&[to(12), tp(2)]])]);
            assert_eq!(
                target_slots_of(&run.requests[0])[0].legal,
                vec![tp(1), tp(2), to(10), to(12)]
            );
            seen.push((run.requests.clone(), run.outcome().clone()));
        }
        assert!(seen.windows(2).all(|w| w[0] == w[1]));
    }
}

/// R3, REQ-CAST-001 (step 2); CR 700.2a: offered modes keep the original ascending order.
#[test]
fn determinism_modes_ascending() {
    for (_, req) in both_kinds(P1) {
        let f = Fake {
            mode_spec: Some(modes_spec(4, 1, 4, false)),
            mode_slots: vec![
                vec![],
                vec![slot(1, 1, &[to(10)])],
                vec![],
                vec![slot(1, 1, &[to(11)])],
            ],
            ..Fake::new()
        }
        .with_rules(&[tr(1, shroud_obj(10), 1)]);
        let p = pending_of(&f, &req, &[]);
        let labels = mode_labels(4);
        assert_eq!(
            modes_payload(&p).0,
            vec![labels[0].clone(), labels[2].clone(), labels[3].clone()]
        );
        let run = drive(
            &f,
            &req,
            &[select_modes(&[2, 0]), select_targets(&[&[to(11)]])],
        );
        let (_, prop) = announced(&run, &f);
        // Original indices of offered 2 and 0 (answer order or ascending order).
        assert!(
            prop.modes == vec![3, 0] || prop.modes == vec![0, 3],
            "{:?}",
            prop.modes
        );
    }
}

/// REQ-CAST-001, REQ-TEST-001: the public outcome types support Clone, Debug, PartialEq and
/// Eq (assumed by the brief's pinned API).
#[test]
fn outcome_types_are_comparable() {
    fn assert_traits<T: Clone + std::fmt::Debug + PartialEq + Eq>() {}
    assert_traits::<CastKind>();
    assert_traits::<CastRequest>();
    assert_traits::<ModeSpec>();
    assert_traits::<CostSpec>();
    assert_traits::<CostChoice>();
    assert_traits::<TargetSlotSpec>();
    assert_traits::<Proposal>();
    assert_traits::<CastStep>();
    assert_traits::<StopReason>();
    assert_traits::<RolledBack>();
    assert_traits::<AnnounceOutcome>();
}
