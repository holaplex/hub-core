#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]

#[allow(missing_copy_implementations)]
mod generated {
    pub mod credits {
        include!(concat!(env!("OUT_DIR"), "/credits.rs"));
    }
    pub mod credits_mpsc {
        include!(concat!(env!("OUT_DIR"), "/credits_mpsc.rs"));
    }
}

pub use generated::*;
