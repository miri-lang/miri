// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=7da1e556c8c4cccb1d71bc92909f6f019f37fe949c8492b48cc967a952f3ad62

// sieve: the sieve of Eratosthenes (see sieve.mi).

use std::time::Instant;

fn kernel(n: i64) -> String {
    let n = n as usize;
    let mut composite = vec![false; n + 1];
    let mut count = 0;
    let mut sum: i64 = 0;
    for p in 2..=n {
        if !composite[p] {
            count += 1;
            sum = (sum + p as i64) % 1000000007;
            for m in (p * p..=n).step_by(p) {
                composite[m] = true;
            }
        }
    }
    format!("{count} {sum}")
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
    let text = std::fs::read_to_string(&args[1]).expect("readable input");
    let n: i64 = text.trim().parse().expect("an integer parameter");
    let hot = args.get(2).is_some_and(|a| a == "hot");
    println!("{}", if hot { run_hot(n) } else { kernel(n) });
}
