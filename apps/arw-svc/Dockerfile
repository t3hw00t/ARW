# syntax=docker/dockerfile:1

## Builder
FROM rust:1 as builder
WORKDIR /work

# Cache deps first
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/work/target \
    cargo build -p arw-svc --release --locked

## Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -u 10001 -r -s /usr/sbin/nologin arw && mkdir -p /app && chown -R arw:arw /app
WORKDIR /app
COPY --from=builder /work/target/release/arw-svc /usr/local/bin/arw-svc

ENV ARW_PORT=8090 \
    ARW_DEBUG=0

USER arw
EXPOSE 8090
ENTRYPOINT ["/usr/local/bin/arw-svc"]
