FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin anpot

# We do not need the Rust toolchain to run the binary!
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /app anpot \
    && mkdir -p /data \
    && chown anpot:anpot /data
WORKDIR /app
COPY --from=builder /app/target/release/anpot /usr/local/bin/anpot
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
ENV DATABASE_URL=sqlite:///data/bot.db?mode=rwc
VOLUME /data
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
