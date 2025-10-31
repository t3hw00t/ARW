FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

# base deps
RUN apt-get update && apt-get install -y \
    build-essential pkg-config libssl-dev git curl ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH=/root/.cargo/bin:$PATH

# mise + project toolchains
RUN curl -fsSL https://mise.run | sh
ENV PATH=/root/.local/bin:$PATH
