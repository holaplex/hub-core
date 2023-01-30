//! Debug binary for the hub core crate

#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]

use hub_core::{clap, consumer::RecvError, prelude::*};

mod proto {
    include!(concat!(env!("OUT_DIR"), "/test.proto.rs"));
}

#[derive(Debug, clap::Args)]
struct Args;

impl hub_core::producer::Message for proto::Test {
    type Key = proto::Test;
}

#[derive(Debug)]
enum MyGroup {
    Test(proto::Test, proto::Test),
}

impl hub_core::consumer::MessageGroup for MyGroup {
    const REQUESTED_TOPICS: &'static [&'static str] = &["foo-bar"];

    fn from_message<M: hub_core::consumer::Message>(msg: &M) -> Result<Self, RecvError> {
        let topic = msg.topic();
        let key = msg.key().ok_or(RecvError::MissingKey)?;
        let val = msg.payload().ok_or(RecvError::MissingPayload)?;
        info!(topic, ?key, ?val);

        match topic {
            "foo-bar" => {
                let key = proto::Test::decode(key)?;
                let val = proto::Test::decode(val)?;

                Ok(MyGroup::Test(key, val))
            },
            t => Err(RecvError::BadTopic(t.into())),
        }
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
            let prod = common.producer_cfg.build::<proto::Test>().await?;
            let cons = common.consumer_cfg.build::<MyGroup>().await?;

            let test = proto::Test { x: "hi".into() };

            prod.send(Some(&test), Some(&test)).await?;

            info!("Testing consumer");
            let msg = cons.stream().next().await;
            info!(?msg, "message received");

            Ok(())
        })
    });
}
