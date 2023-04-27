#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]

// include!(concat!(env!("OUT_DIR"), "/creditsmpsc.proto.rs"));

#[derive(prost::Message, Clone, Copy)]
pub struct CreditsEventKey {} // TODO: stub

#[derive(prost::Message, Clone, Copy)]
pub struct CreditsEvent {} // TODO: stub
