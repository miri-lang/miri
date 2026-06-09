#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

# Fake tint validator for plumbing tests.
# Contract: if the input .wgsl file contains any 64-bit scalar type (i64, u64, f64), exit 1 (invalid).
# Otherwise exit 0 (valid).

if [ $# -lt 1 ]; then
    echo "Usage: fake_tint.sh <wgsl_file> [other args...]" >&2
    exit 1
fi

wgsl_file="$1"

if [ ! -f "$wgsl_file" ]; then
    echo "Error: file not found: $wgsl_file" >&2
    exit 1
fi

# Check if file contains any 64-bit scalar type (browser-invalid WGSL)
if grep -qE 'i64|u64|f64' "$wgsl_file"; then
    echo "error: 64-bit scalar types are not supported in WebGPU" >&2
    exit 1
fi

exit 0
