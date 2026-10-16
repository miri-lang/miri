// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=11d31a7fdb4f7fd9bfbbac59e51ea9b4c2c7d43f9c80be5dab65555e7fd99925

// string-builder: build a long string by appending (see string-builder.mi).

package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

func kernel(n int64) string {
	var builder strings.Builder
	for i := int64(0); i < n; i++ {
		fmt.Fprintf(&builder, "%d:%d;", i, i*i%1000)
	}
	text := builder.String()
	var hash int64
	for k := 0; k < len(text); k++ {
		hash = (hash*31 + int64(text[k])) % 1000000007
	}
	return fmt.Sprintf("%d %d", len(text), hash)
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
