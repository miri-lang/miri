// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=8ae6c70ccd92e1342a504cc59b5b5e36c6e536e7f63230a3efac54aa0aca4227

// spectral-norm: power iteration on an infinite matrix (see spectral-norm.mi).

use std::time::Instant;

fn entry(i: i64, j: i64) -> f64 {
    1.0 / ((i + j) * (i + j + 1) / 2 + i + 1) as f64
}

fn times(v: &[f64]) -> Vec<f64> {
    let n = v.len() as i64;
    (0..n)
        .map(|i| {
            let mut sum = 0.0;
            for j in 0..n {
                sum = sum + entry(i, j) * v[j as usize];
            }
            sum
        })
        .collect()
}

fn times_transposed(v: &[f64]) -> Vec<f64> {
    let n = v.len() as i64;
    (0..n)
        .map(|i| {
            let mut sum = 0.0;
            for j in 0..n {
                sum = sum + entry(j, i) * v[j as usize];
            }
            sum
        })
        .collect()
}

fn times_both(v: &[f64]) -> Vec<f64> {
    times_transposed(&times(v))
}

fn kernel(n: i64) -> String {
    let mut u = vec![1.0; n as usize];
    let mut v = Vec::new();
    for _ in 0..10 {
        v = times_both(&u);
        u = times_both(&v);
    }
    let mut vbv = 0.0;
    let mut vv = 0.0;
    for i in 0..n as usize {
        vbv = vbv + u[i] * v[i];
        vv = vv + v[i] * v[i];
    }
    format!("{}", ((vbv / vv).sqrt() * 1000000000.0) as i64)
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
