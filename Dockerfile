# =============================================
# MEXC Ghost Hunter - Production Dockerfile (VPS)
# Optimized for 2-core VPS deployment
# =============================================

# === Stage 1: Build React Frontend ===
FROM node:20-alpine AS frontend-builder

WORKDIR /app/frontend

COPY frontend/package*.json ./
RUN npm ci

COPY frontend/ ./
RUN npm run build

# === Stage 2: Build Rust Backend ===
FROM rust:1.80-alpine AS rust-builder

WORKDIR /app/backend

# System dependencies
RUN apk add --no-cache musl-dev openssl-dev pkgconfig

# Cache dependencies (layer caching)
COPY backend/Cargo.toml backend/Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release 2>&1 | grep -v "warning:" || true
RUN rm -rf src

# Copy source and build
COPY backend/ ./
RUN cargo build --release

# === Stage 3: Final Runtime Image ===
FROM alpine:3.20 AS runtime

WORKDIR /app

# Runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    libssl3 \
    sqlite-libs \
    tini

# Non-root user for security
RUN addgroup -g 1000 mexc && \
    adduser -D -u 1000 -G mexc mexc

# Copy Rust binary
COPY --from=rust-builder /app/backend/target/release/mexc-ghost-hunter /usr/local/bin/mexc-ghost-hunter

# Copy built React frontend
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist

# Create persistent data directory
RUN mkdir -p /data && \
    chown -R mexc:mexc /data /app

# Switch to non-root user
USER mexc

# Expose port (configurable)
EXPOSE 8080

# Environment variables (production defaults, can be overridden)
ENV PORT=8080 \
    RUST_LOG=info \
    RUST_BACKTRACE=1 \
    DATA_DIR=/data \
    MIN_PROFIT_THRESHOLD=0.0015 \
    TARGET_VOLUME_USD=1000.0 \
    MIN_VOLUME_24H=500000.0

# Volume for persistent SQLite database
VOLUME ["/data"]

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD wget -q -O /dev/null http://127.0.0.1:${PORT}/api/health || exit 1

# Use tini as init to handle signals properly
ENTRYPOINT ["/sbin/tini", "--"]

# Run the application
CMD ["mexc-ghost-hunter"]
