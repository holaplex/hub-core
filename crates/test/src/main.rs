#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]

use core::clap;

#[derive(clap::Args)]
struct Args;

fn main() {
    core::run(|c, Args| Ok(()));
}
