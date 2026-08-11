// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

const DEFINING_MODULE: &str = concat!(
    "@non_exhaustive\n",
    "public enum FsError\n",
    "    NotFound\n",
    "    PermissionDenied\n",
);

#[test]
fn test_match_outside_defining_module_requires_catch_all_even_when_complete() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.errors\n",
                    "let e = FsError.NotFound\n",
                    "match e\n",
                    "    FsError.NotFound: println(\"missing\")\n",
                    "    FsError.PermissionDenied: println(\"denied\")\n",
                ),
            ),
            ("errors.mi", DEFINING_MODULE),
        ],
        "requires a `default` arm",
    );
}

#[test]
fn test_match_outside_defining_module_accepts_a_default_arm() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.errors\n",
                    "let e = FsError.NotFound\n",
                    "match e\n",
                    "    FsError.NotFound: println(\"missing\")\n",
                    "    default: println(\"other\")\n",
                ),
            ),
            ("errors.mi", DEFINING_MODULE),
        ],
        "missing",
    );
}

#[test]
fn test_plain_enum_outside_defining_module_needs_no_catch_all() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.modes\n",
                    "let m = Mode.Fast\n",
                    "match m\n",
                    "    Mode.Fast: println(\"fast\")\n",
                    "    Mode.Slow: println(\"slow\")\n",
                ),
            ),
            (
                "modes.mi",
                concat!("public enum Mode\n", "    Fast\n", "    Slow\n"),
            ),
        ],
        "fast",
    );
}

#[test]
fn test_match_inside_defining_module_accepts_all_variants_without_else() {
    assert_runs_with_output(
        r#"
@non_exhaustive
enum FsError
    NotFound
    PermissionDenied

let e = FsError.NotFound
match e
    FsError.NotFound: println("missing")
    FsError.PermissionDenied: println("denied")
"#,
        "missing",
    );
}

#[test]
fn test_match_inside_defining_module_still_requires_exhaustiveness() {
    assert_compiler_error(
        r#"
@non_exhaustive
enum FsError
    NotFound
    PermissionDenied

let e = FsError.NotFound
match e
    FsError.NotFound: println("missing")
"#,
        "Missing variants",
    );
}
