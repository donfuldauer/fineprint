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

## CR-009 — SBA perform (REQ-SBA-001, REQ-REPL-*): `post_batch` is also called before each Prevent follow-up batch
Requested by: #50 gate 06-turn (round 2, R1; change made by round 1, F6/F13)   Kind: breaking   Status: merged (#284)
Change: `sba::perform_all_relayered` and `sba::perform_all_post_batch` now call their `post_batch` provider, besides the single call after the batch for retained zero-defense battles, once before each `Prevent` follow-up batch of the pass's batch (through `replacement::process_batch_refreshing`), so the follow-up batch is resolved and executed with characteristics evaluated from the inputs it supplies. A pass with a prevention follow-up and no retained battle therefore calls it, where it was called at most once and only for a retained battle before. `sba::perform_all` is unchanged (follow-ups use the pre-pass provider). No signature or type changes. Breaking: requested at the phase 06-turn boundary by its gate review (PR #280) and merged between phases (R2) as the contract-only PR #284; the implementation lands with the gate issue PR #279.
Why: REQ-LAYER-001 (CR 613.1b), REQ-REPL-* (CR 616.1), REQ-CORE-003 (CR 603.10a): follow-up batches must see the state the previous batch left. Gate 06-turn F6/F13.
Consumers affected: every caller of the two entry points whose `post_batch` is not a pure function of its context or counts its calls (it must remain pure, R3); none in-tree besides tests.

## CR-010 — MulliganDecision payload validator (REQ-NET-003): TAKE_MULLIGAN rejected on the final request
Requested by: #50 gate 06-turn (round 2, R8)   Kind: breaking   Status: merged (#284)
Change: `PendingChoiceRequest::validate_response` (choice/request.rs) now rejects `TAKE_MULLIGAN` for a `MULLIGAN_DECISION` payload whose `to_bottom` is at least the hand size (the last mulligan: the whole hand goes on the bottom, CR 103.5). Before, every non-KEEP_HAND answer passed the payload check and the game-start procedure rejected TAKE_MULLIGAN afterwards. The payload fields, canonical encoding, wire names and `ValidatorRule` are unchanged. Breaking: requested at the phase 06-turn boundary by its gate review (PR #280) and merged between phases (R2) as the contract-only PR #284; the implementation lands with the gate issue PR #279.
Why: REQ-NET-003 (the request describes the legal answers); gate 06-turn F86/R8.
Consumers affected: any client or host that submitted TAKE_MULLIGAN on the final request (it was a procedure error at the next replay; it is now a submit-time rejection). No signature changes.

## CR-011 — CastState (REQ-CAST-001): hybrid and Phyrexian symbol slots follow the chosen costs
Requested by: #50 gate 06-turn (round 5, CodeRabbit thread 4718412739)   Kind: additive   Status: merged (#286)
Change: new provided method on `casting::CastState`:
`fn symbol_slots(&self, req: &CastRequest, modes: &[u32], costs: &CostChoice) -> Vec<Vec<String>>`
(one option list per hybrid or Phyrexian symbol of the costs that WILL BE PAID once `modes` and
the alternative, additional, X and harmonize parts of `costs` are chosen; `costs.symbol_options`
is empty in the call). Default: `Vec::new()` when `costs.alternative.is_some()` (an alternative
cost is paid rather than the mana cost, CR 118.9), otherwise `self.cost_spec(req, modes).symbol_slots`.
Hosts whose alternative or additional costs carry symbols of their own override it.
`CostSpec::symbol_slots` is documented as the slots of the normal cost. `announce` change: when
the `CostSpec` lists at least one alternative or additional cost, it asks `CHOOSE_COSTS_AND_X`
first WITHOUT symbol slots (payload `symbol_slots` empty), then calls `CastState::symbol_slots`
with the answer and, when the result is non-empty, asks a second `CHOOSE_COSTS_AND_X` whose
payload has empty `alternative_costs`, `additional_costs` and `harmonize_candidates`, `x_range:
None` and the returned `symbol_slots`; its answer is a `ConfigureCosts` with `alternative: None`,
`additional: []`, `x: None`, `harmonize: None` and one option per slot, and the two answers are
merged into the `CostChoice`. When the spec lists neither an alternative nor an additional cost,
the single request is unchanged. `ChoicePayload`, `ChoiceResponse`, `ValidatorRule`, the payload
validator and all canonical encodings are unchanged.
Why: REQ-CAST-001 (SRS 7.1 step 2); CR 601.2b: "If a cost that will be paid as the spell is being
cast includes hybrid mana symbols, the player announces the nonhybrid equivalent cost they intend
to pay." and CR 118.9 (an alternative cost is paid "rather than paying the spell’s mana cost"):
a spell with a hybrid mana cost and a symbol-free alternative cost asked for the hybrid choice
even when the alternative was chosen, and `PendingChoiceRequest::validate_response` requires one
option per slot. Gate 06-turn round 5.
Consumers affected: every `CastState` implementor (source-compatible: the method is provided),
`casting::announce`; a spell with an alternative or additional cost and a symbol slot now takes two
`CHOOSE_COSTS_AND_X` requests (no replay with such a request exists: no integrating host yet).

## CR-012 — CastState (REQ-CAST-001): mode legality probes the cost choices
Requested by: #50 gate 06-turn (round 5, CodeRabbit thread 4718412705)   Kind: additive   Status: merged (#286)
Change: no signature or type changes. `casting::announce` decides whether a mode (an original
index `m`) is offered by probing the host: for the normal cost and for each alternative cost `i` of
`cost_spec(req, &[m])`, combined with additional-cost subsets (every subset when the spec lists at
most 6 additional costs; otherwise none, each one alone, and all), it calls
`target_slots(req, &[m], &probe)` where `probe` is a `CostChoice` with that `alternative`, that
`additional` (ascending), `x` = the low end of the X range if any, empty `symbol_options` and
`harmonize: None`. The mode is offered when at least one probe gives every target slot at least
`min` legal candidates; before, only `CostChoice::default()` was probed. The check of CR 601.2c
with the costs the player actually chose is unchanged. This supersedes the statement of the
decision "#52 FP-050 - Mode legality and index translation" that mode legality cannot depend on costs.
`CastState::target_slots` may therefore be called before the costs are chosen with a non-default
`CostChoice` for a one-mode list; it must stay pure (R3).
Why: REQ-CAST-001; CR 700.2a: "If one of the modes would be illegal (due to an inability to choose
legal targets, for example), that mode can’t be chosen."; CR 601.2c: "A spell may require some
targets only if an alternative or additional cost (such as a kicker cost) or a particular mode was
chosen for it" and "a spell may require alternative targets only if an alternative or additional
cost was chosen for it." A mode whose only legal targets come through an alternative cost was
removed (and the cast rolled back with `NoLegalMode` when it was the only mode). Gate 06-turn round 5.
Consumers affected: `casting::announce`; `CastState::target_slots` implementors (extra calls only).

## CR-013 — CastState (REQ-CAST-001): mode legality probes every additional-cost subset within a budget (amends CR-012)
Requested by: #50 gate 06-turn (round 6, CodeRabbit thread 4718412705 re-opened)   Kind: additive   Status: merged (#287)
Change: no signature or type changes. Amends the probe set of CR-012. `casting::announce` probes the host
for a mode (original index `m`) over the Cartesian product of {the normal cost, each alternative cost
`i` of `cost_spec(req, &[m])`} and EVERY subset of the additional costs of that spec (ascending
bit-mask order, so the empty subset is first, each subset ascending), calling
`target_slots(req, &[m], &probe)` with the probe fields of CR-012 (`x` = low end of the X range,
empty `symbol_options`, `harmonize: None`), and stops at the first probe that gives every target slot at
least `min` legal candidates (mode offered). The exhaustive product is used only when
`(alternatives + 1) * 2^additional <= 4096` (`alternatives` = the number of alternative costs,
`additional` = the number of additional costs, both from the host's spec; computed without overflow,
and for `additional >= 64` the bound is exceeded). That is at most 4096 calls of `target_slots` per
mode, so untrusted card data cannot force more (R12). When the product exceeds the budget no legality
claim can be proved either way, so the mode is OFFERED without calling `target_slots` at all: the CR
601.2c check of step 3 with the costs the player actually chose is unchanged and rolls the cast back
(CR 734.1) with `MissingTarget` if that choice leaves a slot without enough legal candidates, so no
legal mode is ever omitted and no illegal proposal completes. The CR-012 rules "at most 6 additional
costs" and "none, each one alone, and all" are withdrawn. A spell with no alternative and no additional
cost still makes exactly one call per mode. `CastState::target_slots` must stay pure (R3).
Why: REQ-CAST-001; CR 700.2a: "If one of the modes would be illegal (due to an inability to choose
legal targets, for example), that mode can’t be chosen."; CR 601.2c: "A spell may require some targets
only if an alternative or additional cost (such as a kicker cost) or a particular mode was chosen for
it" and "a spell may require alternative targets only if an alternative or additional cost was chosen
for it."; CR 601.2: "If a player is unable to comply with the requirements of a step listed below
while performing that step, the casting of the spell is illegal; the game returns to the moment before
the casting of that spell was proposed". With CR-012 a mode legal only through a subset that was
not probed (for example the pair [2, 5] of seven additional costs) was omitted. A host-supplied legality
hook was rejected: it would add a required or defaulted host method for a check the existing
`target_slots` already answers. Gate 06-turn round 6.
Consumers affected: `casting::announce`; `CastState::target_slots` implementors (up to 4096 calls per
mode before the costs are chosen, instead of at most 64 per alternative); the spec tests that asserted
the omission.

## CR-014 — CastState (REQ-CAST-001): mode legality probes X (amends CR-012 and CR-013)
Requested by: #50 gate 06-turn (round 7, CodeRabbit thread 4720021411 and re-opened thread 4718412705)   Kind: additive   Status: merged (#289)
Change: no signature or type changes. Amends the probe set of CR-013: X is a probed dimension. For a
mode (original index `m`), `casting::announce` probes the host over the Cartesian product of {the
normal cost, each alternative cost `i` of `cost_spec(req, &[m])`} (outermost), EVERY subset of the
additional costs of that spec (ascending bit-mask order, empty subset first, indices ascending;
middle) and EVERY X value of the spec's X range (ascending; innermost), calling
`target_slots(req, &[m], &probe)` with `probe.x = Some(value)` and, as in CR-012 and CR-013, empty
`symbol_options` and `harmonize: None`, and stops at the first probe that gives every target slot at
least `min` legal candidates (mode offered). The X values are `lo..=hi` when `cost_spec` gives
`x_range: Some((lo, hi))` with `lo <= hi`; when `x_range` is `None` the probe has `x: None` (one
value); when `x_range` is `Some((lo, hi))` with `lo > hi` it is the single value `x = Some(lo)`
(as before). The exhaustive product is used only when
`(alternatives + 1) * 2^additional * x_count <= 4096` (`x_count` = `hi - lo + 1`, or 1 as above;
computed without overflow: `hi - lo + 1` of 2^64 or more, an overflowing product, or
`additional >= 64` exceeds the bound). That is still at most 4096 calls of `target_slots` per mode (the
budget of CR-013 now covers the full product). When the product exceeds the budget, including an X
range too wide to enumerate, the mode is OFFERED without calling `target_slots` at all, exactly as
CR-013 says: the CR 601.2c check of step 3 with the costs and X the player actually chose rolls the
cast back (CR 734.1) with `MissingTarget` if that choice leaves a slot without enough legal
candidates. A spell with no alternative cost, no additional cost and no X range (or a one-value X
range) still makes exactly one call per mode. Hybrid and Phyrexian symbol choices and the harmonize
creature are NOT probed dimensions: they do not change which targets a spell requires (CR 601.2c lists
the mode and alternative or additional costs), and the harmonize creature is tapped in payment
(601.2h), after targets. `CastState::target_slots` must stay pure (R3).
Why: REQ-CAST-001; CR 601.2b: "...If the spell has a variable cost that will be paid as it’s being
cast (such as an {X} in its mana cost; see rule 107.3), the player announces the value of that
variable." and CR 601.2c (targets are announced after 601.2b, so X is known): a target restriction can
refer to X (for example a target whose mana value equals X), so a mode with no legal target at the low
end of the X range and legal targets at a higher X is NOT illegal under CR 700.2a: "If one of the modes
would be illegal (due to an inability to choose legal targets, for example), that mode can’t be
chosen." and was omitted by CR-013, whose check at step 3 runs only after the costs are chosen and
cannot recover a mode that was never offered. CR 107.3a: "...the controller of that spell or ability
chooses and announces the value of X as part of casting the spell or activating the ability."
Alternatives rejected: probing only the low and high end of the X range (unsound: a legal middle X would
be omitted); a host-supplied legality hook (extra host method for what `target_slots` already answers).
Gate 06-turn round 7.
Consumers affected: `casting::announce`; `CastState::target_slots` implementors (up to 4096 calls per
mode before the costs are chosen, now including calls with different `x`); the unit and spec tests that
asserted a single low-end X probe.

## CR-015 — PAY_MANA_AND_COSTS (REQ-CAST-001): the mana-ability window is asked again after each answer that activates abilities
Requested by: #50 gate 06-turn (round 7, CodeRabbit thread 4720021419)   Kind: additive   Status: merged (#289)
Change: no type, field, `ValidatorRule`, validator or canonical-encoding change (`ChoicePayload::PayManaAndCosts`
and `ChoiceResponse::PayCosts` are unchanged); only the request SEQUENCE of step 7 changes, as CR-011
changed the sequence of CHOOSE_COSTS_AND_X. `casting::pay_total_cost` asks PAY_MANA_AND_COSTS one or more
times while the locked mana part is payable and not {0}. Each request lists `mana_abilities` = the abilities
available NOW (`PayState::available_mana_abilities`, ascending, deduplicated) on a state in which the
activations of the earlier answers of this window have been applied, and `methods` and `auto_pay`
computed from that state. An answer that lists at least one ability and in which at least one listed
ability is actually activated (it is available at that moment, not already in progress, and the host
accepts it; abilities are tried in listed order, CR 605.3c) is followed by a NEW request; the `methods`
of such an answer are discarded and its `auto_pay` is ignored (the player chooses them in the request that
ends the window). The window ends with the first answer that is `auto_pay: true`, or lists no ability, or
lists abilities none of which was activated, and on CANCEL_AND_ROLLBACK (which reverses the cast; nothing
has changed in the real state). At most 256 requests are asked per window: the 256th lists no
`mana_abilities`, so it ends the window. Nothing in the real state changes until the window has ended:
the earlier rounds are simulated on a copy of the state, then the concatenation of all listed abilities is
activated for real, in order, exactly as if one answer had listed them (the CR 734.1 dependent-activation
probe covers the concatenation, so an activation of an early round whose mana a library-touching
activation of a later round consumed is committed durably). A request still rejects by membership any
ability not in its own list (R12). `activate_all` keeps checking availability at each activation. An answer
that lists no ability gives the single request of before.
Why: REQ-CAST-001 (SRS 7.1 step 7); CR 601.2g: "If the total cost includes a mana payment, the player
then has a chance to activate mana abilities (see rule 605, "Mana Abilities"). Mana abilities must be
activated before costs are paid."; CR 605.3a: "A player may activate an activated mana ability whenever
they have priority, whenever they are casting a spell or activating an ability that requires a mana
payment, ..."; CR 605.3b: "...it resolves immediately after it is activated."; CR 605.3c: "Once a player
begins to activate a mana ability, that ability can’t be activated again until it has resolved." A single
snapshot offer made an ability that becomes available only after an earlier activation of the same window
(for example one whose source an earlier ability untaps, or whose mana cost needs the mana an earlier
ability produced) unselectable, so the cast rolled back. Alternatives rejected: a host method listing
abilities that could become activatable (the client could not tell which are legal now); a payload field of
conditional abilities (changes `ChoicePayload` and its canonical encoding: breaking); dropping the
membership check (weakens R12). Gate 06-turn round 7.
Consumers affected: `casting::pay` and its unit tests; every client of PAY_MANA_AND_COSTS (an answer that
activates abilities is now followed by another request; no replay with such a request exists: no
integrating host yet).

## CR-016 — PayState (REQ-CAST-001, REQ-CORE-005): the host reports which pool units the last mana-ability activation removed
Requested by: #50 gate 06-turn (round 7, CodeRabbit thread 4720021426 and comment 4720082538)   Kind: additive   Status: merged (#289)
Change: new provided method on `casting::PayState`:
`fn last_activation_removed(&self, player: PlayerId) -> Option<Vec<usize>>`, default `None`.
After a successful `activate_mana_ability(player, ..)`, `Some(v)` lists every unit of
`mana_pool(player)` that this call removed (its cost payment and the triggered mana abilities it caused,
together), as indices into that pool as it was immediately BEFORE the call, strictly ascending, without
duplicates. The units that were not removed keep their relative order and the units the call added come
after them, in the order added. A unit the call removed and an equal unit the call added are reported as
removed and added respectively (the pool may be identical before and after). The value is transient
bookkeeping of the last call: it must not enter the state hash or any canonical encoding, it must survive
`Clone`, and it is read only directly after a successful call on the same value (the scratch copy of the
dependent-activation probe). `None` means the host does not report. `casting::pay` uses `Some(v)` when
it is valid (every index below the pool length before the call, strictly ascending, and
`before.len() - v.len() <= after.len()`); otherwise, and for `None`, it infers the removal from the pool
before and after: the removed units are the multiset difference, each credited to the earliest equal unit
(lowest pool index among equal units), the survivors keep their order and the extra units at the end are
new. The inference is sound exactly when (1) the host removes, among equal units, the lowest-index one
(the tie-break of `plan_payment`) and (2) the activation does not both remove a unit and add an equal unit.
Documented host requirement (added to the `PayState::activate_mana_ability` documentation): a host that
cannot guarantee both MUST override `last_activation_removed`. `ManaActivation` (adding a field is
breaking: it is not `#[non_exhaustive]` and hosts build it as a struct literal), `ManaUnit` equality,
`ChoicePayload` and all encodings are unchanged.
Why: REQ-CORE-005; CR 734.1: "...Each player may also reverse any legal mana abilities that player
activated while making the illegal play, unless mana from those abilities or from any triggered mana
abilities they caused to trigger was spent on another mana ability that wasn’t reversed." and SRS 2.5(a)
(the exception is mandatory). `ManaUnit` equality ignores production history, so a before/after pool
comparison cannot tell which of two equal units an activation removed, and sees nothing when a durable
activation spends a unit produced by an earlier activation and produces an equal one (the pool is
identical): the dependency is missed and the earlier activation is reversed although its mana was spent
on an activation that is not reversed. Alternatives rejected: a `consumed` field on `ManaActivation`
(breaking); a production id on `ManaUnit` (breaking: equality, canonical encoding, state hash; very-high
blast radius); inference only (relies on host invariants the contract did not state). Gate 06-turn round 7.
Consumers affected: `casting::pay::probe_activations`; every `PayState` implementor (source-compatible: the
method is provided; hosts that spend and reproduce equal units, or remove other than the lowest-index equal
unit, must override it).
