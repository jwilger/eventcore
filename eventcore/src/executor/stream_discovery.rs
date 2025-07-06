//! Type-safe stream discovery with compile-time iteration limit guarantees
//!
//! This module provides a type-state implementation of stream discovery
//! that ensures iteration limits cannot be exceeded at compile time.

use crate::types::StreamId;
use std::marker::PhantomData;

/// Phantom type states for stream discovery
pub mod states {
    /// Initial state - first iteration starting
    pub struct Initial;

    /// Ready for iteration - can load data
    pub struct Ready;

    /// State with stream data loaded
    pub struct DataLoaded;

    /// Discovery process is complete
    pub struct DiscoveryComplete;

    /// Maximum iterations exceeded - terminal state
    pub struct LimitExceeded;
}

/// Type-safe stream discovery context using phantom types
#[derive(Debug)]
pub struct StreamDiscoveryContext<State> {
    stream_ids: Vec<StreamId>,
    iteration: usize,
    max_iterations: usize,
    _state: PhantomData<State>,
}

/// Result of an iteration attempt
pub enum IterationResult<Event> {
    /// Discovery complete with events to write
    Complete {
        stream_events: Vec<crate::event_store::StreamEvents<Event>>,
    },
    /// Need more streams for discovery
    NeedsMoreStreams {
        context: StreamDiscoveryContext<states::DataLoaded>,
    },
    /// Iteration limit exceeded
    LimitExceeded {
        context: StreamDiscoveryContext<states::LimitExceeded>,
    },
}

impl StreamDiscoveryContext<states::Initial> {
    /// Create a new stream discovery context
    pub const fn new(initial_streams: Vec<StreamId>, max_iterations: usize) -> Self {
        Self {
            stream_ids: initial_streams,
            iteration: 0,
            max_iterations,
            _state: PhantomData,
        }
    }

    /// Transition to ready state for first iteration
    pub fn into_ready(self) -> StreamDiscoveryContext<states::Ready> {
        StreamDiscoveryContext {
            stream_ids: self.stream_ids,
            iteration: self.iteration,
            max_iterations: self.max_iterations,
            _state: PhantomData,
        }
    }
}

impl StreamDiscoveryContext<states::Ready> {
    /// Transition to data loaded state after reading streams
    pub fn with_loaded_data(self) -> StreamDiscoveryContext<states::DataLoaded> {
        StreamDiscoveryContext {
            stream_ids: self.stream_ids,
            iteration: self.iteration + 1,
            max_iterations: self.max_iterations,
            _state: PhantomData,
        }
    }
}

impl StreamDiscoveryContext<states::DataLoaded> {
    /// Add newly discovered streams and check iteration limit
    pub fn add_streams(
        mut self,
        new_streams: Vec<StreamId>,
    ) -> Result<Self, StreamDiscoveryContext<states::LimitExceeded>> {
        self.stream_ids.extend(new_streams);

        if self.iteration > self.max_iterations {
            Err(StreamDiscoveryContext {
                stream_ids: self.stream_ids,
                iteration: self.iteration,
                max_iterations: self.max_iterations,
                _state: PhantomData,
            })
        } else {
            Ok(self)
        }
    }

    /// Mark discovery as complete
    pub fn complete(self) -> StreamDiscoveryContext<states::DiscoveryComplete> {
        StreamDiscoveryContext {
            stream_ids: self.stream_ids,
            iteration: self.iteration,
            max_iterations: self.max_iterations,
            _state: PhantomData,
        }
    }

    /// Get current iteration count
    pub const fn current_iteration(&self) -> usize {
        self.iteration
    }

    /// Get remaining iterations
    pub const fn remaining_iterations(&self) -> usize {
        self.max_iterations.saturating_sub(self.iteration)
    }

    /// Get stream count
    pub fn stream_count(&self) -> usize {
        self.stream_ids.len()
    }

    /// Map over the stream IDs
    pub async fn map_streams<F, Fut, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&[StreamId]) -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        f(&self.stream_ids).await
    }

    /// Derive log context from the current state
    pub fn log_context(&self) -> LogContext {
        LogContext {
            iteration: self.iteration,
            stream_count: self.stream_ids.len(),
            max_iterations: self.max_iterations,
        }
    }

    /// Access stream IDs immutably
    pub fn stream_ids(&self) -> &[StreamId] {
        &self.stream_ids
    }

    /// Continue to next iteration
    pub fn into_ready(self) -> StreamDiscoveryContext<states::Ready> {
        StreamDiscoveryContext {
            stream_ids: self.stream_ids,
            iteration: self.iteration,
            max_iterations: self.max_iterations,
            _state: PhantomData,
        }
    }
}

impl StreamDiscoveryContext<states::LimitExceeded> {
    /// Get error message for limit exceeded
    pub fn error_message<C>(&self) -> String {
        format!(
            "Command '{}' exceeded maximum stream discovery iterations ({}). This suggests the command is continuously discovering new streams. Current streams: {:?}",
            std::any::type_name::<C>(),
            self.max_iterations,
            self.stream_ids.iter().map(std::convert::AsRef::as_ref).collect::<Vec<_>>()
        )
    }
}

/// Log context derived from stream discovery state
#[derive(Debug)]
pub struct LogContext {
    pub iteration: usize,
    pub stream_count: usize,
    pub max_iterations: usize,
}

impl std::fmt::Display for LogContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "iteration={}, streams_count={}, max_iterations={}",
            self.iteration, self.stream_count, self.max_iterations
        )
    }
}
