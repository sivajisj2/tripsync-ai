# Rust 1.97-slim is available in the demo host's local Docker cache.
FROM rust:1.97-slim AS build
WORKDIR /src
COPY apps/api/Cargo.toml apps/api/Cargo.lock ./
COPY apps/api/src ./src
COPY apps/api/migrations ./migrations
RUN cargo build --release --locked

FROM rust:1.97-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* && mkdir -p /data && chown 10001:10001 /data
WORKDIR /app
COPY --from=build /src/target/release/tripsync-api /app/tripsync-api
COPY --from=build /src/migrations /app/migrations
ENV HOST=0.0.0.0 PORT=8080 APP_ENV=production COOKIE_SECURE=true DATABASE_URL=sqlite:///data/tripsync.db
VOLUME ["/data"]
USER 10001:10001
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=15s CMD curl -fsS http://localhost:8080/ready || exit 1
CMD ["/app/tripsync-api"]
