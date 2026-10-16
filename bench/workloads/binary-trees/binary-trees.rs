// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=1a44636179450603084a14975e0cdd283d2cb7a74ffe0d1b79f412532ee37293

// binary-trees: allocate and walk many complete binary trees (see binary-trees.mi).

use std::time::Instant;

struct Node {
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

fn bottom_up(depth: i64) -> Box<Node> {
    if depth == 0 {
        return Box::new(Node { left: None, right: None });
    }
    Box::new(Node { left: Some(bottom_up(depth - 1)), right: Some(bottom_up(depth - 1)) })
}

fn check(node: &Node) -> i64 {
    match (&node.left, &node.right) {
        (Some(left), Some(right)) => 1 + check(left) + check(right),
        _ => 1,
    }
}

fn kernel(max_depth: i64) -> String {
    let stretch = check(&bottom_up(max_depth + 1));
    let long_lived = bottom_up(max_depth);
    let mut total = 0;
    let mut depth = 4;
    while depth <= max_depth {
        let iterations = 1i64 << (max_depth - depth + 4);
        for _ in 0..iterations {
            total += check(&bottom_up(depth));
        }
        depth += 2;
    }
    format!("{stretch} {total} {}", check(&long_lived))
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
