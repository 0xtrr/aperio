use std::collections::{HashMap, BinaryHeap};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tracing::{info, warn, debug};
use crate::models::job::Job;
use crate::api::routes::AppState;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JobPriority {
    Low = 1,
    Normal = 2,
    High = 3,
}

#[derive(Debug, Clone)]
pub struct QueuedJob {
    pub job: Job,
    pub priority: JobPriority,
    pub queued_at: chrono::DateTime<chrono::Utc>,
}

// Implement ordering for BinaryHeap (higher priority first)
impl PartialEq for QueuedJob {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.queued_at == other.queued_at
    }
}

impl Eq for QueuedJob {}

impl PartialOrd for QueuedJob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueuedJob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher priority comes first, then older jobs (FIFO within same priority)
        self.priority.cmp(&other.priority)
            .then_with(|| other.queued_at.cmp(&self.queued_at))
    }
}

impl QueuedJob {
    pub fn new(job: Job, priority: JobPriority) -> Self {
        Self {
            job,
            priority,
            queued_at: chrono::Utc::now(),
        }
    }
}

pub struct JobQueue {
    queue: Arc<Mutex<BinaryHeap<QueuedJob>>>,
    notify: Arc<Notify>,
    active_jobs: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    max_concurrent_jobs: usize,
    max_queue_size: usize,
    is_shutdown: Arc<Mutex<bool>>,
}

impl JobQueue {
    pub fn new(max_concurrent_jobs: usize) -> Self {
        let max_queue_size = std::env::var("APERIO_MAX_QUEUE_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000); // Default to 1000 jobs max in queue
            
        info!("Initializing job queue with max {} concurrent jobs and max {} queued jobs", 
              max_concurrent_jobs, max_queue_size);
        
        Self {
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
            notify: Arc::new(Notify::new()),
            active_jobs: Arc::new(Mutex::new(HashMap::new())),
            max_concurrent_jobs,
            max_queue_size,
            is_shutdown: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn enqueue(&self, job: Job, priority: JobPriority) -> Result<(), String> {
        let is_shutdown = *self.is_shutdown.lock().await;
        if is_shutdown {
            return Err("Job queue is shutting down".to_string());
        }

        let queued_job = QueuedJob::new(job.clone(), priority.clone());
        let mut queue = self.queue.lock().await;
        
        // Check queue size limit
        if queue.len() >= self.max_queue_size {
            return Err(format!("Queue is full (max {} jobs), try again later", self.max_queue_size));
        }
        
        // BinaryHeap automatically orders by priority (O(log n) insertion)
        queue.push(queued_job);
        
        info!("Enqueued job {} with priority {:?}, queue size: {}", 
              job.id, priority, queue.len());
        
        // Notify worker that new job is available
        self.notify.notify_one();
        
        Ok(())
    }

    pub async fn start_worker(&self, app_state: Arc<AppState>) {
        let queue = self.queue.clone();
        let notify = self.notify.clone();
        let active_jobs = self.active_jobs.clone();
        let max_concurrent = self.max_concurrent_jobs;
        let is_shutdown = self.is_shutdown.clone();

        tokio::spawn(async move {
            info!("Job queue worker started");
            
            loop {
                // Check if we should shutdown
                {
                    let shutdown = *is_shutdown.lock().await;
                    if shutdown {
                        info!("Job queue worker shutting down");
                        break;
                    }
                }

                // Wait for notification only - no periodic polling
                notify.notified().await;

                // Clean up completed jobs
                {
                    let mut active = active_jobs.lock().await;
                    active.retain(|job_id, handle| {
                        if handle.is_finished() {
                            debug!("Job {} completed, removing from active jobs", job_id);
                            false
                        } else {
                            true
                        }
                    });
                }

                // Process as many jobs as we can until we hit the limit or run out of jobs
                loop {
                    // Check if we can start new jobs
                    let current_active = {
                        let active = active_jobs.lock().await;
                        active.len()
                    };

                    if current_active >= max_concurrent {
                        debug!("Max concurrent jobs reached ({}/{}), waiting for notification", current_active, max_concurrent);
                        break;
                    }

                    // Get next job from queue (highest priority first)
                    let next_job = {
                        let mut queue = queue.lock().await;
                        queue.pop()
                    };

                    if let Some(queued_job) = next_job {
                        let job_id = queued_job.job.id.clone();
                        let job_id_for_cleanup = job_id.clone();
                        let app_state_clone = app_state.clone();
                        let active_jobs_clone = active_jobs.clone();
                        let notify_clone = notify.clone();
                        
                        info!("Starting job {} (priority: {:?}, queued for: {:?})", 
                              job_id, 
                              queued_job.priority,
                              chrono::Utc::now().signed_duration_since(queued_job.queued_at));
                        
                        // Spawn job processing directly without TaskManager overhead
                        let handle = tokio::spawn(async move {
                            crate::api::routes::process_job(&job_id_for_cleanup, app_state_clone).await;
                            
                            // Remove from active jobs when done and notify worker
                            {
                                let mut active = active_jobs_clone.lock().await;
                                active.remove(&job_id_for_cleanup);
                            }
                            notify_clone.notify_one();
                        });
                        
                        // Track the job
                        {
                            let mut active = active_jobs.lock().await;
                            active.insert(job_id, handle);
                        }
                    } else {
                        // No more jobs in queue
                        debug!("No more jobs in queue");
                        break;
                    }
                }
            }
        });
    }

    #[allow(dead_code)]
    pub async fn get_queue_stats(&self) -> QueueStats {
        let queue = self.queue.lock().await;
        let active_jobs = self.active_jobs.lock().await;
        
        let mut priority_counts = HashMap::new();
        for queued_job in queue.iter() {
            *priority_counts.entry(queued_job.priority.clone()).or_insert(0) += 1;
        }

        QueueStats {
            queued_jobs: queue.len(),
            active_jobs: active_jobs.len(),
            max_concurrent_jobs: self.max_concurrent_jobs,
            priority_breakdown: priority_counts,
        }
    }

    pub async fn cancel_job(&self, job_id: &str) -> Result<bool, String> {
        // Atomic cancellation with proper coordination
        let mut cancelled = false;
        
        // Step 1: Try to cancel active job
        {
            let mut active = self.active_jobs.lock().await;
            if let Some(handle) = active.remove(job_id) {
                handle.abort();
                info!("Cancelled active job: {}", job_id);
                cancelled = true;
            }
        }

        // Step 2: Try to remove from queue
        {
            let mut queue = self.queue.lock().await;
            let mut temp_jobs = Vec::new();
            let mut found_in_queue = false;
            
            // Drain the queue to find and remove the target job
            while let Some(queued_job) = queue.pop() {
                if queued_job.job.id == job_id && !found_in_queue {
                    found_in_queue = true;
                    info!("Cancelled queued job: {}", job_id);
                    cancelled = true;
                } else {
                    temp_jobs.push(queued_job);
                }
            }
            
            // Rebuild the queue with remaining jobs
            for job in temp_jobs {
                queue.push(job);
            }
        }

        Ok(cancelled)
    }

    /// Get queue statistics safely
    #[allow(dead_code)]
    pub async fn get_queue_info(&self) -> (usize, usize) {
        let queue = self.queue.lock().await;
        let active = self.active_jobs.lock().await;
        (queue.len(), active.len())
    }

    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        info!("Shutting down job queue");
        
        // Mark as shutdown
        {
            let mut shutdown = self.is_shutdown.lock().await;
            *shutdown = true;
        }

        // Cancel all active jobs
        {
            let mut active = self.active_jobs.lock().await;
            for (job_id, handle) in active.drain() {
                warn!("Aborting job {} due to shutdown", job_id);
                handle.abort();
            }
        }

        // Clear queue
        {
            let mut queue = self.queue.lock().await;
            let remaining = queue.len();
            queue.clear();
            if remaining > 0 {
                warn!("Cancelled {} queued jobs due to shutdown", remaining);
            }
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct QueueStats {
    pub queued_jobs: usize,
    pub active_jobs: usize,
    pub max_concurrent_jobs: usize,
    pub priority_breakdown: HashMap<JobPriority, usize>,
}