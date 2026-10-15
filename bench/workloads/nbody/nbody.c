// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=85691f16fe66c33061cdc6fc77cb1fdefcf4dfb8181fbc0a56fb6493824208fe

// nbody: the Jovian planets orbiting the sun (see nbody.mi).

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
#define BODIES 5

static const double PI = 3.141592653589793;
static const double DAYS_PER_YEAR = 365.24;

typedef struct {
    double x, y, z, vx, vy, vz, mass;
} Body;

static double solar_mass(void) { return 4.0 * PI * PI; }

static Body planet(double x, double y, double z, double vx, double vy, double vz, double mass) {
    double d = DAYS_PER_YEAR;
    Body b = {x, y, z, vx * d, vy * d, vz * d, mass * solar_mass()};
    return b;
}

static void solar_system(Body *bodies) {
    Body sun = {0.0, 0.0, 0.0, 0.0, 0.0, 0.0, solar_mass()};
    bodies[0] = sun;
    bodies[1] = planet(4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01, 1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05, 9.54791938424326609e-04);
    bodies[2] = planet(8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01, -2.76742510726862411e-03, 4.99852801234917238e-03, 2.30417297573763929e-05, 2.85885980666130812e-04);
    bodies[3] = planet(1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01, 2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05, 4.36624404335156298e-05);
    bodies[4] = planet(1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01, 2.68067772490389322e-03, 1.62824170038242295e-03, -9.51592254519715870e-05, 5.15138902046611451e-05);
}

static void offset_momentum(Body *bodies) {
    double px = 0.0, py = 0.0, pz = 0.0;
    for (int i = 0; i < BODIES; i++) {
        px = px + bodies[i].vx * bodies[i].mass;
        py = py + bodies[i].vy * bodies[i].mass;
        pz = pz + bodies[i].vz * bodies[i].mass;
    }
    bodies[0].vx = -px / solar_mass();
    bodies[0].vy = -py / solar_mass();
    bodies[0].vz = -pz / solar_mass();
}

static double energy(const Body *bodies) {
    double e = 0.0;
    for (int i = 0; i < BODIES; i++) {
        const Body *b = &bodies[i];
        e = e + 0.5 * b->mass * (b->vx * b->vx + b->vy * b->vy + b->vz * b->vz);
        for (int j = i + 1; j < BODIES; j++) {
            const Body *o = &bodies[j];
            double dx = b->x - o->x;
            double dy = b->y - o->y;
            double dz = b->z - o->z;
            e = e - b->mass * o->mass / sqrt(dx * dx + dy * dy + dz * dz);
        }
    }
    return e;
}

static void advance(Body *bodies, double dt) {
    for (int i = 0; i < BODIES; i++) {
        Body *bi = &bodies[i];
        for (int j = i + 1; j < BODIES; j++) {
            Body *bj = &bodies[j];
            double dx = bi->x - bj->x;
            double dy = bi->y - bj->y;
            double dz = bi->z - bj->z;
            double d2 = dx * dx + dy * dy + dz * dz;
            double mag = dt / (d2 * sqrt(d2));
            double mi = bi->mass * mag;
            double mj = bj->mass * mag;
            bi->vx = bi->vx - dx * mj;
            bi->vy = bi->vy - dy * mj;
            bi->vz = bi->vz - dz * mj;
            bj->vx = bj->vx + dx * mi;
            bj->vy = bj->vy + dy * mi;
            bj->vz = bj->vz + dz * mi;
        }
    }
    for (int i = 0; i < BODIES; i++) {
        bodies[i].x = bodies[i].x + dt * bodies[i].vx;
        bodies[i].y = bodies[i].y + dt * bodies[i].vy;
        bodies[i].z = bodies[i].z + dt * bodies[i].vz;
    }
}

static void kernel(long steps, char *checksum) {
    Body bodies[BODIES];
    solar_system(bodies);
    offset_momentum(bodies);
    double before = energy(bodies);
    for (long s = 0; s < steps; s++) {
        advance(bodies, 0.01);
    }
    double after = energy(bodies);
    snprintf(checksum, CHECKSUM_LEN, "%lld %lld", (long long)(before * 1000000000.0),
             (long long)(after * 1000000000.0));
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
