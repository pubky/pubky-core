
# ========================
# Build Stage
# ========================
FROM rust:1.89.0-alpine3.20 AS builder

# Build platform argument (x86_64 or aarch64) (default: x86_64)
RUN echo "TARGETARCH: $TARGETARCH"

# Install build dependencies, including static OpenSSL libraries
RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    openssl-libs-static \
    pkgconfig \
    build-base \
    curl

# Set cross-compiler environment variables:
# Always set ARM64 variables - safe since unused when targeting x86_64.
# Create environment setup script for x86_64 variables - only when host is ARM so we don't override the native compiler on x86 hosts.
# Set PATH to include both cross-compiler directories (non-existent paths are ignored)
# Set environment variables for static linking with OpenSSL
ENV OPENSSL_STATIC=yes
ENV OPENSSL_LIB_DIR=/usr/lib
ENV OPENSSL_INCLUDE_DIR=/usr/include

# Set the working directory
WORKDIR /usr/src/app

# Add build argument for binary selection (homeserver or testnet)
ARG BUILD_TARGET=testnet

# ---- Dependency layer (cacheable) ----
# Copy only manifests + lockfile. This layer is invalidated only when a
# Cargo.toml or Cargo.lock changes.
COPY Cargo.toml Cargo.lock ./
COPY e2e/Cargo.toml e2e/Cargo.toml
COPY examples/rust/Cargo.toml examples/rust/Cargo.toml
COPY homeservercli/Cargo.toml homeservercli/Cargo.toml
COPY pubky-common/Cargo.toml pubky-common/Cargo.toml
COPY pubky-homeserver/Cargo.toml pubky-homeserver/Cargo.toml
COPY pubky-testnet/Cargo.toml pubky-testnet/Cargo.toml
COPY pubky-sdk/Cargo.toml pubky-sdk/Cargo.toml
COPY pubky-sdk/bindings/js/Cargo.toml pubky-sdk/bindings/js/Cargo.toml
COPY test_utils/pubky_test/Cargo.toml test_utils/pubky_test/Cargo.toml
COPY test_utils/test_macro/Cargo.toml test_utils/test_macro/Cargo.toml
COPY test_utils/drop_db_helper/Cargo.toml test_utils/drop_db_helper/Cargo.toml

# Stub every workspace crate with a dummy lib/main so `cargo build` compiles
# only third-party dependencies.
RUN mkdir -p e2e/src examples/rust/src homeservercli/src \
        pubky-common/src pubky-homeserver/src pubky-testnet/src \
        pubky-sdk/src pubky-sdk/bindings/js/src \
        test_utils/pubky_test/src test_utils/test_macro/src \
        test_utils/drop_db_helper/src \
    && echo "" > e2e/src/lib.rs \
    && echo "" > pubky-common/src/lib.rs \
    && echo "" > pubky-homeserver/src/lib.rs \
    && echo "fn main() {}" > pubky-homeserver/src/main.rs \
    && echo "" > pubky-testnet/src/lib.rs \
    && echo "" > pubky-sdk/src/lib.rs \
    && echo "" > pubky-sdk/bindings/js/src/lib.rs \
    && echo "" > test_utils/pubky_test/src/lib.rs \
    && echo "" > test_utils/test_macro/src/lib.rs \
    && echo "" > test_utils/drop_db_helper/src/lib.rs \
    && echo "fn main() {}" > homeservercli/src/main.rs \
    && echo "fn main() {}" > examples/rust/src/main.rs \
    && cargo build --release --bin pubky-homeserver

# Copy over all the source code
COPY . .

# Rebuild the workspace crates on top of the cached dependency artifacts.
# mtimes from COPY are newer than the stub layer, so cargo rebuilds the
# workspace crates and links them against the cached dependency artifacts.
# The touch makes mtime ordering explicit and cheap.
RUN find . -name '*.rs' -not -path './target/*' -exec touch {} + \
    && cargo build --release --bin pubky-$BUILD_TARGET

# Strip the binary to reduce size
RUN strip target/release/pubky-$BUILD_TARGET

# ========================
# Runtime Stage
# ========================
FROM alpine:3.20

ARG TARGETARCH
ARG BUILD_TARGET=testnet

# Install runtime dependencies (only ca-certificates)
RUN apk add --no-cache ca-certificates

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/src/app/target/release/pubky-$BUILD_TARGET /usr/local/bin/homeserver

# Set the working directory
WORKDIR /usr/src/app

# Expose the port the homeserver listens on (should match that of config.toml)
EXPOSE 6287

# Set the default command to run the binary
CMD ["homeserver"]
