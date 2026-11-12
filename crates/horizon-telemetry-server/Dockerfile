# Built from the repository root: docker build -f crates/horizon-telemetry-server/Dockerfile .
FROM rust:1-slim AS build
WORKDIR /src
# The root manifest only for the shared crate's workspace settings (edition, lints).
COPY Cargo.toml Cargo.toml
# Every crate the server's Cargo.lock builds from a path (tests/test_service_images.py checks the list).
COPY crates/horizon-addresses crates/horizon-addresses
COPY crates/horizon-core crates/horizon-core
COPY crates/horizon-hooks crates/horizon-hooks
COPY crates/horizon-platform crates/horizon-platform
COPY crates/horizon-system crates/horizon-system
COPY crates/horizon-telemetry crates/horizon-telemetry
COPY crates/horizon-telemetry-server crates/horizon-telemetry-server
RUN cargo build --release --locked --manifest-path crates/horizon-telemetry-server/Cargo.toml

FROM debian:stable-slim
COPY --from=build /src/crates/horizon-telemetry-server/target/release/horizon-telemetry-server /usr/local/bin/
# Railway's proxy sets X-Real-IP to the client's address; it is the header the server read first before
# HORIZON_TELEMETRY_CLIENT_IP_HEADER existed, so this keeps the same address for the rate limits (README "Hosting").
ENV HORIZON_TELEMETRY_DB=/data/telemetry.sqlite3 HORIZON_TELEMETRY_TRUST_PROXY=1 \
    HORIZON_TELEMETRY_CLIENT_IP_HEADER=x-real-ip
CMD ["horizon-telemetry-server"]
