-- Create jobs table for persistent job storage
CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY,
    url TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    downloaded_path TEXT,
    processed_path TEXT,
    error_message TEXT,
    processing_time_seconds INTEGER
);

-- Index on status for efficient queries
CREATE INDEX idx_jobs_status ON jobs(status);

-- Index on created_at for ordering
CREATE INDEX idx_jobs_created_at ON jobs(created_at);
