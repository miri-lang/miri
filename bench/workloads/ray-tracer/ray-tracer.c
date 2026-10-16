// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=7431f679d76375b3f6bc66088373c4efa690b278b62595e1a189d35c2a1d7faa

// ray-tracer: spheres shaded by a light, one ray per pixel (see ray-tracer.mi).

// clock_gettime is POSIX: strict C17 headers declare it only when asked.
#define _POSIX_C_SOURCE 200809L

// The other ports never fuse a multiply and an add into one rounding, so
// neither may this one: every language computes the same bits.
#pragma STDC FP_CONTRACT OFF

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <math.h>

#define CHECKSUM_LEN 64

typedef struct {
    double x, y, z;
} Vec3;

typedef struct {
    Vec3 center;
    double radius;
} Sphere;

static Vec3 sub(Vec3 a, Vec3 b) { return (Vec3){a.x - b.x, a.y - b.y, a.z - b.z}; }
static Vec3 scale(Vec3 a, double k) { return (Vec3){a.x * k, a.y * k, a.z * k}; }
static double dot(Vec3 a, Vec3 b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
static Vec3 normalize(Vec3 a) { return scale(a, 1.0 / sqrt(dot(a, a))); }

static double hit(Sphere sphere, Vec3 direction) {
    Vec3 oc = scale(sphere.center, -1.0);
    double b = dot(oc, direction);
    double c = dot(oc, oc) - sphere.radius * sphere.radius;
    double discriminant = b * b - c;
    if (discriminant < 0.0) {
        return -1.0;
    }
    return -b - sqrt(discriminant);
}

static void kernel(long n, char *checksum) {
    const Sphere spheres[4] = {
        {{0.0, 0.0, 5.0}, 1.0},
        {{1.5, 0.5, 6.0}, 1.0},
        {{-1.5, -0.5, 4.5}, 0.75},
        {{0.0, -101.0, 5.0}, 100.0},
    };
    const Vec3 light = normalize((Vec3){-1.0, 1.0, -1.0});
    int64_t hash = 0, hits = 0;
    for (long py = 0; py < n; py++) {
        for (long px = 0; px < n; px++) {
            Vec3 direction = normalize((Vec3){(double)px / (double)n - 0.5, 0.5 - (double)py / (double)n, 1.0});
            double nearest = 1000000.0;
            int64_t shade = 0;
            for (int s = 0; s < 4; s++) {
                double t = hit(spheres[s], direction);
                if (t > 0.0 && t < nearest) {
                    nearest = t;
                    Vec3 normal = normalize(sub(scale(direction, t), spheres[s].center));
                    double lambert = dot(normal, light);
                    shade = lambert > 0.0 ? (int64_t)(lambert * 255.0) : 0;
                }
            }
            if (nearest < 1000000.0) {
                hits += 1;
            }
            hash = (hash * 31 + shade) % 1000000007;
        }
    }
    snprintf(checksum, CHECKSUM_LEN, "%lld %lld", (long long)hash, (long long)hits);
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
