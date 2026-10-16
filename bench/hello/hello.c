// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// The runtime floor: what a program costs before it does anything, measured
// with the same build and run lines as the workloads.

#include <stdio.h>

int main(void) {
    puts("hello");
    return 0;
}
