//! Integration test for eventcore-2n5 Scenario 7: PostgresCoordinator implements heartbeat
//!
//! Scenario: PostgresCoordinator implements heartbeat
//! - Given PostgresCoordinator with heartbeat configuration
//! - When guard.heartbeat() is called
//! - Then coordinator updates last-heartbeat timestamp in database
//! - When guard.is_valid() is called
//! - Then coordinator checks if heartbeat is within timeout window

mod common;

use common::PostgresTestFixture;
use eventcore_postgres::PostgresCoordinator;
use std::time::Duration;

#[tokio::test]
async fn coordinator_invalidates_guard_after_heartbeat_timeout() {
    // Given: PostgresCoordinator with heartbeat configuration
    let fixture = PostgresTestFixture::new().await;
    let heartbeat_timeout = Duration::from_millis(100);

    let coordinator =
        PostgresCoordinator::new(fixture.connection_string.clone(), heartbeat_timeout)
            .expect("should create coordinator");

    // When: Coordinator acquires guard
    let guard = coordinator
        .try_acquire()
        .await
        .expect("should acquire leadership");

    // Then: Guard starts valid
    assert!(guard.is_valid().await, "guard should be valid initially");

    // When: Guard sends heartbeat
    guard.heartbeat().await;

    // Then: Guard remains valid after heartbeat
    assert!(
        guard.is_valid().await,
        "guard should be valid after heartbeat"
    );

    // When: Timeout period elapses without heartbeat
    tokio::time::sleep(heartbeat_timeout + Duration::from_millis(50)).await;

    // Then: Guard becomes invalid after timeout
    assert!(
        !guard.is_valid().await,
        "guard should be invalid after heartbeat timeout expires"
    );
}
