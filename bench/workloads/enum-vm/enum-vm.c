// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=a573c9e02dc0a2b1d0aeb99a29e29bbee70e06af480fe65d5519ea2b3649e15a

// enum-vm: a twelve-instruction register machine (see enum-vm.mi).
// C has no sum types: an instruction is a tag and its operands, switched on.

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define CHECKSUM_LEN 64

typedef enum { CONST, MOV, ADD, SUB, MUL, MOD, AND, ADD_IMM, JUMP_IF_LESS, JUMP_IF_EQUAL, JUMP, HALT } Op;

typedef struct {
    Op op;
    int64_t a, b, c;
} Instr;

static void kernel(long n, char *checksum) {
    const Instr code[] = {
        {CONST, 0, 0, 0}, {CONST, 1, 0, 0}, {CONST, 2, n, 0}, {CONST, 5, 97, 0}, {CONST, 6, 63, 0},
        {CONST, 4, 1000000007, 0}, {CONST, 8, 0, 0}, {CONST, 9, 3, 0}, {JUMP_IF_LESS, 0, 2, 10},
        {HALT, 0, 0, 0}, {MUL, 3, 0, 0}, {MOD, 3, 3, 5}, {AND, 3, 3, 6}, {ADD, 1, 1, 3}, {AND, 7, 0, 6},
        {JUMP_IF_EQUAL, 7, 8, 18}, {SUB, 10, 2, 0}, {JUMP, 19, 0, 0}, {MOV, 10, 9, 0}, {MOD, 10, 10, 9},
        {ADD, 1, 1, 10}, {MOD, 1, 1, 4}, {ADD_IMM, 0, 1, 0}, {JUMP, 8, 0, 0},
    };
    int64_t regs[16] = {0};
    int64_t pc = 0, steps = 0;
    int running = 1;
    while (running) {
        const Instr *i = &code[pc];
        int64_t next = pc + 1;
        switch (i->op) {
        case CONST: regs[i->a] = i->b; break;
        case MOV: regs[i->a] = regs[i->b]; break;
        case ADD: regs[i->a] = regs[i->b] + regs[i->c]; break;
        case SUB: regs[i->a] = regs[i->b] - regs[i->c]; break;
        case MUL: regs[i->a] = regs[i->b] * regs[i->c]; break;
        case MOD: regs[i->a] = regs[i->b] % regs[i->c]; break;
        case AND: regs[i->a] = regs[i->b] & regs[i->c]; break;
        case ADD_IMM: regs[i->a] = regs[i->a] + i->b; break;
        case JUMP_IF_LESS: next = regs[i->a] < regs[i->b] ? i->c : next; break;
        case JUMP_IF_EQUAL: next = regs[i->a] == regs[i->b] ? i->c : next; break;
        case JUMP: next = i->a; break;
        case HALT: running = 0; break;
        }
        pc = next;
        steps += 1;
    }
    snprintf(checksum, CHECKSUM_LEN, "%lld %lld", (long long)regs[1], (long long)steps);
}

static int64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

static void run_hot(long n, char *expected) {
    char checksum[CHECKSUM_LEN];
    kernel(n, expected);
    int agree = 1;
    for (int i = 1; i < 5; i++) {
        kernel(n, checksum);
        agree = agree && strcmp(checksum, expected) == 0;
    }
    for (int i = 0; i < 5; i++) {
        int64_t start = now_ns();
        kernel(n, checksum);
        int64_t elapsed = now_ns() - start;
        fprintf(stderr, "hot_ns=%lld\n", (long long)elapsed);
        agree = agree && strcmp(checksum, expected) == 0;
    }
    if (!agree) {
        fprintf(stderr, "checksum differs between iterations\n");
        exit(1);
    }
}

int main(int argc, char **argv) {
    FILE *f = fopen(argv[1], "r");
    long n = 0;
    if (f == NULL || fscanf(f, "%ld", &n) != 1) {
        return 2;
    }
    fclose(f);
    char checksum[CHECKSUM_LEN];
    if (argc > 2 && strcmp(argv[2], "hot") == 0) {
        run_hot(n, checksum);
    } else {
        kernel(n, checksum);
    }
    printf("%s\n", checksum);
    return 0;
}
