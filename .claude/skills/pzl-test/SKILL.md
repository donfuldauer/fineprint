---
name: pzl-test
description: "How to write Fine Print .pzl scenario tests (REQ-TEST-001): rules-regression scenarios, card behavioural tests, ruling-derived tests and interaction packs, with REQ and CR citations, every required ordering, and determinism-safe assertions. Use when writing or fixing any .pzl file or a test that drives the engine through a scripted game state; agents without the Skill tool read this file directly."
---

# pzl-test

A `.pzl` file is a declarative scenario: an exact starting state, a scripted sequence of player
choices, and assertions (SRS REQ-TEST-001). It is the main evidence that a rule or a card works.
The exact schema is a contract defined in `docs/PLAN.md` §3; this skill is the conventions that
sit on top of it. If the plan's schema and this skill disagree, the schema wins for syntax and
this skill wins for what a good test contains.

## 1. Header (always)

- `title`: what is being proven, in one line.
- `requirements`: every `REQ-*` ID covered (the verifier traces these).
- `cr`: every CR rule relied on, numbered per the SRS rules baseline, with the text obtained from
  the `rules-researcher` (never from memory).
- `rulesBaseline`, `cardDatabase` version, and the `definitionHash` of every card used
  (REQ-TEST-001.1, REQ-API-005), so drift fails loudly instead of silently changing the test.
- `prngSeed`: fixed. Tests never depend on an unseeded value.
- For ruling-derived tests: the ruling date and its text reference.

## 2. State

- Build the **smallest** state that exercises the behaviour: only the zones, objects, counters,
  designations and statuses that matter. State everything that matters explicitly, including
  library order, turn, step, active player and priority.
- Use **only synthetic test cards** (PROCESS.md R5, REQ-NFR-004): invented definitions from the
  shared library, neutrally named by what they do. When the SRS names a real card (the regression
  suite in REQ-TEST-001.2), use the synthetic card that reproduces its abilities and cite the SRS
  item, never the card's name or text.
- Timestamps that matter are set by entry order in the script or explicitly, never left to
  incidental ordering.

## 3. Script

- Drive the engine only through the public actions and choice responses (§15.3 taxonomy). No
  sandbox commands after the initial state unless the test is about sandbox behaviour (then the
  replay must be flagged `sandboxed`).
- Respond to every `PendingChoiceRequest` explicitly; never rely on timeouts or yields.

## 4. Assertions

- Assert the **outcome the rule defines**, at the right granularity: evaluated characteristics
  (with `inspectObject` breakdown where the test is about layers), zone contents and order, life
  and counters, the event stream (subsequence by default; exact sequence only when order is the
  point), pending choices and their option sets, and game results.
- Assert **absence** as well as presence where the rule forbids something (no prompt for damage
  assignment order; ward does not prevent targeting; no trigger from a look-back that should not
  fire).
- Never assert on wall-clock time, memory addresses, hash-map order or anything the plan marks as
  non-canonical bookkeeping (REQ-LOOP-001.1).

## 5. Coverage obligations

- **Orderings.** When the outcome may depend on order, write one scenario per order the rules
  distinguish: both timestamp orders (e.g. Humility + Opalescence), APNAP permutations for
  simultaneous triggers, each replacement tier choice (CR 616.1), each player's choice where the
  rule gives one.
- **Boundaries.** Exactly-lethal and one-short damage; 0 and negative values (power ≤ 0 assigns no
  damage; MV of no mana cost); empty zones; the last card of a library.
- **Zone changes.** When the rule involves an object changing zones, assert both the new object
  (CR 400.7) and the last-known information used.
- **Multiplayer.** When a rule has a multiplayer variant (APNAP, leaving the game, attack targets),
  include a three-or-more-player scenario.
- **Determinism.** Each new scenario is added to the golden replay corpus when the plan says so,
  and must replay to the same state hash on every binding.

## 6. Client behavioral tests (format only)

Clients earn `VERIFIED` for their cards by supplying behavioral `.pzl` tests (REQ-ING-008,
REQ-ING-010b). Those tests use the same format and these same conventions, but they live with the
client's card database, never in this repository. The engine's own tests of that mechanism use
synthetic cards.

## 7. Naming and placement

Follow the plan's paths. Rules-regression scenarios are grouped by SRS area (layers, replacement,
casting, combat, layouts, SBAs, multiplayer, loops) with one file per scenario and a name that
says what it proves, e.g. `layers/humility_then_opalescence.pzl`.
