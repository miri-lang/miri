//! Miri Runtime Core
//!
//! Provides fundamental runtime services for compiled Miri programs:
//! - [`alloc`] — Memory allocation primitives wrapping the system allocator.
//! - [`io`] — Standard I/O operations (print, println, eprint, eprintln).
//! - [`string`] — UTF-8 string type ([`MiriString`]) with full FFI interface.
//!
//! All public functions use `#[no_mangle] extern "C"` for C-compatible FFI,
//! allowing compiled Miri code to call them via the linker.

pub mod alloc;
pub mod io;
pub mod string;

pub use alloc::*;
pub use io::*;
pub use string::*;
