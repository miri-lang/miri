//! Miri Runtime Core
//!
//! Provides fundamental runtime services for compiled Miri programs:
//! - [`alloc`] — Memory allocation primitives wrapping the system allocator.
//! - [`array`] — Fixed-size array type ([`MiriArray`]) with FFI interface.
//! - [`io`] — Standard I/O operations (print, println, eprint, eprintln).
//! - [`list`] — Dynamic list type ([`MiriList`]) with FFI interface.
//! - [`map`] — Hash map type ([`MiriMap`]) with FFI interface.
//! - [`set`] — Hash set type ([`MiriSet`]) with FFI interface.
//! - [`string`] — UTF-8 string type ([`MiriString`]) with full FFI interface.
//! - [`time`] — Time utilities.
//! - [`tuple`] — Tuple length helper.
//!
//! All public FFI functions use `#[no_mangle] extern "C"` for C-compatible FFI,
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
pub mod tuple;

// Internal RC helpers (used by other runtime modules)
pub use rc::*;

// Struct types accessible at crate root (needed by module-internal tests and cross-module code)
pub use array::MiriArray;
pub use list::MiriList;
pub use map::MiriMap;
pub use set::MiriSet;
pub use string::MiriString;

// Stable FFI interface — all miri_rt_* and miri_alloc* symbols at crate root.
// This preserves `crate::miri_rt_list_new()` style calls used in array.rs and tests.
pub use alloc::ffi::*;
pub use array::ffi::*;
pub use io::ffi::*;
pub use list::ffi::*;
pub use map::ffi::*;
pub use set::ffi::*;
pub use string::ffi::*;
pub use time::ffi::*;
pub use tuple::ffi::*;
