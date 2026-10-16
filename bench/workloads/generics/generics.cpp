// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=d1e2192aea208ad0aedb9b5c40594b34338d71b6d4c6bbc0eeb28b896bd0b235

// generics: one generic stack at three element types (see generics.mi).

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>
#include <utility>
#include <vector>

namespace {

template <typename T> class Stack {
  public:
    void push(T item) { items_.push_back(std::move(item)); }
    T pop() {
        T item = std::move(items_.back());
        items_.pop_back();
        return item;
    }
    std::size_t size() const { return items_.size(); }

  private:
    std::vector<T> items_;
};

template <typename A, typename B> struct Pair {
    A first;
    B second;
};

std::string kernel(std::int64_t n) {
    Stack<std::int64_t> ints;
    Stack<double> floats;
    Stack<Pair<std::int64_t, double>> pairs;
    for (std::int64_t i = 0; i < n; i++) {
        ints.push(i % 1000);
        floats.push(static_cast<double>(i % 100) * 0.5);
        pairs.push({i % 10, static_cast<double>(i % 4)});
    }
    std::int64_t a = 0;
    while (ints.size() > 0) {
        a = (a * 31 + ints.pop()) % 1000000007;
    }
    double b = 0.0;
    while (floats.size() > 0) {
        b = b + floats.pop();
    }
    std::int64_t c = 0;
    while (pairs.size() > 0) {
        const auto p = pairs.pop();
        c = (c * 7 + p.first + static_cast<std::int64_t>(p.second)) % 1000000007;
    }
    return std::to_string(a) + " " + std::to_string(static_cast<std::int64_t>(b)) + " " + std::to_string(c);
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
