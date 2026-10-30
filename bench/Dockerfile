# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

# Every toolchain the benchmark suite measures, at the versions
# bench/toolchains.toml pins, so a reader can rebuild the environment and
# rerun the suite. This is a reproducibility artifact: a publication run happens
# on bare metal and records that it did (bench/CHARTER.md section 8).
#
#   docker build -f bench/Dockerfile -t miri-bench bench/
#   docker run --rm -v "$PWD":/miri -w /miri miri-bench cargo test --test mod bench_identity

FROM python:3.13-slim-bookworm

ARG CLANG_MAJOR=21
ARG RUST_VERSION=1.97.0
ARG GO_VERSION=1.25.5
ARG DENO_VERSION=2.9.7

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl git gnupg unzip build-essential pkg-config \
 && curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key | gpg --dearmor -o /usr/share/keyrings/llvm.gpg \
 && echo "deb [signed-by=/usr/share/keyrings/llvm.gpg] http://apt.llvm.org/bookworm/ llvm-toolchain-bookworm-${CLANG_MAJOR} main" > /etc/apt/sources.list.d/llvm.list \
 && apt-get update \
 && apt-get install -y --no-install-recommends clang-${CLANG_MAJOR} libc++-${CLANG_MAJOR}-dev \
 && ln -s /usr/bin/clang-${CLANG_MAJOR} /usr/local/bin/clang \
 && ln -s /usr/bin/clang++-${CLANG_MAJOR} /usr/local/bin/clang++ \
 && rm -rf /var/lib/apt/lists/*

# The toolchain's own bin directory comes first on PATH, through a link with
# no target triple in its name: the runner hands a child only PATH and HOME
# (bench/CHARTER.md section 8), and rustup's proxies would look for their
# toolchains under that HOME.
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/rust/bin:/opt/cargo/bin:/usr/local/go/bin:/opt/deno/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain ${RUST_VERSION} --component rustfmt --component clippy \
 && ln -s "$(echo /opt/rustup/toolchains/${RUST_VERSION}-*)" /opt/rust \
 && rustc --version

RUN arch="$(dpkg --print-architecture)" \
 && curl -fsSL "https://go.dev/dl/go${GO_VERSION}.linux-${arch}.tar.gz" | tar -C /usr/local -xz \
 && go version

RUN case "$(uname -m)" in x86_64) target=x86_64-unknown-linux-gnu ;; aarch64) target=aarch64-unknown-linux-gnu ;; esac \
 && mkdir -p /opt/deno/bin \
 && curl -fsSL -o /tmp/deno.zip "https://github.com/denoland/deno/releases/download/v${DENO_VERSION}/deno-${target}.zip" \
 && unzip -q /tmp/deno.zip -d /opt/deno/bin && rm /tmp/deno.zip \
 && deno --version

# The runner records this beside every result; a CI build sets it to the
# image's digest.
ARG IMAGE_DIGEST=unset
ENV BENCH_IMAGE_DIGEST=${IMAGE_DIGEST}
