// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=c0ffe80bbf2556eb993521cec2036a4beb27b52f914c017a650022b1d73bf374

// csv-aggregate: totals by key over a generated CSV file (see csv-aggregate.mi).

use std::collections::HashMap;
use std::time::Instant;

#[derive(Default)]
struct Totals {
    count: i64,
    quantity: i64,
    amount: f64,
}

fn kernel(text: &str) -> String {
    let mut groups: HashMap<&str, Totals> = HashMap::new();
    for line in text.split('\n').skip(1) {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(',').collect();
        let totals = groups.entry(fields[0]).or_default();
        totals.count += 1;
        totals.quantity += fields[1].parse::<i64>().unwrap_or(0);
        totals.amount = totals.amount + fields[2].parse::<f64>().unwrap_or(0.0);
    }
    let mut keys: Vec<&&str> = groups.keys().collect();
    keys.sort();
    keys.iter()
        .map(|key| {
            let t = &groups[**key];
            format!("{key}:{}:{}:{}", t.count, t.quantity, (t.amount * 100.0) as i64)
        })
        .collect::<Vec<_>>()
        .join(",")
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
