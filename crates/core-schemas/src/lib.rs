//! Synchronized schemas used by the `holaplex-hub-core` crate.

#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    clippy::clone_on_ref_ptr,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]

#[allow(
    clippy::doc_markdown,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    missing_copy_implementations,
    missing_docs
)]
mod generated {
    pub mod credits {
        include!(concat!(env!("OUT_DIR"), "/credits.rs"));
    }
    pub mod credits_mpsc {
        include!(concat!(env!("OUT_DIR"), "/credits_mpsc.rs"));
    }
}

pub use generated::*;
