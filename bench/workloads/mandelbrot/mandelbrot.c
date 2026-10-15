// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=5ae60db49bf7ac4bb66b36471252ddb0fc2c49108c0d70c4735ce49e537e8886

// mandelbrot: the Mandelbrot set packed as a bitmap (see mandelbrot.mi).

// The other ports never fuse a multiply and an add into one rounding, so
// neither may this one: every language computes the same bits.
#pragma STDC FP_CONTRACT OFF

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <stdbool.h>

#define CHECKSUM_LEN 64

static bool stays_bounded(double cr, double ci) {
    double zr = 0.0, zi = 0.0;
    for (int k = 0; k < 50; k++) {
        double tr = zr * zr - zi * zi + cr;
        zi = 2.0 * zr * zi + ci;
        zr = tr;
        if (zr * zr + zi * zi > 4.0) {
            return false;
        }
    }
    return true;
}

static void kernel(long n, char *checksum) {
    long long hash = 0;
    long inside = 0;
    for (long y = 0; y < n; y++) {
        double ci = 2.0 * (double)y / (double)n - 1.0;
        int byte = 0, bits = 0;
        for (long x = 0; x < n; x++) {
            double cr = 2.0 * (double)x / (double)n - 1.5;
            byte = byte * 2;
            if (stays_bounded(cr, ci)) {
                byte += 1;
                inside += 1;
            }
            bits += 1;
            if (bits == 8) {
                hash = (hash * 31 + byte) % 1000000007;
                byte = 0;
                bits = 0;
            }
        }
        if (bits > 0) {
            byte = byte << (8 - bits);
            hash = (hash * 31 + byte) % 1000000007;
        }
    }
    snprintf(checksum, CHECKSUM_LEN, "%lld %ld", hash, inside);
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
