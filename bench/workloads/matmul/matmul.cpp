// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=7128321e8284dbea959401fa00acdba33c134bb6c947fa56063285ebfd9ff82b

// matmul: the naive triple-loop matrix product (see matmul.mi).

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

namespace {

std::vector<std::vector<double>> matrix(std::int64_t n, std::int64_t offset) {
    std::vector<std::vector<double>> rows(n, std::vector<double>(n));
    for (std::int64_t i = 0; i < n; i++) {
        for (std::int64_t j = 0; j < n; j++) {
            rows[i][j] = static_cast<double>((i * j + offset * (i + j)) % 10);
        }
    }
    return rows;
}

std::string kernel(std::int64_t n) {
    const auto a = matrix(n, 1);
    const auto b = matrix(n, 2);
    double total = 0.0;
    for (const auto &row : a) {
        for (std::int64_t j = 0; j < n; j++) {
            double sum = 0.0;
            for (std::int64_t k = 0; k < n; k++) {
                sum = sum + row[k] * b[k][j];
            }
            total = total + sum;
        }
    }
    return std::to_string(static_cast<std::int64_t>(total));
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
