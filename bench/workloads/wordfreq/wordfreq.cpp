// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=311b945de22cc4b20807b2a42accceb39ae9252b2e342415c7195e2c5d85ae8f

// wordfreq: count the words of a text (see wordfreq.mi).

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <iterator>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

namespace {

std::string kernel(const std::string &text) {
    std::unordered_map<std::string_view, std::int64_t> counts;
    const std::string_view all(text);
    std::size_t start = 0;
    for (std::size_t i = 0; i <= all.size(); i++) {
        if (i == all.size() || all[i] == ' ' || all[i] == '\n') {
            if (i > start) {
                counts[all.substr(start, i - start)] += 1;
            }
            start = i + 1;
        }
    }
    std::vector<std::pair<std::string_view, std::int64_t>> ranked(counts.begin(), counts.end());
    std::sort(ranked.begin(), ranked.end(), [](const auto &a, const auto &b) {
        return a.second != b.second ? a.second > b.second : a.first < b.first;
    });
    std::string top;
    for (std::size_t i = 0; i < 10 && i < ranked.size(); i++) {
        if (i > 0) {
            top += ",";
        }
        top += std::string(ranked[i].first) + "=" + std::to_string(ranked[i].second);
    }
    return std::to_string(counts.size()) + " " + top;
}

std::string run_hot(const std::string & n) {
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
    std::ifstream input(argv[1], std::ios::binary);
    if (!input) {
        return 2;
    }
    const std::string n((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
    const bool hot = argc > 2 && std::string(argv[2]) == "hot";
    std::cout << (hot ? run_hot(n) : kernel(n)) << "\n";
    return 0;
}
