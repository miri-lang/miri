// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Error and warning code catalog.
//!
//! Each diagnostic has an optional error code for documentation and tooling.
//! Error codes use "E" prefix, warning codes use "W" prefix.

/// Syntax/parsing error codes.
pub mod syntax {
    pub const INVALID_TOKEN: &str = "E0001";
    pub const UNCLOSED_MULTILINE_COMMENT: &str = "E0002";
    pub const INDENTATION_MISMATCH: &str = "E0003";
    pub const UNCLOSED_STRING_LITERAL: &str = "E0004";
    pub const UNEXPECTED_TOKEN: &str = "E0005";
    pub const UNEXPECTED_EOF: &str = "E0006";
    pub const INVALID_TYPE_DECLARATION: &str = "E0007";
    pub const INVALID_ASSIGNMENT_TARGET: &str = "E0008";
    pub const INTEGER_OVERFLOW: &str = "E0009";
    pub const INVALID_INTEGER_LITERAL: &str = "E0010";
    pub const INVALID_BINARY_LITERAL: &str = "E0011";
    pub const INVALID_OCTAL_LITERAL: &str = "E0012";
    pub const INVALID_HEX_LITERAL: &str = "E0013";
    pub const INVALID_FLOAT_LITERAL: &str = "E0014";
    pub const INVALID_STRING_LITERAL: &str = "E0015";
    pub const INVALID_BOOLEAN_LITERAL: &str = "E0016";
    pub const UNEXPECTED_OPERATOR: &str = "E0017";
    pub const INVALID_LHS_EXPRESSION: &str = "E0018";
    pub const MISSING_STRUCT_MEMBER_TYPE: &str = "E0019";
    pub const INVALID_INHERITANCE_IDENTIFIER: &str = "E0020";
    pub const DUPLICATE_MATCH_PATTERN: &str = "E0021";
    pub const MISSING_MATCH_BRANCHES: &str = "E0022";
    pub const INVALID_REGEX_LITERAL: &str = "E0023";
    pub const INVALID_FORMATTED_STRING: &str = "E0024";
    pub const INVALID_FORMATTED_STRING_EXPR: &str = "E0025";
    pub const BACKSLASH_IN_FSTRING: &str = "E0026";
    pub const INVALID_NUMBER_LITERAL: &str = "E0027";
    pub const MISSING_STRUCT_MEMBERS: &str = "E0028";
    pub const MISSING_ENUM_MEMBERS: &str = "E0029";
    pub const MISSING_TYPE_EXPRESSION: &str = "E0030";
    pub const INVALID_MODIFIER_COMBINATION: &str = "E0031";
}

/// Type checking error codes.
pub mod type_check {
    pub const UNDEFINED_VARIABLE: &str = "E0100";
    pub const TYPE_MISMATCH: &str = "E0101";
    pub const UNKNOWN_TYPE: &str = "E0102";
    pub const MISSING_FIELD: &str = "E0103";
    pub const MISSING_VARIANT: &str = "E0104";
    pub const INCOMPATIBLE_TYPES: &str = "E0105";
    pub const IMMUTABLE_ASSIGNMENT: &str = "E0106";
    pub const MISSING_RETURN: &str = "E0107";
    pub const INVALID_CALL: &str = "E0108";
    pub const ARITY_MISMATCH: &str = "E0109";
}

/// MIR lowering error codes.
pub mod lowering {
    pub const UNSUPPORTED_EXPRESSION: &str = "E0200";
    pub const UNSUPPORTED_STATEMENT: &str = "E0201";
    pub const UNDEFINED_VARIABLE: &str = "E0202";
    pub const TYPE_NOT_FOUND: &str = "E0203";
    pub const BREAK_OUTSIDE_LOOP: &str = "E0204";
    pub const CONTINUE_OUTSIDE_LOOP: &str = "E0205";
    pub const UNSUPPORTED_LHS: &str = "E0206";
}

/// Code generation error codes.
pub mod codegen {
    pub const TARGET_ISA: &str = "E0300";
    pub const MODULE_CREATION: &str = "E0301";
    pub const FUNCTION_DECLARATION: &str = "E0302";
    pub const FUNCTION_DEFINITION: &str = "E0303";
    pub const TRANSLATION: &str = "E0304";
    pub const EMIT: &str = "E0305";
    pub const NOT_SUPPORTED: &str = "E0306";
}

/// Interpreter/runtime error codes.
pub mod runtime {
    pub const DIVISION_BY_ZERO: &str = "E0400";
    pub const REMAINDER_BY_ZERO: &str = "E0401";
    pub const OVERFLOW: &str = "E0402";
    pub const UNDEFINED_FUNCTION: &str = "E0403";
    pub const TYPE_MISMATCH: &str = "E0404";
    pub const INVALID_OPERAND: &str = "E0405";
    pub const UNDEFINED_LOCAL: &str = "E0406";
    pub const UNINITIALIZED_LOCAL: &str = "E0407";
    pub const INVALID_BLOCK: &str = "E0408";
    pub const STACK_OVERFLOW: &str = "E0409";
    pub const NOT_IMPLEMENTED: &str = "E0410";
    pub const INTERNAL: &str = "E0411";
}

/// Warning codes.
pub mod warnings {
    pub const DOUBLE_NEGATION: &str = "W0001";
    pub const NULLABLE_IMMUTABLE: &str = "W0002";
    pub const UNUSED_VARIABLE: &str = "W0003";
    pub const UNREACHABLE_CODE: &str = "W0004";
}
