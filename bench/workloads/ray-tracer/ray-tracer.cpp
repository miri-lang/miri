// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=86c40b245775c4a188a045ad6b5fbb39d2faa5881fa31989ac35a0d923c87d23

// ray-tracer: spheres shaded by a light, one ray per pixel (see ray-tracer.mi).

// The other ports never fuse a multiply and an add into one rounding, so
// neither may this one: every language computes the same bits.
#pragma STDC FP_CONTRACT OFF

#include <array>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <string>

namespace {

struct Vec3 {
    double x, y, z;
};

struct Sphere {
    Vec3 center;
    double radius;
};

Vec3 sub(Vec3 a, Vec3 b) { return {a.x - b.x, a.y - b.y, a.z - b.z}; }
Vec3 scale(Vec3 a, double k) { return {a.x * k, a.y * k, a.z * k}; }
double dot(Vec3 a, Vec3 b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
Vec3 normalize(Vec3 a) { return scale(a, 1.0 / std::sqrt(dot(a, a))); }

double hit(const Sphere &sphere, Vec3 direction) {
    const Vec3 oc = scale(sphere.center, -1.0);
    const double b = dot(oc, direction);
    const double c = dot(oc, oc) - sphere.radius * sphere.radius;
    const double discriminant = b * b - c;
    if (discriminant < 0.0) {
        return -1.0;
    }
    return -b - std::sqrt(discriminant);
}

std::string kernel(std::int64_t n) {
    const std::array<Sphere, 4> spheres{{
        {{0.0, 0.0, 5.0}, 1.0},
        {{1.5, 0.5, 6.0}, 1.0},
        {{-1.5, -0.5, 4.5}, 0.75},
        {{0.0, -101.0, 5.0}, 100.0},
    }};
    const Vec3 light = normalize({-1.0, 1.0, -1.0});
    std::int64_t hash = 0, hits = 0;
    const double size = static_cast<double>(n);
    for (std::int64_t py = 0; py < n; py++) {
        for (std::int64_t px = 0; px < n; px++) {
            const Vec3 direction = normalize({static_cast<double>(px) / size - 0.5, 0.5 - static_cast<double>(py) / size, 1.0});
            double nearest = 1000000.0;
            std::int64_t shade = 0;
            for (const auto &sphere : spheres) {
                const double t = hit(sphere, direction);
                if (t > 0.0 && t < nearest) {
                    nearest = t;
                    const Vec3 normal = normalize(sub(scale(direction, t), sphere.center));
                    const double lambert = dot(normal, light);
                    shade = lambert > 0.0 ? static_cast<std::int64_t>(lambert * 255.0) : 0;
                }
            }
            if (nearest < 1000000.0) {
                hits += 1;
            }
            hash = (hash * 31 + shade) % 1000000007;
        }
    }
    return std::to_string(hash) + " " + std::to_string(hits);
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
