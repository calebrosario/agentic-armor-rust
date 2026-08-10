FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig openssl-dev openssl-libs-static

WORKDIR /build

# Copy both repos (rust-mcp-sdk is a path dependency)
COPY rust-mcp-sdk/ /rust-mcp-sdk/
COPY agentic-armor-rust/ /build/

RUN cargo build --release && \
    strip target/release/agentic-armor

FROM alpine:3.20

RUN apk add --no-cache ca-certificates libssl3 && \
    addgroup -g 1001 armor && \
    adduser -D -u 1001 -G armor armor

COPY --from=builder /build/target/release/agentic-armor /usr/local/bin/agentic-armor

USER armor
ENTRYPOINT ["agentic-armor"]
