//! A Kafka record consumer

use std::fmt;

use futures_util::Stream;
use rdkafka::consumer::{Consumer as _, StreamConsumer};
pub use rdkafka::Message;

use crate::{prelude::*, util::DebugShim};

/// Service startup configuration for consuming Kafka records
#[derive(Debug)]
pub struct Config {
    pub(crate) service_name: String,
    pub(crate) config: DebugShim<rdkafka::ClientConfig>,
}

impl Config {
    /// Construct a new record consumer from this config instance
    ///
    /// # Errors
    /// This function returns an error if the Kafka consumer cannot be created
    /// or if subscribing to the requested topics fails.
    #[inline]
    pub async fn build<G: MessageGroup>(self) -> Result<Consumer<G>> {
        Consumer::new(self).await
    }
}

/// A consumer for requesting, receiving, and parsing messages from one or more
/// Kafka topics
#[derive(Debug)]
pub struct Consumer<G> {
    consumer: DebugShim<StreamConsumer>,
    group: PhantomData<fn() -> ConsumerStream<'static, G>>,
}

impl<G: MessageGroup> Consumer<G> {
    #[instrument(name = "build_consumer")]
    pub(crate) async fn new(mut config: Config) -> Result<Self> {
        let consumer: StreamConsumer = config
            .config
            .0
            .set(
                "group.id",
                format!("{}@{}", std::any::type_name::<G>(), config.service_name),
            )
            .create()
            .context("Failed to create Kafka consumer")?;

        // TODO: backoff and retry for initial boot
        consumer
            .subscribe(G::REQUESTED_TOPICS)
            .context("Failed to subscribe consumer to requested topics")?;

        Ok(Self {
            consumer: DebugShim(consumer),
            group: PhantomData::default(),
        })
    }

    /// Open a stream to receive messages from this consumer
    #[must_use]
    pub fn stream(&self) -> ConsumerStream<G> {
        ConsumerStream {
            stream: self.consumer.0.stream(),
            group: PhantomData::default(),
        }
    }
}

pin_project_lite::pin_project! {
    /// A stream of incoming messages for a consumer, parsed according to the
    /// type of the [`MessageGroup`] the consumer was constructed with
    pub struct ConsumerStream<'a, G> {
        #[pin]
        stream: rdkafka::consumer::MessageStream<'a>,
        group: PhantomData<fn() -> G>,
    }
}

impl<'a, G: MessageGroup> Stream for ConsumerStream<'a, G> {
    type Item = Result<G, RecvError>;

    #[inline]
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.project().stream.poll_next(cx).map(|o| {
            o.map(|r| {
                r.map_err(RecvError::Kafka)
                    .and_then(|m| G::from_message(&m))
            })
        })
    }
}

/// An error originating from a received Kafka record
#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    /// An error occurred reading data from the wire
    #[error("Error receiving messages from Kafka: {0}")]
    Kafka(#[from] rdkafka::error::KafkaError),
    /// An error occurred parsing data from the wire
    #[error("Error decoding Protobuf message")]
    Protobuf(#[from] prost::DecodeError),
    /// The topic of a message did not match one of the expected topics of the
    /// message group
    #[error("Unexpected topic {0:?}")]
    BadTopic(String),
    /// A message had no key but the message group expected one
    #[error("Expected a message key, but did not get one")]
    MissingKey,
    /// A message had no payload but the message group expected one
    #[error("Expected a message payload, but did not get one")]
    MissingPayload,
}

/// Parsing logic for incoming messages from multiple Kafka topics
pub trait MessageGroup: fmt::Debug + Sized {
    /// The topics this message group is interested in consuming
    const REQUESTED_TOPICS: &'static [&'static str];

    /// Construct a new member of this message group from an inbound Kafka
    /// record
    ///
    /// # Errors
    /// This function should return an error if the bytes of the message cannot
    /// be decoded according to the topic it was received from, if the topic is
    /// not listed in [`REQUESTED_TOPICS`](Self::REQUESTED_TOPICS), or if the
    /// message is missing required fields.
    fn from_message<M: Message>(msg: &M) -> Result<Self, RecvError>;
}
