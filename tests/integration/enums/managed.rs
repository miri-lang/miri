// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_enum_managed_payload_drop() {
    assert_runs(
        r#"
use system.collections.list

enum Result
    Success(List<int>)
    Error(String)

fn main()
    // This creates an enum with a managed payload.
    // When `r` goes out of scope, the enum's drop code should decrement the payload's RC.
    // If it doesn't, this will leak memory (verified by leak sanitizer/Miri checks).
    let r = Result.Success(List([1, 2, 3]))
    
    let r2 = Result.Error("Something went wrong")
"#,
    );
}

/// Matching directly on a call expression must release what the call returned.
/// The subject temp is the only owner of it, so without a drop at the end of
/// the match the payload the arms read out of it is never released.
#[test]
fn test_match_on_call_result_releases_payload() {
    assert_runs_with_output(
        r#"
public enum Box
    Val(String)

fn make() Box
    let s = "bo" + "om"
    return Box.Val(s)

fn main()
    match make()
        Box.Val(v): println(v)
"#,
        "boom",
    );
}

/// The same for a collection payload: the release comes from the enum's own
/// drop, so it must reach every managed payload kind, not just `String`.
#[test]
fn test_match_on_call_result_releases_list_payload() {
    assert_runs_with_output(
        r#"
use system.collections.list

public enum Box
    Val([int])

fn make() Box
    return Box.Val(List([1, 2, 3]))

fn main()
    match make()
        Box.Val(v): println(f"{v.length()}")
"#,
        "3",
    );
}

/// Matching on a variable must not release it twice: the subject temp takes its
/// own reference on assignment, and the variable is still released at scope end.
#[test]
fn test_match_on_variable_keeps_single_release() {
    assert_runs_with_output(
        r#"
public enum Box
    Val(String)

fn make() Box
    let s = "bo" + "om"
    return Box.Val(s)

fn main()
    let b = make()
    match b
        Box.Val(v): println(v)
"#,
        "boom",
    );
}
