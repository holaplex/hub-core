# Holaplex Core Library

Common packages and utilities used by Holaplex Rust based services.

## Features

### Kafka

When the `kafka` feature is enable the core library includes functions for producing and consuming messages to Kafka. The `rdkafka` dependency builds [librdkafka](https://github.com/confluentinc/librdkafka) from source which requires the below packages to be installed on your system.

#### MacOS

```
brew install cmake gcc openssl@1.1 pkg-config protobuf

brew info openssl@1.1 # follow the instruction to setup your terminal profile
```