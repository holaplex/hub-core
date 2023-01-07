use std::fmt;

use futures_util::Stream;
use rdkafka::consumer::{Consumer as _, StreamConsumer};
pub use rdkafka::Message;

use crate::{prelude::*, util::DebugShim};

#[derive(Debug)]
pub struct Config {
    pub(crate) service_name: String,
    pub(crate) config: DebugShim<rdkafka::ClientConfig>,
}

impl Config {
    #[inline]
    pub async fn build<G: MessageGroup>(self, group: G) -> Result<Consumer<G>> {
        Consumer::new(self).await
    }
}

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

        consumer
            .subscribe(&[&config.service_name])
            .context("Failed to subscribe consumer to test topic")?;

        Ok(Self {
            consumer: DebugShim(consumer),
            group: PhantomData::default(),
        })
    }

    pub fn stream(&self) -> ConsumerStream<G> {
        ConsumerStream {
            stream: self.consumer.0.stream(),
            group: PhantomData::default(),
        }
    }
}

pin_project_lite::pin_project! {
    pub struct ConsumerStream<'a, G> {
        #[pin]
        stream: rdkafka::consumer::MessageStream<'a>,
        group: PhantomData<fn() -> G>,
    }
}

impl<'a, G: MessageGroup> Stream for ConsumerStream<'a, G> {
    type Item = Result<G, RecvError<G>>;

    #[inline]
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.project().stream.poll_next(cx).map(|o| {
            o.map(|r| {
                r.map_err(RecvError::Kafka)
                    .and_then(|m| G::from_message(&m).map_err(RecvError::Parse))
            })
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RecvError<G: MessageGroup> {
    #[error("Error receiving messages from Kafka: {0}")]
    Kafka(#[source] rdkafka::error::KafkaError),
    #[error("Error parsing received message: {0}")]
    Parse(#[source] G::Error),
}

pub trait MessageGroup: fmt::Debug + Sized {
    type Error: std::error::Error + Send + Sync + 'static;

    const REQUESTED_TOPICS: &'static [&'static str];

    fn from_message<M: Message>(msg: &M) -> Result<Self, Self::Error>;
}
