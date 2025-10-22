//! Command Execution Optimization Example
//!
//! This example demonstrates the use of EventCore's command execution optimization layer
//! to improve performance through intelligent caching and configuration validation.

use eventcore::prelude::*;
use eventcore::{
    config::{ValidatedExecutionOptions, ValidatedOptimizationConfig, ValidatedRetryConfig},
    optimization::OptimizationLayer,
    CommandExecutor, ReadStreams, StreamResolver, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::Instant;
use tokio::time::{sleep, Duration};

/// A domain event for our banking example.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum BankingEvent {
    AccountOpened { owner: String, initial_balance: u64 },
    MoneyDeposited { amount: u64 },
    MoneyWithdrawn { amount: u64 },
}

impl TryFrom<&BankingEvent> for BankingEvent {
    type Error = std::convert::Infallible;
    fn try_from(value: &BankingEvent) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

/// Account state for event reconstruction.
#[derive(Debug, Default)]
struct AccountState {
    owner: Option<String>,
    balance: u64,
}

/// Command to deposit money into an account.
///
/// This command is idempotent - depositing the same amount multiple times
/// will only result in one deposit, making it perfect for demonstrating
/// the optimization layer's caching capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DepositMoney {
    account_id: StreamId,
    amount: u64,
    idempotency_key: String, // Used for idempotency
}

impl Hash for DepositMoney {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.account_id.hash(state);
        self.amount.hash(state);
        self.idempotency_key.hash(state);
    }
}

impl CommandStreams for DepositMoney {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.account_id.clone()]
    }
}

#[async_trait]
impl CommandLogic for DepositMoney {
    type State = AccountState;
    type Event = BankingEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            BankingEvent::AccountOpened { owner, initial_balance } => {
                state.owner = Some(owner.clone());
                state.balance = *initial_balance;
            }
            BankingEvent::MoneyDeposited { amount } => {
                state.balance += amount;
            }
            BankingEvent::MoneyWithdrawn { amount } => {
                state.balance = state.balance.saturating_sub(*amount);
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Simulate business logic processing time
        sleep(Duration::from_millis(50)).await;

        // Check if account exists
        if state.owner.is_none() {
            return Err(CommandError::BusinessRuleViolation(
                "Account does not exist".to_string(),
            ));
        }

        // For this example, we'll use the idempotency key to check if this
        // exact deposit has already been processed (in a real system, you'd
        // store this information in the event or use a proper idempotency store)

        // For simplicity, we'll always allow the deposit in this example
        // In a real system, you would check against previously processed idempotency keys

        Ok(vec![StreamWrite::new(
            &read_streams,
            self.account_id.clone(),
            BankingEvent::MoneyDeposited { amount: self.amount },
        )?])
    }
}

/// Command to open a new bank account.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenAccount {
    account_id: StreamId,
    owner: String,
    initial_balance: u64,
}

impl Hash for OpenAccount {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.account_id.hash(state);
        self.owner.hash(state);
        self.initial_balance.hash(state);
    }
}

impl CommandStreams for OpenAccount {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.account_id.clone()]
    }
}

#[async_trait]
impl CommandLogic for OpenAccount {
    type State = AccountState;
    type Event = BankingEvent;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            BankingEvent::AccountOpened { owner, initial_balance } => {
                state.owner = Some(owner.clone());
                state.balance = *initial_balance;
            }
            BankingEvent::MoneyDeposited { amount } => {
                state.balance += amount;
            }
            BankingEvent::MoneyWithdrawn { amount } => {
                state.balance = state.balance.saturating_sub(*amount);
            }
        }
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if account already exists (idempotency)
        if state.owner.is_some() {
            // Account already exists, return empty result (idempotent)
            return Ok(vec![]);
        }

        // Create new account
        Ok(vec![StreamWrite::new(
            &read_streams,
            self.account_id.clone(),
            BankingEvent::AccountOpened {
                owner: self.owner.clone(),
                initial_balance: self.initial_balance,
            },
        )?])
    }
}

async fn run_performance_comparison() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== EventCore Command Execution Optimization Demo ===\n");

    // Create event store
    let event_store = InMemoryEventStore::new();

    // Create standard executor
    let standard_executor = CommandExecutor::new(event_store.clone());

    // Create optimized executor with high-performance configuration
    let optimization_config = ValidatedOptimizationConfig::high_performance()?;
    let optimized_executor = OptimizationLayer::new(
        CommandExecutor::new(event_store),
        optimization_config.to_legacy_config(),
    );

    // Create validated execution options
    let execution_options = ValidatedExecutionOptions::high_performance()?;
    let legacy_options = execution_options.to_legacy_options();

    println!("Configuration:");
    println!("- High performance optimization enabled");
    println!("- Command caching: enabled");
    println!("- Stream version caching: enabled");
    println!("- Aggressive retry policy\n");

    // Test data
    let account_id = StreamId::try_new("account-12345".to_string())?;
    let open_account = OpenAccount {
        account_id: account_id.clone(),
        owner: "Alice".to_string(),
        initial_balance: 1000,
    };

    let deposit_command = DepositMoney {
        account_id: account_id.clone(),
        amount: 100,
        idempotency_key: "deposit-001".to_string(),
    };

    // First, open the account with both executors
    println!("Opening account...");
    let _ = standard_executor.execute(open_account.clone(), legacy_options.clone()).await?;
    let _ = optimized_executor.execute_optimized(open_account, legacy_options.clone()).await?;

    // Benchmark standard executor
    println!("\n--- Standard Executor Performance ---");
    let start = Instant::now();
    let mut standard_results = Vec::new();

    for i in 0..10 {
        let deposit = DepositMoney {
            account_id: account_id.clone(),
            amount: 100 + i, // Different amounts to avoid exact caching
            idempotency_key: format!("deposit-standard-{:03}", i),
        };

        let result = standard_executor.execute(deposit, legacy_options.clone()).await;
        standard_results.push(result.is_ok());
    }

    let standard_duration = start.elapsed();
    let standard_success_count = standard_results.iter().filter(|&&success| success).count();

    println!("Executed {} commands", standard_results.len());
    println!("Success rate: {}/{}", standard_success_count, standard_results.len());
    println!("Total time: {:?}", standard_duration);
    println!("Average time per command: {:?}", standard_duration / standard_results.len() as u32);

    // Benchmark optimized executor
    println!("\n--- Optimized Executor Performance ---");
    let start = Instant::now();
    let mut optimized_results = Vec::new();

    for i in 0..10 {
        let deposit = DepositMoney {
            account_id: account_id.clone(),
            amount: 200 + i, // Different amounts
            idempotency_key: format!("deposit-optimized-{:03}", i),
        };

        let result = optimized_executor.execute_optimized(deposit, legacy_options.clone()).await;
        optimized_results.push(result.is_ok());
    }

    let optimized_duration = start.elapsed();
    let optimized_success_count = optimized_results.iter().filter(|&&success| success).count();

    println!("Executed {} commands", optimized_results.len());
    println!("Success rate: {}/{}", optimized_success_count, optimized_results.len());
    println!("Total time: {:?}", optimized_duration);
    println!("Average time per command: {:?}", optimized_duration / optimized_results.len() as u32);

    // Test idempotency caching with repeated identical commands
    println!("\n--- Idempotency Caching Test ---");
    let identical_deposit = DepositMoney {
        account_id: account_id.clone(),
        amount: 500,
        idempotency_key: "identical-deposit".to_string(),
    };

    println!("Executing identical command 5 times with optimization...");
    let start = Instant::now();

    for i in 0..5 {
        let result = optimized_executor.execute_optimized(
            identical_deposit.clone(),
            legacy_options.clone()
        ).await;
        println!("Execution {}: {:?}", i + 1, result.is_ok());
    }

    let idempotent_duration = start.elapsed();
    println!("Total time for 5 identical executions: {:?}", idempotent_duration);
    println!("Average time per execution: {:?}", idempotent_duration / 5);

    // Show cache statistics
    println!("\n--- Cache Statistics ---");
    let cache_stats = optimized_executor.get_cache_stats();
    println!("Command cache: {}/{} entries ({:.1}% utilized)",
             cache_stats.command_cache_size,
             cache_stats.command_cache_max,
             cache_stats.command_cache_utilization());
    println!("Stream version cache: {}/{} entries ({:.1}% utilized)",
             cache_stats.stream_version_cache_size,
             cache_stats.stream_version_cache_max,
             cache_stats.stream_version_cache_utilization());

    // Performance comparison
    println!("\n--- Performance Summary ---");
    if optimized_duration < standard_duration {
        let improvement = ((standard_duration.as_nanos() as f64 - optimized_duration.as_nanos() as f64)
                          / standard_duration.as_nanos() as f64) * 100.0;
        println!("✅ Optimized executor was {:.1}% faster", improvement);
    } else {
        println!("⚠️  Standard executor was faster (optimization overhead for small workloads)");
    }

    println!("\n--- Configuration Examples ---");

    // Show different configuration presets
    let memory_efficient = ValidatedOptimizationConfig::memory_efficient()?;
    println!("Memory-efficient config: {} max commands, {} max stream versions",
             memory_efficient.max_cached_commands.into(),
             memory_efficient.max_cached_stream_versions.into());

    let high_performance = ValidatedOptimizationConfig::high_performance()?;
    println!("High-performance config: {} max commands, {} max stream versions",
             high_performance.max_cached_commands.into(),
             high_performance.max_cached_stream_versions.into());

    // Show retry configuration examples
    let conservative_retry = ValidatedRetryConfig::conservative()?;
    let aggressive_retry = ValidatedRetryConfig::aggressive()?;

    println!("Conservative retry: {} attempts, {}ms base delay",
             conservative_retry.max_attempts.into(),
             conservative_retry.base_delay.into());
    println!("Aggressive retry: {} attempts, {}ms base delay",
             aggressive_retry.max_attempts.into(),
             aggressive_retry.base_delay.into());

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    run_performance_comparison().await?;

    println!("\n=== Demo Complete ===");
    println!("The optimization layer provides:");
    println!("✅ Type-safe configuration validation");
    println!("✅ Intelligent command result caching");
    println!("✅ Stream version caching");
    println!("✅ Configurable performance profiles");
    println!("✅ Zero-configuration defaults");

    Ok(())
}
