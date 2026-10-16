// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=3f89e537d3f44e62b14d51e254bea7454df2ac32460fa444f1067ef4e9e70676

// generics: one generic stack at three element types (see generics.mi).

use std::time::Instant;

struct Stack<T> {
    items: Vec<T>,
}

impl<T> Stack<T> {
    fn new() -> Self {
        Stack { items: Vec::new() }
    }

    fn push(&mut self, item: T) {
        self.items.push(item);
    }

    fn pop(&mut self) -> Option<T> {
        self.items.pop()
    }

    fn size(&self) -> usize {
        self.items.len()
    }
}

struct Pair<A, B> {
    first: A,
    second: B,
}

fn kernel(n: i64) -> String {
    let mut ints = Stack::new();
    let mut floats = Stack::new();
    let mut pairs = Stack::new();
    for i in 0..n {
        ints.push(i % 1000);
        floats.push((i % 100) as f64 * 0.5);
        pairs.push(Pair { first: i % 10, second: (i % 4) as f64 });
    }
    let mut a: i64 = 0;
    while ints.size() > 0 {
        a = (a * 31 + ints.pop().unwrap_or(0)) % 1000000007;
    }
    let mut b = 0.0;
    while floats.size() > 0 {
        b = b + floats.pop().unwrap_or(0.0);
    }
    let mut c: i64 = 0;
    while let Some(p) = pairs.pop() {
        c = (c * 7 + p.first + p.second as i64) % 1000000007;
    }
    format!("{a} {} {c}", b as i64)
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
