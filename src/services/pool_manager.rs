use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::debug;

pub struct ConnectionPoolManager {
    download_semaphore: Arc<Semaphore>,
    processing_semaphore: Arc<Semaphore>,
    max_downloads: usize,
    max_processing: usize,
}

impl ConnectionPoolManager {
    pub fn new(max_concurrent_downloads: usize, max_concurrent_processing: usize) -> Self {
        debug!("Initializing connection pool manager with {} download slots and {} processing slots", 
               max_concurrent_downloads, max_concurrent_processing);
        
        Self {
            download_semaphore: Arc::new(Semaphore::new(max_concurrent_downloads)),
            processing_semaphore: Arc::new(Semaphore::new(max_concurrent_processing)),
            max_downloads: max_concurrent_downloads,
            max_processing: max_concurrent_processing,
        }
    }

    pub async fn acquire_download_permit(&self) -> Result<tokio::sync::SemaphorePermit<'_>, tokio::sync::AcquireError> {
        debug!("Acquiring download permit. Available: {}", self.download_semaphore.available_permits());
        let permit = self.download_semaphore.acquire().await?;
        debug!("Download permit acquired. Remaining: {}", self.download_semaphore.available_permits());
        Ok(permit)
    }

    pub async fn acquire_processing_permit(&self) -> Result<tokio::sync::SemaphorePermit<'_>, tokio::sync::AcquireError> {
        debug!("Acquiring processing permit. Available: {}", self.processing_semaphore.available_permits());
        let permit = self.processing_semaphore.acquire().await?;
        debug!("Processing permit acquired. Remaining: {}", self.processing_semaphore.available_permits());
        Ok(permit)
    }

    #[allow(dead_code)]
    pub fn get_download_stats(&self) -> PoolStats {
        PoolStats {
            available: self.download_semaphore.available_permits(),
            total: self.max_downloads,
        }
    }

    #[allow(dead_code)]
    pub fn get_processing_stats(&self) -> PoolStats {
        PoolStats {
            available: self.processing_semaphore.available_permits(),
            total: self.max_processing,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PoolStats {
    pub available: usize,
    pub total: usize,
}

impl PoolStats {
    #[allow(dead_code)]
    pub fn active(&self) -> usize {
        self.total.saturating_sub(self.available)
    }
}