// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=cebd6ba8069e9f5bb43006fc0c2e98c5b127218ba6176bb0942b5a72adb65a25

// mandelbrot: the Mandelbrot set packed as a bitmap (see mandelbrot.mi).

// The other ports never fuse a multiply and an add into one rounding, so
// neither may this one: every language computes the same bits.
#pragma STDC FP_CONTRACT OFF

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>

namespace {

bool stays_bounded(double cr, double ci) {
    double zr = 0.0, zi = 0.0;
    for (int k = 0; k < 50; k++) {
        const double tr = zr * zr - zi * zi + cr;
        zi = 2.0 * zr * zi + ci;
        zr = tr;
        if (zr * zr + zi * zi > 4.0) {
            return false;
        }
    }
    return true;
}

std::string kernel(std::int64_t n) {
    std::int64_t hash = 0, inside = 0;
    for (std::int64_t y = 0; y < n; y++) {
        const double ci = 2.0 * static_cast<double>(y) / static_cast<double>(n) - 1.0;
        std::int64_t byte = 0;
        int bits = 0;
        for (std::int64_t x = 0; x < n; x++) {
            const double cr = 2.0 * static_cast<double>(x) / static_cast<double>(n) - 1.5;
            byte = byte * 2;
            if (stays_bounded(cr, ci)) {
                byte += 1;
                inside += 1;
            }
            bits += 1;
            if (bits == 8) {
                hash = (hash * 31 + byte) % 1000000007;
                byte = 0;
                bits = 0;
            }
        }
        if (bits > 0) {
            byte <<= 8 - bits;
            hash = (hash * 31 + byte) % 1000000007;
        }
    }
    return std::to_string(hash) + " " + std::to_string(inside);
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
