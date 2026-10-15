// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=e6247912922e779ef167b687da20a049834bcee284236626bcf5fdbc068d435a

// sieve: the sieve of Eratosthenes (see sieve.mi).

package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

func kernel(n int64) string {
	composite := make([]bool, n+1)
	var count, sum int64
	for p := int64(2); p <= n; p++ {
		if !composite[p] {
			count++
			sum = (sum + p) % 1000000007
			for m := p * p; m <= n; m += p {
				composite[m] = true
			}
		}
	}
	return fmt.Sprintf("%d %d", count, sum)
}

func runHot(n int64) string {
	expected := kernel(n)
	agree := true
	for i := 1; i < 5; i++ {
		checksum := kernel(n)
		agree = agree && checksum == expected
	}
	for i := 0; i < 5; i++ {
		start := time.Now()
		checksum := kernel(n)
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
