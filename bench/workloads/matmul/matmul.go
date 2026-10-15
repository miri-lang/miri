// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=b6d41aee7466dc6bb2373f82596aed7a97b62128c2dfdeb04478facb79f4bc34

// matmul: the naive triple-loop matrix product (see matmul.mi).

package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

func matrix(n, offset int64) [][]float64 {
	rows := make([][]float64, n)
	for i := int64(0); i < n; i++ {
		row := make([]float64, n)
		for j := int64(0); j < n; j++ {
			row[j] = float64((i*j + offset*(i+j)) % 10)
		}
		rows[i] = row
	}
	return rows
}

func kernel(n int64) string {
	a := matrix(n, 1)
	b := matrix(n, 2)
	total := 0.0
	for _, row := range a {
		for j := int64(0); j < n; j++ {
			sum := 0.0
			for k := int64(0); k < n; k++ {
				sum = sum + row[k]*b[k][j]
			}
			total = total + sum
		}
	}
	return fmt.Sprintf("%d", int64(total))
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
