# syntax=docker/dockerfile:1.6
FROM rust:1.79 as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p arb-nitro-rs

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/arb-nitro-rs /usr/local/bin/arb-nitro-rs
ENTRYPOINT ["/usr/local/bin/arb-nitro-rs"]
