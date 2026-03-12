// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_precedence() {
    assert_operation_outputs(&[
        // Check * and / have higher precedence than + and -
        ("2 + 3 * 4", "14"),
        ("2 * 3 + 4", "10"),
        ("20 / 4 + 2", "7"),
        ("2 + 20 / 4", "7"),
        ("10 - 2 * 3", "4"),
        ("10 * 2 - 3", "17"),
        // Check left-to-right associativity
        ("10 - 5 - 2", "3"),   // (10-5)-2=3
        ("100 / 10 / 2", "5"), // (100/10)/2=5
        ("20 % 10 % 3", "0"),
        // Mixed precedence and associativity
        ("2 + 3 * 4 - 5", "9"),        // 2 + 12 - 5
        ("2 * 3 + 4 * 5", "26"),       // 6 + 20
        ("100 / 2 / 2 * 3 + 1", "76"), // 50 / 2 * 3 + 1 -> 25 * 3 + 1 -> 75 + 1
    ]);
}

#[test]
fn test_parentheses() {
    assert_operation_outputs(&[
        // Basic override precedence
        ("(2 + 3) * 4", "20"),
        ("2 * (3 + 4)", "14"),
        ("20 / (4 + 6)", "2"),
        // Nested parentheses
        ("((2 + 3) * 4)", "20"),
        ("(2 + (3 * 4))", "14"),
        ("((10 - 2) * (3 + 1)) / 4", "8"), // (8 * 4) / 4 = 8
        // "Crazy" nesting
        ("(((1 + 2) * 3) + 4) * 5", "65"), // (3 * 3 + 4) * 5 -> (9 + 4) * 5 -> 13 * 5 = 65
        ("(1 + (2 * (3 + (4 * (5 + 6)))))", "95"), // 1 + (2 * (3 + (4 * 11))) -> 1 + (2 * (3 + 44)) -> 1 + (2 * 47) -> 1 + 94 = 95
        ("((((((1 + 1))))))", "2"),
    ]);
}
