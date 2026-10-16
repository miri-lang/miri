// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=eeebdb882899993a947c711c93f873244f79aeb360a8b46ef3bf5a2ee1ce6a38

// binary-trees: allocate and walk many complete binary trees (see binary-trees.mi).

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define CHECKSUM_LEN 96

typedef struct Node {
    struct Node *left;
    struct Node *right;
} Node;

static Node *bottom_up(long depth) {
    Node *node = malloc(sizeof(Node));
    if (node == NULL) {
        abort();
    }
    node->left = depth > 0 ? bottom_up(depth - 1) : NULL;
    node->right = depth > 0 ? bottom_up(depth - 1) : NULL;
    return node;
}

static int64_t check(const Node *node) {
    if (node->left == NULL || node->right == NULL) {
        return 1;
    }
    return 1 + check(node->left) + check(node->right);
}

static void release(Node *node) {
    if (node->left != NULL) {
        release(node->left);
    }
    if (node->right != NULL) {
        release(node->right);
    }
    free(node);
}

static void kernel(long max_depth, char *checksum) {
    Node *stretch_tree = bottom_up(max_depth + 1);
    int64_t stretch = check(stretch_tree);
    release(stretch_tree);
    Node *long_lived = bottom_up(max_depth);
    int64_t total = 0;
    for (long depth = 4; depth <= max_depth; depth += 2) {
        int64_t iterations = (int64_t)1 << (max_depth - depth + 4);
        for (int64_t i = 0; i < iterations; i++) {
            Node *tree = bottom_up(depth);
            total += check(tree);
            release(tree);
        }
    }
    snprintf(checksum, CHECKSUM_LEN, "%lld %lld %lld", (long long)stretch, (long long)total,
             (long long)check(long_lived));
    release(long_lived);
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
