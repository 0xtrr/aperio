use std::time::Duration;

#[derive(Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub download: DownloadConfig,
    pub processing: ProcessingConfig,
    #[allow(dead_code)]
    pub storage: StorageConfig,
    pub security: SecurityConfig,
    pub queue: QueueConfig,
    pub retention: RetentionConfig,
}

#[derive(Clone)]
pub struct QueueConfig {
    pub max_concurrent_jobs: usize,
}

#[derive(Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub client_timeout: Duration,
    pub keep_alive: Duration,
    pub max_payload_size: usize,
}

#[derive(Clone)]
pub struct DownloadConfig {
    pub download_timeout: Duration,
    pub download_command: String,
    pub allowed_domains: Vec<String>,
    pub max_concurrent_downloads: usize,
}

#[derive(Clone)]
pub struct ProcessingConfig {
    pub processing_timeout: Duration,
    pub ffmpeg_command: String,
    pub video_codec: String,
    pub audio_codec: String,
    pub preset: String,
    pub crf: u32,
    pub audio_bitrate: String,
    pub max_concurrent_processing: usize,
}

#[derive(Clone)]
pub struct StorageConfig {
    #[allow(dead_code)]
    pub storage_type: StorageType,
    #[allow(dead_code)]
    pub local_path: Option<String>,
}

#[derive(Clone)]
pub enum StorageType {
    Local,
}

#[derive(Clone)]
pub struct SecurityConfig {
    pub max_file_size_mb: u64,
    pub max_url_length: usize,
    #[allow(dead_code)]
    pub blocked_ips: Vec<String>,
}

#[derive(Clone)]
pub struct RetentionConfig {
    pub enabled: bool,
    pub retention_days: u32,
    pub cleanup_interval_hours: u64,
}

impl Default for Config {
    fn default() -> Self {
        let parse_env_var = |key: &str, default: &str| -> String {
            std::env::var(key).unwrap_or_else(|_| default.to_string())
        };
        
        let parse_env_number = |key: &str, default: u64| -> u64 {
            std::env::var(key)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(default)
        };
        
        let parse_env_duration = |key: &str, default_secs: u64| -> Duration {
            Duration::from_secs(parse_env_number(key, default_secs))
        };

        Config {
            server: ServerConfig {
                host: parse_env_var("APERIO_HOST", "0.0.0.0"),
                port: parse_env_number("APERIO_PORT", 8080) as u16,
                client_timeout: parse_env_duration("APERIO_CLIENT_TIMEOUT", 1800),
                keep_alive: parse_env_duration("APERIO_KEEP_ALIVE", 1800),
                max_payload_size: parse_env_number("APERIO_MAX_PAYLOAD", 100 * 1024 * 1024) as usize,
            },
            download: DownloadConfig {
                download_timeout: parse_env_duration("APERIO_DOWNLOAD_TIMEOUT", 900),
                download_command: parse_env_var("APERIO_DOWNLOAD_COMMAND", "yt-dlp"),
                allowed_domains: parse_env_var("APERIO_ALLOWED_DOMAINS", "youtube.com,youtu.be,instagram.com")
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                max_concurrent_downloads: parse_env_number("APERIO_MAX_CONCURRENT_DOWNLOADS", 2) as usize,
            },
            processing: ProcessingConfig {
                processing_timeout: parse_env_duration("APERIO_PROCESSING_TIMEOUT", 900),
                ffmpeg_command: parse_env_var("APERIO_FFMPEG_COMMAND", "ffmpeg"),
                video_codec: parse_env_var("APERIO_VIDEO_CODEC", "libx264"),
                audio_codec: parse_env_var("APERIO_VIDEO_AUDIO_CODEC", "aac"),
                preset: parse_env_var("APERIO_PRESET", "medium"),
                crf: parse_env_number("APERIO_CRF", 23) as u32,
                audio_bitrate: parse_env_var("APERIO_AUDIO_BITRATE", "128k"),
                max_concurrent_processing: parse_env_number("APERIO_MAX_CONCURRENT_PROCESSING", 1) as usize,
            },
            storage: StorageConfig {
                storage_type: StorageType::Local,
                local_path: Some(parse_env_var("APERIO_STORAGE_PATH", "/app/storage")),
            },
            security: SecurityConfig {
                max_file_size_mb: parse_env_number("APERIO_MAX_FILE_SIZE_MB", 500),
                max_url_length: parse_env_number("APERIO_MAX_URL_LENGTH", 2048) as usize,
                blocked_ips: vec![
                    "127.0.0.1".to_string(),
                    "localhost".to_string(),
                    "0.0.0.0".to_string(),
                ],
            },
            queue: QueueConfig {
                max_concurrent_jobs: parse_env_number("APERIO_MAX_CONCURRENT_JOBS", 2) as usize,
            },
            retention: RetentionConfig {
                enabled: parse_env_var("APERIO_RETENTION_ENABLED", "true").to_lowercase() == "true",
                retention_days: parse_env_number("APERIO_RETENTION_DAYS", 30) as u32,
                cleanup_interval_hours: parse_env_number("APERIO_CLEANUP_INTERVAL_HOURS", 24),
            },
        }
    }
}

pub fn load_config() -> Config {
    Config::default()
}