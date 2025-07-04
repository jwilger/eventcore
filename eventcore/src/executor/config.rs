//! Enhanced configuration system with type-safe validation.
//!
//! This module provides comprehensive configuration types that use `nutype`
//! validation to prevent invalid configurations from being constructed.
//! All configuration parameters are validated at creation time, eliminating
//! runtime configuration errors.

use nutype::nutype;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Maximum number of retry attempts for command execution.
///
/// Validated to be between 1 and 10 attempts to prevent infinite loops
/// while allowing reasonable retry behavior.
#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 10),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct MaxRetryAttempts(u32);

/// Base delay between retry attempts in milliseconds.
///
/// Validated to be between 10ms and 10 seconds to ensure reasonable
/// retry timing that doesn't overwhelm the system or cause excessive delays.
#[nutype(
    validate(greater_or_equal = 10, less_or_equal = 10_000),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct RetryBaseDelayMs(u64);

impl RetryBaseDelayMs {
    /// Convert to Duration for use with tokio::time::sleep.
    pub fn as_duration(self) -> Duration {
        Duration::from_millis(self.into())
    }
}

/// Maximum delay between retry attempts in milliseconds.
///
/// Validated to be between 100ms and 5 minutes to prevent excessive
/// delays while allowing reasonable exponential backoff.
#[nutype(
    validate(greater_or_equal = 100, less_or_equal = 300_000),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct RetryMaxDelayMs(u64);

impl RetryMaxDelayMs {
    /// Convert to Duration for use with tokio::time::sleep.
    pub fn as_duration(self) -> Duration {
        Duration::from_millis(self.into())
    }
}

/// Exponential backoff multiplier for retry delays.
///
/// Validated to be between 1.1 and 3.0 to ensure exponential growth
/// without creating excessively long delays.
#[nutype(
    validate(greater_or_equal = 1.1, less_or_equal = 3.0),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        PartialOrd,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct BackoffMultiplier(f64);

/// Maximum number of stream discovery iterations.
///
/// Validated to be between 1 and 100 to prevent infinite loops in
/// dynamic stream discovery while allowing complex workflows.
#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 100),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct MaxStreamDiscoveryIterations(usize);

/// Event store operation timeout in milliseconds.
///
/// Validated to be between 1 second and 10 minutes to ensure operations
/// have reasonable timeouts without hanging indefinitely.
#[nutype(
    validate(greater_or_equal = 1_000, less_or_equal = 600_000),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct EventStoreTimeoutMs(u64);

impl EventStoreTimeoutMs {
    /// Convert to Duration for use with tokio::time::timeout.
    pub fn as_duration(self) -> Duration {
        Duration::from_millis(self.into())
    }
}

/// Overall command execution timeout in milliseconds.
///
/// Validated to be between 5 seconds and 1 hour to ensure commands
/// complete in reasonable time while allowing for complex operations.
#[nutype(
    validate(greater_or_equal = 5_000, less_or_equal = 3_600_000),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct CommandTimeoutMs(u64);

impl CommandTimeoutMs {
    /// Convert to Duration for use with tokio::time::timeout.
    pub fn as_duration(self) -> Duration {
        Duration::from_millis(self.into())
    }
}

/// Maximum number of items to cache in memory.
///
/// Validated to be between 100 and 1 million to provide useful caching
/// while preventing excessive memory usage.
#[nutype(
    validate(greater_or_equal = 100, less_or_equal = 1_000_000),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct MaxCacheSize(usize);

/// Cache entry time-to-live in seconds.
///
/// Validated to be between 1 second and 24 hours to ensure cached
/// data doesn't become stale while providing useful performance benefits.
#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 86_400),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct CacheTtlSeconds(u64);

impl CacheTtlSeconds {
    /// Convert to Duration for cache expiration checking.
    pub fn as_duration(self) -> Duration {
        Duration::from_secs(self.into())
    }
}

/// Database connection pool size.
///
/// Validated to be between 1 and 100 connections to ensure the application
/// can connect to the database while preventing connection pool exhaustion.
#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 100),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct PoolSize(u32);

/// Database query timeout in seconds.
///
/// Validated to be between 1 second and 5 minutes to ensure queries
/// complete in reasonable time while preventing indefinite hangs.
#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 300),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Into,
        Serialize,
        Deserialize
    )
)]
pub struct QueryTimeoutSeconds(u64);

impl QueryTimeoutSeconds {
    /// Convert to Duration for use with database query timeouts.
    pub fn as_duration(self) -> Duration {
        Duration::from_secs(self.into())
    }
}

/// Enhanced retry configuration with type-safe validation.
///
/// All parameters are validated at construction time to prevent
/// invalid configurations that could cause runtime failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedRetryConfig {
    /// Maximum number of retry attempts.
    pub max_attempts: MaxRetryAttempts,
    /// Base delay between retry attempts.
    pub base_delay: RetryBaseDelayMs,
    /// Maximum delay between retry attempts.
    pub max_delay: RetryMaxDelayMs,
    /// Multiplier for exponential backoff.
    pub backoff_multiplier: BackoffMultiplier,
}

impl ValidatedRetryConfig {
    /// Create a new retry configuration with safe defaults.
    ///
    /// # Errors
    ///
    /// Returns validation errors if any of the default values are invalid
    /// (which should never happen with proper constants).
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            max_attempts: MaxRetryAttempts::try_new(3)?,
            base_delay: RetryBaseDelayMs::try_new(100)?,
            max_delay: RetryMaxDelayMs::try_new(30_000)?,
            backoff_multiplier: BackoffMultiplier::try_new(2.0)?,
        })
    }

    /// Create a conservative retry configuration with fewer attempts and longer delays.
    ///
    /// Useful for operations that should not be retried aggressively.
    pub fn conservative() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            max_attempts: MaxRetryAttempts::try_new(2)?,
            base_delay: RetryBaseDelayMs::try_new(500)?,
            max_delay: RetryMaxDelayMs::try_new(60_000)?,
            backoff_multiplier: BackoffMultiplier::try_new(2.5)?,
        })
    }

    /// Create an aggressive retry configuration with more attempts and shorter delays.
    ///
    /// Useful for operations that need to be retried quickly.
    pub fn aggressive() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            max_attempts: MaxRetryAttempts::try_new(5)?,
            base_delay: RetryBaseDelayMs::try_new(50)?,
            max_delay: RetryMaxDelayMs::try_new(10_000)?,
            backoff_multiplier: BackoffMultiplier::try_new(1.5)?,
        })
    }

    /// Convert to the legacy RetryConfig for compatibility.
    pub fn to_legacy_config(&self) -> crate::executor::RetryConfig {
        crate::executor::RetryConfig {
            max_attempts: self.max_attempts.into(),
            base_delay: self.base_delay.as_duration(),
            max_delay: self.max_delay.as_duration(),
            backoff_multiplier: self.backoff_multiplier.into(),
        }
    }
}

impl Default for ValidatedRetryConfig {
    fn default() -> Self {
        Self::new().expect("Default retry configuration should always be valid")
    }
}

/// Enhanced execution options with type-safe validation.
///
/// All parameters are validated at construction time to eliminate
/// runtime configuration errors and ensure system stability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedExecutionOptions {
    /// Retry configuration.
    pub retry_config: Option<ValidatedRetryConfig>,
    /// Maximum number of stream discovery iterations.
    pub max_stream_discovery_iterations: MaxStreamDiscoveryIterations,
    /// Timeout for individual EventStore operations.
    pub event_store_timeout: Option<EventStoreTimeoutMs>,
    /// Overall timeout for command execution.
    pub command_timeout: Option<CommandTimeoutMs>,
}

impl ValidatedExecutionOptions {
    /// Create execution options with safe defaults.
    ///
    /// # Errors
    ///
    /// Returns validation errors if any of the default values are invalid.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            retry_config: Some(ValidatedRetryConfig::new()?),
            max_stream_discovery_iterations: MaxStreamDiscoveryIterations::try_new(10)?,
            event_store_timeout: Some(EventStoreTimeoutMs::try_new(30_000)?),
            command_timeout: None, // No overall timeout by default
        })
    }

    /// Create execution options optimized for high-performance scenarios.
    ///
    /// Uses aggressive retry and shorter timeouts for maximum throughput.
    pub fn high_performance() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            retry_config: Some(ValidatedRetryConfig::aggressive()?),
            max_stream_discovery_iterations: MaxStreamDiscoveryIterations::try_new(5)?,
            event_store_timeout: Some(EventStoreTimeoutMs::try_new(10_000)?),
            command_timeout: Some(CommandTimeoutMs::try_new(30_000)?),
        })
    }

    /// Create execution options optimized for reliability over performance.
    ///
    /// Uses conservative retry and longer timeouts for maximum reliability.
    pub fn high_reliability() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            retry_config: Some(ValidatedRetryConfig::conservative()?),
            max_stream_discovery_iterations: MaxStreamDiscoveryIterations::try_new(20)?,
            event_store_timeout: Some(EventStoreTimeoutMs::try_new(60_000)?),
            command_timeout: Some(CommandTimeoutMs::try_new(300_000)?),
        })
    }

    /// Disable retries entirely.
    #[must_use]
    pub const fn without_retry(mut self) -> Self {
        self.retry_config = None;
        self
    }

    /// Set custom retry configuration.
    #[must_use]
    pub const fn with_retry_config(mut self, config: ValidatedRetryConfig) -> Self {
        self.retry_config = Some(config);
        self
    }

    /// Set custom stream discovery iterations limit.
    #[must_use]
    pub const fn with_max_stream_discovery_iterations(
        mut self,
        iterations: MaxStreamDiscoveryIterations,
    ) -> Self {
        self.max_stream_discovery_iterations = iterations;
        self
    }

    /// Set custom event store timeout.
    #[must_use]
    pub const fn with_event_store_timeout(mut self, timeout: Option<EventStoreTimeoutMs>) -> Self {
        self.event_store_timeout = timeout;
        self
    }

    /// Set custom command timeout.
    #[must_use]
    pub const fn with_command_timeout(mut self, timeout: Option<CommandTimeoutMs>) -> Self {
        self.command_timeout = timeout;
        self
    }

    /// Convert to the legacy ExecutionOptions for compatibility.
    pub fn to_legacy_options(&self) -> crate::executor::ExecutionOptions {
        crate::executor::ExecutionOptions {
            context: crate::executor::ExecutionContext::default(),
            retry_config: self
                .retry_config
                .as_ref()
                .map(ValidatedRetryConfig::to_legacy_config),
            retry_policy: crate::executor::RetryPolicy::default(),
            max_stream_discovery_iterations: self.max_stream_discovery_iterations.into(),
            event_store_timeout: self
                .event_store_timeout
                .map(EventStoreTimeoutMs::as_duration),
            command_timeout: self.command_timeout.map(CommandTimeoutMs::as_duration),
            circuit_breaker_config: None, // Not supported in validated options yet
        }
    }
}

impl Default for ValidatedExecutionOptions {
    fn default() -> Self {
        Self::new().expect("Default execution options should always be valid")
    }
}

/// Enhanced optimization configuration with type-safe validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedOptimizationConfig {
    /// Enable command result caching.
    pub enable_command_caching: bool,
    /// Maximum number of cached command results.
    pub max_cached_commands: MaxCacheSize,
    /// Time-to-live for cached command results.
    pub command_cache_ttl: CacheTtlSeconds,
    /// Enable stream version caching.
    pub enable_stream_version_caching: bool,
    /// Maximum number of cached stream versions.
    pub max_cached_stream_versions: MaxCacheSize,
    /// Time-to-live for cached stream versions.
    pub stream_version_cache_ttl: CacheTtlSeconds,
    /// Enable smart retry logic.
    pub enable_smart_retry: bool,
}

impl ValidatedOptimizationConfig {
    /// Create optimization configuration with safe defaults.
    ///
    /// # Errors
    ///
    /// Returns validation errors if any of the default values are invalid.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            enable_command_caching: true,
            max_cached_commands: MaxCacheSize::try_new(10_000)?,
            command_cache_ttl: CacheTtlSeconds::try_new(300)?, // 5 minutes
            enable_stream_version_caching: true,
            max_cached_stream_versions: MaxCacheSize::try_new(50_000)?,
            stream_version_cache_ttl: CacheTtlSeconds::try_new(60)?, // 1 minute
            enable_smart_retry: true,
        })
    }

    /// Create configuration optimized for memory-constrained environments.
    pub fn memory_efficient() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            enable_command_caching: true,
            max_cached_commands: MaxCacheSize::try_new(1_000)?,
            command_cache_ttl: CacheTtlSeconds::try_new(120)?, // 2 minutes
            enable_stream_version_caching: true,
            max_cached_stream_versions: MaxCacheSize::try_new(5_000)?,
            stream_version_cache_ttl: CacheTtlSeconds::try_new(30)?, // 30 seconds
            enable_smart_retry: true,
        })
    }

    /// Create configuration optimized for maximum performance.
    pub fn high_performance() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            enable_command_caching: true,
            max_cached_commands: MaxCacheSize::try_new(100_000)?,
            command_cache_ttl: CacheTtlSeconds::try_new(600)?, // 10 minutes
            enable_stream_version_caching: true,
            max_cached_stream_versions: MaxCacheSize::try_new(500_000)?,
            stream_version_cache_ttl: CacheTtlSeconds::try_new(300)?, // 5 minutes
            enable_smart_retry: true,
        })
    }

    /// Disable all caching.
    pub fn no_caching() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            enable_command_caching: false,
            max_cached_commands: MaxCacheSize::try_new(100)?, // Minimum required
            command_cache_ttl: CacheTtlSeconds::try_new(1)?,
            enable_stream_version_caching: false,
            max_cached_stream_versions: MaxCacheSize::try_new(100)?,
            stream_version_cache_ttl: CacheTtlSeconds::try_new(1)?,
            enable_smart_retry: true,
        })
    }

    /// Convert to the legacy OptimizationConfig for compatibility.
    pub fn to_legacy_config(&self) -> super::optimization::OptimizationConfig {
        super::optimization::OptimizationConfig {
            enable_command_caching: self.enable_command_caching,
            max_cached_commands: self.max_cached_commands.into(),
            command_cache_ttl: self.command_cache_ttl.as_duration(),
            enable_stream_version_caching: self.enable_stream_version_caching,
            max_cached_stream_versions: self.max_cached_stream_versions.into(),
            stream_version_cache_ttl: self.stream_version_cache_ttl.as_duration(),
            enable_smart_retry: self.enable_smart_retry,
        }
    }
}

impl Default for ValidatedOptimizationConfig {
    fn default() -> Self {
        Self::new().expect("Default optimization configuration should always be valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_validation() {
        // Valid values should work
        assert!(MaxRetryAttempts::try_new(3).is_ok());
        assert!(RetryBaseDelayMs::try_new(100).is_ok());
        assert!(BackoffMultiplier::try_new(2.0).is_ok());

        // Invalid values should fail
        assert!(MaxRetryAttempts::try_new(0).is_err()); // Too low
        assert!(MaxRetryAttempts::try_new(11).is_err()); // Too high
        assert!(RetryBaseDelayMs::try_new(5).is_err()); // Too low
        assert!(BackoffMultiplier::try_new(0.5).is_err()); // Too low
        assert!(BackoffMultiplier::try_new(4.0).is_err()); // Too high
    }

    #[test]
    fn test_timeout_validation() {
        // Valid timeouts
        assert!(EventStoreTimeoutMs::try_new(5_000).is_ok());
        assert!(CommandTimeoutMs::try_new(30_000).is_ok());

        // Invalid timeouts
        assert!(EventStoreTimeoutMs::try_new(500).is_err()); // Too short
        assert!(CommandTimeoutMs::try_new(1_000).is_err()); // Too short
        assert!(CommandTimeoutMs::try_new(4_000_000).is_err()); // Too long
    }

    #[test]
    fn test_cache_config_validation() {
        // Valid cache sizes
        assert!(MaxCacheSize::try_new(1_000).is_ok());
        assert!(CacheTtlSeconds::try_new(300).is_ok());

        // Invalid cache sizes
        assert!(MaxCacheSize::try_new(50).is_err()); // Too small
        assert!(MaxCacheSize::try_new(2_000_000).is_err()); // Too large
        assert!(CacheTtlSeconds::try_new(0).is_err()); // Too short
        assert!(CacheTtlSeconds::try_new(100_000).is_err()); // Too long
    }

    #[test]
    fn test_retry_config_presets() {
        let default = ValidatedRetryConfig::default();
        let conservative = ValidatedRetryConfig::conservative().unwrap();
        let aggressive = ValidatedRetryConfig::aggressive().unwrap();

        // Conservative should have fewer attempts and longer delays
        let conservative_attempts: u32 = conservative.max_attempts.into();
        let default_attempts: u32 = default.max_attempts.into();
        assert!(conservative_attempts <= default_attempts);

        let conservative_delay: u64 = conservative.base_delay.into();
        let default_delay: u64 = default.base_delay.into();
        assert!(conservative_delay >= default_delay);

        // Aggressive should have more attempts and shorter delays
        let aggressive_attempts: u32 = aggressive.max_attempts.into();
        assert!(aggressive_attempts >= default_attempts);

        let aggressive_delay: u64 = aggressive.base_delay.into();
        assert!(aggressive_delay <= default_delay);
    }

    #[test]
    fn test_execution_options_presets() {
        let default = ValidatedExecutionOptions::default();
        let high_perf = ValidatedExecutionOptions::high_performance().unwrap();
        let high_rel = ValidatedExecutionOptions::high_reliability().unwrap();

        // High performance should have shorter timeouts
        let high_perf_timeout: u64 = high_perf.event_store_timeout.unwrap().into();
        let default_timeout: u64 = default.event_store_timeout.unwrap().into();
        assert!(high_perf_timeout <= default_timeout);

        // High reliability should have longer timeouts
        let high_rel_timeout: u64 = high_rel.event_store_timeout.unwrap().into();
        assert!(high_rel_timeout >= default_timeout);
    }

    #[test]
    fn test_optimization_config_presets() {
        let default = ValidatedOptimizationConfig::default();
        let memory_efficient = ValidatedOptimizationConfig::memory_efficient().unwrap();
        let high_performance = ValidatedOptimizationConfig::high_performance().unwrap();

        // Memory efficient should have smaller caches
        let memory_commands: usize = memory_efficient.max_cached_commands.into();
        let default_commands: usize = default.max_cached_commands.into();
        assert!(memory_commands < default_commands);

        // High performance should have larger caches
        let high_perf_commands: usize = high_performance.max_cached_commands.into();
        assert!(high_perf_commands > default_commands);
    }

    #[test]
    fn test_legacy_conversion() {
        let validated_config = ValidatedRetryConfig::default();
        let legacy_config = validated_config.to_legacy_config();

        let max_attempts: u32 = validated_config.max_attempts.into();
        let backoff_multiplier: f64 = validated_config.backoff_multiplier.into();

        assert_eq!(legacy_config.max_attempts, max_attempts);
        assert_eq!(
            legacy_config.base_delay,
            validated_config.base_delay.as_duration()
        );
        assert_eq!(
            legacy_config.max_delay,
            validated_config.max_delay.as_duration()
        );
        assert!((legacy_config.backoff_multiplier - backoff_multiplier).abs() < f64::EPSILON);
    }
}
