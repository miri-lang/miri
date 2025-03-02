# Miri: Vision and Design

## **Vision**

Miri is a high-performance, easy-to-learn programming language designed for AI, machine learning, and critical applications (space, science, military, and high-reliability systems). It balances Python-like developer productivity with the performance of Rust and C++, providing memory safety, powerful concurrency, and first-class hardware acceleration.

## **Core Principles**

- **Blend Productivity and Performance**: Offer the simplicity of Python with the efficiency of Rust/C++.
- **Memory and Type Safety**: Ensure safe memory management without sacrificing control.
- **Multi-Paradigm Flexibility**: Support OOP, functional, and reactive programming seamlessly.
- **Concurrency First**: Provide native concurrency primitives for scalable parallel execution.
- **Hardware Awareness**: Optimize for modern hardware, including GPUs and accelerators.

---

## **Key Features**

### **1. Strong Type System with Gradual Typing**

- **Statically typed** with powerful type inference.
- **Gradual typing** allows developers to opt into dynamic behavior for rapid prototyping.
- **Algebraic types and pattern matching** for expressive coding.
- **No `nil/null` references** to prevent common runtime errors.

### **2. Immutability and Scoped Variables**

- **Immutable data structures** by default to ensure safety.
- **Function-level mutable variables** (scoped, ensuring isolation).
- **Instance fields prefixed with `_` and cannot change.

### **3. Memory Management & Safety**

- **Borrowing and ownership model** inspired by Rust, with automatic inference in common cases.
- **Compile-time garbage collection** with an opt-out for manual memory management when needed.
- **Minimal runtime overhead** for high performance.

### **4. Concurrency & Parallelism**

- **Actor-based concurrency model** (similar to Erlang/Akka) for message-passing.
- **Lightweight threads** (goroutine-like) with `async/await` for non-blocking operations.
- **First-class GPU and accelerator support** for AI/ML tasks.
- **Automatic parallelization** for computational workloads.

### **5. Compile-Time Testing & Quality Assurance**

- **Tests are part of compilation**, ensuring every build is validated.
- **Self-testing language features** to add an additional layer of verification.

### **6. Performance Optimization & Hardware Utilization**

- **Highly optimized compiler** with fast incremental compilation.
- **Automatic SIMD vectorization** and parallel execution for CPU-bound tasks.
- **Direct GPU utilization** for AI/ML workloads with minimal boilerplate.

### **7. Simplified Build & Dependency Management**

- **Minimal build tooling**, inspired by Go’s simplicity.
- **Dependency resolution built into the compiler**, avoiding external package managers.
- **Fast compilation times** for iterative development.

---

## **Code Organization & Namespacing**

- **Folder structure defines namespace** (e.g., `Blog/Users/User` → `Blog.Users.User`).
- **One file per type**, ensuring clear separation of concerns.
- **No global variables**, encouraging modular design.

---

## **Extending and Structuring Types**

### **1. Inheritance & Interfaces**

- **Inheritance is used only for abstracting common behaviors**.
- **Abstract classes provide only unimplemented methods**.
- **No overriding of concrete methods** to maintain consistency.
- **Preserve client context**: No surprises when extending types.

### **2. Alternative to Classical Inheritance**

Instead of deep inheritance chains, MiriLang introduces flexible type extensions:

```miri
# Inheritance: This type also receives contracts from the extended types.
extends SomeBaseType1, SomeBaseType2

# Interface Implementation: Enforces contracts to implement all public members.
is SomeInterfaceType1, SomeInterfaceType2

# Composition: Includes reusable components.
includes SomeIncludedType1, SomeIncludedType2
includes func1, func2, func3 from SomeOtherIncludedType
```

- **Fields are protected**: Only visible to types that `extend`, but hidden from `include`.
- **Encapsulation enforced**: No type should depend on another’s internal state.

---

## **Target Use Cases**

1. **High-Performance Systems**: Real-time simulations, game engines, embedded systems.
2. **AI & ML**: Optimized for parallel execution and GPU acceleration.
3. **Distributed & Cloud Computing**: Native support for scalable microservices.
4. **Scientific & Critical Computing**: High-reliability environments like space exploration and defense.

---

Miri is designed for the future of computing.
