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
