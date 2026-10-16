// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=3ef94147b013d4d01579b2ed896ba13bfcb7b51644087ff6fe1ce7f761e79c99

// k-nucleotide: count the substrings of a DNA sequence (see k-nucleotide.mi).

use std::collections::HashMap;
use std::time::Instant;

const MOTIF: &str = "GGTATTTTAATTTATAGT";

fn frequencies(sequence: &str, k: usize) -> HashMap<&str, i64> {
    let mut counts = HashMap::new();
    for i in 0..(sequence.len() + 1).saturating_sub(k) {
        *counts.entry(&sequence[i..i + k]).or_insert(0) += 1;
    }
    counts
}

fn listing(sequence: &str, k: usize) -> String {
    let counts = frequencies(sequence, k);
    let mut keys: Vec<&str> = counts.keys().copied().collect();
    keys.sort();
    keys.iter().map(|key| format!("{key}={}", counts[key])).collect::<Vec<_>>().join(" ")
}

fn kernel(text: &str) -> String {
    let sequence = text.trim();
    let mut parts = vec![listing(sequence, 1), listing(sequence, 2)];
    for length in [3, 4, 6, 12, 18] {
        let prefix = &MOTIF[..length];
        let count = frequencies(sequence, length).get(prefix).copied().unwrap_or(0);
        parts.push(format!("{prefix}={count}"));
    }
    parts.join(" ")
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
