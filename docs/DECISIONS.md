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
uses the cheap multiplier. The seed derivation itself belongs to REQ-API-004 (see HUMAN_QUEUE).
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
keep the REQ-NFR-008 75 % non-rules line-coverage floor for the spike library code
(`spikes/libs/src/**`), with any exemption limited to the benchmark printer
`spikes/libs/examples/bench.rs`; any wider exemption (for example of `src/**` or `tests/**`)
needs an approved SRS change. Alternatively #4 may delete
`spikes/libs` once the production crates adopt the pins, which removes the question.
**Rationale:** The spike is evidence, not product; holding a benchmark printer
(`examples/bench.rs`) to the 75 % floor would make the workspace gate fail for no product
benefit. Keeping it in the workspace makes CI run its golden-value tests on all three
platforms, which is the cross-platform acceptance criterion of #5.
**Alternatives:** a separate workspace under `spikes/` (rejected: CI would not run it without
editing `.github/`, which #5 does not own); deleting the spike in #5 (rejected: the tests are
the cross-platform evidence until production crates carry them).
