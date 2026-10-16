// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=ad8c13627cd31626bab999f87ed0a85c7b2a61d9a77d8a7fc3c456225e039fad

// enum-vm: a twelve-instruction register machine (see enum-vm.mi).

#include <array>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>
#include <variant>
#include <vector>

namespace {

struct Const { std::size_t d; std::int64_t v; };
struct Mov { std::size_t d, a; };
struct Add { std::size_t d, a, b; };
struct Sub { std::size_t d, a, b; };
struct Mul { std::size_t d, a, b; };
struct Mod { std::size_t d, a, b; };
struct And { std::size_t d, a, b; };
struct AddImm { std::size_t d; std::int64_t v; };
struct JumpIfLess { std::size_t a, b, target; };
struct JumpIfEqual { std::size_t a, b, target; };
struct Jump { std::size_t target; };
struct Halt {};

using Instr = std::variant<Const, Mov, Add, Sub, Mul, Mod, And, AddImm, JumpIfLess, JumpIfEqual, Jump, Halt>;

template <class... Fs> struct Overloaded : Fs... {
    using Fs::operator()...;
};

std::vector<Instr> program(std::int64_t n) {
    return {
        Const{0, 0}, Const{1, 0}, Const{2, n}, Const{5, 97}, Const{6, 63}, Const{4, 1000000007}, Const{8, 0},
        Const{9, 3}, JumpIfLess{0, 2, 10}, Halt{}, Mul{3, 0, 0}, Mod{3, 3, 5}, And{3, 3, 6}, Add{1, 1, 3},
        And{7, 0, 6}, JumpIfEqual{7, 8, 18}, Sub{10, 2, 0}, Jump{19}, Mov{10, 9}, Mod{10, 10, 9}, Add{1, 1, 10},
        Mod{1, 1, 4}, AddImm{0, 1}, Jump{8},
    };
}

std::string kernel(std::int64_t n) {
    const auto code = program(n);
    std::array<std::int64_t, 16> regs{};
    std::size_t pc = 0;
    std::int64_t steps = 0;
    bool running = true;
    while (running) {
        std::size_t next = pc + 1;
        std::visit(Overloaded{
                       [&](const Const &i) { regs[i.d] = i.v; },
                       [&](const Mov &i) { regs[i.d] = regs[i.a]; },
                       [&](const Add &i) { regs[i.d] = regs[i.a] + regs[i.b]; },
                       [&](const Sub &i) { regs[i.d] = regs[i.a] - regs[i.b]; },
                       [&](const Mul &i) { regs[i.d] = regs[i.a] * regs[i.b]; },
                       [&](const Mod &i) { regs[i.d] = regs[i.a] % regs[i.b]; },
                       [&](const And &i) { regs[i.d] = regs[i.a] & regs[i.b]; },
                       [&](const AddImm &i) { regs[i.d] = regs[i.d] + i.v; },
                       [&](const JumpIfLess &i) { next = regs[i.a] < regs[i.b] ? i.target : next; },
                       [&](const JumpIfEqual &i) { next = regs[i.a] == regs[i.b] ? i.target : next; },
                       [&](const Jump &i) { next = i.target; },
                       [&](const Halt &) { running = false; },
                   },
                   code[pc]);
        pc = next;
        steps += 1;
    }
    return std::to_string(regs[1]) + " " + std::to_string(steps);
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
