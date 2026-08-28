.PHONY: build release test lint format clean audit gpu-browser-check runtimes conformance-agent grammar-check evals-replay evals-bless

RUNTIMES := $(patsubst %/Cargo.toml,%,$(wildcard src/runtime/*/Cargo.toml))

# Build every runtime static library in RELEASE. The compiler links
# `src/runtime/<name>/target/release` in preference to `target/debug`
# (see `runtime_library_dir` in src/pipeline.rs), so the release staticlib
# is the artifact that actually gets linked. Rebuilding only debug leaves a
# stale release `.a` in place — the compiler keeps linking old runtime code,
# which silently masks intrinsic changes and produces non-reproducible test
# failures. This target keeps the linked artifact fresh; `build` and `test`
# depend on it so nothing links a stale runtime.
# A `for` loop's exit status is that of its last iteration, so a failure in any
# earlier runtime would be swallowed and the recipe would report success. Every
# per-runtime command in this file is followed by `|| exit 1` to fail the recipe
# on the first broken runtime.
runtimes:
	@if [ -n "$(RUNTIMES)" ]; then \
		for rt in $(RUNTIMES); do \
			echo "Building $$rt (release staticlib — the profile the compiler links)"; \
			cargo build --release --manifest-path "$$rt/Cargo.toml" || exit 1; \
		done; \
	fi

build: runtimes
	cargo build

release: runtimes
	cargo build --release

test: runtimes
	cargo test -- --test-threads=4
	@if [ -n "$(RUNTIMES)" ]; then \
		for rt in $(RUNTIMES); do \
			echo "Testing $$rt"; \
			cargo test --manifest-path "$$rt/Cargo.toml" || exit 1; \
		done; \
	fi

lint:
	cargo fmt -- --check
	cargo clippy -- -D warnings
	@if [ -n "$(RUNTIMES)" ]; then \
		for rt in $(RUNTIMES); do \
			echo "Linting $$rt"; \
			cargo fmt --manifest-path "$$rt/Cargo.toml" -- --check || exit 1; \
			cargo clippy --manifest-path "$$rt/Cargo.toml" -- -D warnings || exit 1; \
		done; \
	fi

format:
	cargo fmt
	@if [ -n "$(RUNTIMES)" ]; then \
		for rt in $(RUNTIMES); do \
			echo "Formatting $$rt"; \
			cargo fmt --manifest-path "$$rt/Cargo.toml" || exit 1; \
		done; \
	fi

clean:
	cargo clean
	@if [ -n "$(RUNTIMES)" ]; then \
		for rt in $(RUNTIMES); do \
			echo "Cleaning $$rt"; \
			cargo clean --manifest-path "$$rt/Cargo.toml" || exit 1; \
		done; \
	fi

gpu-browser-check:
	@echo "Checking for tint binary (Chrome's WGSL validator)..."
	@if command -v tint >/dev/null 2>&1 || [ -n "$$MIRI_TINT" ] && [ -x "$$MIRI_TINT" ]; then \
		echo "✓ tint found"; \
		cargo test --features browser-gpu-gate --test mod browser_validation; \
	else \
		echo "✗ tint not found. To enable browser-class WGSL validation:"; \
		echo "  1. Clone Dawn (Google's WebGPU implementation)"; \
		echo "     git clone https://chromium.googlesource.com/chromium/src/third_party/dawn <path>"; \
		echo "  2. Build tint:"; \
		echo "     cd <path> && cmake -DTINT_BUILD_CMD_TOOLS=ON ... && cmake --build . -t tint"; \
		echo "  3. Set MIRI_TINT=<path>/tint or add tint to PATH"; \
		echo "  4. Re-run: make gpu-browser-check"; \
		exit 1; \
	fi

# `make audit` — mechanical sweep against PRINCIPLES.md.
# Advisory (never fails the build). Use the `miri-audit` skill for graded
# scoring + proposed diffs. This target just surfaces the raw signals.
audit:
	@echo "─── Miri principle audit (mechanical sweep) ───"
	@echo "Advisory only. For graded scoring use the miri-audit skill."
	@echo
	@echo "§3.4 — unwrap() / expect() in src/ (production panic risk):"
	@grep -rn --include='*.rs' --exclude-dir=target '\.unwrap()\|\.expect(' src/ 2>/dev/null \
		| grep -v '/tests/' \
		| grep -v '#\[cfg(test)\]' \
		| awk -F: '{print "  "$$1":"$$2}' \
		| sort -u || true
	@echo
	@echo "§5.3 — stdlib name leaks in compiler code (all of src/ except src/stdlib"
	@echo "        and src/ast/types.rs, the sanctioned single home for the name constants;"
	@echo "        comment lines filtered out). Count per file — any non-zero is a candidate"
	@echo "        for routing through the type table instead of a string literal:"
	@grep -rEn --include='*.rs' --exclude-dir=target \
		'"(List|Set|Option|Map|String|Array)"' src/ 2>/dev/null \
		| grep -v '/stdlib/' \
		| grep -v 'src/ast/types.rs:' \
		| grep -vE ':[0-9]+:[[:space:]]*///?' \
		| awk -F: '{c[$$1]++} END {for (f in c) print c[f]"\t"f}' \
		| sort -rn \
		| awk '{print "  "$$1"\t"$$2}' || true
	@echo
	@echo "§3.5 — broad '_ =>' arms in Miri-defined match sites:"
	@grep -rn --include='*.rs' --exclude-dir=target '_ =>' \
		src/mir/ src/type_checker/ src/codegen/ 2>/dev/null \
		| awk -F: '{print "  "$$1":"$$2}' \
		| sort -u || true
	@echo
	@echo "§3.3 — section banner comments (file-too-big smell):"
	@grep -rEn --include='*.rs' --exclude-dir=target \
		'^[[:space:]]*//[[:space:]]*[─-]{3,}' src/ 2>/dev/null \
		| awk -F: '{print "  "$$1":"$$2}' \
		| sort -u || true
	@echo
	@echo "§3.3 — planning-doc comment rot (§/task/milestone refs):"
	@grep -rEn --include='*.rs' --exclude-dir=target \
		'//.*((§[0-9]+\.[0-9]+)|(task [0-9])|(milestone [0-9]))' src/ 2>/dev/null \
		| awk -F: '{print "  "$$1":"$$2}' \
		| sort -u || true
	@echo
	@echo "§4.3 — panic(...) inside src/stdlib/**/*.mi (stdlib must not panic):"
	@grep -rn --include='*.mi' 'panic(' src/stdlib/ 2>/dev/null \
		| awk -F: '{print "  "$$1":"$$2}' \
		| sort -u || true
	@echo
	@echo "§3.1 — functions exceeding 80 lines (hard ceiling):"
	@for f in $$(find src -name '*.rs' -not -path '*/target/*' -not -path '*/tests/*'); do \
		awk -v file="$$f" '/^[[:space:]]*(pub )?(async )?fn / { \
			if (fn != "" && (NR - start) > 80) print "  " file ":" start ": " fn " (" (NR-start) " lines)"; \
			fn=$$0; start=NR \
		} END { \
			if (fn != "" && (NR - start) > 80) print "  " file ":" start ": " fn " (" (NR-start) " lines)" \
		}' "$$f"; \
	done 2>/dev/null || true
	@echo
	@echo "─── End audit. For graded scoring + proposed diffs, run the miri-audit skill. ───"

# Run the published agent conformance corpus (conformance/agent/) against the
# compiler. The same harness also runs as part of `make test`; this target is
# the named entry point for CI and for downstream consumers pinning a toolchain.
conformance-agent:
	cargo test --test mod conformance

# Validate the published PEG grammar (docs/grammar.peg) against the token-level
# corpus. This is the gate for grammar changes: the PEG must accept every file
# the real parser accepts and reject every file the real parser rejects.
grammar-check:
	cargo test --test mod grammar

# Replay the recorded agent transcripts under evals/ against the real compiler
# and compare what the loop cost — invocations, bytes read, bytes written —
# against evals/results/baseline.json. The same harness runs as part of
# `make test`; this target is the named entry point for working on it directly.
# It builds the runtimes first because the transcripts run and test programs,
# which link the runtime staticlib.
evals-replay: runtimes
	cargo test --test mod evals

# Re-record the baseline. Run this when a change deliberately moves what the
# loop costs, and commit the updated table alongside the change that earned it.
# A run that gets cheaper fails the gate until it is re-recorded, so the table
# stays a record of the current cost rather than a high-water mark.
evals-bless: runtimes
	MIRI_EVALS_BLESS=1 cargo test --test mod evals
