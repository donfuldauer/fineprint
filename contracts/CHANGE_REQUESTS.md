# Contract Change Requests

Changes to a contract (PROCESS.md P3) after its owning subphase merged (rule R2). Append only.
Additive changes merge immediately as a contract-only PR; breaking changes merge only between waves.

Format:

```
## CR-<NNN> — <contract>: <title>
Requested by: <work item>   Kind: additive | breaking   Status: open | merged (<PR>) | rejected
Change: <exact types/fields/schema>
Why: <the requirement or finding that needs it>
Consumers affected: <modules, bindings, artifacts>
```

## CR-001 — `fineprint_dsl::mana::ManaSymbol`: colorless-hybrid variant `{C/x}`
Requested by: #169 FP-142   Kind: additive   Status: open
Change: add variant `ManaSymbol::ColorlessHybrid(Color)`, appended LAST so the existing variant
order (and the derived `Ord`) is unchanged. Canonical text `{C/x}` for x in W U B R G (`{C/W}`,
`{C/U}`, `{C/B}`, `{C/R}`, `{C/G}`); `C` first is the only spelling the parser accepts (strict
decode); `is_valid` is true for every color. No existing variant, signature or spelling changes.
Why: CR 107.4 lists {C/W}..{C/G} as monocolored hybrid symbols; REQ-DSL-002 (ManaCostDSL covers
every mana symbol) and REQ-OBJ-001 (mana value and colors for every symbol).
Consumers affected: fineprint-dsl mana (parser, Display, serde), fineprint-core objects
(`symbol_mana_value`, `symbol_colors`), fineprint-ingest import recompute (`symbol_mana_value`,
`symbol_colors`).

## CR-002 — Card DSL (SRS §14): seal and load validate ASTs against the real DSL types
Requested by: #159 FP-140   Kind: breaking   Status: merged (#233)
Change: `CardDefinitionDSL::seal` and `CardDefinitionDSL::from_json` now decode every AST member
(`power`, `toughness`, `text`, `abilities`, partner abilities) as the real SRS §14 type and
reject the definition (`CardDefinitionError::Schema`, or `CanonicalError::Serialize` from `seal`)
when a member is not a valid AST. A definition that carried placeholder content, for example
`power: 1` instead of `power: {"LITERAL": 1}`, was accepted before and is rejected now, even with
its original correct digest. `dslVersion` stays "1.0": no version bump, no legacy reader. The
struct fields and the canonical serialisation of valid definitions are unchanged.
Why: REQ-DSL-010 and REQ-DSL-012 require typed, validated ASTs; the placeholder content was never
a valid SRS §14 AST. This change was merged without an R2 entry and is recorded retroactively by
the phase 02 gate review (a phase boundary), as the process requires for breaking changes.
Consumers affected: fineprint-core, fineprint-ingest (migration: #232 FP-164 moves core and
ingest to the real typed ASTs).

## CR-003 — Card DSL (SRS §14.3): abilities hold the real `effect::Instruction`
Requested by: #161 FP-141   Kind: breaking   Status: merged (#234)
Change: `ability::EffectInstructionAST` (any `op` matching `[A-Z][A-Z0-9_]*` plus opaque members)
is deleted; every `effect`, mode `effect` and `INSTEAD.instructions` position now decodes as
`effect::Instruction`. An ability document whose instruction used an unknown `op` (for example
`{"op":"NOT_AN_OP"}`), omitted a required member (`{"op":"DRAW"}`) or carried an unknown member
parsed before and is rejected now, so card definitions containing such abilities no longer seal
or load. Valid real instructions are unchanged. `dslVersion` stays "1.0".
Why: REQ-DSL-010 (typed ASTs; the placeholder was always marked as temporary until FP-013) and
REQ-NFR-006. The placeholder content was never a valid SRS §14 instruction.
Consumers affected: fineprint-dsl ability and card modules; fineprint-core and fineprint-ingest
through #232 FP-164. Recorded retroactively by the phase 02 gate review.

## CR-004 — Events (REQ-EVENT-001) executed-event encoding: lost attachment links
Requested by: #180 FP-143   Kind: breaking   Status: open
Change: new type `events::UnattachRecord { attachment: ObjectId, host: AttachTarget }`
(`Clone, Copy, Debug, PartialEq, Eq`; canonical encoding: `attachment`, then `host`).
`ExecutedEvent` gains a LAST field `unattached: Vec<UnattachRecord>`, encoded last as a
length-prefixed sequence (`put_len` then the records), so every `ExecutedEvent`'s canonical bytes,
and with them the replay and state hashes, change. `GameEvent` gains a variant
`UnattachFrom { attachment: ObjectId, host: AttachTarget }`, appended LAST with canonical tag 17,
then `attachment`, then `host`. `Outcome` is unchanged; no existing `GameEvent` tag (0..=16) or
`Outcome` tag (0..=5) changes.
Why: REQ-EVENT-001 and REQ-EVENT-002 (the executed log is what trigger detection reads); CR 701.3d
(attaching to a different host, a host leaving the battlefield or an attachment leaving the
battlefield counts as becoming unattached) and CR 603.10c (UNATTACHED looks back in time). Today
those links are dropped without a record, so `UNATTACHED` never triggers for them.
Consumers affected: events executor (`events/exec.rs`), trigger normalizer (`triggers/event.rs`),
replacement bus kind mapping (`replacement/types.rs`), replay and state-hash goldens.

## CR-005 — State model and canonical state hash: layers in GameState
Requested by: #183 FP-144   Kind: breaking   Status: merged (#243)
Change: the placeholder type `state::EffectRegistry` (and its `is_empty`) is removed, with no
compatibility alias. `GameState` stores the real `layers::ContinuousEffects`, the
`layers::TimestampLedger` and the links `BTreeMap<EffectId, state::GeneratingAbility>` (new type
`GeneratingAbility { source: ObjectId, ability: AbilityAST }`); `GameState::effects()` now returns
`&ContinuousEffects`. Merkle layout (`GameState::merkle_tree`): `/effects` keeps its path and its
payload becomes the canonical encoding of `ContinuousEffects` (`put_len` of the effect count, then
per effect in ascending `EffectId` order the id, every field of the effect and its `SameAsLayer`
anchor link as `Option<EffectId>`); two NEW leaves: `/timestamps`, the canonical encoding of the
`TimestampLedger` (`put_len` of the entry count, then object entries tag 0, counter entries tag 1
and sticker entries tag 2, each in ascending key order), and `/effect_generators`, the
`BTreeMap<EffectId, GeneratingAbility>` (each value the source id and the ability as tag 0 and its
RFC 8785 canonical JSON). Every enum has an explicit stable one-byte tag (table in the
`layers::encode` module documentation); DSL values are RFC 8785 canonical JSON (tag 0) with a
structural fallback (tag 1) for values without a JCS form, never MessagePack. Each empty
collection is `put_len(0)` (4 zero bytes). Every other leaf (`/prng`, `/ids/*`, `/turn/*`,
`/players/<id>/*`, `/history`) encodes exactly as before. Wire format: `STATE_FORMAT_VERSION`
1 -> 2; `StateWire` member `effects` now holds `{effects: [{id, effect}], links: [{part,
anchor}]}`, and the required members `timestamps` (`{objects, counters, stickers}`) and
`effect_generators` (`[{effect, source, ability}]`) are added; the top level is a 10-member map.
Version 1 is rejected; there is no legacy reader. The decoder validates the layer data (effect
ids and timestamps issued by the state's generators, registration through
`ContinuousEffects::register`/`register_group` including group links, the 7a CDA rule, the
generating-ability links, ordered unique ledger and link entries). Golden state digests:
`GameState::new(42, 2, 20)` f32bbec462508986efd89ed35f9184bec86a107f774505e19c4f65d3db4fb0d5 ->
7389c79ba648f778a6925577d462382782418b603f487e7eef53a39898a87030; the mutated golden state
7ebe9f5c17abfc986c0168e1722807514fd1372a1e6dcc10dca594afdfa58371 ->
61143120a7a8869bc2fb299244f20d4d6085bf324fefe398afb16d6cf354cb7c; new pinned non-empty layer
state 49b4707ebee9e40df30c49d6ca4fe2289e328f66814ef2b84833e58400ce7cc1.
Why: REQ-LAYER-001, REQ-CORE-002, REQ-NFR-006 (effects and CR 613.7 timestamps stored in the
state, hashed and replayed); REQ-LAYER-002 existence (CR 613.8a): each static (and ETB) effect is
linked to the ability on its source that generates it.
Consumers affected: fineprint-core state and txn; golden state hashes; bindings and replay format
(not yet implemented); #184, #186, #193.
Process note: the contract change and its implementation merged together in #243 (phase 04)
without a separate contract-only PR, and #243 is not a phase boundary. It is recorded as merged and
ratified by the phase 06 gate review (a phase boundary, docs/DECISIONS.md "Gate 06-turn F30 — CR-005
recorded as merged"), as the phase 02 gate did for CR-002 and CR-003.

## CR-006 — Card DSL (SRS 14.4.3): optional "defines" on layer-4 CHAR modifications
Requested by: #198 FP-151   Kind: additive   Status: open
Change: a `CHAR` static effect whose `layer` is `"4"` (with or without `isCDA`) may carry the
flat modification param `"defines"`, e.g. `{"type":"CHAR","layer":"4","affected":{"ref":"SELF"},
"modification":{"op":"ANY_OP","defines":["SUBTYPES"]},"isCDA":true}`. Its value is a non-empty
JSON array of distinct strings drawn from exactly `"SUPERTYPES"`, `"CARD_TYPES"`, `"SUBTYPES"`
(case-sensitive). Decoding and writing a layer-4 `CHAR` effect reject an empty array, a
duplicate, an unknown or wrongly-cased name, and a non-array or non-string element. Absent
`defines` means the effect defines all three parts. On every other layer, and in `PLAYER`
modifications and `RULE` params, `defines` stays an ordinary opaque param. New public API in
`fineprint_dsl::ability` (defined in `ability/statics.rs`; `TypePart` needs adding to the
`pub use self::statics::{...}` re-export list): `enum TypePart {Supertypes, CardTypes, Subtypes}` (with
`SUPERTYPES`, `CARD_TYPES`, `SUBTYPES` wire-name consts, `ALL`, `as_str`, `from_wire`,
`parse_defines`), `CharacteristicEffect::DEFINES` and `CharacteristicEffect::defined_type_parts()`
(`None` off layer 4). `dslVersion` stays "1.0"; decode and RFC 8785 canonical serialisation of
every existing valid definition are unchanged and a valid `defines` round-trips byte-identically.
The only definitions that become invalid are ones that already used a layer-4 `CHAR` `defines`
param with a non-conforming value (none exist in the repository). Numbered CR-006 because
CR-003 to CR-005 were already taken (docs/DECISIONS.md "#198 FP-151 — Change request number").
Why: REQ-OBJ-002 / CR 707.9d needs to know which type-line part (supertypes, card types,
subtypes; separate characteristics, CR 109.3, 205.1b) a layer-4 characteristic-defining ability
defines, so a copy exception touching one part drops only the CDAs that define that part.
Consumers affected: fineprint-dsl ability statics, fineprint-core layers/copy.

## CR-007 — Layer evaluator (REQ-LAYER-001/002): sourced static effects use the source's evaluated controller
Requested by: #203 FP-152   Kind: breaking   Status: open
Change: for a `ContinuousEffect` with `origin: EffectOrigin::Static { .. }` and `source:
Some(s)`, the `MatchContext::effect_controller` passed to `LayerEnvironment::matches` is now the
controller of `s` as evaluated in the layer pass at the point the effect applies (the candidate's
own state so far when `s` is the candidate; otherwise `s` evaluated through the end of the
effect's layer without the effect, the view the existence question already uses), not the
`ContinuousEffect::controller` stored at registration. For effects of layers 3-7d this is the
controller after layer 2; for layers 1a, 1b and 2 the source's state so far in that layer. The
stored controller remains the fallback when the source is not in the universe or has no
evaluated controller. Effects with `EffectOrigin::Resolved`, without a source, with a locked
`Affected::Objects` set or linked `Affected::SameAsLayer` parts are unchanged. New provided
method `LayerEnvironment::static_source_state(&self, effect: EffectId, source: ObjectId) ->
Option<&Evaluated>` (default `None`), answered by the board pass's own environments
(`layers::board` `BoardEnv` / `NestedEnv`); a caller's environment is never asked. The
`MatchContext` field set and the `matches` signature are unchanged. In `replacement::etb`, the
copied statics bound to the entrant (`bind_to`) therefore use the entrant's controller after the
existing layer-2 effects; their stored `entry.controller` is only the fallback.
Why: REQ-LAYER-002 (CR 109.5: "you" of a static ability is the current controller of the object
it is on; CR 611.3a: a static effect is not locked in; CR 613.1b: control changes apply in layer
2, before every later layer; CR 613.7/613.8 order only within a layer); REQ-LAYER-001; ETB
projection CR 614.12, 110.2. Issue #203.
Consumers affected: fineprint-core layers pipeline and board, layer cache (`LayerCache`),
replacement::etb projection, every `LayerEnvironment::matches` implementor reading
`MatchContext::effect_controller` (spec-test environments whose stored controller differs from
the source's controller, e.g. `tests/spec_layers_each_layer.rs`
`selector_sees_layer_2_result_and_context_fields`).

## CR-008 — CharacteristicsProvider::controller (REQ-LAYER-001, REQ-CORE-003, REQ-REPL-*): evaluated controller for LKI capture and the replacement chooser
Requested by: #216 FP-160   Kind: additive   Status: open
Change: new provided method on `memory::CharacteristicsProvider`:
`fn controller(&self, id: ObjectId) -> Option<PlayerId>` (default `None`): the controller of `id`
after layer 2 as of now; `None` means the provider names none and the caller falls back to the
stored controller. `layers::LayeredProvider` overrides it with the controller of
`evaluate(effects, env, id)` (`None` for an unknown or unevaluable object). `capture_lki_on_move`
records, for an object leaving the battlefield, `provider.controller(id)` falling back to the zone
record's controller; every other zone is unchanged (`ObjectRecord::rules_controller`). The
destination of a move stays owner-based. In `replacement::bus`, the CR 616.1 chooser of an affected
permanent is `ctx.provider.controller(object)`, falling back to `Permanent::controller`; the
stack-object, affected-player, token and APNAP paths are unchanged. No existing signature, no
`LkiRecord` field or canonical encoding changes; only the recorded controller VALUE can differ
(REQ-NFR-006).
Why: REQ-LAYER-001 (CR 613.1b: control-changing effects apply in layer 2); REQ-REPL-* (CR 616.1:
the affected object's controller chooses); REQ-CORE-003 (CR 603.10a, CR 603.3a, CR 701.21a: a
look-back sacrifice trigger reads the controller as the permanent last existed on the battlefield,
through `LkiRecord::controller`). The SBA registry already reads the evaluated controller (PR #214,
F4); this gives LKI capture and the chooser the same source of truth. Issue #216.
Consumers affected: every `CharacteristicsProvider` implementor (default `None`: base-only fakes
keep the stored-controller behaviour, source-compatible), `layers::LayeredProvider`,
`memory::capture_lki_on_move`, `replacement::bus` (`Bus::chooser`). Known limit: the wrapping
providers `LkiProvider` in `events/exec.rs` and `CeasingLki` in `sba/perform.rs` do not forward
`controller` yet, so the event-driven (`EventContext::execute`) and SBA perform paths fall back to
the stored controller until they forward it; that is a follow-up (those files belong to another
unit, #209).
