// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    binary, block, call, expression_statement, for_statement, identifier, index,
    int_literal_expression, lambda, let_variable, list, member, parameter, range,
    type_expr_non_null, type_int, variable_statement,
};
use miri::ast::{opt_expr, BinaryOp, MemberVisibility, RangeExpressionType};
use miri::error::syntax::SyntaxErrorKind;

// TODO
