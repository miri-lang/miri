// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=a3ae48b73c0d28f79797db05d087eeb5e8bb009db1c4015959ace0ef517d5a8a

// closures: capturing map, filter and fold pipelines (see closures.mi).

package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

func mapInts(xs []int64, f func(int64) int64) []int64 {
	out := make([]int64, len(xs))
	for i, x := range xs {
		out[i] = f(x)
	}
	return out
}

func filterInts(xs []int64, keep func(int64) bool) []int64 {
	var out []int64
	for _, x := range xs {
		if keep(x) {
			out = append(out, x)
		}
	}
	return out
}

func reduceInts(xs []int64, init int64, f func(int64, int64) int64) int64 {
	acc := init
	for _, x := range xs {
		acc = f(acc, x)
	}
	return acc
}

func kernel(n int64) string {
	xs := make([]int64, n)
	for i := range xs {
		xs[i] = int64(i)
	}
	var total int64
	for k := int64(1); k <= 8; k++ {
		scale := k * 7
		threshold := 100 * k
		scaled := mapInts(xs, func(x int64) int64 { return (x * scale) % 1000 })
		kept := filterInts(scaled, func(x int64) bool { return x > threshold })
		total = (total + reduceInts(kept, 0, func(a, b int64) int64 { return (a + b) % 1000000007 })) % 1000000007
	}
	return fmt.Sprintf("%d", total)
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
