# Human Queue

Items for a person (PROCESS.md R8, P7, P8). Never a request to close a review thread.
Resolved items are struck through with the resolution and a link.

- ~~[setup] Decide architecture and supported platforms.~~ Resolved: SRS §17 (`REQ-NFR-001`–`012`), DECISIONS.md "Architecture — …".
- ~~[setup] Generate the plan.~~ Resolved: `work-unit` issues on GitHub (DECISIONS.md "Repository — The plan is GitHub issues").
- [setup] Once the CI issue (#3, FP-002) merges, add its jobs as required status checks in the `main` branch protection rule (PROCESS.md P6 step 5).
- ~~[PR #47] SRS REQ-NFR-009 says "wave gate"; rename it to "phase gate".~~ Resolved: REQ-NFR-009 now says "phase gate (PROCESS.md P5)"; DECISIONS.md "Architecture" entry updated to match (owner-approved SRS edit).
- [setup] Add these CI jobs as required status checks on main (REQ-NFR-011, PROCESS.md P6 step 5): fmt (ubuntu-latest), fmt (windows-latest), fmt (macos-latest), clippy (ubuntu-latest), clippy (windows-latest), clippy (macos-latest), test (ubuntu-latest), test (windows-latest), test (macos-latest). Names are documented in docs/DEVELOPMENT.md "CI".
- [Decision] FP-003 (#4): after merge, add the coverage (ubuntu-latest), guards and req-coverage jobs to the required status checks on main (REQ-NFR-011), and confirm the crate-to-floor table (base, dsl, core at 90%; others at 75%).
