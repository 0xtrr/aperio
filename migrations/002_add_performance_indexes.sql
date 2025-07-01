-- Additional indexes for better query performance

-- Index on URL for duplicate detection and searching
CREATE INDEX IF NOT EXISTS idx_jobs_url ON jobs(url);

-- Composite index on status and created_at for efficient queue operations
CREATE INDEX IF NOT EXISTS idx_jobs_status_created_at ON jobs(status, created_at);

-- Index on updated_at for cleanup operations
CREATE INDEX IF NOT EXISTS idx_jobs_updated_at ON jobs(updated_at);