// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=60891a6bc8ef5343f990d6b7433c1a128449d31eaf67c7dc0cabe97caadeccdf

// list-sort: sort generated numbers, then records by key (see list-sort.mi).

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define CHECKSUM_LEN 64

typedef struct {
    int64_t key;
    int64_t id;
} Record;

static int by_value(const void *a, const void *b) {
    int64_t x = *(const int64_t *)a, y = *(const int64_t *)b;
    return (x > y) - (x < y);
}

static int by_key_then_descending_id(const void *a, const void *b) {
    const Record *x = a, *y = b;
    if (x->key != y->key) {
        return (x->key > y->key) - (x->key < y->key);
    }
    return (x->id < y->id) - (x->id > y->id);
}

static void kernel(long n, char *checksum) {
    int64_t *numbers = malloc(sizeof(int64_t) * n);
    Record *records = malloc(sizeof(Record) * n);
    Record *by_key = malloc(sizeof(Record) * n);
    int64_t state = 7;
    for (long i = 0; i < n; i++) {
        state = state * 48271 % 2147483647;
        numbers[i] = state % 1000000;
        records[i] = (Record){state % 1000, i};
    }
    qsort(numbers, n, sizeof(int64_t), by_value);
    memcpy(by_key, records, sizeof(Record) * n);
    qsort(by_key, n, sizeof(Record), by_key_then_descending_id);
    int64_t number_hash = 0, record_hash = 0;
    for (long i = 0; i < n; i++) {
        number_hash = (number_hash * 31 + numbers[i]) % 1000000007;
        record_hash = (record_hash * 31 + by_key[i].id) % 1000000007;
    }
    snprintf(checksum, CHECKSUM_LEN, "%lld %lld", (long long)number_hash, (long long)record_hash);
    free(numbers);
    free(records);
    free(by_key);
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
