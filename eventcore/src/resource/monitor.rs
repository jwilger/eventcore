//! Resource monitoring and leak detection
//!
//! This module provides utilities for monitoring resource usage
//! and detecting potential resource leaks in the application.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Resource leak detector for debugging and monitoring
#[derive(Debug, Default)]
pub struct ResourceLeakDetector {
    active_resources: Mutex<HashMap<String, ResourceInfo>>,
}

#[derive(Debug, Clone)]
struct ResourceInfo {
    resource_type: String,
    acquired_at: Instant,
    location: Option<String>,
}

impl ResourceLeakDetector {
    /// Create a new leak detector
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a resource acquisition
    pub fn register_acquisition(
        &self,
        resource_id: &str,
        resource_type: &str,
        location: Option<String>,
    ) {
        if let Ok(mut resources) = self.active_resources.lock() {
            resources.insert(
                resource_id.to_string(),
                ResourceInfo {
                    resource_type: resource_type.to_string(),
                    acquired_at: Instant::now(),
                    location,
                },
            );
        }
    }

    /// Register a resource release
    pub fn register_release(&self, resource_id: &str) {
        if let Ok(mut resources) = self.active_resources.lock() {
            resources.remove(resource_id);
        }
    }

    /// Get statistics about active resources
    pub fn get_stats(&self) -> ResourceLeakStats {
        self.active_resources.lock().map_or_else(
            |_| ResourceLeakStats::default(),
            |resources| {
                let total_count = resources.len();
                let mut by_type = HashMap::new();
                let mut oldest_age = Duration::ZERO;

                for info in resources.values() {
                    *by_type.entry(info.resource_type.clone()).or_insert(0) += 1;
                    let age = info.acquired_at.elapsed();
                    if age > oldest_age {
                        oldest_age = age;
                    }
                }

                ResourceLeakStats {
                    total_active: total_count,
                    by_type,
                    oldest_resource_age: oldest_age,
                }
            },
        )
    }

    /// Find potentially leaked resources (older than threshold)
    pub fn find_potential_leaks(&self, threshold: Duration) -> Vec<String> {
        self.active_resources.lock().map_or_else(
            |_| Vec::new(),
            |resources| {
                resources
                    .iter()
                    .filter(|(_, info)| info.acquired_at.elapsed() > threshold)
                    .map(|(id, _)| id.clone())
                    .collect()
            },
        )
    }
}

/// Statistics about resource usage and potential leaks
#[derive(Debug, Default)]
pub struct ResourceLeakStats {
    /// Total number of active resources
    pub total_active: usize,
    /// Count of active resources by type
    pub by_type: HashMap<String, usize>,
    /// Age of the oldest active resource
    pub oldest_resource_age: Duration,
}

/// Global resource leak detector instance
static GLOBAL_LEAK_DETECTOR: OnceLock<ResourceLeakDetector> = OnceLock::new();

/// Get the global resource leak detector
pub fn global_leak_detector() -> &'static ResourceLeakDetector {
    GLOBAL_LEAK_DETECTOR.get_or_init(ResourceLeakDetector::new)
}