# Multi-stage Dockerfile for AiDb cluster node runner
# Build stage
FROM rust:1.91-bullseye AS builder
WORKDIR /app

# Install system deps required for some crates (if any)
RUN apt-get update && apt-get install -y pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy only manifest first to leverage cache
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY examples ./examples
COPY benches ./benches

# Build the example node runner with raft-cluster feature (build only the one example to save time)
RUN cargo build --release --example node_runner --features "raft-cluster,monitoring"

# Runtime image
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Copy the compiled example binary
COPY --from=builder /app/target/release/examples/node_runner /usr/local/bin/node_runner

# Create directory for data
RUN mkdir -p /data

EXPOSE 50001 50002 50003 8001 8002 8003
ENTRYPOINT ["/usr/local/bin/node_runner"]
