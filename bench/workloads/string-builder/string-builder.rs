// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=801be72bf04a36fbc3239beed451f37ae7e2090cfc7093660e8865bfef5ab828

// string-builder: build a long string by appending (see string-builder.mi).

use std::fmt::Write;
use std::time::Instant;

fn kernel(n: i64) -> String {
    let mut text = String::new();
    for i in 0..n {
        write!(text, "{}:{};", i, i * i % 1000).expect("writing to a String cannot fail");
    }
    let mut hash: i64 = 0;
    for byte in text.bytes() {
        hash = (hash * 31 + byte as i64) % 1000000007;
    }
    format!("{} {hash}", text.len())
}

fn run_hot(n: i64) -> String {
    let expected = kernel(n);
    let mut agree = true;
    for _ in 1..5 {
        let checksum = kernel(n);
        agree = agree && checksum == expected;
    }
    for _ in 0..5 {
        let start = Instant::now();
        let checksum = kernel(n);
        let elapsed = start.elapsed().as_nanos();
        eprintln!("hot_ns={elapsed}");
        agree = agree && checksum == expected;
    }
    if !agree {
        eprintln!("checksum differs between iterations");
        std::process::exit(1);
    }
    expected
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap_or_else(|_| std::process::exit(2));
    let input: i64 = text.trim().parse().unwrap_or_else(|_| std::process::exit(2));
    let hot = args.get(2).is_some_and(|a| a == "hot");
    println!("{}", if hot { run_hot(input) } else { kernel(input) });
}
