// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=abd817e1a115418709c6b92212c57f4a92a758027ce3b3d1bf394d2905900626

// spectral-norm: power iteration on an infinite matrix (see spectral-norm.mi).

// The other ports never fuse a multiply and an add into one rounding, so
// neither may this one: every language computes the same bits.
#pragma STDC FP_CONTRACT OFF

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <math.h>

#define CHECKSUM_LEN 64

static double entry(long i, long j) { return 1.0 / (double)((i + j) * (i + j + 1) / 2 + i + 1); }

static void times(const double *v, double *out, long n) {
    for (long i = 0; i < n; i++) {
        double sum = 0.0;
        for (long j = 0; j < n; j++) {
            sum = sum + entry(i, j) * v[j];
        }
        out[i] = sum;
    }
}

static void times_transposed(const double *v, double *out, long n) {
    for (long i = 0; i < n; i++) {
        double sum = 0.0;
        for (long j = 0; j < n; j++) {
            sum = sum + entry(j, i) * v[j];
        }
        out[i] = sum;
    }
}

static void times_both(const double *v, double *out, double *scratch, long n) {
    times(v, scratch, n);
    times_transposed(scratch, out, n);
}

static void kernel(long n, char *checksum) {
    double *u = malloc(sizeof(double) * n);
    double *v = malloc(sizeof(double) * n);
    double *scratch = malloc(sizeof(double) * n);
    for (long i = 0; i < n; i++) {
        u[i] = 1.0;
    }
    for (int round = 0; round < 10; round++) {
        times_both(u, v, scratch, n);
        times_both(v, u, scratch, n);
    }
    double vbv = 0.0, vv = 0.0;
    for (long i = 0; i < n; i++) {
        vbv = vbv + u[i] * v[i];
        vv = vv + v[i] * v[i];
    }
    snprintf(checksum, CHECKSUM_LEN, "%lld", (long long)(sqrt(vbv / vv) * 1000000000.0));
    free(u);
    free(v);
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
