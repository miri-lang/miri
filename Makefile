.PHONY: build release test lint format clean audit

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
	cargo test -- --test-threads=4
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

clean:
	cargo clean
	@if [ -n "$(RUNTIMES)" ]; then \
		for rt in $(RUNTIMES); do \
			echo "Cleaning $$rt"; \
			cargo clean --manifest-path "$$rt/Cargo.toml"; \
		done; \
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
