FROM --platform=$BUILDPLATFORM rust:1.88 AS builder

ARG TARGETPLATFORM

RUN case "$TARGETPLATFORM" in \
      "linux/amd64") echo "x86_64-unknown-linux-gnu" > /tmp/target ;; \
      "linux/arm64") echo "aarch64-unknown-linux-gnu" > /tmp/target ;; \
      *) echo "Unsupported platform: $TARGETPLATFORM" && exit 1 ;; \
    esac && \
    rustup target add $(cat /tmp/target)

# Install cross-compilation toolchain if needed
RUN if [ "$TARGETPLATFORM" = "linux/amd64" ] && [ "$(uname -m)" != "x86_64" ]; then \
      apt-get update && apt-get install -y gcc-x86-64-linux-gnu && rm -rf /var/lib/apt/lists/*; \
    elif [ "$TARGETPLATFORM" = "linux/arm64" ] && [ "$(uname -m)" != "aarch64" ]; then \
      apt-get update && apt-get install -y gcc-aarch64-linux-gnu && rm -rf /var/lib/apt/lists/*; \
    fi

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY templates/ templates/
COPY static/ static/

RUN TARGET=$(cat /tmp/target) && \
    case "$TARGET" in \
      x86_64-unknown-linux-gnu) export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc ;; \
      aarch64-unknown-linux-gnu) export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc ;; \
    esac && \
    cargo build --release --target "$TARGET" && \
    cp target/$TARGET/release/simple-pages /build/simple-pages

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/simple-pages /usr/local/bin/simple-pages

RUN mkdir -p /data/sites

EXPOSE 8080
ENV DATA_DIR=/data
CMD ["simple-pages"]
