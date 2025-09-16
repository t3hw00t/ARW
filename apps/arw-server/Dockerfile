# syntax=docker/dockerfile:1

## Builder
FROM rust:1-bookworm AS builder
WORKDIR /work

# Cache deps first
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build -p arw-server --release --locked \
    && mkdir -p /work/out \
    && cp /work/target/release/arw-server /work/out/arw-server

## Runtime
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/*
RUN useradd -u 10001 -r -s /usr/sbin/nologin arw && mkdir -p /app && chown -R arw:arw /app
WORKDIR /app
COPY --from=builder /work/out/arw-server /usr/local/bin/arw-server
COPY spec /app/spec
COPY interfaces /app/interfaces

ENV ARW_PORT=8091 \
    ARW_DEBUG=0

USER arw
EXPOSE 8091
HEALTHCHECK --interval=10s --timeout=3s --start-period=3s --retries=5 \
  CMD curl -fsS http://127.0.0.1:${ARW_PORT}/healthz || exit 1
ENTRYPOINT ["/usr/local/bin/arw-server"]
