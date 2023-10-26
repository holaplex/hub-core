//! A Kafka record producer

use std::{
    fmt,
    sync::atomic::{AtomicI64, AtomicUsize, Ordering},
};

use rand::Rng;
use rdkafka::producer::Producer as _;

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

#[derive(Debug)]
struct Shared {
    partition_count: AtomicUsize,
    last_partition_check: AtomicI64,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            partition_count: 1.into(),
            last_partition_check: i64::MIN.into(),
        }
    }
}

/// A producer for emitting messages onto the Kafka topic identified by this
/// service's name
#[derive(Debug, Clone)]
pub struct Producer<M> {
    topic: String,
    shared: Arc<Shared>,
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
            shared: Shared::default().into(),
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
        const INTERVAL: i64 = 5 * 60;

        let now = chrono::Local::now().timestamp();
        let parts = if self
            .shared
            .last_partition_check
            .fetch_update(Ordering::Release, Ordering::Acquire, |t| {
                (t < (now - INTERVAL)).then_some(now)
            })
            .is_ok()
        {
            match self
                .producer
                .0
                .client()
                .fetch_metadata(Some(&self.topic), Duration::from_secs(5))
            {
                Ok(m) => {
                    let topic = m.topics().iter().find(|t| t.name() == self.topic).unwrap();
                    let parts = topic.partitions().len();
                    self.shared.partition_count.store(parts, Ordering::Release);
                    Some(parts)
                },
                Err(e) => {
                    warn!("Updating the partition count failed: {e}");
                    None
                },
            }
        } else {
            None
        };
        let parts = parts.unwrap_or_else(|| self.shared.partition_count.load(Ordering::Relaxed));

        let part = rand::thread_rng()
            .gen_range(0..parts)
            .try_into()
            .unwrap_or(0);

        match self
            .producer
            .0
            .send(
                rdkafka::producer::FutureRecord {
                    topic: &self.topic,
                    partition: Some(part),
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

#[cfg(test)]
mod tests {
    #[derive(Debug)]
    struct Msg;

    impl prost::Message for Msg {
        fn encode_raw<B>(&self, _: &mut B)
        where
            B: prost::bytes::BufMut,
            Self: Sized,
        {
            todo!()
        }

        fn merge_field<B>(
            &mut self,
            _: u32,
            _: prost::encoding::WireType,
            _: &mut B,
            _: prost::encoding::DecodeContext,
        ) -> Result<(), prost::DecodeError>
        where
            B: prost::bytes::Buf,
            Self: Sized,
        {
            todo!()
        }

        fn encoded_len(&self) -> usize {
            todo!()
        }

        fn clear(&mut self) {
            todo!()
        }
    }

    impl super::Message for Msg {
        type Key = ();
    }

    fn assert_send(_: impl Send) {}

    #[should_panic]
    #[allow(unreachable_code)]
    fn test_send_has_send() {
        let _p: super::Producer<Msg> = todo!();
        assert_send(_p.send(None, None));
    }
}
