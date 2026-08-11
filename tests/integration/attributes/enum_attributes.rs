// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_non_exhaustive_enum_still_constructs_and_matches() {
    assert_runs_with_output(
        r#"
@non_exhaustive
enum Status
    Ready
    Busy

fn main()
    let s = Status.Ready
    match s
        Status.Ready: println("ready")
        Status.Busy: println("busy")
"#,
        "ready",
    );
}

#[test]
fn test_must_use_attribute_allows_a_used_value() {
    assert_runs_with_output(
        r#"
@must_use
enum Outcome
    Win(int)
    Lose

fn main()
    let outcome = Outcome.Win(7)
    match outcome
        Outcome.Win(n): println("win")
        Outcome.Lose: println("lose")
"#,
        "win",
    );
}

#[test]
fn test_must_use_attribute_rejects_a_discarded_value() {
    assert_compiler_error(
        r#"
@must_use
enum Outcome
    Win(int)
    Lose

fn produce() Outcome
    return Outcome.Lose

fn main()
    produce()
"#,
        "must be used",
    );
}

#[test]
fn test_must_use_keyword_rejects_a_discarded_value_identically() {
    assert_compiler_error(
        r#"
must_use enum Outcome
    Win(int)
    Lose

fn produce() Outcome
    return Outcome.Lose

fn main()
    produce()
"#,
        "must be used",
    );
}

#[test]
fn test_must_use_keyword_reports_deprecation() {
    assert_compiler_warning(
        r#"
must_use enum Outcome
    Win(int)
    Lose

fn main()
    let outcome = Outcome.Lose
    match outcome
        Outcome.Win(n): println("win")
        Outcome.Lose: println("lose")
"#,
        "deprecated",
    );
}

#[test]
fn test_must_use_attribute_does_not_report_deprecation() {
    assert_runs_with_output(
        r#"
@must_use
enum Outcome
    Win(int)
    Lose

fn main()
    let outcome = Outcome.Lose
    match outcome
        Outcome.Win(n): println("win")
        Outcome.Lose: println("lose")
"#,
        "lose",
    );
}
