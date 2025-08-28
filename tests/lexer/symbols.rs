// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::{Token}};

use super::utils::*;


#[test]
fn test_symbols_with_numbers() {
    lexer_test(":symbol123 :test_2 :_private", vec![
        Token::Symbol,
        Token::Symbol,
        Token::Symbol,
    ]);
}
