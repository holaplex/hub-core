#!/bin/sh

set -ex
cd "`dirname "$0"`"

podman container inspect redpanda-1 || \
  podman run -d --pull=always --name=redpanda-1 --rm \
    -p 8081:8081 \
    -p 8082:8082 \
    -p 9092:9092 \
    -p 9644:9644 \
    docker.redpanda.com/vectorized/redpanda:latest \
    redpanda start \
    --overprovisioned \
    --seeds "redpanda-1:33145" \
    --set redpanda.empty_seed_starts_cluster=false \
    --smp 1  \
    --memory 1G \
    --reserve-memory 0M \
    --check=false \
    --advertise-rpc-addr redpanda-1:33145

while :; do
  ! curl 'http://localhost:8081/v1' || break
  sleep 3
done

curl -X POST 'http://localhost:8081/subjects/foo/versions' \
  -H 'Content-Type: application/vnd.schemaregistry.v1+json' \
  -d "`ruby -e 'require "json";puts JSON::dump({schemaType:"PROTOBUF",schema:File::read("crates/test/src/test.proto")})'`" \
  -vvv
