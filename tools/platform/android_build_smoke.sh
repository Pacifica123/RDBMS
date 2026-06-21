#!/usr/bin/env sh
set -eu

rustup target add aarch64-linux-android
cargo build -p rdbms_android --target aarch64-linux-android --release
