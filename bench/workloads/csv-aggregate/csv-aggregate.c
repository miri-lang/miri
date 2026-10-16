// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=5f8f278627fdca52121de84ddf518531c0ba10f4dba6ef2182515acd8d4928a8

// csv-aggregate: totals by key over a generated CSV file (see csv-aggregate.mi).

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "../../c/table.h"

#define CHECKSUM_LEN 65536
#define MAX_KEY_LEN 31

typedef struct {
    char key[MAX_KEY_LEN + 1];
    int64_t count;
    int64_t quantity;
    double amount;
} Totals;

static int by_key(const void *a, const void *b) { return strcmp(((const Totals *)a)->key, ((const Totals *)b)->key); }

/// The record at `slot`, growing the array of totals to hold it.
static Totals *totals_at(Totals **groups, size_t *cap, size_t slot) {
    if (slot >= *cap) {
        *cap = *cap == 0 ? 64 : *cap * 2;
        *groups = realloc(*groups, sizeof(Totals) * *cap);
        if (*groups == NULL) {
            abort();
        }
    }
    return &(*groups)[slot];
}

static void kernel(const char *text, char *checksum) {
    Totals *groups = NULL;
    size_t used = 0, cap = 0;
    Table index;
    table_init(&index);
    const char *line = strchr(text, '\n');
    while (line != NULL && *++line != '\0') {
        const char *end = strchr(line, '\n');
        size_t line_len = end != NULL ? (size_t)(end - line) : strlen(line);
        const char *comma = memchr(line, ',', line_len);
        if (comma == NULL || (size_t)(comma - line) > MAX_KEY_LEN) {
            fprintf(stderr, "a record has no key or a key longer than %d bytes\n", MAX_KEY_LEN);
            exit(2);
        }
        size_t key_len = (size_t)(comma - line);
        int64_t *slot = table_slot(&index, line, key_len);
        if (*slot == 0) {
            Totals *fresh = totals_at(&groups, &cap, used);
            memset(fresh, 0, sizeof(Totals));
            memcpy(fresh->key, line, key_len);
            *slot = (int64_t)++used;
        }
        Totals *t = &groups[*slot - 1];
        char *after;
        t->count += 1;
        t->quantity += strtoll(comma + 1, &after, 10);
        t->amount = t->amount + strtod(after + 1, NULL);
        if (end == NULL) {
            break;
        }
        line = end;
    }
    qsort(groups, used, sizeof(Totals), by_key);
    Buffer out = {0};
    char field[96];
    for (size_t i = 0; i < used; i++) {
        int len = snprintf(field, sizeof field, "%s%s:%lld:%lld:%lld", i > 0 ? "," : "", groups[i].key,
                           (long long)groups[i].count, (long long)groups[i].quantity,
                           (long long)(groups[i].amount * 100.0));
        buffer_append(&out, field, (size_t)len);
    }
    if (out.len >= CHECKSUM_LEN) {
        fprintf(stderr, "the checksum is longer than %d bytes\n", CHECKSUM_LEN);
        exit(2);
    }
    memcpy(checksum, out.data ? out.data : "", out.len + 1);
    buffer_free(&out);
    free(groups);
    table_free(&index);
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
