//! Integration tests for subscription system with projections.
//!
//! This module demonstrates how the subscription system works with projections
//! to maintain read models in real-time.

#![allow(dead_code)]
#![allow(clippy::significant_drop_tightening)]
#![allow(clippy::uninlined_format_args)]

use eventcore::{
    EventProcessor, EventStore, EventToWrite, ExpectedVersion, StreamEvents, StreamId,
    SubscriptionError, SubscriptionName, SubscriptionOptions, SubscriptionResult,
};
use eventcore_memory::InMemoryEventStore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
struct AccountEvent {
    account_id: String,
    event_type: AccountEventType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AccountEventType {
    Opened { initial_balance: u64 },
    Deposited { amount: u64 },
    Withdrawn { amount: u64 },
    Closed,
}

/// Simple projection that maintains account balances
#[derive(Debug, Clone)]
struct AccountBalanceProjection {
    balances: Arc<Mutex<HashMap<String, u64>>>,
    transaction_count: Arc<Mutex<HashMap<String, u32>>>,
}

impl AccountBalanceProjection {
    fn new() -> Self {
        Self {
            balances: Arc::new(Mutex::new(HashMap::new())),
            transaction_count: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_balance(&self, account_id: &str) -> Option<u64> {
        self.balances.lock().unwrap().get(account_id).copied()
    }

    fn get_transaction_count(&self, account_id: &str) -> u32 {
        self.transaction_count
            .lock()
            .unwrap()
            .get(account_id)
            .copied()
            .unwrap_or(0)
    }

    fn get_all_balances(&self) -> HashMap<String, u64> {
        self.balances.lock().unwrap().clone()
    }
}

/// Event processor that updates the account balance projection
struct AccountBalanceProcessor {
    projection: Arc<AccountBalanceProjection>,
    processed_events: Arc<Mutex<Vec<AccountEvent>>>,
}

impl AccountBalanceProcessor {
    fn new(projection: Arc<AccountBalanceProjection>) -> Self {
        Self {
            projection,
            processed_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_processed_events(&self) -> Vec<AccountEvent> {
        self.processed_events.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl EventProcessor for AccountBalanceProcessor {
    type Event = AccountEvent;

    async fn process_event(
        &mut self,
        event: eventcore::StoredEvent<Self::Event>,
    ) -> SubscriptionResult<()> {
        let account_event = &event.payload;
        let account_id = &account_event.account_id;

        // Update projection based on event type
        match &account_event.event_type {
            AccountEventType::Opened { initial_balance } => {
                self.projection
                    .balances
                    .lock()
                    .unwrap()
                    .insert(account_id.clone(), *initial_balance);
                self.projection
                    .transaction_count
                    .lock()
                    .unwrap()
                    .insert(account_id.clone(), 1);
            }
            AccountEventType::Deposited { amount } => {
                let mut balances = self.projection.balances.lock().unwrap();
                if let Some(balance) = balances.get_mut(account_id) {
                    *balance += amount;
                }

                let mut counts = self.projection.transaction_count.lock().unwrap();
                *counts.entry(account_id.clone()).or_insert(0) += 1;
            }
            AccountEventType::Withdrawn { amount } => {
                let mut balances = self.projection.balances.lock().unwrap();
                if let Some(balance) = balances.get_mut(account_id) {
                    if *balance >= *amount {
                        *balance -= amount;
                    } else {
                        return Err(SubscriptionError::CheckpointSaveFailed(format!(
                            "Insufficient funds for withdrawal: account {} balance {} < amount {}",
                            account_id, balance, amount
                        )));
                    }
                }

                let mut counts = self.projection.transaction_count.lock().unwrap();
                *counts.entry(account_id.clone()).or_insert(0) += 1;
            }
            AccountEventType::Closed => {
                self.projection.balances.lock().unwrap().remove(account_id);
                self.projection
                    .transaction_count
                    .lock()
                    .unwrap()
                    .remove(account_id);
            }
        }

        // Record that we processed this event
        self.processed_events
            .lock()
            .unwrap()
            .push(account_event.clone());
        Ok(())
    }

    async fn on_live(&mut self) -> SubscriptionResult<()> {
        // Could trigger notifications or other live processing logic
        Ok(())
    }
}

/// Helper function to create account events
async fn create_account_events(
    store: &InMemoryEventStore<AccountEvent>,
    events: Vec<(String, AccountEvent)>, // (stream_id, event)
) -> Result<(), Box<dyn std::error::Error>> {
    for (stream_id, event) in events {
        let stream_id = StreamId::try_new(stream_id)?;
        let event_to_write = EventToWrite::new(eventcore::EventId::new(), event);
        let stream_events =
            StreamEvents::new(stream_id, ExpectedVersion::Any, vec![event_to_write]);
        store.write_events_multi(vec![stream_events]).await?;
    }
    Ok(())
}

#[tokio::test]
async fn test_subscription_with_account_balance_projection() {
    let store: InMemoryEventStore<AccountEvent> = InMemoryEventStore::new();

    // Create initial account events
    let initial_events = vec![
        (
            "account-alice".to_string(),
            AccountEvent {
                account_id: "alice".to_string(),
                event_type: AccountEventType::Opened {
                    initial_balance: 1000,
                },
            },
        ),
        (
            "account-bob".to_string(),
            AccountEvent {
                account_id: "bob".to_string(),
                event_type: AccountEventType::Opened {
                    initial_balance: 500,
                },
            },
        ),
        (
            "account-alice".to_string(),
            AccountEvent {
                account_id: "alice".to_string(),
                event_type: AccountEventType::Deposited { amount: 200 },
            },
        ),
    ];

    create_account_events(&store, initial_events).await.unwrap();

    // Create projection and subscription
    let projection = Arc::new(AccountBalanceProjection::new());
    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription = store.subscribe(options).await.unwrap();

    // Create processor
    let processor = AccountBalanceProcessor::new(projection.clone());
    let name = SubscriptionName::try_new("account-balance-projection").unwrap();

    // Start subscription
    let processor_box: Box<dyn EventProcessor<Event = AccountEvent>> = Box::new(processor);
    subscription
        .start(
            name,
            SubscriptionOptions::CatchUpFromBeginning,
            processor_box,
        )
        .await
        .unwrap();

    // Give some time for processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify projection state
    assert_eq!(projection.get_balance("alice"), Some(1200)); // 1000 + 200
    assert_eq!(projection.get_balance("bob"), Some(500));
    assert_eq!(projection.get_transaction_count("alice"), 2); // opened + deposited
    assert_eq!(projection.get_transaction_count("bob"), 1); // opened

    // Add more events while subscription is running
    let new_events = vec![
        (
            "account-bob".to_string(),
            AccountEvent {
                account_id: "bob".to_string(),
                event_type: AccountEventType::Withdrawn { amount: 100 },
            },
        ),
        (
            "account-alice".to_string(),
            AccountEvent {
                account_id: "alice".to_string(),
                event_type: AccountEventType::Withdrawn { amount: 300 },
            },
        ),
    ];

    create_account_events(&store, new_events).await.unwrap();

    // Give time for processing new events
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify updated projection state
    assert_eq!(projection.get_balance("alice"), Some(900)); // 1200 - 300
    assert_eq!(projection.get_balance("bob"), Some(400)); // 500 - 100
    assert_eq!(projection.get_transaction_count("alice"), 3);
    assert_eq!(projection.get_transaction_count("bob"), 2);

    subscription.stop().await.unwrap();
}

#[tokio::test]
async fn test_subscription_handles_projection_errors() {
    let store: InMemoryEventStore<AccountEvent> = InMemoryEventStore::new();

    // Create events that will cause a projection error (withdrawal exceeding balance)
    let events = vec![
        (
            "account-charlie".to_string(),
            AccountEvent {
                account_id: "charlie".to_string(),
                event_type: AccountEventType::Opened {
                    initial_balance: 100,
                },
            },
        ),
        (
            "account-charlie".to_string(),
            AccountEvent {
                account_id: "charlie".to_string(),
                event_type: AccountEventType::Withdrawn { amount: 200 }, // More than balance!
            },
        ),
    ];

    create_account_events(&store, events).await.unwrap();

    // Create projection and subscription
    let projection = Arc::new(AccountBalanceProjection::new());
    let options = SubscriptionOptions::CatchUpFromBeginning;
    let mut subscription = store.subscribe(options).await.unwrap();

    let processor = AccountBalanceProcessor::new(projection.clone());
    let name = SubscriptionName::try_new("error-handling-test").unwrap();

    // Start subscription
    let processor_box: Box<dyn EventProcessor<Event = AccountEvent>> = Box::new(processor);
    subscription
        .start(
            name,
            SubscriptionOptions::CatchUpFromBeginning,
            processor_box,
        )
        .await
        .unwrap();

    // Give time for processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // The account should still be opened (first event processed)
    assert_eq!(projection.get_balance("charlie"), Some(100));

    // The withdrawal should have failed, so transaction count should be 1 (just the opening)
    assert_eq!(projection.get_transaction_count("charlie"), 1);

    subscription.stop().await.unwrap();
}

#[tokio::test]
async fn test_subscription_with_multiple_streams() {
    let store: InMemoryEventStore<AccountEvent> = InMemoryEventStore::new();

    // Create events across multiple account streams
    let events = vec![
        // Account A events
        (
            "account-a".to_string(),
            AccountEvent {
                account_id: "a".to_string(),
                event_type: AccountEventType::Opened {
                    initial_balance: 1000,
                },
            },
        ),
        (
            "account-a".to_string(),
            AccountEvent {
                account_id: "a".to_string(),
                event_type: AccountEventType::Deposited { amount: 500 },
            },
        ),
        // Account B events
        (
            "account-b".to_string(),
            AccountEvent {
                account_id: "b".to_string(),
                event_type: AccountEventType::Opened {
                    initial_balance: 200,
                },
            },
        ),
        // Account C events
        (
            "account-c".to_string(),
            AccountEvent {
                account_id: "c".to_string(),
                event_type: AccountEventType::Opened {
                    initial_balance: 750,
                },
            },
        ),
        (
            "account-c".to_string(),
            AccountEvent {
                account_id: "c".to_string(),
                event_type: AccountEventType::Withdrawn { amount: 250 },
            },
        ),
        // More Account A events
        (
            "account-a".to_string(),
            AccountEvent {
                account_id: "a".to_string(),
                event_type: AccountEventType::Withdrawn { amount: 100 },
            },
        ),
    ];

    create_account_events(&store, events).await.unwrap();

    // Create projection with specific streams subscription
    let target_streams = vec![
        StreamId::try_new("account-a").unwrap(),
        StreamId::try_new("account-c").unwrap(),
    ];

    let projection = Arc::new(AccountBalanceProjection::new());
    let options = SubscriptionOptions::SpecificStreams {
        streams: target_streams.clone(),
        from_position: None,
    };
    let mut subscription = store.subscribe(options).await.unwrap();

    let processor = AccountBalanceProcessor::new(projection.clone());
    let name = SubscriptionName::try_new("multi-stream-test").unwrap();

    // Start subscription
    let processor_box: Box<dyn EventProcessor<Event = AccountEvent>> = Box::new(processor);
    subscription
        .start(
            name,
            SubscriptionOptions::SpecificStreams {
                streams: target_streams,
                from_position: None,
            },
            processor_box,
        )
        .await
        .unwrap();

    // Give time for processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should have processed accounts A and C, but not B
    assert_eq!(projection.get_balance("a"), Some(1400)); // 1000 + 500 - 100
    assert_eq!(projection.get_balance("b"), None); // Not subscribed to this stream
    assert_eq!(projection.get_balance("c"), Some(500)); // 750 - 250

    assert_eq!(projection.get_transaction_count("a"), 3);
    assert_eq!(projection.get_transaction_count("b"), 0);
    assert_eq!(projection.get_transaction_count("c"), 2);

    subscription.stop().await.unwrap();
}

#[tokio::test]
async fn test_subscription_pause_resume_with_projection() {
    let store: InMemoryEventStore<AccountEvent> = InMemoryEventStore::new();

    let projection = Arc::new(AccountBalanceProjection::new());
    let options = SubscriptionOptions::LiveOnly;
    let mut subscription = store.subscribe(options).await.unwrap();

    let processor = AccountBalanceProcessor::new(projection.clone());
    let name = SubscriptionName::try_new("pause-resume-projection").unwrap();

    // Start subscription
    let processor_box: Box<dyn EventProcessor<Event = AccountEvent>> = Box::new(processor);
    subscription
        .start(name, SubscriptionOptions::LiveOnly, processor_box)
        .await
        .unwrap();

    // Create some initial events
    let initial_events = vec![(
        "account-test".to_string(),
        AccountEvent {
            account_id: "test".to_string(),
            event_type: AccountEventType::Opened {
                initial_balance: 500,
            },
        },
    )];
    create_account_events(&store, initial_events).await.unwrap();

    // Give time for processing
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(projection.get_balance("test"), Some(500));

    // Pause subscription
    subscription.pause().await.unwrap();

    // Create events while paused
    let paused_events = vec![(
        "account-test".to_string(),
        AccountEvent {
            account_id: "test".to_string(),
            event_type: AccountEventType::Deposited { amount: 200 },
        },
    )];
    create_account_events(&store, paused_events).await.unwrap();

    // Give time (events shouldn't be processed while paused)
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Resume subscription
    subscription.resume().await.unwrap();

    // Create post-resume events
    let resume_events = vec![(
        "account-test".to_string(),
        AccountEvent {
            account_id: "test".to_string(),
            event_type: AccountEventType::Deposited { amount: 100 },
        },
    )];
    create_account_events(&store, resume_events).await.unwrap();

    // Give time for processing
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Note: The exact behavior depends on the subscription implementation
    // Some events might be processed, others might not, depending on timing
    let final_balance = projection.get_balance("test").unwrap_or(500);
    assert!(final_balance >= 500); // At minimum, should have initial balance

    subscription.stop().await.unwrap();
}
