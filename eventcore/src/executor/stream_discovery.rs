//! Stream discovery iteration logic for dynamic stream resolution.
//! 
//! This module handles the iterative process of discovering additional streams
//! that commands may need during execution, preventing infinite loops while
//! allowing flexible stream resolution.

use crate::types::StreamId;

/// Context for managing stream discovery iteration state.
#[derive(Debug, Clone)]
pub(crate) struct StreamDiscoveryContext {
    /// Current list of stream IDs being processed.
    stream_ids: Vec<StreamId>,
    /// Current iteration count.
    iteration: usize,
    /// Maximum allowed iterations before failing.
    max_iterations: usize,
}

impl StreamDiscoveryContext {
    /// Creates a new stream discovery context.
    pub(crate) fn new(initial_streams: Vec<StreamId>, max_iterations: usize) -> Self {
        Self {
            stream_ids: initial_streams,
            iteration: 0,
            max_iterations,
        }
    }

    /// Adds newly discovered streams and increments iteration count.
    pub(crate) fn add_streams(&mut self, new_streams: Vec<StreamId>) {
        self.stream_ids.extend(new_streams);
        self.iteration += 1;
    }

    /// Increments the iteration counter for the current streams.
    pub(crate) fn next_iteration(&mut self) {
        self.iteration += 1;
    }

    /// Gets the current stream IDs.
    pub(crate) fn stream_ids(&self) -> &[StreamId] {
        &self.stream_ids
    }

    /// Gets the current iteration count.
    pub(crate) fn iteration(&self) -> usize {
        self.iteration
    }

    /// Gets the maximum iterations allowed.
    pub(crate) fn max_iterations(&self) -> usize {
        self.max_iterations
    }

    /// Checks if the iteration limit has been exceeded.
    pub(crate) fn is_limit_exceeded(&self) -> bool {
        self.iteration >= self.max_iterations
    }
}