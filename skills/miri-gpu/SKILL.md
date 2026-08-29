---
name: miri-gpu
description: GPU kernels, forall loops, gpu fn, residency, and gpu frame blocks
---

# GPU Programming in Miri

Miri compiles to GPU through the `forall` construct and `gpu` keyword. This skill teaches how to write GPU kernels, manage device memory, and interact with the GPU execution model.

## GPU Fundamentals

### Implicit Launch with `forall`

The simplest GPU pattern: declare a `gpu var` and iterate over it with `forall`:

```miri
use system.collections.array

const N = 1024

gpu var data = Array<f32, N>()

forall i in 0..N
    data[i] = 2.0
```

Miri infers the grid dimensions from the range and array size.

### Multi-Dimensional Launches

`forall` supports 2-D and 3-D iteration. Literal bounds fold into the launch grid; runtime bounds use uniforms:

**2-D and 3-D launches (top-level only):**

Multi-dimensional `forall` is only valid at the top level, not inside `gpu frame` blocks. Use them to initialize buffers or set up geometry:

```miri
use system.collections.array

const W = 16
const H = 16

gpu var canvas = Array<i32, 256>()

forall x, y in 0..W, 0..H
    canvas[y * W + x] = x + y * 100
```

For 3-D:

```miri
use system.collections.array

const W = 8
const H = 8
const D = 4

gpu var volume = Array<f32, 256>()

forall x, y, z in 0..W, 0..H, 0..D
    volume[z * H * W + y * W + x] = (x as f32) + (y as f32) + (z as f32)
```

**Note:** Inside a `gpu frame` block, all `forall` passes must be 1-dimensional (e.g., `forall idx in 0..N`). Derive 2-D or 3-D coordinates from the flat index using modulo and division.

For a simple launch, initialize a buffer at the top level:

```miri
use system.gpu
use system.collections.array

const N = 256
gpu var data = Array<i32, N>()

forall i in 0..N
    data[i] = i * 2
```

### Explicit Kernel Functions

For more control, write a `gpu fn` (kernel function):

```miri
use system.gpu
use system.collections.array

gpu fn fill(dst out Array<int, 4>)
    let i = kernel.global_idx.x
    dst[i] = i

fn main()
    gpu var out = Array<int, 4>()
    fill(out).launch(Dim3(4, 1, 1), Dim3(1, 1, 1))
```

The `.launch()` method takes grid and block dimensions.

### Kernel Context

Inside a `gpu fn`, access thread and grid information via the `kernel` object:

```miri
use system.gpu
use system.collections.array

gpu fn copy(src Array<f32, 16>, dst out Array<f32, 16>)
    let thread_idx = kernel.thread_idx.x
    let block_idx = kernel.block_idx.x
    let global_idx = kernel.global_idx.x
    let block_dim = kernel.block_dim.x
    let grid_dim = kernel.grid_dim.x
    let warp_size = kernel.warp.size
    let lane_id = kernel.warp.lane_id
    dst[global_idx] = src[global_idx]
```

Available fields: `thread_idx`, `block_idx`, `global_idx`, `block_dim`, `grid_dim`, and `warp.size` / `warp.lane_id`. Each position is a `Dim3` with `.x`, `.y`, `.z` components.

### Residency: Host vs Device

Use `gpu var` to allocate on the GPU; `var` allocates on the host:

```miri
use system.collections.array

fn main()
    var host_array = Array<f32, 4>()
    gpu var device_array = Array<f32, 4>()
    device_array = host_array  // Cross-residency assignment
```

Assignments between host and device are allowed at scope boundaries. Inside a kernel, all data is device-resident.

### Frame Rendering

Declare a `gpu frame` block to run a per-frame GPU computation:

```miri
use system.gpu
use system.collections.array

const WIDTH = 8
const HEIGHT = 8
gpu var canvas = Array<int, 64>()

gpu frame
    forall idx in 0..(WIDTH * HEIGHT)
        let x = idx % WIDTH
        let y = idx / WIDTH
        let distance = ((x as f32 - frame.mouse_x * WIDTH as f32) * (x as f32 - frame.mouse_x * WIDTH as f32) + 
                        (y as f32 - frame.mouse_y * HEIGHT as f32) * (y as f32 - frame.mouse_y * HEIGHT as f32)) as i32
        canvas[idx] = frame.time as i32 + distance
```

The `gpu frame` block runs every display frame and can access frame input fields. The complete list of 14 available fields:

| Field | Type | Purpose |
|-------|------|---------|
| `time` | f32 | Elapsed time in seconds |
| `dt` | f32 | Delta time since last frame |
| `index` | int | Frame counter (0, 1, 2, ...) |
| `mouse_x` | f32 | Normalized mouse X (0.0–1.0) |
| `mouse_y` | f32 | Normalized mouse Y (0.0–1.0) |
| `mouse_down` | bool | True if mouse button is held |
| `drag_dx` | f32 | Accumulated drag X while button held |
| `drag_dy` | f32 | Accumulated drag Y while button held |
| `wheel` | f32 | Scroll wheel delta this frame |
| `clicked` | bool | True if mouse button clicked this frame |
| `double_clicked` | bool | True if double-click this frame |
| `move_x` | f32 | Pointer movement X this frame (any motion, not just drags) |
| `move_y` | f32 | Pointer movement Y this frame (any motion, not just drags) |
| `hovering` | bool | True if pointer is over the canvas |

## GPU Scalar-Width Restrictions

Not all scalar types are valid in GPU device memory. Miri enforces strict portability rules:

- **`bool` is invalid as a buffer element:** bool cannot be stored in arrays or buffers on GPU. Use `i32` (0 or 1) instead.
- **64-bit scalars (`i64`, `u64`, `f64`) are adapter-feature-gated:** They compile on some adapters but fail at runtime on mobile and WebGPU, making them a portability hazard.
- **Valid on all adapters:** `i32`, `u32`, `i16`, `u16`, `f32`.

Attempting to create a GPU array with `bool` fails at compile time:

```miri,fails=MER_TAR_007
use system.collections.array

gpu var buf = Array<bool, 4>()

gpu frame
    forall i in 0..4
        buf[i] = true
```

Use `i32` instead:

```miri
use system.collections.array

gpu var buf = Array<i32, 4>()

gpu frame
    forall i in 0..4
        buf[i] = 1  // 1 represents true
```

## Anti-Hallucination: GPU Syntax That Does Not Exist

### GPU Functions Must Not Have Return Types

GPU kernels return values through `out` parameters only:

```miri,fails=MER_TAR_008
use system.gpu

gpu fn twice(x int) int
    x * 2
```

Correct form uses `out` parameters with array buffers:

```miri
use system.gpu
use system.collections.array

gpu fn double(input Array<int, 16>, result out Array<int, 16>)
    let i = kernel.global_idx.x
    result[i] = input[i] * 2
```

## GPU Test Patterns

Test GPU code like host code — it type-checks without a GPU adapter:

```miri
use system.gpu
use system.collections.array

gpu fn init(data out Array<int, 16>)
    let i = kernel.global_idx.x
    data[i] = i * 2

fn main()
    gpu var result = Array<int, 16>()
    init(result).launch(Dim3(16, 1, 1), Dim3(1, 1, 1))
```

Run with `miri check` to verify syntax and types. No simulator or device is required; only the actual `miri run` command with GPU support needs hardware.
