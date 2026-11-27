// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The legal-action generator and [`explain`] (REQ-DSL-013, REQ-ARCH-002): every candidate of
//! the host is checked against the rules of CR 116.2, 117.1, 305.1-305.3, 601.3 and 602.5, the rule set's
//! permissions and prohibitions (prohibitions win, CR 101.2) and the special-action registry.

use std::collections::BTreeMap;

use fineprint_base::ids::{EffectId, ObjectId, PlayerId};
use fineprint_dsl::ability::{ActivationLimit, ActivationTiming, RuleKind};
use fineprint_dsl::expr::Zone;

use super::registry::{SpecialActionRegistry, SpecialTiming};
use super::types::{
    AbilityCandidate, ActionHost, Allowance, CastCandidate, FaceDownOrigin, FaceUpDenial,
    LandCandidate, LegalAction, Legality, Reason, SpecialCandidate, TimingContext,
    TimingRequirement, TurnFaceUpCandidate,
};
use super::{ACTION_ACTIVATE, ACTION_CAST, ACTION_PLAY_LAND, ACTION_SPECIAL};
use crate::choice::{ChoicePayload, ChoiceResponse};
use crate::layers::{RuleEntry, RuleSet, Verdict};

/// The special-action kind filled in by this unit (CR 116.2b).
const TURN_FACE_UP: &str = "TURN_FACE_UP";

/// The `"zone"` filter parameter.
const PARAM_ZONE: &str = "zone";
/// The `"object"` filter parameter.
const PARAM_OBJECT: &str = "object";
/// The `"kind"` filter parameter (special actions).
const PARAM_KIND: &str = "kind";

/// Every legal non-pass action of `player`, each with what allowed it (CR 117.1).
///
/// Ordered by class (`PLAY_LAND`, `INITIATE_CAST`, `ACTIVATE`, `SPECIAL_ACTION`), then
/// ascending by the response's fields; exact duplicates removed; independent of the host's
/// candidate order (R3).
#[must_use]
pub fn legal_actions(host: &dyn ActionHost, player: PlayerId) -> Vec<LegalAction> {
    let mut all = BTreeMap::new();
    for class in Class::ALL {
        evaluate_class(host, player, class, &mut all);
    }
    all.into_iter()
        .filter_map(|(response, legality)| match legality {
            Legality::Legal { basis } => Some(LegalAction { response, basis }),
            Legality::Illegal { .. } => None,
        })
        .collect()
}

/// The responses of [`legal_actions`], in the same order.
#[must_use]
pub fn priority_actions(host: &dyn ActionHost, player: PlayerId) -> Vec<ChoiceResponse> {
    legal_actions(host, player)
        .into_iter()
        .map(|a| a.response)
        .collect()
}

/// The `PRIORITY_ACTION` payload for `player`: passing is allowed and the actions are
/// [`priority_actions`] (CR 117.1).
#[must_use]
pub fn priority_payload(
    host: &dyn ActionHost,
    player: PlayerId,
    can_propose_shortcut: bool,
) -> ChoicePayload {
    ChoicePayload::PriorityAction {
        can_pass: true,
        can_propose_shortcut,
        actions: priority_actions(host, player),
    }
}

/// Why `action` is legal or illegal for `player`.
///
/// Passing priority is always legal with an empty basis. A response that is not a priority
/// action, or matches no candidate of the host, is illegal with [`Reason::NoSuchAction`].
/// Otherwise the checks of the matching candidate: when several candidates yield the same
/// response, a legal one is used if any (so `explain` is legal exactly when the action is in
/// [`legal_actions`]), else the first in the candidates' total order.
#[must_use]
pub fn explain(host: &dyn ActionHost, player: PlayerId, action: &ChoiceResponse) -> Legality {
    let class = match action {
        ChoiceResponse::PassPriority => return Legality::Legal { basis: Vec::new() },
        ChoiceResponse::PlayLand { .. } => Class::Land,
        ChoiceResponse::InitiateCast { .. } => Class::Cast,
        ChoiceResponse::Activate { .. } => Class::Activate,
        ChoiceResponse::SpecialAction { .. } => Class::Special,
        _ => return no_such_action(),
    };
    let mut all = BTreeMap::new();
    evaluate_class(host, player, class, &mut all);
    all.remove(action).unwrap_or_else(no_such_action)
}

/// `Illegal { [NoSuchAction] }`.
fn no_such_action() -> Legality {
    Legality::Illegal {
        reasons: vec![Reason::NoSuchAction],
    }
}

/// The classes of priority actions, in output order.
#[derive(Clone, Copy)]
enum Class {
    Land,
    Cast,
    Activate,
    Special,
}

impl Class {
    const ALL: [Class; 4] = [Class::Land, Class::Cast, Class::Activate, Class::Special];
}

/// Checks every candidate of `class` and records the result per response in `out`. Candidates
/// are visited in a total order; for a response yielded by several candidates the first legal
/// one wins, else the first one (R3: independent of the host's order).
fn evaluate_class(
    host: &dyn ActionHost,
    player: PlayerId,
    class: Class,
    out: &mut BTreeMap<ChoiceResponse, Legality>,
) {
    let ctx = Ctx {
        rules: host.rule_set(),
        timing: host.timing(player),
        player,
    };
    let results: Vec<(ChoiceResponse, Legality)> = match class {
        Class::Land => {
            let mut cands = host.land_candidates(player);
            cands.sort();
            let played = host.lands_played_this_turn(player);
            cands
                .iter()
                .map(|c| (land_response(c), check_land(&ctx, played, c)))
                .collect()
        }
        Class::Cast => {
            let mut cands = host.cast_candidates(player);
            cands.sort();
            cands
                .iter()
                .map(|c| (cast_response(c), check_cast(&ctx, c)))
                .collect()
        }
        Class::Activate => {
            let mut cands = host.ability_candidates(player);
            cands.sort_by(|a, b| ability_key(a).cmp(&ability_key(b)));
            cands
                .iter()
                .map(|c| (activate_response(c), check_activate(&ctx, c)))
                .collect()
        }
        Class::Special => {
            let registry = SpecialActionRegistry::standard();
            let mut face_up = host.turn_face_up_candidates(player);
            face_up.sort();
            let mut others = host.special_candidates(player);
            others.sort();
            let mut results: Vec<(ChoiceResponse, Legality)> = face_up
                .iter()
                .map(|c| {
                    let response = ChoiceResponse::SpecialAction {
                        kind: TURN_FACE_UP.to_owned(),
                        object: c.object,
                    };
                    (response, check_turn_face_up(&ctx, &registry, c))
                })
                .collect();
            // TURN_FACE_UP candidates come only from `turn_face_up_candidates`.
            results.extend(others.iter().filter(|c| c.kind != TURN_FACE_UP).map(|c| {
                let response = ChoiceResponse::SpecialAction {
                    kind: c.kind.clone(),
                    object: c.object,
                };
                (response, check_special(&ctx, &registry, c))
            }));
            results
        }
    };
    for (response, legality) in results {
        match out.get(&response) {
            Some(Legality::Illegal { .. }) if matches!(legality, Legality::Legal { .. }) => {
                out.insert(response, legality);
            }
            Some(_) => {}
            None => {
                out.insert(response, legality);
            }
        }
    }
}

/// The facts shared by every check of one ask.
struct Ctx<'a> {
    rules: &'a RuleSet,
    timing: TimingContext,
    player: PlayerId,
}

impl Ctx<'_> {
    /// The earliest entry of `kind` in application order that modifies the rules for the player,
    /// has the `"action"` `action` and satisfies `filter`.
    fn first(
        &self,
        kind: RuleKind,
        action: &str,
        filter: impl Fn(&RuleEntry) -> bool,
    ) -> Option<EffectId> {
        self.rules
            .applying(kind, self.player)
            .find(|e| e.action() == Some(action) && filter(e))
            .map(|e| e.id)
    }
}

/// Whether the entry's optional `"zone"` filter admits `zone` (R12: any malformed value admits
/// nothing).
fn zone_matches(entry: &RuleEntry, zone: Zone) -> bool {
    match entry.modification.params.get(PARAM_ZONE) {
        None => true,
        Some(v) => v.as_str().and_then(Zone::from_wire) == Some(zone),
    }
}

/// Whether the entry's optional `"object"` filter admits `object` (R12: any malformed value
/// admits nothing).
fn object_matches(entry: &RuleEntry, object: ObjectId) -> bool {
    match entry.modification.params.get(PARAM_OBJECT) {
        None => true,
        Some(v) => v.as_u64() == Some(object.get()),
    }
}

/// Whether the entry's optional `"kind"` filter admits the special-action kind `kind` (R12: any
/// malformed value admits nothing).
fn kind_matches(entry: &RuleEntry, kind: &str) -> bool {
    match entry.modification.params.get(PARAM_KIND) {
        None => true,
        Some(v) => v.as_str() == Some(kind),
    }
}

/// The declaration rank of a reason, for the reported order.
fn rank(r: &Reason) -> (u8, u8) {
    match r {
        Reason::NoSuchAction => (0, 0),
        Reason::Prohibited { .. } => (1, 0),
        Reason::NoPermission => (2, 0),
        Reason::LandPlaysExhausted { .. } => (3, 0),
        Reason::IsLand => (4, 0),
        Reason::Timing(_) => (5, 0),
        Reason::LimitReached => (6, 0),
        Reason::SummoningSick => (7, 0),
        Reason::ConditionNotMet => (8, 0),
        Reason::UnknownSpecialKind => (9, 0),
        Reason::NotAvailable => (10, 0),
        Reason::FaceUp(d) => (11, face_up_rank(*d)),
    }
}

/// The declaration rank of a face-up denial.
fn face_up_rank(d: FaceUpDenial) -> u8 {
    match d {
        FaceUpDenial::NotOnBattlefield => 0,
        FaceUpDenial::NoTurnFaceUpCost => 1,
        FaceUpDenial::NotCreatureCard => 2,
        FaceUpDenial::NoManaCost => 3,
        FaceUpDenial::InstantOrSorcery => 4,
        FaceUpDenial::NotAllowed => 5,
    }
}

/// Legal with `basis` when no check failed; otherwise every failing reason, `Prohibited` first,
/// then in declaration order, without repeats.
fn finish(mut reasons: Vec<Reason>, basis: Vec<Allowance>) -> Legality {
    if reasons.is_empty() {
        return Legality::Legal { basis };
    }
    reasons.sort_by_key(rank);
    reasons.dedup();
    Legality::Illegal { reasons }
}

// ---------------------------------------------------------------------------------------------
// Play a land
// ---------------------------------------------------------------------------------------------

/// The response that plays `c`.
fn land_response(c: &LandCandidate) -> ChoiceResponse {
    ChoiceResponse::PlayLand {
        object: c.object,
        face_index: c.face_index,
    }
}

/// Checks playing the land `c` when `played` lands were played this turn.
fn check_land(ctx: &Ctx<'_>, played: i64, c: &LandCandidate) -> Legality {
    let mut reasons = Vec::new();
    let mut basis = Vec::new();
    // CR 101.2, 305.2b: a "can't play lands" prohibition wins over every permission and
    // additional land play (unfiltered, the same source as `land_plays_allowed`).
    if let Verdict::Prohibited { by } =
        ctx.rules
            .query(ctx.player, ACTION_PLAY_LAND, RuleKind::PlayPermission)
    {
        reasons.push(Reason::Prohibited { by });
    }
    // CR 305.1: a land card from the hand; any other zone needs a permission.
    if c.zone == Zone::Hand {
        basis.push(Allowance::Rule { cr: "CR 305.1" });
    } else if let Some(id) = ctx.first(RuleKind::PlayPermission, ACTION_PLAY_LAND, |e| {
        zone_matches(e, c.zone) && object_matches(e, c.object)
    }) {
        basis.push(Allowance::Effect { id });
    } else if let Some(label) = &c.own_permission {
        basis.push(Allowance::Own {
            label: label.clone(),
        });
    } else {
        reasons.push(Reason::NoPermission);
    }
    // CR 305.2a, 305.2b: legal only while the lands allowed exceed the lands played.
    let allowed = ctx.rules.land_plays_allowed(ctx.player);
    if allowed <= played {
        reasons.push(Reason::LandPlaysExhausted { allowed, played });
    }
    if !ctx.timing.is_own_turn() {
        // CR 305.3: never during another player's turn.
        reasons.push(Reason::Timing(TimingRequirement::OwnTurn));
    } else if !ctx.timing.sorcery_timing() {
        // CR 116.2a, 305.1: a main phase of their turn with an empty stack.
        reasons.push(Reason::Timing(TimingRequirement::SorceryTiming));
    }
    if reasons.is_empty() && played >= 1 {
        // CR 305.2: the continuous effects that increased the number made this play legal.
        basis.extend(
            ctx.rules
                .applying(RuleKind::AdditionalLandPlay, ctx.player)
                // Same count reading as `RuleSet::land_plays_allowed`: only effects that add
                // at least one play are part of the basis.
                .filter(|e| additional_land_plays(e) > 0)
                .map(|e| Allowance::Effect { id: e.id }),
        );
    }
    finish(reasons, basis)
}

/// The number of additional land plays of an `ADDITIONAL_LAND_PLAY` entry (CR 305.2): `"count"`
/// when it is a non-negative integer, 1 when absent, 0 otherwise. Mirrors the reading of
/// `RuleSet::land_plays_allowed`.
fn additional_land_plays(e: &RuleEntry) -> i64 {
    match e.modification.params.get("count") {
        None => 1,
        Some(v) => v.as_i64().filter(|&n| n >= 0).unwrap_or(0),
    }
}

// ---------------------------------------------------------------------------------------------
// Cast a spell
// ---------------------------------------------------------------------------------------------

/// The response that begins to cast `c`.
fn cast_response(c: &CastCandidate) -> ChoiceResponse {
    ChoiceResponse::InitiateCast {
        object: c.object,
        face_index: c.face_index,
        cast_mode: c.cast_mode.clone(),
    }
}

/// Checks beginning to cast `c` (one face, one cast mode: CR 601.2b, 712.8c, 709.3a).
fn check_cast(ctx: &Ctx<'_>, c: &CastCandidate) -> Legality {
    let mut reasons = Vec::new();
    let mut basis = Vec::new();
    let filter = |e: &RuleEntry| zone_matches(e, c.zone) && object_matches(e, c.object);
    // CR 305.9: an object that is a land can be played only as a land, never cast.
    if c.is_land {
        reasons.push(Reason::IsLand);
    }
    // CR 601.3, 101.2: no rule or effect may prohibit casting it; "can't" wins.
    if let Some(by) = ctx.first(RuleKind::Prohibition, ACTION_CAST, filter) {
        reasons.push(Reason::Prohibited { by });
    }
    // CR 601.3: a rule or effect must allow casting it from its zone (by default the hand).
    if c.zone != Zone::Hand {
        if let Some(id) = ctx.first(RuleKind::CastPermission, ACTION_CAST, filter) {
            basis.push(Allowance::Effect { id });
        } else if let Some(label) = &c.own_permission {
            basis.push(Allowance::Own {
                label: label.clone(),
            });
        } else {
            reasons.push(Reason::NoPermission);
        }
    }
    if c.instant_timing || ctx.timing.sorcery_timing() {
        // CR 117.1a: an instant any time with priority; any other spell in a main phase of
        // their turn with an empty stack.
        basis.push(Allowance::Rule { cr: "CR 117.1a" });
    } else if let Some(id) = ctx.first(RuleKind::TimingPermission, ACTION_CAST, filter) {
        // CR 601.3b-d: an effect lets it be cast as though it had flash.
        basis.push(Allowance::Effect { id });
    } else {
        reasons.push(Reason::Timing(TimingRequirement::SorceryTiming));
    }
    finish(reasons, basis)
}

// ---------------------------------------------------------------------------------------------
// Activate an ability
// ---------------------------------------------------------------------------------------------

/// The response that activates `c`.
fn activate_response(c: &AbilityCandidate) -> ChoiceResponse {
    ChoiceResponse::Activate {
        object: c.object,
        ability_id: c.ability_id.clone(),
    }
}

/// A total-order key over every field of an ability candidate.
type AbilityKey<'a> = (
    ObjectId,
    &'a str,
    Zone,
    Option<ActivationTiming>,
    Option<(Option<u32>, Option<u32>)>,
    (u32, u32),
    (bool, bool, bool, bool),
);

/// The sort key of an ability candidate.
fn ability_key(c: &AbilityCandidate) -> AbilityKey<'_> {
    (
        c.object,
        c.ability_id.as_str(),
        c.zone,
        c.timing,
        c.limits.map(|l: ActivationLimit| (l.per_turn, l.per_game)),
        (c.uses_this_turn, c.uses_this_game),
        (
            c.is_mana_ability,
            c.requires_tap_symbol,
            c.summoning_sick,
            c.only_if_met,
        ),
    )
}

/// Checks activating `c` (CR 602.5).
fn check_activate(ctx: &Ctx<'_>, c: &AbilityCandidate) -> Legality {
    let mut reasons = Vec::new();
    let t = &ctx.timing;
    // CR 602.5, 101.2: a prohibited ability can't begin to be activated.
    if let Some(by) = ctx.first(RuleKind::Prohibition, ACTION_ACTIVATE, |e| {
        zone_matches(e, c.zone) && object_matches(e, c.object)
    }) {
        reasons.push(Reason::Prohibited { by });
    }
    // CR 602.5a: {T} / {Q} abilities of a creature not continuously controlled since the start
    // of its controller's most recent turn (haste is the host's part of `summoning_sick`).
    if c.summoning_sick && c.requires_tap_symbol {
        reasons.push(Reason::SummoningSick);
    }
    // CR 117.1d, 605.3a: an unrestricted mana ability whenever the player has priority; CR 605.1,
    // 602.5d: an explicit timing restriction still binds it. Non-mana abilities with no stated
    // timing are instant-speed (CR 117.1b, 602.5e).
    let timing = if c.is_mana_ability {
        c.timing
    } else {
        Some(c.timing.unwrap_or(ActivationTiming::Instant))
    };
    if let Some(timing) = timing {
        let failed = match timing {
            // CR 117.1b, 602.5e: any time with priority.
            ActivationTiming::Instant => None,
            // CR 602.5d, 307.5: the timing rules for casting a sorcery.
            ActivationTiming::Sorcery => {
                (!t.sorcery_timing()).then_some(TimingRequirement::SorceryTiming)
            }
            ActivationTiming::YourTurn => (!t.is_own_turn()).then_some(TimingRequirement::OwnTurn),
            // An opponent's turn needs an active player other than the acting player: with no
            // active player (the active player left the game, CR 800.4j) it is not an
            // opponent's turn.
            ActivationTiming::OpponentTurn => (t.active_player.is_none() || t.is_own_turn())
                .then_some(TimingRequirement::OpponentTurn),
            ActivationTiming::CombatOnly => (!t.in_combat()).then_some(TimingRequirement::Combat),
        };
        if let Some(req) = failed {
            reasons.push(Reason::Timing(req));
        }
    }
    // CR 602.5b: a use restriction ("only once each turn") stays with the object.
    if let Some(limit) = c.limits {
        let turn_used = limit.per_turn.is_some_and(|n| c.uses_this_turn >= n);
        let game_used = limit.per_game.is_some_and(|n| c.uses_this_game >= n);
        if turn_used || game_used {
            reasons.push(Reason::LimitReached);
        }
    }
    if !c.only_if_met {
        reasons.push(Reason::ConditionNotMet);
    }
    let cr = if c.is_mana_ability {
        "CR 605.3a"
    } else {
        "CR 117.1b"
    };
    finish(reasons, vec![Allowance::Rule { cr }])
}

// ---------------------------------------------------------------------------------------------
// Special actions
// ---------------------------------------------------------------------------------------------

/// Checks the special action `kind` on `object` (CR 116.2, 117.1c): the prohibitions, the
/// registry and its timing, then `extra` (the kind-specific denials) and availability.
fn check_special_kind(
    ctx: &Ctx<'_>,
    registry: &SpecialActionRegistry,
    kind: &str,
    object: ObjectId,
    could_begin_to_cast: bool,
    available: bool,
    extra: Vec<Reason>,
) -> Legality {
    let mut reasons = extra;
    let mut basis = Vec::new();
    // CR 101.2: a prohibition on special actions (of this kind) wins.
    if let Some(by) = ctx.first(RuleKind::Prohibition, ACTION_SPECIAL, |e| {
        kind_matches(e, kind) && object_matches(e, object)
    }) {
        reasons.push(Reason::Prohibited { by });
    }
    match registry.get(kind) {
        None => reasons.push(Reason::UnknownSpecialKind),
        Some(k) => {
            if let Some(req) = special_timing_failure(k.timing, &ctx.timing, could_begin_to_cast) {
                reasons.push(Reason::Timing(req));
            }
            basis.push(Allowance::Rule { cr: k.cr });
        }
    }
    if !available {
        reasons.push(Reason::NotAvailable);
    }
    finish(reasons, basis)
}

/// The timing requirement of a registry kind that fails now, if any (CR 117.1c, 116.2b-m).
fn special_timing_failure(
    timing: SpecialTiming,
    t: &TimingContext,
    could_begin_to_cast: bool,
) -> Option<TimingRequirement> {
    match timing {
        SpecialTiming::AnyPriority => None,
        SpecialTiming::SorceryTiming => {
            (!t.sorcery_timing()).then_some(TimingRequirement::SorceryTiming)
        }
        SpecialTiming::OwnTurnStackEmpty => {
            (!(t.is_own_turn() && t.stack_empty)).then_some(TimingRequirement::OwnTurnStackEmpty)
        }
        SpecialTiming::OwnTurn => (!t.is_own_turn()).then_some(TimingRequirement::OwnTurn),
        // CR 116.2f: only if they could begin to cast the card.
        SpecialTiming::AnyPriorityIfCouldBeginToCast => {
            (!could_begin_to_cast).then_some(TimingRequirement::CouldBeginToCast)
        }
    }
}

/// Checks a special action other than `TURN_FACE_UP`.
fn check_special(
    ctx: &Ctx<'_>,
    registry: &SpecialActionRegistry,
    c: &SpecialCandidate,
) -> Legality {
    check_special_kind(
        ctx,
        registry,
        &c.kind,
        c.object,
        c.could_begin_to_cast,
        c.available,
        Vec::new(),
    )
}

/// Checks turning `c` face up (CR 116.2b).
fn check_turn_face_up(
    ctx: &Ctx<'_>,
    registry: &SpecialActionRegistry,
    c: &TurnFaceUpCandidate,
) -> Legality {
    check_special_kind(
        ctx,
        registry,
        TURN_FACE_UP,
        c.object,
        true,
        true,
        face_up_denials(c),
    )
}

/// Every reason `c` cannot be turned face up.
fn face_up_denials(c: &TurnFaceUpCandidate) -> Vec<Reason> {
    let mut out = Vec::new();
    // CR 708.7: spells normally can't be turned face up.
    if !c.on_battlefield {
        out.push(FaceUpDenial::NotOnBattlefield);
    }
    // CR 731.2g: a merged permanent with an instant or sorcery card stays face down.
    if c.merged_instant_or_sorcery {
        out.push(FaceUpDenial::InstantOrSorcery);
    }
    match c.origin {
        // CR 702.37e, 702.168d: without a morph / disguise cost it can't be turned face up this
        // way.
        FaceDownOrigin::Morph { has_cost } | FaceDownOrigin::Disguise { has_cost } => {
            if !has_cost {
                out.push(FaceUpDenial::NoTurnFaceUpCost);
            }
        }
        // CR 701.40b, 701.58b: a creature card with a mana cost; CR 701.40g: an instant or
        // sorcery card stays face down.
        FaceDownOrigin::Manifest {
            creature_card,
            has_mana_cost,
            instant_or_sorcery,
        }
        | FaceDownOrigin::Cloak {
            creature_card,
            has_mana_cost,
            instant_or_sorcery,
        } => {
            if instant_or_sorcery {
                out.push(FaceUpDenial::InstantOrSorcery);
            }
            if !creature_card {
                out.push(FaceUpDenial::NotCreatureCard);
            }
            if !has_mana_cost {
                out.push(FaceUpDenial::NoManaCost);
            }
        }
        // CR 708.7: only if the rule or effect that made it face down allows it.
        FaceDownOrigin::Other { can_turn_face_up } => {
            if !can_turn_face_up {
                out.push(FaceUpDenial::NotAllowed);
            }
        }
    }
    out.into_iter().map(Reason::FaceUp).collect()
}
