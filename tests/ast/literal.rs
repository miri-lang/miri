// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use miri::lexer::RegexToken;

#[test]
fn test_integer_literal_is_zero() {
    assert!(IntegerLiteral::I8(0).is_zero());
    assert!(IntegerLiteral::I16(0).is_zero());
    assert!(IntegerLiteral::I32(0).is_zero());
    assert!(IntegerLiteral::I64(0).is_zero());
    assert!(IntegerLiteral::I128(0).is_zero());
    assert!(IntegerLiteral::U8(0).is_zero());
    assert!(IntegerLiteral::U16(0).is_zero());
    assert!(IntegerLiteral::U32(0).is_zero());
    assert!(IntegerLiteral::U64(0).is_zero());
    assert!(IntegerLiteral::U128(0).is_zero());

    assert!(!IntegerLiteral::I32(1).is_zero());
    assert!(!IntegerLiteral::I64(-1).is_zero());
    assert!(!IntegerLiteral::U128(42).is_zero());
}

#[test]
fn test_float_literal_is_zero() {
    assert!(FloatLiteral::F32(0.0_f32.to_bits()).is_zero());
    assert!(FloatLiteral::F64(0.0_f64.to_bits()).is_zero());
    assert!(FloatLiteral::F32((-0.0_f32).to_bits()).is_zero());
    assert!(!FloatLiteral::F32(1.0_f32.to_bits()).is_zero());
    assert!(!FloatLiteral::F64(f64::NAN.to_bits()).is_zero());
}

#[test]
fn test_literal_is_zero_numerics() {
    assert!(Literal::Integer(IntegerLiteral::I32(0)).is_zero());
    assert!(!Literal::Integer(IntegerLiteral::I32(3)).is_zero());
    assert!(Literal::Float(FloatLiteral::F64(0.0_f64.to_bits())).is_zero());
    assert!(!Literal::Float(FloatLiteral::F64(2.5_f64.to_bits())).is_zero());
}

#[test]
fn test_integer_literal_to_i128_preserves_value_across_widths() {
    assert_eq!(IntegerLiteral::I8(-7).to_i128(), -7);
    assert_eq!(IntegerLiteral::I16(-30_000).to_i128(), -30_000);
    assert_eq!(IntegerLiteral::I32(i32::MIN).to_i128(), i32::MIN as i128);
    assert_eq!(IntegerLiteral::I64(i64::MIN).to_i128(), i64::MIN as i128);
    assert_eq!(IntegerLiteral::I128(i128::MIN).to_i128(), i128::MIN);
    assert_eq!(IntegerLiteral::U8(255).to_i128(), 255);
    assert_eq!(IntegerLiteral::U16(u16::MAX).to_i128(), u16::MAX as i128);
    assert_eq!(IntegerLiteral::U32(u32::MAX).to_i128(), u32::MAX as i128);
    assert_eq!(IntegerLiteral::U64(u64::MAX).to_i128(), u64::MAX as i128);
    assert_eq!(IntegerLiteral::U128(12345).to_i128(), 12345);
}

#[test]
fn test_integer_literal_to_usize_preserves_small_values() {
    assert_eq!(IntegerLiteral::I8(7).to_usize(), 7);
    assert_eq!(IntegerLiteral::I32(0).to_usize(), 0);
    assert_eq!(IntegerLiteral::U64(42).to_usize(), 42);
    assert_eq!(IntegerLiteral::U128(99).to_usize(), 99);
}

#[test]
fn test_literal_is_zero_non_numeric_variants_are_false() {
    assert!(!Literal::String("0".to_string()).is_zero());
    assert!(!Literal::String(String::new()).is_zero());
    assert!(!Literal::Boolean(false).is_zero());
    assert!(!Literal::Boolean(true).is_zero());
    assert!(!Literal::Identifier("zero".to_string()).is_zero());
    assert!(!Literal::Regex(RegexToken {
        body: String::new(),
        ignore_case: false,
        global: false,
        multiline: false,
        dot_all: false,
        unicode: false,
    })
    .is_zero());
    assert!(!Literal::None.is_zero());
}
