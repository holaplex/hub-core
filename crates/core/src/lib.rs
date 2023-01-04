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
pub extern crate clap;
pub extern crate futures_util;
#[cfg(feature = "rt")]
pub extern crate tokio;

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
    pub use tracing::{
        debug, debug_span, error, error_span, info, info_span, instrument, trace, trace_span, warn,
        warn_span,
    };
    pub use tracing_subscriber::prelude::*;

    /// Result helper that defaults to [`anyhow::Error`]
    pub type Result<T, E = Error> = std::result::Result<T, E>;
}

mod runtime {
    use std::{
        fmt,
        path::{Path, PathBuf},
    };

    use tracing_subscriber::EnvFilter;

    use crate::prelude::*;

    #[derive(Debug, clap::Args)]
    struct CommonArgs<T: clap::Args> {
        /// The capacity of the async thread pool
        #[cfg(feature = "rt")]
        #[clap(short, env)]
        jobs: Option<usize>,

        /// The Kafka broker list
        #[cfg(feature = "kafka")]
        #[clap(long, env)]
        kafka_brokers: String,

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

    /// Common data passed into the program entry point
    #[allow(missing_copy_implementations)]
    #[non_exhaustive]
    #[derive(Debug)]
    pub struct Common {
        #[cfg(feature = "rt")]
        rt: tokio::runtime::Runtime,

        #[cfg(feature = "kafka")]
        producer: DebugShim<rdkafka::producer::FutureProducer>,
        #[cfg(feature = "kafka")]
        consumer: DebugShim<rdkafka::consumer::StreamConsumer>,
    }

    impl Common {
        #[instrument(name = "init_runtime")]
        fn new<T: fmt::Debug + clap::Args>(args: CommonArgs<T>) -> Result<(Self, T)> {
            let CommonArgs {
                #[cfg(feature = "rt")]
                jobs,
                #[cfg(feature = "kafka")]
                kafka_brokers,
                extra,
            } = args;

            #[cfg(feature = "rt")]
            let jobs = jobs.unwrap_or_else(num_cpus::get);

            #[cfg(feature = "rt")]
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(jobs)
                .max_blocking_threads(jobs)
                .build()
                .context("Failed to construct Tokio runtime")?;

            #[cfg(feature = "kafka")]
            let (producer, consumer) = rt.block_on(async {
                use rdkafka::consumer::Consumer;

                let mut config = rdkafka::ClientConfig::new();
                config.set("bootstrap.servers", kafka_brokers);
                let config = config; // no more mut

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
                    #[cfg(feature = "rt")]
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
        #[cfg(feature = "tokio")]
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

        #[command(flatten)]
        common: CommonArgs<T>,
    }

    /// Perform environment setup and run the requested entrypoint
    #[instrument(skip(main))]
    pub fn run<T: fmt::Debug + clap::Args>(main: impl FnOnce(Common, T) -> Result<(), ()>) {
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
        .expect("Failed to load .env files");

        let Opts { log_filter, common } = clap::Parser::parse();

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

        tracing_log::LogTracer::init().expect("Failed to initialize LogTracer");
        tracing::subscriber::set_global_default(
            tracing_subscriber::Registry::default()
                .with(EnvFilter::try_new(&log_filter).unwrap_or_else(|e| {
                    eprintln!("Invalid log filter {log_filter:?}: {e}");
                    std::process::exit(1);
                }))
                .with(tracing_subscriber::fmt::layer()),
        )
        .expect("Failed to set default tracing subscriber");

        let (common, extra) = match Common::new(common) {
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
    }
}
