// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

mod cli;
mod version;
mod lexer;

fn main() {
    let matches = cli::build_cli().get_matches();

    if matches.subcommand().is_none() {
        println!("{}", cli::build_cli().render_long_help().to_string());
    }
}
