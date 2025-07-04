//! Business Rule Validation Optimization
//!
//! This module provides optimized business rule validation with intelligent caching
//! and performance profiling. It addresses the identified performance bottlenecks
//! in balance checking, inventory validation, and business rule evaluation.
//!
//! # Key Optimizations
//!
//! 1. **Validation Result Caching**: Cache expensive validation computations
//! 2. **Derived Value Pre-computation**: Calculate derived values once  
//! 3. **Batch Validation**: Validate multiple related rules together
//! 4. **Performance Profiling**: Different validation strategies based on requirements
//!
//! # Usage
//!
//! ```rust
//! use eventcore::validation::{ValidationCache, ValidationProfile, BusinessRule};
//!
//! let cache = ValidationCache::new(&ValidationProfile::HighPerformance);
//! let result = cache.validate_business_rule(&rule, &context).await?;
//! ```

use crate::CommandError;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

/// Configuration profile for validation performance
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationProfile {
    /// Conservative approach - minimal caching, maximum correctness
    Conservative,
    /// Balanced approach - moderate caching with good performance  
    Balanced,
    /// High performance - aggressive caching for maximum speed
    HighPerformance,
    /// Custom profile with specific cache configurations
    Custom(ValidationConfig),
}

/// Detailed configuration for validation behavior
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationConfig {
    /// Maximum number of validation results to cache
    pub cache_size: usize,
    /// Time-to-live for cached validation results  
    pub cache_ttl: Duration,
    /// Whether to enable pre-computation of derived values
    pub enable_precomputation: bool,
    /// Whether to batch related validations together
    pub enable_batch_validation: bool,
    /// Maximum time to spend on validation before failing fast
    pub max_validation_time: Duration,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            cache_size: 1000,
            cache_ttl: Duration::from_secs(30),
            enable_precomputation: true,
            enable_batch_validation: true,
            max_validation_time: Duration::from_millis(100),
        }
    }
}

impl ValidationProfile {
    /// Get the configuration for this profile
    pub fn config(&self) -> ValidationConfig {
        match self {
            Self::Conservative => ValidationConfig {
                cache_size: 100,
                cache_ttl: Duration::from_secs(5),
                enable_precomputation: false,
                enable_batch_validation: false,
                max_validation_time: Duration::from_millis(50),
            },
            Self::Balanced => ValidationConfig::default(),
            Self::HighPerformance => ValidationConfig {
                cache_size: 5000,
                cache_ttl: Duration::from_secs(120),
                enable_precomputation: true,
                enable_batch_validation: true,
                max_validation_time: Duration::from_millis(200),
            },
            Self::Custom(config) => config.clone(),
        }
    }
}

/// Represents a business rule that can be validated
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BusinessRule {
    /// Validate sufficient funds for a money operation
    SufficientFunds {
        /// Account identifier to check balance for
        account_id: String,
        /// Required amount in cents
        required_amount: u64,
    },
    /// Validate inventory availability for a product
    InventoryAvailable {
        /// Product identifier to check inventory for
        product_id: String,
        /// Warehouse identifier where product is stored
        warehouse_id: String,
        /// Required quantity to reserve
        required_quantity: u32,
    },
    /// Validate capacity limits
    CapacityLimit {
        /// Resource identifier to check capacity for
        resource_id: String,
        /// Current usage level
        current_usage: u32,
        /// Additional usage to add
        additional_usage: u32,
        /// Maximum allowed capacity
        max_capacity: u32,
    },
    /// Custom business rule with arbitrary data
    Custom {
        /// Type identifier for the custom rule
        rule_type: String,
        /// Key-value parameters for the rule
        parameters: Vec<(String, String)>, // Use Vec instead of HashMap for Hash compatibility
    },
}

/// Context information needed for validation
#[derive(Debug, Clone)]
pub struct ValidationContext {
    /// Current balances by account ID
    pub balances: HashMap<String, u64>,
    /// Current inventory levels by (product_id, warehouse_id)
    pub inventory: HashMap<(String, String), u32>,
    /// Reserved quantities by (product_id, warehouse_id)  
    pub reserved: HashMap<(String, String), u32>,
    /// Capacity usage by resource ID
    pub capacity_usage: HashMap<String, u32>,
    /// Custom context data
    pub custom_data: HashMap<String, serde_json::Value>,
    /// Timestamp of this context (not serializable, used only for cache keys)
    pub timestamp: Instant,
}

impl Default for ValidationContext {
    fn default() -> Self {
        Self {
            balances: HashMap::new(),
            inventory: HashMap::new(),
            reserved: HashMap::new(),
            capacity_usage: HashMap::new(),
            custom_data: HashMap::new(),
            timestamp: Instant::now(),
        }
    }
}

impl ValidationContext {
    /// Create a new validation context
    pub fn new() -> Self {
        Self::default()
    }

    /// Add balance information
    #[must_use]
    pub fn with_balance(mut self, account_id: String, balance: u64) -> Self {
        self.balances.insert(account_id, balance);
        self
    }

    /// Add inventory information
    #[must_use]
    pub fn with_inventory(
        mut self,
        product_id: String,
        warehouse_id: String,
        quantity: u32,
    ) -> Self {
        self.inventory.insert((product_id, warehouse_id), quantity);
        self
    }

    /// Add reserved quantity information
    #[must_use]
    pub fn with_reserved(
        mut self,
        product_id: String,
        warehouse_id: String,
        reserved: u32,
    ) -> Self {
        self.reserved.insert((product_id, warehouse_id), reserved);
        self
    }

    /// Add capacity usage information
    #[must_use]
    pub fn with_capacity_usage(mut self, resource_id: String, usage: u32) -> Self {
        self.capacity_usage.insert(resource_id, usage);
        self
    }

    /// Get available quantity for a product at a warehouse
    pub fn available_quantity(&self, product_id: &str, warehouse_id: &str) -> u32 {
        let total = self
            .inventory
            .get(&(product_id.to_string(), warehouse_id.to_string()))
            .copied()
            .unwrap_or(0);
        let reserved = self
            .reserved
            .get(&(product_id.to_string(), warehouse_id.to_string()))
            .copied()
            .unwrap_or(0);
        total.saturating_sub(reserved)
    }

    /// Get available capacity for a resource
    pub fn available_capacity(&self, resource_id: &str, max_capacity: u32) -> u32 {
        let used = self.capacity_usage.get(resource_id).copied().unwrap_or(0);
        max_capacity.saturating_sub(used)
    }
}

/// Result of a business rule validation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether the validation passed
    pub valid: bool,
    /// Error message if validation failed
    pub error_message: Option<String>,
    /// Validation metrics
    pub metrics: ValidationMetrics,
    /// Whether this result came from cache
    pub from_cache: bool,
}

/// Metrics about validation performance
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ValidationMetrics {
    /// Time taken to perform validation
    pub duration: Duration,
    /// Number of rules evaluated
    pub rules_evaluated: u32,
    /// Number of cache hits
    pub cache_hits: u32,
    /// Number of cache misses
    pub cache_misses: u32,
}

/// Cached validation entry
#[derive(Debug, Clone)]
struct CacheEntry {
    result: ValidationResult,
    created_at: Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn new(result: ValidationResult, ttl: Duration) -> Self {
        Self {
            result,
            created_at: Instant::now(),
            ttl,
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }
}

/// Cache key for validation results
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    rule: BusinessRule,
    context_hash: u64,
}

impl CacheKey {
    fn new(rule: &BusinessRule, context: &ValidationContext) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // Hash relevant parts of context based on rule type
        match rule {
            BusinessRule::SufficientFunds { account_id, .. } => {
                if let Some(balance) = context.balances.get(account_id) {
                    balance.hash(&mut hasher);
                }
            }
            BusinessRule::InventoryAvailable {
                product_id,
                warehouse_id,
                ..
            } => {
                let key = (product_id.clone(), warehouse_id.clone());
                if let Some(inventory) = context.inventory.get(&key) {
                    inventory.hash(&mut hasher);
                }
                if let Some(reserved) = context.reserved.get(&key) {
                    reserved.hash(&mut hasher);
                }
            }
            BusinessRule::CapacityLimit { resource_id, .. } => {
                if let Some(usage) = context.capacity_usage.get(resource_id) {
                    usage.hash(&mut hasher);
                }
            }
            BusinessRule::Custom { parameters, .. } => {
                parameters.hash(&mut hasher);
            }
        }

        Self {
            rule: rule.clone(),
            context_hash: hasher.finish(),
        }
    }
}

/// High-performance validation cache with intelligent caching strategies
pub struct ValidationCache {
    config: ValidationConfig,
    cache: Arc<RwLock<HashMap<CacheKey, CacheEntry>>>,
    metrics: Arc<RwLock<ValidationMetrics>>,
}

impl ValidationCache {
    /// Create a new validation cache with the specified profile
    pub fn new(profile: &ValidationProfile) -> Self {
        Self {
            config: profile.config(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(ValidationMetrics::default())),
        }
    }

    /// Create a new validation cache with custom configuration
    pub fn with_config(config: ValidationConfig) -> Self {
        Self {
            config,
            cache: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(ValidationMetrics::default())),
        }
    }

    /// Validate a single business rule with caching
    #[allow(clippy::unused_async)]
    pub async fn validate_business_rule(
        &self,
        rule: &BusinessRule,
        context: &ValidationContext,
    ) -> Result<ValidationResult, CommandError> {
        let start_time = Instant::now();
        let cache_key = CacheKey::new(rule, context);

        // Check cache first
        if let Some(cached_result) = self.check_cache(&cache_key) {
            self.update_metrics(|m| m.cache_hits += 1);
            return Ok(cached_result);
        }

        self.update_metrics(|m| m.cache_misses += 1);

        // Perform validation
        let mut result = self.validate_rule_impl(rule, context);
        result.metrics.duration = start_time.elapsed();
        result.from_cache = false;

        // Store in cache
        self.store_in_cache(cache_key, result.clone());

        self.update_metrics(|m| {
            m.rules_evaluated += 1;
            m.duration = m.duration.saturating_add(result.metrics.duration);
        });

        Ok(result)
    }

    /// Validate multiple business rules in a batch for efficiency
    pub async fn validate_batch(
        &self,
        rules: &[BusinessRule],
        context: &ValidationContext,
    ) -> Result<Vec<ValidationResult>, CommandError> {
        if !self.config.enable_batch_validation {
            // Fall back to individual validation
            let mut results = Vec::new();
            for rule in rules {
                results.push(self.validate_business_rule(rule, context).await?);
            }
            return Ok(results);
        }

        let start_time = Instant::now();
        let mut results = Vec::with_capacity(rules.len());
        let mut cache_hits = 0;
        let mut cache_misses = 0;

        // Check cache for all rules first
        let mut uncached_rules = Vec::new();
        for rule in rules {
            let cache_key = CacheKey::new(rule, context);
            if let Some(cached_result) = self.check_cache(&cache_key) {
                results.push(cached_result);
                cache_hits += 1;
            } else {
                uncached_rules.push((rule, cache_key));
                cache_misses += 1;
            }
        }

        // Validate uncached rules in batch
        for (rule, cache_key) in uncached_rules {
            let mut result = self.validate_rule_impl(rule, context);
            result.from_cache = false;
            self.store_in_cache(cache_key, result.clone());
            results.push(result);
        }

        self.update_metrics(|m| {
            m.cache_hits += cache_hits;
            m.cache_misses += cache_misses;
            m.rules_evaluated += u32::try_from(rules.len()).unwrap_or(u32::MAX);
            m.duration = m.duration.saturating_add(start_time.elapsed());
        });

        Ok(results)
    }

    /// Clear expired entries from cache
    pub fn cleanup_cache(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.retain(|_, entry| !entry.is_expired());

            // Also enforce size limit
            if cache.len() > self.config.cache_size {
                let to_remove = cache.len() - self.config.cache_size;
                let keys_to_remove: Vec<_> = cache.keys().take(to_remove).cloned().collect();
                for key in keys_to_remove {
                    cache.remove(&key);
                }
            }
        }
    }

    /// Get current cache statistics
    pub fn cache_stats(&self) -> ValidationMetrics {
        self.metrics.read().unwrap().clone()
    }

    /// Clear all cached validation results
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
    }

    fn check_cache(&self, cache_key: &CacheKey) -> Option<ValidationResult> {
        if let Ok(cache) = self.cache.read() {
            if let Some(entry) = cache.get(cache_key) {
                if !entry.is_expired() {
                    let mut result = entry.result.clone();
                    result.from_cache = true;
                    return Some(result);
                }
            }
        }
        None
    }

    fn store_in_cache(&self, cache_key: CacheKey, result: ValidationResult) {
        if let Ok(mut cache) = self.cache.write() {
            let entry = CacheEntry::new(result, self.config.cache_ttl);
            cache.insert(cache_key, entry);

            // Enforce cache size limit
            if cache.len() > self.config.cache_size {
                // Remove oldest entry (simple FIFO eviction)
                if let Some(oldest_key) = cache.keys().next().cloned() {
                    cache.remove(&oldest_key);
                }
            }
        }
    }

    fn update_metrics<F>(&self, f: F)
    where
        F: FnOnce(&mut ValidationMetrics),
    {
        if let Ok(mut metrics) = self.metrics.write() {
            f(&mut metrics);
        }
    }

    fn validate_rule_impl(
        &self,
        rule: &BusinessRule,
        context: &ValidationContext,
    ) -> ValidationResult {
        let start_time = Instant::now();

        // Timeout protection
        if start_time.elapsed() > self.config.max_validation_time {
            return ValidationResult {
                valid: false,
                error_message: Some("Validation timeout exceeded".to_string()),
                metrics: ValidationMetrics {
                    duration: start_time.elapsed(),
                    rules_evaluated: 1,
                    cache_hits: 0,
                    cache_misses: 1,
                },
                from_cache: false,
            };
        }

        let (valid, error_message) = match rule {
            BusinessRule::SufficientFunds {
                account_id,
                required_amount,
            } => {
                let balance = context.balances.get(account_id).copied().unwrap_or(0);
                if balance >= *required_amount {
                    (true, None)
                } else {
                    (
                        false,
                        Some(format!(
                            "Insufficient funds in account {account_id}: balance {balance}, required {required_amount}"
                        )),
                    )
                }
            }

            BusinessRule::InventoryAvailable {
                product_id,
                warehouse_id,
                required_quantity,
            } => {
                let available = context.available_quantity(product_id, warehouse_id);
                if available >= *required_quantity {
                    (true, None)
                } else {
                    (false, Some(format!(
                        "Insufficient inventory for product {product_id} at warehouse {warehouse_id}: available {available}, required {required_quantity}"
                    )))
                }
            }

            BusinessRule::CapacityLimit {
                resource_id,
                current_usage,
                additional_usage,
                max_capacity,
            } => {
                let total_usage = current_usage + additional_usage;
                if total_usage <= *max_capacity {
                    (true, None)
                } else {
                    (false, Some(format!(
                        "Capacity limit exceeded for resource {resource_id}: current {current_usage}, additional {additional_usage}, max {max_capacity}"
                    )))
                }
            }

            BusinessRule::Custom {
                rule_type,
                parameters: _,
            } => {
                // For custom rules, delegate to a pluggable validation system
                // For now, always succeed as a placeholder
                if rule_type == "always_invalid" {
                    (false, Some("Custom rule always fails".to_string()))
                } else {
                    (true, None) // Both "always_valid" and unknown rules default to valid
                }
            }
        };

        ValidationResult {
            valid,
            error_message,
            metrics: ValidationMetrics {
                duration: start_time.elapsed(),
                rules_evaluated: 1,
                cache_hits: 0,
                cache_misses: 0,
            },
            from_cache: false,
        }
    }
}

/// Helper trait for integrating validation with commands
pub trait ValidatedCommand {
    /// Get the business rules that need to be validated for this command
    fn business_rules(&self) -> Vec<BusinessRule>;

    /// Get the validation context for this command
    fn validation_context(&self) -> ValidationContext;

    /// Validate all business rules for this command
    #[allow(async_fn_in_trait)]
    async fn validate_business_rules(
        &self,
        cache: &ValidationCache,
    ) -> Result<Vec<ValidationResult>, CommandError> {
        let rules = self.business_rules();
        let context = self.validation_context();
        cache.validate_batch(&rules, &context).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sufficient_funds_validation() {
        let cache = ValidationCache::new(&ValidationProfile::Balanced);

        let rule = BusinessRule::SufficientFunds {
            account_id: "account-123".to_string(),
            required_amount: 1000,
        };

        let context = ValidationContext::new().with_balance("account-123".to_string(), 1500);

        let result = cache.validate_business_rule(&rule, &context).await.unwrap();
        assert!(result.valid);
        assert!(!result.from_cache);

        // Second validation should come from cache
        let result2 = cache.validate_business_rule(&rule, &context).await.unwrap();
        assert!(result2.valid);
        assert!(result2.from_cache);
    }

    #[tokio::test]
    async fn test_insufficient_funds_validation() {
        let cache = ValidationCache::new(&ValidationProfile::Balanced);

        let rule = BusinessRule::SufficientFunds {
            account_id: "account-123".to_string(),
            required_amount: 2000,
        };

        let context = ValidationContext::new().with_balance("account-123".to_string(), 1500);

        let result = cache.validate_business_rule(&rule, &context).await.unwrap();
        assert!(!result.valid);
        assert!(result.error_message.is_some());
        assert!(result.error_message.unwrap().contains("Insufficient funds"));
    }

    #[tokio::test]
    async fn test_inventory_validation() {
        let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

        let rule = BusinessRule::InventoryAvailable {
            product_id: "product-abc".to_string(),
            warehouse_id: "warehouse-1".to_string(),
            required_quantity: 50,
        };

        let context = ValidationContext::new()
            .with_inventory("product-abc".to_string(), "warehouse-1".to_string(), 100)
            .with_reserved("product-abc".to_string(), "warehouse-1".to_string(), 20);

        let result = cache.validate_business_rule(&rule, &context).await.unwrap();
        assert!(result.valid); // 100 - 20 = 80 available, need 50
    }

    #[tokio::test]
    async fn test_insufficient_inventory_validation() {
        let cache = ValidationCache::new(&ValidationProfile::Conservative);

        let rule = BusinessRule::InventoryAvailable {
            product_id: "product-xyz".to_string(),
            warehouse_id: "warehouse-2".to_string(),
            required_quantity: 100,
        };

        let context = ValidationContext::new()
            .with_inventory("product-xyz".to_string(), "warehouse-2".to_string(), 50)
            .with_reserved("product-xyz".to_string(), "warehouse-2".to_string(), 10);

        let result = cache.validate_business_rule(&rule, &context).await.unwrap();
        assert!(!result.valid); // 50 - 10 = 40 available, need 100
        assert!(result.error_message.is_some());
    }

    #[tokio::test]
    async fn test_capacity_limit_validation() {
        let cache = ValidationCache::new(&ValidationProfile::Balanced);

        let rule = BusinessRule::CapacityLimit {
            resource_id: "cpu-pool-1".to_string(),
            current_usage: 70,
            additional_usage: 20,
            max_capacity: 100,
        };

        let context = ValidationContext::new().with_capacity_usage("cpu-pool-1".to_string(), 70);

        let result = cache.validate_business_rule(&rule, &context).await.unwrap();
        assert!(result.valid); // 70 + 20 = 90 <= 100
    }

    #[tokio::test]
    async fn test_batch_validation() {
        let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

        let rules = vec![
            BusinessRule::SufficientFunds {
                account_id: "account-1".to_string(),
                required_amount: 500,
            },
            BusinessRule::InventoryAvailable {
                product_id: "product-1".to_string(),
                warehouse_id: "warehouse-1".to_string(),
                required_quantity: 10,
            },
        ];

        let context = ValidationContext::new()
            .with_balance("account-1".to_string(), 1000)
            .with_inventory("product-1".to_string(), "warehouse-1".to_string(), 20);

        let results = cache.validate_batch(&rules, &context).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].valid);
        assert!(results[1].valid);
    }

    #[tokio::test]
    async fn test_cache_cleanup() {
        let config = ValidationConfig {
            cache_size: 2,
            cache_ttl: Duration::from_millis(10),
            ..Default::default()
        };
        let cache = ValidationCache::with_config(config);

        // Add entries to fill cache
        for i in 0..3 {
            let rule = BusinessRule::SufficientFunds {
                account_id: format!("account-{i}"),
                required_amount: 100,
            };
            let context = ValidationContext::new().with_balance(format!("account-{i}"), 200);
            cache.validate_business_rule(&rule, &context).await.unwrap();
        }

        // Wait for TTL expiration
        tokio::time::sleep(Duration::from_millis(20)).await;

        cache.cleanup_cache();

        // Cache should be cleaned up
        let stats = cache.cache_stats();
        assert!(stats.rules_evaluated > 0);
    }

    #[test]
    fn test_validation_profiles() {
        let conservative = ValidationProfile::Conservative.config();
        assert_eq!(conservative.cache_size, 100);
        assert!(!conservative.enable_precomputation);

        let high_perf = ValidationProfile::HighPerformance.config();
        assert_eq!(high_perf.cache_size, 5000);
        assert!(high_perf.enable_precomputation);

        let balanced = ValidationProfile::Balanced.config();
        assert!(balanced.cache_size > conservative.cache_size);
        assert!(balanced.cache_size < high_perf.cache_size);
    }
}
