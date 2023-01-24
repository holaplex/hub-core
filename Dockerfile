from rust:slim as test
workdir /hub

run apt-get update -y && apt-get install -y cmake g++ libsasl2-dev libssl-dev libudev-dev pkg-config protobuf-compiler

copy Cargo.toml Cargo.lock rust-toolchain.toml .
copy crates crates

run cargo build --locked --bin holaplex-hub-test

cmd ["target/debug/holaplex-hub-test"]
