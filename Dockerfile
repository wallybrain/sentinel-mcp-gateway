# Stage 1: Build
FROM rust:slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY migrations/ migrations/

# sqlx offline mode — migrations are embedded at compile time
ENV SQLX_OFFLINE=true
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*

RUN groupadd -r sentinel && useradd -r -g sentinel -s /sbin/nologin sentinel

COPY --from=builder /build/target/release/sentinel-gateway /usr/local/bin/sentinel-gateway
RUN mkdir -p /etc/sentinel && chmod 755 /etc/sentinel
COPY --chmod=644 sentinel-docker.toml /etc/sentinel/sentinel.toml

USER sentinel

EXPOSE 9200
EXPOSE 9201

HEALTHCHECK --interval=10s --timeout=3s --retries=3 \
  CMD curl -sf http://127.0.0.1:9201/health || exit 1

ENTRYPOINT ["sentinel-gateway"]
CMD ["--config", "/etc/sentinel/sentinel.toml"]
