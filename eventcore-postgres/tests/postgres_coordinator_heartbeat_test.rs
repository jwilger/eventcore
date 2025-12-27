//! Integration test for eventcore-2n5 Scenario 7: PostgresCoordinator implements heartbeat
//!
//! Scenario: PostgresCoordinator provides distributed leadership coordination
//! - Given multiple projector instances connected to same Postgres database
//! - When first instance acquires leadership via PostgresCoordinator
//! - Then coordinator grants leadership and returns valid guard
//! - And subsequent instances cannot acquire leadership while guard is valid

mod common;

use eventcore_postgres::PostgresCoordinator;
use std::time::Duration;

#[tokio::test]
async fn second_instance_cannot_acquire_while_first_holds_leadership() {
    // Given: Two coordinator instances connected to same database
    let fixture = common::PostgresTestFixture::new().await;
    let heartbeat_timeout = Duration::from_millis(300);
    let coordinator1 =
        PostgresCoordinator::new(fixture.connection_string.clone(), heartbeat_timeout)
            .await
            .expect("should create first coordinator");
    let coordinator2 =
        PostgresCoordinator::new(fixture.connection_string.clone(), heartbeat_timeout)
            .await
            .expect("should create second coordinator");

    // When: First instance acquires leadership
    let _guard1 = coordinator1
        .try_acquire()
        .await
        .expect("first instance should acquire leadership");

    // And: Second instance attempts to acquire while first guard is valid
    let guard2 = coordinator2.try_acquire().await;

    // Then: Second instance cannot acquire leadership
    assert!(
        guard2.is_none(),
        "second instance should not acquire leadership while first guard is valid"
    );
}
