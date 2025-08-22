// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_symbol_literal() {
    parse_literal_test(":my_fancy_symbol", symbol("my_fancy_symbol"));
}
