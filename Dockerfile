# Generic image for hosts that run containers. Builds the same static musl binary as the GitHub Actions
# release. Mount a volume at /app to keep bot.db and its backups.
FROM rust:1-alpine AS builder
RUN apk add --no-cache musl-dev gcc make perl
WORKDIR /src
COPY . .
RUN cargo build --release

FROM alpine:3
RUN adduser -D -u 10001 bot && mkdir -p /app && chown bot /app
COPY --from=builder /src/target/release/anpot /usr/local/bin/anpot
USER bot
WORKDIR /app
# DISCORD_TOKEN (and optional settings) come from the environment or a .env file in /app.
ENTRYPOINT ["/usr/local/bin/anpot"]
