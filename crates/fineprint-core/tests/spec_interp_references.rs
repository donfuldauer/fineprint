// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for `resolve_reference` and `resolve_players` (REQ-DSL-003, REQ-ARCH-002;
//! CR 101.3, CR 101.4, CR 608.2h, CR 113.7a, CR 608.2b). Black-box: written from the work-item
//! brief only.

#[path = "spec_interp_support.rs"]
mod support;

use fineprint_core::interp::{
    ChosenValue, Ignored, InterpError, Referent, Resolution, resolve_players, resolve_reference,
    select_objects,
};
use fineprint_core::memory::ChoiceValue;
use support::*;

fn resolve(
    env: &Env,
    c: &fineprint_core::interp::EvalContext,
    r: &ReferenceAST,
) -> Resolution<Referent> {
    resolve_reference(env, c, r).expect("no error")
}

fn only(items: Vec<Referent>) -> Resolution<Referent> {
    Resolution {
        items,
        ignored: Vec::new(),
    }
}

fn unresolved(r: ReferenceAST) -> Resolution<Referent> {
    Resolution {
        items: Vec::new(),
        ignored: vec![Ignored::Unresolved(r)],
    }
}

fn gone(x: Referent) -> Resolution<Referent> {
    Resolution {
        items: Vec::new(),
        ignored: vec![Ignored::Gone(x)],
    }
}

/// A battlefield with a source object 10 (P1's) and an aura-like object 11 attached to 12.
fn base_env() -> Env {
    let mut env = Env::new();
    env.add(creature(10, Zone::Battlefield, P1, 2, 2));
    env.add(artifact(11, Zone::Battlefield, P1));
    env.add(creature(12, Zone::Battlefield, P2, 1, 1));
    env.attached.insert(oid(11), obj(12));
    env.defending.insert(oid(10), P2);
    env
}

// REQ-DSL-003: SELF is the source while it exists; CR 608.2h / 113.7a: from LKI once it left;
// CR 101.3: no source or no information is unresolved.
#[test]
fn req_dsl_003_self_ref() {
    let mut env = base_env();
    let r = ReferenceAST::SelfRef;
    assert_eq!(
        resolve(&env, &ctx_src(P1, oid(10)), &r),
        only(vec![obj(10)])
    );
    assert_eq!(resolve(&env, &ctx(P1), &r), unresolved(r.clone()));
    // The source left with LKI captured: still the source (SELF is exempt from the "must exist
    // now" rule).
    env.vanish_with_lki(oid(10));
    assert_eq!(
        resolve(&env, &ctx_src(P1, oid(10)), &r),
        only(vec![obj(10)])
    );
    // No information at all.
    env.vanish(oid(10));
    assert_eq!(
        resolve(&env, &ctx_src(P1, oid(10)), &r),
        unresolved(r.clone())
    );
}

// REQ-DSL-003: SOURCE_LKI prefers LKI, falls back to current information, else unresolved.
// CR 608.2h, CR 113.7a.
#[test]
fn req_dsl_003_source_lki() {
    let mut env = base_env();
    let r = ReferenceAST::SourceLki;
    // Current information only.
    assert_eq!(
        resolve(&env, &ctx_src(P1, oid(10)), &r),
        only(vec![obj(10)])
    );
    // LKI only.
    env.vanish_with_lki(oid(10));
    assert_eq!(
        resolve(&env, &ctx_src(P1, oid(10)), &r),
        only(vec![obj(10)])
    );
    // Neither, and no source.
    env.vanish(oid(10));
    assert_eq!(
        resolve(&env, &ctx_src(P1, oid(10)), &r),
        unresolved(r.clone())
    );
    assert_eq!(resolve(&env, &ctx(P1), &r), unresolved(r.clone()));
}

// REQ-DSL-003: CONTROLLER is ctx.controller; REQ-ARCH-002: a controller that is not a player of
// the game is gone (CR 101.3).
#[test]
fn req_dsl_003_controller() {
    let env = base_env();
    let r = ReferenceAST::Controller;
    assert_eq!(resolve(&env, &ctx(P2), &r), only(vec![pl(P2)]));
    assert_eq!(resolve(&env, &ctx(NOBODY), &r), gone(pl(NOBODY)));
}

// REQ-DSL-003: OWNER is the owner of the source's view (current, else LKI; CR 608.2h).
#[test]
fn req_dsl_003_owner() {
    let mut env = base_env();
    let r = ReferenceAST::Owner;
    // Controller P1 is not the owner P2 of object 12.
    assert_eq!(resolve(&env, &ctx_src(P1, oid(12)), &r), only(vec![pl(P2)]));
    env.vanish_with_lki(oid(12));
    assert_eq!(resolve(&env, &ctx_src(P1, oid(12)), &r), only(vec![pl(P2)]));
    env.vanish(oid(12));
    assert_eq!(
        resolve(&env, &ctx_src(P1, oid(12)), &r),
        unresolved(r.clone())
    );
    assert_eq!(resolve(&env, &ctx(P1), &r), unresolved(r.clone()));
}

// REQ-DSL-003: ACTIVE_PLAYER.
#[test]
fn req_dsl_003_active_player() {
    let mut env = Env::three(Some(P2));
    let r = ReferenceAST::ActivePlayer;
    assert_eq!(resolve(&env, &ctx(P1), &r), only(vec![pl(P2)]));
    env.active = None;
    assert_eq!(resolve(&env, &ctx(P1), &r), unresolved(r.clone()));
}

// REQ-DSL-003: DEFENDING_PLAYER is the defending player of the source.
#[test]
fn req_dsl_003_defending_player() {
    let env = base_env();
    let r = ReferenceAST::DefendingPlayer;
    assert_eq!(resolve(&env, &ctx_src(P1, oid(10)), &r), only(vec![pl(P2)]));
    // Not attacking.
    assert_eq!(
        resolve(&env, &ctx_src(P1, oid(12)), &r),
        unresolved(r.clone())
    );
    // No source.
    assert_eq!(resolve(&env, &ctx(P1), &r), unresolved(r.clone()));
}

// REQ-DSL-003: TARGET{slot}; REQ-ARCH-002 / CR 101.3: an empty or out-of-range slot is unresolved,
// a target that is gone or an unknown player is gone.
#[test]
fn req_dsl_003_req_arch_002_target() {
    let mut env = base_env();
    let mut c = ctx(P1);
    c.targets = vec![
        Some(obj(12)),
        None,
        Some(pl(P2)),
        Some(obj(11)),
        Some(pl(NOBODY)),
    ];
    assert_eq!(resolve(&env, &c, &target(0)), only(vec![obj(12)]));
    assert_eq!(resolve(&env, &c, &target(1)), unresolved(target(1)));
    assert_eq!(resolve(&env, &c, &target(2)), only(vec![pl(P2)]));
    assert_eq!(resolve(&env, &c, &target(4)), gone(pl(NOBODY)));
    assert_eq!(resolve(&env, &c, &target(5)), unresolved(target(5)));
    assert_eq!(
        resolve(&env, &c, &target(u32::MAX)),
        unresolved(target(u32::MAX))
    );
    // A targeted object that left: gone even with LKI (only SELF/SOURCE_LKI may use LKI).
    env.vanish_with_lki(oid(11));
    assert_eq!(resolve(&env, &c, &target(3)), gone(obj(11)));
}

// REQ-DSL-003: CHOSEN{key}: Object/Player give the referent; a number or an absent key is
// unresolved; a chosen object that is gone is gone.
#[test]
fn req_dsl_003_chosen() {
    let mut env = base_env();
    let mut c = ctx(P1);
    c.chosen.insert("o".into(), chosen_obj(oid(12)));
    c.chosen.insert("p".into(), ChosenValue::Player(P2));
    c.chosen.insert("n".into(), ChosenValue::Number(3));
    c.chosen.insert("g".into(), chosen_obj(oid(11)));
    let key = |k: &str| ReferenceAST::Chosen { key: k.to_string() };
    assert_eq!(resolve(&env, &c, &key("o")), only(vec![obj(12)]));
    assert_eq!(resolve(&env, &c, &key("p")), only(vec![pl(P2)]));
    assert_eq!(resolve(&env, &c, &key("n")), unresolved(key("n")));
    assert_eq!(resolve(&env, &c, &key("absent")), unresolved(key("absent")));
    env.vanish(oid(11));
    assert_eq!(resolve(&env, &c, &key("g")), gone(obj(11)));
}

// REQ-DSL-003: BOUND{var} gives every bound referent in order; an absent var is an error
// (UnboundVariable), not an ignored part.
#[test]
fn req_dsl_003_bound() {
    let env = base_env();
    let mut c = ctx(P1);
    c.bindings
        .insert("$it".into(), vec![obj(12), pl(P2), obj(10)]);
    c.bindings.insert("$none".into(), Vec::new());
    assert_eq!(
        resolve(&env, &c, &bound("$it")),
        only(vec![obj(12), pl(P2), obj(10)])
    );
    assert_eq!(
        resolve(&env, &c, &bound("$none")).items,
        Vec::<Referent>::new()
    );
    assert_eq!(
        resolve_reference(&env, &c, &bound("$missing")),
        Err(InterpError::UnboundVariable("$missing".into()))
    );
}

// REQ-DSL-003: LINKED{link_id} gives the linked referents in order; absent or empty: unresolved.
#[test]
fn req_dsl_003_linked() {
    let env = base_env();
    let mut c = ctx(P1);
    c.linked.insert("L1".into(), vec![obj(12), obj(10)]);
    c.linked.insert("L0".into(), Vec::new());
    let link = |k: &str| ReferenceAST::Linked {
        link_id: k.to_string(),
    };
    assert_eq!(resolve(&env, &c, &link("L1")), only(vec![obj(12), obj(10)]));
    assert_eq!(resolve(&env, &c, &link("L0")), unresolved(link("L0")));
    assert_eq!(resolve(&env, &c, &link("L9")), unresolved(link("L9")));
}

// REQ-DSL-003: ENCHANTED, EQUIPPED and ATTACHED_TO are the same lookup of the source's host.
#[test]
fn req_dsl_003_attachment_refs() {
    let mut env = base_env();
    for r in [
        ReferenceAST::Enchanted,
        ReferenceAST::Equipped,
        ReferenceAST::AttachedTo,
    ] {
        assert_eq!(
            resolve(&env, &ctx_src(P1, oid(11)), &r),
            only(vec![obj(12)]),
            "{r:?}"
        );
        // The source is not attached.
        assert_eq!(
            resolve(&env, &ctx_src(P1, oid(10)), &r),
            unresolved(r.clone()),
            "{r:?}"
        );
        // No source.
        assert_eq!(resolve(&env, &ctx(P1), &r), unresolved(r.clone()), "{r:?}");
    }
    // The host left (CR 101.3): gone.
    env.vanish_with_lki(oid(12));
    for r in [
        ReferenceAST::Enchanted,
        ReferenceAST::Equipped,
        ReferenceAST::AttachedTo,
    ] {
        assert_eq!(
            resolve(&env, &ctx_src(P1, oid(11)), &r),
            gone(obj(12)),
            "{r:?}"
        );
    }
}

// REQ-DSL-003, CR 101.3: the host may be a player; a player host that is seated resolves, an
// absent object or player host is gone, and no attachment or no source is unresolved.
#[test]
fn req_dsl_003_attachment_refs_object_and_player_hosts() {
    for r in [
        ReferenceAST::Enchanted,
        ReferenceAST::Equipped,
        ReferenceAST::AttachedTo,
    ] {
        let mut env = base_env();
        let c = ctx_src(P1, oid(11));
        // Object host that exists.
        assert_eq!(resolve(&env, &c, &r), only(vec![obj(12)]), "{r:?}");
        // Player host that is seated.
        env.attached.insert(oid(11), pl(P2));
        assert_eq!(resolve(&env, &c, &r), only(vec![pl(P2)]), "{r:?}");
        // Player host that is not seated.
        env.attached.insert(oid(11), pl(NOBODY));
        assert_eq!(resolve(&env, &c, &r), gone(pl(NOBODY)), "{r:?}");
        // Object host that is gone.
        env.attached.insert(oid(11), obj(77));
        assert_eq!(resolve(&env, &c, &r), gone(obj(77)), "{r:?}");
        // No attachment, no source.
        env.attached.remove(&oid(11));
        assert_eq!(resolve(&env, &c, &r), unresolved(r.clone()), "{r:?}");
        env.attached.insert(oid(11), pl(P2));
        assert_eq!(resolve(&env, &ctx(P1), &r), unresolved(r.clone()), "{r:?}");
    }
}

// REQ-DSL-003, REQ-ARCH-002, CR 101.3: a player host is not an object (select_objects) and an
// object host is not a player (resolve_players); the wrong kind is Unresolved(reference).
#[test]
fn req_dsl_003_attachment_player_host_by_kind() {
    let mut env = base_env();
    env.attached.insert(oid(11), pl(P2));
    let c = ctx_src(P1, oid(11));
    let r = ReferenceAST::Enchanted;
    assert_eq!(
        select_objects(&env, &c, &ObjectSelectorAST::Ref(r.clone())),
        Ok(Resolution {
            items: Vec::new(),
            ignored: vec![Ignored::Unresolved(r.clone())],
        })
    );
    let r = ReferenceAST::AttachedTo;
    let got = resolve_players(&env, &c, &pref(r)).expect("no error");
    assert_eq!(got.items, vec![P2]);
    assert!(got.ignored.is_empty());
    // An object host through resolve_players is dropped as Unresolved.
    env.attached.insert(oid(11), obj(12));
    let got = resolve_players(&env, &c, &pref(ReferenceAST::Equipped)).expect("no error");
    assert!(got.items.is_empty());
    assert_eq!(
        got.ignored,
        vec![Ignored::Unresolved(ReferenceAST::Equipped)]
    );
}

// REQ-DSL-003: REMEMBERED{key} reads the source's memory; only a remembered player resolves.
#[test]
fn req_dsl_003_remembered() {
    let mut env = base_env();
    env.memory_mut(oid(10))
        .set_choice("who", ChoiceValue::Player(P2));
    env.memory_mut(oid(10))
        .set_choice("n", ChoiceValue::Number(4));
    env.memory_mut(oid(10))
        .set_choice("c", ChoiceValue::Color("red".into()));
    let rem = |k: &str| ReferenceAST::Remembered { key: k.to_string() };
    assert_eq!(
        resolve(&env, &ctx_src(P1, oid(10)), &rem("who")),
        only(vec![pl(P2)])
    );
    for k in ["n", "c", "absent"] {
        assert_eq!(
            resolve(&env, &ctx_src(P1, oid(10)), &rem(k)),
            unresolved(rem(k))
        );
    }
    // No memory, no source.
    assert_eq!(
        resolve(&env, &ctx_src(P1, oid(12)), &rem("who")),
        unresolved(rem("who"))
    );
    assert_eq!(resolve(&env, &ctx(P1), &rem("who")), unresolved(rem("who")));
}

// REQ-DSL-003: THAT_PLAYER is ctx.that_player.
#[test]
fn req_dsl_003_that_player() {
    let env = base_env();
    let r = ReferenceAST::ThatPlayer;
    let mut c = ctx(P1);
    assert_eq!(resolve(&env, &c, &r), unresolved(r.clone()));
    c.that_player = Some(P2);
    assert_eq!(resolve(&env, &c, &r), only(vec![pl(P2)]));
    c.that_player = Some(NOBODY);
    assert_eq!(resolve(&env, &c, &r), gone(pl(NOBODY)));
}

// REQ-DSL-003, CR 101.4: EACH_OPPONENT in APNAP order (seat order rotated so the active player
// is first), without the controller, with each player active in turn.
#[test]
fn req_dsl_003_each_opponent_apnap_three_players() {
    let r = ReferenceAST::EachOpponent;
    // (active, controller) -> expected opponents in APNAP order.
    let table = [
        (Some(P1), P1, vec![P2, P3]),
        (Some(P1), P2, vec![P1, P3]),
        (Some(P1), P3, vec![P1, P2]),
        (Some(P2), P1, vec![P2, P3]),
        (Some(P2), P2, vec![P3, P1]),
        (Some(P2), P3, vec![P2, P1]),
        (Some(P3), P1, vec![P3, P2]),
        (Some(P3), P2, vec![P3, P1]),
        (Some(P3), P3, vec![P1, P2]),
        // No active player: no rotation.
        (None, P2, vec![P1, P3]),
    ];
    for (active, controller, want) in table {
        let env = Env::three(active);
        let got = resolve(&env, &ctx(controller), &r);
        assert_eq!(
            got,
            only(want.into_iter().map(pl).collect()),
            "active {active:?} controller {controller:?}"
        );
    }
    // A one-player game: empty, and not an ignored part.
    let solo = Env::with_players(&[P1], Some(P1));
    assert_eq!(resolve(&solo, &ctx(P1), &r), only(Vec::new()));
}

// REQ-DSL-003, CR 101.4: Players(AllPlayers) is APNAP order; Teammates is empty.
#[test]
fn req_dsl_003_resolve_players_groups() {
    for (active, want) in [
        (Some(P1), vec![P1, P2, P3]),
        (Some(P2), vec![P2, P3, P1]),
        (Some(P3), vec![P3, P1, P2]),
        (None, vec![P1, P2, P3]),
    ] {
        let env = Env::three(active);
        let all = resolve_players(
            &env,
            &ctx(P2),
            &PlayerSelectorAST::Players(PlayerGroup::AllPlayers),
        )
        .expect("no error");
        assert_eq!(all.items, want, "active {active:?}");
        assert!(all.ignored.is_empty());
        let mates = resolve_players(
            &env,
            &ctx(P2),
            &PlayerSelectorAST::Players(PlayerGroup::Teammates),
        )
        .expect("no error");
        assert!(mates.items.is_empty());
    }
}

// REQ-DSL-003: resolve_players(Ref) keeps players and drops objects into Unresolved(r).
#[test]
fn req_dsl_003_resolve_players_ref() {
    let env = base_env();
    let mut c = ctx(P1);
    c.targets = vec![Some(obj(12)), Some(pl(P2))];
    let got = resolve_players(&env, &c, &pref(target(1))).expect("no error");
    assert_eq!(got.items, vec![P2]);
    assert!(got.ignored.is_empty());
    let got = resolve_players(&env, &c, &pref(target(0))).expect("no error");
    assert!(got.items.is_empty());
    assert_eq!(got.ignored, vec![Ignored::Unresolved(target(0))]);
    // EACH_OPPONENT through a player selector.
    let got = resolve_players(&env, &c, &pref(ReferenceAST::EachOpponent)).expect("no error");
    assert_eq!(got.items, vec![P2]);
    // An unbound var is an error here too.
    assert_eq!(
        resolve_players(&env, &c, &pref(bound("$x"))),
        Err(InterpError::UnboundVariable("$x".into()))
    );
}
