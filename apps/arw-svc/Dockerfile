# syntax=docker/dockerfile:1

## Builder
FROM rust:1-bookworm as builder
WORKDIR /work

# Cache deps first
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build -p arw-svc --release --locked \
    && mkdir -p /work/out \
    && cp /work/target/release/arw-svc /work/out/arw-svc

## Runtime
FROM debian:bookworm-slim
# Minimal runtime deps; libxcb1 required by screenshots/X11 even if unused headless
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        libxcb1 \
    && rm -rf /var/lib/apt/lists/*
RUN useradd -u 10001 -r -s /usr/sbin/nologin arw && mkdir -p /app && chown -R arw:arw /app
WORKDIR /app
# Note: when using BuildKit cache mounts, target/ is not committed to the layer.
# Copy the binary from a committed path produced above.
COPY --from=builder /work/out/arw-svc /usr/local/bin/arw-svc
# Ship spec and interfaces so /spec/* and catalog watcher have files
COPY spec /app/spec
COPY interfaces /app/interfaces

ENV ARW_PORT=8090 \
    ARW_DEBUG=0

USER arw
EXPOSE 8090
HEALTHCHECK --interval=10s --timeout=3s --start-period=3s --retries=5 \
  CMD curl -fsS http://127.0.0.1:${ARW_PORT}/healthz || exit 1
ENTRYPOINT ["/usr/local/bin/arw-svc"]
