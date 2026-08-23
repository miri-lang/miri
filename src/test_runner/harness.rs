// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Synthesis of the dispatcher appended to a test file.
//!
//! The dispatcher is ordinary Miri source. It declares the two argv intrinsics
//! it needs rather than importing them, so a test file pulls in no module it
//! did not ask for and the compiler keeps no knowledge of the standard
//! library. Re-declaring a `runtime "core" fn` the file already declares is
//! harmless, so the declarations need no collision check.

use crate::test_runner::TestMarker;

/// Exit status for a dispatcher invoked without a test name.
pub const EXIT_NO_TEST_NAME: i32 = 2;

/// Exit status for a test name the dispatcher does not recognize.
///
/// Distinct from the failure statuses so a mis-dispatched name is reported as
/// a runner fault instead of quietly passing.
pub const EXIT_UNKNOWN_TEST: i32 = 3;

/// Build the dispatcher for `tests`.
///
/// `miri_rt_args_at` indexes the program's own arguments — index 0 is the
/// first one, not the executable path — and terminates the process on an
/// out-of-range index, hence the count guard before the read.
///
/// Test names are interpolated into Miri source, both inside a string literal
/// and as a call. That is safe because a name reaching here came from a parsed
/// function declaration, and the lexer admits only `[a-zA-Z_][a-zA-Z0-9_]*` as
/// an identifier — a name can hold neither a quote nor a newline, so it cannot
/// escape the literal it is written into.
pub fn synthesize(tests: &[TestMarker]) -> String {
    let mut source = String::new();

    source.push_str("runtime \"core\" fn miri_rt_args_count() int\n");
    source.push_str("runtime \"core\" fn miri_rt_args_at(index int) String\n");
    source.push_str("\nfn main() int\n");
    source.push_str(&format!(
        "    if miri_rt_args_count() < 1: return {}\n",
        EXIT_NO_TEST_NAME
    ));
    source.push_str("    let selected = miri_rt_args_at(0)\n");
    source.push_str("    var matched = 0\n");

    for test in tests {
        source.push_str(&format!("    if selected == \"{}\"\n", test.name));
        source.push_str(&format!("        {}()\n", test.name));
        source.push_str("        matched = 1\n");
    }

    source.push_str(&format!(
        "    if matched == 0: return {}\n",
        EXIT_UNKNOWN_TEST
    ));
    source.push_str("    return 0\n");
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(name: &str) -> TestMarker {
        TestMarker {
            name: name.to_string(),
            ignore_reason: None,
            xfail_reason: None,
        }
    }

    #[test]
    fn declares_the_argv_intrinsics_it_uses() {
        let source = synthesize(&[marker("test_adds")]);
        assert!(source.contains("runtime \"core\" fn miri_rt_args_count() int"));
        assert!(source.contains("runtime \"core\" fn miri_rt_args_at(index int) String"));
    }

    #[test]
    fn guards_the_count_before_indexing() {
        let source = synthesize(&[marker("test_adds")]);
        let guard = source
            .find("if miri_rt_args_count() < 1")
            .expect("the guard should be emitted");
        let read = source
            .find("miri_rt_args_at(0)")
            .expect("the read should be emitted");
        assert!(guard < read, "the count guard must precede the argv read");
    }

    #[test]
    fn dispatches_to_every_test() {
        let source = synthesize(&[marker("test_adds"), marker("test_divides")]);
        assert!(source.contains("if selected == \"test_adds\"\n        test_adds()\n"));
        assert!(source.contains("if selected == \"test_divides\"\n        test_divides()\n"));
    }

    #[test]
    fn reports_an_unknown_name_instead_of_passing() {
        let source = synthesize(&[marker("test_adds")]);
        assert!(source.contains(&format!("if matched == 0: return {}", EXIT_UNKNOWN_TEST)));
    }

    #[test]
    fn an_empty_test_list_still_yields_a_compilable_main() {
        let source = synthesize(&[]);
        assert!(source.contains("fn main() int"));
        assert!(source.contains("var matched = 0"));
        assert!(source.trim_end().ends_with("return 0"));
    }

    #[test]
    fn the_two_fault_statuses_stay_distinct() {
        assert_ne!(EXIT_NO_TEST_NAME, EXIT_UNKNOWN_TEST);
        assert_ne!(EXIT_NO_TEST_NAME, 0);
        assert_ne!(EXIT_UNKNOWN_TEST, 0);
        // Exit 1 is what a failing assertion produces; a fault must not collide
        // with it or a runner bug would read as an ordinary test failure.
        assert_ne!(EXIT_NO_TEST_NAME, 1);
        assert_ne!(EXIT_UNKNOWN_TEST, 1);
    }
}
