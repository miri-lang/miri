// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=ee894f803ad6283f37d7eb6b6cfa4f1f0de2e96e474bfc8aae1268f472195335

// matmul: the naive triple-loop matrix product (see matmul.mi).

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define CHECKSUM_LEN 64

static double **matrix(long n, long offset) {
    double **rows = malloc(sizeof(double *) * n);
    for (long i = 0; i < n; i++) {
        rows[i] = malloc(sizeof(double) * n);
        for (long j = 0; j < n; j++) {
            rows[i][j] = (double)((i * j + offset * (i + j)) % 10);
        }
    }
    return rows;
}

static void free_matrix(double **rows, long n) {
    for (long i = 0; i < n; i++) {
        free(rows[i]);
    }
    free(rows);
}

static void kernel(long n, char *checksum) {
    double **a = matrix(n, 1);
    double **b = matrix(n, 2);
    double total = 0.0;
    for (long i = 0; i < n; i++) {
        const double *row = a[i];
        for (long j = 0; j < n; j++) {
            double sum = 0.0;
            for (long k = 0; k < n; k++) {
                sum = sum + row[k] * b[k][j];
            }
            total = total + sum;
        }
    }
    snprintf(checksum, CHECKSUM_LEN, "%lld", (long long)total);
    free_matrix(a, n);
    free_matrix(b, n);
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
