//! Core logic for the Holaplex Hub crates

#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    clippy::clone_on_ref_ptr,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]
#![allow(clippy::module_name_repetitions)]

pub extern crate anyhow;
pub extern crate async_trait;
pub extern crate backon;
pub extern crate chrono;
pub extern crate clap;
pub extern crate futures_util;
pub extern crate hostname;
pub extern crate prost;
pub extern crate prost_types;
pub extern crate reqwest;
pub extern crate serde_json;
pub extern crate serde_with;
pub extern crate thiserror;
pub extern crate tokio;
pub extern crate tracing;
pub extern crate url;
pub extern crate uuid;

pub use runtime::*;

/// Common utilities for all crates
pub mod prelude {
    pub use std::{
        borrow::{
            Borrow, Cow,
            Cow::{Borrowed, Owned},
        },
        fmt,
        future::Future,
        marker::PhantomData,
        mem,
        str::FromStr,
        sync::Arc,
        time::Duration,
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

#[cfg(feature = "asset_proxy")]
pub mod assets;
#[cfg(feature = "kafka")]
pub mod consumer;
#[cfg(feature = "credits")]
pub mod credits;
#[cfg(feature = "kafka_internal")]
pub mod producer;
pub mod triage;
pub mod util;

mod runtime {
    use std::{
        fmt,
        path::{Path, PathBuf},
    };

    use tracing_subscriber::{layer::Layered, EnvFilter};

    use crate::{prelude::*, util::DebugShim};

    #[derive(Debug, clap::Args)]
    struct CommonArgs<T: clap::Args> {
        /// The capacity of the async thread pool
        #[arg(short, env)]
        jobs: Option<usize>,

        /// The Kafka broker list
        #[cfg(feature = "kafka_internal")]
        #[arg(long, env)]
        kafka_brokers: String,

        /// Kafka SASL username
        #[cfg(feature = "kafka_internal")]
        #[arg(long, env, requires("kafka_password"))]
        kafka_username: Option<String>,

        /// Kafka SASL password
        #[cfg(feature = "kafka_internal")]
        #[arg(long, env, requires("kafka_username"))]
        kafka_password: Option<DebugShim<String>>, // hide

        /// Whether to use SSL Kafka channels
        #[cfg(feature = "kafka_internal")]
        #[arg(long, env, default_value_t = true)]
        kafka_ssl: bool,

        /// Path to the credit price sheet TOML configuration file
        #[cfg(feature = "credits")]
        #[arg(long, env)]
        credit_sheet: PathBuf,

        #[cfg(feature = "asset_proxy")]
        #[arg(long, env)]
        asset_cdn: String,

        #[command(flatten)]
        extra: T,
    }

    /// Common data passed into the program entry point
    #[allow(missing_copy_implementations)]
    #[non_exhaustive]
    #[derive(Debug)]
    pub struct Common {
        /// A Tokio runtime for use with async tasks
        pub rt: tokio::runtime::Runtime,

        #[cfg(feature = "kafka")]
        /// Configuration for creating a Kafka message producer for this service
        pub producer_cfg: super::producer::Config,

        #[cfg(feature = "kafka")]
        /// Configuration for creating a Kafka message consumer for this service
        pub consumer_cfg: super::consumer::Config,

        #[cfg(feature = "credits")]
        /// Configuration for consuming and producing credit costs
        pub credits_cfg: super::credits::Config,

        #[cfg(feature = "asset_proxy")]
        /// An IPFS asset proxy
        pub asset_proxy: super::assets::AssetProxy,
    }

    impl Common {
        #[instrument(name = "init_runtime", skip(loki_task))]
        fn new<T: fmt::Debug + clap::Args>(
            cfg: StartConfig,
            args: CommonArgs<T>,
            loki_task: Option<tracing_loki::BackgroundTask>,
        ) -> Result<(Self, T)> {
            let StartConfig { service_name } = cfg;
            let CommonArgs {
                jobs,
                #[cfg(feature = "kafka_internal")]
                kafka_brokers,
                #[cfg(feature = "kafka_internal")]
                kafka_username,
                #[cfg(feature = "kafka_internal")]
                kafka_password,
                #[cfg(feature = "kafka_internal")]
                kafka_ssl,
                #[cfg(feature = "credits")]
                credit_sheet,
                #[cfg(feature = "asset_proxy")]
                asset_cdn,
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

            #[cfg(feature = "credits")]
            let credits_cfg;
            #[cfg(feature = "kafka")]
            let producer_cfg;
            #[cfg(feature = "kafka")]
            let consumer_cfg;

            #[cfg(feature = "kafka_internal")]
            {
                use rdkafka::config::RDKafkaLogLevel;
                use tracing::level_filters::LevelFilter;

                let mut config = rdkafka::ClientConfig::new();
                config
                    .set("bootstrap.servers", kafka_brokers)
                    .set_log_level(match LevelFilter::current() {
                        LevelFilter::OFF => RDKafkaLogLevel::Critical,
                        LevelFilter::ERROR => RDKafkaLogLevel::Error,
                        LevelFilter::WARN => RDKafkaLogLevel::Notice,
                        LevelFilter::INFO => RDKafkaLogLevel::Info,
                        LevelFilter::DEBUG | LevelFilter::TRACE => RDKafkaLogLevel::Debug,
                    });

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

                // Put MPSC producer init here

                #[cfg(feature = "credits")]
                {
                    credits_cfg = super::credits::Config {
                        credit_sheet,
                        config: DebugShim(config.clone()),
                    };
                }

                // Initialize broadcast producer and consumer

                #[cfg(feature = "kafka")]
                {
                    producer_cfg = super::producer::Config {
                        topic: service_name.into(),
                        config: DebugShim(config.clone()),
                    };
                }

                #[cfg(feature = "kafka")]
                {
                    consumer_cfg = super::consumer::Config {
                        service_name: service_name.into(),
                        config: DebugShim(config),
                    };
                }
            }

            #[cfg(feature = "asset_proxy")]
            let asset_proxy = super::assets::AssetProxy::new(&asset_cdn)?;

            Ok((
                Self {
                    rt,
                    #[cfg(feature = "kafka")]
                    producer_cfg,
                    #[cfg(feature = "kafka")]
                    consumer_cfg,
                    #[cfg(feature = "credits")]
                    credits_cfg,
                    #[cfg(feature = "asset_proxy")]
                    asset_proxy,
                },
                extra,
            ))
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

    /// Initial parameters for booting a service
    #[derive(Debug)]
    #[allow(missing_copy_implementations)]
    pub struct StartConfig {
        /// The name of this service, used as an identifier in logs and event
        /// buses
        pub service_name: &'static str,
    }

    /// Perform environment setup and run the requested entrypoint
    pub fn run<T: fmt::Debug + clap::Args>(
        cfg: StartConfig,
        main: impl FnOnce(Common, T) -> Result<()>,
    ) {
        let StartConfig { service_name } = cfg;

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
                drop(span);
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

                let hostname = hostname::get()
                    .unwrap_or_else(|e| init_error!("Failed to get system hostname: {e}"))
                    .to_string_lossy()
                    .into_owned();

                let (loki_layer, loki_task) = loki_endpoint
                    .map(|e| {
                        tracing_loki::layer(
                            e,
                            [
                                ("host_name".into(), hostname),
                                ("service_name".into(), service_name.into()),
                            ]
                            .into_iter()
                            .collect(),
                            [].into_iter().collect(),
                        )
                    })
                    .transpose()
                    .unwrap_or_else(|e| init_error!("Failed to initialize Loki exporter: {e}"))
                    .unzip();

                if let Some(loki_layer) = loki_layer {
                    init_subscriber(log_filter, |r| r.with(loki_layer));
                } else {
                    init_subscriber(log_filter, |r| r);
                }

                drop(span);

                (common, loki_task)
            },
        );

        let (common, loki_task) = smuggled;

        error_span!("run").in_scope(|| {
            let (common, extra) = match Common::new(cfg, common, loki_task) {
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
