// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=4ec12caeda91478da494e2589649827a3c323172996b7df6d776bb15c0cfa79c

// csv-aggregate: totals by key over a generated CSV file (see csv-aggregate.mi).

package main

import (
	"fmt"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"
)

type Totals struct {
	Count    int64
	Quantity int64
	Amount   float64
}

func kernel(text string) string {
	groups := map[string]*Totals{}
	for _, line := range strings.Split(text, "\n")[1:] {
		if line == "" {
			continue
		}
		fields := strings.Split(line, ",")
		t, ok := groups[fields[0]]
		if !ok {
			t = &Totals{}
			groups[fields[0]] = t
		}
		quantity, _ := strconv.ParseInt(fields[1], 10, 64)
		amount, _ := strconv.ParseFloat(fields[2], 64)
		t.Count++
		t.Quantity += quantity
		t.Amount = t.Amount + amount
	}
	keys := make([]string, 0, len(groups))
	for key := range groups {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		t := groups[key]
		parts = append(parts, fmt.Sprintf("%s:%d:%d:%d", key, t.Count, t.Quantity, int64(t.Amount*100.0)))
	}
	return strings.Join(parts, ",")
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
