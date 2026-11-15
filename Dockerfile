FROM rust:1.100-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    clang libclang-dev cmake perl pkg-config libmariadb-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY migrations ./migrations
# Keep local Cargo overrides out of deployment builds (baseline platform CPU).
RUN cargo build --locked --release --bin bluesky-obfs-tunnel

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libmariadb3 libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 tunnel \
    && useradd --uid 10001 --gid tunnel --no-create-home tunnel
WORKDIR /app
COPY --from=builder /build/target/release/bluesky-obfs-tunnel /usr/local/bin/
COPY cli_pattern.bin serv_pattern.bin ./
COPY config/server.example.json ./server.json
USER tunnel
EXPOSE 443
ENTRYPOINT ["bluesky-obfs-tunnel", "--config", "/app/server.json"]
