// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=45e354a69cc048a3fbcfe745be81e5b6c4f0259c07b5d530f463455e4cf8d1cd

// nbody: the Jovian planets orbiting the sun (see nbody.mi).

// The other ports never fuse a multiply and an add into one rounding, so
// neither may this one: every language computes the same bits.
#pragma STDC FP_CONTRACT OFF

#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

namespace {

constexpr double kPi = 3.141592653589793;
constexpr double kSolarMass = 4.0 * kPi * kPi;
constexpr double kDaysPerYear = 365.24;

struct Body {
    double x, y, z, vx, vy, vz, mass;
};

Body planet(double x, double y, double z, double vx, double vy, double vz, double mass) {
    const double d = kDaysPerYear;
    return Body{x, y, z, vx * d, vy * d, vz * d, mass * kSolarMass};
}

std::vector<Body> solar_system() {
    return {
        Body{0.0, 0.0, 0.0, 0.0, 0.0, 0.0, kSolarMass},
        planet(4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01, 1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05, 9.54791938424326609e-04),
        planet(8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01, -2.76742510726862411e-03, 4.99852801234917238e-03, 2.30417297573763929e-05, 2.85885980666130812e-04),
        planet(1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01, 2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05, 4.36624404335156298e-05),
        planet(1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01, 2.68067772490389322e-03, 1.62824170038242295e-03, -9.51592254519715870e-05, 5.15138902046611451e-05),
    };
}

void offset_momentum(std::vector<Body> &bodies) {
    double px = 0.0, py = 0.0, pz = 0.0;
    for (const Body &b : bodies) {
        px = px + b.vx * b.mass;
        py = py + b.vy * b.mass;
        pz = pz + b.vz * b.mass;
    }
    bodies[0].vx = -px / kSolarMass;
    bodies[0].vy = -py / kSolarMass;
    bodies[0].vz = -pz / kSolarMass;
}

double energy(const std::vector<Body> &bodies) {
    double e = 0.0;
    for (std::size_t i = 0; i < bodies.size(); i++) {
        const Body &b = bodies[i];
        e = e + 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz);
        for (std::size_t j = i + 1; j < bodies.size(); j++) {
            const Body &o = bodies[j];
            const double dx = b.x - o.x;
            const double dy = b.y - o.y;
            const double dz = b.z - o.z;
            e = e - b.mass * o.mass / std::sqrt(dx * dx + dy * dy + dz * dz);
        }
    }
    return e;
}

void advance(std::vector<Body> &bodies, double dt) {
    for (std::size_t i = 0; i < bodies.size(); i++) {
        Body &bi = bodies[i];
        for (std::size_t j = i + 1; j < bodies.size(); j++) {
            Body &bj = bodies[j];
            const double dx = bi.x - bj.x;
            const double dy = bi.y - bj.y;
            const double dz = bi.z - bj.z;
            const double d2 = dx * dx + dy * dy + dz * dz;
            const double mag = dt / (d2 * std::sqrt(d2));
            const double mi = bi.mass * mag;
            const double mj = bj.mass * mag;
            bi.vx = bi.vx - dx * mj;
            bi.vy = bi.vy - dy * mj;
            bi.vz = bi.vz - dz * mj;
            bj.vx = bj.vx + dx * mi;
            bj.vy = bj.vy + dy * mi;
            bj.vz = bj.vz + dz * mi;
        }
    }
    for (Body &b : bodies) {
        b.x = b.x + dt * b.vx;
        b.y = b.y + dt * b.vy;
        b.z = b.z + dt * b.vz;
    }
}

std::string kernel(std::int64_t steps) {
    std::vector<Body> bodies = solar_system();
    offset_momentum(bodies);
    const double before = energy(bodies);
    for (std::int64_t s = 0; s < steps; s++) {
        advance(bodies, 0.01);
    }
    const double after = energy(bodies);
    return std::to_string(static_cast<std::int64_t>(before * 1000000000.0)) + " " +
           std::to_string(static_cast<std::int64_t>(after * 1000000000.0));
}

std::string run_hot(std::int64_t n) {
    const std::string expected = kernel(n);
    bool agree = true;
    for (int i = 1; i < 5; i++) {
        const std::string checksum = kernel(n);
        agree = agree && checksum == expected;
    }
    for (int i = 0; i < 5; i++) {
        const auto start = std::chrono::steady_clock::now();
        const std::string checksum = kernel(n);
        const auto elapsed = std::chrono::steady_clock::now() - start;
        std::cerr << "hot_ns=" << std::chrono::duration_cast<std::chrono::nanoseconds>(elapsed).count() << "\n";
        agree = agree && checksum == expected;
    }
    if (!agree) {
        std::cerr << "checksum differs between iterations\n";
        std::exit(1);
    }
    return expected;
}

}  // namespace

int main(int argc, char **argv) {
    std::ifstream input(argv[1]);
    std::int64_t n = 0;
    if (!(input >> n)) {
        return 2;
    }
    const bool hot = argc > 2 && std::string(argv[2]) == "hot";
    std::cout << (hot ? run_hot(n) : kernel(n)) << "\n";
    return 0;
}
