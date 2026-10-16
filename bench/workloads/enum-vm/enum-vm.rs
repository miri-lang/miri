// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=c898923bae362fc76353b7430ba58e958151f20c30bc86e8306a8cdbff95e0e5

// enum-vm: a twelve-instruction register machine (see enum-vm.mi).

use std::time::Instant;

enum Instr {
    Const(usize, i64),
    Mov(usize, usize),
    Add(usize, usize, usize),
    Sub(usize, usize, usize),
    Mul(usize, usize, usize),
    Mod(usize, usize, usize),
    And(usize, usize, usize),
    AddImm(usize, i64),
    JumpIfLess(usize, usize, usize),
    JumpIfEqual(usize, usize, usize),
    Jump(usize),
    Halt,
}

fn program(n: i64) -> Vec<Instr> {
    use Instr::*;
    vec![
        Const(0, 0), Const(1, 0), Const(2, n), Const(5, 97), Const(6, 63), Const(4, 1000000007),
        Const(8, 0), Const(9, 3), JumpIfLess(0, 2, 10), Halt, Mul(3, 0, 0), Mod(3, 3, 5),
        And(3, 3, 6), Add(1, 1, 3), And(7, 0, 6), JumpIfEqual(7, 8, 18), Sub(10, 2, 0), Jump(19),
        Mov(10, 9), Mod(10, 10, 9), Add(1, 1, 10), Mod(1, 1, 4), AddImm(0, 1), Jump(8),
    ]
}

fn kernel(n: i64) -> String {
    let code = program(n);
    let mut regs = [0i64; 16];
    let mut pc = 0;
    let mut steps: i64 = 0;
    let mut running = true;
    while running {
        let mut next = pc + 1;
        match code[pc] {
            Instr::Const(d, v) => regs[d] = v,
            Instr::Mov(d, a) => regs[d] = regs[a],
            Instr::Add(d, a, b) => regs[d] = regs[a] + regs[b],
            Instr::Sub(d, a, b) => regs[d] = regs[a] - regs[b],
            Instr::Mul(d, a, b) => regs[d] = regs[a] * regs[b],
            Instr::Mod(d, a, b) => regs[d] = regs[a] % regs[b],
            Instr::And(d, a, b) => regs[d] = regs[a] & regs[b],
            Instr::AddImm(d, v) => regs[d] = regs[d] + v,
            Instr::JumpIfLess(a, b, target) => next = if regs[a] < regs[b] { target } else { next },
            Instr::JumpIfEqual(a, b, target) => next = if regs[a] == regs[b] { target } else { next },
            Instr::Jump(target) => next = target,
            Instr::Halt => running = false,
        }
        pc = next;
        steps += 1;
    }
    format!("{} {steps}", regs[1])
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
