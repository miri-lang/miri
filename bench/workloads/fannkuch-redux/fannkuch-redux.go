// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=31819afae6c49dd84ab99502e640b45ff8d76a225726ee40b5f7e048f5a54a5a

// fannkuch-redux: pancake flips over every permutation (see fannkuch-redux.mi).

package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

func countFlips(perm, p []int) int64 {
	copy(p, perm)
	var flips int64
	first := p[0]
	for first != 0 {
		for lo, hi := 0, first; lo < hi; lo, hi = lo+1, hi-1 {
			p[lo], p[hi] = p[hi], p[lo]
		}
		flips++
		first = p[0]
	}
	return flips
}

func kernel(n64 int64) string {
	n := int(n64)
	perm := make([]int, n)
	count := make([]int, n)
	scratch := make([]int, n)
	for i := range perm {
		perm[i] = i
	}
	var checksum, maxFlips, index int64
	for {
		flips := countFlips(perm, scratch)
		if flips > maxFlips {
			maxFlips = flips
		}
		if index%2 == 0 {
			checksum += flips
		} else {
			checksum -= flips
		}
		index++
		r := 1
		for ; r < n; r++ {
			first := perm[0]
			copy(perm[:r], perm[1:r+1])
			perm[r] = first
			count[r]++
			if count[r] <= r {
				break
			}
			count[r] = 0
		}
		if r >= n {
			return fmt.Sprintf("%d %d", checksum, maxFlips)
		}
	}
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
