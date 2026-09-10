use std::any::Any;
use std::fmt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::time::Duration;

use futures::FutureExt;
use tokio::time::timeout;

pub(crate) async fn cleanup_pair<First, Second>(
    bound: Duration,
    first: First,
    second: Second,
) -> (
    Result<First::Output, tokio::time::error::Elapsed>,
    Result<Second::Output, tokio::time::error::Elapsed>,
)
where
    First: Future,
    Second: Future,
{
    tokio::join!(timeout(bound, first), timeout(bound, second))
}

pub(crate) type FixtureFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub(crate) struct FixtureTimeouts {
    initialization: Duration,
    body: Duration,
    cleanup: Duration,
}

impl FixtureTimeouts {
    pub(crate) fn new(bound: Duration) -> Self {
        Self {
            initialization: bound,
            body: bound,
            cleanup: bound,
        }
    }
}

pub(crate) enum FixtureLifecycleError<E> {
    InitializationError(E),
    InitializationPanicked(Box<dyn Any + Send>),
    InitializationTimedOut,
    BodyError(E),
    BodyPanicked(Box<dyn Any + Send>),
    BodyTimedOut,
    CleanupError(E),
    CleanupPanicked(Box<dyn Any + Send>),
    CleanupTimedOut,
}

impl<E: fmt::Debug> fmt::Debug for FixtureLifecycleError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InitializationError(error) => formatter
                .debug_tuple("InitializationError")
                .field(error)
                .finish(),
            Self::InitializationPanicked(payload) => formatter
                .debug_tuple("InitializationPanicked")
                .field(&(**payload).type_id())
                .finish(),
            Self::InitializationTimedOut => formatter.write_str("InitializationTimedOut"),
            Self::BodyError(error) => formatter.debug_tuple("BodyError").field(error).finish(),
            Self::BodyPanicked(payload) => formatter
                .debug_tuple("BodyPanicked")
                .field(&(**payload).type_id())
                .finish(),
            Self::BodyTimedOut => formatter.write_str("BodyTimedOut"),
            Self::CleanupError(error) => {
                formatter.debug_tuple("CleanupError").field(error).finish()
            }
            Self::CleanupPanicked(payload) => formatter
                .debug_tuple("CleanupPanicked")
                .field(&(**payload).type_id())
                .finish(),
            Self::CleanupTimedOut => formatter.write_str("CleanupTimedOut"),
        }
    }
}

/// Runs a test-owned resource through bounded initialization, body, and cleanup stages.
///
/// Cleanup is attempted exactly once after every initialization/body outcome. When cleanup also
/// fails, the first initialization or body failure remains the returned diagnostic.
pub(crate) async fn run_fixture<R, E, Initialize, Body, Cleanup>(
    mut resource: R,
    timeouts: FixtureTimeouts,
    initialize: Initialize,
    body: Body,
    cleanup: Cleanup,
) -> Result<(), FixtureLifecycleError<E>>
where
    Initialize: for<'a> FnOnce(&'a mut R) -> FixtureFuture<'a, Result<(), E>>,
    Body: for<'a> FnOnce(&'a mut R) -> FixtureFuture<'a, Result<(), E>>,
    Cleanup: for<'a> FnOnce(&'a mut R) -> FixtureFuture<'a, Result<(), E>>,
{
    let mut primary = match timeout(
        timeouts.initialization,
        AssertUnwindSafe(initialize(&mut resource)).catch_unwind(),
    )
    .await
    {
        Ok(Ok(Ok(()))) => None,
        Ok(Ok(Err(error))) => Some(FixtureLifecycleError::InitializationError(error)),
        Ok(Err(payload)) => Some(FixtureLifecycleError::InitializationPanicked(payload)),
        Err(_) => Some(FixtureLifecycleError::InitializationTimedOut),
    };

    if primary.is_none() {
        primary = match timeout(
            timeouts.body,
            AssertUnwindSafe(body(&mut resource)).catch_unwind(),
        )
        .await
        {
            Ok(Ok(Ok(()))) => None,
            Ok(Ok(Err(error))) => Some(FixtureLifecycleError::BodyError(error)),
            Ok(Err(payload)) => Some(FixtureLifecycleError::BodyPanicked(payload)),
            Err(_) => Some(FixtureLifecycleError::BodyTimedOut),
        };
    }

    let cleanup_failure = match timeout(
        timeouts.cleanup,
        AssertUnwindSafe(cleanup(&mut resource)).catch_unwind(),
    )
    .await
    {
        Ok(Ok(Ok(()))) => None,
        Ok(Ok(Err(error))) => Some(FixtureLifecycleError::CleanupError(error)),
        Ok(Err(payload)) => Some(FixtureLifecycleError::CleanupPanicked(payload)),
        Err(_) => Some(FixtureLifecycleError::CleanupTimedOut),
    };

    match primary {
        Some(failure) => Err(failure),
        None => cleanup_failure.map_or(Ok(()), Err),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{FixtureFuture, FixtureLifecycleError, FixtureTimeouts, run_fixture};
    use tokio::sync::{Notify, oneshot};
    use tokio::time::timeout;

    #[derive(Debug, PartialEq, Eq)]
    enum TestError {
        Initialization,
        Body,
        Cleanup,
    }

    struct TestResource {
        owned: Vec<&'static str>,
        cleaned: Arc<Mutex<Vec<&'static str>>>,
        cleanup_result: Result<(), TestError>,
    }

    fn resource(owned: Vec<&'static str>, cleaned: Arc<Mutex<Vec<&'static str>>>) -> TestResource {
        TestResource {
            owned,
            cleaned,
            cleanup_result: Ok(()),
        }
    }

    fn timeouts() -> FixtureTimeouts {
        FixtureTimeouts::new(Duration::from_millis(20))
    }

    fn succeeds(_: &mut TestResource) -> FixtureFuture<'_, Result<(), TestError>> {
        Box::pin(async { Ok(()) })
    }

    fn initialization_error(_: &mut TestResource) -> FixtureFuture<'_, Result<(), TestError>> {
        Box::pin(async { Err(TestError::Initialization) })
    }

    fn initialization_panic(_: &mut TestResource) -> FixtureFuture<'_, Result<(), TestError>> {
        Box::pin(async { panic!("initialization panic") })
    }

    fn never_completes(_: &mut TestResource) -> FixtureFuture<'_, Result<(), TestError>> {
        Box::pin(std::future::pending())
    }

    fn body_error(_: &mut TestResource) -> FixtureFuture<'_, Result<(), TestError>> {
        Box::pin(async { Err(TestError::Body) })
    }

    fn body_panic(_: &mut TestResource) -> FixtureFuture<'_, Result<(), TestError>> {
        Box::pin(async { panic!("body panic") })
    }

    fn cleanup(resource: &mut TestResource) -> FixtureFuture<'_, Result<(), TestError>> {
        Box::pin(async move {
            resource
                .cleaned
                .lock()
                .expect("cleanup observation mutex should not be poisoned")
                .extend(resource.owned.iter().copied());
            std::mem::replace(&mut resource.cleanup_result, Ok(()))
        })
    }

    // Break caught: returning directly from initialization error leaks resources planned before
    // the async initializer was entered.
    #[tokio::test]
    async fn initialization_error_after_resource_ownership_still_cleans_up() {
        let cleaned = Arc::new(Mutex::new(Vec::new()));

        // Given a resource owned before initialization starts.
        let result = run_fixture(
            resource(vec!["schema"], Arc::clone(&cleaned)),
            timeouts(),
            initialization_error,
            succeeds,
            cleanup,
        )
        .await;

        // Then the initialization error remains primary and cleanup still observes the resource.
        assert!(matches!(
            result,
            Err(FixtureLifecycleError::InitializationError(
                TestError::Initialization
            ))
        ));
        assert_eq!(
            *cleaned.lock().expect("mutex should not be poisoned"),
            ["schema"]
        );
    }

    // Break caught: unwinding initialization before cleanup leaks a schema already named by the
    // fixture owner.
    #[tokio::test]
    async fn initialization_panic_after_resource_ownership_still_cleans_up() {
        let cleaned = Arc::new(Mutex::new(Vec::new()));

        let result = run_fixture(
            resource(vec!["schema"], Arc::clone(&cleaned)),
            timeouts(),
            initialization_panic,
            succeeds,
            cleanup,
        )
        .await;

        assert!(matches!(
            result,
            Err(FixtureLifecycleError::InitializationPanicked(_))
        ));
        assert_eq!(
            *cleaned.lock().expect("mutex should not be poisoned"),
            ["schema"]
        );
    }

    // Break caught: cancelling a timed-out initializer without subsequently cleaning its owner
    // leaks resources created before the await that stalled.
    #[tokio::test]
    async fn initialization_timeout_after_resource_ownership_still_cleans_up() {
        let cleaned = Arc::new(Mutex::new(Vec::new()));

        let result = run_fixture(
            resource(vec!["schema"], Arc::clone(&cleaned)),
            timeouts(),
            never_completes,
            succeeds,
            cleanup,
        )
        .await;

        assert!(matches!(
            result,
            Err(FixtureLifecycleError::InitializationTimedOut)
        ));
        assert_eq!(
            *cleaned.lock().expect("mutex should not be poisoned"),
            ["schema"]
        );
    }

    // Break caught: unwinding or cancelling the contract body before cleanup leaks the fixture.
    #[tokio::test]
    async fn body_panic_and_timeout_each_clean_up() {
        let panic_cleanup = Arc::new(Mutex::new(Vec::new()));
        let timeout_cleanup = Arc::new(Mutex::new(Vec::new()));

        let panic_result = run_fixture(
            resource(vec!["panic-schema"], Arc::clone(&panic_cleanup)),
            timeouts(),
            succeeds,
            body_panic,
            cleanup,
        )
        .await;
        let timeout_result = run_fixture(
            resource(vec!["timeout-schema"], Arc::clone(&timeout_cleanup)),
            timeouts(),
            succeeds,
            never_completes,
            cleanup,
        )
        .await;

        assert!(matches!(
            panic_result,
            Err(FixtureLifecycleError::BodyPanicked(_))
        ));
        assert!(matches!(
            timeout_result,
            Err(FixtureLifecycleError::BodyTimedOut)
        ));
        assert_eq!(
            *panic_cleanup.lock().expect("mutex should not be poisoned"),
            ["panic-schema"]
        );
        assert_eq!(
            *timeout_cleanup
                .lock()
                .expect("mutex should not be poisoned"),
            ["timeout-schema"]
        );
    }

    // Break caught: reporting cleanup failure instead of the body failure hides the first cause
    // and makes the contract diagnosis misleading.
    #[tokio::test]
    async fn cleanup_failure_does_not_replace_primary_failure() {
        let cleaned = Arc::new(Mutex::new(Vec::new()));
        let mut owner = resource(vec!["schema"], Arc::clone(&cleaned));
        owner.cleanup_result = Err(TestError::Cleanup);

        let result = run_fixture(owner, timeouts(), succeeds, body_error, cleanup).await;

        assert!(matches!(
            result,
            Err(FixtureLifecycleError::BodyError(TestError::Body))
        ));
        assert_eq!(
            *cleaned.lock().expect("mutex should not be poisoned"),
            ["schema"]
        );
    }

    // Break caught: cleanup that stops after its first resource leaves the second schema owned by
    // a partially initialized split fixture.
    #[tokio::test]
    async fn two_owned_resources_are_both_cleaned_after_partial_setup() {
        let cleaned = Arc::new(Mutex::new(Vec::new()));

        let result = run_fixture(
            resource(
                vec!["event-schema", "projection-schema"],
                Arc::clone(&cleaned),
            ),
            timeouts(),
            initialization_error,
            succeeds,
            cleanup,
        )
        .await;

        assert!(matches!(
            result,
            Err(FixtureLifecycleError::InitializationError(
                TestError::Initialization
            ))
        ));
        assert_eq!(
            *cleaned.lock().expect("mutex should not be poisoned"),
            ["event-schema", "projection-schema"]
        );
    }

    // Break caught: awaiting the first resource before starting the second means a stalled first
    // cleanup prevents the second resource from receiving any cleanup attempt.
    #[tokio::test]
    async fn pending_first_cleanup_does_not_prevent_observable_second_cleanup() {
        let first_started = Arc::new(Notify::new());
        let first_release = Arc::new(Notify::new());
        let (second_cleaned_sender, second_cleaned_receiver) = oneshot::channel();
        let first_started_by_cleanup = Arc::clone(&first_started);
        let first_release_by_test = Arc::clone(&first_release);
        let first = async move {
            first_started_by_cleanup.notify_one();
            first_release.notified().await;
        };
        let second = async move {
            let _ = second_cleaned_sender.send(());
        };
        let observe_coordination = async move {
            first_started.notified().await;
            let second_observation =
                timeout(Duration::from_millis(50), second_cleaned_receiver).await;
            first_release_by_test.notify_one();
            second_observation
        };

        let ((first_result, second_result), second_observation) = tokio::join!(
            super::cleanup_pair(Duration::from_millis(200), first, second),
            observe_coordination,
        );

        assert!(first_result.is_ok(), "first cleanup should be released");
        assert!(second_result.is_ok(), "second cleanup should complete");
        second_observation
            .expect("second cleanup must start while the first remains pending")
            .expect("second cleanup observation channel should remain open");
    }
}
