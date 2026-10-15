// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=7ebc581b67e8e53457be3074c1d54435a16b78218d5b1169fa4ea170711c2274

// spectral-norm: power iteration on an infinite matrix (see spectral-norm.mi).

// The other ports never fuse a multiply and an add into one rounding, so
// neither may this one: every language computes the same bits.
#pragma STDC FP_CONTRACT OFF

#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

namespace {

double entry(std::int64_t i, std::int64_t j) {
    return 1.0 / static_cast<double>((i + j) * (i + j + 1) / 2 + i + 1);
}

std::vector<double> times(const std::vector<double> &v) {
    const auto n = static_cast<std::int64_t>(v.size());
    std::vector<double> out;
    out.reserve(v.size());
    for (std::int64_t i = 0; i < n; i++) {
        double sum = 0.0;
        for (std::int64_t j = 0; j < n; j++) {
            sum = sum + entry(i, j) * v[j];
        }
        out.push_back(sum);
    }
    return out;
}

std::vector<double> times_transposed(const std::vector<double> &v) {
    const auto n = static_cast<std::int64_t>(v.size());
    std::vector<double> out;
    out.reserve(v.size());
    for (std::int64_t i = 0; i < n; i++) {
        double sum = 0.0;
        for (std::int64_t j = 0; j < n; j++) {
            sum = sum + entry(j, i) * v[j];
        }
        out.push_back(sum);
    }
    return out;
}

std::vector<double> times_both(const std::vector<double> &v) { return times_transposed(times(v)); }

std::string kernel(std::int64_t n) {
    std::vector<double> u(n, 1.0);
    std::vector<double> v;
    for (int round = 0; round < 10; round++) {
        v = times_both(u);
        u = times_both(v);
    }
    double vbv = 0.0, vv = 0.0;
    for (std::int64_t i = 0; i < n; i++) {
        vbv = vbv + u[i] * v[i];
        vv = vv + v[i] * v[i];
    }
    return std::to_string(static_cast<std::int64_t>(std::sqrt(vbv / vv) * 1000000000.0));
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
