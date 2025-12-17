use futures::StreamExt;

/// Error returned when collecting events from a subscription fails.
#[derive(Debug, thiserror::Error)]
pub enum CollectionError {
    /// Timeout expired before collecting the expected number of events.
    #[error("timeout waiting for {expected} events, received {received}")]
    Timeout { expected: usize, received: usize },
}

/// Collect events from a subscription stream with a timeout.
///
/// Returns the collected events or an error if the timeout expires or collection fails.
pub async fn collect_subscription_events<E>(
    subscription: impl futures::Stream<Item = Result<E, eventcore::SubscriptionError>> + Unpin,
    count: usize,
    timeout: std::time::Duration,
) -> Result<Vec<E>, CollectionError>
where
    E: Send + 'static,
{
    let collect_future = subscription
        .take(count)
        .map(|result| result.expect("subscription error during collection"))
        .collect::<Vec<E>>();

    match tokio::time::timeout(timeout, collect_future).await {
        Ok(events) => Ok(events),
        Err(_) => Err(CollectionError::Timeout {
            expected: count,
            received: 0,
        }),
    }
}

/// Partition subscription results into successful events and errors.
pub async fn partition_subscription_results<E>(
    subscription: impl futures::Stream<Item = Result<E, eventcore::SubscriptionError>> + Unpin,
    count: usize,
    timeout: std::time::Duration,
) -> Result<(Vec<E>, Vec<eventcore::SubscriptionError>), CollectionError>
where
    E: Send + 'static,
{
    let collect_future = subscription.take(count).collect::<Vec<_>>();

    match tokio::time::timeout(timeout, collect_future).await {
        Ok(results) => {
            let mut events = Vec::new();
            let mut errors = Vec::new();
            for result in results {
                match result {
                    Ok(event) => events.push(event),
                    Err(err) => errors.push(err),
                }
            }
            Ok((events, errors))
        }
        Err(_) => Err(CollectionError::Timeout {
            expected: count,
            received: 0,
        }),
    }
}
