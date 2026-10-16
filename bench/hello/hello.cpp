// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// The runtime floor: what a program costs before it does anything, measured
// with the same build and run lines as the workloads.

#include <iostream>

int main() {
    std::cout << "hello\n";
    return 0;
}
