#!/bin/bash

set -e
cd "$(dirname "$0")"

[[ -z "$CARGO" ]] && CARGO=cargo

jq --version
git config --local core.hooksPath scripts/git-hooks
rustup --version
rustc --version
cargo --version
