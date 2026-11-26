# Development guide

This file replaces a plan file: it holds the layout, commands, gates and contracts. The plan
itself is the set of GitHub issues labelled `work-unit` (milestones M1-M3). Process rules are in
`docs/PROCESS.md` (P1-P8, R1-R12); requirements are in `docs/SRS.md`.

## Toolchain

- Rust is pinned to exactly `1.97.0` in `rust-toolchain.toml` (profile `minimal`, components
  rustfmt and clippy). CI and every developer build use it. Changing it is a recorded decision
  in `docs/DECISIONS.md`.
- No extra targets are installed yet. The wasm target and `llvm-tools` arrive with later issues
  (CI #3, coverage #4, WASM binding #104).
- Platforms (REQ-NFR-003): Linux x86_64 `x86_64-unknown-linux-gnu`; Windows x86_64
  `x86_64-pc-windows-msvc`; macOS arm64 `aarch64-apple-darwin`; browsers
  `wasm32-unknown-unknown`.
- Edition 2024, resolver 3, license MPL-2.0 in every crate, `publish = false`.
- Satisfied by this layout: REQ-NFR-001 (one workspace, exact pin, unsafe confined to the FFI
  crate), REQ-ARCH-003 (crate and artifact naming), REQ-NFR-012 (MPL-2.0 in every package and
  header in every source file).

### Cloud agent environment

- Fresh Antigravity cloud VMs automatically run `.agents/environment.json` →
  `.agents/setup-env.sh` from the repository root (idempotent and safe to re-run with
  `bash .agents/setup-env.sh`).
- `.agents/setup-env.sh` ensures the pinned Rust toolchain from `rust-toolchain.toml` is installed
  with `rustfmt`, `clippy`, `llvm-tools`, and the `wasm32-unknown-unknown` target; installs
  `cargo-llvm-cov` at the exact version pinned in `.github/workflows/ci.yml` (prebuilt Linux binary
  with `cargo install --locked` fallback); installs any missing Linux prerequisites
  (`build-essential`, `pkg-config`, `curl`, `ca-certificates`, `python3`, `jq`, `git`); persists
  Cargo's bin directory in `~/.bashrc` for non-interactive shells; and warms the locked Cargo cache
  for the root workspace and `spikes/bindings`.
- Commit attribution: `scripts/install-git-hooks.sh` sets `core.hooksPath=.githooks` only when
  no other hook setup exists: it does not replace a `core.hooksPath` that points elsewhere or
  non-sample hooks (regular files or symbolic links) already in `.git/hooks`, but prints the manual
  command instead; `fineprint.coauthor` is always recorded; `FINEPRINT_FORCE_HOOKS=1` forces the
  replacement. When it prints "NOT enabling .githooks", an agent must make `.githooks/commit-msg`
  effective before any commit (call `.githooks/commit-msg "$1"` from the existing `commit-msg`
  hook, preserving every existing hook, or, only when none has to be kept, re-run with
  `FINEPRINT_FORCE_HOOKS=1`); no agent commit may be made while the trailer hook is not effective.
  `.githooks/commit-msg` then adds `Co-authored-by:` for the human contributor
  to agent-authored commits (`AGENTS.md` "Commit attribution"). Claude Code runs the script at
  `SessionStart`; `.agents/setup-env.sh` runs it on Antigravity VMs.
- Each issue works in its own git worktree `../fineprint-issue-<N>` (`work-issue` skill §2), or
  directly on a branch inside the checkout when a cloud sandbox restricts sibling directories and
  only one issue runs in the VM.
- Agent instructions and definitions are shared across Claude Code and Gemini / Antigravity:
  `AGENTS.md` is canonical (imported by `CLAUDE.md` and `GEMINI.md`), `.claude/agents/` and
  `.claude/skills/` hold the canonical prompts and skill procedures, and `.agents/agents/` and
  `.agents/skills/` provide Antigravity / Gemini adapters plus `pr-watch`.

## Workspace and crate map

| Crate | Path | Role | Depends on |
|---|---|---|---|
| `fineprint-base` | `crates/fineprint-base` | rules crate: shared base types | sha2 (SHA-256, FP-004) |
| `fineprint-dsl` | `crates/fineprint-dsl` | rules crate: DSL types and runtime | base, serde, serde_json, serde_json_canonicalizer (FP-010) |
| `fineprint-core` | `crates/fineprint-core` | rules crate: the rules engine | base, dsl, serde, rmp-serde (FP-017) |
| `fineprint-ingest` | `crates/fineprint-ingest` | offline card toolchain | base, dsl (not core: REQ-NFR-005) |
| `fineprint-ffi` | `crates/fineprint-ffi` | C ABI; the only crate allowed unsafe code | core |
| `fineprint-wasm` | `crates/fineprint-wasm` | WebAssembly binding | core |
| `fineprint-bot` | `crates/fineprint-bot` | reference bots | core |
| `fineprint-cli` | `clients/cli` | terminal client (library stub now) | core, ingest, bot |
| `xtask` | `xtask` | repository automation (binary) | nothing |

Dependency direction is downward only: base, dsl, core, then bindings and tools. The rules
crates are base, dsl and core: no floats, no `HashMap`/`HashSet` (REQ-CORE-006, enforced by
`clippy.toml` `disallowed-types`). The other crates carry
`#![allow(clippy::disallowed_types)]`. Every crate except `fineprint-ffi` has
`#![forbid(unsafe_code)]`.

### Naming (REQ-ARCH-003)

| Item | Name |
|---|---|
| Product / machine id | Fine Print / `fineprint` |
| Rust crate / .NET assembly / C header / WASM module | `fineprint-core` / `FinePrint.Core` / `fineprint.h` / `fineprint.wasm` |
| API namespace | `FinePrint` (for example `FinePrint.IRulesEngine`) |
| Offline card toolchain | `fineprint-ingest` |
| Card database artifact | `*.fpdb` (`CardDatabaseArtifact`) |
| Scenario / test files | `*.pzl` |
| Replay logs | `*.fpreplay` |

## `fineprint-core` module map

Modules live under `crates/fineprint-core/src/`. None exist yet; each directory is created by
the first issue that needs it.

| Module | Owns |
|---|---|
| `state/` | the game state model and canonical hash inputs; created by the first issue that needs it |
| `zones/` | zone containers and ordering; created by the first issue that needs it |
| `permanent/` | battlefield permanent state; created by the first issue that needs it |
| `objects/` | game objects and identity; created by the first issue that needs it |
| `layouts/` | card layouts and faces; created by the first issue that needs it |
| `memory/` | remembered information about objects; created by the first issue that needs it |
| `knowledge/` | hidden-information and knowledge tracking; created by the first issue that needs it |
| `txn/` | transactions and rollback; created by the first issue that needs it |
| `events/` | the event model and event log; created by the first issue that needs it |
| `choice/` | the pending-choice protocol; created by the first issue that needs it |
| `replacement/` | replacement and prevention effects; created by the first issue that needs it |
| `triggers/` | triggered abilities and the stack of triggers; created by the first issue that needs it |
| `layers/` | continuous-effect layering and dependencies; created by the first issue that needs it |
| `sba/` | state-based actions; created by the first issue that needs it |
| `turn/` | turn, phase and step structure; created by the first issue that needs it |
| `actions/` | player and special actions; created by the first issue that needs it |
| `casting/` | casting and activation procedure; created by the first issue that needs it |
| `mana/` | mana pool and payment; created by the first issue that needs it |
| `resolve/` | stack resolution; created by the first issue that needs it |
| `interp/` | the DSL interpreter; created by the first issue that needs it |
| `effects/` | effect primitives; created by the first issue that needs it |
| `designations/` | designations and markers; created by the first issue that needs it |
| `combat/` | combat procedure; created by the first issue that needs it |
| `keywords/` | keyword macros; created by the first issue that needs it |
| `native/` | native-effect registry; created by the first issue that needs it |
| `loops/` | loop detection and shortcuts; created by the first issue that needs it |
| `scenario/` | scenario loading for tests; created by the first issue that needs it |
| `api/` | the public engine facade; created by the first issue that needs it |
| `view/` | redacted per-player views; created by the first issue that needs it |
| `replay/` | replay recording and playback; created by the first issue that needs it |
| `sandbox/` | what-if evaluation; created by the first issue that needs it |
| `fork/` | state forking; created by the first issue that needs it |
| `support/` | support-report data; created by the first issue that needs it |
| `format/` | shared formatting helpers; created by the first issue that needs it |

## Tests

- Unit tests sit next to the code. Independent spec tests are named
  `crates/<crate>/tests/spec_<area>_*.rs`.
- Every test names the `REQ-*` IDs it covers. Rules behaviour gets a `.pzl` scenario once the
  format exists (#51 FP-049).
- Fixtures use synthetic cards only (R5): `tests/fixtures/cards/`, scenarios in
  `tests/scenarios/` (`.pzl`), golden hashes in `tests/golden/`.

## Commands

Available now:

```
cargo build --workspace
cargo test --workspace
cargo fmt --all --check          # apply with: cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask coverage                 # runs cargo llvm-cov, prints the per-crate table, checks floors
cargo xtask coverage --input <file>  # same, from `cargo llvm-cov --json --summary-only` output
cargo xtask coverage --root <dir>    # workspace root for path-to-crate mapping (default: git toplevel, else cwd; a relative dir is resolved against the current directory)
                                       # exits 1 if the report has files but none maps to a workspace crate under the root
cargo xtask guard headers            # MPL-2.0 header in every source file
cargo xtask guard card-data          # no committed card data or rules text
cargo xtask guard card-branch        # no card-name/id literal comparisons in fineprint-core
cargo xtask guard all                # the three guards, every violation reported
cargo xtask cr fetch [--url <url>] [--force] [--cache <dir>]
                                     # download the official CR text into git-ignored cr-cache/ (uses curl)
cargo xtask cr index [--cache <dir>] [--root <dir>] [--out <file>]
                                     # parse the CR, scan citations, write docs/cr-citations.json
cargo xtask cr check [--cache <dir>] [--root <dir>] [--index <file>]
                                     # fail on a cited rule missing from the CR, not indexed, or with changed text
cargo run --locked -p fineprint-ingest -- vocab --cr cr-cache/rules.txt
                                     # regenerate crates/fineprint-dsl/data/vocab.json (RulesVocabulary, #15 FP-014)
cargo run --locked -p fineprint-ingest -- vocab --cr cr-cache/rules.txt --check
                                     # compare the committed vocab.json with regeneration (writes nothing)
cargo run --locked -p fineprint-ingest -- fetch [--cache <dir>] [--force] [--mtgjson]
                                     # download the Scryfall Oracle Cards and Rulings bulk files (and,
                                     # with --mtgjson, MTGJSON AtomicCards) into git-ignored ingest-cache/,
                                     # at most once per 24 hours (uses curl; #87 FP-085)
cargo run --locked -p fineprint-ingest -- import [--cache <dir>] [--out <file>|-] [--mtgjson-report <file>|-] [--strict]
                                     # normalize the cached bulk files into ingest-cache/normalized/cards.json
FINEPRINT_REQUIRE_CR_CACHE=1 cargo test --locked -p fineprint-ingest --test spec_ingest_vocab
                                     # the real-CR vocabulary test; with the variable set a missing
                                     # cr-cache/rules.txt fails it (without it, the test is skipped)
cargo test --workspace --locked --features fineprint-core/full-recompute
                                     # the whole suite with the layer cache bypassed (REQ-LAYER-005)
rm -rf target/differential && \
FINEPRINT_DIFF_DIR="$PWD/target/differential/cached" cargo test --locked -p fineprint-core --test spec_layers_cache_diff --test spec_layers_cache_diff_state && \
FINEPRINT_DIFF_DIR="$PWD/target/differential/full" cargo test --locked -p fineprint-core --features full-recompute --test spec_layers_cache_diff --test spec_layers_cache_diff_state && \
diff -r target/differential/cached target/differential/full
                                     # layer-result digests (*.digest) and GameState state hashes (*.statehash) of the differential scenarios, both modes; must be identical
cargo bench --locked -p fineprint-core --bench layers_recompute -- --noplot
                                     # 200-permanent layer recompute benchmark (criterion; REQ-NFR-009)
cargo llvm-cov --workspace --all-targets --no-default-ignore-filename-regex --ignore-filename-regex '(/\.cargo/(registry|git)/|/\.rustup/|/rustc/)' --json --summary-only   # needs: rustup component add llvm-tools;
                                                   # cargo install cargo-llvm-cov --locked --version 0.9.1
```

`cargo xtask cr` (REQ-DOC-001, REQ-NFR-007; details in `xtask/src/cr/mod.rs` and its
submodules): the rules body is the text between the last `1. Game Concepts` line and the last
`Glossary` line; a rule's text is its first line plus following non-blank lines up to a blank line
or the next rule (examples included); a section heading's text is the heading line. Normalization
`sha256-normalized-v1`: strip the BOM, replace every run of Unicode whitespace (CR and LF
included) with one ASCII space, trim; nothing else changes. The hash is the lowercase hex SHA-256
of the normalized text. Citations come from `docs/SRS.md` (`CR` lists and bare dotted rule
numbers) and from `CR ...` lists in comments of tracked `.rs .cs .c .cpp .h .ts .js .py .toml .yml
.yaml` files, except `xtask/src/cr/` (its own fixtures). `check` compares citation sets and hashes,
never line numbers.

Environment: none needed. `RUSTFLAGS="-D warnings"` is optional; `CARGO_TERM_COLOR=never` for
logs. xtask is run as `cargo xtask <command>` (alias in `.cargo/config.toml`);
`cargo run -p xtask -- <command>` is equivalent. xtask uses the standard library only and takes
its file list from `git ls-files`, so new files must be `git add`ed to be checked. Exit codes: 0
success (one summary line on stdout), 1 violations (`path:line: message` per line on stderr),
2 usage or input error.

`fineprint-ingest vocab --cr <rules.txt> [--out <file>|-] [--check]` (#15 FP-014; details in
`crates/fineprint-ingest/src/vocab/mod.rs`): generates the `RulesVocabulary` from the cached CR
text. `--out` defaults to `crates/fineprint-dsl/data/vocab.json` relative to the current directory
(run it from the repository root); `--out -` writes the JSON to stdout and the summary line to
stderr. A file is written atomically (a temporary file next to it, then a rename). `--cr` and
`--out` never take one of the options as their value (`--out --check` is a usage error; write
`./--check` for such a file). `--check` writes nothing and compares bytes with the `--out` file;
a mismatch names the first differing top-level member (never a value from the file) and says to
rerun the same command without `--check`. A missing default `vocab.json` must be restored from
git (`git checkout -- crates/fineprint-dsl/data/vocab.json`): `fineprint-dsl` embeds it, so
nothing builds without it. Exit codes as for xtask: 0 success, 1 violations (check
mismatch, missing committed file, keyword id collision), 2 usage or input error (unreadable or
unparseable CR text, a missing required rule). Output is byte-identical for the same CR bytes
whatever the line endings or BOM.

`fineprint-ingest fetch [--cache <dir>] [--force] [--mtgjson]` (#87 FP-085; details in
`crates/fineprint-ingest/src/import/mod.rs`): makes at most one request to the Scryfall
`bulk-data` metadata endpoint per run (none when every needed file is fresh) and downloads the
`oracle_cards` and `rulings` entries' `download_uri` (accepted only as `https://` on
`scryfall.com`, `*.scryfall.com` or `*.scryfall.io`, never the API host `api.scryfall.com`, and
without curl glob characters `[ ] { }`); it never requests a per-card endpoint.
`--mtgjson` also downloads MTGJSON `AtomicCards.json`; without it MTGJSON is never contacted. A file
fetched less than 24 hours ago is skipped unless `--force`. The cache (default `ingest-cache/`
relative to the current directory, git-ignored) holds `scryfall/oracle-cards.json`,
`scryfall/rulings.json`, `mtgjson/AtomicCards.json` and a `source.json` per group
(`sourceUrl`, `updatedAt`, `sha256`, `byteCount`, `fetchedAtEpochSeconds` per file). Concurrent
runs on one cache serialize on an exclusive lock of `<group>/.lock` (the lock file is left in
place). Downloads go to a per-run part file `{file}.{pid}.part` that is validated; a failed or
invalid download leaves the cache unchanged. The commit then writes `source.json` without the
entries of the files being replaced, renames the part files into place, and writes the full new
`source.json` (each write atomic), so after any failure a file is either absent from the metadata
(refetched next run) or consistent with it. It needs `curl` on the `PATH`; every response (the
1 MiB metadata and each bulk file, at most the per-file cap) is streamed through the program and
cut off, the curl process killed, as soon as the cap is exceeded, so the cap does not depend on the
curl or libcurl version (`--max-filesize` stays as a second line of defence; a curl older than
8.4.0 or an unparseable version only prints a warning). curl runs with `--disable` (no `~/.curlrc`) and `--globoff`, HTTPS only, and
follows no redirect (proxy and CA settings come from the environment, as for `cargo xtask cr
fetch`). Exit codes: 0 success, 2 usage, input or network error.

`fineprint-ingest import [--cache <dir>] [--out <file>|-] [--mtgjson-report <file>|-] [--strict]`
(#87 FP-085): reads the cached bulk files and writes the normalized records, keyed and sorted by
`oracle_id`, with the sources' `updatedAt` and sha256, the rulings, the rejected objects and the
warnings, to `<cache>/normalized/cards.json` (or `--out`; `-` is stdout, the summary line then goes
to stderr). Mana value, colors and color identity are recomputed with integers; the float `cmc` only
flags a mismatch. `--mtgjson-report` writes the MTGJSON cross-validation report (needs
`fetch --mtgjson`). `--out` and `--mtgjson-report` must not name the same file (exit 2, nothing written); a
report destination that only aliases the records file on the file system (for example a name that
differs in case on a case-insensitive volume) is also rejected, after the records are written: exit
2, no report written, the records file intact. A missing Oracle Cards file is exit 2 (run `fetch`); rejected records and
diagnostics are reported and exit 0, or 1 with `--strict`. Output is byte-identical for the same
cache. Card data stays in the git-ignored cache or the explicit `--out`/`--mtgjson-report` path.

CI format, lint and test on Linux, Windows and macOS is available now; see "CI" below.

Lands later:

- `cargo run -p xtask -- check-paths`: not delivered by #4 FP-003 (its guards are the three
  above); a path check lands only if a later issue asks for it.
- `.pzl` runner `cargo run -p fineprint-cli -- pzl <file>` or an xtask equivalent (final syntax
  set by #51 FP-049).
- Replay and golden-hash verification: lands with #83 FP-081 and #106 FP-104.
- Benchmarks beyond the layer recompute benchmark (#36 FP-035): land with #120 FP-118. Fuzzer: lands with #119 FP-117.
- Toolchain CLI `fineprint-ingest`: the binary and its `vocab` subcommand landed with #15 FP-014
  and `fetch`/`import` (the Scryfall bulk import) with #87 FP-085; the card-database commands
  land with #100 FP-098.
- C ABI header check (cbindgen): lands with #102 FP-100.
- `dotnet test` for FinePrint.Core: lands with #103 FP-101.
- `wasm-pack test` / wasm-bindgen + npm: lands with #104 FP-102.
- Parser-coverage measurement from the CI cache: lands with #101 FP-099.

## CI

The workflow is `.github/workflows/ci.yml` (owned by #3 FP-002). It runs on every
`pull_request` and on pushes to `main` only (a push to a pull-request branch is already covered
by its `pull_request` run); a newer run on the same ref cancels the superseded one (concurrency
group per workflow and ref). The first job, `changes`, decides whether a pull request changes
anything besides documentation: Markdown files other than `docs/SRS.md` (which the xtask `cr`
tests read), and `.claude/**`. If it changes only documentation, `fmt`, `clippy` and `test` still
report success under their required names but skip their steps, and `coverage` is skipped;
`guards` and `req-coverage` always run, and every push to `main` runs everything
(docs/DECISIONS.md "CI — PR load"). The toolchain comes from `rust-toolchain.toml` (no version is repeated in
the workflow). Caches of the cargo registry and `target/` are keyed on OS, toolchain and
`Cargo.lock`, with a fallback to the same OS and toolchain. Line endings are checked out as
committed (`core.autocrlf false`) so all three systems see the same bytes.

| Job name | Command |
|---|---|
| `changes` | `git diff --no-renames --name-only` between the pull request's base and head; outputs `code` (always `true` on a push) |
| `fmt (ubuntu-latest)` | `cargo fmt --all --check` |
| `clippy (ubuntu-latest)` | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| `test (ubuntu-latest)` | `cargo test --workspace --locked` |
| `test (windows-latest)` | `cargo test --workspace --locked` |
| `test (macos-latest)` | `cargo test --workspace --locked` |
| `coverage (ubuntu-latest)` | `cargo llvm-cov --workspace --locked --all-targets --no-default-ignore-filename-regex --ignore-filename-regex '(/\.cargo/(registry\|git)/\|/\.rustup/\|/rustc/)' --json --summary-only --output-path target/llvm-cov-summary.json`, then `cargo xtask coverage --input target/llvm-cov-summary.json` (cargo-llvm-cov 0.9.1, `llvm-tools` added in the job) |
| `guards` | `cargo xtask guard all` |
| `differential (ubuntu-latest)` | `cargo test --workspace --locked --features fineprint-core/full-recompute`, then `spec_layers_cache_diff` (layer-result `*.digest` files) and `spec_layers_cache_diff_state` (`GameState::state_hash` `*.statehash` files) in both modes with an absolute `FINEPRINT_DIFF_DIR` (`target/differential/cached` and `target/differential/full`), then a step that fails unless both directories hold the same files with identical bytes and at least 7 `*.statehash` files (REQ-LAYER-005; the cached-mode suite is the `test` job). Skips its steps on documentation-only pull requests like `test` |
| `bench (ubuntu-latest)` | `cargo bench --locked -p fineprint-core --bench layers_recompute -- --noplot`; Criterion writes to `$RUNNER_TEMP/criterion` (`CRITERION_HOME`, outside the cached `target`, so no earlier result is published); the summary step fails unless all four groups have results, writes their median table to the job summary, and that directory is uploaded as an artifact (actions/upload-artifact v4.6.2) for the trend (REQ-NFR-009). NOT a required status check |
| `req-coverage` | `python3 scripts/req_coverage.py` (whole plan, `GH_TOKEN` from `github.token`, permissions `contents: read`, `issues: read`) |
| `cr` (workflow `.github/workflows/cr.yml`) | `cargo xtask cr fetch` (pull requests; `--force` on the weekly schedule and `workflow_dispatch`), then `cargo xtask cr check`, then `cargo run --locked -p fineprint-ingest -- vocab --cr cr-cache/rules.txt --check` (runs even if the citation check failed), then `cargo test --locked -p fineprint-ingest --test spec_ingest_vocab` with `FINEPRINT_REQUIRE_CR_CACHE=1` (step `env:`; the real-CR test fails instead of skipping when `cr-cache/rules.txt` is missing; also runs even if an earlier check failed); `cr-cache/` cached with actions/cache. Runs on pull requests touching `docs/SRS.md`, `docs/cr-citations.json`, `xtask/src/cr/**`, `crates/fineprint-dsl/data/vocab.json`, `crates/fineprint-dsl/src/vocab/**`, `crates/fineprint-ingest/src/vocab/**`, `**/*.rs` or the workflow, and weekly (Monday). NOT a required status check (it depends on the network) |

`fmt` and `clippy` run on Linux only: formatting and lints do not depend on the OS, and `test`
builds and runs the whole workspace on all three systems (REQ-NFR-003). The five job names
`fmt (ubuntu-latest)`, `clippy (ubuntu-latest)` and `test (ubuntu-latest|windows-latest|macos-latest)`
are the contexts listed as required status checks on `main` (REQ-NFR-011); the maintainer adds `coverage (ubuntu-latest)`, `guards` and `req-coverage`
after #4 merges (docs/HUMAN_QUEUE.md). Do not rename them without updating branch protection. Binding and
browser jobs are added by later issues (#6, #104).

## Coverage floors (REQ-NFR-008)

Line coverage by `cargo-llvm-cov`: at least 90% for the rules core (state model, zones, objects
and layouts, events, layers, turn structure, casting, SBAs, DSL runtime, API and redaction); at
least 75% for everything else (bindings, `fineprint-ingest`, CLI and tooling). Each new public
function has at least one test. `cargo xtask coverage` enforces these per crate; the table lives
in `xtask/src/coverage.rs` (`FLOORS`) and is mirrored here.

What is measured: every workspace crate's `src/`, `tests/`, `examples/` and `benches/` files
(`--all-targets` with cargo-llvm-cov's default filename filter turned off). Excluded: registry and
git dependency sources (`/.cargo/registry/`, `/.cargo/git/`), the rustup toolchain (`/.rustup/`)
and std sources (`/rustc/`). Files outside the workspace crates (for example `spikes/bindings`) are
not attributed to any crate. `xtask coverage` runs the same flags as the CI job
(`llvm_cov_args` in `xtask/src/coverage.rs`).

| Crate | Floor |
|---|---|
| `fineprint-base` | 90% |
| `fineprint-dsl` | 90% |
| `fineprint-core` | 90% |
| `fineprint-ingest` | 75% |
| `fineprint-ffi` | 75% |
| `fineprint-wasm` | 75% |
| `fineprint-bot` | 75% |
| `fineprint-cli` | 75% |
| `xtask` | 75% |
| `fineprint-spike-libs` | 75% |

- Percent per crate is the sum of covered lines over the sum of measured lines of all that
  crate's files (not an average of per-file percentages), judged with integer arithmetic:
  `covered * 100 >= floor * lines`. Files outside the workspace (registry sources) are ignored.
- A crate with zero measured lines, or absent from the JSON, prints `n/a` and passes (the
  stub crates have no code yet); the row is still printed.
- A workspace crate found in the JSON but missing from the table fails the check: add it to
  `FLOORS` in `xtask/src/coverage.rs` and to the table above in the same change; a new crate
  directory outside `crates/<dir>`, `clients/cli`, `xtask` and `spikes/libs` also needs an arm in
  `crate_of_rooted`. `spikes/libs/**` (`src`, `examples`, `tests`) is measured as
  `fineprint-spike-libs` with no exemption.
- CI installs `llvm-tools` and `cargo-llvm-cov` itself (not in `rust-toolchain.toml`).

## Repository guards (REQ-NFR-012, REQ-NFR-004, REQ-ARCH-001, REQ-DSL-013)

`cargo xtask guard` runs over `git ls-files` (tracked and staged files).

- `headers`: every `.rs .cs .c .cpp .h .ts .js .py` file has the MPL-2.0 header sentence within
  its first 20 lines (after an optional `#!` line); comment markers (`//`, `///`, `//!`, `#`,
  `/*`, `*`, `<!--`) are stripped and whitespace collapsed before a case-sensitive containment
  check. Skipped: paths under `target/`, `node_modules/`, `dist/`. Generated files need the
  header too.
- `card-data`: file-name checks apply everywhere, including `docs/` and `.github/ISSUE_TEMPLATE`;
  content checks are skipped under those two. Rejected: `*.fpdb`,
  `*.cr.txt`; file names `scryfall-*.json`, `oracle-cards-*.json`, `default-cards-*.json`,
  `all-cards-*.json`, `unique-artwork-*.json`, `rulings-*.json`, `AtomicCards*.json`,
  `AllPrintings*.json`, `AllIdentifiers*.json` (case-insensitive); `.json` files with an
  `oracle_text` or `oracleText` key; any text file with five or more lines that start with a
  rules-number shape followed by prose. Content is scanned for UTF-8 files under 5 MiB; a file of
  5 MiB or more outside the exempt directories is reported as an incomplete scan (a violation).
- `card-branch`: `.rs` files under `crates/fineprint-core/` must not compare `card_def_id`,
  `cardDefId`, `card_name`, `name`, `def_id`, `definition_id` (including `.name()` and
  `.card_def_id()`) with a string literal: `x == "..."`, `"..." != x`, `matches!(x, "...")`,
  `match x { "..." => .. }`, `x.eq("...")`, `x.eq_ignore_ascii_case("...")`. Comments are
  ignored. Allowlist: files under `crates/fineprint-core/src/support/`, and a line ending in
  `// guard-allow: card-branch (<reason>)` with a non-empty reason. The full list of what the
  scan does not catch is in the doc comment of `xtask/src/guard.rs`.

## Cache and fixture paths

- `ingest-cache/`: git-ignored bulk-data cache; never committed or published.
- `cr-cache/`: git-ignored fetched rules text (`rules.txt` plus `source.json` with the source
  URL, effective date, SHA-256 and byte count, written by `cargo xtask cr fetch`); rules text is
  never committed, only the citation index `docs/cr-citations.json` (#7 FP-006) is.
- Committed synthetic data: `tests/fixtures/cards/`, `tests/scenarios/`, `tests/golden/`.
- Already git-ignored: `target/`, `*.fpdb`, `*.cr.txt`, `*.wasm`, bulk-data json
  (`scryfall-*.json`, `oracle-cards-*.json`, `rulings-*.json`, `AtomicCards*.json`), `dist/`,
  `node_modules/`, `.env`, `*.log`.

## Owned paths and shared files

Each issue lists its owned paths (R1); change nothing outside them. Shared append-only files
(`docs/DECISIONS.md`, `docs/HUMAN_QUEUE.md`) take appended entries only. Contract changes after
the owning unit merges go through `contracts/CHANGE_REQUESTS.md` (R2). Blast radius levels
(local, module, very-high) are defined in PROCESS.md P3.

## Contracts (PROCESS P3) and owners

| Contract | Owner |
|---|---|
| Workspace, toolchain, lint config | FP-001 (#2) |
| CI | FP-002 (#3) |
| Coverage gates and repository guards (xtask) | FP-003 (#4) |
| Dependency spike | FP-004 (#5) |
| Binding spike | FP-005 (#6) |
| CR citation index | FP-006 (#7) |
| Public engine API (`IRulesEngine`, SRS 13.2) | FP-077 (#79) Rust facade |
| Bindings | FP-100 (#102) C ABI; FP-101 (#103) .NET; FP-102 (#104) WASM |
| Card DSL schema and `dslVersion` (SRS 14) | FP-010 (#11), FP-011 (#12), FP-012 (#13), FP-013 (#14) |
| State model | FP-017 (#18) |
| Canonical state hash (Merkle framework) | FP-009 (#10) |
| Replay format `*.fpreplay` and header | FP-081 (#83) |
| Card database artifact `*.fpdb` | FP-098 (#100) |
| `RulesVocabulary` artifact (see below) | FP-014 (#15) |
| Override submission format and card support report | FP-084 (#86) |
| Synthetic test-card library | FP-016 (#17) |
| `.pzl` scenario format | FP-049 (#51) |
| Wire protocol (wire schema) | FP-078 (#80) |
| `PendingChoiceRequest` taxonomy (choice protocol) | FP-027 (#28) |
| Native-effect registry IDs (`id@version`) | FP-062 (#64) |
| PRNG algorithm and seed derivation | FP-008 (#9) |

**RulesVocabulary** (owner FP-014, #15; REQ-OBJ-001, REQ-SUB-001, REQ-DSL-014): the rules
vocabulary the CR defines (card types, supertypes, subtypes by card type, keyword abilities and
actions with their ids, counter kinds that carry rules, the CR 111.10 predefined tokens) as data.
Schema and loader: `fineprint_dsl::vocab::RulesVocabulary` (`crates/fineprint-dsl/src/vocab/`,
wire version `vocabularyVersion` 1). Committed file: `crates/fineprint-dsl/data/vocab.json`
(RFC 8785 canonical JSON plus one trailing newline; names, ids, rule references and token
characteristics only, no rules prose), embedded by `RulesVocabulary::bundled()`. Generator:
`fineprint-ingest vocab` (`crates/fineprint-ingest/src/vocab/`). Regenerate after every CR
baseline change; the `cr` workflow fails when the file and the regeneration differ.
