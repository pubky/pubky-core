# ========================
# Build Stage
# ========================
FROM rust:1.89.0-alpine3.20 AS builder

# Build platform argument (x86_64 or aarch64) (default: x86_64)
ARG TARGETARCH=x86_64
RUN echo "TARGETARCH: $TARGETARCH"

# Install build dependencies, including static OpenSSL libraries
RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    openssl-libs-static \
    pkgconfig \
    build-base \
    curl

# Detect host architecture and install cross-compiler when needed
RUN HOSTARCH=$(uname -m) && \
    echo "Host architecture: $HOSTARCH" && \
    echo "Target architecture: $TARGETARCH" && \
    if [ "$TARGETARCH" = "aarch64" ]; then \
        echo "Installing ARM64 cross-compiler only for target aarch64" && \
        wget -qO- https://musl.cc/aarch64-linux-musl-cross.tgz | tar -xz -C /usr/local; \
    elif [ "$TARGETARCH" = "x86_64" ] && [ "$HOSTARCH" = "aarch64" ]; then \
        echo "Installing x86_64 and ARM64 cross-compilers for ARM host targeting x86_64" && \
        wget -qO- https://musl.cc/aarch64-linux-musl-cross.tgz | tar -xz -C /usr/local; \
        wget -qO- https://musl.cc/x86_64-linux-musl-cross.tgz | tar -xz -C /usr/local; \
    fi

# Set cross-compiler environment variables conditionally
ENV CC_aarch64_unknown_linux_musl="aarch64-linux-musl-gcc"
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="aarch64-linux-musl-gcc"

# Create environment setup script for x86_64 variables (only when host is ARM)
RUN if [ "$(uname -m)" = "aarch64" ]; then \
        echo 'export CC_x86_64_unknown_linux_musl="x86_64-linux-musl-gcc"' > /tmp/x86-env.sh && \
        echo 'export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="x86_64-linux-musl-gcc"' >> /tmp/x86-env.sh; \
    fi

# Set PATH to include both cross-compiler directories (non-existent paths are ignored)
ENV PATH="/usr/local/aarch64-linux-musl-cross/bin:/usr/local/x86_64-linux-musl-cross/bin:$PATH"

# Set environment variables for static linking with OpenSSL
ENV OPENSSL_STATIC=yes
ENV OPENSSL_LIB_DIR=/usr/lib
ENV OPENSSL_INCLUDE_DIR=/usr/include

# Add the MUSL target for static linking
RUN rustup target add $TARGETARCH-unknown-linux-musl

# Set the working directory
WORKDIR /usr/src/app

# Copy over Cargo.toml and Cargo.lock for dependency caching
COPY Cargo.toml Cargo.lock ./

# Copy over all the source code
COPY . .

# Add build argument for binary selection (homeserver or testnet)
ARG BUILD_TARGET=testnet

# Build the project in release mode for the MUSL target
RUN if [ -f /tmp/x86-env.sh ]; then . /tmp/x86-env.sh; fi && cargo build --release --bin pubky-$BUILD_TARGET --target $TARGETARCH-unknown-linux-musl

# Strip the binary to reduce size
RUN if [ "$TARGETARCH" = "aarch64" ]; then \
        aarch64-linux-musl-strip target/aarch64-unknown-linux-musl/release/pubky-$BUILD_TARGET; \
    elif [ "$TARGETARCH" = "x86_64" ]; then \
        x86_64-linux-musl-strip target/x86_64-unknown-linux-musl/release/pubky-$BUILD_TARGET; \
    fi

# ========================
# Runtime Stage
# ========================
FROM alpine:3.20

ARG TARGETARCH=x86_64
ARG BUILD_TARGET=testnet

# Install runtime dependencies (only ca-certificates)
RUN apk add --no-cache ca-certificates

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/src/app/target/$TARGETARCH-unknown-linux-musl/release/pubky-$BUILD_TARGET /usr/local/bin/homeserver

# Set the working directory
WORKDIR /usr/local/bin

# Expose the port the homeserver listens on (should match that of config.toml)
EXPOSE 6287

# Set the default command to run the binary
CMD ["homeserver"]
