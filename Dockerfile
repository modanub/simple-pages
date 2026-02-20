FROM rust:1.85 AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY templates/ templates/
COPY static/ static/

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/simple-pages /usr/local/bin/simple-pages

RUN mkdir -p /data/sites

EXPOSE 8080
ENV DATA_DIR=/data
CMD ["simple-pages"]
