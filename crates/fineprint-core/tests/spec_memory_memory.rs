// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for per-object `Memory` and `MemoryTable` (REQ-CORE-007, CR 400.7, 400.7d,
//! 607.2p, 607.2q, 607.5a). Black-box: written from the work-item brief only.

#[path = "spec_memory_support.rs"]
mod support;

use std::collections::BTreeMap;

use fineprint_core::memory::{
    ChoiceValue, DurableRef, LinkEntry, Memory, MemoryTable, SpellHistory,
};
use fineprint_core::zones::{Moved, ZoneId, ZoneKind};
use support::*;

fn all_choices() -> Vec<(&'static str, ChoiceValue)> {
    vec![
        ("color", ChoiceValue::Color("red".into())),
        (
            "creature_type",
            ChoiceValue::CreatureType("test_kind".into()),
        ),
        ("card_name", ChoiceValue::CardName("test.named_card".into())),
        ("player", ChoiceValue::Player(P2)),
        ("number", ChoiceValue::Number(-3)),
        ("mode", ChoiceValue::Mode(2)),
        ("anchor", ChoiceValue::Anchor("test_anchor".into())),
    ]
}

fn full_history() -> SpellHistory {
    let mut mana_spent = BTreeMap::new();
    mana_spent.insert(("G".to_string(), "land".to_string()), 2);
    mana_spent.insert(("C".to_string(), "artifact".to_string()), 1);
    let mut additional_costs = BTreeMap::new();
    additional_costs.insert("kicker".to_string(), true);
    additional_costs.insert("gift".to_string(), false);
    SpellHistory {
        costs_paid: vec!["mana".into(), "kicker".into()],
        mana_spent,
        additional_costs,
        x_value: Some(4),
        cast_face: Some("test.front_face".into()),
        exiled_to_pay: vec![DurableRef::from_parts(cid(900), oid(901))],
    }
}

fn fake_move(old: u64, new: u64, from: ZoneId, to: ZoneId) -> Moved {
    Moved {
        old: oid(old),
        new: oid(new),
        from,
        to,
    }
}

/// A Memory with everything set: ordinary and persistent choices, history, links.
fn loaded_memory() -> Memory {
    let mut m = Memory::new();
    m.set_choice("ordinary", ChoiceValue::Color("white".into()));
    m.set_persistent_choice("kept", ChoiceValue::CreatureType("test_kind".into()));
    m.set_spell_history(full_history());
    m.linked_mut().push(lid(1), LinkEntry::Int(7));
    m
}

// ---------------------------------------------------------------------------------------------
// (a), (b) choices and spell history
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007, CR 607.5a: every ChoiceValue variant round-trips, ordinary and persistent; an
// unmade choice is undefined (None).
#[test]
fn memory_choices_every_variant() {
    let mut m = Memory::new();
    assert_eq!(m.choice("color"), None);
    for (k, v) in all_choices() {
        m.set_choice(k, v.clone());
    }
    for (k, v) in all_choices() {
        assert_eq!(m.choice(k), Some(&v), "{k}");
    }
    let mut p = Memory::new();
    for (k, v) in all_choices() {
        p.set_persistent_choice(k, v.clone());
    }
    for (k, v) in all_choices() {
        assert_eq!(p.choice(k), Some(&v), "{k}");
    }
    assert_eq!(m.choice("never_chosen"), None);
    // Replacing an ordinary choice keeps the latest value.
    m.set_choice("color", ChoiceValue::Color("blue".into()));
    assert_eq!(m.choice("color"), Some(&ChoiceValue::Color("blue".into())));
}

// REQ-CORE-007, CR 400.7d: the spell history round-trips; an empty Memory has none.
#[test]
fn memory_spell_history_round_trip() {
    let mut m = Memory::new();
    assert_eq!(m.spell_history(), None);
    let h = full_history();
    m.set_spell_history(h.clone());
    assert_eq!(m.spell_history(), Some(&h));
    let h2 = SpellHistory {
        x_value: None,
        ..full_history()
    };
    m.set_spell_history(h2.clone());
    assert_eq!(m.spell_history(), Some(&h2));
}

// REQ-CORE-007: a new Memory is empty and equals the default.
#[test]
fn memory_new_is_empty_default() {
    let m = Memory::new();
    assert_eq!(m, Memory::default());
    assert!(m.linked().is_empty());
    assert_eq!(m.link_source(lid(1)), None);
    assert_eq!(m.spell_history(), None);
    assert_eq!(SpellHistory::default().x_value, None);
    assert!(SpellHistory::default().exiled_to_pay.is_empty());
}

// ---------------------------------------------------------------------------------------------
// (c) apply_zone_change
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007, CR 400.7d, 607.2p, 607.2q, 400.7: stack -> battlefield carries the spell
// history (all fields) and persistent choices; drops ordinary choices and the linked store.
#[test]
fn zone_change_stack_to_battlefield_carries_history() {
    let mut t = MemoryTable::new();
    *t.get_mut_or_default(oid(10)) = loaded_memory();
    let m = fake_move(10, 11, ZoneId::Stack, ZoneId::Battlefield);
    assert!(t.apply_zone_change(&m));
    assert!(t.get(oid(10)).is_none());
    let new = t.get(oid(11)).expect("new object has memory");
    let h = new.spell_history().expect("history carried");
    assert_eq!(h, &full_history());
    assert_eq!(h.additional_costs.get("kicker"), Some(&true));
    assert_eq!(h.x_value, Some(4));
    assert_eq!(h.cast_face.as_deref(), Some("test.front_face"));
    assert_eq!(h.costs_paid, vec!["mana".to_string(), "kicker".to_string()]);
    assert_eq!(
        h.mana_spent.get(&("G".to_string(), "land".to_string())),
        Some(&2)
    );
    assert_eq!(h.exiled_to_pay.len(), 1);
    assert_eq!(
        new.choice("kept"),
        Some(&ChoiceValue::CreatureType("test_kind".into()))
    );
    assert_eq!(new.choice("ordinary"), None);
    assert!(new.linked().is_empty());
    assert_eq!(new.linked().entries(lid(1)), None);

    // Exactly that, nothing more.
    let mut expect = Memory::new();
    expect.set_persistent_choice("kept", ChoiceValue::CreatureType("test_kind".into()));
    expect.set_spell_history(full_history());
    assert_eq!(new, &expect);
}

// REQ-CORE-007, CR 400.7, 607.2p: any other zone change carries only persistent choices.
#[test]
fn zone_change_other_pairs_carry_only_persistent_choices() {
    let pairs = [
        (ZoneId::Stack, ZoneId::Graveyard(P1)),
        (ZoneId::Hand(P1), ZoneId::Battlefield),
        (ZoneId::Battlefield, ZoneId::Graveyard(P1)),
        (ZoneId::Battlefield, ZoneId::Exile),
        (ZoneId::Exile, ZoneId::Battlefield),
        (ZoneId::Graveyard(P1), ZoneId::Stack),
        (ZoneId::Battlefield, ZoneId::Stack),
        (ZoneId::Exile, ZoneId::Exile),
        (ZoneId::Command, ZoneId::Command),
    ];
    for (from, to) in pairs {
        let mut t = MemoryTable::new();
        *t.get_mut_or_default(oid(10)) = loaded_memory();
        assert!(
            t.apply_zone_change(&fake_move(10, 11, from, to)),
            "{from:?}"
        );
        assert!(t.get(oid(10)).is_none(), "{from:?} -> {to:?}");
        let new = memory_or_empty(&t, oid(11));
        assert_eq!(new.spell_history(), None, "{from:?} -> {to:?}");
        assert_eq!(new.choice("ordinary"), None);
        assert_eq!(
            new.choice("kept"),
            Some(&ChoiceValue::CreatureType("test_kind".into()))
        );
        assert!(new.linked().is_empty());
        let mut expect = Memory::new();
        expect.set_persistent_choice("kept", ChoiceValue::CreatureType("test_kind".into()));
        assert_eq!(new, expect, "{from:?} -> {to:?}");
    }
}

// REQ-CORE-007, CR 400.7: a same-zone reposition (old == new) changes nothing and returns
// false.
#[test]
fn zone_change_reposition_changes_nothing() {
    let mut t = MemoryTable::new();
    *t.get_mut_or_default(oid(10)) = loaded_memory();
    let before = t.clone();
    for z in [ZoneId::Battlefield, ZoneId::Stack, ZoneId::Graveyard(P1)] {
        assert!(!t.apply_zone_change(&fake_move(10, 10, z, z)));
        assert_eq!(t, before);
    }
    assert_eq!(t.get(oid(10)), Some(&loaded_memory()));
}

// REQ-CORE-007, CR 400.7: the return value says whether the old object had a Memory.
#[test]
fn zone_change_return_value() {
    let mut t = MemoryTable::new();
    assert!(!t.apply_zone_change(&fake_move(1, 2, ZoneId::Hand(P1), ZoneId::Stack)));
    assert!(t.get(oid(1)).is_none());
    assert_eq!(memory_or_empty(&t, oid(2)), Memory::new());
    t.get_mut_or_default(oid(3));
    assert!(t.apply_zone_change(&fake_move(3, 4, ZoneId::Hand(P1), ZoneId::Stack)));
    assert!(t.get(oid(3)).is_none());
    assert_eq!(memory_or_empty(&t, oid(4)), Memory::new());
}

// REQ-CORE-007, CR 400.7, 607.1d: cross-object link sources are not carried either.
#[test]
fn zone_change_drops_link_sources() {
    let mut w = World::new();
    let src = w.create(ZoneKind::Battlefield, P1);
    let made = w.create(ZoneKind::Battlefield, P1);
    w.memories
        .link_created(&w.zones, src, made, lid(3))
        .unwrap();
    assert!(w.memories.get(made).unwrap().link_source(lid(3)).is_some());
    let m = w.move_to(made, ZoneKind::Graveyard);
    assert_eq!(
        memory_or_empty(&w.memories, m.new).link_source(lid(3)),
        None
    );
    let m2 = w.move_to(src, ZoneKind::Exile);
    assert!(memory_or_empty(&w.memories, m2.new).linked().is_empty());
}

// ---------------------------------------------------------------------------------------------
// (d) scenarios
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007, REQ-CORE-003, CR 400.7d: scenario A: a spell cast kicked with X = 3 resolves;
// the permanent it becomes reads both from its Memory.
#[test]
fn scenario_a_kicker_and_x_read_by_permanent() {
    let mut w = World::new();
    let card = w.create(ZoneKind::Hand, P1);
    let cast = w.move_to(card, ZoneKind::Stack);
    let spell = cast.new;
    let mut history = SpellHistory {
        x_value: Some(3),
        costs_paid: vec!["mana".into(), "kicker".into()],
        ..SpellHistory::default()
    };
    history.additional_costs.insert("kicker".into(), true);
    history
        .mana_spent
        .insert(("R".to_string(), "land".to_string()), 3);
    w.memory_mut(spell).set_spell_history(history.clone());
    w.memory_mut(spell)
        .set_choice("target_note", ChoiceValue::Number(1));

    let resolved = w.move_to(spell, ZoneKind::Battlefield);
    assert_eq!(resolved.from, ZoneId::Stack);
    assert_eq!(resolved.to, ZoneId::Battlefield);
    let perm = resolved.new;
    assert!(w.permanents.get(perm).is_some());
    assert!(w.memories.get(spell).is_none());
    let mem = w.memories.get(perm).expect("permanent has memory");
    let h = mem.spell_history().expect("CR 400.7d");
    assert_eq!(h.additional_costs.get("kicker"), Some(&true));
    assert_eq!(h.x_value, Some(3));
    assert_eq!(h, &history);
    assert_eq!(mem.choice("target_note"), None);
    // The LKI of the spell holds the history too.
    assert_eq!(
        w.lki.get(spell).unwrap().memory().spell_history(),
        Some(&history)
    );
    // The permanent dies: the card in the graveyard has no history.
    let died = w.move_to(perm, ZoneKind::Graveyard);
    assert_eq!(memory_or_empty(&w.memories, died.new).spell_history(), None);
    assert_eq!(
        w.lki.get(perm).unwrap().memory().spell_history(),
        Some(&history)
    );
}

// REQ-CORE-007, CR 607.2q, 400.7d, 400.7: scenario B: a card exiled to pay the cost of a
// permanent spell is still referenced by the permanent; once the permanent dies, gone.
#[test]
fn scenario_b_exiled_to_pay_follows_spell_to_permanent() {
    let mut w = World::new();
    let fodder = w.create(ZoneKind::Graveyard, P1);
    let card = w.create(ZoneKind::Hand, P1);
    let spell = w.move_to(card, ZoneKind::Stack).new;
    let exiled = w.move_to(fodder, ZoneKind::Exile).new;
    let r = DurableRef::new(&w.zones, exiled).unwrap();
    w.memory_mut(spell).set_spell_history(SpellHistory {
        exiled_to_pay: vec![r],
        ..SpellHistory::default()
    });

    let perm = w.move_to(spell, ZoneKind::Battlefield).new;
    let h = w.memories.get(perm).unwrap().spell_history().unwrap();
    assert_eq!(h.exiled_to_pay, vec![r]);
    assert_eq!(
        h.exiled_to_pay[0].resolve(&w.zones, Some(ZoneKind::Exile)),
        Some(exiled)
    );

    let dead = w.move_to(perm, ZoneKind::Graveyard).new;
    assert!(w.memories.get(perm).is_none());
    assert_eq!(memory_or_empty(&w.memories, dead).spell_history(), None);
}

// ---------------------------------------------------------------------------------------------
// (e), (f) clones, table basics
// ---------------------------------------------------------------------------------------------

// REQ-CORE-007: clones of Memory and MemoryTable are independent.
#[test]
fn memory_and_table_clone_independence() {
    let m = loaded_memory();
    let mut c = m.clone();
    assert_eq!(c, m);
    c.set_choice("ordinary", ChoiceValue::Number(1));
    c.linked_mut().push(lid(2), LinkEntry::Player(P1));
    assert_ne!(c, m);
    assert_eq!(m, loaded_memory());

    let mut t = MemoryTable::new();
    *t.get_mut_or_default(oid(1)) = loaded_memory();
    let mut tc = t.clone();
    assert_eq!(tc, t);
    tc.get_mut_or_default(oid(1))
        .set_persistent_choice("x", ChoiceValue::Mode(3));
    tc.get_mut_or_default(oid(2));
    assert_ne!(tc, t);
    assert_eq!(t.get(oid(1)), Some(&loaded_memory()));
    assert!(t.get(oid(2)).is_none());
    assert_eq!(t.len(), 1);
}

// REQ-CORE-007: get_mut_or_default creates an empty Memory; len/is_empty count entries.
#[test]
fn table_get_mut_or_default_len_is_empty() {
    let mut t = MemoryTable::new();
    assert_eq!(t, MemoryTable::default());
    assert!(t.is_empty());
    assert_eq!(t.len(), 0);
    assert!(t.get(oid(5)).is_none());
    assert_eq!(t.get_mut_or_default(oid(5)), &Memory::new());
    assert_eq!(t.len(), 1);
    assert!(!t.is_empty());
    t.get_mut_or_default(oid(5))
        .set_choice("n", ChoiceValue::Number(1));
    assert_eq!(t.len(), 1);
    assert_eq!(
        t.get_mut_or_default(oid(5)).choice("n"),
        Some(&ChoiceValue::Number(1))
    );
    t.get_mut_or_default(oid(2));
    assert_eq!(t.len(), 2);
    assert_eq!(t.get(oid(2)), Some(&Memory::new()));
}
