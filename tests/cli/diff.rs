// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The unified diff `miri fmt --check` and `miri patch --dry-run` print.

use miri::cli::diff::unified_diff;

#[test]
fn test_identical_texts_produce_a_header_and_no_hunk() {
    let diff = unified_diff("a.mi", "fn main()\n", "fn main()\n");
    assert_eq!(diff, "--- a/a.mi\n+++ b/a.mi\n");
}

#[test]
fn test_one_changed_line_is_shown_with_its_context() {
    let before = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
    let after = "1\n2\n3\n4\nfive\n6\n7\n8\n9\n";
    assert_eq!(
        unified_diff("a.mi", before, after),
        "--- a/a.mi\n+++ b/a.mi\n@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n"
    );
}

#[test]
fn test_changes_far_apart_are_separate_hunks() {
    let before: String = (1..=20).map(|n| format!("{n}\n")).collect();
    let after: String = (1..=20)
        .map(|n| match n {
            2 => "two\n".to_string(),
            19 => "nineteen\n".to_string(),
            _ => format!("{n}\n"),
        })
        .collect();
    let diff = unified_diff("a.mi", &before, &after);

    assert_eq!(diff.matches("@@ -").count(), 2, "got:\n{diff}");
    assert!(
        diff.contains("@@ -1,5 +1,5 @@\n 1\n-2\n+two\n 3\n"),
        "got:\n{diff}"
    );
    assert!(diff.contains("-19\n+nineteen\n 20\n"), "got:\n{diff}");
    assert!(
        !diff.contains(" 10\n"),
        "a line far from both changes is not shown:\n{diff}"
    );
}

#[test]
fn test_changes_whose_context_touches_share_a_hunk() {
    let before = "1\n2\n3\n4\n5\n6\n7\n8\n";
    let after = "one\n2\n3\n4\n5\n6\n7\neight\n";
    let diff = unified_diff("a.mi", before, after);
    assert_eq!(diff.matches("@@ -").count(), 1, "got:\n{diff}");
    assert!(diff.contains("@@ -1,8 +1,8 @@\n"), "got:\n{diff}");
}

#[test]
fn test_an_inserted_blank_line_is_one_added_line() {
    let before = "struct A\n    x int\nstruct B\n    y int\n";
    let after = "struct A\n    x int\n\nstruct B\n    y int\n";
    assert_eq!(
        unified_diff("a.mi", before, after),
        "--- a/a.mi\n+++ b/a.mi\n@@ -1,4 +1,5 @@\n struct A\n     x int\n+\n struct B\n     y int\n"
    );
}

#[test]
fn test_text_added_to_an_empty_file_starts_at_line_zero() {
    assert_eq!(
        unified_diff("a.mi", "", "fn main()\n"),
        "--- a/a.mi\n+++ b/a.mi\n@@ -0,0 +1,1 @@\n+fn main()\n"
    );
}

#[test]
fn test_a_missing_final_line_break_is_marked() {
    assert_eq!(
        unified_diff("a.mi", "fn main()", "fn main()\n"),
        "--- a/a.mi\n+++ b/a.mi\n@@ -1,1 +1,1 @@\n-fn main()\n\\ No newline at end of file\n+fn main()\n"
    );
}

#[test]
fn test_an_absolute_path_is_labelled_without_a_doubled_slash() {
    let diff = unified_diff("/tmp/a.mi", "x\n", "y\n");
    assert!(
        diff.starts_with("--- a/tmp/a.mi\n+++ b/tmp/a.mi\n"),
        "got:\n{diff}"
    );
}
