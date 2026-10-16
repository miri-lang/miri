// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=dd92f27473cb9597eed6b0c246faf1db667b0d9746798d262ab5552e88869442

// closures: capturing map, filter and fold pipelines (see closures.mi).

use std::time::Instant;

fn kernel(n: i64) -> String {
    let xs: Vec<i64> = (0..n).collect();
    let mut total = 0;
    for k in 1..=8 {
        let scale = k * 7;
        let threshold = 100 * k;
        let scaled: Vec<i64> = xs.iter().map(|x| (x * scale) % 1000).collect();
        let kept: Vec<i64> = scaled.into_iter().filter(|x| *x > threshold).collect();
        total = (total + kept.iter().fold(0, |a, b| (a + b) % 1000000007)) % 1000000007;
    }
    format!("{total}")
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
