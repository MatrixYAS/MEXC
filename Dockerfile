# =============================================
# MEXC Ghost Hunter - Final Dockerfile (per guide)
# Optimized for Hugging Face Spaces + 2-core VPS
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

# Cache dependencies
COPY backend/Cargo.toml backend/Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Copy source and build
COPY backend/ ./
RUN cargo build --release

# === Stage 3: Final Runtime Image ===
FROM alpine:3.20 AS runtime
WORKDIR /app

# Runtime dependencies
RUN apk add --no-cache ca-certificates libssl3 sqlite-libs

# Copy Rust binary
COPY --from=rust-builder /app/backend/target/release/mexc-ghost-hunter /usr/local/bin/mexc-ghost-hunter

# Copy built React frontend
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist

# Create persistent data directory + empty DB placeholder
RUN mkdir -p /data && touch /data/mexc.db

# Expose port for Hugging Face
EXPOSE 7860

# Environment variables (can be overridden in Hugging Face settings)
ENV PORT=7860
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1
ENV ADMIN_PASSWORD=ghosthunter123
ENV MIN_PROFIT_THRESHOLD=0.0015
ENV TARGET_VOLUME_USD=1000.0
ENV MIN_VOLUME_24H=500000.0
ENV ENCRYPTION_SALT=mexc-ghost-hunter-salt-2026

# Volume for persistent SQLite database
VOLUME ["/data"]

# Run the application
CMD ["mexc-ghost-hunter"]
