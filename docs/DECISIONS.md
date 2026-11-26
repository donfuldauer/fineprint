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

## Self-chaining — handoff through the backstop scheduled task

**Superseded** by "Self-chaining — next session starts from a merged issue/ PR" below:
`fire_trigger` is refused for a task the session did not create.

**Context (2026-11-20).** Self-chaining used `create_session`, so each session started the next.
That ran without prompts while the chain descended from a session the owner started. After the
chain broke, the backstop scheduled task restarted it. From then on every session descended from a
scheduled-task session and was tagged `config:headless-lineage`. In those sessions every
remote-session tool call (`get_session`, `create_session`, `unsubscribe_pr_activity`) asked the
owner for approval, flagged "This connector call requires your approval to proceed." with no
always-allow option. The allow rules in `.claude/settings.json` did not stop those prompts. The
session started by the scheduled task itself made the same calls without prompting. This is
observed behaviour, not documented, and is inferred from session tags and transcripts.

**Decision.** `work-issue` §8 hands off by calling `fire_trigger` on the backstop scheduled task
(`Fine Print: next ready issue (backstop)`) instead of calling `create_session`. Every session in
the chain is then started by the scheduled task, whether the owner or the task started the first
one. The fired run's text names the firing session, and the task ignores that session when it
checks for a live chain. The marker comment is posted as pending before firing, then edited to
handed off (or failed). A retry that finds a pending marker fires again: a duplicate run
finds the first run's session as a live chain and stops. §1 step 4 reads the session's own ID from `CLAUDE_CODE_REMOTE_SDK_URL`
instead of calling `get_session`. The session-depth limit is dropped, because fired sessions have
no parent. The owner chose no replacement cap: the chain stops when no issue is ready, or when the
task finds another live chain.

**Rationale.** Sessions started by the scheduled task run without remote-session approvals, so the
chain runs unattended.

**Risks.** If fired runs start prompting too, or `fire_trigger` itself prompts, the chain needs
approvals again. The next scheduled run restarts a broken chain within four hours. A scheduled
run and a fired run can start at the same moment. Claim ordering (§1 step 4) keeps them off the
same issue. A run that finds another live chain stops, so at most one chain survives.

**Alternatives.** Keep `create_session` and approve each handoff (rejected: the chain is meant to
run unattended). Have the owner restart the chain by hand after every break (rejected: the
backstop exists to do that).

## Self-chaining — next session starts from a merged issue/ PR

**Context (2026-11-21).** The `fire_trigger` handoff above failed in its first use (issue #40,
PR #202): a session may fire only scheduled tasks it created, and the backstop was created in the
web UI. PR #205 moved the start of the next session to a scheduled task with a GitHub trigger on
pull requests merged into `main` from `issue/` branches. The kilohertz-pinball repository made the
same change (its PR #143); its CodeRabbit rounds added unblocking rules that are adopted here.

**Decision.**
- A session never starts its successor; the merge of an `issue/` branch does (`work-issue` §8).
  A session on another branch says in its PR that the chain will not continue from that merge.
- The next session can start before the merging session finishes unblocking, so `work-issue`
  §1 step 0 runs the same sweep as §7 step 3 before choosing an issue.
- The sweep covers every open `work-unit` issue labelled `blocked`, not only the merged issue's
  "blocking" list, so a blocker replaced by another issue is caught when the replacement merges.
- Before changing labels the sweep re-reads them and skips an issue that lost `blocked` or gained
  `status:blocked-failure`. It adds `ready` and then removes `blocked` with single-label REST
  calls, so the issue is never without both and no other label is rewritten.
- The "Unblocked by #<D> (PR #<P>)" comment names the blocker whose merged PR satisfied it last,
  is posted once per unblock (an earlier comment naming another blocker or PR does not suppress
  it), and is omitted (with a RUN_LOG note) when none can be identified.
- `blocked` on an issue with no blockers is a person's hold: the sweep leaves it, and an explicit
  "work on #N" request stops on it. On an explicit request, a `blocked` label whose blockers are
  all satisfied is stale and is replaced with `ready`.

**Rationale.** The merge trigger needs no handoff code, so no remote-session approvals, no marker
comments and no depth limit. The sweep makes unblocking independent of which session finishes
first.

**Accepted risk.** A person could add `status:blocked-failure` between the re-read and the label
calls. The window is seconds long, adding `ready` never removes the hold label, and P7 sets that
label only on an issue a session was working, not on a `blocked` one. (kilohertz-pinball accepts
a wider version of this race because its MCP tools only replace the full label list; fineprint
uses label-level `gh` calls.)

**Alternatives.** The `fire_trigger` handoff (rejected: refused for tasks the session did not
create). `create_session` chaining (rejected: approval prompts). Unblocking only from
`issues/<N>/dependencies/blocking` (rejected: misses replacement-covered blockers).

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
subgames are later work. Update (#208 FP-154): the CR 404.3 graveyard case is done in `events/`;
CR 401.4 and CR 405.3 remain later work. (g) Only objects on the stack or the battlefield have a controller
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

   *Landed in #180 (FP-143):* executed events now carry `ExecutedEvent::unattached` records and
   `events_from_batch` emits `UNATTACHED` from them; the player-leaves-game case stays deferred
   (see "#180 FP-143 — Host player leaving the game: deferred").

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

## #31 FP-030 CR round 4 — delayed trigger condition at a phase or step

**Decision:** `DelayedTriggers::fire(env, turn, id)` evaluates the stored intervening "if"
(CR 603.4) with the captured bindings in `StateView::PostEvent` before it counts the limit
(CR 603.2h); a false condition counts nothing and does not consume a one-shot (CR 603.7b);
`take_triggered` and `fire` share one condition helper and one counter set. This supersedes
(i) the sentence "`fire` ... counts no limit" in "#31 FP-030 CR round 2 — delayed trigger
limits", (ii) "No intervening 'if' is evaluated here" in "#31 FP-030 CR round 3 — delayed
trigger limits at a phase or step" and (iii) the words "only possible in
`take_triggered`/`fire`", which read as if `fire` evaluated the condition before round 4: from
round 4 on both do.

**Rationale:** The finding, verified: a false occurrence used up a per-turn or per-game limit
and a later true occurrence returned None.

**Alternatives:** A `condition: bool` parameter (rejected: the caller would need the stored
predicate and the captured bindings, which `DelayedTriggers` owns, and could skip the check); a
documented caller-side check (rejected: the same, and `take_triggered` does it inside, so the
two entry points would differ).

## #31 FP-030 CR round 4 — limits of state triggers are the caller's

**Decision:** `StateTriggers::check` does not apply a state trigger's `limit` and does not stay
in step with a delayed ability that has a state trigger (state.rs doc).

**Rationale:** `check(env)` has no turn and no Memory, `detect` skips `Trigger::State`
(detect.rs), and the integration unit owns turn state and Memory; changing `check`'s signature
touches every caller and test and is out of scope for this round.

**Alternatives:** Counters inside `StateTriggers` keyed by (source, ability id) and a `turn`
parameter (rejected for this round: a signature change to a public method; recommended as the
first item of the integration unit); a P8 item (rejected: a design choice, not a human task).

## #32 FP-031 — Layer pipeline API and modification set

**Decision:** `fineprint_core::layers` evaluates a closed Rust enum `LayerModification` (layer
1a `CopyValues`, 1b `FaceDown`, 2 `SetController`, 3 `SetText`, 4 `AddTypes`/`SetTypes`/
`RemoveSubtypes`, 5 `SetColors`/`AddColors`, 6 `AddAbility`/`RemoveAllAbilities`/`RemoveKeyword`,
7a and 7b `SetPt`, 7c `ModifyPt`, 7d `SwitchPt`), not DSL `Modification.op` strings; the table is
`LayerModification::belongs_to` and `ContinuousEffects::register` enforces it
(`LayerError::LayerMismatch`). The module is standalone: it is not wired into `GameState`, the
Merkle layout or the state hash, and `state::EffectRegistry` is untouched. Besides the contract
names it exports the helpers `KEYWORD_COUNTER_KINDS`, `keyword_counter_ability`, `literal`,
`literal_of` and `pt_counter_delta`. The `IdGenerator<Timestamp>` parameters are named `ids`,
because `gen` is a reserved keyword in edition 2024 (the types are exactly the contract's).

**Rationale:** Decision "#13 FP-012" keeps `Modification.op` an open string with the layer-order
checks in the toolchain, so the engine needs its own closed set; a Rust enum makes an op in the
wrong layer unrepresentable after registration. The set is the minimum that covers each layer and
sublayer (CR 613.1, 613.2, 613.4) with the brief's tests.

**Alternatives:** Interpret DSL `Modification` values directly (rejected: open op strings, and
compiling DSL to the enum belongs to the integration unit); wire into `GameState` now (rejected:
out of scope for this issue).

## #32 FP-031 — Placeholder DSL trees in `Characteristics`

**Decision:** `Characteristics.abilities`, `power`, `toughness` and `rules_text` are the
placeholder JSON wrappers of `fineprint_dsl::card` (`AbilityAST(Value)`,
`ValueExpressionAST(Value)`), not the real `fineprint_dsl::ability::AbilityAST` and
`fineprint_dsl::expr::ValueExpressionAST`. The brief's "`AbilityAST::Keyword` with `keyword` = kind
and `base.ability_id` = `counter:<kind>`" is therefore the JSON object
`{"kind":"KEYWORD","abilityId":"counter:<kind>","keyword":"<kind>"}` (the wire form of a
keyword instance); `RemoveKeyword(k)` removes every ability whose JSON object has
`"kind":"KEYWORD"` and `"keyword":k`. "`ValueExpressionAST::Literal(n)`" is the bare JSON integer
`n` (the form the layouts and objects tests already use for printed P/T); `literal_of` also
reads `{"LITERAL": n}`. `LayerEnvironment::eval_value` takes the placeholder type, since that is
what a base value holds.

**Rationale:** `Characteristics` belongs to the objects unit and may not change here; the
placeholder wrappers are what an evaluated object can hold. The JSON shapes match the DSL wire
format, so the later switch to real ASTs is mechanical.

**Alternatives:** Change `Characteristics` to the real ASTs (rejected: another unit's file and a
breaking change); a side list of keyword counters outside `abilities` (rejected: LKI and every
consumer read `abilities`).

## #32 FP-031 — Ordering, tie-break and P/T presence

**Decision:** Layers apply in `Layer::ALL` order (1a, 1b, 2, 3, 4, 5, 6, 7a, 7b, 7c, 7d; there is
nothing after 7d). Within a layer, steps sort by timestamp; at equal timestamps effects come
before counters, effects by ascending `EffectId`, counters by ascending kind. `is_cda` is stored
and ignored. A missing counter timestamp is `Timestamp::new(0)`; counters with amount below 1 are
ignored. P/T presence is decided once at the start of layer 7 with `pt_applies(on_battlefield)`;
when the object has no P/T the evaluated `power` and `toughness` are `None` (so a noncreature
permanent with printed P/T evaluates to `None`, CR 208.3) and 7a-7d change nothing. When it has
P/T, both are written back as literals after every step, so a later selector sees the current
values. Arithmetic saturates instead of overflowing. P/T counters change power and toughness only
when the object's card types after layers 1-6 include Creature (CR 122.1a), so a noncreature card
with printed P/T off the battlefield keeps its printed values while layer-7 effects still apply
to it (CodeRabbit round 1, comment 4637189743).

**Rationale:** The tie-break is the classifier's (brief: "do not revisit"); CDA-first and 613.8
are later units. Reading "otherwise they are left None" as "the evaluated values are None" keeps
the result consistent with CR 208.3 for consumers that do not call `pt_applies` themselves.

**Alternatives:** Keep the printed values on a noncreature permanent (rejected: a consumer would
see P/T the object does not have); order equal timestamps by registration order (rejected:
registration order must not change results, R3).

## #32 FP-031 — `sameAsLayer` rejected

**Decision:** `ContinuousEffects::register` rejects `Affected::SameAsLayer(_)` with
`LayerError::Unsupported`; evaluation treats it as applying to nothing (unreachable through the
API).

**Rationale:** CR 613.6 lock-in is a later unit (SRS 6.3, decision "#14 FP-013"); silently
treating it as a selector or as an empty set would give wrong results.

**Alternatives:** Accept and ignore (rejected: silent wrong answers).

## #32 FP-031 — Timestamp ledger behaviour

**Decision:** Every ledger operation and helper that issues timestamps works on a clone of the
generator (and of the ledger or registry) and commits only on success, so `IdExhausted` leaves
the ledger, the registry and the generator unchanged, also when the generator runs out in the
middle (an object stamped but its stickers not). `StampReason` does not change behaviour
(CR 613.7d-j each give one new timestamp). In `stamp_objects_simultaneously` each object's
stickers are stamped right after that object, before the next object (CR 613.7k with 613.7m).
`restamp_static_effects` gives granted effects (`granted_at: Some(_)`) whose current timestamp is
later than the old object timestamp new stamps from the generator in ascending (old timestamp, `EffectId`) order;
it does not check that they exceed `new_object_ts` (true whenever the caller issued
`new_object_ts` from the same generator first). `static_effect_timestamp` returns `object_ts` for
`Resolved`. The current effect timestamp, not `granted_at` (which never changes), decides which
granted effects followed the host and which follow the granting effect, so a second and third
re-stamp keep the CR 613.7a relative order (CodeRabbit round 1, comment 4637189775).

**Rationale:** All-or-nothing keeps the single monotonic counter and the ledger consistent
(REQ-CORE-002). The brief fixes the rest.

**Alternatives:** Let the generator advance on a failed operation (rejected: a gap is harmless
but an unchanged state is easier to reason about and to test).

## #32 FP-031 — Keyword counter variants

**Decision:** `keyword_counter_keyword(kind)` is the one classifier for keyword counters used by
both the layer-6 step selection and `apply_counter`: `Some(kind)` for one of the fifteen
`KEYWORD_COUNTER_KINDS`, or for one of them followed by exactly one ASCII space and a non-empty
parameter that does not itself start with a space ("hexproof from red"); `None` otherwise ("ward",
"ward 2", "flyingx", "hexproofish", "hexproof ", "hexproof  from red", and any case variation).
The granted ability keeps the full kind string as its keyword and in its ability id
(CodeRabbit round 1, comment 4637189536).

**Rationale:** CR 122.1b includes "any variants of those keywords"; the brief fixes the
name-space-parameter shape. Rejecting an empty parameter and a doubled space keeps malformed
kinds from becoming keywords no rule names. Matching is case-sensitive like the counter kinds
themselves.

**Alternatives:** Accept any suffix after the space, including empty or space-led (rejected:
"hexproof " would grant a keyword that is neither hexproof nor a variant).

## #32 FP-031 — Follow-up work for later units

**Decision:** Left for later issues: (1) wire `ContinuousEffects` and `TimestampLedger` into
`GameState`, its Merkle layout and the state hash, replacing the placeholder
`state::EffectRegistry`; (2) switch the call sites that build a `CharacteristicsProvider` or read
raw counters (events, triggers, replacement, zones, LKI capture) to `LayeredProvider` and the
evaluated characteristics; (3) CR 613.8 dependency ordering; (4) characteristic-defining
abilities first (CR 613.3, 613.4a) using the stored `is_cda`; (5) CR 613.6 lock-in
(`sameAsLayer`); (6) the cached evaluation mode and the cached-versus-full-recompute differential
check (REQ-LAYER-005); (7) the passes after the layers (CR 613.10, 613.11); (8) compiling DSL
characteristic effects into `LayerModification` and word-level text changes (CR 612); (9) effect durations and
expiry; (10) CDA-first ordering (CR 613.3, 613.4a) and CR 613.8 dependency recomputation,
including recomputing dependencies as effects apply: #33 FP-032 (CodeRabbit comments 4637189551,
4637189562); (11) multi-layer affected-set retention (CR 613.6), requiring `Affected::Objects`
for `EffectOrigin::Resolved` effects that modify characteristics or control (CR 611.2c), and
checking that a static effect's source still has the ability when the effect first applies
(CR 613.6, 611.3b): #34 FP-033 (comments 4637189504, 4637189573, 4637189588); (12) passing the
evaluated state (current controller, earlier-layer results of other objects) to non-literal CDA
value expressions (`LayerEnvironment::eval_value`), which needs layer-major evaluation across
objects after #33: #186 FP-146 (comment 4637189737); (13) exposing the evaluated controller
through `LayeredProvider` before LKI capture reads it: #184 FP-145 (comment 4637189755); (14)
sticker timestamps when an object becomes a component of a merged permanent (CR 613.7k, 123.5b;
a merge operation taking both ids): the merged-permanent unit #65 FP-063 (comment 4637189771).

**Rationale:** Out of scope per the brief; listed so the plan can pick them up.

**Alternatives:** None.

## #32 FP-031 — RemoveKeyword and hexproof variants

**Decision:** `LayerModification::RemoveKeyword(k)` removes every keyword ability whose keyword
is `k` exactly and, for `k` = "hexproof" only, every variant "hexproof from [quality]" (the
name, one ASCII space, a non-empty parameter not starting with a space; the same `is_variant_of`
helper the keyword-counter classifier uses). The list of such keywords is the private const
`REMOVAL_REMOVES_VARIANTS = ["hexproof"]`. `RemoveKeyword("hexproof from red")` stays exact.
Implemented in this unit (CodeRabbit round 2, outside-diff comment on `pipeline.rs`).

**Rationale:** CR 702.11e states that an effect that makes an object lose hexproof makes it lose
all "hexproof from [quality]" abilities; the baseline states removal-removes-variants for
hexproof only, so protection, landwalk and the other keyword-counter keywords stay exact. The
other two clauses of 702.11e (targeting a creature as though it didn't have hexproof, and an
effect that looks for a card with hexproof) belong to target legality and DSL predicate
evaluation, not the layer pipeline: they are left to #52 FP-050 (target legality) and #57
FP-055 (predicate evaluation).

**Alternatives:** Leave `RemoveKeyword` exact and defer 702.11e to DSL compilation (rejected:
the classifier already exists and the review asked for it); remove variants of every keyword
(rejected: no CR rule says so for other keywords).

## #33 FP-032 tester - Outside dependent of a loop that the first application breaks

**Decision:** The spec tests for a dependency loop with an outside dependent effect assert the
SRS 6.2 step 3 sequence literally: the loop's earliest member is applied (Loop), the relation
is recomputed (CR 613.8c), and the outside effect waits only for the effects it still depends
on. In `loop_with_outside_dependent_waits_for_all_members` (three-member loop) the outside
effect still depends on a remaining member and waits until after all of them, as the brief
states. In `two_effect_loop_with_earlier_outside_dependent_recomputes` (two-member loop, outside
effect earliest) the first application stops the other member from applying, so the outside
effect no longer depends on it and is applied before it.

**Rationale:** SRS 6.2 requires recomputing after each application and choosing the earliest
candidate; a member that no longer applies changes nothing, so nothing depends on it. The
brief's "waits until after all of them" holds whenever the dependency persists.

**Alternatives:** Assert that the outside effect always follows every loop member (rejected:
contradicts SRS 6.2 step 3 and CR 613.8c as quoted in the brief).

## #33 FP-032 tester - Modelling the SRS 6.2 dependency examples

**Decision:** The synthetic dependency examples use condition 613.8a (b) "what it applies to":
the dependent effect has a selector (has the marker ability; is a creature) that the other
effect's application changes. Effect existence is not modelled (the registry API has no link
from an effect to the ability that generates it). An effect that grants a keyword and one that
removes it, two P/T-setting effects, and two effects that overwrite each other are treated as
timestamp cases, not dependencies. A CDA chosen in layers 2-6 while a non-CDA step of its layer
is unapplied is recorded as CdaFirst even when it is also the earliest step (the brief's
Selection definition).

**Rationale:** Applicability is the only (b) relation observable through the pre-existing API;
the timestamp cases follow the SRS 6.2 timestamp example and the brief's item 4.

**Alternatives:** Model existence through `source` (rejected: no API for it).

## #33 FP-032 — Dependency loop definition

**Decision:** When no unapplied step of a layer is a candidate (every one depends on another),
the loop applied is a strongly connected component of the dependency graph over the unapplied
steps whose members depend on no step outside it (no incoming edge from outside, edges read
"A must precede B"). With several such components, the one containing the step with the
earliest ordering key is taken; only its earliest member is applied, labelled `Loop`, and the
whole relation is recomputed before the next choice (CR 613.8b, 613.8c). This is an engine
interpretation of "dependency loop" (SRS 6.2), not CR text.
**Rationale:** SRS 6.2 step 3 defines the loop this way. A step that depends on a loop member
without being depended on by it is not part of the loop and waits for it; such a component
always exists when there is no candidate, so the procedure never stalls. Applying one member
at a time follows SRS 6.2 ("Apply the earliest-timestamped member, recompute, and repeat").
**Alternatives:** Any cycle (rejected: a step outside the loop that waits for it would be
treated as a member); applying every member of the loop in one go (rejected: SRS 6.2 and
CR 613.8c require recomputing after each application).

## #33 FP-032 — Scope of the trial application

**Decision:** Dependency (CR 613.8a (b)) is judged on the object under evaluation only. For
each unapplied A, A is applied to a clone of the object's partially evaluated state (the
scratch copy, never the real state); B depends on A when B's affected set includes the object
in exactly one of the two states (its existence and what it applies to), or when B applies in
both and what B does differs. "What B does" is compared field by field on the object's
characteristics and controller: a field counts as unchanged when B leaves it with the same value
in both states or untouched in both; for sets and colors, element by element (kept as it was in
both, or present in both, or absent in both); for the ability list, each ability's copy count is
the same in both or changed by the same amount; for numeric power and toughness, the same
amount is added; the P/T pair also counts as unchanged when it is switched in both. Effect
existence through the generating ability is now checked through the
`LayerEnvironment::generating_ability_exists` hook (superseded: see "#33 FP-032 — Review round 1
dispositions"). The set of `env.objects()` that a selector matches on other objects is not
compared.
**Rationale:** Evaluation is per object, so the partially evaluated state of other objects is
not available (layer-major evaluation across objects is #186 FP-146); evaluating them inside the
trial would recurse through the whole game for every pair. Comparing what B does as a change
rather than as a final state is what keeps unrelated effects independent (two type-adding
effects, a setter after an adder) while any change of applicability is still found. The
comparison reads only characteristics, never the modification kind (REQ-DSL-013, R4), except
the declared numeric P/T adjustment (see "#33 FP-032 — Review round 1 dispositions").
**Alternatives:** Compare B's final output literally (rejected: every additive pair, such as two
subtype grants, would form a loop); evaluate every object of `env.objects()` up to the current
layer for the trial (deferred to layer-major evaluation, #186 FP-146).

## #33 FP-032 — Non-CDA effects in layer 7a

**Decision:** An effect registered in layer 7a with `is_cda: false` is neither rejected nor moved:
it applies in layer 7a in the selection order like any 7a effect. Layer 7a has no CDA-first
grouping (it is ordered by the selection procedure alone), so `is_cda` decides only condition
(c) of dependency there.
**Rationale:** The brief asks to keep such an effect applying in the 7a order; the registry
already accepts `SetPt` in 7a regardless of `is_cda`, and rejecting it would change the
REQ-LAYER-001 registration contract (R2). SRS 6.2 item 1 and CR 613.4a describe which effects
belong to 7a; the validation "7a implies isCDA" belongs with DSL/override validation
(REQ-DSL-004) or the #183 FP-144 wiring, and rejecting it in `register` would change merged #32
behaviour and its tests (`layers/tests.rs` registers a non-CDA 7a effect) (review round 1, F3).
**Alternatives:** Reject at registration (rejected: contract change); move it to 7b (rejected:
reinterprets the declared layer).

## #33 FP-032 — Tie-break unchanged; simultaneous timestamps out of scope

**Decision:** The ordering key stays the one of "#32 FP-031 — Ordering, tie-break and P/T
presence": timestamp, then effects before counters, then ascending `EffectId`, then ascending
counter kind. It decides the order among candidates and the earliest member of a loop.
Assigning timestamps (CR 613.7m APNAP order, CR 613.7n static before resolved) is the timestamp
ledger's job (#32 FP-031); this unit only orders by the timestamps it is given.
**Rationale:** No CR rule orders two effects with equal timestamps from separate static abilities
of one object; keeping the existing key makes the no-dependency, no-CDA order identical to the
REQ-LAYER-001 order (property test).
**Alternatives:** A new key (rejected: changes REQ-LAYER-001 results without a rules reason).

## #33 FP-032 — Selection labels in the order record

**Decision:** Each `OrderStep` is labelled: `Loop` if chosen as the earliest member of a
dependency loop; else `CdaFirst` if chosen in layers 2-6 while it is a CDA and a non-CDA step of
its layer is still unapplied; else `Dependency` if an unapplied step of its layer with an
earlier ordering key existed when it was chosen (it was released by dependency); else
`Timestamp`. `applied` is false only for an effect whose affected set did not include the object
when it was chosen; counters are always applied. A CDA chosen first while non-CDAs wait is
`CdaFirst` even when it is also the earliest step.
**Rationale:** The brief's definition; the labels are computed from plain data, so the record is
deterministic and ready for a rules-trace event.
**Alternatives:** Label by cause analysis (which dependency released it) (rejected: more data
than the record needs, and not required by SRS 6.2).

## #33 FP-032 — Cached versus full-recompute differential

**Decision:** The P5 gate item "cached and full-recompute agree" is not applicable to this unit:
there is no cached evaluation mode yet. The differential test belongs to the unit that adds the
cache (REQ-LAYER-005).
**Rationale:** Nothing to compare against; `evaluate` and `evaluate_with_order` always recompute.
**Alternatives:** A placeholder test (rejected: it would assert nothing).

## #33 FP-032 — Review round 1 dispositions

**Decision:** (a) F4, effect existence (CR 613.8a, 611.3, 113.6): `LayerEnvironment` gains the
additive method `generating_ability_exists(effect, source, source_view) -> bool`, default `true`.
`step_applies` (used by both the real application and the dependency trial) applies an effect
only if its affected set includes the object and, for an `EffectOrigin::Static` effect with
`source: Some(src)`, the hook says its generating ability exists. `source_view` is the state so
far only when `src` is the object being evaluated, `None` otherwise. `Resolved` effects
(CR 611.2: they exist independently of their source's abilities) and static effects without a
source are not checked. The trial then treats "B stops existing after A" exactly like "B stops
matching". Existence for sources other than the evaluated object is left to #186 FP-146
(layer-major evaluation) and a real ability-identity implementation of the hook to #183 FP-144.
This supersedes the sentence "Effect existence through the generating ability is not modelled"
of "#33 FP-032 — Scope of the trial application". `LayerEnvironment` is used only inside
`layers/` (not a P3 contract), so the additive defaulted method is not an R2 contract change.
(b) F1, saturation: for a step that declares a numeric P/T adjustment (a `ModifyPt` effect, or
P/T counters: amount times the per-counter change, computed in `i128` without saturation), the
"same amount added in both states" comparison uses the declared adjustment, not the difference
of the saturated values. A step that does not apply, P/T counters on a non-creature and an
object without a P/T pair declare nothing, so differences in whether the step does anything are
still found; if either state declares, power and toughness count as unchanged exactly when the
two declarations are equal. Saturating arithmetic in `apply_effect`/`apply_counter` is unchanged.
The former fallback in the power/toughness comparison ("literal numbers with the same amount
added", ordering.rs `number`) is removed: with adders declared, no modification that declares
nothing can add the same nonzero amount in two different states (setters write the same value,
a switch keeps toughness minus power only when it changes nothing), so it was unreachable.
(c) F3: a non-CDA effect registered in layer 7a stays accepted (see the amended "#33 FP-032 —
Non-CDA effects in layer 7a").
**Rationale:** (a) Without the check, an effect whose generating ability another effect removed
could start matching after that effect and apply (a false dependency); SRS 6.2 item 4 requires
comparing B's existence. A hook keeps the layer system free of ability identity, which the
registry does not model yet. (b) At the `i64` bounds the saturated write makes an adder appear
to add a different amount depending on the state (0 at the bound, 1 below it), creating a false
dependency between two adders and a result different from timestamp order. Reading the declared
amount is a generic read of the modification, never of card identity (R4); it is the one
exception to "the comparison never reads which modification B is".
**Alternatives:** (a) Add an ability link to the registry now (rejected: contract change, and
belongs to #183 FP-144); compare existence with a separate trial path (rejected: the real
application and the trial would disagree). (b) Widen P/T to `i128` everywhere (rejected: changes
the `i64` contract and the saturation behaviour); compare saturated values (the bug).

## #34 FP-033 - "Starts to apply" and the empty first-layer set

**Decision:** The set of a multi-layer effect is fixed by its anchor, the part in the earliest
layer or sublayer of the group (613.1 order); each `SameAsLayer` part applies to an object
exactly when that anchor applied to it earlier in the same evaluation. When the anchor applied to
no object (an empty set, a selector that matched nothing, or a static anchor whose generating
ability did not exist), the linked parts apply to nothing: the empty set is locked like any
other. This is an INFERENCE, not a rule: CR 613.6 does not define "starts to apply" nor say what
happens when the first layer's set is empty.
**Rationale:** 613.6 says the effect "will continue to be applied to the same set of objects";
reading "the same set" literally makes the empty set stay empty, and it keeps one rule (follow the
anchor) with no special case. Letting a later part fall back to its own selector would make the
later layer choose a set the first layer did not, which is what 613.6 forbids.
**Alternatives:** Treat an empty first set as "never started" and let later parts choose their
own set (rejected: not supported by the CR text and introduces a second matching rule); define
the start as the first layer whose set is non-empty (rejected: same reason).

## #34 FP-033 - SRS 6.3 paraphrase gaps

**Decision:** Where SRS 6.3 paraphrases the CR, the CR text governs: (i) SRS 6.3 omits the CR
611.2b qualifier "and doesn't begin again during that spell or ability's resolution";
`ForAsLongAsWindow::effect_does_something` implements the CR (a window that ended before the
first application but begins again during the resolution does something). (ii) CR 611.2c: each
part of a resolved group is locked independently (`register_group` resolves every resolved
selector part on its own, all against the registry as it was before the call). (iii) CR 613.6
covers sublayers (7a to 7b to 7c to 7d) and effects from any source, static or resolved; for a
static ability the lock holds only within one evaluation (the record lives in the per-object
evaluation), so its selector is matched again on the next computation (CR 611.3a).
**Rationale:** The brief's CR baseline excerpts and the researcher notes; the SRS is a
paraphrase. No rule number differs between SRS and CR, so nothing goes to HUMAN_QUEUE.
**Alternatives:** Follow the SRS wording for 611.2b (rejected: CR governs); lock a static group's
set across evaluations (rejected: contradicts 611.3a).

## #34 FP-033 - Lock-in API shape

**Decision:** A multi-layer effect is registered with `ContinuousEffects::register_group(env,
parts)`, one `ContinuousEffect` per layer or sublayer, all or nothing. A part with
`Affected::SameAsLayer(l)` links to its anchor: the non-linked part of the same group whose layer
is `l`, the lowest `EffectId` if several; `l` must be strictly earlier in `Layer::ALL` order,
otherwise `LayerError::BadLink(part)`. Links are stored in the registry (part to anchor);
removing an anchor drops the links to it, so its parts then apply to nothing. Resolved selector
parts are locked to `Affected::Objects` only by group registration (`resolve_affected_set`:
every object of `env.objects()` evaluated fully, ascending `ObjectId`); a resolved `SameAsLayer`
part keeps `SameAsLayer` and follows its (locked) anchor. Plain `register` is unchanged: it never
locks a selector and still rejects `SameAsLayer` with `Unsupported`, because a link needs a
group. Parts are validated in ascending `EffectId` order, so neither results nor the reported
error depend on the order the caller lists the parts in. A linked part does not consult
`generating_ability_exists` (613.6: "even if the ability generating the effect is removed during
this process"); the dependency trial uses the same membership rule as the real application.
`register_group_for_as_long_as` validates and locks exactly as `register_group`, then registers
nothing and returns `Ok(false)` when the window makes the effect do nothing (CR 611.2b).
**Rationale:** The shape fixed by the brief so that the Independent Tester can code against it;
additive to the existing API; keeps one source of truth for membership (`step_applies`) and no
state outside one evaluation (R3).
**Alternatives:** A group id on each effect, with the anchor found at evaluation time (rejected:
larger API change, and the anchor would be re-derived on every evaluation); locking resolved
selectors in plain `register` (rejected: changes existing behaviour and tests); letting linked
parts check the generating ability (rejected: contradicts 613.6).

## #34 FP-033 - Rules-modifying parts of resolved effects not locked here

**Decision:** CR 611.2c also says that a part of a resolved effect that does not modify
characteristics or control "modifies the rules of the game" and can affect objects that were not
affected when it began. Such parts belong to the passes after the layers (613.10, 613.11) and to
REQ-LAYER-004; this unit locks only characteristic- and control-changing parts (every
`LayerModification` is one) and does nothing for rules-modifying parts.
**Rationale:** The layers module models only layer modifications; the passes after the layers are
a later unit.
**Alternatives:** Model rules-modifying parts now (rejected: out of scope, no data model yet).

## #34 FP-033 - Links follow only the first-layer anchor

**Decision:** Refines (does not replace) "#34 FP-033 - Lock-in API shape": its phrase "the
non-linked part of the same group whose layer is `l`" is narrowed to "`l` must be the earliest
layer among the group's non-linked parts". Let FIRST be the earliest layer, in `Layer::ALL`
order, among the parts of a group whose affected set is not `SameAsLayer`. A part with
`Affected::SameAsLayer(l)` is valid only if `l == FIRST` and `l` is strictly earlier than the
part's own layer; its anchor is the non-linked part of layer FIRST with the lowest `EffectId`.
Otherwise `register_group` returns `LayerError::BadLink(part)` and the registry is unchanged.
Parts with a selector or an object set (not `SameAsLayer`) stay independent at any layer,
including between FIRST and a linked part: they are never linked and keep their own set (CR
611.2c). This is an INFERENCE from CR 613.6 ("If an effect starts to apply in one layer ... it
will continue to be applied to the same set of objects"), SRS 6.3 ("the affected set chosen in
the first layer where it applies") and "#34 FP-033 - 'Starts to apply' and the empty first-layer
set". PR #188 CodeRabbit comment 4638613308.
**Rationale:** A link to a mid-chain independent part would start a separate chain, letting a
later layer choose a set the effect's first layer did not choose.
**Alternatives:** Represent several explicit chains per group (rejected: a larger model change,
and it contradicts the single-set reading of 613.6); leave the check as it was, only "strictly
earlier" (rejected: a link could follow a set the effect's first layer did not choose).

## #34 FP-033 - Duplicate ids are reported before layer mismatches

**Decision:** `register_group` sorts the parts by ascending `EffectId`, then pass 1 returns
`DuplicateEffect(id)` for the first id already registered or repeated in the group; only then
pass 2 returns `LayerMismatch` for the first part whose modification is outside its layer. A
group with a duplicate id is always reported as `DuplicateEffect`, whatever else is wrong with it.
PR #188 CodeRabbit comment 4638613299.
**Rationale:** The reported error must not depend on the order the caller lists the parts in
(REQ-CORE-001, R3); with one combined pass and a stable sort, two parts sharing an id gave
`DuplicateEffect` or `LayerMismatch` depending on caller order.
**Alternatives:** Report the lowest-id error of either kind (rejected: still depends on how a tie
between a duplicate and a mismatch on the same id is broken).

## #35 FP-034 - Maximum hand size is a pass B rule

**Decision:** The maximum hand size is computed only in pass B, from `MAX_HAND_SIZE` rule
entries (`RuleSet::max_hand_size`); it is never a pass A player effect.
**Rationale:** CR 613.11 names "modify a player's maximum hand size" as its example of an effect
that modifies the game rules; the DSL has `RuleKind::MaxHandSize`; SRS §14.4.3 lists `MAX_HAND_SIZE`
among the rule kinds. SRS 6.4's mention of "maximum hand size when it is phrased as a player
effect" in pass A is covered by the HUMAN_QUEUE wording fix already on this branch.
**Alternatives:** A player property in pass A (rejected: contradicts the 613.11 example and the
DSL); both passes (rejected: two sources of truth).

## #35 FP-034 - Dependency hook in the passes after the layers

**Decision:** Both passes order effects by ascending (timestamp, `EffectId`) with an optional
dependency hook `PostLayerEnvironment::depends_on(a, b)` ("a depends on b"), consulted only
between effects of the same pass. Repeatedly, among the unapplied effects that depend on no
unapplied effect, the earliest key applies; when none is free, the earliest-key member of a
dependency loop with no way out (a strongly connected component of the unapplied effects that
depends on nothing outside itself) applies, and the relation is looked at again after each
application. Self-dependence is ignored. Effects whose `effect_exists` is false are dropped first.
**Rationale:** CR 613.8a defines dependency within a layer, but CR 613.10 and 613.11 both say
"See also the rules for timestamp order and dependency (rules 613.7 and 613.8)" and SRS 6.4 asks
for "timestamp order with dependency". The semantics mirror the layer ordering of "#32 FP-031"
(613.8b: wait until just after all dependencies; simultaneous dependents in timestamp order; a
loop in timestamp order). Choosing only among closed loops keeps an effect that depends on a loop
waiting until after the loop member it depends on. This is a modelling choice.
**Alternatives:** Pure timestamp order (rejected: drops the 613.8 reference); applying the
earliest remaining effect when none is free (rejected: an effect outside a loop that depends on a
loop member could apply before it).

## #35 FP-034 - "Can't lose" and "can't win" are pass B rules

**Decision:** "Can't lose the game" and "can't win the game" effects are `WIN_LOSE_RULE` rule
entries in pass B; no DSL change.
**Rationale:** The DSL already makes `WIN_LOSE_RULE` a `RuleKind`; such effects modify the game
rules (CR 613.11).
**Alternatives:** Player properties in pass A (rejected: would need a DSL change and duplicates
the rule kind).

## #35 FP-034 - Resolved effects after the layers and CR 611.2c

**Decision:** A rule effect (pass B) is never locked: its selector is matched against the players
each time the rule set is built, whatever its origin (CR 611.2c: an effect that modifies the rules
"can affect objects that weren't affected when that continuous effect began"). A player effect
(pass A) of `EffectOrigin::Resolved` applies to the player set fixed at registration:
`register_player` rejects a resolved effect with a `PlayerAffected::Selector` with
`LayerError::Unsupported`, so the caller resolves the set and passes `PlayerAffected::Fixed`.
This closes "#34 FP-033 - Rules-modifying parts of resolved effects not locked here".
**Rationale:** Keeps the CR 611.2c split between characteristic/player changes (fixed set) and
rules changes (not locked) without the passes needing the game state at registration.
**Alternatives:** Locking in `register_player` through the environment (rejected: the registry
would need an environment; the caller already knows the players at resolution); locking rule
effects too (rejected: contradicts 611.2c).

## #35 FP-034 - RuleModification parameter conventions

**Decision:** `RuleModification.params` is opaque in the DSL; pass B reads:
- `"action"` (string) for `PROHIBITION`, `CAST_PERMISSION`, `PLAY_PERMISSION` and
  `TIMING_PERMISSION`, compared by exact string equality; an entry with no string `"action"`
  applies to no action. `RuleSet::query(player, action, permission)` returns `Prohibited` for the
  earliest applicable prohibition with that action (CR 101.2: prohibitions are evaluated last,
  whatever the timestamps), else `Permitted` for the earliest applicable entry of the given
  permission kind with that action, else `NoEffect`. Land plays use the action `"PLAY_LAND"`.
- `ADDITIONAL_LAND_PLAY`: `"count"`, absent = 1, a non-negative JSON integer = that number, any
  other value (negative, non-integer, wrong type) = 0. `land_plays_allowed` = 0 under an applicable
  `"PLAY_LAND"` prohibition (CR 305.2b, 101.2), else 1 + the sum (CR 305.2), saturating.
- `MAX_HAND_SIZE`: exactly one of `"set": n` (integer), `"delta": n` (integer) or
  `"unlimited": true`, applied in application order from 7 (CR 402.2): set replaces, delta adds
  (saturating; nothing when there is no maximum), unlimited removes the maximum until a later set.
  An entry with none of the keys, more than one of them, a value of the wrong type, or
  `"unlimited"` not `true` is ignored. Only the final value is clamped to a minimum of 0, so
  `set -2` then `delta +3` gives 1.
**Rationale:** Fixed by the brief so that the implementation and the Independent Tester agree;
ignoring malformed entries (rather than panicking or guessing) keeps untrusted data untrusted
(R12); clamping only the result follows the brief's "Result clamps at a minimum of 0".
**Alternatives:** Typed DSL parameters per kind (rejected: a contract change, `fineprint-dsl` is
not owned here); clamping after every step (rejected: the brief clamps the result); letting the
first of several keys win (rejected: depends on key precedence the DSL does not define).

## #35 FP-034 - Pass A returns applied effects, not typed properties

**Decision:** `compute_player_properties` returns, per player, the applied player effects
(`AppliedPlayerEffect { id, modification }`) in application order. Typed derived properties
(protection, hexproof and others) are derived by later consumers.
**Rationale:** The DSL `Modification` of a `PlayerEffect` is generic (`op` plus opaque params);
the engine has no closed set of player properties yet.
**Alternatives:** A typed `PlayerProperties` struct now (rejected: would invent a vocabulary the
DSL does not define).

## #35 FP-034 - Cost stages ordering details

**Decision:** `RuleSet::cost_stages(player)` lists only entries applying to `player`: `grants` are
the `ALT_COST_GRANT` and `ADDITIONAL_COST_REQUIRE` entries merged in application order (CR 601.2b);
`increases` the `COST_INCREASE` entries; `reductions` the `COST_REDUCTION` entries in application
order (the default the player may change with `reorder_reductions`, CR 601.2f); `total_effects`
all `COST_MINIMUM` entries, then all `COST_SET` entries, each kind in application order.
`reorder_reductions` accepts exactly a permutation of the current ids and otherwise returns
`LayerError::Unsupported` leaving the list unchanged. Rule entries keep every player of their
resolved set, including an empty set; `RuleSet::entries` lists them all. Players in a fixed set
that the environment does not list are dropped in both passes.
**Rationale:** Matches the brief's contract; nothing is computed here, the casting procedure uses
the stages.
**Alternatives:** Grants grouped by kind (rejected: loses the application order the brief asks
for).

## #35 FP-034 - An effect that depends on a dependency loop waits for it

**Decision:** In passes A and B, when no remaining effect is free, the earliest-key member of a
dependency loop with no way out (a strongly connected group that depends on nothing outside
itself) is applied next. An effect that depends on a loop member but is not in the loop still
waits until that member has been applied. Example: e2 and e3 depend on each other and e1
depends on e2, so the order is e2, e1, e3. This corrects the Implementer and Independent Tester
briefs, which said "apply the earliest-key remaining effect".

**Rationale:** CR 613.8b: "An effect dependent on one or more other effects waits to apply until
just after all of those effects have been applied. ... If several dependent effects form a
dependency loop, then this rule is ignored and the effects in the dependency loop are applied in
timestamp order." Only effects in the loop drop the waiting rule. D2 ("Dependency hook in the
passes after the layers") also asks for the same semantics as the layer ordering, and
`layers/ordering.rs` already resolves loops this way. The two readings agree whenever every
remaining effect is itself in a loop, which was the brief's stated precondition.

**Alternatives:** Apply the earliest-key remaining effect even if it is outside the loop (the
brief's literal wording). Rejected because an effect would then apply before an effect it
depends on, which contradicts 613.8b.

## #36 FP-035 — Coarse invalidation and the (stateVersion, ObjectId) key

**Decision:** `LayerCache` stores finished `Evaluated` values keyed by `(StateVersion, ObjectId)`.
Every invalidation, whatever its `Invalidation` cause, bumps the version by exactly 1 and drops
every entry. The cause is recorded for readability and counting only; it never narrows what is
dropped. Registry changes are detected by the cache itself: it keeps a clone of the
`ContinuousEffects` it last evaluated against and, when the registry passed in compares unequal,
invalidates with `EffectRegistration` before evaluating (never on the first call). Errors are
returned, counted as misses and never stored. `evaluate_all` compares the registry once per call
and keeps the entries computed before a first error.
**Rationale:** SRS 6.5 asks for the `(stateVersion, ObjectId)` key and invalidation on anything that
can affect an effect's existence, selector or output. One object's evaluation can depend on other
objects (selector matching, CDA value expressions, the dependency trial), and #186 FP-146 makes
that dependence stronger, so dropping only the changed object's entry is not sound in general.
A full drop is always sound and costs one recompute of the board. Comparing the registry snapshot
costs `O(effects)` per call and makes registry soundness independent of which instance the caller
passes.
**Alternatives:** Per-object or per-cause invalidation (rejected: unsound under cross-object
dependence, and #186 would have to undo it). A registry generation counter inside
`ContinuousEffects` (rejected: `registry.rs` is outside this unit and a counter does not catch a
replaced registry instance). Storing errors (rejected: an error is not a characteristic value and
the brief forbids it).

## #36 FP-035 — Scope against #183 FP-144 and #186 FP-146

**Decision:** The cache is standalone, like the rest of `layers`. It owns its own `StateVersion`
counter (starting at 0), detects registry changes by snapshot, and takes every environment change
(zone, control, counters, expiry, timestamps, designations, attachment, status, player values) as
an explicit `invalidate(cause)` from the caller. Nothing is wired into `state/`, `GameState` or
`state_hash`; `evaluate`, `evaluate_all`, `ContinuousEffects` and `LayerEnvironment` are unchanged.
**Rationale:** `GameState` has no state version yet and the layers module is not wired into it;
that wiring, timestamps in the state and the layer state in the Merkle hash are #183 FP-144, which
will call `invalidate` from the real mutation points and may extend the differential to
`GameState::state_hash` and golden replays. #186 FP-146 changes how objects are evaluated relative
to each other; coarse invalidation stays sound under it.
**Alternatives:** Deriving the version from a game-state counter now (rejected: none exists and
wiring is #183's scope).

## #36 FP-035 — Differential design

**Decision:** The differential compares digests of `evaluate_all` results, not `state_hash`.
`evaluated_digest` is SHA-256 over a canonical encoding (tag `fineprint.layers.evaluated.v1`, entry
count, then per object in ascending `ObjectId` order the id, the characteristics in the layout of
"#24 FP-023 — Characteristics encoding", and the controller). The encoding of the characteristics
is a private copy in `layers/cache.rs` because the LKI encoder in `memory/lki.rs` is private and
outside this unit. CI job `differential (ubuntu-latest)` runs (1) the whole workspace suite with
`--features fineprint-core/full-recompute`, (2) and (3) the Independent Tester's
`spec_layers_cache_diff` in both modes with `FINEPRINT_DIFF_DIR` set, each scenario writing
`<scenario>.digest` (lowercase hex plus newline), and (4) a bash comparison that requires at least
5 digest files, identical file names and byte-identical files. `FINEPRINT_DIFF_DIR` is read only
by that test code, never by rules code (R3). In CI the variable is set to
`${{ github.workspace }}/target/differential/<mode>` (absolute), because cargo runs a test binary
with its package directory as the working directory; the brief's relative
`target/differential/<mode>` would land in `crates/fineprint-core/target/...` and the comparison
step, which runs at the workspace root, would find nothing.
**Rationale:** The layers are not in `GameState` yet (see the scope entry), so `state_hash` cannot
see them; the digest of the evaluated board is the closest observable equivalent. The whole suite
also runs in full-recompute mode because SRS 6.5 requires the full regression suite in both modes
(the cached mode is the existing `test` job). Digest files make a disagreement visible per
scenario.
**Alternatives:** Comparing in one process with both modes (impossible: the mode is a compile-time
feature). Extending `state_hash` (rejected: #183). A relative path with `cd` in the comparison step
(rejected: an absolute path is clearer and robust).
**Follow-up:** the state-hash comparison that #36's deliverable names is #193 FP-148 (depends on #183 FP-144); #36's issue text records this scope.

## #36 FP-035 — Stale-cache demonstration plan

**Decision:** No sabotage switch ships in the code. The acceptance criterion "a deliberately stale
cache in a scratch commit fails it" is demonstrated by the Orchestrator on a scratch branch. The
scratch mutation made `invalidate` a no-op (the version is not bumped and the entries are not
dropped, so lookups keep hitting the old entries under the same version) and switched off
registry-change detection; the spec tests' comparator check (cached results against
`layers::evaluate_all`) and the digest comparison then fail. The unit test
`environment_changes_need_a_declared_invalidation` also shows, in cached mode, that an undeclared
change keeps serving the old version's value. On draft PR #192 (scratch commit 4e74362, closed
unmerged) `differential (ubuntu-latest)` failed (3 cache unit tests failed in the full-recompute
suite) and `test (ubuntu-latest)` failed (6 failed); run locally, 26 of the 51
`spec_layers_cache_*` tests failed, including `stale_cache_is_detected_by_the_differential` and
`diff_after_declared_changes`.
**Rationale:** A feature or flag that makes the cache stale would be dead weight and a risk in
production code; the scratch commit proves the gate without shipping it.
**Alternatives:** A `stale-cache` test feature (rejected for the reason above).

## #36 FP-035 — upload-artifact pin

**Decision:** The `bench` job uploads `target/criterion` with `actions/upload-artifact@v4.6.2`
(first-party, exact tag, verified with `git ls-remote --tags` because `gh api` had no repository
access in the session: tag `v4.6.2` is commit `ea165f8d65b6e75b540449e92b4886f43607fa02`), artifact
`criterion-layers-recompute-<sha>`, `if-no-files-found: error`, 90 days retention. The ci.yml
header now lists it with checkout and cache.
**Rationale:** REQ-NFR-009 asks for benchmark results tracked as a trend; the artifact is the data
the phase gate compares. Exact first-party tags match the existing pinning policy.
**Alternatives:** Committing results to the repository (rejected: noise in history); a third-party
benchmark action (rejected: not first-party).

## #36 FP-035 — criterion pin and manifest edits

**Decision:** `fineprint-core` gets the dev-dependency `criterion = { version = "=0.8.2",
default-features = false }` (newest release on crates.io at the time of writing, 2026-02-04; its
`rust-version` is 1.86, below the pinned 1.97.0), a `[[bench]] name = "layers_recompute",
harness = false`, and `[features] full-recompute = []` (not default). The `cargo_bench_support`
feature is not enabled: in 0.8.2 it is empty and unused. `Cargo.lock` gains only the packages the
dev-dependency pulls in (32 entries: criterion, criterion-plot, clap, regex, ciborium, half,
itertools, tinytemplate, walkdir and their dependencies, including a second `syn` 2.0.119 next to
the existing 3.x, which makes two existing references read `syn 3.0.10`); no existing version
changed. Dev-dependencies never reach the rules crates' runtime.
**Rationale:** Criterion is the standard harness with median estimates and `estimates.json` output
the CI summary reads; default features would add plotters and rayon (threads), not needed.
**Alternatives:** criterion 0.5.1 (rejected: older, no `rust-version`); a hand-written timing loop
(rejected: no statistics, reinvents the harness); `divan` (rejected: no `estimates.json` contract
for the summary step).

## #36 FP-035 — Stale docs/DEVELOPMENT.md lines left for the Orchestrator

**Decision:** `docs/DEVELOPMENT.md` is outside this unit's owned paths, so its "Lands later" lines
for the differential and the benchmark, and its CI table (which lists the required jobs), are left
unchanged and reported to the Orchestrator for a follow-up edit: the new jobs are
`differential (ubuntu-latest)` and `bench (ubuntu-latest)` (not required), the commands are
`cargo test --workspace --locked --features fineprint-core/full-recompute` and
`cargo bench --locked -p fineprint-core --bench layers_recompute -- --noplot`.
**Rationale:** R1 (owned paths); the brief says to report, not edit.
**Alternatives:** Editing DEVELOPMENT.md here (rejected: outside owned paths).

## #37 FP-036 — Placeholder bridge

**Decision:** `Characteristics.rules_text` is still the placeholder `card::TextTokenStream` (a list
of JSON values). Layer 3 converts it to the real `fineprint_dsl::text::TextTokenStream` through
MessagePack (`rmp_serde::to_vec` of the placeholder, `rmp_serde::from_slice` into the real stream,
which runs the FP-011 validation; the reverse for encoding): `layers::decode_rules_text`,
`layers::encode_rules_text`. A `REPLACE_TEXT_WORD` writes the text back only when a word changed,
so unchanged text stays byte-identical. Text that is empty or does not decode (for example a
placeholder element holding a plain string) makes the effect a no-op; evaluation never fails or
panics on it. The shim is removed when #159 (FP-140) types `rules_text`.
**Rationale:** fineprint-core has no `serde_json` dependency and the brief forbids adding one;
`rmp_serde` is already a dependency and both types are serde-compatible. A no-op on undecodable
text keeps evaluation total (R12) while the placeholder can still hold test values.
**Alternatives:** Adding `serde_json` to fineprint-core (rejected: Cargo edits forbidden); an error
from evaluation on undecodable text (rejected: the brief makes it a no-op, and existing tests use
placeholder text); typing `rules_text` here (rejected: #159's work).

## #37 FP-036 — Word replacement scope (CR 612)

**Decision:** `LayerModification::ReplaceTextWord{category, from, to}` with category `CARD_NAME` is
rejected at registration (`LayerError::Unsupported`) and `rewrite_text_words` returns
`UnsupportedCategory` for it: CR 612.2 says a colour-word or subtype change can't change a card
name, and the CR defines no word-for-word replacement of card names (name changes are 612.6-612.9,
out of scope). `COLOR_WORD`, `BASIC_LAND_TYPE` and `CREATURE_TYPE` are accepted (612.2; a
creature-type word that also names a token is a `CREATURE_TYPE` token, 612.2a). `NUMBER` is
accepted as a design choice: no CR rule covers it, and only a card's own text could ask for it
(612.1 "any words or symbols"). There is no category-compatibility check between `from` and `to`,
and `from == to` is an ordinary replacement that changes nothing: the CR imposes neither. Name
references (`SELF` and `OTHER`) are never rewritten (201.5, 612.2), nor are literal tokens or
`CARD_NAME` words, whatever the effect's category. Abilities granted by effects are not text and are
never rewritten: layer 3 never touches `Characteristics.abilities` (612.3). A copy does not keep text
changes (707.2, 707.2b); copy is layer 1 and belongs to #38.
**Rationale:** The rules researcher's conclusions for #37 against CR 2026-11-06; the CR wins over
the SRS (P1).
**Alternatives:** Accepting `CARD_NAME` as SRS §14.5 allows (rejected: no CR basis, and 612.2
protects names); rejecting `NUMBER` (rejected: SRS §14.5 lists it and nothing in the CR forbids
it); requiring `from` and `to` to belong to the same vocabulary (rejected: the CR has no such rule
and the layers module has no vocabulary).

## #37 FP-036 — SRS §14.5 versus the CR

**Decision:** Where SRS §14.5 (REQ-DSL-012) and CR 2026-11-06 differ, this unit follows the CR and
leaves the SRS unedited (a person edits it; HUMAN_QUEUE P8 line):
(1) §14.5 lists `CARD_NAME` as a rewritable category and lets `REPLACE_TEXT_WORD` take any
category; CR 612.2 forbids colour and subtype replacements from changing card names (612.2a's
exception is creature-type words that also name tokens), the CR defines no word replacement of
card-name words (name changes are 612.6-612.9), and no CR rule exists for `NUMBER`.
(2) §14.5 cites "CR 201.5 family" for self-references; 201.5 is about name references ("this
creature" and "~" are Oracle conventions; 201.5a/201.5b resolution of names in gained abilities is
outside layer 3); `NameRef{OTHER}` is not addressed by the SRS (612.2 protects it).
(3) §14.5 omits CR 612.3 (granted abilities are not rewritten), CR 613.8 dependency between
text-changing effects, CR 612.4 (tokens' text can be changed) and CR 707.2 (text changes are not
copied).
**Rationale:** P1: the CR wins; SRS edits are a person's job (P8).
**Alternatives:** Implementing the SRS as written (rejected: contradicts CR 612.2).

## #37 FP-036 — Dependency between text effects

**Decision:** Chained text-changing effects use the generic dependency trial (`ordering.rs`,
CR 613.8a-c) plus one comparison specific to `REPLACE_TEXT_WORD`: when B is a word replacement
with `from != to` and both A and B apply, B depends on A exactly when applying A changes the
tokens B applies to (the word tokens of B's category and `from` value, by token position and id,
CR 612.2), because B does the same thing (rewrite to `to`) to each of them (CR 613.8a (b)); a
change A makes to any other token of the same `rules_text` creates no dependency. Every other
case of the trial is unchanged (B does not apply in either state: independent; B's existence or
affected set changes: dependent; A changes nothing: independent; the CDA / non-CDA exclusion;
every B that is not a word replacement, `SetText` included, and a no-op `from == to`
replacement keep the field-by-field comparison). On text with a Swamp word and no Island word,
A = Swamp->Island and B = Island->Forest give Forest in both timestamp orders (B depends on A);
A = Swamp->Island and B = Swamp->Forest depend on each other (a loop, applied in timestamp order,
613.8b), so the earlier timestamp's `to` wins. Former gap G6 is fixed in PR #196 (review round 1,
CodeRabbit finding 4640489029): the trial no longer compares the whole `rules_text` field for a
`REPLACE_TEXT_WORD`, so on mixed Swamp+Island text A = Swamp->Island, B = Island->Forest gives
Forest for both words in both orders (B depends on A, A independent of B), and A = Swamp->Island
with B = Island->Swamp is a real mutual dependency (each changes the other's matching tokens)
applied in timestamp order (CR 613.8b): A earlier gives [Swamp, Swamp], B earlier gives
[Island, Island]. The Island/Swamp swap result is an engine derivation of 613.8a/b, not a CR
worked example. `SetText` and `REPLACE_TEXT_WORD` on the same object: the replacement depends on
the `SetText` under CR 613.8a (b) (applying the `SetText` changes the tokens the replacement
applies to) and the `SetText` does not depend on the replacement, so per CR 613.8b the `SetText`
applies first in both timestamp orders and the replacement rewrites the new text (derivation by
the rules researcher, CR 2026-11-06; this supersedes the brief's original expectation that a
later `SetText` wins unchanged). Unit test `set_text_applies_before_replace_in_both_orders`
still passes under the per-token comparison. Review round 2 (CodeRabbit finding 4640922156):
the per-token comparison applies only when A is also a `REPLACE_TEXT_WORD`; for a whole-text
`SetText` A the trial keeps the field-by-field comparison of B's output. Under the round-1 rule
a `SetText` that keeps B's matched word at the same position with the same id (base
[Swamp w, "old"], `SetText` to [Swamp w, "new"], B = Swamp->Island) left B's token set
unchanged, so neither effect depended on the other; with B earlier the `SetText` then overwrote
B's result and gave [Swamp, "new"]. Now B depends on the `SetText` and both timestamp orders
give [Island, "new"], `SetText` first (tests
`set_text_keeping_matched_word_applies_before_replace_in_both_orders`,
`set_text_keeping_matched_word_and_replace_both_orders`). Check of other A kinds: the only
layer-3 modifications are `SetText` and `REPLACE_TEXT_WORD` (`LayerModification::belongs_to`
in registry.rs; `apply_effect` in pipeline.rs writes `rules_text` in layer 3 only for these
two); `CopyValues` and `FaceDown` also change `rules_text` but are layer 1 and are never
compared with a layer-3 B (CR 613.8a (a)); counters are not text. A `REPLACE_TEXT_WORD` A cannot
keep B's token set and still change B's outcome: it decodes, rewrites only the `value` of
matching word tokens and re-encodes (ids, order and kinds kept, REQ-DSL-012), and B's outcome
depends only on which tokens match B's category and `from` (each is rewritten to `to`), so its
effect on B is fully captured by B's token set. No counterexample found.
**Rationale:** CR 613.8a (b) asks whether applying A changes what B applies to or what it does
to them; for a word replacement that is exactly its set of matching tokens. The original brief
kept ordering.rs out of scope, so the first submission used the generic trial alone; the
per-token comparison was adopted in review round 1 (it closes follow-up #194, FP-149). It reads
only token data (REQ-DSL-013) and is a pure function of the two scratch states (REQ-CORE-001).
**Alternatives:** The generic whole-field trial for word replacements too (rejected in review
round 1: on mixed Swamp+Island text it reports a mutual dependency and gives [Island, Forest]);
the per-token comparison was adopted for `REPLACE_TEXT_WORD` only, and the generic trial remains
for every other modification. The per-token comparison for every A, `SetText` included
(rejected in review round 2: a whole-text `SetText` that keeps the matched word's position and
id was missed, so the `SetText` overwrote the replacement).

## #37 FP-036 — Engine gaps for #37

**Decision:** Recorded, not absorbed:
G1 typed `rules_text` and typed abilities are #159 (FP-140) and #161 (FP-141); until then the
placeholder bridge applies.
G2 cross-object source view is #186 (FP-146) and wiring is #144; `MatchContext` gives only the
candidate's view, so the resolution helpers (`resolve_word_ref`, `text_token_matches`,
`resolve_predicate_words`, `resolve_selector_words`) take the source's evaluated table from the
caller.
G3 the compile step from the DSL `REPLACE_TEXT_WORD` effect to `LayerModification::ReplaceTextWord`
(DECISIONS "#34 FP-033" item 8) is follow-up #195 (FP-150).
G4 word references reachable only from a value expression outside predicates and selectors are not
resolved by these helpers (also #195).
G5 copy and text changes (CR 707.2) belong to #38.
G6 whole-field dependency comparison of `rules_text` (entry above): fixed in PR #196 review round 1
(follow-up #194 is closed by that PR; no longer a gap).
**Rationale:** Each gap is owned by another issue or needs one; the brief scopes this unit to the
layers module.
**Alternatives:** Implementing them here (rejected: outside owned paths and scope).

## #38 FP-037 tester — Independent Tester judgement calls

**Decision:** (1) `CopyException::SetManaCost` is tested on the mana cost only; the spec tests do
not assert whether the copy's colors follow the new cost, because the brief pins only the
replaced field. (2) `NotCopied(ColorIndicator)` keeps only the copier's color indicator and copies
the colors; `NotCopied(Colors)` keeps both (brief: "that field takes `own`'s value; NotCopied(Colors)
also takes own.color_indicator"). (3) A gained ability (707.9a) is asserted to be appended after
the copied abilities, as the brief says "abilities gets it appended". (4) The face-down input of
the copy tests is a layer-1b `FaceDown` effect the tests register themselves (a colorless 2/2
creature with no name, cost, subtypes, text or abilities); no face-down transition is tested
(#39). (5) The spec tests reuse the layer-test environment by including
`tests/spec_layers_support.rs` read-only with `#[path]` (as `spec_layouts_composite_support.rs`
does for the layout helpers) and build their own synthetic definitions; no existing test file is
edited.
**Rationale:** Each choice asserts exactly what the brief pins and no more (R10), and follows the
#23 precedent for tester entries.
**Alternatives:** Assert that `SetManaCost` recomputes colors from the cost (not specified;
CR 707.9b says only the modified characteristic changes); copy the support code (duplicates
several hundred lines without changing what is tested).

## #38 FP-037 — Copy of a merged or mutated permanent

**Decision:** The copy gets the merged permanent's copiable values as produced by layer 1a,
i.e. the topmost component's characteristics PLUS all abilities of every component. 702.140e is
the "otherwise specified by the effect that caused them to merge" of 731.2a, so it is part of
the 1a merge effect and hence of the copiable values. `CopiableValues::merged` and
`merge_effect` therefore carry `MergedPermanent::base_characteristics` (top component, plus
every component's abilities top to bottom when mutated), and `copiable_characteristics` of the
merged object returns the same (test `copy_of_mutated_permanent`).
**Rationale:** CR 731.2a says the merge (including its specifications) is a copiable effect;
CR 613.2c makes layer-1 results the copiable values. Orchestrator decision (R8), recorded here
by the Implementer. Judgement call: the CR is silent.
**Alternatives:** Top component only (the rules-researcher's tentative inference: 702.140e is not
stated as a layer-1 effect); rejected by the Orchestrator for the reason above.

## #38 FP-037 — Copiable values API and the `ValueSource` design

**Decision:** `CopiableValues { characteristics, source }` with `ValueSource::{Plain,
Fixed { mana_value }, CopyOfBackFaceOrMelded}` as pinned by the brief, plus one private flag
(`zero_when_copied`) that tells a `Fixed` from a nonmodal back face or a melded permanent (its
copy has mana value 0, CR 202.3b, 202.3c) from a `Fixed` merged permanent whose top is an
ordinary object (its copy is `Plain`). The flag is `false` for every other source; `modified`
keeps it. A merged permanent's value is `Fixed` only when the top component's mana value differs
from the characteristics' own off the stack; it is flagged when the top component is a nonmodal
DFC with its back face up or copying a back face (its copy has mana value 0, CR 202.3b). A copy
of a back face or melded permanent has mana value 0 even when an exception sets a mana cost
(CR 202.3b "the mana value of the copy is 0"). A `Fixed` value is the stored number in every
context (the front cost is taken with X = 0, CR 202.3e).
`copiable_characteristics` runs layers 1a and 1b with the evaluator's own per-layer procedure
(`run_layer`, same ordering, lock-in and generating-ability checks), not `evaluate` through an
environment adapter as the brief's guidance suggested: `evaluate` always starts layer 7, which
writes power and toughness back as evaluated literals and removes them from noncreatures
(CR 208.3), so a CDA expression would have been copied as a number. No existing file changed.
`merge_effect` has no source (`None`): the merge is not an ability of the object.
**Rationale:** The pinned API must stay as given; the flag is the minimum extra state that keeps
the copy rule for merged values distinguishable. Running only layer 1 keeps the copiable values
exactly as CR 613.2c defines them (the characteristics after layer 1, P/T as printed or set).
**Alternatives:** An extra `ValueSource` variant (rejected: the enum is pinned); `evaluate` with
a counter-free adapter (rejected: CDA P/T flattened to literals, noncreature P/T dropped);
a merged value always `Fixed` (rejected: the pinned rule is `Plain` when equal).

## #38 FP-037 — SetManaCost and colors

**Decision:** `CopyException::SetManaCost` changes the mana cost; the colors are re-derived with
`base_colors(new cost, color indicator, CdaColors::None)` only if the running colors equalled
`base_colors(old cost, color indicator, CdaColors::None)`; otherwise they are left as they are.
**Rationale:** Colors come from the cost (CR 105.2, 202.2) unless something else set them; when
the copied colors are not the cost's derivation (a set color, a CDA), changing the cost must not
overwrite them. As directed by the brief.
**Alternatives:** Never touch colors (rejected: a copy "except its mana cost is ..." would keep
stale cost colors); always re-derive (rejected: would erase set colors).

## #38 FP-037 — CDA abilities and CR 707.9d

**Decision:** A characteristic-defining ability is recognisable in the placeholder ability JSON:
a `"STATIC"` ability whose `effects` contain a `"CHAR"` effect with `"isCDA": true`
(fineprint_dsl `CharacteristicEffect.is_cda`). Its `layer` names what it defines: `"4"` types,
`"5"` colors, `"7a"` power and toughness. `copy_with` drops a copied ability that defines a
characteristic the copy effect sets (`SetColors`, `Set*Types`, `SetPower`, `SetToughness`) or
does not copy (`NotCopied` of colors, a type part, power or toughness), before applying the
exceptions, so gained abilities are never dropped. `Add*Types` keep type CDAs (707.9d last
sentence). Any type exception drops any layer-4 CDA (the layer does not say which part of the
type line it defines); a power or toughness exception drops a layer-7a CDA (which defines both).
`modified` (not a copy) drops nothing. Only ability data is read, never card identity (R4).
**Rationale:** CR 707.9d; the marker exists in the DSL, so the brief's "if yes" branch applies.
**Alternatives:** Dropping nothing (rejected: the marker exists); inspecting the modification op
to tell type-line parts apart (rejected: ops are free strings in the placeholder DSL).

## #38 FP-037 — Not implemented in layer 1a

**Decision:** Not implemented: the exceptions of CR 707.9e, 707.9f and 707.9g (replacement-effect
and conditional exceptions); copies of spells (CR 707.10); tokens that copy double-faced objects
(CR 707.8a, 707.10g); the face-down status itself (no `FaceDown` effect is created: a copy is
never face down). Stated in the module documentation of `layers/copy.rs`.
**Rationale:** Out of the brief's scope; each needs machinery (replacement effects, the stack,
token creation, layer 1b semantics) owned by other units.
**Alternatives:** Partial support here (rejected: outside owned paths and scope).

## #38 FP-037 — Face-down merged components deferred to layer 1b (#39)

**Decision:** `merge_effect` and `CopiableValues::merged` use
`MergedPermanent::base_characteristics`, which returns pre-layer-1b values whatever a component's
`face_up` says; face-down merged components are not modelled or tested here and remain with #39
FP-038 (see "#23 FP-022 — Face-down merged components deferred to layer 1b (#39)").
**Rationale:** Layer 1b semantics and turning face up are #39's scope.
**Alternatives:** Modelling them here (rejected: outside scope).

## #38 FP-037 — CodeRabbit round 1: colors pinned by SetColors and NotCopied(Colors)

**Decision:** `apply_exceptions` keeps a `colors_pinned` flag (since round 2 it starts from the
pin of the values it changes and is returned, see "#38 FP-037 — CodeRabbit round 2: explicit
colors carried with the copiable values"). `SetColors(_)` and `NotCopied(Colors)` set it (CR 707.9b, 707.9c, 707.9d); no other
exception changes it. `SetManaCost` re-derives the colors only when the flag is false and the
colors still equal the derivation of the old cost and color indicator. So on a green `{2}{G}`
source `[SetColors(GREEN), SetManaCost({1}{W})]` stays green, `[SetManaCost({1}{W})]` becomes
white, `[SetManaCost({1}{W}), SetColors(GREEN)]` is green and two costs re-derive twice. Since
round 2 the final flag is carried with the copiable values to a later copy or modification
(it was per call in round 1). This refines "#38 FP-037 — SetManaCost and colors".
**Rationale:** The equality guard alone cannot tell colors derived from the cost from colors an
exception set to the same value (CodeRabbit PR #197). The equality stays as the second guard for
colors the source had that were not derived from its cost.
**Alternatives:** Tracking a per-characteristic "derived" bit in `Characteristics` (rejected:
changes a shared type outside the owned paths); never re-deriving colors (rejected: a cost-only
exception must update cost-derived colors).

## #38 FP-037 — CodeRabbit round 1: a back-face top of a merged permanent is always Fixed

**Decision:** `CopiableValues::merged` decides first whether the topmost component is a nonmodal
double-faced card with its back face up or copying a back face. Such a top always gives
`ValueSource::Fixed { mana_value }` with the copy-as-0 classification, even when the merged mana
value equals the characteristics' own (a `{0}` front over a back without a mana cost). Any other
top is `Plain` when the numbers match and `Fixed` (copying as `Plain`) otherwise.
**Rationale:** CR 202.3b: a copy of a nonmodal back face has mana value 0, so the classification
must survive equal numbers; otherwise a copy with a set cost `{5}` had mana value 5.
**Alternatives:** Keeping the early `Plain` return (rejected: loses the classification).

## #38 FP-037 — CodeRabbit round 1: mana-value provenance in layer 1a and Evaluated

**Decision:** `ValueSource` moves to `layers/registry.rs` (re-exported from `copy` and `layers`,
so `fineprint_core::layers::ValueSource` is unchanged). A new public `ManaValueProvenance` holds a
`ValueSource` and the copy-as-0 flag (and, since round 2, the colors pin) in crate-private
fields; it is built only by
`ManaValueProvenance::plain()` (also `Default`) or `CopiableValues::provenance()`, so the flag
stays consistent with the source and `CopiableValues`' own flag stays private. Accessors:
`source()`, `copies_as_zero()`, `is_plain()`, `mana_value(&Characteristics, ctx)`.
`LayerModification::CopyValuesWithProvenance { characteristics: Box<Characteristics>, provenance:
ManaValueProvenance }` is new, layer 1a only, and replaces all characteristics like `CopyValues`,
which is unchanged and means plain provenance. `copiable_effect` builds `CopyValues` for plain
values and the new variant otherwise. `Evaluated` gains `pub mana_value_provenance`, which starts
`evaluate_with_order` and the layer-1 run as the base's provenance
(`LayerEnvironment::base_mana_value_provenance`, plain by default; see "#38 FP-037 — CodeRabbit
round 3: base mana-value provenance from the environment"); `CopyValues` and `FaceDown` reset it to
plain (a face-down object has no mana cost, CR 708.2, 202.3a), the new variant sets it, nothing
else changes it. `Evaluated::mana_value(ctx)` reads it exactly as `CopiableValues::mana_value`.
`copiable_values(effects, env, id)` runs the same layer 1a and 1b as `copiable_characteristics`
(kept, same signature) and returns `CopiableValues::with_provenance(characteristics,
provenance)`; registering `copiable_effect(V)` and reading `copiable_values` gives a value with
`V`'s characteristics and `V`'s exported provenance (`V.provenance()`, colors pin included), not
necessarily one `==` to `V`, when the registered effect applies to the target (a `Static`-origin
effect is skipped while its generating ability does not exist; a `Resolved` one always applies)
and no other applicable effect changes the characteristics or the provenance before the end of
layer 1 (a layer-1b `FaceDown` effect, a later layer-1a copy or modification, or an earlier one
that is not replaced; see "#38 FP-037 — CodeRabbit round 2: round-trip guarantee qualified" and
"#38 FP-037 — CodeRabbit round 3: round-trip guarantee requires an applying effect and
preserves characteristics and provenance").
`evaluated_digest` encodes the provenance (variant tag, the `Fixed` number, the flag; since
round 2 also the colors pin) and its tag
becomes `fineprint.layers.evaluated.v2`; no golden digest constant exists (checked with grep).
A base object that is itself a nonmodal back face or melded permanent starts with the
provenance the environment supplies through `LayerEnvironment::base_mana_value_provenance`
(round 3; the default is plain); wiring that method to the game state stays with #183. This
supersedes "#38 FP-037 — Copiable values API and the `ValueSource` design"
where it says no existing file changed and `CopyValues` is the only layer-1a payload.
**Rationale:** CodeRabbit PR #197: the provenance was lost between `copiable_effect` and a later
copy, so a back-face copy with a set cost came back with mana value 5. Additive: `CopyValues` and
every existing constructor keep their meaning, and spec tests that build `CopyValues` still hold.
**Alternatives:** Changing `CopyValues` to carry the provenance (rejected: breaks existing
callers and spec tests); public fields `source` and `zero_when_copied` on the new variant
(rejected: allows inconsistent combinations and exposes the private flag); a side table keyed by
`EffectId` (rejected: not a value of the effect, so registration order and the cache would need
special handling).

## #38 FP-037 — CodeRabbit round 1: P/T-only layer-1a modification

**Decision:** `LayerModification::ApplyCopyExceptions(Vec<CopyException>)` is new, layer 1a
only. Its apply arm runs `apply_exceptions` on the object's current characteristics with no "own"
values (since round 2 `NotCopied` is ignored, see "#38 FP-037 — CodeRabbit round 2: provenance
export and NotCopied in non-copy modifications"), drops no characteristic-defining ability and
keeps the mana-value source and copy-as-0 flag (round 2: it carries the colors pin). `modification_effect(exceptions, source, controller,
origin, timestamp, objects)` builds it (layer 1a, not a CDA, `Affected::Objects`). It orders by
timestamp like any layer-1a effect: an earlier copy is overridden only in the fields it sets and
removing that copy restores the object's own values while the set power and toughness stay; a
later copy replaces everything. `CopiableValues::modified` and the snapshot use of
`copiable_effect` are unchanged; their documentation points to `modification_effect` as the form
that carries only the fields an "as ... enters" ability sets.
**Rationale:** CodeRabbit PR #197: a snapshot built by `modified` and stored as `CopyValues`
carries the copied name, cost and abilities, which then survive the removal of the copy effect.
CR 613.2a, 707.2.
**Alternatives:** A dedicated P/T-only variant (rejected: other "as ... enters" abilities may set
more characteristics, and the exception vocabulary already exists); changing `modified` to return
a delta (rejected: breaks its tested values-level behaviour).

## #38 FP-037 — CodeRabbit round 1: layer-4 CDA parts deferred

**Decision:** The current policy stays: any type-line exception drops any layer-4
characteristic-defining ability. The reason is stated on `CopyException::cda_target`: the
placeholder ability data does not say which part of the type line a layer-4 CDA defines; per-part
targets are tracked in #198 (FP-151).
**Rationale:** The layer-4 CHAR modification is `{"op": <free string>, ...}` with no op
vocabulary, so the defined part cannot be read without inventing names (R4, R8).
**Alternatives:** Reading op names now (rejected: would invent a vocabulary #198 owns).

## #38 FP-037 — CodeRabbit round 2: explicit colors carried with the copiable values

**Decision:** `ManaValueProvenance` gains a crate-private `colors_pinned: bool` (false in
`plain()`) with the public accessor `colors_pinned()`; its docs now say it records how the mana
value and the colors were derived (the type is not renamed). `CopiableValues` gains the same
private flag (false in `plain`, `nonmodal_back_face`, `melded`, `merged`) and carries it through
`provenance()` and `with_provenance`. `apply_exceptions(c, own, exceptions, colors_pinned) ->
bool` starts from the pin of the values it changes and returns the final pin: `SetColors(_)` and,
in a copy, `NotCopied(Colors)` set it; nothing clears it; `SetManaCost` re-derives the colors only
when the pin is false and the colors equal the derivation of the old cost and indicator.
`copy_with` and `modified` give the source's pin OR the exceptions' final pin. A pinned
provenance is not plain, so `copiable_effect` builds `CopyValuesWithProvenance` for it and the
pin survives `copiable_effect` -> `copiable_values` and `Evaluated::mana_value_provenance`. The
evaluator's `ApplyCopyExceptions` arm starts from the evaluated provenance's pin and stores the
final pin back (source and copy-as-0 flag unchanged); `CopyValues` and `FaceDown` reset the
provenance to plain, pin included. `evaluated_digest` encodes the pin after the copy-as-0 flag;
the tag stays `fineprint.layers.evaluated.v2` because v2 first appears in this PR and no earlier
v2 encoding exists on `main`, so no persisted digest can disagree. On a green `{2}{G}` source,
`copy_with(own, [SetColors(GREEN)])` then `copy_with(own, [SetManaCost({1}{W})])` is green with
cost `{1}{W}`; without the first exception it is white. Limit: a base object's own colors and
values built by the layout constructors start unpinned, because nothing in a printed object says
its colors were set by a copy exception; the pin only arises from a copy or modification.
**Rationale:** CodeRabbit PR #197 (round 2): a per-call flag lost the explicit-versus-derived
distinction between copies, so a copy of a copy with explicit green and a white cost turned
white, while CR 707.9b makes the set colors part of the copy's copiable values. Carrying the pin
in the provenance reuses the round-1 path that already crosses `copiable_effect`, the evaluator,
the cache and the digest; all existing values and spec equalities without an explicit-colors
exception are unchanged.
**Alternatives:** A per-characteristic "derived" bit in `Characteristics` (rejected: a shared
type outside the owned paths); a separate `ColorProvenance` type on `Evaluated` (rejected: one
more public field and payload for one bit); bumping the digest tag to v3 (rejected: v2 is not on
`main` yet).

## #38 FP-037 — CodeRabbit round 2: provenance export and NotCopied in non-copy modifications

**Decision:** (1) `CopiableValues::provenance()` exports the copy-as-0 flag only when `source` is
`ValueSource::Fixed { .. }`, and `false` for any other source. Back-face values whose public
`source` is reassigned to `Plain` export a plain provenance, digest like plain values and copy as
`Plain`; reassigned to `CopyOfBackFaceOrMelded` they still copy as 0 (that source does so
itself). Genuine back-face and melded values keep the flag; a `Fixed` value without it is
unchanged. `CopiableValues`' `==` still compares the raw private flag (equality is not
normalised), so such a reassigned value is not `==` to `CopiableValues::plain` of the same
characteristics although its provenance is. (2) `apply_exceptions` takes `own:
Option<&Characteristics>`: `Some(own)` in `copy_with` (unchanged), `None` in
`CopiableValues::modified` and in the evaluator's `ApplyCopyExceptions` arm, where
`NotCopied(_)` is skipped completely (no field change, and `NotCopied(Colors)` sets no pin). On a
3-power object `[SetPower(5), NotCopied(Power)]` and `[NotCopied(Power), SetPower(5)]` both give
5; `[NotCopied(Colors), SetManaCost(..)]` behaves as the cost alone. The needless clone in the
evaluator arm is gone.
**Rationale:** CodeRabbit PR #197 (round 2). (1) `source` is a public field, so the export must
keep the provenance consistent with it; the mana-value calculation already ignored the flag for
non-`Fixed` sources. (2) A modification that is not a copy has no original values to keep
(CR 707.9c concerns copy effects); using the current characteristics as "own" made `NotCopied`
undo an earlier exception of the same effect. Round 1 already decided that `NotCopied` changes
nothing in a non-copy modification.
**Alternatives:** (1) Making `source` private with a setter (rejected: breaks the public field
the spec tests read); normalising `==` too (rejected: changes existing equality behaviour).
(2) Rejecting `NotCopied` in `modification_effect` (rejected: an error path for a harmless
no-op, and `modified` returns no `Result`).

## #38 FP-037 — CodeRabbit round 2: round-trip guarantee qualified

**Decision:** The round trip "registering `copiable_effect(V)` and reading `copiable_values`
gives `V`'s characteristics and exported provenance (`V.provenance()`) back" (round 3: not a value
`==` to `V`, because `CopiableValues`' `==` compares the raw private flag and `source`) holds only
when the registered effect applies to the target (round 3: a `Static`-origin effect is skipped
while its generating ability does not exist; a `Resolved` one always applies) and no other
applicable effect changes the characteristics or the provenance before the end of layer 1: a
layer-1b `FaceDown` effect (which replaces the characteristics and resets the provenance to
plain), a later layer-1a copy or modification, or an earlier one that is not replaced. The
`copiable_values` doc comment and the round-1 entry
"#38 FP-037 — CodeRabbit round 1: mana-value provenance in layer 1a and Evaluated" say so. A
unit test shows that with a `FaceDown` effect the read gives the face-down values and plain
provenance, and without it gives `V` (both timestamp orders).
**Rationale:** CodeRabbit PR #197 (round 2): the unqualified sentence was false whenever another
layer-1 effect applies (CR 613.2b, 613.2c).
**Alternatives:** Making `copiable_values` ignore other effects (rejected: it must report the
actual copiable values, CR 613.2c).

## #38 FP-037 — CodeRabbit round 2: as-enters P/T settings are Resolved-origin effects

**Decision:** An "as ... enters" or "as ... is turned face up" power and toughness setting
(CR 208.2b; replacement effects per CR 614.1c and 614.1e) is registered as a layer-1a effect with
`EffectOrigin::Resolved`, built by `modification_effect` (field-only: it writes only the fields it
sets). `copiable_effect(CopiableValues::modified(..))` is not the route for it: it stores a full
snapshot, so one built while an earlier copy is active keeps that copy's name, cost and
abilities after the copy expires; `Resolved` keeps a setting active but does not limit the
fields it writes (round 3, "#38 FP-037 — CodeRabbit round 3: as-enters P/T settings use
modification_effect"). Once applied it keeps applying after the permanent loses the ability
that generated it: the evaluator consults `generating_ability_exists` only for `Static` origins.
The docs of `modification_effect`, `copiable_effect` and `CopiableValues::modified` say so, and a
unit test shows a `Resolved` setting whose ability is gone still applies (alone, over an earlier
copy and after that copy is removed) while the same effect with a `Static` origin is dropped.
Existing tests keep their origins.
**Rationale:** Rules-researcher answer (CR baseline 2026-11-06): the characteristics chosen or
determined this way affect the creature's copiable values (CR 208.2b, 707.2, 613.2a); a
continuous effect from a resolution lasts as stated (CR 611.2a), unlike a static-ability effect
that applies only while its generator exists (CR 611.3b). The review-body finding on
`copy_tests.rs` noted that no test checked that the source ability can be lost.
**Alternatives:** A dedicated origin variant for as-enters effects (rejected: `Resolved` already
has the needed lifetime); keeping `Static` and having the caller re-register (rejected: the
evaluator would drop the setting when the ability is lost).

## #38 FP-037 — CodeRabbit round 3: round-trip guarantee requires an applying effect and preserves characteristics and provenance

**Decision:** The round trip is stated (in the `copiable_values` doc comment, the `layers` module
docs, "#38 FP-037 — CodeRabbit round 1: mana-value provenance in layer 1a and Evaluated" and
"#38 FP-037 — CodeRabbit round 2: round-trip guarantee qualified") as: registering
`copiable_effect(V)` on an object and reading `copiable_values` gives a value with `V`'s
characteristics and `V`'s exported provenance (`V.provenance()`, colors pin included) when
(1) the registered effect applies to the object (a `Static`-origin effect is skipped while
`LayerEnvironment::generating_ability_exists` is false; a `Resolved` one always applies) and
(2) nothing changes the characteristics or the provenance before the end of layer 1. It does not
promise `CopiableValues` `==`: `==` still compares the raw private copy-as-0 flag and `source`, so
a back-face value whose public `source` was reassigned to `Plain` reads back with equal
characteristics and equal `provenance()` but is not `==` to the original. `==` is unchanged.
Unit tests: `round3_round_trip_requires_applying_effect` (a `Static` copy with its ability absent
gives the base, or the earlier copy; `Resolved`, or the ability present, gives `V`; both EffectId
orders) and `round3_round_trip_preserves_characteristics_and_provenance`.
**Rationale:** CodeRabbit PR #197 round 3 (threads r4644520230 and r4644520239): the earlier
sentences promised `V` itself and omitted the applying condition, both false in the cases above
(CR 613.2c, 707.2).
**Alternatives:** Normalising `==` (rejected: round 2 kept raw equality on purpose and spec
tests compare values); making `copiable_values` report the raw flag of the effect (rejected: the
effect carries only the exported provenance, by design of round 1).

## #38 FP-037 — CodeRabbit round 3: as-enters P/T settings use modification_effect

**Decision:** An "as ... enters" or "as ... is turned face up" power and toughness setting is
registered with `modification_effect(exceptions, .., EffectOrigin::Resolved, ..)` only. The docs
of `modification_effect`, `copiable_effect` and `CopiableValues::modified` and the `layers` module
docs recommend it and no longer offer `copiable_effect(CopiableValues::modified(..))` for this
workflow; the round-2 entry "#38 FP-037 — CodeRabbit round 2: as-enters P/T settings are
Resolved-origin effects" is edited to say so. `modified` stays as the values-level operation.
Unit test: `round3_as_enters_pt_with_modification_effect_after_copy_expires` (an earlier copy C
with another name, cost and an extra ability, a later or earlier `Resolved` setting built while C
is active, C removed: the setting's P/T stays with the object's own name, cost and abilities;
both timestamp and EffectId orders; the snapshot route keeps C's name and abilities, shown as the
contrast).
**Rationale:** CodeRabbit PR #197 round 3 (thread r4644520252): a full snapshot built while a
copy is active keeps the copied characteristics after the copy expires; `Resolved` fixes the
lifetime, not the fields written. `modification_effect` writes only the fields it sets
(CR 613.2c, 707.2).
**Alternatives:** Keeping both routes documented (rejected: the snapshot route is wrong whenever
a copy is active); removing `CopiableValues::modified` (rejected: public API and tested
values-level behaviour).

## #38 FP-037 — CodeRabbit round 3: base mana-value provenance from the environment

**Decision:** `LayerEnvironment` gains the defaulted method
`fn base_mana_value_provenance(&self, id: ObjectId) -> ManaValueProvenance` (default
`ManaValueProvenance::plain()`): the provenance of the object's base mana value before layer 1
(CR 613.1). An environment returns `CopiableValues::nonmodal_back_face(..).provenance()` for the
back face of a nonmodal double-faced object and `CopiableValues::melded(..)?.provenance()` for a
melded permanent (CR 202.3b, 202.3c, 713.5c). `evaluate_with_order` and the layer-1 run behind
`copiable_values` and `copiable_characteristics` start `Evaluated::mana_value_provenance` from it
(called only after `base(id)` returned `Some`), instead of `plain()`. Layers 1a and 1b are
unchanged: `CopyValues` and `FaceDown` reset it to plain (CR 708.2, 202.3a),
`CopyValuesWithProvenance` sets it, `ApplyCopyExceptions` keeps it. A change to the value is a
change to the object's base and is declared to a `LayerCache` like any other base change
(`Invalidation::ZoneChange`, whose doc says so). The digest already encodes the provenance; its
tag stays `fineprint.layers.evaluated.v2` (v2 is not on `main` yet) and the encoding uses no
hash-ordered container or floating point. Every existing `impl LayerEnvironment` compiles
unchanged and behaves as before. Wiring the method to the game state stays with #183. Tests:
`round3_base_provenance_evaluate_back_face`, `round3_base_provenance_copiable_values_back_face`,
`round3_base_provenance_melded`, `round3_base_provenance_replaced_or_kept_in_layer_one`,
`round3_property_base_provenance`, the extended `property_copiable_values_round_trip`, and
`base_mana_value_provenance_cached_and_fresh_agree` (cache_tests.rs).
**Rationale:** CodeRabbit PR #197 round 3 (review-body finding on pipeline.rs, verified): with a
costless back face whose front costs `{3}{R}` as the base, `evaluate` gave mana value 0 instead
of 4 and a copy with `SetManaCost({5})` of the `copiable_values` read gave 5 instead of 0,
because neither evaluator could receive the layout's distinction. It is a defect in this issue's
deliverable, so it is fixed here. A defaulted trait method is additive: no struct literal or
existing implementation changes.
**Alternatives:** A new `ObjectBase` field (rejected: `ObjectBase` is a public struct with about
14 struct literals in the crate, tests and benches, so adding a field breaks them); a separate
public `evaluate_with_provenance` entry point (rejected: two evaluators would drift and callers
would have to remember to use it); leaving it to #183 (rejected: it is a defect in this issue's
deliverable); changing `provider.rs` or wiring `GameState` now (rejected: not needed; #183
supplies the real environment).

## #39 FP-038 — Ward {2} ability shape of the face-down profiles

**Decision:** `FaceDownProfile::with_ward_2()` (disguise, cloak; CR 702.168a, 701.58a) is the
default 2/2 profile plus exactly one ability, the JSON object `{"kind": "KEYWORD", "abilityId":
"facedown:ward", "keyword": "ward", "cost": "{2}"}` (all string values). `has_ward_2()` is true
iff the abilities contain that exact value, so a ward keyword with another cost is not ward {2}.
**Rationale:** The brief pins this shape; it is the keyword shape of
`layers::keyword_counter_ability` plus a cost member, so a later keyword macro for ward can read
it the same way. Ward is not a keyword counter (SRS 6.1), hence its own `abilityId` prefix.
**Alternatives:** A dedicated `Characteristics` field or a native-effect reference (rejected:
the profile must be ordinary characteristics so it is the copiable values, CR 708.2); a numeric
cost member (rejected: the AbilityAST placeholder carries costs as text).

## #39 FP-038 — Turn-face-down refusal API and check order

**Decision:** `layers::turn_face_down` takes a `FaceDownTarget { already_face_down,
double_faced_permanent, merged_contains_double_faced }` supplied by the caller and refuses, in
this order, `AlreadyFaceDown` (CR 708.2b), `DoubleFacedPermanent` (CR 712.13),
`MergedContainsDoubleFaced` (CR 731.2j). A refusal changes nothing (no effect, no timestamp).
Otherwise it stamps the object with `StampReason::TurnedFaceUpOrDown` on clones of the ledger
and generator, builds `face_down_effect` with that timestamp, registers it (atomic in the
registry) and commits the clones only after registration succeeded; a stamping or registration
error is `FaceDownRefusal::Layer` with nothing changed. `layers::turn_face_up` checks
`NotFaceDown` first (unknown id, not a `FaceDown` modification, or an affected set that is not a
locked set containing the object), then `InstantOrSorcery` (CR 701.40g, 701.58g, 731.2g), then
stamps and removes the effect, all or nothing. If a `FaceDown` effect's locked set holds other
objects too, only the named object is taken out of the set (same id, same timestamp); the
others stay face down. `face_down_effect` itself refuses nothing: a double-faced card may enter
or be cast face down (CR 712.12).
**Rationale:** The layer module does not know a permanent's card or status (it is standalone,
#32), so the caller states the facts and the function applies the CR's "nothing happens". The
order is the brief's. Checking `NotFaceDown` before `InstantOrSorcery` means a permanent that is
not face down is never "revealed and left face down". Narrowing a shared set keeps the other
objects' face-down status, which removing the whole effect would end; the builder only ever
creates single-object sets, so this case arises only from hand-built effects.
**Alternatives:** Reading the facts from `Zones`/`MergedPermanent` (rejected: couples layers to
zones and layouts; the integration unit wires it); refusing a multi-object `FaceDown` effect as
`NotFaceDown` (rejected: the object is face down by it, so the refusal would be wrong).

## #39 FP-038 — Face-down merged components resolved (#23 open question)

**Decision:** Resolves the open question of "#23 FP-022 — Face-down merged components deferred
to layer 1b (#39)" for the merge effect as built: a merged permanent has only the topmost
component's characteristics (CR 731.2a, `merge_effect` in layer 1a); its face-down status is
the topmost component's (CR 731.2e) and is applied by one `face_down_effect` on the merged object
in layer 1b, which wins over the merge in either timestamp order; turning it face up removes
that effect and shows the topmost component's characteristics again (CR 731.2f for the whole
permanent, 708.8). A face-down lower component of a non-mutated merged permanent contributes
nothing. `layouts/merged.rs` is unchanged. Tests:
`req_obj_003_merged_permanent_face_down_both_orders`, `req_obj_003_merged_refusals`.
**Rationale:** CR 731.2a gives the merged permanent only the topmost component's
characteristics; CR 731.2e makes the topmost component decide the status; CR 731.2f turns all
components together, so one effect on the one object models it. Whether a face-down lower
component of a *mutated* permanent contributes its abilities (CR 702.140e against CR 708.2) is a
rules question reported to the Orchestrator for the rules-researcher; today `merge_effect` with
`mutated = true` includes every component's abilities whatever its `face_up` flag, and changing
that would touch `layouts/merged.rs` or `layers/copy.rs`, outside this unit's owned paths.
**Alternatives:** Per-component face-down effects (rejected: components are not objects with
their own characteristics, CR 731.2a); deciding the mutated case here (rejected: a rules
question, which agents do not decide).
**Mutated case (rules-researcher answer, CR 2026-11-06):** a face-down lower component of a
mutated permanent whose topmost component is face up **does** contribute its abilities, so the
current `merge_effect` (`mutated = true`) behaviour stands and no code changes. Basis: CR
702.140e ("A mutated permanent has all abilities of each card and token that represents it")
is the "unless otherwise specified" exception of CR 731.2a; CR 731.2e makes the merged
permanent's status that of its topmost component, so with a face-up top the permanent is face
up and CR 708.2 / layer 1b (613.2b) do not apply to it. If the topmost component is face down,
the whole permanent is face down and layer 1b leaves only the profile (CR 708.2), with no
component's abilities. This is an inference from those explicit rules: no CR rule or ruling in
the baseline states the case directly.

## #39 FP-038 — LKI of a face-down permanent leaving the battlefield

**Decision:** A face-down permanent's last known information is its face-down characteristics:
the caller evaluates (for example through `LayeredProvider`) before it removes the face-down
effect for the zone change, so the look-back sees the profile, not the card. Test:
`req_obj_003_lki_before_leaving_is_the_profile`.
**Rationale:** CR 603.10 looks back at "the appearance of objects immediately prior to the
event"; immediately prior, the permanent is face down and has only the listed characteristics
(CR 708.2). The 708.9 reveal on leaving tells players the card's identity; it does not change
what the permanent looked like on the battlefield. This is an inference from those two rules,
not a quoted ruling.
**Alternatives:** Looking back at the revealed card's characteristics (rejected: that is the
appearance after the event, not before it).

## #39 FP-038 — Both directions of turning stamp the object

**Decision:** `turn_face_down` and `turn_face_up` each give the object a new timestamp from the
caller's generator with `StampReason::TurnedFaceUpOrDown`, and return it; the face-down effect's
own timestamp is the one issued when turning face down. A refused call issues nothing.
**Rationale:** CR 613.7f: "A permanent receives a new timestamp each time it turns face up or
face down." The generator is monotonic, so the face-up timestamp is later than every effect
stamped before it, a copy registered while face down included.
**Alternatives:** Stamping only when turning face up (rejected: CR 613.7f names both).

## #39 FP-038 — Scope of the reveal rule and the profile module

**Decision:** `layouts::must_reveal_on_leave(face_down, from, to)` is true iff `face_down`,
`from != to`, and either `from` is the battlefield or `from` is the stack and `to` is not the
battlefield (CR 708.9). It is a predicate only; the reveals on a player leaving and at game end,
and the looks of CR 708.5, stay in `knowledge::Knowledge` and are not reimplemented. The profile
lives in `layouts/facedown.rs` (data, no layer effect) and the layer-1b builder and the turning
plumbing in `layers/facedown.rs`, as the brief's owned paths split them.
**Rationale:** The brief pins the predicate's truth table; a same-zone move is not leaving the
zone. Keeping profiles in `layouts` follows that module's role (characteristics data before the
layers) while the effect builders live with the other layer builders.
**Alternatives:** Wiring the reveal into `Knowledge::on_move` now (rejected: outside owned paths;
`Knowledge::on_move` already applies the CR 708.9 reveals to moves).

## #39 FP-038 — Turning one object of a shared face-down effect up narrows the anchor in place

**Decision.** Add `ContinuousEffects::narrow_objects` and use it in `turn_face_up` when other objects remain; links to the anchor are kept. An effect whose set becomes empty is still removed with `remove` (nothing remains to apply to, so its links are dropped).

**Rationale.** CR 708.8 requires the object to turn face up while the others stay face down, so refusing is wrong; CR 613.6 requires the parts of a multi-layer effect to keep applying to the objects the anchor applied to. `remove` deletes every `SameAsLayer` link to the anchor and `register` cannot restore them, so remove plus register lost the linked parts for the remaining objects. The new method is additive, a few lines, and leaves evaluation untouched.

**Alternatives.** Refusing turn-up when links exist (violates CR 708.8); dropping the narrowed-set path (regresses the documented behaviour that the other objects stay face down); re-registering the whole group from facedown.rs (`register_group` needs a `LayerEnvironment`, which would change the `turn_face_up` signature).

## #40 FP-039 — ETB projection API and modelling choices

**Decision.** The ETB look-ahead of SRS 8.1 (REQ-EVENT-001, CR 614.12) is a standalone module,
`crates/fineprint-core/src/replacement/etb.rs`, with the API fixed by the brief (`EtbEffect`,
`EtbEffects`, `EtbContext`, `EtbEntry`, `EtbApplied`, `ProjectedPermanent`, `project`,
`applicable`, `resolve_entering`, `EtbResolved::record_into`; since PR #202 review round 1 also
`EtbApplied::order`, `EtbApplied::sequence`, `CopyAbilities`, `EtbEffects::set_copy_abilities`
and `EtbError::NotACopyEffect`). `EtbApplied::order` is the application order (CR 616.1f),
first applied first, filled by `resolve_entering`; `effects` stays the set that stops an effect
applying twice. `sequence()` is `order` (skipping ids not in `effects` and repeated ids), then
every id of `effects` not in `order`, ascending: with an empty `order` it is ascending id.
Choices made in it:

1. **An ETB effect model of its own.** `EtbEffect` carries a scope (`ThisPermanent` /
   `OtherPermanents`), a condition on the projected permanent (`HasCardType`, `LacksCardType`,
   `HasSubtype`) and an action (`EntersTapped`, `EntersWithCounters`, `EntersAsCopyOf`,
   `AddCardTypes`, `Choose`). The bus `ReplacementEffect` cannot carry it: its `EventMatcher`
   tests the event against the current state, and its tier is declarative data ("#29 FP-028 —
   Tiers are declarative data"), whereas CR 614.12 needs conditions on the characteristics the
   permanent would have on the battlefield. The tier of an `EtbEffect` is likewise declarative.
2. **Standalone.** Nothing is wired into `process_batch` or `GameState` ("#29 FP-028 —
   Standalone replacement registry", "#32 FP-031 — Follow-up work for later units"); the bus's
   `Modification::EntersAsCopyOf` is untouched. Wiring the projection into `process_batch`,
   compiling DSL ETB replacements (SRS 14.4.4 `CHOOSE ... store`) to `EtbEffect` and applying
   the result (tapped status, counters, copy, Memory) to the entered permanent are later work.
3. **Order of the three CR 614.12 inputs** (the CR and the SRS give none; the SRS a/b/c is not
   an order): (a) applied effects are replayed in `EtbApplied::sequence()` order and the LAST
   applied copy effect replaces the base characteristics (CR 707.2, CR 616.1f); the object is
   always presented as on the battlefield (CR 208.3 P/T presence follows); (b)+(c) the entry's
   own statics (or the last copy's declared statics, item 4) are registered on a clone of the
   continuous effects in force and the layers are evaluated once over both, so their relative
   order is the layer system's (CR 613); (a) applied `AddCardTypes` are one synthetic layer-4
   `AddTypes` effect (CR 613.1d, CR 205.1b) registered on the same clone, with an id strictly
   above every registered id and a timestamp strictly above every registered timestamp (checked
   arithmetic, `LayerError::IdExhausted` on overflow; CR 613.7), source and locked affected set
   (`Affected::Objects({entrant})`) the entrant, origin `EffectOrigin::Resolved`; it is
   invisible to the caller. It realises a replacement effect, not a static ability of the
   entrant, so it is not a generating-ability effect: the layer pipeline asks
   `LayerEnvironment::generating_ability_exists` only for `EffectOrigin::Static` effects with a
   source, and so never consults the hook for it (an environment answering false for every id
   still sees the added type; CR 613.1d). A static origin with no source would also skip the
   hook; `Resolved` with the entrant as source was chosen because it keeps the source and the
   locked set the entrant's and states that the effect exists independently of any ability. No rule classifies an additive
   "enters as an additional type" effect or fixes its timestamp: the reading chosen is a
   replacement effect (CR 614.1c) whose type change is a layer-4 continuous effect later than
   every other, so P/T (CR 208.3, CR 208.3a) and layers 5-7 see the added type; it is not
   copiable (CR 707.2). The earlier "unioned in after the layers" is withdrawn (a printed-P/T
   noncreature lost its P/T). Tapped status and counters are collected from the applied
   effects. The base controller and the
   counters are the environment's (the projection overrides only `on_battlefield`, the copied
   characteristics and, for a copy, a plain mana-value provenance); the counters an entry
   enters with are reported in `ProjectedPermanent::counters`, not fed to layer 7c.
4. **Copy rule.** The LAST applied copy supplies the base (CR 707.2, CR 616.1f). Once an
   `EntersAsCopyOf` is applied, the entry's `own_statics` are not registered and
   `ThisPermanent` effects whose source is the entering object no longer become applicable (an
   already applied one stays applied). The copy model's abilities replace them through its
   declared `CopyAbilities` (CR 707.5): its `statics` are registered bound to the entrant
   (`source` the entrant; `controller` the entrant's `EtbEntry::controller`, so a
   controller-relative selector such as "artifacts you control" reads the entrant's controller
   through `MatchContext::effect_controller`, CR 707.5: the copied abilities are the entrant's;
   an `Affected::Objects` set becomes `{entrant}`, a selector is kept) and its `etb` ids apply
   to the entrant as `ThisPermanent` effects of the entrant. One controller per projection
   (CR 614.12, CR 707.5): `project` (so `applicable` and `resolve_entering`, which reports it as
   a procedure error before any request) refuses an entry whose environment base reports
   `controller: Some(c)` with `c != EtbEntry::controller`, with
   `EtbError::ControllerMismatch { object, entry, environment }`, before any layer is evaluated
   or copied static registered (an unknown object stays `LayerError::UnknownObject`); a base with
   `controller: None` is accepted and the entry's controller is then the only one (copied
   statics and the added-types effect use it). Otherwise a copied "artifacts you control" static
   bound to the entry's controller would not match its own entrant evaluated under the
   environment's controller. Only the
   last applied copy's declaration counts, never an earlier copy's (in the spirit of CR 707.9e:
   a later copy effect overrides what an earlier one gave). Without a declaration a copy brings
   no statics and no ETB effects. Every `ThisPermanent` effect also needs
   `LayerEnvironment::generating_ability_exists(effect.id, entry.object, Some(projected))` to be
   true (CR 614.12, CR 603.6d: the "enters with" ability is a static ability of the permanent as
   it would exist); the ETB effect id shares the static-effect id space for this query (default
   true). `OtherPermanents` effects of other sources are unaffected.
5. **Payable totals (CR 614.12b; the CR does not define "payable").** Each `EtbOption::cost`
   is committed to the entry's controller. For a chooser present in `payable`, `remaining`
   starts at its value. An option is allowed iff its cost is 0, or `cost + reserve <=
   remaining` in u128 (no saturation), where `reserve` comes from an EXACT lookahead (CR 614.12b,
   CR 616.2). Effects never apply twice and no condition reads `chosen`, so an entry's state is
   its applied set plus its last applied copy id (which fixes the projection). `mincost(state)`
   is 0 if nothing applies, else the minimum over the candidates c of the top applicable tier
   (CR 616.1a-e) of `cheapest(c) + mincost(state + c)` (`cheapest` = the cheapest option of a
   `Choose`, 0 for other actions): it counts effects that become applicable (CR 616.2) and not
   those a mandatory replacement removes. It is memoised per entry on (applied set, last copy)
   with at most 4096 states (`MAX_LOOKAHEAD_STATES`; exceeding it is a procedure error, never a
   silent truncation, R3). Every distinct state, the starting state first, is charged when it
   is ENTERED (before anything below it is explored), including a state whose value is 0 at
   once because no unapplied effect can commit a cost (that shortcut still skips the
   projection); a memo hit is free. So an entry whose reachable states number exactly 4096
   passes and 4097 fail, for a deep chain as for a wide search. The procedure error text is
   "the CR 614.12b lookahead of object N exceeds 4096 states". The search is iterative (an
   explicit stack of frames, each holding its state, its top-tier candidates, the next one to
   explore and the least total so far), so a 4096-deep chain cannot overflow the call stack.
   Within `resolve_entering`, the reserve lookahead of each entry's start and the lookahead of
   the entry's own resolution are separate searches, each with its own count. The reserve of option o of `Choose` X in state S is
   `mincost(S + X)` plus the sum of `mincost(empty)` of every LATER entry of the same
   controller (entries are independent; computed before the first entry is resolved). Earlier
   entries have already deducted their costs. Choosing subtracts the option's cost. No allowed
   option is `EtbError::Unpayable` (reported as a procedure error whose text contains "not
   payable"). A chooser absent from `payable` has no limit and runs no lookahead. The CR 616.1
   order choice (`CHOOSE_REPLACEMENT_EFFECT`, CR 616.1e) is never filtered by payability: CR
   614.12b restricts only the choices the effects require. Every candidate is offered (never filtered by payability or by completion), but the
   request's `default_on_timeout` is `SelectReplacement { index: k }` with k the smallest
   candidate index (ascending effect id) whose continuation (a) COMPLETES: some sequence of
   further top-tier applications re-projects without error until nothing applies, and (b) is
   payable under the same exact lookahead, `cheapest(c) + mincost(state + c) + later <=
   remaining` (u128); so a timeout never picks an unpayable or failing order when another
   exists; k = 0 when no candidate is both. A chooser without a budget runs only the completion
   check (payability is unlimited). (issue #204 FP-153.) The completion search is a separate,
   memoised (applied set, last copy), iterative search with its own count bounded by
   `MAX_LOOKAHEAD_STATES`; over that bound it raises no error and every candidate stays eligible
   (best effort, R3). Design choice: the default avoids a failing continuation; declared
   copy-static ids are NOT validated up front, because `EtbEffects::add` and
   `set_copy_abilities` do not know `ctx.continuous`, and an early check in `resolve_entering`
   would reject entries that have a completing order and would still miss collisions deeper in
   a chain. The zero-cost shortcut of the cost lookahead (no projection when no unapplied
   effect has cost > 0) stays, for reservation only; in the cost lookahead a state whose
   projection fails is an infeasible branch (skipped in the minimum, an infeasible root reserves
   0), never a procedure error, so a colliding candidate beside a costed `Choose` does not abort
   the entry before the request; the failure surfaces only when that candidate is answered.
   Completion, not the shortcut, decides eligibility. A manual answer of an
   unpayable candidate is still the "not payable" error. Applying the reserve across the
   SEVERAL effects of one entry goes beyond the CR's literal text (which speaks of multiple
   permanents); it is kept on purpose, as the original brief required (requirement 7). A
   `Choose` with an empty `options` list is refused by `EtbEffects::add` with
   `EtbError::EmptyChoice(id)` (the set unchanged): it has no feasible continuation, and
   counting it as cost 0 would let an earlier choice spend the budget before the impossible
   choice fails (CR 614.12b). A `Choose` with at least one option, a single cost-0 option
   included, is accepted and always payable. A `Choose` whose options repeat a text is refused
   by `add` with `EtbError::DuplicateOptionText { effect, text }` (the set unchanged; checked
   after the empty-options check): the answer is `Text(x)`, which cannot tell two options of one
   text (and two costs) apart, so the cheaper one could never be chosen (CR 614.12a,
   CR 614.12b). Distinct texts, of equal costs or not, stay accepted. `set_copy_abilities` is
   unaffected.
   Before any reserve is computed and before any request, `resolve_entering` projects EVERY
   entry from its empty `EtbApplied`, whether or not its chooser has a budget, and fails with
   the first projection error (an unknown object, a duplicate own-static id, ...) as a
   procedure error: all choices are made before anything enters (CR 614.12a), so no request is
   issued for an earlier entry of a call that cannot complete. The reserve's zero-cost shortcut
   is kept.
6. **Chosen values are visible to later steps.** The CR does not say that a later choice or a
   later effect reads an earlier chosen value. The reading chosen: every chosen value is put in
   `EtbApplied::chosen` as soon as it is made, so every later projection (and so a later
   effect's applicability and action) carries it, and a later `Choose` with the same key
   overwrites it ("the last chosen value", CR 607.2d). No current action or condition reads
   `chosen`; the projection carries it for the integration unit.
7. **Requests.** Replacement order uses `CHOOSE_REPLACEMENT_EFFECT` exactly as `Bus::choose`
   (reason "CR 616.1", source the entering object, event `ZONE_CHANGE`); choices use
   `CHOOSE_OPTION_OR_VALUE` with `option_type` `COLOR` / `CREATURE_TYPE` / `ANCHOR_WORD`,
   citing CR 614.12a, or CR 614.12c for anchor words only ("#24 FP-023 — SRS precision notes").
   One allowed option is taken without asking.
8. **Memory.** `EtbResolved::memory` holds each chosen value through `Memory::set_choice`;
   `record_into` writes the values under the keys of `projected.applied.chosen` (Memory has no
   choice iterator) with `set_choice` for the new object (CR 400.7), keeping its other keys.

**Rationale.** The API is fixed by the brief; the choices above are the readings most
consistent with CR 614.12, CR 616.1f, SRS 8.1 and the earlier decisions cited, and keep the
module a pure function of borrowed inputs and recorded answers (R3, "#28 FP-027").

**Alternatives.** Extending the bus `ReplacementEffect` with projected conditions (rejected:
changes the bus's declarative data and its matcher semantics, outside this unit); feeding
`EntersWithCounters` into the layer evaluation (rejected: not in the brief's projection
definition, which is what the spec tests check); a full combinatorial payability search over
all orders of application (rejected: unbounded and not required by the CR text; the one-step
lookahead was chosen first and replaced by the exact memoised lookahead in PR #202 review,
because it missed newly applicable choices and counted removed ones); unioning added types
after the layers (withdrawn in PR #202 review: P/T and later layers must see the type);
setting the projected controller to `EtbEntry::controller` (rejected: the brief defines the
projection from the environment's base, and no `ControlEtb` action exists yet; a mismatch
between the two is refused instead, item 4, PR #202 review round 3 H2); a dedicated
`ChoiceError` variant for the lookahead-limit error (declined in PR #202 review round 3: it
needs a change in `choice/`, the FP-027 choice-protocol contract, R1/R2; the procedure error
keeps the text "the CR 614.12b lookahead of object N exceeds 4096 states").

Amended in PR #202 review round 1 (F1-F8).

Amended in PR #202 review round 2 (G1-G5): copied statics take the entrant's controller (G1);
the synthetic type effect is `Resolved` and never consults the generating-ability hook (G2);
empty `Choose` options are refused at `add` (G3); lookahead states are charged when entered,
memo hits free, iterative search (G4); every starting projection is validated before any
reserve or request (G5).

Amended in PR #202 review round 3 (H1-H3): duplicate option texts refused at `add` (H1, item 5);
a controller mismatch between the environment base and the entry refused, `None` accepted (H2,
item 4); the `CHOOSE_REPLACEMENT_EFFECT` default is the first candidate with a payable
continuation, else 0, every candidate still offered (H3, item 5).

## #41 FP-040 — The SbaEnvironment seam

**Decision.** Facts the engine state does not hold yet come from a caller-supplied
`sba::SbaEnvironment`: whether an object is a copy of a spell or of a card (CR 704.5e), the
CR 723.3c exemption, the set of objects dealt damage by a deathtouch source since the last check
(CR 704.5h) and the game variant (CR 704.5c). Every method has a default (standard game, no
copies, no exemption), except the deathtouch pair `deathtouch_damaged` / `clear_deathtouch_damaged`,
which has no default and is required together: an environment that reports an object MUST
forget it in the clear, which `perform_all` calls after every successful pass (never on
`Pending` or an error); an environment that does not track deathtouch returns an empty set and
does nothing (or uses `StoredSbaEnvironment`, empty by default). Toughness, abilities, card types and loyalty come only from the caller's
`ContinuousEffects` and `LayerEnvironment` (CR 613.1, CR 613.4, CR 306.5c), never from
`GameState`. `StoredSbaEnvironment` is a plain stored implementation. Battlefield objects the
layer environment cannot evaluate (any `LayerError`, typically `UnknownObject`) are skipped by
the object entries, because the pinned `check` returns no error.

The copy, exemption and deathtouch answers are keyed by `ObjectId` and therefore last ONE zone
residency (CR 400.7: a moved object is a new object). Within one pass `perform_all` follows the
moves of its own batch (see `Zones::cease_to_exist and the owned-path extension`). A caller
whose state changes zones outside `perform_all`, or whose object survives a move made during a
pass (for example a card copy moved onto the battlefield by a replacement effect), must re-key
its environment to the new id before the next check; `SbaOutcome.ceased` and the
`Outcome::ZoneChange` (old -> new) entries of `SbaOutcome.executed` say what to re-key. A
resolved permanent-spell copy is a permanent, not a copy of a spell: the caller must stop
answering `is_spell_copy` for it. This is documented on `SbaEnvironment` and
`StoredSbaEnvironment`. Known follow-up: the engine holding copy status itself, so that no
caller re-keying is needed.

**Rationale.** Copies of spells, the deathtouch damage record, the variant and layer
evaluation inside `GameState` (#183, #186) are owned by later units; the seam lets this unit
deliver the full CR 704.5a-704.5i semantics now and lets the heartbeat and layer-integration
units wire the facts without changing the registry.

**Alternatives.** Adding copy and deathtouch markers to `events::ObjectMarkers` (rejected:
outside the owned paths, R1); returning a `Result` from `check` (rejected: the API is pinned);
a no-op default for the deathtouch clear (rejected in PR #207 review: an environment could
report damage and never close the window, so a stale entry would destroy a creature at a later
check after it lost indestructible).

Amended in PR #207 review round 1: the deathtouch pair is required; the one-zone-residency
contract and the caller's re-keying duty are stated.

## #41 FP-040 — Losing the game is a player designation

**Decision.** A player who loses through CR 704.5a, CR 704.5b or CR 704.5c gets the player
designation `"lostTheGame"` (`sba::LOST_THE_GAME`, value `DesignationValue::Flag`), and
`SbaOutcome.lost_players` lists every player newly losing in the pass. A player carrying the
designation is no longer a subject of these entries. The draw or win decision (CR 104.4a) is
left to the caller, who sees every loser of the pass together.

**Rationale.** No game-loss event or player-elimination procedure exists yet; a designation is
part of the hashed `GameState` and rolls back with it.

**Alternatives.** A new `GameEvent::LoseGame` (rejected: changes the events contract, R2); a
separate loser set outside `GameState` (rejected: not hashed, not part of rollback).

## #41 FP-040 — Zones::cease_to_exist and the owned-path extension

**Decision.** `Zones::cease_to_exist(id) -> Result<CardId, ZoneError>` removes the object and its
record, returns its card, puts the card into no outside-the-game pool and issues no id; the
CardId is retired and must not be reused (match-scoped, REQ-CORE-002); enforcement (making the
`Zones` container reject it, and/or retiring the token mark) is a known follow-up.
`perform_all` calls it for CR 704.5d and CR 704.5e after the move batch. For each ceasing
object it records why it ceases (token, CR 704.5d; copy of a spell or copy of a card,
CR 704.5e). If the object is still present it ceases as before. If a replacement effect of the
batch moved it, `perform_all` follows the executed `Outcome::ZoneChange` (old -> new) entries of
every executed batch, chains included, to its final id and tests the reason again in the new
zone (token: not the battlefield; spell copy: not the stack; card copy: neither the stack nor
the battlefield; the CR 723.3c exemption read from the old id); if it still holds, the new
object ceases in the same pass and its new id is in `SbaOutcome.ceased`, otherwise (for example
a card copy moved onto the battlefield) it stays. A token that has left the battlefield is
never moved by the event executor (`events/exec.rs` leaves it in its current zone), so a token
named by an `Instead` keeps its id and ceases under it. The object kind (token, spell copy, card copy)
persists across a move by an effect: the same end state as the repeated check, independent of
the caller re-keying its environment. A ceasing object still in the permanent container also
leaves it. LKI is captured (REQ-CORE-003: the object leaves its zone) by `perform_all` before
the first removal of the pass, with exile as the pseudo-destination of `capture_lki_on_move` (so a record is
stored from any zone, a token already in a graveyard included), counters from the object
markers and attachments from the permanent container (as the event executor does); an object
the provider does not know gets no record and still ceases; no `ZoneChange` is emitted;
`cease_to_exist` itself captures nothing. The capture happens after the move batch, for every
ceasing object, before the first removal (a removal unattaches what is attached to the removed
object, which would change a later object's record; CR 704.3, CR 704.8), not before the batch:
capturing a ceasing object that an `Instead` then moves would make the bus fail with
`AlreadyCaptured`, losing the whole pass. The state the record reads is nevertheless that of
before the batch (CR 704.8): when the pass has both moves and ceasing objects, `perform_all`
snapshots (clones) the permanent container and the object markers before the batch, writes
nothing into the live LKI store, and captures each ceasing object the batch did not move from
that snapshot (so a host that dies in the batch does not erase the attachment of a ceasing
copy attached to it). An object the batch moved and that then ceases under its new id is
captured from the live state, as the object in the zone it ceases from (the snapshot knows only
the old id); this and the characteristics read from the caller's provider, which `perform_all`
cannot snapshot, are the known limits. A ceasing object is never itself a batch member,
and only a battlefield spell copy among them is a permanent, whose state a batch can change only
through a replacement effect, so the record is the object as it last existed when it left; an
object the batch moved that then ceases under its new id gets a record for the new id as well.
The token mark (per card) and any Memory entry of the
object stay, since those containers belong to other modules. The brief extended this unit's
owned paths (R1) to `zones/container.rs` and `zones/tests.rs` for exactly this method.

**Rationale.** `send_outside` puts the card into an outside pool, which is wrong for a token or
a copy (CR 111.7, CR 707.10a): it ceases to exist.

**Alternatives.** Ceasing before the move batch (rejected: an `Instead` replacement naming the
object would then fail the whole pass with `UnknownObject`). Leaving a moved ceasing object to
the next check (rejected in PR #207 review: CR 704.3 and CR 400.7 do not require it, but the
caller's id-keyed environment would lose the copy status of the new object). Capturing LKI into
a scratch `LkiStore` before the batch and merging it (rejected: `LkiStore` has no insertion API
outside `memory/`, which this unit may not edit; a future change in `memory/` could allow strict
pre-pass capture).

Amended in PR #207 review round 1: follow-the-move cessation, LKI of ceasing objects, the
retired-CardId contract.
Amended in PR #207 review round 2: all ceasing LKI is captured before any removal, from the
pre-batch state for objects the batch did not move.

## #41 FP-040 — Two-Headed Giant gating

**Decision.** The CR 704.5c entry has the gate `VariantGate::Except(TwoHeadedGiant)` (its text:
"Ignore this rule in Two-Headed Giant games"); CR 704.5a stays active in every variant (no such
clause). The team checks CR 704.6a and CR 704.6b are out of scope for this unit and need a
follow-up unit (team model plus registry entries).

**Rationale.** The CR text of the two entries, quoted in the brief.

**Alternatives.** Gating CR 704.5a too (rejected: contradicts the CR text).

## #41 FP-040 — CR 704.7 merging per subject and result kind

**Decision.** Losses of the game are merged per player: one designation and one
`lost_players` entry whatever the number of reasons, all listed in `loss_reasons` in registry
order. Losses of different players are never merged. Moves to the graveyard are one
`ZoneChange` per object (an object with CR 704.5g and CR 704.5h findings moves once, both
findings reported), all in ONE batch submitted to `process_batch` with `sba = true`, which
merges same-object same-destination members (the inherited limitation "FP-028 Known
limitation: CR 704.7 merging only within one event kind" still holds). Every move uses
`ZoneChangeCause::StateBasedAction`, so a shield counter's destroy replacement does not apply
(CR 122.1c, CR 701.8b; "FP-028 SBA destruction and the shield counter's destroy replacement").

**Rationale.** CR 704.7: one replacement effect replaces all SBAs with the same result at the
same time; a result belongs to one player or object.

**Alternatives.** Distinguishing CR 704.5f ("put into") and CR 704.5g/h ("destroyed") with a
`Destroy` cause (rejected: the shield counter would then wrongly replace SBA destruction).

## #41 FP-040 — Indestructible and regeneration

**Decision.** A creature whose evaluated abilities include a `KEYWORD` ability with keyword
`"indestructible"` is skipped by CR 704.5g and CR 704.5h (CR 702.12b) but not by CR 704.5f.
Regeneration (CR 701.8c) is not modelled; the CR 704.5g/h entries say so in their comments and
in `SbaAction::Destroy`.

**Rationale.** CR 702.12b; the engine has no regeneration shield yet.

**Alternatives.** Reading indestructible from base characteristics (rejected: CR 613.4, a
granted or removed keyword must count).

## #41 FP-040 — The deathtouch flag and the empty-library flag are consumed by each pass

**Decision.** `perform_all` calls `SbaEnvironment::clear_deathtouch_damaged` after every
successful pass and consumes `EventState::take_empty_library_draws` on every successful pass
(before the move batch, so an empty-library draw caused by a replacement during the batch counts
for the next check), whether or not anyone loses or dies. The deathtouch window closes at every
check, for every creature, indestructible and phased-out ones included: an indestructible
creature dealt deathtouch damage that survives a check is not destroyed by the stale entry
after it later loses indestructible. On `Pending` or an error, both flags,
every context component and the replacement registry are as before the call.

**Rationale.** Both conditions are "since the last time state-based actions were checked"
(CR 704.5b, CR 704.5h); the check happens even when nothing applies.

**Alternatives.** Consuming only when a finding applies (rejected: a stale flag would make a
player lose at a later check).

## #41 FP-040 — Phased-out permanents and the state-based actions

**Decision:** 704.5f-704.5i evaluate only phased-in permanents (`SbaView::new` skips battlefield objects whose `Permanents` status has `phased_out`; an object with no `Permanents` entry counts as phased in). 704.5d/e stay by zone, so phased-out tokens and card copies on the battlefield survive. Marked damage, counters and the deathtouch flag are neither cleared on phase-out nor reset on phase-in; the deathtouch flag is still consumed by each successful pass, even for a phased-out creature.

**Rationale:** CR 702.26b ("treated as though it does not exist"), CR 702.26d (phasing is not a zone change; tokens continue to exist; counters remain), and CR 704.5 has no phased-out wording.

**Alternatives:** checking phased-out permanents (rejected: contradicts 702.26b); making 704.5d/e skip phased-out objects (rejected: 702.26d says tokens continue to exist while phased out).

## #41 FP-040 — performed means state changed

**Decision.** `SbaOutcome.performed` is `true` exactly when the pass changed the game state: a
move occurred (`executed` non-empty), an object ceased to exist, a player lost the game, or the
replacement registry changed (for example a one-use replacement effect was consumed). A finding
whose only move a replacement effect skipped is not performed, and consuming the empty-library
and deathtouch flags alone does not count. `findings` stays the record of what the check
detected. The heartbeat repeats the check while `performed`.

**Rationale.** CR 704.3: "If any state-based actions are performed as a result of a check, the
check is repeated". A persistent `Skip` on the only move left the state byte-identical, so a
heartbeat repeating on "any finding" would never stop.

**Alternatives.** `!findings.is_empty()` (rejected: the loop above); counting
`BusReport.applications` (rejected: an application that changes nothing would still repeat the
check, and a registry change is the observable effect).

## #41 FP-040 — Known follow-ups from round 1 review

**Decision.** Two review points are left to later units, not done in this unit. (1) The owner's
choice of order for several cards entering one graveyard at the same time (CR 404.3: "If an
effect or rule puts two or more cards into the same graveyard at the same time, the owner of
those cards may arrange them in any order.") is a property of every simultaneous multi-card
graveyard entry (the bus and zone-move layer, `events/` and `replacement/`) and is already
listed as later work in `#19 FP-018 (f)`; until it lands `perform_all` keeps ascending
`ObjectId` order with `Position::Top` (the ordering prompt is SRS §2.1 item 4, ORDER_CARDS).
Update: the `events/` API for this choice landed in #208 FP-154 (see "#208 FP-154 — Owner orders
simultaneous graveyard entry (CR 404.3)"); wiring it into `perform_all` is a follow-up.
(2) CR 704.5g/704.5h destruction must be observable as "destroyed" by triggers (CR 701.8b: "The
only ways a permanent can be destroyed are as a result of an effect that uses the word "destroy"
or as a result of the state-based actions that check for lethal damage (see rule 704.5g) or
damage from a source with deathtouch (see rule 704.5h). ..."); this needs a `ZoneChange` cause
or flag in `events/` and trigger normalization in `triggers/`, while shield counters stay
inapplicable (CR 122.1c). Until then those moves use `ZoneChangeCause::StateBasedAction` and
the distinction is available from `SbaOutcome.findings` (entry ids 704.5g/704.5h versus
704.5f).

**Rationale.** Both need changes in modules this unit does not own (R1).

**Alternatives.** Implementing them here (rejected: outside the owned paths, and the graveyard
ordering belongs to every simultaneous move, not only to state-based actions).

## #45 FP-044 — CardCatalog and BanListSource traits

**Decision.** `validate_deck` takes a `CardCatalog` (support status, digital-only, basic-land
facts per card id) and a `BanListSource` as trait objects. The API unit that registers the card
database implements them.

**Rationale.** No card database registry exists yet, and this unit must not depend on one.

**Alternatives.** Depending on `CardDefinitionDSL` directly (rejected: needs full definitions
with hashes for every check).

## #45 FP-044 — ZonesConfig::free_graveyard_reorder stays

**Decision.** The "moves to FormatConfig" note in "#19 FP-018 — Zones API" (b) is not done here.
`ZonesConfig::free_graveyard_reorder` stays where it is. The same holds for the engine-level undo
(RewindToSnapshot) gate noted under the txn snapshot entry. The unit that builds Zones from a
FormatConfig (match creation/API) does both; the Orchestrator files a follow-up issue.

**Rationale.** `zones/` is outside this unit's owned paths, and SRS §11.1 has no such member.

**Alternatives.** Editing `zones/` here (rejected: R1).

## #45 FP-044 — Policy semantics

**Decision.** `PolicyAction` has no Allow. `min_support_status` only distinguishes VERIFIED from
the rest (AutoParsed rejected under VERIFIED; Partial/Unsupported are governed only by their own
`on_*` action). Copies are counted per cardDefId over main+sideboard combined. An id absent from a
ban list is LEGAL; Restricted means at most 1 copy, and a Restricted entry takes precedence over
the basic-land exemption (house rule / format policy: the CR is silent on restriction; CR 100.2a
exempts basic lands only from the four-copy limit). Digital-only cards are rejected when the
extension is disabled OR `allowDigitalOnly` is false. `enforceColorIdentity` is stored but not
enforced (commanders deferred, REQ-FMT-002). `FirstTurnDrawRule` / `TimeoutPolicy` variants beyond
the SRS examples (NEVER_SKIP, ALWAYS_SKIP, AUTO_PASS, GAME_LOSS) are house-rule values. `maxMain`
null means no maximum. The preset ban-list version 2026-11-10 is the SRS example version and
preset ids are placeholders for data the client supplies. `BanListRef::parse` rejects only empty
parts and `/` or `@` inside parts (whitespace is not rejected).

**Rationale.** REQ-ING-008 (the engine never silently runs an incomplete card), REQ-FMT-004, and
SRS §11.1.

**Alternatives.** An Allow action (rejected: contradicts REQ-ING-008); trimming whitespace in
refs (rejected: not in the pinned contract).

## #42 FP-041 — World rule: the time with the world supertype is a caller-supplied stamp

**Decision.** CR 704.5k compares how long each permanent "has had the world supertype", not
when it entered the battlefield. `SbaEnvironment::world_supertype_since(object) -> u64` returns a
caller-supplied monotonic stamp of when the object last gained the supertype (default 0;
`StoredSbaEnvironment::world_since`, a missing object answers 0). A LARGER stamp means it gained
the supertype later, so it has had it for a SHORTER time. Among the evaluated, phased-in world
permanents, if exactly one holds the largest stamp, every other one is a finding; if two or more
hold it, all world permanents are findings. A single world permanent never goes. No choice, no
APNAP or timestamp tie-break. The default 0 therefore ties everything: a caller that does not
supply stamps sees every world permanent go when two or more exist.

**Rationale.** CR 704.5k and CR 205.4b (a permanent can gain the supertype after it entered);
the state model records no "gained a supertype" time, and the object timestamp is not that
measure. A stamp keyed by object id lasts one zone residency (CR 400.7), like the other seams.

**Alternatives.** The object's entry timestamp (rejected: wrong measure when the supertype is
granted later); the layer-system timestamp of the effect granting "World" (rejected: no such
query exists yet and a printed supertype has no effect timestamp); default "newest wins by id"
(rejected: an invented tie-break the CR forbids).

## #42 FP-041 — New SbaEnvironment seams for CR 704.5k-704.5t

**Decision.** Seven defaulted methods are added to `SbaEnvironment` (additive; every existing
implementor compiles unchanged), each with a matching public field of `StoredSbaEnvironment`:
`world_supertype_since` (`world_since`), `aura_can_enchant(aura, target) -> bool` default true
(`illegal_aura_targets`, the illegal pairs), `counter_limits(object) -> BTreeMap<String, i64>`
default empty (`counter_limits`), `saga_final_chapter(object) -> Option<i64>` default None, Some
only for an object with one or more chapter abilities (`saga_final_chapter`),
`chapter_ability_pending(object)` default false (`chapter_pending`),
`dungeons_at_bottommost_room() -> BTreeSet<ObjectId>` default empty (`bottommost_dungeons`) and
`room_ability_pending(dungeon)` default false (`room_pending`). Names, supertypes, card types,
subtypes and counters are read from the evaluated characteristics and
`LayerEnvironment::counters`; controllers from the layer evaluation (CR 613.1b, see "#42 FP-041
— Legend rule: conflict graph, maximal independent set"); attachments from `Permanents`. The engine itself
checks the structural part of CR 704.5m (not attached, attached to itself, target gone, player
left, Aura that is a creature or a battle); `aura_can_enchant` answers only the enchant ability
and other applicable effects (CR 303.4c). A negative counter limit counts as 0; several limits for
one kind are the caller's to reduce (the map holds one N per kind, the smallest applies).

**Rationale.** The state model has no enchant-ability legality, no "can't have more than N
counters" ability data, no chapter-ability or room-ability trigger tracking, no venture marker
and no supertype-gain time yet. Following "#41 FP-040 — The SbaEnvironment seam", facts the
state does not hold come from a caller seam; the heartbeat and layer-integration units replace
these seams with engine state. Every answer is a pure function of caller state (REQ-CORE-001).

**Alternatives.** Adding these facts to `GameState` or the permanent container now (rejected:
outside the owned paths and contracts of this unit, R1/R2); hard-coding "no limits / no sagas"
without a seam (rejected: the entries could not be tested or used).

## #42 FP-041 — Legend rule: conflict graph, maximal independent set

**Decision.** (Revised in PR #214 review round 1; replaces "keep exactly one per connected
component".) The vertices are the evaluated, phased-in, named legendary battlefield permanents,
grouped by their EVALUATED controller (`Evaluated::controller` after layer 2, CR 613.1b; the
`Permanents` controller only if the evaluation names none). Within one controller an edge joins
two permanents that have at least one name in common (CR 201.2a; a nameless object matches
nothing, CR 201.2a, CR 708.2a). "Same name" is pairwise, not an equivalence. Every connected
component with at least one edge is ONE group; isolated vertices are never asked. `check` is pure
and reports EVERY member of every group as a CR 704.5j finding. `perform_all` asks, before any
state change (including consuming the empty-library flag), one `SELECT_OBJECTS_FROM_ZONE` choice
per group (order: see "#42 FP-041 — F3 APNAP then lowest id"): chooser the controller, reason
`CR 704.5j` with no source, candidates and enumerated options the group ascending, min = 1, and
max = 1 when every pair of members shares a name (a clique, which includes the usual one-name
group and a cyclic overlap such as {X, Y}, {Y, Z}, {X, Z}), otherwise max = group size - 1. The
answer `SubmitObjectSelection { objects }` is the SURVIVOR set and is valid exactly when it is a
MAXIMAL independent set of the component: every id a candidate, no duplicate, no two selected
sharing a name, and every unselected candidate sharing a name with at least one selected one.
`default_on_timeout` is the greedy maximal independent set by ascending id (take the lowest id,
then each next id that shares no name with one already taken). Every unselected member is a move
to its owner's graveyard with cause `StateBasedAction` in the one batch. For A {X, Y}, B {Y},
C {X} under one controller the valid answers are {A} and {B, C}; {A, B}, {B} alone and {} are
rejected; the default is {A} (A < B < C). An invalid answer (the session rejects counts outside
min..max and non-candidates first) is `BusInterrupt::Error(BusError::Choice(
ChoiceError::Invalid { request_id, explanation }))` naming the legend-rule request, with nothing
changed; the session rejects it without recording it and the request stays pending (see "#42
FP-041 — Invalid legend-rule answers are correctable choice errors"). For a clique the request and outcome are
exactly the former ones (min = max = 1, default the lowest id).

**Rationale.** CR 704.5j, CR 201.2a; the rules-researcher (PR #214 round 1): the legal end states
of the A/B/C case are {A} or {B, C}, no two survivors may share a name, and the CR gives no
official example for overlapping names, so the algorithm is recorded here. A maximal independent
set is exactly a set of survivors in which no two share a name and every removed permanent
shares a name with a survivor (it really was in conflict), which is what "chooses one of them,
and the rest are put into their owners' graveyards" yields however the overlapping choices are
sequenced. One ask per component keeps all choices before any change (restore-on-`Pending`
stays trivially true), is always satisfiable (the greedy default), and after the pass no two
same-named legends of one controller remain. CR 613.1b / CR 110.2: the controller is the one
after control-changing effects. SRS choice row "SELECT_OBJECTS_FROM_ZONE ... legend rule".

**Alternatives.** Connected component, keep exactly one (rejected by review: forbids the legal
{B, C}); the researcher's iterative "lowest violating legend and its overlap set" asks (rejected:
several sequential asks per controller, each depending on the previous answer, for the same end
states); one choice per name (rejected: an object with two names would be in two groups and could
be both kept and lost); asking the active player (rejected: CR 704.5j says "that player");
keeping the newest automatically (rejected: invents a rule); the stored `Permanents` controller
(rejected: ignores layer-2 control changes).

## #42 FP-041 — Mixed results: ceasing wins over a move; Sacrifice cause; counter amounts

**Decision.** (Rewritten in PR #214 review round 2; the round-1 title was "Mixed results: move
wins over unattach and counter removal; Sacrifice cause", and the rule "a move wins over
unattaching and counter removal" is withdrawn, see "#42 FP-041 — One SBA batch: moves, counter
removals and unattach events".) Within one pass: ceasing to exist wins over a move (as before),
and an object that ceases to exist gets no `RemoveCounters` and no `Unattach` event. A permanent
that moves (or whose move a replacement effect prevents or redirects) gets every other
applicable action in the same batch: its counter removals (CR 704.5q, CR 704.5r) and its one
unattach event (CR 704.5n, CR 704.5p). An object with a CR 704.5s finding and another move
finding moves once with `ZoneChangeCause::Sacrifice`. When CR 704.5q and CR 704.5r both remove
counters of one kind from one object, one event removes the larger of the two amounts (the
result equals applying 704.5q and then 704.5r). A dungeon removed from the game (CR 704.5t) is
handled as in "#42 FP-041 — Removing a dungeon from the game".

**Rationale.** CR 704.3 (all applicable state-based actions performed simultaneously as a single
event; REQ-SBA-001 "perform every applicable SBA simultaneously as one event"), CR 704.8 (LKI
from before the pass). Sacrifice is the more specific description of what happens to the Saga
(CR 715.4) and lets sacrifice-matching effects see it. Taking the larger counter amount never
removes more counters than either rule demands.

**Alternatives.** Summing the two counter amounts (rejected: removes more than either rule
requires; noted as a rules question to the Orchestrator); unattaching through `Permanents::detach`
after the batch (rejected in PR #214: no event, so "becomes unattached" triggers never saw it);
"a move wins": no counter removal or unattach for a moving permanent (round 1; withdrawn in round
2: CR 704.3 gives no exclusion of a permanent from a second applicable state-based action because
another one moves it, and it forced the follow-up batch whose defects review round 2 found).

## #42 FP-041 — Other judgement calls (Aura and battle, unknown targets, phasing)

**Decision.** (1) An attached Aura that is also a battle is unattached by CR 704.5p /
CR 310.10 in the first check and stays on the battlefield. It is put into the graveyard by
CR 704.5m as "not attached" in the next check (CR 704.3). If the object it is attached to is
independently illegal (enchant ability, CR 303.4c), CR 704.5m also fires in the first check and
both are performed in the one event (its unattach event, then its move; see "#42 FP-041 — One
SBA batch: moves, counter removals and unattach events"). Creature-Auras (CR 303.4d) are unchanged: graveyard in one
pass. The rules-researcher established this from CR 310.10, 704.5p, 704.5m and 303.4c: "illegal"
in CR 704.5m is defined by the enchant ability and effects on the object it is attached to, not
by the Aura's own types. (2) (Revised in PR #214 review round 2.) Each subtype's restriction
applies: an attached Equipment is legal only on a creature (CR 301.5), an attached Fortification
only on a land (CR 301.6), so an artifact that is both an Equipment and a Fortification is legal
only on a permanent that is both a creature and a land:
`legal = (!is_equipment || target_is_creature) && (!is_fortification || target_is_land) &&
equipment_can_attach(pair)`, "creature" and "land" read from the evaluated types. The CR has no
rule for an object with both subtypes; this is the plain reading of both restrictions
(rules-researcher, round 2). (3) An attachment whose target permanent is on the battlefield but
cannot be evaluated by the layer environment is not judged by CR 704.5n's type check (existing
"skip what cannot be evaluated" policy); a pair the environment marks illegal
(`equipment_can_attach` false) is still unattached (see "#42 FP-041 — F5 equipment_can_attach
seam"). (4) An attachment to a phased-out permanent is not checked by CR 704.5m, 704.5n or
704.5p (CR 702.26b); a phased-out permanent is never a legend-group member, world permanent,
counter holder or Saga for these entries. (5) A player who has the `LOST_THE_GAME` designation or
is not in the game counts as having left the game for CR 303.4c. (6) The container refuses
self-attachment, so "attached to itself" is checked defensively by
`target_self_or_gone` and unit-tested there.

**Rationale.** CR 310.10 restricts the battle as the attacher, so a battle-Aura's own types do
not make the attachment illegal for CR 704.5m. The observable difference from a one-pass
graveyard is that it is unattached when it leaves (last known information) and its host loses the
Aura's effects between the two checks. The rest follows the decisions of
#41 FP-040 on phasing and unevaluable objects.

**Alternatives.** (1) The former one-pass graveyard for a battle-Aura, treating it as illegal
under CR 704.5m (rejected: contradicts CR 310.10 / 704.5p as established by the rules-researcher).
(2) Legal on a creature OR a land (round 1; rejected in review round 2: an Equipment-Fortification
on a nonland creature would stay attached although CR 301.6 says a Fortification "can't legally
be attached to an object that isn't a land").

## #42 FP-041 — 704.5q and 704.5r on the same permanent and kind

**Decision.** Per counter kind, remove the larger of the two amounts: an order-independent union
of both removals.

**Rationale.** The CR does not address the overlap. Counters of one kind are interchangeable
(CR 122.1); applying 704.5q then 704.5r removes the larger amount, whereas applying 704.5r then
704.5q would differ. Examples: 5 +1/+1, 2 -1/-1, limit 1 on +1/+1 gives events (+1/+1, 4) and
(-1/-1, 2), final (1, 0). 3 +1/+1, 5 -1/-1, limit 1 on +1/+1: 704.5q removes 3 of each, 704.5r
would remove 2 of +1/+1, so events are (+1/+1, 3) and (-1/-1, 3), final 0 +1/+1 and 2 -1/-1.

**Alternatives.** Summing the amounts (rejected: it removes more than either rule requires);
applying 704.5r first (rejected: order-dependent).

## #42 FP-041 — F3 APNAP then lowest id

**Decision.** The CR 704.5j asks are ordered by the APNAP rank of the group's (evaluated)
controller (CR 101.4: the active player, then the turn order from the active player; computed
inside the sba module from `GameState::turn_order` and `TurnInfo::active_player` as
`Bus::player_order` does, the bus being private; players not in the turn order last, by id;
an empty turn order means all players ascending), then, within one controller, by the group's
lowest object id. All asks happen before any state change; all moves go in one batch after every
answer.

**Rationale.** CR 101.4 (different players choose in APNAP order, then the actions happen
simultaneously). CR 101.4b ("A player knows the choices made by the previous players when
making their choice") prescribes no mechanism (rules-researcher, round 2); asking in APNAP order
only guarantees that every earlier choice exists before a later player is asked, it is not
itself how the later player learns it. The mechanism: legend-rule requests are not hidden
(`hidden: false`), so each accepted survivor set is public information, recorded in order and
available as `ChoiceSession::answers()`; presenting the earlier players' accepted selections to
the later chooser before they answer is owned by the wire-protocol unit #80 (FP-078). (Revised
in PR #214 review round 2; the round-1 text said the request order was how the later player
knows.) CR 101.4c leaves the order of one player's several choices to that player, and the
rules-researcher accepts a deterministic order as an implementation choice: lowest id is the
existing tie-break (R3).

**Alternatives.** Lowest id across players (rejected by review: breaks CR 101.4); asking the
controller to order their own groups (rejected: an extra choice with no effect on the end state,
since every move waits for all answers).

## #42 FP-041 — One SBA batch: moves, counter removals and unattach events

**Decision.** (PR #214 review round 2, the Orchestrator's "option (c)"; replaces the round-1
section "F1 follow-up batch (CR 704.3 simultaneity vs engine batching)" and the round-1 rule
"move wins over unattach and counter removal".) `perform_all` builds ONE batch and makes ONE
`process_batch(.., sba = true)` call per pass: the CR 704.5n / CR 704.5p `Unattach` events
ascending id (submitted first since review round 3, see "#42 FP-041 — Planned unattach and
pre-pass host (round 3)"), then the zone changes (CR 704.5f-704.5k, CR 704.5m, CR 704.5s, legend
losers) ascending object id, then the CR 704.5q / CR 704.5r `RemoveCounters` events ascending
object id then kind (amounts from the check snapshot), for EVERY applicable permanent, including a permanent
that also moves in the same pass. Only an object that ceases to exist (CR 704.5d, CR 704.5e; its
ceasing path is unchanged) and a dungeon removed by CR 704.5t get no counter removal and no
unattach event. There is no follow-up batch. Consequences:
- The bus executes every event that is not a zone change before every zone change of the batch
  (its reconcile order) and captures last known information of every object before any event
  runs (CR 704.8), so a counter removal or unattach event for a permanent that also moves never
  names a moved object, and the moving permanent's record still shows its counters and its
  attachment.
- Replacement effects are matched against the state before the event: eligibility such as
  `requires_counter` is judged before sibling events of the pass (for example another
  permanent's counter removal) change the state, and the ids the batch retires are tracked in one
  bus call, so an `Instead` naming a permanent that dies in the same pass is handled like any
  other same-batch event.
- Exactly ONE `Unattach` event per attachment that ends in the pass: one per attachment even if
  CR 704.5n and CR 704.5p both apply, and an attachment whose host also leaves gets its one event
  against the attachment from before the pass (executed before the host's zone change, so the
  host leaving, which clears attachments without an event, has nothing left to clear). The zone
  change of a leaving attachment emits no extra `Unattach` (CR 701.3d, CR 603.10c).
- Counter removals of a permanent that also leaves are emitted (CR 704.3), with no look-back
  entry (CR 603.10 has none for counter removal); counters left on an object that moves get no
  removal event (CR 122.2: they "simply cease to exist").
- `SbaOutcome::unattached` lists, ascending, only the permanents unattached by every `Unattach`
  event of the batch, planned or produced by a replacement (for example an `Instead` of another
  event of the batch; review round 3, H4), that are still on the battlefield as the same object
  at the END of the pass (CR 704.5n, CR 704.5p: "It remains on the battlefield"); an unattach
  that found nothing attached is no event and lists nothing; the event stays in `executed`
  either way (decision D2 of review round 2). `performed` is true whenever any event ran.
- `Pending` or an error anywhere in the batch restores every component of the context, the
  replacement registry and the environment flags.

**Rationale.** CR 704.3 ("performs all applicable state-based actions simultaneously as a single
event") and REQ-SBA-001 ("perform every applicable SBA simultaneously as one event"); CR 704.8
(LKI from before the state-based actions); CR 614.1 (replacement effects "watch for a particular
event that would happen"). The round-1 premise, that the events executor rejects an event naming
an object an earlier event of the batch moved, does not hold for SBA batches: the bus always
executes the non-zone-change events first. The follow-up batch it motivated read replacement
eligibility after the main batch had changed the state (review G1), started a second bus call
without the first one's retired ids (G2) and lost the unattach event of an attachment whose host
left in the main batch (G9). Two parts are JUDGEMENT CALLS where the CR is silent
(rules-researcher, round 2), anchored on REQ-SBA-001 "one event": (1) replacement applicability
for the parts of one SBA event is judged against the PRE-EVENT state (CR 704.3 + CR 614.1);
(2) a permanent that leaves in the pass still gets its counter removals and unattach event in the
same event (CR 704.3 gives no exclusion of a permanent from a second applicable SBA because
another SBA moves it).

**Alternatives.** The round-1 follow-up batch for objects whose move was prevented (rejected in
review round 2: G1, G2, G9 above); a follow-up batch sharing one bus context or propagating
retired ids (rejected: still judges eligibility after the main batch, and needs bus changes
outside this unit); dropping the removals and unattach of a moving permanent (rejected: CR 704.3,
and the attachment of a leaving host would lose its event); waiting for the next check
(rejected: `performed` could be false, so there might be no next check).

## #42 FP-041 — Removing a dungeon from the game

**Decision.** (PR #214 review round 2; G7 DEFERRED, behaviour unchanged.) The correct semantics:
a dungeon card removed from the game by CR 704.5t (completing it, CR 309.7) goes OUTSIDE the game
and stays available to a later venture (CR 309.2a: a player who ventures without a dungeon in the
command zone "choose[s] a dungeon card they own from outside the game"; CR 400.11: outside the
game is not a zone; CR 701.49c). Removal from the game is not ceasing to exist. The current code
still uses the ceasing path: no zone change, last known information captured from the pre-pass
state, then `Zones::cease_to_exist`, which retires its `CardId`; it is reported in
`SbaOutcome::removed_from_game`, not `ceased` (if it also ceases by CR 704.5d/e it is reported as
ceased). That retirement is the WRONG model, kept only as a placeholder: no venture
implementation exists yet. The fix is a dedicated dungeon `OutsidePool` variant
(`zones/types.rs`) moved through `Zones::send_outside` (`zones/container.rs`), an R2 contract
change that lands as a separate contract-only PR; the Orchestrator filed it as issue #215
(FP-159), which depends on the venture unit. The call site in `sba/perform.rs` carries a comment
pointing here. The SBA side keeps no counter removal or unattach event for such a dungeon.

**Rationale.** CR 309.2a, 309.7, 400.11, 701.49c (rules-researcher, round 2: the card goes outside
the game and stays available to a later venture). Changing the zones contract is outside this
unit's owned paths (R1, R2); until a venture exists, nothing can observe the difference.

**Alternatives.** Sending the dungeon to an existing outside pool now (rejected: no dungeon pool
variant exists and adding one is a contract change, R2); keeping the ceasing model as final
(rejected: contradicts CR 309.2a / 701.49c).

## #42 FP-041 — Invalid legend-rule answers are correctable choice errors

**Decision.** (PR #214 review round 2.) A legend-rule answer that passes the session's own
validation (count within min..max, candidates only) but is not a selection or not a maximal
independent survivor set is rejected by the procedure with `BusInterrupt::Error(BusError::Choice(
ChoiceError::Invalid { request_id, explanation }))`, `request_id` the id of the legend-rule
request just asked (deterministic: `ChoiceContext::next_request_id` at that ask). The conversion
`impl From<BusInterrupt> for Interrupt` keeps a choice error's class: `BusError::Choice(e)`
becomes `Interrupt::Error(e)`; every other bus error still becomes
`Interrupt::Error(ChoiceError::Procedure(..))`. A host procedure that propagates the interrupt
therefore makes `ChoiceSession::submit` return the `Invalid` error without recording the answer,
and the same request stays pending for a corrected answer. `perform_all` restores everything on
the error as before.

**Rationale.** The FP-027 convention ("#28 FP-027 — Card-name validation is structural"):
procedure-level validation returns `Interrupt::Error(ChoiceError::Invalid { request_id,
explanation })` and must not re-ask. With the former `Procedure` error the run completed with an
error output, so `submit` recorded the invalid answer and closed the session (review G5). Any
other choice error that crosses the bus (for example `Divergence` or `RequestIdMismatch` from an
ask inside a replacement choice) now also keeps its class instead of being flattened to
`Procedure`; the session rejects it unchanged either way.

**Alternatives.** Re-asking the same request inside the procedure (rejected: the session would
record the invalid answer as accepted); a dedicated SBA error type (rejected: the session already
has the right error class); converting only `Invalid` (rejected: every choice error is better
reported with its own class, and the conversion stays one arm).

## #42 FP-041 — F2 unattach events and the 704.5p inference

**Decision.** CR 704.5n and CR 704.5p unattach through `GameEvent::Unattach { attachment }`
events in the pass's one `process_batch` call (submitted before the zone changes and counter
removals since review round 3, ascending id; see "#42 FP-041 — One SBA batch: moves, counter removals and unattach events"), no
longer through a direct
`Permanents::detach`. The events executor detaches (once); the trigger normaliser turns each
into an `EventKind::Unattached` occurrence (CR 603.2), and the batch captures last known
information before any of its events, so look-back (CR 603.10c, CR 704.8) sees the attachments
from before the pass. The bus executes the non-zone-change events of a batch before its zone
changes (its own reconcile order), so an attached permanent whose host dies in the same pass is
unattached by its event before the host moves, and is listed in `SbaOutcome::unattached` if it
is still on the battlefield at the end of the pass (`unattached` covers every Unattach event of
the batch, planned or produced by a replacement; review round 3, H4). The
704.5p unattachment of creatures, battles and other non-Aura/Equipment/Fortification permanents
emits the same event.

**Rationale.** CR 701.3d defines "becomes unattached" for Auras, Equipment and Fortifications;
CR 603.10c makes such triggers look back in time. Applying the same event to the 704.5p
permanents is an INFERENCE beyond CR 701.3d's wording (rules-researcher, PR #214 round 1): it
treats every state-based unattachment alike and lets a future trigger on any attached permanent
see it; a trigger that only cares about Auras, Equipment and Fortifications filters by type.
Phasing (CR 702.26j) is not involved: the SBA path never phases anything.

**Alternatives.** Keeping the direct detach (rejected by review: triggers never see it);
emitting events only for 704.5n (rejected: two control paths for one action, and the 704.5p
case would still be invisible to triggers).

## #42 FP-041 — F5 equipment_can_attach seam

**Decision.** `SbaEnvironment::equipment_can_attach(attachment, target) -> bool`, default
`true`, with the public field `StoredSbaEnvironment::illegal_equipment_targets:
BTreeSet<(ObjectId, AttachTarget)>` (default empty; the stored environment answers
`!contains`). Under CR 704.5n an attached Equipment or Fortification on a phased-in
battlefield permanent is illegal when `equipment_can_attach` is false for that exact pair, or,
when the target can be evaluated, when the type check fails: it becomes unattached and stays on
the battlefield. (Revised in PR #214 review round 2: the pair is asked after the phased-out guard
and BEFORE the target's facts are looked up, so a marked pair is unattached even when the layer
environment cannot evaluate the target.) A phased-out target is still not checked
(CR 702.26b); an attachment to a player is illegal anyway.

**Rationale.** CR 702.16d: protection makes Equipment and Fortifications with the stated
quality unattach as a state-based action; CR 301.5b / 301.5c: an object that "can't be equipped"
by it is illegal. The state model has no protection or "can't be equipped" data yet, so, like
`aura_can_enchant` (CR 303.4c), the caller supplies the answer; the heartbeat and
layer-integration units replace the seam with engine state (SRS lists an attachment-legality
predicate in the protection bundle).

**Alternatives.** Reading protection from evaluated abilities now (rejected: no quality matching
exists in the state model, R4 forbids a special case); folding it into `aura_can_enchant`
(rejected: different rule, different consequence: graveyard versus unattach).

## #42 FP-041 — Planned unattach and pre-pass host (round 3)

**Decision.** (PR #214 review round 3, H6.) `GameEvent::Unattach { attachment }` names only the
attachment and the events executor detaches whatever host is current when it runs, so a planned
CR 704.5n / CR 704.5p unattach executed AFTER an `Attach` that a replacement effect made of
another event of the same batch would clear the new link. `perform_all` now submits the batch's
`Unattach` events FIRST (ascending id), then the zone changes, then the counter removals. The bus
executes the events that are not zone changes in submission-lineage order (an `Instead`
descendant at the position of the event it replaces, CR 614.6), so every planned unattach runs
before an `Attach` produced by replacing a zone change or a counter removal of the batch: it
clears the link from before the pass and the replacement's new link stays. Still one batch, one
`Unattach` event per ending attachment, last known information captured before the batch, no new
event type and no change outside `sba/`. Known limitation (judgement call, deferred): an
`Attach` of attachment E produced by replacing the planned unattach of a LOWER-id attachment D in
the same pass still executes before E's own planned unattach and is cleared by it; fixing that
needs the unattach event to name the host it clears (a `GameEvent` / canonical-encoding / events
executor change, an R2 contract change outside this unit). `SbaOutcome::unattached` still lists
such a re-attached attachment (its unattach event occurred and it remains on the battlefield).

**Rationale.** CR 614.1 (a replacement "completely or partially replace[s] that event with a
different event") and CR 614.6 (the modified event occurs instead): the attachment a replacement
makes is part of the one SBA event (CR 704.3), and the SBA's own action is to end the illegal
attachment that existed when the check found it (CR 704.5n, CR 704.5p). Ordering inside the
module is the only mechanism that needs no contract change. The only tests whose assertions
changed are the two in-module tests that asserted the old submission order
(`prevented_move_still_removes_counters_and_unattaches`,
`unattach_events_in_the_batch_reach_triggers_with_look_back`); every spec test passes unchanged.

**Alternatives.** A per-event guard in the bus (rejected: `process_batch` has none; adding one is a
bus change outside this unit); detecting after the batch that the attachment's host changed and
re-attaching directly (rejected: a state change outside the event system, no new timestamp, and
the spurious `Unattach` event would stay in the log for triggers); a second batch after the main
one (rejected: breaks the single SBA event, CR 704.3, REQ-SBA-001); omitting the planned unattach
when some event of the batch re-attaches the attachment (rejected: replacements are applied
inside `process_batch`, so this is unknown before the bus runs); an `Unattach` naming its host
(rejected for this unit: R2 contract change; the residual case above is deferred to it).

## Process — Dual-harness agent documentation (Claude Code and Gemini/Antigravity)

**Decision:** Support both Claude Code and Gemini / Antigravity across the repository's agent
instructions, subagents, skills, templates and process documents, using a single-source-of-truth
layout with no git symlinks:
- `AGENTS.md` is the canonical root instruction file. `CLAUDE.md` imports `@AGENTS.md` with brief
  Claude Code notes; `GEMINI.md` imports `@./AGENTS.md` with brief Gemini / Antigravity notes
  (Gemini CLI inlines `@./AGENTS.md`; Antigravity loads `AGENTS.md` directly and treats
  `@./AGENTS.md` as a path reference without duplicating context).
- `.claude/agents/` and `.claude/skills/` remain the canonical Markdown prompts and skill
  procedures, updated to be dual-platform (Claude Code and Gemini / Antigravity model tiers,
  REST/`ccr` and `gh` CLI / MCP tooling, and harness-neutral worktree fallback).
- `.agents/agents/` provides Antigravity custom subagent definitions (with Antigravity YAML
  frontmatter and safe cross-surface tool lists: `view_file` and `run_command` for read-only roles
  including `rules-researcher`, plus `replace_file_content` and `write_to_file` for `task-executor`
  and `fixture-author`) that forward to `.claude/agents/`. Standalone Gemini CLI does not load
  `.agents/agents/`; its main session does not run executor, fixture-author or verifier roles
  directly and uses `.claude/agents/` only through custom subagents mirrored locally under
  `.gemini/agents/`, as documented in `GEMINI.md`.
- `.agents/skills/` provides Gemini / Antigravity skill adapters forwarding to `.claude/skills/`
  (`work-issue`, `run-plan`, `plan-from-srs`, `triage`, `pzl-test`), plus the Gemini /
  Antigravity-specific `pr-watch` skill (`.agents/skills/pr-watch/SKILL.md`), which stands in for
  Claude Code's `subscribe_pr_activity` / auto-fix loop by scheduling non-blocking check-ins
  (`/schedule` or a single async background check-in), addressing CodeRabbit findings, resolving
  threads itself, and driving PRs to CodeRabbit approval and green CI.
- `docs/PROCESS.md` P1 and P4–P7, `docs/DEVELOPMENT.md`, `docs/BRIEF_TEMPLATE.md`, `README.md`,
  `.github/ISSUE_TEMPLATE/work-unit.md`, `.github/pull_request_template.md` and `.gitignore`
  (ignoring local Claude Code files and `.gemini/settings.json` for personal project-specific
  Gemini CLI settings) are updated for both harnesses.

**Rationale:** The repository previously assumed Claude Code exclusively (`CLAUDE.md`,
`.claude/agents/`, `.claude/skills/`, Claude-only model names, and `subscribe_pr_activity` /
`send_later` / `get_session` tooling). Antigravity discovers `AGENTS.md`, `.agents/agents/*.md`
and `.agents/skills/<name>/SKILL.md` natively, while Gemini CLI discovers `GEMINI.md` and
`.agents/skills/<name>/SKILL.md`. Forwarding from thin `.agents/` adapters to the canonical
`.claude/` files avoids duplicating long prompt bodies and avoids git symlinks, which require
elevated privileges or Developer Mode on Windows (a first-class target OS under REQ-NFR-003).
Antigravity custom subagent frontmatter requires exact Antigravity tool names (unmapped tool names
can hang subagent startup), so `.agents/agents/*.md` declares its own frontmatter rather than
sharing `.claude/agents/*.md` directly. Because Antigravity has no built-in PR auto-fix or
GitHub-webhook subscription, a dedicated `pr-watch` skill gives Antigravity sessions a repeatable,
non-blocking check-in and thread-resolution procedure without touching Claude Code's native PR
subscription.

**Alternatives:**
- Duplicate full skill and agent Markdown bodies in `.agents/` and `.claude/`: rejected because
  every process edit would risk drift between the two copies.
- Symlink `CLAUDE.md`, `GEMINI.md`, `.agents/skills/` and `.claude/skills/`: rejected because git
  symlinks break or check out as plain text files on Windows without Developer Mode.
- Use `.agents/skills.json` pointing to `.claude/skills/` instead of `.agents/skills/<name>/SKILL.md`:
  rejected because Gemini CLI reads `.agents/skills/` directly and does not read Antigravity's
  `skills.json` discovery manifest.
- Mirror `.gemini/agents/` in the repository for Gemini CLI: deferred in favor of scoping
  `.agents/agents/` to Antigravity and documenting the standalone Gemini CLI workflow in
  `GEMINI.md`, avoiding unverified version-specific Gemini CLI frontmatter.

## Infrastructure — Cloud agent environment setup (.agents/environment.json)

**Decision:** Add repo-scoped Antigravity cloud environment configuration (`.agents/environment.json`
invoking `.agents/setup-env.sh`) so fresh Linux cloud VMs boot with the pinned Rust toolchain from
`rust-toolchain.toml` plus `llvm-tools` and `wasm32-unknown-unknown`, `cargo-llvm-cov` pinned to
match `.github/workflows/ci.yml`, required Linux packages (`build-essential`, `pkg-config`, `curl`,
`ca-certificates`, `python3`, `jq`, `git`), non-interactive `~/.bashrc` `PATH` persistence for
Cargo's bin directory, and pre-fetched locked Cargo crate caches for the root workspace and
`spikes/bindings`.

**Rationale:** `rust-toolchain.toml` deliberately omits `llvm-tools` and `wasm32-unknown-unknown`
(CI installs them separately), and `cargo-llvm-cov` is required locally on every PR to run
`cargo xtask coverage` (REQ-NFR-008). Installing only the Linux packages Fine Print actually needs
(skipping GUI, GPU, audio and unrelated runtime dependencies) keeps fresh VM bootstrap fast while
ensuring `cargo fmt`, `cargo clippy`, `cargo test`, `cargo xtask coverage`, `cargo xtask guard`,
`cargo xtask cr`, `scripts/req_coverage.py` and `pr-watch` work out of the box. Checking `dpkg-query`,
active toolchain components/targets and `cargo llvm-cov --version` makes re-runs idempotent and
fast.

**Alternatives:**
- Add `llvm-tools` and `wasm32-unknown-unknown` to `rust-toolchain.toml`: rejected because it
  changes every developer's and every CI job's toolchain download (including `fmt`, `clippy` and
  `guards`, which do not need them), contradicting `docs/DEVELOPMENT.md`.
- Install Playwright browsers or rebuild `cbindgen` / `wasm-bindgen-cli` on every VM boot:
  deferred because `spikes/bindings` is a completed Phase 00 spike and the real FFI / .NET / WASM
  bindings land in Phase 14 (#102–#104); adding them later keeps current cloud VM startup fast.

## Process — Initial-usable-build review rhythm (tests along the way, sanity review on boundaries, full review at M1)

**Superseded** by "Process — Gate reviews at phase and milestone completion" below.

**Decision:** While Milestone M1 — Two-player engine builds the first usable end-to-end two-player
build, split PR code review into three mutually exclusive label-driven modes in `docs/PROCESS.md`
P6 and `.coderabbit.yaml`:

1. **`review:skip`** (default for routine M1 vertical-slice chunks: individual SBA additions,
   effect families, keyword families, grammar template slices, demo decks/clients, `.pzl`
   regression packs, benchmarks, and scoped follow-ups on already-reviewed spines): no automatic
   per-PR CodeRabbit review. Every PR remains gated by Implementer + Independent Tester tests,
   coverage floors (90% rules core / 75% others, REQ-NFR-008), repository guards
   (`cargo xtask guard all`), differential cached vs. full-recompute layer digests (REQ-LAYER-005),
   `work-verifier` (`pass`, P5 unit gate), and all required multi-OS CI checks.
2. **`review:sanity`** (targeted mid-stream sanity review for anything expensive to change later):
   required on any issue or PR defining or changing
   - P3 public interfaces, schemas, wire/event contracts, or bindings (`#16`, `#17`, `#51`, `#64`,
     `#70`, `#79`, `#80`, `#81`, `#83`, `#86`, `#100`, `#102`, `#103`, `#104`, `#159`, `#161`,
     `#169`, or any PR touching `contracts/CHANGE_REQUESTS.md`);
   - core deterministic state machines, rules loops, or `very-high` blast-radius rules evaluation
     (`#44`, `#46`, `#52`–`#54`, `#57`, `#58`, `#67`, `#68`, `#77`, `#78`, `#183`, `#184`, `#186`);
   - security or compliance-critical paths: R5 card-data boundary (`#17`, `#87`, `#100`, `#101`),
     R11 override validation and `VERIFIED` enforcement (`#86`, `#98`), and R12 redaction,
     determinized forking, and untrusted-input parsers (`#16`, `#51`, `#80`, `#81`, `#83`, `#85`,
     `#87`, `#89`, `#91`, `#100`); or
   - automated milestone determinism, conformance, or release gates (`#106`, `#121`, `#122`).
   CodeRabbit runs in `quiet` profile with `auto_pause_after_reviewed_commits: 2`; blocking
   boundary, contract, schema, state-machine, determinism (R3), no-card-branch (R4),
   security/compliance (R5/R11/R12) and verified correctness defects are fixed, while cosmetic
   style, naming and minor refactor nitpicks are deferred to `#219` (FP-161) or a new M2
   `work-unit` issue.
3. **`review:full`**: runs the cohesive full review pass over the working end-to-end M1 build in
   `#219` (FP-161, gating `#123` FP-121 Milestone M1 acceptance), lands high-leverage cross-cutting
   fixes, and flips `.coderabbit.yaml` to post-usable per-chunk review (`auto_review.enabled: true`,
   `profile: chill`, `labels: ["!review:skip"]`, `auto_pause_after_reviewed_commits: 2`) for `#123`
   (`review:full`) and all M2/M3 work (`#124`–`#141`, all transitively blocked by `#123`).

A PR carries at most one `review:*` label; upgrading a `review:skip` issue/PR removes `review:skip`,
and if `review:skip` ever coexists with `review:sanity` or `review:full`, the stronger review label
wins. Because `docs/SRS.md` REQ-NFR-011 bullet 2 still reads "reviewed by CodeRabbit" unconditionally
and only a person edits `docs/SRS.md` (P1, P8), a proposed wording alignment is queued in
`docs/HUMAN_QUEUE.md` while this owner-requested workflow takes effect immediately.

**Rationale:** Running multi-round full CodeRabbit polish loops on every isolated M1 vertical slice
slows down reaching the first working two-player build and risks polishing abstractions whose real
shape only becomes clear once the full engine, DSL runtime, toolchain, bindings and clients run
end-to-end. Keeping Implementer + Independent Tester tests, 90%/75% coverage floors, differential
cached/full-recompute checks, repo guards, `work-verifier` and multi-OS CI mandatory per PR catches
logic regressions immediately; targeted `review:sanity` passes protect every P3 contract, core
deterministic state machine and R5/R11/R12 security boundary that is expensive to retrofit; and
`#219` (FP-161) gives the full review pass the cohesive context needed to catch cross-cutting
drift, duplicated logic and contract/state-transition bugs before flipping to tighter per-PR review
for M2/M3.

**Alternatives:**
- Keep full per-PR CodeRabbit review (`auto_review.enabled: true`, `auto_pause_after_reviewed_commits: 0`)
  on all 102 open M1 PRs: rejected because per-chunk polish loops bottleneck delivery of the first
  usable two-player build and create sunk-cost attachment to early module shapes.
- Disable CodeRabbit completely until M1 acceptance: rejected because P3 contracts (`fineprint-dsl`,
  `IRulesEngine`, wire protocol, bindings, `*.fpdb`/`*.fpreplay`/`.pzl`), core deterministic rules
  loops (priority/SBA heartbeat, layers/`GameState` hash, casting, DSL interpreter, combat solvers,
  loops) and R5/R11/R12 security boundaries (redaction, determinized forking, `VERIFIED`, untrusted
  parsers) are expensive to retrofit once downstream modules build on them.
- Rely on agent self-review during M1: rejected; R9 (no self-review) remains intact, with
  `work-verifier` checking objective facts and CodeRabbit remaining the sole code reviewer.

## Process — Gate reviews at phase and milestone completion

**Decision:** At the repository owner's request, align the review rhythm with the sibling
projects: no code review on routine unit-of-work PRs, and one cumulative CodeRabbit review at the
completion of each phase and milestone (`docs/PROCESS.md` P6 "Gate reviews"). This supersedes the
label-driven `review:skip` / `review:sanity` / `review:full` modes and the planned flip to per-PR
review in `#219` (FP-161).

1. **Routine PRs** merge on green required CI, coverage floors, repository guards, differential
   cached vs. full-recompute layer digests, and `work-verifier` (`pass`). No `review:*` label is
   applied and no CodeRabbit review is requested.
2. **Gate issues** are the last open `work-unit` issue of a `phase:*` label or a milestone (or an
   issue labelled `review:gate`). The gate issue's branch is opened twice: once against `main`
   (closes the issue) and once, review-only, against `review-base/<gate>`, a branch at the newest
   `reviewed/*` tag, so CodeRabbit sees every change merged since the previous gate. The first
   base is `reviewed/baseline` at `60024a4`, the last commit of the former rhythm.
3. **`.coderabbit.yaml`:** `auto_review.enabled: false` (nothing is reviewed automatically),
   `base_branches: ["^review-base/.*"]`, `profile: chill`, `request_changes_workflow: true`; the
   Orchestrator requests `@coderabbitai full review`, then `@coderabbitai review` after each fix
   push.
4. **Close out:** the gate issue's PR merges only once CodeRabbit approves every review PR of the
   gate; the review PRs close unmerged; the merge commit is tagged `reviewed/<gate>`.
5. **Granularity:** phases (`phase:*`), not only milestones, because a cumulative M1 diff
   (about 140 issues) exceeds what one CodeRabbit pass can review; oversized gate diffs are split
   into area review PRs.
6. **`#219` (FP-161)** keeps its cohesive M1 audit as a `review:gate` issue but no longer flips
   `.coderabbit.yaml` to per-PR review. The retired `review:*` labels on open issues, and review-mode
   notes in their bodies, are ignored.

**Rationale:** Per-PR CodeRabbit rounds were the main source of fix loops on unit PRs, while unit
correctness is already gated by Implementer and Independent Tester tests, coverage floors,
guards, differential checks and the verifier. A cumulative review at each phase and milestone
keeps review where it pays off (cross-cutting drift, duplicated logic, contract and boundary
defects) and matches the gate-only review rhythm of the owner's other projects. CodeRabbit stays
the sole code reviewer (R9); only its timing changes.

**Alternatives:**
- Keep `review:sanity` on contract and core-loop PRs: rejected by the owner's direction that
  review happens only at phase and milestone completion; contracts are still protected by R2 and
  reviewed at the first gate after they land.
- Review on the gate issue's own PR against `main`: rejected because CodeRabbit would see only that
  issue's diff, not the phase's cumulative changes.
- Milestone-only gates: rejected for diff size (point 5).

## Process — Manual testing waits for final acceptance

**Decision:** At the owner's direction (2026-11-25), no session waits for a person to test
anything by hand. Agents automate what they can, record anything only a person can check in the
item's DECISIONS.md entry under "Manual check at final acceptance", and continue. A pending manual
check never holds a unit, a PR merge, the work-verifier's verdict, a gate, or a run (PROCESS.md
P8 "Manual testing"; `work-issue` "Never").

**Rationale:** The owner reserves manual testing for one pass once everything is complete, so the
run stays unattended.

**Alternatives:**
- Pause a unit until the owner tests it by hand: rejected by the owner's direction.
## #57 FP-055 tester — order of `Resolution::ignored` and of resolved devotion words

**Decision.** The interpreter spec tests (`crates/fineprint-core/tests/spec_interp_*.rs`) assert
that `Resolution::ignored` lists the impossible parts in the order of the input they come from
(e.g. a `BOUND` list `[O2, unknown player, O1 gone, P2, O77 gone]` gives items `[O2, P2]` and
ignored `[Gone(unknown), Gone(O1), Gone(O77)]`), and that `DEVOTION` passes the resolved colour
words to `InterpEnv::devotion` in the order they are written in the attribute.

**Rationale.** The brief fixes the order of `items` ("items in this order") and of
`partition_possible` (input order, CR 101.3) but not the order of `ignored` or of the word list;
input order is the only order that needs no extra key, matches how `items` and
`partition_possible` behave, and keeps the report of ignored parts deterministic (R3).

**Alternatives.** Sorting ignored entries (rejected: `Ignored` has no ordering in the pinned
API); not asserting the order (rejected: an unpinned order would let a hash-ordered or
environment-ordered result through, against R3).

## #57 FP-055 — Interpreter API and InterpEnv boundary

**Decision.** `fineprint_core::interp` exposes exactly the API pinned by the brief:
`Referent`, `ChosenValue`, `ObjectView` (+ `ObjectView::new`), `PlayerStat`, the `InterpEnv`
trait, `EvalContext` (+ `EvalContext::new`), `Ignored`, `Resolution<T>` (with `Default` for any
`T`), `Partition<T>`, `#[non_exhaustive] InterpError`, and the functions `resolve_reference`,
`resolve_players`, `select_objects`, `eval_predicate_object`, `eval_predicate_player`,
`eval_value`, `compare` and `partition_possible`. Additions: `Display` and `std::error::Error`
for `InterpError`. Submodules (`types`, `resolve`, `expr`) are private and re-exported from
`interp/mod.rs`. The game is read only through `InterpEnv`; every object is read as an
already-layer-evaluated `ObjectView`. `InterpEnv::attached_to(object)` returns
`Option<Referent>` (the object or player `object` is attached to), not `Option<ObjectId>`,
because the engine can attach an Aura or similar to a player (`AttachTarget::Player`); this is
the one change to the pinned API, made in PR review round 1. `crates/fineprint-core/src/lib.rs`
received one wiring edit (a doc line plus `pub mod interp;`) outside the Implementer's owned
paths, authorised by the classifier's brief for #57. `InterpEnv::defending_player(attacker)` is
the defending player an attacker's abilities refer to (CR 508.5): the player it is attacking
or the controller of the planeswalker / protector of the battle it is attacking and, once it is
no longer attacking (removed from combat, CR 506.4), the player it was attacking just before;
`None` only for an object that never attacked this combat. Retaining the last-attacked player
when a creature is removed from combat is the integration unit's duty, tracked by the follow-up
issue #221 FP-162; the interpreter does not check whether the source is currently attacking.
Likewise `InterpEnv::objects_in` and `object` must exclude phased-out permanents (a phased-out
permanent is treated as though it does not exist, CR 702.26b, while its zone is unchanged,
CR 702.26d); `lki` is unaffected. The integration unit's `InterpEnv` must do that filtering: the
interpreter relies on it and has no phased-out concept of its own (a phased-out target thus
resolves as `Gone`, derived from CR 702.26b with CR 608.2b).

**Rationale.** `GameState` has no zones, permanents, stack or wired history ledger yet (#18
FP-017), and #31 FP-030 puts real-state environments in the integration unit; an injected trait
(like `TriggerEnv` and `LayerEnvironment`) lets the expression layer be complete and tested now
and keeps it a pure function of the environment and the context (REQ-CORE-006). Evaluated
numeric `power`/`toughness`/`mana_value` are carried by the view because `Characteristics`
holds placeholder JSON wrappers for P/T (#32 FP-031).

**Alternatives.** Reading `GameState` directly (rejected: the data is not there and the
integration belongs to a later unit); passing raw `Characteristics` and evaluating layers inside
the interpreter (rejected: layers are the layer system's job, and the CDA path is #186 FP-146).

## #57 FP-055 — Interpretations where the brief was silent

**Decision.** The brief's marked decisions, as implemented: `TEAMMATES` is the empty list (no
team formats yet); the filter-selector default zone is the battlefield; `ENCHANTED`, `EQUIPPED`
and `ATTACHED_TO` are the same lookup (`InterpEnv::attached_to(source)`) and may name an object
or a player host (a player host is checked against `env.players()`, an object host with
`env.object`, either becomes `Ignored::Gone` when absent); `REMEMBERED(key)`
resolves only a remembered `ChoiceValue::Player`; `CONTROLS` is "some player in the set controls
some selected object"; `PLAYER_HAS_MAX_SPEED` is true iff the resolved set is non-empty and all
have max speed, independent of the subject; division by zero is `InterpError::DivisionByZero`
(malformed, not an impossible instruction); `MEMORY(key)` is the `Number` or the `Mode` index,
otherwise 0. Further interpretations made here:
1. Every player referent (CONTROLLER, OWNER, ACTIVE_PLAYER, DEFENDING_PLAYER, TARGET, CHOSEN,
   BOUND, LINKED, REMEMBERED, THAT_PLAYER) is checked against `env.players()` and becomes
   `Ignored::Gone(Player)` if absent; every object referent except SELF/SOURCE_LKI, including
   each member of a `BOUND` or `LINKED` list, is checked with `env.object(id)`.
2. For a player subject, object-only predicates (`HAS_CARD_TYPE`, `HAS_COLOR`, `HAS_COUNTER`
   with a comparison, `MANA_VALUE`, `POWER`, `TOUGHNESS`, `CONTROLLED_BY`, `OWNED_BY`) return
   false without evaluating their words, values or player selectors (so they cannot error).
3. For an object subject, `POWER`/`TOUGHNESS` evaluate the right-hand value before looking at the
   subject's P/T, so a malformed value errors the same way whether or not the subject has P/T;
   a subject without P/T is then false.
4. `CONTROLS` reads each selected object's controller from `env.object(id)` (current
   information; an object without a controller controls nothing).
5. `resolve_players` / `select_objects` reference form: the ignored entries follow the input
   order of the underlying reference's referents: each gone referent's `Gone` entry and each
   wrong-kind referent's `Unresolved(ref)` entry appear where that referent stands in the list
   (`items` also in input order).
6. Selector `count`: 0 or less keeps none; a positive count larger than `usize::MAX` (32-bit or
   wasm32 targets) keeps all; ids `objects_in` reports that `env.object`
   does not know are skipped silently (no `Ignored` entry: they are not part of the instruction).
7. `SPELLS_CAST_THIS_TURN` with a condition and no player counts every entry of
   `env.spells_cast_this_turn()` whose view passes the condition, with no filter on the caster
   (so a caster no longer in the game still counts), consistent with the unconditioned ledger
   total. With a player selector the caster must be in the resolved player set.
8. `ATTR` and `DISTINCT` read objects with `view_of` (current information, then LKI), except
   `ATTR(SOURCE_LKI, attr)`, which reads the source's captured last known information first
   and falls back to its current information (CR 608.2h, CR 113.7a, CR 608.2b);
   `DISTINCT ... COLOR` counts only the five colour bits.
9. `MANA_SPENT_TO_CAST` resolves its reference with `resolve_reference`; both `SELF` and
   `SOURCE_LKI` can resolve an object no longer present (`SELF` through current-then-last-known
   information, `SOURCE_LKI` through its captured view, CR 608.2h); every other reference
   requires the object to be present. It sums `env.mana_spent_to_cast` over the Object items.
10. `DEFENDING_PLAYER`: a source that never attacked gives `None` (the reference is
   `Unresolved`; the CR is silent, a modelling choice); a retained defending player who has left
   the game gives `Ignored::Gone(Player)`. Recommendation (not a CR claim): an attacker whose
   planeswalker or battle was removed from combat while it was still attacking (permanent module
   `Attacking(None)`, CR 506.4c) should also keep its last defending player.
11. A player selector used by a HISTORY query is a set of distinct players: each matching player
   (and each cast) is counted once, however often the selector names the player.

**Rationale.** Each choice follows the brief's tables literally where they speak, keeps
evaluation integer-only, side-effect-free and independent of iteration order (R3), and keeps the
CR 101.3 split (impossible instruction = ignored part; malformed expression = error).

**Alternatives.** Evaluating object-only predicates' operands for player subjects (rejected: the
brief says "false", and an error there would make a player-subject test depend on unrelated
data); treating an unknown referent player as resolved (rejected: CR 101.3 says the impossible
part is ignored); counting only casters in `env.players()` for conditioned spell queries with no
player (rejected in review round 1: it disagreed with the unconditioned ledger total, which
counts every caster).

## #57 FP-055 — Deferred

**Decision.** Not built in this unit: the `InterpEnv` implementation on the real game state
(integration unit); engine retention of the last-attacked player in the permanent combat state
and the `InterpEnv` implementation on the real state, both owned by #221 FP-162;
spec-generated YES_NO text (#28); non-literal CDA evaluation through the layer environment
(#186 FP-146).

**Rationale.** Outside issue #57's deliverables.

**Alternatives.** None considered.

## #43 FP-042 — New SbaEnvironment seams for CR 704.5v-704.5aa

**Decision.** Five defaulted methods are added to `SbaEnvironment` (additive; every existing
implementor compiles unchanged), each with a matching public field of `StoredSbaEnvironment`:
`battle_protector(battle) -> Option<PlayerId>` default `None` (`battle_protectors`),
`can_be_protector(battle, player) -> bool` default `true` (`illegal_protectors`, the illegal
pairs), `battle_ability_pending(battle) -> bool` default `false` (`battle_pending`; any triggered
ability of the battle waiting to be put on the stack or on the stack), `attachment_timestamp(
attachment) -> u64` default `0`, larger = more recent (`attachment_since`, missing = 0) and
`set_battle_protector(battle, protector)` default no-op (writes `battle_protectors`).
`perform_all` calls `set_battle_protector` once per chosen protector, ascending by battle, only
after the pass succeeded. `SbaOutcome` gains `protectors_chosen` (ascending by battle) and
`speed_set` (ascending); `SbaAction` gains `ChooseProtector` and `SetSpeed`. Defense is the number
of `defense` counters from `LayerEnvironment::counters` (CR 310.4c; none = 0); Siege, Role and
start your engines! are read from the evaluated characteristics; whether a battle is attacked is
read from `Permanents` combat state (`Attacking(Some(AttackTarget::Permanent(b)))`). Choosing a
protector and setting a speed count as performed (CR 704.3), so the check repeats and the next
pass finds nothing.

**Rationale.** The state model holds no protector designation, no trigger-source tracking and no
Aura attachment timestamp yet. Following "#41 FP-040 — The SbaEnvironment seam" and "#42 FP-041
— New SbaEnvironment seams for CR 704.5k-704.5t", facts the state does not hold come from a
caller seam, every answer a pure function of caller state (REQ-CORE-001). A protector choice is a
change of the game state, so a pass that only chose a protector must be reported as performed.

**Alternatives.** A protector designation on `Permanents` (rejected: outside the owned paths,
R1/R2); not counting a protector choice as performed (rejected: the heartbeat would not repeat
the check after a state change).

## #43 FP-042 — 704.5x and 704.5y exempt a battle being attacked

**Decision.** Both CR 704.5x and CR 704.5y skip a battle that a creature is attacking
(`Attacking(Some(AttackTarget::Permanent(battle)))` on any permanent); an attacker whose battle
was removed from combat (`Attacking(None)`, CR 506.4c) attacks nothing and exempts nothing. A
protector who left the game or has `LOST_THE_GAME` is no protector (704.5x, not 704.5y).

**Rationale.** CR 310.11 states the state-based action for both cases with "a battle that isn't
being attacked"; the CR 704.5y text has no such clause, but 310.11 is the rule it summarises
("See rule 310"). Changing the protector of an attacked battle would also remove it from combat
(CR 506.4), which the rules avoid by the exemption.

**Alternatives.** Exempting only 704.5x (rejected: contradicts 310.11).

## #43 FP-042 — Meaning of "can't be its protector"

**Decision.** A protector in the game "can't be its protector" (CR 704.5y) when it is not an
appropriate choice for the battle's type (a Siege: not an opponent of the battle's current,
evaluated controller, CR 310.12a; a battle with no battle type: not its controller, CR 310.9a)
or `can_be_protector(battle, player)` is false (effects, CR 101.2). The candidates of 704.5x and
704.5y are the players in the game for which the same test passes, ascending; with Siege as the
only battle type (CR 205.3q), a battle without the Siege subtype has no battle type. A battle
whose evaluation names no controller has no candidates.

**Rationale.** The CR does not define the phrase; using one legality test for detection and for
candidates guarantees the chosen protector is legal and the repeated check finds nothing (the old
protector is never a candidate again, by construction).

**Alternatives.** Only `can_be_protector` (rejected: a control change would leave a Siege
protected by its own controller, which 310.12a forbids).

## #43 FP-042 — Order of protector requests

**Decision.** All protector requests of one pass are asked before any state change, AFTER the
legend-rule requests of the pass, in APNAP order of the choosing controllers (CR 101.4, the same
`apnap_order` as the legend-rule groups), and within one controller by ascending battle id. A
battle with no candidate gets no request and is put into its owner's graveyard in the one batch.

**Rationale.** CR 101.4 orders the players; the order of one player's simultaneous choices is
free, so ascending id is a deterministic stand-in (as "#42 FP-041 — F3 APNAP then lowest id").
Legend first follows the registry order (704.5j before 704.5x).

**Alternatives.** Interleaving legend and protector requests by player (rejected: no rule asks
for it; it would complicate the plan for no observable rules difference).

## #43 FP-042 — No protector request for a battle at 0 defense

**Decision.** A battle that is also a CR 704.5v or CR 704.5w finding in the same pass gets no
protector request and no `protectors_chosen` entry; its 704.5x/704.5y finding is still reported
in `findings`. It moves once.

**Rationale.** It is put into its owner's graveyard in the same simultaneous event (CR 704.3);
asking a choice whose result is discarded would only add a meaningless decision to the replay.

**Alternatives.** Asking anyway (rejected: no effect on the game).

## #43 FP-042 — Role rule tie-break

**Decision.** CR 704.5z groups phased-in Auras with subtype Role by (attachment target, the
Role's evaluated controller). In each group of two or more, the Role with the greatest
`attachment_timestamp` stays; on a tie the HIGHER ObjectId counts as more recent. Each other
Role goes to its owner's graveyard; a token Role then ceases to exist by 704.5d in the next pass.

**Rationale.** CR 613.7m makes the relative order of simultaneous timestamps the controller's
choice; a deterministic stand-in avoids a new choice in the SBA pass (R3). The higher id is the
object created later, the natural "most recent".

**Alternatives.** Asking the controller (rejected: new choice type and plan for a corner case);
the lower id (rejected: less natural).

## #43 FP-042 — Start your engines! keyword literal and the speed designation

**Decision.** A permanent has start your engines! when its evaluated abilities (layers included)
hold a `KEYWORD` ability whose `keyword` is `"START_YOUR_ENGINES"`, the vocabulary id in
`fineprint-dsl/data/vocab.json`. Speed is the player designation `SPEED = "speed"` with value
`DesignationValue::Int(n)`; no designation is no speed (CR 702.179b). 704.5aa sets `Int(1)` once
per player, after every choice of the pass is answered; it is no event and is reported in
`SbaOutcome::speed_set`.

**Rationale.** The vocabulary id is the stable identifier the ingest pipeline emits for this
keyword (the existing lowercase `indestructible` literal predates it). Player designations are an
open registry holding `Int` (SRS 3.1), like `LOST_THE_GAME`.

**Alternatives.** The lowercase name `"start your engines!"` (rejected: not the vocabulary id);
a new `PlayerState` field (rejected: outside the owned paths).

## #43 FP-042 — Existing 704.5p battle spec test gets defense and a protector

**Decision.** In `spec_sba_attach.rs::req_sba_001_704_5p_battle_attached`, each typeless battle
gets 1 defense counter and its controller as protector, so that only CR 704.5p applies.

**Rationale.** Under CR 704.5w and 704.5x a battle with no defense counters and no protector is
now also put into the graveyard, which made that test's assertions wrong. The fix keeps every
704.5p assertion and models a legal battle (CR 310.4c, CR 310.9a); it is not a weakening (R10).

**Alternatives.** Asserting the extra 704.5w/704.5x findings in that test (rejected: mixes rules,
and the battles would leave); leaving it failing (rejected).

## #43 FP-042 — Where the FP-042 spec tests live

**Decision.** The issue's owned paths list only `src/sba/`; the independent spec tests live in
`crates/fineprint-core/tests/spec_sba_{battle,role,speed}.rs` (FP-041 precedent, DEVELOPMENT.md
`spec_<area>_*.rs`), and the mixed one-pass simultaneity and random-layout property tests sit at
the end of `spec_sba_battle.rs`.

**Rationale.** Independent Tester paths must be disjoint from the Implementer's (P5).

**Alternatives.** A separate `spec_sba_fp042_property.rs` (rejected: no benefit).

## #87 FP-085 — Wiring edit and path grant

**Decision.** Besides the owned `crates/fineprint-ingest/src/import/`, the unit edits only what
the brief grants: one `pub mod import;` line with its doc comment in
`crates/fineprint-ingest/src/lib.rs`; the `fetch` and `import` dispatch arms and the extended
usage text in `crates/fineprint-ingest/src/main.rs`; the new spec test
`crates/fineprint-ingest/tests/spec_ingest_import.rs`; `docs/DEVELOPMENT.md`; this file. No
`Cargo.toml` or `Cargo.lock` change: `serde_json`, `fineprint-base` and `fineprint-dsl` suffice.
The atomic writer `vocab::cli::write_atomic` is reused (called, not modified).
**Rationale.** As in "#15 FP-014 — Wiring edit and path grant".
**Alternatives.** A separate binary (rejected: one toolchain CLI).

## #87 FP-085 — Recomputation duplicated rather than a core dependency

**Decision.** `import/recompute.rs` holds its own small, pure versions of the per-symbol colors,
per-symbol mana value, cost colors, record mana value and color identity, instead of calling
`fineprint-core` (`colors_of_mana_cost`, `symbol_mana_value`, `color_identity`).
**Rationale.** `fineprint-ingest` depends on `fineprint-base` and `fineprint-dsl` only
(REQ-NFR-005). The functions are a few lines each, follow the same per-symbol table, and are
tested on every symbol kind.
**Alternatives.** Depend on core (forbidden); move the functions to dsl (a contract change in a
crate this unit does not own).

## #87 FP-085 — Raw layout, no CR 205 validation, station P/T deferred

**Decision.** The record keeps the Scryfall `layout` string raw (an unrecognized layout is not an
error); the `LayoutBehavior` mapping is REQ-ING-003, a later unit. The type line is split
lexically on the first em dash (U+2014; a hyphen is not a separator) with the fixed supertype
words `Basic Legendary Ongoing Snow World Elite Host`; it is not validated against the CR 205
vocabulary and the vocabulary JSON is not read (`typeLineEmpty` flags a missing or empty line).
`power`, `toughness`, `loyalty` and `defense` stay the source strings; the station-card P/T
attachment is left to the later layout unit. `all_parts` is kept raw (`id`, `component`, `name`)
for `relatedTokens`/`meldPartner`; `oracle_tags` and token records are not imported.
**Rationale.** The brief scopes those steps to later units; keeping the raw hooks lets them work
without re-importing.
**Alternatives.** Validate types now (needs the vocabulary and a policy for unknown words; later
unit).

## #87 FP-085 — curl as the HTTP client, behind an Http trait

**Decision.** `fetch` uses the system `curl` through `std::process::Command` (`--fail --location
--silent --show-error --proto =https --proto-redir =https --max-filesize --max-time`, the
`User-Agent` and `Accept` headers), behind the `Http` trait (`get_json`, `download_to_file`);
tests inject a fake. A missing curl is exit 2 with a message.
**Rationale.** Same choice as "#6 FP-005" for `cargo xtask cr fetch`: no TLS in the standard
library, no new dependency (the brief forbids `Cargo.toml` changes), and curl applies the proxy
and CA settings of the environment. Time limits: 60 s for the metadata, 3600 s for a bulk file;
the metadata response is capped at 1 MiB.
**Alternatives.** An HTTP crate (a dependency change).

## #87 FP-085 — 24-hour cache, clock injection and cache metadata

**Decision.** Each group directory (`scryfall/`, `mtgjson/`) has a `source.json`
`{"formatVersion":1,"files":{<file>:{sourceUrl, updatedAt, sha256, byteCount,
fetchedAtEpochSeconds}}}`. A file is fresh when its entry exists, the file exists, and
`fetchedAt <= now < fetchedAt + 86400`; a future `fetchedAt` is stale. If every needed Scryfall
file is fresh, no request is made (not even the bulk-data metadata request); otherwise one
metadata request is made and only the stale files are downloaded. `fetch` takes `now` as a
parameter; only `cli::run_fetch` reads the system clock. All stale files of a group are
downloaded to `.part` files and validated before any is renamed, so a failure leaves the group
unchanged. The MTGJSON `updatedAt` is the file's `meta.date` (there is no bulk-data entry).
`download_uri` must be `https://` on `scryfall.com`, `*.scryfall.com` or `*.scryfall.io` with
lowercase labels, no credentials, no port, no whitespace or backslash.
**Rationale.** REQ-ING-001 (cache at least 24 hours, no crawling), R7 (no wall clock in tests),
R12 (the URI is server data).
**Alternatives.** One `source.json` for the whole cache (rejected: the groups are fetched
independently).

## #87 FP-085 — Record-level mana value per layout and the cmc check

**Decision.** One face: that face's cost. `split`: the sum of all faces (CR 202.3d, 709.4b).
`adventure`: the main face (CR 716.4); `flip`: the normal face (CR 710.2). Every other layout
with several faces (`transform`, `modal_dfc` front face per CR 712.6a, `meld`, `reversible_card`,
unknown layouts): the front face (CR 202.3b; melded permanents,
CR 202.3c, are not computed at import). `cmc` matches when it is a finite JSON number, `cmc * 2`
is exactly a whole number, and it equals `manaValue * 2`; a value such as 2.9 is a mismatch, not
rounded (the brief asks for "no tolerance beyond exact representation"). A missing, non-numeric
or non-finite `cmc` is `cmcMissing`. `cmc` is never stored.
**Rationale.** The brief's per-layout rule; exactness follows the "no tolerance" clause, which a
plain `round()` would violate.
**Alternatives.** `round(cmc * 2)` alone (accepts 2.9 as 3).

## #87 FP-085 — Color indicator and cost colors

**Decision.** A face's `computedColors` is the union of the colors of its mana cost and of its
color indicator (CR 105.2, 202.2e, 204.2); where both are present this is an inference (no CR
example covers both), matching `fineprint-core`'s `base_colors`. This supersedes the brief's
"indicator replaces cost" wording on the rules-researcher's ruling. Card-level colors for the
cross-check: `split` = union of the halves (CR 709.4, 709.4b); `adventure` = main face (CR 716.4);
`flip` = normal face (CR 710.2); `transform`, `modal_dfc` and others = front face (CR 712.6a).
`colorsMismatch` is raised when (a) a face's own `colors` differs from that face's computed
colors, or (b) a present top-level `colors` differs from the card-level colors of its layout. An
absent top-level `colors` never raises it (transform and modal_dfc sources carry none). Source
values never alter computed values. `colorIdentity` is the union over every face, an Adventure's
face and a double-faced card's back included (CR 903.4, 903.4d), of the computed colors and the
mana symbols of the rules text outside reminder text.
**Rationale.** The rules-researcher's rulings on CR 105.2, 202.2e, 204.2, 709.4b, 716.4, 710.2,
712.6a and 903.4d.
**Alternatives.** Indicator replaces cost (the earlier behaviour, rejected: contradicts the CR
and `base_colors`); checking only the front face (rejected: spurious mismatches for split cards).

## #87 FP-085 — digitalOnly, empty costs and field types

**Decision.** `digitalOnly` is true when `digital` is true, or `games` is a non-empty array
without `"paper"`. `emptyManaCostUnexpected` is raised for a face with no mana cost (an empty or
absent `mana_cost`) when the layout is `normal` and the face's card types do not contain `Land`;
the record is not rejected. A JSON `null` in an optional field counts as absent; every other
wrong type rejects the record with `fieldType:<field>` (`card_faces.<field>` for a face), an
unknown color letter with `unknownColor:<field>`, a missing `name` or `layout` with
`missingField:<field>`, an unparseable cost with `manaCostParse`, a missing `oracle_id` with
`notGameplayObject`, a non-UUID-shaped one with `oracleIdShape`.
**Rationale.** REQ-ING-002 and R12: reasons name fields only, never input text.
**Alternatives.** Rejecting nulls (Scryfall does not emit them; accepting them is harmless).

## #87 FP-085 — Output shapes, default paths and --strict

**Decision.** `<cache>/normalized/cards.json` (default `--out`) has the members `formatVersion`
(1), `sources` (`oracleCards`, `rulings`: `{updatedAt, sha256, byteCount}` or null), `records`
(one `NormalizedCardRecord` per line, sorted by `oracle_id`), `rulings` (by `oracle_id`, source
order), `rejected` (`{index, oracleId, reason}`) and `warnings` (fixed strings such as
`sourceHashMismatch:oracleCards`, `sourceMetadataMissing`, `rulingsUnknownOracleId:<n>`,
`rulingsInvalid:<n>`), in that order. Inside a record the members are sorted by name
(`serde_json`'s map without `preserve_order`), a fixed order. The sha256 recorded is the one of
the bytes read; a difference from `source.json` is a warning. The MTGJSON report (only at an
explicit `--mtgjson-report` path or stdout) lists per `oracle_id` the disagreeing fields with
numbers and color letters printed and only the kind (`string`, `list(n)`, `absent`, ...) of other
values, plus `onlyInScryfall`/`onlyInMtgjson` ids, `matched`, `agreed` and `invalidEntries`.
`import` exits 0 despite rejections and diagnostics; `--strict` exits 1 when at least one record
was rejected or has a diagnostic (warnings do not count). Bulk arrays are split into elements by
a small scanner and each element parsed on its own, so memory is bounded by the largest element;
inputs above 4 GiB are refused before reading.
**Rationale.** REQ-ING-001 (provenance), R3 (byte-identical output), R12, REQ-NFR-004 (card data
only in the git-ignored cache or an explicit path).
**Alternatives.** One `serde_json::Value` for the whole file (memory grows with the file).

## #88 FP-086 — Battle layout maps to DFC_NONMODAL (D1)

**Decision.** Scryfall `battle` maps to `LayoutBehavior::DfcNonmodal`; one face gives `[Main]`,
two faces give `[Main, Back]`.
**Rationale.** `LayoutBehavior` has no Battle variant and battle cards are double-faced
(SRS §4.2 row "same-named behavior" has no match; the DFC layout is the nearest).
**Alternatives.** A new `Battle` variant (a dsl contract change, R2); treating it as unknown.

## #88 FP-086 — Emblem layout maps to TOKEN with token_kind Emblem (D2)

**Decision.** Scryfall `emblem` maps to `LayoutBehavior::Token` with `token_kind = Emblem`;
`token` and `double_faced_token` give `Token` / `DfcToken` with `token_kind = Token`.
**Rationale.** There is no Emblem variant, and the SRS row says "Token or emblem records".
**Alternatives.** A new `Emblem` variant (contract change, R2).

## #88 FP-086 — Meld record shape (D3)

**Decision.** A meld record is one source object with exactly one face `[Main]`; the melded
result is a separate record. `FaceKind::MeldResult` is not assigned by this unit. Partners are
`all_parts` entries with component `meld_part` or `meld_result` and a name differing from the
record's own, sorted and deduplicated; none gives `MeldPartnerMissing`.
**Rationale.** SRS §4.2: meld cards are not DFCs (CR 713.1).
**Alternatives.** Two faces including the result face.

## #88 FP-086 — UNSUPPORTED is a support status, not a layout

**Decision.** `UNSUPPORTED` is carried as `SupportStatus::Unsupported` by `LayoutError` and
`UnsupportedReason`; no layout variant is added and the dsl crate is untouched. Layouts the SRS
table does not list (for example `attraction`, `conspiracy`) are unknown layouts
(`UnknownLayout`). `augment` and `host` are always Unsupported (`ext.unset`; no extension flag
exists yet). `Verified` is never produced (R11).
**Rationale.** `LayoutBehavior` has no Unsupported variant; contracts change only by R2.
**Alternatives.** Adding a variant (contract change).

## #88 FP-086 — Wiring edit and path grant

**Decision.** The only edit outside `crates/fineprint-ingest/src/layout/` is the single
`pub mod layout;` line with its doc comment in `crates/fineprint-ingest/src/lib.rs`, plus this
file. No `Cargo.toml`/`Cargo.lock` change, no re-exports; `import/**` is untouched.
**Rationale.** Same pattern as "#87 FP-085 — Wiring edit and path grant".
**Alternatives.** None.

## #89 FP-087 — Wiring edit and path grant

**Decision.** The only edit outside `crates/fineprint-ingest/src/tokenize/` is the single
`pub mod tokenize;` line with its doc comment in `crates/fineprint-ingest/src/lib.rs`, plus
these appended entries. No `Cargo.toml`/`Cargo.lock` change, no re-exports from `lib.rs`; the
module re-exports only its own config limits (`MAX_CONFIG_BYTES` and friends) from its private
`config` submodule.
**Rationale.** Same pattern as "#87 FP-085 — Wiring edit and path grant" and "#88 FP-086 —
Wiring edit and path grant".
**Alternatives.** None.

## #89 FP-087 — API shape

**Decision.** `tokenize_face(&TokenizeInput, &RulesVocabulary, &TokenizerConfig) ->
TokenizedFace` and `tokenize_record(&NormalizedCardRecord, &RulesVocabulary, &TokenizerConfig)
-> Vec<TokenizedFace>` (one per face, config key `card_def_id`). The vocabulary is an explicit
argument so the output is a pure function of (text, face name and subtypes, vocabulary,
config). `TokenizeInput { card_key, face_name, subtypes, text }` (`TokenizeInput::from_face`).
`TokenizedFace { display, truncated, abilities, sections }`; `TokenizedAbility { original, kind,
anchor, section, body }`; `TokenizedText { original, text, tokens, spans }` (spans are byte
ranges of `text`, parallel to `tokens`, contiguous and covering `text`). `AbilityKind` is
`Plain`, `Anchored` (anchor text in `anchor`), `Modal { modes: Vec<TokenizedText> }` (header in
`body`; `anchor` may also be set) or `Chapter { chapters: Vec<u32> }`. Station symbols, level
bands and class levels are `Section`s (`SectionKind::Station { threshold }`,
`LevelBand { min, max }`, `ClassLevel { level, cost }`, plus `header` and the striation's raw
`pt` line); each ability's `section: Option<usize>` indexes `TokenizedFace::sections`. Helpers:
`normalize_text`, `strip_reminder_text`, `render_tokens(&TextTokenStream, placeholder)` with
`SELF_REF_PLACEHOLDER = "~"`. All types derive `Debug`, `Clone`, `PartialEq`, `Eq`.
**Rationale.** Sections make group membership explicit for the later parser without nesting
abilities; a header line with nothing after it (`N+ |`, `LEVEL a-b`, `{c}: Level N —`) opens a
section and yields no ability. Spans let the invariant "tokens reproduce the stripped text" be
checked even though `NameRef{SELF}` carries no text.
**Alternatives.** A nested `Vec` per section; a structural kind per ability (StationLine etc.)
with the following lines left Plain (membership would be implicit).

## #89 FP-087 — Self-reference phrase set

**Decision.** `NameRef{SELF}` (name `None`) for: the normalized face name (case-sensitive,
whole-word), configured `shortNames` (case-sensitive, whole-word; CR 201.5c), every `~`, and
"this <noun>" with "this" and the noun matched case-insensitively, where the noun is any card
type of `RulesVocabulary::card_types`, any of the face's own subtypes (for example "this
Equipment"), or one of `spell`, `permanent`, `card`, `token`. Name patterns are matched first
(leftmost, longest, self before other); "this" phrases and words are matched only in the gaps.
"this ability" is not a self-reference (it names an ability, not the object).
**Rationale.** SRS §14.5 and REQ-ING-004(b) list the name, "this creature", "this spell" and
"similar phrases"; these are Oracle conventions, not CR text (decision "#37 FP-036").
**Alternatives.** Only card types (would miss "this spell", "this permanent"); all subtypes of
the vocabulary (would turn "this Goblin" on an unrelated card into a self-reference).

## #89 FP-087 — Word value casing

**Decision.** A `Word` token's `value` is the matched source text verbatim (after
normalization), in its source casing ("Red", "red", "RED"); the category says which list it
matched. Matching is case-insensitive and whole-word: a word character is a letter, digit or
`_`, so "Goblins", "Redness" and "red_x" are not marked, while "non-Goblin" and "Goblin's"
mark "Goblin". Plural forms are not marked by this unit. Multi-word creature types ("Time
Lord") match only whole, with a single space. Vocabulary spellings are normalized like the
text (curly apostrophes become `'`). On a collision the priority is self-reference, color
word, basic land type, creature type.
**Rationale.** Keeping the source text makes `render_tokens` reproduce the ability text
exactly (display, round trip); the canonical form is a case-insensitive lookup away.
**Alternatives.** The canonical vocabulary spelling as value (loses the display form);
plural handling (needs an inflection table with no CR basis).

## #89 FP-087 — NUMBER and CARD_NAME not emitted

**Decision.** The tokenizer never emits `NUMBER` or `CARD_NAME` words; numbers and names stay
literal text or `NameRef`s.
**Rationale.** Decision "#37 FP-036 — SRS §14.5 versus the CR": CR 612.2 protects card names
from text-changing effects and no CR rule makes numbers rewritable.
**Alternatives.** Emitting them per SRS §14.5 (contradicts the CR).

## #89 FP-087 — Reminder-text config key and shape

**Decision.** The meaningful-parenthesis exceptions, shortened names and other names are client
configuration: `{"formatVersion":1,"cards":{"<oracle_id>":{"keepParentheses":["(...)"],
"shortNames":[...],"otherNames":[...]}}}`, loaded by `TokenizerConfig::from_json` into
`BTreeMap`/`BTreeSet`s. The key is the record's `oracle_id` (`card_def_id`), which is stable
across printings and unique, unlike a face name. A kept span is the exact top-level
parenthesized text (parentheses included, normalized); inner parentheses of a kept span are not
stripped. The loader refuses documents above 4 MiB, malformed JSON (including `serde_json`'s
recursion limit), unknown members, wrong types, `formatVersion` other than 1, empty, multi-line
or over-512-byte strings, lists over 256 entries, and `keepParentheses` entries not starting with
`(` and ending with `)`. Duplicate JSON keys keep the last value. The repository holds no card
list; tests use invented keys.
**Rationale.** REQ-ING-004(a) asks for a detector that knows the exceptions; CR 207.2a does not
make parentheses rules text, so the exceptions are data a client supplies (R5, R12).
**Alternatives.** Keying by face name (collides across cards, changes with translations);
a regex per card (harder to validate).

## #89 FP-087 — Line-structure lexical conventions

**Decision.** On each reminder-stripped, trimmed line, in this order: station `N+ | text`,
`N+|text`, `N+ |` (1–6 digits); level band `LEVEL a-b` / `LEVEL a+` (keyword
case-insensitive, 1–6 digits, `a <= b`, the en dash already normalized to `-`); class level
`<cost>: Level N — <text>` (non-empty cost without an em dash, keyword case-insensitive,
1–6 digits, N >= 1, em dash required). Within a station or level-band section the first bare
P/T line (`[0-9X*+-]{1,8}/[0-9X*+-]{1,8}`) becomes `Section::pt`; elsewhere it is a plain line.
When the first section header of a face is a class level, an implicit `ClassLevel { level: 1,
cost: None }` section holds the lines above it (CR 717.3); stations and bands have no implicit
section (CR 722.4, 711.4). For the body of a line: chapter `<numerals> — <effect>` (canonical
upper-case Roman numerals 1–99, comma-separated, strictly increasing, non-empty effect);
otherwise an anchor `<Prefix> — <text>` (em dash with a space on each side; prefix starts
upper-case, only letters, spaces, `'` and `-`, at most 6 words and 64 bytes, and not made only
of Roman numeral letters); then a modal header: the (post-anchor) text contains the word
"choose" in any case and is followed by option lines. Bullet lines (`•`) are options only when
the header ends with an em dash; pawprint-led lines (`{P}`) are options after any "choose"
header, and keep their pawprints in the mode text (they are the mode's cost, CR 700.2i). A
bullet's mode text is the text after the bullet, cost included (CR 700.2h). Everything else is
Plain. The brief named headers that begin with "Choose"; headers such as "When ... enters,
choose one —" are accepted too.
**Rationale.** SRS §16 and the issue give these shapes; they are data-format conventions of
the bulk text. Triggered modal headers are common and splitting their bullets into separate
abilities would lose the mode grouping (CR 700.2).
**Alternatives.** Only headers beginning "Choose" (strict reading of the brief); anchors
without spaces around the dash (would capture keyword costs such as "Ward—").

## #89 FP-087 — Limits and noncharacter handling

**Decision.** Normalization replaces Unicode noncharacters with U+FFFD, so
`TextTokenStream::new` cannot fail on tokenizer output (a failure would still yield an empty
stream, never a panic). Raw text above `MAX_TEXT_BYTES` (65,536) is cut at the last char
boundary within the limit and `TokenizedFace::truncated` is set. Parenthesis matching is a
single stack pass (linear on `((((…`), lexicon lookup at each word start is bounded by the
longest phrase length, and every numeric field is capped at 6 digits or the chapter bound.
Normalization also maps tab, U+2028 and U+2029 to a space, U+2010 and U+2011 to `-`, and strips
any run of leading BOMs (so it is idempotent).
**Rationale.** R12 and REQ-ING-004 ask for total, bounded parsing; real Oracle text is far
below the cap.
**Alternatives.** Rejecting such input with an error (the toolchain would lose the whole card).

## #159 FP-140 — Card wrappers validated against the real AST types

**Decision.** `card::ValueExpressionAST(Value)`, `card::TextTokenStream(Vec<Value>)`,
`card::AbilityAST(Value)` and `card::PartnerAbilityDSL(Value)` keep their tuple shape, `pub`
field, derives and `#[serde(transparent)]` plain serde (permissive, unchecked building blocks).
The issue deliverable "become (or re-export) the real types" is met by validated typed views at
the two checked boundaries: each wrapper gets `to_real(&self) -> Result<real type, String>`
(into `expr::ValueExpressionAST`, `text::TextTokenStream`, `ability::AbilityAST`), and
`CardDefinitionDSL::validate_asts` checks every position (`faces[i].power`,
`faces[i].toughness`, `faces[i].text`, `faces[i].abilities[j]`,
`commanderEligibility.partnerAbilities[j]`) with a located message. `seal` runs the check on
the normalised, depth-checked value before hashing (failure: `CanonicalError::Serialize`);
`from_json` runs it after the lossless-decode step and before the hash comparison (failure:
`CardDefinitionError::Schema`). A position passes only if the strict decoder of the real type
accepts it, read from a borrowed `&Value`, and the decoded value re-encodes (through
`canonical::to_checked_value`) to exactly the original JSON, so each valid AST has one spelling
and the check never changes canonical JSON or `definitionHash`. `to_canonical_json`,
`compute_hash`, `verify_hash` and plain serde do not validate. The error type of `to_real` is
`String` (the decoder's message), so no new public error type or `CardDefinitionError` variant
is added. This supersedes the open parts of "#11 FP-010 — Placeholder AST types" and "#12
FP-011 — FP-010 placeholders stay in card/". Follow-up (the Orchestrator files it): migrate
fineprint-core (about 45 files in src, tests and benches that build or decode the wrappers with
deliberately fake content, for example `layers/text.rs` and `layouts/facedown.rs`) to the real
types, then swap the field types.
**Rationale.** Swapping the Rust field types now would break fineprint-core and
fineprint-ingest, which construct these wrappers directly; validating at `seal` and `from_json`
gives the acceptance criteria (invalid content rejected on load and seal, round trip and
canonical form with the real types) with an additive public API only.
**Alternatives.** Replace the field types with the real types in this unit (rejected: breaks
the core crate, out of the owned paths); validate inside the wrappers' `Deserialize` (rejected:
changes plain serde, which core relies on for placeholder content).

## #159 FP-140 — PartnerAbilityDSL is a KEYWORD ability

**Decision.** A `commanderEligibility.partnerAbilities` entry must decode as an
`ability::AbilityAST` whose `kind()` is `"KEYWORD"`; `PartnerAbilityDSL::to_real` and
`validate_asts` reject any other kind with a message naming the kind found.
**Rationale.** SRS §14.2 leaves `PartnerAbilityDSL` undefined; the SRS commander section lists
the partner variants (partner, partner—[text], partner with [name], choose a Background,
Doctor's companion) as keyword abilities, so the keyword instance form (`keyword` plus opaque
`params`) carries them.
**Alternatives.** Any ability kind (rejected: a partner entry that is a spell or trigger has no
meaning); a dedicated partner schema (rejected: not in the SRS; can be added later as an
additive contract change).

## #159 FP-140 — Effect lists and textSpan not additionally checked

**Decision.** Abilities are validated only through `ability::AbilityAST`'s own decoder. Its
effect lists still hold the strict placeholder `ability::EffectInstructionAST` (required `op`
matching `[A-Z][A-Z0-9_]*`, other members opaque); card/ adds no effect-specific check, so the
card check tightens automatically when #161 (FP-141) swaps in `effect::Instruction`. Fixtures
with a non-empty effect list use a real instruction wire form (`{"op":"POPULATE"}`) so FP-141
does not change their hash; the REPLACEMENT fixture uses the replacement op `{"op":"SKIP"}`. An
ability's `textSpan` is not checked against the length of the face `text`: not enforced.
**Rationale.** #161 owns `src/ability/` and the effect swap; a span-range check is not required
by the contracts of this unit.
**Alternatives.** Decode effect lists as `effect::Instruction` from card/ now (rejected:
duplicates #161's work and would diverge from the ability decoder); enforce `textSpan` ranges
(rejected: no contract requires it yet).

## #159 FP-140 — Null power/toughness and regenerated fixtures

**Decision.** `Some(ValueExpressionAST(Value::Null))` for `power` or `toughness` is no longer
sealable or loadable (a present `null` is not a value expression); plain serde still decodes
`null` as `Some(...)`, unchanged. The fixtures of `crates/fineprint-dsl/src/card/tests.rs` that
held placeholder content were migrated to valid synthetic ASTs (`{"const":2}` →
`{"LITERAL":2}`, `{"kw":"Flying"}` → a LITERAL token, `{"keyword":"FLYING"}` → a KEYWORD
ability, partner placeholders → KEYWORD abilities, random power `null`/`[n,"*"]` → `"X"`/
`{"LITERAL":n}`, random toughness → a `MATH` expression). Tests that reached the canonicaliser's
depth, noncharacter and number-normalisation limits through placeholder JSON now reach them
through valid content (nested `MATH` chains, a noncharacter in a LITERAL token's text,
`{"LITERAL": -0.0 | 1.0 | 1e0}` and KEYWORD `params`); `{"LITERAL": 2.5}` is now asserted to be
refused by `seal` (non-integral, REQ-CORE-006). `card/tests.rs` pins no golden hash literal, so
no literal was regenerated; for the record, the base fixture `sealed()` changed from
`fddf030ec9d6bea9c2f508c8de7b37a2e944c55952d5e3032bd93983d8b4eb44` (placeholder content) to
`ce23dc12dae8b49033785a3832ebece12870f940ce895ab3d01f370b8feee216` (valid real ASTs), because
its AST content changed; the hashing procedure itself is unchanged.
**Rationale.** R10: test intent kept, content made valid; REQ-NFR-006: hashes stay well-defined.
**Alternatives.** Keep `null` power loadable as "no value" (rejected: a second spelling of an
absent member, and not a real value expression).

## #159 FP-140 — Test-path ownership

**Decision.** The Independent spec tests of this unit live in
`crates/fineprint-dsl/tests/spec_dsl_card*.rs` (including the migration of `spec_dsl_card.rs`)
and `crates/fineprint-dsl/tests/spec_dsl_review1.rs`, and belong to the Tester role, a grant the
Orchestrator adds to the path check. The Implementer's unit tests are
`crates/fineprint-dsl/src/card/tests.rs`.
**Rationale.** Same split as "#13 FP-012 — Test-path ownership" (P5 role separation).
**Alternatives.** Implementer-owned integration tests (rejected: no independence).

## #159 FP-140 — Spec test migration and golden hash

**Decision.** The Independent Tester migrated `crates/fineprint-dsl/tests/spec_dsl_card.rs` and
`crates/fineprint-dsl/tests/spec_dsl_review1.rs` from free-form placeholder content to valid
synthetic real ASTs (`{"LITERAL":n}`, `"X"`, `MATH` chains; LITERAL/WORD/NAME_REF tokens with
unique ids; STATIC, ACTIVATED (empty `effect`) and KEYWORD abilities, exotic payloads moved to
KEYWORD `params`; KEYWORD `PARTNER` partner abilities). Regenerated golden hash in
`spec_dsl_card.rs` (`PINNED_HASH` and its copy in `PINNED_SEALED`):
`36337ac7f9d175df1da81a397b1b788a4f2c88ee12dcc1b937584e5ca34f3aa3` →
`e930e89d0aec056f27c5a0bac081553c5435af9dbebf4b13f42d65b294d2b770`, because the pinned minimal
definition's power and toughness changed from the placeholder `1` to `{"LITERAL":1}`; the new
value was computed with `printf %s '<canonical>' | sha256sum` and matches `compute_hash`.
`spec_dsl_review1.rs` pins no hash. Limits formerly reached through placeholder JSON are reached
through valid content (a `MATH` chain of exactly k containers in `power` for the seal/load depth
agreement; a noncharacter in a LITERAL `text` or WORD `value`; `{"LITERAL":-0.0|1.0}` for number
normalisation) or through raw document text (over-deep arrays in `power`/`abilities`, unsafe
integers), since document-level checks run before the AST check; `compute_hash`/
`to_canonical_json` depth tests stay on placeholder content because those do not validate ASTs.
Two brief-table entries are asserted at the canonical level: `{"LITERAL":9007199254740992}` is
`Canonical(UnsafeInteger)` from `from_json` (canonical checks precede the AST check), and `seal`
may report it as `UnsafeInteger` or `Serialize` (both reject; the canonical-level error is the
same check `from_json` applies first). `seal` of `{"LITERAL":1.0}` / `-0.0` is asserted to
normalise to the integer and succeed, matching the loader. The independent tests live in
`crates/fineprint-dsl/tests/spec_dsl_card*.rs` (new: `spec_dsl_card_ast.rs`) and
`spec_dsl_review1.rs` (a path-check grant the Orchestrator adds).
**Rationale.** R10 (intent kept, nothing weakened), REQ-NFR-006 (hash well-defined over single-
spelling real ASTs), FP-010 loader order (canonical → version → schema → AST → hash).
**Alternatives.** Require `Schema`/`Serialize` for the unsafe LITERAL (rejected: contradicts the
documented canonical-first order); keep placeholder content in the migrated files (rejected:
`seal`/`from_json` now refuse it).

## #161 FP-141 — Real effect::Instruction in abilities

**Decision.**
(a) `ability::EffectInstructionAST` is deleted. `SpellAbilityAST.effect`, `ActivatedAbilityAST.effect`,
`TriggeredAbilityAST.effect`, `ModeAST.effect` and `ReplacementOp::Instead.instructions` hold
`crate::effect::Instruction` directly; there is no alias and no re-export from `ability`
(consumers import `fineprint_dsl::effect::Instruction`). The module docs now say instructions are
the real type and that modifications, RULE effects and keyword params remain opaque. The helpers
`check_upper_name`, `check_params`, `rest_params` and `write_params` stay: modifications, RULE
effects and keyword names still use them. This supersedes "#13 FP-012 — EffectInstructionAST is a
placeholder", the placeholder clause of "#13 FP-012 — Rule E5 covers filters and bind keys only"
and "#14 FP-013 — Ability placeholder stays" (the older entries are not edited).

(b) Emptiness is unchanged: a SPELL, ACTIVATED, TRIGGERED or mode `effect` list may be empty;
`INSTEAD.instructions` must be non-empty. Lists nested inside an `effect::Instruction` follow
`effect::`'s own rule (non-empty).

(c) Rule E5 re-check against the real instruction. A real instruction has no flat free-form
member: every member is typed and an unknown member is an error, so `lookBack`, `look_back` or
`LOOK-BACK` directly on an instruction is rejected as unknown. The only positions inside an
instruction that can carry a free-form key are the `filters` and `bind` of a trigger event matcher
(`CREATE_DELAYED_TRIGGER.trigger`, `REFLEXIVE_TRIGGER` has none), and they decode through the
ability `Trigger`, so `check_no_lookback` still applies: a delayed trigger whose matcher has a
look-back `filters` or `bind` key is rejected (tested). `NATIVE_EFFECT.params` is opaque by
contract (REQ-DSL-015): a `lookBack` key there is data and round-trips unchanged (tested); E5 is
not widened to it. `CREATE_CONTINUOUS_EFFECT` carries `StaticEffect`s, whose opaque params are
already outside E5 by "#13 FP-012". `check_no_lookback` semantics are unchanged.

(d) Tests changed (R10): in `ability/tests.rs`, the SRS §14.4.1 example 1 effect became a valid
`DISCARD` (`player` a bound reference, no `chooser`; the placeholder form was not a real
instruction) and its assertion now uses `Instruction::name()`; the §14.4.2 activated example's
`TAP_OR_UNTAP` uses the real member `action` instead of `mode`; the mode, INSTEAD and
`Instead` serialize fixtures use `POPULATE`; the opaque-number, noncharacter and nesting tests
moved their opaque values into `NATIVE_EFFECT.params` (nesting limits shift by one because
`params` is an extra object level: 123 accepted, 124 rejected); the E5 test that said
instruction members are opaque data now asserts that a look-back member on an instruction is
rejected; `req_dsl_005_effect_instruction_placeholder` (it only proved free-form ops and opaque
members are accepted) is replaced by the reject/accept/round-trip, emptiness, Serialize re-check
and E5 tests of REQ-DSL-010; the serialize test no longer builds `EffectInstructionAST`. In
`props.rs` `gen_instr` draws from a fixed list of valid instructions and the random-bytes run
covers `Instruction`.

**Rationale.** REQ-DSL-010 wants abilities to carry real instruction ASTs; reusing the merged
FP-013 decoder gives strict decoding, canonical form and Serialize invariant checks for free, and
keeps card fixtures using `{"op":"POPULATE"}` hashing the same.

**Alternatives.** A type alias or re-export of `Instruction` in `ability` (rejected: two import
paths for one type); widening E5 to `NATIVE_EFFECT.params` (rejected: contradicts REQ-DSL-015
opacity); tightening list emptiness (rejected: not requested, would change the wire contract).

## #161 FP-141 — Spec-test readings after the instruction swap

**Decision.** The Independent Tester's judgement calls in `tests/spec_dsl_ability*.rs`:
(1) the SRS §14.4 examples are tested with real instruction members, as in the unit tests: the
example-1 `DISCARD` `player` is `{"ref":"BOUND","var":"$P"}` with no `chooser`, and the activated
example's `TAP_OR_UNTAP` uses `action`, not `mode`; (2) property templates that need a free-form
name or string inside an instruction use the opaque `CREATE_TOKEN.inline` map, since a typed
instruction has no free-form members; (3) nesting-limit spec tests assert rejection at 70 and
200 nested `IF`s and do not pin the exact boundary (the unit tests pin it); (4) the spec test that
asserted a look-back member on an instruction is opaque data now asserts it is rejected as an
unknown member, and E5 inside a `CREATE_DELAYED_TRIGGER` matcher is asserted through the shared
ability `Trigger` decoder.
**Rationale.** R10: the placeholder content was not a valid real instruction; the assertions keep
their intent against the real type. The exact nesting boundary depends on wrapper depth, which is
an implementation detail for black-box tests.
**Alternatives.** Keep the SRS shorthand and assert rejection (rejected: loses the round-trip
coverage of the examples); pin the exact nesting boundary in spec tests (rejected: couples the
black-box tests to wrapper depth).

## #169 FP-142 — Colorless-hybrid ManaSymbol variant

**Decision.** `{C/W}`, `{C/U}`, `{C/B}`, `{C/R}`, `{C/G}` are represented by a new variant
`ManaSymbol::ColorlessHybrid(Color)`, appended last (CR-001, additive) so existing variant order
and the derived `Ord` are unchanged. The parser accepts only the `C`-first uppercase spelling;
`{W/C}`, `{C/P}`, `{C/C}`, lowercase forms and the like stay `UnknownSymbol`. Mana value is 1
(CR 202.3f: largest component of {C} and {c}); colors are exactly c (CR 107.4e, 202.2d); color
identity picks this up unchanged through `symbol_colors` (CR 903.4). The owned paths were
extended to the two `ManaSymbol` matches in `crates/fineprint-ingest/src/import/recompute.rs`
(`symbol_colors`, `symbol_mana_value`) plus an inline test module there: both matches are
exhaustive, so the workspace does not compile without the new arms. This entry supersedes the
citation in "#21 FP-020 — `{C/W}` is not representable in `ManaSymbol`", which credited "colored
with mana value 1" to CR 107.4e alone: the correct citation is CR 107.4/107.4e for the symbol and
its color, and CR 202.3f for its mana value. The follow-up that entry described has now landed.
**Rationale.** A dedicated variant keeps `GenericHybrid`'s `n >= 1` rule and `{1/W}` semantics
intact, and appending it is the only additive change allowed by R2.
**Alternatives.** Encode `{C/c}` as `GenericHybrid(0, c)` or `GenericHybrid(1, c)` (rejected:
changes the meaning and validity of existing symbols and their canonical text); a generic
`Hybrid`-with-colorless enum for the left half (rejected: changes an existing variant, breaking);
add a wildcard arm in ingest instead of explicit arms (rejected: would silently mis-handle future
symbols).

## Gate 02-state-objects — Review baseline after the history rewrite, and tags this environment cannot push

**Decision.** The first gate review (phase `02-state-objects`, review PR #236) uses
`review-base/02-state-objects` at `16ca856` (the PR #220 merge on `main`) as its base. PROCESS.md P6
and the `work-issue` skill name `60024a4` as the `reviewed/baseline` commit, but `main` no longer
contains it after a history rewrite; `16ca856` is the same merge with an identical tree
(`5eb310e8…`). The `reviewed/baseline` tag should point at `16ca856`. The Claude Code web session's
proxy refuses tag pushes (git and REST), so the `reviewed/baseline`, `reviewed/02-state-objects` and
`phase-02-green` tags are created by a person (P8 item in `docs/HUMAN_QUEUE.md`); until
`reviewed/baseline` exists, a later gate that finds no `reviewed/*` tag uses `16ca856` as its base.

**Rationale.** The tree-identical commit preserves the intended review range (everything since the
former per-PR rhythm ended); P6 forbids pausing for a person, so the review proceeds on the branch,
which the proxy does allow, and only the tags wait.

**Alternatives.** Basing the review on the root commit (rejected: re-reviews everything already
reviewed per PR); skipping the gate until tags can be pushed (rejected: P6 and R8).

## Gate 02-state-objects F3 — Battle protector candidates and OPPONENTS are team-unaware until #134

**Decision.** In the supported scope (no team model) a player's opponents are every other player in the game. The battle-protector candidate test (`appropriate_protector` in `crates/fineprint-core/src/sba/registry.rs`, CR 310.12a "Only an opponent of a Siege's controller can be its protector") and the interpreter's `OPPONENTS` / `TEAMMATES` references (`interp/resolve.rs`: `TEAMMATES` is empty) do not consult teams. `FormatConfig.players.teams` is only a count; the engine has no team model (decision #41 FP-040 "Two-Headed Giant gating" keeps team formats gated off). #134 FP-132 (Two-Headed Giant) adds the team-aware opponent predicate (CR 102.3: "In a multiplayer game between teams, a player's teammates are the other players on their team, and the player's opponents are all players not on their team") and applies it to the protector candidates and legality test and to `OPPONENTS` / `TEAMMATES`.

**Rationale.** CR 102.2 ("In a two-player game, a player's opponent is the other player") and free-for-all multiplayer need no team data; team games are not reachable until #134 lifts the gating, so no supported game can get a teammate as protector. The finding is correct for team games and is carried to the issue that introduces them.

**Alternatives.** Adding a partial team model now (rejected: it is #134's contract and would be dead code until then).

## Gate 02-state-objects F11 — `oracleTextHash` is a text-only placeholder until #98

**Decision.** `oracleTextHash` as computed by the Scryfall import (`crates/fineprint-ingest/src/import/normalize.rs`) hashes the normalized Oracle text only. REQ-ING-012 defines it as `sha256(normalized oracle_text ⊕ type_line ⊕ mana_cost ⊕ P/T/loyalty/defense)` per face; the full definition lands with #98 FP-096 ("oracleTextHash exactly as REQ-ING-012 defines"), which is the first consumer (change detection and status demotion). Nothing reads the current value before then.

**Rationale.** The text-only value is an import-time placeholder; computing the final definition belongs with the code that compares it against a definition's `source.oracleTextHash`, so it is defined once and tested against the requirement there.

**Alternatives.** Computing the full definition now (rejected: duplicate of #98's deliverable with no consumer to test it against).

## Gate 02-state-objects F4 — CR 704.5y applies to an attacked battle

Supersedes "#43 FP-042 — 704.5x and 704.5y exempt a battle being attacked".

**Decision.** CR 704.5y is operative as written: a battle whose protector (a player in the game)
can't be its protector is a 704.5y finding whether or not a creature is attacking it, and its
controller is asked to choose a new protector (reason `CR 704.5y`; with no candidate the battle
goes to its owner's graveyard). Only CR 704.5x keeps the attack condition: a battle being
attacked with no player in the game as its protector is not a finding. An attacker whose battle
was removed from combat (`Attacking(None)`, CR 506.4c) still attacks nothing and exempts nothing.
A protector who left the game or has `LOST_THE_GAME` is still no protector (704.5x, not 704.5y).
In code, `SbaView::battles()` lists every phased-in battle (704.5y);
`SbaView::unattacked_battles()` filters it to the unattacked ones (704.5x only).

Changing the protector of a battle that is being attacked removes that battle from combat
(CR 506.4: "A permanent is removed from combat if ... its controller or protector changes ... A
planeswalker or battle that's removed from combat stops being attacked"; CR 506.4c: its attackers
continue to be attacking creatures that attack no player, planeswalker or battle, and deal no
combat damage if unblocked). Combat is not modelled yet, so the SBA pass reports the change only
through `SbaOutcome::protectors_chosen` and does not touch combat state; the combat work units
(phase 09: #67, #68, #69) and #221 (which consumes the SBA outcome) must apply CR 506.4 / 506.4c
from that report.

**Rationale.** R8 judgement call. CR 704.5y reads "If a battle's protector is a player who can't
be its protector, that battle's controller chooses an appropriate player to become its
protector" with no "being attacked" clause, whereas CR 704.5x explicitly adds "and no attacking
creatures are currently attacking that battle". CR 310.11 reads "If a battle that isn't being
attacked has no player designated as its protector, or its protector is a player who can't be
its protector, its controller chooses an appropriate player to be its protector. ... This is a
state-based action (see rule 704)", which grammatically applies the attack condition to both
cases. The tension is resolved in favour of the more specific state-based-action rule 704.5y:
CR 704 is the rule that defines what the state-based actions are, and CR 310.11 points to it
("see rule 704"); 310.11 is read as a summary. The deliberate difference between the 704.5x and
704.5y texts (the attack clause on one only) is the stronger signal. The combat consequence
(CR 506.4) is real but is a consequence the rules provide for, not a reason to skip the action.

**Alternatives.** Keep exempting both (the superseded decision; rejected: contradicts the text
of 704.5y, finding F4 of the phase 02 gate review). Exempt neither (rejected: 704.5x states its
attack condition explicitly). Remove the battle from combat inside the SBA pass (rejected: combat
state is owned by the phase 09 combat units; the pass reports, combat applies).

## Gate 02-state-objects F2 — One APNAP request sequence per SBA pass

Supersedes "#43 FP-042 — Order of protector requests".

**Decision.** The legend-rule requests (CR 704.5j) and the battle protector requests
(CR 704.5x, CR 704.5y) of one SBA pass form ONE request sequence, ordered by (1) the APNAP rank of
the choosing player (CR 101.4, the same `apnap_order` as before: turn order starting from the
active player, players not in the turn order last ascending), then (2) within one player, every
legend-rule group before every protector request, then (3) the existing tie-break: the legend
group's lowest member id, or ascending battle id. Request ids are assigned in this asking order.
All requests are still asked before any state change (a pending request leaves the state and
registries untouched), and every move, protector record and unprotectable-battle move is applied
together afterwards in the one batch. A battle with no candidate (or no controller) still gets
no request and goes to its owner's graveyard; a battle that is also a 704.5v / 704.5w finding
still gets no request. The old rule "every legend request first, then every protector request"
is gone; "legend before protector" survives only as the within-player tie-break (2).

**Rationale.** CR 101.4 orders choices per PLAYER, not per kind of choice: "the active player
... makes any choices required, then the next player in turn order ... makes any choices
required, followed by the remaining nonactive players in turn order. Then the actions happen
simultaneously." The legend and protector choices come from the same check (CR 704.3), so an
active player's protector choice precedes a nonactive player's legend choice (finding F2 of the
phase 02 gate review). The order of one player's own simultaneous choices is free; legend first
(registry order, 704.5j before 704.5x) then ascending id is a deterministic stand-in (R3).

**Alternatives.** Keep kind-first ordering (the superseded decision; rejected: violates CR 101.4
when two different players choose). Within one player, protector before legend (rejected: no
rules difference; legend first keeps the previous behaviour for a single chooser, so existing
single-player tests and replays keep their request order).

## Gate 02-state-objects F5 — Battle protector pair is a required pair (documentation)

**Decision.** `SbaEnvironment::battle_protector` and `SbaEnvironment::set_battle_protector` keep
their defaults (`None` / no-op) and their signatures, and are documented as a REQUIRED PAIR for
any environment whose game can have a battle, in the style of the deathtouch pair: an environment
that answers `battle_protector` MUST remember what `set_battle_protector` stored;
`perform_all` writes the chosen protectors only after the whole pass succeeded (never on
`BusInterrupt::Pending` or an error); an environment implementing neither re-asks the protector
at every check; `StoredSbaEnvironment` implements both. No error path is added.

**Rationale.** The brief forbids changing `SbaEnvironment` methods or `SbaOutcome` fields; the
default pair is consistent (a no-op setter with a `None` getter never claims to remember), and
the write is already after the last fallible step. Existing tests prove a stored environment
does not re-ask after a pass (`sba::tests::sba_704_5x_siege_without_protector`: the repeated
check is empty and the next pass performs nothing) and pin the defaults
(`sba::tests::fp042_environment_defaults`).

**Alternatives.** Make `set_battle_protector` return a `Result` and fail the pass on an
unsupported write (rejected: an API change to a contract-facing trait, outside this fix's
scope; it would go through `contracts/CHANGE_REQUESTS.md`). Remove the defaults (rejected:
breaking for every environment that has no battles).

## Gate 02-state-objects F1 — Protector request names its battle

**Decision.** A battle protector request (`CHOOSE_PLAYER`, CR 704.5x / 704.5y) carries the
battle in `reason.source` (`Some(battle)`); the payload and options stay the candidate players.

**Rationale.** Two requests with identical candidate lists (one controller, two Sieges) must be
distinguishable by the client; `ChoiceReason::source` is the existing field for the object a
choice is about, so no wire-shape change is needed.

**Alternatives.** Add the battle to the `ChoosePlayer` payload (rejected: a wire-shape change to
the choice protocol contract).

## Gate 02-state-objects F24 — DISTINCT over SOURCE_LKI reads the captured view

**Decision.** Amends the section "#57 FP-055 - Interpretations" item 8. `DISTINCT` over the
selector `Ref(SOURCE_LKI)` reads each object's view through the same function as `ATTR(SOURCE_LKI)`
(`reference_view` in `interp/expr.rs`): the captured last known information of the source first,
the current view when nothing was captured. Every other selector keeps reading the current view
first. Tap status (F28): `TAPPED` and `UNTAPPED` hold only for an object in the battlefield zone
(CR 110.5d); `FACE_UP` stays "not face down".

**Rationale.** Two reads of one reference must agree for a source that left the battlefield
(REQ-DSL-003). A filter selector never names the source through LKI, so it keeps the current view.

**Alternatives.** Applying the LKI policy to any selector that happens to contain the source id
(rejected: the policy belongs to the reference form); leaving `DISTINCT` on current views
(rejected: contradicts `ATTR`).

## Gate 02-state-objects F7 - Retroactive Card DSL change requests; no dslVersion bump

**Decision.** Record #159 (validation of AST members on seal and load) as CR-002 and #161 (abilities
hold the real `effect::Instruction`, which narrows the accepted ability set) as CR-003, both breaking,
both merged without an R2 entry and recorded retroactively by the phase 02 gate. `dslVersion` stays
"1.0": no bump and no migration tool. The migration of in-repo consumers is #232 FP-164.

**Rationale.** The rejected placeholder content (`power: 1`, instructions with arbitrary `op`) was
never a valid SRS section 14 AST. No definitions are persisted yet (REQ-ING-007 definitions are the
client's), the project is pre-release, and the change landed at a phase boundary, which R2 requires
for breaking changes. Strict validation is kept (REQ-DSL-010, REQ-DSL-012).

**Alternatives.** Bump to "1.1" and keep a legacy reader for the placeholder shapes: rejected, as it
would preserve invalid content and add a code path with no users.

## Gate 02-state-objects F12 — curl 8.4.0 minimum and a capped metadata read

**Decision.** The bulk-data metadata response is read from curl's piped stdout through
`take(MAX_METADATA_BYTES + 1)`; past the 1 MiB cap the child is killed and the run fails with an
error naming the cap (`run_capped`, `read_stream_capped` in `import/fetch.rs`). Bulk downloads keep
curl's `--max-filesize` cap and require curl 8.4.0 or later, checked once per process with
`curl --disable --version` before the first download: an older parsed version is an error naming
8.4.0 and the cap; an unparsable version proceeds with a warning on stderr. Documented in
`docs/DEVELOPMENT.md` (`fetch` paragraph).
**Rationale.** Before 8.4.0 curl ignores `--max-filesize` when the size is unknown at the start of
the transfer, so the cap must not rely on it for metadata (read in-process) and must be guaranteed
for bulk files (version floor). Streaming the multi-GB bulk files through the process to count bytes
would duplicate what curl already does; an unparsable version string is treated as a warning so that
vendor builds with unusual banners still work.
**Alternatives.** Stream bulk downloads through stdout into the part file with an in-process cap
(more code and an extra copy of every byte); refuse an unparsable version (breaks unusual builds).

## Gate 02-state-objects F13 — Three-step group commit instead of generation directories

**Decision.** `fetch` commits a cache group in three steps under the group lock: (1) atomically
rewrite `source.json` with the entries of the files about to be replaced removed (skipped when no
entry is removed), (2) rename each staged part file into place, (3) atomically write the full new
`source.json`. A file with no metadata entry is "not fetched": it is stale and refetched next run,
never trusted. A failure at any step leaves every file either absent from the metadata or consistent
with it; remaining part files are removed. This amends "#87 FP-085 — 24-hour cache, clock injection
and cache metadata" (its "a failure leaves the group unchanged" now holds for failures before the
commit; a failure during the commit leaves the group consistent rather than unchanged).
**Rationale.** It meets the integrity goal of the finding (an incomplete generation is never treated
as fresh) while keeping the fixed, documented cache layout (`scryfall/oracle-cards.json`, ...) that
`import` and users read directly.
**Alternatives.** Generation directories published by one atomic switch (a directory rename or a
pointer file): considered and not chosen, because it changes the documented cache layout that
`import` reads and a portable atomic directory replacement does not exist in the standard library.

## Gate 02-state-objects F14 — Group lock and per-run part files

**Decision.** Each group directory is created first, then `fetch` takes an exclusive
`std::fs::File::lock` on `<group>/.lock`, held from reading `source.json` through staging and
commit. The lock file is left in place (removing it would race with a waiting run). Part files are
per run, `{file}.{pid}.part`, opened with `create_new` (a leftover of a crashed run with the same pid
is removed under the lock and creation retried), and removed on every failure path.
**Rationale.** Serializing the whole read-stage-commit sequence means a second run sees the first
run's fresh metadata and downloads nothing; per-run names keep a part file owned by one run.
**Alternatives.** Merging metadata against the current generation without serializing downloads
(duplicate downloads, more complex); random part names (not needed under the lock).

## Gate 02-state-objects F15 — curl as the HTTP client: no redirects, --disable, --globoff

**Decision.** Amends "#87 FP-085 — curl as the HTTP client, behind an Http trait": curl now runs
with `--disable` as the first argument (no `~/.curlrc`; proxy and CA settings still come from the
environment), then `--fail --silent --show-error --globoff --proto =https --max-filesize
--max-time`, the headers, `--output <dest>` passed as the native `OsString` path, and `--url`.
`--location` and `--proto-redir` are dropped: no redirect is followed (the live bulk hosts answer
200 directly; a 3xx answer fails body validation and its part file is discarded). A curl failure is
reported as `curl failed for <role> (exit N)` plus the first stderr line cut to 200 bytes with
control characters replaced, never the URL (F34). (Also covers F33, F35, F36.)
**Rationale.** Following redirects to any https host defeats the download host allowlist (R12); a
user curlrc can disable certificate checks; URL globbing can turn one validated URI into several
transfers; lossy path conversion can write to the wrong file; URIs can carry query strings that
must not reach error text.
**Alternatives.** Following redirects and validating each hop (curl cannot apply a host policy per
hop without a custom client).

## Gate 02-state-objects F37 — download_uri host policy excludes the API host

**Decision.** Amends "#87 FP-085 — 24-hour cache, clock injection and cache metadata" (its last
sentence on `download_uri`): `download_uri` must be `https://` on `scryfall.com`, `*.scryfall.com` or
`*.scryfall.io` with lowercase labels, except the exact host `api.scryfall.com`, with no credentials,
no port, no whitespace, control characters or backslash, and no curl glob character `[ ] { }`. The
metadata endpoint keeps its own request path (`BULK_DATA_URL`).
**Rationale.** A `download_uri` naming the API host would cause a second API request per run before
the body validator could reject it (REQ-ING-001: one API request per run). Upper-case hosts are
already rejected by the label rule, so the exact-match exclusion covers every accepted spelling.
**Alternatives.** An explicit list of known file hosts (`data.scryfall.io`, ...): rejected because
Scryfall has changed file hosts before and `c2.scryfall.com` is still asserted valid.

## Gate 02-state-objects F8 — The records file and the MTGJSON report must be different files

**Decision.** Before writing anything, `import` resolves each non-`-` destination (the records
file, default `<cache>/normalized/cards.json` with its directory created first, and
`--mtgjson-report`) by canonicalizing the parent directory and joining the file name; when both
resolve to the same path it exits 2 with a usage message (`the records file and
--mtgjson-report name the same file` plus the usage line) and writes neither file. A parent that
cannot be canonicalized (it does not exist) is compared as written; the write then fails with
exit 2 as before. `-` for both stays the existing parse-time usage error; one stdout destination
is valid.
**Rationale.** REQ-ING-001 / REQ-NFR-004: an exit 0 with the records replaced by the report
loses the records artifact. Canonicalizing the parent catches `.`/`..` spellings and symlinked
directories without requiring the file itself to exist.
**Alternatives.** Comparing the strings as written (misses equivalent spellings); canonicalizing
the whole file path (fails for a file that does not exist yet; a symlinked file is replaced by
the atomic rename, not written through, so it cannot alias).

## Gate 02-state-objects F9 — At most 16 faces per Scryfall card object

**Decision.** `normalize::MAX_FACES = 16`. A `card_faces` array longer than that rejects the
object with the new `RejectReason::TooManyFaces` (wire string `tooManyFaces`, additive) before
any `RawFace` is built or the shared type line is copied; the other objects still import.
**Rationale.** R12: per-object work and memory must be bounded; one 30 KB type line copied and
split for thousands of faces is a large amplification. Real multi-face layouts have two faces;
16 leaves ample headroom. The layout unit has no explicit face-count limit to reuse (it only
checks counts per layout after normalization, too late to prevent the allocations).
**Alternatives.** A limit of 2 (would turn future layouts into rejections rather than layout
diagnostics); a byte budget on the type line (does not bound the per-face work).

## Gate 02-state-objects F10 — A face without its own type line takes only its part of the combined line

**Decision.** When at least one face of a multi-face object has no `type_line`, the top-level
line is split on ` // `: with one part per face, each face without its own line takes its
matching part; with exactly one part, that line is copied to each face without its own line;
any other part count (on a non-empty line) adds `typeLineFaceMismatch` and those faces keep no
type line (empty types, plus `typeLineEmpty`). Faces with their own line always keep it. This
one rule replaces the former two branches (no face lines / some face lines), whose results for
the no-face-line case it reproduces.
**Rationale.** The former fallback copied the whole combined line, so a second face got the
first face's subtypes and the literal `//` as a subtype, which also made the per-face MTGJSON
cross-check disagree.
**Alternatives.** Keeping the whole-line copy (wrong types); always rejecting such objects
(normalization flags shape problems with diagnostics, it does not reject them).

## Gate 02-state-objects F16 — The MTGJSON cross-check compares every face

**Decision.** The MTGJSON entries of an `oracle_id` are put in face order by `side` (stable
sort) when every entry has a string `side`, else kept in source order; entry `i` is compared
with the record's face `i`. `manaValue` and `colorIdentity` are card-wide, compared once from
the first entry; `colors`, `types`, `supertypes`, `subtypes`, `power`, `toughness`, `loyalty`
and `defense` are per face, each disagreement written `{"face": i, "field": ..., "scryfall":
..., "mtgjson": ...}`. A different number of faces adds `{"field": "faces", "scryfall": n,
"mtgjson": m}` and the common leading faces are still compared. Order within a card's `fields`:
card-wide fields, then `faces`, then each face's fields in face order. A card is `agreed` only
when nothing disagrees. The per-face `colors` entry moves after `colorIdentity` in the report
(the existing report test is updated accordingly).
**Rationale.** REQ-ING-001: "Any disagreement in a characteristic SHALL be reported"; checking
only the front face counted back-face disagreements as agreement.
**Alternatives.** Matching faces by name (would need names in the comparison path; `side` is the
source's face identifier); reporting only the face count when it differs (hides real
characteristic disagreements on the common faces).

## Gate 02-state-objects F17 — The bulk-array scanner streams; amends "#87 FP-085 — Output shapes, default paths and --strict"

**Decision.** `json_array::ArrayElements` is a public iterator that yields one top-level element
slice at a time (`Result<&[u8], ArrayError>`, the same elements and error values as
`split_array`, an error yielded once after the elements that precede it, then fused).
`validate_array`, `import_oracle_cards` and `import_rulings` iterate it, so no per-element
index is built; a late structural error still fails the whole import (what was built is
discarded). `split_array` stays as the collecting wrapper. This amends the sentence of "#87
FP-085 — Output shapes, default paths and --strict" that says bulk arrays are split by a small
scanner "so memory is bounded by the largest element": until this change the splitter built a
`Vec<&[u8]>` of every element first (up to several times the input size for small elements);
memory is now bounded by the input plus the largest element.
**Rationale.** R12: bounded memory for untrusted bulk input; the decision text claimed a
property the code did not have.
**Alternatives.** A two-pass scan (validate structure, then process; same result, twice the
scanning); a full streaming JSON parser (larger change, not needed).

## Gate 02-state-objects F38 — No stale `updatedAt` for bytes that do not match the recorded digest

**Decision.** The provenance `updatedAt` of an imported file (normalized output and MTGJSON
report) is copied from the cache metadata only when the recomputed sha256 equals the recorded
one; otherwise it is null. The `sourceHashMismatch:*` warning (and the report's `hashMismatch`)
and the recomputed hash are unchanged. Amends "#87 FP-085 — Output shapes, default paths and
--strict" (the recorded `updatedAt` no longer accompanies a digest mismatch).
**Rationale.** REQ-ING-001 provenance: a timestamp describing other bytes would be false
provenance for the bytes actually read.
**Alternatives.** Keeping the timestamp beside the warning (pairs a stale timestamp with a new
hash).

## Gate 02-state-objects F40 — Multi-word subtype self-reference

**Decision.** `Lexicon::new` recovers multi-word subtype boundaries from the rules vocabulary: every multi-word subtype phrase in the vocabulary whose words appear consecutively (case-insensitive) in the face's own subtypes also gets the key `this <phrase>` as a SELF entry. The importer's word-level subtype output is unchanged.

**Rationale.** Keeps `mtgjson.rs` subtype comparison intact and needs no contract or importer change; longest-match marking then covers the whole phrase.

**Alternatives.** Changing the importer to store whole phrases (rejected: breaks the mtgjson comparison); joining every adjacent subtype pair (rejected: would invent phrases).

## Gate 02-state-objects F20 — Late gate after any routine merge

**Decision.** The gate check and the merge are not atomic, so after every routine merge the merging session re-queries the open `work-unit` issues of the merged issue's phase label and milestone. If none remain and no `reviewed/<gate>` tag exists, it runs a "late gate" at once: branch `gate/<gate>` from the merge commit on `main` carrying the gate `docs/RUN_LOG.md` row, a PR from it to `main` (no `Closes`) in the role of the gate issue's PR, a review PR against `review-base/<gate>` (cumulative since the last `reviewed/*` tag), then P6 steps 3-7 (full review, dispositions, resolved threads, merge, review PR closed unmerged, tags). It covers unlabelled final issues; a `review:gate` label on the planned final issue still takes the normal pre-merge path. Written into docs/PROCESS.md P6 "Late gate", work-issue §5 step 2 and §7 step 2, run-plan §2.

**Rationale.** No lock orders rechecks and merges across parallel sessions (run-plan runs three at once); a post-merge check is the only mechanism that needs no external state and is idempotent (the `reviewed/<gate>` tag marks completion).

**Alternatives.** A merge-time lock or label claim (needs shared mutable state, can deadlock); requiring `review:gate` on every phase's last issue (does not cover the race for an unlabelled final issue); serialising same-phase merges (loses parallelism).

## Gate 02-state-objects F22 F23 — Split gate reviews: no-overlay areas, area heads, path coverage

**Decision.** Area review branches are built with `git checkout --no-overlay <gate head> -- <paths>` and rebuilt from the current gate head after each push. Each is verified by `git diff --name-status <base> review/<gate>-<area>` equal to `git diff --name-status <base> <gate head> -- <paths>`, and the union of the area paths must cover `git diff --name-only <base> <gate head>` (deletions included). Each area PR's approval must cover that area branch's own current head, not the shared gate head. Amends the P6 step 3 and step 6 wording.

**Rationale.** Overlay checkout keeps base files the gate deleted; area heads can never equal the gate head, so the old condition was unsatisfiable.

**Alternatives.** Merging the gate head into each area branch (puts the whole diff in every area); approving only the largest area.

## Gate 02-state-objects F43 — Installer never replaces an existing hook setup

**Decision.** `scripts/install-git-hooks.sh` sets `core.hooksPath=.githooks` only when it is unset or already `.githooks` AND `git rev-parse --git-path hooks` holds no non-`.sample` file. Otherwise it leaves the setting, prints a warning on stderr naming what exists and the manual command (`git config core.hooksPath .githooks`, calling existing hooks from `.githooks`), and exits 0. `FINEPRINT_FORCE_HOOKS=1` forces the replacement. `fineprint.coauthor` resolution is unchanged and always happens. The warning repeats each session while the conflict stands.

**Rationale.** Git uses one hooks directory, so replacing it silently disables a contributor's checks; the script runs at SessionStart so it must never fail or block.

**Alternatives.** Chaining existing hooks automatically (fragile, executes unknown scripts); prompting (impossible at SessionStart); keeping the unconditional replacement.

## Gate 02-state-objects R3 — Protector request after the batch for a retained zero-defense battle

**Decision.** This SUPERSEDES the decision "#43 FP-042 — No protector request for a battle at 0
defense", whose Decision read: "A battle that is also a CR 704.5v or CR 704.5w finding in the same
pass gets no protector request and no `protectors_chosen` entry; its 704.5x/704.5y finding is still
reported in `findings`. It moves once." That decision is superseded as follows. Before the batch,
nothing changes: a battle that is also a CR 704.5v or CR 704.5w finding still gets no protector
request, and the pre-batch request sequence (request ids included) is unchanged. The pass now
remembers each such battle with the CR reference of its CR 704.5x or CR 704.5y finding
(`Plan::retained_battles`). After the batch, if a remembered battle is still on the battlefield
as the same object (same `ObjectId`; a battle that moved has another id or zone) and is still a
CR 704.5x or CR 704.5y subject in a fresh view of the state after the batch, it means a replacement
effect replaced its graveyard move (CR 614.6, CR 704.7). Its controller is then asked the usual
protector choice in the same pass: one `CHOOSE_PLAYER`, with the request shape of a pre-batch
protector choice and candidates computed from the post-batch view. These requests come after
every pre-batch request and every request raised by the batch. They are ordered by the chooser's
APNAP rank, then ascending battle id, and use `choices.next_request_id()` at that point. Each
answer goes into `protectors_chosen` (sorted by battle; recorded through
`SbaEnvironment::set_battle_protector` only when the pass succeeds), which makes the pass
`performed`. A retained battle without a controller or without a candidate gets no request and
the next pass finds it again. A battle that actually leaves gets no request and no
`protectors_chosen` entry, as before. A pending or failed post-batch request rolls the whole pass
back like any other request.

**Rationale.** The rules-researcher's answers (CR 2026-11-06):
(1) CR 704.5x applies independently of CR 704.5v and CR 704.5w. If the graveyard move is
replaced, the battle stays and ends the event with the chosen protector.
(2) A no-candidate graveyard move is part of the same event, and per CR 704.7 a single
replacement replaces both moves.
(3) When the move goes through, skipping the choice cannot be observed: the protector designation
leaves with the object, and its last known information comes from before the state-based actions
(CR 704.8). Asking only matters when the move is replaced.
So suppressing the request for a battle that actually leaves is still correct. A battle whose move
was replaced must get its protector choice, otherwise it would stay on the battlefield with no
protector forever (gate review round 2, finding R3). Asking only after the batch keeps the replay
of every normal game unchanged.

**Alternatives.** Asking every zero-defense battle before the batch. Rejected: it adds a
meaningless decision to the replay of every normal case and changes existing replay logs. Skipping
forever (the superseded behaviour). Rejected: a battle whose move a replacement keeps skipping
would stay unprotected and never be asked.

## Gate 02-state-objects R5 — Bulk downloads are capped in-process

Amends "Gate 02-state-objects F12 — curl 8.4.0 minimum and a capped metadata read" (its curl
version floor and its fail-open handling of an unparseable version).

**Decision.** `Curl::download_to_file` runs curl without `--output`: curl writes the body to
stdout and this program streams it, through a fixed 64 KiB buffer, into the existing part file
(opened for writing and truncated, never created there; flushed at the end) with a byte cap of
`Request.max_bytes` (`run_capped_to_file`, built on `copy_stream_capped`). At most `cap + 1`
bytes are read and at most `cap` bytes are written; as soon as more than `cap` bytes arrive the
curl process is killed and waited, and the caller (`stage`) removes the part file. The metadata
read keeps its in-memory 1 MiB cap (`run_capped`). The curl version check is advisory only:
`check_curl_version` returns `Ok(None)` for 8.4.0 or later and `Ok(Some(warning))` for an older or
unparseable version (never `Err`), and at most one warning per process is printed on stderr; a
failure to read `curl --version` is also only a warning (a missing curl still fails at the
download's spawn). `--max-filesize <cap>` stays in the arguments as a second line of defence.

**Rationale.** `curl --version` reports the curl frontend version, which need not be the version
of the linked libcurl that actually enforces `--max-filesize` on a response of unknown size; an
unparseable banner was fail-open; and with `--output` curl writes the file itself, so no
in-process count could stop an oversized body before it reached the disk (the later
`read_capped` only rejected it after it was written). Counting the bytes in this process makes
the cap hold whatever curl or libcurl is installed (R12, REQ-NFR-004).

**Alternatives.** Parse the `libcurl/x.y.z` token and fail closed on an older or unknown
version (rejected: it still trusts the library's behaviour, and it breaks unusual or vendor
builds whose banner differs). Keep `--output` and check the file size afterwards (rejected: the
oversized body is already on disk by then).

## Gate 02-state-objects R4 — Destination identity check after the records are written

**Amends:** "Gate 02-state-objects F8 — The records file and the MTGJSON report must be different files" (the spelling pre-check stays).

**Decision.** `import` keeps the pre-write spelling check (`resolve_dest`, exit 2 before anything is written). It adds an identity check: after the records file is written and before the report is written, when both destinations are files and the report destination exists and is the same file as the records destination (`same_file::is_same_file`: device and inode on Unix, the volume serial and file index on Windows), `import` exits 2 with the usage message "the records file and --mtgjson-report name the same file" and writes no report; the records file just written stays intact. The check is skipped when the report destination is a symbolic link, since the atomic rename replaces the link itself and cannot reach the records. The `same-file` crate, exact-pinned (`=1.0.6`, already in Cargo.lock through another package), is added as a workspace dependency used by `fineprint-ingest` only; it does not enter the rules crates.

**Rationale.** Names such as `cards.json` and `Cards.json` differ in spelling but name one file on a case-insensitive file system (APFS, NTFS), so a spelling comparison lets the report replace the records and the command succeeds. File identity is the only reliable test, and it needs the file to exist: when neither file exists there is no identity yet, so a check before writing is impossible; the records file is therefore written first and the report destination tested next. One crate on every platform means one code path and no unsafe.

**Alternatives.** Std-only `MetadataExt` (dev+ino) on Unix plus the crate on Windows (rejected: two code paths for no saving). Comparing lower-cased names (rejected: wrong on case-sensitive file systems, where case-distinct names are valid). Probing the directory's case sensitivity (rejected: racy and per directory).

## Gate 02-state-objects R3b — Post-batch layer inputs for the retained-battle request

Amends "Gate 02-state-objects R3 — Protector request after the batch for a retained zero-defense
battle" (CodeRabbit gate round 3, thread 4697581022).

**Decision.** `fineprint_core::sba` gains one additive entry point,
`perform_all_relayered(registry, ctx, replacements, effects, layer_env, env, choices, post_batch)`,
and one type alias,
`PostBatchLayers<'a> = &'a mut dyn FnMut(&EventContext<'_>, &mut dyn FnMut(&ContinuousEffects, &dyn LayerEnvironment))`.
After the batch, once the `&mut ctx` borrow of the bus has ended, the pass calls `post_batch` with
the post-batch `EventContext`; the caller builds the layer inputs of that state and calls the inner
closure with them, inside which the engine builds the post-batch `SbaView` used for BOTH the
"still a CR 704.5x/704.5y subject" test and `protector_asks` (controller and candidates) of
`Plan::retained_battles`. The provider is called at most once per pass, and only when a retained
battle is still on the battlefield (an ordinary pass never calls it). Everything before the batch
(check, `Plan::new`, the pre-batch request sequence and request ids, the batch) still reads
`effects`/`layer_env`. `perform_all` keeps its signature and behaviour: it delegates to
`perform_all_relayered` with a provider that hands back the same pre-batch `effects`/`layer_env`,
and its rustdoc says so and points callers whose layer inputs depend on state the batch changes
(unattach, counter removal, zone changes) to the new entry point. The name `perform_all_relayered`
is the brief's recommendation and is kept.

**Rationale.** `LayerEnvironment` is a caller-implemented trait (a pure function of the caller's
state); the engine cannot rebuild it from `ctx`, and it cannot read the mutated context while the
batch holds `&mut ctx`. Only the caller can supply post-batch layer inputs, so the engine hands it
the post-batch context through a callback. The callback shape keeps the inputs inside the call
(no lifetime escape, no unsafe code, no engine-side clone of the whole state), and calling it
inside the pass keeps the rollback guarantee: a Pending post-batch request still restores every
component through `perform_all`'s `Saved` restore.

**Alternatives.** Change the `perform_all` signature (rejected: a breaking change to about twelve
callers and to the public API, R2). A second pass that asks the retained battles with fresh inputs
(rejected: the zero-defense finding would suppress the request again, since `Plan::new` cannot
know the move was replaced). Keep the stale pre-batch inputs (rejected: wrong chooser or
candidates when the batch changes a controller or counters that a layer effect reads).

## Gate 02-state-objects R8b — An MTGJSON side that names an absent face is a disagreement

Amends "Gate 02-state-objects R8" (the face pairing by `side`).

**Decision.** In the MTGJSON cross-check, when every entry of a card carries a single lowercase letter `side` and an entry's letter names a face the normalized record does not have, that entry is reported as one disagreement `{"face": <index>, "field": "side", "scryfall": "absent", "mtgjson": "present"}` (`b` is index 1). It is emitted in entry order, after the card-wide and `faces` count disagreements, interleaved with the per-face ones. The `faces` count disagreement is unchanged, and so is the source-order pairing used when any `side` is unusable.

**Rationale.** Silently skipping the entry made a card whose only face was never compared look agreed. The shape follows the per-face shape (`face`, `field`, `scryfall`, `mtgjson`) and carries kind words only, so untrusted MTGJSON text never enters the report.

**Alternatives.** A top-level `unmatchedSides` list (rejected: changes the report layout and the `fields` consumers). Reusing `field: "faces"` with counts (rejected: counts can be equal, so it would not identify the entry). Rejecting the entry in `parse_atomic` (rejected: the card's other entries are still comparable, and a rejected record would hide the card from the report).

## Gate 02-state-objects R3c — Post-batch eligibility, CR reference and lost controller for the retained-battle request

Amends "Gate 02-state-objects R3b — Post-batch layer inputs for the retained-battle request" and
"Gate 02-state-objects R3 — Protector request after the batch for a retained zero-defense battle"
(review threads 4698236604, 4698236617, 4698236625).

**Decision.**
1. Additive eligibility-view entry point: `pub type PostBatchState<'a>` (a provider that calls its
   callback once with the post-batch `&ContinuousEffects`, `&dyn LayerEnvironment` and a READ-ONLY
   `&dyn SbaEnvironment`) and `pub fn perform_all_post_batch(...)` (same arguments as
   `perform_all_relayered`, `post_batch: PostBatchState<'_>`), re-exported from `sba`. The
   post-batch `SbaView` of the retained battles reads `battle_protector` and `can_be_protector`
   from that view (both the CR 704.5x/704.5y subject test and the candidates). Internally all three
   entry points share one private pass whose provider callback takes
   `Option<&dyn SbaEnvironment>`; `perform_all_relayered` passes `None` (the pass's own `env`), so
   its behaviour is byte-for-byte unchanged and `PostBatchLayers` is unchanged. Writes
   (`set_battle_protector`, the deathtouch flag) stay on the original `&mut dyn SbaEnvironment`
   after the pass succeeds. The game variant is still read from the pass's own `env`.
2. CR reference from the post-batch entry: `plan.retained_battles` only selects the deferred battle
   ids; each battle's `ProtectorAsk.cr` (and so the request's `reason.cr`) is the `cr_ref` of the
   post-batch ChooseProtector entry that flags it (if two entries ever flagged one battle, the
   first in registry order wins).
3. Lost controller: in the deferred stage only, a retained battle whose controller has the
   `LOST_THE_GAME` designation after the batch (or is not a player of the game) takes the same
   no-request path as a battle without a controller: no request, no request id consumed, nothing
   pending, not in `protectors_chosen`; the next pass finds it again. The shared `protector_asks`
   and the pre-batch request sequence are unchanged.

**Rationale.** `SbaEnvironment` is caller-supplied state, like `LayerEnvironment`, and the engine
cannot ask the caller to re-read it while the batch holds `&mut ctx`; only the caller can describe
the post-batch eligibility, and only read-only, so the writes remain where the rollback contract
expects them. The engine takes the CR reference from the rule that actually flags the battle after
the batch (a protector who lost the game in the same pass turns a CR 704.5y subject into a
CR 704.5x one). A player who has lost the game cannot answer; the request would stay Pending until
its default fires.

**Alternatives.** Change `PostBatchLayers` to add the view (rejected: breaks the R3b public type,
even if unreleased, and its callers). A second pass (rejected as in R3b). Apply the lost-controller
check in the shared `protector_asks` (rejected: it would change the pre-batch request sequence).
Keep the pre-batch CR reference (rejected: names a rule that no longer flags the battle).
Open rules question for the rules-researcher: whether such a battle should instead leave with the
player who lost, or another player choose; a follow-up decision amends this one if needed.
Answered in R3d: CR 800.4a, CR 800.4h.

## Gate 02-state-objects R3d — No protector request to a controller who lost the game, on every path

**Decision.** Amends "Gate 02-state-objects R3c". The live-controller check of the post-batch
retained-battle request (the controller is a player of the game without the `LOST_THE_GAME`
designation) now applies to the ordinary, pre-batch protector requests too, through one shared
helper (`can_choose_protector` in `crates/fineprint-core/src/sba/perform.rs`). On either path a
CR 704.5x or CR 704.5y battle whose controller has lost the game, or is not a player of the game,
gets no request and consumes no request id. On the ordinary path that battle also does not move:
it stays on the battlefield without a protector, and the next pass finds it again (the same
outcome as the deferred path). A battle with no candidate still goes to its owner's graveyard
without a request, as before; for example, a typeless battle whose controller lost the game has
no candidate because its only appropriate player has left. Requests to live controllers, their
order (CR 101.4) and their request ids are unchanged.

**Rationale.** CR 704.5x and CR 704.5y name "that battle's controller" as the chooser. CR 800.4a
removes (exiles) the battle when that player leaves the game, so normally no choice remains. CR
800.4h says: "If a rule requires a player who has left the game to make a choice, the next player
in turn order makes that choice." That is reachable only if such a choice survives the departure.
The engine marks a player who lost with the [`LOST_THE_GAME`] designation but does not yet perform
the departure (#125), so asking that player would suspend the pass on a choice nobody can answer. This can happen on the ordinary path
(CodeRabbit thread 4699044484). In pass 1, a Siege at defense 0 has its graveyard move replaced
by a defense counter while its controller loses the game. In pass 2, the Siege is at defense 1,
is a CR 704.5x subject, and is planned as an ordinary request. CR 800.4a makes the leaving
player's objects leave the game or be exiled. Performing that is the caller's job (player
removal), not the SBA pass's, so the pass does not invent a graveyard move for such a battle.
Leaving the battle in place keeps the two paths identical and lets the player-leaves-game
procedure deal with it. One helper keeps the two checks from drifting apart.

**Alternatives.** (1) Treat a lost controller like a missing controller on the ordinary path, so
the battle goes to the graveyard with the batch. Rejected: no rule supports a graveyard move for
this reason, the move would pre-empt CR 800.4a exile and the triggers that look at it, and it
would differ from the deferred path. (2) Hand the choice to another player, such as the next
player in APNAP order. Rejected: CR 800.4a applies first and removes the battle when the player
leaves (performed by the player-leaves-game procedure, #125), and CR 800.4h applies only to a
player who has left the game and only to a choice that survives departure. Routing such a
surviving choice to the next player in turn order (CR 800.4h, not APNAP) belongs with #125
(rules-researcher, CR 2026-11-06).
(3) Keep the check on the deferred path only. Rejected: that is the reported suspension defect.

## #180 FP-143 — Lost attachment links live on `ExecutedEvent`, not on `Outcome`

**Decision:** The links an executed event removes are reported in a new last field
`ExecutedEvent::unattached: Vec<UnattachRecord>` (`attachment`, `host`), encoded last as a
length-prefixed sequence (CR-004). `Outcome` and every existing tag are unchanged.

**Rationale:** `Outcome` is `Copy` and is matched by value across the engine (SBA, replacement
bus, entering copies); one event can end any number of links (a host with several attachments),
so the fan-out needs a `Vec`, which would make `Outcome` non-`Copy` and change every variant's
shape. On `ExecutedEvent` the change is one trailing field: existing consumers keep reading
`outcome` unchanged, and the encoding change is a single appended sequence.

**Alternatives:** A new `Outcome` variant or a field on `Outcome::Attached` /
`Outcome::ZoneChange` (rejected: breaks `Copy`, changes every match site, and still could not
carry several links without a `Vec`). A separate log of unattach records beside the batch
(rejected: trigger normalization would have to join two logs and the batch order would be lost).

## #180 FP-143 — `GameEvent::UnattachFrom`, and why the SBA switch is a follow-up

**Decision:** Add `GameEvent::UnattachFrom { attachment, host }` (tag 17, appended last). It
clears the link only if the attachment is attached to exactly `host` and is otherwise a no-op;
plain `Unattach` keeps clearing whatever the current host is. The replacement bus maps it to the
same `UNATTACH` kind and the same affected object as `Unattach`. `sba/perform.rs` keeps
submitting plain `Unattach`; adopting `UnattachFrom` there is a follow-up for the SBA owner.

**Rationale:** CR 701.3d ("becoming unattached from that object or player") names the host. The
residual SBA case in "#42 FP-041 — Planned unattach and pre-pass host (round 3)" (a replacement
re-attaches E, which E's own planned `Unattach` then clears from the new host) needs an unattach
that only ends the planned link. `sba/**` is not owned by this unit (R1), so only the event is
added here.

**Alternatives:** Change `Unattach` to carry an optional host (rejected: changes an existing tag's
encoding and every literal). Change the SBA pass here (rejected: outside the owned paths).

## #180 FP-143 — Order of unattach records

**Decision:** Within one executed event the records are: for `Attach` to a different host, one
record for the previous host; for `Unattach`/`UnattachFrom`, the cleared link; for a permanent
leaving the battlefield, first its own link (under its old id), then one record per permanent
attached to it, ascending id (`Permanents::attachments_of` order). Both are read before
`Permanents::leave` drops them. A same-zone reposition, a first attach and a same-host attach
(CR 701.3b) record nothing. Records are produced in the same runs as outcomes, so the planning
run's records are discarded with its state and a failed batch logs nothing.

**Rationale:** The leaving object is the event's subject, so its own link is reported first; the
ascending order for its attachments is the engine's stable ordering key (R3). An earlier
`Unattach` in the same batch has already cleared a link, so a later zone change finds none and no
link is reported twice (this is how the SBA pass, which submits its unattaches before its zone
changes, stays at one occurrence per link).

**Alternatives:** Attachments first, then the own link (rejected: no rules difference, and the
subject-first order matches the occurrence order of the event itself). Timestamp order for the
attachments (rejected: ids are the documented stable key and need no extra lookup).

## #180 FP-143 — Normalizer order: the event's own occurrences, then its records

**Decision:** `events_from_batch` emits, for each executed event, its own occurrences first, then
one `UNATTACHED` occurrence per record in record order (subject = the attachment's pre-event id
in both views). `Unattach` and `UnattachFrom` emit their single `UNATTACHED` occurrence from the
event itself and ignore their records. So a move to a new host is `ATTACHED` then `UNATTACHED`,
and a host dying is its zone-change occurrence then the `UNATTACHED` occurrences.

**Rationale:** REQ-EVENT-002 fixes batch order for the indices; within one event the event
itself happened, and the links ended as part of it. Ignoring the records of an explicit unattach
keeps existing executed-event literals (which have no records) triggering exactly once and makes
duplication impossible.

**Alternatives:** Records before the event's own occurrences (rejected: an `ATTACHED` trigger
would then see an index after its own `UNATTACHED`, which reads as unattach-then-attach). Emitting
records for `Unattach` instead of the event (rejected: literals without records would stop
triggering).

## #180 FP-143 — Host player leaving the game: deferred

**Decision:** An attachment whose host player leaves the game (CR 701.3d, "or that player leaves
the game"; CR 800.4a) is not reported here. The engine has no player-leaves-game event yet
(#125), so there is nothing to attach a record to. `UnattachRecord.host` already supports
`AttachTarget::Player`, so the player-leaves procedure can report these links with no further
contract change. Phasing (CR 702.26i, CR 702.26j) is not modelled and touches no link.

**Rationale:** Out of the owned paths and without an executing event; the record type is ready.

**Alternatives:** Synthesize records from a player designation change (rejected: there is no such
event in the executed log, and inventing one would be a contract change outside this unit).

## #180 FP-143 — Tokens ceasing to exist in the SBA pass log no record

**Decision:** `sba/perform.rs` (around line 891) removes ceasing objects with
`Permanents::leave` directly, not through a zone-change event, so a link they were part of ends
without an `UnattachRecord` and without `UNATTACHED`. This unit does not change it (`sba/**` is
not owned here); it is recorded for the SBA owner as a follow-up.

**Rationale:** The executor can only report links that its own events end. In the normal flow a
token leaves the battlefield through a zone change (which records its links) and only then ceases
to exist in another zone, where it has no links, so the path matters only for an object that
ceases to exist while still on the battlefield.

**Alternatives:** Report from `Permanents::leave` (rejected: the permanent container is not owned
here and has no log). Change `perform.rs` here (rejected: R1).

## #180 FP-143 — CR-004 lands as its own commit in the issue PR

**Decision:** The CR-004 entry (`contracts/CHANGE_REQUESTS.md`) is the first commit of the #180
branch, touching only that file, and merges in the issue's PR rather than in a separate
contract-only PR. The `Status` is set to merged when the PR merges.

**Rationale:** #180 is the plan unit that the plan itself scheduled for this contract change
(its first deliverable is the R2 change to the outcome encoding), as #169 FP-142 did for CR-001
(merged in #235). The change is breaking only in its canonical bytes (new trailing sequence on
`ExecutedEvent`, new tag 17); no tag, field or signature changes, no replay or state-hash golden
exists yet (replay lands in #83/#106; `EventState` is not in the `GameState` hash), and it lands
right after the phase 02 gate (#235), at a phase boundary. The scheduled-session chain advances
only on merged `issue/` branches, so a separate contract branch would also stall the chain.

**Alternatives:** A separate `contract/` PR merged first (rejected: no extra review value for a
plan-scheduled change, an extra unauthorised branch, and it would leave the contract on `main`
without its implementation).

## #183 FP-144 — Ability identity by value

**Decision:** A static (or ETB) effect is linked to its generating ability as a
`state::GeneratingAbility { source, ability: AbilityAST }`. The ability exists on its source iff
the source's abilities contain an element equal to the linked `AbilityAST` (the JSON value);
duplicate identical abilities on one source are one identity. An effect id without a link is not
modelled and exists (`true`); a link whose source differs from the asked source is `false`. With
`source_view` given, the view's abilities decide. No branch on card name, id or ability content.

**Rationale:** Evaluated `Characteristics.abilities` is a `Vec<AbilityAST>` without ids, so value
identity is the only identity the layer output carries (brief D4). Equality of the JSON value is
deterministic and needs no new field on the DSL types.

**Alternatives:** Ability ids inside the AST (rejected: no such field is guaranteed by the DSL);
positional indices (rejected: layer 6 reorders and removes abilities).

## #183 FP-144 — Existence on another source

**Decision:** When the layer system asks without a `source_view` (the source is not the object
under evaluation), `GameEnvironment` evaluates the source through the layers with the state's
effects minus the effect asked about, one level deep (inside that evaluation an effect of yet
another source is judged on that source's base), and checks the evaluated abilities. This
replaces the base-only fallback named in brief D4.

**Rationale:** The base-only fallback contradicts the issue's acceptance criterion and brief S1
("B grants a marker to objects with no abilities, A removes all abilities; no marker results"):
evaluated on another object, the base of sB still has B's ability, so B would grant the marker
there although A removed the ability (CR 611.3b, 613.8a). Evaluating the source makes no object
get the marker in both timestamp orders. The effect asked about is left out because an effect
never depends on itself (CR 613.8a names "the other" effect): a "lose all abilities" static
effect that also removes its own source's ability keeps applying to other objects. One level
keeps the evaluation finite and deterministic; full layer-major evaluation across objects stays
#186.

**Alternatives:** The base-only fallback of D4 (rejected: wrong result for the acceptance
scenario on any object other than sB); full recursive evaluation (rejected: may not terminate
and is #186's scope); evaluating with the effect included (rejected: a self-removing remover
would stop applying elsewhere).

## #183 FP-144 — Ledger read access

**Decision:** `layers::TimestampLedger` gets one crate-internal read accessor,
`entries() -> LedgerEntries` (borrowed maps), used only by the canonical encoding
(`layers/encode.rs`) and the state's wire form. The ledger is rebuilt on decode through its own
public stamping operations (a generator positioned just before each stored timestamp).
`ContinuousEffects` links are read through the existing `pub(super) links()` from
`layers/encode.rs`; `ManaValueProvenance` parts through crate-internal helpers there.

**Rationale:** The ledger's fields are private and the identifier types have no serde, so serde
derives on the ledger would need field adapters plus a MessagePack round trip just to read the
maps back; a read-only accessor changes no ledger behaviour and is the smallest glue.

**Alternatives:** Serde derives with `#[serde(with)]` adapters (rejected: more code, and the hash
would still need the maps); making the fields `pub` (rejected: exposes mutation).

## #183 FP-144 — Wire structs, not serde derives on layer types

**Decision:** The version-2 wire form of the layer components is a set of explicit wire structs
in `state/effects_wire.rs` (raw `u64` ids, mirror enums of `LayerModification`, `CopyException`,
`Characteristics` and the rest); no serde derive is added to a layer type. An `Option` of a JSON
value (power, toughness) travels as an array of zero or one element so `Some(null)` survives.
Decoding rebuilds the registry through `register` / `register_group` (each anchor with its linked
parts; orphaned `SameAsLayer` parts through a stand-in anchor, id 0, removed at once, as removing
the real anchor did) and requires the rebuilt registry to equal the described one. `serialize`
does not validate: a state made inconsistent through `effects_mut()` or `timestamps_mut()`
serializes but does not deserialize.

**Rationale:** The id newtypes and `Characteristics` (outside the owned paths) have no serde, and
the wire member names are the contract, so mirror structs are the smallest owned change and keep
validation next to the wire. Going through registration makes decode enforce exactly what
registration enforces.

**Alternatives:** Serde derives in `layers/` with `with` adapters (rejected: touches many layer
types for transport only); validating on serialize (rejected: transport only, and it would double
the cost).

## #183 FP-144 — Effect links for groups, removal and timestamps at registration

**Decision:** `GameState::register_effect_group` registers a multi-layer effect with a link per
part (each part checked like `register_effect`). Every static effect with a source needs a link,
`SameAsLayer` parts included. `remove_effect` removes the continuous effect and its link, and also
drops the link of an ETB effect id; it returns whether anything was removed.
`register_effect` checks only what the brief lists (links, 7a, registration); an effect timestamp
not issued by the state is accepted there but rejected by `deserialize`.
`stamp_objects_simultaneously` uses `GameState::apnap_order`: the turn order rotated to start at
the active player (CR 101.4: the other players follow in turn order), the plain turn order when no
active player is set.

**Rationale:** The brief pins the `register_effect` validations; groups and ETB links need the
same link bookkeeping, and a uniform "static with a source has a link" rule is what decode checks.

**Alternatives:** A timestamp check in `register_effect` (rejected: not in the pinned API's
validation list); leaving ETB links on `remove_effect` (rejected: dangling link in the hash).

## #183 FP-144 — Golden state hashes regenerated

**Decision:** The state Merkle tree gains `/timestamps` and `/effect_generators` and the payload of
`/effects` changes (CR-005), so the two golden state digests in `state/tests.rs` are recomputed
once and replaced: `GOLDEN_NEW_42_2_20`
f32bbec462508986efd89ed35f9184bec86a107f774505e19c4f65d3db4fb0d5 ->
7389c79ba648f778a6925577d462382782418b603f487e7eef53a39898a87030; `GOLDEN_MUTATED`
7ebe9f5c17abfc986c0168e1722807514fd1372a1e6dcc10dca594afdfa58371 ->
61143120a7a8869bc2fb299244f20d4d6085bf324fefe398afb16d6cf354cb7c. A new pinned digest,
`GOLDEN_LAYERS` 49b4707ebee9e40df30c49d6ca4fe2289e328f66814ef2b84833e58400ce7cc1
(`state/effects_tests.rs`), covers a non-empty state: two stamped objects, one counter stamp, one
static effect with its link, one resolved effect and one ETB link; a second build of it in another
semantically equal call order hashes equal. There are no golden replay files yet: the replay
corpus does not exist (no `tests/golden/` directory).

**Rationale:** The layout change is intended and recorded as CR-005; the digests are values of
the new frozen layout, recomputed once, not adjusted to make a test pass.

**Alternatives:** Keeping `/effects` as an empty placeholder (rejected: the unit's goal is to hash
the effects).

## #183 FP-144 — LayerCache invalidation table

**Decision:** No cache field is added to `GameState`. The `state::environment` module
documentation maps each `GameState` mutation to the `layers::Invalidation` a cache user declares:
registry changes none (detected; `EffectExpiry` optional on removal), ETB links none, counter
stamps `Timestamp`, other ledger writes `Timestamp`, generator issues and the PRNG none, player and
turn writes `PlayerValue`; world changes (zone, control, counters, attachments, status,
designations) are declared by the world's owner. A differential unit test checks cached equals
full recompute through `GameEnvironment` after each kind of mutation.

**Rationale:** The cache detects registry changes itself; the environment reads the ledger only
for counter timestamps and the world for everything else, so the table follows what
`GameEnvironment` reads. Object timestamps are listed under `Timestamp` to stay safe when a later
environment reads them.

**Alternatives:** Helper methods returning the cause (rejected: the table and the test suffice,
brief D7).

## #183 FP-144 — Order record on B's source in the PR #187 scenario

**Decision:** In the B-before-A timestamp order, evaluated on B's source, the test asserts what the
brief requires: A applies, B does not apply, and B is not ordered after A by dependency
(`Selection::Dependency` / `Selection::Loop`). The outcome-level criterion "no marker on any
object, both orders" is asserted by `pr187_a_before_b_no_object_gets_the_marker` and
`pr187_b_before_a_no_object_gets_the_marker`. Whether the order record must show a CR 613.8a
existence dependency of B on A on B's own source is follow-up #242 (FP-168): it needs a
rules-researcher answer and would change `layers/ordering`.

**Rationale:** The Independent Tester read the rules-researcher's Q6 answer as "A's step precedes
B's" on sB, but the brief's T3 clause says to assert B is not ordered after A on account of
dependency, the issue's acceptance criterion says B is "not ordered after A by a false dependency"
with no marker, and the merged test
`layers::ordering_tests::req_layer_002_removed_generating_ability_stops_effect_both_orders` asserts
the pipeline's order (`[B Timestamp not applied, A Timestamp applied]`). Trial application finds no
dependency because B applies to sB neither with nor without A. Final characteristics are correct.
The brief wins (R10).

**Alternatives:** Changing the ordering pipeline to record the existence dependency now (rejected:
outside this unit, rules question open, tracked in #242); dropping the test (rejected: the order
record still deserves the brief's assertions).

## #184 FP-145 — Audit of characteristic and counter consumers

**Decision:** No rules consumer on the LKI, trigger, replacement-bus or SBA ceasing path reads base
characteristics; nothing in code changed, only the contract was made explicit and guarded. Audit
at `bf8c676` (non-test code of `events/`, `memory/`, `triggers/`, `replacement/`,
`sba/perform.rs`; searched for `CharacteristicsProvider`, `capture_lki_on_move`, `ObjectBase`,
`.base(`, `ObjectMarkers::counters`/`counter(` and `Characteristics` reads):

- `events/exec.rs` `EventContext::capture` → `LkiProvider`: characteristics from the injected
  `EventContext::provider` (**evaluated**, per the documented contract); counters from
  `ObjectMarkers` and attachments from `Permanents` (**raw by design**, existing events decision).
- `events/exec.rs` `held_counters` / `add_counters` / `remove_counters`: `ObjectMarkers` and player
  counters (**raw by design**: the event writes these amounts).
- `events/state.rs` `ObjectMarkers::counters` / `counter`: the single store of counters
  (**raw by design**).
- `sba/perform.rs` `capture_ceasing_lki` → `CeasingLki`: same shape as `LkiProvider`
  (**evaluated** characteristics from `ctx.provider`; counters and attachments raw by design). The
  SBA predicates already read `layers::evaluate` through the caller's effects and
  `LayerEnvironment`.
- `memory/lki.rs` `capture_lki_on_move`: takes the injected provider (**evaluated** when the
  caller follows the contract); its only non-test callers are the two above.
- `triggers/` (`detect.rs`, `event.rs`, `lookback.rs`, `state.rs`, the rest): read no
  characteristics. Selector matching is delegated to `TriggerEnv::object_matches`; `event.rs`
  reads only the LKI record's controller (CR 701.21a sacrifice subject). `ability.base` there is
  the DSL `AbilityBase` field, not an object base.
- `replacement/bus.rs`: reads no characteristics; its only counter read is the
  `requires_counter` gate on `ObjectMarkers::counter` (**raw by design**, CR 122.1d/122.1h-style
  counter conditions count counters, which layers do not change).
- `replacement/etb.rs`: reads `LayerEnvironment::base` only inside the `Projected` adaptor and
  `project`, which feed `layers::evaluate` (CR 614.12) (**evaluated**; exempt from the guard).
- No non-test code builds a provider from base characteristics; the only non-test
  `CharacteristicsProvider` implementations are `LayeredProvider`, `LkiProvider` and `CeasingLki`.

Interpretation of the issue's deliverables: "`capture_lki_on_move` callers pass the layered
provider" is met by the documented, tested contract on `EventContext::provider`,
`CharacteristicsProvider`, `LkiProvider` and `CeasingLki` plus the snapshot recipe, because no
in-repo non-test caller builds a provider and `EventContext` cannot borrow the state it mutates.
"Counter reads use evaluated characteristics": `ObjectMarkers` stays the single raw source, and
layers (CR 613.1-613.4) never change counter amounts, so evaluated and raw counters coincide.

Snapshot recipe (documented on `EventContext` and in `memory`): `let snap = game.clone(); let env =
GameEnvironment::new(&snap, &world); let provider = LayeredProvider::new(snap.effects(), &env);`
built before the context, so before the batch (CR 603.10a, CR 704.8 pre-event capture). An object
created inside the batch is unknown to the snapshot, as to any injected provider (event capture
fails with `MemoryError::NoCharacteristics`; the ceasing capture skips it).

Guard: `memory/consumer_guard_tests.rs` reads the consumer files with `include_str!` (compile
time, no runtime I/O) and asserts their code before `#[cfg(test)]`, with `//` comments stripped,
contains none of `ObjectBase`, `.base(`, `LayerEnvironment::base`; `replacement/etb.rs` is
asserted to call `evaluate(` instead. The same file holds the shared synthetic scene
(`test.lki_granted_type_2_2`) used by the regression tests in `events/tests/lki_layers.rs`
(event execution, batch, SBA ceasing token through the public `sba::perform_all`),
`replacement/tests.rs` and `triggers/tests.rs`; it is declared `pub(crate)` under `#[cfg(test)]`
so those modules can share it. The SBA ceasing test lives in `events/tests/` because
`sba/perform.rs` has no test module inside the region this unit may edit.

Controller: exposing the evaluated (layer 2) controller through `LayeredProvider` and using it in
LKI and the replacement chooser is not done here. Decision "#32 FP-031" item 13 named #184, but
#216 FP-160 owns the evaluated controller in LKI and the chooser; LKI keeps the zone record's
rules-level controller.

**Rationale:** The audit found no defect, so a code change would only add risk; the contract was
implicit ("the layer system plugs in there later") and is now explicit, with a test that fails if
a consumer starts reading bases and regression tests that fail if the injected view is not the
evaluated one (each with a base-only control). Keeping counters raw preserves the existing events
decision that LKI matches what the events did.

**Alternatives:** Building the layered provider inside `EventContext` (rejected: needs new fields
or a borrow of `game` the context already holds mutably; the field set is frozen); routing LKI
counters through `LayeredProvider::counters` (rejected: same amounts, and it would break the
single-source decision); a runtime file-reading guard (rejected: R3).

## #16 FP-015 — Lint API and lint ids

**Decision.** `fineprint_dsl::lint` exposes exactly the brief's API: `Severity` (`Error` <
`Warning`), `LintId` (a private `&'static str`, ordered by its string) with the twelve constants
`FPL-SCHEMA`, `FPL-MANA-ABILITY`, `FPL-TIER-HINT`, `FPL-LAYER-KIND`, `FPL-TEXT-COVERAGE`,
`FPL-LOOK-BACK`, `FPL-MAX-ITER`, `FPL-TOKEN-REF`, `FPL-LINK-ID`, `FPL-MANA-VALUE`, `FPL-COLORS`,
`FPL-COLOR-IDENTITY` and `LintId::ALL` in that order; `LintLocation`, `Lint`, `LintReport`,
`SourceFaceCharacteristics`, `SourceCharacteristics`, `LintContext` and the pure function
`lint_definition`. The report is sorted by (face, ability, id string, severity, span, path,
message), `None` first, then de-duplicated. Ability-level findings carry the ability's own
`textSpan` in `text_span` when it is well-formed and fits `u32`; card-level and face-level
findings other than coverage gaps have none. Messages copy only identifiers from the input
(ability ids, token ids, link ids, member names), clipped to 64 characters; decoder messages of
`FPL-SCHEMA` are clipped to 240 characters; JSON paths in messages show `$` and at most the last
eight segments. The two touches outside `src/lint/` are the `pub mod lint;` line in `lib.rs`
and the regenerated `docs/cr-citations.json`.
**Rationale.** A string-backed id is stable on the wire and sorts the same everywhere. Filling
`text_span` for ability findings is what "locations with text spans" asks for. Clipping keeps
untrusted input from flooding reports (R12) and bounds path rendering whatever the nesting depth.
**Alternatives.** An enum of lint ids (rejected: the string is the contract); unclipped decoder
messages (rejected: an untrusted override could put megabytes of text in one finding).

## #16 FP-015 — Raw versus typed lint inputs

**Decision.** The typed decoders already refuse a `REPEAT_WHILE` without `maxIter`, a `CHAR`
effect without `layer` and look-back keys, so `FPL-LOOK-BACK`, `FPL-MAX-ITER`, `FPL-LAYER-KIND`,
`FPL-LINK-ID`, `FPL-TOKEN-REF` and the span reading of `FPL-TEXT-COVERAGE` run on the raw
`serde_json::Value` of every ability, decodable or not, through one iterative walk (explicit
stack, no recursion). `FPL-MANA-ABILITY`, `FPL-TIER-HINT` and the ability part of
`FPL-COLOR-IDENTITY` run only where `to_real()` succeeds. `FPL-SCHEMA` is emitted in addition
for every position that fails to decode (all positions, not only the first). Partner abilities
get `FPL-SCHEMA`, `FPL-LOOK-BACK`, `FPL-MAX-ITER` and `FPL-TOKEN-REF` (location
`commanderEligibility.partnerAbilities[k]`); the face-specific checks do not apply to them.
**Rationale.** A failing definition then reports the specific lint id and not only a generic
schema error; typed checks need a decoded value to be meaningful.
**Alternatives.** Typed-only checks (rejected: the specific violations would be unreportable);
recursive raw walks (rejected: stack overflow on hostile nesting).

## #16 FP-015 — Mana-ability check baseline

**Decision.** Only `isManaAbility: true` can be a finding. KEYWORD abilities are skipped (macros
expand later); SPELL, STATIC and REPLACEMENT with `true` are Errors (CR 605.1 covers activated and
triggered abilities only). ACTIVATED (CR 605.1a): one Error per violated criterion: requires a
target (any top-level or mode target slot, CR 115.6); loyalty (`isLoyalty: true` or a `LOYALTY`
cost node, `COMPOSITE` included); library movement (`SEARCH_LIBRARY`, `MILL`, `DRAW`,
`PUT_ON_TOP_OR_BOTTOM` anywhere in `effect` or `modes`, a `MOVE` whose `toZone` is `LIBRARY` or
whose `objects` selector names the `LIBRARY` zone, an `EXILE_FROM_ZONE` cost from `LIBRARY`); no
instruction at all (top-level `effect` empty and no mode with an instruction). TRIGGERED
(CR 605.1b): requires a target; the trigger is not an `EVENT` trigger of kind `ACTIVATED` or
`RESOLVED`; no instruction. The loyalty and library criteria are not applied to triggered
abilities. `None` and `false` are never findings.
**Rationale.** This DSL baseline has no add-mana instruction and no "mana added to a pool" event,
so "could add mana" can be neither proved nor disproved from structure; the check is weakened to
"has some instruction" and the triggering event set to the two events that exist. Implementing
the CR text, not the looser SRS paraphrase, keeps library and loyalty criteria off triggers.
**Alternatives.** Reporting `None`/`false` on abilities that might add mana (rejected:
undecidable, would be noise). Follow-up: add a mana-production instruction and a "mana added"
event, then make the check strict in both directions.

## #16 FP-015 — Tier-hint check baseline

**Decision.** A typed REPLACEMENT with a `tierHint` is an Error if `match.event` is not `ENTERS`
(a `ZONE_CHANGE`-based hint is rejected in this baseline) and, separately, if every
`replaceWith` operation is `SKIP` or `PREVENT`. An absent hint is never a finding (616.1e);
`isSelfReplacement` (616.1a) is not compared with the hint.
**Rationale.** CR 616.1b–d all concern how an object enters the battlefield, which the DSL
models as the `ENTERS` event; an operation that only skips or prevents modifies nothing about the
entry. The wire catalogue has no field naming a control, copy or back-face modification, so the
three hints cannot be told apart structurally.
**Alternatives.** Accepting `ZONE_CHANGE` with a `to: BATTLEFIELD` filter (rejected for now: two
spellings of one event). Follow-up: a wire convention naming the modified aspect, then check each
hint against 616.1b, 616.1c and 616.1d individually.

## #16 FP-015 — Text coverage

**Decision.** `textSpan` is an inclusive token index range `[start, end]` into `face.text`
(N = raw array length). A well-formed span (two non-negative integers, `start <= end`) with
`end >= N` is an Error at the ability, and still covers its in-range part; a present but
malformed span (wrong type, wrong length, negative, `start > end`) is also an Error at the
ability and covers nothing. Each maximal uncovered run is one finding at `faces[i].text` with
`text_span = (first, last)`: Error for `VERIFIED` and `AUTO_PARSED`, Warning for `PARTIAL`. For
`UNSUPPORTED` the whole check is skipped. Overlaps are fine; reminder text is not excluded.
**Rationale.** REQ-ING-009 requires full coverage for `AUTO_PARSED` and above; a `PARTIAL`
definition is expected to have gaps but they are still worth surfacing. Treating malformed spans
as "any other out-of-range span" keeps a client override from hiding a gap behind garbage.
**Alternatives.** Half-open spans (rejected: SRS §14.3 says "[start, end]" and the typed decoder
allows `start == end`); ignoring malformed spans here (rejected: they would only surface as
`FPL-SCHEMA`).

## #16 FP-015 — Linked abilities structurally

**Decision.** Declarations are the string `linkIds` of every face ability (all faces form one
card, CR 607.1b); references are objects `{"ref":"LINKED","linkId":<string>}` at any depth of any
face ability. A reference to an id no ability declares is an Error at the referencing ability.
An id declared by exactly one ability and referenced by none is a Warning at that ability. An
ability that declares and references the same id is linked to itself (CR 607.1c) and passes;
ids declared by several abilities, and abilities declaring several ids, pass (CR 607.4).
**Rationale.** It is the structural reading of "linked abilities share a `LinkId`": every
reference resolves, and a lone declaration is suspicious but could be a deliberate unpaired
link in a partial definition.
**Alternatives.** Error for a lone declaration (rejected: too strong for an unpaired link that
the toolchain cannot prove wrong).

## #16 FP-015 — Source characteristics and token ids

**Decision.** Source data reaches the lints through `LintContext`: `source` (per-face integer
mana value and colors, and a color identity) and `known_token_ids`. An absent input skips the
checks that need it with no finding: `FPL-MANA-VALUE` and `FPL-COLORS` need `source` with a
value for that face index (extra or missing entries are ignored); the source comparison of
`FPL-COLOR-IDENTITY` needs `source.color_identity`; `FPL-TOKEN-REF` needs `known_token_ids`.
`FPL-COLOR-IDENTITY` always compares with the definition's own `colorIdentity`.
**Rationale.** The lint is a pure function; the caller converts the source's floating `cmc` to an
integer (REQ-ING-002 forbids it as a rules value). Skipping silently keeps definitions without a
source (client overrides) lintable.
**Alternatives.** A finding "source missing" (rejected: noise for every override).

## #16 FP-015 — Characteristics checks

**Decision.** Mana value per face is integer (`u64`, saturating): generic n, X/Y/Z 0 (202.3e),
`{n/c}` max(n, 1) (202.3f), every other symbol 1 (202.3g for Phyrexian). No mana cost is 0
(202.3a); the `BACK` face of a `DFC_NONMODAL` card uses the cost of its first `MAIN` face
(202.3b); `LEFT_HALF`/`RIGHT_HALF` faces of a `SPLIT` card each have the combined cost of all
halves (202.3d, off the stack); `MELD_RESULT` faces are skipped (202.3c is a runtime property).
Colors per face are the colors of its cost symbols (hybrid and Phyrexian symbols count all their
colors, 202.2d) plus its color indicator (204.2). Color identity is the union over all faces
(903.4d) of cost colors and indicators, plus the symbols of every `PAY_MANA` or `WATERBEND` cost
node (`{"cost":...,"mana":...}`) found anywhere in a decodable ability's JSON. For colors and
identity, a computed color missing from the declared/source value is an Error and a
declared/source color that cannot be derived is a Warning. LITERAL text tokens are never scanned
for symbols.
**Rationale.** Asymmetry: a computed color is certain, while an extra declared color may come
from a characteristic-defining ability or rules text this baseline does not compute (105.2,
903.4). Reminder text (903.4c) cannot be told apart reliably in LITERAL tokens, so scanning them
would produce false identity. Cost nodes in a decodable ability are typed `CostAST` values;
scanning the decoded-and-canonical JSON reaches every one (activated costs, `COMPOSITE`, RULE
costs, instruction `unless`/`altCost`/`cost` lists) without a 72-op visitor, and a cost node
inside an opaque parameter is still a mana symbol of the rules text.
**Alternatives.** Scanning LITERAL text (rejected, above); identity from activated costs only
(rejected: misses other typed costs). Limitations: colors and identity from CDAs and from rules
text that is not a typed cost are not computed.

## #16 FP-015 — Spec-test judgement calls

**Decision.** (1) The deep-nesting spec test uses 700 instruction levels (about 2100 nested JSON
containers) and 1200 bare array/object levels, on the default test stack. (2) The spec path
invariant allows `text_span` on `faces[i].text` and on ability paths only. (3) For an empty text
stream (N = 0) only "no tokens and no spanned abilities gives no finding" is asserted. (4) "Error
for each violated criterion" (FPL-MANA-ABILITY) is asserted as at least one finding per violated
criterion, not an exact count. (5) The no-copied-prose marker is not used as a token or link id,
since a message may legitimately name an unknown token id.
**Rationale.** (1) `serde_json`'s recursive Drop aborts near 1000 instruction levels and the
existing `AbilityAST::to_real` overflows between 800 and 900, so deeper tests would test those,
not the lint; a bigger test stack would hide real recursion problems. (2) The implementation
attaches the ability's own span to ability-level findings, which the brief does not forbid. (3)
The brief's "N = 0: no coverage finding" and "end >= N is an Error" conflict for a span on empty
text. (4)–(5) The brief pins neither an exact count nor message wording.
**Alternatives.** Running on a bigger stack; spans only on `faces[i].text`; asserting either N = 0
outcome; exact finding counts.

## #186 FP-146 tester - which generating-ability calls the foreign-source view tests assert

**Decision:** The B-6 spec test (`spec_layers_cross_object_view.rs`) asserts, for a static
effect whose source sS is not the object asked for, that *some* `generating_ability_exists` call
gets the exact foreign view (sS through the end of the effect's own layer, the effect left out),
and that *every* call for that effect respects the bounds that hold whatever the moment: `Some`
view, layer-2 controller applied, the effect's own result absent, no result of a later layer. It
does not require every call to show the same-layer results, because the effect also affects sS,
and in a layer-major pass the engine legitimately asks about it for sS as the object in progress
(before the same-layer grant applied), which a black-box test cannot tell apart. For an effect
that does not affect its source (the layer-6 one), every call must equal the exact foreign view.
Likewise, value-expression calls are asserted only for the object the test asks about when other
CDA objects exist (their own 7a calls are not specified by the brief).

**Rationale:** Brief B-6 defines the foreign view and keeps the in-progress view "as before" for
the object being evaluated; layer-major evaluation (the deliverable) processes every object, so
in-progress calls for the source are expected. The chosen assertions keep the full foreign view
mandatory and make every call obey the layer bounds.

**Alternatives:** Requiring every call to equal the foreign view (rejected: contradicts the
in-progress rule for the source itself); making the layer-4 effect not affect its source
(rejected: the brief asks that "the effect itself, if it modifies the source, is not reflected in
its own view").

## #186 FP-146 — Additive view API instead of a new `eval_value` signature

**Decision:** The read view is a new public type `layers::EvaluatedView<'a>` (`get(id)`,
`objects()` ascending, `layer()`) and a new defaulted trait method
`LayerEnvironment::eval_value_with_view(expr, object, view)`, whose default calls the unchanged
`eval_value(expr, object)`. The layer pipeline calls only `eval_value_with_view`, and only for a
non-literal power or toughness (`literal_of` is read directly). `eval_value`, `MatchContext` and
every other signature are unchanged; `start_layer_7` (crate-private) gains the view parameter.
**Rationale:** Changing `eval_value` would break `state/environment.rs`, `replacement/etb.rs`, the
benches and many test environments, all outside #186's owned paths, and would be a breaking change
of a widely implemented trait. A defaulted method keeps every existing implementor compiling with
its old behaviour, and an environment opts in by overriding one method.
**Alternatives:** Change `eval_value` to take the view (rejected: breaking, outside owned paths);
pass the view through `MatchContext` (rejected: the brief keeps `MatchContext` unchanged and a CDA
is not a selector match); a view handed to every trait method (rejected: no requirement needs it).

## #186 FP-146 — Board pass, universe and view semantics

**Decision:** `evaluate`, `evaluate_with_order`, `evaluate_all` and `copiable_values` run one
layer-major board pass (`layers/board.rs`). Universe: `env.objects()` sorted ascending and
de-duplicated, plus the object asked for, keeping only objects whose `base` is `Some`; an object
without a base is absent from every view and is an error only for the object asked for
(`UnknownObject`), or, in `evaluate_all`, for the first listed object without a base by ascending
id. Objects are always processed by ascending `ObjectId`; each keeps its own `Scratch`, its own
`LockRecord` (CR 613.6, never shared across objects; a clone is used only for a nested evaluation
of the same object) and its own order record. Within a layer each object is ordered against its
own state only (the dependency trial, CR 613.8a, stays on the object under evaluation) and reads
the others only through the start-of-layer snapshot, so no result depends on which object of the
layer runs first. CDA view (CR 604.3, 613.4a, 613.5): frozen at the start of layer 7a before any
object's layer 7 starts; every object after layers 1a-6; power and toughness as after layer 6 (a
CDA object's own expression is still the raw expression): a CDA reading another object's layer-7
values is not supported (no recursion, no cycles). Existence view (CR 613.8a, 611.3, 113.6): for
the object under evaluation its state so far (unchanged); for another source of the universe, the
source evaluated from its start-of-layer state and lock-in record through the end of the effect's
own layer with the effect left out, every other step of that layer for the source applied in the
REQ-LAYER-002 order, and inside that nested run the existence of effects of yet other sources is
asked with their start-of-layer state (one level deep, terminating). A source outside the universe
still gets `None`; resolved and sourceless static effects are never asked. The foreign view is
computed lazily per (layer, effect) and memoised in a pass-local `RefCell` map: a pure function of
the start-of-layer snapshot, so the memo never changes a result. `copiable_values` is the layers
1a-1b prefix of the same board pass (same universe and existence views).
**Rationale:** The issue asks that a CDA see the current controller and the earlier-layer results
of every object and that the existence question get the foreign source's partially evaluated
state. A plain start-of-layer snapshot of the source would miss a same-layer removal (the #183
scenario: B's source loses B's ability to A in layer 6), so the source is run through the end of
the effect's own layer without the effect, as the #183 `SourceEnvironment` already did for all
layers. Stopping at the effect's own layer keeps a removal in a later layer from undoing an effect
that already applied in an earlier one. Ascending iteration and snapshots give REQ-CORE-001
determinism and `evaluate(id) == evaluate_all()[id]`.
**Alternatives:** Start-of-layer snapshot only (rejected: fails the #183 same-layer trap); the
source through all layers without the effect (rejected: a later-layer removal would retroactively
stop an earlier-layer effect; also more work); full recursive nesting (rejected: may not
terminate); eager computation of every foreign view at each layer start (rejected: more
environment calls for effects that match no object, same results).

## #186 FP-146 — Follow-up: the game-state and entering-permanent environments do not forward the view

**Decision:** `state/environment.rs` (`GameEnvironment`, `SourceEnvironment`, `ObjectWorld`) and
`replacement/etb.rs` (`Projected`) still use the default `eval_value_with_view`, so a real
`GameState` evaluation does not yet hand the `EvaluatedView` to its `ObjectWorld`; adding an
`ObjectWorld::eval_value_with_view` (or equivalent) and forwarding it from `GameEnvironment`,
`SourceEnvironment` and `Projected` is outside #186's owned paths and is requested from the
Orchestrator as a follow-up. The existence view needs no forwarding: `GameEnvironment` and
`Projected` already receive and use `source_view`, so `GameEnvironment` now judges a source of the
universe on the layer system's foreign view, and its own one-level fallback evaluation (#183
"Existence on another source") is reached only when no view is given, that is for a source with no
base (which then has no abilities).
**Rationale:** R1: those files are not owned by #186. The default keeps their behaviour exactly as
before for value expressions.
**Alternatives:** Edit the state and replacement environments in this unit (rejected: outside the
owned paths).

## #186 FP-146 — Statements superseded

**Decision:** This unit supersedes, without editing them: (1) "#33 FP-032 — Scope of the trial
application": its rationale "evaluation is per object, so the partially evaluated state of other
objects is not available" no longer holds; the decision itself (dependency judged on the object
under evaluation only, the trial never evaluating other objects) is kept, and the deferred
alternative is realised only as the read views above. (2) "#33 FP-032 — Review round 1
dispositions" (a): "`source_view` is the state so far only when `src` is the object being
evaluated, `None` otherwise" now reads: `Some` for every source of the universe (the foreign view
for another object), `None` only for a source with no base. (3) "#36 FP-035 — Coarse invalidation
and the (stateVersion, ObjectId) key" and "#36 FP-035 — Scope against #183 FP-144 and #186
FP-146": the cache stays standalone and coarse invalidation stays sound; each cache miss now runs
one board pass of `evaluate`; `LayerCache::evaluate_all` keeps its per-object hit/miss path
(statistics unchanged; batching misses into one pass is left to #190 FP-147). (4) "#183 FP-144 —
Existence on another source": the `GameEnvironment` fallback described there is now used only
without a view (see the follow-up section above).
**Rationale:** R8: earlier entries are not rewritten; this section names what changed.
**Alternatives:** Editing the earlier entries (rejected: DECISIONS.md is append-only).

## #186 FP-146 — Existing unit test of the foreign-source exemption updated (R10)

**Decision:** `layers/ordering_tests.rs::req_layer_002_existence_exemptions_both_orders` asserted
that a static effect whose source is another object with a base (object 2) is asked with
`source_view == None`. Brief R-4 requires `Some(view)` for any source in the universe, so the
case now uses a source outside the universe (object 9, no base), for which the brief keeps `None`;
the assertions are otherwise unchanged. The in-universe foreign source is covered by
`layers/cross_object_tests.rs` (`req_layer_001_foreign_source_view_contents`,
`req_layer_001_same_layer_removal_on_foreign_source`).
**Rationale:** R10: a test that contradicts the brief is corrected to assert exactly what the
brief requires; the exemption it checks (no view, treated as existing by that environment) still
exists for sources outside the universe.
**Alternatives:** Keep object 2 and assert the new view (rejected: with that environment the
grant would stop existing and the test would no longer test the exemption).

## #186 FP-146 — `LayerCache::evaluate_all` visits objects by ascending id

**Decision:** `LayerCache::evaluate_all` now looks objects up in ascending `ObjectId` order
(`env.objects()` sorted, duplicates kept, so a listed duplicate is still looked up again and
counted as before) instead of the environment's order, so its first error is the one of
`layers::evaluate_all` (the first object without a base by ascending id, brief R-2/R-5) for any
order of `env.objects()`. Hit/miss counting per lookup, version bumps and "entries computed before
the first error stay stored" are unchanged. Each miss is still one `evaluate(id)` board pass;
batching all misses into one pass is left to #190 FP-147. The unit test
`layers/cache_tests.rs::evaluate_all_first_error_rule` (environment listing 1, 50, 2, 60 with no
base for 50 and 60) asserted one stored entry (object 1, computed before 50 in the listed order);
in ascending order objects 1 and 2 are computed before 50, so it now asserts two (R10: corrected to
the brief's first-error rule; the error and the equality with `evaluate_all` are unchanged).
**Rationale:** With layer-major evaluation `evaluate_all` sorts its universe, and the brief
requires the cache to equal it and results to be independent of the order of `env.objects()`
(REQ-CORE-001); the environment's order is documented as ascending anyway.
**Alternatives:** Keep the environment's order (rejected: the cache's first error would differ
from `evaluate_all`'s for an unsorted list); de-duplicate as well (rejected: would change the hit
count for a listed duplicate).

## #16 FP-015 — Deep-input spec tests build and drop fixtures without recursion

**Decision.** Supersedes item (1) of "#16 FP-015 — Spec-test judgement calls". Deep-input spec
tests build their fixtures by moving values into `serde_json::Map` / `Value::Array` (no `json!`
interpolation of deep values, no `clone` of them) and drop them with an explicit-stack helper, so
their stack use does not depend on depth. `req_ing_009_deep_nesting` now runs at 5000 instruction
levels and 20 000 bare array/object levels on the default test stack.
**Rationale.** The 700-level test overflowed on `test (windows-latest)`. The issue-investigator
measured the cause as the test's own fixture code (`json!` interpolation deep-copies through
`serde_json::to_value`, plus recursive `clone` and Drop); `lint_definition` and every `to_real`
decoder use constant stack (about 183 KiB) up to 50 000 levels. The earlier notes that serde Drop
aborts near 1000 levels and `to_real` overflows at 800–900 were measurements of the fixture code.
**Alternatives.** A larger-stack test thread (rejected: hides real recursion); a lower depth
(rejected: weaker R12 coverage and still platform-dependent).

## #17 FP-016 — Test and loader location

**Decision.** The loader lives at `tests/fixtures/cards/loader.rs` (inside the issue's owned paths)
and is included with `#[path]`. The library test is `crates/fineprint-dsl/tests/spec_dsl_card_library.rs`,
the one file outside the issue's listed owned paths.
**Rationale.** The root `tests/` directory is not a Cargo target, and `fineprint-dsl` owns the card
type and the lints the test exercises.
**Alternatives.** A new crate for the library (rejected: needs a coverage-floor arm and a workspace
change).

## #17 FP-016 — Mana abilities use NATIVE_EFFECT test.add_mana

**Decision.** Land cards produce mana with `NATIVE_EFFECT` id `test.add_mana` version 1, params
`{"mana":"{W}"}` etc., with `isManaAbility` true and a `TAP_SOURCE` cost.
**Rationale.** The DSL has no add-mana instruction; a native effect is the sanctioned escape (R4).
**Alternatives.** Inventing an `ADD_MANA` op (rejected: not in the DSL schema).

## #17 FP-016 — Face name equals cardDefId; seal in place

**Decision.** Every face `name` equals `cardDefId`, so a real card name cannot appear. New cards are
written with `"definitionHash": ""` and sealed by the `#[ignore]` test `seal_library`, which rewrites
every library file as pretty JSON (2-space indent, trailing newline).
**Rationale.** Hand-computing SHA-256 of RFC 8785 bytes is error-prone; the generator is the only
writer and uses the same `seal()` the loader verifies against.
**Alternatives.** A build script or xtask command (rejected: outside owned paths).

## #17 FP-016 — Opaque ENCHANT and EQUIP keyword params

**Decision.** `ENCHANT` carries `{"what":"CREATURE"}` and `EQUIP` carries `{"cost":"{1}"}` as opaque
params until keyword macros exist. Equipment also has an explicit sorcery-speed `ACTIVATED` attach
ability (PAY_MANA {1}, target a creature you control), so both forms are present.
**Rationale.** Keyword params are opaque in the current DSL; the explicit ability gives scenarios a
runnable attach path independent of macro expansion.
**Alternatives.** Leaving out the keyword or the activated ability (rejected: the brief asks for both).

## #17 FP-016 — Other fixture judgement calls

**Decision.** (1) The any-target selector of `burn_3_any_target` is an OR of `IS_CREATURE`,
`IS_PLAYER` and `HAS_CARD_TYPE` `{"name":"Planeswalker"}` with no `zones` member (a player is in no
zone). (2) The five colored mana lands declare `colorIdentity` equal to their produced color (CR 903.4,
per the rules-researcher); `mana_land_colorless` stays 0. `FPL-COLOR-IDENTITY` cannot yet derive
produced-mana symbols (DECISIONS "#16 FP-015 — Characteristics checks"), so
`req_ing_009_every_card_is_lint_clean` allows exactly one `FPL-COLOR-IDENTITY` warning on exactly those
five cards (and none elsewhere) until #252 (FP-172) removes the allowance; their identity is asserted
directly against the table, with the source identity omitted from their lint context to keep the
warning count at one. (3) The no-card-data test matches quoted member names so the allowed
`oracleTextHash` does not match `oracleText`. (4) `load(id)` finds the file by stem across families.
**Rationale.** Each is the simplest choice consistent with the lints as implemented.
**Alternatives.** Declaring land identity 0 (rejected: wrong under CR 903.4).

## #190 FP-147 — Equivalence spec tests: per-object checks on the 200-object board use a sample

**Decision:** In `tests/spec_layers_perf_equiv.rs`, the per-object checks on board a (200
permanents) — `evaluate(id)` equal to the `evaluate_all` entry, `evaluate_with_order` and the
pinned `OrderRecord` steps, `LayerCache::evaluate(id)` — run on a fixed sample of 14 objects
(CDA, plain creatures, artifacts, lands, counters, locked sets); the small boards check every
object. The cache equivalence (requirement 4) and the `evaluate_all` bookkeeping (requirement 5)
on board a share one cache sequence (cold, warm, invalidation, registry change) so board a pays
as few cold fills as possible, and the order-independence test fills a cache per listing only on
board b (board a's listings are checked uncached against the golden digest). Board a's "fixed
shuffle" is the literal permutation formula `i -> 1 + (77 i mod 200)` rather than 200 written-out
ids (board b's shuffle is written out). Goldens were recorded at 1360900.
**Rationale:** On the base (1360900) one whole-board pass of board a takes about 0.9 s in a debug
test build, and each `evaluate(id)` and each cold cache miss is one such pass (an O(n) fill of n
passes), so checking all 200 objects per step would take tens of minutes; the brief asks for a
fast run (requirement 10) and the whole-board digests already cover every object.
**Alternatives:** Every object of board a in every check (rejected: far too slow on the base);
a smaller "bench-like" board (rejected: the brief fixes 200 permanents).

## #190 FP-147 — Equivalence spec tests: board c changes the counted set in layers 2 and 4

**Decision:** Board c ("CDA and view") changes the set counted by its characteristic-defining P/T
with a layer-2 control change and layer-4 type changes, and puts layer-7b and 7c effects on the
counting objects and on the animated artifacts in an order that depends on the timestamps (both
orders tested).
**Rationale:** The brief asks for "a layer-7b/7c effect that changes the counted set", but under
the documented contract the CDA reads a snapshot of all objects after layers 1a-6, and 7b/7c
effects change neither control nor types, so they cannot change the counted set; layers 2 and 4
are the documented way the counted set changes, and the 7b/7c effects still interact with the CDA
values (7a before 7b/7c, timestamp order within 7b).
**Alternatives:** A 7b/7c effect alone (rejected: it cannot change what is counted, so the board
would not test the view).

## #190 FP-147 - Whole-board cache fill: baseless ids, misses and what is stored

**Decision:** `LayerCache::evaluate_all` keeps its lookup loop (ids of `env.objects()` sorted, not
de-duplicated; a hit or a miss per lookup, as in "#186 FP-146 - LayerCache::evaluate_all visits
objects by ascending id"), but every miss of one call is answered from one board pass, run
lazily at the first miss, over `env.objects()` sorted, de-duplicated and restricted to the ids
that have a base (`board::universe_known`, the universe `evaluate(id)` uses for every listed id
with a base, contract A). An id without a base is not in that pass: its lookup is a miss and
returns `UnknownObject(id)`, which, by ascending order, is the first error of the uncached
`evaluate_all`. Each result is stored when its lookup is visited, so the entries of ids visited
before the error stay stored and the results of ids after it are discarded (not stored), exactly
as when each miss was its own `evaluate(id)`. A listed duplicate is a second lookup: a hit in
cached mode, a miss in full-recompute mode answered with the same result (no second pass). In
full-recompute mode the call runs the same single pass and stores nothing; every lookup is a
miss. `LayerCache::evaluate(id)` is unchanged and stores only its own object.
**Rationale:** Contract A of the brief makes the pass result for every listed id with a base equal
to `evaluate(id)`, so values, errors, hit/miss counts and `len()` stay exactly as at 1360900 while
a cold fill costs one pass instead of N. Storing only visited ids keeps `len()` after an error
equal to the old behaviour (`evaluate_all_first_error_rule` and the spec tests assert it).
**Alternatives:** Store every object of the pass even after an error (rejected: changes `len()`
and which later lookups hit after an error, a visible change of the cache's bookkeeping); run the
pass eagerly on every call (rejected: a fully warm call would pay a pass for nothing); keep N
passes in full-recompute mode (rejected: the brief allows one pass, results are identical and the
differential job compares the values, not the number of passes).

## #190 FP-147 - Foreign-source views computed before the layer runs

**Decision:** In a board pass, the view of each static effect of the layer whose source is on the
board (its source evaluated through the end of the layer without the effect, one level deep,
CR 613.8a) is computed for every such effect before any object runs the layer, from the objects'
states at that moment, instead of lazily (memoised in a `RefCell`) from a copy of every object's
state and lock-in record taken at the start of each layer. The nested run reads other sources'
start-of-layer states from the same, still unmodified, slots.
**Rationale:** The view is a pure function of the start-of-layer snapshot (the binding semantics
of "#186 FP-146 - Board pass, universe and view semantics"), so computing it eagerly gives the
same value as computing it on first use, while removing the per-layer copy of the whole board.
It may compute a view no object asks for (for example when the effect applies only to its own
source); that is one single-object layer run per such effect, at most `E` per pass.
**Alternatives:** Keep the lazy memo and the per-layer snapshot (rejected: the copy costs every
object's state and lock record in every layer); lazy views read from slots not yet run (rejected:
needs a snapshot anyway for sources that already ran the layer).

## #190 FP-147 - Lazy dependency relation and comparison shortcuts

**Decision:** Within a layer the dependency relation (CR 613.8a) is evaluated lazily, cell by cell
in row order, until the earliest step that depends on no other unapplied step is found; trial
applications are cached for the relation's lifetime (one selection); the whole relation is
computed only when no step is independent (a loop, CR 613.8b). The set and ability-list
comparisons of "what B does" return early in cases where the definition's answer is already
certain: the same value after B in both states, B left the field untouched in both, or (ability
lists) B appended the same items in both. The relation is still recomputed after every
application (CR 613.8c).
**Rationale:** Every cell is a pure function of the steps, the current state and the lock-in
record, so the order in which cells are evaluated cannot change a cell, the earliest independent
step or the loop chosen; the shortcuts are implied by the definitions. Unit tests compare the lazy
relation with the eager pre-change relation (kept under `cfg(test)`) in every cell order and the
shortcuts with the definitions, and golden fingerprints recorded at 1360900 pin the results.
**Alternatives:** Memoise trials across selections (rejected: the state changes after every
application and the lock-in record grows, so reuse would need a proof per case for little gain);
restrict which fields a modification can touch (rejected: a second description of every
modification that could drift from `apply_effect`).

## #193 FP-148 — Independent tester: golden replays not applicable

**Decision.** No golden replay check was run for this unit. `crates/fineprint-core/tests/golden/` does
not exist on main (DECISIONS "#183 FP-144 — Golden state hashes regenerated"), and the new target
`spec_layers_cache_diff_state_check.rs` pins no golden hash value, only cached == reference equality.
**Rationale.** Nothing to replay; the brief says to record this.
**Alternatives.** Creating a golden file (rejected: outside owned paths and forbidden by the brief).

## #193 FP-148 — Independent tester: evaluation feedback into the state hash

**Decision.** Each step writes, per player, `life` (first 12 hex digits of `evaluated_digest` of the
objects that player controls) and the player counter `test.power` (sum of literal evaluated powers)
through `player_mut`, to both the cached and the reference state. These feedback writes are not
followed by `Invalidation::PlayerValue`, because the wrapper world and effects used read no state
player value; a declared `PlayerValue` there would flush the cache and hide the stale-cache cases.
Player-value changes the wrapper does read (`player_value`) and `set_turn_number` /
`set_active_player` writes are declared with `PlayerValue`.
**Rationale.** The cache is not part of `GameState`, so the hash can depend on evaluation only through
fed-back state; the table's `PlayerValue` is for values a CDA or condition reads.
**Alternatives.** Declaring `PlayerValue` after each feedback (rejected: makes the stale test vacuous).

## #193 FP-148 — Invalidation declared by the scenarios (implementer)

**Decision.** The scenarios of `spec_layers_cache_diff_state.rs` declare the `Invalidation` values of
the `state::environment` table themselves, after every mutation (`ZoneChange`, `ControlChange`,
`CounterChange`, `Attachment`, `Status`, `Designation`, `Timestamp`, `PlayerValue`, `EffectExpiry`;
registrations are left to the cache's own registry detection except one scenario that declares
`EffectRegistration`).
**Rationale.** Issue #241 FP-167 (calling `LayerCache::invalidate` from the real zone, control,
counter, attachment and status mutation points) is open and not a dependency. On main the `GameState`
holds no objects, so the world is a test double owned by the scenarios; nothing here has to change
when #241 lands.
**Alternatives.** Waiting for #241 (rejected: not a dependency, and the differential needs to exist
now); declaring every invalidation after every step (rejected: hides missing declarations).

## #193 FP-148 — State hash fed by cached evaluations (implementer)

**Decision.** After each evaluation a scenario writes the evaluated characteristics into the state
through `player_mut`: life = 20 + total evaluated power of the objects the player controls, and the
player counters `test.abilities` and `test.types` (numbers of evaluated abilities and card types). The
cached state is fed from the cached evaluation, a cloned reference state from the uncached
`layers::evaluate_all`. The feedback writes are followed by `Invalidation::PlayerValue` (they are
player writes) and by a second, re-warming cached evaluation, so the next mutation is declared
against a warm cache and an undeclared one is observable. Players' `test.rank` counter, written by the
scenarios, is what the wrapper world's selectors and CDA read. Per-step mismatches are collected; the
`.statehash` file is written first, then equality is asserted.
**Rationale.** The cache is not part of `GameState` and `state_hash` hashes only the state, so
evaluating alone cannot change it; only fed-back evaluations make the hash a function of the cache.
Without the re-warm, the required `PlayerValue` after the feedback would empty the cache and make
every stale case vacuous.
**Alternatives.** Hashing the evaluated map next to the state (rejected: not `GameState::state_hash`);
omitting `PlayerValue` after feedback (rejected: the brief requires it).

## #193 FP-148 — Golden replays not applicable (implementer)

**Decision.** No golden replay check is added. There is no `tests/golden/` directory and no golden
replay corpus on main (see "#183 FP-144 — Golden state hashes regenerated": "There are no golden
replay files yet"); replay verification lands with #83 FP-081 and #106 FP-104, so the issue's third
deliverable has nothing to check yet. No test pins a new golden value of `state_hash`.
**Rationale.** Nothing to replay.
**Alternatives.** Creating a corpus here (rejected: outside owned paths, belongs to those units).

## #195 FP-150 — Typed REPLACE_TEXT_WORD accessor, no new wire rejection

**Decision:** The DSL keeps `REPLACE_TEXT_WORD` as an opaque op on the wire. `Modification` gains
only `REPLACE_TEXT_WORD` (the op name), `replace_text_word(category, from, to)` (builds the flat
params `category` as its wire name, `from`, `to`) and `as_replace_text_word()`, a typed view that
returns `Ok(None)` for other ops, `Ok(Some(..))` for a well-formed one and `Err(message)` for a
missing, non-string or unknown category, a missing or non-string word, any other member, or typed
power/toughness. `from_json`, `check` and the canonical bytes are unchanged; `CARD_NAME` and
empty or noncharacter words are returned as parsed.
**Rationale:** REQ-DSL-012 needs a typed model, but adding a rejection to the decoder would change
the set of documents that parse (a breaking contract change, R2). The engine already owns the CR
612.2 checks (`validate_modification`), so the DSL view stays a pure reader and every refusal
happens once, in the compile step.
**Alternatives:** A dedicated `Modification` variant or typed fields (rejected: breaking wire and
struct change); rejecting malformed or `CARD_NAME` replacements in `from_json` (rejected: new
wire rejection, and the CR 612.2 rule belongs to the engine).

## #195 FP-150 — Compile step lives in fineprint-core layers

**Decision:** `compile_text_modification` and `compile_text_effect` are in
`fineprint_core::layers` (`layers/compile.rs`). They map a well-formed `REPLACE_TEXT_WORD` to
`LayerModification::ReplaceTextWord` and run the same `validate_modification` registration uses,
so `CARD_NAME` (CR 612.2) and invalid words give exactly the registration error and every `Ok`
result registers. Malformed members give `Unsupported("REPLACE_TEXT_WORD with malformed
members")`, any other op `Unsupported("not a text-changing modification")`, and
`compile_text_effect` additionally refuses a layer other than 3 with
`Unsupported("REPLACE_TEXT_WORD outside layer 3 (CR 613.1c)")`, checked after the modification's
own errors. `from == to` compiles (ordinary no-op).
**Rationale:** The dependency direction is base <- dsl <- core: `LayerModification` lives in core,
so only core can produce it. Reusing `validate_modification` keeps one source of truth for the
CR 612.2 rules.
**Alternatives:** A compile step in fineprint-dsl (rejected: dsl cannot depend on core); a new
error variant for malformed members (rejected: `LayerError` is shared; `Unsupported` with a
static message is what the brief and registration already use).

## #195 FP-150 — Gap G4 closed by resolve_value_words

**Decision:** The private `resolve_value` in `layers/text.rs` is made public, unchanged in
behaviour, as `fineprint_core::layers::resolve_value_words`. Word references reachable only from
a value expression (outside any predicate or selector) are resolved by calling it against the
evaluated text table. The interpreter wiring (#57 FP-055) just calls it when it evaluates a value
expression of an object whose text may have changed; no further engine work is needed for G4.
**Rationale:** The function already walked value expressions for the predicate and selector
resolvers; exporting it closes G4 with no new code path and the same error rules.
**Alternatives:** Placing the whole of G4 in the interpreter (rejected: duplicates the walk);
a new generic AST visitor (rejected: larger change, outside this unit).

## #195 FP-150 — Category and words chosen on resolution

**Decision:** "With the categories and values chosen on resolution" (SRS 14.5) means the resolving
interpreter (#57) builds the modification with the chosen literal values through
`Modification::replace_text_word` and compiles it with `compile_text_modification` /
`compile_text_effect`. This unit adds no choice wiring and no choice-reference members in the
modification.
**Rationale:** The choice framework and `CREATE_CONTINUOUS_EFFECT` resolution belong to the
interpreter; a literal modification keeps the compile step pure and total.
**Alternatives:** Choice references (for example `{"choice": ...}`) in `category`/`from`/`to`
resolved by the compile step (rejected: needs game state and choice results, which the compile
step must not read, R3).

## #195 FP-150 tester — Independent Tester judgement calls

**Decision:** (1) The DSL spec tests do not assert the accessor's result for an empty or
noncharacter `from`/`to` (the brief lists those as compile-step rejections only); the core spec
tests assert `LayerError::Unsupported` for them. (2) "Documents that parsed before still parse"
is tested by requiring every malformed `REPLACE_TEXT_WORD` member set that the opaque-params
parser accepted (missing member, wrong JSON type, unknown category, extra param) to still parse
and re-serialise unchanged, with the accessor returning `Err`. (3) The acceptance scenario follows
the brief's token list literally (a `COLOR_WORD` word "Swamp" read by a `HAS_SUBTYPE` predicate),
since no category-compatibility check exists (DECISIONS "#37 FP-036 — Word replacement scope").
(4) Requirement 11(a) mirrors both #37 assertions for the compiled form: the chain
(Island->Forest, both words Forest in every order) and the swap loop (Island->Swamp, timestamp
order).
**Rationale:** Each choice asserts exactly what the brief and the #37 decisions pin, never less.
**Alternatives:** Asserting accessor rejection of empty words (rejected: not in the brief's Err
list); a land-type word for the acceptance scenario (rejected: departs from the brief's token list).

## #198 FP-151 — Per-part CDA targets for copy exceptions

**Decision:** (a) A layer-4 `CHAR` effect's modification may carry `defines`, a non-empty array of
distinct names from `"SUPERTYPES"`, `"CARD_TYPES"`, `"SUBTYPES"` (validated on decode and write
for every layer-4 `CHAR` effect, opaque on other layers; typed view `TypePart` and
`CharacteristicEffect::defined_type_parts`). A layer-4 CDA without `defines` defines all three
parts, so every existing definition keeps the conservative behaviour; in fineprint-core a
malformed `defines` (which decoding refuses) also counts as all three. (b) JUDGEMENT CALL, the CR
is silent on multi-characteristic CDAs: a CDA is not copied as a whole as soon as it defines any
characteristic the exception sets, retains or does not copy, so a CDA defining card types and
subtypes is dropped by an exception touching either and kept by one touching only supertypes.
(c) `copy_with` uses one target per type-line part (`Supertypes`, `CardTypes`, `Subtypes`):
`Set*`/`NotCopied` of one part drop only the CDAs that define that part, so a subtype-only CDA
survives `SetSupertypes`. (d) `AddSupertypes`, `AddCardTypes` and `AddSubtypes` keep every type
CDA. This refines "#38 FP-037 — CDA abilities and CR 707.9d" (which dropped any layer-4 CDA on
any type exception); the rest of that decision stands.
**Rationale:** (a) the "absent = all three" fallback keeps #38's behaviour and every current
test valid. (b) CR 707.9d attaches the consequence ("is not copied") to the whole
characteristic-defining ability, and the CR has no partial-copy mechanism. (c) CR 109.3 lists
card type, subtype and supertype as separate characteristics and CR 205.1b treats them
separately, so 707.9d's "that characteristic" applies per part. (d) CR 707.9d last sentence: for
"in addition to its other types" exceptions, any CDA that defines card type, supertype and/or
subtype is copied.
**Alternatives:** Keep one `Types` target (rejected: too broad, drops a subtype CDA when only
supertypes are set); copy a multi-part CDA partially (rejected: no rule support); key the parts
on a new `op` vocabulary (rejected: ops are free strings in the DSL).

## #198 FP-151 — Change request number

**Decision:** The contract change requested as "CR-003" is recorded as `CR-006` in
`contracts/CHANGE_REQUESTS.md`.
**Rationale:** CR-003 to CR-005 already exist and the file is append-only; reusing a number would
make two entries ambiguous.
**Alternatives:** Reusing CR-003 (rejected: duplicate id); editing the existing CR-003 (rejected:
append-only, R2).

## #198 FP-151 — `TypePart` reached through the `statics` module

**Decision:** `TypePart` is defined in `crates/fineprint-dsl/src/ability/statics.rs` and
re-exported from `fineprint_dsl::ability` (the Orchestrator added `ability/mod.rs` to the unit's
owned paths for this one change). fineprint-core's copy exceptions parse a layer-4 `defines`
value with `TypePart::parse_defines`, so the wire vocabulary lives only in the DSL; any `Err`, or
an absent value, still means all three parts.
**Rationale:** One source for the vocabulary; the behaviour is identical to the earlier direct
JSON read (absent, non-array, empty, non-string, unknown, wrongly-cased and duplicate all fall
back to all three parts). Core already depended on fineprint-dsl.
**Alternatives:** Core reading the `defines` JSON itself (rejected: duplicated vocabulary);
moving `TypePart` to another exported module (rejected: `statics` is where layer-4 CHAR
modifications are defined).

## #203 FP-152 tester — Independent Tester judgement calls

**Decision:** (1) The ETB projection returns only the entrant's evaluation, so requirement 8's
"A2 (P2's artifact) affected and A1 not" is observed on the entrant itself: an artifact entering
under P1 (entry and environment controller P1, as the projection requires) is given to P2 by a
layer-2 effect already in force; its own or copied static "artifacts you control are creatures"
must make it a creature, and a second static "artifacts you don't control gain a synthetic type"
must not apply to it (that second static is what the old controller's perspective would select).
(2) Beyond the brief's list, the layer tests add a stale stored controller case (stored P1,
source's base controller P2, no layer-2 effect: "you" is P2, CR 109.5 "current controller"),
a stolen-and-returned case and a case where both the candidate and the source change control;
each follows from the brief's `MatchContext::effect_controller` contract.
**Rationale:** Asserts exactly the brief's contract through the public API, never less.
**Alternatives:** Evaluating A1/A2 separately with `evaluate_all` inside the ETB file (rejected:
that tests the layer evaluator, already covered, not the projection's copied statics).

## #203 FP-152 — Controller of a sourced static effect

**Decision:** (1) The controller a selector sees for a static effect with a source is the source's
controller in the current layer pass: the candidate's own state so far when the source is the
candidate, otherwise the source's view the board pass already builds for the existence question
(the source evaluated through the end of the effect's own layer without the effect; one level
deep, other sources at their start-of-layer state). For layers 3-7d this is the post-layer-2
controller. For layers 1a, 1b and 2 it is the source's state so far in that layer (no
fixed point is sought for a control-changing static whose source itself changes control in
layer 2). (2) The stored `ContinuousEffect::controller` is the fallback when the source is not in
the universe or its evaluated controller is `None` (CR 108.4a, 109.5 owner semantics remain the
registering caller's job). (3) The foreign source's state reaches `pipeline::applies_to` through
a new provided method `LayerEnvironment::static_source_state` (default `None`), answered by the
board pass's `BoardEnv` (its existing per-effect views) and `NestedEnv` (start-of-layer slots);
`MatchContext` is unchanged. (4) `replacement::etb::bind_to` keeps storing `entry.controller`,
now only a fallback: the bound static's source is the entrant, so the evaluator reads the
entrant's post-layer-2 controller (CR 614.12, 110.2, 613.1b).
**Rationale:** CR 109.5 and 611.3a (rules-researcher, #203): "you" of a static is the current
controller of the object it is on and the effect is not locked in; layer 2 precedes layers 3+
(CR 613.1b), and timestamp/dependency order only within a layer (CR 613.7, 613.8a), so both
timestamp orders give the same result. Reusing the existence-question view keeps one source
evaluation per layer and effect, so the cached and full-recompute paths and the dependency trial
(`step_applies` is shared) agree with no extra cost. Layers 1-2 have no completed layer-2 result
to read; the state so far is the only value available without iteration.
**Alternatives:** Passing the source state through `MatchContext` (rejected: the brief freezes
its field set); a pipeline-side wrapper environment capturing views (rejected: `LayerCache` calls
`run_board` directly, so cached and uncached results would differ); a fixed-point iteration for
layer-2 statics whose source changes control in layer 2 (rejected: no rule excerpt asks for it,
cost and termination risk); overwriting the stored controller at each control change in the
state (rejected: state mutation outside the evaluator and not layer-aware).
**Owned paths:** `crates/fineprint-core/src/layers/board.rs` (only the two `static_source_state`
overrides of `BoardEnv` and `NestedEnv`) was added to the owned paths by the Orchestrator (R1
extension), because `LayerCache` calls `run_board` directly, so a pipeline-only wrapper would
desynchronise cached and uncached results.

## #203 FP-152 — Existing spec test corrected to the static-controller contract

**Decision:** `crates/fineprint-core/tests/spec_layers_each_layer.rs`
`selector_sees_layer_2_result_and_context_fields` (Orchestrator-granted owned-path extension)
registered a static with stored controller P2 whose source was controlled by P1 and asserted
`effect_controller == P2`, the stored value. It is corrected (R10) to put the source under P2
and register the static with a stale stored controller P1, still asserting
`effect_controller == P2` for every candidate, plus every original assertion.
**Rationale:** Brief #203 requirement 1 (REQ-LAYER-002 deliverable 1; CR 109.5, 611.3a): a
sourced static's controller is the source's evaluated controller. The corrected test now asserts
that contract (a stale stored value must not be seen), so it is stronger, not weaker.
**Alternatives:** Moving the source under P2 and keeping stored P2 (rejected: would no longer
distinguish stored from evaluated controller); leaving the test (rejected: contradicts the brief).

## #208 FP-154 — Owner orders simultaneous graveyard entry (CR 404.3)

**Decision.** (a) Opt-in additive API in `events/`: `EventContext::graveyard_order_groups(batch)`
lists the `GraveyardOrderGroup`s of a final (post-replacement, CR 614.6) batch, ascending by owner,
leaving every component unchanged; `GraveyardOrderGroup::request(id)` builds the owner's
`ORDER_CARDS` request; `GraveyardOrderGroup::order_from_response` maps the answer to a
`GraveyardOrder`; `EventContext::execute_batch_ordered(batch, orders)` executes the batch with the
chosen orders. `execute_batch`, `execute` and `execute_batch_with_entry_copies` are unchanged
(same graveyard order, ids and log). New error `EventError::InvalidGraveyardOrder { owner }`.
(b) Group rule, from the planning run: executed `ZoneChange` events (not draws) requested at
`Position::Top`, that change zones (`old != new`: a graveyard-to-graveyard reposition is not "put
into"), into a graveyard (CR 400.3: the owner's), of a nontoken card (CR 111.6, CR 108.2b; a token
takes no part and does not count toward the two, CR 111.7). One group per owner with two or more
such cards. `Bottom` and `Index` moves keep their requested position and never form or join a
group. JUDGEMENT CALL beyond the brief: a card that a later event of the same batch moves again
(out of the graveyard or repositioned, possible only through a predicted id) is not counted,
since its arrival position no longer exists at the end of the batch. (c) Encoding: one
`ORDER_CARDS` request per group, chooser the owner, `reason.cr = "CR 404.3"`, the cards ascending,
`destinations = [Graveyard]`, `min == max == n`, public (`hidden: false`, a graveyard is public),
options in the shape `generate_request` gives an `ORDER` choice (the cards as enumerated options
while `n!` is below `ENUMERATION_LIMIT`, a `ValidatorRule::Permutation` contract otherwise; that
rule's doc says "in top", but validation is driven by the payload, whose only destination is
`Graveyard`). The answer is `SUBMIT_ORDERING` with empty `top`/`bottom` and the `graveyard` list
top first (index 0 = topmost). (d) The timeout default is the order `execute_batch` yields,
"submission order of arrival": each card goes on top in submission order, so the last-submitted
card ends on top. (e) Placement: the group's cards take the positions ("slots") they occupy in
the `execute_batch` result, in the chosen order; every other object of the graveyard keeps its
`execute_batch` position. The group is therefore contiguous on top of the graveyard unless
another object of the same batch landed between its cards (a token, which ceases to exist at
the next state-based-action check, CR 111.7, or an `Index` move, which keeps its requested
position); in that case that object keeps its place. The arrangement is done with same-zone
repositions (CR 400.7: no new object ids, no events), so `ExecutedBatch` (events, outcomes, ids)
and LKI are exactly those of `execute_batch`. (f) Multiple owners: one request per owner with a
group, asked in ascending `PlayerId` order; callers pass any request ids. (g) Invalid orders
(owner without a group, owner twice, not a permutation of the group's cards) fail with
`InvalidGraveyardOrder` naming the lowest offending owner (checked in ascending owner order, so
the result does not depend on entry order) and change nothing. (h) No contract change:
`ORDER_CARDS` with the `graveyard` destination is already in SRS 15.3; `generate_request`'s
`ORDER` kind offers only `Top`, and a graveyard-offering `ChoiceKind` would be a later additive R2
item. (i) Deferred: CR 401.4 (library: a hidden choice with positions and the non-reveal rule)
and CR 405.3 (stack: APNAP stack code outside `events/`). Wiring the choice into
`sba/perform.rs` `perform_all` and `replacement/bus.rs` is a follow-up issue; until then they keep
ascending `ObjectId` order (#41 FP-040 follow-up (1)).

**Rationale.** An additive API keeps every existing caller, test and golden replay identical
(the default equals today's result) and lets the callers that own SBA and the bus adopt it later.
Computing groups from the planning run uses exactly the moves the real run makes, after
replacement (REQ-EVENT-001). The slot placement honours the brief's "Bottom/Index keep their
requested position" for every other object, makes the default a no-op, and gives contiguity in
every case where only the group's cards arrive at the top; the CR gives no sequencing between
owners (CR 404.3 does not cite CR 101.4) and the piles are independent, so ascending `PlayerId` is
a deterministic choice with no rules effect.

**Alternatives.** Reordering inside `execute_batch` itself (rejected: changes every caller and the
golden corpus, and needs a choice context in the executor). Permuting the execution order of the
group's moves (rejected: assigns different object ids than `execute_batch` and still leaves
interleaved objects between the cards). Forcing a contiguous block that pushes interleaved
objects below it (rejected: moves `Index` arrivals away from their requested position and makes
an explicitly submitted default differ from `execute_batch`). APNAP sequencing of owners
(rejected: not stated by CR 404.3; ascending `PlayerId` is recorded instead). Using
`Zones::reorder_graveyard` (rejected: gated by the CR 404.2 tournament flag).

## #211 FP-157 — Retire ceased CardIds in Zones

**Decision:** `Zones` has a private `retired: Arc<BTreeSet<CardId>>` (empty in `Zones::new`,
written with `Arc::make_mut`). `cease_to_exist` inserts the removed object's CardId after the
removal, only on success; `send_outside` and `move_object` do not retire. New
`ZoneError::CardCeased(CardId)`. Precedence: after `require_player` (an unknown player still gives
`UnknownPlayer`) and before every other check: `create_object` rejects before `CardAlreadyPresent`
and the controller check; `enter_from_outside` rejects before `NoSuchOutsideCard` (a retired card
is in no pool, so the caller learns the real reason); `add_outside_card` also rejects before
`CardAlreadyPresent` (judgement call: otherwise a retired card could re-enter through a pool).
All checks run before any mutation, so the container and generator are unchanged on error. The
retired set is part of `PartialEq` and of the canonical encoding, written (length prefix, ascending
CardIds) after the command zone and before `max_issued`, which stays the last 8 bytes. `state/`
and every golden state hash are unaffected because `Zones` is not in the state hash yet; the
encoding of a `Zones` with an empty set grows only by the 8-byte zero length prefix.
`events::ObjectMarkers` is deliberately not cleared: a retired id is now unreachable through
`Zones`, so the stale token mark cannot be hit. The perform.rs dungeon-removal placeholder (CR
704.5t) calls `cease_to_exist` until #215 adds an outside pool, so it retires that card; #215 must
switch it to `send_outside`. The "#41 FP-040" sentence "enforcement ... is a known follow-up" is
now done (that entry is append-only and unchanged).
**Rationale:** CardIds are match-scoped and never reused (REQ-CORE-002); the engine never reissues
one, but `Zones` did not enforce it. Unlike ObjectIds (issued by a generator `Zones` sees, where
the #167 high-water mark suffices and tracking every id was rejected as unbounded), CardIds are
supplied by the caller and need not be dense, so an exact set is the only precise way; it is
bounded by the number of tokens and copies that ever ceased in a match.
**Alternatives:** Clearing the token mark on cease (rejected: outside owned paths, and would
silently allow id reuse); a high-water CardId mark only (rejected: CardIds are not issued by
`Zones` and need not be dense); rejecting only in `create_object` / `enter_from_outside`
(rejected: `add_outside_card` would be a back door).

## #216 FP-160 — Evaluated controller through the CharacteristicsProvider

**Decision:** The layer-2 controller (CR 613.1b) reaches LKI capture and the CR 616.1 replacement
chooser through a new provided method `CharacteristicsProvider::controller(id) -> Option<PlayerId>`
(default `None`), overridden by `LayeredProvider` with `evaluate(..).ok().and_then(|e|
e.controller)`. `capture_lki_on_move` uses it only for an object leaving the battlefield, falling
back to the zone record's controller; the stack and every other zone keep
`ObjectRecord::rules_controller`. `Bus::chooser` uses it only for an affected permanent, falling
back to `Permanent::controller`; a non-permanent keeps its rules-level controller or owner (CR
108.4a). The stored `Permanent::controller` and the zone record are not changed, and destinations
stay owner-based (CR 400.3). Contract entry CR-008 (additive). The wrappers `LkiProvider`
(events/exec.rs) and `CeasingLki` (sba/perform.rs) must forward the method for the event-driven
and SBA paths to benefit; that is a follow-up in their owning unit.
**Rationale:** The injected provider is already, by the caller's contract, the layer-evaluated
view, so it is the single source of truth every consumer shares, mirroring the SBA registry's
"evaluated, else stored" rule (PR #214, F4). A defaulted method keeps every implementor
source-compatible and leaves base-only fakes on the old behaviour. The LKI encoding is unchanged;
only the recorded value can differ.
**Alternatives:** Syncing `Permanent::controller` from layer 2 after each evaluation (rejected: no
state-level caller exists that runs the layers and writes back, and it would need edits in
permanent/, events/ and the state driver, outside this unit); computing the controller separately
in each consumer from a `ContinuousEffects` reference (rejected: duplicates the evaluation and
gives consumers no common fallback rule); consulting the provider for stack objects too (rejected:
the layer system's controller for a spell is not a control change the brief covers, and CR 109.4
behaviour is to stay unchanged).

## #209 FP-155 - SBA destruction cause

**Decision.** A new zone-change cause `ZoneChangeCause::StateBasedDestroy` (canonical tag 8; tags
0-7 unchanged) marks a permanent destroyed by the lethal-damage (CR 704.5g) or deathtouch
(CR 704.5h) state-based action: such a permanent is "destroyed" (CR 701.8b). `perform_findings`
submits the move of a `SbaAction::Destroy` finding with `StateBasedDestroy`; `PutIntoGraveyard`
findings (CR 704.5f, CR 704.5i, CR 704.5k, ...) and the legend-rule and unprotectable-battle moves
keep `StateBasedAction`. In the CR 704.7 merge, one object still moves once per pass, with the
cause of the highest fixed precedence, whatever the order of its findings or pre-inserts:
`Sacrifice` (CR 704.5s, decision #42 FP-041) over `StateBasedDestroy` (the object was destroyed,
CR 701.8b) over `StateBasedAction`; every finding is still reported in `SbaOutcome.findings`.
Trigger normalization maps `StateBasedDestroy` exactly like `Destroy` (zone-derived kinds plus
`DESTROYED`), so "whenever a creature is destroyed" listeners fire for CR 704.5g/h deaths and not
for CR 704.5f/i deaths. `Destroy` (destruction as the result of an effect) remains the only cause
the shield counter's destroy replacement matches (CR 122.1c); `shield_counter_destroy` is
unchanged, so a shield counter still does not stop a CR 704.5g/h death. Consequences for
replacement matchers (which compare `cause` by equality): a matcher keyed on `StateBasedAction`
no longer matches CR 704.5g/h moves; a matcher keyed on `StateBasedDestroy` does not match
CR 704.5f/i moves; a regeneration hook (CR 701.8c, not modelled yet) would match
`StateBasedDestroy`. A state containing a matcher keyed on the new cause hashes differently from
any existing state; existing states and goldens are unchanged.

This entry supersedes the "Alternatives" remark (and the sentence "Every move uses
`ZoneChangeCause::StateBasedAction`") in "#41 FP-040 — CR 704.7 merging per subject and result
kind", and the remark "a separate 'SBA destroy' cause (not needed ...)" in "#29 FP-028 — SBA
destruction and the shield counter's destroy replacement". The rest of the FP-028 decision holds:
a mixed `Destroy` + `StateBasedAction` group applies the shield to the `Destroy` member only.

**Rationale.** CR 701.8b: a permanent is destroyed by an effect that uses the word "destroy" or by
the state-based actions that check for lethal damage or deathtouch damage; a permanent put into
its owner's graveyard for any other reason has not been destroyed. Triggers on destruction must
therefore see CR 704.5g/h deaths, while CR 122.1c limits the shield counter to destruction as the
result of an effect. A distinct cause expresses both facts with the existing equality matcher and
an explicit precedence keeps the merge deterministic (R3).

**Alternatives.** A flag on `StateBasedAction` (rejected: the replacement matcher compares a plain
cause by equality, so the flag would either be invisible to matchers or change the meaning of
every existing `StateBasedAction` matcher). Making SBA destruction `Destroy` (rejected: the shield
counter would then wrongly replace it; see decision "#29 FP-028 — SBA destruction and the shield
counter's destroy replacement"). Letting the first finding decide the merged cause (rejected:
depends on registry order instead of a rule).

## #44 FP-043 — turn/ module stub

**Decision.** `crates/fineprint-core/src/turn/` did not exist on main, so this unit creates
`turn/mod.rs` with only the module documentation, `pub mod heartbeat;`, the `#[cfg(test)]` test
module line and the re-exports of the heartbeat API, and adds `pub mod turn;` to `lib.rs`
(between `triggers` and `txn`). Unit #46 FP-045 (the turn sequencer) appends its own module to
`turn/mod.rs`.
**Rationale.** R1: when two units share a module in one phase, the earlier unit pre-stubs the
module declarations. Keeping the stub minimal leaves FP-045 a clean append.
**Alternatives.** A top-level `heartbeat` module (rejected: the issue's owned path is
`turn/heartbeat.rs`); leaving the declarations to FP-045 (rejected: this unit's code would not
compile).

## #44 FP-043 — Heartbeat seam: HeartbeatHost and SbaStage

**Decision.** The heartbeat (`turn::heartbeat`) is written against a `HeartbeatHost` trait: the
host supplies the game state (turn order, active player, designations), one state-based-action
pass, trigger placement over its own waiting queue, the stack (empty test, resolve top), the
legal priority actions as exact `ChoiceResponse`s and taking an action. The heartbeat changes
only the priority player (`GameState::set_priority_player`) and never touches the stack. A
host's `sba_pass` delegates to `sba_pass_post_batch(SbaStage { .. }, choices)`, which calls
`sba::perform_all_post_batch` with the host's `PostBatchState` provider and never `perform_all`
or `perform_all_relayered` (binding issue comments, "Gate 02-state-objects R3c"). The heartbeat
keeps its pass state in its own `HeartbeatState` (pass set and pass log), not in `GameState`, so
the canonical state hash is unchanged. The default `has_left_game` is "not a player of the game,
or has the `LOST_THE_GAME` designation". When the priority player has left the game while
deciding (a host that models leaving at any time), the answer is ignored and priority passes on
(CR 800.4a); no pass is recorded for a player who is no longer in the game.
**Rationale.** The stack, resolution, casting, legal-action generation and the real-state
environments arrive in later units ("#31 FP-030 — Real-state environments belong to the
integration unit"); a trait seam lets the heartbeat be finished and tested now with fakes and
keeps it a pure function of (host, state, answers) for the `ChoiceSession` re-execution model.
**Alternatives.** Adding stack and pass fields to `GameState` (rejected: changes the state hash
and is owned by other units); calling `sba::perform_all` directly (rejected by the binding issue
comment).

## #44 FP-043 — Settle loop repeats on performed, not findings, and caps iterations

**Decision.** `settle_before_priority` repeats step 1 while `SbaOutcome.performed` (never on
findings alone, "#41 FP-040 — performed means state changed"). After step 2 it returns to step 1
when the placement stacked OR removed any trigger; only an empty placement (nothing stacked,
nothing removed) ends the settle. `SettleReport.triggers_stacked` counts stacked triggers only.
Every host call of the settle (one SBA pass or one placement) counts one iteration; the call
that would exceed `MAX_SETTLE_ITERATIONS` (10 000) is not made and the settle returns
`ChoiceError::Procedure` naming the limit. With no player in the game, step 2 is skipped (there
is no APNAP order and no player who can control a trigger) and the settle ends after step 1.
**Rationale.** Rules-researcher ruling Q4 (binding, replaces the brief's provisional "removal
does not repeat"): CR 603.3c and CR 603.3d remove a trigger "from the stack", so it was put on the
stack and counts as processed; CR 603.3b then has the game "once again" check state-based
actions. The cap guards against a host that never settles without affecting any legal game
(R3: deterministic error, never a hang).
**Alternatives.** Repeating only on stacked triggers (the brief's provisional choice, overruled
by Q4); counting only loop rounds rather than host calls (equivalent bound, less precise).

## #44 FP-043 — Priority player is None when the step ends

**Decision.** When every player still in the game passes in succession with an empty stack,
`run_heartbeat` sets the priority player to `None` and returns `HeartbeatOutcome::StepEnds`,
doing nothing else; when no player is in the game it also sets `None` and returns
`NoPlayerInGame` without asking. Expiring "until end of step" effects and emptying mana pools
(CR 500.5) is the turn sequencer's (#46 FP-045).
**Rationale.** CR 117.4: "if the stack is empty, the phase or step ends"; no player holds
priority between steps (CR 117.3a: the active player receives priority at the beginning of most
steps, after turn-based actions). Leaving a stale priority player would let a later reader think
someone still holds priority. The 117.3d mana announcement is informational only (ruling Q6);
the pass record is the engine's record.
**Alternatives.** Keeping the last passer as the priority player (rejected: misleading);
performing the CR 500.5 work here (rejected: the sequencer owns step boundaries).

## #44 FP-043 — Priority start when the active player has left or is absent

**Decision.** Whenever the active player would receive priority (the step start, CR 117.3a;
after a resolution, CR 117.3b) and has left the game, the next player in turn order after the
departed active player's seat who is still in the game receives it (CR 800.4j with CR 800.4a);
APNAP order for trigger placement starts at that same player and continues in turn order over
the players still in the game. If `TurnInfo::active_player()` is `None` (no seat is known),
both start at the first player in turn order who is still in the game. If the player about to
receive priority left during the settle, priority goes to the next player in turn order after
them (CR 800.4a).
**Rationale.** Rules-researcher ruling Q5 (a) and (b): CR 800.4j says "the next player in turn
order receives priority"; the CR is silent on APNAP order without an active player, and starting
both at the same player keeps one notion of "who acts first". With no active player at all the
turn order's first in-game player is the only seat-free deterministic choice.
**Alternatives.** Starting APNAP at the first player in turn order even when the departed active
player's seat is known (rejected: inconsistent with the priority rule); failing with an error
when there is no active player (rejected: CR 800.4j lets the turn continue without one).

## #44 FP-043 — Mana abilities and special actions reset PassCount

**Decision.** Every non-pass answer to `PRIORITY_ACTION` (casting, activating any ability
including a mana ability, playing a land, any special action) goes through
`HeartbeatHost::take_action`; the actor keeps priority, PassCount is reset to 0 and the heartbeat
returns to step 1. Binding, no longer provisional.
**Rationale.** Rules-researcher ruling Q2 (an inference; the CR has no explicit sentence): CR
117.4 counts passes only "without taking any actions in between", CR 117.3c and CR 116.3 give
the actor priority afterward, and CR 605.3b says a mana ability "doesn't go on the stack" but is
still an activation. SRS 5.1's "do not reset PassCount beyond the action they are part of" is read
as: they reset it exactly once, as the action they are. (The SRS discrepancy is recorded by the
Orchestrator.)
**Alternatives.** Leaving PassCount unchanged for mana abilities and special actions (rejected
by Q2: a player who acts has not passed "in succession").

## #44 FP-043 — Pass counting uses the players still in the game

**Decision.** `HeartbeatState` keeps the set of players who passed since the last action,
resolution or step start. Before every comparison the heartbeat forgets the passes of players
who have left the game; `pass_count()` is the size of that set (the number of players still in
the game who passed in succession), and a `PassRecord.pass_count` is that value right after the
pass. "All players passed" is `pass_count >= players_in_game`. A departure can therefore never
make the all-passed test succeed before every remaining player has passed.
**Rationale.** Rules-researcher ruling Q5 (c) (CR silent; judgement call, changes the brief's
plain counter): CR 117.4 "all players pass in succession" is about the players in the game, and
CR 800.4j has the turn continue without the departed player.
**Alternatives.** A plain counter compared with the current players-in-game count (rejected: a
departed player's earlier pass would still count, so a remaining player could be skipped);
resetting PassCount to 0 on any departure (rejected by Q5: it makes players who already passed
pass again for no rules reason).

## #44 FP-043 tester — Independent Tester judgement calls

**Decision.** The black-box heartbeat spec tests (`crates/fineprint-core/tests/spec_turn_heartbeat*.rs`)
read the brief as follows where it left room:

1. `SettleReport.sba_passes` counts every `sba_pass` call of the settle (performed or not), and
   `triggers_stacked` the total length of the `stacked` lists the placements returned.
2. A settle loop that only removed triggers (CR 603.3c-d) returns to the SBA check (rules-researcher
   ruling Q4, which replaces the brief's provisional "does not loop").
3. With no active player at all (`active_player() == None`), priority and the APNAP list start at the
   first in-game player in turn order (brief); with a departed active player they start at the next
   in-game player after that player's seat (ruling Q5a/Q5b).
4. `run_heartbeat` with nobody in the game returns `NoPlayerInGame` on entry: no request and no host
   call (no settle).
5. Ruling Q5c (a departed player's pass no longer counts) is tested on the `PassRecord`s and on the
   all-passed decision, with a host that concedes a player between two passes inside
   `priority_actions` (no other host call happens between passes). `HeartbeatState::pass_count()`
   takes no host, so it is asserted only at asks where the heartbeat has observed every departure
   (after a pass it handled), not at the ask during whose `priority_actions` a concession happens.
6. The settle guard is checked by counting the fake's calls: `sba_pass` calls within
   `MAX_SETTLE_ITERATIONS ± 2` for an always-performing SBA pass; `place_triggers` calls within
   `[MAX_SETTLE_ITERATIONS / 2 - 2, MAX_SETTLE_ITERATIONS + 2]` for an always-stacking placement (an
   iteration may count one SBA pass or one SBA-plus-placement round).
7. Property invariant (e) counts triggers stacked by the settle as pushes: resolutions = initial
   depth + pushes by actions + stacked triggers.

**Rationale.** Each choice is the plainest reading of the brief, the SRS 5.1 diagram and the
rules-researcher rulings; item 5 follows from the pinned signature `pass_count(&self) -> usize`.

**Alternatives.** Counting only performed passes in `sba_passes` (rejected: the field name counts
passes run); asserting `pass_count()` at the ask right after a concession (rejected: not observable
through a host-free accessor); excluding triggers from invariant (e) (rejected: the fake host's
placements push onto the stack the heartbeat later resolves).

## #46 FP-045 — TurnHost seam

**Decision.** The turn sequencer (`turn::steps`) is written against a `TurnHost` trait, a
supertrait of `HeartbeatHost`: the host begins turns, phases and steps, expires effects at an
`ExpiryPoint`, empties mana pools, applies phasing, runs the day/night check, reports untap
options and untaps, draws, reports hands and maximum hand sizes, discards, performs the CR 514.2
actions, reports Sagas and adds lore counters, and runs the combat-step actions (a no-op default).
The sequencer itself keeps only `TurnState` (the shared `HeartbeatState` and the previous turn's
active player) and never mutates `GameState` directly; `TurnState` is not part of the canonical
state hash.
**Rationale.** It mirrors "#44 FP-043 — Heartbeat seam": the real-state host, the stack, effect
durations and zones' turn-based moves belong to later units, and a trait seam lets this unit be
finished and tested now with a recording fake, while staying a pure function of (host, state,
answers) for `ChoiceSession` re-execution (R3, REQ-CORE-001).
**Alternatives.** Driving `GameState` and the zone and effect registries directly (rejected: the
required state does not exist in core yet and other units own it); one monolithic callback per
step (rejected: the call order of CR 502 and CR 514 would not be enforced by the sequencer).

## #46 FP-045 — CR 514.3a needs-priority test

**Decision.** After the CR 514.1 and CR 514.2 actions of each cleanup step the sequencer runs
`settle_before_priority` once and gives priority (runs the heartbeat, then another cleanup step)
iff `sba_passes > 1 || triggers_stacked > 0 || !stack_is_empty()`. Otherwise the step ends with no
priority. A placement that only removed triggers (CR 603.3c-d) makes the settle loop back to
another SBA pass, so `sba_passes > 1` and it counts as "needs priority".
**Rationale.** "#44 FP-043 — Settle loop repeats on performed, not findings": the settle repeats
only when a state-based action was performed or a trigger was stacked or removed, so a settle with
more than one SBA pass is exactly "state-based actions were performed and/or triggers were put on
the stack" (CR 514.3a). A non-empty stack (an object left from the end step through an unusual
host) also needs priority to resolve. The settle performs the SBAs and stacks the triggers in the
order CR 514.3a gives; the heartbeat's own settle then finds nothing new.
**Alternatives.** A separate "would anything happen" query on the host (rejected: duplicates the
settle and the trait would grow); treating a removed-only placement as nothing (rejected: the
settle's own loop already treats it as processed, and the trigger was put on the stack first).

## #46 FP-045 — First-turn draw: Two-Headed Giant out of scope

**Decision.** `first_turn_draw_skipped` skips only on turn number 1, for the host's starting
player, when the rule says so: `AlwaysSkip` skips, `NeverSkip` never does, and `CrDefault` skips
iff `game().player_count() == 2` (CR 103.8a). CR 103.8b (the Two-Headed Giant starting team) is
not modelled: core has no team structure yet, so in a Two-Headed Giant game `CrDefault` does not
skip (player count 4). With no known starting player nothing is skipped.
**Rationale.** The brief fixes this rule; teams belong to a later multiplayer unit, which can
extend the check through the host.
**Alternatives.** A host query "is the starting team" (rejected: not in the fixed API and no team
state exists).

## #46 FP-045 — Host-owned turn state

**Decision.** The day/night designation and its cast-spell history, mana pools (including rule
modifications of how mana empties, SRS 613.11), hands, maximum hand sizes, effect durations,
marked damage, phasing and the current phase/step are owned by the host. The sequencer only tells
the host when (CR 500.4, 500.5, 502, 504, 514) and passes `previous_active` (the previous turn's
active player, `None` on the first turn) to the day/night check.
**Rationale.** Core has no such state yet; owning it here would duplicate later units' state and
change the canonical hash.
**Alternatives.** Adding day/night and mana-pool fields to `GameState` (rejected: not an owned
path, and contracts change only through R2).

## #46 FP-045 — Segment, run_phase and run_segment split

**Decision.** A `Segment` is a step or a main phase (main phases have no steps, CR 505.2).
`run_turn` = `begin_turn` + `run_phase` for each phase; `run_phase` does the phase boundary work
(`begin_phase`, `BeginPhase` expiry, segments, `EndPhase` expiry, mana) and `run_segment` the step
boundary work (`begin_step`, `BeginStep` expiry, body, `EndStep` expiry, mana) with no phase work;
a main-phase segment has no step boundary work (its boundary is the phase's). The cleanup segment
repeats the whole step sequence per cleanup step and reports `cleanup_steps`. `run_phase` and
`run_segment` read the active player from the game and fail with `ChoiceError::Procedure` when
there is none.
**Rationale.** Later units (combat, extra phases and turns, the game loop) need to run single
phases and steps; the split keeps CR 500.4 and CR 500.5 at exactly one place per level and lets
"until end of combat" expire at `EndPhase(Combat)` (CR 500.5a).
**Alternatives.** One monolithic `run_turn` (rejected: extra combat phases and turn-structure
changes could not reuse it).

## #46 FP-045 — Registry actions take &mut dyn TurnHost

**Decision.** `TurnBasedAction::run` takes `&mut dyn TurnHost` (fixed API) while the entry
points are generic over `H: TurnHost + ?Sized`. An unsized `H` cannot be coerced to a trait
object, so `TurnHost` also has the supertrait `AsDynTurnHost` (one method,
`as_dyn_turn_host`), blanket-implemented for every sized `TurnHost` and available on
`dyn TurnHost` through the supertrait. Implementors of `TurnHost` never implement it by hand.
**Rationale.** Keeps every pinned signature unchanged and invisible to host implementors.
**Alternatives.** A private forwarding adapter struct implementing all 30 host methods (rejected:
large, error-prone boilerplate); requiring `H: Sized` (rejected: changes the pinned signatures).

## #46 FP-045 — Hash of Segment and ExpiryPoint

**Decision.** `events::Phase` and `events::Step` do not implement `Hash` (an un-owned file), so
`Segment` and `ExpiryPoint` implement `Hash` by hand over the enum discriminants of the value and
of its step or phase, instead of `#[derive(Hash)]`. The other derives are as pinned.
**Rationale.** The fieldless discriminants identify the values exactly, so `Hash` agrees with the
derived `Eq`; the public trait set is the pinned one.
**Alternatives.** Adding `Hash` to `Phase` and `Step` (rejected: outside the owned paths; can be
requested later and the manual impls removed).

## #46 FP-045 — Registry entries of steps

**Decision.** `TurnBasedRegistry` entries for a step run after the step's built-in turn-based
actions (untap, draw, the combat hook; in the cleanup step after the CR 514.2 actions of each
cleanup step) and before the heartbeat or the CR 514.3a check; for a main phase before the
heartbeat. The standard registry has entries only at the precombat main phase.
**Rationale.** The built-in actions are the ones CR 703.4 lists "immediately after" the step
begins; an extra registered action must not run before them (for example before phasing,
CR 502.1), and it must run before priority (CR 117.3a: after turn-based actions).
**Alternatives.** Running step entries right after the begin-step expiry (rejected: would break
the CR 502.1-502.3 order).

## #46 FP-045 — Untap call without a choice

**Decision.** When `UntapOptions::choosable` is empty the untap step makes exactly one call,
`untap(active, &options.forced, ..)`, with the host's forced list as given (not re-sorted); an
empty forced list makes no call. Only when a `CHOOSE_UNTAP_SET` request was asked does the sequencer build
the union of the forced and the chosen permanents, sort and deduplicate it, and skip the call if
the union is empty.
**Rationale.** The brief's call order pins `untap(active, &o.forced, ..)` for the no-choice case
and "do not call `untap` with an empty slice"; the host already promises
ascending ids for `forced`, so passing it through loses nothing and keeps the host in charge of
its own list (the independent spec test asserts the list is passed unchanged).
**Alternatives.** Always sorting and skipping empty calls (rejected: contradicts the pinned call
order).

## #48 FP-046 — Starting player draw

**Decision.** With no `StartOptions::starting_player` the starting player is
`turn_order[next_bounded(n)]` from the state's PRNG (exactly one call; none when the option is
set). The chooser of later games of a match, Archenemy (CR 103.1b), Power Play (CR 103.1c),
shared team turns (CR 103.1a), commander, companion and sticker steps (CR 103.2) are out of scope;
`StartOptions::starting_player` is the hook for them.
**Rationale.** CR 103.1 allows any mutually agreeable method in game 1, so a seeded draw is
deterministic (R3) and replayable.
**Alternatives.** A choice request to a player (rejected: CR 103.1 gives game 1 no chooser).

## #48 FP-046 — GameStartHost seam

**Decision.** `turn::start::run_game_start` is written against a `GameStartHost` trait (library
order, hand, draw, return-to-library, put-on-bottom, set starting player, set life, pregame
eligibility and actions); the procedure holds no state beyond its locals and never mutates
`GameState` except through the PRNG.
**Rationale.** Mirrors "#46 FP-045 — TurnHost seam": zones and the game object arrive in later
units; `set_starting_player` is what `TurnHost::starting_player()` reports.
**Alternatives.** Mutating `GameState` directly (rejected: it has no zones or libraries yet).

## #48 FP-046 — Zero-hand mulligan limit

**Decision.** When the counted mulligans (`mulligan_to_bottom`) reach the player's starting hand
size (CR 103.5, last sentence), or exceed the cards actually in hand, no `MULLIGAN_DECISION` is
asked: the player keeps and the whole hand is put on the library bottom in ascending id order,
so the opening hand is empty. The mulligan request's mulligan decision uses the `Payload`
validator rule with min = max = the bottom count.
**Rationale.** The order of a forced bottom has no player choice; ascending ids is deterministic.
The extra "exceeds the hand" guard keeps a short library from producing an invalid default.
**Alternatives.** Asking with a one-answer request (rejected: no decision exists).

## #48 FP-046 — Free first mulligan condition

**Decision.** The free first mulligan applies iff the format flag
`free_first_mulligan_multiplayer` is set and `player_count > 2`; two-player Brawl and Two-Headed
Giant specifics are not modelled.
**Rationale.** CR 103.5c names "a multiplayer game and any Brawl game"; core has no Brawl
variant flag in the brief's inputs and the flag exists for the format to opt in.
**Alternatives.** Applying it to any format with the flag set (rejected: two-player games are
not multiplayer).

## #48 FP-046 — Pregame action as one subset ask

**Decision.** Each player with eligible cards gets exactly one `PREGAME_ACTION` request (min 0,
max = number of eligible cards) answered with a subset in the order to take the actions; an empty
answer performs nothing, and a player without eligible cards is not asked.
**Rationale.** CR 103.6 lets the player take "any such actions in any order"; one ask keeps the
protocol small and the SRS 15.3 row is `SUBMIT_PREGAME` with eligible cards.
**Alternatives.** One ask per card or per action (rejected: more round trips, no extra
information).

## #48 FP-046 — Order of PRNG calls in the game start

**Decision.** One `next_bounded(n)` (unless the starting player is given), one shuffle per player
in stored turn order, then per mulligan round the shuffles of the players who took a mulligan in
declaration order (starting player first, then turn order). Opening draws, declarations and
bottoming use no randomness.
**Rationale.** A fixed seed must give fixed hands and library order; independent tests compute
golden values from this order.
**Alternatives.** Shuffling in declaration order at the start (rejected: the stored order is
independent of who starts).

## #48 FP-046 — Spec-test judgement calls (Independent Tester)

**Decision.** The black-box spec tests (`tests/spec_turn_start*.rs`) assert an invalid starting
player as any `ChoiceSession::start` error (the brief said a `Procedure`-style error), and do not
test a zero-player host (`GameState::new(0)` is rejected, so no such host can be built) or a
wrong answer variant reaching the procedure (`ChoiceSession::submit` validates the response type
first; rejection is tested through `validate` and `submit`). The CR 103.5c "first mulligan shows
`to_bottom == 0`" check covers both the first ask (0 taken) and the ask after the first mulligan
(1 taken); the ask after the second shows 1.
**Rationale.** These follow the binding "specified behaviour" of the brief and the existing
`GameState` and choice-session contracts; the unreachable branches remain defensive code.
**Alternatives.** Exposing `ChoiceContext::new` to reach the defensive branches (rejected:
`src/choice` is outside #48's owned paths and the branches are unreachable through the public
protocol).
