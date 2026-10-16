// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=26cdb5be64414d8b41a33e036915dafde2539641cd1c62c322607702db964559

// ray-tracer: spheres shaded by a light, one ray per pixel (see ray-tracer.mi).
//
// Go may fuse a multiply and an add into one rounding, even across inlined
// calls; the other ports never do. Every product is wrapped in float64(),
// which the language specification defines as an explicit rounding, so every
// language computes the same bits.

package main

import (
	"fmt"
	"math"
	"os"
	"strconv"
	"strings"
	"time"
)

type Vec3 struct{ X, Y, Z float64 }

type Sphere struct {
	Center Vec3
	Radius float64
}

func sub(a, b Vec3) Vec3 { return Vec3{a.X - b.X, a.Y - b.Y, a.Z - b.Z} }

func scale(a Vec3, k float64) Vec3 { return Vec3{float64(a.X * k), float64(a.Y * k), float64(a.Z * k)} }

func dot(a, b Vec3) float64 {
	return float64(float64(a.X*b.X)+float64(a.Y*b.Y)) + float64(a.Z*b.Z)
}

func normalize(a Vec3) Vec3 { return scale(a, 1.0/math.Sqrt(dot(a, a))) }

func hit(sphere Sphere, direction Vec3) float64 {
	oc := scale(sphere.Center, -1.0)
	b := dot(oc, direction)
	c := dot(oc, oc) - float64(sphere.Radius*sphere.Radius)
	discriminant := float64(b*b) - c
	if discriminant < 0.0 {
		return -1.0
	}
	return -b - math.Sqrt(discriminant)
}

func kernel(n int64) string {
	spheres := []Sphere{
		{Vec3{0.0, 0.0, 5.0}, 1.0},
		{Vec3{1.5, 0.5, 6.0}, 1.0},
		{Vec3{-1.5, -0.5, 4.5}, 0.75},
		{Vec3{0.0, -101.0, 5.0}, 100.0},
	}
	light := normalize(Vec3{-1.0, 1.0, -1.0})
	var hash, hits int64
	for py := int64(0); py < n; py++ {
		for px := int64(0); px < n; px++ {
			direction := normalize(Vec3{float64(px)/float64(n) - 0.5, 0.5 - float64(py)/float64(n), 1.0})
			nearest := 1000000.0
			var shade int64
			for _, sphere := range spheres {
				t := hit(sphere, direction)
				if t > 0.0 && t < nearest {
					nearest = t
					normal := normalize(sub(scale(direction, t), sphere.Center))
					lambert := dot(normal, light)
					if lambert > 0.0 {
						shade = int64(float64(lambert * 255.0))
					} else {
						shade = 0
					}
				}
			}
			if nearest < 1000000.0 {
				hits++
			}
			hash = (hash*31 + shade) % 1000000007
		}
	}
	return fmt.Sprintf("%d %d", hash, hits)
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
