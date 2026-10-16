// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=4343f3221538e074067c7b21b5a75d0ac817a76806d60eb91add443623a5d6a5

// list-sort: sort generated numbers, then records by key (see list-sort.mi).

use std::time::Instant;

#[derive(Clone, Copy)]
struct Record {
    key: i64,
    id: i64,
}

fn kernel(n: i64) -> String {
    let mut numbers = Vec::with_capacity(n as usize);
    let mut records = Vec::with_capacity(n as usize);
    let mut state: i64 = 7;
    for i in 0..n {
        state = state * 48271 % 2147483647;
        numbers.push(state % 1000000);
        records.push(Record { key: state % 1000, id: i });
    }
    numbers.sort();
    let mut by_key = records.clone();
    by_key.sort_by(|a, b| a.key.cmp(&b.key).then(b.id.cmp(&a.id)));
    let mut number_hash: i64 = 0;
    let mut record_hash: i64 = 0;
    for i in 0..n as usize {
        number_hash = (number_hash * 31 + numbers[i]) % 1000000007;
        record_hash = (record_hash * 31 + by_key[i].id) % 1000000007;
    }
    format!("{number_hash} {record_hash}")
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
