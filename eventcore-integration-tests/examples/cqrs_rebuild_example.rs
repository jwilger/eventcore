//! Example demonstrating CQRS projection rebuild capabilities.
//!
//! This example shows:
//! - Setting up a CQRS projection with read model storage
//! - Performing different types of rebuilds
//! - Monitoring rebuild progress
//! - Handling cancellation and errors
//! - Implementing incremental rebuilds

#![allow(clippy::too_many_lines)]
#![allow(dead_code)]
#![allow(clippy::use_self)]
#![allow(clippy::inefficient_to_string)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::useless_vec)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::similar_names)]

use async_trait::async_trait;
use eventcore::{
    cqrs::{
        CheckpointStore, CqrsError, CqrsProjection, CqrsResult, InMemoryCheckpointStore,
        InMemoryReadModelStore, Query, ReadModelStore, RebuildCoordinator,
    },
    Event, EventId, EventStore, EventToWrite, ExpectedVersion, Projection, ProjectionCheckpoint,
    ProjectionConfig, ProjectionResult, ProjectionStatus, StreamEvents, StreamId, Timestamp,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::time::interval;
use tracing::{info, warn};

// Domain events for an analytics system
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum AnalyticsEvent {
    PageViewed {
        user_id: String,
        page: String,
        duration_ms: u64,
    },
    UserRegistered {
        user_id: String,
        referrer: Option<String>,
    },
    PurchaseCompleted {
        user_id: String,
        amount: u64,
        items: Vec<String>,
    },
}

// Read model for user analytics
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct UserAnalytics {
    user_id: String,
    total_page_views: u64,
    total_time_spent_ms: u64,
    pages_visited: Vec<String>,
    registration_date: Timestamp,
    referrer: Option<String>,
    total_purchases: u64,
    total_spent: u64,
    items_purchased: Vec<String>,
    last_activity: Timestamp,
}

impl UserAnalytics {
    const fn new(user_id: String, registration_date: Timestamp, referrer: Option<String>) -> Self {
        Self {
            user_id,
            total_page_views: 0,
            total_time_spent_ms: 0,
            pages_visited: Vec::new(),
            registration_date,
            referrer,
            total_purchases: 0,
            total_spent: 0,
            items_purchased: Vec::new(),
            last_activity: registration_date,
        }
    }
}

// Query types for the analytics projection
#[derive(Debug, Clone)]
enum AnalyticsQuery {
    TopUsersByPageViews(usize),
    UsersWithPurchases,
    UsersByReferrer(String),
}

// Implement the Query trait
impl Query for AnalyticsQuery {
    type Model = UserAnalytics;

    fn matches(&self, model: &Self::Model) -> bool {
        match self {
            AnalyticsQuery::TopUsersByPageViews(_) => true, // All models match for sorting
            AnalyticsQuery::UsersWithPurchases => model.total_purchases > 0,
            AnalyticsQuery::UsersByReferrer(referrer) => model.referrer.as_ref() == Some(referrer),
        }
    }

    fn apply_ordering_and_limits(&self, mut models: Vec<Self::Model>) -> Vec<Self::Model> {
        match self {
            AnalyticsQuery::TopUsersByPageViews(limit) => {
                models.sort_by(|a, b| b.total_page_views.cmp(&a.total_page_views));
                models.truncate(*limit);
                models
            }
            _ => models,
        }
    }
}

// Analytics projection state
#[derive(Debug, Clone, Default)]
struct AnalyticsState {
    processed_events: u64,
}

// The CQRS projection implementation
#[derive(Debug)]
struct UserAnalyticsProjection {
    config: ProjectionConfig,
}

impl UserAnalyticsProjection {
    fn new() -> Self {
        Self {
            config: ProjectionConfig::new("user-analytics"),
        }
    }
}

#[async_trait]
impl Projection for UserAnalyticsProjection {
    type State = AnalyticsState;
    type Event = AnalyticsEvent;

    fn config(&self) -> &ProjectionConfig {
        &self.config
    }

    async fn get_state(&self) -> ProjectionResult<Self::State> {
        // This method is not used in the rebuild flow
        Ok(AnalyticsState::default())
    }

    async fn get_status(&self) -> ProjectionResult<ProjectionStatus> {
        Ok(ProjectionStatus::Running)
    }

    async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
        Ok(ProjectionCheckpoint::initial())
    }

    async fn save_checkpoint(&self, _checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
        Ok(())
    }

    async fn initialize_state(&self) -> ProjectionResult<Self::State> {
        Ok(AnalyticsState::default())
    }

    async fn apply_event(
        &self,
        state: &mut Self::State,
        _event: &Event<Self::Event>,
    ) -> ProjectionResult<()> {
        state.processed_events += 1;
        Ok(())
    }

    fn should_process_event(&self, event: &Event<Self::Event>) -> bool {
        // Process all analytics events
        matches!(
            event.payload,
            AnalyticsEvent::PageViewed { .. }
                | AnalyticsEvent::UserRegistered { .. }
                | AnalyticsEvent::PurchaseCompleted { .. }
        )
    }
}

#[async_trait]
impl CqrsProjection for UserAnalyticsProjection {
    type ReadModel = UserAnalytics;
    type Query = AnalyticsQuery;

    fn extract_model_id(&self, event: &Event<Self::Event>) -> Option<String> {
        match &event.payload {
            AnalyticsEvent::PageViewed { user_id, .. }
            | AnalyticsEvent::UserRegistered { user_id, .. }
            | AnalyticsEvent::PurchaseCompleted { user_id, .. } => Some(user_id.clone()),
        }
    }

    async fn apply_to_model(
        &self,
        model: Option<Self::ReadModel>,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<Option<Self::ReadModel>> {
        match &event.payload {
            AnalyticsEvent::UserRegistered { user_id, referrer } => {
                // Create new user analytics
                Ok(Some(UserAnalytics::new(
                    user_id.clone(),
                    event.created_at,
                    referrer.clone(),
                )))
            }
            AnalyticsEvent::PageViewed {
                user_id,
                page,
                duration_ms,
            } => {
                // Update existing user analytics
                if let Some(mut analytics) = model {
                    analytics.total_page_views += 1;
                    analytics.total_time_spent_ms += duration_ms;
                    if !analytics.pages_visited.contains(page) {
                        analytics.pages_visited.push(page.clone());
                    }
                    analytics.last_activity = event.created_at;
                    Ok(Some(analytics))
                } else {
                    warn!("Page view for unregistered user: {}", user_id);
                    Ok(None)
                }
            }
            AnalyticsEvent::PurchaseCompleted {
                user_id,
                amount,
                items,
            } => {
                // Update purchase statistics
                if let Some(mut analytics) = model {
                    analytics.total_purchases += 1;
                    analytics.total_spent += amount;
                    for item in items {
                        if !analytics.items_purchased.contains(item) {
                            analytics.items_purchased.push(item.clone());
                        }
                    }
                    analytics.last_activity = event.created_at;
                    Ok(Some(analytics))
                } else {
                    warn!("Purchase for unregistered user: {}", user_id);
                    Ok(None)
                }
            }
        }
    }
}

// Generate sample events for testing
async fn generate_sample_events(event_store: &dyn EventStore<Event = AnalyticsEvent>) {
    let users = vec!["alice", "bob", "charlie", "diana", "eve"];
    let pages = vec!["/home", "/products", "/about", "/contact", "/blog"];
    let items = vec!["laptop", "mouse", "keyboard", "monitor", "headphones"];
    let referrers = vec![
        Some("google"),
        Some("facebook"),
        None,
        Some("twitter"),
        None,
    ];

    // Register users
    for (i, user) in users.iter().enumerate() {
        let stream_id = StreamId::try_new(format!("user-{user}")).unwrap();
        let event = AnalyticsEvent::UserRegistered {
            user_id: user.to_string(),
            referrer: referrers[i].map(String::from),
        };
        let event_write = EventToWrite::new(EventId::new(), event);
        let stream_write = StreamEvents::new(stream_id, ExpectedVersion::Any, vec![event_write]);
        event_store
            .write_events_multi(vec![stream_write])
            .await
            .unwrap();
    }

    // Generate page views
    for (i, user) in users.iter().enumerate() {
        let stream_id = StreamId::try_new(format!("user-{user}")).unwrap();
        for (j, page) in pages.iter().enumerate() {
            if (i + j) % 2 == 0 {
                // Some variety in who views what
                let event = AnalyticsEvent::PageViewed {
                    user_id: user.to_string(),
                    page: page.to_string(),
                    duration_ms: ((i + 1) * (j + 1) * 1000) as u64,
                };
                let event_write = EventToWrite::new(EventId::new(), event);
                let stream_write =
                    StreamEvents::new(stream_id.clone(), ExpectedVersion::Any, vec![event_write]);
                event_store
                    .write_events_multi(vec![stream_write])
                    .await
                    .unwrap();
            }
        }
    }

    // Generate some purchases
    for (i, user) in users.iter().enumerate() {
        if i % 2 == 0 {
            // Only some users make purchases
            let stream_id = StreamId::try_new(format!("user-{user}")).unwrap();
            let selected_items: Vec<String> = items
                .iter()
                .enumerate()
                .filter(|(j, _)| (i + j) % 3 == 0)
                .map(|(_, item)| item.to_string())
                .collect();

            if !selected_items.is_empty() {
                let event = AnalyticsEvent::PurchaseCompleted {
                    user_id: user.to_string(),
                    amount: (selected_items.len() as u64) * 100,
                    items: selected_items,
                };
                let event_write = EventToWrite::new(EventId::new(), event);
                let stream_write =
                    StreamEvents::new(stream_id.clone(), ExpectedVersion::Any, vec![event_write]);
                event_store
                    .write_events_multi(vec![stream_write])
                    .await
                    .unwrap();
            }
        }
    }

    info!("Generated sample events for {} users", users.len());
}

// Display progress during rebuild
fn display_progress(progress: &eventcore::cqrs::RebuildProgress) {
    print!("\r");
    print!("Progress: {} events", progress.events_processed);

    if progress.events_per_second > 0.0 {
        print!(" @ {:.0} events/sec", progress.events_per_second);
    }

    if let Some(pct) = progress.completion_percentage() {
        print!(" ({:.1}%)", pct);
    }

    if let Some(eta) = progress.estimated_completion {
        let remaining = eta.duration_since(std::time::Instant::now());
        print!(" - ETA: {:?}", remaining);
    }

    print!("          "); // Clear any remaining characters
    use std::io::{self, Write};
    io::stdout().flush().unwrap();
}

// Example: Basic rebuild from beginning
async fn example_rebuild_from_beginning(
    coordinator: Arc<RebuildCoordinator<UserAnalyticsProjection, AnalyticsEvent>>,
) -> CqrsResult<()> {
    println!("\n=== Example 1: Rebuild From Beginning ===");
    println!("Starting complete rebuild...");

    let start = std::time::Instant::now();
    let progress = coordinator.rebuild_from_beginning().await?;

    println!("\nRebuild completed!");
    println!("  Total events: {}", progress.events_processed);
    println!("  Models updated: {}", progress.models_updated);
    println!("  Duration: {:?}", start.elapsed());
    println!(
        "  Average rate: {:.0} events/sec",
        progress.events_per_second
    );

    Ok(())
}

// Example: Incremental rebuild from checkpoint
async fn example_incremental_rebuild(
    coordinator: Arc<RebuildCoordinator<UserAnalyticsProjection, AnalyticsEvent>>,
    checkpoint_store: Arc<dyn CheckpointStore<Error = CqrsError>>,
) -> CqrsResult<()> {
    println!("\n=== Example 2: Incremental Rebuild ===");

    // Load the last checkpoint
    let checkpoint = checkpoint_store
        .load("user-analytics")
        .await?
        .unwrap_or_else(|| {
            println!("No checkpoint found, starting from beginning");
            ProjectionCheckpoint::initial()
        });

    if let Some(event_id) = checkpoint.last_event_id {
        println!("Resuming from checkpoint: {:?}", event_id);
    }

    let progress = coordinator.rebuild_from_checkpoint(checkpoint).await?;

    println!("\nIncremental rebuild completed!");
    println!("  New events processed: {}", progress.events_processed);
    println!("  Models updated: {}", progress.models_updated);

    Ok(())
}

// Example: Rebuild with progress monitoring
async fn example_monitored_rebuild(
    coordinator: Arc<RebuildCoordinator<UserAnalyticsProjection, AnalyticsEvent>>,
) -> CqrsResult<()> {
    println!("\n=== Example 3: Monitored Rebuild ===");
    println!("Starting rebuild with progress monitoring...");

    // Start rebuild in background
    let rebuild_handle = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.rebuild_from_beginning().await })
    };

    // Monitor progress
    let mut ticker = interval(Duration::from_millis(100));
    loop {
        ticker.tick().await;

        let progress = coordinator.get_progress().await;
        display_progress(&progress);

        if !progress.is_running {
            println!(); // New line after progress
            break;
        }
    }

    // Get final result
    let final_progress = rebuild_handle.await.unwrap()?;
    println!("Monitoring complete. Final statistics:");
    println!("  Events: {}", final_progress.events_processed);
    println!("  Models: {}", final_progress.models_updated);

    Ok(())
}

// Example: Rebuild with cancellation
async fn example_cancellable_rebuild(
    coordinator: Arc<RebuildCoordinator<UserAnalyticsProjection, AnalyticsEvent>>,
) -> CqrsResult<()> {
    println!("\n=== Example 4: Cancellable Rebuild ===");
    println!("Starting rebuild (will cancel after 50ms)...");

    // Start rebuild
    let rebuild_handle = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.rebuild_from_beginning().await })
    };

    // Cancel after short delay
    tokio::time::sleep(Duration::from_millis(50)).await;
    println!("Cancelling rebuild...");
    coordinator.cancel();

    // Wait for cancellation
    match rebuild_handle.await.unwrap() {
        Ok(progress) => {
            println!("Rebuild completed before cancellation:");
            println!("  Events processed: {}", progress.events_processed);
        }
        Err(e) => {
            println!("Rebuild cancelled successfully: {}", e);
        }
    }

    Ok(())
}

// Example: Query rebuilt data
async fn example_query_results(
    read_model_store: Arc<
        dyn ReadModelStore<Model = UserAnalytics, Query = AnalyticsQuery, Error = CqrsError>,
    >,
) -> CqrsResult<()> {
    println!("\n=== Example 5: Query Rebuilt Data ===");

    // Get specific user
    if let Some(analytics) = read_model_store.get("alice").await? {
        println!("Alice's analytics:");
        println!("  Page views: {}", analytics.total_page_views);
        println!("  Time spent: {}ms", analytics.total_time_spent_ms);
        println!("  Total purchases: {}", analytics.total_purchases);
        println!("  Total spent: ${}", analytics.total_spent);
    }

    // Query all users (for demo purposes)
    let all_users = read_model_store
        .query(AnalyticsQuery::UsersWithPurchases)
        .await?;
    println!("\nUsers with purchases: {}", all_users.len());

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(true)
        .init();

    // Create event store and generate sample data
    let event_store = Arc::new(InMemoryEventStore::<AnalyticsEvent>::new());
    generate_sample_events(event_store.as_ref()).await;

    // Create projection and storage
    let projection = UserAnalyticsProjection::new();
    let read_model_store = Arc::new(InMemoryReadModelStore::<UserAnalytics, AnalyticsQuery>::new());
    let checkpoint_store = Arc::new(InMemoryCheckpointStore::new());

    // Create rebuild coordinator
    let coordinator = Arc::new(RebuildCoordinator::new(
        projection,
        event_store.clone(),
        read_model_store.clone(),
        checkpoint_store.clone(),
    ));

    // Run examples
    example_rebuild_from_beginning(coordinator.clone()).await?;
    example_incremental_rebuild(coordinator.clone(), checkpoint_store.clone()).await?;
    example_monitored_rebuild(coordinator.clone()).await?;
    example_cancellable_rebuild(coordinator.clone()).await?;
    example_query_results(read_model_store.clone()).await?;

    println!("\n=== All Examples Completed Successfully! ===");

    Ok(())
}
