# Aperio - Video Processing API

A REST API service for downloading and processing videos from YouTube and Instagram.

## Features

- Download videos from YouTube and Instagram
- Process videos using FFmpeg for optimization
- Store processed videos for later retrieval
- Asynchronous job processing system with priority support
- Automated job retention and cleanup system
- Production-ready metrics and monitoring (Prometheus compatible)
- Comprehensive health checks and observability
- Enhanced security with domain whitelisting and input validation
- Docker-ready deployment with optimized performance settings

## Docker Deployment

### Quick Start

The easiest way to deploy Aperio is using Docker Compose:

```bash
# Build and start the service
docker-compose up -d

# Check logs
docker-compose logs -f
```

### Configuration

Aperio can be configured using environment variables in the docker-compose.yml file:

```yaml
environment:
  - APERIO_HOST=0.0.0.0
  - APERIO_PORT=8080
  - APERIO_ALLOWED_DOMAINS=youtube.com,youtu.be,instagram.com,vimeo.com
  # Add other configuration as needed
```

## API Endpoints

### Start a new video processing job

```bash
curl -X POST http://localhost:8080/process \
  -H "Content-Type: application/json" \
  -d '{"url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"}'
```

### Check job status

```bash
curl -X GET http://localhost:8080/status/{job_id}
```

### Download processed video

```bash
curl -X GET http://localhost:8080/video/{job_id} --output video.mp4
```

### Stream video inline

```bash
curl -X GET http://localhost:8080/stream/{job_id}
```

### Cancel a job

```bash
curl -X DELETE http://localhost:8080/jobs/{job_id}
```

### List jobs with pagination

```bash
curl -X GET "http://localhost:8080/jobs?page=0&page_size=20&status=completed"
```

## Building from Source

```bash
# Clone the repository
git clone https://github.com/0xtrr/aperio.git
cd aperio

# Build
cargo build --release

# Run
cargo run --release
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| APERIO_HOST | Host address to bind | 0.0.0.0 |
| APERIO_PORT | Port to listen on | 8080 |
| APERIO_CLIENT_TIMEOUT | Client request timeout (seconds) | 1800 |
| APERIO_KEEP_ALIVE | Keep-alive duration (seconds) | 1800 |
| APERIO_MAX_PAYLOAD | Maximum payload size (bytes) | 104857600 |
| APERIO_DOWNLOAD_TIMEOUT | Download timeout (seconds) | 900 |
| APERIO_DOWNLOAD_COMMAND | Download command | yt-dlp |
| APERIO_ALLOWED_DOMAINS | Allowed domains (comma-separated) | youtube.com,youtu.be,instagram.com |
| APERIO_PROCESSING_TIMEOUT | Processing timeout (seconds) | 900 |
| APERIO_FFMPEG_COMMAND | FFmpeg command | ffmpeg |
| APERIO_VIDEO_CODEC | Video codec | libx264 |
| APERIO_AUDIO_CODEC | Audio codec | aac |
| APERIO_PRESET | Encoding preset | medium |
| APERIO_CRF | Constant Rate Factor (quality) | 23 |
| APERIO_AUDIO_BITRATE | Audio bitrate | 128k |
| APERIO_MAX_CONCURRENT_DOWNLOADS | Maximum concurrent downloads | 2 |
| APERIO_MAX_CONCURRENT_PROCESSING | Maximum concurrent processing jobs | 1 |
| APERIO_MAX_CONCURRENT_JOBS | Maximum total concurrent jobs | 2 |
| APERIO_STORAGE_PATH | Path for storing files | /app/storage |
| APERIO_WORKING_DIR | Path for temporary files | /app/working |
| APERIO_DATABASE_URL | Database connection string | sqlite:///app/storage/aperio.db |
| APERIO_CORS_ORIGINS | Allowed CORS origins (comma-separated) | Restrictive by default |
| APERIO_MAX_FILE_SIZE_MB | Maximum file download size in MB | 500 |
| APERIO_MAX_URL_LENGTH | Maximum URL length in characters | 2048 |
| APERIO_RETENTION_ENABLED | Enable automatic job retention/cleanup | true |
| APERIO_RETENTION_DAYS | Days to keep completed/failed jobs | 30 |
| APERIO_CLEANUP_INTERVAL_HOURS | Hours between cleanup cycles | 24 |
| APERIO_DB_MAX_CONNECTIONS | Database connection pool size | Auto (4x CPU cores, 10-100) |
| RUST_LOG | Logging level and targets | aperio=info,actix_web=info |
| APERIO_LOG_FORMAT | Log output format (json/pretty) | json |

## Monitoring & Health Checks

Aperio includes comprehensive monitoring and observability features:

### Health Check Endpoints

- **`GET /health`** - Basic health status (returns 200/500 based on health)
- **`GET /health/detailed`** - Detailed health information with component status
- **`GET /health/ready`** - Kubernetes readiness probe (database connectivity)
- **`GET /health/live`** - Kubernetes liveness probe (service responsiveness)
- **`GET /metrics`** - Application metrics in JSON format
- **`GET /metrics/prometheus`** - Prometheus-compatible metrics for monitoring systems
- **`GET /metrics/history`** - Historical metrics data (last 50 points)

### Health Check Response Example
```json
{
  "status": "healthy",
  "timestamp": 1672531200,
  "version": "0.1.0",
  "uptime_seconds": 3600,
  "checks": {
    "database": {
      "status": "healthy",
      "message": "Database connection successful",
      "response_time_ms": 5
    },
    "disk_space": {
      "status": "healthy",
      "message": "Working directory accessible"
    },
    "dependencies": {
      "status": "healthy",
      "message": "All dependencies available"
    }
  }
}
```

### Metrics Response Example

**JSON Format (`/metrics`):**
```json
{
  "counters": {
    "aperio_job_requests_total": {
      "value": 150,
      "labels": {}
    },
    "aperio_jobs_completed_total": {
      "value": 142,
      "labels": {}
    },
    "aperio_jobs_failed_total": {
      "value": 8,
      "labels": {"phase": "download"}
    }
  },
  "gauges": {
    "aperio_jobs_active": {
      "value": 1.0,
      "labels": {}
    }
  },
  "histograms": {
    "aperio_job_duration_ms": {
      "buckets": [[1000.0, 45], [5000.0, 120], [10000.0, 150]],
      "sum": 678540.0,
      "count": 150,
      "labels": {}
    }
  },
  "timestamp": "2024-01-01T12:00:00Z"
}
```

**Prometheus Format (`/metrics/prometheus`):**
```
# TYPE aperio_job_requests_total counter
aperio_job_requests_total 150
# TYPE aperio_jobs_active gauge
aperio_jobs_active 1.0
# TYPE aperio_job_duration_ms histogram
aperio_job_duration_ms_bucket{le="1000"} 45
aperio_job_duration_ms_bucket{le="5000"} 120
aperio_job_duration_ms_bucket{le="+Inf"} 150
aperio_job_duration_ms_sum 678540.0
aperio_job_duration_ms_count 150
```

### Structured Logging

Aperio uses structured logging with configurable output:

```bash
# JSON format (default) - ideal for log aggregation
APERIO_LOG_FORMAT=json

# Pretty format - ideal for development
APERIO_LOG_FORMAT=pretty

# Configure log levels
RUST_LOG=aperio=debug,actix_web=info,sqlx=warn
```

Log entries include:
- Request correlation IDs for tracing
- Structured fields for easy parsing
- Performance metrics and timing
- Error context and debugging information

## Security Features

Aperio includes comprehensive security measures:

### Enhanced URL Validation
- **HTTPS Only**: Only HTTPS URLs are accepted for security
- **Domain Whitelist**: Configurable allowed domains via `APERIO_ALLOWED_DOMAINS`
- **IP Address Blocking**: Prevents access to private/internal IP ranges
- **File Size Limits**: Configurable maximum file downloads (default: 500MB)
- **Path Traversal Protection**: Prevents directory traversal attacks
- **URL Length Limits**: Configurable maximum URL length (default: 2048 chars)

### Security Headers
All responses include security headers:
- **Content Security Policy (CSP)**: Prevents XSS attacks
- **X-Content-Type-Options**: Prevents MIME type sniffing
- **X-Frame-Options**: Prevents clickjacking
- **Strict Transport Security**: Enforces HTTPS
- **Referrer Policy**: Controls referrer information

### CORS Configuration
Configure allowed origins with `APERIO_CORS_ORIGINS`:
```bash
# Allow specific domains
APERIO_CORS_ORIGINS=https://yourdomain.com,https://app.yourdomain.com

# Allow all origins (not recommended for production)
APERIO_CORS_ORIGINS=*
```

### Input Sanitization
- All user inputs are validated and sanitized
- Job IDs are restricted to prevent injection attacks
- URLs undergo comprehensive security validation

## Performance & Optimization

Aperio has been optimized for efficient video processing with minimal resource usage:

### Key Optimizations
- **Smart Format Selection**: yt-dlp preferentially downloads H.264+AAC to minimize re-encoding
- **Optimized FFmpeg Settings**: Balanced quality/speed with `preset=medium`, `crf=23`, and proper threading
- **Event-Driven Architecture**: No CPU-intensive polling loops, uses async notifications
- **Resource Limiting**: Configurable concurrency controls prevent system overload

### Performance Settings
The default configuration is optimized for single-user deployments:
- **2 concurrent downloads** - Prevents excessive yt-dlp processes
- **1 concurrent processing job** - Avoids FFmpeg CPU conflicts  
- **2 total concurrent jobs** - Overall system limit

For high-performance deployments, you can increase these limits via environment variables, but monitor CPU usage as FFmpeg can be very resource-intensive.

## Job Retention & Cleanup

Aperio includes an automated retention system to prevent storage bloat and maintain optimal performance:

### Automatic Cleanup Features
- **Database Cleanup**: Automatically removes old completed, failed, and cancelled jobs from the database
- **File Cleanup**: Removes associated video files and temporary data for deleted jobs
- **Configurable Retention**: Set how long to keep job records and files
- **Background Processing**: Runs cleanup cycles automatically without blocking operations

### Retention Configuration
```bash
# Enable/disable automatic cleanup (default: enabled)
APERIO_RETENTION_ENABLED=true

# Keep jobs for 30 days (default)
APERIO_RETENTION_DAYS=30

# Run cleanup every 24 hours (default)
APERIO_CLEANUP_INTERVAL_HOURS=24
```

### Cleanup Behavior
- **Jobs Cleaned**: Only `Completed`, `Failed`, and `Cancelled` jobs are eligible for cleanup
- **Active Jobs Protected**: `Pending`, `Claimed`, `Downloading`, and `Processing` jobs are never cleaned up
- **File Safety**: Cleanup includes race condition protection to avoid removing files being actively processed
- **Logging**: All cleanup operations are logged with detailed statistics

### Manual Cleanup
While the system runs automatic cleanup, you can also trigger manual cleanup through the retention service API (if exposed) or by restarting the service with a lower retention period temporarily.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
