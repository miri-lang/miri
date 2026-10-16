// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=a88c0cbe1d98fa4f824c72f0e78a41bef0ac7b1c618a8da2f1090982ee06d2a6

// dispatch: shapes behind one interface, their areas summed (see dispatch.mi).
// C has no interfaces: each shape carries a pointer to its kind's table of
// functions, which is how C writes dynamic dispatch.

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define CHECKSUM_LEN 64

typedef struct Shape Shape;

typedef struct {
    double (*area)(const Shape *);
} ShapeKind;

struct Shape {
    const ShapeKind *kind;
    double a;
    double b;
};

static double circle_area(const Shape *s) { return 3.0 * s->a * s->a; }
static double square_area(const Shape *s) { return s->a * s->a; }
static double rectangle_area(const Shape *s) { return s->a * s->b; }
static double triangle_area(const Shape *s) { return s->a * s->b * 0.5; }

static const ShapeKind CIRCLE = {circle_area};
static const ShapeKind SQUARE = {square_area};
static const ShapeKind RECTANGLE = {rectangle_area};
static const ShapeKind TRIANGLE = {triangle_area};

static Shape *shape(int i) {
    static const ShapeKind *const kinds[4] = {&CIRCLE, &SQUARE, &RECTANGLE, &TRIANGLE};
    Shape *s = malloc(sizeof(Shape));
    if (s == NULL) {
        abort();
    }
    s->kind = kinds[i % 4];
    s->a = (double)(i % 7 + 1);
    s->b = (double)(i % 5 + 1);
    return s;
}

static void kernel(long n, char *checksum) {
    Shape *shapes[1000];
    for (int i = 0; i < 1000; i++) {
        shapes[i] = shape(i);
    }
    double total = 0.0;
    for (long round = 0; round < n; round++) {
        for (int i = 0; i < 1000; i++) {
            total = total + shapes[i]->kind->area(shapes[i]);
        }
    }
    snprintf(checksum, CHECKSUM_LEN, "%lld", (long long)total);
    for (int i = 0; i < 1000; i++) {
        free(shapes[i]);
    }
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
