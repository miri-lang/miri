// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=caad1b61affcb92780555d07781e16322c6062339ac23b16e1407a2987c8b70f

// The plain structures the C ports need and C's standard library lacks,
// written once and shared by every C workload: a growable byte buffer and a
// chained hash table from a byte string to a count, hashed with FNV-1a.
//
// A key is a view (a pointer and a length) into memory the caller keeps alive
// for as long as the table; the table copies nothing. Everything is
// `static` in this header, so a workload is still one translation unit built
// with the charter's single-file build line.

#ifndef BENCH_TABLE_H
#define BENCH_TABLE_H

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} Buffer;

static inline void buffer_reserve(Buffer *b, size_t extra) {
    if (b->len + extra + 1 <= b->cap) {
        return;
    }
    size_t cap = b->cap == 0 ? 64 : b->cap;
    while (cap < b->len + extra + 1) {
        cap *= 2;
    }
    char *grown = realloc(b->data, cap);
    if (grown == NULL) {
        abort();
    }
    b->data = grown;
    b->cap = cap;
}

static inline void buffer_append(Buffer *b, const char *bytes, size_t len) {
    buffer_reserve(b, len);
    memcpy(b->data + b->len, bytes, len);
    b->len += len;
    b->data[b->len] = '\0';
}

static inline void buffer_free(Buffer *b) {
    free(b->data);
    b->data = NULL;
    b->len = b->cap = 0;
}

typedef struct Slot {
    const char *key;
    size_t len;
    int64_t count;
    struct Slot *next;
} Slot;

typedef struct {
    Slot **buckets;
    size_t bucket_count;
    size_t size;
} Table;

static inline uint64_t fnv1a(const char *key, size_t len) {
    uint64_t hash = 14695981039346656037ull;
    for (size_t i = 0; i < len; i++) {
        hash ^= (unsigned char)key[i];
        hash *= 1099511628211ull;
    }
    return hash;
}

static inline void table_init(Table *t) {
    t->bucket_count = 16;
    t->size = 0;
    t->buckets = calloc(t->bucket_count, sizeof(Slot *));
    if (t->buckets == NULL) {
        abort();
    }
}

static inline void table_grow(Table *t) {
    size_t count = t->bucket_count * 2;
    Slot **buckets = calloc(count, sizeof(Slot *));
    if (buckets == NULL) {
        abort();
    }
    for (size_t i = 0; i < t->bucket_count; i++) {
        Slot *slot = t->buckets[i];
        while (slot != NULL) {
            Slot *next = slot->next;
            size_t at = fnv1a(slot->key, slot->len) & (count - 1);
            slot->next = buckets[at];
            buckets[at] = slot;
            slot = next;
        }
    }
    free(t->buckets);
    t->buckets = buckets;
    t->bucket_count = count;
}

/// The count stored under `key`, created at zero when absent.
static inline int64_t *table_slot(Table *t, const char *key, size_t len) {
    size_t at = fnv1a(key, len) & (t->bucket_count - 1);
    for (Slot *slot = t->buckets[at]; slot != NULL; slot = slot->next) {
        if (slot->len == len && memcmp(slot->key, key, len) == 0) {
            return &slot->count;
        }
    }
    if (t->size + 1 > t->bucket_count) {
        table_grow(t);
        at = fnv1a(key, len) & (t->bucket_count - 1);
    }
    Slot *slot = malloc(sizeof(Slot));
    if (slot == NULL) {
        abort();
    }
    slot->key = key;
    slot->len = len;
    slot->count = 0;
    slot->next = t->buckets[at];
    t->buckets[at] = slot;
    t->size += 1;
    return &slot->count;
}

/// The count stored under `key`, or zero when absent.
static inline int64_t table_get(const Table *t, const char *key, size_t len) {
    size_t at = fnv1a(key, len) & (t->bucket_count - 1);
    for (Slot *slot = t->buckets[at]; slot != NULL; slot = slot->next) {
        if (slot->len == len && memcmp(slot->key, key, len) == 0) {
            return slot->count;
        }
    }
    return 0;
}

/// Every slot of the table, in no particular order, in a fresh array of
/// `t->size` pointers the caller frees.
static inline Slot **table_slots(const Table *t) {
    Slot **all = malloc(sizeof(Slot *) * (t->size == 0 ? 1 : t->size));
    if (all == NULL) {
        abort();
    }
    size_t n = 0;
    for (size_t i = 0; i < t->bucket_count; i++) {
        for (Slot *slot = t->buckets[i]; slot != NULL; slot = slot->next) {
            all[n++] = slot;
        }
    }
    return all;
}

static inline void table_free(Table *t) {
    for (size_t i = 0; i < t->bucket_count; i++) {
        Slot *slot = t->buckets[i];
        while (slot != NULL) {
            Slot *next = slot->next;
            free(slot);
            slot = next;
        }
    }
    free(t->buckets);
    t->buckets = NULL;
    t->bucket_count = t->size = 0;
}

/// The whole file at `path` in a fresh NUL-terminated buffer, or NULL.
static inline char *read_file(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (f == NULL) {
        return NULL;
    }
    Buffer b = {0};
    char chunk[65536];
    size_t got;
    while ((got = fread(chunk, 1, sizeof chunk, f)) > 0) {
        buffer_append(&b, chunk, got);
    }
    fclose(f);
    if (b.data == NULL) {
        buffer_append(&b, "", 0);
    }
    *len = b.len;
    return b.data;
}

#endif
