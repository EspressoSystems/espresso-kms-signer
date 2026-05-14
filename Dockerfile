# syntax=docker/dockerfile:1.7
# Multi-stage musl static build → distroless base (REQ-M-002).
# Target image < 20 MB, runs as non-root (uid 65532 = nonroot in distroless).
#
# The builder runs as $TARGETPLATFORM (emulated by QEMU when cross-arch), so
# musl tooling resolves natively and we avoid pinning a cross-toolchain.

FROM rust:1.91-alpine AS builder
ARG TARGETPLATFORM

# aws-lc-sys (pulled in by aws-sdk-kms) requires cmake, perl, and make.
RUN apk add --no-cache musl-dev cmake perl make

RUN case "$TARGETPLATFORM" in \
      "linux/amd64") RUST_TARGET=x86_64-unknown-linux-musl ;; \
      "linux/arm64") RUST_TARGET=aarch64-unknown-linux-musl ;; \
      *) echo "Unsupported TARGETPLATFORM: $TARGETPLATFORM" >&2; exit 1 ;; \
    esac && \
    echo "$RUST_TARGET" > /rust-target && \
    rustup target add "$RUST_TARGET"

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN RUST_TARGET="$(cat /rust-target)" && \
    cargo build --release --target "$RUST_TARGET" && \
    cp "target/$RUST_TARGET/release/espresso-kms-signer" /espresso-kms-signer

# --- runtime stage ---
FROM gcr.io/distroless/static-debian12:nonroot AS runtime

COPY --from=builder /espresso-kms-signer /usr/local/bin/espresso-kms-signer

USER nonroot
ENTRYPOINT ["/usr/local/bin/espresso-kms-signer"]
