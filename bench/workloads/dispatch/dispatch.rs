// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=fef61095af853284518c466ba19f31329b39e0d95d6be9ed3db8300a03f90601

// dispatch: shapes behind one interface, their areas summed (see dispatch.mi).

use std::time::Instant;

trait Shape {
    fn area(&self) -> f64;
}

struct Circle {
    radius: f64,
}

struct Square {
    side: f64,
}

struct Rectangle {
    width: f64,
    height: f64,
}

struct Triangle {
    base: f64,
    height: f64,
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        3.0 * self.radius * self.radius
    }
}

impl Shape for Square {
    fn area(&self) -> f64 {
        self.side * self.side
    }
}

impl Shape for Rectangle {
    fn area(&self) -> f64 {
        self.width * self.height
    }
}

impl Shape for Triangle {
    fn area(&self) -> f64 {
        self.base * self.height * 0.5
    }
}

fn shape(i: i64) -> Box<dyn Shape> {
    let a = (i % 7 + 1) as f64;
    let b = (i % 5 + 1) as f64;
    match i % 4 {
        0 => Box::new(Circle { radius: a }),
        1 => Box::new(Square { side: a }),
        2 => Box::new(Rectangle { width: a, height: b }),
        _ => Box::new(Triangle { base: a, height: b }),
    }
}

fn kernel(n: i64) -> String {
    let shapes: Vec<Box<dyn Shape>> = (0..1000).map(shape).collect();
    let mut total = 0.0;
    for _ in 0..n {
        for s in &shapes {
            total = total + s.area();
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
    let text = std::fs::read_to_string(&args[1]).unwrap_or_else(|_| std::process::exit(2));
    let input: i64 = text.trim().parse().unwrap_or_else(|_| std::process::exit(2));
    let hot = args.get(2).is_some_and(|a| a == "hot");
    println!("{}", if hot { run_hot(input) } else { kernel(input) });
}
