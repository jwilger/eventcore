//! Benchmarks for business rule validation optimization
//!
//! This benchmark suite measures the performance improvements achieved by the
//! validation optimization system, comparing different caching strategies,
//! batch validation, and performance profiles.

#![allow(clippy::pedantic)]
#![allow(missing_docs)]

use criterion::async_executor::FuturesExecutor;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use eventcore::validation::{
    BusinessRule, ValidationCache, ValidationConfig, ValidationContext, ValidationProfile,
};
use std::hint::black_box;
use std::time::Duration;

// ============================================================================
// Benchmark Setup
// ============================================================================

fn create_sample_rules(count: usize) -> Vec<BusinessRule> {
    let mut rules = Vec::with_capacity(count);

    for i in 0..count {
        match i % 4 {
            0 => rules.push(BusinessRule::SufficientFunds {
                account_id: format!("account-{}", i % 10), // Reuse accounts for cache testing
                required_amount: 1000 + (i as u64 * 100),
            }),
            1 => rules.push(BusinessRule::InventoryAvailable {
                product_id: format!("product-{}", i % 5),
                warehouse_id: format!("warehouse-{}", i % 3),
                required_quantity: 10 + (i as u32 % 20),
            }),
            2 => rules.push(BusinessRule::CapacityLimit {
                resource_id: format!("resource-{}", i % 7),
                current_usage: 30 + (i as u32 % 40),
                additional_usage: 10 + (i as u32 % 15),
                max_capacity: 100,
            }),
            _ => rules.push(BusinessRule::Custom {
                rule_type: "benchmark_rule".to_string(),
                parameters: vec![
                    ("param1".to_string(), format!("value-{}", i)),
                    ("param2".to_string(), format!("other-{}", i % 3)),
                ],
            }),
        }
    }

    rules
}

fn create_comprehensive_context() -> ValidationContext {
    let mut context = ValidationContext::new();

    // Add balances for accounts
    for i in 0..10 {
        context = context.with_balance(format!("account-{}", i), 100000 + i * 10000);
    }

    // Add inventory for products
    for i in 0..5 {
        for j in 0..3 {
            context = context
                .with_inventory(
                    format!("product-{}", i),
                    format!("warehouse-{}", j),
                    100 + i * 20,
                )
                .with_reserved(
                    format!("product-{}", i),
                    format!("warehouse-{}", j),
                    5 + i * 2,
                );
        }
    }

    // Add capacity usage for resources
    for i in 0..7 {
        context = context.with_capacity_usage(format!("resource-{}", i), 30 + i * 5);
    }

    context
}

// ============================================================================
// Single Validation Benchmarks
// ============================================================================

fn bench_single_validation_no_cache(c: &mut Criterion) {
    c.bench_function("single_validation_no_cache", |b| {
        b.to_async(FuturesExecutor).iter(|| async {
            // Create new cache each time to avoid caching effects
            let cache = ValidationCache::new(&ValidationProfile::Conservative);

            let rule = BusinessRule::SufficientFunds {
                account_id: "bench-account".to_string(),
                required_amount: 5000,
            };

            let context = ValidationContext::new().with_balance("bench-account".to_string(), 10000);

            black_box(cache.validate_business_rule(&rule, &context).await.unwrap())
        });
    });
}

fn bench_single_validation_with_cache(c: &mut Criterion) {
    c.bench_function("single_validation_with_cache", |b| {
        let cache = ValidationCache::new(&ValidationProfile::HighPerformance);
        let rule = BusinessRule::SufficientFunds {
            account_id: "cached-account".to_string(),
            required_amount: 5000,
        };
        let context = ValidationContext::new().with_balance("cached-account".to_string(), 10000);

        // Prime the cache
        futures::executor::block_on(async {
            cache.validate_business_rule(&rule, &context).await.unwrap();
        });

        b.to_async(FuturesExecutor).iter(|| async {
            black_box(cache.validate_business_rule(&rule, &context).await.unwrap())
        });
    });
}

// ============================================================================
// Batch Validation Benchmarks
// ============================================================================

fn bench_batch_validation_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_validation_sizes");

    let cache = ValidationCache::new(&ValidationProfile::HighPerformance);
    let context = create_comprehensive_context();

    for size in [1, 5, 10, 25, 50, 100].iter() {
        let rules = create_sample_rules(*size);

        group.bench_with_input(BenchmarkId::new("batch_size", size), size, |b, _| {
            b.to_async(FuturesExecutor).iter(|| async {
                black_box(cache.validate_batch(&rules, &context).await.unwrap())
            });
        });
    }

    group.finish();
}

fn bench_batch_vs_individual_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_vs_individual");

    let rules = create_sample_rules(20);
    let context = create_comprehensive_context();

    // Batch validation
    group.bench_function("batch_validation", |b| {
        let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

        b.to_async(FuturesExecutor)
            .iter(|| async { black_box(cache.validate_batch(&rules, &context).await.unwrap()) });
    });

    // Individual validation
    group.bench_function("individual_validation", |b| {
        let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

        b.to_async(FuturesExecutor).iter(|| async {
            let mut results = Vec::new();
            for rule in &rules {
                results.push(cache.validate_business_rule(rule, &context).await.unwrap());
            }
            black_box(results)
        });
    });

    group.finish();
}

// ============================================================================
// Performance Profile Benchmarks
// ============================================================================

fn bench_validation_profiles(c: &mut Criterion) {
    let mut group = c.benchmark_group("validation_profiles");

    let rules = create_sample_rules(10);
    let context = create_comprehensive_context();

    let profiles = vec![
        ("conservative", ValidationProfile::Conservative),
        ("balanced", ValidationProfile::Balanced),
        ("high_performance", ValidationProfile::HighPerformance),
    ];

    for (name, profile) in profiles {
        group.bench_with_input(BenchmarkId::new("profile", name), &profile, |b, profile| {
            let cache = ValidationCache::new(profile);

            b.to_async(FuturesExecutor).iter(|| async {
                black_box(cache.validate_batch(&rules, &context).await.unwrap())
            });
        });
    }

    group.finish();
}

// ============================================================================
// Cache Performance Benchmarks
// ============================================================================

fn bench_cache_hit_ratios(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_hit_ratios");

    let context = create_comprehensive_context();

    // Test different cache hit scenarios
    let scenarios = vec![
        ("0_percent_hits", 0),   // All unique rules
        ("50_percent_hits", 50), // Half repeated rules
        ("90_percent_hits", 90), // Mostly repeated rules
    ];

    for (name, hit_percentage) in scenarios {
        group.bench_with_input(
            BenchmarkId::new("hit_ratio", name),
            &hit_percentage,
            |b, &hit_percentage| {
                let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

                b.to_async(FuturesExecutor).iter(|| async {
                    // Generate rules with specified hit ratio
                    let mut rules = Vec::new();
                    for i in 0..100 {
                        if i < hit_percentage {
                            // Repeated rule for cache hits
                            rules.push(BusinessRule::SufficientFunds {
                                account_id: "repeated-account".to_string(),
                                required_amount: 1000,
                            });
                        } else {
                            // Unique rule for cache misses
                            rules.push(BusinessRule::SufficientFunds {
                                account_id: format!("unique-account-{}", i),
                                required_amount: 1000 + i as u64,
                            });
                        }
                    }

                    black_box(cache.validate_batch(&rules, &context).await.unwrap())
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Complex Validation Scenarios
// ============================================================================

fn bench_realistic_order_processing(c: &mut Criterion) {
    c.bench_function("realistic_order_processing", |b| {
        let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

        b.to_async(FuturesExecutor).iter(|| async {
            // Simulate processing 10 orders with 3-5 items each
            for order_id in 0..10 {
                let mut rules = vec![
                    // Customer has sufficient funds
                    BusinessRule::SufficientFunds {
                        account_id: format!("customer-{}", order_id % 5), // Reuse some customers
                        required_amount: 5000 + (order_id * 1000),
                    },
                ];

                // Each order has 3-5 inventory checks
                let item_count = 3 + (order_id % 3);
                for item_id in 0..item_count {
                    rules.push(BusinessRule::InventoryAvailable {
                        product_id: format!("product-{}", item_id % 8), // Limited product catalog
                        warehouse_id: "main-warehouse".to_string(),
                        required_quantity: 1 + (item_id % 5) as u32,
                    });
                }

                let context = ValidationContext::new()
                    .with_balance(format!("customer-{}", order_id % 5), 100000)
                    .with_inventory("product-0".to_string(), "main-warehouse".to_string(), 100)
                    .with_inventory("product-1".to_string(), "main-warehouse".to_string(), 100)
                    .with_inventory("product-2".to_string(), "main-warehouse".to_string(), 100)
                    .with_inventory("product-3".to_string(), "main-warehouse".to_string(), 100)
                    .with_inventory("product-4".to_string(), "main-warehouse".to_string(), 100)
                    .with_inventory("product-5".to_string(), "main-warehouse".to_string(), 100)
                    .with_inventory("product-6".to_string(), "main-warehouse".to_string(), 100)
                    .with_inventory("product-7".to_string(), "main-warehouse".to_string(), 100);

                black_box(cache.validate_batch(&rules, &context).await.unwrap());
            }
        });
    });
}

fn bench_high_frequency_trading_simulation(c: &mut Criterion) {
    c.bench_function("high_frequency_trading_simulation", |b| {
        let cache = ValidationCache::new(&ValidationProfile::HighPerformance);

        b.to_async(FuturesExecutor).iter(|| async {
            // Simulate 100 rapid trading validations
            for trade_id in 0..100 {
                let rule = BusinessRule::SufficientFunds {
                    account_id: format!("trader-{}", trade_id % 10), // 10 active traders
                    required_amount: 1000 + (trade_id * 50),
                };

                let context = ValidationContext::new()
                    .with_balance(format!("trader-{}", trade_id % 10), 1000000);

                black_box(cache.validate_business_rule(&rule, &context).await.unwrap());
            }
        });
    });
}

// ============================================================================
// Memory and Resource Usage Benchmarks
// ============================================================================

fn bench_cache_memory_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_memory_efficiency");

    let cache_sizes = vec![100, 1000, 5000, 10000];

    for cache_size in cache_sizes {
        let config = ValidationConfig {
            cache_size,
            cache_ttl: Duration::from_secs(300),
            enable_precomputation: true,
            enable_batch_validation: true,
            max_validation_time: Duration::from_millis(100),
        };

        group.bench_with_input(
            BenchmarkId::new("cache_size", cache_size),
            &cache_size,
            |b, _| {
                let cache = ValidationCache::with_config(config.clone());
                let context = create_comprehensive_context();

                b.to_async(FuturesExecutor).iter(|| async {
                    // Fill cache up to its limit
                    for i in 0..cache_size {
                        let rule = BusinessRule::SufficientFunds {
                            account_id: format!("mem-test-{}", i),
                            required_amount: 1000,
                        };
                        let ctx = context
                            .clone()
                            .with_balance(format!("mem-test-{}", i), 2000);
                        black_box(cache.validate_business_rule(&rule, &ctx).await.unwrap());
                    }
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Benchmark Registration
// ============================================================================

criterion_group!(
    single_validation_benches,
    bench_single_validation_no_cache,
    bench_single_validation_with_cache
);

criterion_group!(
    batch_validation_benches,
    bench_batch_validation_sizes,
    bench_batch_vs_individual_validation
);

criterion_group!(profile_benches, bench_validation_profiles);

criterion_group!(cache_benches, bench_cache_hit_ratios);

criterion_group!(
    realistic_scenario_benches,
    bench_realistic_order_processing,
    bench_high_frequency_trading_simulation
);

criterion_group!(memory_benches, bench_cache_memory_efficiency);

criterion_main!(
    single_validation_benches,
    batch_validation_benches,
    profile_benches,
    cache_benches,
    realistic_scenario_benches,
    memory_benches
);
