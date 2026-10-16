// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=a91b645476a3c58bb4c3e804d20470a0fbb8228082e8666a33da280e413862b5

// fannkuch-redux: pancake flips over every permutation (see fannkuch-redux.mi).

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define CHECKSUM_LEN 64

static int count_flips(const int *perm, int *p, long n) {
    memcpy(p, perm, sizeof(int) * n);
    int flips = 0;
    int first = p[0];
    while (first != 0) {
        for (int lo = 0, hi = first; lo < hi; lo++, hi--) {
            int t = p[lo];
            p[lo] = p[hi];
            p[hi] = t;
        }
        flips += 1;
        first = p[0];
    }
    return flips;
}

static void kernel(long n, char *checksum) {
    int *perm = malloc(sizeof(int) * n);
    int *count = calloc(n, sizeof(int));
    int *scratch = malloc(sizeof(int) * n);
    for (long i = 0; i < n; i++) {
        perm[i] = (int)i;
    }
    long long sum = 0;
    int max_flips = 0;
    long long index = 0;
    for (;;) {
        int flips = count_flips(perm, scratch, n);
        if (flips > max_flips) {
            max_flips = flips;
        }
        sum += index % 2 == 0 ? flips : -flips;
        index += 1;
        long r = 1;
        for (; r < n; r++) {
            int first = perm[0];
            for (long k = 0; k < r; k++) {
                perm[k] = perm[k + 1];
            }
            perm[r] = first;
            count[r] += 1;
            if (count[r] <= r) {
                break;
            }
            count[r] = 0;
        }
        if (r >= n) {
            break;
        }
    }
    snprintf(checksum, CHECKSUM_LEN, "%lld %d", sum, max_flips);
    free(perm);
    free(count);
    free(scratch);
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
