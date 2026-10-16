// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=72a96422911de4e16bf3d999c7095c138d6bdb8934eace559b844b75aee912bc

// json-roundtrip: parse, walk and rewrite a JSON document (see json-roundtrip.mi).
//
// The C++ standard library has no JSON parser, and the charter admits no
// third-party libraries, so this port carries a small recursive-descent parser
// and writer for the JSON the generator emits: objects, arrays, ASCII strings
// without escapes, integers, booleans and null.

#include <charconv>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <iterator>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace {

struct Json {
    enum class Kind { Object, Array, Text, Number, Bool, Null } kind = Kind::Null;
    std::vector<std::pair<std::string_view, Json>> members;
    std::vector<Json> items;
    std::string_view text;
    std::int64_t number = 0;
    bool flag = false;
};

class Parser {
  public:
    explicit Parser(std::string_view source) : source_(source) {}

    std::optional<Json> value() {
        skip_whitespace();
        if (at_ >= source_.size()) {
            return std::nullopt;
        }
        switch (source_[at_]) {
        case '{':
            return object();
        case '[':
            return array();
        case '"': {
            Json json;
            json.kind = Json::Kind::Text;
            if (!text(json.text)) {
                return std::nullopt;
            }
            return json;
        }
        case 't':
            return literal("true", Json::Kind::Bool, true);
        case 'f':
            return literal("false", Json::Kind::Bool, false);
        case 'n':
            return literal("null", Json::Kind::Null, false);
        default:
            return number();
        }
    }

  private:
    void skip_whitespace() {
        while (at_ < source_.size() && (source_[at_] == ' ' || source_[at_] == '\n' || source_[at_] == '\t' || source_[at_] == '\r')) {
            at_++;
        }
    }

    bool consume(char expected) {
        skip_whitespace();
        if (at_ < source_.size() && source_[at_] == expected) {
            at_++;
            return true;
        }
        return false;
    }

    bool text(std::string_view &out) {
        if (!consume('"')) {
            return false;
        }
        const std::size_t end = source_.find('"', at_);
        if (end == std::string_view::npos) {
            return false;
        }
        out = source_.substr(at_, end - at_);
        at_ = end + 1;
        return true;
    }

    std::optional<Json> literal(std::string_view word, Json::Kind kind, bool flag) {
        if (source_.substr(at_, word.size()) != word) {
            return std::nullopt;
        }
        at_ += word.size();
        Json json;
        json.kind = kind;
        json.flag = flag;
        return json;
    }

    std::optional<Json> number() {
        Json json;
        json.kind = Json::Kind::Number;
        const char *first = source_.data() + at_;
        const auto [end, error] = std::from_chars(first, source_.data() + source_.size(), json.number);
        if (error != std::errc()) {
            return std::nullopt;
        }
        at_ += static_cast<std::size_t>(end - first);
        return json;
    }

    std::optional<Json> object() {
        at_++;
        Json json;
        json.kind = Json::Kind::Object;
        if (consume('}')) {
            return json;
        }
        do {
            std::string_view key;
            if (!text(key) || !consume(':')) {
                return std::nullopt;
            }
            auto item = value();
            if (!item) {
                return std::nullopt;
            }
            json.members.emplace_back(key, std::move(*item));
        } while (consume(','));
        return consume('}') ? std::optional<Json>(std::move(json)) : std::nullopt;
    }

    std::optional<Json> array() {
        at_++;
        Json json;
        json.kind = Json::Kind::Array;
        if (consume(']')) {
            return json;
        }
        do {
            auto item = value();
            if (!item) {
                return std::nullopt;
            }
            json.items.push_back(std::move(*item));
        } while (consume(','));
        return consume(']') ? std::optional<Json>(std::move(json)) : std::nullopt;
    }

    std::string_view source_;
    std::size_t at_ = 0;
};

void write(const Json &value, std::string &out) {
    switch (value.kind) {
    case Json::Kind::Object:
        out += '{';
        for (std::size_t i = 0; i < value.members.size(); i++) {
            if (i > 0) {
                out += ',';
            }
            out += '"';
            out += value.members[i].first;
            out += "\":";
            write(value.members[i].second, out);
        }
        out += '}';
        break;
    case Json::Kind::Array:
        out += '[';
        for (std::size_t i = 0; i < value.items.size(); i++) {
            if (i > 0) {
                out += ',';
            }
            write(value.items[i], out);
        }
        out += ']';
        break;
    case Json::Kind::Text:
        out += '"';
        out += value.text;
        out += '"';
        break;
    case Json::Kind::Number:
        out += std::to_string(value.number);
        break;
    case Json::Kind::Bool:
        out += value.flag ? "true" : "false";
        break;
    case Json::Kind::Null:
        out += "null";
        break;
    }
}

std::pair<std::int64_t, std::int64_t> walk(const Json &value) {
    std::pair<std::int64_t, std::int64_t> totals{1, 0};
    switch (value.kind) {
    case Json::Kind::Object:
        for (const auto &member : value.members) {
            const auto inner = walk(member.second);
            totals.first += inner.first;
            totals.second += inner.second;
        }
        break;
    case Json::Kind::Array:
        for (const auto &item : value.items) {
            const auto inner = walk(item);
            totals.first += inner.first;
            totals.second += inner.second;
        }
        break;
    case Json::Kind::Number:
        totals.second = value.number;
        break;
    case Json::Kind::Text:
    case Json::Kind::Bool:
    case Json::Kind::Null:
        break;
    }
    return totals;
}

std::string kernel(const std::string &text) {
    Parser parser(text);
    const auto document = parser.value();
    if (!document) {
        return "invalid";
    }
    const auto [nodes, sum] = walk(*document);
    std::string written;
    write(*document, written);
    return std::to_string(nodes) + " " + std::to_string(sum) + " " + std::to_string(written.size());
}

std::string run_hot(const std::string & n) {
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
    std::ifstream input(argv[1], std::ios::binary);
    if (!input) {
        return 2;
    }
    const std::string n((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>());
    const bool hot = argc > 2 && std::string(argv[2]) == "hot";
    std::cout << (hot ? run_hot(n) : kernel(n)) << "\n";
    return 0;
}
