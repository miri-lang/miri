// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=f03704a47b9ca4cb81906592eb514fae94595f95dc994cc996a8b4d697949f56

// wordfreq: count the words of a text (see wordfreq.mi).

use std::collections::HashMap;
use std::time::Instant;

fn kernel(text: &str) -> String {
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for line in text.split('\n') {
        for word in line.split(' ') {
            if !word.is_empty() {
                *counts.entry(word).or_insert(0) += 1;
            }
        }
    }
    let mut ranked: Vec<(&str, i64)> = counts.iter().map(|(word, count)| (*word, *count)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let top: Vec<String> = ranked.iter().take(10).map(|(word, count)| format!("{word}={count}")).collect();
    format!("{} {}", counts.len(), top.join(","))
}

fn run_hot(n: &str) -> String {
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
    let input = std::fs::read_to_string(&args[1]).unwrap_or_else(|_| std::process::exit(2));
    let hot = args.get(2).is_some_and(|a| a == "hot");
    println!("{}", if hot { run_hot(&input) } else { kernel(&input) });
}
