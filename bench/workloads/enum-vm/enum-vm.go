// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=3e937b50e93b73cb39e6c7b960051c40ada32d02c2fbddd78dee31ddb123ab1c

// enum-vm: a twelve-instruction register machine (see enum-vm.mi).
// Go has no sum types: an instruction is an opcode and its operands, switched on.

package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

type Op int

const (
	Const Op = iota
	Mov
	Add
	Sub
	Mul
	Mod
	And
	AddImm
	JumpIfLess
	JumpIfEqual
	Jump
	Halt
)

type Instr struct {
	Op      Op
	A, B, C int64
}

func program(n int64) []Instr {
	return []Instr{
		{Const, 0, 0, 0}, {Const, 1, 0, 0}, {Const, 2, n, 0}, {Const, 5, 97, 0}, {Const, 6, 63, 0},
		{Const, 4, 1000000007, 0}, {Const, 8, 0, 0}, {Const, 9, 3, 0}, {JumpIfLess, 0, 2, 10},
		{Halt, 0, 0, 0}, {Mul, 3, 0, 0}, {Mod, 3, 3, 5}, {And, 3, 3, 6}, {Add, 1, 1, 3}, {And, 7, 0, 6},
		{JumpIfEqual, 7, 8, 18}, {Sub, 10, 2, 0}, {Jump, 19, 0, 0}, {Mov, 10, 9, 0}, {Mod, 10, 10, 9},
		{Add, 1, 1, 10}, {Mod, 1, 1, 4}, {AddImm, 0, 1, 0}, {Jump, 8, 0, 0},
	}
}

func kernel(n int64) string {
	code := program(n)
	var regs [16]int64
	var pc, steps int64
	running := true
	for running {
		i := code[pc]
		next := pc + 1
		switch i.Op {
		case Const:
			regs[i.A] = i.B
		case Mov:
			regs[i.A] = regs[i.B]
		case Add:
			regs[i.A] = regs[i.B] + regs[i.C]
		case Sub:
			regs[i.A] = regs[i.B] - regs[i.C]
		case Mul:
			regs[i.A] = regs[i.B] * regs[i.C]
		case Mod:
			regs[i.A] = regs[i.B] % regs[i.C]
		case And:
			regs[i.A] = regs[i.B] & regs[i.C]
		case AddImm:
			regs[i.A] = regs[i.A] + i.B
		case JumpIfLess:
			if regs[i.A] < regs[i.B] {
				next = i.C
			}
		case JumpIfEqual:
			if regs[i.A] == regs[i.B] {
				next = i.C
			}
		case Jump:
			next = i.A
		case Halt:
			running = false
		}
		pc = next
		steps++
	}
	return fmt.Sprintf("%d %d", regs[1], steps)
}

func runHot(input int64) string {
	expected := kernel(input)
	agree := true
	for i := 1; i < 5; i++ {
		checksum := kernel(input)
		agree = agree && checksum == expected
	}
	for i := 0; i < 5; i++ {
		start := time.Now()
		checksum := kernel(input)
		elapsed := time.Since(start).Nanoseconds()
		fmt.Fprintf(os.Stderr, "hot_ns=%d\n", elapsed)
		agree = agree && checksum == expected
	}
	if !agree {
		fmt.Fprintln(os.Stderr, "checksum differs between iterations")
		os.Exit(1)
	}
	return expected
}

func main() {
	text, err := os.ReadFile(os.Args[1])
	if err != nil {
		os.Exit(2)
	}
	n, err := strconv.ParseInt(strings.TrimSpace(string(text)), 10, 64)
	if err != nil {
		os.Exit(2)
	}
	if len(os.Args) > 2 && os.Args[2] == "hot" {
		fmt.Println(runHot(n))
	} else {
		fmt.Println(kernel(n))
	}
}
