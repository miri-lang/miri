// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=1b909bbef2a7592a168c981da25959883ceaa34dc9822cb00dd776908ae612bb

// json-roundtrip: parse, walk and rewrite a JSON document (see json-roundtrip.mi).

package main

import (
	"encoding/json"
	"fmt"
	"os"
	"time"
)

func walk(value any) (int64, int64) {
	var nodes, sum int64 = 1, 0
	switch v := value.(type) {
	case map[string]any:
		for _, item := range v {
			n, s := walk(item)
			nodes, sum = nodes+n, sum+s
		}
	case []any:
		for _, item := range v {
			n, s := walk(item)
			nodes, sum = nodes+n, sum+s
		}
	case float64:
		sum = int64(v)
	}
	return nodes, sum
}

func kernel(text string) string {
	var document any
	if err := json.Unmarshal([]byte(text), &document); err != nil {
		return "invalid"
	}
	nodes, sum := walk(document)
	written, err := json.Marshal(document)
	if err != nil {
		return "invalid"
	}
	return fmt.Sprintf("%d %d %d", nodes, sum, len(written))
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
