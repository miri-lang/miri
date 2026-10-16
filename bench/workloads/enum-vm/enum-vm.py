# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=7e6f2e3e398ab4e3d075469d15ca0b078b53e05dbcea886185c35c176b8ccdc3

"""enum-vm: a twelve-instruction register machine (see enum-vm.mi)."""

import sys
import time
from dataclasses import dataclass


@dataclass(frozen=True)
class Const:
    d: int
    v: int


@dataclass(frozen=True)
class Mov:
    d: int
    a: int


@dataclass(frozen=True)
class Add:
    d: int
    a: int
    b: int


@dataclass(frozen=True)
class Sub:
    d: int
    a: int
    b: int


@dataclass(frozen=True)
class Mul:
    d: int
    a: int
    b: int


@dataclass(frozen=True)
class Mod:
    d: int
    a: int
    b: int


@dataclass(frozen=True)
class And:
    d: int
    a: int
    b: int


@dataclass(frozen=True)
class AddImm:
    d: int
    v: int


@dataclass(frozen=True)
class JumpIfLess:
    a: int
    b: int
    target: int


@dataclass(frozen=True)
class JumpIfEqual:
    a: int
    b: int
    target: int


@dataclass(frozen=True)
class Jump:
    target: int


@dataclass(frozen=True)
class Halt:
    pass


def program(n):
    return [
        Const(0, 0), Const(1, 0), Const(2, n), Const(5, 97), Const(6, 63), Const(4, 1000000007),
        Const(8, 0), Const(9, 3), JumpIfLess(0, 2, 10), Halt(), Mul(3, 0, 0), Mod(3, 3, 5),
        And(3, 3, 6), Add(1, 1, 3), And(7, 0, 6), JumpIfEqual(7, 8, 18), Sub(10, 2, 0), Jump(19),
        Mov(10, 9), Mod(10, 10, 9), Add(1, 1, 10), Mod(1, 1, 4), AddImm(0, 1), Jump(8),
    ]


def kernel(n):
    code = program(n)
    regs = [0] * 16
    pc = 0
    steps = 0
    running = True
    while running:
        following = pc + 1
        match code[pc]:
            case Const(d, v):
                regs[d] = v
            case Mov(d, a):
                regs[d] = regs[a]
            case Add(d, a, b):
                regs[d] = regs[a] + regs[b]
            case Sub(d, a, b):
                regs[d] = regs[a] - regs[b]
            case Mul(d, a, b):
                regs[d] = regs[a] * regs[b]
            case Mod(d, a, b):
                regs[d] = regs[a] % regs[b]
            case And(d, a, b):
                regs[d] = regs[a] & regs[b]
            case AddImm(d, v):
                regs[d] = regs[d] + v
            case JumpIfLess(a, b, target):
                following = target if regs[a] < regs[b] else following
            case JumpIfEqual(a, b, target):
                following = target if regs[a] == regs[b] else following
            case Jump(target):
                following = target
            case Halt():
                running = False
        pc = following
        steps += 1
    return f"{regs[1]} {steps}"


def run_hot(n):
    expected = kernel(n)
    agree = True
    for _ in range(4):
        checksum = kernel(n)
        agree = agree and checksum == expected
    for _ in range(5):
        start = time.perf_counter_ns()
        checksum = kernel(n)
        elapsed = time.perf_counter_ns() - start
        print(f"hot_ns={elapsed}", file=sys.stderr)
        agree = agree and checksum == expected
    if not agree:
        print("checksum differs between iterations", file=sys.stderr)
        sys.exit(1)
    return expected


def main():
    with open(sys.argv[1]) as f:
        n = int(f.read().strip())
    hot = len(sys.argv) > 2 and sys.argv[2] == "hot"
    print(run_hot(n) if hot else kernel(n))


if __name__ == "__main__":
    main()
