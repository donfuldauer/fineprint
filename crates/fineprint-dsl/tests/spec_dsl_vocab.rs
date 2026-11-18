// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Spec tests for the bundled `RulesVocabulary` (REQ-SUB-001, REQ-OBJ-001, REQ-DSL-014;
//! #15 FP-014). They check the committed `crates/fineprint-dsl/data/vocab.json` against the SRS
//! (§4.1, §6.1, §10.1, §10.2, §14.7) and need no CR cache.

use fineprint_dsl::canonical;
use fineprint_dsl::vocab::RulesVocabulary;
use serde_json::Value;

fn vocab() -> &'static RulesVocabulary {
    RulesVocabulary::bundled().expect("the committed vocabulary loads")
}

/// REQ-SUB-001: exactly the SRS §10.1 token letters and names, in order (no `l`, no `o`).
#[test]
fn req_sub_001_token_letters_and_names() {
    let v = vocab();
    let letters: Vec<&str> = v
        .predefined_tokens
        .iter()
        .map(|t| t.letter.as_str())
        .collect();
    assert_eq!(
        letters,
        [
            "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "m", "n", "p", "q", "r", "s",
            "t", "u", "v", "w", "x"
        ]
    );
    assert!(v.token("l").is_none() && v.token("o").is_none());
    let names: Vec<&str> = v
        .predefined_tokens
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Treasure",
            "Food",
            "Gold",
            "Walker",
            "Shard",
            "Clue",
            "Blood",
            "Powerstone",
            "Incubator",
            "Cursed",
            "Monster",
            "Royal",
            "Sorcerer",
            "Virtuous",
            "Wicked",
            "Young Hero",
            "Map",
            "Junk",
            "Lander",
            "Mutagen",
            "Vibranium",
            "Heartwood"
        ]
    );
    for t in &v.predefined_tokens {
        assert_eq!(t.rule_ref, format!("111.10{}", t.letter));
    }
}

/// REQ-SUB-001: the Incubator is double-faced with a 0/0 artifact creature back face; Heartwood
/// is red and green; Walker is a 2/2 black creature.
#[test]
fn req_sub_001_token_characteristics() {
    let v = vocab();
    let inc = v.token("i").unwrap();
    assert_eq!(inc.name, "Incubator");
    assert_eq!(inc.faces.len(), 2);
    let back = &inc.faces[1];
    assert_eq!((back.power, back.toughness), (Some(0), Some(0)));
    assert!(back.card_types.iter().any(|c| c == "Artifact"));
    assert!(back.card_types.iter().any(|c| c == "Creature"));
    assert!(back.subtypes.iter().any(|s| s == "Phyrexian"));
    assert!(inc.faces[0].card_types == ["Artifact"]);
    let heart = v.token("Heartwood").unwrap();
    assert_eq!(heart.faces[0].colors, ["red", "green"]);
    let walker = v.token("Walker").unwrap();
    assert_eq!(walker.faces[0].colors, ["black"]);
    assert_eq!(
        (walker.faces[0].power, walker.faces[0].toughness),
        (Some(2), Some(2))
    );
    assert_eq!(walker.faces[0].card_types, ["Creature"]);
    for t in &v.predefined_tokens {
        assert!(!t.faces.is_empty() && t.faces.len() <= 2, "{}", t.name);
    }
    // every single-faced token other than the Incubator has one face
    assert_eq!(
        v.predefined_tokens
            .iter()
            .filter(|t| t.faces.len() == 2)
            .count(),
        1
    );
}

/// REQ-SUB-001, REQ-DSL-014: Empower Jace is a keyword action only, never a predefined token.
#[test]
fn req_sub_001_empower_jace_is_not_a_token() {
    let v = vocab();
    let action = v.keyword_action("EMPOWER_JACE").expect("keyword action");
    assert_eq!(action.rule_ref, "701.71");
    assert!(v.keyword_ability("EMPOWER_JACE").is_none());
    assert!(
        v.predefined_tokens
            .iter()
            .all(|t| !t.rule_ref.starts_with("701.") && !t.name.contains("Jace"))
    );
}

/// REQ-DSL-014: the SRS §14.7 macro ids exist with the SRS rule numbers; ids are unique across
/// both lists; 701.1/702.1 are not keywords.
#[test]
fn req_dsl_014_keyword_ids() {
    let v = vocab();
    for (id, rule) in [
        ("FLYING", "702.9"),
        ("MENACE", "702.111"),
        ("VIGILANCE", "702.20"),
        ("TRAMPLE", "702.19"),
    ] {
        assert_eq!(
            v.keyword_ability(id).map(|k| k.rule_ref.as_str()),
            Some(rule)
        );
    }
    let mut ids: Vec<&str> = v
        .keyword_abilities
        .iter()
        .chain(&v.keyword_actions)
        .map(|k| k.id.as_str())
        .collect();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n);
    assert!(
        v.keyword_abilities
            .iter()
            .all(|k| k.rule_ref.starts_with("702."))
    );
    assert!(
        v.keyword_actions
            .iter()
            .all(|k| k.rule_ref.starts_with("701."))
    );
    assert!(
        v.keyword_abilities
            .iter()
            .chain(&v.keyword_actions)
            .all(|k| k.rule_ref != "701.1" && k.rule_ref != "702.1")
    );
    assert_eq!(
        v.keyword_ability("DOUBLE_STRIKE").unwrap().name,
        "Double Strike"
    );
}

/// REQ-SUB-002 context (SRS §6.1, §10.2), REQ-OBJ-001: the 15 keyword-counter kinds, no ward,
/// and the shield / stun / finality classes with their rules.
#[test]
fn req_obj_001_counter_kinds() {
    let v = vocab();
    let keyword: Vec<&str> = v
        .counter_kinds
        .iter()
        .filter(|c| c.class == "KEYWORD")
        .map(|c| c.kind.as_str())
        .collect();
    assert_eq!(
        keyword,
        [
            "flying",
            "first strike",
            "double strike",
            "deathtouch",
            "decayed",
            "exalted",
            "haste",
            "hexproof",
            "indestructible",
            "lifelink",
            "menace",
            "reach",
            "shadow",
            "trample",
            "vigilance"
        ]
    );
    assert!(v.counter_kind("ward").is_none());
    for c in v.counter_kinds.iter().filter(|c| c.class == "KEYWORD") {
        assert_eq!(c.rule_ref, "122.1b");
    }
    for (kind, class, rule) in [
        ("shield", "SHIELD", "122.1c"),
        ("stun", "STUN", "122.1d"),
        ("finality", "FINALITY", "122.1h"),
    ] {
        let c = v.counter_kind(kind).unwrap();
        assert_eq!((c.class.as_str(), c.rule_ref.as_str()), (class, rule));
    }
}

/// REQ-OBJ-001: card types and supertypes match SRS §4.1 (capitalised, CR order); creature
/// and kindred share one subtype list; spell types belong to instants and sorceries.
#[test]
fn req_obj_001_types_and_subtypes() {
    let v = vocab();
    assert_eq!(
        v.card_types,
        [
            "Artifact",
            "Battle",
            "Conspiracy",
            "Creature",
            "Dungeon",
            "Enchantment",
            "Instant",
            "Kindred",
            "Land",
            "Phenomenon",
            "Plane",
            "Planeswalker",
            "Scheme",
            "Sorcery",
            "Vanguard"
        ]
    );
    for s in ["Basic", "Legendary", "Snow", "World", "Ongoing"] {
        assert!(v.is_supertype(s), "{s}");
    }
    let creature: Vec<&str> = v.subtypes_of("Creature").collect();
    assert!(!creature.is_empty());
    assert_eq!(creature, v.subtypes_of("Kindred").collect::<Vec<_>>());
    assert!(v.is_subtype_of("Creature", "Time Lord"));
    assert!(!v.is_subtype_of("Creature", "Time"));
    assert_eq!(
        v.subtypes_of("Instant").collect::<Vec<_>>(),
        v.subtypes_of("Sorcery").collect::<Vec<_>>()
    );
    assert!(v.subtypes_of("Instant").count() > 0);
    assert!(v.is_subtype_of("Land", "Urza\u{2019}s"));
    assert!(v.is_subtype_of("Plane", "The Abyss"));
    for ct in ["Conspiracy", "Phenomenon", "Scheme", "Vanguard"] {
        assert_eq!(v.subtypes_of(ct).count(), 0, "{ct}");
    }
    let groups: Vec<(&str, &str)> = v
        .subtype_groups
        .iter()
        .map(|g| (g.group.as_str(), g.rule_ref.as_str()))
        .collect();
    assert_eq!(
        groups,
        [
            ("artifact", "205.3g"),
            ("enchantment", "205.3h"),
            ("land", "205.3i"),
            ("planeswalker", "205.3j"),
            ("spell", "205.3k"),
            ("creature", "205.3m"),
            ("planar", "205.3n"),
            ("dungeon", "205.3p"),
            ("battle", "205.3q"),
        ]
    );
    for t in &v.predefined_tokens {
        for f in &t.faces {
            for s in &f.subtypes {
                assert!(f.card_types.iter().any(|c| v.is_subtype_of(c, s)));
            }
        }
    }
}

/// REQ-SUB-001, REQ-OBJ-001: from_slice(file) == bundled(); to_canonical_bytes == file bytes;
/// the file is canonical JSON with exactly one trailing newline.
#[test]
fn req_sub_001_round_trip() {
    let bytes = RulesVocabulary::bundled_bytes();
    let v = RulesVocabulary::from_slice(bytes).unwrap();
    assert_eq!(&v, vocab());
    assert_eq!(v.to_canonical_bytes().unwrap(), bytes);
    assert!(bytes.ends_with(b"}\n"));
    assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
    let json = &bytes[..bytes.len() - 1];
    assert_eq!(canonical::canonicalize(json).unwrap(), json);
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/data/vocab.json");
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

/// The longest string the vocabulary may hold outside token ability texts (names, ids, kinds,
/// dates and rule references). The longest real entry is far shorter.
const MAX_NAME_CHARS: usize = 48;

fn walk<'a>(v: &'a Value, key: &str, out: &mut Vec<(String, &'a str)>) {
    match v {
        Value::String(s) => out.push((key.to_owned(), s)),
        Value::Array(a) => a.iter().for_each(|x| walk(x, key, out)),
        Value::Object(m) => {
            for (k, x) in m {
                walk(x, k, out);
            }
        }
        _ => {}
    }
}

fn has_rule_number(s: &str) -> bool {
    let b = s.as_bytes();
    b.windows(5)
        .any(|w| w[..3].iter().all(u8::is_ascii_digit) && w[3] == b'.' && w[4].is_ascii_digit())
}

/// REQ-NFR-004, REQ-SUB-001: the committed file is vocabulary only: no `oracle` key, no long
/// string outside token abilities, rule numbers only in `ruleRef` values.
#[test]
fn req_sub_001_no_card_data_or_prose() {
    let value: Value = serde_json::from_slice(RulesVocabulary::bundled_bytes()).unwrap();
    fn keys(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Array(a) => a.iter().for_each(|x| keys(x, out)),
            Value::Object(m) => {
                for (k, x) in m {
                    out.push(k.clone());
                    keys(x, out);
                }
            }
            _ => {}
        }
    }
    let mut all_keys = Vec::new();
    keys(&value, &mut all_keys);
    assert!(
        all_keys
            .iter()
            .all(|k| !k.to_lowercase().contains("oracle"))
    );
    let mut strings = Vec::new();
    walk(&value, "", &mut strings);
    assert!(strings.len() > 500);
    for (key, s) in &strings {
        if key == "ruleRef" {
            assert!(fineprint_dsl::vocab::is_rule_ref(s), "{s}");
            continue;
        }
        assert!(!has_rule_number(s), "rule number outside ruleRef: {s}");
        if key != "abilities" {
            assert!(s.chars().count() <= MAX_NAME_CHARS, "{key}: {s}");
        }
    }
}

/// REQ-OBJ-001 (SRS §4.1 "not hardcoded"): no subtype from the bundled vocabulary appears as a
/// string literal in the non-test source of the vocabulary modules of either crate.
#[test]
fn req_obj_001_no_hardcoded_subtypes() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let files = [
        "fineprint-dsl/src/vocab/mod.rs",
        "fineprint-ingest/src/vocab/mod.rs",
        "fineprint-ingest/src/vocab/cr.rs",
        "fineprint-ingest/src/vocab/tokens.rs",
        "fineprint-ingest/src/vocab/cli.rs",
        "fineprint-ingest/src/main.rs",
    ];
    let v = vocab();
    for f in files {
        let src = std::fs::read_to_string(format!("{root}/{f}")).unwrap();
        for g in &v.subtype_groups {
            for s in &g.subtypes {
                assert!(
                    !src.contains(&format!("\"{s}\"")),
                    "{f} hard-codes the subtype {s}"
                );
            }
        }
        for t in &v.predefined_tokens {
            assert!(
                !src.contains(&format!("\"{}\"", t.name)),
                "{f} hard-codes the token {}",
                t.name
            );
        }
    }
}
