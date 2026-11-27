// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Seeded property tests for the legal-action generator (REQ-DSL-013, REQ-ARCH-002, R3, R12;
//! CR 101.2, 117.1): every offered action is accepted when submitted as a PRIORITY_ACTION
//! answer, nothing else is, no offered action is prohibited, and the result is reproducible.
//! Randomness is `Pcg64Dxsm::from_u64_seed` over fixed seeds only (R3). Black-box: written from
//! the work-item brief only. Every name is invented (R5).

#[path = "spec_actions_support.rs"]
mod support;

use fineprint_base::ids::{ObjectId, PlayerId};
use fineprint_core::actions::*;
use fineprint_core::choice::{ChoicePayload, ChoiceResponse};
use fineprint_core::events::{Phase, Step};
use fineprint_core::layers::RuleSet;
use fineprint_dsl::ability::{ActivationTiming, RuleKind};
use fineprint_dsl::expr::Zone;
use serde_json::{Value, json};
use support::*;

const SEEDS: u64 = 160;

/// Objects come from this small pool so that "object" filters hit.
const POOL: u64 = 8;

/// `n` distinct objects from the pool.
fn objects(rng: &mut Rng, n: usize) -> Vec<u64> {
    let mut pool: Vec<u64> = (1..=POOL).collect();
    rng.shuffle(&mut pool);
    pool.truncate(n);
    pool
}

fn count(rng: &mut Rng) -> usize {
    rng.index(7)
}

const ZONES: [Zone; 4] = [Zone::Hand, Zone::Graveyard, Zone::Exile, Zone::Battlefield];

fn random_zone_filter(rng: &mut Rng) -> Value {
    match rng.below(5) {
        0 => json!(5),
        1 => json!(["HAND"]),
        _ => json!(rng.pick(&ZONES).as_str()),
    }
}

fn random_object_filter(rng: &mut Rng) -> Value {
    match rng.below(6) {
        0 => json!("x"),
        1 => json!(-1),
        _ => json!(1 + rng.below(POOL)),
    }
}

/// A random rule entry (id given), including malformed params.
fn random_rule(rng: &mut Rng, id: u64) -> R {
    let kinds = [
        RuleKind::Prohibition,
        RuleKind::Prohibition,
        RuleKind::PlayPermission,
        RuleKind::CastPermission,
        RuleKind::TimingPermission,
        RuleKind::AdditionalLandPlay,
    ];
    let kind = *rng.pick(&kinds);
    let players: &[PlayerId] = match rng.below(4) {
        0 => &[P1],
        1 => &[P2],
        2 => &[P1, P2],
        _ => &[],
    };
    let ts = 1 + rng.below(20);
    let mut m = serde_json::Map::new();
    if kind == RuleKind::AdditionalLandPlay {
        match rng.below(5) {
            0 => {}
            1 => {
                m.insert("count".into(), json!("x"));
            }
            2 => {
                m.insert("count".into(), json!(-1));
            }
            _ => {
                m.insert("count".into(), json!(rng.below(3)));
            }
        }
        return r(id, kind, Value::Object(m), players, ts);
    }
    let actions = [
        ACTION_PLAY_LAND,
        ACTION_CAST,
        ACTION_ACTIVATE,
        ACTION_SPECIAL,
        "test.junk",
    ];
    let action: Option<&str> = match rng.below(8) {
        0 => None,
        1 => {
            m.insert("action".into(), json!(5));
            None
        }
        _ => Some(*rng.pick(&actions)),
    };
    if let Some(a) = action {
        m.insert("action".into(), json!(a));
    }
    // The brief: do not test filtered PLAY_LAND prohibitions.
    let unfiltered = kind == RuleKind::Prohibition && action == Some(ACTION_PLAY_LAND);
    let special = action == Some(ACTION_SPECIAL);
    if !unfiltered {
        if !special && rng.one_in(3) {
            m.insert("zone".into(), random_zone_filter(rng));
        }
        if rng.one_in(3) {
            m.insert("object".into(), random_object_filter(rng));
        }
        if special && kind == RuleKind::Prohibition && rng.one_in(2) {
            let v = if rng.one_in(5) {
                json!([])
            } else {
                json!(rng.pick(&KIND_IDS))
            };
            m.insert("kind".into(), v);
        }
    }
    r(id, kind, Value::Object(m), players, ts)
}

fn random_origin(rng: &mut Rng) -> FaceDownOrigin {
    match rng.below(5) {
        0 => FaceDownOrigin::Morph {
            has_cost: rng.coin(),
        },
        1 => FaceDownOrigin::Disguise {
            has_cost: rng.coin(),
        },
        2 => FaceDownOrigin::Manifest {
            creature_card: !rng.one_in(4),
            has_mana_cost: !rng.one_in(4),
            instant_or_sorcery: rng.one_in(4),
        },
        3 => FaceDownOrigin::Cloak {
            creature_card: !rng.one_in(4),
            has_mana_cost: !rng.one_in(4),
            instant_or_sorcery: rng.one_in(4),
        },
        _ => FaceDownOrigin::Other {
            can_turn_face_up: rng.coin(),
        },
    }
}

/// Maybe appends an exact duplicate of a random element.
fn maybe_dup<T: Clone>(rng: &mut Rng, v: &mut Vec<T>) {
    if !v.is_empty() && rng.one_in(4) {
        let x = v[rng.index(v.len())].clone();
        v.push(x);
    }
}

/// A random small two-player host from `seed`.
fn random_host(seed: u64) -> FakeHost {
    let mut rng = Rng::new(seed);
    let active = match rng.below(9) {
        0 => None,
        1..=4 => Some(P1),
        _ => Some(P2),
    };
    let phases = [
        (Phase::Beginning, Some(Step::Upkeep)),
        (Phase::PrecombatMain, None),
        (Phase::Combat, Some(Step::DeclareAttackers)),
        (Phase::Combat, Some(Step::CombatDamage)),
        (Phase::PostcombatMain, None),
        (Phase::Ending, Some(Step::End)),
    ];
    let (phase, step) = *rng.pick(&phases);
    let stack_empty = !rng.one_in(3);
    let n_rules = rng.index(9);
    let items: Vec<R> = (0..n_rules)
        .map(|i| random_rule(&mut rng, 1 + u64::try_from(i).expect("small")))
        .collect();
    let mut h = FakeHost::new()
        .with_rules(&items)
        .active(active)
        .phase(phase, step)
        .stack(stack_empty)
        .played(P1, i64::try_from(rng.below(3)).expect("small"))
        .played(P2, i64::try_from(rng.below(3)).expect("small"));

    let n = count(&mut rng);
    for o in objects(&mut rng, n) {
        h.lands.push(LandCandidate {
            object: oid(o),
            face_index: u32::try_from(rng.below(2)).expect("small"),
            zone: *rng.pick(&[Zone::Hand, Zone::Hand, Zone::Graveyard, Zone::Exile]),
            own_permission: rng.one_in(5).then(|| "test.own_land".to_owned()),
        });
    }
    maybe_dup(&mut rng, &mut h.lands);

    let n = count(&mut rng);
    for o in objects(&mut rng, n) {
        h.casts.push(CastCandidate {
            object: oid(o),
            face_index: u32::try_from(rng.below(2)).expect("small"),
            zone: *rng.pick(&[Zone::Hand, Zone::Hand, Zone::Graveyard, Zone::Exile]),
            cast_mode: rng.pick(&["test.normal", "test.alt"]).to_string(),
            is_land: rng.one_in(6),
            instant_timing: rng.coin(),
            own_permission: rng.one_in(5).then(|| "test.own_cast".to_owned()),
        });
    }
    maybe_dup(&mut rng, &mut h.casts);

    let n = count(&mut rng);
    for o in objects(&mut rng, n) {
        let timing = if rng.one_in(3) {
            None
        } else {
            Some(*rng.pick(ActivationTiming::ALL))
        };
        let limits = match rng.below(4) {
            0 => Some(limit(
                Some(1 + u32::try_from(rng.below(2)).expect("small")),
                None,
            )),
            1 => Some(limit(
                None,
                Some(1 + u32::try_from(rng.below(3)).expect("small")),
            )),
            _ => None,
        };
        h.abilities.push(AbilityCandidate {
            object: oid(o),
            ability_id: rng.pick(&["test.a", "test.b"]).to_string(),
            zone: *rng.pick(&[Zone::Battlefield, Zone::Battlefield, Zone::Graveyard]),
            timing,
            limits,
            uses_this_turn: u32::try_from(rng.below(3)).expect("small"),
            uses_this_game: u32::try_from(rng.below(4)).expect("small"),
            is_mana_ability: rng.one_in(4),
            requires_tap_symbol: rng.coin(),
            summoning_sick: rng.one_in(3),
            only_if_met: !rng.one_in(5),
        });
    }
    maybe_dup(&mut rng, &mut h.abilities);

    let n = count(&mut rng);
    let kinds: Vec<&str> = KIND_IDS
        .iter()
        .skip(1)
        .copied()
        .chain(["test.unknown_kind"])
        .collect();
    for o in objects(&mut rng, n) {
        h.specials.push(SpecialCandidate {
            kind: rng.pick(&kinds).to_string(),
            object: oid(o),
            could_begin_to_cast: rng.coin(),
            available: !rng.one_in(4),
        });
    }
    maybe_dup(&mut rng, &mut h.specials);

    let n = count(&mut rng);
    for o in objects(&mut rng, n) {
        h.face_up.push(TurnFaceUpCandidate {
            object: oid(o),
            on_battlefield: !rng.one_in(5),
            merged_instant_or_sorcery: rng.one_in(6),
            origin: random_origin(&mut rng),
        });
    }
    maybe_dup(&mut rng, &mut h.face_up);
    h
}

/// Every candidate's response, each once.
fn candidate_responses(h: &FakeHost) -> Vec<ChoiceResponse> {
    let mut v: Vec<ChoiceResponse> = Vec::new();
    v.extend(h.lands.iter().map(land_resp));
    v.extend(h.casts.iter().map(cast_resp));
    v.extend(h.abilities.iter().map(ability_resp));
    v.extend(h.specials.iter().map(special_resp));
    v.extend(h.face_up.iter().map(face_up_resp));
    v.sort();
    v.dedup();
    v
}

/// Independent re-derivation (from the brief's param conventions) of whether an applicable
/// PROHIBITION of `action` forbids a candidate: "action" must be that string; for PLAY_LAND the
/// prohibition is unfiltered; otherwise every present filter must be well-formed and match
/// ("zone": a Zone wire name; "object": an unsigned integer equal to the raw ObjectId; "kind":
/// a string, SPECIAL_ACTION only). CR 101.2.
fn independently_prohibited(
    rs: &RuleSet,
    p: PlayerId,
    action: &str,
    zone: Option<Zone>,
    object: ObjectId,
    kind: Option<&str>,
) -> bool {
    rs.applying(RuleKind::Prohibition, p).any(|e| {
        let params = &e.modification.params;
        if params.get("action").and_then(Value::as_str) != Some(action) {
            return false;
        }
        if action == ACTION_PLAY_LAND {
            return true;
        }
        if let Some(z) = params.get("zone") {
            match (z.as_str(), zone) {
                (Some(s), Some(cz)) if s == cz.as_str() => {}
                _ => return false,
            }
        }
        if let Some(o) = params.get("object")
            && o.as_u64() != Some(object.get())
        {
            return false;
        }
        if let Some(k) = params.get("kind") {
            match (k.as_str(), kind) {
                (Some(s), Some(ck)) if s == ck => {}
                _ => return false,
            }
        }
        true
    })
}

/// Whether the offered action `a` is forbidden by an applicable prohibition of its class.
fn offered_is_prohibited(h: &FakeHost, p: PlayerId, a: &ChoiceResponse) -> bool {
    let rs = &h.rules;
    match a {
        ChoiceResponse::PlayLand { object, .. } => {
            independently_prohibited(rs, p, ACTION_PLAY_LAND, None, *object, None)
        }
        ChoiceResponse::InitiateCast { object, .. } => h
            .casts
            .iter()
            .filter(|c| &cast_resp(c) == a)
            .any(|c| independently_prohibited(rs, p, ACTION_CAST, Some(c.zone), *object, None)),
        ChoiceResponse::Activate { object, .. } => h
            .abilities
            .iter()
            .filter(|c| &ability_resp(c) == a)
            .any(|c| independently_prohibited(rs, p, ACTION_ACTIVATE, Some(c.zone), *object, None)),
        ChoiceResponse::SpecialAction { kind, object } => {
            independently_prohibited(rs, p, ACTION_SPECIAL, None, *object, Some(kind))
        }
        other => panic!("not an action: {other:?}"),
    }
}

/// REQ-DSL-013, REQ-ARCH-002 (the acceptance criterion "every offered action is accepted by
/// submission"): over 160 seeds and both players, for the PRIORITY_ACTION request built around
/// priority_payload exactly as the heartbeat builds it: (i) every offered action validates,
/// (ii) PassPriority validates, (iii) explain is Legal with a non-empty basis for each,
/// (iv) every candidate not offered is Illegal (non-empty reasons, Prohibited first) and is
/// rejected by validate, (v) no offered action is forbidden by an applicable prohibition of its
/// class, re-derived independently (CR 101.2), (vi) the same seed gives the same payload (R3).
#[test]
fn req_dsl_013_req_arch_002_every_offered_action_is_accepted() {
    // CR 101.2, CR 117.1
    let mut total_offered = 0usize;
    let mut total_refused = 0usize;
    let mut total_prohibited_refusals = 0usize;
    for seed in 0..SEEDS {
        let h = random_host(seed);
        for p in [P1, P2] {
            let request = priority_request(&h, p);
            let actions = payload_actions(&request.payload);
            assert!(
                matches!(
                    request.payload,
                    ChoicePayload::PriorityAction {
                        can_pass: true,
                        can_propose_shortcut: false,
                        ..
                    }
                ),
                "seed {seed}"
            );
            // (ii)
            assert_eq!(
                request.validate(&ChoiceResponse::PassPriority),
                Ok(()),
                "seed {seed}"
            );
            for a in &actions {
                // (i)
                assert_eq!(request.validate(a), Ok(()), "seed {seed} {p:?} {a:?}");
                // (iii)
                match explain(&h, p, a) {
                    Legality::Legal { basis } => {
                        assert!(!basis.is_empty(), "seed {seed} {a:?}");
                        assert!(rule_strings_ok(&basis), "seed {seed} {basis:?}");
                    }
                    Legality::Illegal { reasons } => {
                        panic!("seed {seed} {p:?}: offered {a:?} but explain says {reasons:?}")
                    }
                }
                // (v)
                assert!(
                    !offered_is_prohibited(&h, p, a),
                    "seed {seed} {p:?}: offered {a:?} is prohibited; rules {:?}",
                    h.rules
                );
            }
            total_offered += actions.len();
            // (iv)
            for c in candidate_responses(&h) {
                if actions.contains(&c) {
                    continue;
                }
                total_refused += 1;
                match explain(&h, p, &c) {
                    Legality::Illegal { reasons } => {
                        assert!(!reasons.is_empty(), "seed {seed} {c:?}");
                        assert!(
                            !reasons.contains(&Reason::NoSuchAction),
                            "seed {seed} {c:?}: {reasons:?}"
                        );
                        for (i, x) in reasons.iter().enumerate() {
                            if matches!(x, Reason::Prohibited { .. }) {
                                assert_eq!(i, 0, "seed {seed}: {reasons:?}");
                                total_prohibited_refusals += 1;
                            }
                        }
                    }
                    Legality::Legal { basis } => {
                        panic!("seed {seed} {p:?}: {c:?} not offered but Legal {basis:?}")
                    }
                }
                assert!(request.validate(&c).is_err(), "seed {seed} {c:?}");
            }
            // The legal actions are the payload's actions (and in the same order).
            let la: Vec<ChoiceResponse> = legal_actions(&h, p)
                .into_iter()
                .map(|a| a.response)
                .collect();
            assert_eq!(la, actions, "seed {seed}");
            // (vi)
            let again = random_host(seed);
            assert_eq!(
                priority_payload(&again, p, false),
                request.payload,
                "seed {seed}"
            );
            assert_eq!(
                legal_actions(&again, p),
                legal_actions(&h, p),
                "seed {seed}"
            );
        }
    }
    // The generator is not vacuous.
    assert!(total_offered > 200, "offered {total_offered}");
    assert!(total_refused > 200, "refused {total_refused}");
    assert!(
        total_prohibited_refusals > 10,
        "prohibited {total_prohibited_refusals}"
    );
}

/// REQ-DSL-013 (R3): reversing every candidate vector never changes the result over the seeded
/// hosts (CR 117.1).
#[test]
fn req_dsl_013_seeded_hosts_independent_of_candidate_order() {
    // CR 117.1
    for seed in 0..SEEDS {
        let h = random_host(seed);
        let mut rev = h.clone();
        rev.lands.reverse();
        rev.casts.reverse();
        rev.abilities.reverse();
        rev.specials.reverse();
        rev.face_up.reverse();
        let mut shuffled = h.clone();
        let mut rng = Rng::new(seed ^ 0x5eed);
        rng.shuffle(&mut shuffled.lands);
        rng.shuffle(&mut shuffled.casts);
        rng.shuffle(&mut shuffled.abilities);
        rng.shuffle(&mut shuffled.specials);
        rng.shuffle(&mut shuffled.face_up);
        for p in [P1, P2] {
            let want = legal_actions(&h, p);
            assert_eq!(legal_actions(&rev, p), want, "seed {seed}");
            assert_eq!(legal_actions(&shuffled, p), want, "seed {seed}");
        }
    }
}
