// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=1e3bb2483086402032af2c3a73d355beaa604fd9ab26a7c5d353571284189113

// list-sort: sort generated numbers, then records by key (see list-sort.mi).

package main

import (
	"fmt"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"
)

type Record struct {
	key, id int64
}

func kernel(n int64) string {
	numbers := make([]int64, n)
	records := make([]Record, n)
	state := int64(7)
	for i := int64(0); i < n; i++ {
		state = state * 48271 % 2147483647
		numbers[i] = state % 1000000
		records[i] = Record{state % 1000, i}
	}
	sort.Slice(numbers, func(a, b int) bool { return numbers[a] < numbers[b] })
	byKey := append([]Record(nil), records...)
	sort.SliceStable(byKey, func(a, b int) bool {
		if byKey[a].key != byKey[b].key {
			return byKey[a].key < byKey[b].key
		}
		return byKey[a].id > byKey[b].id
	})
	var numberHash, recordHash int64
	for i := int64(0); i < n; i++ {
		numberHash = (numberHash*31 + numbers[i]) % 1000000007
		recordHash = (recordHash*31 + byKey[i].id) % 1000000007
	}
	return fmt.Sprintf("%d %d", numberHash, recordHash)
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
