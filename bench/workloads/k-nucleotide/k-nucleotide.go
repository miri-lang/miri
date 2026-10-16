// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=0a6ef207c1f9c9f668f7d05bd30ef6533761d68f05e118649fa3125458a515b3

// k-nucleotide: count the substrings of a DNA sequence (see k-nucleotide.mi).

package main

import (
	"fmt"
	"os"
	"sort"
	"strings"
	"time"
)

const motif = "GGTATTTTAATTTATAGT"

func frequencies(sequence string, k int) map[string]int {
	counts := map[string]int{}
	for i := 0; i+k <= len(sequence); i++ {
		counts[sequence[i:i+k]]++
	}
	return counts
}

func listing(sequence string, k int) string {
	counts := frequencies(sequence, k)
	keys := make([]string, 0, len(counts))
	for key := range counts {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		parts = append(parts, fmt.Sprintf("%s=%d", key, counts[key]))
	}
	return strings.Join(parts, " ")
}

func kernel(text string) string {
	sequence := strings.TrimSpace(text)
	parts := []string{listing(sequence, 1), listing(sequence, 2)}
	for _, length := range []int{3, 4, 6, 12, 18} {
		prefix := motif[:length]
		parts = append(parts, fmt.Sprintf("%s=%d", prefix, frequencies(sequence, length)[prefix]))
	}
	return strings.Join(parts, " ")
}

func runHot(input string) string {
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
	n := string(text)
	if len(os.Args) > 2 && os.Args[2] == "hot" {
		fmt.Println(runHot(n))
	} else {
		fmt.Println(kernel(n))
	}
}
