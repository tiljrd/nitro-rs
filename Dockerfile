# syntax=docker/dockerfile:1.6
FROM rust:latest as builder

# Install build dependencies (git, clang/libclang, llvm, build tools)
RUN apt-get update && apt-get install -y --no-install-recommends \
    git clang libclang-dev llvm-dev pkg-config cmake make g++ libc6-dev \
    && rm -rf /var/lib/apt/lists/*

# Prepare workspace and clone external path dependencies expected by Cargo.toml
WORKDIR /
RUN git clone --depth=1 https://github.com/tiljrd/arb-alloy.git && \
    git clone --depth=1 https://github.com/tiljrd/reth.git

# Copy the nitro-rs source
WORKDIR /app
COPY . .

# Build the binary
RUN cargo build --release -p arb-nitro-rs

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/arb-nitro-rs /usr/local/bin/arb-nitro-rs
ENTRYPOINT ["/usr/local/bin/arb-nitro-rs"]
