// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=b52a17f25b95536f5d8da307a35b702147d78b5642b68d3ad713a1bd5ab16ad7

// list-sort: sort generated numbers, then records by key (see list-sort.mi).

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

namespace {

struct Record {
    std::int64_t key;
    std::int64_t id;
};

std::string kernel(std::int64_t n) {
    std::vector<std::int64_t> numbers;
    std::vector<Record> records;
    numbers.reserve(n);
    records.reserve(n);
    std::int64_t state = 7;
    for (std::int64_t i = 0; i < n; i++) {
        state = state * 48271 % 2147483647;
        numbers.push_back(state % 1000000);
        records.push_back(Record{state % 1000, i});
    }
    std::sort(numbers.begin(), numbers.end());
    std::vector<Record> by_key = records;
    std::stable_sort(by_key.begin(), by_key.end(), [](const Record &a, const Record &b) {
        return a.key != b.key ? a.key < b.key : a.id > b.id;
    });
    std::int64_t number_hash = 0, record_hash = 0;
    for (std::int64_t i = 0; i < n; i++) {
        number_hash = (number_hash * 31 + numbers[i]) % 1000000007;
        record_hash = (record_hash * 31 + by_key[i].id) % 1000000007;
    }
    return std::to_string(number_hash) + " " + std::to_string(record_hash);
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
