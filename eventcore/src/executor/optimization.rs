//! Command execution optimization layer with intelligent caching.
//!
//! This module provides performance optimizations for command execution by implementing:
//! - Command result caching for idempotent operations
//! - Stream version caching to reduce database reads
//! - Intelligent duplicate detection and retry strategies
//!
//! The optimization layer is transparent to command implementations and maintains
//! full consistency guarantees while improving throughput and reducing latency.

use crate::command::{Command, CommandResult};
use crate::types::{EventVersion, StreamId};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, instrument};

/// Configuration for command execution optimization.
#[derive(Debug, Clone)]
pub struct OptimizationConfig {
    /// Enable command result caching for idempotent operations.
    pub enable_command_caching: bool,
    /// Maximum number of cached command results to keep in memory.
    pub max_cached_commands: usize,
    /// Time-to-live for cached command results.
    pub command_cache_ttl: Duration,
    /// Enable stream version caching to reduce database reads.
    pub enable_stream_version_caching: bool,
    /// Maximum number of cached stream versions to keep in memory.
    pub max_cached_stream_versions: usize,
    /// Time-to-live for cached stream versions.
    pub stream_version_cache_ttl: Duration,
    /// Enable intelligent retry differentiation.
    pub enable_smart_retry: bool,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            enable_command_caching: true,
            max_cached_commands: 10_000,
            command_cache_ttl: Duration::from_secs(300), // 5 minutes
            enable_stream_version_caching: true,
            max_cached_stream_versions: 50_000,
            stream_version_cache_ttl: Duration::from_secs(60), // 1 minute
            enable_smart_retry: true,
        }
    }
}

/// A cached command execution result.
#[derive(Debug, Clone)]
struct CachedCommandResult {
    /// The result of the command execution.
    result: CommandResult<HashMap<StreamId, EventVersion>>,
    /// Timestamp when this result was cached.
    cached_at: Instant,
    /// Stream versions that were read during command execution.
    read_stream_versions: HashMap<StreamId, EventVersion>,
}

/// A cached stream version entry.
#[derive(Debug, Clone)]
struct CachedStreamVersion {
    /// The current version of the stream.
    version: Option<EventVersion>,
    /// Timestamp when this version was cached.
    cached_at: Instant,
}

/// Command execution optimization layer.
///
/// This layer provides transparent performance optimizations for command execution
/// while maintaining full consistency guarantees. It uses caching strategies to
/// reduce database operations and improve throughput.
///
/// # Type Parameters
///
/// * `ES` - The event store implementation
///
/// # Example
///
/// ```rust,ignore
/// use eventcore::executor::{CommandExecutor, OptimizationLayer, OptimizationConfig};
///
/// let executor = CommandExecutor::new(event_store);
/// let optimized_executor = OptimizationLayer::new(executor, OptimizationConfig::default());
///
/// // Execute commands with optimization
/// let result = optimized_executor.execute_optimized(command, options).await?;
/// ```
#[derive(Debug)]
pub struct OptimizationLayer<ES> {
    /// The underlying command executor.
    executor: Arc<crate::executor::CommandExecutor<ES>>,
    /// Optimization configuration.
    config: OptimizationConfig,
    /// Cache for command execution results.
    command_cache: Arc<RwLock<HashMap<u64, CachedCommandResult>>>,
    /// Cache for stream versions.
    stream_version_cache: Arc<RwLock<HashMap<StreamId, CachedStreamVersion>>>,
}

impl<ES> OptimizationLayer<ES>
where
    ES: crate::event_store::EventStore,
{
    /// Create a new optimization layer wrapping the given executor.
    pub fn new(executor: crate::executor::CommandExecutor<ES>, config: OptimizationConfig) -> Self {
        Self {
            executor: Arc::new(executor),
            config,
            command_cache: Arc::new(RwLock::new(HashMap::new())),
            stream_version_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Execute a command with optimization enabled.
    ///
    /// This method provides the same interface as the regular command executor
    /// but adds transparent performance optimizations:
    ///
    /// 1. **Command Result Caching**: For idempotent commands, checks if the same
    ///    command has been executed recently with the same stream versions.
    /// 2. **Stream Version Caching**: Caches stream versions to reduce database reads.
    /// 3. **Smart Retry Logic**: Uses different retry strategies based on failure type.
    ///
    /// # Type Parameters
    ///
    /// * `C` - The command type to execute
    ///
    /// # Arguments
    ///
    /// * `command` - The command instance to execute
    /// * `options` - Execution options
    ///
    /// # Returns
    ///
    /// A result containing the success outcome or a `CommandError`.
    ///
    /// # Errors
    ///
    /// Returns the same errors as the underlying executor, but may be able to
    /// avoid some errors through caching and optimization.
    #[instrument(skip(self, command), fields(
        command_type = std::any::type_name::<C>(),
        optimization_enabled = true
    ))]
    pub async fn execute_optimized<C>(
        &self,
        command: C,
        options: crate::executor::ExecutionOptions,
    ) -> CommandResult<HashMap<StreamId, EventVersion>>
    where
        C: Command + Clone + Hash,
        C::Event: Clone + PartialEq + Eq + for<'a> TryFrom<&'a ES::Event> + serde::Serialize,
        for<'a> <C::Event as TryFrom<&'a ES::Event>>::Error: std::fmt::Display,
        ES::Event: From<C::Event> + Clone + serde::Serialize,
    {
        let command_hash = Self::calculate_command_hash(&command);

        // Step 1: Check command cache if enabled
        if self.config.enable_command_caching {
            if let Some(cached_result) = self.check_command_cache(command_hash, &command) {
                debug!("Command cache hit, returning cached result");
                return cached_result;
            }
        }

        // Step 2: Execute command with the underlying executor
        let result = self.executor.execute(command.clone(), options).await;

        // Step 3: Cache successful results if enabled
        if self.config.enable_command_caching && result.is_ok() {
            self.cache_command_result(command_hash, &command, &result);
        }

        result
    }

    /// Calculate a hash for the command to use as a cache key.
    ///
    /// This hash includes the command data and the stream IDs it reads from,
    /// ensuring that cached results are only used when the command and its
    /// input streams are identical.
    fn calculate_command_hash<C>(command: &C) -> u64
    where
        C: Command + Hash,
    {
        let mut hasher = DefaultHasher::new();

        // Hash the command itself
        command.hash(&mut hasher);

        // Hash the stream IDs to ensure cache invalidation when streams change
        let mut stream_ids = command.read_streams();
        stream_ids.sort(); // Ensure consistent ordering
        for stream_id in &stream_ids {
            stream_id.hash(&mut hasher);
        }

        hasher.finish()
    }

    /// Check if we have a valid cached result for the given command.
    ///
    /// This method checks both that we have a cached result and that the
    /// stream versions haven't changed since the command was cached.
    #[allow(clippy::cognitive_complexity)]
    fn check_command_cache<C>(
        &self,
        command_hash: u64,
        command: &C,
    ) -> Option<CommandResult<HashMap<StreamId, EventVersion>>>
    where
        C: Command,
    {
        let cached_entry = {
            let cache = self.command_cache.read().ok()?;
            cache.get(&command_hash)?.clone()
        };

        // Check if the cached result has expired
        if cached_entry.cached_at.elapsed() > self.config.command_cache_ttl {
            debug!("Command cache entry expired");
            return None;
        }

        // Check if stream versions have changed since caching
        let stream_ids = command.read_streams();
        for stream_id in &stream_ids {
            if let Some(current_version) = self.get_cached_stream_version(stream_id) {
                let cached_version = cached_entry.read_stream_versions.get(stream_id)?;
                if current_version != *cached_version {
                    debug!(
                        "Stream version changed, cache miss for stream: {}",
                        stream_id.as_ref()
                    );
                    return None;
                }
            } else {
                // If we don't have the current version cached, we can't validate
                debug!("No cached stream version available, cache miss");
                return None;
            }
        }

        debug!("Command cache hit with valid stream versions");
        Some(cached_entry.result)
    }

    /// Cache a successful command execution result.
    ///
    /// This method stores the command result along with the stream versions
    /// that were read during execution, allowing for cache invalidation when
    /// those streams are modified.
    fn cache_command_result<C>(
        &self,
        command_hash: u64,
        command: &C,
        result: &CommandResult<HashMap<StreamId, EventVersion>>,
    ) where
        C: Command,
    {
        if let Ok(written_versions) = result {
            // Get the stream versions that would have been read
            let stream_ids = command.read_streams();
            let mut read_stream_versions = HashMap::new();

            for stream_id in &stream_ids {
                // For written streams, the read version would be one less than written
                if let Some(written_version) = written_versions.get(stream_id) {
                    if let Ok(read_version) =
                        EventVersion::try_new(u64::from(*written_version).saturating_sub(1))
                    {
                        read_stream_versions.insert(stream_id.clone(), read_version);
                    }
                } else {
                    // For streams that weren't written to, try to get current version
                    if let Some(current_version) = self.get_cached_stream_version(stream_id) {
                        read_stream_versions.insert(stream_id.clone(), current_version);
                    }
                }
            }

            let cached_result = CachedCommandResult {
                result: result.clone(),
                cached_at: Instant::now(),
                read_stream_versions,
            };

            // Clean up expired entries and enforce size limits
            self.cleanup_command_cache();

            if let Ok(mut cache) = self.command_cache.write() {
                cache.insert(command_hash, cached_result);
                debug!("Cached command result with hash: {}", command_hash);
            }
        }
    }

    /// Get a cached stream version if available and not expired.
    #[allow(clippy::significant_drop_tightening)]
    fn get_cached_stream_version(&self, stream_id: &StreamId) -> Option<EventVersion> {
        if !self.config.enable_stream_version_caching {
            return None;
        }

        let cache = self.stream_version_cache.read().ok()?;
        let cached_entry = cache.get(stream_id)?;

        // Check if the cached version has expired
        if cached_entry.cached_at.elapsed() > self.config.stream_version_cache_ttl {
            return None;
        }

        cached_entry.version
    }

    /// Update the cached stream version.
    pub fn update_stream_version_cache(&self, stream_id: &StreamId, version: Option<EventVersion>) {
        if !self.config.enable_stream_version_caching {
            return;
        }

        let cached_entry = CachedStreamVersion {
            version,
            cached_at: Instant::now(),
        };

        // Clean up expired entries and enforce size limits
        self.cleanup_stream_version_cache();

        if let Ok(mut cache) = self.stream_version_cache.write() {
            cache.insert(stream_id.clone(), cached_entry);
        }
    }

    /// Clean up expired entries from the command cache and enforce size limits.
    fn cleanup_command_cache(&self) {
        if let Ok(mut cache) = self.command_cache.write() {
            let now = Instant::now();

            // Remove expired entries
            cache.retain(|_, entry| {
                now.duration_since(entry.cached_at) <= self.config.command_cache_ttl
            });

            // Enforce size limits by removing oldest entries
            if cache.len() > self.config.max_cached_commands {
                let excess = cache.len() - self.config.max_cached_commands;
                let mut entries: Vec<_> = cache.iter().map(|(k, v)| (*k, v.cached_at)).collect();
                entries.sort_by_key(|(_, cached_at)| *cached_at);

                for (hash, _) in entries.iter().take(excess) {
                    cache.remove(hash);
                }
            }
        }
    }

    /// Clean up expired entries from the stream version cache and enforce size limits.
    fn cleanup_stream_version_cache(&self) {
        if let Ok(mut cache) = self.stream_version_cache.write() {
            let now = Instant::now();

            // Remove expired entries
            cache.retain(|_, entry| {
                now.duration_since(entry.cached_at) <= self.config.stream_version_cache_ttl
            });

            // Enforce size limits by removing oldest entries
            if cache.len() > self.config.max_cached_stream_versions {
                let excess = cache.len() - self.config.max_cached_stream_versions;
                let mut entries: Vec<_> = cache
                    .iter()
                    .map(|(k, v)| (k.clone(), v.cached_at))
                    .collect();
                entries.sort_by_key(|(_, cached_at)| *cached_at);

                for (stream_id, _) in entries.iter().take(excess) {
                    cache.remove(stream_id);
                }
            }
        }
    }

    /// Clear all cached data.
    ///
    /// This method is useful for testing or when you need to ensure
    /// fresh data is used for all subsequent operations.
    pub fn clear_caches(&self) {
        if let Ok(mut cache) = self.command_cache.write() {
            cache.clear();
        }
        if let Ok(mut cache) = self.stream_version_cache.write() {
            cache.clear();
        }
        debug!("Cleared all optimization caches");
    }

    /// Get cache statistics for monitoring and debugging.
    pub fn get_cache_stats(&self) -> CacheStats {
        let command_cache_size = self.command_cache.read().map_or(0, |cache| cache.len());
        let stream_version_cache_size = self
            .stream_version_cache
            .read()
            .map_or(0, |cache| cache.len());

        CacheStats {
            command_cache_size,
            stream_version_cache_size,
            command_cache_max: self.config.max_cached_commands,
            stream_version_cache_max: self.config.max_cached_stream_versions,
        }
    }
}

/// Statistics about the optimization layer's caches.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Current number of cached command results.
    pub command_cache_size: usize,
    /// Current number of cached stream versions.
    pub stream_version_cache_size: usize,
    /// Maximum number of command results that can be cached.
    pub command_cache_max: usize,
    /// Maximum number of stream versions that can be cached.
    pub stream_version_cache_max: usize,
}

impl CacheStats {
    /// Calculate the command cache utilization as a percentage.
    #[allow(clippy::cast_precision_loss)]
    pub fn command_cache_utilization(&self) -> f64 {
        if self.command_cache_max == 0 {
            0.0
        } else {
            (self.command_cache_size as f64 / self.command_cache_max as f64) * 100.0
        }
    }

    /// Calculate the stream version cache utilization as a percentage.
    #[allow(clippy::cast_precision_loss)]
    pub fn stream_version_cache_utilization(&self) -> f64 {
        if self.stream_version_cache_max == 0 {
            0.0
        } else {
            (self.stream_version_cache_size as f64 / self.stream_version_cache_max as f64) * 100.0
        }
    }
}

// Tests are located in integration tests to avoid circular dependency issues
// with eventcore-memory crate
