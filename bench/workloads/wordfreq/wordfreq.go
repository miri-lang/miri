// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-25 by review-agent sha256=bb96bc3fe48d12a66a9911409a32e876daa83a04c60746c08ae0caec06fe8ff0

// wordfreq: count the words of a text (see wordfreq.mi).

package main

import (
	"cmp"
	"fmt"
	"os"
	"slices"
	"strings"
	"time"
)

type entry struct {
	word  string
	count int
}

func kernel(text string) string {
	counts := map[string]int{}
	for _, line := range strings.Split(text, "\n") {
		for _, word := range strings.Split(line, " ") {
			if word != "" {
				counts[word]++
			}
		}
	}
	entries := make([]entry, 0, len(counts))
	for word, count := range counts {
		entries = append(entries, entry{word, count})
	}
	slices.SortFunc(entries, func(a, b entry) int {
		if a.count != b.count {
			return cmp.Compare(b.count, a.count)
		}
		return strings.Compare(a.word, b.word)
	})
	top := make([]string, 0, 10)
	for i := 0; i < 10 && i < len(entries); i++ {
		top = append(top, fmt.Sprintf("%s=%d", entries[i].word, entries[i].count))
	}
	return fmt.Sprintf("%d %s", len(counts), strings.Join(top, ","))
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
