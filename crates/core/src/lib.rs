//! Core logic for the Holaplex Hub crates

#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]

pub extern crate anyhow;
pub extern crate async_trait;
pub extern crate clap;
pub extern crate futures_util;
pub extern crate prost;
pub extern crate prost_types;
pub extern crate tokio;
pub extern crate tracing;
pub extern crate url;

pub use runtime::*;

/// Common utilities for all crates
pub mod prelude {
    pub use std::{
        borrow::{
            Borrow, Cow,
            Cow::{Borrowed, Owned},
        },
        future::Future,
        sync::Arc,
    };

    pub use anyhow::{anyhow, bail, ensure, Context as _, Error};
    pub use async_trait::async_trait;
    pub use futures_util::{FutureExt, StreamExt, TryFutureExt};
    pub use prost::Message;
    pub use tracing::{
        self, debug, debug_span, error, error_span, info, info_span, instrument, trace, trace_span,
        warn, warn_span,
    };
    pub use tracing_subscriber::prelude::*;
    pub use url::Url;

    /// Result helper that defaults to [`anyhow::Error`]
    pub type Result<T, E = Error> = std::result::Result<T, E>;
}

mod runtime {
    use std::{
        fmt,
        path::{Path, PathBuf},
    };

    use tracing_subscriber::{layer::Layered, EnvFilter};

    use crate::prelude::*;

    #[derive(Debug, clap::Args)]
    struct CommonArgs<T: clap::Args> {
        /// The capacity of the async thread pool
        #[arg(short, env)]
        jobs: Option<usize>,

        /// The Kafka broker list
        #[cfg(feature = "kafka")]
        #[arg(long, env)]
        kafka_brokers: String,

        /// Kafka SASL username
        #[cfg(feature = "kafka")]
        #[arg(long, env, requires("kafka_password"))]
        kafka_username: Option<String>,

        /// Kafka SASL password
        #[cfg(feature = "kafka")]
        #[arg(long, env, requires("kafka_username"))]
        kafka_password: Option<DebugShim<String>>, // hide

        /// Whether to use SSL Kafka channels
        #[cfg(feature = "kafka")]
        #[arg(long, env, default_value_t = true)]
        kafka_ssl: bool,

        #[command(flatten)]
        extra: T,
    }

    #[derive(Clone, Copy)]
    #[repr(transparent)]
    struct DebugShim<T>(T);

    impl<T> fmt::Debug for DebugShim<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct(std::any::type_name::<T>())
                .finish_non_exhaustive()
        }
    }

    impl<T> From<T> for DebugShim<T> {
        fn from(val: T) -> Self {
            Self(val)
        }
    }

    /// Common data passed into the program entry point
    #[allow(missing_copy_implementations)]
    #[non_exhaustive]
    #[derive(Debug)]
    pub struct Common {
        rt: tokio::runtime::Runtime,

        #[cfg(feature = "kafka")]
        producer: DebugShim<rdkafka::producer::FutureProducer>,
        #[cfg(feature = "kafka")]
        consumer: DebugShim<rdkafka::consumer::StreamConsumer>,
    }

    impl Common {
        #[instrument(name = "init_runtime", skip(loki_task))]
        fn new<T: fmt::Debug + clap::Args>(
            args: CommonArgs<T>,
            loki_task: Option<tracing_loki::BackgroundTask>,
        ) -> Result<(Self, T)> {
            let CommonArgs {
                jobs,
                #[cfg(feature = "kafka")]
                kafka_brokers,
                #[cfg(feature = "kafka")]
                kafka_username,
                #[cfg(feature = "kafka")]
                kafka_password,
                #[cfg(feature = "kafka")]
                kafka_ssl,
                extra,
            } = args;

            let jobs = jobs.unwrap_or_else(num_cpus::get);

            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(jobs)
                .max_blocking_threads(jobs)
                .build()
                .context("Failed to construct Tokio runtime")?;

            if let Some(loki_task) = loki_task {
                rt.spawn(async move {
                    loki_task.await;
                    error!("Loki exporter task quit unexpectedly!");
                });
            }

            #[cfg(feature = "kafka")]
            let (producer, consumer) = rt.block_on(async {
                use rdkafka::consumer::Consumer;

                let mut config = rdkafka::ClientConfig::new();
                config.set("bootstrap.servers", kafka_brokers);

                if let Some((user, pass)) = kafka_username.zip(kafka_password) {
                    config
                        .set("sasl.mechanism", "SCRAM-SHA-512")
                        .set("sasl.username", user)
                        .set("sasl.password", pass.0)
                        .set(
                            "security.protocol",
                            if kafka_ssl {
                                "SASL_SSL"
                            } else {
                                "SASL_PLAINTEXT"
                            },
                        );
                } else {
                    config.set(
                        "security.protocol",
                        if kafka_ssl { "SSL" } else { "PLAINTEXT" },
                    );
                }
                let config = config; // no more mut

                let admin: rdkafka::admin::AdminClient<_> = config
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

                let producer: rdkafka::producer::FutureProducer =
                    config.create().context("Failed to create Kafka producer")?;
                let consumer: rdkafka::consumer::StreamConsumer = config
                    .clone()
                    .set("group.id", "foo")
                    .create()
                    .context("Failed to create Kafka consumer")?;

                consumer
                    .subscribe(&["fucc"])
                    .context("Failed to subscribe consumer to test topic")?;

                producer
                    .send(
                        rdkafka::producer::FutureRecord {
                            topic: "fucc",
                            partition: None,
                            payload: Some("fucc"),
                            key: Some("fucc"),
                            timestamp: None,
                            headers: None,
                        },
                        None,
                    )
                    .await
                    .map_err(|(e, m)| e)
                    .context("Failed to send test message")?;

                Result::<_>::Ok((DebugShim(producer), DebugShim(consumer)))
            })?;

            Ok((
                Self {
                    rt,
                    #[cfg(feature = "kafka")]
                    producer,
                    #[cfg(feature = "kafka")]
                    consumer,
                },
                extra,
            ))
        }

        /// Expose the Tokio async runtime
        pub fn rt(&self) -> &tokio::runtime::Runtime {
            &self.rt
        }
    }

    fn dotenv(name: impl AsRef<Path>) -> Result<Option<PathBuf>, dotenv::Error> {
        match dotenv::from_filename(name) {
            Ok(p) => Ok(Some(p)),
            Err(dotenv::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    #[derive(Debug, clap::Parser)]
    struct Opts<T: fmt::Debug + clap::Args> {
        /// The log filter, using env_logger-like syntax
        #[arg(long, env = "RUST_LOG")]
        log_filter: Option<String>,

        /// Endpoint to use for exporting logs to Grafana Loki
        #[arg(long, env)]
        loki_endpoint: Option<Url>,

        #[command(flatten)]
        common: CommonArgs<T>,
    }

    macro_rules! init_error {
        ($($args:tt)*) => ({
            ::tracing::error!($($args)*);
            ::std::process::exit(1);
        })
    }

    fn fmt_layer<S>() -> tracing_subscriber::fmt::Layer<S> {
        tracing_subscriber::fmt::layer()
    }

    #[instrument(name = "bootstrap_logger", skip(log_filter, f))]
    fn init_subscriber<
        S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    >(
        log_filter: impl AsRef<str>,
        f: impl FnOnce(Layered<EnvFilter, tracing_subscriber::Registry>) -> S,
    ) where
        Layered<tracing_subscriber::fmt::Layer<S>, S>: Into<tracing::Dispatch>,
    {
        let log_filter = log_filter.as_ref();
        let reg = tracing_subscriber::Registry::default().with(
            EnvFilter::try_new(log_filter).unwrap_or_else(|e| {
                init_error!("Invalid log filter {log_filter:?}: {e}");
            }),
        );

        f(reg)
            .with(fmt_layer())
            .try_init()
            .unwrap_or_else(|e| init_error!("Failed to set tracing subscriber: {e}"));
    }

    /// Perform environment setup and run the requested entrypoint
    pub fn run<T: fmt::Debug + clap::Args>(main: impl FnOnce(Common, T) -> Result<(), ()>) {
        // Construct a temporary logger on this thread until the full logger is ready
        let smuggled = tracing::subscriber::with_default(
            tracing_subscriber::Registry::default().with(fmt_layer()),
            || {
                let span = error_span!("boot").entered();

                [
                    ".env.local",
                    if cfg!(debug_assertions) {
                        ".env.dev"
                    } else {
                        ".env.prod"
                    },
                    ".env",
                ]
                .into_iter()
                .try_for_each(|p| {
                    dotenv(p)
                        .map(|_| ())
                        .with_context(|| format!("Failed to load .env file {p:?}"))
                })
                .unwrap_or_else(|e| init_error!("Failed to load .env files: {e:?}"));

                let opts = clap::Parser::parse();
                std::mem::drop(span);
                let span = error_span!("boot", ?opts).entered();
                let Opts {
                    log_filter,
                    loki_endpoint,
                    common,
                } = opts;

                let log_filter = log_filter.map_or_else(
                    || {
                        Borrowed(if cfg!(debug_assertions) {
                            "debug"
                        } else {
                            "info"
                        })
                    },
                    Owned,
                );

                let (loki_layer, loki_task) = loki_endpoint
                    .map(|e| {
                        tracing_loki::layer(e, [].into_iter().collect(), [].into_iter().collect())
                    })
                    .transpose()
                    .unwrap_or_else(|e| init_error!("Failed to initialize Loki exporter: {e}"))
                    .unzip();

                if let Some(loki_layer) = loki_layer {
                    init_subscriber(log_filter, |r| r.with(loki_layer));
                } else {
                    init_subscriber(log_filter, |r| r);
                }

                std::mem::drop(span);

                (common, loki_task)
            },
        );

        let (common, loki_task) = smuggled;

        error_span!("run").in_scope(|| {
            let (common, extra) = match Common::new(common, loki_task) {
                Ok(t) => t,
                Err(e) => {
                    error!("Failed to initialize runtime: {e:?}");
                    std::process::exit(-1);
                },
            };

            std::process::exit(match main(common, extra) {
                Ok(()) => 0,
                Err(e) => {
                    error!("{e:?}");
                    1
                },
            });
        });
    }
}
