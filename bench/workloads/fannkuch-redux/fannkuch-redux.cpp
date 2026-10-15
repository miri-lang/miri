// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=e4a5e88a9bf8c777ce30bc26d7690c7303497124ab183bb0ac2e4aa8cc2c61c6

// fannkuch-redux: pancake flips over every permutation (see fannkuch-redux.mi).

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <numeric>
#include <string>
#include <vector>

namespace {

std::int64_t count_flips(const std::vector<int> &perm, std::vector<int> &p) {
    p = perm;
    std::int64_t flips = 0;
    int first = p[0];
    while (first != 0) {
        std::reverse(p.begin(), p.begin() + first + 1);
        flips += 1;
        first = p[0];
    }
    return flips;
}

std::string kernel(std::int64_t n) {
    std::vector<int> perm(n);
    std::iota(perm.begin(), perm.end(), 0);
    std::vector<std::int64_t> count(n, 0);
    std::vector<int> scratch(n);
    std::int64_t checksum = 0, max_flips = 0, index = 0;
    for (;;) {
        const std::int64_t flips = count_flips(perm, scratch);
        max_flips = std::max(max_flips, flips);
        checksum += index % 2 == 0 ? flips : -flips;
        index += 1;
        std::int64_t r = 1;
        for (; r < n; r++) {
            std::rotate(perm.begin(), perm.begin() + 1, perm.begin() + r + 1);
            count[r] += 1;
            if (count[r] <= r) {
                break;
            }
            count[r] = 0;
        }
        if (r >= n) {
            return std::to_string(checksum) + " " + std::to_string(max_flips);
        }
    }
}

std::string run_hot(std::int64_t n) {
    const std::string expected = kernel(n);
    bool agree = true;
    for (int i = 1; i < 5; i++) {
        const std::string checksum = kernel(n);
        agree = agree && checksum == expected;
    }
    for (int i = 0; i < 5; i++) {
        const auto start = std::chrono::steady_clock::now();
        const std::string checksum = kernel(n);
        const auto elapsed = std::chrono::steady_clock::now() - start;
        std::cerr << "hot_ns=" << std::chrono::duration_cast<std::chrono::nanoseconds>(elapsed).count() << "\n";
        agree = agree && checksum == expected;
    }
    if (!agree) {
        std::cerr << "checksum differs between iterations\n";
        std::exit(1);
    }
    return expected;
}

}  // namespace

int main(int argc, char **argv) {
    std::ifstream input(argv[1]);
    std::int64_t n = 0;
    if (!(input >> n)) {
        return 2;
    }
    const bool hot = argc > 2 && std::string(argv[2]) == "hot";
    std::cout << (hot ? run_hot(n) : kernel(n)) << "\n";
    return 0;
}
