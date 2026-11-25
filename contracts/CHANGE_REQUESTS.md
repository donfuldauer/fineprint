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
