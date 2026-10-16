// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=e48dd5e58f038b1b213f1fa15871cc8931d476ad943998a45747fd3b001aa338

// generics: one generic stack at three element types (see generics.mi).

package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

type Stack[T any] struct {
	items []T
}

func (s *Stack[T]) Push(item T) { s.items = append(s.items, item) }

func (s *Stack[T]) Pop() T {
	item := s.items[len(s.items)-1]
	s.items = s.items[:len(s.items)-1]
	return item
}

func (s *Stack[T]) Size() int { return len(s.items) }

type Pair[A, B any] struct {
	First  A
	Second B
}

func kernel(n int64) string {
	var ints Stack[int64]
	var floats Stack[float64]
	var pairs Stack[Pair[int64, float64]]
	for i := int64(0); i < n; i++ {
		ints.Push(i % 1000)
		floats.Push(float64(i%100) * 0.5)
		pairs.Push(Pair[int64, float64]{i % 10, float64(i % 4)})
	}
	var a int64
	for ints.Size() > 0 {
		a = (a*31 + ints.Pop()) % 1000000007
	}
	b := 0.0
	for floats.Size() > 0 {
		b = b + floats.Pop()
	}
	var c int64
	for pairs.Size() > 0 {
		p := pairs.Pop()
		c = (c*7 + p.First + int64(p.Second)) % 1000000007
	}
	return fmt.Sprintf("%d %d %d", a, int64(b), c)
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
