//! metrics imports
pub use opentelemetry::{
    metrics::{Counter, Histogram, MeterProvider as _, Unit},
    KeyValue,
};
pub use opentelemetry_prometheus::{exporter, PrometheusExporter};
pub use opentelemetry_sdk::{
    metrics::{MeterProvider, PeriodicReader},
    runtime, Resource,
};
pub use prometheus::{Encoder, Registry, TextEncoder};
