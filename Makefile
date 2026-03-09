.PHONY: build release test lint format

RUNTIMES := $(patsubst %/Cargo.toml,%,$(wildcard src/runtime/*/Cargo.toml))

build:
	cargo build
	@if [ -n "$(RUNTIMES)" ]; then \
		for rt in $(RUNTIMES); do \
			echo "Building $$rt"; \
			cargo build --manifest-path "$$rt/Cargo.toml"; \
		done; \
	fi

release:
	cargo build --release
	@if [ -n "$(RUNTIMES)" ]; then \
		for rt in $(RUNTIMES); do \
			echo "Building $$rt (release)"; \
			cargo build --release --manifest-path "$$rt/Cargo.toml"; \
		done; \
	fi

test:
	cargo test
	@if [ -n "$(RUNTIMES)" ]; then \
		for rt in $(RUNTIMES); do \
			echo "Testing $$rt"; \
			cargo test --manifest-path "$$rt/Cargo.toml"; \
		done; \
	fi

lint:
	cargo fmt -- --check
	cargo clippy -- -D warnings
	@if [ -n "$(RUNTIMES)" ]; then \
		for rt in $(RUNTIMES); do \
			echo "Linting $$rt"; \
			cargo fmt --manifest-path "$$rt/Cargo.toml" -- --check; \
			cargo clippy --manifest-path "$$rt/Cargo.toml" -- -D warnings; \
		done; \
	fi

format:
	cargo fmt
	@if [ -n "$(RUNTIMES)" ]; then \
		for rt in $(RUNTIMES); do \
			echo "Formatting $$rt"; \
			cargo fmt --manifest-path "$$rt/Cargo.toml"; \
		done; \
	fi
