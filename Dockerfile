# syntax=docker/dockerfile:1.7
# Multi-stage musl static build → distroless base (REQ-M-002).
# Target image < 20 MB, runs as non-root (uid 65532 = nonroot in distroless).

FROM --platform=$BUILDPLATFORM rust:1.91-alpine AS builder

# aws-lc-sys (pulled in by aws-sdk-kms) requires cmake, perl, and make.
RUN apk add --no-cache musl-dev cmake perl make

RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --target x86_64-unknown-linux-musl

# --- runtime stage ---
FROM gcr.io/distroless/static-debian12:nonroot AS runtime

COPY --from=builder \
    /build/target/x86_64-unknown-linux-musl/release/espresso-kms-signer \
    /usr/local/bin/espresso-kms-signer

USER nonroot
ENTRYPOINT ["/usr/local/bin/espresso-kms-signer"]
