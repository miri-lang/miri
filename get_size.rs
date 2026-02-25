use std::mem::size_of;
mod src {
    pub mod lexer {
        pub mod token;
        pub mod error; // wait, token imports crate::error
    }
}
