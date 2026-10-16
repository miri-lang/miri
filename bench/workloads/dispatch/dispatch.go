// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=b2798884ac490a71a118fe142153bdef09f8dc721641def0b8a1ceb0a32fc7e1

// dispatch: shapes behind one interface, their areas summed (see dispatch.mi).

package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

type Shape interface {
	Area() float64
}

type Circle struct{ radius float64 }
type Square struct{ side float64 }
type Rectangle struct{ width, height float64 }
type Triangle struct{ base, height float64 }

func (c Circle) Area() float64    { return 3.0 * c.radius * c.radius }
func (s Square) Area() float64    { return s.side * s.side }
func (r Rectangle) Area() float64 { return r.width * r.height }
func (t Triangle) Area() float64  { return t.base * t.height * 0.5 }

func shape(i int64) Shape {
	a := float64(i%7 + 1)
	b := float64(i%5 + 1)
	switch i % 4 {
	case 0:
		return &Circle{a}
	case 1:
		return &Square{a}
	case 2:
		return &Rectangle{a, b}
	default:
		return &Triangle{a, b}
	}
}

func kernel(n int64) string {
	shapes := make([]Shape, 0, 1000)
	for i := int64(0); i < 1000; i++ {
		shapes = append(shapes, shape(i))
	}
	total := 0.0
	for round := int64(0); round < n; round++ {
		for _, s := range shapes {
			total = total + s.Area()
		}
	}
	return fmt.Sprintf("%d", int64(total))
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
