// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=a9c8e9babbba7ba28d0ba2b5c92c1a64e24826fa3339049595ca14b964f6c1a4

// wordfreq: count the words of a text (see wordfreq.mi).

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "../../c/table.h"

#define CHECKSUM_LEN 1024

static int by_count_then_word(const void *a, const void *b) {
    const Slot *x = *(const Slot *const *)a, *y = *(const Slot *const *)b;
    if (x->count != y->count) {
        return (x->count < y->count) - (x->count > y->count);
    }
    size_t shorter = x->len < y->len ? x->len : y->len;
    int order = memcmp(x->key, y->key, shorter);
    return order != 0 ? order : (x->len > y->len) - (x->len < y->len);
}

static void kernel(const char *text, char *checksum) {
    Table counts;
    table_init(&counts);
    const char *start = text;
    for (const char *p = text;; p++) {
        if (*p == ' ' || *p == '\n' || *p == '\0') {
            if (p > start) {
                *table_slot(&counts, start, (size_t)(p - start)) += 1;
            }
            start = p + 1;
            if (*p == '\0') {
                break;
            }
        }
    }
    Slot **ranked = table_slots(&counts);
    qsort(ranked, counts.size, sizeof(Slot *), by_count_then_word);
    int used = snprintf(checksum, CHECKSUM_LEN, "%zu ", counts.size);
    for (size_t i = 0; i < 10 && i < counts.size; i++) {
        used += snprintf(checksum + used, CHECKSUM_LEN - used, "%s%.*s=%lld", i > 0 ? "," : "",
                         (int)ranked[i]->len, ranked[i]->key, (long long)ranked[i]->count);
    }
    free(ranked);
    table_free(&counts);
}

static int64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

static void run_hot(const char * n, char *expected) {
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
    size_t len = 0;
    char *n = read_file(argv[1], &len);
    if (n == NULL) {
        return 2;
    }
    char checksum[CHECKSUM_LEN];
    if (argc > 2 && strcmp(argv[2], "hot") == 0) {
        run_hot(n, checksum);
    } else {
        kernel(n, checksum);
    }
    printf("%s\n", checksum);
    free(n);
    return 0;
}
