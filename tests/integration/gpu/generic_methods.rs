// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic method is compiled on the host, once per type argument its
//! calls bind, so device code declares none, and a static generic method
//! called in device code is refused at the call. An instance generic method
//! is not reached from device code today at all: no class instance is
//! GPU-compatible, so `MER_TAR_008` refuses the receiver before the call is
//! judged. A free generic function serves the device instead.

use super::utils::*;

/// The help each refusal carries.
const HELP: &str = "use a free generic function";

/// A `gpu fn` method declaring its own type parameters is refused.
#[test]
fn a_gpu_generic_method_is_refused() {
    let source = concat!(
        "use system.gpu\n",
        "\n",
        "class Box\n",
        "    var n int\n",
        "    gpu fn scale<T>(x T)\n",
        "        let y = x\n",
        "\n",
        "fn main()\n",
        "    println(\"ok\")\n",
    );
    assert_compiler_error(source, "MER_TAR_002");
    assert_compiler_error(
        source,
        "the generic method `scale` is declared as device code",
    );
    assert_compiler_error(source, HELP);
}

/// A `gpu fn` enum method declaring its own type parameters is refused.
#[test]
fn a_gpu_generic_enum_method_is_refused() {
    assert_compiler_error(
        concat!(
            "use system.gpu\n",
            "\n",
            "enum Shade\n",
            "    Light\n",
            "    gpu fn scale<T>(x T)\n",
            "        let y = x\n",
            "\n",
            "fn main()\n",
            "    println(\"ok\")\n",
        ),
        "the generic method `scale` is declared as device code",
    );
}

/// A static generic method called in a `forall` body is refused at the call.
#[test]
fn a_generic_method_called_in_device_code_is_refused() {
    let source = concat!(
        "use system.gpu\n",
        "use system.collections.array\n",
        "\n",
        "class Box\n",
        "    static fn make<T>(x T) T\n",
        "        return x\n",
        "\n",
        "fn main()\n",
        "    gpu var dst = [0, 0, 0]\n",
        "    gpu forall i in 0..3\n",
        "        dst[i] = Box.make(i)\n",
    );
    assert_compiler_error(source, "MER_TAR_002");
    assert_compiler_error(source, "the generic method `make` is called in device code");
    assert_compiler_error(source, HELP);
}

/// A static generic method called in a `gpu fn` body is refused at the call.
#[test]
fn a_generic_method_called_in_a_gpu_fn_is_refused() {
    assert_compiler_error(
        concat!(
            "use system.gpu\n",
            "\n",
            "class Box\n",
            "    static fn make<T>(x T) T\n",
            "        return x\n",
            "\n",
            "gpu fn kern(n int)\n",
            "    let m = Box.make(n)\n",
            "\n",
            "fn main()\n",
            "    println(\"ok\")\n",
        ),
        "the generic method `make` is called in device code",
    );
}
