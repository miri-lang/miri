// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The contract every runtime trap answers to.
//!
//! A trap is how a program dies, and the envelope is how a tool learns that it
//! did. So each one has to arrive as a registered code with `ok` false, not as
//! a sentence on stderr beside a successful-looking run. These tests hold that
//! for the traps a program can reach today and refuse the shape that let two of
//! them ship uncoded: a trap that prints its own message and leaves through its
//! own exit call, where nothing forces it to name a code on the way out.

use crate::utils::miri_cmd;
use miri::diagnostics::DiagnosticCode;
use std::io::Write;
use std::str::FromStr;
use tempfile::NamedTempFile;

/// A program that dies, and the code its death must be reported as.
struct Trap {
    code: &'static str,
    source: &'static str,
}

/// Every trap a Miri program can raise through `miri run`.
///
/// A trap missing from here is a trap nothing checks, so a new one belongs in
/// this table on the day it is written.
const TRAPS: &[Trap] = &[
    Trap {
        code: "MER_RT_001",
        source: "fn main() int\n    var d = 0\n    10 / d\n",
    },
    Trap {
        code: "MER_RT_002",
        source: "fn main() int\n    var d = 0\n    10 % d\n",
    },
    Trap {
        code: "MER_RT_005",
        source: "use system.testing\n\nfn main()\n    assert_eq(1, 2)\n",
    },
    Trap {
        code: "MER_RT_011",
        source: "fn at(a [int; 3], i int) int\n    a[i]\n\nfn main()\n    let l = [1, 2, 3]\n    println(f\"{at(l, 7)}\")\n",
    },
    Trap {
        code: "MER_RT_011",
        source: "use system.os\n\nfn main()\n    let args = Args()\n    println(args.element_at(9))\n",
    },
    Trap {
        code: "MER_RT_012",
        source: "fn main()\n    panic(\"boom\")\n",
    },
];

#[test]
fn test_every_runtime_trap_answers_with_a_code_and_a_failed_run() {
    for trap in TRAPS {
        let mut file = NamedTempFile::new().expect("a temporary file");
        write!(file, "{}", trap.source).expect("the source is written");
        let path = file.path().to_str().expect("the path is UTF-8");

        let output = miri_cmd()
            .arg("run")
            .arg(path)
            .arg("--format")
            .arg("json")
            .output()
            .expect("miri runs");
        let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
        let envelope: serde_json::Value =
            serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("not JSON: {}", stdout));

        assert_eq!(
            envelope["ok"], false,
            "a program that trapped reports ok true: {}",
            trap.source
        );
        assert_eq!(
            envelope["diagnostics"][0]["code"], trap.code,
            "the trap in {} is not reported as {}",
            trap.source, trap.code
        );
        assert_eq!(
            output.status.code(),
            Some(1),
            "a trapped program should exit 1: {}",
            trap.source
        );
    }
}

/// The runtime sources a trap can be written in.
fn runtime_core_sources() -> Vec<(String, String)> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/runtime/core/src");
    let mut sources = Vec::new();
    let mut pending = vec![std::path::PathBuf::from(dir)];
    while let Some(path) = pending.pop() {
        let entries = std::fs::read_dir(&path).expect("the runtime source directory is readable");
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                pending.push(entry_path);
            } else if entry_path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&entry_path).expect("the source is readable");
                sources.push((entry_path.display().to_string(), text));
            }
        }
    }
    sources
}

#[test]
fn test_a_trap_leaves_the_runtime_by_the_one_door_that_reports_it() {
    // Reporting the code is part of leaving, so it cannot be forgotten: the
    // exit call lives in exactly one function, which takes the code as an
    // argument. A second exit anywhere else is a trap that dies without saying
    // what it was — which is how the bounds check and `panic` came to report a
    // successful run.
    //
    // Status 1 is what a fault exits with: 99 belongs to the leak observer and
    // the status a program chooses for itself comes from `exit`, so neither is
    // matched. A comment is prose about an exit rather than one of them.
    let doors: Vec<String> = runtime_core_sources()
        .into_iter()
        .filter(|(_, text)| {
            text.lines()
                .any(|line| !line.trim_start().starts_with("//") && line.contains("_exit(1)"))
        })
        .map(|(path, _)| path)
        .collect();

    assert_eq!(
        doors.len(),
        1,
        "a trap must exit through trap.rs alone, but these files exit on their own: {:?}",
        doors
    );
    assert!(
        doors[0].ends_with("trap.rs"),
        "the exit door moved out of trap.rs to {}",
        doors[0]
    );
}

#[test]
fn test_every_code_the_runtime_names_is_a_registered_runtime_code() {
    // The runtime is a separate crate and cannot reach the registry, so it
    // writes its codes as text. This is what keeps that text honest: a typo, or
    // a code retired on the compiler side, fails here rather than reaching a
    // reader as a diagnostic naming nothing.
    let mut seen = 0;
    for (path, text) in runtime_core_sources() {
        for start in text.match_indices("MER_").map(|(at, _)| at) {
            let name: String = text[start..]
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            let code = DiagnosticCode::from_str(&name)
                .unwrap_or_else(|_| panic!("{} names {}, which is not registered", path, name));
            assert_eq!(
                code.area(),
                "RT",
                "{} names {}, which is not a runtime code",
                path,
                name
            );
            seen += 1;
        }
    }
    assert!(seen > 0, "no runtime code was found to check");
}
