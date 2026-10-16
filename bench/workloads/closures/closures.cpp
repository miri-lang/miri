// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=eae790111e22c2227479c6f85a802285060490df7aadee4adb1ea38a1bc40f85

// closures: capturing map, filter and fold pipelines (see closures.mi).

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <iterator>
#include <numeric>
#include <string>
#include <vector>

namespace {

std::string kernel(std::int64_t n) {
    std::vector<std::int64_t> xs(n);
    std::iota(xs.begin(), xs.end(), 0);
    std::int64_t total = 0;
    for (std::int64_t k = 1; k <= 8; k++) {
        const std::int64_t scale = k * 7;
        const std::int64_t threshold = 100 * k;
        std::vector<std::int64_t> scaled(xs.size());
        std::transform(xs.begin(), xs.end(), scaled.begin(), [scale](std::int64_t x) { return (x * scale) % 1000; });
        std::vector<std::int64_t> kept;
        std::copy_if(scaled.begin(), scaled.end(), std::back_inserter(kept), [threshold](std::int64_t x) { return x > threshold; });
        total = (total + std::accumulate(kept.begin(), kept.end(), std::int64_t{0},
                                         [](std::int64_t a, std::int64_t b) { return (a + b) % 1000000007; })) %
                1000000007;
    }
    return std::to_string(total);
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
