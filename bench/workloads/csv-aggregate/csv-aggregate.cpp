// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=d5124a42b270512504a698cf8dc774b16d3f89cd0c121130e421530cce2e695c

// csv-aggregate: totals by key over a generated CSV file (see csv-aggregate.mi).

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <iterator>
#include <algorithm>
#include <unordered_map>
#include <string>
#include <string_view>
#include <vector>

namespace {

struct Totals {
    std::int64_t count = 0;
    std::int64_t quantity = 0;
    double amount = 0.0;
};

std::vector<std::string_view> split(std::string_view text, char separator) {
    std::vector<std::string_view> parts;
    std::size_t start = 0;
    for (std::size_t i = 0; i <= text.size(); i++) {
        if (i == text.size() || text[i] == separator) {
            parts.push_back(text.substr(start, i - start));
            start = i + 1;
        }
    }
    return parts;
}

std::string kernel(const std::string &text) {
    std::unordered_map<std::string_view, Totals> groups;
    const auto lines = split(text, '\n');
    for (std::size_t i = 1; i < lines.size(); i++) {
        if (lines[i].empty()) {
            continue;
        }
        const auto fields = split(lines[i], ',');
        Totals &t = groups[fields[0]];
        t.count += 1;
        t.quantity += std::stoll(std::string(fields[1]));
        t.amount = t.amount + std::stod(std::string(fields[2]));
    }
    std::vector<std::string_view> keys;
    keys.reserve(groups.size());
    for (const auto &entry : groups) {
        keys.push_back(entry.first);
    }
    std::sort(keys.begin(), keys.end());
    std::string out;
    for (const auto key : keys) {
        const Totals &t = groups.at(key);
        if (!out.empty()) {
            out += ",";
        }
        out += std::string(key) + ":" + std::to_string(t.count) + ":" + std::to_string(t.quantity) + ":" +
               std::to_string(static_cast<std::int64_t>(t.amount * 100.0));
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
