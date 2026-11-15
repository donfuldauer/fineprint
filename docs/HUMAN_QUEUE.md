# Human Queue

Items for a person (PROCESS.md R8, P7, P8). Never a request to close a review thread.
Resolved items are struck through with the resolution and a link.

- ~~[setup] Decide architecture and supported platforms.~~ Resolved: SRS §17 (`REQ-NFR-001`–`012`), DECISIONS.md "Architecture — …".
- ~~[setup] Generate the plan.~~ Resolved: `work-unit` issues on GitHub (DECISIONS.md "Repository — The plan is GitHub issues").
- [setup] Once the CI issue (#3, FP-002) merges, add its jobs as required status checks in the `main` branch protection rule (PROCESS.md P6 step 5).
- ~~[PR #47] SRS REQ-NFR-009 says "wave gate"; rename it to "phase gate".~~ Resolved: REQ-NFR-009 now says "phase gate (PROCESS.md P5)"; DECISIONS.md "Architecture" entry updated to match (owner-approved SRS edit).
- [setup] Add these CI jobs as required status checks on main (REQ-NFR-011, PROCESS.md P6 step 5): fmt (ubuntu-latest), fmt (windows-latest), fmt (macos-latest), clippy (ubuntu-latest), clippy (windows-latest), clippy (macos-latest), test (ubuntu-latest), test (windows-latest), test (macos-latest). Names are documented in docs/DEVELOPMENT.md "CI".
- [#5 FP-004] PCG64-DXSM seeding convention: the spike seeds like NumPy (standard 128-bit multiplier during `srandom`), which differs from pcg-cpp/`rand_pcg` (cheap multiplier); the REQ-API-004 owner should confirm (DECISIONS.md "FP-004 — PRNG").
- [#5 FP-004] Cross-check the JCS number table against the final RFC 8785 Appendix B text: rfc-editor.org and datatracker were unreachable, so the vectors came from the RFC author's draft source plus Node.js (DECISIONS.md "FP-004 — RFC 8785 canonical JSON").
- [#5 FP-004] Confirm that `unicode-ident`'s Unicode-3.0 conjunct (compile-time proc-macro dependency only) is acceptable under the MPL-2.0 licence policy (DECISIONS.md "FP-004 — Licence check").
- [#4 FP-003] Apply the decided handling of `spikes/libs/**` in the repo guards (header, unsafe and card-data guards on; non-rules; 75 % floor applies to all of `spikes/libs/**`, no exemption without an approved SRS change) or delete the spike once production crates adopt the pins (DECISIONS.md "FP-004 — spikes/ directory handling by repo guards").
- [#6 FP-005] Runner image, C compiler and exact browser builds observed in the first Spikes run are in that run's logs; DECISIONS.md lists only the pinned versions.
