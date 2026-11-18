// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! The `fineprint-ingest` command line (offline card toolchain).
//!
//! Subcommands: `vocab` (see `fineprint_ingest::vocab::cli`). Exit codes: 0 success,
//! 1 violations, 2 usage or input error.

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::process::ExitCode;

const USAGE: &str = "usage: fineprint-ingest <command> [options]\n\
                     commands:\n  vocab --cr <rules.txt> [--out <file>|-] [--check]";

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let code = match args.first().and_then(|a| a.to_str()) {
        Some("vocab") => fineprint_ingest::vocab::cli::run(&args[1..], &mut stdout, &mut stderr),
        _ => {
            use std::io::Write;
            let _ = writeln!(stderr, "{USAGE}");
            2
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
