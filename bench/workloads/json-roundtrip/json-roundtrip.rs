// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=3c757480c5359e2a9db0998b9c67d68216a533e60dfddc19ba335cc31250e7c7

// json-roundtrip: parse, walk and rewrite a JSON document (see json-roundtrip.mi).
//
// Rust's standard library has no JSON parser, and the charter admits no
// third-party crates, so this port carries a small recursive-descent parser
// and writer for the JSON the generator emits: objects, arrays, ASCII strings
// without escapes, integers, booleans and null.

use std::time::Instant;

enum Json<'a> {
    Object(Vec<(&'a str, Json<'a>)>),
    Array(Vec<Json<'a>>),
    Text(&'a str),
    Number(i64),
    Bool(bool),
    Null,
}

struct Parser<'a> {
    source: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Parser<'a> {
    fn skip_whitespace(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Option<()> {
        self.skip_whitespace();
        (self.bytes.get(self.at) == Some(&byte)).then(|| self.at += 1)
    }

    fn literal(&mut self, word: &str, value: Json<'a>) -> Option<Json<'a>> {
        let end = self.at + word.len();
        (self.bytes.get(self.at..end) == Some(word.as_bytes())).then(|| {
            self.at = end;
            value
        })
    }

    fn text(&mut self) -> Option<&'a str> {
        self.expect(b'"')?;
        let start = self.at;
        while *self.bytes.get(self.at)? != b'"' {
            self.at += 1;
        }
        self.at += 1;
        self.source.get(start..self.at - 1)
    }

    fn number(&mut self) -> Option<Json<'a>> {
        let start = self.at;
        if self.bytes.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        self.source.get(start..self.at)?.parse().ok().map(Json::Number)
    }

    fn sequence<T>(&mut self, close: u8, mut item: impl FnMut(&mut Self) -> Option<T>) -> Option<Vec<T>> {
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.at) == Some(&close) {
            self.at += 1;
            return Some(items);
        }
        loop {
            items.push(item(self)?);
            self.skip_whitespace();
            match self.bytes.get(self.at)? {
                b',' => self.at += 1,
                byte if *byte == close => {
                    self.at += 1;
                    return Some(items);
                }
                _ => return None,
            }
        }
    }

    fn value(&mut self) -> Option<Json<'a>> {
        self.skip_whitespace();
        match *self.bytes.get(self.at)? {
            b'{' => {
                self.at += 1;
                let members = self.sequence(b'}', |p| {
                    let key = p.text()?;
                    p.expect(b':')?;
                    Some((key, p.value()?))
                })?;
                Some(Json::Object(members))
            }
            b'[' => {
                self.at += 1;
                Some(Json::Array(self.sequence(b']', Self::value)?))
            }
            b'"' => self.text().map(Json::Text),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'n' => self.literal("null", Json::Null),
            _ => self.number(),
        }
    }
}

fn write(value: &Json, out: &mut String) {
    match value {
        Json::Object(members) => {
            out.push('{');
            for (i, (key, item)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('"');
                out.push_str(key);
                out.push_str("\":");
                write(item, out);
            }
            out.push('}');
        }
        Json::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        Json::Text(text) => {
            out.push('"');
            out.push_str(text);
            out.push('"');
        }
        Json::Number(number) => out.push_str(&number.to_string()),
        Json::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Json::Null => out.push_str("null"),
    }
}

fn walk(value: &Json) -> (i64, i64) {
    let add = |(nodes, sum): (i64, i64), item: &Json| {
        let (n, s) = walk(item);
        (nodes + n, sum + s)
    };
    match value {
        Json::Object(members) => members.iter().map(|(_, item)| item).fold((1, 0), add),
        Json::Array(items) => items.iter().fold((1, 0), add),
        Json::Number(number) => (1, *number),
        Json::Text(_) | Json::Bool(_) | Json::Null => (1, 0),
    }
}

fn kernel(text: &str) -> String {
    let mut parser = Parser { source: text, bytes: text.as_bytes(), at: 0 };
    let Some(document) = parser.value() else {
        return "invalid".to_string();
    };
    let (nodes, sum) = walk(&document);
    let mut written = String::new();
    write(&document, &mut written);
    format!("{nodes} {sum} {}", written.len())
}

fn run_hot(n: &str) -> String {
    let expected = kernel(n);
    let mut agree = true;
    for _ in 1..5 {
        let checksum = kernel(n);
        agree = agree && checksum == expected;
    }
    for _ in 0..5 {
        let start = Instant::now();
        let checksum = kernel(n);
        let elapsed = start.elapsed().as_nanos();
        eprintln!("hot_ns={elapsed}");
        agree = agree && checksum == expected;
    }
    if !agree {
        eprintln!("checksum differs between iterations");
        std::process::exit(1);
    }
    expected
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input = std::fs::read_to_string(&args[1]).unwrap_or_else(|_| std::process::exit(2));
    let hot = args.get(2).is_some_and(|a| a == "hot");
    println!("{}", if hot { run_hot(&input) } else { kernel(&input) });
}
