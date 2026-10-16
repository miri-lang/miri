// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=85188f6536d3277b83108e178da94c4a1d592f1802a1b14caf32c9c746ac4f84

// k-nucleotide: count the substrings of a DNA sequence (see k-nucleotide.mi).

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "../../c/table.h"
#include <ctype.h>

#define CHECKSUM_LEN 1024

static const char MOTIF[] = "GGTATTTTAATTTATAGT";

static void frequencies(const char *sequence, size_t len, size_t k, Table *counts) {
    table_init(counts);
    for (size_t i = 0; i + k <= len; i++) {
        *table_slot(counts, sequence + i, k) += 1;
    }
}

static int by_key(const void *a, const void *b) {
    const Slot *x = *(const Slot *const *)a, *y = *(const Slot *const *)b;
    return memcmp(x->key, y->key, x->len);
}

static int listing(const char *sequence, size_t len, size_t k, char *out, int room) {
    Table counts;
    frequencies(sequence, len, k, &counts);
    Slot **keys = table_slots(&counts);
    qsort(keys, counts.size, sizeof(Slot *), by_key);
    int used = 0;
    for (size_t i = 0; i < counts.size; i++) {
        used += snprintf(out + used, room - used, "%s%.*s=%lld", i > 0 ? " " : "", (int)keys[i]->len,
                         keys[i]->key, (long long)keys[i]->count);
    }
    free(keys);
    table_free(&counts);
    return used;
}

static void kernel(const char *text, char *checksum) {
    while (isspace((unsigned char)*text)) {
        text++;
    }
    size_t len = strlen(text);
    while (len > 0 && isspace((unsigned char)text[len - 1])) {
        len--;
    }
    int used = listing(text, len, 1, checksum, CHECKSUM_LEN);
    used += snprintf(checksum + used, CHECKSUM_LEN - used, " ");
    used += listing(text, len, 2, checksum + used, CHECKSUM_LEN - used);
    const size_t lengths[] = {3, 4, 6, 12, 18};
    for (size_t i = 0; i < 5; i++) {
        Table counts;
        frequencies(text, len, lengths[i], &counts);
        used += snprintf(checksum + used, CHECKSUM_LEN - used, " %.*s=%lld", (int)lengths[i], MOTIF,
                         (long long)table_get(&counts, MOTIF, lengths[i]));
        table_free(&counts);
    }
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
