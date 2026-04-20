# ========================
# Build Stage
# ========================
FROM rust:1.89.0-alpine3.20 AS builder

# TARGETARCH is set by Docker Buildx when using --platform flag
ARG TARGETARCH
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

# Copy over Cargo.toml and Cargo.lock for dependency caching
COPY Cargo.toml Cargo.lock ./

# Copy over all the source code
COPY . .

# Build argument for binary selection (always homeserver for Umbrel)
ARG BUILD_TARGET=homeserver

# Build the project in release mode for the MUSL target
# Only apply environment setup script only when host is ARM so we don't override the native compiler on x86 hosts
RUN cargo build --release --bin pubky-$BUILD_TARGET

# Strip the binary to reduce size
RUN strip target/release/pubky-$BUILD_TARGET

# =======================
# Runtime Stage
# =======================
FROM alpine:3.20

ARG TARGETARCH
ARG BUILD_TARGET=homeserver

# Install runtime dependencies (ca-certificates for TLS, wget and netcat for healthchecks, su-exec for user switching, gettext for envsubst)
RUN apk add --no-cache ca-certificates wget netcat-openbsd su-exec gettext

# Create non-root user (use 1000:1000 to match Umbrel's default)
RUN addgroup -g 1000 homeserver && \
    adduser -D -u 1000 -G homeserver homeserver

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/src/app/target/release/pubky-$BUILD_TARGET /usr/local/bin/homeserver

# Copy entrypoint script and config template, fix line endings (remove Windows CRLF to ensure Unix LF)
# This ensures the scripts work regardless of the line endings in the source repository
COPY deployments/umbrel/entrypoint.sh /tmp/entrypoint.sh
COPY deployments/umbrel/config.toml.template /usr/local/share/config.toml.template
RUN sed -i 's/\r$//' /tmp/entrypoint.sh && \
    mv /tmp/entrypoint.sh /usr/local/bin/entrypoint.sh && \
    chmod +x /usr/local/bin/entrypoint.sh && \
    chmod 644 /usr/local/share/config.toml.template

# Set the working directory for data
WORKDIR /data

# Expose ports (6286: HTTP, 6287: Pubky TLS, 6288: Admin, 6289: Metrics)
EXPOSE 6286 6287 6288 6289

# Use entrypoint script that handles config creation and user switching
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
