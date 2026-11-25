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
