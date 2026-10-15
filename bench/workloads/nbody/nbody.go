// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=6c287c87a9c862a7c681a273c2c959f079b06c6bf3663d6b9b7a8c2b8f87a510

// nbody: the Jovian planets orbiting the sun (see nbody.mi).
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

const (
	pi          = 3.141592653589793
	solarMass   = 4.0 * pi * pi
	daysPerYear = 365.24
)

type Body struct {
	x, y, z, vx, vy, vz, mass float64
}

func planet(x, y, z, vx, vy, vz, mass float64) Body {
	d := daysPerYear
	return Body{x, y, z, vx * d, vy * d, vz * d, mass * solarMass}
}

func solarSystem() []Body {
	return []Body{
		{0.0, 0.0, 0.0, 0.0, 0.0, 0.0, solarMass},
		planet(4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01, 1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05, 9.54791938424326609e-04),
		planet(8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01, -2.76742510726862411e-03, 4.99852801234917238e-03, 2.30417297573763929e-05, 2.85885980666130812e-04),
		planet(1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01, 2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05, 4.36624404335156298e-05),
		planet(1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01, 2.68067772490389322e-03, 1.62824170038242295e-03, -9.51592254519715870e-05, 5.15138902046611451e-05),
	}
}

func offsetMomentum(bodies []Body) {
	var px, py, pz float64
	for _, b := range bodies {
		px = px + float64(b.vx*b.mass)
		py = py + float64(b.vy*b.mass)
		pz = pz + float64(b.vz*b.mass)
	}
	bodies[0].vx = -px / solarMass
	bodies[0].vy = -py / solarMass
	bodies[0].vz = -pz / solarMass
}

func energy(bodies []Body) float64 {
	e := 0.0
	for i, b := range bodies {
		e = e + float64(0.5*b.mass*(float64(float64(b.vx*b.vx)+float64(b.vy*b.vy))+float64(b.vz*b.vz)))
		for _, o := range bodies[i+1:] {
			dx := b.x - o.x
			dy := b.y - o.y
			dz := b.z - o.z
			e = e - float64(b.mass*o.mass)/math.Sqrt(float64(float64(dx*dx)+float64(dy*dy))+float64(dz*dz))
		}
	}
	return e
}

func advance(bodies []Body, dt float64) {
	for i := range bodies {
		bi := &bodies[i]
		for j := i + 1; j < len(bodies); j++ {
			bj := &bodies[j]
			dx := bi.x - bj.x
			dy := bi.y - bj.y
			dz := bi.z - bj.z
			d2 := float64(float64(dx*dx)+float64(dy*dy)) + float64(dz*dz)
			mag := dt / (d2 * math.Sqrt(d2))
			mi := bi.mass * mag
			mj := bj.mass * mag
			bi.vx = bi.vx - float64(dx*mj)
			bi.vy = bi.vy - float64(dy*mj)
			bi.vz = bi.vz - float64(dz*mj)
			bj.vx = bj.vx + float64(dx*mi)
			bj.vy = bj.vy + float64(dy*mi)
			bj.vz = bj.vz + float64(dz*mi)
		}
	}
	for i := range bodies {
		b := &bodies[i]
		b.x = b.x + float64(dt*b.vx)
		b.y = b.y + float64(dt*b.vy)
		b.z = b.z + float64(dt*b.vz)
	}
}

func kernel(steps int64) string {
	bodies := solarSystem()
	offsetMomentum(bodies)
	before := energy(bodies)
	for s := int64(0); s < steps; s++ {
		advance(bodies, 0.01)
	}
	after := energy(bodies)
	return fmt.Sprintf("%d %d", int64(before*1000000000.0), int64(after*1000000000.0))
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
