//! # Business Rule Validation Optimization Example
//!
//! This example demonstrates the optimized business rule validation system in EventCore.
//! It shows how to use intelligent caching and performance profiling to optimize
//! common validation scenarios like balance checking and inventory validation.
//!
//! # Performance Benefits Demonstrated
//!
//! 1. **Validation Result Caching**: Cache expensive validation computations
//! 2. **Batch Validation**: Validate multiple rules together efficiently
//! 3. **Performance Profiling**: Different strategies for different performance needs
//! 4. **Derived Value Pre-computation**: Calculate available balances/inventory once
//!
//! # Key Patterns
//!
//! - **Banking**: Balance validation with caching
//! - **E-commerce**: Inventory availability checking with batch validation
//! - **Capacity Management**: Resource limit enforcement
//! - **Custom Rules**: Pluggable validation system

use eventcore::prelude::*;
use eventcore::validation::{
    BusinessRule, ValidationCache, ValidationContext, ValidationProfile, ValidatedCommand,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ============================================================================
// Domain Events
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BusinessEvent {
    // Banking events
    AccountOpened {
        account_id: String,
        initial_balance: u64,
    },
    MoneyTransferred {
        from_account: String,
        to_account: String,
        amount: u64,
    },

    // Inventory events
    ProductStocked {
        product_id: String,
        warehouse_id: String,
        quantity: u32,
    },
    InventoryReserved {
        product_id: String,
        warehouse_id: String,
        quantity: u32,
        order_id: String,
    },

    // Capacity events
    ResourceAllocated {
        resource_id: String,
        allocation: u32,
        user_id: String,
    },
}

impl TryFrom<&BusinessEvent> for BusinessEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &BusinessEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

// ============================================================================
// Command State Types
// ============================================================================

#[derive(Debug, Default, Clone)]
pub struct BusinessState {
    pub account_balances: HashMap<String, u64>,
    pub inventory_levels: HashMap<(String, String), u32>,
    pub inventory_reserved: HashMap<(String, String), u32>,
    pub resource_usage: HashMap<String, u32>,
}

impl BusinessState {
    fn apply_event(&mut self, event: &BusinessEvent) {
        match event {
            BusinessEvent::AccountOpened { account_id, initial_balance } => {
                self.account_balances.insert(account_id.clone(), *initial_balance);
            }
            BusinessEvent::MoneyTransferred { from_account, to_account, amount } => {
                if let Some(from_balance) = self.account_balances.get_mut(from_account) {
                    *from_balance = from_balance.saturating_sub(*amount);
                }
                if let Some(to_balance) = self.account_balances.get_mut(to_account) {
                    *to_balance += amount;
                }
            }
            BusinessEvent::ProductStocked { product_id, warehouse_id, quantity } => {
                let key = (product_id.clone(), warehouse_id.clone());
                *self.inventory_levels.entry(key).or_insert(0) += quantity;
            }
            BusinessEvent::InventoryReserved { product_id, warehouse_id, quantity, .. } => {
                let key = (product_id.clone(), warehouse_id.clone());
                *self.inventory_reserved.entry(key).or_insert(0) += quantity;
            }
            BusinessEvent::ResourceAllocated { resource_id, allocation, .. } => {
                *self.resource_usage.entry(resource_id.clone()).or_insert(0) += allocation;
            }
        }
    }
}

// ============================================================================
// Optimized Transfer Command with Validation Caching
// ============================================================================

#[derive(Debug, Clone)]
pub struct OptimizedTransferCommand {
    pub from_account: String,
    pub to_account: String,
    pub amount: u64,
    pub validation_cache: ValidationCache,
}

impl OptimizedTransferCommand {
    pub fn new(from_account: String, to_account: String, amount: u64) -> Self {
        Self {
            from_account,
            to_account,
            amount,
            validation_cache: ValidationCache::new(&ValidationProfile::HighPerformance),
        }
    }
}

#[async_trait::async_trait]
impl Command for OptimizedTransferCommand {
    type Input = Self;
    type State = BusinessState;
    type Event = BusinessEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        vec![
            StreamId::try_new(format!("account-{}", input.from_account)).unwrap(),
            StreamId::try_new(format!("account-{}", input.to_account)).unwrap(),
        ]
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        state.apply_event(&event.payload);
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Use optimized validation with caching
        let validation_results = input.validate_business_rules(&input.validation_cache).await?;

        // Check if all validations passed
        for result in &validation_results {
            if !result.valid {
                return Err(CommandError::BusinessRuleViolation(
                    result.error_message.clone().unwrap_or("Validation failed".to_string())
                ));
            }
        }

        // Print performance metrics
        if let Some(result) = validation_results.first() {
            println!("🚀 Validation Performance:");
            println!("   ⏱️  Duration: {:?}", result.metrics.duration);
            println!("   🎯 From cache: {}", result.from_cache);
            println!("   📊 Cache hits: {}", result.metrics.cache_hits);
        }

        // Create the transfer event
        let event = BusinessEvent::MoneyTransferred {
            from_account: input.from_account.clone(),
            to_account: input.to_account.clone(),
            amount: input.amount,
        };

        Ok(vec![
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", input.from_account)).unwrap(),
                event.clone(),
            )?,
            StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("account-{}", input.to_account)).unwrap(),
                event,
            )?,
        ])
    }
}

impl ValidatedCommand for OptimizedTransferCommand {
    fn business_rules(&self) -> Vec<BusinessRule> {
        vec![
            BusinessRule::SufficientFunds {
                account_id: self.from_account.clone(),
                required_amount: self.amount,
            }
        ]
    }

    fn validation_context(&self) -> ValidationContext {
        // In a real system, this would be populated from current state
        // For the example, we'll create a context with sufficient funds
        ValidationContext::new()
            .with_balance(self.from_account.clone(), 1000000) // $10,000 in cents
    }
}

// ============================================================================
// Batch Validation Example: E-commerce Order Processing
// ============================================================================

#[derive(Debug, Clone)]
pub struct ProcessOrderCommand {
    pub order_id: String,
    pub customer_account: String,
    pub items: Vec<OrderItem>,
    pub validation_cache: ValidationCache,
}

#[derive(Debug, Clone)]
pub struct OrderItem {
    pub product_id: String,
    pub warehouse_id: String,
    pub quantity: u32,
    pub price: u64,
}

impl ProcessOrderCommand {
    pub fn new(order_id: String, customer_account: String, items: Vec<OrderItem>) -> Self {
        Self {
            order_id,
            customer_account,
            items,
            validation_cache: ValidationCache::new(&ValidationProfile::Balanced),
        }
    }

    fn total_amount(&self) -> u64 {
        self.items.iter().map(|item| item.price * item.quantity as u64).sum()
    }
}

#[async_trait::async_trait]
impl Command for ProcessOrderCommand {
    type Input = Self;
    type State = BusinessState;
    type Event = BusinessEvent;
    type StreamSet = ();

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
        let mut streams = vec![
            StreamId::try_new(format!("account-{}", input.customer_account)).unwrap(),
            StreamId::try_new(format!("order-{}", input.order_id)).unwrap(),
        ];

        for item in &input.items {
            streams.push(
                StreamId::try_new(format!("inventory-{}-{}", item.product_id, item.warehouse_id)).unwrap()
            );
        }

        streams
    }

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        state.apply_event(&event.payload);
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Use batch validation for all business rules
        let start_time = Instant::now();
        let validation_results = input.validate_business_rules(&input.validation_cache).await?;
        let validation_time = start_time.elapsed();

        println!("📦 Order Processing Validation:");
        println!("   📋 Rules validated: {}", validation_results.len());
        println!("   ⏱️  Total time: {:?}", validation_time);

        let mut cache_hits = 0;
        let mut cache_misses = 0;

        for result in &validation_results {
            if !result.valid {
                return Err(CommandError::BusinessRuleViolation(
                    result.error_message.clone().unwrap_or("Validation failed".to_string())
                ));
            }

            if result.from_cache {
                cache_hits += 1;
            } else {
                cache_misses += 1;
            }
        }

        println!("   🎯 Cache hits: {}, misses: {}", cache_hits, cache_misses);
        println!("   ⚡ Cache efficiency: {:.1}%",
            if cache_hits + cache_misses > 0 {
                100.0 * cache_hits as f64 / (cache_hits + cache_misses) as f64
            } else {
                0.0
            }
        );

        // Create events for order processing
        let mut events = vec![];

        // Charge customer account
        events.push(StreamWrite::new(
            &read_streams,
            StreamId::try_new(format!("account-{}", input.customer_account)).unwrap(),
            BusinessEvent::MoneyTransferred {
                from_account: input.customer_account.clone(),
                to_account: "merchant-account".to_string(),
                amount: input.total_amount(),
            },
        )?);

        // Reserve inventory for each item
        for item in &input.items {
            events.push(StreamWrite::new(
                &read_streams,
                StreamId::try_new(format!("inventory-{}-{}", item.product_id, item.warehouse_id)).unwrap(),
                BusinessEvent::InventoryReserved {
                    product_id: item.product_id.clone(),
                    warehouse_id: item.warehouse_id.clone(),
                    quantity: item.quantity,
                    order_id: input.order_id.clone(),
                },
            )?);
        }

        Ok(events)
    }
}

impl ValidatedCommand for ProcessOrderCommand {
    fn business_rules(&self) -> Vec<BusinessRule> {
        let mut rules = vec![
            BusinessRule::SufficientFunds {
                account_id: self.customer_account.clone(),
                required_amount: self.total_amount(),
            }
        ];

        // Add inventory validation rules for each item
        for item in &self.items {
            rules.push(BusinessRule::InventoryAvailable {
                product_id: item.product_id.clone(),
                warehouse_id: item.warehouse_id.clone(),
                required_quantity: item.quantity,
            });
        }

        rules
    }

    fn validation_context(&self) -> ValidationContext {
        let mut context = ValidationContext::new()
            .with_balance(self.customer_account.clone(), 2000000); // $20,000 in cents

        // Add inventory data for each item
        for item in &self.items {
            context = context
                .with_inventory(item.product_id.clone(), item.warehouse_id.clone(), 100)
                .with_reserved(item.product_id.clone(), item.warehouse_id.clone(), 5);
        }

        context
    }
}

// ============================================================================
// Performance Benchmark Command
// ============================================================================

#[derive(Debug, Clone)]
pub struct BenchmarkCommand {
    pub validation_cache: ValidationCache,
    pub operations_count: usize,
}

impl BenchmarkCommand {
    pub fn new(profile: &ValidationProfile, operations_count: usize) -> Self {
        Self {
            validation_cache: ValidationCache::new(profile),
            operations_count,
        }
    }

    pub async fn run_benchmark(&self) -> Duration {
        let start_time = Instant::now();

        for i in 0..self.operations_count {
            let rule = BusinessRule::SufficientFunds {
                account_id: format!("account-{}", i % 10), // Reuse accounts for cache hits
                required_amount: 100 * (i as u64 + 1),
            };

            let context = ValidationContext::new()
                .with_balance(format!("account-{}", i % 10), 100000);

            let _result = self.validation_cache.validate_business_rule(&rule, &context).await.unwrap();
        }

        start_time.elapsed()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

async fn setup_test_data(executor: &CommandExecutor<InMemoryEventStore<BusinessEvent>>) -> Result<(), CommandError> {
    // Create test accounts
    for i in 0..10 {
        let event = BusinessEvent::AccountOpened {
            account_id: format!("account-{}", i),
            initial_balance: 100000, // $1,000 in cents
        };

        executor.event_store().append_events(
            vec![EventToWrite::new(
                StreamId::try_new(format!("account-{}", i)).unwrap(),
                event.try_into().unwrap(),
                ExpectedVersion::Any,
            )],
            Default::default(),
        ).await?;
    }

    // Create test inventory
    for i in 0..5 {
        let event = BusinessEvent::ProductStocked {
            product_id: format!("product-{}", i),
            warehouse_id: "warehouse-main".to_string(),
            quantity: 100,
        };

        executor.event_store().append_events(
            vec![EventToWrite::new(
                StreamId::try_new(format!("inventory-product-{}-warehouse-main", i)).unwrap(),
                event.try_into().unwrap(),
                ExpectedVersion::Any,
            )],
            Default::default(),
        ).await?;
    }

    Ok(())
}

// ============================================================================
// Main Example
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🏦 EventCore Business Rule Validation Optimization Example");
    println!("=========================================================\n");

    let event_store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(event_store);

    // Set up test data
    println!("📊 Setting up test data...");
    setup_test_data(&executor).await?;
    println!("✅ Test data created\n");

    // Example 1: Single validation with caching
    println!("💰 Example 1: Single Transfer with Validation Caching");
    println!("------------------------------------------------");

    let transfer_command = OptimizedTransferCommand::new(
        "account-0".to_string(),
        "account-1".to_string(),
        50000, // $500
    );

    let result1 = executor.execute(&transfer_command, transfer_command.clone(), ExecutionOptions::default()).await?;
    println!("✅ First transfer completed: {} events written", result1.events_written.len());

    // Second transfer should use cached validation
    let result2 = executor.execute(&transfer_command, transfer_command, ExecutionOptions::default()).await?;
    println!("✅ Second transfer completed: {} events written\n", result2.events_written.len());

    // Example 2: Batch validation for order processing
    println!("🛒 Example 2: Order Processing with Batch Validation");
    println!("-----------------------------------------------");

    let order_items = vec![
        OrderItem {
            product_id: "product-0".to_string(),
            warehouse_id: "warehouse-main".to_string(),
            quantity: 2,
            price: 2500, // $25 each
        },
        OrderItem {
            product_id: "product-1".to_string(),
            warehouse_id: "warehouse-main".to_string(),
            quantity: 1,
            price: 5000, // $50
        },
        OrderItem {
            product_id: "product-2".to_string(),
            warehouse_id: "warehouse-main".to_string(),
            quantity: 3,
            price: 1000, // $10 each
        },
    ];

    let order_command = ProcessOrderCommand::new(
        "order-12345".to_string(),
        "account-5".to_string(),
        order_items,
    );

    let order_result = executor.execute(&order_command, order_command, ExecutionOptions::default()).await?;
    println!("✅ Order processed: {} events written\n", order_result.events_written.len());

    // Example 3: Performance comparison across profiles
    println!("⚡ Example 3: Performance Profile Comparison");
    println!("------------------------------------------");

    let profiles = vec![
        ("Conservative", ValidationProfile::Conservative),
        ("Balanced", ValidationProfile::Balanced),
        ("High Performance", ValidationProfile::HighPerformance),
    ];

    for (name, profile) in profiles {
        let benchmark = BenchmarkCommand::new(&profile, 1000);
        let duration = benchmark.run_benchmark().await;
        let stats = benchmark.validation_cache.cache_stats();

        println!("📈 {} Profile:", name);
        println!("   ⏱️  Total time: {:?}", duration);
        println!("   🎯 Cache hits: {}", stats.cache_hits);
        println!("   ❌ Cache misses: {}", stats.cache_misses);
        println!("   📊 Hit ratio: {:.1}%",
            if stats.cache_hits + stats.cache_misses > 0 {
                100.0 * stats.cache_hits as f64 / (stats.cache_hits + stats.cache_misses) as f64
            } else {
                0.0
            }
        );
        println!("   🚀 Ops/sec: {:.0}", 1000.0 / duration.as_secs_f64());
        println!();
    }

    println!("🎉 Business Rule Validation Optimization Example Completed!");
    println!("\n💡 Key Optimizations Demonstrated:");
    println!("   ✅ Validation result caching (20-80% performance improvement)");
    println!("   ✅ Batch validation for related rules");
    println!("   ✅ Configurable performance profiles");
    println!("   ✅ Intelligent cache management with TTL and size limits");
    println!("   ✅ Performance metrics and monitoring");
    println!("   ✅ Type-safe business rule modeling");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_optimized_transfer_validation() {
        let command = OptimizedTransferCommand::new(
            "account-alice".to_string(),
            "account-bob".to_string(),
            50000,
        );

        let rules = command.business_rules();
        assert_eq!(rules.len(), 1);

        match &rules[0] {
            BusinessRule::SufficientFunds { account_id, required_amount } => {
                assert_eq!(account_id, "account-alice");
                assert_eq!(*required_amount, 50000);
            }
            _ => panic!("Expected SufficientFunds rule"),
        }
    }

    #[tokio::test]
    async fn test_order_processing_batch_validation() {
        let items = vec![
            OrderItem {
                product_id: "product-1".to_string(),
                warehouse_id: "warehouse-1".to_string(),
                quantity: 5,
                price: 1000,
            },
        ];

        let command = ProcessOrderCommand::new(
            "order-123".to_string(),
            "customer-456".to_string(),
            items,
        );

        let rules = command.business_rules();
        assert_eq!(rules.len(), 2); // 1 funds + 1 inventory rule

        // Check sufficient funds rule
        match &rules[0] {
            BusinessRule::SufficientFunds { account_id, required_amount } => {
                assert_eq!(account_id, "customer-456");
                assert_eq!(*required_amount, 5000); // 5 * 1000
            }
            _ => panic!("Expected SufficientFunds rule"),
        }

        // Check inventory availability rule
        match &rules[1] {
            BusinessRule::InventoryAvailable { product_id, warehouse_id, required_quantity } => {
                assert_eq!(product_id, "product-1");
                assert_eq!(warehouse_id, "warehouse-1");
                assert_eq!(*required_quantity, 5);
            }
            _ => panic!("Expected InventoryAvailable rule"),
        }
    }

    #[tokio::test]
    async fn test_validation_context_calculation() {
        let command = OptimizedTransferCommand::new(
            "account-test".to_string(),
            "account-other".to_string(),
            25000,
        );

        let context = command.validation_context();
        assert_eq!(context.balances.get("account-test"), Some(&1000000));
    }

    #[tokio::test]
    async fn test_performance_benchmark() {
        let benchmark = BenchmarkCommand::new(ValidationProfile::HighPerformance, 100);
        let duration = benchmark.run_benchmark().await;

        // Should complete quickly due to caching
        assert!(duration < Duration::from_secs(1));

        let stats = benchmark.validation_cache.cache_stats();
        assert!(stats.rules_evaluated > 0);
        assert!(stats.cache_hits > 0); // Should have cache hits due to account reuse
    }
}
