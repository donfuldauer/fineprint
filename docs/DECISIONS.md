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

**Rationale.** REQ-NFR-006 asks for a Merkle hash that can locate the first differing component.
Hashing labels into the parent makes the digest commit to the component names, so two states
that hold the same component bytes under different names (for example a moved object) hash
differently, and children can be added or removed without positional ambiguity; the child count
and the injective length escape make every internal preimage parse uniquely. The distinct
`0x00`/`0x01` prefixes keep leaves and internal nodes from colliding. Returning `Option` avoids
inventing a path for equal trees. Keeping the trait in place honours the #8 contract (R2).

**Alternatives.** Moving the trait into `encode` (rejected: breaks the #8 contract and imports).
A plain `u32` length that saturates or panics above `u32::MAX` (rejected: not injective or not
panic-free). Positional children without labels as in the FP-004 prototype (rejected: the
divergence finder needs stable names, and label-free hashing cannot tell renamed components
apart). Returning the root path for equal trees (rejected: ambiguous with a root-level
difference). Timing assertions for incremental cost (rejected: R7). proptest (rejected: not an
approved dependency).
