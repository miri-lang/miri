// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=8be1403a24161be43ad4a1a821848a89d84d30232130490fd923bcdf87367d1d

// k-nucleotide: count the substrings of a DNA sequence (see k-nucleotide.mi).

#include <algorithm>
#include <cctype>
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

constexpr std::string_view kMotif = "GGTATTTTAATTTATAGT";

std::unordered_map<std::string_view, std::int64_t> frequencies(std::string_view sequence, std::size_t k) {
    std::unordered_map<std::string_view, std::int64_t> counts;
    for (std::size_t i = 0; i + k <= sequence.size(); i++) {
        counts[sequence.substr(i, k)] += 1;
    }
    return counts;
}

std::string listing(std::string_view sequence, std::size_t k) {
    const auto counts = frequencies(sequence, k);
    std::vector<std::string_view> keys;
    keys.reserve(counts.size());
    for (const auto &entry : counts) {
        keys.push_back(entry.first);
    }
    std::sort(keys.begin(), keys.end());
    std::string out;
    for (std::size_t i = 0; i < keys.size(); i++) {
        if (i > 0) {
            out += " ";
        }
        out += std::string(keys[i]) + "=" + std::to_string(counts.at(keys[i]));
    }
    return out;
}

std::string kernel(const std::string &text) {
    std::string_view sequence(text);
    const auto is_space = [](char c) { return std::isspace(static_cast<unsigned char>(c)) != 0; };
    while (!sequence.empty() && is_space(sequence.front())) {
        sequence.remove_prefix(1);
    }
    while (!sequence.empty() && is_space(sequence.back())) {
        sequence.remove_suffix(1);
    }
    std::string out = listing(sequence, 1) + " " + listing(sequence, 2);
    for (const std::size_t length : {3, 4, 6, 12, 18}) {
        const auto prefix = kMotif.substr(0, length);
        const auto counts = frequencies(sequence, length);
        const auto found = counts.find(prefix);
        out += " " + std::string(prefix) + "=" + std::to_string(found == counts.end() ? 0 : found->second);
    }
    return out;
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
