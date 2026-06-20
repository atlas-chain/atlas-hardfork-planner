# syntax=docker/dockerfile:1

FROM rust:1.96-bookworm AS builder
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && cp target/release/arkiv-hardfork-planner /usr/local/bin/arkiv-hardfork-planner

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/arkiv-hardfork-planner /usr/local/bin/arkiv-hardfork-planner
COPY arkiv-protocol-schedule.json /etc/arkiv/arkiv-protocol-schedule.json

ENV LISTEN_HOST=0.0.0.0 \
    LISTEN_PORT=28882 \
    SCHEDULE_PATH=/etc/arkiv/arkiv-protocol-schedule.json

EXPOSE 28882
ENTRYPOINT ["arkiv-hardfork-planner"]
