// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=74892d27a5a1da60063f5a24718f892f21aef2204150ff93284f6bacc5ea7411

// generics: one generic stack at three element types (see generics.mi).
// C has no generics: a macro writes the stack once per element type, which
// is how C monomorphizes.

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define CHECKSUM_LEN 96

typedef struct {
    int64_t first;
    double second;
} Pair;

#define DEFINE_STACK(Name, T)                                                   \
    typedef struct {                                                            \
        T *items;                                                               \
        size_t len;                                                             \
        size_t cap;                                                             \
    } Name;                                                                     \
    static void Name##_push(Name *s, T item) {                                  \
        if (s->len == s->cap) {                                                 \
            s->cap = s->cap == 0 ? 16 : s->cap * 2;                             \
            s->items = realloc(s->items, sizeof(T) * s->cap);                   \
            if (s->items == NULL) {                                             \
                abort();                                                        \
            }                                                                   \
        }                                                                       \
        s->items[s->len++] = item;                                              \
    }                                                                           \
    static T Name##_pop(Name *s) { return s->items[--s->len]; }                 \
    static size_t Name##_size(const Name *s) { return s->len; }

DEFINE_STACK(IntStack, int64_t)
DEFINE_STACK(FloatStack, double)
DEFINE_STACK(PairStack, Pair)

static void kernel(long n, char *checksum) {
    IntStack ints = {0};
    FloatStack floats = {0};
    PairStack pairs = {0};
    for (long i = 0; i < n; i++) {
        IntStack_push(&ints, i % 1000);
        FloatStack_push(&floats, (double)(i % 100) * 0.5);
        PairStack_push(&pairs, (Pair){i % 10, (double)(i % 4)});
    }
    int64_t a = 0;
    while (IntStack_size(&ints) > 0) {
        a = (a * 31 + IntStack_pop(&ints)) % 1000000007;
    }
    double b = 0.0;
    while (FloatStack_size(&floats) > 0) {
        b = b + FloatStack_pop(&floats);
    }
    int64_t c = 0;
    while (PairStack_size(&pairs) > 0) {
        Pair p = PairStack_pop(&pairs);
        c = (c * 7 + p.first + (int64_t)p.second) % 1000000007;
    }
    snprintf(checksum, CHECKSUM_LEN, "%lld %lld %lld", (long long)a, (long long)b, (long long)c);
    free(ints.items);
    free(floats.items);
    free(pairs.items);
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
