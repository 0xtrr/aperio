use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    pub name: String,
    pub value: f64,
    pub timestamp: DateTime<Utc>,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Counter {
    pub value: u64,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gauge {
    pub value: f64,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Histogram {
    pub buckets: Vec<(f64, u64)>, // (upper_bound, count)
    pub sum: f64,
    pub count: u64,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingTimeMetrics {
    pub download_duration_ms: Vec<f64>,
    pub processing_duration_ms: Vec<f64>,
    pub total_duration_ms: Vec<f64>,
    pub success_rate: f64,
    pub error_count_by_type: HashMap<String, u64>,
}

pub struct MetricsRegistry {
    counters: Arc<RwLock<HashMap<String, Counter>>>,
    gauges: Arc<RwLock<HashMap<String, Gauge>>>,
    histograms: Arc<RwLock<HashMap<String, Histogram>>>,
    metrics_history: Arc<RwLock<Vec<MetricPoint>>>,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self {
            counters: Arc::new(RwLock::new(HashMap::new())),
            gauges: Arc::new(RwLock::new(HashMap::new())),
            histograms: Arc::new(RwLock::new(HashMap::new())),
            metrics_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Increment a counter metric
    pub async fn increment_counter(&self, name: &str, labels: HashMap<String, String>) {
        let mut counters = self.counters.write().await;
        let counter = counters.entry(name.to_string()).or_insert(Counter {
            value: 0,
            labels: labels.clone(),
        });
        counter.value += 1;
        
        self.record_metric_point(name, counter.value as f64, labels).await;
    }

    /// Set a gauge metric value
    pub async fn set_gauge(&self, name: &str, value: f64, labels: HashMap<String, String>) {
        let mut gauges = self.gauges.write().await;
        gauges.insert(name.to_string(), Gauge {
            value,
            labels: labels.clone(),
        });
        
        self.record_metric_point(name, value, labels).await;
    }

    /// Record a histogram value
    pub async fn record_histogram(&self, name: &str, value: f64, labels: HashMap<String, String>) {
        let mut histograms = self.histograms.write().await;
        let histogram = histograms.entry(name.to_string()).or_insert_with(|| {
            Histogram {
                buckets: vec![
                    (1.0, 0), (5.0, 0), (10.0, 0), (25.0, 0), (50.0, 0),
                    (100.0, 0), (250.0, 0), (500.0, 0), (1000.0, 0), (f64::INFINITY, 0)
                ],
                sum: 0.0,
                count: 0,
                labels: labels.clone(),
            }
        });

        histogram.sum += value;
        histogram.count += 1;

        // Update buckets
        for (upper_bound, count) in &mut histogram.buckets {
            if value <= *upper_bound {
                *count += 1;
            }
        }

        self.record_metric_point(name, value, labels).await;
    }

    /// Record a metric point in history
    async fn record_metric_point(&self, name: &str, value: f64, labels: HashMap<String, String>) {
        let mut history = self.metrics_history.write().await;
        history.push(MetricPoint {
            name: name.to_string(),
            value,
            timestamp: Utc::now(),
            labels,
        });

        // Keep only last 1000 points to prevent memory growth
        if history.len() > 1000 {
            history.drain(0..100);
        }
    }

    /// Get all current metrics as Prometheus format
    pub async fn get_prometheus_format(&self) -> String {
        let mut output = String::new();
        
        // Counters
        let counters = self.counters.read().await;
        for (name, counter) in counters.iter() {
            output.push_str(&format!("# TYPE {name} counter\n"));
            let labels_str = self.format_labels(&counter.labels);
            output.push_str(&format!("{}{{{}}} {}\n", name, labels_str, counter.value));
        }

        // Gauges
        let gauges = self.gauges.read().await;
        for (name, gauge) in gauges.iter() {
            output.push_str(&format!("# TYPE {name} gauge\n"));
            let labels_str = self.format_labels(&gauge.labels);
            output.push_str(&format!("{}{{{}}} {}\n", name, labels_str, gauge.value));
        }

        // Histograms
        let histograms = self.histograms.read().await;
        for (name, histogram) in histograms.iter() {
            output.push_str(&format!("# TYPE {name} histogram\n"));
            let labels_str = self.format_labels(&histogram.labels);
            
            // Buckets
            for (upper_bound, count) in &histogram.buckets {
                let bucket_label = if upper_bound.is_infinite() {
                    "+Inf".to_string()
                } else {
                    upper_bound.to_string()
                };
                let label_prefix = if labels_str.is_empty() { 
                    String::new() 
                } else { 
                    format!("{labels_str},") 
                };
                output.push_str(&format!(
                    "{name}_bucket{{{label_prefix}le=\"{bucket_label}\"}} {count}\n"
                ));
            }
            
            // Sum and count
            output.push_str(&format!("{}_sum{{{}}} {}\n", name, labels_str, histogram.sum));
            output.push_str(&format!("{}_count{{{}}} {}\n", name, labels_str, histogram.count));
        }

        output
    }

    /// Get metrics in JSON format
    pub async fn get_json_format(&self) -> serde_json::Value {
        serde_json::json!({
            "counters": *self.counters.read().await,
            "gauges": *self.gauges.read().await,
            "histograms": *self.histograms.read().await,
            "timestamp": Utc::now()
        })
    }

    /// Get recent metrics history
    pub async fn get_metrics_history(&self, limit: Option<usize>) -> Vec<MetricPoint> {
        let history = self.metrics_history.read().await;
        let limit = limit.unwrap_or(100);
        if history.len() > limit {
            history[history.len() - limit..].to_vec()
        } else {
            history.clone()
        }
    }

    fn format_labels(&self, labels: &HashMap<String, String>) -> String {
        if labels.is_empty() {
            return String::new();
        }
        
        labels.iter()
            .map(|(k, v)| format!("{k}=\"{v}\""))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Global metrics instance
static METRICS: std::sync::OnceLock<MetricsRegistry> = std::sync::OnceLock::new();

/// Get the global metrics registry
pub fn get_metrics() -> &'static MetricsRegistry {
    METRICS.get_or_init(|| {
        info!("Initializing global metrics registry");
        MetricsRegistry::new()
    })
}

/// Convenience macros for common metrics operations
#[macro_export]
macro_rules! counter_inc {
    ($name:expr) => {
        $crate::services::metrics::get_metrics().increment_counter($name, std::collections::HashMap::new()).await
    };
    ($name:expr, $($key:expr => $value:expr),*) => {
        {
            let mut labels = std::collections::HashMap::new();
            $(labels.insert($key.to_string(), $value.to_string());)*
            $crate::services::metrics::get_metrics().increment_counter($name, labels).await
        }
    };
}

#[macro_export]
macro_rules! gauge_set {
    ($name:expr, $value:expr) => {
        $crate::services::metrics::get_metrics().set_gauge($name, $value, std::collections::HashMap::new()).await
    };
    ($name:expr, $value:expr, $($key:expr => $val:expr),*) => {
        {
            let mut labels = std::collections::HashMap::new();
            $(labels.insert($key.to_string(), $val.to_string());)*
            $crate::services::metrics::get_metrics().set_gauge($name, $value, labels).await
        }
    };
}

#[macro_export]
macro_rules! histogram_record {
    ($name:expr, $value:expr) => {
        $crate::services::metrics::get_metrics().record_histogram($name, $value, std::collections::HashMap::new()).await
    };
    ($name:expr, $value:expr, $($key:expr => $val:expr),*) => {
        {
            let mut labels = std::collections::HashMap::new();
            $(labels.insert($key.to_string(), $val.to_string());)*
            $crate::services::metrics::get_metrics().record_histogram($name, $value, labels).await
        }
    };
}