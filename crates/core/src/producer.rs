use std::fmt;

use rdkafka::message::ToBytes;

use crate::{prelude::*, util::DebugShim};

#[derive(Debug)]
pub struct Config {
    pub(crate) service_name: String,
    pub(crate) config: DebugShim<rdkafka::ClientConfig>,
}

impl Config {
    #[inline]
    pub async fn build<M: Message>(self) -> Result<Producer<M>> {
        Producer::new(self).await
    }
}

#[derive(Debug)]
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
                    name: "hi",
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
            topic: config.service_name,
            producer: DebugShim(producer),
            msg: PhantomData::default(),
        })
    }

    #[instrument(level = "debug")]
    pub async fn send(
        &self,
        payload: Option<&M::Value>,
        key: Option<&M::Key>,
    ) -> Result<(), SendError> {
        match self
            .producer
            .0
            .send(
                rdkafka::producer::FutureRecord {
                    topic: &self.topic,
                    partition: None,
                    payload,
                    key,
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

#[derive(Debug, thiserror::Error)]
#[error("Error sending message to Kafka: {0}")]
pub struct SendError(#[source] rdkafka::error::KafkaError);

pub trait Message: fmt::Debug {
    type Key: fmt::Debug + ToBytes + ?Sized;
    type Value: fmt::Debug + ToBytes + ?Sized;
}
