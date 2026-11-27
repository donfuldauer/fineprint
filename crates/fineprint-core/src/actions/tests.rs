// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// Unit tests of the legal-action generator: REQ-DSL-013 (land plays, casts, activations,
// special actions, explain; no card-specific branch: only kinds, params and host facts are
// read), REQ-ARCH-002 (prohibitions evaluated last, CR 101.2), REQ-CORE-001 / R3 (output
// independent of the candidate order). Synthetic ids, synthetic rule effects and neutral labels
// only (R5).

use std::collections::BTreeSet;

use fineprint_base::ids::{EffectId, ObjectId, PlayerId, Timestamp};
use fineprint_base::prng::Pcg64Dxsm;
use fineprint_dsl::ability::{ActivationLimit, ActivationTiming, RuleKind, RuleModification};
use fineprint_dsl::expr::{PlayerSelectorAST, Zone};

use super::*;
use crate::choice::{
    ChoiceOptions, ChoicePayload, ChoiceReason, ChoiceResponse, ChoiceType, PendingChoiceRequest,
    RequestId, ValidatorContract, ValidatorRule,
};
use crate::events::{Phase, Step};
use crate::layers::{
    EffectOrigin, PlayerAffected, PostLayerEffects, PostLayerEnvironment, RuleEffect, RuleSet,
    build_rule_set,
};

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn p(n: u64) -> PlayerId {
    PlayerId::new(n)
}

fn o(n: u64) -> ObjectId {
    ObjectId::new(n)
}

fn e(n: u64) -> EffectId {
    EffectId::new(n)
}

/// Two players, every selector matches.
struct Env;

impl PostLayerEnvironment for Env {
    fn players(&self) -> Vec<PlayerId> {
        vec![p(1), p(2)]
    }

    fn matches_player(
        &self,
        _selector: &PlayerSelectorAST,
        _controller: PlayerId,
        _source: Option<ObjectId>,
        _candidate: PlayerId,
    ) -> bool {
        true
    }
}

/// One synthetic rule effect: id, timestamp, kind, players and params.
#[derive(Clone)]
struct Fx {
    id: u64,
    ts: u64,
    kind: RuleKind,
    players: Vec<u64>,
    params: Vec<(&'static str, serde_json::Value)>,
}

fn fx(id: u64, ts: u64, kind: RuleKind, action: &str) -> Fx {
    Fx {
        id,
        ts,
        kind,
        players: vec![1],
        params: vec![("action", action.into())],
    }
}

impl Fx {
    fn with(mut self, key: &'static str, v: impl Into<serde_json::Value>) -> Fx {
        self.params.push((key, v.into()));
        self
    }

    fn for_players(mut self, players: &[u64]) -> Fx {
        self.players = players.to_vec();
        self
    }

    fn bare(id: u64, ts: u64, kind: RuleKind) -> Fx {
        Fx {
            id,
            ts,
            kind,
            players: vec![1],
            params: Vec::new(),
        }
    }
}

fn rules(list: &[Fx]) -> RuleSet {
    let mut reg = PostLayerEffects::new();
    for f in list {
        let mut m = RuleModification::new(f.kind);
        for (k, v) in &f.params {
            m.params.insert((*k).to_owned(), v.clone());
        }
        reg.register_rule(
            e(f.id),
            RuleEffect {
                source: Some(o(900)),
                controller: p(2),
                timestamp: Timestamp::new(f.ts),
                origin: EffectOrigin::Static { granted_at: None },
                players: PlayerAffected::Fixed(f.players.iter().map(|&n| p(n)).collect()),
                modification: m,
            },
        )
        .expect("register rule");
    }
    build_rule_set(&reg, &Env)
}

fn timing(active: u64, phase: Phase, stack_empty: bool) -> TimingContext {
    TimingContext {
        player: p(1),
        active_player: Some(p(active)),
        phase: Some(phase),
        step: None,
        stack_empty,
    }
}

/// Player 1's precombat main phase with an empty stack.
fn main1() -> TimingContext {
    timing(1, Phase::PrecombatMain, true)
}

/// A fake host: everything is a field.
#[derive(Clone)]
struct Host {
    rules: RuleSet,
    timing: TimingContext,
    played: i64,
    lands: Vec<LandCandidate>,
    casts: Vec<CastCandidate>,
    abilities: Vec<AbilityCandidate>,
    face_up: Vec<TurnFaceUpCandidate>,
    specials: Vec<SpecialCandidate>,
}

impl Host {
    fn new() -> Host {
        Host {
            rules: RuleSet::default(),
            timing: main1(),
            played: 0,
            lands: Vec::new(),
            casts: Vec::new(),
            abilities: Vec::new(),
            face_up: Vec::new(),
            specials: Vec::new(),
        }
    }

    fn rules(mut self, list: &[Fx]) -> Host {
        self.rules = rules(list);
        self
    }

    fn timing(mut self, t: TimingContext) -> Host {
        self.timing = t;
        self
    }

    fn played(mut self, n: i64) -> Host {
        self.played = n;
        self
    }

    fn land(mut self, c: LandCandidate) -> Host {
        self.lands.push(c);
        self
    }

    fn cast(mut self, c: CastCandidate) -> Host {
        self.casts.push(c);
        self
    }

    fn ability(mut self, c: AbilityCandidate) -> Host {
        self.abilities.push(c);
        self
    }

    fn face_up(mut self, c: TurnFaceUpCandidate) -> Host {
        self.face_up.push(c);
        self
    }

    fn special(mut self, c: SpecialCandidate) -> Host {
        self.specials.push(c);
        self
    }
}

impl ActionHost for Host {
    fn rule_set(&self) -> &RuleSet {
        &self.rules
    }
    fn timing(&self, _player: PlayerId) -> TimingContext {
        self.timing
    }
    fn lands_played_this_turn(&self, _player: PlayerId) -> i64 {
        self.played
    }
    fn land_candidates(&self, _player: PlayerId) -> Vec<LandCandidate> {
        self.lands.clone()
    }
    fn cast_candidates(&self, _player: PlayerId) -> Vec<CastCandidate> {
        self.casts.clone()
    }
    fn ability_candidates(&self, _player: PlayerId) -> Vec<AbilityCandidate> {
        self.abilities.clone()
    }
    fn turn_face_up_candidates(&self, _player: PlayerId) -> Vec<TurnFaceUpCandidate> {
        self.face_up.clone()
    }
    fn special_candidates(&self, _player: PlayerId) -> Vec<SpecialCandidate> {
        self.specials.clone()
    }
}

fn land(id: u64, zone: Zone) -> LandCandidate {
    LandCandidate {
        object: o(id),
        face_index: 0,
        zone,
        own_permission: None,
    }
}

fn spell(id: u64, zone: Zone, instant: bool) -> CastCandidate {
    CastCandidate {
        object: o(id),
        face_index: 0,
        zone,
        cast_mode: "NORMAL".to_owned(),
        is_land: false,
        instant_timing: instant,
        own_permission: None,
    }
}

fn ability(id: u64, ab: &str) -> AbilityCandidate {
    AbilityCandidate {
        object: o(id),
        ability_id: ab.to_owned(),
        zone: Zone::Battlefield,
        timing: None,
        limits: None,
        uses_this_turn: 0,
        uses_this_game: 0,
        is_mana_ability: false,
        requires_tap_symbol: false,
        summoning_sick: false,
        only_if_met: true,
    }
}

fn face_down(id: u64, origin: FaceDownOrigin) -> TurnFaceUpCandidate {
    TurnFaceUpCandidate {
        object: o(id),
        on_battlefield: true,
        merged_instant_or_sorcery: false,
        origin,
    }
}

fn special(kind: &str, id: u64) -> SpecialCandidate {
    SpecialCandidate {
        kind: kind.to_owned(),
        object: o(id),
        could_begin_to_cast: true,
        available: true,
    }
}

fn play(id: u64) -> ChoiceResponse {
    ChoiceResponse::PlayLand {
        object: o(id),
        face_index: 0,
    }
}

fn cast_r(id: u64, face: u32, mode: &str) -> ChoiceResponse {
    ChoiceResponse::InitiateCast {
        object: o(id),
        face_index: face,
        cast_mode: mode.to_owned(),
    }
}

fn act(id: u64, ab: &str) -> ChoiceResponse {
    ChoiceResponse::Activate {
        object: o(id),
        ability_id: ab.to_owned(),
    }
}

fn sa(kind: &str, id: u64) -> ChoiceResponse {
    ChoiceResponse::SpecialAction {
        kind: kind.to_owned(),
        object: o(id),
    }
}

fn actions(h: &Host) -> Vec<ChoiceResponse> {
    priority_actions(h, p(1))
}

fn why(h: &Host, r: &ChoiceResponse) -> Legality {
    explain(h, p(1), r)
}

fn illegal(reasons: Vec<Reason>) -> Legality {
    Legality::Illegal { reasons }
}

fn legal(basis: Vec<Allowance>) -> Legality {
    Legality::Legal { basis }
}

fn rule(cr: &'static str) -> Allowance {
    Allowance::Rule { cr }
}

fn eff(id: u64) -> Allowance {
    Allowance::Effect { id: e(id) }
}

fn prohibited(id: u64) -> Reason {
    Reason::Prohibited { by: e(id) }
}

fn idx(n: u64) -> usize {
    usize::try_from(n).expect("small index")
}

fn small(n: u64) -> u32 {
    u32::try_from(n).expect("small value")
}

/// Asserts the action's legality and that it is offered exactly when legal (REQ-DSL-013).
fn check(h: &Host, r: &ChoiceResponse, expected: &Legality) {
    assert_eq!(&why(h, r), expected, "explain {r:?}");
    let offered = actions(h).contains(r);
    assert_eq!(
        offered,
        matches!(expected, Legality::Legal { .. }),
        "offered {r:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// TimingContext
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-013; CR 305.3, 307.5: the timing helpers.
#[test]
fn req_dsl_013_timing_context_helpers() {
    let t = main1();
    assert!(t.is_own_turn() && t.is_main_phase() && t.sorcery_timing() && !t.in_combat());
    let post = timing(1, Phase::PostcombatMain, true);
    assert!(post.is_main_phase() && post.sorcery_timing());
    assert!(!timing(1, Phase::PrecombatMain, false).sorcery_timing());
    assert!(!timing(2, Phase::PrecombatMain, true).sorcery_timing());
    let combat = timing(1, Phase::Combat, true);
    assert!(combat.in_combat() && !combat.is_main_phase() && !combat.sorcery_timing());
    for ph in [Phase::Beginning, Phase::Ending] {
        assert!(!timing(1, ph, true).is_main_phase());
    }
    let none = TimingContext {
        player: p(1),
        active_player: None,
        phase: None,
        step: Some(Step::Upkeep),
        stack_empty: true,
    };
    assert!(!none.is_own_turn() && !none.is_main_phase() && !none.in_combat());
}

// ---------------------------------------------------------------------------------------------
// Play a land
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-013; CR 116.2a, 305.1: a land from the hand in the player's main phase.
#[test]
fn req_dsl_013_land_from_hand_is_legal() {
    let h = Host::new().land(land(10, Zone::Hand));
    check(&h, &play(10), &legal(vec![rule("CR 305.1")]));
    assert_eq!(actions(&h), vec![play(10)]);
}

/// REQ-DSL-013; CR 305.3, 116.2a: not their turn reports only OwnTurn; wrong phase or a
/// non-empty stack reports SorceryTiming.
#[test]
fn req_dsl_013_land_timing() {
    let base = Host::new().land(land(10, Zone::Hand));
    for t in [
        timing(2, Phase::PrecombatMain, true),
        timing(2, Phase::Combat, false),
    ] {
        check(
            &base.clone().timing(t),
            &play(10),
            &illegal(vec![Reason::Timing(TimingRequirement::OwnTurn)]),
        );
    }
    for t in [
        timing(1, Phase::Combat, true),
        timing(1, Phase::Beginning, true),
        timing(1, Phase::PrecombatMain, false),
        timing(1, Phase::PostcombatMain, false),
    ] {
        check(
            &base.clone().timing(t),
            &play(10),
            &illegal(vec![Reason::Timing(TimingRequirement::SorceryTiming)]),
        );
    }
    let h = base.timing(timing(1, Phase::PostcombatMain, true));
    check(&h, &play(10), &legal(vec![rule("CR 305.1")]));
}

/// REQ-DSL-013; CR 305.2a, 305.2b: one land per turn by default.
#[test]
fn req_dsl_013_land_count_exhausted() {
    let h = Host::new().land(land(10, Zone::Hand)).played(1);
    check(
        &h,
        &play(10),
        &illegal(vec![Reason::LandPlaysExhausted {
            allowed: 1,
            played: 1,
        }]),
    );
}

/// REQ-DSL-013; CR 305.2, 305.2a: one additional land play: the second land is legal with the
/// effect in its basis, the third is not.
#[test]
fn req_dsl_013_additional_land_play() {
    let extra = Fx::bare(5, 1, RuleKind::AdditionalLandPlay);
    let h = Host::new().land(land(10, Zone::Hand)).rules(&[extra]);
    check(&h, &play(10), &legal(vec![rule("CR 305.1")]));
    let h = h.played(1);
    check(&h, &play(10), &legal(vec![rule("CR 305.1"), eff(5)]));
    let h = h.played(2);
    check(
        &h,
        &play(10),
        &illegal(vec![Reason::LandPlaysExhausted {
            allowed: 2,
            played: 2,
        }]),
    );
}

/// REQ-ARCH-002, REQ-DSL-013; CR 101.2 (its example), 305.2b: an additional land play and a
/// "can't play lands" prohibition: no land, in both timestamp orders.
#[test]
fn req_arch_002_additional_land_play_and_prohibition() {
    for (t_extra, t_no) in [(1, 2), (2, 1)] {
        let h = Host::new().land(land(10, Zone::Hand)).rules(&[
            Fx::bare(5, t_extra, RuleKind::AdditionalLandPlay),
            fx(6, t_no, RuleKind::Prohibition, ACTION_PLAY_LAND),
        ]);
        for played in [0, 1] {
            check(
                &h.clone().played(played),
                &play(10),
                &illegal(vec![
                    prohibited(6),
                    Reason::LandPlaysExhausted { allowed: 0, played },
                ]),
            );
        }
    }
}

/// REQ-ARCH-002, REQ-DSL-013; CR 101.2, 305.1: PLAY_LAND matrix: permission only, prohibition
/// only, both in both timestamp orders, another player's prohibition.
#[test]
fn req_arch_002_play_land_matrix() {
    let perm = |ts| fx(5, ts, RuleKind::PlayPermission, ACTION_PLAY_LAND).with("zone", "GRAVEYARD");
    let no = |ts| fx(6, ts, RuleKind::Prohibition, ACTION_PLAY_LAND);
    let base = Host::new().land(land(10, Zone::Graveyard));
    // Without a permission: NoPermission.
    check(&base, &play(10), &illegal(vec![Reason::NoPermission]));
    // Permission only.
    check(
        &base.clone().rules(&[perm(1)]),
        &play(10),
        &legal(vec![eff(5)]),
    );
    // Prohibition only.
    check(
        &base.clone().rules(&[no(1)]),
        &play(10),
        &illegal(vec![
            prohibited(6),
            Reason::NoPermission,
            Reason::LandPlaysExhausted {
                allowed: 0,
                played: 0,
            },
        ]),
    );
    // Both, prohibition older and newer.
    for (tp, tn) in [(1, 2), (2, 1)] {
        check(
            &base.clone().rules(&[perm(tp), no(tn)]),
            &play(10),
            &illegal(vec![
                prohibited(6),
                Reason::LandPlaysExhausted {
                    allowed: 0,
                    played: 0,
                },
            ]),
        );
    }
    // A prohibition for player 2 only does not affect player 1.
    let h = base.rules(&[perm(1), no(2).for_players(&[2])]);
    check(&h, &play(10), &legal(vec![eff(5)]));
}

/// REQ-DSL-013; CR 305.1: zone and object filters of PLAY_PERMISSION; own permission; the
/// earliest matching permission in application order is the basis.
#[test]
fn req_dsl_013_land_permission_filters() {
    let base = Host::new().land(land(10, Zone::Graveyard));
    let wrong_zone = fx(5, 1, RuleKind::PlayPermission, ACTION_PLAY_LAND).with("zone", "EXILE");
    let wrong_obj = fx(6, 2, RuleKind::PlayPermission, ACTION_PLAY_LAND).with("object", 11u64);
    let right_obj = fx(7, 3, RuleKind::PlayPermission, ACTION_PLAY_LAND).with("object", 10u64);
    let other_action = fx(8, 0, RuleKind::PlayPermission, ACTION_CAST);
    let h = base
        .clone()
        .rules(&[wrong_zone.clone(), wrong_obj.clone(), other_action.clone()]);
    check(&h, &play(10), &illegal(vec![Reason::NoPermission]));
    let any = fx(9, 4, RuleKind::PlayPermission, ACTION_PLAY_LAND);
    let h = base.rules(&[wrong_zone, wrong_obj, other_action, any, right_obj]);
    // right_obj (timestamp 3) applies before any (timestamp 4).
    check(&h, &play(10), &legal(vec![eff(7)]));
    // Own permission of the card.
    let mut c = land(10, Zone::Graveyard);
    c.own_permission = Some("test.self_replay".to_owned());
    check(
        &Host::new().land(c),
        &play(10),
        &legal(vec![Allowance::Own {
            label: "test.self_replay".to_owned(),
        }]),
    );
}

/// REQ-DSL-013; CR 305.9: a land that is also another type is never offered as a cast, only as
/// a land play.
#[test]
fn req_dsl_013_land_is_never_cast() {
    let mut c = spell(10, Zone::Hand, true);
    c.is_land = true;
    let h = Host::new().land(land(10, Zone::Hand)).cast(c);
    assert_eq!(actions(&h), vec![play(10)]);
    check(&h, &cast_r(10, 0, "NORMAL"), &illegal(vec![Reason::IsLand]));
}

// ---------------------------------------------------------------------------------------------
// Cast
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-013; CR 117.1a: a noninstant spell needs sorcery timing; an instant does not.
#[test]
fn req_dsl_013_cast_timing() {
    let h = Host::new()
        .cast(spell(10, Zone::Hand, false))
        .cast(spell(11, Zone::Hand, true));
    check(
        &h,
        &cast_r(10, 0, "NORMAL"),
        &legal(vec![rule("CR 117.1a")]),
    );
    check(
        &h,
        &cast_r(11, 0, "NORMAL"),
        &legal(vec![rule("CR 117.1a")]),
    );
    for t in [
        timing(2, Phase::PrecombatMain, true),
        timing(1, Phase::PrecombatMain, false),
        timing(1, Phase::Combat, true),
    ] {
        let h = h.clone().timing(t);
        check(
            &h,
            &cast_r(10, 0, "NORMAL"),
            &illegal(vec![Reason::Timing(TimingRequirement::SorceryTiming)]),
        );
        check(
            &h,
            &cast_r(11, 0, "NORMAL"),
            &legal(vec![rule("CR 117.1a")]),
        );
    }
}

/// REQ-DSL-013, REQ-ARCH-002; CR 601.3b-d, 101.2: a TIMING_PERMISSION lets a noninstant spell
/// be cast as though it had flash; filters apply; a prohibition still wins in both orders.
#[test]
fn req_dsl_013_cast_flash_grant() {
    let off = timing(2, Phase::Ending, false);
    let grant = fx(5, 1, RuleKind::TimingPermission, ACTION_CAST);
    let h = Host::new()
        .cast(spell(10, Zone::Hand, false))
        .timing(off)
        .rules(std::slice::from_ref(&grant));
    check(&h, &cast_r(10, 0, "NORMAL"), &legal(vec![eff(5)]));
    check(
        &h.clone().rules(&[grant.with("object", 11u64)]),
        &cast_r(10, 0, "NORMAL"),
        &illegal(vec![Reason::Timing(TimingRequirement::SorceryTiming)]),
    );
    for (tg, tn) in [(1, 2), (2, 1)] {
        let h = h.clone().rules(&[
            fx(5, tg, RuleKind::TimingPermission, ACTION_CAST),
            fx(6, tn, RuleKind::Prohibition, ACTION_CAST),
        ]);
        check(&h, &cast_r(10, 0, "NORMAL"), &illegal(vec![prohibited(6)]));
    }
}

/// REQ-ARCH-002, REQ-DSL-013; CR 601.3, 101.2: CAST matrix from the graveyard: no permission,
/// permission only, prohibition only, both in both orders, another player's prohibition, and
/// zone/object filters on the prohibition.
#[test]
fn req_arch_002_cast_matrix() {
    let perm = |ts| fx(5, ts, RuleKind::CastPermission, ACTION_CAST).with("zone", "GRAVEYARD");
    let no = |ts| fx(6, ts, RuleKind::Prohibition, ACTION_CAST);
    let base = Host::new().cast(spell(10, Zone::Graveyard, true));
    let r = cast_r(10, 0, "NORMAL");
    let ok = legal(vec![eff(5), rule("CR 117.1a")]);
    check(&base, &r, &illegal(vec![Reason::NoPermission]));
    check(&base.clone().rules(&[perm(1)]), &r, &ok);
    check(
        &base.clone().rules(&[no(1)]),
        &r,
        &illegal(vec![prohibited(6), Reason::NoPermission]),
    );
    for (tp, tn) in [(1, 2), (2, 1)] {
        check(
            &base.clone().rules(&[perm(tp), no(tn)]),
            &r,
            &illegal(vec![prohibited(6)]),
        );
    }
    check(
        &base.clone().rules(&[perm(1), no(2).for_players(&[2])]),
        &r,
        &ok,
    );
    // A prohibition limited to another zone or object does not apply; one for this object does.
    check(
        &base.clone().rules(&[perm(1), no(2).with("zone", "HAND")]),
        &r,
        &ok,
    );
    check(
        &base.clone().rules(&[perm(1), no(2).with("object", 99u64)]),
        &r,
        &ok,
    );
    check(
        &base.rules(&[perm(1), no(2).with("object", 10u64)]),
        &r,
        &illegal(vec![prohibited(6)]),
    );
    // A hand spell is unaffected by a graveyard-only prohibition, and a graveyard permission
    // does not touch it.
    let h = Host::new()
        .cast(spell(11, Zone::Hand, true))
        .rules(&[no(1).with("zone", "GRAVEYARD"), perm(2)]);
    check(
        &h,
        &cast_r(11, 0, "NORMAL"),
        &legal(vec![rule("CR 117.1a")]),
    );
}

/// REQ-DSL-013; CR 601.3: an own permission of the card allows casting from another zone.
#[test]
fn req_dsl_013_cast_own_permission() {
    let mut c = spell(10, Zone::Exile, false);
    c.own_permission = Some("test.cast_from_exile".to_owned());
    check(
        &Host::new().cast(c),
        &cast_r(10, 0, "NORMAL"),
        &legal(vec![
            Allowance::Own {
                label: "test.cast_from_exile".to_owned(),
            },
            rule("CR 117.1a"),
        ]),
    );
}

/// REQ-DSL-013; CR 601.2b, 712.8b-c, 709.3a: every face and cast mode is its own action,
/// checked separately.
#[test]
fn req_dsl_013_cast_faces_and_modes_separate() {
    let mut back = spell(10, Zone::Hand, true);
    back.face_index = 1;
    let mut alt = spell(10, Zone::Hand, false);
    alt.cast_mode = "TEST_ALT".to_owned();
    let front = spell(10, Zone::Hand, false);
    let h = Host::new()
        .cast(alt)
        .cast(back)
        .cast(front)
        .timing(timing(1, Phase::Combat, true));
    // Only the instant-speed back face is legal in combat.
    assert_eq!(actions(&h), vec![cast_r(10, 1, "NORMAL")]);
    let h = h.timing(main1());
    assert_eq!(
        actions(&h),
        vec![
            cast_r(10, 0, "NORMAL"),
            cast_r(10, 0, "TEST_ALT"),
            cast_r(10, 1, "NORMAL"),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Activate
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-013; CR 117.1b, 602.5d, 602.5e: every ActivationTiming, true and false.
#[test]
fn req_dsl_013_activation_timing_variants() {
    let with = |t: Option<ActivationTiming>| {
        let mut c = ability(10, "a");
        c.timing = t;
        c
    };
    let own_main = main1();
    let own_combat = timing(1, Phase::Combat, true);
    let opp_main = timing(2, Phase::PrecombatMain, true);
    let ok = legal(vec![rule("CR 117.1b")]);
    let fail = |r| illegal(vec![Reason::Timing(r)]);
    let cases: Vec<(Option<ActivationTiming>, TimingContext, Legality)> = vec![
        (None, opp_main, ok.clone()),
        (Some(ActivationTiming::Instant), opp_main, ok.clone()),
        (Some(ActivationTiming::Sorcery), own_main, ok.clone()),
        (
            Some(ActivationTiming::Sorcery),
            own_combat,
            fail(TimingRequirement::SorceryTiming),
        ),
        (Some(ActivationTiming::YourTurn), own_combat, ok.clone()),
        (
            Some(ActivationTiming::YourTurn),
            opp_main,
            fail(TimingRequirement::OwnTurn),
        ),
        (Some(ActivationTiming::OpponentTurn), opp_main, ok.clone()),
        (
            Some(ActivationTiming::OpponentTurn),
            own_main,
            fail(TimingRequirement::OpponentTurn),
        ),
        (Some(ActivationTiming::CombatOnly), own_combat, ok),
        (
            Some(ActivationTiming::CombatOnly),
            own_main,
            fail(TimingRequirement::Combat),
        ),
    ];
    for (t, ctx, expected) in cases {
        let h = Host::new().ability(with(t)).timing(ctx);
        check(&h, &act(10, "a"), &expected);
    }
}

/// REQ-DSL-013; CR 117.1d, 605.3a, 605.1, 602.5d: an unrestricted mana ability is legal whenever
/// the player has priority; an explicit timing restriction still binds it; CR 602.5a still
/// applies.
#[test]
fn req_dsl_013_mana_ability_ignores_timing() {
    let mut c = ability(10, "m");
    c.is_mana_ability = true;
    // No stated timing: legal in combat on another player's turn.
    let h = Host::new()
        .ability(c.clone())
        .timing(timing(2, Phase::Combat, false));
    check(&h, &act(10, "m"), &legal(vec![rule("CR 605.3a")]));
    // CR 602.5d: "Activate only as a sorcery" still binds the mana ability.
    c.timing = Some(ActivationTiming::Sorcery);
    let h = Host::new()
        .ability(c.clone())
        .timing(timing(2, Phase::Combat, false));
    check(
        &h,
        &act(10, "m"),
        &illegal(vec![Reason::Timing(TimingRequirement::SorceryTiming)]),
    );
    // The same ability at sorcery timing is legal.
    let h = Host::new().ability(c.clone()).timing(main1());
    check(&h, &act(10, "m"), &legal(vec![rule("CR 605.3a")]));
    c.requires_tap_symbol = true;
    c.summoning_sick = true;
    check(
        &Host::new().ability(c),
        &act(10, "m"),
        &illegal(vec![Reason::SummoningSick]),
    );
}

/// REQ-DSL-013; CR 800.4j: OpponentTurn needs an active player other than the acting player;
/// with no active player it is illegal, on a real opponent's turn it is legal.
#[test]
fn req_dsl_013_opponent_turn_needs_active_player() {
    let mut c = ability(10, "a");
    c.timing = Some(ActivationTiming::OpponentTurn);
    let mut none = main1();
    none.active_player = None;
    let h = Host::new().ability(c.clone()).timing(none);
    check(
        &h,
        &act(10, "a"),
        &illegal(vec![Reason::Timing(TimingRequirement::OpponentTurn)]),
    );
    let h = Host::new()
        .ability(c)
        .timing(timing(2, Phase::PrecombatMain, true));
    check(&h, &act(10, "a"), &legal(vec![rule("CR 117.1b")]));
}

/// REQ-DSL-013; CR 305.2: an additional land play with `"count": 0` is not part of the basis of
/// a second land play; the effect with a positive count is.
#[test]
fn req_dsl_013_land_basis_excludes_zero_count() {
    let one = Fx::bare(5, 1, RuleKind::AdditionalLandPlay).with("count", 1);
    let zero = Fx::bare(6, 2, RuleKind::AdditionalLandPlay).with("count", 0);
    let h = Host::new()
        .land(land(10, Zone::Hand))
        .rules(&[one, zero])
        .played(1);
    check(&h, &play(10), &legal(vec![rule("CR 305.1"), eff(5)]));
}

/// REQ-DSL-013; CR 701.40b, 701.40c: for one face-down object a legal candidate (the morph
/// route) plus an illegal manifest candidate with no mana cost yields a legal response.
#[test]
fn req_dsl_013_turn_face_up_candidates_merge() {
    let legal_c = face_down(40, FaceDownOrigin::Morph { has_cost: true });
    let bad = face_down(
        40,
        FaceDownOrigin::Manifest {
            creature_card: true,
            has_mana_cost: false,
            instant_or_sorcery: false,
        },
    );
    let h = Host::new().face_up(bad).face_up(legal_c);
    check(&h, &sa("TURN_FACE_UP", 40), &legal(vec![rule("CR 116.2b")]));
}

/// REQ-DSL-013; CR 602.5b: per-turn and per-game limits.
#[test]
fn req_dsl_013_activation_limits() {
    let with = |per_turn, per_game, turn, game| {
        let mut c = ability(10, "a");
        c.limits = Some(ActivationLimit { per_turn, per_game });
        c.uses_this_turn = turn;
        c.uses_this_game = game;
        Host::new().ability(c)
    };
    let ok = legal(vec![rule("CR 117.1b")]);
    let used = illegal(vec![Reason::LimitReached]);
    check(&with(Some(1), None, 0, 5), &act(10, "a"), &ok);
    check(&with(Some(1), None, 1, 1), &act(10, "a"), &used);
    check(&with(Some(2), None, 1, 1), &act(10, "a"), &ok);
    check(&with(None, Some(2), 0, 1), &act(10, "a"), &ok);
    check(&with(None, Some(2), 0, 2), &act(10, "a"), &used);
    check(&with(Some(3), Some(1), 0, 1), &act(10, "a"), &used);
}

/// REQ-DSL-013; CR 602.5a: {T}/{Q} abilities of a summoning-sick creature; sickness alone or
/// the tap symbol alone does not matter.
#[test]
fn req_dsl_013_summoning_sickness() {
    for (sick, tap, ok) in [
        (true, true, false),
        (true, false, true),
        (false, true, true),
    ] {
        let mut c = ability(10, "a");
        c.summoning_sick = sick;
        c.requires_tap_symbol = tap;
        let expected = if ok {
            legal(vec![rule("CR 117.1b")])
        } else {
            illegal(vec![Reason::SummoningSick])
        };
        check(&Host::new().ability(c), &act(10, "a"), &expected);
    }
}

/// REQ-DSL-013; CR 602.5: a host-evaluated "activate only if" condition.
#[test]
fn req_dsl_013_activation_condition() {
    let mut c = ability(10, "a");
    c.only_if_met = false;
    check(
        &Host::new().ability(c),
        &act(10, "a"),
        &illegal(vec![Reason::ConditionNotMet]),
    );
}

/// REQ-ARCH-002, REQ-DSL-013; CR 602.5, 101.2: ACTIVATE matrix: a permission-kind entry does not
/// change activation, a prohibition forbids it in both timestamp orders, another player's
/// prohibition does not, object/zone filters.
#[test]
fn req_arch_002_activate_matrix() {
    let perm = |ts| fx(5, ts, RuleKind::TimingPermission, ACTION_ACTIVATE);
    let no = |ts| fx(6, ts, RuleKind::Prohibition, ACTION_ACTIVATE);
    let base = Host::new().ability(ability(10, "a"));
    let ok = legal(vec![rule("CR 117.1b")]);
    check(&base.clone().rules(&[perm(1)]), &act(10, "a"), &ok);
    check(
        &base.clone().rules(&[no(1)]),
        &act(10, "a"),
        &illegal(vec![prohibited(6)]),
    );
    for (tp, tn) in [(1, 2), (2, 1)] {
        check(
            &base.clone().rules(&[perm(tp), no(tn)]),
            &act(10, "a"),
            &illegal(vec![prohibited(6)]),
        );
    }
    check(
        &base.clone().rules(&[no(1).for_players(&[2])]),
        &act(10, "a"),
        &ok,
    );
    check(
        &base.clone().rules(&[no(1).with("object", 11u64)]),
        &act(10, "a"),
        &ok,
    );
    check(
        &base.clone().rules(&[no(1).with("zone", "GRAVEYARD")]),
        &act(10, "a"),
        &ok,
    );
    check(
        &base.rules(&[no(1).with("object", 10u64).with("zone", "BATTLEFIELD")]),
        &act(10, "a"),
        &illegal(vec![prohibited(6)]),
    );
}

// ---------------------------------------------------------------------------------------------
// Special actions
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-013; CR 116.2b-k, 116.2m: the registry order, rules and timings; PLAY_LAND (CR
/// 116.2a) is not a special-action kind.
#[test]
fn req_dsl_013_registry_table() {
    let reg = SpecialActionRegistry::standard();
    let table: Vec<(&str, &str, SpecialTiming)> =
        reg.kinds().iter().map(|k| (k.id, k.cr, k.timing)).collect();
    assert_eq!(
        table,
        vec![
            ("TURN_FACE_UP", "CR 116.2b", SpecialTiming::AnyPriority),
            ("END_EFFECT", "CR 116.2c", SpecialTiming::AnyPriority),
            (
                "IGNORE_STATIC_EFFECT",
                "CR 116.2d",
                SpecialTiming::AnyPriority
            ),
            (
                "DISCARD_AS_INSTANT",
                "CR 116.2e",
                SpecialTiming::AnyPriority
            ),
            (
                "SUSPEND",
                "CR 116.2f",
                SpecialTiming::AnyPriorityIfCouldBeginToCast
            ),
            ("COMPANION", "CR 116.2g", SpecialTiming::SorceryTiming),
            ("FORETELL", "CR 116.2h", SpecialTiming::OwnTurn),
            ("ROLL_PLANAR_DIE", "CR 116.2i", SpecialTiming::SorceryTiming),
            (
                "TURN_CONSPIRACY_FACE_UP",
                "CR 116.2j",
                SpecialTiming::AnyPriority
            ),
            ("PLOT", "CR 116.2k", SpecialTiming::SorceryTiming),
            ("UNLOCK", "CR 116.2m", SpecialTiming::SorceryTiming),
        ]
    );
    assert!(reg.get(ACTION_PLAY_LAND).is_none());
    assert_eq!(reg.get("PLOT").map(|k| k.cr), Some("CR 116.2k"));
    assert_eq!(SpecialActionRegistry::default(), reg);
}

/// REQ-DSL-013; CR 117.1c, 116.2c-m: every registry kind from `special_candidates` in every
/// timing group, true and false.
#[test]
fn req_dsl_013_special_kinds_timing() {
    let reg = SpecialActionRegistry::standard();
    let contexts = [
        main1(),
        timing(1, Phase::Combat, true),
        timing(1, Phase::PrecombatMain, false),
        timing(2, Phase::PrecombatMain, true),
    ];
    for k in reg.kinds().iter().filter(|k| k.id != "TURN_FACE_UP") {
        for t in contexts {
            for could in [true, false] {
                let mut c = special(k.id, 20);
                c.could_begin_to_cast = could;
                let h = Host::new().special(c).timing(t);
                let failed = match k.timing {
                    SpecialTiming::AnyPriority => None,
                    SpecialTiming::SorceryTiming => {
                        (!t.sorcery_timing()).then_some(TimingRequirement::SorceryTiming)
                    }
                    SpecialTiming::OwnTurnStackEmpty => (!(t.is_own_turn() && t.stack_empty))
                        .then_some(TimingRequirement::OwnTurnStackEmpty),
                    SpecialTiming::OwnTurn => {
                        (!t.is_own_turn()).then_some(TimingRequirement::OwnTurn)
                    }
                    SpecialTiming::AnyPriorityIfCouldBeginToCast => {
                        (!could).then_some(TimingRequirement::CouldBeginToCast)
                    }
                };
                let expected = match failed {
                    None => legal(vec![rule(k.cr)]),
                    Some(r) => illegal(vec![Reason::Timing(r)]),
                };
                check(&h, &sa(k.id, 20), &expected);
            }
        }
    }
    // Spot checks of each group, independent of the formula above.
    let h = Host::new()
        .timing(timing(1, Phase::Combat, false))
        .special(special("FORETELL", 1))
        .special(special("PLOT", 2))
        .special(special("COMPANION", 3))
        .special(special("END_EFFECT", 4));
    assert_eq!(actions(&h), vec![sa("END_EFFECT", 4), sa("FORETELL", 1)]);
    // CR 702.170a: PLOT needs the main phase, so the end step does not offer it.
    let h = h.timing(timing(1, Phase::Ending, true));
    assert_eq!(actions(&h), vec![sa("END_EFFECT", 4), sa("FORETELL", 1)]);
    let h = h.timing(main1());
    assert_eq!(
        actions(&h),
        vec![
            sa("COMPANION", 3),
            sa("END_EFFECT", 4),
            sa("FORETELL", 1),
            sa("PLOT", 2)
        ]
    );
    let h = h.timing(timing(2, Phase::PrecombatMain, true));
    assert_eq!(actions(&h), vec![sa("END_EFFECT", 4)]);
}

/// REQ-DSL-013; CR 116.2: unknown kinds and unavailable actions are never offered; PLAY_LAND is
/// not a special-action kind; a TURN_FACE_UP from `special_candidates` is not used.
#[test]
fn req_dsl_013_special_unknown_and_unavailable() {
    let mut off = special("COMPANION", 3);
    off.available = false;
    let h = Host::new()
        .special(special("TEST_UNKNOWN", 1))
        .special(special(ACTION_PLAY_LAND, 2))
        .special(off)
        .special(special("TURN_FACE_UP", 4));
    assert!(actions(&h).is_empty());
    check(
        &h,
        &sa("TEST_UNKNOWN", 1),
        &illegal(vec![Reason::UnknownSpecialKind]),
    );
    check(
        &h,
        &sa(ACTION_PLAY_LAND, 2),
        &illegal(vec![Reason::UnknownSpecialKind]),
    );
    check(
        &h,
        &sa("COMPANION", 3),
        &illegal(vec![Reason::NotAvailable]),
    );
    check(
        &h,
        &sa("TURN_FACE_UP", 4),
        &illegal(vec![Reason::NoSuchAction]),
    );
    // Unknown and unavailable together, after a prohibition.
    let mut both = special("TEST_UNKNOWN", 5);
    both.available = false;
    let h = Host::new()
        .special(both)
        .rules(&[fx(6, 1, RuleKind::Prohibition, ACTION_SPECIAL)]);
    check(
        &h,
        &sa("TEST_UNKNOWN", 5),
        &illegal(vec![
            prohibited(6),
            Reason::UnknownSpecialKind,
            Reason::NotAvailable,
        ]),
    );
}

/// REQ-DSL-013; CR 116.2b, 708.7, 731.2g, 702.37e, 702.168d, 701.40b, 701.40g, 701.58b: the
/// TURN_FACE_UP denial table.
#[test]
fn req_dsl_013_turn_face_up_table() {
    use FaceDownOrigin as F;
    use FaceUpDenial as D;
    let manifest = |c, m, i| F::Manifest {
        creature_card: c,
        has_mana_cost: m,
        instant_or_sorcery: i,
    };
    let cloak = |c, m, i| F::Cloak {
        creature_card: c,
        has_mana_cost: m,
        instant_or_sorcery: i,
    };
    let cases: Vec<(FaceDownOrigin, Vec<D>)> = vec![
        (F::Morph { has_cost: true }, vec![]),
        (F::Morph { has_cost: false }, vec![D::NoTurnFaceUpCost]),
        (F::Disguise { has_cost: true }, vec![]),
        (F::Disguise { has_cost: false }, vec![D::NoTurnFaceUpCost]),
        (manifest(true, true, false), vec![]),
        (manifest(false, true, false), vec![D::NotCreatureCard]),
        (manifest(true, false, false), vec![D::NoManaCost]),
        (
            manifest(false, true, true),
            vec![D::NotCreatureCard, D::InstantOrSorcery],
        ),
        (cloak(true, true, false), vec![]),
        (
            cloak(false, false, false),
            vec![D::NotCreatureCard, D::NoManaCost],
        ),
        (cloak(true, true, true), vec![D::InstantOrSorcery]),
        (
            F::Other {
                can_turn_face_up: true,
            },
            vec![],
        ),
        (
            F::Other {
                can_turn_face_up: false,
            },
            vec![D::NotAllowed],
        ),
    ];
    // CR 116.2b: any time with priority, even on an opponent's turn with a non-empty stack.
    let combat_opp = timing(2, Phase::Combat, false);
    for (origin, denials) in cases {
        let h = Host::new()
            .face_up(face_down(30, origin))
            .timing(combat_opp);
        let expected = if denials.is_empty() {
            legal(vec![rule("CR 116.2b")])
        } else {
            illegal(denials.into_iter().map(Reason::FaceUp).collect())
        };
        check(&h, &sa("TURN_FACE_UP", 30), &expected);
    }
    // A face-down spell; a merged permanent with an instant or sorcery card (reported once even
    // when the manifest says so too).
    let mut spell_fd = face_down(31, F::Morph { has_cost: true });
    spell_fd.on_battlefield = false;
    let mut merged = face_down(32, manifest(true, true, true));
    merged.merged_instant_or_sorcery = true;
    let h = Host::new().face_up(spell_fd).face_up(merged);
    check(
        &h,
        &sa("TURN_FACE_UP", 31),
        &illegal(vec![Reason::FaceUp(D::NotOnBattlefield)]),
    );
    check(
        &h,
        &sa("TURN_FACE_UP", 32),
        &illegal(vec![Reason::FaceUp(D::InstantOrSorcery)]),
    );
    let mut all = face_down(33, F::Morph { has_cost: false });
    all.on_battlefield = false;
    all.merged_instant_or_sorcery = true;
    check(
        &Host::new().face_up(all),
        &sa("TURN_FACE_UP", 33),
        &illegal(vec![
            Reason::FaceUp(D::NotOnBattlefield),
            Reason::FaceUp(D::NoTurnFaceUpCost),
            Reason::FaceUp(D::InstantOrSorcery),
        ]),
    );
}

/// REQ-ARCH-002, REQ-DSL-013; CR 116.2, 101.2: SPECIAL_ACTION matrix: a permission-kind entry
/// changes nothing; a prohibition wins in both orders; another player's prohibition does not
/// apply; kind and object filters; malformed filters match nothing (R12).
#[test]
fn req_arch_002_special_action_matrix() {
    let perm = |ts| fx(5, ts, RuleKind::PlayPermission, ACTION_SPECIAL);
    let no = |ts| fx(6, ts, RuleKind::Prohibition, ACTION_SPECIAL);
    let base = Host::new()
        .face_up(face_down(30, FaceDownOrigin::Morph { has_cost: true }))
        .special(special("FORETELL", 40));
    let up = sa("TURN_FACE_UP", 30);
    let fore = sa("FORETELL", 40);
    let ok_up = legal(vec![rule("CR 116.2b")]);
    let ok_fore = legal(vec![rule("CR 116.2h")]);
    let h = base.clone().rules(&[perm(1)]);
    check(&h, &up, &ok_up);
    check(&h, &fore, &ok_fore);
    for list in [vec![no(1)], vec![perm(1), no(2)], vec![perm(2), no(1)]] {
        let h = base.clone().rules(&list);
        check(&h, &up, &illegal(vec![prohibited(6)]));
        check(&h, &fore, &illegal(vec![prohibited(6)]));
    }
    check(&base.clone().rules(&[no(1).for_players(&[2])]), &up, &ok_up);
    // Kind filter.
    let h = base.clone().rules(&[no(1).with("kind", "FORETELL")]);
    check(&h, &up, &ok_up);
    check(&h, &fore, &illegal(vec![prohibited(6)]));
    // Object filter.
    let h = base.clone().rules(&[no(1).with("object", 30u64)]);
    check(&h, &up, &illegal(vec![prohibited(6)]));
    check(&h, &fore, &ok_fore);
    // Malformed filters match nothing.
    for bad in [
        no(1).with("kind", 7),
        no(1).with("object", "thirty"),
        no(1).with("object", -30),
    ] {
        let h = base.clone().rules(&[bad]);
        check(&h, &up, &ok_up);
        check(&h, &fore, &ok_fore);
    }
}

// ---------------------------------------------------------------------------------------------
// Untrusted params, explain, order, determinism
// ---------------------------------------------------------------------------------------------

/// REQ-DSL-013, R12: malformed or missing "action", "zone" and "object" never panic and match
/// nothing.
#[test]
fn req_dsl_013_malformed_params_match_nothing() {
    let base = Host::new()
        .cast(spell(10, Zone::Graveyard, true))
        .ability(ability(11, "a"));
    let bad = vec![
        Fx::bare(1, 1, RuleKind::CastPermission),
        fx(2, 2, RuleKind::CastPermission, ACTION_CAST).with("zone", 3),
        fx(3, 3, RuleKind::CastPermission, ACTION_CAST).with("zone", "NOT_A_ZONE"),
        fx(4, 4, RuleKind::CastPermission, ACTION_CAST).with("object", "10"),
        Fx::bare(5, 5, RuleKind::Prohibition).with("action", 12),
        fx(6, 6, RuleKind::Prohibition, ACTION_ACTIVATE).with("zone", serde_json::json!([1])),
        fx(7, 7, RuleKind::Prohibition, ACTION_ACTIVATE).with("object", serde_json::Value::Null),
    ];
    let h = base.rules(&bad);
    check(
        &h,
        &cast_r(10, 0, "NORMAL"),
        &illegal(vec![Reason::NoPermission]),
    );
    check(&h, &act(11, "a"), &legal(vec![rule("CR 117.1b")]));
}

/// REQ-DSL-013: explain of PassPriority, of non-priority responses and of unknown candidates;
/// Illegal lists Prohibited first and every failing check.
#[test]
fn req_dsl_013_explain_edges() {
    let h = Host::new();
    assert_eq!(why(&h, &ChoiceResponse::PassPriority), legal(vec![]));
    for r in [
        play(1),
        cast_r(1, 0, "NORMAL"),
        act(1, "a"),
        sa("PLOT", 1),
        ChoiceResponse::SelectModes { modes: vec![0] },
    ] {
        assert_eq!(why(&h, &r), illegal(vec![Reason::NoSuchAction]), "{r:?}");
    }
    // Every failing check of an ability, Prohibited first.
    let mut c = ability(10, "a");
    c.timing = Some(ActivationTiming::Sorcery);
    c.limits = Some(ActivationLimit {
        per_turn: Some(1),
        per_game: None,
    });
    c.uses_this_turn = 1;
    c.summoning_sick = true;
    c.requires_tap_symbol = true;
    c.only_if_met = false;
    let h = Host::new()
        .ability(c)
        .timing(timing(2, Phase::Combat, false))
        .rules(&[fx(9, 1, RuleKind::Prohibition, ACTION_ACTIVATE)]);
    assert_eq!(
        why(&h, &act(10, "a")),
        illegal(vec![
            prohibited(9),
            Reason::Timing(TimingRequirement::SorceryTiming),
            Reason::LimitReached,
            Reason::SummoningSick,
            Reason::ConditionNotMet,
        ])
    );
    // Every failing check of a cast.
    let mut s = spell(11, Zone::Library, false);
    s.is_land = true;
    let h = Host::new()
        .cast(s)
        .timing(timing(2, Phase::Combat, false))
        .rules(&[fx(9, 1, RuleKind::Prohibition, ACTION_CAST)]);
    assert_eq!(
        why(&h, &cast_r(11, 0, "NORMAL")),
        illegal(vec![
            prohibited(9),
            Reason::NoPermission,
            Reason::IsLand,
            Reason::Timing(TimingRequirement::SorceryTiming),
        ])
    );
    // Every failing check of a land play.
    let h = Host::new()
        .land(land(12, Zone::Exile))
        .played(1)
        .timing(timing(2, Phase::Combat, true));
    assert_eq!(
        why(&h, &play(12)),
        illegal(vec![
            Reason::NoPermission,
            Reason::LandPlaysExhausted {
                allowed: 1,
                played: 1,
            },
            Reason::Timing(TimingRequirement::OwnTurn),
        ])
    );
    // The earliest prohibition in application order is reported.
    let h = Host::new().ability(ability(10, "a")).rules(&[
        fx(8, 5, RuleKind::Prohibition, ACTION_ACTIVATE),
        fx(9, 1, RuleKind::Prohibition, ACTION_ACTIVATE),
    ]);
    assert_eq!(why(&h, &act(10, "a")), illegal(vec![prohibited(9)]));
}

/// REQ-DSL-013, R3: the same response from several candidates: a legal one wins, whatever the
/// host's order, so `explain` agrees with `legal_actions`; exact duplicates appear once.
#[test]
fn req_dsl_013_duplicate_responses() {
    let a = land(10, Zone::Graveyard);
    let b = land(10, Zone::Hand);
    for order in [vec![a.clone(), b.clone()], vec![b, a]] {
        let mut h = Host::new();
        h.lands = order;
        assert_eq!(actions(&h), vec![play(10)]);
        assert_eq!(why(&h, &play(10)), legal(vec![rule("CR 305.1")]));
    }
    let h = Host::new()
        .ability(ability(10, "a"))
        .ability(ability(10, "a"));
    assert_eq!(actions(&h), vec![act(10, "a")]);
    // Both illegal: the first in the candidates' total order (Battlefield sorts before
    // Graveyard, the Zone declaration order).
    let mut x = ability(10, "a");
    x.only_if_met = false;
    let mut y = x.clone();
    y.zone = Zone::Graveyard;
    for order in [vec![x.clone(), y.clone()], vec![y, x]] {
        let mut h = Host::new()
            .rules(&[fx(9, 1, RuleKind::Prohibition, ACTION_ACTIVATE).with("zone", "GRAVEYARD")]);
        h.abilities = order;
        assert_eq!(
            why(&h, &act(10, "a")),
            illegal(vec![Reason::ConditionNotMet])
        );
    }
}

/// REQ-DSL-013: legal_actions order: classes, then response fields; the basis is never empty;
/// priority_actions and priority_payload agree.
#[test]
fn req_dsl_013_output_order_and_payload() {
    let h = Host::new()
        .special(special("PLOT", 2))
        .face_up(face_down(1, FaceDownOrigin::Disguise { has_cost: true }))
        .ability(ability(5, "b"))
        .ability(ability(5, "a"))
        .ability(ability(3, "z"))
        .cast(spell(9, Zone::Hand, true))
        .cast(spell(4, Zone::Hand, true))
        .land(land(8, Zone::Hand))
        .land(land(7, Zone::Hand));
    let all = legal_actions(&h, p(1));
    assert!(all.iter().all(|a| !a.basis.is_empty()));
    let expected = vec![
        play(7),
        play(8),
        cast_r(4, 0, "NORMAL"),
        cast_r(9, 0, "NORMAL"),
        act(3, "z"),
        act(5, "a"),
        act(5, "b"),
        sa("PLOT", 2),
        sa("TURN_FACE_UP", 1),
    ];
    assert_eq!(actions(&h), expected);
    assert_eq!(
        all.iter().map(|a| a.response.clone()).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        priority_payload(&h, p(1), true),
        ChoicePayload::PriorityAction {
            can_pass: true,
            can_propose_shortcut: true,
            actions: expected,
        }
    );
}

// ---------------------------------------------------------------------------------------------
// Random hosts: determinism and submission (property-style, seeded)
// ---------------------------------------------------------------------------------------------

fn pick(rng: &mut Pcg64Dxsm, n: u64) -> u64 {
    rng.next_bounded(n).unwrap_or(0)
}

fn coin(rng: &mut Pcg64Dxsm) -> bool {
    pick(rng, 2) == 1
}

const ZONES: [Zone; 4] = [Zone::Hand, Zone::Graveyard, Zone::Exile, Zone::Battlefield];
const PHASES: [Phase; 5] = [
    Phase::Beginning,
    Phase::PrecombatMain,
    Phase::Combat,
    Phase::PostcombatMain,
    Phase::Ending,
];
const KINDS: [&str; 6] = [
    "TURN_FACE_UP",
    "SUSPEND",
    "COMPANION",
    "FORETELL",
    "PLOT",
    "TEST_UNKNOWN",
];

fn zone_of(rng: &mut Pcg64Dxsm) -> Zone {
    ZONES[idx(pick(rng, 4))]
}

fn random_fx(rng: &mut Pcg64Dxsm, id: u64) -> Fx {
    let kinds = [
        RuleKind::Prohibition,
        RuleKind::CastPermission,
        RuleKind::PlayPermission,
        RuleKind::TimingPermission,
        RuleKind::AdditionalLandPlay,
    ];
    let acts = [
        ACTION_PLAY_LAND,
        ACTION_CAST,
        ACTION_ACTIVATE,
        ACTION_SPECIAL,
    ];
    let kind = kinds[idx(pick(rng, 5))];
    let action = acts[idx(pick(rng, 4))];
    let mut f = fx(id, pick(rng, 10), kind, action);
    // Most prohibitions hit the other player so legal actions remain.
    if kind == RuleKind::Prohibition && pick(rng, 3) != 0 {
        f.players = vec![2];
    }
    if coin(rng) {
        f = f.with("zone", zone_of(rng).as_str());
    }
    if coin(rng) {
        f = f.with("object", pick(rng, 6));
    }
    if coin(rng) {
        f = f.with("kind", KINDS[idx(pick(rng, 6))]);
    }
    f
}

fn random_host(rng: &mut Pcg64Dxsm) -> Host {
    let n_fx = pick(rng, 6);
    let fxs: Vec<Fx> = (0..n_fx).map(|i| random_fx(rng, i + 1)).collect();
    let played = i64::try_from(pick(rng, 3)).expect("small");
    let mut h = Host::new().rules(&fxs).played(played);
    h.timing = TimingContext {
        player: p(1),
        active_player: Some(p(1 + pick(rng, 2))),
        phase: Some(PHASES[idx(pick(rng, 5))]),
        step: None,
        stack_empty: coin(rng),
    };
    for _ in 0..pick(rng, 4) {
        let mut c = land(pick(rng, 6), zone_of(rng));
        c.face_index = small(pick(rng, 2));
        if coin(rng) {
            c.own_permission = Some("test.own".to_owned());
        }
        h.lands.push(c);
    }
    for _ in 0..pick(rng, 5) {
        let id = pick(rng, 6);
        let zone = zone_of(rng);
        let instant = coin(rng);
        let mut c = spell(id, zone, instant);
        c.face_index = small(pick(rng, 2));
        c.cast_mode = ["NORMAL", "TEST_ALT"][idx(pick(rng, 2))].to_owned();
        c.is_land = pick(rng, 4) == 0;
        if coin(rng) {
            c.own_permission = Some("test.own".to_owned());
        }
        h.casts.push(c);
    }
    let timings = [
        None,
        Some(ActivationTiming::Instant),
        Some(ActivationTiming::Sorcery),
        Some(ActivationTiming::YourTurn),
        Some(ActivationTiming::OpponentTurn),
        Some(ActivationTiming::CombatOnly),
    ];
    for _ in 0..pick(rng, 5) {
        let id = pick(rng, 6);
        let ab = ["a", "b"][idx(pick(rng, 2))];
        let mut c = ability(id, ab);
        c.zone = zone_of(rng);
        c.timing = timings[idx(pick(rng, 6))];
        if coin(rng) {
            c.limits = Some(ActivationLimit {
                per_turn: Some(1 + small(pick(rng, 2))),
                per_game: coin(rng).then_some(2),
            });
        }
        c.uses_this_turn = small(pick(rng, 3));
        c.uses_this_game = small(pick(rng, 3));
        c.is_mana_ability = coin(rng);
        c.requires_tap_symbol = coin(rng);
        c.summoning_sick = coin(rng);
        c.only_if_met = pick(rng, 4) != 0;
        h.abilities.push(c);
    }
    for _ in 0..pick(rng, 3) {
        let origin = match pick(rng, 3) {
            0 => FaceDownOrigin::Morph {
                has_cost: coin(rng),
            },
            1 => FaceDownOrigin::Manifest {
                creature_card: coin(rng),
                has_mana_cost: coin(rng),
                instant_or_sorcery: coin(rng),
            },
            _ => FaceDownOrigin::Other {
                can_turn_face_up: coin(rng),
            },
        };
        let mut c = face_down(pick(rng, 6), origin);
        c.on_battlefield = pick(rng, 4) != 0;
        h.face_up.push(c);
    }
    for _ in 0..pick(rng, 4) {
        let kind = KINDS[idx(pick(rng, 6))];
        let mut c = special(kind, pick(rng, 6));
        c.could_begin_to_cast = coin(rng);
        c.available = pick(rng, 4) != 0;
        h.specials.push(c);
    }
    h
}

/// Every candidate's response (offered or not).
fn candidate_responses(h: &Host) -> BTreeSet<ChoiceResponse> {
    let mut out = BTreeSet::new();
    out.extend(h.lands.iter().map(|c| ChoiceResponse::PlayLand {
        object: c.object,
        face_index: c.face_index,
    }));
    out.extend(h.casts.iter().map(|c| ChoiceResponse::InitiateCast {
        object: c.object,
        face_index: c.face_index,
        cast_mode: c.cast_mode.clone(),
    }));
    out.extend(
        h.abilities
            .iter()
            .map(|c| act(c.object.get(), &c.ability_id)),
    );
    out.extend(h.face_up.iter().map(|c| sa("TURN_FACE_UP", c.object.get())));
    out.extend(h.specials.iter().map(|c| sa(&c.kind, c.object.get())));
    out
}

/// A deterministic shuffle of `v` driven by `rng` (Fisher-Yates).
fn shuffle<T>(rng: &mut Pcg64Dxsm, v: &mut [T]) {
    for i in (1..v.len()).rev() {
        let j = idx(pick(rng, u64::try_from(i + 1).expect("small")));
        v.swap(i, j);
    }
}

/// The PRIORITY_ACTION request built like the heartbeat's `ask_priority`.
fn request(h: &Host) -> PendingChoiceRequest {
    PendingChoiceRequest {
        request_id: RequestId::new(0),
        chooser: p(1),
        choice_type: ChoiceType::PriorityAction,
        reason: ChoiceReason {
            cr: "CR 117.1".to_owned(),
            source: None,
        },
        min: 1,
        max: 1,
        hidden: false,
        reveal_to: Vec::new(),
        default_on_timeout: ChoiceResponse::PassPriority,
        options: ChoiceOptions::Validator(ValidatorContract {
            min: 1,
            max: 1,
            rule: ValidatorRule::Payload,
        }),
        payload: priority_payload(h, p(1), false),
    }
}

/// REQ-DSL-013, REQ-ARCH-002, R3, R7; CR 117.1, 101.2: over 128 seeded random hosts, every
/// offered action is accepted when submitted as a PRIORITY_ACTION answer and explained Legal;
/// every other candidate response is rejected and explained Illegal; the output is strictly
/// ascending and independent of the candidates' order; an action under an applicable
/// unfiltered prohibition of its class is never offered.
#[test]
fn req_dsl_013_property_offered_actions_are_accepted() {
    let mut offered_total = 0usize;
    for seed in 0..128u64 {
        let mut rng = Pcg64Dxsm::from_u64_seed(seed);
        let h = random_host(&mut rng);
        let req = request(&h);
        let offered = actions(&h);
        offered_total += offered.len();
        assert!(offered.windows(2).all(|w| w[0] < w[1]), "seed {seed}");
        assert_eq!(req.validate(&ChoiceResponse::PassPriority), Ok(()));
        for a in &offered {
            assert_eq!(req.validate(a), Ok(()), "seed {seed} {a:?}");
            assert!(
                matches!(why(&h, a), Legality::Legal { ref basis } if !basis.is_empty()),
                "seed {seed} {a:?}"
            );
        }
        for r in candidate_responses(&h) {
            if !offered.contains(&r) {
                assert!(req.validate(&r).is_err(), "seed {seed} {r:?}");
                assert!(
                    matches!(why(&h, &r), Legality::Illegal { ref reasons } if !reasons.is_empty()),
                    "seed {seed} {r:?}"
                );
            }
        }
        // CR 101.2: an unfiltered prohibition of a class for player 1 forbids the whole class.
        for entry in h.rules.applying(RuleKind::Prohibition, p(1)) {
            let params = &entry.modification.params;
            if params.contains_key("zone") || params.contains_key("object") {
                continue;
            }
            let hit = |r: &ChoiceResponse| match (entry.action(), r) {
                (Some(ACTION_PLAY_LAND), ChoiceResponse::PlayLand { .. })
                | (Some(ACTION_CAST), ChoiceResponse::InitiateCast { .. })
                | (Some(ACTION_ACTIVATE), ChoiceResponse::Activate { .. }) => true,
                (Some(ACTION_SPECIAL), ChoiceResponse::SpecialAction { .. }) => {
                    !params.contains_key("kind")
                }
                _ => false,
            };
            assert!(!offered.iter().any(hit), "seed {seed}");
        }
        // Shuffled candidates give the identical output (R3).
        let mut s = h.clone();
        shuffle(&mut rng, &mut s.lands);
        shuffle(&mut rng, &mut s.casts);
        shuffle(&mut rng, &mut s.abilities);
        shuffle(&mut rng, &mut s.face_up);
        shuffle(&mut rng, &mut s.specials);
        assert_eq!(
            legal_actions(&s, p(1)),
            legal_actions(&h, p(1)),
            "seed {seed}"
        );
        for r in candidate_responses(&h) {
            assert_eq!(why(&s, &r), why(&h, &r), "seed {seed} {r:?}");
        }
    }
    // The generator does offer actions on these hosts (the property is not vacuous).
    assert!(offered_total > 64, "offered {offered_total}");
}

/// REQ-DSL-013, R3: reversing the candidate lists of a fixed host does not change the output.
#[test]
fn req_dsl_013_shuffled_candidates_same_output() {
    let h = Host::new()
        .land(land(2, Zone::Hand))
        .land(land(1, Zone::Graveyard))
        .cast(spell(4, Zone::Hand, true))
        .cast(spell(3, Zone::Hand, false))
        .ability(ability(6, "b"))
        .ability(ability(6, "a"))
        .face_up(face_down(8, FaceDownOrigin::Morph { has_cost: true }))
        .face_up(face_down(7, FaceDownOrigin::Morph { has_cost: true }))
        .special(special("UNLOCK", 9))
        .special(special("END_EFFECT", 9));
    let mut r = h.clone();
    r.lands.reverse();
    r.casts.reverse();
    r.abilities.reverse();
    r.face_up.reverse();
    r.specials.reverse();
    assert_eq!(legal_actions(&r, p(1)), legal_actions(&h, p(1)));
    // Land 1 (graveyard, no permission) is not offered; the other nine are.
    assert_eq!(legal_actions(&h, p(1)).len(), 9);
}
