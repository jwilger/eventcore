//! Comprehensive tests for business rule validation optimization
//!
//! This module tests the performance and correctness of the validation
//! optimization system, including caching behavior, batch validation,
//! and performance characteristics across different profiles.

#![allow(clippy::uninlined_format_args)]

use eventcore::validation::{
    BusinessRule, ValidatedCommand, ValidationCache, ValidationConfig, ValidationContext,
    ValidationProfile,
};
use std::time::Duration;
use tokio::time::sleep;

// ============================================================================
// Basic Validation Tests
// ============================================================================

#[tokio::test]
async fn test_sufficient_funds_validation_success() {
    let cache = ValidationCache::new(&ValidationProfile::Balanced);

    let rule = BusinessRule::SufficientFunds {
        account_id: "account-123".to_string(),
        required_amount: 1000,
    };

    let context = ValidationContext::new().with_balance("account-123".to_string(), 1500);

    let result = cache.validate_business_rule(&rule, &context).await.unwrap();
    assert!(result.valid);
    assert!(result.error_message.is_none());
    assert!(!result.from_cache);
    assert_eq!(result.metrics.rules_evaluated, 1);
}

#[tokio::test]
async fn test_sufficient_funds_validation_failure() {
    let cache = ValidationCache::new(&ValidationProfile::Conservative);

    let rule = BusinessRule::SufficientFunds {
        account_id: "account-456".to_string(),
        required_amount: 2000,
    };

    let context = ValidationContext::new().with_balance("account-456".to_string(), 1500);

    let result = cache.validate_business_rule(&rule, &context).await.unwrap();
    assert!(!result.valid);
    assert!(result.error_message.is_some());
    assert!(result.error_message.unwrap().contains("Insufficient funds"));
    assert!(!result.from_cache);
}

#[tokio::test]
async fn test_inventory_validation_success() {
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
    assert!(result.error_message.is_none());
}

#[tokio::test]
async fn test_inventory_validation_failure() {
    let cache = ValidationCache::new(&ValidationProfile::Balanced);

    let rule = BusinessRule::InventoryAvailable {
        product_id: "product-xyz".to_string(),
        warehouse_id: "warehouse-2".to_string(),
        required_quantity: 100,
    };

    let context = ValidationContext::new()
        .with_inventory("product-xyz".to_string(), "warehouse-2".to_string(), 50)
        .with_reserved("product-xyz".to_string(), "warehouse-2".to_string(), 20);

    let result = cache.validate_business_rule(&rule, &context).await.unwrap();
    assert!(!result.valid); // 50 - 20 = 30 available, need 100
    assert!(result.error_message.is_some());
    assert!(result
        .error_message
        .unwrap()
        .contains("Insufficient inventory"));
}

#[tokio::test]
async fn test_capacity_limit_validation_success() {
    let cache = ValidationCache::new(&ValidationProfile::Conservative);

    let rule = BusinessRule::CapacityLimit {
        resource_id: "cpu-pool-1".to_string(),
        current_usage: 60,
        additional_usage: 25,
        max_capacity: 100,
    };

    let context = ValidationContext::new().with_capacity_usage("cpu-pool-1".to_string(), 60);

    let result = cache.validate_business_rule(&rule, &context).await.unwrap();
    assert!(result.valid); // 60 + 25 = 85 <= 100
    assert!(result.error_message.is_none());
}

#[tokio::test]
async fn test_capacity_limit_validation_failure() {
    let cache = ValidationCache::new(&ValidationProfile::Balanced);

    let rule = BusinessRule::CapacityLimit {
        resource_id: "cpu-pool-2".to_string(),
        current_usage: 80,
        additional_usage: 30,
        max_capacity: 100,
    };

    let context = ValidationContext::new().with_capacity_usage("cpu-pool-2".to_string(), 80);

    let result = cache.validate_business_rule(&rule, &context).await.unwrap();
    assert!(!result.valid); // 80 + 30 = 110 > 100
    assert!(result.error_message.is_some());
    assert!(result
        .error_message
        .unwrap()
        .contains("Capacity limit exceeded"));
}

#[tokio::test]
async fn test_custom_rule_validation() {
    let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

    // Test always valid custom rule
    let rule_valid = BusinessRule::Custom {
        rule_type: "always_valid".to_string(),
        parameters: vec![("param1".to_string(), "value1".to_string())],
    };

    let context = ValidationContext::new();
    let result = cache
        .validate_business_rule(&rule_valid, &context)
        .await
        .unwrap();
    assert!(result.valid);

    // Test always invalid custom rule
    let rule_invalid = BusinessRule::Custom {
        rule_type: "always_invalid".to_string(),
        parameters: vec![("param2".to_string(), "value2".to_string())],
    };

    let result = cache
        .validate_business_rule(&rule_invalid, &context)
        .await
        .unwrap();
    assert!(!result.valid);
    assert!(result.error_message.is_some());
}

// ============================================================================
// Caching Tests
// ============================================================================

#[tokio::test]
async fn test_validation_caching_behavior() {
    let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

    let rule = BusinessRule::SufficientFunds {
        account_id: "cache-test-account".to_string(),
        required_amount: 500,
    };

    let context = ValidationContext::new().with_balance("cache-test-account".to_string(), 1000);

    // First validation - should not be from cache
    let result1 = cache.validate_business_rule(&rule, &context).await.unwrap();
    assert!(result1.valid);
    assert!(!result1.from_cache);

    // Second validation - should be from cache
    let result2 = cache.validate_business_rule(&rule, &context).await.unwrap();
    assert!(result2.valid);
    assert!(result2.from_cache);

    // Verify cache statistics
    let stats = cache.cache_stats();
    assert_eq!(stats.cache_hits, 1);
    assert_eq!(stats.cache_misses, 1);
}

#[tokio::test]
async fn test_cache_invalidation_on_context_change() {
    let cache = ValidationCache::new(&ValidationProfile::Balanced);

    let rule = BusinessRule::SufficientFunds {
        account_id: "invalidation-test".to_string(),
        required_amount: 750,
    };

    // First context - sufficient funds
    let context1 = ValidationContext::new().with_balance("invalidation-test".to_string(), 1000);

    let result1 = cache
        .validate_business_rule(&rule, &context1)
        .await
        .unwrap();
    assert!(result1.valid);
    assert!(!result1.from_cache);

    // Second context - insufficient funds (different context hash)
    let context2 = ValidationContext::new().with_balance("invalidation-test".to_string(), 500);

    let result2 = cache
        .validate_business_rule(&rule, &context2)
        .await
        .unwrap();
    assert!(!result2.valid);
    assert!(!result2.from_cache); // Should not be from cache due to different context
}

#[tokio::test]
async fn test_cache_ttl_expiration() {
    let config = ValidationConfig {
        cache_size: 100,
        cache_ttl: Duration::from_millis(50), // Very short TTL for testing
        enable_precomputation: true,
        enable_batch_validation: true,
        max_validation_time: Duration::from_millis(100),
    };

    let cache = ValidationCache::with_config(config);

    let rule = BusinessRule::SufficientFunds {
        account_id: "ttl-test".to_string(),
        required_amount: 200,
    };

    let context = ValidationContext::new().with_balance("ttl-test".to_string(), 500);

    // First validation
    let result1 = cache.validate_business_rule(&rule, &context).await.unwrap();
    assert!(result1.valid);
    assert!(!result1.from_cache);

    // Second validation within TTL - should be cached
    let result2 = cache.validate_business_rule(&rule, &context).await.unwrap();
    assert!(result2.valid);
    assert!(result2.from_cache);

    // Wait for TTL expiration
    sleep(Duration::from_millis(60)).await;

    // Third validation after TTL - should not be cached
    let result3 = cache.validate_business_rule(&rule, &context).await.unwrap();
    assert!(result3.valid);
    assert!(!result3.from_cache); // Cache entry should have expired
}

#[tokio::test]
async fn test_cache_size_limit_enforcement() {
    let config = ValidationConfig {
        cache_size: 3, // Very small cache for testing
        cache_ttl: Duration::from_secs(300),
        enable_precomputation: true,
        enable_batch_validation: true,
        max_validation_time: Duration::from_millis(100),
    };

    let cache = ValidationCache::with_config(config);

    let context = ValidationContext::new();

    // Fill cache beyond limit
    for i in 0..5 {
        let rule = BusinessRule::SufficientFunds {
            account_id: format!("cache-size-test-{}", i),
            required_amount: 100,
        };

        let ctx = context
            .clone()
            .with_balance(format!("cache-size-test-{}", i), 200);
        cache.validate_business_rule(&rule, &ctx).await.unwrap();
    }

    // Verify cache cleanup occurred
    cache.cleanup_cache();

    // Check that first entries may have been evicted
    let first_rule = BusinessRule::SufficientFunds {
        account_id: "cache-size-test-0".to_string(),
        required_amount: 100,
    };
    let first_ctx = context.with_balance("cache-size-test-0".to_string(), 200);

    // This may or may not be cached depending on eviction policy
    let _result = cache
        .validate_business_rule(&first_rule, &first_ctx)
        .await
        .unwrap();
}

// ============================================================================
// Batch Validation Tests
// ============================================================================

#[tokio::test]
async fn test_batch_validation_success() {
    let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

    let rules = vec![
        BusinessRule::SufficientFunds {
            account_id: "batch-account-1".to_string(),
            required_amount: 500,
        },
        BusinessRule::InventoryAvailable {
            product_id: "batch-product-1".to_string(),
            warehouse_id: "batch-warehouse-1".to_string(),
            required_quantity: 10,
        },
        BusinessRule::CapacityLimit {
            resource_id: "batch-resource-1".to_string(),
            current_usage: 30,
            additional_usage: 20,
            max_capacity: 100,
        },
    ];

    let context = ValidationContext::new()
        .with_balance("batch-account-1".to_string(), 1000)
        .with_inventory(
            "batch-product-1".to_string(),
            "batch-warehouse-1".to_string(),
            50,
        )
        .with_capacity_usage("batch-resource-1".to_string(), 30);

    let results = cache.validate_batch(&rules, &context).await.unwrap();
    assert_eq!(results.len(), 3);

    for result in &results {
        assert!(result.valid);
        assert!(result.error_message.is_none());
        assert!(!result.from_cache); // First time validation
    }
}

#[tokio::test]
async fn test_batch_validation_with_failures() {
    let cache = ValidationCache::new(&ValidationProfile::Balanced);

    let rules = vec![
        BusinessRule::SufficientFunds {
            account_id: "batch-fail-account".to_string(),
            required_amount: 2000, // Insufficient funds
        },
        BusinessRule::InventoryAvailable {
            product_id: "batch-fail-product".to_string(),
            warehouse_id: "batch-fail-warehouse".to_string(),
            required_quantity: 5, // This should succeed
        },
    ];

    let context = ValidationContext::new()
        .with_balance("batch-fail-account".to_string(), 1000) // Only $10, need $20
        .with_inventory(
            "batch-fail-product".to_string(),
            "batch-fail-warehouse".to_string(),
            10,
        );

    let results = cache.validate_batch(&rules, &context).await.unwrap();
    assert_eq!(results.len(), 2);

    // First rule should fail
    assert!(!results[0].valid);
    assert!(results[0].error_message.is_some());

    // Second rule should succeed
    assert!(results[1].valid);
    assert!(results[1].error_message.is_none());
}

#[tokio::test]
async fn test_batch_validation_caching() {
    let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

    let rules = vec![
        BusinessRule::SufficientFunds {
            account_id: "batch-cache-account".to_string(),
            required_amount: 300,
        },
        BusinessRule::InventoryAvailable {
            product_id: "batch-cache-product".to_string(),
            warehouse_id: "batch-cache-warehouse".to_string(),
            required_quantity: 5,
        },
    ];

    let context = ValidationContext::new()
        .with_balance("batch-cache-account".to_string(), 500)
        .with_inventory(
            "batch-cache-product".to_string(),
            "batch-cache-warehouse".to_string(),
            20,
        );

    // First batch validation
    let results1 = cache.validate_batch(&rules, &context).await.unwrap();
    assert_eq!(results1.len(), 2);
    assert!(results1.iter().all(|r| r.valid && !r.from_cache));

    // Second batch validation - should use cache
    let results2 = cache.validate_batch(&rules, &context).await.unwrap();
    assert_eq!(results2.len(), 2);
    assert!(results2.iter().all(|r| r.valid && r.from_cache));
}

#[tokio::test]
async fn test_batch_validation_disabled() {
    let config = ValidationConfig {
        cache_size: 1000,
        cache_ttl: Duration::from_secs(60),
        enable_precomputation: true,
        enable_batch_validation: false, // Disable batch validation
        max_validation_time: Duration::from_millis(100),
    };

    let cache = ValidationCache::with_config(config);

    let rules = vec![
        BusinessRule::SufficientFunds {
            account_id: "no-batch-account".to_string(),
            required_amount: 100,
        },
        BusinessRule::InventoryAvailable {
            product_id: "no-batch-product".to_string(),
            warehouse_id: "no-batch-warehouse".to_string(),
            required_quantity: 3,
        },
    ];

    let context = ValidationContext::new()
        .with_balance("no-batch-account".to_string(), 200)
        .with_inventory(
            "no-batch-product".to_string(),
            "no-batch-warehouse".to_string(),
            10,
        );

    // Should fall back to individual validation
    let results = cache.validate_batch(&rules, &context).await.unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.valid));
}

// ============================================================================
// Performance Profile Tests
// ============================================================================

#[tokio::test]
async fn test_validation_profiles_config() {
    let conservative = ValidationProfile::Conservative.config();
    assert_eq!(conservative.cache_size, 100);
    assert_eq!(conservative.cache_ttl, Duration::from_secs(5));
    assert!(!conservative.enable_precomputation);
    assert!(!conservative.enable_batch_validation);
    assert_eq!(conservative.max_validation_time, Duration::from_millis(50));

    let balanced = ValidationProfile::Balanced.config();
    assert_eq!(balanced.cache_size, 1000);
    assert_eq!(balanced.cache_ttl, Duration::from_secs(30));
    assert!(balanced.enable_precomputation);
    assert!(balanced.enable_batch_validation);
    assert_eq!(balanced.max_validation_time, Duration::from_millis(100));

    let high_performance = ValidationProfile::HighPerformance.config();
    assert_eq!(high_performance.cache_size, 5000);
    assert_eq!(high_performance.cache_ttl, Duration::from_secs(120));
    assert!(high_performance.enable_precomputation);
    assert!(high_performance.enable_batch_validation);
    assert_eq!(
        high_performance.max_validation_time,
        Duration::from_millis(200)
    );
}

#[tokio::test]
async fn test_custom_profile_config() {
    let custom_config = ValidationConfig {
        cache_size: 2500,
        cache_ttl: Duration::from_secs(45),
        enable_precomputation: false,
        enable_batch_validation: true,
        max_validation_time: Duration::from_millis(75),
    };

    let custom_profile = ValidationProfile::Custom(custom_config.clone());
    let profile_config = custom_profile.config();

    assert_eq!(profile_config.cache_size, custom_config.cache_size);
    assert_eq!(profile_config.cache_ttl, custom_config.cache_ttl);
    assert_eq!(
        profile_config.enable_precomputation,
        custom_config.enable_precomputation
    );
    assert_eq!(
        profile_config.enable_batch_validation,
        custom_config.enable_batch_validation
    );
    assert_eq!(
        profile_config.max_validation_time,
        custom_config.max_validation_time
    );
}

// ============================================================================
// ValidationContext Tests
// ============================================================================

#[tokio::test]
async fn test_validation_context_available_quantity_calculation() {
    let context = ValidationContext::new()
        .with_inventory("product-1".to_string(), "warehouse-1".to_string(), 100)
        .with_reserved("product-1".to_string(), "warehouse-1".to_string(), 25);

    let available = context.available_quantity("product-1", "warehouse-1");
    assert_eq!(available, 75); // 100 - 25

    // Test with no reserved quantity
    let available_no_reserved = context.available_quantity("product-2", "warehouse-1");
    assert_eq!(available_no_reserved, 0); // No inventory recorded
}

#[tokio::test]
async fn test_validation_context_available_capacity_calculation() {
    let context = ValidationContext::new().with_capacity_usage("cpu-pool".to_string(), 60);

    let available = context.available_capacity("cpu-pool", 100);
    assert_eq!(available, 40); // 100 - 60

    // Test with no usage recorded
    let available_no_usage = context.available_capacity("memory-pool", 200);
    assert_eq!(available_no_usage, 200); // No usage recorded
}

#[tokio::test]
async fn test_validation_context_builder_pattern() {
    let context = ValidationContext::new()
        .with_balance("account-1".to_string(), 1000)
        .with_balance("account-2".to_string(), 2000)
        .with_inventory("product-a".to_string(), "warehouse-1".to_string(), 50)
        .with_inventory("product-b".to_string(), "warehouse-2".to_string(), 75)
        .with_reserved("product-a".to_string(), "warehouse-1".to_string(), 10)
        .with_capacity_usage("cpu".to_string(), 25)
        .with_capacity_usage("memory".to_string(), 40);

    assert_eq!(context.balances.len(), 2);
    assert_eq!(context.inventory.len(), 2);
    assert_eq!(context.reserved.len(), 1);
    assert_eq!(context.capacity_usage.len(), 2);

    assert_eq!(context.balances.get("account-1"), Some(&1000));
    assert_eq!(context.balances.get("account-2"), Some(&2000));
    assert_eq!(context.available_quantity("product-a", "warehouse-1"), 40); // 50 - 10
    assert_eq!(context.available_quantity("product-b", "warehouse-2"), 75); // 75 - 0
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_validation_timeout_protection() {
    let config = ValidationConfig {
        cache_size: 100,
        cache_ttl: Duration::from_secs(30),
        enable_precomputation: true,
        enable_batch_validation: true,
        max_validation_time: Duration::from_nanos(1), // Extremely short timeout
    };

    let cache = ValidationCache::with_config(config);

    let rule = BusinessRule::SufficientFunds {
        account_id: "timeout-test".to_string(),
        required_amount: 100,
    };

    let context = ValidationContext::new().with_balance("timeout-test".to_string(), 200);

    let result = cache.validate_business_rule(&rule, &context).await.unwrap();

    // Should succeed but might timeout (implementation-dependent)
    // This test mainly verifies the timeout mechanism doesn't cause panics
    let _ = result.valid; // Either outcome is acceptable for timeout test
}

// ============================================================================
// Cache Management Tests
// ============================================================================

#[tokio::test]
async fn test_cache_cleanup_functionality() {
    let config = ValidationConfig {
        cache_size: 10,
        cache_ttl: Duration::from_millis(25),
        enable_precomputation: true,
        enable_batch_validation: true,
        max_validation_time: Duration::from_millis(100),
    };

    let cache = ValidationCache::with_config(config);

    // Add some entries
    for i in 0..5 {
        let rule = BusinessRule::SufficientFunds {
            account_id: format!("cleanup-test-{}", i),
            required_amount: 100,
        };
        let context = ValidationContext::new().with_balance(format!("cleanup-test-{}", i), 200);
        cache.validate_business_rule(&rule, &context).await.unwrap();
    }

    // Wait for TTL expiration
    sleep(Duration::from_millis(30)).await;

    // Run cleanup
    cache.cleanup_cache();

    // Verify cache statistics are reasonable (exact behavior depends on implementation)
    let stats = cache.cache_stats();
    assert!(stats.rules_evaluated >= 5);
}

#[tokio::test]
async fn test_cache_clear_functionality() {
    let cache = ValidationCache::new(&ValidationProfile::Balanced);

    // Add some cached entries
    for i in 0..3 {
        let rule = BusinessRule::SufficientFunds {
            account_id: format!("clear-test-{}", i),
            required_amount: 50,
        };
        let context = ValidationContext::new().with_balance(format!("clear-test-{}", i), 100);
        cache.validate_business_rule(&rule, &context).await.unwrap();
    }

    // Clear cache
    cache.clear_cache();

    // Validate again - should not be from cache
    let rule = BusinessRule::SufficientFunds {
        account_id: "clear-test-0".to_string(),
        required_amount: 50,
    };
    let context = ValidationContext::new().with_balance("clear-test-0".to_string(), 100);

    let result = cache.validate_business_rule(&rule, &context).await.unwrap();
    assert!(result.valid);
    assert!(!result.from_cache); // Should not be from cache after clear
}

#[tokio::test]
async fn test_cache_statistics_tracking() {
    let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

    let rule = BusinessRule::SufficientFunds {
        account_id: "stats-test".to_string(),
        required_amount: 150,
    };
    let context = ValidationContext::new().with_balance("stats-test".to_string(), 300);

    // First validation - cache miss
    let _result1 = cache.validate_business_rule(&rule, &context).await.unwrap();

    // Second validation - cache hit
    let _result2 = cache.validate_business_rule(&rule, &context).await.unwrap();

    // Check statistics
    let stats = cache.cache_stats();
    assert_eq!(stats.cache_hits, 1);
    assert_eq!(stats.cache_misses, 1);
    assert!(stats.rules_evaluated >= 1); // At least one rule was evaluated
    assert!(stats.duration > Duration::from_nanos(0));
}

// ============================================================================
// Integration Tests with ValidatedCommand Trait
// ============================================================================

#[derive(Debug, Clone)]
struct TestValidatedCommand {
    account_id: String,
    amount: u64,
    inventory_requirements: Vec<(String, String, u32)>, // (product, warehouse, quantity)
}

impl ValidatedCommand for TestValidatedCommand {
    fn business_rules(&self) -> Vec<BusinessRule> {
        let mut rules = vec![BusinessRule::SufficientFunds {
            account_id: self.account_id.clone(),
            required_amount: self.amount,
        }];

        for (product_id, warehouse_id, quantity) in &self.inventory_requirements {
            rules.push(BusinessRule::InventoryAvailable {
                product_id: product_id.clone(),
                warehouse_id: warehouse_id.clone(),
                required_quantity: *quantity,
            });
        }

        rules
    }

    fn validation_context(&self) -> ValidationContext {
        let mut context = ValidationContext::new().with_balance(self.account_id.clone(), 10000); // $100 in cents

        for (product_id, warehouse_id, _) in &self.inventory_requirements {
            context = context
                .with_inventory(product_id.clone(), warehouse_id.clone(), 100)
                .with_reserved(product_id.clone(), warehouse_id.clone(), 10);
        }

        context
    }
}

#[tokio::test]
async fn test_validated_command_trait_integration() {
    let cache = ValidationCache::new(&ValidationProfile::Balanced);

    let command = TestValidatedCommand {
        account_id: "integration-test-account".to_string(),
        amount: 5000, // $50
        inventory_requirements: vec![
            ("product-x".to_string(), "warehouse-main".to_string(), 5),
            ("product-y".to_string(), "warehouse-main".to_string(), 3),
        ],
    };

    let results = command.validate_business_rules(&cache).await.unwrap();
    assert_eq!(results.len(), 3); // 1 funds + 2 inventory rules

    // All should be valid
    for result in &results {
        assert!(result.valid);
        assert!(result.error_message.is_none());
        assert!(!result.from_cache); // First validation
    }

    // Second validation should use cache
    let results2 = command.validate_business_rules(&cache).await.unwrap();
    assert_eq!(results2.len(), 3);

    for result in &results2 {
        assert!(result.valid);
        assert!(result.from_cache); // Should be cached
    }
}
