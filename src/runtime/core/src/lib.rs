//! Miri Runtime Core
//!
//! Provides fundamental memory management, string handling, and collections for the Miri language.

pub mod alloc;
pub mod io;
pub mod string;

pub use alloc::*;
pub use io::*;
pub use string::*;
