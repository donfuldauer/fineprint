# Decisions

Every ambiguity an agent resolved, with rationale (PROCESS.md R8), and the architecture decisions
the plan is built on (`plan-from-srs` §1).
Format: `## <work item | Architecture> — <short title>` then **Decision**, **Rationale**,
**Alternatives**.

## Repository — Process adapted from Kilohertz Pinball

**Decision:** The agent roster (work-classifier, task-executor, work-verifier, issue-investigator),
the triage fast path, model tiering with the Fable allowance rule, and the CodeRabbit PR workflow
follow the Kilohertz Pinball repository's v5 process. Fine Print adds a `rules-researcher` (no
rules text from memory) and a `fixture-author` (synthetic test cards for the regression suite),
and replaces the simulation-thread rules with determinism, card-data and card-branch rules (R3–R5).
**Rationale:** The process ran a large unattended build there; Fine Print's risks are rules
fidelity, determinism and card coverage rather than real-time physics.
**Alternatives:** A fresh process (no track record); a single generalist agent (no tiering, no
independent testing).

## Architecture — decisions recorded in the SRS

Made by the maintainer on 2026-11-14 through `plan-from-srs` §1 and written into `docs/SRS.md`
§17. Each section: **Decision** (with the requirement IDs), **Rationale**, **Alternatives**.

## Architecture — A1 Core language

**Decision:** Rust, pinned toolchain, one workspace with the toolchain; unsafe code only in the FFI
crate (REQ-NFR-001).
**Rationale:** Matches the SRS as written; integer-only determinism is easy to enforce; first-class
C ABI and WASM targets.
**Alternatives:** C# / .NET, C++, TypeScript.

## Architecture — A2 Bindings

**Decision:** C ABI (cbindgen `fineprint.h`), .NET 10 LTS (`FinePrint.Core` over P/Invoke) and
WASM (wasm-bindgen, npm with TypeScript types), all core for milestone 1 (REQ-NFR-002,
REQ-API-001, REQ-CORE-006).
**Rationale:** Covers native embedders, Unity/Godot and .NET servers, and browser frontends.
.NET 10 is the current LTS; wasm-bindgen gives typed JS/TS access in browsers and Node.
**Alternatives:** shipping bindings later; .NET Standard 2.1 for older Unity; .NET 8 (support ends
Nov 2026); the WASM component model; raw C-ABI exports to JS.

## Architecture — A3 Platforms

**Decision:** Linux x86_64, Windows x86_64 (MSVC), macOS arm64, and WASM in headless Chromium,
Firefox and WebKit; golden replays must hash identically everywhere (REQ-NFR-003).
**Rationale:** The desktop, server and browser hosts the SRS names.
**Alternatives:** a smaller matrix, adding platforms later.

## Architecture — A4 Card definitions and overrides

**Decision:** Fine Print maintains no card definitions or overrides. Clients run
`fineprint-ingest` on their own card data, get a support report of cards needing overrides
(REQ-ING-016), and supply overrides, which the engine validates and smoke-tests
(REQ-ING-007). `VERIFIED` is earned only by client-supplied behavioral tests that pass
(REQ-ING-008). Fine Print's tests use synthetic cards (REQ-TEST-001). LLM-assisted
drafting is withdrawn (REQ-ING-011). No card data anywhere in the repo (REQ-NFR-004).
**Rationale:** The maintainer does not want to maintain a card corpus. It also keeps the public
repository and releases free of card IP, which leaves the licensing question with whoever
distributes card data.
**Alternatives:** a project-maintained override repository (separate or in this repo);
publishing parser-built databases; clients declaring `VERIFIED`; dropping `VERIFIED`; testing
against real cards parsed from cached data.

## Architecture — A5 Toolchain

**Decision:** `fineprint-ingest` in Rust with a pest (PEG) grammar, sharing the DSL types crate
(REQ-NFR-005).
**Rationale:** One language; the toolchain emits exactly the types the engine loads.
**Alternatives:** Rust with tree-sitter (GLR), Python with Lark.

## Architecture — A6 Serialization and hashing

**Decision:** RFC 8785 canonical JSON with SHA-256 for content hashes; MessagePack for transport
only; a Merkle canonical state hash, SHA-256 unless the Phase 0 spike shows it is too slow
(REQ-NFR-006).
**Rationale:** Merkle hashing lets the investigator find the first divergent component between two
states, which matters for cross-binding determinism bugs.
**Alternatives:** Zobrist hashing.

## Architecture — A7 Comprehensive Rules text

**Decision:** Fetched in CI and cached, never committed; the repo commits the citation index with
per-rule text hashes (REQ-NFR-007).
**Rationale:** The repository is public and the CR text belongs to Wizards of the Coast.
**Alternatives:** vendoring the CR text in the repository.

## Architecture — A8 Milestone 1 scope

**Decision:** Two-player constructed play; multiplayer, Commander, community formats, the digital
extension, subgames, casual variants and ante deferred, with the model built for N players
(REQ-NFR-010).
**Rationale:** The smallest scope that exercises every generic rule, layout, the DSL, bindings and
toolchain.
**Alternatives:** 1v1 plus Commander; the full SRS at once.

## Architecture — A9 Coverage and performance

**Decision:** 90% line coverage for the rules core, 75% elsewhere (REQ-NFR-008); the performance
reference is GitHub's hosted `ubuntu-latest` runner, tracked as a trend with a 10% phase-gate
regression limit (REQ-NFR-009).
**Rationale:** Rules correctness is the product; CI-reproducible benchmarks.
**Alternatives:** 85/60 or 95/80 floors; a local reference machine; no reference yet.

## Architecture — A10 Card data during development

**Decision:** Scryfall bulk files at most daily into git-ignored local and CI caches, only for
parser coverage and conformance; only aggregate numbers are published (REQ-NFR-004, REQ-ING-013).
**Rationale:** Grammar work needs real text to measure against; the public repo must not carry it.
**Alternatives:** synthetic data only.

## Architecture — A11 Repository, review and CI

**Decision:** Public repository `donfuldauer/fineprint`, protected `main`, required CI checks,
CodeRabbit review under `docs/PROCESS.md` (REQ-NFR-011).
**Rationale:** The unattended PR workflow depends on an external reviewer and required checks.
**Alternatives:** direct pushes until CodeRabbit is installed.

## Architecture — A12 Code license

**Decision:** MPL-2.0, with the Exhibit A header in every source file (REQ-NFR-012). Supersedes an
initial choice of GPL-3.0-or-later.
**Rationale:** The maintainer wants distributed modifications to stay open source with attribution,
while allowing closed-source clients (for example game clients) to embed the engine. MPL-2.0's
file-level copyleft does that with no relinking requirement, which suits Rust's static linking and
the bundled WASM package; the LGPL's relinking obligation would burden both.
**Alternatives:** GPL-3.0-or-later (every embedding app must be GPL-compatible); LGPL-3.0
(relinking requirement); MIT OR Apache-2.0 (no copyleft); AGPL-3.0 (also covers network use).

## Repository — SRS is not versioned

**Decision:** `docs/SRS.md` carries no version number, revision summary or [NEW]/[REV] markers;
git history is its change record. Requirement IDs are never reused; removed requirements stay as
one-line [WITHDRAWN] entries. Appendix A keeps the old errata as a checklist of common rules
misreadings.
**Rationale:** Maintainer's decision; the repository already version-controls the document.
**Alternatives:** semantic versioning of the SRS inside the document.

## Repository — The plan is GitHub issues

**Decision:** The implementation plan is the set of GitHub issues labelled `work-unit`, one per
reviewable PR, with "blocked by" dependency links, `ready`/`blocked` labels and milestones M1–M3.
Nothing in the repository holds the work breakdown; `docs/DEVELOPMENT.md` (created by the
workspace issue) holds only the layout, commands, gates and contracts. "Start work on issue X"
runs the `work-issue` skill.
**Rationale:** Maintainer's decision: work units belong in the issue tracker, where each one maps
to one PR and can be picked up by any session.
**Alternatives:** a generated `docs/PLAN.md` mirroring the issues (rejected: duplicate source of
truth in the repository).

## Planning — One demonstration client per binding

**Decision:** The "test client for each targeted platform" is four clients, one per binding, each
run by CI on every platform that binding targets: a Rust terminal client and a C demo and a .NET
demo (Linux, Windows, macOS), and a web client (Chromium, Firefox, WebKit). They are work units
only; the SRS is unchanged.
**Rationale:** A client per binding exercises every shipped API on every platform in REQ-NFR-003
without building three near-identical desktop apps.
**Alternatives:** one native GUI client per operating system.

## FP-001 — Toolchain pin and workspace conventions

**Decision:** The toolchain is pinned to exactly `1.97.0` (`rust-toolchain.toml`, profile
`minimal`, components rustfmt and clippy, no targets yet); changing it is a recorded decision
(REQ-NFR-001). Edition 2024, resolver 3, `rust-version = "1.97"`. `publish = false` on every
crate until a release issue and a human decision (PROCESS P8). `license = "MPL-2.0"` is written
literally in every crate. One workspace-wide `clippy.toml` bans `f32`, `f64`, `HashMap` and
`HashSet` via `disallowed-types`; clippy 1.97 matches the float primitives (proved with a
temporary violation in `fineprint-core`). The non-rules crates (ingest, ffi, wasm, bot, cli,
xtask) carry `#![allow(clippy::disallowed_types)]`; the ffi crate is the only one without
`forbid(unsafe_code)`. `fineprint-ingest` depends on base and dsl only, not core, because the
toolchain shares the DSL types (REQ-NFR-005). `fineprint-cli` lives at `clients/cli` as a
library stub. Issue #2's owned-path list omitted `clients/cli` and `docs/DECISIONS.md`; both
were authorised in the brief.
**Rationale:** An exact pin makes CI and developer builds identical. A single clippy.toml is
the only config clippy reads per workspace; the crate-level allow keeps the rule scoped to the
rules crates without extra files.
**Alternatives:** floating `stable` (rejected: non-reproducible); per-crate clippy.toml files
(rejected: duplication, drift); `[workspace.lints]` in place of attributes (rejected: the
non-rules allow and the unsafe rule are clearer next to the code).

## FP-002 (#3) — Red-run evidence on the PR branch

Decision: The "deliberately failing test turns the run red" criterion was shown by pushing a scratch failing test (`crates/fineprint-base/tests/ci_red_check.rs`, commit 71c7a75) to the PR branch and reverting it in the next commit. The three `test` jobs concluded `failure` on 71c7a75; `fmt` and `clippy` stayed green.
Rationale: The session may push only to its assigned branch, so a separate scratch branch was not allowed. The branch tree after the revert equals the verified tree apart from this entry.
Alternatives: A separate `scratch/` branch (not permitted here).

## FP-003 (#4) — Coverage floors per crate and guard design

**Decision:** (i) Crate-to-floor table: `fineprint-base`, `fineprint-dsl`, `fineprint-core` at 90%; `fineprint-ingest`, `fineprint-ffi`, `fineprint-wasm`, `fineprint-bot`, `fineprint-cli` (`clients/cli`) and `xtask` at 75%. Percent is summed covered over summed measured lines per crate; a crate with zero measured lines or absent from the JSON is `n/a` and passes; a workspace crate missing from the table fails. (ii) xtask uses no third-party crates: a hand-written JSON reader (depth cap 64, never panics) parses `cargo llvm-cov` output and files come from `git ls-files`. (iii) The header guard normalises the first 20 lines (after a shebang) by stripping comment markers and collapsing whitespace, then requires the MPL-2.0 sentence by containment, so wrapped and block-comment headers pass. (iv) card-data heuristics: bulk-data names, `.fpdb`/`.cr.txt`, `.json` with an Oracle text key, and five or more lines in a file (not necessarily consecutive, since real rules text separates rules with blank lines) that start with a rules-number shape followed by at least three words of prose; content is scanned only for UTF-8 files under 5 MiB. card-branch is a lexical line scan, not a parser (limits listed in `xtask/src/guard.rs`). (v) `cargo-llvm-cov` is pinned to 0.9.1 (newest on crates.io when checked, minimum Rust 1.87, below the pinned 1.97.0) and `llvm-tools` is installed by the CI coverage job, not added to `rust-toolchain.toml`.
**Rationale:** The SRS gives two floors (REQ-NFR-008) but no crate list; the rules crates (base, dsl, core) are the "rules core". Integer arithmetic keeps the gate exact and deterministic. Std-only code keeps the supply chain empty (the dependency spike is #5). Keeping `llvm-tools` out of the toolchain file keeps developer installs small and does not change the pinned toolchain contract owned by #2.
**Alternatives:** Averaging per-file percentages (rejected: skews small files); `cargo llvm-cov --fail-under-lines` (rejected: workspace-wide, no per-crate floors); `serde_json` (rejected: no third-party crates in xtask); consecutive-line CR detection (rejected: misses real rules text); `llvm-tools` in `rust-toolchain.toml` components (rejected: changes the toolchain contract for every developer).

## FP-003 (#4) — Review-round fixes to guard behaviour

**Decision:** (a) The `@generated` header exemption is removed; generated files need the MPL-2.0 header. (b) `.cpp` files are header-checked. (c) Card-data file-name checks (`*.fpdb`, `*.cr.txt`, bulk-data names) apply under `docs/` and `.github/ISSUE_TEMPLATE`; only the content checks are exempt there. (d) A file of 5 MiB or more outside the exempt directories is an explicit "scan incomplete" violation, superseding the earlier "scanned only under 5 MiB" clause. (e) `crate_of` classifies by the innermost workspace component of the path. (f) The card-branch block-comment blanking tracks nesting depth.
**Rationale:** A silent skip (generated marker, oversized file, exempt directory for artifact names) lets forbidden or unlicensed content through unnoticed; failing loudly keeps the guards honest. An outer `crates/` directory in a developer checkout path must not change crate attribution, and Rust block comments nest, so the lexer must too.
**Alternatives:** Incremental (streaming) scan of large files (rejected: more code, no benefit at current sizes; the largest tracked file is about 165 KB); passing the workspace root to `crate_of` (rejected: changes its public signature).

## FP-003 (#4) — crate_of xtask arm restricted to workspace-root package

**Decision:** `crate_of` maps a path to `xtask` only when the component after `xtask` is a package-layout entry (`src`, `tests`, `benches`, `examples`, `build.rs`) and the component before it is not `src`, `tests`, `benches` or `examples`. The `clients/cli` arm gets the same "not inside another crate's source subdirectory" check. Innermost-wins is kept.
**Rationale:** A module directory named `xtask` inside a rules-core crate (for example `crates/fineprint-core/src/xtask/mod.rs`) was attributed to xtask and gated at 75% instead of 90%.
**Alternatives:** Passing the workspace root to `crate_of` (rejected: changes its public signature); outermost-wins (rejected: breaks checkouts under an outer `crates/` directory).

## FP-003 (#4) — Coverage paths anchored to the workspace root

**Decision:** Coverage file paths are mapped to crates by `crate_of_rooted(root, path)`, which supersedes the earlier root-free `crate_of` entries (innermost match, and the `xtask` / `clients/cli` previous-component check). Both arguments are normalised (`\` to `/`, trailing `/` of the root dropped); the path must lie below the root; only the first components of the relative path are examined: `crates/<dir>/<more>` is `<dir>`, `clients/cli/<more>` is `fineprint-cli`, `xtask/<more>` is `xtask`, anything else is ignored. `parse_summary(text, root)` takes the root. `cargo xtask coverage` finds it with `git rev-parse --show-toplevel`, falls back to the current directory, and exits non-zero with a message if both fail; `--root <dir>` overrides it.
**Rationale:** A root-free heuristic cannot tell `<root>/xtask/src/a.rs` from `<root>/crates/fineprint-core/src/nested/xtask/src/a.rs`, so a nested directory could pick the wrong coverage floor (75% instead of 90%). Anchoring to the root removes the ambiguity: nested `xtask`, `crates` or `clients` directories cannot claim anything, and registry, rustup and rustc sources fall outside the root. CI runs from the repository checkout, so it needs no change.
**Alternatives:** More path heuristics (rejected: still wrong for some layouts); reading `Cargo.toml` metadata for package manifests (rejected: needs a TOML parser or `cargo metadata`, and the layout is fixed by the SRS); root from `CARGO_MANIFEST_DIR` (rejected: compile-time path, wrong for a relocated binary).

## FP-003 (#4) — Coverage fails when no file lies under the workspace root

**Decision:** `cargo xtask coverage` exits 1 with `coverage: none of the N files in the coverage report lies under the workspace root <root>; pass --root <dir> if the checkout path differs` when `data[0].files` is non-empty but no file path lies under the root. An empty files list, or files that are under the root but map to no crate, keep the previous behaviour (n/a, PASS). The check is in the run path; `crate_of_rooted` and the new `under_root` share one prefix helper.
**Rationale:** With a symlinked checkout or a wrong `--root`, every file fell outside the root, every crate showed n/a, and the gate passed vacuously (REQ-NFR-008).
**Alternatives:** Making `parse_summary` itself return an error (rejected: it is the pure parser and existing callers rely on an empty result for foreign roots); failing on an empty files list too (rejected: nothing to attribute is not a root mismatch); canonicalising the root with symlink resolution (rejected: needs filesystem access in the mapping and does not fix a wrong `--root`).
## FP-004 — PRNG: in-house PCG64-DXSM (no crate)

**Decision:** The PCG64-DXSM generator (REQ-CORE-001) is a ~40-line pure-integer in-house
implementation (`spikes/libs/src/prng.rs`, to be moved into a rules crate by its owner): state
is `(state: u128, increment: u128)`; the LCG steps with the 64-bit "cheap multiplier"
`0xda942042e4dd58b5`, and the output is the DXSM permutation of the pre-step state (the NumPy
`PCG64DXSM` / pcg-cpp `cm_setseq_dxsm_128_64` definition). Seeding `new(initstate, initseq)`
follows NumPy (`pcg_setseq_128_srandom_r`), which steps with the standard 128-bit PCG
multiplier `0x2360ed051fc65da44385df649fccf645` while seeding only. No crate is pinned for the
PRNG. The brief described the LCG as using "the standard PCG 128-bit default multiplier"; the
published vectors show that multiplier is used only during NumPy's seeding, and the stepping
uses the cheap multiplier. The seed derivation itself belongs to REQ-API-004
(decided in "FP-004 — Resolution of the queued items").
**Rationale:** Published vectors: NumPy `numpy/random/tests/data/pcg64dxsm-testset-1.csv` (seed
0xdeadbeaf) and `-2.csv` (seed 0), rows 0–15 and row 999 of each, hard-coded in
`tests/prng.rs`; 2 of 2 seed/stream pairs and 34 of 34 outputs match, plus the seeded raw state
(NumPy 2.5.3 `SeedSequence(seed).generate_state(4, uint64)` gives the 128-bit
`initstate`/`initseq`; that derivation was computed locally, not published). Throughput
(`cargo run --release -p fineprint-spike-libs --example bench`, median of 51 samples of 10^6
calls): 1.58–2.15 ns per u64 across two runs, equal to `rand_pcg` (1.60–2.12 ns). Zero
dependencies, so no `getrandom`/OS entropy can be reached and wasm32 has nothing to build
beyond core integer arithmetic. Licence: MPL-2.0 (own code). The output is frozen with the
engine major version because no third-party update can change it.
**Alternatives:** `rand_pcg =0.10.2` (`Pcg64Dxsm` = `Lcg128CmDxsm64`, MIT OR Apache-2.0, deps:
`rand_core 0.10.1` only, no `getrandom` in 0.10): output matches the published rows when built
with `from_state` from NumPy's seeded state, but its `new` seeds with the cheap multiplier
(pcg-cpp convention), so `new(initstate, initseq)` gives a different state than NumPy (asserted
in `req_core_001_rand_pcg_candidate_matches_published_vectors`). Rejected: an extra dependency
for 40 lines, and a seeding convention that differs from the published vectors. `pcg_rand`
and `pcg` crates were not evaluated (rand_pcg already covers the crate option).

## FP-004 — RFC 8785 canonical JSON: serde_json_canonicalizer

**Decision:** `serde_json_canonicalizer = "=0.4.1"` (no features exist) over `serde_json =
"=1.0.154"` with `default-features = false, features = ["std", "float_roundtrip"]`
(`float_roundtrip` makes decimal parsing correctly rounded, so the canonical number is computed
from the exact double; it is enabled on the documented behaviour of the feature, not on a
measured failure). `serde = "=1.0.229"` (`std`, `derive`). Wrapper: `canonicalize(bytes)`
parses into `serde_json::Value` and serialises with JCS. The wrapper rejects duplicate property
names, compared after string decoding, in every nested object via a serde visitor pre-pass
(RFC 8785 requires I-JSON, where serde_json's `Value` would silently keep the last duplicate).
**Rationale:** Vectors (hard-coded in `spikes/libs/tests/jcs.rs`), all passed by both
candidates: 23 of 23 number-serialisation rows of the JCS specification table (copied from the
RFC author's source `cyberphone/ietf-json-canon` draft XML; every row re-checked with Node.js 22
`JSON.stringify`) plus NaN and Infinity rejected; 32 of 32 extra random doubles formatted by
Node.js (not published); 6 of 6 reference files from `cyberphone/json-canonicalization/testdata`
(arrays, french, structures, unicode, values, weird), where `weird` proves UTF-16 code-unit key
order (U+1F602 sorts before U+FB33, the reverse of UTF-8/code-point order, also asserted
directly); string escaping (`\u0000`, `\u001f`, `\b\t\n\f\r`, `\"`, `\\`, `/` unescaped, DEL
and non-ASCII raw); lone surrogate rejected. Large numbers: both candidates serialise integers
beyond 2^53 as the nearest double, silently (9007199254740993 -> `9007199254740992`, u64::MAX
-> `18446744073709552000`, i64::MIN -> `-9223372036854776000`); `1e400` is rejected by the
parser. This is the RFC's ECMAScript behaviour, so definition JSON that is content-hashed must
keep integers within ±(2^53−1) or carry larger values as strings. Speed on a 23 114-byte
document (200 objects): 0.64 ms (both candidates within 3 %). Licence: MIT; full tree below.
Bounded random input (4000 inputs) never panics and canonicalisation is idempotent.
**Alternatives:** `serde_jcs =0.2.0` (MIT OR Apache-2.0, depends on the older `ryu-js 0.2`):
same vector results and speed (0.62–0.66 ms), rejects `i128` ("i128 is not supported") where
`serde_json_canonicalizer` formats it as a double; rejected for the smaller API surface, older
number formatter and lower adoption (crates.io: 6.6 M vs 18.3 M downloads). An in-house
canonicalizer is not needed: no vector failed.

## FP-004 — MessagePack transport: rmp-serde, named struct fields

**Decision:** `rmp-serde = "=1.3.1"` (no features exist). Structs are encoded as maps keyed by
field name (`rmp_serde::to_vec_named`); enums use serde's externally tagged form, which
rmp-serde writes as a one-entry map `{"Variant": payload}` and a bare string for unit variants.
MessagePack is transport only; nothing feeds its bytes into a hash (REQ-NFR-006).
**Rationale:** Round trip of a three-level nested enum (unit, newtype, tuple and struct
variants; `Vec`, `Option`, `BTreeMap` payloads; i64::MIN/MAX, u64::MAX; non-ASCII strings)
passes, and three hard-coded byte vectors (`"Plain"` = `a5 50 6c 61 69 6e`, an extremes tuple
of 31 bytes, a nested struct of 52 bytes) pin the exact encoding (`tests/transport.rs`). 4000
random and mutated inputs decode without panicking. Sizes for 200 synthetic snapshots: named
16 325 bytes, compact (struct as array) 6 397 bytes, JSON 23 114 bytes; named encode 24–25 µs,
decode 42–43 µs. Named fields were chosen over the 2.5x smaller compact form because the web and
.NET clients decode by field name, and adding a field does not shift positions; the size is
still 29 % below JSON. Licence: MIT (`rmp 0.8.15` MIT, `num-traits` MIT OR Apache-2.0).
**Alternatives:** compact arrays (`rmp_serde::to_vec`, 6 397 bytes): rejected for client
schema fragility, can be revisited by the API contract owner. Other MessagePack crates were not
evaluated: rmp-serde passed every check and is the de facto serde implementation (153 M
downloads).

## FP-004 — State hash: SHA-256 Merkle (sha2), budget met

**Decision:** The canonical state hash digest is SHA-256 from `sha2 = "=0.11.1"` with
`default-features = false` (the default `alloc` and `oid` features are not needed; the backend
is chosen by runtime CPU-feature detection, SHA-NI on x86_64 and SHA2 on aarch64, otherwise the
portable software code). The REQ-NFR-006 escape clause is not used. Prototype tree
(`spikes/libs/src/merkle.rs`): leaf = `SHA-256(0x00 || tag || fields)`, parent =
`SHA-256(0x01 || child digests)`; integers little-endian fixed width, strings and lists
prefixed with a `u32` LE length; root children in fixed order players, battlefield, stack,
pending triggers, turn position, PRNG; battlefield = node(membership leaf, binary tree over
permanents in ObjectId order).
**Rationale:** Measured with `cargo run --release -p fineprint-spike-libs --example bench`,
median of 51 samples, on Linux 6.18 x86_64, Intel Xeon @ 2.10 GHz (family 6 model 207, KVM,
4 vCPU; not the reference ubuntu-latest runner), synthetic 4-player state with 200 permanents
(16 345 payload bytes):

| Figure | SHA-256 (auto: SHA-NI) | SHA-256 (soft backend) | BLAKE3 1.8.7 (`pure`) |
|---|---|---|---|
| Full recompute | 58–68 µs | 185 µs | 88–98 µs |
| Incremental, 1 permanent | 2.4–3.3 µs | 5.5 µs | 3.3–4.4 µs |
| Incremental, 10 permanents | 14.1–14.3 µs | 41 µs | 20 µs |
| Payload throughput, full | 240–280 MB/s | 88 MB/s | 167–185 MB/s |
| Raw 1 MiB buffer | 1 492–1 614 MB/s | 330 MB/s | 3 001–3 261 MB/s |

Budget (REQ-LAYER-005, ≤ 2 ms full board, 200 permanents): on the local 4 vCPU Xeon KVM VM
above only, full recompute PASS at 3–9 % of the budget even with the software backend;
incremental PASS. The reference ubuntu-latest result is outstanding until a CI benchmark run
records it (no CI job runs the benchmark yet). The soft figures came from
`RUSTFLAGS='--cfg sha2_backend="soft"'`. Correctness tests (`tests/merkle.rs`): 10 repeated
full hashes equal; incremental equals full after 1 and 10 permanent changes and after 600
PRNG-driven random mutations (permanent edits, players, stack, triggers, turn, PRNG, permanents
added and removed, battlefields of 200, 0, 1 and 7) comparing every cached digest; the
first-differing-component walk finds each kind of change. Golden root of
`synthetic_state(0x5eed0004, 200)`: SHA-256
`738f06e1f327b8745855fdd2837722d572e0488a76052b1a4864314b567c33ba` (reproduced by an
independent Python `hashlib` implementation of the encoding), BLAKE3
`85bf5e48d505692983eb9715328151028010bb9c8048ad8b0dc31ddbcf827e17`. Licence: sha2 and its 8
dependencies are MIT OR Apache-2.0 (libc, via cpufeatures, is used on some targets only).
**Alternatives:** BLAKE3 (`blake3 =1.8.7`, `pure` feature so no C or assembly; CC0-1.0 OR
Apache-2.0): faster on large buffers but 1.4x slower on this tree of small leaves; not needed
because SHA-256 meets the budget. Kept as a spike-only dependency for the comparison and not
pinned. Zobrist hashing was already rejected in A6.

## FP-004 — pest grammar crates

**Decision:** `pest = "=2.9.6"` and `pest_derive = "=2.9.6"`, both `default-features = false`.
With pest's `std` feature off, the target-side pest depends only on `ucd-trie`; the crate root
needs `extern crate alloc;` because pest_derive then emits `alloc::` paths. pest is for
`fineprint-ingest` (offline toolchain) only and must not enter the rules crates (REQ-NFR-005).
**Rationale:** A synthetic grammar file (`spikes/libs/src/sentence.pest`, invented sentences
about marbles and bowls) accepts 3 of 3 valid inputs, rejects 7 of 7 invalid ones, and 4000
random strings never panic (`tests/grammar.rs`). Compile cost: clean `cargo build --release -p
pest -p pest_derive` 5.6 s, whole spike crate 10.0 s (same machine as the hash entry). Pure
Rust on the target side. On the build host, pest_derive's own pest copy keeps `std`, which
pulls `pest_stack_guard` -> `stacker`/`psm` (psm assembles a small file with `cc`); this runs at
compile time only and does not reach the target. Licence: MIT OR Apache-2.0 for pest,
pest_derive, pest_generator, pest_meta, ucd-trie, pest_stack_guard, stacker, psm.
**Alternatives:** target-side `std` feature (adds the stack guard; rejected for now because it
adds the psm assembly build step to every target including wasm32; the ingest owner may enable
it for native-only builds). tree-sitter and Lark were rejected earlier (A5).

## FP-004 — Licence check of the pinned crates

**Decision:** All pinned crates and their transitive dependencies are accepted.
**Rationale:** `cargo metadata --format-version 1 --locked`, walking the dependency graph from
the seven pinned crates: 35 packages over normal dependencies (including proc-macro hosts), 41
including build dependencies. Licences of the 41: MIT OR Apache-2.0 (31), MIT (4: rmp,
rmp-serde, serde_json_canonicalizer, zmij), Unlicense OR MIT (1: memchr), Apache-2.0 OR MIT
(2: autocfg, object), and three to note: `ryu-js 1.0.3` Apache-2.0 OR BSL-1.0 (Apache-2.0 elected);
`ar_archive_writer 0.5.5` Apache-2.0 WITH LLVM-exception (build dependency of psm, compile time
only); `unicode-ident 1.0.27` (MIT OR Apache-2.0) AND Unicode-3.0 (a permissive data licence;
used only inside the `proc-macro2`/`syn` proc-macros at compile time, nothing is linked into
the output). No GPL, AGPL, LGPL or unknown licence. Comparison-only crates: blake3 (CC0-1.0
OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception), constant_time_eq (CC0-1.0 OR MIT-0 OR
Apache-2.0), arrayvec, rand_pcg, rand_core, serde_jcs (MIT OR Apache-2.0), ryu-js 0.2.2
(Apache-2.0 OR BSL-1.0).
**Alternatives:** `cargo-deny` (not added: the brief forbids it here; the guards are #4's).

## FP-004 — spikes/ directory handling by repo guards

**Decision:** `spikes/libs/**` is a non-rules, throwaway area and a workspace member
(`fineprint-spike-libs`, `publish = false`). Nothing depends on it. The guards that #4 (FP-003)
adds should (1) apply the MPL header, `forbid(unsafe_code)` and card-data guards to it, (2)
treat it as non-rules for the no-float/no-HashMap rule (the spike's PRNG and Merkle code still
avoid both; f64 appears only in `tests/jcs.rs` under the documented crate-level allow), and (3)
apply the REQ-NFR-008 75 % non-rules line-coverage floor to all of `spikes/libs/**` (`src`,
`examples` and `tests`, as the coverage tool measures them) with no exemption; any exemption
(for example of `examples/bench.rs`) needs an approved SRS change first. Alternatively #4 may
delete `spikes/libs`, but only after the production crates carry replacement golden-value tests
(equivalent PRNG, canonical-JSON, MessagePack and Merkle-hash vectors) that run in CI on Linux,
Windows and macOS; adopting the pins alone is not enough.
**Rationale:** The spike is evidence, not product, but REQ-NFR-008 has no benchmark
exemption. The practical consequence for #4: if the benchmark printer
(`examples/bench.rs`) cannot reach the floor, cover it with a smoke test that runs it with tiny
iteration counts, or delete the spike once production crates carry replacement golden-value tests
that run in CI on Linux, Windows and macOS (adopting the pins alone is not enough). Keeping it in
the workspace makes CI run its golden-value tests on all three platforms, which is the
cross-platform acceptance criterion of #5.
**Alternatives:** a separate workspace under `spikes/` (rejected: CI would not run it without
editing `.github/`, which #5 does not own); deleting the spike in #5 (rejected: the tests are
the cross-platform evidence until production crates carry them).

## FP-004 — Resolution of the queued items

**Decision:** The open `[#5 FP-004]` and `[#4 FP-003]` items in HUMAN_QUEUE.md are resolved as
follows. (1) PRNG seeding: the initial generator is built from the u64 match seed by expanding it
with SplitMix64. Run SplitMix64 from state = seed and take four successive outputs o0..o3;
initstate = (o0 << 64) | o1 and initseq = (o2 << 64) | o3 (big-endian word order, as written).
Seeding then follows the NumPy `srandom` convention exactly as `Pcg64Dxsm::new` implements it in
`spikes/libs/src/prng.rs`: the standard 128-bit multiplier during seeding only, the cheap
multiplier for stepping, and the top bit of initseq discarded. Fork reseeding is defined
bit-exactly as follows. Let the parent generator's raw state be (state: u128, increment: u128), as
returned by `Pcg64Dxsm::raw()`, and forkSalt a u64. Reduce to a 64-bit seed by XOR-folding:
`seed64 = hi64(state) ^ lo64(state) ^ hi64(increment) ^ lo64(increment) ^ forkSalt`, where hi64 is
the top 64 bits and lo64 is the bottom 64 bits. The increment is included so that two generators
with equal state but different streams fork differently. Then apply the same expansion as the
initial seed: SplitMix64 from state = seed64, four successive outputs o0..o3, initstate =
(o0 << 64) | o1, initseq = (o2 << 64) | o3, then NumPy `srandom` seeding via `Pcg64Dxsm::new`. One
helper `from_u64_seed(seed64)` therefore serves both createMatch seeding and fork reseeding. This
is the literal REQ-API-004 formula with its types made explicit (parentPrngState is the
generator's whole raw state, folded), and repeated forks with the same salt stay reproducible.
Issue #9 (FP-008) implements this and must add a test with fixed expected values for both the
initial seed and one fork. (2) JCS number
table: the vectors are accepted as they stand and the item is closed. (3) unicode-ident: the
Unicode-3.0 conjunct is accepted, and the accepted-licence policy is recorded. This is a licensing
decision under PROCESS.md P8 and it is approved: the repository owner explicitly approved accepting
the Unicode-3.0 conjunct and this accepted-licence list when asked in this session (2026-11-15).
The owner can reopen it at any time. (4) spikes/libs
guard handling: already implemented by #4 (FP-003); nothing further.
**Rationale:** (1) SRS `createMatch(config, seed: uint64)` gives a 64-bit match seed, and
REQ-CORE-001 says the seed derivation is specified in REQ-API-004. REQ-API-004 (§13.4) gives only
the fork formula, not the types or the expansion, so this entry defines the initial expansion and
makes the fork typing explicit.
SplitMix64 is already mandated for forks, so one expansion primitive serves the whole engine; it is
a few lines of integer arithmetic with no floats and no tables. The NumPy convention is the one
anchored by published vectors: `tests/prng.rs` checks the seeded state against NumPy's published
CSV rows, generated from NumPy's own initstate/initseq. The pcg-cpp/`rand_pcg` convention differs
only in seeding and has no published vectors to test against. The choice is frozen per engine
major version (REQ-CORE-001), so either convention would be deterministic; this one is testable.
Scope note: REQ-API-004 gives only the formula, so this entry makes the typing explicit (state
folded, same expansion as the initial seed). The SRS text is not edited here, because there is no
owner-approved SRS edit. The SRS could spell out the folding later as an optional clarification.
The decision is recorded as a resolved HUMAN_QUEUE.md line (PROCESS.md R8).
(2) The final RFC 8785 text could not be fetched. rfc-editor.org, datatracker.ietf.org,
www.ietf.org and tools.ietf.org are outside this session's allowed network hosts: DNS or proxy
refusal on every attempt, including via the web fetch tool. The basis for accepting the vectors:
the rule the table illustrates is defined by reference to ECMAScript Number-to-String
serialization, and Node.js 22 implements that rule. All 23 of 23 Appendix B rows, plus 32
additional random doubles, were re-derived with Node.js 22 and matched by both candidate crates.
The RFC author's six reference files (including the UTF-16 key-order case) also pass. Residual
risk: the table rows came from the author's draft source, not the final published text, so a
transcription difference in the final text is theoretically possible. It would surface as a test
correction, not a design change, and affects only a spike vector. No person action is required,
and the network settings are not to be changed for this.
(3) `unicode-ident 1.0.27` is `(MIT OR Apache-2.0) AND Unicode-3.0`. It is used only by
compile-time proc-macros (proc-macro2/syn behind pest_derive and serde_derive), and nothing is
linked into any output. Unicode-3.0 is a permissive, OSI-approved licence compatible with MPL-2.0
distribution. The accepted-licence policy used by the FP-004 licence check: MIT, Apache-2.0
(including WITH LLVM-exception), BSD-2-Clause, BSD-3-Clause, ISC, Unlicense, CC0-1.0, MIT-0,
MPL-2.0, and Unicode-3.0; dual licences where at least one option is in this set; BSL-1.0 only as
the non-elected option of an OR. Rejected: GPL, AGPL, LGPL, and any unknown or custom licence.
Condition: if a Unicode-3.0 component is ever linked into a distributed artifact, the packaging
issue must ship its notice text. This is not the case today. This is policy only: no
licence-checking tool exists yet (cargo-deny is not added), and nothing in xtask enforces the list.
(4) Issue #4 (FP-003) is merged and implements the handling. `xtask/src/coverage.rs` maps
`spikes/libs/**` (src, examples and tests) to the crate `fineprint-spike-libs` with a 75 % floor
and no exemption (floor table row; docs/DEVELOPMENT.md "Coverage floors"; "FP-003 (#4) — Coverage
floors per crate and guard design"). `spikes/bindings` is outside the floor table. The
`coverage (ubuntu-latest)`, `guards` and `req-coverage` jobs are green on main commit ce37085,
which contains PR #146. The deletion condition stays as recorded in "FP-004 — spikes/ directory
handling by repo guards" and is not changed here.
**Alternatives:** (1) NumPy `SeedSequence` for the expansion: rejected, a large Python-specific
mixing function that is not needed. The pcg-cpp/`rand_pcg` `new` convention: rejected, it differs
from the published-vector anchor. Deriving initseq as a constant: rejected, it loses independent
streams for forked or reseeded generators. Fork reseed taking only `state` and ignoring
`increment`: rejected, because forks of equal-state, different-stream generators would collide.
Fork reseed mixing via nested SplitMix64 calls instead of XOR: rejected, because it departs from
the literal REQ-API-004 formula. (2) Leaving the item open until the final RFC text can
be fetched: rejected, because this session has no fetch route and no person action is available;
the residual risk is recorded instead. (3) Adding cargo-deny now to enforce the list: not done in
this change, and the list is policy only. Keeping the item open for a later approval: not needed,
because the owner approved it explicitly when asked. (4) Editing the spikes/ guard entry or deleting the
spike now: rejected, because #4 already implements the floor and deletion is gated on production
golden-value tests.

## FP-005 (#6) — Binding toolchain spike: cbindgen, .NET 10 P/Invoke, wasm-bindgen, headless browsers

**Decision:** The binding toolchains are proved on a toy SHA-256 function in an isolated spike
workspace, `spikes/bindings/`, run by a separate workflow, `.github/workflows/spikes.yml`
(`Spikes`). The jobs are `c-abi (<os>)`, `dotnet (<os>)` (ubuntu-latest, windows-latest,
macos-latest), `wasm-node (ubuntu-latest)`, `wasm-browser (chromium|firefox|webkit)` and
`parity`. Every caller reads the single fixed-input file `spikes/bindings/vectors.txt` (empty
input, `abc`, and `seq200` = the 200 bytes 0x00..0xC7, whose digest
`1901da1c…dec6f` was computed with `sha256sum` and confirmed with `openssl dgst -sha256`). Each caller
prints `<name> <hex>\n` per input, and `parity` fails unless all 13 outputs are byte-identical to
the expected digests. `parity` recomputes those digests with `sha256sum` on every run.

- *Isolation.* `spikes/bindings/Cargo.toml` has its own `[workspace]` table and its own
  `Cargo.lock`. It is not one of the root members: root `cargo metadata --no-deps` still lists
  exactly the nine members, and the root fmt, clippy and test gates are unchanged and green. The
  crate inherits the root `rust-toolchain.toml` (1.97.0), `rustfmt.toml` and `clippy.toml`,
  because both tools search upward. `disallowed-types` does not trip, so no allow attribute is
  needed. The spike is outside the root llvm-cov gate (#4). The crate's own 12 unit tests
  exercise every branch of `fp_spike_sha256`: null `out`, `len > isize::MAX`, `len == 0` with
  null or non-null `data`, null `data` with `len > 0`, and success. A bounded 0..=300-length run
  checks the function against sha2's incremental API. Line coverage was NOT measured, so the 75%
  floor (REQ-NFR-008) is not claimed as met: it is an open gate item deferred to #4 (FP-003,
  coverage gates and repository guards), which owns the coverage tooling. `unsafe` is confined
  to the one FFI function, which documents its safety contract.
- *Pinned versions used.* rustc/cargo 1.97.0 (root pin). The wasm32 target is added in CI with
  `rustup target add wasm32-unknown-unknown`; the pin file is unchanged. Rust crates: `sha2 =0.11.1`
  (default features off) and `wasm-bindgen =0.2.130`, a wasm32-only dependency. CLIs:
  `cbindgen 0.29.4` and `wasm-bindgen-cli 0.2.130`, both installed with
  `cargo install --locked --version` and cached by version. The workflow fails unless the CLI
  version equals the `wasm-bindgen` version in `Cargo.lock`. .NET SDK `10.0.401` (runtime
  10.0.12) is pinned in `spikes/bindings/global.json` with `rollForward: disable` and installed by
  `actions/setup-dotnet` from that file. Node `22.23.4`. npm, with `package-lock.json` and `npm ci`:
  `typescript 7.0.2` (the native `tsc`; the lock carries its per-platform optional packages) and
  `@playwright/test 1.65.1`. That Playwright release bundles Chromium 159.0.8154.6 (r1255),
  Firefox 159.0 (r1572) and WebKit 27.2 (r2388). Actions, all first-party and pinned to exact
  tags: actions/checkout v4.2.2, actions/cache v4.2.0, actions/setup-dotnet v4.3.1,
  actions/setup-node v4.4.0, actions/upload-artifact v4.6.2 and actions/download-artifact v4.3.0.
  No third-party actions are used. Runner images: ubuntu-latest, windows-latest and
  macos-latest (arm64). Each job logs the exact image, compiler and SDK versions (`dotnet --info`,
  `cc --version`, `npx playwright --version`). This entry was written before the first CI run,
  so the observed image versions are in that run's logs, not here.
- *RID layout (.NET).* `FinePrint.Spike.Interop` is a class library with the `[LibraryImport]`
  wrapper. Its csproj packs the cdylib as `runtimes/linux-x64/native/libfineprint_spike_bindings.so`,
  `runtimes/win-x64/native/fineprint_spike_bindings.dll` and
  `runtimes/osx-arm64/native/libfineprint_spike_bindings.dylib`, each only if it exists. Each job
  packs its own OS's RID, and the pack fails if none is present. The console app
  `FinePrint.Spike.Console` consumes it as a *package* from a local feed (`dist/nuget`, set in
  `dotnet/nuget.config`), not as a project reference. The native library is therefore resolved
  through `deps.json` runtime-target assets, as a real FinePrint.Core consumer's would be, with
  no PATH or LD_LIBRARY_PATH changes. The job runs the app through `dotnet run` and from
  portable `dotnet publish` output. `dotnet/check-rid-layout.cs`, a .NET 10 file-based app, then
  checks that the `.nupkg` entry, the published file and the `deps.json` native asset are all
  at `runtimes/<rid>/native/<lib>`. As a local negative check, removing the published `runtimes/`
  directory makes the app fail with `DllNotFoundException`.
- *C caller on Windows.* The C caller is built with MSVC `cl /W4 /WX /std:c11`. The workflow
  enters the VS developer environment with `vswhere` → `Microsoft.VisualStudio.DevShell.dll` →
  `Enter-VsDevShell -arch=x64`, links against the import library
  `target/release/fineprint_spike_bindings.dll.lib`, and runs with `target/release` on PATH. On
  Linux and macOS the system `cc` (gcc / Apple clang) is used with `-Wall -Wextra -Wpedantic
  -Werror` and an rpath to `target/release`. No helper crate or `cc` build script is involved.
- *Findings for #102 (C ABI), #103 (.NET) and #104 (WASM).*
  1. Rust cdylibs export only `#[unsafe(no_mangle)] pub extern "C"` items. On Linux
     `nm -D --defined-only` shows exactly one dynamic symbol, `fp_spike_sha256`. No version script
     or `__declspec` is needed. On Windows the MSVC target also emits `<name>.dll.lib`, which is
     the import library that C links against. The DLL depends on the VC runtime
     (`VCRUNTIME140.dll`), which the runners provide.
  2. cbindgen's default includes duplicate `sys_includes`. Use `no_includes = true` with
     explicit `stddef.h` and `stdint.h`. `usize_is_size_t = true` maps `usize` to `size_t`. Rust
     doc comments become `//` comments (`documentation_style = "c99"`), and `pub const`
     integers become `#define`s. The MPL text goes in `header =`. MSVC `/W4 /WX` needs
     `_CRT_SECURE_NO_WARNINGS` for `fopen`, and an explicit cast for the `size_t` → `int`
     argument of `fgets`.
  3. On Windows, C stdout in text mode turns `\n` into CRLF. The C caller sets `_O_BINARY`,
     and the .NET caller writes raw UTF-8 bytes to `Console.OpenStandardOutput()` instead of
     using `Console.WriteLine`. Redirects run in bash, not PowerShell, whose redirection
     re-encodes output.
  4. `[LibraryImport]` needs `AllowUnsafeBlocks`. `fixed` on an empty span yields a null
     pointer, so the C ABI must accept `data == NULL` with `len == 0`. XML comments in csproj
     files cannot contain `--` (MSB4025). File-based apps default to `PublishAot=true`, which
     restores ILCompiler, so the check script sets `#:property PublishAot=false`. The console
     restores into `dist/nuget-packages` (`RestorePackagesPath`), which keeps the global cache
     out of play but does not by itself refresh an already-extracted `0.0.0-spike` after a
     repack. So the csproj has a target before `Restore` that removes
     `dist/nuget-packages/fineprint.spike.interop` (lowercase package id), forcing every restore
     to re-extract the package currently in `dist/nuget`. The local feed
     directory must exist before any restore that reads `nuget.config`.
  5. wasm-bindgen: the CLI version must equal the crate version exactly, so pin both and check
     them against each other in CI. `--target nodejs` emits CommonJS, which the ESM caller loads
     through `createRequire`. `--target web` needs `init({ module_or_path })`, and the server must
     send `application/wasm`. The `#[no_mangle]` C function is also exported raw from the
     `.wasm` (it appears in `InitOutput`). The real `fineprint-wasm` crate should keep the C ABI
     out of the wasm build. The generated `.d.ts` types `sha256_hex(data: Uint8Array | number[]):
     string`. A `// @ts-expect-error` line in `web/consumer.ts` proves the types are not `any`.
  6. Playwright: browser binaries come from the Playwright CDN and need
     `npx playwright install --with-deps <browser>` (apt dependencies) on ubuntu-latest. WebKit
     on Linux is Playwright's own build, not Safari. Tests wait on the server's listening promise
     and on the page's `window.spikeResult` promise, with no sleeps (R7). The default
     `test-results/` is redirected into the ignored `dist/`. The browser jobs reuse the
     `--target web` build uploaded by `wasm-node`, so all three engines run the same `.wasm`.
  7. Local evidence (Linux x86_64 container): spike fmt, clippy (native and wasm32) and the 12
     unit tests passed. The header drift check passed. The C caller built and ran with gcc 13.3
     and clang 18.1. The .NET pack, run, publish, apphost and RID check passed with SDK
     10.0.401, taken from the conda-forge package because the Microsoft download hosts are
     blocked in this sandbox. The Node.js caller and `tsc --noEmit` passed, and the Playwright
     test passed in a local Chrome 153 (`channel: "chrome"`). Playwright's own
     Chromium, Firefox and WebKit builds, Windows and macOS were not available locally, so the
     `Spikes` workflow is the evidence for them. `actionlint` 1.7.12 with shellcheck 0.11.0
     reports no findings on `spikes.yml`.
  8. The executor session cannot push. The Orchestrator commits and pushes to the assigned
     branch only, as recorded for FP-002. This entry was written by the Implementer.
**Rationale:** A separate workspace and workflow prove every binding path on all REQ-NFR-003
platforms without changing the root gates or the nine required checks. Packaging the native
library as a NuGet runtime asset, and consuming it as a package, tests the resolution path that
FinePrint.Core will ship. A single vectors file with independently computed digests makes the
parity check meaningful.
**Alternatives:** Add the spike to the root workspace members. Rejected: the root gates and
coverage would see it, and `unsafe` would leak into the root workspace. Use a ProjectReference
plus a copy of the native library to the output directory. Rejected: it does not exercise the
`runtimes/<rid>/native` resolution. Build the C caller through a `cc`-crate helper crate.
Rejected: the program would be a Rust binary rather than a C one. Use `wasm-pack`. Rejected:
it adds a tool whose wasm-bindgen pin is indirect. Use a third-party MSVC setup action.
Rejected: `vswhere` + `Enter-VsDevShell` needs no third-party code.

## FP-003 (#4) — Relative --root is resolved against the current directory
**Decision:** `cargo xtask coverage --root <dir>` resolves a relative `<dir>` against the current
working directory with a purely lexical normalisation (`.` and empty components dropped, `..`
pops, never above the filesystem root, no trailing separator) before comparing it with the
absolute filenames in the llvm-cov report. Absolute roots (leading `/` or `\`, or a drive prefix
such as `C:\`) and the auto-detected root are used unchanged. If the current directory cannot be
read and the root is relative, the command exits 2 with a message.
**Rationale:** Since a root matching no file is a failure, `--root .` made every valid report
fail. Lexical resolution is deterministic and does not touch the filesystem, so it cannot
disagree with the paths llvm-cov recorded the way a symlink-resolving `canonicalize` could.
**Alternatives:** `std::fs::canonicalize`. Rejected: resolves symlinks and needs the path to
exist, so it may mismatch the report. Reject relative roots with an error. Rejected: `.` and `..`
are the natural spellings when running from the workspace or from `xtask/`.

## FP-003 (#4) — resolve_root preserves UNC and verbatim prefixes

**Decision:** `resolve_root` treats a leading UNC prefix (`\\server\share` or `//server/share`) as one
non-poppable unit and re-emits it with both leading separators; `..` never removes the server or the
share. The Win32 verbatim forms are supported too: `\\?\C:` (drive, rooted like `C:\`) and
`\\?\UNC\server\share` are kept as prefixes. Separator style still follows the cwd; absolute `--root`
values pass through unchanged; no canonicalisation. `rel_to_root`/`under_root` needed no change.

**Rationale:** Windows allows UNC current directories. The old code dropped one leading separator, so
the root became `/server/share/repo` while llvm-cov filenames normalise to `//server/share/repo/...`,
every file failed `under_root` and `run` exited 1 despite valid coverage. The verbatim forms cost a
few lines in the same prefix parser.

**Alternatives:** UNC only (rejected: small extra code covers verbatim); `std::fs::canonicalize`
(rejected: filesystem access, resolves symlinks and yields `\\?\` paths that differ from the report).

## FP-003 (#4) — spikes/libs is measured against the non-rules coverage floor

**Decision:** `crate_of_rooted` maps `spikes/libs/<more>` (leading root-relative components
`spikes`, `libs`, then at least one more) to `fineprint-spike-libs`, and `FLOORS` gains
`("fineprint-spike-libs", 75)` after `xtask`. `spikes/bindings/**` and any other `spikes/` path stay
unmapped.

**Rationale:** The #5 entry "FP-004 — spikes/ directory handling by repo guards" requires the
REQ-NFR-008 75% non-rules floor on all of `spikes/libs/**` (`src`, `examples`, `tests`) with no
exemption. Without the mapping the spike's ~474 lines were silently unmeasured. The existing
nested-directory protection is unchanged: only the leading components are examined.

**Alternatives:** Exempting `examples/bench.rs` (rejected: needs an approved SRS change first);
mapping all of `spikes/**` (rejected: `spikes/bindings` is not a Rust workspace member measured by
llvm-cov); deleting the spike (rejected: only allowed once replacement golden-value tests exist).

## FP-003 (#4) — Coverage measures tests/examples/benches of workspace crates

**Decision.** `cargo xtask coverage` and the CI `coverage` job run `cargo llvm-cov --workspace
--all-targets --no-default-ignore-filename-regex --ignore-filename-regex
'(/\.cargo/(registry|git)/|/\.rustup/|/rustc/)' --json --summary-only` (CI adds `--locked` and
`--output-path`). Workspace `tests/`, `examples/` and `benches/` files are therefore measured.

**Rationale.** cargo-llvm-cov 0.9.1 drops those files by default, which left
`spikes/libs/examples/bench.rs` and `spikes/libs/tests/*.rs` out of the report although
FP-004 requires the floor on all of `spikes/libs/**`. Measured with the new flags:
`fineprint-spike-libs` 1105 lines, 916 covered, 82.89% (floor 75%); `xtask` 2026/1937, 95.60%;
no other crate regressed (all others n/a). `spikes/libs/examples/bench.rs` is measured at 0/181
(the example is only built, not run); tests/*.rs are 97-100%; src files 94-100%. Plain
`--workspace` without `--all-targets` omits bench.rs even with the filter off, so `--all-targets`
is part of the flag set. The two producers (`llvm_cov_args` and the CI step) must stay identical;
a unit test pins the argument list.

**Alternatives.** Leave the default filter (rejected: contradicts FP-004); exempt bench.rs
(rejected: needs an SRS-approved exemption); add a bench smoke test (not needed: floor met).

## FP-007 (#8) — Numeric and identity vocabulary

**Decision.** `fineprint-base` gains `num` (`Int`, `Fixed2`, `OverflowError`), `ids` (seven `u64`
newtype IDs, the `Id` and `CanonicalEncode` traits, `IdGenerator`, `IdGenerators`, `IdExhausted`)
and `collections` (`OrderedMap`/`OrderedSet` as `BTreeMap`/`BTreeSet` aliases and
`stable_sort_by_object_timestamp`). Choices:
- No operator overloading on `Int` or `Fixed2`; every arithmetic method names its overflow policy,
  `checked_*` (returns `None`) or `saturating_*` (clamps). Nothing panics or wraps.
- `Int` division and remainder truncate toward zero (Rust `/` and `%`); `None` for a zero divisor
  and for `MIN / -1`.
- `Fixed2` has one rounding rule for every precision-dropping operation (`checked_mul`,
  `checked_div`, `round_int`): round half away from zero, computed exactly in `i128`.
- IDs are `u64` newtypes ordered by the `u64`. Generators issue `last + 1`, so the first issued value
  is 1 and 0 is never issued; at `u64::MAX` `issue` returns `Err(IdExhausted)` and leaves the
  generator unchanged (an error, not a wrap).
- `CanonicalEncode` appends fixed-width little-endian bytes (8 bytes for every ID, `Int` and the
  `Fixed2` raw value), per the FP-004 "State hash" decision. Tag bytes and length prefixes are left
  to the hashing issue (#10).
- The property tests use a hand-rolled deterministic harness (SplitMix64 with literal seeds, at least
  5 000 iterations per property plus a fixed edge-value list, `i128` oracles) instead of proptest.
- The brief mentions `Fixed2` `try_` helpers without listing them; `Fixed2::try_from_int` and
  `Fixed2::try_from_halves` (returning `Result<Fixed2, OverflowError>`) were added as extra items.
- `crates/fineprint-base/src/lib.rs` was edited only to declare `pub mod collections; pub mod ids;
  pub mod num;` with doc lines, although it was not in the issue's owned list (the brief authorised
  it).
- Clippy `disallowed-types` evidence: a throwaway `src/num/scratch_disallowed.rs` using `f64` and
  `std::collections::HashMap`, declared from `num/mod.rs`, made
  `cargo clippy -p fineprint-base --all-targets -- -D warnings` exit with status 101, with:
  `error: use of a disallowed type `f64`` (note `REQ-CORE-006: no floating point in rules paths`)
  and `error: use of a disallowed type `std::collections::HashMap`` (note `REQ-CORE-006: hash
  iteration order is forbidden; use an ordered container`). The file and the `mod` line were then
  deleted and the same command exited 0.

**Rationale.** REQ-CORE-006 forbids floats and hash-order iteration and asks for scale-2 fixed point;
spelling the overflow policy in every call makes overflow mistakes visible at the call site. Round
half away from zero is a single symmetric rule (sign-independent), and truncating `Int` division
matches the host language so that no hidden adjustment exists. Starting IDs at 1 keeps 0 free as a
"none issued" marker in saved state (REQ-CORE-002). proptest is not in the FP-004 dependency table
and adding a dependency needs its own decision; `fineprint-base` depends on nothing.

**Alternatives.** Operator overloading that panics or wraps on overflow (rejected: hides the
policy); banker's rounding or truncation for `Fixed2` (rejected: two rules, or asymmetric for
negative values); floor division for `Int` (rejected: differs from `/` and surprises callers); IDs
starting at 0 or wrapping on exhaustion (rejected: ambiguous saved state, repeated IDs); a
length-prefixed or tagged encoding in the hook (rejected: belongs to #10); proptest (rejected: new
dependency).

## #7 FP-006 — `cr fetch` shells out to curl

**Decision.** `cargo xtask cr fetch` downloads with the `curl` program through
`std::process::Command` (arguments passed separately, no shell): `--fail --location --silent
--show-error --max-time 120 --output <cache>/<name>.part --url <url>`, never `-k`/`--insecure`;
curl applies the environment's proxy and CA settings itself. Each download goes to a `.part` file
that is validated and then renamed into place, so a failed run never leaves a partial cache. The
client sits behind the `Http` trait (`xtask/src/cr/fetch.rs`) so tests inject a fake and never
touch the network; a missing curl exits 2 with a clear message.

**Rationale.** xtask is standard-library only (FP-003), and the standard library has no TLS.
curl is preinstalled on the CI runners and on developer machines.

**Alternatives.** A Rust TLS/HTTP crate (rejected: xtask is std-only, no new dependencies); a
shell or Python script (rejected: harder to unit test and to keep on the exit-code contract).

## #7 FP-006 — CR source URL discovery and cache layout

**Decision.** Without `--url`, fetch downloads `https://magic.wizards.com/en/rules` and takes the
first `https://media.wizards.com/...MagicCompRules....txt` link, encoding spaces as `%20`; if none
is found it exits 2 suggesting `--url <url>`. `--url` overrides discovery. The cache is
`cr-cache/rules.txt` (the bytes as downloaded) and `cr-cache/source.json` (`sourceUrl`,
`effectiveDate` as an ISO date parsed from "These rules are effective as of <Month D, YYYY>.",
`sha256`, `byteCount`). A download that is empty, not UTF-8 or has no effective-date line is
rejected (exit 2) and the cache is left untouched. An existing cache is reused unless `--force`.
`fetch` also takes `--cache <dir>` (not in the brief) so tests and CI can point at another
directory; `index` and `check` refuse a cache whose `rules.txt` no longer matches the recorded
SHA-256 (exit 2, refetch with `--force`).

**Rationale.** The download link carries the release date and changes with each release; the
rules page is the stable entry point. Recording the checksum and date lets `check` warn when a
newer release is cached.

**Alternatives.** A hard-coded URL per baseline (rejected: breaks on every release); storing the
URL only in the index (rejected: the fetch must work before any index exists).

## #7 FP-006 — Command spelling `cargo xtask cr fetch|index|check`

**Decision.** The CR tooling is `cargo xtask cr fetch`, `cargo xtask cr index` and
`cargo xtask cr check`, replacing the `cite-check` placeholder in docs/DEVELOPMENT.md.

**Rationale.** Matches the issue's deliverables; one `cr` command group keeps the three steps
(network, regenerate, verify) separate so CI can cache the fetch and run only `check`.

**Alternatives.** A single `cite-check` that fetches and checks (rejected: mixes network and
verification, and has no way to regenerate the index).

## #7 FP-006 — Rule text extent and normalization

**Decision.** The rules body is the text strictly between the LAST line equal to
`1. Game Concepts` and the LAST line equal to `Glossary` (BOM and CR stripped first). A rule starts
at `ddd.n[x[x]][.] ` or a section heading `ddd. Title`; its text is the first line (number
included) plus following non-blank lines up to the next blank line OR the next rule start, so
`Example:` lines are included and a changed example re-flags the rule. A heading's text is the
heading line alone. A repeated number keeps the later entry. Normalization
`sha256-normalized-v1`: strip the BOM, replace every run of Unicode whitespace with one ASCII
space, trim; quotes and dashes are kept byte for byte. The hash is the SHA-256 of the normalized
text only. Two adaptations to the real 2026-11-06 file, which differs from the brief's layout
facts: (1) sub-rules can have two letters (a sub-rule after the `z` sub-rule), so up to two
lowercase letters are accepted in rule numbers, in both the parser and the scanner; (2) one
section heading directly follows the previous rule with no blank line, so a rule start also ends
the previous rule. The parse finds 3331 rules and no duplicates.

**Rationale.** Conservative: any edit to a cited rule's wording or examples is flagged for
re-verification. Without the two adaptations the two-letter sub-rule would be swallowed into its
predecessor and the heading's text would be appended to the previous rule.

**Alternatives.** Hashing the first line only (rejected: misses changes in continuation lines and
examples); excluding examples (rejected: the brief prefers conservative re-flagging); Unicode NFC
normalization (rejected: needs tables std lacks, and the official text is stable).

## #7 FP-006 — Citation grammar, range endpoints and the errata exclusions

**Decision.** In `docs/SRS.md`: every bare dotted token `ddd.n[x[x]]` (not preceded by a letter,
digit or dot; not followed by a letter, digit, `_` or a dot plus digit, so version-like numbers
are skipped), and every item of a `CR `/`CRs ` list separated by `,` `;` `/` or `and`. An item is a
rule number or a range with an attached en dash or hyphen; only the two endpoints are indexed. A
bare section counts only inside a `CR` list. In source comments only `CR` lists count (the R6
style), not bare numbers. One extension of the brief's grammar: after a dotted start, a bare-number
end is a rule in the same section (`CR 702.178–179` cites 702.178 and 702.179); the brief's
grammar read that end as section 179, which does not exist, and the real SRS uses this shape.
The exclusion list holds exactly two entries, both `docs/SRS.md` errata rows (29: `906`, 31:
`729.4`), matched by path, rule and a marker text on the row, so it cannot hide a source comment.
The real scan gives 456 cited rules (67 sections, 389 rules; one location outside the SRS, an
example citation in xtask/src/guard.rs's doc comment) and 2 excluded citations; no other SRS token
is missing from the CR.

**Rationale.** The SRS cites in several shapes; endpoint-only ranges keep the index small and
avoid inventing rules between endpoints. Restricting source comments to `CR` lists avoids false
positives from ordinary numbers in code comments.

**Alternatives.** Expanding ranges to every rule in between (rejected: brief says endpoints
only); bare tokens in source comments (rejected: false positives); a per-line exclusion by line
number (rejected: SRS edits shift lines).

## #7 FP-006 — `cr check` compares sets and hashes, not locations

**Decision.** `check` fails (exit 1) when a cited rule is missing from the fetched CR, is cited
but not in the committed index, has a hash different from the committed one, or when the index is
missing, unparsable or of an unknown `schemaVersion`. Locations and line numbers are never
compared. An indexed rule that is no longer cited, and a fetched effective date that differs from
`baseline.effectiveDate`, are `warning:` lines only. `index` refuses to write when a citation is
missing from the CR, or when `docs/SRS.md` does not contain the fetched baseline id
(`CR-<effective date>`).

**Rationale.** SRS and source edits move lines constantly; only the set of cited rules and their
verified text matter (REQ-DOC-001). The baseline-id check keeps the index tied to the SRS rules
baseline.

**Alternatives.** Comparing locations (rejected: every edit would fail the check); failing on
uncited index entries (rejected: harmless and fixed by re-indexing).

## #7 FP-006 — `cr.yml` is not a required check

**Decision.** `.github/workflows/cr.yml` (job `cr`) runs on pull requests touching
`docs/SRS.md`, `docs/cr-citations.json`, `xtask/src/cr/**`, any source file with an extension the
citation scanner reads (`.rs .cs .c .cpp .h .ts .js .py .toml .yml .yaml`) or the workflow, weekly
(Monday 04:17 UTC) and on `workflow_dispatch`. It caches `cr-cache/` with actions/cache (exact key
on the index hash, no prefix restore), runs `cr fetch` on pull requests and `cr fetch --force` on
the schedule and manual runs, then `cr check`. It is not a required status check.

**Rationale.** The job depends on the network and an external site; a required check would block
merges on outages. The weekly forced refetch detects a new CR release; a changed rule then fails
the job, which flags it for re-verification. REQ-DOC-001 also requires a changed rule to block
release; no release step reads this job's result yet, so that gate is not enforced by this unit.
The enforcement mechanism (for example a release precondition requiring a passing `cr` run on the
release commit, or a required status check) is an open maintainer choice recorded in
`docs/HUMAN_QUEUE.md`.

**Alternatives.** Required check (rejected: network dependence); running on every pull request
(rejected: unnecessary downloads).

## Self-chaining — claim ordering

**Decision.** `work-issue` §1 step 4 arbitrates concurrent claims by GitHub issue-comment ID. Each
issue's governing claim is its lowest-ID claim comment that names a live session. A new claim wins
only if its ID is lower than the governing claim of the same issue and of every issue whose owned
paths overlap. Labels change only after the claim holds.

**Rationale.** GitHub offers no atomic lock on issues. Timestamps have one-second resolution, so
they tie. Comment IDs are unique, so they don't tie. Comparing only an issue's latest claim let a
third session's later claim hide an earlier winner, so every comparison uses the governing claim.

**Assumption (undocumented).** Issue-comment IDs are allocated in creation order across issues,
and a comment is listed immediately after its creation call returns. Both are observed behaviour,
not guaranteed by the GitHub REST documentation. If either fails, two sessions can rarely start
overlapping work. **Residual risk:** the P6 merge conditions (branch up to date with `main`,
required checks green) often surface such overlap as a merge conflict or a red check, but they do
not guarantee it. Two branches can make independent textual edits that merge cleanly and pass CI
while their combined behaviour conflicts. The owner weighs this risk against the lock below.

**Alternatives.** A lock file committed to a branch, using git push as the atomic step (rejected
for now: it adds commits and a cleanup path for every claim). A single orchestrating session
(`run-plan`) that alone selects work (still available: the race only exists when several sessions
pick work independently).

## CI — PR load

**Context.** Every push to a pull-request branch ran ci.yml twice, once for `push` and once for
`pull_request`. The concurrency group is keyed by ref, so neither run cancelled the other. Every
PR also ran the full matrix, including PRs that change only Markdown or agent/skill text, and
spikes.yml ran its five binding jobs on every push and PR.

**Decision.**
1. ci.yml and spikes.yml run on `push` to `main` only, plus `pull_request`. The PR run covers
   the branch head.
2. A `changes` job runs first. On a pull request whose changed files are only Markdown other than
   `docs/SRS.md`, or files under `.claude/`, the `fmt`, `clippy` and `test` jobs report success
   under their required names with every step skipped, and `coverage` is skipped. The skip is
   per step because a job-level `if` on a matrix job reports a skipped check under the unexpanded
   name, which never satisfies a required context. `docs/SRS.md` counts as code because the
   xtask `cr` tests read it, and so does every non-Markdown file under `docs/` (for example
   `docs/cr-citations.json`). Nothing compiled or tested includes another Markdown file. `guards`
   and `req-coverage` always run. Every push to `main` runs everything.
3. spikes.yml runs only when `spikes/bindings/**`, `rust-toolchain.toml` or the workflow itself
   changes, and on `workflow_dispatch`. Its workspace is isolated, so nothing else can change its
   result. Its jobs are not required checks.
4. `fmt` and `clippy` run on `ubuntu-latest` only. Formatting is OS-independent. Lints differ
   only for `cfg(target_os)` code, and `test` still compiles and runs the whole workspace on
   Linux, Windows and macOS. The required checks become `fmt (ubuntu-latest)`,
   `clippy (ubuntu-latest)` and the three `test` jobs.

**Relation to the SRS.** REQ-NFR-003 requires CI to build, run the full test suite and replay
the golden corpus on every listed platform. REQ-NFR-011 makes those jobs required checks on
`main`.
- `test` still does exactly that on all three systems for every pull request that changes
  anything that can be built or tested, and on every push to `main`.
- A documentation-only pull request cannot change a build or test result, so skipping the steps
  is an interpretation of "every change", not a weakening of the gate. The repository owner
  confirmed it on 2026-11-18 and removed the Windows and macOS `fmt`/`clippy` checks from the
  ruleset.
- Dropping Windows and macOS `fmt`/`clippy` removes no build or test from any platform.

**Residual risk.**
- A lint that fires only under a Windows or macOS `cfg` is no longer caught in CI. Today the
  workspace has no such code.
- If a future test reads another Markdown file, the docs-only classification must be widened in
  the `changes` job.

## #9 FP-008 — PRNG module

**Decision** (REQ-CORE-001, REQ-API-004). `fineprint_base::prng` is the engine's only source
of randomness, in-house, with no new dependency (`fineprint-base` still depends on nothing).
- `ALGORITHM_ID = "PCG64-DXSM"`, the value replay headers and the state record.
- `Pcg64Dxsm` is ported from `spikes/libs/src/prng.rs`; `from_u64_seed(seed64)` is the single
  SplitMix64 seed-expansion helper for match seeding and fork reseeding, as decided in
  "FP-004 — Resolution of the queued items". `fork(salt)` is
  `from_u64_seed(hi64(state) ^ lo64(state) ^ hi64(increment) ^ lo64(increment) ^ salt)` and does
  not advance the parent.
- Bounded integers use Lemire's multiply-with-rejection exactly: `m = x * bound` (128-bit);
  if `lo64(m) < bound`, reject while `lo64(m) < (2^64 - bound) % bound`; return `hi64(m)`.
  `bound == 0` is `None` with no draw; `bound == 1` is `Some(0)` and consumes one draw.
- Shuffle is Fisher-Yates from the end: for `i` from `len - 1` down to 1, swap `i` with
  `next_bounded(i + 1)`. Lengths 0 and 1 consume no draws.
- `choose` draws `next_bounded(len)`; empty slices give `None` with no draw.
- `random_subset(n, k)` is selection sampling over `0..n` (choose `i` when
  `next_bounded(n - i) < k - chosen`), stopping as soon as `k` indices are chosen, and returns
  the indices in ascending order. `k > n` is `None` and `k == 0` is empty, both with no draw.
  If the result vector cannot be reserved (`Vec::try_reserve_exact(k)` fails, which includes the
  capacity overflow of `random_subset(usize::MAX, usize::MAX)`), the result is also `None` with
  no draw: the reservation is made before the first draw, so the call never panics and the
  generator is untouched. The public contract is therefore `None` when `k > n` or when `k`
  indices cannot be reserved; a `Some` result is unchanged.
- `from_raw(state, increment)` rejects an even increment (`None`) and validates nothing else.
- `CanonicalEncode` appends 32 bytes: `state` little-endian (16), then `increment`
  little-endian (16); no tag, no length prefix.
- `crates/fineprint-base/src/lib.rs` gained only the `pub mod prng;` declaration, by explicit
  authorisation in the issue #9 brief (exception to R1).

**Rationale.** REQ-CORE-001 requires one specified algorithm, stored in the state, with output
fixed per engine major version, so every sampling algorithm (and the number of draws it consumes)
is part of the frozen contract and is pinned by hard-coded vectors that CI checks on Linux,
Windows and macOS. Lemire's method is unbiased and needs only integer arithmetic; the
from-the-end Fisher-Yates and ascending selection sampling are the textbook forms and make the
draw count a simple function of the input sizes. An odd increment is the only structural
invariant of a PCG stream, so it is the only check `from_raw` makes on untrusted saved state
(R12). Little-endian fixed-width bytes follow the `CanonicalEncode` contract. CodeRabbit
(PR #155) found that `Vec::with_capacity(k)` panics for `k` whose byte size exceeds `isize::MAX`,
which contradicts the module's non-panicking guarantee; a fallible reservation before any draw
was chosen over dropping the up-front reservation, because it keeps the single-allocation
behaviour for normal inputs and keeps draw consumption unchanged.

**Alternatives.** The `rand_pcg`/`rand` crates (rejected: a dependency in the rules crate, and
`rand_pcg` seeds with the cheap multiplier, which differs from NumPy; see
"FP-004 — PRNG: in-house PCG64-DXSM (no crate)"). Modulo reduction or the "widening multiply
without rejection" (rejected: biased). Floyd's or reservoir subset sampling (rejected: unordered
output or more draws for small `k`). Fisher-Yates from the front (equivalent in distribution;
rejected only to fix one frozen order).

## #10 FP-009 — Canonical encoding and Merkle hash framework

**Decision.**
- The `CanonicalEncode` trait stays in `crates/fineprint-base/src/ids/encode.rs` (the #8
  contract) and is re-exported as `fineprint_base::encode::CanonicalEncode`; both paths name the
  same trait. It is not moved, so the #8 contract and every existing import stay stable.
- `fineprint-base` depends on `sha2` through the workspace pin `=0.11.1` with
  `default-features = false`, as authorised by "FP-004 — State hash: SHA-256 Merkle (sha2),
  budget met".
- Length prefixes (`put_len`): a length below `u32::MAX` is a `u32` little-endian (4 bytes); any
  other length, `u32::MAX` included, is the escape `0xFFFF_FFFF` followed by the length as a
  `u64` little-endian. Every `usize` has exactly one encoding, with no panic and no saturation.
- Layouts: integers `u8..u64`, `i8..i64` fixed width little-endian; `bool` one byte `0x00`/`0x01`;
  `str`/`String` length-prefixed UTF-8; `[T]`/`Vec<T>` length then elements; `[T; N]` elements
  only (N is part of the type); `Option<T>` `0x00` or `0x01` then the value; tuples (2 and 3)
  fields in order; `BTreeMap`/`BTreeSet` length then entries in ascending key order; `&T`
  delegates; `Digest` its 32 raw bytes; structs through `impl_canonical_struct!` (listed fields in
  listed order, no prefix). No impl for `usize`/`isize` or floats.
- Leaf digest = SHA-256(`0x00` || payload); internal digest = SHA-256(`0x01` ||
  `put_len(child count)` || for each child in ascending `Label` order: canonical label bytes ||
  child digest). `Label::Name(s)` encodes as `0x00` + length-prefixed UTF-8, `Label::Index(i)`
  as `0x01` + 8 bytes little-endian; every Name sorts before every Index.
- The parent hash covers each child's label and the child count. This extends the FP-004
  prototype layout (`parent = SHA-256(0x01 || child digests)`), which hashed a fixed child order
  without labels.
- `diff_first_divergence(a, b)` returns `Option<ComponentPath>` (`None` when the trees are
  structurally identical) rather than the bare `ComponentPath` of the issue text.
- `MerkleTree::hash_operations()` counts the leaf and node digests computed by `root_digest` and
  `digest_at` (not by `full_root_digest` or `diff_first_divergence`), so the incremental cost is
  tested exactly without timing (R7).
- Property tests use a hand-rolled deterministic harness (PCG64-DXSM with literal seeds, at least
  5 000 iterations, fixed edge values) instead of proptest, which is not an approved dependency
  ("FP-007").
- Explicit grants used (exceptions to R1, authorised by the #10 brief): `lib.rs` gained only
  `pub mod encode;` and `pub mod hash;` with one doc line each; `crates/fineprint-base/Cargo.toml`
  gained only `sha2 = { workspace = true }`; `Cargo.lock` changed only by cargo (fineprint-base
  gains the dependency `sha2`); `docs/DEVELOPMENT.md` changed only the "Depends on" cell of the
  `fineprint-base` crate-map row.
- `MerkleTree::set_leaf` rejects any path of more than `MAX_PATH_DEPTH` = 256 labels with
  `PathError::TooDeep`, before any mutation (CodeRabbit finding, PR #156). The digest, view,
  comparison, clone and drop code recurses once per level and stays recursive; the bound makes
  that safe. The limit changes no digest byte layout.

**Rationale.** REQ-NFR-006 asks for a Merkle hash that can locate the first differing component.
Hashing labels into the parent makes the digest commit to the component names, so two states
that hold the same component bytes under different names (for example a moved object) hash
differently, and children can be added or removed without positional ambiguity; the child count
and the injective length escape make every internal preimage parse uniquely. The distinct
`0x00`/`0x01` prefixes keep leaves and internal nodes from colliding. Returning `Option` avoids
inventing a path for equal trees. Keeping the trait in place honours the #8 contract (R2). The depth bound of 256 levels is far
deeper than any component tree the engine's zone/object/player hashing needs, and keeps the
recursion well inside a 2 MiB thread stack (tested on one).

**Alternatives.** Moving the trait into `encode` (rejected: breaks the #8 contract and imports).
A plain `u32` length that saturates or panics above `u32::MAX` (rejected: not injective or not
panic-free). Positional children without labels as in the FP-004 prototype (rejected: the
divergence finder needs stable names, and label-free hashing cannot tell renamed components
apart). Returning the root path for equal trees (rejected: ambiguous with a root-level
difference). Timing assertions for incremental cost (rejected: R7). proptest (rejected: not an
approved dependency). For the depth finding, option (b), making digest/view/diff/clone/drop
iterative for any depth (rejected: it adds iterative re-implementations and a custom `Drop` to
the hash-critical path for depths nothing needs, and a hand-rolled iterative `View` risks
diverging from the recursive reference); a smaller limit such as 64 (rejected: too close to the
existing depth-64 test and to plausible state nesting).

## Process — Judgement calls are decisions, not queue items

**Decision.** An agent's recommendation on a judgement call is the decision (R8). It is recorded
once in `docs/DECISIONS.md` (Decision / Rationale / Alternatives) and is final unless a later
DECISIONS.md entry or the repository owner changes it. `docs/HUMAN_QUEUE.md` is kept only for the
P8 items an agent cannot do or decide itself: architecture and platform decisions that become
`REQ-NFR-*`, edits to `docs/SRS.md` or `docs/PROCESS.md`, anything with legal weight, publishing a
release, and repository settings an agent cannot change (for example branch-protection required
checks). Consequently: a `[Fable wanted]` note goes in the unit's `docs/RUN_LOG.md` row; a
CodeRabbit that never reviews is a RUN_LOG row; a unit that fails P7 is marked `blocked` in
RUN_LOG with the `status:blocked-failure` label and an issue comment with the failure history; a
deferred review finding becomes a new `work-unit` issue or a DECISIONS.md entry; an SRS-versus-CR
erratum is recorded in DECISIONS.md with one P8 queue line proposing the SRS edit.

**Rationale.** Asked by the repository owner (2026-11-18). Queue lines for decided calls asked for
a confirmation nobody gave, and the run never waited for them anyway.

**Alternatives.** Keeping informational queue lines (rejected: the owner asked not to). Removing
the queue entirely (rejected: P8 items such as SRS edits, licensing and branch-protection settings
can only be done by a person).

Queue clean-up: every open non-P8 line in HUMAN_QUEUE.md was struck through as decided, citing its existing DECISIONS.md section; the crate-to-floor table of FP-003 (#4) (base, dsl, core at 90%; others at 75%) is confirmed as decided; the only open queue items are P8 (repository settings).

## #10 FP-009 — Resolution of the queued items

**Decision.** Both #10 queue items are final decisions with no person action.

1. `diff_first_divergence(a, b) -> Option<ComponentPath>` is the approved signature (`None` =
   structurally identical trees); issue #10's deliverable line was updated to match. The
   `sha2 =0.11.1` dependency (already pinned and licence-accepted in FP-004) and hashing child
   labels into parent digests (so renamed components cannot collide) are likewise final.
2. `MAX_PATH_DEPTH` = 256 labels stays.

**Rationale.** (1) A bare `ComponentPath` has no unambiguous "equal" value: the root path is also
the answer for a difference at the root. `Option` makes the equal case explicit for the
investigator use (REQ-NFR-006). (2) SRS §17 REQ-NFR-006 (docs/SRS.md ~line 1408) lists the hashed
components as each zone, object, player, the stack, pending triggers, turn position and PRNG
state, so a state tree is a handful of levels deep (state, zone, object, characteristic, value:
roughly 5 to 8). 256 is two orders of magnitude of headroom and keeps the recursive
digest/diff/clone/drop well inside a 2 MiB stack (tested). If a future unit needs deeper paths, it
raises the constant through a DECISIONS.md entry; the hash layout does not depend on the limit.

**Alternatives.** Iterative traversal for unbounded depth (rejected in the earlier #10 entry). A
lower limit such as 64 (rejected: too close to the depth-64 tests and leaves little margin).

## #7 FP-006 — Release gate for a failed `cr` run

**Decision.** A failed `cr` run blocks a release through a release precondition: the release workflow (issue #122, FP-120) requires a successful `cr` run (`cr fetch --force` then `cr check`) on the exact release commit before publishing, and fails otherwise (REQ-DOC-001). `cr` stays a non-required PR check.

**Rationale.** A required status check would block every merge during an outage of the external rules site (the reason recorded in "#7 FP-006 — `cr.yml` is not a required check"); a precondition on the release commit enforces REQ-DOC-001 exactly where it applies, at release time, and the release itself is a P8 action a person triggers.

**Alternatives.** Making `cr` a required status check (rejected: network-dependent merge blocking); leaving the choice open (rejected: R8, judgement calls are decided).

## #11 FP-010 tests — idempotence vs. unsafe-integer output

**Superseded** by "## #11 FP-010 — Safe integer range for hashed JSON": any number whose value is integral with magnitude above 2^53-1 is UnsafeInteger whatever its spelling, and idempotence holds for every accepted vector.

**Decision.** The spec tests assert that a non-integer number token whose value lies between 2^53
and 1e21 (for example `1e20`, `1.2345678901234568e20`) canonicalises to its ECMAScript plain-digit
form (`100000000000000000000`, `123456789012345680000`), and that re-canonicalising that output is
`UnsafeInteger`. The idempotence assertion is applied to every other vector and to all random
documents, but not to these outputs.

**Rationale.** The brief requires three things that cannot all hold for these inputs: ECMAScript
number forms including "123456789012345680000 style cases", rejection of integer tokens with
magnitude above 2^53-1 "anywhere", and idempotence "on every vector". ECMAScript prints these values
without an exponent, and the result is a plain integer token beyond the safe range. The specific
clauses (number forms, unsafe-integer rejection, both quoted from RFC 8785 / I-JSON practice in
REQ-NFR-006) are kept over the general one; definitions are unaffected because every typed
integer field is limited to the safe range.

**Alternatives.** Rejecting any number whose value is an integer beyond 2^53-1 whatever its
spelling (rejected: also rejects `1e21`, which the brief requires to canonicalise to `1e+21`);
dropping the large-value vectors (rejected: weakens the test, R10).

## #11 FP-010 — serde, serde_json and serde_json_canonicalizer in fineprint-dsl

**Decision.** `fineprint-dsl` depends on `serde`, `serde_json` and `serde_json_canonicalizer`
through the existing `[workspace.dependencies]` pins (`=1.0.229`, `=1.0.154` with `std` and
`float_roundtrip`, `=0.4.1`), chosen by the FP-004 spike ("FP-004 — RFC 8785 canonical JSON:
serde_json_canonicalizer"). `serde_json`'s `preserve_order` feature stays off, so
`serde_json::Map` is a `BTreeMap` (REQ-CORE-006). No other dependency is added.
**Rationale.** REQ-NFR-006 requires RFC 8785 canonical JSON for `definitionHash`; FP-004 already
selected and licence-checked these crates. Reusing the pins keeps one version of each.
**Alternatives.** Depending on the spike crate (rejected: spikes are not product code); a
hand-written JCS serialiser (rejected: FP-004 measured the library against the RFC vectors).

## #11 FP-010 — Wiring edits outside the owned directories

**Decision.** Besides `src/card/`, `src/mana/` and `src/canonical/`, the unit edits only
`crates/fineprint-dsl/src/lib.rs` (three `pub mod` declarations with doc comments),
`crates/fineprint-dsl/Cargo.toml` (the three dependencies above) and `Cargo.lock` (the
resulting lock entries), as the brief sanctions.
**Rationale.** The new modules cannot be reached or built without them (R1 minimum wiring).
**Alternatives.** None that compile.

## #11 FP-010 — definitionHash excludes the definitionHash member

**Decision.** `definitionHash` is the lowercase-hex SHA-256 (`fineprint_base::hash::sha256`) of
the RFC 8785 bytes of the serialised definition with the `definitionHash` member omitted.
`verify_hash` compares strings exactly, so uppercase hex is a mismatch. `from_json` checks, in
order: canonical-level JSON checks, a string `dslVersion` with the engine's major, the typed
schema, then the hash.
**Rationale.** SRS §14.2 says "sha256 of canonical JSON of this definition"; a hash cannot cover
itself, and omitting the member (rather than hashing a placeholder value) leaves no magic value
in the format. A single spelling of the hex text keeps hash comparison byte-exact.
**Alternatives.** Hashing with `definitionHash: ""` (rejected: an invented placeholder);
case-insensitive comparison (rejected: two spellings of one artifact identity).

## #11 FP-010 — ManaCostDSL JSON is the canonical symbol string

**Decision.** `ManaCostDSL` serialises as one JSON string of its canonical text (`"{2}{W}{W}"`).
An empty or invalid string is a deserialisation error. "No mana cost" is the absence of the
face's `manaCost` member (`Option::None`); `{0}` is `"{0}"`. Every symbol has exactly one
accepted spelling, so text and value map one to one and canonical JSON is unique.
Malformed-bracket offsets: an unterminated `{` and an empty `{}` report the offset of that `{`;
a nested `{` reports the inner `{`; a stray `}` or text outside braces reports its first byte.
`UnknownSymbol.symbol` carries the braced text including the braces.
**Rationale.** The string form is compact, readable in definition diffs and is the same grammar
the ingestion parser reads (SRS §16). One spelling per symbol keeps `definitionHash` stable.
**Alternatives.** An array of tagged symbol objects (rejected: verbose, and a second grammar to
keep in sync); reporting the end of input for an unterminated `{` (rejected: the `{` is the byte
that lacks its partner).

## #11 FP-010 — ColorBitmask bit assignment

**Decision.** `ColorBitmask(u8)` uses W = 1, U = 2, B = 4, R = 8, G = 16, serialised as a JSON
integer 0..=31; any other value (32, negative, non-integer) is rejected on input.
**Rationale.** The bits follow the conventional W U B R G color order, which is also the
declaration order of `mana::Color`. An integer is the cheapest canonical form.
**Alternatives.** An array of color letters (rejected: several spellings of one set unless
sorted, larger JSON).

## #11 FP-010 — Mana symbol set limited to SRS §4.1

**Decision.** The parser accepts exactly the SRS §4.1 symbol classes: generic `{n}`
(0..=1 000 000, no leading zeros), colored, colorless `{C}`, snow `{S}`, `{X}`/`{Y}`/`{Z}`, two-color
hybrid `{a/b}` (a ≠ b, written order kept, so `{W/U}` and `{U/W}` are different values),
monocolored hybrid `{n/c}` (n ≥ 1), Phyrexian `{c/P}` and hybrid Phyrexian `{a/b/P}`.
Colorless hybrid, half mana, energy and infinity symbols are rejected as unknown symbols.
**Rationale.** SRS §4.1 is the normative list for `ManaCost`; symbols outside it have no rules
support in the engine yet, and accepting them would let definitions carry costs nothing can pay.
Adding a class later is an additive grammar change.
**Alternatives.** Accepting every symbol seen in bulk data (rejected: outside §4.1, unhandled);
normalising hybrid order (rejected: the written order is data, and one spelling per value
already holds).

## #11 FP-010 — Strict deny_unknown_fields on definition types

**Decision.** Every definition struct uses `#[serde(rename_all = "camelCase",
deny_unknown_fields)]`. A definition with a newer minor `dslVersion` that adds members is
therefore rejected with `CardDefinitionError::Schema` until this engine learns the member; only
the major version is gated by `check_dsl_version`. Plain `Deserialize` performs no version or
hash check; `CardDefinitionDSL::from_json` is the checked loader. A present `power` or
`toughness` member is decoded as `Some` even when it is JSON `null`, so
`Some(ValueExpressionAST(Value::Null))` survives a round trip.
**Rationale.** The hash is verified over the checked input (with only `definitionHash`
removed), and the loader also requires the decoded value to re-serialise to exactly that input
(see "#11 FP-010 — Lossless decode and hash over the checked input"); an unknown member would be
dropped by decoding, so the decode would not be lossless and it cannot be tolerated. Failing loudly is also R11 ("never silently runs an incomplete definition").
**Alternatives.** Ignoring unknown members (rejected: the hash would not cover them); keeping
unknown members in a catch-all map (rejected: the engine would carry data it cannot interpret).

## #11 FP-010 — Placeholder AST types

**Decision.** `ValueExpressionAST`, `AbilityAST` and `PartnerAbilityDSL` wrap a
`serde_json::Value` and `TextTokenStream` wraps `Vec<serde_json::Value>`, all
`#[serde(transparent)]`, until FP-011/012/013 define the real schemas. Their content takes part
in canonical JSON and the hash.
**Rationale.** The top-level schema and hash can land and be tested now without inventing AST
shapes that later issues own (R1).
**Alternatives.** Empty structs (rejected: definitions with content could not round-trip);
defining the ASTs here (rejected: owned by later units).

## #11 FP-010 — Safe integer range for hashed JSON

**Decision.** `canonical::canonicalize` and `to_canonical_vec` reject, with
`CanonicalError::UnsafeInteger`, every number whose value is an integer of magnitude above
`MAX_SAFE_INTEGER` (2^53 − 1), whatever its spelling: integer literals such as
`9007199254740992`, and also exponent or fraction spellings with an integral value such as
`1e20`, `1e21` or `9007199254740992.0`. Non-integer numbers (for example `1.5`, `1e-7`) are
accepted. The check is lexical, with no floating-point type (REQ-CORE-006): integer literals on
the input, and the JCS output's ECMAScript number forms (a positive exponent means an integer of
at least 1e21; no `.` and no exponent means an integer).
**Rationale.** JCS prints large integral doubles silently rounded ("FP-004 — RFC 8785 canonical
JSON"), so such values must travel as strings. The brief also requires idempotence
(`canonicalize(canonicalize(x)) == canonicalize(x)`) and rejection of out-of-range integer
literals: the canonical form of `1e20` is the integer literal `100000000000000000000`, so
accepting `1e20` while rejecting that literal would break idempotence. Rejecting by value is the
only rule that satisfies both.
**Alternatives.** Rejecting only digit-only literals (rejected: not idempotent, as above);
accepting exponent-form integers of 1e21 or more because JCS prints them with an exponent
(rejected: they are still integers beyond the safe range, and it makes the rule depend on a
printing threshold).

## #11 FP-010 — Checked serializer replaces serde_json::to_value

**Decision.** `canonical::to_canonical_vec` (and through it `CardDefinitionDSL::to_canonical_json`,
`compute_hash` and `seal`) serialises with a private `serde::Serializer`
(`canonical/ser.rs`, `to_checked_value`) that builds the same `serde_json::Value` as
`serde_json::to_value` for every input that function accepts (demonstrated by a differential unit
test), and errors instead where `to_value` would silently lose or alter content: non-finite
floats (`CanonicalError::Serialize`, at any depth), a member name emitted twice
(`CanonicalError::DuplicateKey`), noncharacters, nesting beyond `MAX_DEPTH`, and integers outside
±`MAX_SAFE_INTEGER` (`UnsafeInteger`, all widths up to 128 bits). Scalars, floats and map keys
are delegated to `serde_json::value::Serializer` after the checks, so number and key formatting
is serde_json's own. Its `serialize_f32`/`serialize_f64` methods carry a scoped
`#[allow(clippy::disallowed_types)]`; the two unit tests that construct NaN and infinities do
too. The old post-hoc `check_value_integers` pass is removed (redundant). The final lexical scan
of the JCS bytes stays (it catches large integral doubles such as `1e20`).
**Rationale.** REQ-NFR-006: exported bytes must be what the loader accepts, and RFC 8785 requires
an error for NaN and infinity. REQ-CORE-006 bans floats in rules arithmetic; the serializer's float
methods are the boundary that rejects non-finite values and passes finite ones through untouched,
so naming the type there is the only way to enforce the rule, and the allow is limited to those
methods. Duplicate detection uses the object's own ordered `serde_json::Map` (BTreeMap-backed,
`preserve_order` off) via `contains_key` before insertion, which is the per-object ordered set the
brief asks for without keeping a second copy of the names.
**Alternatives.** Post-checking the `Value` from `to_value` (rejected: NaN is already `null` and
the first duplicate already lost); a separate `BTreeSet` per object (equivalent; not needed);
writing JSON text and re-parsing it (rejected: serde_json's writer also emits `null` for NaN).

## #11 FP-010 — Duplicate member names on export are DuplicateKey

**Decision.** A `Serialize` impl that emits the same member name twice (through `SerializeMap` or
`SerializeStruct`) gives `CanonicalError::DuplicateKey(<name>)`, the same kind the input path uses.
**Rationale.** One error kind for one defect, whichever path finds it; callers already match it.
**Alternatives.** `CanonicalError::Serialize` (rejected: hides the kind behind a message).

## #11 FP-010 — Noncharacters rejected without a new error variant

**Decision.** Noncharacters (U+FDD0..=U+FDEF and every code point whose low 16 bits are FFFE or
FFFF) are rejected in decoded member names and string values, literal or escaped (RFC 7493 §2.1).
On input (`canonicalize`, `parse_checked`, hence `from_json`) the error is
`CanonicalError::Syntax`; on export (`to_canonical_vec`) it is `CanonicalError::Serialize`; both
messages name the code point (`U+XXXX`). The input check is an iterative walk of the parsed tree.
**Rationale.** The input is not acceptable I-JSON, which the `Syntax` variant documents; on
export the value cannot be represented, which is what `Serialize` means. No variant is added
because the Independent Tester's spec tests match `CanonicalError` exhaustively.
**Alternatives.** A new `Noncharacter` variant (rejected: breaks exhaustive matches, a public API
change); accepting noncharacters (rejected: RFC 7493 §2.1).

## #11 FP-010 — One depth budget: MAX_DEPTH = 127

**Decision.** `canonical::MAX_DEPTH = 127` is the largest nesting depth (simultaneously open
arrays and objects; a scalar adds none) accepted by loading, hashing and export. The checked
serializer errors with `CanonicalError::Serialize("nesting depth exceeds 127")` the moment a
container would exceed it, before recursing. For a card the definition object, `faces`, a face
and `abilities` use four levels. The parser limit is not relaxed.
**Rationale.** The value was read from serde_json 1.0.154's parser (`remaining_depth` starts at
128 and is decremented on every `[`/`{`, failing when it reaches 0, so 127 levels pass and 128 do
not) and is pinned by a unit test that `canonicalize` accepts 127 nested arrays and rejects 128,
so the constant cannot drift from the parser. Checking before recursion means a pathologically
deep value (tested at 100 000 levels) cannot overflow the stack.
**Alternatives.** Raising the parser limit (rejected: weakens the untrusted-input bound); a
smaller separate export limit (rejected: two budgets are what produced the defect).

## #11 FP-010 — No explicit null for optional members

**Decision.** Every optional, non-nullable member (`manaCost`, `colorIndicator`, `loyalty`,
`defense`, `handModifier`, `lifeModifier`, `knownGaps`, `meldPartner`, `commanderEligibility`,
`relatedTokens`, `parserVersion`, `overrideSource`, `verifiedBy`) uses
`deserialize_with = "present"`: omission gives `None`, a present `null` is an error (also for
plain serde `Deserialize`). `power` and `toughness` keep `Some(Value::Null)` for `null`.
**Rationale.** The SRS marks these members `?:`, never `| null`, and the canonical form omits
them; accepting `null` made two documents with different bytes decode to one value.
**Alternatives.** Rejecting `null` only in `from_json` (rejected: the type's own decoder should
match the schema; the lossless check would catch it anyway).

## #11 FP-010 — Lossless decode and hash over the checked input

**Decision.** `CardDefinitionDSL::from_json` takes the checked document in its canonical form
(parsed back from the JCS bytes, so `1.0`, `-0.0` and `1` load as the integer value), checks
`dslVersion` first as before, decodes the typed struct (`Schema` on failure), then requires the
decoded value re-serialised with the checked serializer to equal that document exactly
(`Schema`: "decoding changed the content ..."), and finally compares `definitionHash` with
SHA-256 of the RFC 8785 bytes of the checked document with only `definitionHash` removed
(`HashMismatch`). The string enums now decode only from strings (hand-written `Deserialize` in
`string_enum!`). Plain serde `Deserialize` stays the documented unchecked building block (it may
still accept positional arrays).
**Rationale.** The digest must cover what the file says, not what decoding kept: a positional
array, a `null` member or a singleton-object enum decoded to the same struct and passed the old
re-serialised-hash check. One equality check rejects every lossy decode, including ones not yet
imagined, without per-type map-only deserializers; the error order of the contract is unchanged.
**Alternatives.** Map-only deserializers per struct (rejected: covers only the listed cases);
hashing the input while skipping the lossless check (rejected: a decoded value could still differ
from the hashed document).

## #11 FP-010 — Sealing normalises placeholder numbers

**Decision.** `seal` replaces the value with the decode of its own canonical JSON before storing
the hash, so a placeholder holding `-0.0` or `1.0` is stored as `0` or `1`. The canonical bytes
and the hash are unchanged; only sealed values carry the guarantee
`from_json(&v.to_canonical_json()?) == Ok(v)`.
**Rationale.** JCS writes `-0.0` as `0`; without normalising, the loaded value would not equal the
sealed one although the hash agreed.
**Alternatives.** Rejecting non-canonical number spellings in placeholders (rejected: valid JSON
the JCS defines; normalising is lossless for the hash).

## #11 FP-010 — ManaCostDSL::new validates with the parser's rule

**Decision.** `ManaSymbol::is_valid` (new public method) states the parser's rules
(`Generic` 0..=1 000 000, `GenericHybrid` 1..=1 000 000, two different colours for `Hybrid` and
`HybridPhyrexian`); the parser filters every candidate symbol through it and `ManaCostDSL::new`
rejects the first invalid symbol with the existing `ManaParseError::UnknownSymbol { offset,
symbol }`, `offset` being its byte offset in the cost's canonical text. The parser's
`symbol_from_inner` no longer allocates (it takes at most four `/`-parts from the iterator).
**Rationale.** A constructed cost must print as text the loader accepts. Reusing `UnknownSymbol`
keeps `ManaParseError` unchanged (the spec tests match it exhaustively) and is what the parser
reports for the same text. One shared predicate means parser and constructor cannot disagree
(an equivalence unit test covers a spread of symbols).
**Alternatives.** A new error variant (rejected: public API change); a private check duplicated
in `new` (rejected: could drift from the parser).

## #11 FP-010 — seal rejects unsupported dslVersion

**Decision.** `CardDefinitionDSL::seal` calls `check_dsl_version` first and maps its error to
`CanonicalError::Serialize(e.to_string())`. Supported versions (`1.x[.y]`) seal as before. Seal and
`compute_hash` also ignore the previous `definitionHash` value (cleared before validation).
**Rationale.** Every `Ok` sealed value must round-trip through `from_json`, which rejects other
majors. The signature and the error enums are frozen (exhaustive matches in spec tests); the error
text carries the `DslVersionError` message.
**Alternatives.** Returning `CardDefinitionError` (breaking signature change, R2); a new
`CanonicalError` variant (breaks exhaustive matches).

## #11 FP-010 — Test-path ownership

**Decision.** The issue's owned paths (src/card, src/mana, src/canonical) are the Implementer's.
Independent spec tests under crates/fineprint-dsl/tests/spec_dsl_*.rs (including
spec_dsl_review1.rs) are owned by the Independent Tester role, per docs/DEVELOPMENT.md (Tests:
crates/<crate>/tests/spec_<area>_*.rs); every classifier brief for this unit names them as the
Tester's owned paths and lists them as DO NOT TOUCH for the Implementer and Fixer. They are not an
extension of the issue's owned paths.
**Rationale.** R1 limits each role to the paths in its brief; CodeRabbit flagged the new file
against the issue's list only.
**Alternatives.** Move the tests into src/ (rejected: the Tester must stay independent of src/,
R9).

## #11 FP-010 — compute_hash hashes the borrowed definition

**Decision.** `compute_hash` serialises `&self` through a crate-private checked-serializer entry
point, `canonical::to_checked_value_without_member(value, "definitionHash")`, which leaves the
named member out of the outermost struct or map before its value is serialised or checked (a
repeated skipped member counts as absent; members of that name at any deeper level are kept and
checked). The result is hashed exactly as before (`hash_without_member`: RFC 8785 bytes, SHA-256,
lowercase hex), so every valid card keeps its digest. No clone of the definition or any `Value`
remains on the hash path. This supersedes the "cleared before validation" mechanism of the entry
"seal rejects unsupported dslVersion" for `compute_hash`; `seal` still clears the field it owns.
**Rationale.** Cloning recursed through placeholder `Value`s without a depth bound, so an
in-memory value nested past `MAX_DEPTH` could overflow the stack before the checked serializer's
depth check returned `CanonicalError::Serialize`. Skipping the member before serialisation keeps
a stale hash (noncharacter, wrong length, anything) out of both the hash and the checks
(REQ-DSL-002, REQ-NFR-006, REQ-CORE-006).
**Alternatives.** Clone and clear the field (rejected: unbounded `Clone` recursion before the depth
check). Serialise `self` unchanged and remove the member afterwards (rejected: a stale hash with a
noncharacter would fail the check). Note: `serde_json::Value`'s `Drop` is also recursive, so an
erroring by-value `seal` still drops a deep value; out of scope (no custom `Drop`).

## #12 FP-011 — Spec-test readings of the selector and `$var` rules

**Decision.** The Independent Tester's spec tests read two contract clauses as follows.
(1) A filter selector whose flat predicate is `SAME_AS` (`{"pred":"SAME_AS","ref":"TARGET","slot":0}`)
is the filter form: it has a predicate, so the `ref`/`slot` members belong to the predicate and
the value round-trips exactly (`req_dsl_003_selector_with_flat_same_as`). (2) A `BOUND` var must
match `$` followed by one or more of `A-Z a-z 0-9 _` over the whole string, so `$$a`, `a$b` and
`$a-b` are rejected.
**Rationale.** (1) The wire contract defines the reference form as a ReferenceAST with no other
member and lets any PredicateAST sit flat in a filter selector; only the filter reading is
valid, and G5 requires every valid document to round-trip. (2) G3 states the full shape of the
string; any other reading would allow several spellings of characters outside the class.
**Alternatives.** Treat any object with `ref` as the reference form (rejected: it would reject a
valid filter selector). Accept a `$var` prefix match (rejected: G3 gives the whole shape).

## #12 FP-011 — Wire catalogue as implemented

**Decision.** The JSON of `fineprint_dsl::expr`, `text` and `choice` is exactly the FP-011 wire
contract of the brief (G1–G6). Rust shapes chosen where the contract names only the JSON:
`ReferenceAST::{SelfRef, …, Target{slot: u32}, Chosen{key}, Bound{var}, Linked{link_id},
Remembered{key}}`; `PlayerSelectorAST::{Ref, Players(PlayerGroup)}`;
`ObjectSelectorAST::{Ref(ReferenceAST), Filter(SelectorFilter{zones, count, ordering,
predicate})}` with `Ordering{by: OrderKey, direction: Direction}`; `PredicateAST` has one variant
per `pred`/`op` name (`HasCounter{kind, compare: Option<(Cmp, Box<Value>)>}`,
`SameAs{reference}`, `Controls{selector, player}`); `ValueExpressionAST::{Literal(i64), X,
Count, Attr{reference, attr}, PlayerAttr{reference, attr}, Math{op, lhs, rhs}, Distinct{attr,
selector}, History(Box<HistoryQuery>), Memory(String)}`; the JSON member `where` of
`HistoryQuery` is the Rust field `condition`. `TextToken::{Literal{id, text}, Word{id, category,
value}, NameRef{id, kind: NameRefKind, name: Option<String>}}` with `NameRefKind::{SelfRef,
Other}` (strings `SELF`/`OTHER`); `ChoiceSpec` has public fields plus `ChoiceSpec::new(chooser,
kind)` and `ChoiceSpec::validate()`. Every string enum has `ALL`, `as_str` and `from_wire`.
**Rationale.** One variant per wire name keeps `decode` injective and lets `match` be
exhaustive; a fieldless `NameRefKind` mirrors the `ref` member, and the SELF-without-name /
OTHER-with-name rule is enforced by `TextTokenStream::new` and by Deserialize (`TextError`).
**Alternatives.** `NameRefKind::Other(String)` (rejected: the `kind` name implies a fieldless
selector and the standalone JSON of the enum would be unclear); derived serde with
`#[serde(flatten)]` (rejected: flatten buffers through a map that accepts floats and duplicate
keys, and cannot express the flat-ref and flat-predicate styles strictly).

## #12 FP-011 — Strict decoding through a depth-checked buffer

**Decision.** Every Deserialize in the three modules first buffers its input into a
`serde_json::Value` with a private `DeserializeSeed` that counts open containers and fails above
`canonical::MAX_DEPTH` (127) before recursing, rejects duplicate member names, every
non-integer number (`1.0`, `1e2`, `-0`, `-0.0` all arrive as floats), integers outside
±(2^53 − 1), and noncharacters in member names and strings. Hand-written conversion then
rejects unknown, missing and `null` optional members. Serialize writes directly (no
`to_value`), so `canonical::to_canonical_vec` (which checks depth) also bounds export; a plain
`serde_json::to_vec` does not check nesting depth, so a hand-built value deeper than 127 can be
written but will not decode, and callers who need output guaranteed to reload must use
`to_canonical_vec`. Serialize re-checks (value-level only)
the invariants a hand-built value can break (empty strings, bad `$var`, empty AND/OR or
DEVOTION, empty or repeated zones, a filter with neither zones nor a predicate, an invalid
token, an invalid ChoiceSpec, a LITERAL outside the safe range), failing instead of writing
JSON that would not decode.
**Rationale.** G4–G6 and R12 must hold for every deserializer, not only `serde_json::from_str`
(whose own limit is also 127): `serde_json::Value` as a deserializer has no depth limit. This
holds with two caveats. The depth limit and the duplicate-name check act while JSON is read, so
untrusted JSON must be decoded directly from bytes/str/reader into the schema type; once parsed
into a `serde_json::Value`, duplicate names have already collapsed to one entry and the typed
decoder cannot see them. And for an in-memory `Value` the caller must bound its depth before
building or handing over an owned value, because a very deep owned `Value` can overflow the
stack in `serde_json`'s own recursive drop before this crate returns its error; prefer
deserialising from a borrowed `&Value`. The
noncharacter rule is the same as `canonical`'s, so anything that decodes can be exported
(`to_canonical_vec(&decode(J))` is the RFC 8785 form of J).
**Alternatives.** Derive with `deny_unknown_fields` (rejected: cannot express the flat styles
nor reject floats in `i64` positions uniformly across deserializers); accept noncharacters
(rejected: the decoded value could then fail to export).

## #12 FP-011 — FP-010 placeholders stay in card/

**Decision.** The two `serde(transparent)` placeholders are left untouched, and their shapes
differ: `card::ValueExpressionAST(pub Value)` accepts any JSON value, and
`card::TextTokenStream(pub Vec<Value>)` accepts a JSON array whose elements are any JSON value.
The real types (`expr::ValueExpressionAST`, `text::TextTokenStream`) are stricter than either.
Swapping the card definition over to them is follow-up issue #159 (FP-140)'s work, not this
one's.
**Rationale.** The brief limits this unit to the new `expr`, `text`, `choice` directories plus
three `pub mod` lines; changing `card/` would change FP-010's definition hash inputs and tests.
**Alternatives.** Re-export the new types from `card` now (rejected: outside owned paths).

## #12 FP-011 — SRS shorthand is not a wire form

**Decision.** SRS shorthand and inline spellings (`TARGET(n)`, `CHOSEN(key)`, `BOUND($var)`,
an AST embedding a rewritable word's literal value) are not accepted. References are
`{"ref":…}` objects, and a rewritable word is referenced by token id
(`{"textToken":"t14"}`) or given as a fixed `{"name":…}` (WordRef).
**Rationale.** SRS §14.5 requires by-id references to rewritable words; G5 requires exactly
one spelling per value.
**Alternatives.** Accept the call-style strings as aliases (rejected: two spellings per value).

## #12 FP-011 — Choices where the brief is silent

**Decision.** (1) Selector form detection: an object with `pred` or `op` is the filter form
(even with `ref`, which then belongs to a flat `SAME_AS`); otherwise an object with `ref` is the
reference form and any other member is rejected. (2) `WordRef` with both `textToken` and `name`
is rejected. (3) `ENUM` candidates are free-form strings: non-empty, distinct, order kept.
(4) `ChoiceSpec` bounds that are not both `LITERAL` are not compared (the toolchain's job).
(5) A `TextToken` decoded on its own runs the same per-token checks as the stream (everything
but id uniqueness). (6) `TextError::Noncharacter` reports the token id, or the position for the
id itself. (7) The `X` expression serialises as the bare string `"X"`; `{"X":…}` is rejected.
**Rationale.** Each choice keeps decode injective and lossless (G5) and matches the SRS
examples (`{"zones":["BATTLEFIELD"],"pred":"IS_CREATURE"}`, `{"ref":"TARGET","slot":0}`).
**Alternatives.** Treat any `ref` member as the reference form (rejected: a valid flat
`SAME_AS` filter would be refused).

## #13 FP-012 — Wire catalogue as implemented

**Decision.** `fineprint_dsl::ability` implements the FP-012 wire contract of the issue #13
brief (W1–W7, E5, sections A–J) on top of the FP-011 strict plumbing. Judgement calls fixed by
that contract and implemented as written: `StepId` has 13 values (UNTAP … CLEANUP) and
`ActivationTiming` 5 (INSTANT, SORCERY, YOUR_TURN, OPPONENT_TURN, COMBAT_ONLY; absent means
INSTANT); `TargetSlotAST` is `{slot,selector,min,max}` (min <= max, slots distinct within one
array) and `ModeSetAST` is `{min,max,modes}` (ids distinct, min <= max <= modes) with
`ModeAST {id,targets?,effect}`; `EventKind` (57) and `RuleKind` (34) are closed Rust enums that
grow only by a DSL minor version; `FilterValue` is decided by JSON type (bool, safe integer,
`"ANY"`, a zone name, any other non-empty string as `Name`, an object as a predicate), so a
`Name` equal to `"ANY"` or a zone name is refused by `FilterValue::name` and by Serialize;
`CHOOSE {choice: ChoiceSpec, store}` is the fifth `ReplacementOp`; `Modification` is
`{op, …flat opaque params}` with typed `power`/`toughness` (`PtValue`: a JSON integer or a
value-expression object, never the bare `"X"`) only for `SET_BASE_PT` and `MODIFY_PT`, and
`RuleModification` is `{kind, cost?, …flat opaque params}` where only `cost` is typed (a
non-empty `CostAST` array); the 613.6 lock-in cross-check (every CHAR `{"sameAsLayer":L}` needs
another CHAR effect of the same ability in layer L whose `affected` is a selector, so dangling,
self and chained lock-in fail on decode and encode); rule E5 on `filters`/`bind` keys;
`functionsIn` optional (absent = toolchain default). Rust shapes chosen where the brief names
only JSON: each ability struct holds `base: AbilityBase`; `ReplacementAbilityAST::matcher` is
JSON `match` and `overrides_614_5` is `overrides614_5`; `CostAST::ExileFromZone` holds
`count` and `min_total_mv` as two options with an exactly-one check; `ReplacementOp::Choose`
boxes its `ChoiceSpec` (clippy `large_enum_variant`); two public types beyond the catalogue,
`PtValue` and `ShieldUses` (`Count(u32 >= 1)` | `CounterBacked`). The standalone JSON of each
ability struct is the whole ability object with its own `kind`, and of each static-effect
struct the effect object with its own `type`. Helper API: `AbilityAST::{KINDS, kind(), base()}`,
`AbilityBase::new`, `CostAST::{NAMES, name()}`, `ReplacementOp::name`, `EventMatcher::new`,
`Modification::new`, `RuleModification::new`, `EffectInstructionAST::new`,
`KeywordInstanceAST::new`, `FilterValue::name`, `From<PlayerGroup> for EventSubject`.
**Rationale.** The brief fixes the wire forms so the Independent Tester can compile against
them; the Rust shapes keep decode injective (one spelling per value) and let Serialize re-check
every value-level invariant (W3).
**Alternatives.** An `ExileAmount` enum (rejected: one more public type for one node);
`Expr(X)` accepted as a power (rejected: the brief says "a ValueExpressionAST object").

## #13 FP-012 — EffectInstructionAST is a placeholder

**Decision.** `EffectInstructionAST` lives in `ability/` as a strict placeholder: required `op`
matching `[A-Z][A-Z0-9_]*`, any other members stored as opaque JSON (`BTreeMap<String,
Value>`, non-empty names, no null anywhere, integers only), written back unchanged. FP-013
(issue #14, owner of `src/effect/`) supplies the real type; swapping it in edits `ability/`
and is follow-up work together with #159.
**Rationale.** Abilities need an instruction list now; the real instruction schema is another
unit's contract. Same pattern as "#11 FP-010 — Placeholder AST types".
**Alternatives.** Define the instruction schema here (rejected: outside owned paths and
another unit's contract).

## #13 FP-012 — Wiring edit

**Decision.** The only edit outside `src/ability/` is the single `pub mod ability;` line (with
its doc comment) in `crates/fineprint-dsl/src/lib.rs`.
**Rationale.** As in "#11 FP-010 — Wiring edits outside the owned directories": the module must
be declared to compile, and the brief lists this line as owned.
**Alternatives.** None that compile.

## #13 FP-012 — Test-path ownership

**Decision.** The Independent spec tests live in `crates/fineprint-dsl/tests/spec_dsl_ability*.rs`
and belong to the Tester role; they are not an extension of the issue's owned paths for the
Implementer, whose unit tests are `src/ability/tests.rs` and `src/ability/tests/props.rs`.
**Rationale.** As in "#11 FP-010 — Test-path ownership": independent tests are written from the
brief alone.
**Alternatives.** Implementer-owned integration tests (rejected: no independence).

## #13 FP-012 — Rule E5 covers filters and bind keys only

**Decision.** Rule E5 (a key whose lower-cased form without `_`/`-` starts with `lookback` is an
error) applies to `filters` keys of `EventMatcher` and `ReplacementMatch` and to `bind` keys,
on decode and encode. It does not apply to the flat members of the opaque placeholders
(`EffectInstructionAST` members, `Modification` and `RULE` params, keyword `params`) nor to
`MODIFY_EVENT.set` keys: those are opaque by contract (brief sections H, I, J), so a member
named `lookBack` there is plain data and round-trips unchanged. Every typed node (ability,
trigger, matcher, replacement match, CHAR and PLAYER effects) rejects such a member as unknown
(W1), so no look-back flag can be set on a trigger.
**Rationale.** The brief scopes E5 to `filters` and `bind`, and defines the placeholder
members as "any other members, each an opaque JSON value"; the Independent Tester's spec test
`req_dsl_006_look_back_name_inside_effect_instruction_is_opaque` reads it the same way. An
earlier draft extended E5 to opaque members; it was withdrawn as contrary to H/I.
**Alternatives.** Apply E5 to every flat opaque member name (rejected: contradicts the opaque
contract of H and I, and the instruction schema of FP-013 will be typed anyway).

## #13 FP-012 — Smaller choices where the brief is silent

**Decision.** (1) Opaque values are re-checked on encode (no null, integers within the safe
range, no noncharacters, nesting bounded by `MAX_DEPTH`). The nesting check is relative to the
opaque value itself, not to the complete document: a value that passes it can still sit deep
enough in an ability that the whole document exceeds `MAX_DEPTH`, so output of
`serde_json::to_vec` is not guaranteed to reload as `AbilityAST`. Callers that need reloadable
output use `canonical::to_canonical_vec`, which checks the complete document. Inside opaque values, empty member names and empty
strings are allowed; only the flat member names must be non-empty. (2) `EventSubject` is the
`Players` variant exactly when the object has a `players` member; otherwise it is decoded as an
`ObjectSelectorAST`. `Affected` is `SameAsLayer` exactly when the object has `sameAsLayer`.
(3) `bind` values need not be distinct; mode-level target slots are checked for distinctness
within their own array only. (4) `sameAsLayer` does not have to name an earlier layer than the
effect's own; the toolchain owns layer-order checks. (5) `ShieldUses::Count` is a `u32` (W7).
(6) A `MODIFY_EVENT.set` value has the same `FilterValue` form as a filter.
**Rationale.** Each keeps decode strict, injective and lossless (W1, W4, W5) without adding
rules the SRS does not state.
**Alternatives.** Reject empty names inside opaque values (rejected: they are valid JSON and
the brief says "any JSON value containing no null").

## #13 FP-012 — Spec-test readings of the ability wire contract

**Decision.** The Independent Tester's spec tests read the brief's open points as follows.
(1) Rule E5 (no look-back keys) is asserted only on `EventMatcher` `filters` and `bind`; the
`ReplacementMatch.filters` and `MODIFY_EVENT.set` keys are not tested either way. (2) The 613.6
lock-in self-reference check is tested only when no other effect sits at the referenced layer.
(3) A player group (`{"players":...}`) is not used as a `TargetSlotAST` selector. (4) An empty
keyword `params` object is left untested. (5) A bare `"X"` for `SET_BASE_PT`/`MODIFY_PT` power
or toughness is rejected (the brief requires an integer or an object). (6) Non-integer numbers
are checked through `from_str`/`from_slice`; nesting limits also through `from_value`. (7) Unsafe
integers inside opaque values are rejected under W2.
**Rationale.** Each reading is the narrowest one the brief supports; the untested points are
left for the units that give them meaning (FP-013 effects, the replacement bus).
**Alternatives.** Assert behaviour the brief does not state (rejected: R10, tests assert only
what the brief requires).

## #13 FP-012 — SRS §14.4 examples written in shorthand

**Decision.** Three published SRS §14.4 examples use four spellings in all that the FP-011 wire contract rejects
(decision "#12 FP-011 — SRS shorthand is not a wire form"), and are tested in adapted form:
(1) the activated-ability example (SRS §14.4.2) writes the subtype as
`{"textToken":"CREATURE_TYPE","value":"…"}`, which `WordRef` rejects (a rewritable word is a token
id `{"textToken":"t14"}` or a fixed `{"name":…}`, SRS §14.5); EX3 uses `{"name":"TestType"}`.
(2) The max-speed example (SRS §14.4.3) writes `"player":"CONTROLLER"` and
`"affected":{"selector":"OTHER_CREATURES_YOU_CONTROL"}`; `PlayerSelectorAST` needs
`{"ref":"CONTROLLER"}` and there is no named-selector form, so EX8 spells the selector out as a
filter. (3) The "as this enters, choose a creature type" example (SRS §14.4.4) writes
`"choice":"CREATURE_TYPE","chooser":"CONTROLLER"` beside `op`; `ChoiceSpec` is an object, so
EX10 nests `{"chooser":{"ref":"CONTROLLER"},"kind":"CREATURE_TYPE"}` in `choice`. Spec tests pin
that the verbatim spellings are rejected, so the gap stays visible. The proposed SRS edit is a
PROCESS.md P8 line in `docs/HUMAN_QUEUE.md`. The issue's acceptance criterion "every SRS
§14.4.1–14.4.4 JSON example parses" is read as "every example, in the FP-011 wire form".
**Rationale.** SRS §14.5 requires by-id word references and the wire contract requires one
spelling per value (G5); accepting the shorthand as aliases would break both. The SRS examples
are illustrative JSON, and the SRS text around them (§14.5) is the normative rule.
**Alternatives.** Accept the shorthand as aliases in the decoder (rejected: two spellings per
value, and a change to the FP-011 contract owned by #12); leave the examples untested
(rejected: the acceptance criterion would have no evidence).

## #14 FP-013 — Spec-test readings of the effect wire contract

**Decision.** The independent spec tests (`crates/fineprint-dsl/tests/spec_dsl_effect*.rs`) read
the FP-013 brief as follows, and assert only these readings. (1) "Wrong member type" uses values
of a different JSON type from the valid one (a number, a bool, an array, an object) and, for number
and bool members, the same value written as a string; arbitrary strings are not used because STR
members, VAL (`"X"`) and SET_DESIGNATION `value` accept strings. (2) `PlayerSelectorAST` has
no `EACH_OPPONENT` group: the brief's "EACH_OPPONENT" is the reference `{"ref":"EACH_OPPONENT"}`.
(3) The A8 enums' `ALL` is iterated by reference and only `as_str`, the JSON form and the order
are asserted; the return type of `from_wire` is not fixed by the brief and is not tested.
(4) `NativeEffectAST` decoded standalone is tested only with `"op":"NATIVE_EFFECT"` present; whether
a standalone document without `op` is rejected is not stated and is not tested. (5) A `-0`
spelling is checked only through `from_str`/`from_slice`, since a `serde_json::Value` cannot
carry it faithfully; `1.0`, `1e2` and unsafe integers are checked through all three entry
points. (6) The nesting budget counts open containers like the FP-012 tests: a document of
exactly 127 decodes, 128 is rejected. The at-budget test lives in its own test binary
(`spec_dsl_effect_depth.rs`) so a stack overflow cannot hide the other results. (7) The
Serialize re-checks of CHOOSE_MODE also cover max > mode count, duplicate and empty mode ids,
empty mode effect and empty mode targets, as value-level invariants under A2. (8) No 613.6
`sameAsLayer` value is used in CREATE_CONTINUOUS_EFFECT, as the brief excludes that check.
REMEMBER's Rust shape and `RememberSource` are tested through JSON only.
**Rationale.** Each is the narrowest reading the brief supports (R10, and the FP-012 decision
"Spec-test readings of the ability wire contract").
**Alternatives.** Assert the unstated points: standalone NATIVE_EFFECT without `op`, the
`from_wire` signature, the REMEMBER field layout (rejected: the brief does not fix them).

## #14 FP-013 — Wire catalogue as implemented

**Decision.** `fineprint_dsl::effect::Instruction` implements the 72-op FP-013 catalogue exactly as
the brief's section C lists it, in that order (`Instruction::NAMES`). Where SRS §14.4.5 names an op
without members or with `…`, the members are the brief's: e.g. `PREVENT{duration, source?,
recipients?, amount?, combatOnly?}`, `SET_DESIGNATION{kind, value, to?}`, `FIGHT{first, second}`,
`BITE{source, target}`, `VOTE{voters, choice, store}`. Further readings: `ifActionTaken`
(REFLEXIVE_TRIGGER, 603.12) is a non-empty instruction list, not a predicate; REMEMBER has a
required `into` (`MEMORY` | `LINKED`) and exactly one of `value`/`objects`; `Position` is
`TOP` | `BOTTOM` only; `modifiers` and `except` are `Option<BTreeMap<String, Value>>` (non-empty
opaque objects); CHOOSE_MODE `min`/`max` are `u32` with `min <= max <= modes.len()` (so `0,0` is
legal). Standalone JSON of the helper types: `TokenSource` is `{"tokenDef":…}` or
`{"inline":{…}}`, `RememberSource` is `{"value":…}` or `{"objects":…}` (exactly one member), and
`DesignationValue` is a JSON boolean or a non-empty string. `Instruction::locks_affected_set()`
returns `true` for CREATE_CONTINUOUS_EFFECT with `lockAffectedSet` absent or `true` (611.2c),
`false` when it is `false`, and `false` for every other op (they create no continuous effect).
Opaque values are checked with the container holding them at depth 1 (a `params` member value
starts at depth 2), the same counting as the ability module.
**Rationale.** These are the narrowest readings consistent with the brief and SRS §14.4.5; the
standalone forms of the helper types are the flat members they contribute to their instruction,
so one spelling exists per value.
**Alternatives.** Type `ifActionTaken` as a predicate (rejected: the brief fixes INSTRS); let
`locks_affected_set` return `Option<bool>` (rejected: the brief fixes `-> bool`); make the helper
types unit-less newtypes without a standalone JSON form (rejected: section F requires
Serialize/Deserialize on every type).

## #14 FP-013 — Ability placeholder stays

**Decision.** `ability::EffectInstructionAST` is not replaced in this unit; the real instruction
type is `effect::Instruction`. Swapping the placeholder into `SpellAbilityAST`,
`ActivatedAbilityAST`, `TriggeredAbilityAST`, `ModeAST` and the replacement ops is follow-up work
(together with #159).
**Rationale.** The swap edits `src/ability/` (outside this unit's owned paths) and would break the
#13 spec tests, which accept free-form `op` names in effect lists.
**Alternatives.** Swap now (rejected: R1 owned paths, and it breaks merged #13 spec tests).

## #14 FP-013 — Wiring edit

**Decision.** The only edit outside `crates/fineprint-dsl/src/effect/` is the single
`pub mod effect;` line with its `///` doc comment in `crates/fineprint-dsl/src/lib.rs`, after the
`ability` module; there are no re-exports.
**Rationale.** Same pattern as "#13 FP-012 — Wiring edit"; the module must be declared to compile.
**Grant.** The work-classifier's Implementer brief for #14 lists `crates/fineprint-dsl/src/lib.rs`
(this one line only) under OWNED PATHS. Issue #14's "Owned paths" section now names it too: the
maintainer approved that amendment during PR #163 review.
**Alternatives.** Re-export the types at the crate root (rejected: not asked for, widens the diff).

## #14 FP-013 — Test-path ownership

**Decision.** The independent spec tests of this unit live in
`crates/fineprint-dsl/tests/spec_dsl_effect*.rs` and belong to the Tester role; the Implementer's
unit tests live in `src/effect/tests.rs` and `src/effect/tests/props.rs`.
**Rationale.** Same split as "#13 FP-012 — Test-path ownership" (P5 role separation).
**Alternatives.** None considered.

## #14 FP-013 — No lock-in cross-check inside CREATE_CONTINUOUS_EFFECT

**Decision.** CREATE_CONTINUOUS_EFFECT does not run the 613.6 `sameAsLayer` cross-check that a
STATIC ability runs: a CHAR effect with a dangling `{"sameAsLayer": L}` is accepted in this node.
The toolchain owns that check for instruction-created effects.
**Rationale.** `ability`'s `check_lock_in` is private to `src/ability/` (not an owned path), and
the brief assigns the check to the toolchain. Each STATIC effect still checks itself.
**Alternatives.** Duplicate the lock-in logic in `effect/` (rejected: two copies of one rule).

## #14 FP-013 — SRS §14.4.5 tail

**Decision.** "and so on" and the sentence "Every CR 701 keyword action has exactly one
instruction or macro" are read as: the instruction set is closed (exactly the 72 ops, unknown
`op` is an error) and grows only by a DSL minor version; completeness against CR 701 is a
toolchain check, not part of this unit.
**Rationale.** A closed set gives one spelling per value and strict decoding; the CR 701 list is
not in the brief, and rules content is never written from memory.
**Alternatives.** Accept unknown ops as opaque (rejected: the brief's rule A6).

## #14 FP-013 — Small stack frames on the instruction recursion path

**Decision.** Decoding and encoding keep the nine list-holding ops (CREATE_DELAYED_TRIGGER,
REFLEXIVE_TRIGGER, IF, FOR_EACH, REPEAT, REPEAT_WHILE, MAY, UNLESS, CHOOSE_MODE) in small
functions of their own, apart from the large flat match, and decode lists with a plain loop.
**Rationale.** With one large match, a depth-127 document (63 nested MAYs) overflowed the 2 MiB
stack of a test thread in an unoptimised build; after the split, the deepest legal tree decodes
and encodes within 512 KiB in a debug build (measured), so `MAX_DEPTH` input is safe.
**Alternatives.** Lower the nesting limit for instructions (rejected: changes the FP-011 depth
contract); spawn a large-stack thread in the decoder (rejected: threads in rules code, R3).

## #14 FP-013 — lockAffectedSet:false rejected for CHAR effects

**Decision.** `CREATE_CONTINUOUS_EFFECT` with `lockAffectedSet:false` is rejected, on decode and on
encode, when any effect is a CHAR effect (every layer, layer 2 control included). The member stays
optional: absent and `true` are accepted for every effect mix, and `false` is accepted only for
RULE/PLAYER-only effects. `Instruction::locks_affected_set()` returns true for any instruction with
a CHAR effect. Dropping the member is a possible later change.

**Rationale.** Per CR 611.2c the set of objects a resolving characteristic- or control-changing
effect affects is determined when it begins and never changes; lock status is per part, so a
RULE/PLAYER-only effect may carry `false` harmlessly. CR 611.3a ("isn't locked in") concerns static
abilities, which use their own envelope, so it does not apply to a resolving instruction. SRS
§14.4.5 is silent on `false`, so this fills an omission and does not contradict it.

**Alternatives.** Keep accepting `false` and make the helper return true for CHAR: rejected, the
wire value would still misstate the rule. Drop the member: deferred, it is a wire-contract change.

## Repository — Trademark notice replaces the Fan Content Policy disclaimer

**Decision.** Made by the maintainer on 2026-11-18. Fine Print's README, package metadata and
documentation carry a trademark and non-affiliation notice instead of the Fan Content Policy
disclaimer (SRS §1.3 item 3, REQ-ARCH-003 item 2, REQ-NFR-012, PROCESS.md R5). The policy's
disclaimer becomes the responsibility of clients that import card data or build or distribute a
card database. REQ-ARCH-001 (zero hardcoded IP) and REQ-NFR-007 (CR text never committed) are
unchanged, and so are the references to the game and to the CR.
**Rationale.** The Fan Content Policy is a permission to use Wizards of the Coast IP, so its
disclaimer is a condition of that permission. Fine Print contains and distributes none of that IP,
apart from card names the SRS uses as interaction examples (REQ-NFR-004). It names the game only
descriptively and cites rule numbers; it does not commit the CR text, which CI fetches into the
git-ignored `cr-cache/` and its build cache (only rule numbers and hashes are committed, in
`docs/cr-citations.json`), and design notes quote at most a few words of a rule. Two kinds of
Wizards of the Coast material are involved. The CR text (Architecture A7) is fetched by CI only to
check citations, and is neither committed nor distributed; reading it to build a compatible engine
does not rely on the policy's permission. Card data enters only through clients, under Scryfall's
terms (which require the policy) when it comes from Scryfall, so the policy's obligations sit with
those clients. The maintainer made this decision knowing that CI fetches the CR text. This is not
legal advice.
**Alternatives.** Keep the disclaimer: rejected, because Fine Print uses none of the policy's
permission. Scrub every reference to the game and stop fetching the CR: rejected. The CR is the
specification, the citation check needs the text, and removing the game's name would make the
documents harder to use without changing the legal position.

## #15 FP-014 — RulesVocabulary wire shape

**Decision.** The wire shape is the brief's, unchanged in its member names: `vocabularyVersion`
(1), `crEffectiveDate`, `cardTypes`, `supertypes`, `subtypeGroups` (`group`, `cardTypes`,
`ruleRef`, `subtypes`), `keywordAbilities` / `keywordActions` (`id`, `name`, `ruleRef`),
`counterKinds` (`kind`, `class`, `ruleRef`) and `predefinedTokens` (`letter`, `ruleRef`, `name`,
`faces` with `name`, `colors`, `supertypes`, `cardTypes`, `subtypes`, `power`, `toughness`,
`abilities`). Every list keeps CR text order (so the creature list starts with the two-word type
the CR names first). Token `colors` are stored in white, blue, black, red, green order (a set, not
a CR list). A group's `group` is the word the CR uses in "called ... types": for CR 205.3n that
is `planar`, not the brief's example `plane`. The counter kind of CR 122.1a is the literal
`+X/+Y` (class `POWER_TOUGHNESS`), standing for the whole `+X/+Y` / `-X/-Y` family the rule
describes. Classes other than `KEYWORD` and `POWER_TOUGHNESS` are the keyword-id slug of the kind
(`SHIELD`, `STUN`, `LOYALTY`, `POISON`, `DEFENSE`, `FINALITY`, `RAD`, `HONE`), so a new CR 122.1
sub-rule needs no code change. `to_canonical_bytes` returns `Result<Vec<u8>, VocabError>` rather
than `Vec<u8>`: a hand-built value can hold an integer outside the RFC 8785 safe range, and the
function must not panic. `bundled()` returns `Result<&'static RulesVocabulary, VocabError>`,
memoised in a `OnceLock` whose value depends only on the embedded bytes. The loader also checks
cross-references (group and face card types are card types, face supertypes are supertypes, a
face's subtypes belong to its card types), a calendar-valid date, and power and toughness both
present or both null.
**Rationale.** Field names are fixed by the brief. `planar` is what the text says and is derived
structurally; mapping it to `plane` would need a hard-coded table. A slug class keeps counter
classes data-driven. Returning `Result` keeps the no-panic rule for library code.
**Alternatives.** A fixed class enum (rejected: a new counter rule would need an engine change);
`group` derived from the card type (rejected: the spell group has no single card type);
panicking `to_canonical_bytes` (rejected).

## #15 FP-014 — Keyword id slug rule

**Decision.** `id` = the heading name with apostrophes (`'`, U+2019, U+2018, U+02BC) removed,
Latin-1 letters with diacritics folded to their base letters (`é` to `E`, `Æ` to `AE`, `ß` to
`SS`), every other character that is not an ASCII letter or digit treated as a separator,
separator runs collapsed to one `_`, leading and trailing separators dropped, letters
upper-cased. `Double Strike` gives `DOUBLE_STRIKE`, `For Mirrodin!` gives `FOR_MIRRODIN`, and
the heading `∞ (Infinity)` gives `INFINITY`. An empty result is an input error (exit 2); an id
collision within 701, within 702 or between them is exit 1. A keyword is an unlettered
`701.N` / `702.N` rule with N >= 2 whose text is heading-shaped (one line, at most 8 words and 64
characters, no `.` at the end or `. ` inside); 701.1 and 702.1 are skipped as introductions, and
any other prose-shaped rule in that position is an input error rather than a silently skipped or
invented keyword. In the 2026-11-06 text only 701.1 and 702.1 are prose; 71 actions and 195
abilities result, with no collision.
**Rationale.** Matches the SRS macro ids (`FLYING`, `MENACE`, `VIGILANCE`, `TRAMPLE`) and keeps
ids ASCII. Failing on unexpected prose keeps a CR layout change visible.
**Alternatives.** Skipping prose headings (rejected: silent partial output); full Unicode
transliteration (rejected: needs tables, and no heading needs it).

## #15 FP-014 — Token abilities are CR 111.10 token characteristics (R5)

**Decision.** Each token face's `abilities` lists, in order, the quoted ability texts of its
CR 111.10 definition without the quotation marks (a trailing comma the quoting style puts inside
the closing mark is dropped) and the unquoted keyword phrases the definition gives (for example
`enchant creature`, `indestructible`). The framing sentence, cross-reference sentences ("See
rule ...") and any other prose are not stored. These strings are committed in `vocab.json`.
**Rationale.** PROCESS.md R5 allows "Rules vocabulary the CR itself defines (keywords, CR 111.10
tokens, counter kinds, designations)" and SRS §1 item 1 allows "predefined token definitions from
CR 111.10"; a token's abilities are part of its definition (SRS §10.1 quotes one). A test bounds
every other string to 48 characters and allows rule numbers only in `ruleRef`.
**Alternatives.** Storing ability ids only (rejected: there is no ability catalogue yet, and the
engine needs the definition).

## #15 FP-014 — Card types and supertypes are capitalised

**Decision.** `cardTypes`, `supertypes` and the token faces' type lists store card types and
supertypes with a leading capital (`Artifact`, `Legendary`); the CR lists them in lower case.
Subtypes, keyword names and counter kinds keep the CR's exact spelling and Unicode (curly
apostrophes, hyphens, `Urza’s`, `Time Lord`, `The Abyss`), not normalised.
**Rationale.** SRS §4.1 spells them capitalised, and lookups are exact-match.
**Alternatives.** Lower case as in the CR (rejected: every consumer would re-capitalise).

## #15 FP-014 — Wiring edit and path grant

**Decision.** Besides the owned `crates/fineprint-dsl/src/vocab/`,
`crates/fineprint-ingest/src/vocab/` and `crates/fineprint-dsl/data/vocab.json`, the unit edits
only what the brief grants: one `pub mod vocab;` line with its doc comment in each of
`crates/fineprint-dsl/src/lib.rs` and `crates/fineprint-ingest/src/lib.rs`; the new binary
`crates/fineprint-ingest/src/main.rs` (subcommand dispatch); `crates/fineprint-ingest/Cargo.toml`
(`serde_json` from the workspace pins and a `[[bin]]` entry) and the resulting `Cargo.lock`
entry; the two spec test files; `.github/workflows/cr.yml`; `docs/DEVELOPMENT.md`; this file.
**Rationale.** As in "#14 FP-013 — Wiring edit": the modules must be declared to compile, and
the generator needs a binary and a CI step.
**Alternatives.** None that deliver the deliverables.

## #15 FP-014 — Committed file location; generator in fineprint-ingest, not xtask

**Decision.** The vocabulary is committed as `crates/fineprint-dsl/data/vocab.json` and embedded
with `include_bytes!`, so the rules crate loads it with no runtime I/O. The generator lives in
`fineprint-ingest` (`fineprint-ingest vocab`), not in xtask. Its default `--out` is that path
relative to the current directory (every documented command runs from the repository root).
**Rationale.** The generator must build the `RulesVocabulary` type and use the dsl canonical
serialiser, so it depends on `fineprint-dsl`; `fineprint-ingest` already depends on dsl (and not
on core, REQ-NFR-005), while xtask is standard-library only and depends on nothing. The CR text
parser is therefore a second, independent parser that follows xtask's region rule (last
`1. Game Concepts` line to last `Glossary` line) without depending on xtask.
**Alternatives.** Generator in xtask (rejected: would pull serde and dsl into xtask); default
`--out` from `git rev-parse` (rejected: needs git at run time for no gain).

## #15 FP-014 — CI step in the non-required cr workflow

**Decision.** `.github/workflows/cr.yml` gains the step `cargo run --locked -p fineprint-ingest
-- vocab --cr cr-cache/rules.txt --check` after `cargo xtask cr check`, with
`if: ${{ !cancelled() }}` so it reports even when the citation check fails, and path triggers for
`crates/fineprint-dsl/data/vocab.json`, `crates/fineprint-dsl/src/vocab/**` and
`crates/fineprint-ingest/src/vocab/**`. The workflow stays a non-required check. The CI test jobs
do not fetch the CR: every mandatory test passes without the cache, and the one test that reads
`cr-cache/rules.txt` returns early when it is absent (it is not `#[ignore]`d).
**Rationale.** "#7 FP-006 — `cr.yml` is not a required check": the check depends on the network.
**Alternatives.** A required job (rejected, same reason); committing the CR text (forbidden).

## #15 FP-014 — CR text and SRS agree for this baseline

**Decision.** No erratum. In the 2026-11-06 text the CR 122.1b keyword-counter list is exactly the
15 kinds of SRS §6.1 (ward is not among them); shield, stun and finality counters are 122.1c,
122.1d and 122.1h as SRS §10.2 says; CR 111.10 has letters a to x without l and o, with the
names of SRS §10.1; card types and supertypes match SRS §4.1. No `docs/HUMAN_QUEUE.md` line.
**Rationale.** Verified against the cached text and asserted by `spec_dsl_vocab.rs`.
**Alternatives.** None needed.

## #15 FP-014 — Citation index regenerated in this PR

**Decision.** The Orchestrator ran `cargo xtask cr index` on this branch (triage T0) and commits
the regenerated `docs/cr-citations.json`: 113 added lines, nothing removed. Before that, `cargo
xtask cr check` reported 14 problems: the new code comments cite CR 122.1, 205.2a, 205.3g, 205.4a,
701.1 and 702.1, and the index did not list them. The Implementer confirmed each of these rules
exists in the cached 2026-11-06 text. After re-indexing, `cr check` passes (462 cited rules).
**Rationale.** The check's own remedy is "re-verify the citation, then run `cargo xtask cr index`"
(#7 FP-006). A unit that adds citations brings the index up to date in the same PR, so the `cr`
workflow stays green. The file is generated, so this is a mechanical edit outside the unit's
owned paths.
**Alternatives.** A separate follow-up issue (rejected: until it merged, the `cr` workflow would
fail on every pull request).

## #15 FP-014 CR round 1 — double-faced token face order

**Decision.** A CR 111.10 token whose framing description is exactly `double-faced token` needs
exactly one `Its front face is ...` and one `Its back face is ...` sentence, in that order; the
faces are emitted front first, back second. A back-face sentence before the front-face sentence
is an input error (the generator does not reorder). A missing, repeated or unknown label (the
CR uses the lower-case words `front` and `back`), and any face sentence in a token not declared
double-faced, are input errors.
**Rationale.** The current CR text lists the front face first, so rejecting the reversed order
costs nothing today, keeps the generator simple and makes a changed CR shape visible to the
maintainer instead of silently reinterpreting it (the generator's rule for unexpected text).
**Alternatives.** Reordering reversed sentences (accepted input the CR has never used; a
silent reinterpretation). Keeping the old unlabelled reading (a back-first text would make the
back the front; CodeRabbit finding r4620683745).

## #15 FP-014 CR round 1 — token ability grammar

**Decision.** Every unquoted fragment of a token's `with ...` ability list (between `,` and
`and` separators) must be a keyword ability phrase: compared case-insensitively, either a
keyword-ability name from the generated CR 702 list alone, or that name followed by one space
and one parameter: a card type from the generated card-type list (`enchant creature`), a number
of at most six digits or `X`, or a mana cost made of one or more `{...}` symbols of ASCII letters,
digits and `/`. Anything else is an input error naming the token rule and the fragment. Quoted
abilities are kept exactly as before. Keyword actions (CR 701) are not accepted. The current CR
produces two unquoted fragments, `indestructible` and `enchant creature`, which both validate;
`vocab.json` is unchanged.
**Rationale.** Unquoted text in a token definition is the CR's keyword shorthand; validating it
against the keyword list the generator already builds keeps prose out of `vocab.json`
(REQ-NFR-004) without hard-coding any keyword (the names come from the text). The parameter
forms cover the shapes the CR uses for keyword parameters without accepting free text.
**Alternatives.** Accepting only the exact fragments seen today (would break on the next CR
with a new keyword token). Accepting any phrase that starts with a keyword name (lets prose
through after a keyword). A full keyword-parameter grammar per keyword (too much for the
generator; parameters such as `protection from ...` would need it, and the generator fails loudly
if the CR starts to use one, so it can be extended then).

## #15 FP-014 CR round 1 — CLI details

**Decision.** (1) A missing or unreadable `--out` file in `--check` mode is "restore from git"
when the path is exactly the default `crates/fineprint-dsl/data/vocab.json` (given or implied),
and "regenerate it" otherwise. (2) The mismatch hint says "rerun the same command without
--check (keeping --cr <path> --out <path>)" with the parsed paths, not a literal shell command
(quoting differs between shells). (3) A file that does not load is described by a fixed category
(`unparseable JSON`, `wrong shape`, or `field: <path>`); a shape error names no field, because
serde's message can quote input names or values. (4) The atomic write's temporary file is
`.<file name>.<pid>.tmp` beside the destination; it is removed after a failure only if this
call created it. (5) Unit-test scratch directories live under `<workspace>/target/fp-vocab-tests`
and integration-test ones under `CARGO_TARGET_TMPDIR`.
**Rationale.** The brief's definitions of fixed for findings r4620683663, r4620683679,
r4620683700, r4620683631 and r4620683732, applied with the smallest change.
**Alternatives.** Printing a quoted literal command (shell-specific). Mapping serde shape
errors to a field name by parsing serde's message (fragile, can echo input).

## #18 FP-017 — Arc copy-on-write snapshots, no persistent-collection crate

**Decision.** `GameState` holds `players: BTreeMap<PlayerId, Arc<PlayerState>>` and the turn
order as `Arc<Vec<PlayerId>>`; `player_mut` and the order writers go through `Arc::make_mut`.
`clone()` copies the small handle structures (the map of `Arc`s, the PRNG, the generators, the
turn position) and shares every player until it is written. `PartialEq` and the hash compare
values only; `shares_player_with` (`Arc::ptr_eq`) is the one place pointers are observed.
Later units add their components the same way: an `Arc`-shared field, a `_mut` accessor through
`Arc::make_mut`, a leaf group in `merkle_tree` and a member of the wire struct.
**Rationale.** Meets the "persistent/structurally shared representation with cheap snapshots"
deliverable with the standard library only (the brief forbids a new dependency), keeps the
iteration order of a `BTreeMap` (REQ-CORE-006) and needs no `unsafe` code.
**Alternatives.** `im`/`rpds` persistent maps (new dependency; hash-array-mapped variants
iterate in hash order). Deep `Clone` (snapshots cost O(state)). Wrapping the whole player map in
one `Arc` (O(1) clone, but a write copies every handle; can be added later if profiling asks).

## #18 FP-017 — serde and rmp-serde pins reused in fineprint-core

**Decision.** `fineprint-core` depends on the workspace pins `serde = "=1.0.229"` and
`rmp-serde = "=1.3.1"` chosen by FP-004; no new crate and no version change. Encoding uses
`rmp_serde::to_vec_named`; decoding uses `rmp_serde::Deserializer::new(Cursor::new(bytes))` and
rejects input unless the cursor position equals the input length.
**Rationale.** FP-004 already evaluated, licensed and pinned both crates for transport; the
`Cursor` reader copies a string or byte field only as far as the input actually holds, so a
hostile length header cannot cause a large allocation, and `position()` gives the trailing-byte
check.
**Alternatives.** `rmp_serde::from_slice` (no way to detect trailing bytes); a hand-written
MessagePack decoder (more code to make panic-free).

## #18 FP-017 — Game-state wire shape, u128 as [hi, lo], strict decoding

**Decision.** The transport form is the map
`{format_version, prng_algorithm, prng: {state, increment}, ids: {card, object,
ability_instance, effect, timestamp, link, player}, turn: {number, active_player,
priority_player, order}, players: [{id, life, counters, designations}], effects: {}, history: {}}`
with every identifier as a raw `u64` (the id newtypes do not implement serde), each PRNG
`u128` word as `[hi, lo]` two `u64`s, and counter amounts as `i64` in `1..=i64::MAX` (see
"Counters are non-negative i64"; amounts below 1, and unsigned values above `i64::MAX`, are
rejected). Designations use serde's externally tagged form (`"Flag"`,
`{"Int": i}`, `{"Text": s}`). Every struct is `deny_unknown_fields`; the two `Option` members of
`turn` must be present (`nil` for none: serde would otherwise accept a missing `Option` member as
`None`); `players` must be in strictly ascending id order (the order `serialize` writes). Serializing the
same state always produces the same bytes, and `serialize(deserialize(b)) == b` for every `b`
produced by `serialize`. Every failure is `StateError::Decode`.
**Rationale.** The brief's wire shape; rmp-serde's `u128` support is not guaranteed in other
clients' MessagePack libraries. Rejecting missing members and unsorted players keeps the player
list well formed. The decoder accepts any valid MessagePack spelling of the same state
(non-minimal integers, any member order); this is intended, because the bytes are transport only
and never a hash input (REQ-NFR-006).
**Alternatives.** `u128` as MessagePack bin or as a decimal string (less portable / larger).
Accepting players in any order and sorting.

## #18 FP-017 — Frozen Merkle layout of the game state

**Decision.** Leaves (payload = canonical encoding): `/prng`; `/ids/{card, object,
ability_instance, effect, timestamp, link, player}` (each `last_issued()` as `u64`);
`/turn/{number, active_player, priority_player, order}`; `/players/<index id>/{life, counters,
designations}` (counters as `BTreeMap<String, i64>`; they were `u64` before PR #166 review
round 1, and the bytes are unchanged because every stored amount is in `1..=i64::MAX`, where
`i64` and `u64` share the same 8-byte little-endian encoding); `/effects`; `/history`. Pinned golden digests: `GameState::new(42, 2, 20)` =
`f32bbec462508986efd89ed35f9184bec86a107f774505e19c4f65d3db4fb0d5`; the mutated state in
`state::tests::golden_mutated` = `7ebe9f5c17abfc986c0168e1722807514fd1372a1e6dcc10dca594afdfa58371`.
**Rationale.** One leaf per rules-relevant component lets `diff_first_divergence` name the
component that differs (REQ-NFR-006); keying players by `Label::index(id)` makes a player's
subtree digest independent of seat and of the other players. Later units add sibling subtrees
(`/zones`, objects, format config) without moving these leaves; such additions change the
golden root and are recorded as contract changes.
**Alternatives.** One leaf per player (coarser divergence reports). Seat-indexed players (a
turn-order change would move every player subtree).

## #18 FP-017 — MAX_PLAYERS = 256

**Decision.** A game state holds 1 to 256 players (`MAX_PLAYERS`); `new`, `add_player` and
`deserialize` enforce it.
**Rationale.** The brief's value; it bounds the work an untrusted serialization can demand while
staying far above any multiplayer format (REQ-NFR-010 asks for N players, not unbounded).
**Alternatives.** No bound (unbounded untrusted work); a small bound such as 8 (would exclude
large multiplayer variants).

## #18 FP-017 — DesignationValue is Flag / Int / Text under a free string key

**Decision.** Player designations are a `BTreeMap<String, DesignationValue>` with
`DesignationValue = Flag | Int(i64) | Text(String)`, canonical tags `0x00`, `0x01` + 8 bytes LE,
`0x02` + length-prefixed UTF-8. Counter kinds are likewise free strings (`BTreeMap<String,
i64>`, never holding an amount below 1).
**Rationale.** SRS §3.1 / §10.2: new sets add designations and counter kinds without engine
changes, so neither can be a closed enum. The three value shapes cover presence-only, numeric
and named designations with integer-only numbers (REQ-CORE-006).
**Alternatives.** A closed enum of known designations (engine change per set). A
`serde_json::Value`-like payload (floats and unordered maps would leak into the hash).

## #18 FP-017 — Placeholder effect registry and history ledger

**Decision.** `EffectRegistry` and `HistoryLedger` are empty `#[non_exhaustive]` structs with
`is_empty() == true`; each hashes as `put_len(0)` (4 zero bytes) at `/effects` and `/history`
and is the empty map `{}` on the wire. The units that fill them in replace the struct body,
the payload and the wire member (with a `STATE_FORMAT_VERSION` bump if the wire shape changes).
**Rationale.** Fixes the leaf positions now, so the effects and history units extend the state
without moving any other leaf.
**Alternatives.** Leaving them out until needed (the layout would shift later).

## #18 FP-017 — Wiring edits outside crates/fineprint-core/src/state/

**Decision.** Three authorised wiring edits: `pub mod state;` with a one-line doc comment in
`crates/fineprint-core/src/lib.rs`; `serde` and `rmp-serde` (workspace pins) under
`[dependencies]` in `crates/fineprint-core/Cargo.toml` (with the `Cargo.lock` lines cargo
adds); and the "Depends on" cell of the `fineprint-core` row in `docs/DEVELOPMENT.md`.
**Rationale.** The module and its dependencies cannot be reached otherwise; the brief grants
exactly these edits.
**Alternatives.** None that keep the module compilable.

## #18 FP-017 — PlayerMut guard instead of &mut PlayerState

**Decision.** `GameState::player_mut(id)` returns `Option<PlayerMut<'_>>`, a guard that reads the
player through `Deref<Target = PlayerState>` (no `DerefMut`) and writes it only through
`set_life`, `set_counter`, `set_designation` and `clear_designation`. Copy-on-write
(`Arc::make_mut`) still happens when `player_mut` is called and un-shares only that player.
`PlayerState`'s own setters stay public for owned values; the id has no setter.
**Rationale.** A private `id` field with `&mut PlayerState` was unsound: assignment through the
reference (`*p = PlayerState::new(other_id, ..)`) replaces the whole value, so a stored player's
`id()` could differ from its map key, and serialization (which writes `id()`) and the Merkle tree
(which keys `/players/<id>` by `id()`) would then write duplicate ids or collide (REQ-CORE-001,
REQ-CORE-002). The guard makes that impossible in safe code.
**Alternatives.** Field setters on `GameState` (one method per player field, n methods growing
with every new player field). Serializing and hashing by map key instead of `id()` (leaves
`id()` inconsistent with the key). A closure editor `edit_player(id, |p| ..)` (same problem if the
closure receives `&mut PlayerState`).

## #18 FP-017 — Counters are non-negative i64

**Decision.** Player counter amounts are `i64` everywhere (`counter`, `set_counter`, `counters`,
the wire form and the `/players/<id>/counters` leaf). The stored range is `1..=i64::MAX`:
`set_counter` with an amount of 0 or below removes the entry (a count is never negative), and the
decoder rejects any amount below 1, and any MessagePack unsigned value above `i64::MAX`, with
`StateError::Decode`. The frozen Merkle bytes and the golden digests are unchanged.
**Rationale.** REQ-CORE-006: "Numbers SHALL be signed 64-bit integers." A `u64` amount above
`i64::MAX` would have no representation in bindings that follow that rule and would break
cross-binding replay. For amounts in `1..=i64::MAX` the 8-byte little-endian encodings of `i64`
and `u64` are identical, so no hash changes.
**Alternatives.** `u64` with a cap check at the boundary (the type would still admit values the
SRS forbids). `i64` allowing negative amounts (a negative count has no meaning; zero and
negative entries would also let "no counters" have several distinct hashes). Saturating arithmetic on out-of-range inputs (hides
caller errors; removal on `<= 0` is the natural floor).

## #18 FP-017 — Full state hash vs the loop-detection hash

**Decision.** `state_hash` is the full canonical hash (REQ-NFR-006, REQ-CORE-006) and includes
`/ids/*`, the monotonic identifier generators including `/ids/timestamp`, so replays and
divergence debugging see them. The REQ-LOOP-001 rules-relevant hash (SRS §12.1 item 1: excludes
the timestamp counter and other monotonic bookkeeping, keeps relative timestamp order, SHALL be
incremental) is owned by issue #77 (FP-075); it can be derived by omitting the separate `/ids`
subtree and adding relative timestamp order once objects and effects exist. Incremental retention
of the `MerkleTree` (`set_leaf` plus the cached `root_digest` from FP-009) is likewise left to
#77, because where the tree lives (in the state or in the detector) depends on its consumer and
on the Arc-shared components of #19. `state_hash` stays a correct full recompute.
**Rationale.** This unit's acceptance criteria ask for the canonical full hash; the loop hash has
no consumer until #77, and the frozen `/ids` subtree is separate so it can be dropped there
without changing any other leaf.
**Alternatives.** Dropping `/ids/timestamp` from the frozen layout now (would hide divergence in
replays and is not required by this unit's acceptance criteria). A second loop hash now (no
consumer).

## #19 FP-018 — Zones API

**Decision.** (a) `fineprint_core::zones::Zones` is a standalone container: it is not wired into
`GameState`, its Merkle layout or its wire form (`state/` is not owned by this unit). The next unit
adds it as an `Arc`-shared component with a `_mut` accessor, a Merkle leaf group and a wire
member, as the `state` module doc describes; `Zones` already implements `CanonicalEncode` for that
leaf group. (b) The optional free graveyard reordering flag (REQ-ZONE-001 item 4, the CR 404.2
tournament option) is `ZonesConfig::free_graveyard_reorder` (default `false`) because
`FormatConfig` does not exist yet; it moves there when it does. `reorder_graveyard` checks the flag
first (so a disabled flag reports `ReorderNotAllowed` even for an unknown player or an unchanged
order), then the player, then that the order is a permutation of exactly the current contents.
(c) `move_object`, `create_object` and `enter_from_outside` take `&mut IdGenerator<ObjectId>`
because `GameState`'s generators are private; `ids` must be the generator that issued every
existing `ObjectId` in the container. The id is issued from a copy of the generator after every
other check and the copy is written back only on success, so an error leaves the container and
the generator unchanged and a real zone change can never return `Moved { old == new }`. The
container keeps a private high-water mark `max_issued` (the highest ObjectId raw value it has
ever issued; 0 when none; PR #167 round 2, finding G5); an issued id at or below it is rejected
as `ZoneError::IdCollision` (a high-water rejection, not proof of reuse: it also rejects ids the
container never held, e.g. first id 10 from a generator restored at 9, then a generator proposing 5
gets `IdCollision(5)`; PR #167 round 3, H1; round 1 finding F3 had rejected only live ids), so a stale or saved generator cannot silently resurrect a retired id. The mark is part
of `PartialEq`/`Eq` and is appended to the canonical encoding as 8 bytes little-endian; a
reposition never touches it. Rationale: ids are never reused (REQ-CORE-002), and the `GameState`
integration unit will hold the generator and the container together. Alternative considered:
tracking every retired id, rejected as unbounded. The frozen state hash and wire are unaffected
(`state/` is not touched). (d) A
move into the zone the object is already in (same `ZoneId`; a per-player destination is always
the owner's, CR 400.3, so a library-to-library move is always the same library) depends on the
zone (PR #167 round 1, finding F1, which corrects the original brief's "a same-zone move also
creates a new ObjectId"): in a library, hand, graveyard, the battlefield or the stack it is not a
zone change (CR 400.7 speaks of moving "from one zone to another"), so the object keeps its
`ObjectId`, the generator is not advanced, `Moved` has `old == new`, and the object is only
repositioned (ordered zones: removed and re-inserted at the requested position of the remaining
pile, CR 400.5; unordered zones: position ignored; a reposition inside one zone is not a move from
one zone to another, CR 400.7),
with `face_down` and the controller taken from the request as for any move; in exile and the
command zone the object becomes a new object without changing zones (CR 400.8, CR 406.7 for
exile, CR 400.10 for the command zone) and gets a fresh `ObjectId`. CR 400.9 and CR 401.6 are
later work. (e) Outside the game is never a zone (CR 400.11):
it is a per-owner set of `OutsidePool` pools of `CardId`s with no object ids; a card is in at most
one place (a zone object or one pool), and empty pools are not stored so equal contents are equal
values. (f) The SRS REQ-ZONE-001 item 1 clause attributing re-evaluation "including partway
through casting or resolution" to CR 401.5 is not covered by CR 401.5 (casting, activation and
special actions only) and is out of scope here, with the rest of top-card reveal statics;
simultaneous-entry ordering choices (CR 401.4, CR 404.3, CR 405.3), merged permanents, ante and
subgames are later work. (g) Only objects on the stack or the battlefield have a controller
(CR 109.4). The container keeps an explicit controller only there: a request controller
elsewhere, or `set_controller` elsewhere, is `ControllerNotApplicable` as a container rule (the
CR has exceptions, CR 109.4a-g, such as emblems in the command zone, CR 109.4c; emblems and
variant cards are not modelled yet); on entering any other zone the stored `controller` field is
the owner, the CR 108.4a fallback, and `ObjectRecord::rules_controller` returns `None` there
(PR #167 round 1, finding F4). (h) The CR 400.7 exception registry has 12
entries (CR 400.7a-k and CR 400.7m) because the CR has no letter l; each is a named stub function
returning `ExceptionOutcome::NotImplemented` until the unit that needs it fills it in.
`Position::Index(n)` past the bottom of any ordered zone means the bottom (the CR 401.7 treatment,
applied to every ordered zone). (i) Out of scope: cardless stack objects. In this unit the stack
holds only card-backed objects (spells, and copies that have a `CardId`), and
`Zones::create_object` requires a `CardId`. Abilities go on the stack without a card (CR 405.1;
an object can be an ability on the stack, CR 109.1) and SRS REQ-CORE-002 gives them a separate
`AbilityInstanceId`; copies of spells that are not cards are also cardless. Their representation
is the job of the units that own them: #31 FP-030 (putting triggered abilities on the stack),
#58 FP-056 (resolution of spells and abilities) and #66 FP-064 (copies of spells and abilities),
with activated abilities in a later stack/ability unit. This limit is documented on `Zones`,
`ZoneId::Stack`, `ZoneKind::Stack` and the module doc (PR #167 round 1, finding F2, deferred).
**Rationale.** Keeps the frozen state hash and wire contracts untouched while giving the next
unit a ready component; follows the issue brief, SRS REQ-ZONE-001 and REQ-CORE-002, and the
rules-researcher answers for #19 and for PR #167 round 1 (CR 400.7 applies only to moves between
zones; CR 400.8, CR 406.7 and CR 400.10 are the only same-zone new-object rules).
**Alternatives.** Wiring into `GameState` now (outside owned paths, would change the frozen Merkle
layout). Outside the game as a `ZoneKind` (contradicts CR 400.11). A new id for every same-zone
move (the original brief's reading; contradicts CR 400.7, which applies to moves from one zone to
another, and would make a library reposition a new object). Keeping the id for exile and command
same-zone moves too (contradicts CR 400.8, CR 406.7, CR 400.10). Inventing a cardless stack
entry now (owned by later units; would pre-empt their design). A 13-entry registry with a
placeholder for a missing letter (no such rule exists).

## #19 FP-018 — Zones spec-test readings

**Decision.** Readings the Independent Tester chose where the brief was silent, all of which the
implementation meets: (1) a move within one ordered zone (for example library to library at
`Index(k)`) removes the object first, then re-inserts it at `k` in the remaining pile; since PR
#167 round 1 (finding F1) this is a reposition of the same object, which keeps its `ObjectId`,
not a new object (only exile-to-exile and command-to-command moves give a new id, CR 400.8,
CR 406.7, CR 400.10), and the spec tests were corrected to assert that per zone kind;
(2) `add_outside_card` with an unknown owner is `UnknownPlayer(owner)`; (3) a request controller
naming the owner is still `ControllerNotApplicable` outside the battlefield and the stack;
(4) `reorder_graveyard` with an empty list is `InvalidOrder` for a non-empty graveyard and accepted
for an empty one; (5) the spec tests do not require a `ZonesConfig` difference to change the
canonical bytes; the implementation does encode the flag first, which is allowed.
**Rationale.** Each is the literal reading of the #19 brief and keeps the zone-move primitive the
single place an object changes position.
**Alternatives.** Inserting at `k` counted before removal (makes `Index(k)` depend on where the
object was); accepting `Some(owner)` everywhere (two spellings for one request).

## #20 FP-019 — Permanent state API

**Decision.** `fineprint_core::permanent` provides a standalone `Permanents` container
(`BTreeMap<ObjectId, Permanent>`) with the API of the #20 brief: `Status` (four independent
booleans, default untapped/unflipped/face up/phased in, CR 110.5b), an open designation map
`BTreeMap<String, DesignationValue>` that reuses the #18 `DesignationValue` type and its canonical
tags, continuous-control tracking as a turn number, an `Option<AttachTarget>` link, and a
`CombatState` (`AttackState`, blocking set, `blocked`, `removed_from_combat`). Readings chosen:
(1) continuity is strict: `controlled_continuously_since_start_of(t)` is
`controlled_since_turn() < t`, so a permanent that came under control during the controller's
turn `t` is not continuous for that turn (CR 302.6); haste is the caller's. No summoning-sickness
boolean is stored. (2) Attack state is the enum `NotAttacking | Attacking(Option<AttackTarget>)`
because CR 506.4c keeps a creature attacking after the planeswalker or battle it attacked is
removed from combat, while it attacks nothing; `Attacking(None)` is that state. (3) `leave(x)`
returns the pre-removal value, unattaches everything attached to `x` (CR 701.3d), removes `x` from
every other blocking set, and rewrites attackers of `Permanent(x)` to `Attacking(None)` (CR 506.4,
506.4c); other `blocked` flags stay (CR 509.1h). (4) `remove_from_combat(x)` resets `x`'s combat
fields, sets `removed_from_combat`, and rewrites attackers of `x` to `Attacking(None)`, but leaves
other permanents' blocking sets and `blocked` flags alone: the CR describes the removed creature
ceasing to be blocking or blocked and keeps the attacker blocked (CR 509.1h); pruning stale
references is left to the combat unit, which reads `removed_from_combat`. (5) `set_controller` to
the same player is a complete no-op (the CR is silent; no control change happened); to a different
player it stores the given turn as-is and then applies exactly `remove_from_combat` (CR 506.4,
613.1b). (6) `set_phased_out` applies `remove_from_combat` only on a false-to-true transition
(CR 702.26b, 506.4) and never changes controller or controlled-since (CR 702.26d). (7) `attach`
checks, in order, unknown id, unknown target permanent, self-attach, cycle (walking attachment
links from the target, bounded by `len()+1`), and replaces the old link; player targets are not
validated. (8) `remove_from_combat` on a permanent recorded as attacking itself resets it to
`NotAttacking` (it is not redirected to `Attacking(None)`), because the redirect applies only to
other permanents. (9) Canonical encoding: `put_len` of the count, then per permanent in ascending
id: id, controller, the four status bytes, designations, controlled-since (u64 LE), attachment as
`Option` (tag 0x00 permanent / 0x01 player), combat state (attack tag 0x00 not attacking / 0x01
attacking then `Option<AttackTarget>` with tag 0x00 player / 0x01 permanent; blocking set;
`blocked`; `removed_from_combat`). (10) The container is standalone: not wired into `GameState`,
its Merkle layout or wire form; `ObjectRecord.face_down` in zones is reconciled with
`Status::face_down` by the integration unit.
**Rationale.** Follows SRS REQ-ZONE-001 item 3, REQ-CORE-006 and the #20 brief's API and CR
excerpts; keeps the frozen state and zones contracts untouched, as #19 did.
**Alternatives.** A stored summoning-sickness flag (forbidden by REQ-ZONE-001). Non-strict
continuity (`<=`) (would make a permanent gained during the controller's turn able to attack that
turn). `Option<AttackTarget>` alone for attack state (cannot express CR 506.4c). Pruning other
blockers' sets in `remove_from_combat` (not in the brief; would hide the removed attacker from the
combat unit). Resetting control tracking on a same-controller set (records a control change that
did not happen). Wiring into `GameState` now (outside owned paths; changes frozen layouts).

## #20 FP-019 — Permanent spec-test readings

**Decision.** Readings the Independent Tester chose where the brief was silent, all of which the
implementation meets: (1) `set_blocking` on an unheld blocker whose slice also holds unheld ids is
left untested (which error wins is unspecified); (2) `attach` with both the id and the target
unknown, or an unknown id attaching to itself, is `UnknownObject(id)`, following the stated check
order; (3) a control change keeps the permanent's status (CR 110.5c); whether designations and
attachments survive a control change is not asserted; (4) `set_phased_out(id, true)` on an
already phased-out permanent keeps combat state set after the first phase-out, and
`removed_from_combat` stays true; (5) `remove_from_combat`, `set_controller` and a phase-out of a
permanent that another permanent is attacking rewrite that attack to `Attacking(None)` (CR 506.4c);
(6) `set_blocking` and `set_blocked` leave `removed_from_combat` unchanged.
**Rationale.** Each is the literal reading of the #20 brief's behaviour contract, or the CR text
it quotes.
**Alternatives.** Clearing `removed_from_combat` on any combat setter (would hide that the
permanent was removed); treating a phase-out of an attacked permanent as leaving its attackers'
targets intact (contradicts CR 506.4c).

## #21 FP-020 — SRS section 4.1 errata

**Decision:** Treat REQ-OBJ-001 as follows: variable symbols are {X} and {Y} only (CR 107.3p; no {Z}); the no-mana-cost MV-0 exception applies only to the back face of a nonmodal double-faced permanent or spell and to melded permanents (CR 202.3a); ColorIdentity includes alternative characteristics (CR 903.4e), the back face (903.4d) and a pre-game color choice (903.4b); monocolored hybrid includes {C/W},{C/U},{C/B},{C/R},{C/G} (CR 107.4).
**Rationale:** CR text of baseline 2026-11-06 wins (P1).
**Alternatives:** Keep {Z} as a reserved symbol; rejected as not CR-backed.

## #21 FP-020 — Characteristics API

**Decision:** `fineprint_core::objects` is a standalone module (not wired into `GameState`, its
Merkle layout or wire form) with exactly the pinned API of the #21 brief: `VariableValues`,
`ManaValueContext` (`OffStack` / `OnStack`), `SplitHalf`, `SplitState`, `ManaValueSubject`
(`Plain`, `NonmodalBackFace`, `CopyOfNonmodalBackFace`, `Melded`, `CopyOfMelded`, `Split`),
`symbol_mana_value`, `mana_value`, `color_mask`, `union_colors`, `symbol_colors`,
`colors_of_mana_cost`, `base_colors`, `IdentityFace`, `color_identity`, `ObjectError`,
`validate_types`, `can_gain_subtype` and `Characteristics` (`from_face`, `mana_value`,
`pt_applies`). `Characteristics` reuses the `fineprint_dsl::card` placeholder types
(`ColorBitmask`, `TextTokenStream`, `AbilityAST`, `ValueExpressionAST`) so a `CardFaceDSL` maps
to it one to one. `{Z}` is treated exactly like `{X}` and `{Y}` (0 off the stack, the supplied
value on it) because the DSL still accepts it; `VariableValues` carries `z` for that reason.
Copies of spells are not modelled: the caller passes the same `OnStack` values (CR 707.10). Mana
value arithmetic saturates in `i64`. `ObjectError` is not `#[non_exhaustive]` so integration
tests can match it exhaustively. `from_face` sets `colors` from cost and indicator only; CDA
colors are applied by later work.
**Rationale:** The brief pins the API for the Independent Tester; reusing the DSL types avoids a
second representation of the same data.
**Alternatives:** Rejecting `{Z}` at mana-value time (would make the function partial over a type
the DSL accepts); own wrapper types for P/T and abilities (duplicate data, no benefit until the
real ASTs land).

## #21 FP-020 — `{C/W}` is not representable in `ManaSymbol`

**Decision:** CR 107.4e makes `{C/W}`-style monocolored hybrid symbols colored with mana value 1,
but `fineprint_dsl::mana::ManaSymbol` cannot represent them (`GenericHybrid` requires `n >= 1`
and `parse` rejects `{C/W}`). This unit covers every existing `ManaSymbol` variant and does not
edit `fineprint-dsl`. The follow-up is an additive `ManaSymbol` variant proposed through
`contracts/CHANGE_REQUESTS.md` and landed as a separate contract-only PR (R2), after which
`symbol_mana_value` (1) and `symbol_colors` (the color) gain one arm each.
**Rationale:** `fineprint-dsl` and `contracts/**` are outside this unit's owned paths; the
change is additive and can land independently.
**Alternatives:** Encoding `{C/W}` as `GenericHybrid(1, c)` (changes `{1/W}` semantics and the
DSL validity rule); editing the DSL in this unit (violates R1/R2).

## #21 FP-020 — pt_applies for creatures and printed power/toughness (CodeRabbit r1)

**Decision:** `Characteristics::pt_applies(on_battlefield)` is
`is_creature || (!on_battlefield && (power.is_some() || toughness.is_some()))`. A creature has
power and toughness in every zone; a missing value counts as 0 (CR 208.5). A noncreature has them
only off the battlefield and only when a power or toughness is printed (CR 208.3); a noncreature
permanent never has them. Either printed field counts (`||`, not both), although CR 208.3 says
"a power and toughness".
**Rationale:** CodeRabbit round 1 on PR #170 showed a battlefield creature with no printed P/T
was reported as having none, contradicting CR 208.5. The DSL permits one-sided printed values,
and a one-sided noncreature card is not a legal card anyway, so `||` keeps the function total
without inventing a rejection path.
**Alternatives:** Require both fields for a noncreature (rejects a DSL-valid shape for no rules
gain); treat CDA-defined P/T as printed (CDAs are applied by later layer work).

## #21 FP-020 — base_colors takes CdaColors; split NotOnStack ignores the supplied context (CodeRabbit r1)

**Decision:** `base_colors(cost, indicator, CdaColors)` with `CdaColors::None | Add(mask) |
Set(mask)`. `None` is cost ∪ indicator; `Add(m)` adds `m` (CR 604.3 "add to"); `Set(m)` is
exactly `m`, overriding cost and indicator (CR 604.3 "override"). A devoid-style CDA is
`Set(ColorBitmask::NONE)` (CR 702.114a). `Characteristics::from_face` passes `None`.
`IdentityFace` keeps a plain `cda_colors` mask, passed as `Add(mask)`, so color identity never
loses cost colors (CR 903.4); `Set` is the only way a CDA overrides cost and indicator, and it is
not used for identity. Applying CDA colors inside `Characteristics` and other color-changing
effects stays later layer-5 work (CR 613.3, 613.1e). The API is new in this PR, so no consumer
breaks. Also: for `SplitState::NotOnStack` the state decides the context — both halves are
evaluated `OffStack` whatever context is passed (CR 202.3d, 202.3e, 107.3g); `OnStackFused` and
`OnStackHalf` use the supplied context.
**Rationale:** CodeRabbit round 1 on PR #170: a plain mask cannot express a colorless-defining
CDA, and a `NotOnStack` split evaluated with an on-stack context counted X.
**Alternatives:** `Option<ColorBitmask>` (cannot express add versus override of a non-empty
mask); a bool `colorless` flag (covers only devoid); keeping a plain mask (rejected by the
finding); making `NotOnStack` honour `ctx` (contradicts CR 202.3e).

## #22 FP-021 — SRS 4.1/4.2 gaps

**Decision:** The CR baseline 2026-11-06 wins (P1); the unit follows it where SRS 4.1/4.2 is
silent or too coarse. (1) SPLIT battlefield cell "N/A (instant or sorcery)": instants and
sorceries cannot enter the battlefield (CR 304.4, 307.4), permanent split cards are ROOM
(CR 709.5, out of scope here); the cell should also cite CR 709.3b and 709.4a-d. This unit's
`SplitLayout` still returns the combined characteristics on the battlefield without error
(CR 709.4 says every zone except the stack). (2) FLIP: only name, text box, type line and
power/toughness switch; mana cost, color and mana value are unchanged (CR 710.1c). Selection is
by flipped status, which is status and not copied (CR 110.5, 707.2, 707.3), so the SRS 4.2
lead-in "all alternative characteristics are copiable" does not apply to flip. (3) DFC_MODAL
battlefield "The face chosen": add CR 712.9, 712.10, 712.11, 712.11a; CR 712.13 (cannot be turned
face down) applies to every double-faced permanent. (4) LEVELER: band base P/T and abilities are
static abilities (CR 711.2a, 711.2b, layer 7b); base characteristics carry the uppermost P/T
below the first band and off the battlefield (CR 711.5, 711.6); level bands are not applied in
this unit. (5) The SRS 4.1 CR 202.3a bullet should read "except the back face of a nonmodal
double-faced permanent or spell, and melded permanents" (a modal back face with no cost has mana
value 0). (6) PROTOTYPE battlefield cell "Same as on the stack" holds only if cast prototyped;
otherwise normal characteristics (CR 719.4).
**Rationale:** CR text wins (P1); only a person edits the SRS, so the proposed SRS edit is a
HUMAN_QUEUE line added by the Orchestrator.
**Alternatives:** Follow the SRS cells literally (contradicts CR 710.1c, 712.6e, 719.4).

## #22 FP-021 — Layouts API

**Decision:** `fineprint_core::layouts` exposes the API pinned in the brief: `CastChoice`,
`FaceUp`, `LayoutStatus`, `LayoutQuery`, `LayoutError`, the `Layout` trait with one unit struct
per supported layout (NORMAL, SPLIT, FLIP, DFC_NONMODAL, DFC_MODAL, ADVENTURER, OMEN, PROTOTYPE,
LEVELER, SAGA, CLASS, CASE), `layout_for`, the free functions `base_characteristics`,
`mana_value`, `gating`, and `Gating`. Every other `LayoutBehavior` (ROOM and MELD included) is
`UnsupportedLayout`. The module is standalone: not wired into `GameState`, the Merkle layout or
the wire form. The cast choice is consulted only on the stack and, for PROTOTYPE, the
battlefield. Copies are not modelled except the `copying_back_face` mana-value flag (nonmodal
only, CR 202.3b, 712.6d, and only on the stack or battlefield; in every other zone the flag is
ignored and the front face's cost applies, CR 712.6a). Faces are looked up by `face_kind`; the face set must be exact.
Errors: `LayoutMismatch`, then `BadFaces`, then `InvalidCast`, then `Object`. SPLIT combined
characteristics: names are the set of both names; the mana cost is the left symbols followed by
the right symbols (`None` when neither half has a cost); the color indicator is the union of the
halves' indicators (`None` when both are `None`); colors, supertypes, card types and subtypes are
unions; rules text and abilities are left then right; power, toughness, loyalty, defense, hand
and life modifiers are `None`. Judgement calls: (a) `mana_value` runs the same selection as
`base_characteristics` first, so both return the same error for the same query (a type-line
error of the selected face makes mana value fail too); (b) only the faces a query selects are
passed to `Characteristics::from_face` (a bad type line on an unselected face is not reported);
(c) `gating` checks only the layout and the face set (no type-line validation), and returns
`Ok(None)` for the eight non-gating layouts once those checks pass; (d) amended in CodeRabbit
round 1: off the stack X (and Y, Z) count 0 whatever context is supplied. Every layout's
`mana_value` passes the caller's context on only when `query.zone` is the stack and
`ManaValueContext::OffStack` in every other zone (CR 202.3e, 107.3g; the permanent's X is 0,
CR 107.3m; Y follows X, CR 107.3p). This replaces the earlier reading that a supplied `OnStack`
context is honoured off the stack.
**Rationale:** The brief pins the API for the Independent Tester; making `mana_value` and
`base_characteristics` agree on errors keeps callers from seeing a mana value for an object whose
characteristics cannot be built; validating only selected faces matches the selection tables
("face X chars"); gating is data-only and needs no type line. For (d), the query already says
where the object is, so the zone decides the context: CR 202.3e and 107.3g make X 0 in every
zone other than the stack, and a caller passing a stale `OnStack` context (for example for a
permanent that was an X spell) must not change the mana value; this matches the #21 rule that
split `NotOnStack` ignores the supplied context.
**Alternatives:** Validate every face on every call (rejects an object for a face it does not
have in that zone); compute mana value from the raw faces without validation (the two functions
could disagree); `gating` returning `None` without any check for non-gating layouts
(inconsistent error precedence).

## #22 FP-021 — Gating data is printed abilities

**Decision:** `Gating` carries the Main face's printed abilities in definition order and, for
LEVELER, the printed (uppermost) power/toughness of the Main face. Structured gating data (level
bands with their ranges and P/T, chapter numbers and the final chapter, class level bars, Case
to-solve and solved abilities) lands with the real ability AST; this unit exposes the printed
abilities and the uppermost P/T. No JSON convention inside the placeholder `AbilityAST` is
invented.
**Rationale:** `AbilityAST` is an opaque placeholder today (FP-011/012/013 replace it), so band
and chapter structure cannot be read without inventing a format the real AST would contradict.
**Alternatives:** A provisional JSON shape inside `AbilityAST` (would need migrating and could
diverge from the real AST); omitting `Gating` until the AST lands (the issue requires gating
data now).

## #22 FP-021 — Flip loyalty, defense, hand and life modifiers

**Decision:** A flipped flip permanent on the battlefield takes its name, text box (rules text and
abilities), type line (supertypes, card types, subtypes), power and toughness from the Flipped
face (CR 710.1b, 710.2); its mana cost, color and color indicator stay the Main face's
(CR 710.1c); its loyalty, defense, hand modifier and life modifier also stay the Main face's.
**Rationale:** CR 710.1b lists the alternative characteristics of the bottom half as name, text
box, type line, power and toughness, and CR 710.2 says only those normal versions stop applying
once flipped. Loyalty, defense and the hand and life modifiers are not among them, so nothing
replaces the Main face's values (CodeRabbit round 1, PR #171).
**Alternatives:** Take all four from the Flipped face (the first implementation: reads more into
CR 710.1b than it says); set them to `None` when flipped (drops printed Main-face values no rule
removes).

## #23 FP-022 — SRS 4.2 gaps for room, meld, station, preparation and mutate

**Decision:** Where SRS §4.2 (REQ-OBJ-002) and the CR baseline (2026-11-06) differ, the CR wins
(P1): (1) STATION: the stack shows the printed characteristics except power and toughness, which
are absent (CR 722.2c: no power or toughness in any zone other than the battlefield); (2) MELD:
when a melded permanent leaves the battlefield, one permanent leaves and both cards are put into
the new zone (CR 713.8), not "the two cards move separately"; (3) ROOM: on the battlefield a
Room's name, mana cost and rules text come only from its unlocked halves, so its mana value and
colors come from the unlocked halves' costs and are 0/colorless with neither unlocked (CR 709.5,
202.3, 202.3a; 709.5 is more specific than 709.4b); the unlock special action needs priority
(CR 116.2m, 709.5e).
**Rationale:** Rules-researcher check of the §4.2 rows against the CR text (issue #23).
**Alternatives:** Follow the SRS wording (contradicts the CR on the stack P/T and on the
leave-the-battlefield event count).

## #23 FP-022 — Composite layouts are dedicated APIs; layout_for unchanged

**Decision:** ROOM, MELD, STATION, PREPARATION and MUTATE get their own public types in
`fineprint_core::layouts` (`RoomLayout` with `RoomQuery`/`RoomDoors`, `MeldLayout` and
`MeldedPair`, `StationLayout` and `active_station_symbols`, `PreparationLayout` with
`prepare_spell_characteristics`/`become_prepared`, `MutateLayout` and `MergedPermanent`).
`layout_for`, the free functions `base_characteristics`/`mana_value`/`gating`, the `Layout`
trait, `LayoutQuery`, `LayoutStatus` and `CastChoice` are unchanged, so `layout_for` still
returns `UnsupportedLayout` for these five behaviors. Four `LayoutError` variants are added
(`NotMeldPair`, `AlreadyPrepared`, `TooFewComponents`, `InvalidStationSymbols`). The exhaustive
`match` in `layouts/tests.rs::is_defined_error` gains the four variants in its `false` arm (no
existing layout returns them), the only edit needed for it to compile.
**Rationale:** A Room needs door state, a melded permanent needs two definitions and a merged
permanent an ordered list of components; none fits `LayoutQuery` without changing a frozen
contract and the #22 spec tests. Wiring into `layout_for` belongs to the integration issue.
**Alternatives:** Extend `LayoutQuery`/`LayoutStatus` with door, prepared and merge fields
(breaks the #22 API and its frozen tests); make `LayoutError` `#[non_exhaustive]` (does not
help inside the crate).

## #23 FP-022 — A locked Room half keeps its color indicator

**Decision:** On the battlefield a Room's colors are `base_colors(cost of the unlocked halves,
union of both halves' color indicators, CdaColors::None)`, and `color_indicator` is the union of
both halves' indicators whatever the doors; a locked half loses only its name, mana cost, rules
text and abilities. With both halves locked the Room is colorless unless a half has an indicator.
**Rationale:** CR 709.5 removes only "the name, mana cost, or rules text" of a locked half; a
color indicator is none of these, so nothing removes it, while the cost colors go with the cost
(CR 202.2). Abilities follow the rules text they are printed in.
**Alternatives:** Drop a locked half's indicator too (reads more into CR 709.5 than it says);
keep the combined cost's colors (contradicts the removed mana cost, see the #23 SRS 4.2 gaps
entry).

## #23 FP-022 — Station symbols are caller-supplied; the layer system picks P/T

**Decision:** `active_station_symbols` takes the station symbols as `StationSymbol` values
(threshold, abilities, optional P/T box) supplied by the caller, checks there are one or two
(CR 722.1), returns none off the battlefield or with a negative counter count, and otherwise
returns every symbol whose threshold is at most the charge-counter count, in printed order. When
two symbols apply, both are returned; choosing whose P/T box is the base P/T is left to the
layer system. `StationLayout` itself keeps the Main face's printed abilities (CR 722.4) and drops
power and toughness off the battlefield (CR 722.2c).
**Rationale:** `AbilityAST` is an opaque placeholder (as in the #22 gating decision), so the
striations cannot be read from a definition without inventing a JSON convention. Picking between
two active P/T boxes is a layer-system question, and there is no layer system yet. A negative counter
count cannot occur in the game, so it activates nothing rather than activating a `{0+}` symbol.
**Alternatives:** Parse symbols from `AbilityAST` (invents a format); return only the highest
active symbol (pre-empts the layer system); clamp negative counts to 0.

## #23 FP-022 — Merged permanents: `mutated` flag, abilities-only union

**Decision:** `MergedPermanent::new(components, mutated)` takes an explicit `mutated` flag. The
merged permanent's base characteristics are the topmost component's, selected with its own
layout on the battlefield (CR 731.2a); when `mutated` is true its `abilities` are the
concatenation of every component's abilities, top to bottom, without deduplication
(CR 702.140e), and every other field, `rules_text` included, stays the topmost component's.
Errors come from the topmost component, then, only when mutated, from the others in order.
**Rationale:** CR 731.2a lets the effect that caused the merge specify otherwise; mutate is the
one such effect in scope, so a flag keeps the general merge and mutate apart. `rules_text` is an
opaque token stream with no ability boundaries, so only the structured ability list is united.
**Alternatives:** Always unite abilities (wrong for non-mutate merges); concatenate rules text
too (mixes printed text of several cards into one stream with no defined meaning); deduplicate
abilities (CR 702.140e says "all abilities of each card").

## #23 FP-022 — Token components use a stand-in definition

**Decision:** A `MergedComponent` that is a token is described by `is_token: true` plus any
definition whose layout the component resolver supports (MUTATE, MELD, STATION, PREPARATION or
any `layout_for` layout). TOKEN and DFC_TOKEN definitions return `UnsupportedLayout` until the
token issue, like every other unsupported layout (ROOM included).
**Rationale:** Token definitions are out of scope for this unit; the token flag is what CR 731.2d
needs, and the characteristics come from whatever definition the token copies or was created
from.
**Alternatives:** Add TOKEN support here (out of scope); reject token components (the merged
token rule could not be tested).

## #23 FP-022 — Independent Tester judgement calls

**Decision:** (1) A Room's `color_indicator` is the union of both halves' indicators in the
combined zones and on the battlefield whatever the door state, and the cast half's own on the
stack (as SPLIT in #22). (2) A Room definition error on the shared type line surfaces on the
battlefield even with both halves locked, because the shared type line always applies (CR 709.5a).
(3) `MergedPermanent::mana_value` with an unsupported top component returns the same
`UnsupportedLayout` error as `base_characteristics`. (4) Negative station thresholds are not
specified: the spec tests assert only that a negative counter count activates no symbol, including
a `{0+}` symbol.
**Rationale:** The briefs fixed colors and errors but not these fields and edge cases; each choice
matches the Implementer's entries above and the nearest existing layout behaviour.
**Alternatives:** Locked halves drop their indicator (rejected in "A locked Room half keeps its
color indicator"); a separate mana-value error path for merged permanents (no rule asks for it).

## #23 FP-022 — Room halves must share one type line

CodeRabbit round 1, PR #172.
**Decision:** A ROOM definition whose two faces' type lines differ (supertypes, card types or
subtypes) is malformed and is rejected with `LayoutError::BadFaces` in every zone, the stack with
any cast half and the battlefield with any door state included (both halves locked too, as in
"Independent Tester judgement calls" (2)). The check runs in the shared selection path, after the
face-set check and before `InvalidCast` and `Object`. Each type-line field is compared as a set
(order and repeats ignored), because `Characteristics` and the combined view keep the types as
sets. On the stack the cast half's record is returned; its type line equals the shared line (CR
709.5a). The spec fixtures `room_left()`/`room_right()` behind `room_sample()` live in
`tests/spec_layouts_composite_support.rs`, not under `src/layouts/` as the brief assumed; they
were given the one shared type line there (the brief owns `room_sample()` "wherever it is
defined"), and the spec tests that built a Room from one rejected half now put the rejected type
line on both halves.
**Rationale:** CR 709.5a gives both halves the one shared type line; 709.5b exempts only the
halves' existence from 709.3b, so the shared type line also applies on the stack. Set comparison
matches the semantics of the type-line field everywhere it is read.
**Alternatives:** The union of both halves' types on the stack (rejected: it would accept data
that contradicts the one shared type line); silently using the cast half's line (the CodeRabbit
finding); an order-sensitive comparison of the stored lists (rejected: order carries no meaning
in the type line as `Characteristics` stores it).

## #23 FP-022 — Face-down merged components deferred to layer 1b (#39)

CodeRabbit round 1, PR #172.
**Decision:** `MergedPermanent::base_characteristics` returns the pre-layer-1b values (printed
values selected with each component's layout), whatever any component's `face_up` flag says.
`face_up()` reports the topmost component's status (CR 731.2e) so that layer 1b can apply CR
708.2. The face-down characteristics of merged components are not modelled here. Whether a
face-down lower component of a mutated permanent contributes its printed abilities is open for
#39: CR 702.140e gives the permanent the abilities of each card and token that represents it, CR
708.2 limits face-down permanents, and a component is not itself a permanent; no ruling was
found. The unit tests no longer pin printed values to a face-down top.
**Rationale:** SRS §4.3 / REQ-OBJ-003 apply face-down status in layer 1b (CR 613.2b), which is
issue #39 FP-038; this module selects base characteristics and copiable values before layer 1b.
**Alternatives:** Modelling face-down values in this module (rejected: it duplicates layer 1b and
pins an unsettled rule).

## #24 FP-023 — Memory and LKI API shape; capture before move

**Decision:** A new standalone module `fineprint_core::memory` holds `DurableRef`, `LinkEntry`,
`LinkedStore`, `MoveFinder`, `ChoiceValue`, `SpellHistory`, `Memory`, `MemoryTable`, the
`CharacteristicsProvider` trait, `LkiRecord`, `LkiStore`, `capture_lki_on_move`,
`information_for` and `MemoryError`, exactly as fixed by the brief. Nothing is wired into
`GameState`. The protocol is *capture before move*: the caller calls `capture_lki_on_move` before
`Zones::move_object` (afterward the old record is gone), then calls
`MemoryTable::apply_zone_change` with the returned `Moved`. For simultaneous state-based
departures (CR 704.8) the caller captures every departing object first and only then moves them.
Failure recovery: the batch is one transaction over every component it mutates: `Zones`, the
`IdGenerator<ObjectId>`, the `LkiStore`, the `MemoryTable` and `Permanents` (all derive `Clone`; the
`LkiStore` clone is a cheap `Arc` snapshot, and mutating a clone never changes the original).
`Permanents` is included because a permanent's departure also mutates it: `Permanents::leave`
removes the permanent and unattaches everything attached to it, so restoring only the other four
would return the object to `Zones` without restoring its permanent state or attachments. Before the
first capture, the caller snapshots all five. On any failed capture or failed `Zones::move_object`,
the caller restores all of those snapshots together, for a single move and for the whole
simultaneous-departure batch alike. So a partly applied batch (earlier moves succeeded, a later one
failed) is fully undone: no object has moved, no ids are consumed, and no LKI record,
`MemoryTable` change, permanent removal or unattachment remains. A retry starts from the original
state and can capture again instead of getting `AlreadyCaptured`. General rule: the transaction
covers every state component the caller's departure handling mutates during the batch (these five
today; any component added later joins the set). Equivalently, the batch may run on clones of all
of those components that are swapped in only when every step succeeds. Restoring only the `LkiStore` is not enough, since it
would drop an earlier object's LKI but leave its move in place. This matches the transactional
rollback that checkpoints (#26 FP-025) will provide once state is wired.
A capture is made iff the request makes the object leave its zone (destination differs, or exile
-> exile / command -> command, CR 400.8, 406.7, 400.10), which matches exactly when
`Zones::move_object` issues a new id; same-zone repositions return `Ok(false)`. Records are keyed
by the old id and never overwritten (`AlreadyCaptured`). Counters with an amount below 1 are
dropped and `attached_here` is sorted and deduplicated at capture, so the record is canonical
whatever the provider returns.
`apply_zone_change`: a same-zone reposition (`old == new`) changes nothing and returns `false`
(Orchestrator clarification of brief clause 4); for a real zone change it returns `true` iff the
old object had a Memory, and the new object then always gets a fresh Memory (possibly empty); an
object with no Memory gives the new object none.
Extras beyond the fixed API: `LinkedStore::define` (a link that is defined but empty, distinct
from undefined, CR 607.5a), `Memory::set_link_source`, `Memory::is_empty`, `MemoryTable::ids`.
**Rationale:** The zone primitive is the single place that forgets the old object (#19 FP-018), so
LKI must be taken before it runs; making the caller sequence capture-then-move is the only way to
get the CR 704.8 "state before any of those actions" semantics without wiring into `GameState`.
**Alternatives:** A wrapper that captures and moves in one call (rejected: cannot express
simultaneous departures and would need write access to zones/); capturing after the move from a
pre-move clone (rejected: costs a full snapshot per move).

## #24 FP-023 — "Generation" of a durable reference

**Decision:** The generation of a `DurableRef` is the `ObjectId` the object had when the link was
made. A reference resolves iff `Zones::object_of_card(card) == Some(generation)` (and the zone kind
matches if one is expected). Any zone change, including exile -> exile, therefore breaks the link
(CR 400.7, 607.2a). The only ways past this are `MoveFinder` (CR 400.7j, scoped to one effect or
cost) and the explicit carry-overs of `apply_zone_change`.
**Rationale:** Object ids are never reused (#19 high-water mark), so the id is a unique generation
counter for free; SRS 2.6 asks for "CardId plus the ObjectId generation at the moment of linking".
**Alternatives:** A separate per-card generation counter (redundant with never-reused ids).

## #24 FP-023 — Registry outcome `Applies`; logic lives in memory/

**Decision:** `ExceptionOutcome` gains `Applies(Exception400_7)`. `exception_400_7d_spell_information`
and `exception_400_7j_effect_finds_moved_object` return `Applies(D)` / `Applies(J)` and their doc
comments name `MemoryTable::apply_zone_change` (D) and `MoveFinder` (J). The other ten stay
`NotImplemented`. The zones module does not import memory/ (doc comments name the items as plain
code text, not intra-doc links), so there is no dependency cycle. The existing registry tests in
`src/zones/tests.rs` and `tests/spec_zones_exceptions.rs` were corrected to assert exactly that
(R10); the two D/J stub tests in `src/zones/tests.rs` were renamed `exception_400_7d_applies` /
`exception_400_7j_applies`.
**Rationale:** The registry signature `fn(ObjectId, ObjectId)` cannot carry the Memory table or an
effect's finder, so the stateful behaviour must live where the state is; the registry stays the
named, tested index of the exceptions (SRS 2.2).
**Alternatives:** Changing the registry signature (a contract change touching every entry);
making zones/ depend on memory/ (an import cycle).

## #24 FP-023 — Characteristics encoding

**Decision:** `Characteristics` has no canonical encoding, so `LkiRecord` encodes it field by field
in declaration order: `names`, `supertypes`, `card_types`, `subtypes` as canonical string sets; the
mana cost as `Option` of its canonical text (`Display`); the color indicator as `Option<u8>` bits;
colors as `u8` bits; loyalty, defense, hand and life modifiers as `Option<i64>`. The placeholder
DSL trees (`rules_text`, `abilities`, `power`, `toughness`) are encoded as tag `0` plus the RFC 8785
canonical JSON bytes (`fineprint_dsl::canonical::to_canonical_vec`, length-prefixed); if that fails
(an integer outside the JSON safe range, a noncharacter), tag `1` plus the rmp-serde bytes; if
both fail, tag `2` alone. The zone id is its kind tag plus the `Option` owner.
**Rationale:** Canonical JSON is already the DSL's hash form (key order and number spelling fixed),
so equal values give equal bytes and any difference changes them; the fallback keeps the encoding
total without panicking (R12). No `Debug` output is used.
**Alternatives:** rmp-serde for everything (float `-0.0`/`0.0` and map-order concerns); `Debug`
text (not a stable format); adding `CanonicalEncode` to `Characteristics` (outside owned paths).
When the real DSL ASTs land (FP-011/012/013) they may get their own canonical encoding; this
function is the one place to switch.

## #24 FP-023 — SRS precision notes (LKI and Memory)

**Decision:** (1) REQ-CORE-007 attaches "(CR 614.12c)" to all choices made as an object enters,
but 614.12c covers only anchor words; chosen values come from 614.12a with the links of 607.2d and
607.2f. `ChoiceValue` covers both; only `Anchor` cites 614.12c. (2) CR 604.7 (static abilities
can't use last known information) and CR 704.8 (LKI of simultaneous SBA departures is derived from
the state before any of them) apply to LKI but are not cited in SRS 2.3; both are implemented here
(`InformationUser::StaticAbility` gets no LKI; the capture-before-move protocol). A one-line SRS
edit could be proposed in docs/HUMAN_QUEUE.md as a P8 item; this unit does not add it.
**Rationale:** Implement what the CR says; record where the SRS citation is narrower.
**Alternatives:** Follow the SRS citation literally (would miss 604.7 and 704.8).

## #24 FP-023 — CR 607.2q inside SpellHistory; other readings

**Decision:** (1) The cards exiled to pay a permanent spell's cost are stored as
`SpellHistory::exiled_to_pay: Vec<DurableRef>` and travel with the spell history from the stack to
the battlefield only (CR 607.2q, 400.7d); they are not copied into the Linked Store. Their durable
references still resolve only while each card stays the exiled generation. (2) A choice key holds
one value: `set_choice` and `set_persistent_choice` each remove the key from the other map, so the
last choice wins ("the last chosen [value]", CR 607.2d). (3) Only the persistent choices (CR
607.2p) survive every zone change; ordinary choices, the Linked Store and link sources never do
(rules-researcher reading: 400.7e/j "can find" is a tracked reference, not a carry-over of Memory).
(4) `MoveFinder` maps each recorded old generation to the newest object; a move into a hidden zone
drops the chain (CR 400.7j says "public zone"); a reposition is never recorded. (5)
`information_for` returns `Current` only when the held id is in a zone of the expected kind; since
a moved object has a new id, a public -> hidden move falls through to LKI (CR 608.2h).
(6) `capture_lki_on_move` takes the permanent state only when the object is on the battlefield.
**Rationale:** Keeps 607.2q data next to the 400.7d data it belongs to and makes the one
carry-over rule (stack -> battlefield) cover both.
**Alternatives:** Writing exiled_to_pay into the Linked Store under a reserved link id
(rejected: no hardcoded link registry, SRS 2.6).

## #25 FP-024: KnownTo is player memory

**Decision:** The stored `KnownTo` set of a card persists across every zone change, including
public-to-hidden moves and the CR 708.9 reveals, except that a move out of a hidden zone the
viewer did not observe forgets the candidates (see "#25 FP-024: Unobserved hidden moves forget
the candidates" below); it is keyed by `CardId`. `Knowledge::on_move`
adds every player when the object left a public zone face up, the owner when it left their hand,
and the controller when it left the battlefield or the stack face down.
**Rationale:** SRS 2.4 defines `KnownTo` as who knows the identity "because it was revealed,
looked at, or seen in a public zone and tracked since". This is a design choice, not CR-derived:
CR 400.7 makes a moved object a new object with no memory of its previous existence, which would
reset the knowledge.
**Alternatives:** Reset `KnownTo` at every zone change per CR 400.7 (players would "forget" a
card they watched go into a library); keep it only for public-to-public moves.

## #25 FP-024: Set knowledge survives a shuffle

**Decision:** `shuffle_library` never touches `KnownTo`; the identities of a library's contents
remain known as a set and the projection exposes them as `LibraryView::known_cards`.
**Rationale:** SRS 2.4 ("It does not clear identity knowledge of the library's contents as a
set"). Design-level, not CR-derived: CR 701.24a speaks only of order.
**Alternatives:** Clearing identity knowledge on shuffle (contradicts SRS 2.4).

## #25 FP-024: A shuffle clears all position knowledge of the library

**Decision:** `shuffle_library(owner, excluded)` clears every viewer's position entries for that
library, excluded found cards included (CR 701.24a, CR 701.24b). `excluded` is validated only
(each id must be in that library, else `NotInLibrary`) and keeps no entry. There is no
disclosure parameter; `shuffle_library_disclosed` is removed. Identity/set knowledge (`KnownTo`)
is never touched. A found card whose placement is disclosed afterwards (CR 401.4) is
re-established by the caller with `set_position_known` after the shuffle call.
`shuffle_face_down_pile` is unchanged: it clears the stored `KnownTo` of the shuffled cards,
which ends their CR 406.3 look permissions, and removes position entries naming them. Every id
is validated before anything changes.
**Rationale:** SRS 2.4: "Shuffling a library clears all position knowledge for that library".
The CodeRabbit pre-merge Linked Issues check on 6cceea3 found that retaining excluded entries
deviated from it. With no entry kept for anyone, the excluded-entry channel (a viewer holding
an entry for A learning whether A or X was the excluded found card) is removed entirely. It is
conservative: position knowledge may be lost, never leaked; the caller restores found-card
placements with `set_position_known` (CR 401.4).
**Alternatives:** Keeping excluded entries for viewers in `known_by`: rejected, it deviates from
the SRS and added an API. Opening an SRS change request: rejected, not needed when the code
follows the SRS. Dropping the `excluded` argument: rejected, it keeps the frozen three-argument
signature and the CR 701.24b validation.

## #25 FP-024: Identity per CardId, positions per (viewer, library owner), pruned on change

**Decision:** Identity knowledge is stored per `CardId` (the `ObjectId` changes at every zone
change, CR 400.7). Position knowledge is a map per (viewer, library owner) from index to
`ObjectId`. `position_known` filters lazily against the live library, and `on_move` also prunes
stored entries of the libraries the move left or entered that no longer match the live library,
besides dropping entries that name `moved.old`. A known card whose index shifts therefore becomes
unknown until `set_position_known` is called again. This coverage holds for moves through
`on_move` / `on_move_observed` / `on_move_indexed`, for insertions into a library outside
`on_move` (`create_object`, `enter_from_outside`) that the caller follows with
`on_library_changed` (or `on_library_changed_indexed`; see "#25 FP-024: on_library_changed"
below), and for removals (`Zones::send_outside` of a library or hand object) that the caller
follows with `on_removed_from_hidden_zone` (or its indexed form; see "#25 FP-024:
on_removed_from_hidden_zone" below); a stale entry left by a change the caller did not report
could otherwise become valid again when a later move restores the earlier shape of the library.
Since PR #174 round 4 the prune applies only to viewers who know the index of the change; every
other viewer loses all its entries for the affected library (see
"#25 FP-024: Position knowledge survives a library change only for viewers who know the index").
**Rationale:** Conservative: knowledge is never re-derived from an omniscient coincidence
(R12, REQ-CORE-004).
**Alternatives:** Lazy filtering only (stale entries can reappear when the library returns to an
earlier shape); shifting entries' indexes on every insert/remove (would need to know where every
move inserted, and claims knowledge of positions after hidden inserts).

## #25 FP-024: The view exposes library knowledge as a set of CardIds

**Decision:** `LibraryView` holds only `count` and `known_cards: BTreeSet<CardId>`; opponent
hands are `HandView::Opponent { count, known_cards }`. No position, no `ObjectId`, no number of
known positions of a hidden zone is in `ZonesView`; a viewer reads its own position knowledge
through `Knowledge::position_known`. Object ids of face-down objects in public zones are in the
view (the object's existence is public). The wire view schema is later work.
**Rationale:** SRS 2.4 ("A redacted library view SHALL expose that set knowledge, never
position"); the ids of hidden objects would correlate with order and identity.
**Alternatives:** Exposing known positions to the viewer in the view (SRS 2.4 forbids it for the
redacted library view).

## #25 FP-024: SRS 2.4 paraphrase gaps

**Decision:** Implement per the CR as quoted for this unit. CR 708.5 covers only face-down spells
on the stack and face-down permanents on the battlefield (including phased-out ones); the owner
of a face-down card has no implicit right elsewhere ("any other zone"). SRS 2.4 does not cite
CR 406.3 (face-down exile looks), CR 400.2, CR 401 or CR 402, which this unit implements as the
visibility rules (REQ-ZONE-001). A one-line SRS precision edit would be a P8 item; this unit does
not add it.
**Rationale:** Implement what the CR says; record where the SRS paraphrase is narrower.
**Alternatives:** Follow the SRS wording literally ("a controller may look at face-down
permanents and spells they control") without the zone restriction.

## #25 FP-024: Error order and validation choices

**Decision:** (1) Errors are checked in this order: `add_known` UnknownObject then
UnknownPlayer; `grant_look` UnknownObject, UnknownPlayer, then NotFaceDown; `can_identify`
UnknownObject then UnknownPlayer (an unknown viewer is an error, not `false`);
`set_position_known` UnknownPlayer (viewer, then library owner), UnknownObject, NotInLibrary,
IndexMismatch (an index past the bottom is an IndexMismatch). (2) `shuffle_face_down_pile`
reports UnknownObject for an unknown id and NotFaceDown for any id that is not face down in
exile (face up, or face down in any other zone), checking every id before any change (see
"#25 FP-024: shuffle_face_down_pile is exile-only" below). (3) `on_move` rule (a) also applies to a same-zone
reposition of a public face-up object (harmless: everyone could already see it). (4)
`KnowledgeError` keeps exactly the frozen derives (`Clone, Debug, PartialEq, Eq`) and is not
`#[non_exhaustive]`, so external tests can match it exhaustively.
**Rationale:** R12 (every caller-supplied id is validated, nothing panics) and the frozen API.
**Alternatives:** `Ok(false)` for an unknown viewer in `can_identify` (silently hides caller
bugs).

## #25 FP-024: Unobserved hidden moves forget the candidates

**Decision:** `Knowledge::on_move_observed(zones, before, moved, observers)` runs a step 0 before
the existing effects (a)-(d), only when `before` was in a library or a hand and
`moved.from != moved.to`. A player V observed the move iff (i) V is in `observers`; or (ii) V
may see the new object by the implicit entitlements of `can_identify` (public zone face up, V's
own hand, a face-down permanent or spell V controls), ignoring stored `KnownTo`; or (iii) the
object left V's hand (V chose it). These three conditions depend only on the caller's observers,
the destination and the source zone, never on which candidate moved. A player who did not
observe it is removed from the stored `KnownTo`
of every card now in `moved.from` and of the moved card (emptied sets are dropped), and, when
the source is a library, loses all of its position entries for that library. An observer's
knowledge is kept exactly as before. `on_move` is `on_move_observed` with no observers. Being
an observer keeps identity knowledge only: since PR #174 round 4 position entries of the
libraries the move left or entered survive only for the viewers in `index_known_by` of
`on_move_indexed` (see "#25 FP-024: Position knowledge survives a library change only for viewers who know the index").

Knowledge cannot tell a disclosed-position move (a draw from the top) from a secret selection
of a library card, so a viewer who knows a position is not an observer by that fact. The caller
must pass such viewers in `observers` when the selected position is itself public. Consequence:
a viewer who knows the top card and is not passed as an observer forgets it when the top card
is drawn; conservative, never leaks.

The round-1 wording of this entry, "or (iv) it left a library where V had a stored position
entry naming it", is superseded (removed in PR #174 review round 2).
**Rationale:** Twin-world non-interference for REQ-CORE-004 (PR #174, F1): a viewer who knows a
set but cannot tell which member moved must not be able to tell from the view; if any card of
the source zone stayed known, two worlds that differ only in which card moved would project to
different views (the moved card shows up in the destination's known cards, or the remaining set
shrinks). This is a design choice, not CR-derived. Set knowledge across `shuffle_library` is
unchanged (SRS 2.4).
**Alternatives:** Forgetting only the moved card (leaks through the remaining set); tracking
per-zone possibility sets ("in zone A or zone B"; a much larger representation); making the
caller pass every observer through an extra `on_move` argument (breaks the frozen `on_move`);
keeping condition (iv) (rejected: observation status depends on `moved.old`, so twin worlds
differ: CodeRabbit round 2, thread 4625001805); inferring observation from position entries
plus a "disclosed" flag (needs a caller-provided flag anyway, which `observers` already is).

## #25 FP-024: shuffle_face_down_pile is exile-only

**Decision:** `shuffle_face_down_pile` rejects, with `NotFaceDown(id)`, every id that is not a
face-down card in exile (a face-up card, or a face-down card in a library, on the battlefield,
on the stack or anywhere else); `UnknownObject(id)` for an unknown id. Every id is validated
before any state changes. Valid exile piles behave as before.
**Rationale:** Look permissions on face-down cards exist only in exile (CR 406.3); a face-down
library card is part of its library and is shuffled with `shuffle_library`, which keeps set
knowledge (SRS 2.4). Clearing its `KnownTo` would drop it from every viewer's library known
cards although it stays in the library (PR #174, F2).
**Alternatives:** Routing library cards through position-only clearing (two behaviours behind
one entry point, and a library shuffle already exists for that).

## #25 FP-024: on_library_changed

**Decision:** `Knowledge::on_library_changed(zones, owner)` drops every viewer's position entry
for the library of `owner` that no longer matches the live library (`UnknownPlayer(owner)`,
state unchanged, otherwise). It is position-only and is the hook for insertions. Caller
contract: after any `Zones` mutation that inserts into a library outside `on_move`
(`create_object` or `enter_from_outside` into a library), call it for that library. After an
insertion at index `n`, entries with an index below `n` are kept and entries at `n` or after it
are dropped, because those objects shifted. Removals (`Zones::send_outside` of a library or
hand object) use `on_removed_from_hidden_zone` instead (see that entry), which also prunes the
library. This closes the case where a stale entry becomes valid again by coincidence (PR #174,
F4: the viewer knows A at index 0, a card is created on top, and drawing that card would put A
back at index 0). Since PR #174 round 4 the keep-below-`n` rule applies only to the viewers in
`index_known_by` of `on_library_changed_indexed`; `on_library_changed` (nobody knows the index)
drops every viewer's entries for that library (see "#25 FP-024: Position knowledge survives a library change only for viewers who know the index").
**Rationale:** The lazy filter compares only the current library, so it cannot see that an
entry was invalidated earlier; pruning at the change is the only point where the shift is
visible. A library is one face-down pile whose order players can't look at or change
(CR 401.2); dropping stale entries is the engine's own conservative policy, not a CR rule.
**Alternatives:** Shifting entries' indexes on every insert (claims position knowledge after a
hidden insert); removing the claim from the identity/position decision without adding a hook
(leaves the stale-entry revival).

## #25 FP-024: Spec-test choices for the hidden-move rule (PR #174 round 1)

**Decision:** The Independent Tester's spec tests (`crates/fineprint-core/tests/spec_knowledge_*.rs`)
pin these points the fix brief left open: (1) rule (d) prunes position entries of both the
library a card moved from and the library it moved to on every move, so a hand-to-library insert
cannot revive a stale entry; (2) superseded: condition (iv) was removed in round 2, and the spec
tests now assert that position knowledge is not observation and that observers are explicit
(only `observers`, the implicit entitlements on the new record and the source hand count); (3) observing a move only keeps existing
knowledge, it never adds the observer to the card's `KnownTo`; (4) error precedence among
several invalid ids in one `shuffle_face_down_pile` call is not specified (tests use one invalid
id per pile); (5) removals (`Zones::send_outside` of a library or hand object) use
`on_removed_from_hidden_zone` and insertions (`create_object`, `enter_from_outside`) use
`on_library_changed`, which is position-only; the spec tests assert position entries after
insertions and both identity and position after removals; round 4 added index disclosure (see
"#25 FP-024: Position knowledge survives a library change only for viewers who know the index").
**Rationale:** (1) and (2) match the implementation, which the shadow-model property test
confirms; (3) follows "if V observed it, V's knowledge is kept"; (4) and (5) are left open
rather than over-specified until the integration unit defines the callers.
**Alternatives:** Pruning only the source library (fails the shadow model and allows the F4
revival through a hand-to-library insert).

## #25 FP-024: on_removed_from_hidden_zone

**Decision:** `Knowledge::on_removed_from_hidden_zone(&mut self, zones: &Zones, before:
&ObjectRecord, observers: &[PlayerId]) -> Result<(), KnowledgeError>`, called AFTER
`Zones::send_outside` with `before` copied before the removal. It applies the step-0 forgetting
rule of `on_move_observed` to a removal, through the same private helper: a player V observed
the removal iff (i) V is in `observers` or (iii) `before.zone` is V's hand; (ii) does not apply
(there is no new object). Every non-observer is removed from the stored `KnownTo` of every card
now in `before.zone` and of `before.card` (emptied sets dropped) and, for a library source,
loses all position entries for that library; observers keep their knowledge. A library source
is then pruned against the live library as `on_library_changed` would. Since PR #174 round 4
that prune applies only to the viewers in `index_known_by` of
`on_removed_from_hidden_zone_indexed`; every other viewer, observers included, loses all its
entries for that library (see "#25 FP-024: Position knowledge survives a library change only for viewers who know the index"). Errors, checked first
and leaving the state unchanged: `UnknownPlayer(o)` when `before.zone` is the library or hand of
an unknown `o`, then `UnknownPlayer` for the first unknown observer. When `before.zone` is not a
library or a hand the call is a no-op returning `Ok` before any validation (the brief's literal
"does nothing and returns Ok"; a public-zone removal leaks nothing). The hook never reads
`zones.object(before.id)`. Caller contract: every `Zones::send_outside` of a library or hand
object is followed by this call, with the players who watched which card left as observers.
**Rationale:** Twin-world non-interference (REQ-CORE-004): with only the position prune of
`on_library_changed`, removing A vs B from a library whose set {A, B} P2 knows leaves {B} vs
{A} in P2's projection, so P2 learns which card left (PR #174 round 3, comment 4625229511).
This is a design choice, not CR-derived; the CR context is the hidden ordered library
(CR 401.2). The same entry records that round-3 finding 4625229503 (a known position treated as
observation of a private library-to-face-down-exile selection) was already closed in round 2 by
dropping step-0 condition (iv); unit test `twin_worlds_undisclosed_selection_to_face_down_exile`
pins it.
**Alternatives:** Leaving it to the caller (rejected: a standalone projection leak); extending
`on_library_changed` with observers (rejected: it is also the insertion path, where nothing is
forgotten); a Knowledge-owned wrapper around `Zones::send_outside` (rejected: Knowledge would
mutate Zones, R1/R2).

## #25 FP-024: Position knowledge survives a library change only for viewers who know the index

**Decision:** Position knowledge about a library survives a change of that library's contents
(a card moved out, moved in, removed, inserted or repositioned) only for the viewers the caller says know the index of the change. Every other viewer
loses all its position entries for every affected library. Identity/set knowledge is not
touched by this rule; it stays governed by step 0 and the removal hook, i.e. by `observers`.
`observers` and `index_known_by` are separate: an announced identity is not an index, and a
known index does not make a viewer an observer (nor undo step 0). Four methods carry the
disclosure: `on_move_indexed(zones, before, moved, observers, index_known_by)`,
`on_removed_from_hidden_zone_indexed(zones, before, observers, index_known_by)` and
`on_library_changed_indexed(zones, owner, index_known_by)`. The shorthands pass an empty set
(`on_move` = `on_move_indexed(.., &[], &[])`, `on_move_observed`
= `on_move_indexed(.., observers, &[])`, `on_removed_from_hidden_zone` = indexed with `&[]`,
`on_library_changed` = indexed with `&[]`). A shuffle clears all entries of the library for
every viewer (see "#25 FP-024: A shuffle clears all position knowledge of the library"); the
shuffle shorthand relationship is gone. The
default is that nobody knows the index, so nobody keeps positions for the affected library.
Affected libraries: `moved.from` and `moved.to` when they are libraries (one library for a
reposition), the removal source library, the library of `owner`. For a viewer in the set the
earlier semantics apply (the entry naming the moved or removed object goes, entries whose object
shifted go, entries still matching the live library stay). The move, removal and insertion
paths share one private helper (`apply_library_change`). Errors, state unchanged: `on_move_indexed` UnknownObject(moved.new),
then the first unknown observer, then the first unknown `index_known_by` player;
`on_removed_from_hidden_zone_indexed` UnknownPlayer(zone owner), then observers, then
`index_known_by`; `on_library_changed_indexed` UnknownPlayer(owner), then `index_known_by`;
`shuffle_library` UnknownPlayer(owner), then NotInLibrary for the first bad excluded id.
`KnowledgeError` is unchanged. Caller contract: pass a
viewer as knowing the index only when the index is public to it (a draw of a top card the
viewer knows is on top needs it in both `observers` and `index_known_by`; an insertion announced
as "third from the top").
**Rationale:** Twin-world non-interference (REQ-CORE-004): pruning against the live library
keeps an entry when the change was below it and drops it when the change was above it, so a
viewer who did not know the index learns on which side the change happened (PR #174 round 4,
comments 4625492354 and 4625492361). The class sweep found six instances of this channel, all
closed here: (a) `on_move` effect (d) for a move out of a library; (b) a move into a library at
an undisclosed index; (c) a same-zone library reposition; (d) `on_removed_from_hidden_zone`;
(e) `on_library_changed` after `create_object` / `enter_from_outside`; (f) `shuffle_library`
with `excluded`, where keeping an excluded card's entry told a viewer which card was found
(closed in round 6 by clearing every entry on a shuffle). Swept and found independent of hidden information: `set_position_known`,
`on_player_leaves`, `on_game_end`, `shuffle_face_down_pile`, `grant_look`, `add_known`,
`reveal_to_all`, the projection counts (CR 401.3, CR 402.3) and step-0 identity forgetting. A
design decision, not CR-derived; the CR context is the hidden ordered library (CR 401.2).
**Alternatives:** Keeping the prune semantics for observers (rejected: an observer knows which
card left, not where, so it still leaks the index side); clearing identity too (rejected: set
knowledge must survive, SRS 2.4); one disclosure struct argument instead of extra methods
(rejected: larger churn on the frozen API).

## #25 FP-024: Spec-test choices for index disclosure (PR #174 round 4)

**Decision:** The Independent Tester's round-4 spec tests (1) compare every player not in
`index_known_by` in the fixed twin tests, observers included (the announced card is the same in
both worlds); (2) build the Removal and MoveOut twin worlds by reordering the clone's library with
same-zone moves `Knowledge` is not told about, keeping every index that holds a live position
entry, and skip a twin that is not indistinguishable to every player before the change; (3) count
the generator's disclosed-index controls rather than asserting each instance (the fixed tests
assert their controls exactly); (4) leave unasserted what a public-source removal no-op does with
an unknown `index_known_by` player.
**Rationale:** (1) is stronger than required and safe; (2) makes the hidden-index twin reachable
without changing what any player could know; (3) and (4) avoid over-specifying cases the brief
left open.
**Alternatives:** Comparing only non-observers (weaker); generating twins from scratch (rarely
reachable with live position entries).

## #25 FP-024: Leak-property test choices (PR #174 round 5)

**Decision:** In `tests/spec_knowledge_leak_property.rs`: (a) `create_object` insertions made by
the generic step generator count toward the `create_object` counters, separately from
`enter_from_outside`; (b) since round 6 a shuffle clears everything, so the spec tests assert every entry gone and a
restored-order resurrection probe, and the `known_by` / stale-excluded checks (and the
`stale_excluded` counter) were removed; (c) when `index_known_by` is not empty, the
MoveOut and Removal index twins sometimes seed an entry for a disclosed viewer on a card other
than the announced one, so the disclosed-index control stays reachable. Pre-change equivalence of
twin worlds is decided by the shadow model; a model-equivalent pair is never discarded and must
give equal production views and positions.
**Rationale:** Each counter and check then proves the path it names; (b) is the only way to
observe a kept stale entry through the public API; (c) changes only which states are generated.
**Alternatives:** Asserting `position_known` is empty after the shuffle (passes through lazy
filtering even when the entry is kept); discarding twins whose production views differ (could
hide a leak).

## #25 FP-024: Leak-property seeds raised to 16 (PR #174 round 6)

**Decision:** `tests/spec_knowledge_leak_property.rs` runs 16 seeds × 250 steps (was 8) for 2 and
3 players. The disclosed-index control counter (`idx_control > 0`) is not asserted for the
Shuffle class, because a shuffle now discloses no position to anyone; that class instead asserts
`idx_twins`, `idx_compared`, `idx_holder_compared` and the shuffle emptiness and
resurrection-probe counters > 0.
**Rationale:** Removing the shuffle disclosure shifted the generator's random stream and two
chance-dependent control counters (Removal, MoveOut) fell to 0 with 8 seeds; more seeds restore
coverage without weakening any assertion.
**Alternatives:** Re-tuning the generator's distribution (more fragile); dropping the controls
(weakens the suite).

## #26 FP-025: Generic `Transactional` trait over the state

**Decision:** Rollback lives in a new module `fineprint_core::txn` as `Transactions<S:
Transactional>`, a live state plus a stack of frames (checkpoint clone, durable records). The
trait asks the state for `state_hash`, `priority_player` and `carry_durable(restored, before,
after)`; `GameState` implements it using only its public API.
**Rationale:** `GameState`'s fields are private to `state/`, which this unit may not edit (R1).
A checkpoint is one `clone()`, which is a cheap Arc-sharing snapshot (#18 FP-017), and restoring
is replacing the live value, so the restored hash is identical by construction (CR 734.1). The
trait also lets the bookkeeping be tested on a toy state with no `GameState` at all.
**Alternatives:** An undo log of inverse operations (every later mutator would have to record
its inverse; easy to get wrong, and more than one clone of work); methods on `GameState` (needs
edits in `state/`, outside the owned paths).

## #26 FP-025: Durable-record propagation rule

**Decision:** Each open frame keeps an ordered, non-overlapping list of steps
`Entry { before, after, durable }` and a cursor: the live state up to which the steps account for
its history (from the cursor to the live state the history is continuous). Own steps
(`durable: false`) are the frame's own changes, which its rollback reverses; durable steps survive
it. When a finished child F is handed to its parent P (with no parent everything is dropped, so
records are dropped when the root frame is popped): `commit(F)` with no steps does nothing;
`commit(F)` with steps gives P an own step `(P.cursor, F.checkpoint)`, F's steps with their tags,
and an own step `(F.cursor, live)`, and moves P's cursor to `live`; `commit_durable(F)` with a
parent gives P an own step `(P.cursor, F.checkpoint)`, every step of F re-tagged durable, and a
durable step `(F.cursor, live)` (with F holding no steps this is the earlier single record
`(F.checkpoint, live)`), and moves P's cursor to `live`; at depth 1 it is a plain `commit`.
`rollback(F)` restores F's checkpoint and applies only F's durable steps, in order, each with its
own `carry_durable(restored, before, after)` call; if F held durable steps and has a parent, P
gets an own step `(P.cursor, F.checkpoint)` and F's durable steps, and P's cursor becomes the
post-rollback live state; otherwise P is untouched. Steps are never merged or dropped for being
empty. Only the innermost transaction may be committed or rolled back; a failed call changes
nothing.
**Rationale:** Net rule (CR 734.1, SRS §2.5): after rolling back an ancestor the state is its
checkpoint with every durable effect beneath it applied exactly once, in time order, each as its
own step. A carry need not be linear (the counter carry clamps at 0), so one net pair per durable
frame loses effects: root checkpoint 1, root sets 5, inside durable A durable C1 sets 0 and durable
C2 sets 5; A's net pair (5, 5) leaves 1 after the root's rollback, the steps give clamp(1 - 5) = 0
then + 5 = 5 (CodeRabbit PR #175 round 3). Each change lies in exactly one step, so in the
append-only case nothing is applied twice (the round 1 finding stays fixed). The steps are kept
as the frames' history rather than reconstructed from the durable records alone, because a
rolled-back child with a durable grandchild makes the live state jump: A checkpoint 0, child X
+2, durable grandchild G 2 -> 7, +1, X rolls back (live 5), A commits durably; filling the gaps
between records ((0, 2), (2, 7)) gives 7 after the root's rollback, the correct answer is 5.
Memory cost: no clone for a checkpoint beyond its snapshot, a plain commit of a frame with no
steps, or a rollback with no durable step; otherwise at most three state clones per durable
commit, per plain commit of a frame holding steps, and per rollback that forwards durable steps
to a parent (the parent's cursor when it is not already an owned state, the child's cursor or
checkpoint, and the live state).
**Alternatives:** A single covering record per durable frame, dropping inherited records (loses
non-linear effects: the round 3 finding); forwarding inherited records plus gap steps between
them (unsound with rolled-back children, as above); forwarding the inherited records and the
covering record (applies descendant effects twice: the round 1 finding); a general state diff
type recorded at commit (the state has none yet); allowing out-of-order commit (no rules
procedure needs it; it makes nesting ambiguous).

## #26 FP-025: SnapshotStore semantics

**Decision:** `SnapshotStore<S: Clone>` is a bounded `VecDeque` of snapshots, oldest first.
`new(0)` is `ZeroCapacity`; `record` appends a clone and drops the oldest when full; `rewind(n)`
returns the snapshot `n` records before the latest (n = 0 is the latest), discards the `n`
newest, and for `n >= len()` returns `NotEnoughSnapshots` leaving the store unchanged. It has no
dependency on `Transactions` and no FormatConfig gate.
**Rationale:** SRS §13.3 item 7 `RewindToSnapshot` undoes N decision ticks; SRS §2.5 makes
engine-level undo a separate feature not governed by CR 734. FormatConfig does not exist yet, so
the gate "never exposed in rules-enforced matches unless FormatConfig allows it" belongs to the
later API unit that exposes `RewindToSnapshot`.
**Alternatives:** Unbounded history (memory grows without limit in long sandbox sessions);
building rewind on `Transactions` (mixes sandbox undo with rules rollback, which §2.5 forbids).

## #26 FP-025: GameState carry rule and its limits

**Decision:** `carry_durable` for `GameState` carries, when the component differs between
`before` and `after`: the PRNG (set to `after`'s), each of the card, object, ability-instance,
effect, timestamp and link generators (issued on `restored` until it reaches `after`'s
`last_issued`; never rewound; stops without panicking if issuing fails), and for each player in
`before`, `after` and `restored` whose state changed `before` -> `after`, only that change
applied over the restored player: life gets `after - before` added (saturating); each counter
kind whose count changed gets its per-key delta added, clamped at 0 (the entry is removed),
because a count is never negative in this state model; each designation whose value changed
(added, changed or removed) takes `after`'s value or is cleared, because designations are not
additive. Keys unchanged `before` -> `after` are not touched. This supersedes clause 4(iii) of
the Implementer brief (copy from `after`): SRS §2.5 requires that rolling back the outer action
leaves only the durable sub-transaction's effects in place, so outer changes made before the
sub began must be reverted (CodeRabbit PR #175 round 1). It never carries turn number, active player,
priority, turn order, the player generator or players added meanwhile, the effect registry or
the history ledger. Components added to `GameState` by later units (zones, permanents, memory,
knowledge) must be added to this impl by those units. The library-touching zone data a CR 734.1
category (b) mana ability changes cannot be carried yet: zones are not part of `GameState`, and
`state/` and `zones/` are owned by other units (R1), so wiring them is a later unit's change
(through R2 if a contract changes).
**Rationale:** These are the only `GameState` components today a durable sub-transaction can
change; not carrying priority keeps CR 734.2 priority retention true. PRNG and generator values
are absolute, so ids issued by the durable sub-transaction are never issued again. Limit: the
PRNG and the id generators are carried as absolute values at the durable commit, so when the
durable frame's window starts at an outer checkpoint they can include draws and ids made by the
parent action before the sub began; a PRNG delta cannot be expressed.
**Alternatives:** Copying the whole player from `after` (rejected: carries the reversed outer
action's earlier changes to the same player); carrying the whole `after` state (would also
carry the reversed action's changes and priority).

## #26 FP-025: Spec-test choices

**Decision:** In `crates/fineprint-core/tests/spec_txn_*.rs`: (1) the brief's "P0"/"P1" are the
first and second seats, `PlayerId(1)` and `PlayerId(2)`, because `GameState::new` issues players
`1..=n`; (2) the reference model keeps each frame's history as ordered `(before, after)` steps
tagged own or durable: a durable commit hands the parent an own step up to the frame's
checkpoint, the frame's steps made durable and a durable step from its cursor to the live state;
a rollback carries only durable steps, one `carry_durable` call each, in time order, and hands
them up; so each durable effect is carried exactly once, step by step, never as one net pair
(see "Durable-record propagation rule"); (3) the "never issued" id is taken from a second `Transactions` advanced further than the
instance under test, as the brief allows; (4) the id-generator carry is only asserted where the
outer action issued no ids before the durable sub began, because i(ii) does not say whether
"differs" compares `after` with `before` or with `restored`.
**Rationale:** (1) and (3) follow the public API; (2) follows the propagation rule; (4) avoids asserting a case the brief leaves
open.
**Alternatives:** Excluding nested durable-in-durable sequences from the property model (weaker);
asserting one reading of i(ii) (could report a false defect).

## #26 FP-025: TxnId is instance-local

**Decision:** A `TxnId` is valid only on the `Transactions` that issued it, and callers must
keep each handle with its instance. Using a handle issued by another instance is a caller error
with an unspecified result: `UnknownTxn`, `NotInnermost`, or, if its number equals an open
frame's, treatment as that frame. This is documented on `TxnId` and `Transactions`; no code
change (CodeRabbit PR #175 round 1).
**Rationale:** Rejecting foreign handles needs an instance identity in each `TxnId`, which needs
a global counter or other global mutable state; R3 forbids that, and the deterministic engine
has no per-instance source of identity.
**Alternatives:** An instance id inside `TxnId` (needs global mutable state, R3).

## #26 FP-025: Property oracle compares observable state, not carry call lists

**Decision:** In `tests/spec_txn_property.rs` the per-step comparison between the reference model
and `Transactions` uses each harness's observable state. For the append-only Toy this is the
state with its recorded `carry_durable` call list cleared; for `GameState` and the clamped state
it is the full state. The Toy's appended log is still checked exactly against the independently
tracked bytes, and flat cases keep exact call-count and call-argument assertions in
`spec_txn_durable.rs` (CodeRabbit PR #175 round 3).
**Rationale:** Under the entry/cursor propagation rule the number of `carry_durable` calls for a
durable frame that itself holds durable work is not specified (zero-width own segments may be
re-tagged durable and passed as no-op steps), so a correct implementation may produce a
different call list. The effect of the calls is fully specified and is what the oracle checks.
**Alternatives:** Compare the full Toy including its call list (over-specifies an unspecified
detail); drop the Toy from the property run (loses the independent byte oracle).

## #26 FP-025: Clamped property oracle shares the cursor rule (deferred)

**Decision:** keep the clamped reference model on the specified entry/cursor rule for this PR; the cursor-rule error class (cursor not moved to the post-rollback state) is pinned by the fixed tests `counter_example_gamestate_counter_and_life`, `ntoy_counter_example_all_variants` and lib `req_core_005_cr_734_1_durable_after_rolled_back_child_not_replayed`, which fail if the cursor is not moved to the post-rollback state.

**Rationale:** an action-level non-linear oracle must re-derive which effects survive which rollback, i.e. re-implement the same semantics in another form, a larger change than this review round; the random runs still check every step of the specified model against the implementation, and the fixed cases cover the shared-rule risk CodeRabbit names (PR #175 round 4).

**Alternatives:** action-level effect tracking for the clamped state (deferred; candidate follow-up when the casting unit first uses durable sub-transactions).

## #27 FP-026 — Spec-test choices

**Decision:** The Independent Tester's spec tests (`crates/fineprint-core/tests/spec_events_*.rs`)
read the brief as follows where it is silent or allows two readings, and assert only the common
ground: (1) `EventState`, `ObjectMarkers`, `History`, `EventLog`, `Zones`, `Permanents`,
`MemoryTable` and `LkiStore` are compared through their canonical bytes, never through
`PartialEq`/`Debug` (the brief lists only `Default; Clone; CanonicalEncode` for `EventState`);
`GameState` and `IdGenerator` are compared by value. (2) Attaching to an unknown player, and a
`TurnBegins` whose turn number would overflow, are asserted only as "some error, nothing changed"
(the brief names no variant). (3) Error precedence between checks (for example a negative amount
against an unknown target) is not asserted; every error test uses inputs with exactly one fault.
(4) The ledger key of an event that follows a `TurnBegins` in the same batch is not asserted;
only events in separate calls are. (5) "Replaced" events are not modelled in this unit, so the
"replaced events are not counted" clause is tested through no-op, failed and rolled-back events.
(6) LKI counters and attached objects are those of the `CharacteristicsProvider` (as
`capture_lki_on_move` defines), so LKI tests set them on the test provider rather than through
`AddCounters`. (7) The world starts at turn 1 with player 1 active (`set_turn_number(1)`),
since `GameState::new` is pre-game (turn 0). (8) Self-attachment is asserted as
`EventError::Permanent(_)` ("a cycle or other Permanents error"), and an unknown object
(in no zone) named by Tap/Untap/Attach as `UnknownObject`.
**Rationale:** R10/R12 black-box discipline: assert exactly what the brief states, no more; the
comparisons above need only the traits the brief guarantees.
**Alternatives:** Assert specific error variants or `PartialEq` on `EventState` (over-specifies
items the brief leaves open); route LKI counters through `ObjectMarkers` (contradicts the
existing `capture_lki_on_move` contract).

## #27 FP-026 — Events that change nothing are no events

**Decision:** These execute as no-ops (`Ok(None)`, nothing changed, logged or ledgered): gaining
0 life (CR 119.9, CR 119.10); losing 0 life, treated the same by analogy (no CR rule says so);
0 damage (CR 120.8); putting or removing 0 counters, and removing counters when none are held;
tapping a tapped and untapping an untapped permanent (CR 701.26a, CR 701.26b); turning a
permanent to the face it already shows; becoming a copy of what it already copies; attaching to
the current host (CR 701.3b); unattaching an unattached object. A negative amount is
`InvalidAmount`. The amount checks come before the target checks, in the order the brief lists
them (0 counters on an unknown object is a no-op, not an error).
**Rationale:** No event means nothing for triggers or replacement effects to see (CR 119.9,
CR 120.8); a single rule for all "nothing happens" cases keeps the log clean for trigger
matching.
**Alternatives:** Logging zero events with a flag (would let triggers fire on non-events).

## #27 FP-026 — The ledger reads executed events only

**Decision:** `History` is fed only from events that actually executed (after replacement and
prevention, which happen before execution), keyed by the game turn number at execution time
(after a `TurnBegins` in the same batch, the new turn) and the player concerned. Damage to a
player also records `LifeLost` (CR 119.2, CR 120.3a) besides `DamageDealtToPlayer`;
`DamageDealtBy` is keyed by `source_controller` when it is `Some` (not validated against the
player list). `record_spell_cast` is the one non-event input, because casting belongs to the
casting unit (CR 601.2i). Sums saturate; a `LifeChange` of `i64::MIN` records `i64::MAX` lost.
**Rationale:** "life lost this turn" must include damage, and a prevented or replaced thing
never happened.
**Alternatives:** Recording at submission (would count replaced events); a separate casting
event (casting is out of scope).

## #27 FP-026 — Standalone event state and the rollback set

**Decision:** Object counters, marked damage, token card ids and copy-of links live in
`ObjectMarkers`; with the `EventLog`, `History`, the current phase/step and the
empty-library-draw set they form `EventState`, which is not wired into `GameState` (its Merkle
layout and wire form are untouched, and so is `GameState`'s placeholder `HistoryLedger`). The
`ids` field of `EventContext` is the generator `Zones` uses, because `GameState`'s own object
generator is not reachable. `execute_batch` snapshots and, on any error, restores game, zones,
ids, permanents, memories, LKI and the event state together.
**Rationale:** The standalone pattern of #18-#26; the integration unit joins the components.
**Alternatives:** Adding fields to `GameState` (outside the owned paths and a wire-format change).

## #27 FP-026 — Event set and causes

**Decision:** `GameEvent` (`#[non_exhaustive]`) is exactly the SRS 8.1 list plus `Unattach`
(CR 701.3d). Sacrifice, destroy, discard, mill, exile and countering a spell are `ZoneChange`
with a `ZoneChangeCause` label that does not change execution. Shuffle, phasing (CR 702.26d: not
a zone change), control change (CR 613.1b), winning and losing, and destroy's own replaceable
event (CR 701.8) are later units' events. Phase and step beginnings cite CR 500.6 and CR 603.2b
(not 500.8); token creation cites CR 111.2 and CR 701.7a; CR 108.4 is not cited for control.
**Rationale:** The brief's fixed API; `#[non_exhaustive]` lets later units add events.
**Alternatives:** One variant per keyword action now (premature; their replacement handling is
not designed yet).

## #27 FP-026 — Draws: empty library and replaced draws

**Decision:** A draw from an empty library is no event and adds the player to the
empty-library-draw set (CR 121.4, CR 704.5b input; `take_empty_library_draws` clears it); the
flag is part of the rollback set. A draw that a replacement effect replaced never reaches
execution, so it is never counted (CR 121.6a, CR 121.5). A draw captures LKI like any zone exit.
**Rationale:** CR 704.5b reads "attempted to draw ... since the last time state-based actions
were checked".
**Alternatives:** Making the empty draw an error (would roll back the whole batch).

## #27 FP-026 — Tokens off the battlefield and token creation

**Decision:** A `ZoneChange` of an object whose card id is marked as a token and which is not on
the battlefield is a no-op (CR 111.8). The token mark is per `CardId` and survives the token's
zone change (ceasing to exist is the CR 704.5d state-based action, later). Token creation does
not model CR 111.5 (the caller decides whether the token can be created) and records no LKI
(there was no previous object); unknown owner or controller is `UnknownPlayer`, a duplicate
card id surfaces as `EventError::Zone(CardAlreadyPresent)`.
**Rationale:** The brief's semantics; CR 111.8 says the token remains in its current zone.
**Alternatives:** An error for the token move (would roll back simultaneous moves of other
objects).

## #27 FP-026 — Damage to objects and object-naming errors

**Decision:** Damage to an object only marks damage (CR 120.3e); creature/planeswalker/battle
legality (CR 120.1a) and the CR 120.3b-d and CR 120.3f-h results are later units'. For every
event that names an object, an id no zone holds is `UnknownObject` (this is also what an event
gets for an id an earlier event of the same batch replaced), and an object in a zone that is not
a held permanent is `NotOnBattlefield` where the event needs a permanent (damage to an object,
tap/untap, attach, become-copy, face up/down). `Unattach` of an object in a zone that is not
attached (for example in a hand) is a no-op.
**Rationale:** One consistent rule; the brief names `UnknownObject` for replaced ids in a batch
and for tap/attach, and `NotOnBattlefield` for objects off the battlefield.
**Alternatives:** `NotOnBattlefield` for unknown ids too (contradicts the batch rule).

## #27 FP-026 — Attach timestamps and illegal attachments

**Decision:** Attaching to the current host is a no-op (CR 701.3b). A cycle, a self-attach or
an unknown object is an error that rolls back the batch (`EventError::Permanent` /
`UnknownObject` / `NotOnBattlefield` / `UnknownPlayer`). Enchant/equip legality is the caller's.
Every real attach issues a new timestamp from `GameState::issue_timestamp` (CR 701.3c), returned
only in `Outcome::Attached` (Permanents holds no timestamps; the layer unit stores them).
**Rationale:** The brief; no component stores per-permanent timestamps yet.
**Alternatives:** Storing the timestamp in `ObjectMarkers` (not in the fixed API).

## #27 FP-026 — Same-zone reposition

**Decision:** A `ZoneChange` that only repositions an object in its zone (library, hand,
graveyard, battlefield, stack) is an executed, logged event with `Outcome::ZoneChange` where
`old == new`, but not a zone change (CR 400.7): no LKI, markers (counters, damage, copy link)
kept, the permanent untouched, Memory untouched.
**Rationale:** It changes library/graveyard order, which later units may observe; it is not a
new object.
**Alternatives:** Treating it as a no-op (loses an observable order change).

## #27 FP-026 — Removing counters reports the actual number

**Decision:** `RemoveCounters` removes `min(amount, held)` and reports that number in
`Outcome::CountersRemoved { removed }`; with nothing held it is a no-op.
**Rationale:** Later units ("for each counter removed this way") need the real number.
**Alternatives:** An error when fewer are held (a rule-legal effect would fail).

## #27 FP-026 — LKI capture order inside a batch

**Decision:** `execute_batch` first runs the batch once without capturing LKI to find, in
order, exactly which objects leave their zones (a draw's card depends on library order, which
earlier events of the same batch can change), restores the snapshot, captures LKI for every
such object that exists at the start of the batch (CR 704.8, CR 603.10a), and then executes the
batch for real. An object that only exists because of an earlier event in the same batch (for
example a token created and then moved) is captured just before its own move. `lki_captured` is
true iff a record was stored for the departing object. LKI counters are the provider's counters
overridden per kind by the counters `ObjectMarkers` holds, and the objects attached here are the
provider's plus the permanents attached to it in `Permanents`, so the record shows the counters
and attachments the events module itself put there.
**Rationale:** The brief requires capture of every departing object before any move; a planning
run is the only exact way to know which library card each draw takes. The provider wrapper
keeps `capture_lki_on_move` unchanged while making the record match the state this module owns.
**Alternatives:** Predicting the k-th card of each library (wrong when the batch also moves
library cards); capturing just before each move (violates CR 704.8 for simultaneous
departures); provider counters only (the record would miss counters put by events).

## #27 FP-026 — CR citation index

**Decision:** The new CR citations in `crates/fineprint-core/src/events/` were checked against
the CR 2026-11-06 text (every cited rule exists), but `docs/cr-citations.json` is outside this
unit's owned paths, so `cargo xtask cr index` is left to the Orchestrator after both roles
finish; until then `cargo xtask cr check` reports the new citations as "not in
docs/cr-citations.json".
**Rationale:** R1 owned paths.
**Alternatives:** Regenerating the index from the Implementer (edits a file outside the owned paths).

## #27 FP-026 — LKI sources: markers govern counters, Permanents govern attachments

**Decision:** (CodeRabbit round 1, F1/F2.) `ObjectMarkers` is the single source of counters on
objects: `AddCounters`, `RemoveCounters` and the LKI record all read and write it, and the
caller's `CharacteristicsProvider::counters` is not consulted by the events module. The
permanent container is the single source of the objects attached to a permanent: the LKI
record's `attached_here` is exactly `Permanents::attachments_of(AttachTarget::Permanent(id))`
(ascending), and the provider's `attached_here` is not consulted. This supersedes the
"provider counters overridden per kind by the markers" and "provider's attachments plus
`Permanents`" text of "LKI capture order inside a batch". The Implementer's unit test
`req_event_001_lki_counters_merge` is replaced by `req_event_001_lki_counters_single_source`
(R10: a recorded contract correction, not a weakened assertion; the counters the events put
are still asserted exactly, and the provider-only kind is now asserted absent).
**Rationale:** With two sources a provider count and no marker made `AddCounters` start from 0
and replace the total, `RemoveCounters` find nothing, and LKI report the provider total again;
`Unattach` edits only `Permanents`, so a stale provider attachment survived into LKI. A
"provider baseline plus marker tombstone" model would need markers to store 0, breaking the
"amounts >= 1" contract and the canonical encoding. The events module is what puts counters
and attaches, so its own state is the authority (CR 122.1, CR 122.2, CR 701.3a, CR 701.3d).
**Alternatives:** Provider as baseline with zero-count tombstones (encoding change); reading
the provider in `held_counters` too (two writers of one value); the union of both attachment
sources (stale entries survive `Unattach`).

## #27 FP-026 — Draw of a token in a library

**Decision:** (F4.) If the top card of the library is a token's card (`ObjectMarkers::is_token`),
`Draw` is a no-op (`Ok(None)`): no move, no LKI, no `CardsDrawn`, no log entry, and no
empty-library flag (the library is not empty). Same rule as `ZoneChange` of a token off the
battlefield.
**Rationale:** CR 111.8: such a token "remains in its current zone instead"; a blocked move is
not a draw (CR 121.1 puts the top card into the hand, which does not happen), and CR 121.4 is
about an empty library.
**Alternatives:** An error (would roll back simultaneous events); skipping to the next card
(not what CR 121.1 says).

## #27 FP-026 — Face-down state kept in the zone record and Permanents together

**Decision:** (F5, F6, F10.) The zone record's `face_down` and the permanent's
`status().face_down` never disagree after an event. (1) A `ZoneChange` onto the battlefield
sets the new permanent's face status from the zone record (a face-down entry is face down).
(2) A same-zone battlefield reposition (where `Zones` overwrites the record's `face_down` with
the request's) sets the permanent's status to the record's. (3) `TurnFaceUp`/`TurnFaceDown`
update both: `Zones` has no public face setter and zones/ is not owned by #27, so the record is
updated by a same-zone battlefield reposition (`MoveRequest { to: Battlefield, position: Top,
face_down, controller: Some(record.controller) }`; same id, `ids` untouched, the battlefield is
unordered; the controller is passed explicitly because `None` would fall back to the owner),
then `Permanents::set_face_down`. Both happen inside the batch, so a later failure rolls both
back. The event is a no-op only if both already equal the requested status; otherwise it
repairs them and occurs (`Outcome::Applied`). A later zones/integration unit may add a direct
setter and replace the reposition. This settles, for events, the #20 note that the record's
`face_down` is "reconciled with `Status::face_down` by the integration unit".
**Rationale:** The redacted projection (REQ-CORE-004) and LKI capture read the zone record: a
permanent turned face down only in `Permanents` stayed identifiable to an opponent (a
hidden-information leak, R12) and its LKI said face up while its permanent snapshot said face
down. CR 701.27b and CR 708.8: turning face up or down is an action on the same object (no
zone change, CR 400.7).
**Alternatives:** Making the projection read `Permanents` (knowledge/ is not owned and the
projection takes only `Zones`); adding a setter to `Zones` (outside the owned paths; left to a
later unit).

## #27 FP-026 — Same-zone reposition reports no LKI

**Decision:** (F3.) A same-zone reposition always reports `lki_captured == false`, even when the
same object was pre-captured because a later event of the batch (for example a draw of the
only library card) moves it out of its zone; that later event reports `true`.
**Rationale:** The flag belongs to the event that departs (CR 400.7: a reposition is not a zone
change; CR 603.10a, CR 704.8 capture is for the departure).
**Alternatives:** Reporting the pre-capture on the first event naming the object (misattributes
the record to a non-departure).

## #27 FP-026 — History copy-on-write cost

**Decision:** (F12, deferred.) Keep the `Arc::make_mut` pattern in `History`: the planning run
and the real run each copy the history map on their first history write after a snapshot.
**Rationale:** The same copy-on-write pattern as `Zones`, `Permanents`, `MemoryTable` and
`LkiStore`; no SRS requirement or issue criterion bounds it; changing it would redesign the
batch executor's planning/rollback. Revisit when a performance or replay unit measures it.
**Alternatives:** Staging history updates until the batch succeeds; a persistent map that
shares unchanged entries.

## #28 FP-027 — Spec-test choices

**Decision:** The Independent Tester's spec tests (`crates/fineprint-core/tests/spec_choice_*.rs`)
read the brief as follows where it is silent, and assert only the common ground:
(1) Unsorted Objects/Players candidates are valid input: the mapping table says they "are sorted
ascending", requirement 5 says the OBJECTS default is ascending "even if candidates were given
unsorted", and property (iv) says their order "does not change the generated request". So
generation must sort them, not reject them; only duplicates are `InvalidSpec`. Tests about other
properties use ascending candidates so that one fault does not mask unrelated checks.
(2) Resolved `min`/`max` for kinds whose bounds the table does not use (COLOR, CREATURE_TYPE,
PILE, PLAYER, YES_NO, CARD_NAME, DIVISION) are passed as 1/1, ORDER as n/n; request `min`/`max`
are asserted only for OBJECTS and MODE (copied, max clamped to n), NUMBER (copied) and
PLAYER/YES_NO (1/1; YES_NO also with resolved 0/0 and 0/5, as it has no candidates to bound).
(3) Answer order is free where the table states no order: SubmitObjectSelection ids, SelectModes
indices and SubmitDivision assignments are accepted in any order; the order of the DIVISION
default's assignment list is not asserted (compared as a sorted list).
(4) A submit whose id is not the pending one, earlier or later, is `StaleRequest { submitted,
pending: Some(real) }`; after Done every submit is `NotPending`. `submit_default` with a wrong id
is asserted only as some error.
(5) Unpinned items are asserted as "some Err" or "is a Validator": wrong-typed or missing
`resolved.division`, duplicate or empty Strings candidates, replay failures, procedure failures,
`ctx.ask` with the wrong id; `ValidatorContract` contents are never inspected; `ChoiceError` is
matched with `matches!`, never compared with `==`.
(6) A "control character" in a card name is any `char::is_control` character (Unicode Cc,
including DEL and C1 controls such as U+009B).
(7) NUMBER ranges stay within `0..=i64::MAX` (the answer carries an `i64`); the u64::MAX overflow
cases use DIVISION totals and OBJECTS/MODE max values instead.
(8) Divergence is provoked by a `Cell` counter folded into the request's `reason.cr` (first ask)
or into a NUMBER max (second ask); only `Divergence { .. }` is asserted, not its index.
**Rationale:** R10/R12 black-box discipline: assert exactly what the brief states; where it is
silent, take the reading most consistent with the SRS 15.3 table and the brief's own examples.
**Alternatives:** Requiring callers to pre-sort candidates (contradicts requirement 5 and
property (iv)); asserting request bounds for every kind or a fixed assignment order
(over-specifies); ASCII-only control check (narrower than "control characters").

## #28 FP-027 — Suspension design: deterministic re-execution with a recorded answer list

**Decision:** Rules code that needs a player's decision stays plain synchronous code. A procedure
is `Fn(&mut S, &mut ChoiceContext<'_>) -> Result<O, Interrupt>` and asks with
`ctx.ask(request)?`. If that ask already has a recorded answer, `ask` returns it. Otherwise it
returns `Interrupt::Pending(request)` and the procedure propagates it. `ChoiceSession` keeps the
base state (one cheap Arc-sharing clone, the same checkpoint mechanism as #26), the ordered list
of accepted answers, and the digest of the request each answer was accepted for. Every accepted
answer re-runs the procedure from a fresh clone of the base with the whole list. Whatever the
procedure changed before it suspended is dropped with that clone, so while a request is pending
the visible state is exactly the base, and `state_hash()` is the base's hash. A request's id is
the 0-based position of its ask in the run (`ctx.next_request_id()`), so no global counter is
needed. When a re-run reaches an answered ask, it compares `PendingChoiceRequest::digest()` (SHA-256
of the canonical encoding) with the recorded digest. A mismatch, or a run that finishes before
reaching a recorded answer, is `ChoiceError::Divergence { index }`, never ignored. A failed
`submit` changes nothing, because the new list is committed only after its re-run succeeds.
**Rationale:** R3 makes a procedure a pure function of (state, answers), PRNG draws included: each
run starts from the base's PRNG, so re-execution is exact. No procedure has to be written by hand
as a state machine. The answer list is the unit for replay logs, forks and serialisation: a
pending decision is just the base plus its answers. No threads or async are needed (R3,
REQ-CORE-006). Cost: a run with `k` asks does `O(k^2)` procedure work over the session (each
answer re-runs the prefix). This is accepted because `k` is small for rules procedures.
Consequences: a procedure must be deterministic and must have no side effects outside `S` (a
`Cell` counter is caught as divergence). Every decision must go through `ctx.ask` with
`next_request_id()` (a different id is `ChoiceError::RequestIdMismatch`). A suspension must be
propagated: asking again after it, finishing after it, or returning a `Pending` that `ask` did not
return is `ChoiceError::Procedure`. Limits: the session is not wired into `GameState` or its
Merkle layout, and its wire schema belongs to the wire-protocol unit (#80), like the other
standalone #27 components. Timeouts are the caller's: `submit_default(id)` is exactly
`submit(id, pending.default_on_timeout.clone())`, and the engine never reads a clock (R3, R7).
**Alternatives:** (b) Explicit continuations: procedures written as state machines stored in the
state. Rejected: every procedure becomes a state machine, the continuation data must be hashed,
redacted and serialised, and the error risk is high. (c) Coroutines, async or threads. Rejected:
R3 and REQ-CORE-006 (scheduling must not affect results, and suspended stacks cannot be hashed or
serialised). (d) Undo-log rollback and redo with #26 `Transactions`. Rejected for resumption:
durable-record semantics exist for CR 734 rollback, not for replaying a procedure. `Transactions`
stays the mechanism for illegal or cancelled actions: `CANCEL_AND_ROLLBACK` handling arrives with
casting and will use it.

## #28 FP-027 — Generation: candidate handling, errors and request bounds

**Decision:** (1) Objects and Players candidates are sorted ascending by generation; only
duplicates are `InvalidSpec`. Strings keep their order and must be non-empty and distinct. This
matches the Independent Tester's reading ("#28 FP-027 — Spec-test choices"). My first
implementation rejected unsorted input, and the spec tests showed that the brief's "are sorted
ascending" means the engine sorts. (2) Missing candidates are `MissingCandidates`. So are
candidates given for a kind that needs none (NUMBER, CARD_NAME, YES_NO, DIVISION), and a
`division` given for a non-DIVISION kind (the brief's "missing/extra candidate kind"). Candidates
of the wrong kind are `InvalidSpec`. (3) `resolved.min > resolved.max` is `Unsatisfiable` for every
kind. (4) Request bounds: OBJECTS/MODE keep `min` and clamp `max` to n. NUMBER bounds the value
and clamps `max` to `i64::MAX`, because the answer carries an `i64`; a `min` above `i64::MAX` is
`Unsatisfiable`. ORDER and DIVISION use n/n and r/r (items and recipients). COLOR,
CREATURE_TYPE, PILE, PLAYER, YES_NO and CARD_NAME use 1/1. (5) `random: true` returns
`Generated::Random` right after `spec.validate()`, before candidate checks. (6) A YES_NO request's
payload description is empty: `reason` says why it is asked.
**Rationale:** The brief's mapping table and pinned rules. The request's `min`/`max` are the
values validation reads, so a raising procedure can tighten them.
**Alternatives:** Rejecting unsorted ids (contradicts the spec-test reading of requirement 5);
`InvalidSpec` for extra candidates (the brief groups them with missing ones); rejecting NUMBER
ranges above `i64::MAX` (the saturation tests need huge ranges to be accepted).

## #28 FP-027 — Option counting, enumerated items and ORDER restricted to top

**Decision:** The number of distinct valid answers is computed exactly in `u128`/`u64` and capped
at `ENUMERATION_LIMIT`, with an early exit. The decision needs only `N < 10_000`, so the contract
does not carry N. `C(n, k)` uses the exact multiplicative recurrence and stops as soon as it
reaches the limit, so huge inputs cost almost nothing and never overflow. Enumerated items are
whole answers where an answer is a single value (player, text, number, yes/no). For OBJECTS,
MODE, ORDER and DIVISION they are building blocks (candidates, mode indices, cards, recipients),
combined under the request's `min`/`max` and payload. ORDER from a spec offers only the `top`
destination: the answer is `top` = a permutation of all candidates, with `bottom` and `graveyard`
empty. Scry-like requests raised by procedures list more destinations in
`ChoicePayload::OrderCards`. `ValidatorContract { min, max, rule }` carries the domain in `rule`
(Subset, Modes, IntegerRange, OneOf, OneOfPlayers, Permutation, Division, CardName, or Payload
for the procedure-raised types), so a remote client can pre-check an answer.
**Rationale:** SRS 15.3 asks for pre-validated options below 10,000. Enumerating every subset or
permutation would be exponential even below the limit and would duplicate the payload. The pinned
mapping table lists exactly these items.
**Alternatives:** Listing every whole answer (8192 subsets for n = 13); carrying N in the contract
(it would have to saturate, so it adds nothing).

## #28 FP-027 — Card-name validation is structural

**Decision:** A `CHOOSE_CARD_NAME` request is always a validator contract
(`ValidatorRule::CardName { max_bytes: 256 }`). An answer is accepted when its name is non-empty
after trimming whitespace, is at most 256 UTF-8 bytes (the raw string) and contains no
`char::is_control` character. Each failure gets its own explanation. Whether the name exists in
the card database (either face of a DFC, CR 712.16; a prepare spell name, CR 723.5) is checked by
the raising procedure: an unknown name makes it return an explained error
(`Interrupt::Error(ChoiceError::Invalid { request_id, explanation })`), so the original request
stays pending for a valid retry, and a known name proceeds. It must not re-ask, because
`ChoiceSession::submit` would record the answer as accepted and replace the request. Without a
provided default, generation returns `NoDefault`.
**Rationale:** Pinned by the brief: the engine does not hold the card database (REQ-ARCH-001,
R5).
**Alternatives:** An engine-side name list (would put card data in the engine).

## #28 FP-027 — Payloads and responses of the procedure-raised types

**Decision:** `ChoicePayload` has one variant per choice type, and its canonical tag equals
`ChoiceType::tag()`. The fields the brief left to this unit are designed index-first, and every
index refers to a list in the payload. `ConfigureCosts { alternative: Option<u32>, additional:
Vec<u32>, x: Option<u64>, symbol_options: Vec<u32> (one per hybrid/Phyrexian slot), harmonize:
Option<ObjectId> }`; `SelectTargets { targets: Vec<Vec<ChoiceTarget>> }` (one list per slot);
`PayCosts { auto_pay: bool, methods: Vec<(u32, Vec<ObjectId>)>, mana_abilities: Vec<(ObjectId,
String)> }`; `SubmitAttackers { attacks: Vec<(ObjectId, ChoiceTarget)> }`; `SubmitBlockers {
blocks: Vec<(ObjectId, Vec<ObjectId>)> }`; `OrderTriggers { order: Vec<u32> }` (a permutation);
`SelectReplacement { index: u32 }`; `SubmitPregame { objects: Vec<ObjectId> }`. The
`PRIORITY_ACTION` payload lists each legal action as the exact response that takes it, and has a
`can_propose_shortcut` flag. The SRS response list has no ProposeShortcut response, so that
response arrives with the shortcut unit. Validation of these types checks the variant (the
explanation names both the response and the request type), membership, distinctness, index
ranges and payload counts. Deferred to the raising procedures: whether a payment covers the
cost, attack and block requirements and restrictions (menace, requirement maximisation),
trample lethal floors, and the legality of combined target sets. A payload that does not match
the request's type is rejected as malformed, never accepted.
**Rationale:** Indices into the request keep responses small and checkable without hidden
information (R12). Handlers arrive with the procedures that raise these types (issue text).
**Alternatives:** Free-form strings for costs and targets (cannot be validated); adding a
ProposeShortcut response now (not in the SRS 15.3 response column).

## #28 FP-027 — Canonical encoding tags and session error choices

**Decision:** Every enum encodes a one-byte `put_variant` tag in declaration order, then its
fields in declaration order. The tags are `ChoiceType` 0..19 (table row), `ChoiceResponse` 0..28
(the order of the SRS response list), `ChoiceTarget` Player 0 / Object 1, `ChoiceValue` Number 0
/ Text 1, `ChoiceOption` 0..6, `ChoiceOptions` Enumerated 0 / Validator 1, `ValidatorRule` 0..8,
`OrderDestination` 0..2, `AllowedValues` Range 0 / Texts 1, `ResolvedCandidates` 0..3, and
`Generated` Ask 0 / Random 1. Structs use `impl_canonical_struct!` in field order.
Session errors: `replay` with more answers than the procedure asks for is `NotPending`. A
`submit_default` with a wrong id is `StaleRequest` (it is `submit`). `ChoiceError` gains the
`RequestIdMismatch { expected, got }` variant (the brief allows a dedicated variant). `Generated`
and `Status` keep the pinned unboxed layout with a reasoned `allow(clippy::large_enum_variant)`.
**Rationale:** The #27 convention for canonical encodings. The session errors follow the pinned
API meaning ("NotPending: submit after Done").
**Alternatives:** Boxing the request in `Generated::Ask` and `Status::Pending` (changes the
pinned API).

## #28 FP-027 — CodeRabbit round 1: judgement calls and deferrals

**Decision:**
(i) CHOOSE_OPTIONAL_TRIGGER description from generation: not changed. A YES_NO generated from a
`ChoiceSpec` has no decision text (`ResolvedChoice` carries none); procedures raising
CHOOSE_OPTIONAL_TRIGGER build `ChoicePayload::ChooseOptionalTrigger { description }` themselves;
interpreter-supplied text for spec-generated YES_NO arrives with the DSL interpreter, issue #57
(FP-055).
(ii) PRIORITY_ACTION enumerated options: deferred to issue #49 (FP-047, legal-action generation;
consumer #44 FP-043). The payload `actions` (exact responses) and `can_pass` already carry and
validate every legal answer; mirroring them in `ChoiceOptions::Enumerated` needs an additive
`ChoiceOption` variant (next free canonical tag, 7) designed with the producer; it changes no
existing encoding.
(iii) Attack requirements and attack costs in the DECLARE_ATTACKERS payload: deferred to issue #67
(FP-065, attack constraint solver). Block combination constraints (menace, minimum blockers,
requirements) in DECLARE_BLOCKERS: deferred to issue #68 (FP-066). Attack/block costs are raised
as PAY_MANA_AND_COSTS (SRS 15.3). The field shapes depend on the solvers; additive payload changes
then. Today's `validate` checks eligibility, one target per attacker, and per-blocker capacity and
candidates only.
(iv) Payment-method eligibility: done now. `ChoicePayload::PayManaAndCosts.methods` is
`Vec<(String, Vec<ObjectId>)>` (display label, the objects eligible for that method), and
`validate` requires each `PAY_COSTS` method entry to use an index in range and distinct objects
from that method's list. Whether a payment covers the cost is deferred to issue #54 (FP-052, mana
pool, mana abilities and paying costs).
(v) Request well-formedness at `ChoiceContext::ask`, checked after the suspension and request-id
checks and before the replay/suspend branch (so replay and live runs behave identically, R3): an
invalid default gives `ChoiceError::InvalidDefault`; a validator contract whose `min`/`max` differ
from the request's gives `ChoiceError::Procedure`; the session never publishes either.
(vi) `validate` keeps `Result<(), String>` (pinned by the issue brief, public API); a typed
validation error is deferred to the wire-protocol unit, issue #80 (FP-078), if it needs one. With
(v) a malformed request never reaches `submit`, so the session boundary has one error class
(`ChoiceError::Invalid`).
(vii) Unknown card names: see "#28 FP-027 — Card-name validation is structural" (the raising
procedure returns an explained `ChoiceError::Invalid`; it never re-asks).
**Rationale:** Smallest changes that close the review findings without changing the pinned public
API beyond `PayManaAndCosts.methods`, without renumbering canonical tags, and without designing
payload fields ahead of the units that produce them.
**Alternatives:** A `ChoiceOption::Response` variant now (rejected: designed without the
producer); extending the attack/block payloads now (speculative); syncing validator bounds by
recomputing the contract in `validate` (rejected: hides the producer's bug); a single source for
the bounds (rejected: `min`/`max` and `ValidatorContract` are pinned public fields).

## #28 FP-027 — CodeRabbit round 2: judgement calls

**Decision:**
(i) PAY_COSTS: a method index may appear at most once per response; objects for one method go in
one entry. `validate` rejects a repeated index with a non-empty explanation. Cross-method object
overlap is not checked here (eligibility lists are per method; whether a payment covers the cost
stays deferred to issue #54, FP-052).
(ii) The `req_net_003_ask_rejects_stale_validator_bounds` success fixture is corrected to a range
(5..=14_999) at or above `ENUMERATION_LIMIT`, so it is a valid validator-contract request.
`well_formed` (session.rs) is unchanged: it checks only what makes a request unusable by the
session (default validates, validator bounds equal request bounds). Whether a request uses
Enumerated below `ENUMERATION_LIMIT` and Validator at or above it is a property of the producer:
`generate_request` guarantees it by counting; a procedure-built request is the procedure's
responsibility.
**Rationale:** (i) A repeated index would bypass the per-method distinctness check, so the answer
is ambiguous. (ii) The option count depends on type-specific candidate data (subsets, permutations,
divisions, card names) that the request does not carry uniformly for payload-built types;
re-deriving it in `well_formed` duplicates generate.rs and risks rejecting valid procedure-raised
requests (CHOOSE_CARD_NAME is always a validator by design).
**Alternatives:** (i) Merging repeated entries (rejected: silently reinterprets the answer);
tracking objects across all methods (rejected: not stated by SRS 15.3, may break legitimate
overlap). (ii) A count-based check in `well_formed` reusing generate.rs (rejected, reasons above).

## #29 FP-028 — Spec-test choices

**Decision:** The Independent Tester's spec tests (`crates/fineprint-core/tests/spec_replacement_*.rs`)
read the brief as follows where it is silent, and assert only the common ground:
(1) The bus is always driven through a `ChoiceSession` over a test state holding the world
components and the `ReplacementRegistry` (a `ChoiceContext` is only handed out by a session); a
`BusInterrupt::Error` becomes the procedure's output so that clause k (registry and world unchanged
on error) can be checked on the final state. World components are compared by canonical bytes
(`GameState` and `IdGenerator` by value), as in "#27 FP-026 — Spec-test choices".
(2) Whether an empty parent batch (every event dropped) appears in `BusReport::executed` is not
asserted: tests look only at batches that hold events, and at the logged batches (an empty batch is
never logged, clause l).
(3) An effect id already present in an incoming event's applied set is expected to stay in the
executed event's applied set (it was applied to that event); only `contains` is asserted.
(4) `reason.source` is asserted as `Some(object)` for object events and `None` for a Draw (no
affected object). The request's options, bounds and default are not asserted.
(5) The remaining amount of a `Shield::Pool` is checked only behaviourally (the next damage event)
and by the registry's canonical bytes changing; `ReplacementRegistry::get(..).shield` is not
asserted to reflect the remainder.
(6) The "903.9b repeated application" regression is tested two ways: a bounded ZoneChange loop (an
override effect "to hand -> command zone" applies again after a non-override effect "command zone ->
hand"; applications `[override, other, override]`), and an override that keeps matching forever
(`RepeatLimit`, nothing changed).
(7) 704.7 merging (review round 1, brief clause j): with `sba = true` only `ZoneChange` members
merge, and only when they affect the same object with the same destination (same object, same kind
of result); such a group makes one request (none with one candidate), one application, one shield
use and one executed event. Members for different objects never merge, whatever their kind,
candidate set or chooser; non-ZoneChange members never merge. Tests with several groups assert the
multiset of requests where the order is not fixed by APNAP/submission order. Cross-kind and
non-ZoneChange same-result merges (for example two different SBAs that both cause a player to lose
the game) are out of scope and untested. With `sba = false` a duplicate move of one object is only
asserted to fail and roll back (the error variant is not asserted). Review rounds 2 (F2) and 3
(H2), member-wise groups: with `sba = true` the group key is (object, destination) over ZoneChange
members; the candidates of a group are the effects eligible for at least one member (an effect
matching only one member's cause is offered); one `CHOOSE_REPLACEMENT_EFFECT` request when more
than one candidate is left after the tier filter, none otherwise; the chosen effect is applied once
(one `applications` entry, one shield use) to every member it matches, and members it does not
match are unchanged and still move the object; further effects are then considered again
(616.1f). The object moves at most once: members left with one destination are one move; when the
surviving destinations differ, the member whose destination differs from the group's original
destination wins (so a redirect matching only one member's cause still exiles the object; amended in
review round 4, (20) and (22), from "a member modified by an application wins"). That rule is the
brief's reading, not a CR statement. If every member is dropped the object does
not move (not tested directly; covered only by the property's at-most-once check). Every group test
runs both submission orders and compares output, registry bytes, world snapshot and state hash.
`shield_counter_destroy` matches `ZoneChangeCause::Destroy` only (CR 122.1c, 701.8b): with
[Destroy, StateBasedAction] it is applied once, the counter is removed, and the StateBasedAction
member moves the object to the graveyard once. In the two-cause-specific-effects case (Destroy-only
exile, StateBasedAction-only skip) the round-4 test asserts: one request offering both, no error,
`applications` equal to [chosen, other] (the other is then the only candidate and applies without
a request, 616.1f), exactly one move, to exile (the only surviving move is the redirected Destroy
member), both one-use effects removed from the registry, and order independence for the same
answer. The H2 property answers by effect-id preference (effect 1, then 2) so the
answer does not depend on the option order, and checks that its cases reach requests,
applications, moved and unmoved outcomes.
(8) The property generator never produces an unbounded override (overrides are only `Add(n >= 1)`
with `amount_below`) and uses only events and children no generated effect can loop on, so every
case must end with `Ok`; invariant (1) is checked as "a non-override effect appears in
`applications` at most once per submitted event". Generated damage comes from one of two fixed
battlefield source objects or from no source, so pool shields meet multi-source damage; the runner
answers a CR 615.7 `DIVIDE_VALUE` request as (17) states, and one test asserts that at least one
generated case raises such a request.
(9) F3 (stack chooser): the public zones API creates a stack object with an explicit controller
(`Zones::create_object` with `MoveRequest { to: Stack, controller: Some(p) }`), so the spell case is
tested directly on the stack (owned by player 1, controlled by player 2, and the reverse).
(10) F7 (work budget): the test world holds `MAX_BUS_EVENTS` plain `LifeChange` events in one batch,
so the boundary is tested directly (4096 succeed, 4097 is `WorkLimit`, nothing changed), plus a
children case (4095 members + one replaced Draw with one child is 4097: `WorkLimit`; one member
fewer succeeds). "Returns quickly" is not asserted on wall-clock time (R7); the doubling-override
case simply has to finish. Amended in review round 6 (thread r4633092145; item 34).
(11) F8 (pool allocation): when a source's allotment is 0, whether the pool effect still counts as
applied to that source's event is not asserted (only that it appears in `applications` and the
damage outcome); exact application lists are asserted only where unambiguous ((A:3,B:0): once;
(A:1,B:2): twice). Only one pool is used per test, so the order of simultaneous allocation requests
of several pools is not tested. The request's options are asserted as
`Enumerated([Object(A), Object(B)])`, `min = max = 2`; its default follows the capped fill of (17)
(for pool 3 against 3 + 3 that is (A:3, B:0)). The round-1 readings "the allocation request precedes
every other request of the batch" and "an allotment a source cannot use stays in the pool" are
withdrawn by review round 2 (G2, G4): see (15) and (17).
(12) F6 (enters as copy): the copy is part of the entry (CR 707.5): the call executes and logs ONE
non-empty batch for the entry, holding the entering event and no `BecomeCopy` event (the log gains
one batch, not two); a `Skip` effect on `BecomeCopy` is not applied; `copy_of(new id)` is the
original. Whether an empty batch also appears in `BusReport::executed` is not asserted ((2)). The
rollback when the copy step fails is provoked with a copied object id that does not exist; only
`BusError::Event(_)` and an unchanged state are asserted, not the inner `EventError`.
(13) F11/F4: an Untap of an untapped permanent and zero damage are asserted to leave the state
completely unchanged and log nothing; a Tap of a tapped permanent executing on its own is not
tested (brief: unchanged, no CR text). Amended in review round 6 (D6, clause B4, thread
r4632818842; CR 701.26a, 614.7), replacing the round-5 amendment: a Tap of a tapped permanent would
not happen, exactly like an Untap of an untapped permanent, so it offers no replacement or
prevention effect (no request, no application, no shield use) and changes nothing (nothing executed
or logged); `req_event_001_r6_d6_tap_of_tapped_permanent_offers_no_effect` asserts this, with an
untapped control. The terminating chain no longer starts from a tapped permanent (item 29).
(14) G1 (copy source, CR 608.2h, SRS 2.3): the entry copy succeeds when the original is destroyed in
the SAME batch as the entering event, in both submission orders, for a zone-change entry and a
token; the copy link is the original's (old) id and the original's executed move ends in a
graveyard. How the source's characteristics are resolved (current information or last known
information) is not asserted. The lenient source check is only for the entry copy: a standalone
`BecomeCopy` whose copied object left the battlefield in an earlier call fails with
`BusError::Event(_)` and changes nothing. A copied id that never existed still rolls back the whole
call ((12)).
(15) G2 (phased allocation, CR 615.7, 616.1f, 616.2): the allocation request is made when the pool
is about to apply, over the members' current damage. Tested: a self-replacement doubling (1 + 1 ->
2 + 2 > 3: one request, total 3; (A:1,B:2) leaves A dealing 1, (A:2,B:1) leaves B dealing 1); a
self-replacement reduction (2 + 2 -> 1 + 1 <= 3: no request, 2 prevented, the pool keeps 1 point,
checked against a later event with the modifier removed from the registry); the doubling and the
pool in the same tier: choosing the pool first gives [CHOOSE, CHOOSE] and no division, choosing the
doubling first gives [CHOOSE, CHOOSE, DIVIDE_VALUE]; the order of the two choice requests between
A's and B's events is not asserted beyond preceding the division. A request a member makes before
the pool applies (the active player's Draw choice) precedes the `DIVIDE_VALUE`
(`req_event_001_r2_g2_allocation_follows_earlier_choices`, which replaces
`req_event_001_r1_f8_allocation_request_comes_first`). The replay of the doubled allocation is
compared by output, registry bytes, canonical state and hash.
(16) G3 (shield-counter follow-ups, CR 122.1c, 615.13): the CR does not settle how many shield
counters simultaneous damage to one permanent removes; the tests pin the Orchestrator's reading
(R8) that ONE counter is removed for all simultaneous damage to that permanent: a shield-less
`Prevent { then }` effect queues its follow-up once per (effect, recipient) per batch, while every
damage event is still fully prevented and counted in `prevented`. Separate calls remove one counter
each; two permanents with their own shields each lose one; a generic follow-up (`LifeChange` of
player 1) on damage to player 2 from two sources executes once. `BusReport::applications` is
asserted to list the effect once per prevented damage event ([e1, e1]); this is a reading given by
the brief, not derived from the CR.
(17) G4 (allotment caps, CR 615.7): each allotment is capped at its source's current damage. An
over-cap answer (pool 4 with A 3, B 2: (A:1,B:3), (A:4,B:0), (A:0,B:4); pool 3: (A:0,B:3)) is
asserted as rejected in either manifestation: the session refuses the answer and stays pending with
its base unchanged, or the call ends with a bus error and the world and registry unchanged; the
error variant is not asserted. The timeout default fills recipients in ascending id order up to each
source's damage (pool 4, A 3, B 2: (A:3,B:1); pool 3: (A:3,B:0)), and answering with it is
asserted to complete. With valid answers a batch that asked drains the pool and removes the effect;
a later damage event is dealt in full. The property runner answers every allocation request with the
request's own `default_on_timeout` (it cannot see current damage after generated modifications, so
any other division could exceed a cap); it checks that the default divides the total among exactly
the request's recipients, each at least its minimum, and the "every case ends Ok" invariant is
kept. Sourceless damage (review round 3, H5) is outside the division: it is never a
`DIVIDE_VALUE` recipient (CR 120.1), and it takes only what the division leaves, after the members
that hold an allotment have taken theirs.
(18) H4 (overriding pool, CR 614.5, 615.7): a pool of 3 with `with_overrides_614_5(true)` against
3 + 3 from two sources is tested for all four valid divisions (A:1,B:2), (A:2,B:1), (A:3,B:0),
(A:0,B:3): `Ok` (no `RepeatLimit`), `prevented == 3`, each source deals its damage minus its share
(a 0 remainder means no executed event), the pool effect removed; the same four for a
non-overriding pool; and an overriding pool with a single source (one event of 5: 3 prevented, 2
dealt; two events of 3: 3 prevented, 3 dealt) with no request. `applications` is not asserted
for these cases.
(19) H5 (sourceless damage in a divided pool, CR 120.1, 615.7): pool 4 against sourceless 4, A 4
and B 4, in the submission orders [sourceless, A, B], [A, B, sourceless] and [B, sourceless, A],
answered (A:4,B:0), (A:2,B:2) and (A:0,B:4): exactly one request whose recipients are A and B
(total 4), `prevented == 4`, A and B deal 4 minus their share, the sourceless 4 is dealt in full,
the pool removed, and the registry bytes agree across the three orders. Sourceless damage with
only ONE source (no division) has no round-2 test to carry over and none is added (the brief adds
no new expectation for it).
(20) J1 (review round 4, CR 704.3, 704.7, 614.6, 614.7): an `Instead` child of the group's
application that moves the group's object competes with the group's surviving moves; a move whose
final destination differs from the group's original destination wins, never an error and never a
rollback, and the object moves exactly once (counted over every executed batch by its old id). The
child goes through the bus itself and its FINAL destination is used (exile redirected to hand puts
the card in its owner's hand, `applications` [e1, e2]). A child dropped by a Skip leaves the
surviving StateBasedAction member to move the object to the graveyard; `applications` is not
asserted there. "The card ends in zone Z" is asserted as: the new object of the single executed
move carries the same card and is in `Zones::contents(Z)`, and the old id is in no zone.
(21) J2 (CR 704.8, 122.1c): the departing object's LKI record is the state before any of the
group's events: `ctx.lki.get(old id).counters()` holds ("shield", n) for n = 1 and n = 2, while
the shield removes one counter. The RemoveCounters and the move share ONE executed batch, the
removal first, and exactly one RemoveCounters is executed. "No other executed batch" is asserted
over the non-empty executed batches and the non-empty logged batches (whether empty batches are
listed is not asserted). With a second object Y moved by an unrelated StateBasedAction, Y's LKI
exists and X's still holds the counter; because different objects are processed in submission
order, whole-run equality (output, registry bytes, world, hash) is asserted only between the two
orders of X's members with Y at the same position (Y first, last, and between X's members), not
across different placements of Y.
(22) J3 (CR 704.7, 616.1f): a modification that leaves the destination alone (`SetFaceDown(false)`
on a face-up object, `SetController(P1)`) never beats a redirect: the card ends in exile for both
answer preferences and both submission orders; only the common ground is asserted (no error, at
most one request, one move, both effects applied once and consumed; the order of the two
applications is not asserted). This is the brief's reading, not a CR statement: the CR is silent
on one object moving to two zones in one event. With two redirects to different zones (exile,
hand) only exactly one move, a zone in {exile, hand}, no error and order independence for the same
answer are asserted.
(23) J4 (CR 704.7, 707.5): of two identical entries hand to battlefield with `sba = true`, the
member carrying the entry-copy plan (C applied to it) wins over the one whose applied set already
held C: the card enters once, as a copy, with no `BecomeCopy` event executed or logged, one
application of C, C consumed, in both orders. This is the brief's reading, not a CR statement.
Controls: both applied sets empty (one application, a copy); both holding C (no application, C
kept, not a copy).
(24) K2/K3/K5/E2/E3 (review round 5, CR 704.3, 614.6, 616.2; tests in
`spec_replacement_round5.rs`): an `Instead` lineage is part of the ONE executed batch of the call
whatever it descends from (a CR 704.7 group member, a singleton, an `sba = false` member, a
grandchild of a nested lineage); "one batch" is asserted over the non-empty executed batches and
the non-empty logged batches, the logged batch equal in canonical bytes to the reported one. The
order inside the batch is asserted only where the brief states it: a non-move final event (a
`RemoveCounters` child) precedes the surviving zone changes. A fully replaced group or singleton
(its only result a counter removal) leaves the object on the battlefield and still shares the
batch with the other object's move. E2: a child removing a counter from an object that moves in
the same batch leaves that object's LKI at the pre-batch count (CR 704.8). K5 (reading Q3): the
grandchild's `RemoveCounters` of a counter the object does not have is still replaced (nothing
executes between generations); the variant whose grandchild moves to the group's own destination
asserts only Ok, one move and the graveyard.
(25) K4 (CR 615.5, 615.13, 122.1c, reading Q1: the CR is silent on simultaneous additional effects
of prevention): K4 is not a defect. The group case (the damage to A is the `Instead` child of a
group member) equals the plain batch [damage A, damage B]: the two follow-ups (1 damage to Q each)
are one later batch and Q's shield removes ONE counter (2 -> 1). The inverse (a lineage damage to
Q simultaneous with a submitted damage to Q) shares one removal too and reports the same
`prevented` (2) as the control [damage Q, damage Q] in a fresh world. The prevention effects on A
and B carry no shield. Follow-ups whose object was moved by the call's earlier batch (damage to X,
a move of X) are asserted to be ignored (Ok, the earlier batch kept, the follow-up not executed,
one non-empty batch); a follow-up naming a never-issued id fails with an unknown-object error at
any event layer (`EventError::UnknownObject` or the zone, permanent or memory variant) and the call
rolls back.
(26) E1/E4 (CR 704.3, 704.7, 400.7): one object with several submitted destinations and no
effects (two singletons; two groups; a group and a singleton) moves once, without error or
rollback, in both the submitted and the reverse order, with equal report, registry bytes, state
hash and zone. The test asserts only "one of {graveyard, exile}, the same for both orders".
Observation (not asserted): with the round-5 implementation all three shapes put the card in the
graveyard.
(27) B3 (reading Q2: the CR is silent on one object moving twice in one simultaneous event): a
move produced or redirected by a replacement (its final destination differs from its own submitted
member's, or it descends from a member of another object) beats a plain submitted move. K2 (c)
with `sba = false` (X's move replaced by a child exiling Y, Y's own move submitted) is therefore
NOT a caller duplicate and succeeds; the `sba = false` failure for two SUBMITTED moves of one object
(`req_event_001_r1_f2_no_coalescing_without_sba`) stays as (7) states. N1: of two entries hand to
battlefield, one with cause Other and C already applied, the other with cause StateBasedAction and
no applied effect, the Other move wins on canonical event content (the test checks first that
`bytes` of the Other event sorts before the StateBasedAction one) and the losing move cannot supply
its copy plan: no copy link on the new id, both orders equal. N2 strengthens
`req_event_001_r4_j1_instead_child_same_destination`: the only executed move of X across all
batches is the child `zone_change(X, Graveyard, Other)` (encoding precondition checked the same
way). N3: a child starting at the group's destination and redirected to exile is a redirect by its
FINAL destination and wins (exile, `applications` [e1, e2]).
(28) C1 (clause B1; CR 614.6 "a modified event occurs instead", CR 704.3; REQ-EVENT-001
"replacement-generated events re-enter the bus"): `req_event_001_executed_batches_parent_then_children`
is corrected from three non-empty batches (Tap; life(P1, +1); life(P2, -1)) to ONE batch
[life(P1, +1), life(P2, -1), Tap]: the damage's lineage (in the order the effect listed it, the
Draw child replaced in place by life(P2, -1)) precedes the later member's Tap; the logged batch
equals the reported one (bytes and index), the life totals and `applications` [1, 2] are kept.
(29) C2 (clause B4, reading Q3; CR 614.6, 616.1f): `req_event_001_override_effect_applies_again_to_instead_child`
keeps its scenario but now asserts `DepthExceeded` with the state unchanged: the lineage is judged
against the pre-call state, so the overriding effect keeps matching its own child Tap (the charge
counter is never removed between generations). Its old purpose (CR 614.5 override semantics) moves
to `req_event_001_override_effect_terminating_chain_applies_again`, re-specified in review round 6
(D6, clause B4, thread r4632818842; CR 701.26a, 614.7: the round-5 chain started with a Tap of a
tapped permanent, which now offers no effect): an UNTAPPED permanent, e1 (Tap ->
Instead[AddCounters charge 1], overriding) and e2 (AddCounters -> Instead[Tap]), every event
possible against the pre-call state: Ok, `applications` [e1, e2, e1], the permanent untapped with
one charge counter, ONE batch holding exactly one AddCounters with applied set {e1, e2}.
(30) B6 (CodeRabbit K1): separate results (different objects, separate groups) are scheduled in
APNAP order then submission order and may compete for a one-use effect, and an object's new id and
the batch order depend on the position of its move; so whole-run equality (report, registry bytes,
world snapshot, canonical state, state hash, and a re-run) is asserted only between the two orders
of ONE group's members, or of one object's two events, with every other event at the same position.
Other reorderings (K2 b) assert only final zones and counts. Amended in review round 6 (clause B1):
an `Instead` descendant takes the submission position of the event it replaces (item 31).
(31) D1 (review round 6, clause B1, thread r4632671472; CR 101.4, 616.1, SRS 8.1 [2]; tests in
`spec_replacement_round6.rs`): a descendant is resolved in APNAP order of its own chooser at the
submission position of the event it replaces, as soon as that event has resolved. The tests observe
this only through which simultaneous life gain takes a one-use doubler (a descendant gain of the
active player wins in both submission orders, also when its parent's chooser is the nonactive
player; two gains of one player go by the parent's position) and through the chooser of the first
two `CHOOSE_REPLACEMENT_EFFECT` requests (`PendingChoiceRequest::chooser`, as in
`spec_replacement_choice.rs`); the total number of requests is asserted only as at least two. No
existing spec test pinned the old "descendants after the cohort" order: none needed correcting.
(32) D2 (review round 6, clause B2, thread r4632671485; CR 101.3, 614.6, 400.7, 701.3b, 608.2h):
in a later batch of the call (a `Prevent { then }` follow-up, or the `Instead` descendants of one),
the roles checked against objects an earlier batch moved are the affected object, the
`Attach` host (`AttachTarget::Permanent`) and `BecomeCopy.copied` (an engine limitation: the rules
would copy last known information); such an event is ignored and the rest of its batch executes. A
damage source is not a role: damage from a moved source is still dealt (CR 608.2h). A never-issued
id still fails with `UnknownObject` and rolls the call back. Each shape runs directly and via an
`Instead` of the follow-up gain (with a gain of 2 in place of 1).
(33) D6 (review round 6, clause B4, thread r4632818842; CR 701.26a, 614.7): a Tap of a tapped
permanent offers no effect and is a no-op (items 13 and 29).
(34) Lineage positions (review round 6, thread r4633092145; CR 616.1, 101.4): a descendant's
position is its parent's position followed by its index in the parent's `Instead` list, recursively,
so lineages expand depth-first. `req_event_001_r6_lineage_grandchild_takes_parent_position`: Tap T
-> Instead[Tap U, P1 gains 3], Tap U -> Instead[P1 gains 1], a one-use doubler of player 1's gains:
the grandchild gain of 1 is doubled (P1 +5, applications [e1, e2, e9]); with the list [gains 3,
Tap U] the gain of 3 is (+7, [e1, e9, e2]); each also with a gain of player 2 before and after the
Tap. Corrected under R10: `req_event_001_r1_f7_doubling_override_is_work_limit` is renamed
`req_event_001_r1_f7_runaway_override_is_depth_or_work_limit` (the old name became wrong): the
narrow overriding chain Draw -> Instead[Draw, Draw] now asserts `DepthExceeded` (it reaches the
child-depth limit before the work budget); a wide shallow fan-out added to the same test (41
submitted gains of player 2, each replaced without override by 100 gains of player 1, 4100
descendants at depth 1) asserts `WorkLimit`. A shallow fan-out was chosen over the suggested
overriding 100-per-level fan-out because whether that one hits the depth or the work limit first
depends on how the budget counts unresolved descendants, which the brief leaves open. Both cases
assert the state hash, the canonical state and the registry bytes unchanged.
(35) Lineage position beyond resolution (CodeRabbit review 5980983595; CR 615.7, 616.1, 101.4):
an `Instead` descendant behaves exactly as if submitted at the position of the event it replaces,
recursively, also (a) when stopped damage resumes after a pool prevention and (b) in the order of
execution. Observed only by comparison with the equivalent direct batch:
`req_event_001_r6_pool_resumes_in_lineage_order` (one source, one chooser preferring the pool e1
over the one-use doubler e9 of damage below 3: Tap T -> Instead[Tap U, 3 damage], Tap U ->
Instead[1 damage] ends at 17 like the direct [1, 3]; with [3 damage, Tap U] at 15 like [3, 1];
life, damage prevented and the executed (source, amount) list equal the control) and
`req_event_001_r6_execution_follows_lineage_order` (Tap T -> Instead[Tap U, exile Y], Tap U ->
Instead[exile X]: executed and logged zone changes [X, Y], with the same new ids and zone bytes as
the direct [exile X, exile Y]; control [Y, X]). The number of requests and the applied sets are not
compared with the control (the lineage carries extra applied effects). No existing spec test
pinned creation-order execution or resumption: none needed correcting.
**Rationale:** R10/R12 black-box discipline: assert exactly what the brief states; where it is
silent, assert the reading every conforming implementation shares.
**Alternatives:** Asserting empty batches in `executed`, the pool remainder through `get`, or a fixed
order between merge groups of one chooser (each over-specifies what the brief leaves open).

## #29 FP-028 — Contained effects are filtered before the tier restriction

**Decision:** When the bus collects R(E'), contained effects (`ReplacementEffect::contained`) are
removed from the candidates while at least one non-contained effect applies to E'. The tier
restriction (first non-empty tier of CR 616.1a-e) is applied afterwards to what remains. A
contained effect becomes choosable once no non-contained effect applies.
**Rationale:** CR 616.1g (text supplied by the rules-researcher): "While following the steps in
616.1a-f, one replacement or prevention effect may apply to an event, and another may apply to an
event contained within the first event. In this case, the second effect can't be chosen until
after the first effect has been chosen." "Can't be chosen" makes a contained effect ineligible at
every tier step. Filtering by tier first would give an empty choice when the tier-winning set is
all contained.
**Alternatives:** Tier first, then the contained filter (rejected: empty choice sets, see above).

## #29 FP-028 — CR 704.7 merging of state-based-action batches

**Decision:** (Review round 1, F2/F5; amended in review round 2, F2, and replaced by member-wise
semantics in review round 3, H2; execution restructured in review round 4, J1-J4, and in review round 5, K1-K5.) With `sba = true` only `ZoneChange` members merge. Members form a
group when they have the same affected object and the same destination (`request.to`) as
submitted: the same object and the same kind of result. Every member stays in the list, tagged
with its group; the group is resolved at the position of its first member in submission order
(for APNAP and execution order). Collection: an effect is a candidate for the group if it is
eligible (the per-member filters: not already applied to that member unless it overrides
CR 614.5, shield left, matcher matching that member's CURRENT event) for at least one member;
candidates are deduplicated by `EffectId`, and the CR 616.1g contained filter and the tier
restriction run on that union. One choice is made for the group (asked of the object's chooser;
no request if the restricted set has one effect). The chosen effect is applied ONCE (one
`BusReport::applications` entry, one shield use, one set of `Instead` or `Prevent` child events,
taken from the first matched member in content order) to EVERY member it is eligible for and
matches; each such member's event is modified or dropped and the effect is added to its applied
set. Members it does not match are unchanged and keep resolving: the loop repeats over the group
with the union rule until no candidate remains (CR 616.1f). Execution (review round 4; detail in
"CR 704.7 group execution: one resolved batch"): the batch is first resolved completely, then
executed as ONE batch. The object moves at most once. Every `Instead` descendant, of a group
member or of any other member, is resolved through the bus as a member of the same batch (not
executed separately) and its final events are part of the same executed batch; a final `Instead`
move of an object is not executed on its own but competes with that object's other final moves.
The executed move (round 5, "Two destinations for one object"): if a candidate is a redirect
(its final destination differs from its root's submitted destination, or its root was not a move
of this object), only redirects compete, otherwise all do; the least by (canonical `GameEvent`
bytes, canonical `AppliedEffects` bytes, entry-copy plan) wins and is executed at the position
of the least root among the object's candidates; the others are dropped (not executed, not
logged). If every member was dropped (`Skip` or `Instead` on all), the replacement events of the
fully replaced group stay in the same executed batch (review round 5, K3); if only some were
dropped, the remaining members still move it. The result never depends on submission order within a
group: both orders give the same `BusReport`, registry and state hash. Members for different
objects never merge, whether or not they share kind, candidate set or chooser; each is processed
on its own (own request if it has two or more candidates, own application, own shield use) in
APNAP and submission order. Non-`ZoneChange` events never merge. With `sba = false` nothing
merges and equal duplicate moves are not coalesced (the second fails with `UnknownObject` and the
batch rolls back).
**Rationale:** CR 704.3 performs all applicable state-based actions simultaneously as a single
event; CR 704.5g and CR 704.5h both destroy, and CR 701.8b makes those state-based actions
destructions, so two state-based actions destroying one permanent are one result. CR 704.7: "If
multiple state-based actions would have the same result at the same time, a single replacement
effect will replace all of them." The CR text does not define the key of "the same result"; per
R8 same object and same destination is the narrowest reading that covers the lethal-damage-plus-
deathtouch case, and CR 616.1 makes each affected object's choice its own, so different
permanents are distinct results. Rules-researcher conclusions (review round 3): the CR gives no
rule that a replacement matching one state-based action cancels the others, so each replacement
applies to the members it matches (one replacement replaces all same-result actions it applies
to) and members it does not match still move the object; considering only one member's cause
hides a replacement that matches another member; same-cause duplicates merge and one replacement
covers both; CR 704.5f and CR 704.5g/h are mutually exclusive (toughness), so real mixed-cause
groups are for example a CR 704.5g destruction plus a move that is not a destruction. The
execution rule for differing destinations follows CR 704.7 (the replacement replaces the result
of the actions it applies to, so the object ends where the replacement sent it); unmatched
members still move the object, except when a surviving move to a different destination than the
group's original one (a redirect) takes precedence (CR 704.7; the object cannot move twice in
one batch). Determinism (R3, REQ-CORE-001): candidates are iterated by `EffectId`, matched members
in content order, and the executed move chosen by destination and content, so submission order
within a group has no effect.
**Alternatives:** Grouping by kind, candidate set and chooser (round 0; rejected in review:
different permanents are not the same result); deduplicating by event kind or destination alone
(rejected: merges different objects); merging only equal events (rejected: a cause difference
such as lethal damage versus deathtouch would keep two moves of one object); keeping the first
submitted member (round 1; rejected in round 2: submission order changed which cause-sensitive
replacement applied); survivor-only collection (round 2; rejected in round 3: it hides a
replacement matching a discarded member's cause); any matching replacement cancels the whole
merged move (rejected: the CR has no such rule; unmatched members still move the object); a
precedence among causes (still rejected: not settled by the supplied rules text); the union of
the members' applied sets (rejected: it would apply CR 614.5 to the history of an event that did
not happen); running the child events of a group application after the move (rejected: a child
such as the shield counter's removal would name an object that has already left, and the batch
would fail); running the group's child events as a separate batch before the move (round 3;
superseded in round 4: it retired the object id before the group's own move (J1) and captured
LKI after the shield counter's removal (J2, CR 704.8)); preferring members that were `modified`
(round 3; superseded: a modification that leaves the destination unchanged must not beat a
redirect, J3); breaking ties by index (superseded: the entry-copy plan is part of the execution
inputs, J4).

## #29 FP-028 — CR 704.7 group execution: one resolved batch

**Decision:** (Review round 4 J1-J4; replaced in round 5, K1-K5; scheduling revised in round 6,
D1.) The bus processes a batch in two
stages. RESOLVE runs the SRS 8.1 loop for every member and executes nothing. A member is a
submitted event or an `Action::Instead` descendant of one (a lineage member): an applied
`Instead` queues its events (charged against the work budget, each with the parent's applied set
after the application, CR 614.5), and right after the parent has resolved they are taken up as new
members of the SAME batch, with the parent's root (the submitted member, or the first member of its CR 704.7
group), the root's submitted (object, destination) and depth parent + 1 (above `MAX_CHILD_DEPTH`
is `DepthExceeded`). The loop: phase A repeatedly takes the not-started member with the least key
(APNAP rank of its chooser, chooser, lineage path, member index; a group at its first member, its other
members started with it; only submitted members are grouped), resolves it and at once takes up the
`Instead` events it queued, which then compete by the same key (see "Descendant scheduling and the
roles of the impossible-instruction check (round 6)"); when nothing is unstarted, phases B and C
allocate the lowest stopped CR 615.7 pool per chooser in APNAP order over all members and resume
each chooser's stopped members in the phase A key order (lineage path; members with an allotment
before sourceless ones, round 6 review 5980983595), then take up their `Instead` events; the loop ends when nothing is pending. All
members share one queue: one `prevented_once` set and one entry-copy list. The `Prevent { then }`
follow-ups of the whole batch, lineage members included, form ONE following child batch
(`depth + 1`, `sba` false), as before. RECONCILE: the final `ZoneChange`s of every member are
gathered per object and at most one is executed per object. One candidate is executed as is.
Exception: with `sba = false` and every candidate a submitted member, all are kept (a caller's
duplicate still fails with `UnknownObject` and rolls back). Otherwise a candidate is a redirect if
its root's submitted move is not a move of this object, or its FINAL `request.to` differs from its
root's submitted destination (the final destination counts, not the initial one: N3); if a
redirect exists only redirects compete; the winner is the least by (canonical event bytes,
canonical applied-set bytes, entry-copy plan: a candidate with a plan before one without, then the
lesser plan by `ObjectId` sequence), the candidate position breaking a remaining tie between
interchangeable inputs. The winner's plan is executed; the losers (and their plans) are not
executed, logged or reported; this never fails. EXECUTE: ONE `execute_batch_with_entry_copies`
call: first the final events that are not zone changes, ordered by (lineage path, member index),
then the kept zone changes, each at the least (lineage path, member index) of its object's
candidates (round 6 review 5980983595; before, (root, member index); identical for submitted
members, whose path is [root]). Non-moves
come first because `Tap`, `Attach`, `RemoveCounters` and the like need a live permanent (a shield
counter's removal precedes its object's move). This also changes the order of `sba = false`
batches: [move Y, tap Y] used to fail with `UnknownObject` and now succeeds. K1: the choice among
ONE object's competing moves depends on content, not on submission position; results of
DIFFERENT objects or separate groups are resolved in APNAP order, then submission order, and may
compete for a one-use effect, so reversing the submission can change which object a one-use
redirect applies to. Tests: `req_event_001_r4_j1_*`, `req_event_001_r4_j2_*`,
`req_event_001_r4_j3_*`, `req_event_001_r4_j4_*`, `req_event_001_r5_k2_*`, `req_event_001_r5_k3_*`,
`req_event_001_r5_e3_*`, `req_event_001_r5_k5_*`, `req_event_001_r5_e1_e4_*`,
`req_event_001_r5_e2_*`, `req_event_001_r5_n1_*`, `req_event_001_r5_n3_*`,
`req_event_001_children_reenter_bus` (the `Instead` child executes in the same batch as its
sibling), `req_event_001_r6_d1_*` (round 6 scheduling).
**Rationale:** CR 704.3 performs all applicable state-based actions simultaneously as a single
event, and CR 614.6 makes a replacement's modified event occur instead of the replaced one, so
the modified event, at any generation, takes the replaced event's place inside the same event
(rules-researcher, rounds 4 and 5). CR 704.8 derives a departing permanent's last known
information from the game state before any of those state-based actions; the events layer
captures LKI for every object a batch moves before any event of that batch, so one executed batch
gives that by construction. One object with several final moves is settled by "Two destinations
for one object" (researcher Q2). Rounds 3 and 4 kept a separate path for children of a group that
still moved and deferred every other `Instead` child to a later batch; every round found a new
shape where the two paths met (K2: a child move of an object with a plain move, K3: a fully
replaced group, K5: a grandchild move, K4-inverse: a second `prevented_once`), each a retired id
and a rollback or a split simultaneous event. One rule for every lineage removes the seam.
**Alternatives:** Patching K2, K3 and K5 separately (rejected: the same patch-on-patch structure
as rounds 3 and 4); a separate batch per lineage (rejected: splits one simultaneous event and
`prevented_once`); one batch per `Prevent` follow-up (rejected: changes the plain K4 shape from one
counter removal to two); executing children and reconciling afterwards (rejected: the object id is
already retired and LKI already captured); a change in `events/` (not needed).

## #29 FP-028 — Instead lineage joins the batch and is judged against the pre-batch state

**Decision:** (Review round 5, K2/K3/K5, researcher Q3.) Every `Action::Instead` lineage is part of
the same simultaneous batch as the event it replaces, at every generation, whether or not the
replaced member is in a CR 704.7 group (see "CR 704.7 group execution: one resolved batch"). Since
nothing executes while a batch resolves, a lineage member is collected against the state before
the batch: a replacement that needs a counter sees it even if another part of the same modified
event removes it. Consequence: an `overrides_614_5` effect with `requires_counter` whose `Instead`
output matches itself again (Tap -> Instead[remove the counter, Tap]) no longer stops after a
staged execution removed the counter; it applies at every generation until `DepthExceeded`, and
everything is rolled back (unit test
`req_event_001_r5_override_matching_own_instead_output_exceeds_depth`; the Independent Tester's
spec test of an overriding chain is rewritten as a terminating chain). An event of a LATER batch of
the same call (a `Prevent` follow-up or its `Instead` lineage) that names, as its affected object or as the host of an `Attach` or the copied object of a `BecomeCopy`, the old id
of an object an executed batch of this call moved to another zone is an impossible instruction
(the roles: "Descendant scheduling and the roles of the impossible-instruction check (round 6)"):
it is ignored (not offered to effects, not executed, no error, the earlier batches kept; test
`req_event_001_r5_follow_up_naming_moved_object_is_ignored`). An id that was never issued, or
that left in an earlier call, is still `UnknownObject` (tests
`req_event_001_r1_f6_enters_as_copy_is_not_replaced`,
`req_event_001_r2_g1_source_stays_and_standalone_stays_strict`).
**Rationale:** CR 614.6: "If an event is replaced, it never happens. A modified event occurs
instead, which may in turn be replaced"; for the ignored events, CR 101.3 ("Any part of an
instruction that's impossible to perform is ignored"; SRS REQ-ARCH-002: the rest is still carried
out) and CR 614.6: "the modified event may
contain instructions that can't be carried out, in which case the impossible instruction is simply
ignored", CR 614.7 and CR 400.7 (the moved object is a new object); CR 616.1f repeats the process "taking into account only
replacement or prevention effects that would now be applicable"; CR 616.2: an effect can become
applicable as the result of another that modifies the event; CR 120.4d: "Finally, the damage event
occurs." The state changes only when the modified event occurs. The CR does not say this
verbatim; the structure implies it (rules-researcher, round 5, Q3).
**Alternatives:** Staged execution per generation (rejected: it splits one simultaneous event,
retires object ids that a later generation names, and captures LKI after part of the event).

## #29 FP-028 — Two destinations for one object

**Decision:** (Review round 5, K2/K5/E1/E4, researcher Q2; an R8 judgement.) In one executed batch
an object moves EXACTLY ONCE. Its candidates are the final zone changes of every member that
names it (group members, ungrouped members, `Instead` lineage moves). A move produced or redirected
by a replacement (a redirect: its root was not submitted as a move of this object, or its final
destination differs from its root's submitted destination) beats a plain submitted move; among
redirects, or among plain moves if there is no redirect, the least by (canonical event bytes,
canonical applied-set bytes, entry-copy plan) wins. This never fails and never rolls back. With
`sba = false` and only submitted candidates nothing is reconciled (the caller's duplicate fails in
the events layer, as before; test `req_event_001_r1_f2_same_object_sba_moves_execute_once`). For
example [X to the graveyard (sba), X to exile (sba)] moves X to the graveyard: the destination is
encoded before the cause and the graveyard's tag (3) is below exile's (5) (test
`req_event_001_r5_e1_e4_two_destinations_one_object_moves_once`).
**Rationale:** The CR is silent on one object moving twice in one simultaneous event. No pair of
CR 704.5 state-based actions sends one object to different zones; different zones arise only
through a replacement, so the case reduces to a replacement-produced move, which is the modified
event that occurs instead (CR 614.6) and so takes precedence over the plain move; the plain move
then finds the object gone and does nothing, by analogy with CR 614.7 and CR 400.7, not as a
rule. CR 704.3 and CR 704.7 make the state-based actions one event, so the result must not depend
on submission order (researcher Q2).
**Alternatives:** Failing or rolling back (rejected: the engine must never fail on it); the first
submitted move wins (rejected: submission-order dependence); the last wins (rejected: the same).

## #29 FP-028 — Known limitations of the one-batch resolution (round 5)

**Decision:** Recorded, not fixed in this unit. (b) A CR 615.7 allocation of a pool happens on the
damage stopped at that moment; damage that a lineage member produces later (an `Instead` applied
in phase C) is allocated afresh: if the pool is left after the first allocation, the later damage
triggers a second allocation, which may be a division (test
`req_event_001_r5_limit_b_lineage_damage_after_division`: pool e1 is allocated without a division
on 2 + 2 and keeps 1; lineage damage from two sources asks a second division of the remaining 1).
A full division uses the whole pool, so no division follows a division of the same pool. (d) The
events that are not zone changes execute before the zone changes, which changes the order for
`sba = false` batches ([move Y, tap Y] now succeeds). (e) Results of DIFFERENT objects or groups
still compete for a one-use effect in APNAP order of their choosers, then submission order (an
`Instead` descendant at the position of the event it replaces, recursively), so reversing the
submission can change which object such an effect applies to (K1; documented). Since round 6 (D1)
descendants are no longer deferred behind the members that were not yet started with their parent:
each is scheduled as soon as its parent has resolved.
(f) "Instead, do A, then B" where B names a new object made by A cannot be expressed as
`Instead(Vec)` (the lineage is judged against the pre-batch state); a later unit. (g) Closed in round 6
(D2) except one limitation: the impossible-instruction check now covers the affected object, the
host of an `Attach` and the copied object of a `BecomeCopy`; a damage source is deliberately not
covered (CR 608.2h). The remaining limitation: a `BecomeCopy` whose copied object this call
already moved is ignored instead of copying its last known information (CR 608.2h); that needs an
events-layer change in a later unit.
(h) Recorded after review 5981232036 (bus.rs:466, outside the diff). Phase C resumes all of one
chooser's stopped pool members before any `Instead` descendant created during that resumption is
taken up. Such a descendant is scheduled by phase A only after the pool round, not at the position
of the event it replaces. A one-use effect can therefore go to a later stopped member first.
Example: two simultaneous 3-damage events to P1 from one source; pool e1 of 1 applies to both;
e2 replaces damage below 3 with Instead[1 damage to P1] on the first; e9 doubles damage once.
P1 ends at 13, where the lineage-position rule would give 15. Both outcomes are legal orders for
simultaneous events; the engine is deterministic either way; only the internal lineage-position
rule is not met inside phase C. A fix must run each resumed member's descendants, with their
choices and any pool they stop at, before the next stopped member resumes; a later unit.
**Rationale:** (b) is rare (it needs an `Instead` on partly prevented damage); (e) follows from
CR 616.1 (each object's choice is its own, choosers in APNAP order); (f) needs sequenced
sub-events, outside this unit; (h) needs nested resolution inside phase C, needs an `Instead` on
damage partly prevented by a pool plus a one-use effect matching a sibling, and is an ordering
choice among simultaneous events rather than a rules violation.
**Alternatives:** Re-running the allocation for all damage whenever lineage damage appears
(rejected: it would undo prevention already applied); content order across objects (rejected:
CR 616.1 orders by chooser, and the one-use effect's choice belongs to the first chooser).

## #29 FP-028 — Descendant scheduling and the roles of the impossible-instruction check (round 6)

**Decision:** (Review round 6, D1, D2 and thread r4633092145.) Scheduling: every not-yet-started
member of a batch, submitted or `Instead` descendant, is ordered by ONE key: (APNAP rank of its
chooser, chooser id, lineage path compared lexicographically, member index). The rank is the
chooser's position in the APNAP order; choosers outside it come last by ascending id. The lineage
path of a submitted member is [its index] ([the group's first index] for a CR 704.7 group member);
a descendant's path is its parent's path followed by its position in the parent's `Instead`
output. So a descendant takes the exact position of the event it replaces, recursively: a
grandchild sorts at its parent's place among the parent's siblings, not after them (test
`req_event_001_r6_lineage_path_grandchild_takes_parent_position`: T -> Instead[tap U, gain 3],
U -> Instead[gain 1], a one-use doubler: the gain of 1 takes it, 2 + 3 = 25). The member index
only separates a group's members, which share a path. The first round-6 version ordered by
(root, member index), so every descendant of one submitted event shared one position and creation
order decided (27 in that shape); rejected in review (r4633092145). Consequence: lineages are
expanded depth-first, so an overriding `Instead` chain with a small fan-out (a draw doubled at
every level) reaches `MAX_CHILD_DEPTH` (about 130 events) before `MAX_BUS_EVENTS` and is
`DepthExceeded`, not `WorkLimit`; a wide fan-out (100 per level) still hits `WorkLimit`; both roll
everything back (unit test `req_event_001_r1_f7_work_budget`, corrected under R10). Phase A takes the not-started member with the least key (a group at its first member, the
group's other members started with it), resolves it, and at once takes up the `Instead` events it
queued as not-started members. A descendant can start only after its parent resolved; if its
chooser ranks before members that already resolved, it simply runs next. Requests for a
descendant's choices come in this order. For submitted members only, the key is the previous
(rank, chooser, index) order. The same path order is used everywhere members are ordered by
position (CodeRabbit review 5980983595): phase C resumes a chooser's stopped pool members in this
key order (allotted members before sourceless ones, a stable sort), not by creation index, so a
descendant that phase A stopped first also resumes first (test
`req_event_001_r6_pool_resumption_follows_lineage_path`: T -> Instead[tap U, 3 damage],
U -> Instead[1 damage], pool 1, a one-use doubler of damage below 3: the 1 is prevented and 3 is
dealt, as for the direct [1 damage, 3 damage]; before, the 3 resumed first, was cut to 2, doubled
to 4, and 5 was dealt); the executed batch orders the events that are not zone changes, and the
kept zone changes, by (lineage path, member index) instead of (root, member index) (test
`req_event_001_r6_execution_order_follows_lineage_path`: a grandchild move executes, and gets its
new id, at its parent's place, as in the direct batch). Corrected under R10 by that execution
order: the unit test `req_event_001_r6_lineage_path_grandchild_takes_parent_position` now expects
the executed life deltas [(P1, +2), (P1, +3)] instead of [(P1, +3), (P1, +2)], because the direct
batch [gain 1, gain 3] executes the doubled gain of 1 first (its life totals 25, 21 and 27 are
unchanged). Phase B is order-independent: the pool's
choosers are taken in APNAP order (rank, then id, independent of member indices), and the
allocation sums each source's damage in a `BTreeMap` keyed by source. Members taken up after the
resumption are scheduled by phase A. `DepthExceeded` is raised as soon as the too-deep generation
is taken up (same error, same rollback). Impossible-instruction roles: an event of a later batch
of the same call is ignored if it names a retired id (an object an executed batch of this call
moved) as (i) its affected object, (ii) the host of an `Attach` to a permanent, or (iii) the copied
object of a `BecomeCopy`. The source of a `Damage` does NOT count: damage from a source that has
left its zone is still dealt. (iii) is an engine limitation: the events layer's standalone
`BecomeCopy` accepts only a copied object in a zone and would roll back the whole call, so the bus
ignores such an event instead of copying last known information. Ids never issued, or retired in
an earlier call, still fail with `UnknownObject`. Tests: `req_event_001_r6_d1_*`,
`req_event_001_r6_d2_*` (in one batch, an `Attach` onto X executes before X's move and Z ends
unattached: the permanent container ends attachments to an object that leaves the battlefield).
**Rationale:** CR 101.4 and CR 616.1 order simultaneous choices APNAP; SRS 8.1 step [2]
"Simultaneous choosers -> APNAP". Lineage members are judged against the pre-batch state, so their
choices are simultaneous with the submitted members' choices; the outcome must not depend on
whether a gain was submitted directly or produced by `Instead` (review D1: a one-use effect went to
P2's gain because P1's descendant waited for the whole cohort). CR 101.3: "Any part of an
instruction that's impossible to perform is ignored", and SRS REQ-ARCH-002: the rest is still
carried out, so an impossible `Attach` is dropped and the other events of its batch execute
(before: `UnknownObject` rolled back the whole call). CR 614.6: "the impossible instruction is
simply ignored"; CR 400.7: the moved object is a new object; CR 701.3b: an attach to an object it
can't be attached to does not move the attachment. CR 608.2h: "it's the object as it exists—or as
it most recently existed—that does it", so the damage source keeps dealing damage, and a copy would
use last known information.
**Alternatives:** Scheduling descendants after the whole cohort of members not yet started with
their parent (round 5; rejected, D1); ordering descendants by (root, creation index) (round 6 first
version; rejected, r4633092145: a grandchild lost its parent's place among the parent's siblings);
dropping a damage event whose source moved
(rejected: CR 608.2h); letting a `BecomeCopy` of a moved object fail (rejected: rolls back the
whole call); changing `events/` to copy from last known information (outside this unit's paths; a
later unit).

## #29 FP-028 — Known limitation: CR 704.7 merging only within one event kind

**Decision:** CR 704.7 merging here covers only `ZoneChange` events of the same object to the same
destination. Merges across kinds (for example two different state-based actions that both cause a
game loss) and merges of non-`ZoneChange` events are out of scope; there is no game-loss
`BusEventKind` in this unit. Pool prevention across simultaneous damage is not a CR 704.7 merge;
see "Pool prevention is allocated by the affected player".
**Rationale:** The result comparison for other kinds needs event kinds this unit does not have;
the zone-change key is the one review round 1 required.
**Alternatives:** A result-equivalence relation across kinds (deferred to the unit that adds
game-loss events).
## #29 FP-028 — A tier set with one effect is applied without asking

**Decision:** When the choosable set (after the contained filter and the tier restriction) has
exactly one effect, it is applied with no `CHOOSE_REPLACEMENT_EFFECT` request; with two or more
the chooser is asked.
**Rationale:** Brief requirement 2; a one-option choice carries no decision and would only add a
round trip and a request id.
**Alternatives:** Always ask (rejected: noise for clients, changes request numbering).

## #29 FP-028 — Chooser, APNAP order and the turn-order fallback

**Decision:** The chooser of an event: for `CreateToken`, its controller if set, else its owner;
for an affected object, the controller in the permanent container if it is a permanent; else its
zone record's rules controller (`ObjectRecord::rules_controller`, set on the stack, so a spell
owned by P1 and controlled by P2 is P2's choice); only an object with no controller falls back to
its owner (an unknown object is `BusError::Event(UnknownObject)`); for an affected player, that
player; otherwise the first player of the APNAP order (the active player). APNAP order is
`GameState::turn_order()` rotated to start at the active player; if the turn order is empty, the
players in ascending id order; with no active player, unrotated. Events are processed in that
order of their choosers (as submitted), submission order within one chooser; choosers outside the
order come last by ascending id. An `Instead` descendant is scheduled with the same order as the
submitted events, at the position of the event it replaces (recursively, by lineage path), as soon
as that event has resolved (round 6, D1 and r4633092145; CR 101.4, CR 616.1). The chooser asked at a request is recomputed from the event as
modified so far (a `SetController` on a token changes who chooses next).
**Rationale:** SRS 8.1 step [2]; CR 616.1 "the affected object's controller (or its owner if it
has no controller)"; CR 112.2 every spell has a controller; CR 109.4 only objects on the stack or
the battlefield have one (review round 1, F3). `GameState` has a turn-order API, so the
ascending-id fallback is needed only when the order is empty.
**Alternatives:** Owner for every non-permanent (round 0; rejected in review: asks the wrong
player for a spell controlled by another player); fixing the chooser at submission time
(rejected: the affected object's controller can change through an earlier application).
## #29 FP-028 — Bus limits MAX_CHILD_DEPTH and MAX_REPEATS

**Decision:** `MAX_CHILD_DEPTH = 64`: the submitted batch is depth 0; an `Instead` lineage member
is one deeper than the event it replaces, within the same batch (review round 5: the depth counts
`Instead` lineage generations, and a lineage deeper than 64 is `BusError::DepthExceeded`, rolled
back); a `Prevent` follow-up batch is one deeper than the batch that queued it, as before, and its
lineages count on from there. `MAX_REPEATS = 256`: an event may receive at most 256
applications; the next is `BusError::RepeatLimit`. Review round 1 (F7) adds
`MAX_BUS_EVENTS = 4096` and `BusError::WorkLimit`: within one `process_batch`/`process_event`
call every event instance the bus takes up (the submitted members and every child at every depth)
is charged against one budget, before the children are queued; exactly 4096 is allowed, one more
is `WorkLimit`. The budget is a plain counter in the call, not time. All three errors roll
everything back. The copy of an enters-as-copy entry is executed directly and not charged.
**Rationale:** Self-feeding `Instead` effects and unbounded CR 614.5 overrides (CR 903.9b-style)
would otherwise loop forever. The depth and repeat bounds alone are not enough: an overriding
`Instead` that replaces one event with two doubles the work at each level, so it exhausts memory
(about 740 MB at depth 22) long before depth 64. The budget keeps the bus total in time and space;
the bounds are far above any real chain.
**Alternatives:** No bound (non-terminating); depth and repeat bounds only (round 0; rejected in
review: exponential growth); a time budget (rejected: R3/R7, not deterministic).
## #29 FP-028 — API details and additions beyond the fixed API

**Decision:** The fixed API is implemented unchanged. Additions: `AmountOp::apply` (public),
`impl From<BusError> for BusInterrupt`, `impl From<EventError> for BusInterrupt`,
`impl From<EventError> for BusError`, `Display` for `BusInterrupt`, and `CanonicalEncode` for
`BusEventKind`, `Tier`, `EventMatcher`, `AmountOp`, `Modification`, `Action`, `Shield` and
`ReplacementEffect` (the registry encoding is built from them; zone kinds use their one-byte tag).
The request's enumerated options are `ChoiceOption::Number(i)` for each index `i` of the payload's
effect list, `min = max = 1`, `reveal_to` empty. `BusReport::executed` lists only batches in which
an event occurred (as the event log does). `EntersAsCopyOf` takes effect only when the executed
event's outcome is a token creation or a zone change into the battlefield from another zone;
review round 2 (G1/F6) sets the copy link as part of that entering event, with no replacement
collection (see "Enters-as-copy is applied as part of the entry event"). `SetDestination` keeps the rest of the request,
except that review round 1 (F9) clears the controller when the new zone is neither the
battlefield nor the stack (only those zones take a controller, CR 109.4; `Zones` rejects the
combination with `ControllerNotApplicable`). Review round 1 (F7) adds `pub const
MAX_BUS_EVENTS: usize = 4096` and `BusError::WorkLimit` to the public API. On
`BusInterrupt::Pending` the registry and the context are also restored, so a caller outside a
`ChoiceSession` sees no partial change either.
**Rationale:** Index options are unambiguous even with equal descriptions; the conversions let
procedures use `?`; the encodings make the registry hashable.
**Alternatives:** Text options with the descriptions (ambiguous when two descriptions are equal).

## #29 FP-028 — Instead children inherit the applied set

**Decision:** (Review round 1, F1.) An event produced by `Action::Instead` is the modified event
that replaces its parent: it carries the parent's applied set as it stands after that
application, the applying effect included. Events that are independent new events start with an
empty applied set: `Prevent { then }` follow-ups and every member of the submitted batch (history
is per batch member, never shared). An enters-as-copy is not an event: its copy link is set as
part of the entering event. `overrides_614_5` effects still apply again.
**Rationale:** CR 614.5: a replacement effect "gets only one opportunity to affect an event or any
modified events that may replace that event"; CR 614.6: a replaced event never happens and the
modified event occurs instead. With fresh sets two effects that replace each other's output
alternate until `DepthExceeded` and roll the event back. CR 615.5: a prevention's additional
effect takes place immediately afterward; it is a separate event, so it starts fresh. This is the
conservative reading the rules-researcher gave.
**Alternatives:** Fresh sets for every child (round 0; rejected in review: ping-pong loops);
inheriting for `then` children too (rejected: they are new events, not modified events).

## #29 FP-028 — Events that would not happen offer no effects

**Decision:** (Review round 1, F4/F11; round 6, D6.) Before collection the bus checks whether the
event would happen at all: a `Damage` with amount <= 0, an `Untap` of an object that is not a
tapped permanent, and a `Tap` of an object that is not an untapped permanent (a permanent that is
already tapped, or an object that is not a held permanent) would not. For such an event the
choosable set is empty: no request, no application, no shield consumption; the event goes
unchanged to the executor, which treats it as a no-op. An `Instead` lineage is judged against the
pre-batch state, so a lineage `Tap` of a permanent that is untapped before the batch stays
possible. Test: `req_event_001_r6_d6_tap_of_tapped_permanent_offers_no_effect`.
**Rationale:** CR 614.7: an effect that would replace an event that never happens does nothing;
CR 614.7a and CR 120.8: a source that would deal 0 damage deals no damage; CR 701.26b: only
tapped permanents can be untapped, so CR 122.1d's stun counter replaces only a real untap;
CR 701.26a: "Only untapped permanents can be tapped." Round 0 let 0 damage consume a one-use
shield and an untapped permanent lose its stun counter.
**Alternatives:** Dropping the event in the bus (rejected: the executor already treats it as a
no-op and logs nothing); keeping `Tap` of a tapped permanent replaceable (rejected: CR 701.26a,
CR 614.7; review round 6 D6).

## #29 FP-028 — Pool prevention is allocated by the affected player

**Decision:** (Review round 1, F8; amended in review round 2, G2 and G4, and in review round 3, H4 and
H5.) The allocation is made on
the damage as it stands when the shield applies. Each member's resolve loop is resumable: a member
stops immediately before it would apply a `Shield::Pool` effect P not yet allocated for it (after
any `CHOOSE_REPLACEMENT_EFFECT` that selected P, or when P is the single candidate of its tier),
and nowhere else. Phase A: every member, in APNAP order, runs until it stops or finishes. Phase B:
for the lowest `EffectId` pool among stopped members, the stopped members of that pool are
grouped by chooser (APNAP order of choosers); for each group in turn the bus builds the table
from the members' CURRENT events: eligible members are the group's `Damage` events with
`source = Some(_)`, current amount > 0, matched by P now. If they come from at least two
distinct sources and their total exceeds what is left of P's pool, the chooser is asked once
with the existing `DIVIDE_VALUE` choice (no change to `choice/`): total = the remaining pool,
recipients = the sources in ascending `ObjectId` order with minimum 0, `min = max` = the number
of recipients, options `ChoiceOption::Object` per source, reason `CR 615.7`. Otherwise nothing is
asked and P applies in order. Phase C: P is applied to that group's members in APNAP order, each
running on until its next stop or finish; then the next group, and Phase B repeats while any
member is stopped (a member reaching another pool joins that pool's group; a new table for the
same pool and chooser replaces the old one). Caps (G4): `cap(s)` is the sum of the current
amounts of source s's eligible members; the allotment total is min(remaining pool, total
eligible damage), and a request is made only when the total exceeds the pool, so the shares sum
to the pool. The bus rejects an answer whose share for a source exceeds its cap with
`BusError::Choice(ChoiceError::Procedure(..))` naming the source and its cap (the call fails and
everything rolls back), after the choice layer's own validation; the timeout default fills the
sources in ascending `ObjectId` order, each up to its cap, until the pool is used (pool 4 with
A:3, B:2 gives (A:3, B:1)). When P applies to a member in the table it prevents the minimum of
the source's unused allotment, the member's current damage and the pool left; events of one
source use its allotment in APNAP and submission order. A member whose source's allotment is used
up or 0 is not given P (no application) and continues; this includes a member that chose P over
another effect and then finds its allotment 0. A pool that is allocated for a member and whose
allotment for that member's source is used up is not offered again, so an overriding (CR 614.5
exception) pool cannot loop on a spent allotment (review round 3, H4). Sourceless damage is not
in the table (a sourceless share cannot be offered in `DIVIDE_VALUE`; every damage has a source
object, CR 120.1) and draws on what the division leaves, after the allotted members of that
chooser: in Phase C the group is stably sorted so members with an allotment take P first
(review round 3, H5). With valid answers every point of the pool is used by the batch whenever the
eligible damage allows it, so a batch that asked for an allocation leaves the pool drained and P
removed from the registry. Request order: the `DIVIDE_VALUE` request comes after every request
members make before reaching the pool (for example the active player's unrelated
`CHOOSE_REPLACEMENT_EFFECT` in the same batch) and before any request made after the pool is
applied. `Shield::Uses` shields are unchanged. Known limitations: a pool can keep points when the
total eligible damage was below it, or when a modification applied after the allocation (in
Phase C) changed the damage; the grouping is per chooser, not per shielded object or player; a
member that chose P and finds its allotment 0 continues without it; a member stopped at a
different pool is outside that pool's division, but every allocation of the damage remains
reachable because the player chooses the order of the pools through CHOOSE_REPLACEMENT_EFFECT and
divides each pool among the members that chose it (review round 3, H1; not a defect).
**Rationale:** CR 615.7: "If damage would be dealt to the shielded permanent or player by two or
more applicable sources at the same time, the player or the controller of the permanent chooses
which damage the shield prevents. Once the shield has been reduced to 0, any remaining damage is
dealt normally. Such effects count only the amount of damage; the number of events or sources
dealing it doesn't matter." CR 616.1f: "Once the chosen effect has been applied, this process is
repeated (taking into account only replacement or prevention effects that would now be
applicable) until there are no more left to apply." CR 616.2: "A replacement or prevention
effect can become applicable to an event as the result of another replacement or prevention
effect that modifies the event." So the division is decided on the damage after earlier
modifications, and whether a same-tier modification or the shield goes first is the chooser's
CR 616.1e choice. Damage is dealt normally only once the shield is 0, so a share above its
source's damage must not leave the shield with points while damage is dealt: capping is the
correction chosen. The DIVIDE_VALUE payload carries only per-recipient minimums, so the maximum
is enforced by the bus. In round 0 the submission order made the choice; in round 1 the table
was fixed before any modification.
**Alternatives:** Asking per event (rejected: the shield counts amounts, not events); a new choice
type (rejected: `DIVIDE_VALUE` already validates a division); one allocation before any member is
resolved (round 1; rejected in round 2: modifications changed the damage after the table was
fixed); a lazy check at the pool's first application (rejected: an earlier member draws on the
pool before a later member's modifications exist); redistributing an unused allotment to the
remaining damage (rejected for the cap, which keeps the answer exactly what the player chose);
adding a per-recipient maximum to `choice/` (outside this unit).

## #29 FP-028 — SBA destruction and the shield counter's destroy replacement

**Decision:** (Review round 3, H2.) `ZoneChangeCause::Destroy` is destruction as the result of an
effect; `ReplacementEffect::shield_counter_destroy` matches `Destroy` only and is unchanged. A
caller that models lethal damage or deathtouch (CR 704.5g, CR 704.5h) submits
`ZoneChangeCause::StateBasedAction`, to which the shield counter does not apply and which
regeneration (not in this unit) would replace. A mixed `Destroy` + `StateBasedAction` group is
the only way one object's CR 704.7 group has a shield-matching member and a non-matching one;
member-wise semantics apply the shield to the `Destroy` member only: the counter is spent (its
removal is part of the same executed batch, placed before the move, and the departing object's
last known information is captured before the batch with the counter still on it (CR 704.8;
test `req_event_001_r4_j2_lki_keeps_pre_sba_counters`)), the `Destroy` member does not move, and the
`StateBasedAction` member still moves the object to the graveyard (test
`req_event_001_r3_h2_d_shield_counter_on_mixed_group`).
**Rationale:** CR 701.8b: state-based actions that check for lethal damage or deathtouch damage
destroy; CR 122.1c: the shield counter replaces destruction "as the result of an effect"; a
state-based action is not the result of an effect (CR 704.1; rules-researcher, review round 3).
So an SBA destruction is a destruction, but not one the shield counter replaces.
**Alternatives:** Letting the shield counter match `StateBasedAction` (rejected: contradicts
CR 122.1c); a separate "SBA destroy" cause (not needed: `StateBasedAction` already carries it,
and no regeneration effect exists in this unit).

## #29 FP-028 — SRS erratum: state-based-action table rows 704.5g and 704.5h

**Decision:** `docs/SRS.md` is not edited (only a person edits it, PROCESS.md P8). The erratum is recorded here and a P8 line in `docs/HUMAN_QUEUE.md` proposes the edit. The CR 2026-11-06 text wins over the SRS. Two rows of the SRS state-based-action table disagree with it: (1) the 704.5g row says "It is destroyed. Regeneration and shield counters can replace this."; CR 704.5g says only "Regeneration can replace this event.", and the shield counter's destroy replacement (CR 122.1c) applies only to destruction "as the result of an effect", which a state-based action is not (CR 701.8b); (2) the 704.5h row says "It is destroyed." and omits that "Regeneration can replace this event." (CR 704.5h). The bus follows the CR: `ReplacementEffect::shield_counter_destroy` matches `ZoneChangeCause::Destroy` only (destruction as the result of an effect), so it does not apply to a move submitted with `ZoneChangeCause::StateBasedAction`; regeneration is not part of this unit.
**Rationale:** The SRS rules baseline (CR 2026-11-06) is the normative source for rule text, and the SRS rows are summaries of it. The proposed rows: 704.5g "It is destroyed. Regeneration can replace this." and 704.5h "It is destroyed. Regeneration can replace this." (shield counters removed from the 704.5g row).
**Alternatives:** Implementing the SRS row as written, so that the shield counter replaces lethal-damage destruction (rejected: contradicts CR 122.1c and 701.8b); editing the SRS in this PR (not allowed for agents, P8).

## #29 FP-028 — CR 704.7 group with two cause-specific effects: request sequence

**Decision:** (Review round 3, H2 (g).) With e1 (one use, `Destroy` only, redirect to exile) and
e2 (one use, `StateBasedAction` only, `Skip`) on a [`Destroy`, `StateBasedAction`] group of one
permanent of P1: one `CHOOSE_REPLACEMENT_EFFECT` request to P1 offering both effects (the union);
the chosen effect applies to its member; the other is then the only candidate and applies without
a request (CR 616.1f). Answer e1: applications [e1, e2]; answer e2: applications [e2, e1]. In both
cases the `StateBasedAction` member is skipped and the `Destroy` member moves the card to exile;
both effects are removed; the same for both submission orders (test
`req_event_001_r3_h2_g_union_of_candidates_one_request`).
**Rationale:** Derived from the member-wise rules of "CR 704.7 merging of state-based-action
batches" (union collection, one choice per round, application to matching members only,
repetition until no candidate remains).
**Alternatives:** Two separate requests, one per member (rejected: CR 704.7 makes the group one
event with one choice per round).

## #29 FP-028 — Enters-as-copy is applied as part of the entry event

**Decision:** (Review round 1, F6; replaced in review round 2, G1/F6.) `Modification::EntersAsCopyOf`
does not queue a `BecomeCopy` child and is not routed through replacement collection, so nothing
can skip it. The bus executes the surviving events with
`EventContext::execute_batch_with_entry_copies`, which sets the new object's copy link
(`copy_of(new) == Some(copied)`) inside the entering event's own execution, in the one executed
batch of the entry, before the next event of the batch and before the batch is logged:
`BusReport::executed` and the event log show one batch for the entry, holding the entering event
and no `BecomeCopy` event. The round 1 separate `BecomeCopy` batch is withdrawn. The source is
taken from its current information while it is in a zone, or from its last known information
when it left earlier in the same batch (LKI captured before the batch's moves) or in an earlier
call; the copy, and the whole call, fails with `UnknownObject` only when the source never existed
(no zone record and no LKI). A source that leaves later in the same batch is still in its zone
when the copy is set. `copy_of` may therefore name a retired id; resolving the copiable values
through `memory::information_for` / `LkiStore` belongs to the layer unit (#40 FP-039 / #38
FP-037) and is not implemented here. A `BecomeCopy` submitted on its own (a genuine post-entry
copy) still goes through replacement and stays strict. The CR 614.12 ETB look-ahead (projected
characteristics for other replacement effects) is still deferred to #40 FP-039 / #38 FP-037.
**Rationale:** CR 707.5: an object that enters "as a copy" "becomes a copy as it enters the
battlefield. It doesn't enter the battlefield, and then become a copy of that permanent." CR
603.6d: such an effect occurs as part of the event that puts the permanent onto the battlefield;
CR 614.1c makes "enters as" effects replacement effects; CR 616.1c ranks enter-as-copy effects;
CR 614.12a: a choice for an enter-modifying effect is made before the permanent enters. CR 707.2:
the copy takes the copiable values of the original. CR 608.2h and SRS 2.3 (REQ-CORE-003): an
effect that needs information about an object that left its zone uses its last known
information; in round 1 a source destroyed in the same call made the copy fail.
**Alternatives:** The round 1 separate copy batch right after the entry batch (rejected in round
2: the permanent existed for one executed batch as a non-copy, which CR 707.5 forbids); a
`BecomeCopy` child through the bus (round 0; rejected in round 1: another effect could skip it).

## #29 FP-028 — Scope note: enters-as-copy is part of the entry, through a new events entry point

**Decision:** (Review round 2, G1/F6.) The bus sets an enters-as-copy link through one new,
additive public method in `events/exec.rs`:
`EventContext::execute_batch_with_entry_copies(&mut self, batch: &EventBatch, copies: &[(usize,
ObjectId)]) -> Result<ExecutedBatch, EventError>`, each pair being (index into `batch.events()`
of an entering event, copied object). It has exactly the semantics of `execute_batch` (planning
run, LKI capture before any move of the batch, all-or-nothing, one logged `ExecutedBatch` iff an
event occurred) plus, in the real run only, the copy link set right after a listed event that put
an object onto the battlefield; the copied object must have a zone record or an LKI record (the
planning run does not check, as LKI for same-batch departures does not exist there yet).
`execute_batch` keeps its signature and behaviour and delegates with an empty list; `execute`
is unchanged. A standalone `GameEvent::BecomeCopy` stays strict: `UnknownObject` for a copied
object with no zone record, even when an LKI record exists. The method is used only by the bus.
This is not a P3 contract change and no R2 request was filed: the P3 contract list (state model,
canonical state hash, choice protocol and the others) does not include `EventContext`'s method
set; no signature, `GameEvent` / `EventState` / `EventLog` encoding, state hash or replay format
changes. Editing `events/exec.rs` is an R1 exception granted by the work item for this one
additive method (and its unit test in `events/tests.rs`).
**Rationale:** CR 707.5 forbids entering and then becoming a copy, so the link must be written
inside the entering event's execution; the bus alone cannot do it, because `set_copy` is
`pub(super)` to `events` and the link is only written by the executor, and a separate executed
batch is exactly what CR 707.5 rules out. Loosening `become_copy` to accept LKI was rejected:
`LkiStore` is never purged, so any long-departed id would become a valid standalone copy source.
CR 608.2h and SRS 2.3 supply the LKI fallback for the source.
**Alternatives:** Loosening `become_copy` (rejected, above); a `BecomeCopy` batch after the entry
(round 1; rejected, CR 707.5); a new `GameEvent` variant or a field on `ZoneChange` /
`CreateToken` (rejected: a P3 encoding and replay change).

## #29 FP-028 — One prevention follow-up per effect and recipient per batch

**Decision:** (Review round 2, G3; an R8 judgement; extended in review round 5, K4.) Within one bus
batch (one simultaneous batch of members, its `Instead` lineage members included since round 5;
the `Prevent` follow-up batch is separate), a prevention effect without a shield
(`effect.shield.is_none()`) whose action is `Prevent { then }` queues its `then` follow-up events
once per key (effect id, affected object, affected player), however many simultaneous damage
events to that recipient it prevents. Every one of those damage events is still fully prevented
and counted in `BusReport::prevented`. This applies to every shield-less `Prevent { then }`
effect, not only shield counters: a generic effect that prevents damage to one player from two
simultaneous sources and then gains life queues its life gain once. `Shield::Uses` and
`Shield::Pool` effects are unchanged (they count their own consumption and queue a follow-up per
application). A later batch, a later call and a child batch each start with a fresh set, so
separate damage batches remove separately. The work budget is charged only for follow-ups
actually queued. `BusReport::applications` still lists the effect once per prevented damage event
(each executed or prevented event's applied set contains it, and the field is "every application
of an effect"); only the follow-up is grouped. The CR 704.7 merge and the pool and uses
accounting are unchanged.
**Rationale:** CR 122.1c: "One or more shield counters on a permanent create a single replacement
effect and a single prevention effect that protect the permanent", whose prevention half is "If
damage would be dealt to this permanent, prevent that damage and remove a shield counter from it."
CR 615.13 speaks of a prevention effect being "applied to one or more simultaneous damage events",
which treats one application as covering simultaneous damage. Without the grouping, each
simultaneous damage event saw the counter before the queued removals ran and two counters were
removed for one simultaneous strike. The counter-argument, CR 615.10's per-event wording, is
limited to fixed-amount prevention. The CR does not settle the question; this is the
better-supported reading, recorded as an R8 judgement. Round 5 (K4, rules-researcher Q1, also an
R8 judgement: the CR is silent): the additional effects produced while applying prevention to
simultaneous damage events are simultaneous and form one modified event (CR 615.5: "the rest of
the effect takes place immediately afterward"; CR 120.4's example: the damage event "becomes
[5 damage is dealt to the defending player, the defending player gains 5 life]"; CR 615.13), so
follow-up damage of two prevented events of one batch to a permanent with two shield counters
costs ONE counter (CR 122.1c). All `Prevent` follow-ups of a batch form one child batch; the
group case equals the plain case (tests `req_event_001_r5_k4_group_case_equals_plain_batch`,
`req_event_001_r5_k4_inverse_simultaneous_damage_shares_one_removal`).
**Alternatives:** One follow-up per damage event (round 1; rejected in round 2: two counters for
one simultaneous strike); grouping only counter-backed effects (rejected: a matcher-specific rule
the bus would have to special-case); reporting one application per group (rejected: the applied
sets of the prevented events each contain the effect).

## #29 FP-028 — Tiers are declarative data

**Decision:** An effect's `Tier` is data on the effect. The bus does not check that a
`Modification` fits its tier (for example that a `BackFaceEtb` effect turns a face). The
back-face-up tier (CR 616.1d) is therefore only as correct as the effect's author, the DSL
toolchain, which validates `tierHint` against CR 616.1b-d.
**Rationale:** The engine has no face-turning modification yet; tier validation belongs to the
toolchain (SRS 14.4.4).
**Alternatives:** Inferring tiers from actions (cannot see self-replacement or back faces).

## #29 FP-028 — Standalone replacement registry

**Decision:** `ReplacementRegistry` is a separate component passed to `process_batch` next to the
`EventContext`. It is not joined to `GameState`, its Merkle layout or its wire form. The caller
keeps it equal to the effects in force; the bus only consumes shields and removes exhausted
effects.
**Rationale:** Same approach as "#27 FP-026 — Standalone event state and the rollback set"; the
integration unit joins the components.
**Alternatives:** Storing the registry in `GameState` now (outside the owned paths).

## #30 FP-029 — SRS §8.2 rule citations (CR 2026-11-06 wins)

**Decision:** Trigger detection follows the CR where SRS §8.2 cites it loosely: (1) item 2's
cost-payment LKI for tapped creatures rests on CR 702.122e, 702.171c and 702.184a, with LKI use
per CR 608.2h and 113.7a, not CR 400.7j (which covers objects moved to a public zone); (2) item 7's
ordering of waiting triggers is the two-part process of CR 603.3b (triggers whose condition is not
another ability triggering go first); (3) item 8's once-per-turn limit cites CR 603.2h for the
"Do this only once each turn" form; other once-per-turn limits are card text. "From anywhere" alone
does not make a trigger a leaves-the-battlefield ability (CR 603.6c): a `PUT_INTO_GRAVEYARD`
matcher with no `from: BATTLEFIELD` filter is matched post-event. A `ZONE_CHANGE` matcher with
`to: HAND` or `to: LIBRARY` and no `from` filter still looks back when the object was visible to
all players (CR 603.10a). The proposed SRS edit is a P8 line in docs/HUMAN_QUEUE.md.
**Rationale:** rules-researcher answers for #30 against the 2026-11-06 baseline; the SRS list of
603.10a–g look-back categories itself matches the CR.
**Alternatives:** Following the SRS citations literally (would cite a rule that does not cover
tapping).

## #30 FP-029 — Trigger detection design

**Decision:**
1. *Memory key scheme.* Trigger limits are counted per ability instance (source `ObjectId` +
   ability id) in the source's `Memory` as ordinary choices (`set_choice`, `ChoiceValue::Number`):
   `trigger.limit.turn:<abilityId>` (the turn the per-turn count belongs to),
   `trigger.limit.turn_count:<abilityId>` (triggerings in that turn) and
   `trigger.limit.game_count:<abilityId>` (triggerings this game). The per-turn count is read as 0
   when the stored turn differs from the current one. Keys are written only for abilities with a
   `limit`. A candidate triggers only if both counts are below their limits; then both increment.
   A non-number value under a key reads as 0.
2. *`TriggerEnv` abstraction.* Detection reads the game only through a trait (registrations per
   view, subject/predicate/condition evaluation per view, visibility). The integration unit
   implements it on the real state; unit and spec tests use fakes. Detection itself is pure
   apart from the limit counters it writes to the `MemoryTable`.
3. *No APNAP in the queue.* `WaitingTriggers` keeps creation order and offers
   `take_for_controller`; the two-part CR 603.3b ordering belongs to the stack unit.
4. *"From anywhere" is not look-back (CR 603.6c).* `PUT_INTO_GRAVEYARD` looks back only with
   `from: BATTLEFIELD`; with no `from` filter (or `from: ANY`) it is matched post-event, even when
   the object came from the battlefield.
5. *Table choices.* `ZONE_CHANGE`: from battlefield → leaves-the-battlefield, else from graveyard →
   leaves-graveyard, else to hand or library → hand/library (CR 603.10a), else none. `EXILED`:
   from battlefield or graveyard → those categories, else none (exile is not a hand or library).
   `RETURNED_TO_HAND`: from battlefield or graveyard → those categories, otherwise the
   hand/library category, which applies (pre-event view) only when the object's pre-event id was
   visible to all players; otherwise it is matched post-event.
6. *Intervening-if in the matcher's view.* The CR 603.4 trigger-time check runs in the same view
   as the matcher (pre-event for look-back triggers), with the candidate's bindings. A false
   condition consumes no limit.
7. *Limits vs CR 603.2h.* The SRS `limit.perTurn` / `limit.perGame` is a counter on triggerings;
   CR 603.2h covers only the "Do this only once each turn" wording. The counter model is what the
   SRS specifies and is implemented as such; whether a given wording maps to it is the
   definition author's concern.
8. *Grouping.* A `ONE_OR_MORE` candidate groups the matching events per ability instance
   (source id, ability id), across both views: the first matching event fixes the candidate's
   view, bindings and look-back category, and later matching events (either view) only add their
   indices (PR #179 round 1).
9. *Look-back candidates and Memory.* `detect` takes the pre-batch `MemoryTable` as `pre_memory`
   through `detect_with_pre_event_memory(env, pre_memory, memory, turn, events)`;
   `detect(env, memory, turn, events)` uses a copy of `memory` taken before the call. Limit counts
   of every candidate, including a look-back candidate whose source Memory `apply_zone_change`
   retired, are read from `pre_memory` plus an in-call tally of matches already accepted, so a
   once-per-turn limit cannot fire again in the same turn. Counters are written to `memory` only
   for a source that survives, defined as a post-event registration with the same source id and
   ability id; a departing source gets no entry, so no Memory is recreated for a retired id. The
   integration unit passes the pre-batch table (before the executor retires departing Memory) as
   `pre_memory`.

**Rationale:** SRS 8.2 (items 1-3 and 8) and the brief's table; CR 603.10, 603.10a-g, 603.6a,
603.6c, 603.4, 603.2c, 603.3b. Ordinary choices are lost on a zone change (the memory module's rule), so a new
object starts with fresh limits, as a new object should. Putting the turn number in the Memory
avoids a reset pass at each turn start.

**Alternatives:** A per-turn reset of counters at turn start (rejected: needs a hook into the turn
structure that this unit does not own); keying limits by ability id alone in a global table
(rejected: SRS 8.2 item 8 says per ability instance in Memory); ordering the queue by APNAP here
(rejected: CR 603.3b is two-part and belongs to the stack unit); treating every
`PUT_INTO_GRAVEYARD` from the battlefield as look-back (rejected: CR 603.6c).

## #30 FP-029 CR round 1 (PR #179) — instance identity, damage and life loss, gain of control

**Decision:**
1. *Ability instance identity.* An ability instance is (source `ObjectId`, ability id) as supplied
   by `TriggerEnv::registrations`. A producer that grants two independent copies of one definition
   ability to one permanent must give each registration a distinct ability id. Limit counters and
   `ONE_OR_MORE` grouping use that identity, preserved across the pre- and post-event views when
   the same source survives. No instance-key field was added to `TriggerRegistration`.
2. *Damage to a player.* Damage to a player by a source without infect causes life loss
   (CR 120.3a), so the normalizer emits a `DAMAGE_DEALT` occurrence followed by a `LIFE_LOST`
   occurrence carrying the player, the amount, the damage source and the recipient. Damage to a
   permanent never produces `LIFE_LOST` (CR 120.3c, 120.3d, 120.3e, 120.3h), and damage of 0 or
   less produces no `LIFE_LOST` (CR 120.8). Infect (CR 120.3b gives poison counters instead) is not
   modelled: `GameEvent::Damage` carries no infect flag and the executor always applies life
   loss. The effects unit that adds infect must add the flag to the event and make the normalizer
   skip `LIFE_LOST` for it.
3. *Gain of control.* CR 603.10d looks back only for a player losing control and for an opponent
   gaining control from that player; a bare "gains control" trigger uses the post-event state.
   `CONTROL_CHANGED` has no direction in the DSL matcher and the executor has no control-change
   event, so the classifier keeps `ControlChanged` as the look-back side (`ControlLost`) and
   nothing can emit the event yet. Deferred to #61 FP-059 (Effects III, control), which adds the
   control-change event and must give the matcher or event a gain/loss direction so a gain-side
   matcher is evaluated post-event; the gain-side test lands with it.
4. *Other round-1 choices.* `DESTROYED` is classified leaves-the-battlefield (destroy is the
   battlefield-to-graveyard move, CR 701.8a; an inference, since CR 603.10 does not name
   "destroyed"; it stays a kind distinct from `DIES`, CR 701.8b). A draw is a library-to-hand
   move: occurrence kinds {`DRAWN`, `ZONE_CHANGE`}, never `RETURNED_TO_HAND` (CR 121.1); the
   library is a hidden zone, so the hand/library look-back applies only if the environment reports
   the object visible to all. A created token enters the battlefield (CR 111.2, 701.7a): kinds
   {`TOKEN_CREATED`, `ENTERS`}, player subject the owner (creator), the controller as a separate
   `controller` field, no origin zone. Re-exile is `EXILED` but not `ZONE_CHANGE` (CR 400.8,
   406.7). A same-zone reposition (same object id) is no event (CR 400.7, an inference from "from
   one zone to another"). The sacrificing player is the permanent's controller read from its LKI
   record (CR 701.21a), through `events_from_batch_with_lki`; `events_from_batch` without LKI
   leaves the player unset. A counter kind spelled as a zone name becomes a `Zone` filter value.
5. *`detect` writes no counters for departing sources.* Because `detect` delegates to
   `detect_with_pre_event_memory`, it no longer writes limit counters for a source listed only in
   the pre-event registrations; matching, output and in-call limit enforcement are unchanged.

**Rationale:** rules-researcher answers for #30 round 1 against the 2026-11-06 baseline; CodeRabbit
findings on PR #179.

**Alternatives:** Adding an instance key to `TriggerRegistration` (breaks every environment fake);
inventing a direction filter on `CONTROL_CHANGED` (the DSL is outside this unit's paths); modelling
infect in the normalizer (no flag in the event).

## #30 FP-029 CR round 2 (PR #179) — survival, discard and mill, token destination, unattach

**Decision:**
1. *Surviving sources keep their limit counts.* Whether detection writes a trigger limit counter
   back to `Memory` is decided by `TriggerEnv::source_survives(source)`. It is true while the
   object still exists under the same id after the batch: a phased-out permanent still exists
   (CR 702.26d), while a zone change retires the id and its Memory (CR 400.7). The default method
   treats a source as surviving if and only if a post-event registration lists it (the round-1
   behaviour). The integration unit that implements `TriggerEnv` must override it with real object
   liveness, so that a phased-out permanent with a `perGame` limit does not fire again after it
   phases in. This supersedes the "survives" definition in "Trigger detection design" item 9:
   survival is `source_survives`, not post-event registration membership.
2. *Discard and mill players.* For `DISCARDED` and `MILLED`, the occurrence's player subject is the
   owner of the source hand or library (CR 701.9a, 701.17a). A sacrifice keeps its LKI controller
   (CR 701.21a).
3. *Token destination.* A created token's `ENTERS` occurrence carries `to: BATTLEFIELD`, with no
   `from` field and no `ZONE_CHANGE` kind (CR 701.7a, 603.6a).
4. *UNATTACHED, deferred to #180 (FP-143).* Under CR 701.3d an attachment becomes unattached in
   any of these cases:
   - it moves to a new host;
   - its host leaves the battlefield;
   - it leaves the battlefield;
   - its player host leaves the game.

   Phasing does not unattach it (CR 702.26j). The executor's `Outcome::Attached` records no
   previous host, and `Permanents::leave` logs nothing, so the normalizer cannot emit `UNATTACHED`
   for these cases. Today only an explicit `Unattach` event emits it (CR 603.10c). Reporting the
   lost links in executed outcomes changes the REQ-EVENT-001 outcome encoding, so it is its own
   unit, #180. Once that lands, `events_from_batch` emits `UNATTACHED` occurrences from the new
   records.

**Rationale:** CodeRabbit round-2 findings on PR #179, using the cached CR 2026-11-06 text. The
executor and its outcome encoding are owned by the closed #27 FP-026, so changing them needs the R2
contract-change protocol.

**Alternatives:** Deriving only the host-leaves case from LKI now (rejected: the move-to-new-host
case would stay wrong and the behaviour would be split across two sources). Changing `Outcome`
inside #30 (rejected: outside the owned paths, and it breaks the canonical encoding).

## #31 FP-030 — Independent Tester: what the spec tests pin and leave open

**Decision:** The black-box spec tests (`crates/fineprint-core/tests/spec_triggers_stack_*.rs`)
pin the ask sequence only where the brief fixes it. They pin:

- the ORDER requests' order (part 1 players in APNAP order, then part 2 players in APNAP order);
- sequential request ids;
- the modes request coming before the single targets request for a trigger.

They do not pin whether one player's CHOOSE_MODES / CHOOSE_TARGETS requests come before or after
the next player's ORDER request. Tests where both happen answer by request type. Target lists in
answers are given in ascending id order, so no test depends on whether `StackedTrigger.targets`
keeps the answer's order or sorts it.

**Rationale:** The brief (ask sequence items 1 to 3) says ordering happens before modes and
targets and that requests arrive in APNAP order, but it does not say how one player's target
requests interleave with the next player's ORDER request. It does not say whether chosen targets
are normalised either. Pinning either point would test something the contract does not state (R8).

**Alternatives:** Pin per-player completion (ORDER, then all of that player's modes and targets,
before the next player's ORDER), as CR 101.4b might suggest. Rejected: the brief does not state it,
and the integration unit can pin it later if needed.

## #31 FP-030 — Real-state environments belong to the integration unit

**Decision:** #31 delivers state, delayed, reflexive and sourceless triggers and the placement
procedure as pure library code behind the injected traits `TriggerEnv` (from #30) and the new
`PlacementEnv`. Implementing either trait on the real game state is out of scope for #31: the game
state has no stack and no ability registry yet. The game-loop integration unit implements both,
emits `TriggerPutOnStackEvent` / `TriggerRemovedEvent` (SRS 15.4) from `TriggerPlacement`, runs
the state-based-action loop of CR 603.3b, and takes over the four #30 requirements: (1) a real
`TriggerEnv::source_survives`; (2) snapshot the `MemoryTable` before the executor retires departing
Memory and pass it as `pre_memory` to `detect_with_pre_event_memory`; (3) normalise batches with
`events_from_batch_with_lki`; (4) give every ability instance a distinct ability id
(`DelayedTriggers::registrations` already does this with `delayed#<n>`). The "Out of scope"
paragraph of `triggers/mod.rs` says so.

**Rationale:** The brief's GOAL and REQUIREMENTS 7(a); R1 (the state, stack and loop are not owned
paths of #31); the #30 decisions "Trigger detection design" item 2 and "CR round 1" item 1.

**Alternatives:** A partial `TriggerEnv` on `GameState` now (rejected: no stack or registry to read,
so it would be a stub that the integration unit rewrites).

## #31 FP-030 — Inferences and choices in the trigger lifecycle

**Decision:**
1. *Outstanding from trigger time (CR 603.8).* A state-trigger instance counts as outstanding from
   the moment it triggers (while it waits and while it is on the stack) until
   `StateTriggers::instance_left_stack`. A false intervening "if" does not mark it.
2. *No ORDER ask below two triggers (CR 603.3b).* `ORDER_SIMULTANEOUS_TRIGGERS` is asked per player
   per part only when that player has 2 or more triggers in that part; a single trigger needs no
   ordering (the CR does not say a single trigger is ordered).
3. *Ordering before modes and targets.* Each player's order is chosen first; then each trigger, in
   that order, gets its modes and targets as it goes on the stack. A trigger removed later still
   took part in the ordering, and its removal leaves the others' relative order unchanged (the CR
   is silent).
4. *One removal reason.* Every "no legal choice" removal at placement (an unfillable required
   target slot, no choosable mode, fewer legal modes than the minimum, or no mode chosen for a
   min-0 mode set, CR 603.3c-d) uses `RemovedReason::NoLegalTargets`, which maps to
   `TriggerRemovedEvent{NO_LEGAL_TARGETS}`; SRS 15.4 has no other fitting reason.
5. *Unfillable slot.* A slot is unfillable when it has fewer legal targets than its minimum (no
   legal target for a min-1 slot is the common case). A slot with min 0 and no legal target stays
   in the request with empty `legal` and max 0 and does not remove the trigger. A mode is legal
   iff each of its own slots is fillable in this sense.
6. *One-shot delayed triggers (CR 603.7b).* When several waiting entries match a one-shot delayed
   trigger at once, the first matching element in input (creation) order is used and the
   registration is consumed. THIS DEPARTS FROM CR 603.7b, which gives the controller of the delayed
   trigger the choice of which simultaneous event causes it to trigger. The discarded matches
   cannot be recovered through stack ordering. A matching element whose intervening "if" is false
   is skipped and does not consume a one-shot entry. The departure is tracked as a follow-up owned
   by issue #182 (a `CHOOSE` choice for the controller, to be added with the integration of
   delayed triggers).
7. *Delayed triggers persist* when their source leaves its zone (the CR is silent; CR 603.7c only
   says the ability no longer affects an object that left its expected zone).
8. *Modes recorded ascending* as original indices into the mode list, whatever order the answer
   lists them in.
9. *Optional ask at resolution (CR 603.5).* `place_triggers` never asks
   `CHOOSE_OPTIONAL_TRIGGER`; the resolution unit calls `ask_optional_trigger`. Its default is `NO`
   (declining is the safe default and matches the generated yes/no default).
10. *SourcelessAbility split.* The inherent abilities are seven `SourcelessAbility` values under
    four `SourcelessKind` registrations (2 monarch, CR 726.2; 3 initiative, CR 727.2; rad counters,
    CR 729.1; speed, CR 702.179d), so one registration per player per designation fires several
    distinct abilities with stable ids.
11. *`responds_to_trigger`.* True iff the ability's trigger is an event matcher of kind
    `EventKind::Triggered` (wire `TRIGGERED`), the DSL's form of "another ability triggering"
    (CR 603.3b part 2); every other trigger (other event kinds, phase/step, state) is part 1.
12. *Request options.* `ORDER_SIMULTANEOUS_TRIGGERS` lists the trigger indices as
    `ChoiceOption::Number(i)` while n! is below `ENUMERATION_LIMIT`, and carries a validator
    contract (rule `Payload`, bounds n..n) from 8 triggers on. `CHOOSE_TARGETS` has request bounds
    (sum of slot minimums, sum of clamped slot maximums), lists the distinct legal targets of all
    slots (players before objects, ascending) while the number of valid answers is below the
    limit, and otherwise carries a validator contract (rule `Payload`) with the request bounds.
    `CHOOSE_MODES` is always enumerated (`ChoiceOption::Mode` per offered mode), as the brief
    says. Every request has `hidden: false` and an empty `reveal_to`.
13. *Duplicate players in the APNAP list* are a procedure error, like a controller missing from it.

**Rationale:** The brief's contract and ask sequence; CR 603.3b-d, 603.5, 603.7b, 603.8, 700.2b-c;
SRS 15.3 (ENUMERATION_LIMIT), SRS 15.4; R3 (sorted containers, deterministic re-execution).

**Alternatives:** Ask ORDER for a single trigger (rejected: a no-op request); interleave modes and
targets with ordering (rejected: the order must be fixed before triggers go on the stack); a
separate removal reason for "no mode" (rejected: SRS 15.4 defines only INTERVENING_IF_FALSE and
NO_LEGAL_TARGETS); removing a delayed trigger when its source leaves (rejected: not in the CR);
asking the optional choice at placement (rejected: CR 603.5 makes it at resolution).

## #31 FP-030 — State-trigger re-arm loop

**Decision:** A state trigger removed at placement for having no legal choice leaves the stack and
re-arms; if its condition still holds it triggers again at once, and the cycle can repeat
indefinitely. #31 does not break that loop: it is left to the CR 104.4b loop handling (a later
unit).

**Rationale:** CR 603.8 re-checks the condition after the instance leaves the stack; stopping the
cycle is a loop-detection question, not a trigger-placement one.

**Alternatives:** Suppress re-arming after a placement removal (rejected: CR 603.8 gives no such
exception).

## #31 FP-030 — SRS gaps found

**Decision:** Three SRS gaps, queued as P8 SRS edits in docs/HUMAN_QUEUE.md; the code follows the
CR (CR 2026-11-06 wins):
1. SRS 8.2 item 7 omits the two-part process of CR 603.3b. Already queued by #30 (HUMAN_QUEUE
   "[#30 FP-029] SRS §8.2" item (2)); not queued again.
2. SRS 8.2 item 4 omits "if the object with the ability is still in the same zone" from the
   re-check after the instance leaves the stack (CR 603.8).
3. SRS 15.3 gives no timing for `CHOOSE_OPTIONAL_TRIGGER`; CR 603.5 makes the "may" choice when
   the ability resolves.

**Rationale:** R8; PROCESS.md P8 (SRS edits need a person).

**Alternatives:** Edit the SRS here (rejected: P8).

## #31 FP-030 CR round 1 — ordering labels

**Decision:** The `ORDER_SIMULTANEOUS_TRIGGERS` payload keeps each trigger's
`PendingTrigger::description()` as its label. Only when two or more triggers of the same group
(same player, same CR 603.3b part) share that label is ` with ` and the trigger's chooser-visible
bindings appended, in binding-name order: `<name>=player <n>` for a player binding and
`<name>=object <n>` for an object binding that `PlacementEnv::is_public_object` accepts. Value
bindings and non-public objects are never shown; with nothing to show the label stays the base
description. `is_public_object` defaults to `false`, so by default no object is shown.
`PendingTrigger::description()` and `ask_optional_trigger` are unchanged.

**Rationale:** CR 603.3b and CR 101.4c let the controller choose the order, so the request should
let them tell otherwise identical entries apart. Which objects a player may see is the
hidden-information rule REQ-CORE-004, which belongs to the integration unit (DECISIONS "#31 FP-030
— Real-state environments belong to the integration unit"); the safe default is to show nothing
until that unit supplies the real visibility rule.

**Alternatives:** Always show every binding (rejected: leaks private objects, R12, REQ-CORE-004);
show the event indices (rejected: `PendingTrigger` does not carry them); change
`description()` for every trigger (rejected: changes labels that are already unique and the
optional-trigger request).

## #31 FP-030 CR round 1 — deferred findings

**Decision:** These CodeRabbit round-1 findings on PR #181 are deferred to issue #182 and are not
implemented in this unit:

(a) A one-shot delayed trigger's executable ability. `take_triggered` and `fire` remove the
registry entry, so a `PendingTrigger` of a delayed trigger has no stored effect. The form (carry
the ability in `PendingTrigger`, or a resolver-side snapshot keyed by the pending trigger) depends
on the stack-object and resolver representation (#58 FP-056), which does not exist yet. Thread:
https://github.com/donfuldauer/fineprint/pull/181#discussion_r4635776824

(b) `StateTriggers::check` ignores `limit` (perTurn, perGame) because it has no access to Memory.
The per-ability counters live in `MemoryTable` and are shared with event detection. The game-loop
integration (#44 FP-043) owns the call site and Memory access. Thread:
https://github.com/donfuldauer/fineprint/pull/181#discussion_r4635776870

(c) The CR 603.7b controller choice of which simultaneous event causes a one-shot delayed trigger
to trigger (see item 6 of "#31 FP-030 — Inferences and choices in the trigger lifecycle"). Until
it is added, the implementation departs from CR 603.7b.

(d) Combined target legality across slots (unverified review finding on `place.rs`).
`PlacementEnv::legal_targets` gives independent per-slot candidate lists, so "must target X if
able"-style requirements across slots cannot be expressed. This needs a combined-legality
extension of `PlacementEnv`, together with the casting legality unit (#52 FP-050) and the real
environments. Per-slot repeats stay allowed.

**Rationale:** Each item needs the real game state, stack or resolver that the integration unit
builds (see "#31 FP-030 — Real-state environments belong to the integration unit"). Deferring
keeps this unit's public API from changing twice.

**Alternatives:** Implement a stopgap interface now (rejected: it would be replaced by the
integration design and would change the public API again). Add the controller choice of (c) now
(rejected: it needs the same request and answer protocol as the integration unit's choices and
no card needs it yet).

## #31 FP-030 CR round 2 — delayed trigger limits

**Decision:** The trigger limit of a delayed triggered ability (`limit`, CR 603.2h) is kept by
`DelayedTriggers` in per-registration counters (the turn last counted, the count that turn, the
count this game). `DelayedTriggers::take_triggered(env, turn, waiting)` checks it for each
element after the intervening "if" passed (CR 603.4) and before the one-shot bookkeeping
(CR 603.7b), in input order. An element that fails its condition, or is dropped by the limit,
counts nothing and does not consume a one-shot. `registrations()` lists every delayed ability
with both `intervening_if` and `limit` set to `None`, so `detect` never counts a delayed
ability's limit, and Memory carries no `trigger.limit.*` counters for delayed abilities. The
counters are not keyed by source, so they survive the source leaving its zone and are not reset
by it, consistent with "#31 FP-030 — Inferences and choices in the trigger lifecycle" (delayed
triggers persist). `fire` (phase or step delayed triggers, no event) counts no limit. The
registration given to `TriggerEnv::condition_holds` inside `take_triggered` keeps the
intervening "if" and also has `limit` set to `None` (the same helper builds both).

**Rationale:** `detect` (unit #30's `detect.rs`, which this unit may not change) counts a limit
before it can see a delayed trigger's captured bindings, so since round 1 (where the condition
moved to `take_triggered`) an event that failed the condition used up the limit, within one batch
(the in-call tally) and across calls in the same turn (Memory), and suppressed a later passing
event. Counting in `DelayedTriggers` after the condition restores the order condition, then
limit.

**Alternatives:** Change `detect.rs` to accept captured bindings for delayed registrations
(rejected: the file belongs to #30 and must stay byte-identical). Roll Memory back after
`take_triggered` drops an element (rejected: it does not repair the in-batch tally, which drops
the later passing candidate before `take_triggered` sees it).

## #31 FP-030 CR round 3 — delayed trigger limits at a phase or step

**Decision:** `DelayedTriggers::fire(id, turn)` checks and counts the registration's limit with the
same counters as `take_triggered`; a dropped firing counts nothing and does not consume a
one-shot. This supersedes the sentence "`fire` ... counts no limit" in "#31 FP-030 CR round 2 —
delayed trigger limits".

**Rationale:** Finding 1 verified: a duration registration with `per_game: Some(1)` fired on every
call of `fire` (CR 603.2h, CR 603.7b).

**Alternatives:** Leave `fire` unlimited (rejected: the DSL attaches `limit` to every triggered
ability; SRS 8.2 item 8).

## #31 FP-030 CR round 3 — delayed trigger counters are not in the MemoryTable

**Decision:** The limit counters of a delayed triggered ability stay in `Delayed.counts` inside
`DelayedTriggers`; they are not written to `MemoryTable`.

**Rationale:** (i) `MemoryTable` is `BTreeMap<ObjectId, Memory>`
(crates/fineprint-core/src/memory/record.rs): its only identity is an object id, and
`MemoryTable::apply_zone_change` removes the old object's Memory when the object changes zone and
gives the new object only persistent choices (CR 400.7, CR 607.2p). A delayed ability is
registered by an effect, is not an object and has no object id of its own; its source is an
object whose Memory is retired on a zone change, yet a delayed trigger persists when its source
leaves its zone (docs/DECISIONS.md "#31 FP-030 — Inferences and choices") and its used limit must
still count. Keying the counters by the source's id would reset or lose them at the zone change;
keying them by a synthetic id would need a new identity space and a change in the memory module,
which this unit may not touch. (ii) `detect` must not count a delayed ability's limit in Memory at
all, because it checks the limit before it can see the captured bindings; the CR 603.4 order
(condition, then limit) is only possible in `take_triggered`/`fire` (docs/DECISIONS.md "#31 FP-030
CR round 2 — delayed trigger limits"). (iii) `DelayedTriggers` is itself per-game state with the
same purpose as Memory here: one value per ability instance (`delayed#<n>`, never reused, ids
unique per game), cloneable as a snapshot, ordered containers only (R3), alive exactly as long as
the registration; its counters are as deterministic and as serialisable as a MemoryTable entry.
The intent of SRS 8.2 item 8 (limits tracked per ability instance, persistent across the game,
deterministic state) is met; only the literal word "Memory" is not.

**Alternatives:** Store under the source's Memory (rejected, retired at zone change); a new Memory
slot keyed by `delayed#<n>` (rejected, memory module is another unit's, and the key space is not
object ids); change `detect.rs` (rejected, unit #30's file).
