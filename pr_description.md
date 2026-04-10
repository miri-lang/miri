🚨 Severity: CRITICAL
💡 Vulnerability: Integer overflow in map allocation size calculations (`capacity * key_size` and `capacity * value_size.max(1)`).
🎯 Impact: Allows an attacker to trigger an integer overflow, allocating an undersized buffer and causing a Heap Buffer Overflow (OOM DoS or RCE).
🔧 Fix: Use `.checked_mul()` for size calculations and return `None` gracefully on overflow.
✅ Verification: Covered by existing `cargo test` in `src/runtime/core`.
