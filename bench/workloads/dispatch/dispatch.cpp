// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=9bd2888eae3f5916190e3731c5d9e4c05da30742254ccb94b62a1d5165ed6428

// dispatch: shapes behind one interface, their areas summed (see dispatch.mi).

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <memory>
#include <string>
#include <vector>

namespace {

class Shape {
  public:
    virtual ~Shape() = default;
    virtual double area() const = 0;
};

class Circle final : public Shape {
  public:
    explicit Circle(double radius) : radius_(radius) {}
    double area() const override { return 3.0 * radius_ * radius_; }

  private:
    double radius_;
};

class Square final : public Shape {
  public:
    explicit Square(double side) : side_(side) {}
    double area() const override { return side_ * side_; }

  private:
    double side_;
};

class Rectangle final : public Shape {
  public:
    Rectangle(double width, double height) : width_(width), height_(height) {}
    double area() const override { return width_ * height_; }

  private:
    double width_, height_;
};

class Triangle final : public Shape {
  public:
    Triangle(double base, double height) : base_(base), height_(height) {}
    double area() const override { return base_ * height_ * 0.5; }

  private:
    double base_, height_;
};

std::unique_ptr<Shape> shape(int i) {
    const double a = i % 7 + 1;
    const double b = i % 5 + 1;
    switch (i % 4) {
    case 0:
        return std::make_unique<Circle>(a);
    case 1:
        return std::make_unique<Square>(a);
    case 2:
        return std::make_unique<Rectangle>(a, b);
    default:
        return std::make_unique<Triangle>(a, b);
    }
}

std::string kernel(std::int64_t n) {
    std::vector<std::unique_ptr<Shape>> shapes;
    shapes.reserve(1000);
    for (int i = 0; i < 1000; i++) {
        shapes.push_back(shape(i));
    }
    double total = 0.0;
    for (std::int64_t round = 0; round < n; round++) {
        for (const auto &s : shapes) {
            total = total + s->area();
        }
    }
    return std::to_string(static_cast<std::int64_t>(total));
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
