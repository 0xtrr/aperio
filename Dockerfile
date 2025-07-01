# Build stage
FROM rust:1.88-slim as builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy the entire project
COPY . .

# Display directory contents to debug
RUN ls -la

# Build the application
RUN cargo build --release

# Display the built binary
RUN ls -la target/release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ffmpeg \
    python3 \
    python3-pip \
    python3-venv \
    curl \
    && python3 -m venv /opt/venv \
    && . /opt/venv/bin/activate \
    && pip install --no-cache-dir yt-dlp \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Add the venv to PATH
ENV PATH="/opt/venv/bin:$PATH"

# Create app user and directories
RUN useradd -m -u 1000 aperio \
    && mkdir -p /app/storage /app/working \
    && chown -R aperio:aperio /app

# Copy the binary from the builder stage to the runtime stage
COPY --from=builder /app/target/release/aperio /usr/local/bin/aperio

# Expose port
EXPOSE 8080

# Set working directory
WORKDIR /app

# Set environment variables
ENV RUST_LOG=info
ENV APERIO_HOST=0.0.0.0
ENV APERIO_PORT=8080

# Switch to non-root user
USER aperio

# Run the application
ENTRYPOINT ["/usr/local/bin/aperio"]
