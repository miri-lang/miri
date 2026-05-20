// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    assign, binary, call, expression_statement, float32_literal_expression, identifier, index,
    int_literal_expression, lhs_index, lhs_member, member, string_literal_expression,
};
use miri::ast::{AssignmentOp, BinaryOp};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_member_expression() {
    parser_test(
        "
obj.prop
",
        vec![expression_statement(member(
            identifier("obj"),
            identifier("prop"),
        ))],
    );
}

#[test]
fn test_assign_to_member_expression() {
    parser_test(
        "
obj.prop = 1
",
        vec![expression_statement(assign(
            lhs_member(identifier("obj"), identifier("prop")),
            AssignmentOp::Assign,
            int_literal_expression(1),
        ))],
    );
}

#[test]
fn test_assign_to_index_expression() {
    parser_test(
        "
obj['prop'] = 1
",
        vec![expression_statement(assign(
            lhs_index(identifier("obj"), string_literal_expression("prop")),
            AssignmentOp::Assign,
            int_literal_expression(1),
        ))],
    );
}

#[test]
fn test_assign_to_chained_member_expression() {
    parser_test(
        "
obj.a.b['prop'][0] = 1.0
",
        vec![expression_statement(assign(
            lhs_index(
                index(
                    member(member(identifier("obj"), identifier("a")), identifier("b")),
                    string_literal_expression("prop"),
                ),
                int_literal_expression(0),
            ),
            AssignmentOp::Assign,
            float32_literal_expression(1.0),
        ))],
    );
}

#[test]
fn test_chained_calls_and_member_access() {
    // `a.b(c).d` should parse as `((a.b)(c)).d`
    parser_test(
        "a.b(c).d",
        vec![expression_statement(member(
            call(
                member(identifier("a"), identifier("b")),
                vec![identifier("c")],
            ),
            identifier("d"),
        ))],
    );
}

#[test]
fn test_member_access_with_keyword_as_property() {
    parser_error_test(
        "obj.if",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".into(),
            found: "if".into(),
        },
    );
}

#[test]
fn test_index_access_with_complex_expression() {
    // The expression inside index brackets can be complex.
    parser_test(
        "my_map[get_key() + '_suffix']",
        vec![expression_statement(index(
            identifier("my_map"),
            binary(
                call(identifier("get_key"), vec![]),
                BinaryOp::Add,
                string_literal_expression("_suffix"),
            ),
        ))],
    );
}

#[test]
fn test_error_on_invalid_member_property() {
    // A dot must be followed by an identifier or integer literal, not an operator.
    parser_error_test(
        "obj.+",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".into(),
            found: "+".into(),
        },
    );
}

#[test]
fn test_error_on_unclosed_index_expression() {
    // An index expression must have a closing bracket.
    parser_error_test("obj[0", &SyntaxErrorKind::UnexpectedEOF);
}

/// Multiline method chain: a `.method()` on a continuation line continues the
/// previous expression instead of being a syntax error.
#[test]
fn test_multiline_method_chain() {
    parser_test(
        "
obj
    .foo()
    .bar()
",
        vec![expression_statement(call(
            member(
                call(member(identifier("obj"), identifier("foo")), vec![]),
                identifier("bar"),
            ),
            vec![],
        ))],
    );
}

/// Multiline field access: a `.field` on a continuation line still parses as
/// member access on the previous receiver.
#[test]
fn test_multiline_field_access() {
    parser_test(
        "
obj
    .a
    .b
",
        vec![expression_statement(member(
            member(identifier("obj"), identifier("a")),
            identifier("b"),
        ))],
    );
}

/// Multiline chain mixing field access and method calls.
#[test]
fn test_multiline_mixed_chain() {
    parser_test(
        "
obj
    .a
    .b()
    .c
",
        vec![expression_statement(member(
            call(
                member(member(identifier("obj"), identifier("a")), identifier("b")),
                vec![],
            ),
            identifier("c"),
        ))],
    );
}

/// A leading `..` (range operator) on a continuation line must NOT be folded
/// into the previous expression — it is still a range and the statement on the
/// previous line must end normally.
#[test]
fn test_leading_double_dot_does_not_continue() {
    // `a` is one statement, `..b` would start a new statement. The lexer must
    // still emit ExpressionStatementEnd between them, so the second line is a
    // syntactically invalid range-without-LHS and we expect a parse error.
    parser_error_test(
        "
a
    ..b
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".into(),
            found: "..".into(),
        },
    );
}

/// Multiline tuple field access: `.0` on a continuation line.
#[test]
fn test_multiline_tuple_field_access() {
    parser_test(
        "
t
    .0
",
        vec![expression_statement(member(
            identifier("t"),
            int_literal_expression(0),
        ))],
    );
}

/// A multiline chain immediately followed by a normal sibling statement.
/// Verifies the chain ends at the statement boundary, not at the next `.`.
#[test]
fn test_multiline_chain_followed_by_sibling_statement() {
    parser_test(
        "
obj
    .foo()
next
",
        vec![
            expression_statement(call(member(identifier("obj"), identifier("foo")), vec![])),
            expression_statement(identifier("next")),
        ],
    );
}

/// A leading `..=` (range-inclusive operator) must NOT be folded into the
/// previous expression, mirroring the `..` rule.
#[test]
fn test_leading_range_inclusive_does_not_continue() {
    parser_error_test(
        "
a
    ..=b
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".into(),
            found: "..=".into(),
        },
    );
}

/// Identifiers starting with `_` are valid Miri identifiers, so a `._name`
/// continuation line must still chain onto the previous expression.
#[test]
fn test_multiline_underscore_prefixed_member() {
    parser_test(
        "
obj
    ._private
",
        vec![expression_statement(member(
            identifier("obj"),
            identifier("_private"),
        ))],
    );
}
