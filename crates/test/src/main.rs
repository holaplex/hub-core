//! Debug binary for the hub core crate

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
    include!(concat!(env!("OUT_DIR"), "/test.proto.rs"));
}

#[derive(Debug, clap::Args)]
struct Args;

#[derive(Debug)]
struct MyMessage;

impl hub_core::producer::Message for MyMessage {
    type Key = str;
    type Value = str;
}

#[derive(Debug, hub_core::thiserror::Error)]
#[error("foo")]
struct MyError;

#[derive(Debug)]
struct MyGroup;

impl hub_core::consumer::MessageGroup for MyGroup {
    const REQUESTED_TOPICS: &'static [&'static str] = &["foo-bar"];

    type Error = MyError;

    fn from_message<M: hub_core::consumer::Message>(msg: &M) -> Result<Self, Self::Error> {
        let topic = msg.topic();
        let key = msg.key();
        let val = msg.payload();
        info!(topic, ?key, ?val);

        Ok(MyGroup)
    }
}

fn main() {
    let opts = hub_core::StartConfig {
        service_name: "foo-bar",
    };

    hub_core::run(opts, |common, Args| {
        let test = proto::Test { x: "hi".into() };

        info!(test = ?test, "hello!");

        info!(test_encoded = ?test.encode_to_vec(), "encoded");

        common.rt.block_on(async move {
            let prod = common.producer_cfg.build::<MyMessage>().await?;
            let cons = common.consumer_cfg.build(MyGroup).await?;

            prod.send(Some("hi"), Some("hi")).await?;

            info!("Testing consumer");
            let msg = cons.stream().next().await;
            info!(?msg, "message received");

            Ok(())
        })
    });
}
