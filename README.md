# Miri Programming Language

Miri is a modern, minimal, GPU-first, statically-typed programming language designed for high performance and developer productivity. It combines the readability of Ruby and Python with the safety and speed of Rust. This is essential to balance in the age of Generative AI, when lots of code is written by machines, and should be understood by humans.

## Key Features

- **Indentation-sensitive syntax**: Clean and readable code without curly braces.
- **Static Typing**: Catch errors at compile time with powerful type inference.
- **Immutable by Default**: Variables are immutable unless declared with `var`.
- **No Nulls**: Null safety is built into the type system.

## Quick Start

### Hello World

Create a file named `hello.mi`:

```miri
use system.io.{println}

fn main()
    println("Hello, World!")
```

### Variables

```miri
let x = 10           // Immutable integer
var y = 20           // Mutable integer
y = 30               // OK
let name String = "Miri" // Explicit type
```

### Functions

```miri
fn add(a int, b int) int
    a + b

let result = add(5, 10)
```

### Control Flow

```miri
if x > 10
    print("Big")
else
    print("Small")

for i in 1..5
    print(i)
```

### Collections

```miri
let array = [1, 2, 3]
let list List<int> = [1, 2, 3]
let map = {"key": "value"}
let tuple = (1, true)
let set = {1, 2, 3}
```

### Structs

```miri
struct User
    id int
    name string

let u = User(id: 1, name: "Alice")
```

## Project Structure

Miri enforces a clean project structure:

- One public type per file.
- File name matches the type name.
- Folder structure defines the namespace.

## Building from Source

Miri is written in Rust. To build it, you need a stable Rust toolchain.

```bash
cargo build --release
```

The binary will be available at `target/release/miri`.

## Running Tests

You can run the language test suite using Cargo:

```bash
cargo test
```

## Contributing

To contribute to Miri, please ensure your code meets the quality standards:

1. **Format your code**:

    ```bash
    cargo fmt
    ```

2. **Run the linter**:

    ```bash
    cargo clippy -- -D warnings
    ```

3. **Run tests**:

    ```bash
    cargo test
    ```

## License

Apache-2.0
