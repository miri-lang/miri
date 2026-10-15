// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=1c5a4a5036bb17c45766af56b7a960ce22dda66a2385a99233d9d3a3afb065ae

// mandelbrot: the Mandelbrot set packed as a bitmap (see mandelbrot.mi).
//
// Go may fuse a multiply and an add into one rounding; the other ports never
// do. Each product an add consumes is wrapped in float64(), which the language
// specification defines as an explicit rounding, so every language computes
// the same bits.

package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

func staysBounded(cr, ci float64) bool {
	zr, zi := 0.0, 0.0
	for k := 0; k < 50; k++ {
		tr := float64(float64(zr*zr)-float64(zi*zi)) + cr
		zi = float64(2.0*zr*zi) + ci
		zr = tr
		if float64(zr*zr)+float64(zi*zi) > 4.0 {
			return false
		}
	}
	return true
}

func kernel(n int64) string {
	var hash, inside int64
	for y := int64(0); y < n; y++ {
		ci := float64(2.0*float64(y))/float64(n) - 1.0
		var pixels, bits int64
		for x := int64(0); x < n; x++ {
			cr := float64(2.0*float64(x))/float64(n) - 1.5
			pixels = pixels * 2
			if staysBounded(cr, ci) {
				pixels++
				inside++
			}
			bits++
			if bits == 8 {
				hash = (hash*31 + pixels) % 1000000007
				pixels = 0
				bits = 0
			}
		}
		if bits > 0 {
			pixels <<= 8 - bits
			hash = (hash*31 + pixels) % 1000000007
		}
	}
	return fmt.Sprintf("%d %d", hash, inside)
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
