//! Consistent execution context that prevents race conditions
//!
//! This module provides a simple API that captures StreamData at the beginning
//! of command execution and ensures the same data is used for both state
//! reconstruction and version calculation.

use std::collections::HashMap;

use crate::command::Command;
use crate::event_store::{EventToWrite, ExpectedVersion, StreamData, StreamEvents};
use crate::types::{EventId, EventVersion, StreamId};

use super::ExecutionContext;

/// A consistent view of stream data that ensures the same versions are used
/// for both state reconstruction and event writing.
///
/// This struct captures the stream data at a specific point in time and provides
/// methods to determine expected versions based on that captured state, preventing
/// race conditions where different versions might be seen between reads.
#[derive(Debug, Clone)]
pub struct ConsistentStreamView<E> {
    /// The captured stream data
    stream_data: StreamData<E>,
    
    /// Map of stream IDs to their highest version in the captured data
    stream_versions: HashMap<StreamId, EventVersion>,
}

impl<E> ConsistentStreamView<E> 
where
    E: Clone,
{
    /// Create a new consistent view from stream data
    pub fn new(stream_data: StreamData<E>) -> Self {
        // Build a map of stream versions from the data
        let mut stream_versions = HashMap::new();
        
        for event in stream_data.events() {
            stream_versions
                .entry(event.stream_id.clone())
                .and_modify(|v: &mut EventVersion| {
                    if event.event_version > *v {
                        *v = event.event_version;
                    }
                })
                .or_insert(event.event_version);
        }
        
        Self {
            stream_data,
            stream_versions,
        }
    }
    
    /// Get the stream data for state reconstruction
    pub fn stream_data(&self) -> &StreamData<E> {
        &self.stream_data
    }
    
    /// Get the expected version for a stream based on the captured state
    pub fn expected_version(&self, stream_id: &StreamId) -> ExpectedVersion {
        match self.stream_versions.get(stream_id) {
            Some(version) => ExpectedVersion::Exact(*version),
            None => ExpectedVersion::New,
        }
    }
    
    /// Prepare stream events with consistent versioning
    pub fn prepare_stream_events<C>(
        &self,
        events_to_write: Vec<(StreamId, C::Event)>,
        context: &ExecutionContext,
    ) -> Vec<StreamEvents<C::Event>>
    where
        C: Command,
        C::Event: serde::Serialize + Send + Sync,
    {
        // Group events by stream
        let mut events_by_stream: HashMap<StreamId, Vec<EventToWrite<C::Event>>> = HashMap::new();
        
        for (stream_id, event) in events_to_write {
            let event_to_write = EventToWrite::new(EventId::new(), event)
                .with_metadata(context.clone().into());
            
            events_by_stream
                .entry(stream_id)
                .or_default()
                .push(event_to_write);
        }
        
        // Create StreamEvents with versions from our consistent view
        events_by_stream
            .into_iter()
            .map(|(stream_id, events)| {
                let expected_version = self.expected_version(&stream_id);
                StreamEvents::new(stream_id, expected_version, events)
            })
            .collect()
    }
}