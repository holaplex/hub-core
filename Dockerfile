FROM lukemathwalker/cargo-chef:0.1.50-rust-buster AS chef
WORKDIR /app

FROM chef AS planner
COPY Cargo.* ./
COPY crates crates
COPY scripts scripts
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Deps
RUN apt-get update -y && \
  apt-get install -y \
    librdkafka-dev \
    libsasl2-dev \
  && \
  rm -rf /var/lib/apt/lists/*

# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY Cargo.* ./
COPY crates crates
COPY scripts scripts

RUN cargo build --bin holaplex-hub-test --release


FROM debian:bullseye-slim as base
WORKDIR /app
RUN apt-get update -y && \
  apt-get install -y \
    ca-certificates \
    libssl1.1 \
    librdkafka-dev \
    libsasl2-dev \
  && \
  rm -rf /var/lib/apt/lists/*

FROM base AS hub-test
COPY --from=builder /app/target/release/holaplex-hub-test bin/
