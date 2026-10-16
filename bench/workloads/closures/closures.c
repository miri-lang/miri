// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=86e9dfbcf6b705018f1e16d1e363796ffa3c822471d95d31fd91c3dd30a681bc

// closures: capturing map, filter and fold pipelines (see closures.mi).
// C has no closures: each stage is a function taking the captured values in a
// context struct, which is how C writes one.

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define CHECKSUM_LEN 64

typedef struct {
    int64_t scale;
    int64_t threshold;
} Captures;

static int64_t scaled(int64_t x, const Captures *c) { return (x * c->scale) % 1000; }
static int kept(int64_t x, const Captures *c) { return x > c->threshold; }
static int64_t summed(int64_t a, int64_t b) { return (a + b) % 1000000007; }

static size_t map(const int64_t *in, size_t n, int64_t *out, int64_t (*f)(int64_t, const Captures *), const Captures *c) {
    for (size_t i = 0; i < n; i++) {
        out[i] = f(in[i], c);
    }
    return n;
}

static size_t filter(const int64_t *in, size_t n, int64_t *out, int (*f)(int64_t, const Captures *), const Captures *c) {
    size_t m = 0;
    for (size_t i = 0; i < n; i++) {
        if (f(in[i], c)) {
            out[m++] = in[i];
        }
    }
    return m;
}

static int64_t reduce(const int64_t *in, size_t n, int64_t init, int64_t (*f)(int64_t, int64_t)) {
    int64_t acc = init;
    for (size_t i = 0; i < n; i++) {
        acc = f(acc, in[i]);
    }
    return acc;
}

static void kernel(long n, char *checksum) {
    int64_t *xs = malloc(sizeof(int64_t) * (size_t)n);
    int64_t *mapped = malloc(sizeof(int64_t) * (size_t)n);
    int64_t *filtered = malloc(sizeof(int64_t) * (size_t)n);
    for (long i = 0; i < n; i++) {
        xs[i] = i;
    }
    int64_t total = 0;
    for (int64_t k = 1; k <= 8; k++) {
        const Captures c = {k * 7, 100 * k};
        size_t m = map(xs, (size_t)n, mapped, scaled, &c);
        size_t f = filter(mapped, m, filtered, kept, &c);
        total = (total + reduce(filtered, f, 0, summed)) % 1000000007;
    }
    snprintf(checksum, CHECKSUM_LEN, "%lld", (long long)total);
    free(xs);
    free(mapped);
    free(filtered);
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
