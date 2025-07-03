//! Banking example application
//!
//! This example demonstrates a simple banking system with:
//! - Account creation
//! - Money transfers between accounts
//! - Balance tracking via projections

use anyhow::Result;
use eventcore::{CommandExecutor, Event, EventStore, ExecutionOptions, Projection, ReadOptions};
use eventcore_examples::banking::{
    commands::{OpenAccount, TransferMoneyCommand},
    events::BankingEvent,
    projections::AccountBalanceProjectionImpl,
    types::{AccountHolder, AccountId, CustomerName, Money, TransferId},
};
use eventcore_memory::InMemoryEventStore;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting banking example");

    // Create an in-memory event store
    let event_store: InMemoryEventStore<BankingEvent> = InMemoryEventStore::new();

    // Create command executor
    let executor = CommandExecutor::new(event_store.clone());

    // Create two accounts
    let alice_id = AccountId::try_new("ACC-ALICE".to_string())?;
    let bob_id = AccountId::try_new("ACC-BOB".to_string())?;

    // Open Alice's account with $1000
    info!("Opening Alice's account with $1,000");
    let open_alice = OpenAccount::new(
        alice_id.clone(),
        AccountHolder {
            name: CustomerName::try_new("Alice Smith".to_string())?,
            email: "alice@example.com".to_string(),
        },
        Money::from_cents(100_000)?, // $1,000
    );

    executor
        .execute(open_alice, ExecutionOptions::default())
        .await?;

    // Open Bob's account with $500
    info!("Opening Bob's account with $500");
    let open_bob = OpenAccount::new(
        bob_id.clone(),
        AccountHolder {
            name: CustomerName::try_new("Bob Jones".to_string())?,
            email: "bob@example.com".to_string(),
        },
        Money::from_cents(50_000)?, // $500
    );

    executor
        .execute(open_bob, ExecutionOptions::default())
        .await?;

    // Transfer $200 from Alice to Bob
    info!("Transferring $200 from Alice to Bob");
    let transfer = TransferMoneyCommand::new(
        TransferId::generate(),
        alice_id.clone(),
        bob_id.clone(),
        Money::from_cents(20_000)?, // $200
        Some("Payment for services".to_string()),
    )?;

    executor
        .execute(transfer, ExecutionOptions::default())
        .await?;

    // Create and update projection
    let mut projection = AccountBalanceProjectionImpl::new();

    // Read all events and apply to projection
    info!("Building account balance projection");
    let alice_stream = eventcore::StreamId::try_new(format!("account-{alice_id}"))?;
    let bob_stream = eventcore::StreamId::try_new(format!("account-{bob_id}"))?;

    let stream_ids = vec![alice_stream, bob_stream];
    let stream_data = event_store
        .read_streams(&stream_ids, &ReadOptions::default())
        .await?;

    // Apply events to the projection
    for stored_event in stream_data.events() {
        // Create an Event from the StoredEvent
        let event = Event::new(
            stored_event.stream_id.clone(),
            stored_event.payload.clone(),
            stored_event.metadata.clone().unwrap_or_default(),
        );
        AccountBalanceProjectionImpl::apply_event(&mut projection, &event).await?;
    }

    // Display final balances
    info!("Final account balances:");
    let state = projection.get_state().await?;
    for (account_id, balance) in &state.balances {
        info!(
            "  {}: {} (transactions: {})",
            account_id, balance.balance, balance.transaction_count
        );
    }

    // Try an invalid transfer (insufficient funds)
    info!("Attempting transfer with insufficient funds");
    let invalid_transfer = TransferMoneyCommand::new(
        TransferId::generate(),
        alice_id.clone(),
        bob_id.clone(),
        Money::from_cents(200_000)?, // $2,000 (more than Alice has)
        Some("This should fail".to_string()),
    )?;

    match executor
        .execute(&invalid_transfer, ExecutionOptions::default())
        .await
    {
        Ok(_) => panic!("Transfer should have failed"),
        Err(e) => info!("Transfer correctly rejected: {}", e),
    }

    info!("Banking example completed successfully");
    Ok(())
}
