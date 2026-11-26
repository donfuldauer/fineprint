# Synthetic card library

`tests/fixtures/cards/` is the shared library of invented card definitions that every spec test
and `.pzl` scenario loads (REQ-TEST-001). It is a contract owned by FP-016 (#17); later fixture
issues extend it. The engine's own tests never use real cards (R5, REQ-NFR-004).

## Layout and naming

- File: `tests/fixtures/cards/<family>/<what_it_does>.json`.
- Families (directories): `creatures`, `lands`, `keywords`, `spells`, `auras`, `equipment`.
- `cardDefId` is exactly `test.<what_it_does>`: lowercase ASCII `[a-z0-9_]` after `test.`, equal to
  the file stem prefixed with `test.`. Every face `name` equals the `cardDefId`.
- `<what_it_does>` describes behaviour or shape (`flier_2_2`, `burn_3_any_target`), never a real
  name and never flavour.
- All cards: layout `NORMAL`, `supportStatus` `AUTO_PARSED` (never `VERIFIED`, R11), source origin
  `PARSER`, `oracleTextHash` 64 zeros, `rulesBaseline` `test-baseline`, legality
  `{"testformat": "LEGAL"}`. LITERAL text tokens are neutral invented strings starting `test `.
- Mana abilities use `NATIVE_EFFECT` `test.add_mana` v1 (the DSL has no add-mana instruction).
  `ENCHANT` and `EQUIP` keywords carry opaque params until keyword macros exist.

## Why real cards are forbidden

No card names, Oracle text, rulings, flavour text, art or real card definitions may appear
anywhere in this repository (R5, REQ-NFR-004); `cargo xtask guard card-data` enforces part of it
in CI. The game's card text and names belong to their rights holder and are outside this
project's licence, so fixtures reproduce the rules-relevant shape of an interaction (layers,
costs, timestamps, zones) with invented cards instead.

## Adding a card

1. Write `tests/fixtures/cards/<family>/<what_it_does>.json` with `"definitionHash": ""`, using only
   DSL constructs and vocabulary ids (`crates/fineprint-dsl/data/vocab.json`).
2. Seal it: `cargo test -p fineprint-dsl --test spec_dsl_card_library -- --ignored seal_library`
   (rewrites `definitionHash` in place for every library file; the only code that writes files).
3. Add the card to the `EXPECTED` characteristics table in
   `crates/fineprint-dsl/tests/spec_dsl_card_library.rs` (mana value, colors, color identity,
   written by hand from the cost).
4. Run `cargo test -p fineprint-dsl --test spec_dsl_card_library` and `cargo xtask guard all`
   (stage new files with `git add` first so the guards see them).

## Loading a card from another crate

```rust
#[path = "../../../tests/fixtures/cards/loader.rs"]
mod card_library;

let flier = card_library::load("test.flier_2_2");
```

Also available: `library_root()`, `library_paths()` (sorted), `load_all()`, `load_path(&Path)`.
The including crate needs `fineprint-dsl` as a (dev-)dependency; adjust the `../` count to the
including file's depth.

## Changing an existing card

A card is a shared contract (P3). Change an existing card only when the work item says so, and
list every scenario that uses it in the change.

## The 30 cards

creatures (vanilla, no abilities):
- `vanilla_white_1_1` {W}; `vanilla_blue_1_3` {1}{U}; `vanilla_black_2_1` {1}{B};
  `vanilla_red_3_1` {2}{R}; `vanilla_green_2_2` {1}{G}; `vanilla_colorless_2_2` {2};
  `vanilla_gold_wu_2_2` {W}{U}; `vanilla_green_4_4` {3}{G}; `vanilla_red_5_5` {4}{R}{R};
  `vanilla_white_0_4` {2}{W}; `vanilla_black_1_1_no_cost` (no mana cost, mana value 0).

lands (one tap mana ability each, no cost):
- `mana_land_white`, `mana_land_blue`, `mana_land_black`, `mana_land_red`, `mana_land_green` (color identity = produced color),
  `mana_land_colorless`.

keywords (one keyword ability each):
- `flier_2_2` FLYING; `first_strike_2_1` FIRST_STRIKE; `trample_3_3` TRAMPLE; `haste_2_1` HASTE;
  `lifelink_2_2` LIFELINK; `deathtouch_1_1` DEATHTOUCH; `vigilance_2_3` VIGILANCE.

spells:
- `burn_3_any_target` instant {R}, 3 damage to one target; `draw_two` sorcery {2}{U}, draw two.

auras:
- `aura_plus_2_2` {1}{W} and `aura_plus_0_3` {1}{U}: enchant keyword, attach on resolution,
  layer 7c power/toughness change to the enchanted creature.

equipment:
- `equipment_plus_1_1` {1} and `equipment_plus_2_0` {2}: layer 7c power/toughness change to the
  equipped creature, EQUIP keyword and a sorcery-speed activated attach ability.
