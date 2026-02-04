set shell := ["zsh", "-lc"]

# Default target
_default:
    @just --list

fmt:
    cargo fmt

build:
    cargo build

release:
    cargo build --release

check:
    cargo check

run *args:
    cargo run -- {{args}}

# Build for Linux (x86_64 musl static binary)
build-linux:
    cargo build --release --target x86_64-unknown-linux-musl

# Build for macOS (aarch64)
build-macos:
    #!/usr/bin/env bash
    if [[ "$(uname)" == "Darwin" ]]; then
        cargo build --release --target aarch64-apple-darwin
    else
        docker run --rm -v "$(pwd)":/app -w /app messense/cargo-zigbuild:latest \
            cargo zigbuild --release --target aarch64-apple-darwin
    fi
