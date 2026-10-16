// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=f862e5c0d5e9e8a226f4d35d241b82c2b22cd38d9feaa8c9ee02397add8e5587

// binary-trees: allocate and walk many complete binary trees (see binary-trees.mi).

package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

type Node struct {
	left, right *Node
}

func bottomUp(depth int64) *Node {
	if depth == 0 {
		return &Node{}
	}
	return &Node{bottomUp(depth - 1), bottomUp(depth - 1)}
}

func check(node *Node) int64 {
	if node.left == nil || node.right == nil {
		return 1
	}
	return 1 + check(node.left) + check(node.right)
}

func kernel(maxDepth int64) string {
	stretch := check(bottomUp(maxDepth + 1))
	longLived := bottomUp(maxDepth)
	var total int64
	for depth := int64(4); depth <= maxDepth; depth += 2 {
		iterations := int64(1) << (maxDepth - depth + 4)
		for i := int64(0); i < iterations; i++ {
			total += check(bottomUp(depth))
		}
	}
	return fmt.Sprintf("%d %d %d", stretch, total, check(longLived))
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
