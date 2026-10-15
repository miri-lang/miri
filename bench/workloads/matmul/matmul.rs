// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=ecf382d0ee39270a3d7a28ea8efe703b38811ae7da01d0f7eeddfe0bd763a005

// matmul: the naive triple-loop matrix product (see matmul.mi).

use std::time::Instant;

fn matrix(n: i64, offset: i64) -> Vec<Vec<f64>> {
    (0..n)
        .map(|i| (0..n).map(|j| ((i * j + offset * (i + j)) % 10) as f64).collect())
        .collect()
}

fn kernel(n: i64) -> String {
    let a = matrix(n, 1);
    let b = matrix(n, 2);
    let n = n as usize;
    let mut total = 0.0;
    for row in &a {
        for j in 0..n {
            let mut sum = 0.0;
            for k in 0..n {
                sum = sum + row[k] * b[k][j];
            }
            total = total + sum;
        }
    }
    format!("{}", total as i64)
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
