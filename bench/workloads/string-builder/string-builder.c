// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=b98f969659aa30391c26a73612ef0a32dfad66aff075e470f4b2fc8e2b23755a

// string-builder: build a long string by appending (see string-builder.mi).

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "../../c/table.h"

#define CHECKSUM_LEN 64

static void kernel(long n, char *checksum) {
    Buffer text = {0};
    char piece[48];
    for (long i = 0; i < n; i++) {
        int len = snprintf(piece, sizeof piece, "%ld:%ld;", i, i * i % 1000);
        buffer_append(&text, piece, (size_t)len);
    }
    int64_t hash = 0;
    for (size_t k = 0; k < text.len; k++) {
        hash = (hash * 31 + (unsigned char)text.data[k]) % 1000000007;
    }
    snprintf(checksum, CHECKSUM_LEN, "%zu %lld", text.len, (long long)hash);
    buffer_free(&text);
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
