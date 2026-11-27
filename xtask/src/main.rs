// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Repository automation for Fine Print.
//!
//! Commands: `coverage [--input <file>]`, `guard <headers|card-data|card-branch|all>`,
//! `cr <fetch|index|check>` and `pzl [--cards <dir>] <path>...`.
//! Run as `cargo xtask <command>` (alias in `.cargo/config.toml`).

// non-rules crate: REQ-CORE-006 applies to the rules crates only
#![allow(clippy::disallowed_types)]
#![forbid(unsafe_code)]

mod coverage;
mod cr;
mod guard;
mod pzl;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <command>\n  coverage [--input <llvm-cov-summary.json>]\n  guard <headers|card-data|card-branch|all>\n  cr <fetch|index|check> [options]\n  pzl [--cards <dir>] <path.pzl|dir>...";

/// Dispatch a command line (without the program name) and return the exit code.
fn dispatch(args: &[String]) -> u8 {
    match args.split_first() {
        Some((cmd, rest)) if cmd == "coverage" => coverage::run(rest),
        Some((cmd, rest)) if cmd == "guard" => guard::run(rest),
        Some((cmd, rest)) if cmd == "cr" => cr::run(rest),
        Some((cmd, rest)) if cmd == "pzl" => pzl::run(rest),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(dispatch(&args))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-NFR-008, REQ-TEST-001: unknown or missing commands, and `pzl` without a path or with
    /// an unreadable path, exit 2.
    #[test]
    fn req_nfr_008_dispatch_usage() {
        assert_eq!(dispatch(&[]), 2);
        assert_eq!(dispatch(&["nope".to_string()]), 2);
        assert_eq!(dispatch(&["guard".to_string()]), 2);
        assert_eq!(dispatch(&["coverage".to_string(), "--x".to_string()]), 2);
        assert_eq!(dispatch(&["cr".to_string()]), 2);
        assert_eq!(dispatch(&["cr".to_string(), "nope".to_string()]), 2);
        assert_eq!(dispatch(&["pzl".to_string()]), 2);
        assert_eq!(
            dispatch(&["pzl".to_string(), "/nonexistent/xtask-pzl".to_string()]),
            2
        );
    }
}
