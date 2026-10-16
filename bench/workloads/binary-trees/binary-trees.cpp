// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=91538bffa3c258287dc87f0489ac70949fcf2657e453985efeab7ddfc2433cfd

// binary-trees: allocate and walk many complete binary trees (see binary-trees.mi).

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <memory>
#include <string>

namespace {

struct Node {
    std::unique_ptr<Node> left;
    std::unique_ptr<Node> right;
};

std::unique_ptr<Node> bottom_up(std::int64_t depth) {
    auto node = std::make_unique<Node>();
    if (depth > 0) {
        node->left = bottom_up(depth - 1);
        node->right = bottom_up(depth - 1);
    }
    return node;
}

std::int64_t check(const Node &node) {
    if (!node.left || !node.right) {
        return 1;
    }
    return 1 + check(*node.left) + check(*node.right);
}

std::string kernel(std::int64_t max_depth) {
    const std::int64_t stretch = check(*bottom_up(max_depth + 1));
    const auto long_lived = bottom_up(max_depth);
    std::int64_t total = 0;
    for (std::int64_t depth = 4; depth <= max_depth; depth += 2) {
        const std::int64_t iterations = std::int64_t{1} << (max_depth - depth + 4);
        for (std::int64_t i = 0; i < iterations; i++) {
            total += check(*bottom_up(depth));
        }
    }
    return std::to_string(stretch) + " " + std::to_string(total) + " " + std::to_string(check(*long_lived));
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
