# syntax=docker/dockerfile:1

FROM rust:1.96-bookworm AS builder
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && cp target/release/atlas-hardfork-planner /usr/local/bin/atlas-hardfork-planner

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/atlas-hardfork-planner /usr/local/bin/atlas-hardfork-planner
COPY atlas-protocol-schedule.json /etc/atlas/atlas-protocol-schedule.json

ENV LISTEN_HOST=0.0.0.0 \
    LISTEN_PORT=28882 \
    HTML_TITLE="Atlas Hardfork Planner" \
    SCHEDULE_PATH=/etc/atlas/atlas-protocol-schedule.json

EXPOSE 28882
ENTRYPOINT ["atlas-hardfork-planner"]
