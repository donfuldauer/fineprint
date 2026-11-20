// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Property tests for the post-layer rule set (REQ-LAYER-004, REQ-CORE-001; CR 101.2, 305.2,
//! 305.2b, 613.11, 613.7): prohibition beats permission in every merged query, extra land plays
//! follow the same precedence, and the registration order never changes a verdict or an entry
//! list. A seeded SplitMix64 generator with fixed seeds (no clock, R3, R7). Black-box: written
//! from the work-item brief only.

#[path = "spec_layers_post_support.rs"]
mod support;

use fineprint_base::ids::{EffectId, PlayerId, Timestamp};
use fineprint_core::layers::{EffectOrigin, PlayerAffected, RuleEffect, Verdict, build_rule_set};
use fineprint_dsl::ability::{RuleKind, RuleModification};
use fineprint_dsl::expr::PlayerSelectorAST;
use support::*;

const CASES: u64 = 600;
const ACTIONS: [&str; 3] = ["CAST", "PLAY_LAND", "ACTIVATE"];
const PERMISSIONS: [RuleKind; 3] = [
    RuleKind::CastPermission,
    RuleKind::PlayPermission,
    RuleKind::TimingPermission,
];

#[derive(Clone, Debug)]
enum Scope {
    Fixed(Vec<PlayerId>),
    Sel(PlayerSelectorAST),
}

#[derive(Clone, Debug)]
struct Spec {
    id: u64,
    t: u64,
    kind: RuleKind,
    action: Option<&'static str>,
    /// For ADDITIONAL_LAND_PLAY: `None` = no "count" member.
    count: Option<i64>,
    scope: Scope,
    controller: PlayerId,
    resolved: bool,
}

impl Spec {
    fn key(&self) -> (Timestamp, EffectId) {
        (ts(self.t), eid(self.id))
    }

    fn applies(&self, p: PlayerId, players: &[PlayerId]) -> bool {
        match &self.scope {
            Scope::Fixed(ps) => ps.contains(&p),
            Scope::Sel(s) => players.contains(&p) && selector_matches(s, self.controller, p),
        }
    }

    fn effect(&self) -> RuleEffect {
        let mut m = RuleModification::new(self.kind);
        if let Some(a) = self.action {
            m.params.insert("action".to_owned(), jv(a));
        }
        if let Some(c) = self.count {
            m.params.insert("count".to_owned(), jv(&c));
        }
        RuleEffect {
            source: Some(oid(1000 + self.id)),
            controller: self.controller,
            timestamp: ts(self.t),
            origin: if self.resolved {
                EffectOrigin::Resolved
            } else {
                statik()
            },
            players: match &self.scope {
                Scope::Fixed(ps) => PlayerAffected::Fixed(set(ps)),
                Scope::Sel(s) => PlayerAffected::Selector(s.clone()),
            },
            modification: m,
        }
    }
}

struct Case {
    players: Vec<PlayerId>,
    specs: Vec<Spec>,
}

fn random_case(rng: &mut Rng) -> Case {
    let n_players = rng.below(4) + 1;
    let players: Vec<PlayerId> = (1..=n_players).map(pid).collect();
    let n = usize::try_from(rng.below(9)).unwrap();
    let mut ids: Vec<u64> = (1..=u64::try_from(n).unwrap()).collect();
    rng.shuffle(&mut ids);
    let mut specs = Vec::new();
    for id in ids {
        let kind = match rng.below(9) {
            0 | 1 => RuleKind::CastPermission,
            2 => RuleKind::PlayPermission,
            3 => RuleKind::TimingPermission,
            4..=6 => RuleKind::Prohibition,
            _ => RuleKind::AdditionalLandPlay,
        };
        let action = if rng.below(8) == 0 {
            None
        } else {
            Some(ACTIONS[rng.index(ACTIONS.len())])
        };
        let count = match rng.below(6) {
            0 => None,
            1 => Some(-1),
            k => Some(i64::try_from(k).unwrap() - 2),
        };
        let scope = if rng.below(2) == 0 {
            let mut ps: Vec<PlayerId> = players
                .iter()
                .copied()
                .filter(|_| rng.below(2) == 0)
                .collect();
            if rng.below(6) == 0 {
                ps.push(P9);
            }
            Scope::Fixed(ps)
        } else {
            Scope::Sel(match rng.below(4) {
                0 => all_players(),
                1 => you(),
                2 => opponents(),
                _ => nobody(),
            })
        };
        specs.push(Spec {
            id,
            t: rng.below(4) + 1,
            kind,
            action,
            count,
            scope,
            controller: players[rng.index(players.len())],
            resolved: rng.below(3) == 0,
        });
    }
    Case { players, specs }
}

/// The verdict the brief requires (prohibition first, then the earliest permission).
fn expected_verdict(case: &Case, p: PlayerId, action: &str, permission: RuleKind) -> Verdict {
    let earliest = |kind: RuleKind| {
        case.specs
            .iter()
            .filter(|s| s.kind == kind && s.action == Some(action) && s.applies(p, &case.players))
            .min_by_key(|s| s.key())
            .map(|s| eid(s.id))
    };
    if let Some(by) = earliest(RuleKind::Prohibition) {
        Verdict::Prohibited { by }
    } else if let Some(by) = earliest(permission) {
        Verdict::Permitted { by }
    } else {
        Verdict::NoEffect
    }
}

fn expected_lands(case: &Case, p: PlayerId) -> i64 {
    let prohibited = case.specs.iter().any(|s| {
        s.kind == RuleKind::Prohibition
            && s.action == Some("PLAY_LAND")
            && s.applies(p, &case.players)
    });
    if prohibited {
        return 0;
    }
    1 + case
        .specs
        .iter()
        .filter(|s| s.kind == RuleKind::AdditionalLandPlay && s.applies(p, &case.players))
        .map(|s| s.count.map_or(1, |c| c.max(0)))
        .sum::<i64>()
}

fn build(case: &Case, order: &[Spec]) -> fineprint_core::layers::RuleSet {
    let reg = rules_registry(order.iter().map(|s| (s.id, s.effect())).collect());
    build_rule_set(&reg, &Env::new(&case.players))
}

/// REQ-LAYER-004, REQ-CORE-001: prohibition beats permission in every merged query (issue
/// acceptance criterion). Over 600 seeded cases: (i) an applicable prohibition with the same
/// action gives Prohibited (by the earliest in application order) for every permission kind;
/// (ii) otherwise an applicable permission of the queried kind gives Permitted; (iii) else
/// NoEffect; (iv) a shuffled registration order gives identical verdicts and entries
/// (CR 101.2: the "can't" effect takes precedence; CR 613.11, CR 613.7).
#[test]
fn prohibition_beats_permission_in_every_merged_query() {
    // CR 101.2, CR 613.11, CR 613.7
    let mut rng = Rng::new(0x0035_0001);
    let mut prohibited_seen = 0u32;
    let mut permitted_seen = 0u32;
    let mut overridden_seen = 0u32;
    for case_no in 0..CASES {
        let case = random_case(&mut rng);
        let rs = build(&case, &case.specs);
        let mut shuffled = case.specs.clone();
        rng.shuffle(&mut shuffled);
        let rs2 = build(&case, &shuffled);

        for &p in &case.players {
            for action in ACTIONS {
                for kind in PERMISSIONS {
                    let want = expected_verdict(&case, p, action, kind);
                    let got = rs.query(p, action, kind);
                    assert_eq!(got, want, "case {case_no}: {p:?} {action} {kind:?}");
                    assert_eq!(rs2.query(p, action, kind), got, "case {case_no} shuffled");
                    match want {
                        Verdict::Prohibited { .. } => {
                            prohibited_seen += 1;
                            let has_perm = case.specs.iter().any(|s| {
                                s.kind == kind
                                    && s.action == Some(action)
                                    && s.applies(p, &case.players)
                            });
                            if has_perm {
                                overridden_seen += 1;
                            }
                        }
                        Verdict::Permitted { .. } => permitted_seen += 1,
                        Verdict::NoEffect => {}
                    }
                }
            }
        }
        for kind in RuleKind::ALL {
            assert_eq!(
                entries_view(&rs, *kind),
                entries_view(&rs2, *kind),
                "case {case_no}: entries({kind:?})"
            );
        }
        // Entries of a kind are in ascending (timestamp, EffectId) order (no dependencies).
        for kind in RuleKind::ALL {
            let keys: Vec<_> = rs
                .entries(*kind)
                .iter()
                .map(|e| (e.timestamp, e.id))
                .collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted, "case {case_no}: {kind:?}");
        }
    }
    // The generator exercises every branch, including a permission overridden by a prohibition.
    assert!(prohibited_seen > 100, "{prohibited_seen}");
    assert!(permitted_seen > 100, "{permitted_seen}");
    assert!(overridden_seen > 20, "{overridden_seen}");
}

/// REQ-LAYER-004, REQ-CORE-001: `land_plays_allowed` is 0 whenever a PLAY_LAND prohibition
/// applies, else 1 + the sum of additional land play counts (missing count 1, negative 0);
/// registration order never changes it (CR 101.2, CR 305.2, CR 305.2b).
#[test]
fn land_plays_property() {
    // CR 101.2, CR 305.2, CR 305.2b
    let mut rng = Rng::new(0x0035_0002);
    let mut zero_with_extra = 0u32;
    let mut above_one = 0u32;
    for case_no in 0..CASES {
        let case = random_case(&mut rng);
        let rs = build(&case, &case.specs);
        let mut shuffled = case.specs.clone();
        rng.shuffle(&mut shuffled);
        let rs2 = build(&case, &shuffled);
        for &p in &case.players {
            let want = expected_lands(&case, p);
            let got = rs.land_plays_allowed(p);
            assert_eq!(got, want, "case {case_no}: {p:?} {:?}", case.specs);
            assert_eq!(rs2.land_plays_allowed(p), got, "case {case_no} shuffled");
            if got == 0 {
                assert!(matches!(
                    rs.query(p, "PLAY_LAND", RuleKind::PlayPermission),
                    Verdict::Prohibited { .. }
                ));
                if case
                    .specs
                    .iter()
                    .any(|s| s.kind == RuleKind::AdditionalLandPlay && s.applies(p, &case.players))
                {
                    zero_with_extra += 1;
                }
            }
            if got > 1 {
                above_one += 1;
            }
        }
    }
    assert!(zero_with_extra > 20, "{zero_with_extra}");
    assert!(above_one > 100, "{above_one}");
}
