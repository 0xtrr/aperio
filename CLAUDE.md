# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Aperio is a REST API service for downloading and processing videos from YouTube and Instagram, built with Rust using the Actix Web framework. The application uses an asynchronous job processing system with SQLite database persistence.

## Development Commands

### Building and Running
```bash
# Build the project
cargo build

# Build for release (optimized)
cargo build --release

# Run the application
cargo run

# Run with release optimizations
cargo run --release
```

### Code Quality
```bash
# Check for compilation errors
cargo check

# Run clippy linter (fix automatically)
cargo clippy --fix --allow-dirty --allow-staged

# Run clippy for checking only
cargo clippy

# Format code
cargo fmt
```

### Docker Operations
```bash
# Build and start with Docker Compose
docker-compose up -d

# View logs
docker-compose logs -f

# Stop services
docker-compose down
```

### Database Operations
```bash
# Database migrations are automatically run on startup
# Migration files are located in migrations/
```

## Architecture Overview

### Core Components

**Job Processing Pipeline:**
- Jobs flow through: `Pending` → `Downloading` → `Processing` → `Completed`
- Event-driven job queue with priority support (High/Normal/Low)
- Configurable concurrency limits to prevent resource exhaustion

**Service Layer Architecture:**
- `JobQueue` - Event-driven job scheduling with priority queue
- `DownloadService` - Handles yt-dlp video downloads with security validation
- `ProcessService` - FFmpeg video processing with optimized settings
- `JobRepository` - SQLite database operations with transaction safety
- `SecurityValidator` - URL validation, domain whitelisting, path traversal protection
- `ConnectionPoolManager` - Semaphore-based resource limiting for downloads/processing

**External Dependencies:**
- `yt-dlp` for video downloads (configurable format selection)
- `ffmpeg` for video processing (optimized settings for quality/speed balance)

### Key Architectural Decisions

**Performance Optimizations:**
- Event-driven job queue (no polling loops)
- Async file operations throughout
- Smart yt-dlp format selection to prefer H.264+AAC (reduces re-encoding)
- FFmpeg optimized with: `preset=medium`, `crf=23`, `threads=0`, compatibility settings

**Resource Management:**
- Default concurrency limits: 2 downloads, 1 processing, 2 total jobs
- Configurable via environment variables (see README.md)
- Semaphore-based permits prevent resource contention

**Security Model:**
- HTTPS-only URL validation
- Domain whitelist (configurable via `APERIO_ALLOWED_DOMAINS`)
- Input sanitization and path traversal protection
- File size limits and secure temporary file handling

### Configuration

The application uses environment-based configuration with sensible defaults. Key performance settings:

- `APERIO_MAX_CONCURRENT_DOWNLOADS=2` (prevents excessive yt-dlp processes)
- `APERIO_MAX_CONCURRENT_PROCESSING=1` (prevents FFmpeg CPU overload)
- `APERIO_MAX_CONCURRENT_JOBS=2` (overall job limit)

### API Endpoints

**Core Endpoints:**
- `POST /process` - Start video processing job
- `GET /status/{job_id}` - Check job status
- `GET /video/{job_id}` - Download processed video
- `GET /stream/{job_id}` - Stream video inline
- `DELETE /jobs/{job_id}` - Cancel job

**Monitoring:**
- `GET /health` - Basic health check
- `GET /health/detailed` - Detailed system status
- `GET /metrics` - Application metrics

### Error Handling

The application uses a structured error system (`AppError`) with retry logic:
- Download failures: 2 attempts with exponential backoff
- Processing failures: 1 attempt (no retries)
- Database operations: 3 attempts with short delays

### File Management

**Working Directory Structure:**
- Downloads: `{working_dir}/{job_id}_original.{ext}`
- Processed: `{working_dir}/{job_id}_processed.mp4`
- Storage: Persistent volume at `/app/storage` (Docker)

### Monitoring and Observability

- Structured JSON logging with correlation IDs
- Health checks for database, disk space, and dependencies
- Request tracing and performance metrics
- Configurable log levels via `RUST_LOG`

## Development Notes

### Adding New Features

When extending the API:
1. Add new endpoints to `src/api/routes.rs`
2. Update `AppState` if new services are needed
3. Follow the existing error handling patterns
4. Add appropriate input validation via `SecurityValidator`

### Performance Considerations

- The job queue uses event-driven notifications (no polling)
- File operations are async to prevent blocking
- External commands (yt-dlp, ffmpeg) use timeouts and resource limits
- Database operations use transactions for consistency

### Security Guidelines

- All URLs must pass `SecurityValidator::validate_url()`
- User inputs must be validated via `SecurityValidator::validate_input()`
- File paths must use `SecurityValidator::safe_job_file_path()`
- Never expose internal paths or system information in API responses