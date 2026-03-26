# =============================================
# MEXC Ghost Hunter - Multi-stage Dockerfile
# Optimized for Hugging Face Spaces (port 7860) + 2-core VPS
# =============================================

# === Stage 1: Build React Frontend ===
FROM node:20-alpine AS frontend-builder

WORKDIR /app/frontend

# Copy frontend files
COPY frontend/package*.json ./
RUN npm ci --only=production

COPY frontend/ ./
RUN npm run build

# === Stage 2: Build Rust Backend ===
FROM rust:1.80-alpine AS rust-builder

WORKDIR /app/backend

# Install system dependencies
RUN apk add --no-cache musl-dev openssl-dev pkgconfig

# Copy Cargo files first for better caching
COPY backend/Cargo.toml backend/Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Copy actual source code
COPY backend/ ./

# Build optimized release binary
RUN cargo build --release

# === Stage 3: Final Runtime Image ===
FROM rust:1.80-alpine AS runtime

WORKDIR /app

# Install minimal runtime dependencies
RUN apk add --no-cache ca-certificates libssl3

# Copy Rust binary
COPY --from=rust-builder /app/backend/target/release/mexc-ghost-hunter /usr/local/bin/mexc-ghost-hunter

# Copy built React frontend (static files)
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist

# Copy SQLite database directory (will be mounted as volume)
RUN mkdir -p /data

# Expose port for Hugging Face (7860) and general use
EXPOSE 7860

# Set environment variables
ENV PORT=7860
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1

# Create a volume for persistent SQLite database
VOLUME ["/data"]

# Run the Rust binary (it serves both API and static React files)
CMD ["mexc-ghost-hunter"]
