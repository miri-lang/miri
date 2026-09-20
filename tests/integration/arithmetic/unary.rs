// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_unary_operations_on_integers() {
    assert_operation_outputs(&[
        ("-123", "-123"),
        ("-(-123)", "123"),
        ("--10", "10"),
        ("+10", "10"),
        ("++10", "10"),
        ("+(-10)", "-10"),
        ("-(-10)", "10"),
        ("-(2 + 3)", "-5"),
        ("-2 * 3", "-6"),
        ("2 * -3", "-6"),
        ("5 - -3", "8"),
        ("-(-(-5))", "-5"),
        ("-(-(-(-5)))", "5"),
    ]);
}

#[test]
fn test_double_negation() {
    assert_compiler_warning("--5", "Decrement operator not supported");
}

/// The operators that answer their operand unchanged still have to write it
/// where the caller asked for it. Printing one reads the answer directly and so
/// says nothing about this; binding one to a name reads the place the operator
/// was told to fill, which is left holding its initial value when the operator
/// hands back the operand without writing it.
#[test]
fn test_unary_identity_operators_bound_to_a_name_carry_the_value() {
    assert_runs_with_output(
        r#"
        let a = 1.5
        let plus_float = +a
        println(f'{plus_float}')

        let c = 7
        let plus_int = +c
        println(f'{plus_int}')

        var e = 3
        let incremented = ++e
        println(f'{incremented}')

        let g = 10
        let doubly_negated = --g
        println(f'{doubly_negated}')
        "#,
        "1.5\n7\n3\n10",
    );
}

/// The same place, reached as a field rather than a local.
#[test]
fn test_unary_identity_into_a_field_carries_the_value() {
    assert_runs_with_output(
        r#"
class Holder
    v float
    fn init(v float)
        self.v = +v

fn main()
    let h = Holder(2.5)
    println(f'{h.v}')
"#,
        "2.5",
    );
}
