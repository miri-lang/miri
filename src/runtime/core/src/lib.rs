//! Miri Runtime Core
//!
//! Provides fundamental runtime services for compiled Miri programs:
//! - [`alloc`] — Memory allocation primitives wrapping the system allocator.
//! - [`array`] — Fixed-size array type ([`MiriArray`]) with FFI interface.
//! - [`io`] — Standard I/O operations (print, println, eprint, eprintln).
//! - [`list`] — Dynamic list type ([`MiriList`]) with FFI interface.
//! - [`string`] — UTF-8 string type ([`MiriString`]) with full FFI interface.
//!
//! All public functions use `#[no_mangle] extern "C"` for C-compatible FFI,
//! allowing compiled Miri code to call them via the linker.

pub mod alloc;
pub mod array;
pub mod io;
pub mod list;
pub mod map;
pub mod rc;
pub mod set;
pub mod string;
pub mod time;

pub use alloc::*;
pub use array::*;
pub use io::*;
pub use list::*;
pub use map::*;
pub use rc::*;
pub use set::*;
pub use string::*;
pub use time::*;
