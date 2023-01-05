//! Core build script tools for holaplex-hub

#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]

pub fn run(p: impl AsRef<std::path::Path>) -> Result<(), std::io::Error> {
    prost_build::compile_protos(&[p], &[] as &[&'static str])
}
