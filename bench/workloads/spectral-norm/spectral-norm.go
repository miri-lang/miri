// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=a48e57979d9dd878cebc40a159ebc6875527368265b02611689a1af26d7bda15

// spectral-norm: power iteration on an infinite matrix (see spectral-norm.mi).
//
// Go may fuse a multiply and an add into one rounding; the other ports never
// do. Each product an add consumes is wrapped in float64(), which the language
// specification defines as an explicit rounding, so every language computes
// the same bits.

package main

import (
	"fmt"
	"math"
	"os"
	"strconv"
	"strings"
	"time"
)

func entry(i, j int64) float64 {
	return 1.0 / float64((i+j)*(i+j+1)/2+i+1)
}

func times(v []float64) []float64 {
	n := int64(len(v))
	out := make([]float64, 0, n)
	for i := int64(0); i < n; i++ {
		sum := 0.0
		for j := int64(0); j < n; j++ {
			sum = sum + float64(entry(i, j)*v[j])
		}
		out = append(out, sum)
	}
	return out
}

func timesTransposed(v []float64) []float64 {
	n := int64(len(v))
	out := make([]float64, 0, n)
	for i := int64(0); i < n; i++ {
		sum := 0.0
		for j := int64(0); j < n; j++ {
			sum = sum + float64(entry(j, i)*v[j])
		}
		out = append(out, sum)
	}
	return out
}

func timesBoth(v []float64) []float64 {
	return timesTransposed(times(v))
}

func kernel(n int64) string {
	u := make([]float64, n)
	for i := range u {
		u[i] = 1.0
	}
	var v []float64
	for round := 0; round < 10; round++ {
		v = timesBoth(u)
		u = timesBoth(v)
	}
	vbv, vv := 0.0, 0.0
	for i := range u {
		vbv = vbv + float64(u[i]*v[i])
		vv = vv + float64(v[i]*v[i])
	}
	return fmt.Sprintf("%d", int64(math.Sqrt(vbv/vv)*1000000000.0))
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
