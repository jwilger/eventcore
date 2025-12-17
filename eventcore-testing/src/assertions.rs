use futures::StreamExt;

/// Error returned when collecting events from a subscription fails.
#[derive(Debug, thiserror::Error)]
pub enum CollectionError {
    /// Timeout expired before collecting the expected number of events.
    #[error("timeout waiting for {expected} events, received {received}")]
    Timeout { expected: usize, received: usize },

    /// A subscription error occurred during collection.
    #[error("subscription error during collection: {0}")]
    SubscriptionError(#[from] eventcore::SubscriptionError),
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
    let mut events = Vec::with_capacity(count);
    let mut stream = subscription.take(count);

    let collect_future = async {
        while let Some(result) = stream.next().await {
            events.push(result?);
        }
        Ok::<_, CollectionError>(())
    };

    match tokio::time::timeout(timeout, collect_future).await {
        Ok(Ok(())) => Ok(events),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(CollectionError::Timeout {
            expected: count,
            received: events.len(),
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
    let mut events = Vec::new();
    let mut errors = Vec::new();
    let mut stream = subscription.take(count);

    let collect_future = async {
        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => events.push(event),
                Err(err) => errors.push(err),
            }
        }
    };

    match tokio::time::timeout(timeout, collect_future).await {
        Ok(()) => Ok((events, errors)),
        Err(_) => Err(CollectionError::Timeout {
            expected: count,
            received: events.len() + errors.len(),
        }),
    }
}
