//! A Kafka record producer

use std::fmt;

use crate::{prelude::*, util::DebugShim};

/// Service startup configuration for producing Kafka records
#[derive(Debug, Clone)]
pub struct Config {
    pub(crate) topic: String,
    pub(crate) config: DebugShim<rdkafka::ClientConfig>,
}

impl Config {
    /// Construct a new record producer from this config instance
    ///
    /// # Errors
    /// This function returns an error if a Kafka topic with the service's name
    /// cannot be created or a Kafka client cannot successfully be initialized.
    #[inline]
    pub async fn build<M: Message>(self) -> Result<Producer<M>> {
        Producer::new(self).await
    }
}

/// A producer for emitting messages onto the Kafka topic identified by this
/// service's name
#[derive(Debug, Clone)]
pub struct Producer<M> {
    topic: String,
    producer: DebugShim<rdkafka::producer::FutureProducer>,
    msg: PhantomData<fn(&M)>,
}

impl<M: Message> Producer<M> {
    #[instrument(name = "build_producer")]
    pub(crate) async fn new(config: Config) -> Result<Self> {
        let admin: rdkafka::admin::AdminClient<_> = config
            .config
            .0
            .create()
            .context("Failed to create Kafka admin client")?;

        admin
            .create_topics(
                &[rdkafka::admin::NewTopic {
                    name: &config.topic,
                    config: vec![],
                    num_partitions: 1,
                    replication: rdkafka::admin::TopicReplication::Fixed(1),
                }],
                &rdkafka::admin::AdminOptions::new(),
            )
            .await
            .context("Failed to create test topic")?;

        let producer = config
            .config
            .0
            .create()
            .context("Failed to create Kafka producer")?;

        Ok(Self {
            topic: config.topic,
            producer: DebugShim(producer),
            msg: PhantomData::default(),
        })
    }

    /// Send a single record to the Kafka broker
    #[instrument(level = "debug")]
    pub async fn send(
        &self,
        payload: Option<&M>, // TODO: don't wrap this in Option
        key: Option<&M::Key>,
    ) -> Result<(), SendError> {
        match self
            .producer
            .0
            .send(
                rdkafka::producer::FutureRecord {
                    topic: &self.topic,
                    partition: None,
                    payload: payload.map(prost::Message::encode_to_vec).as_deref(),
                    key: key.map(prost::Message::encode_to_vec).as_deref(),
                    timestamp: None,
                    headers: None,
                },
                None,
            )
            .await
        {
            Ok((partition, offset)) => trace!(partition, offset, "Message delivered"),
            Err((err, msg)) => {
                error!(%err, ?msg, "Failed to send message");
                return Err(SendError(err));
            },
        }

        Ok(())
    }
}

/// An error originating from an outgoing Kafka record
#[derive(Debug, thiserror::Error, Triage)]
#[error("Error sending message to Kafka: {0}")]
pub struct SendError(#[source] rdkafka::error::KafkaError);

/// A Protobuf message payload with an associated Protobuf key
pub trait Message: fmt::Debug + prost::Message {
    /// The key type for this message
    type Key: fmt::Debug + prost::Message;
}
