# syntax=docker/dockerfile:1.6
FROM rust:latest as builder

# Install build dependencies (git, clang/libclang, llvm, build tools)
RUN apt-get update && apt-get install -y --no-install-recommends \
    git clang libclang-dev llvm-dev pkg-config cmake make g++ libc6-dev \
    && rm -rf /var/lib/apt/lists/*

# Prepare workspace and clone external path dependencies expected by Cargo.toml
WORKDIR /
# arb-alloy main is fine
RUN git clone --depth=1 https://github.com/tiljrd/arb-alloy.git
# Pin reth to the PR branch that fixes the inner attribute issue in arbitrum/evm
RUN git clone --depth=1 --branch devin/1755179801-arbitrum-evm-attr-fix https://github.com/tiljrd/reth.git

# Copy the nitro-rs source
WORKDIR /app
COPY . .

# Build the binary
RUN cargo build --release -p arb-nitro-rs

FROM debian:bookworm-slim
# Install runtime deps (OpenSSL runtime for libssl.so.3)
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/arb-nitro-rs /usr/local/bin/arb-nitro-rs
ENTRYPOINT ["/usr/local/bin/arb-nitro-rs"]
