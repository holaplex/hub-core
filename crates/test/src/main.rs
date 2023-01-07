#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]

use hub_core::{clap, prelude::*};

mod proto {
//    include!(concat!(env!("OUT_DIR"), "/test.proto.rs"));
}

#[derive(Debug, clap::Args)]
struct Args;

fn main() {
    hub_core::run(|c, Args| {
        /*
        let test = proto::Test { x: "hi".into() };

        info!(test = ?test, "hello!");

        info!(test_encoded = ?test.encode_to_vec(), "encoded");
        */
        Ok(())
    });
}
