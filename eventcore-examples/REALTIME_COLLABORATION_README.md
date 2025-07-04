# Real-Time Collaborative Document Editing Example

This example demonstrates how to build a real-time collaborative document editing system using EventCore's multi-stream event sourcing capabilities. It showcases patterns for implementing features like Google Docs or Notion where multiple users can edit the same document simultaneously.

## Overview

The example implements a simplified collaborative text editor with the following features:

- **Multi-user document editing**: Multiple users can edit the same document concurrently
- **Real-time synchronization**: Changes are immediately visible to all connected users
- **Conflict resolution**: Version-based conflict detection ensures consistency
- **User presence**: Track active users, cursor positions, and typing indicators
- **Access control**: Permission-based editing (read, write, admin)
- **Document history**: Full event log enables undo/redo and time travel

## Domain Model

### Core Types

- **Document**: A collaborative text document with versioned content
- **Session**: An active editing session for a user in a document
- **TextOperation**: Represents insert, delete, or format operations
- **UserPresence**: Tracks user activity, cursor position, and selection

### Events

The system uses the following events to capture all state changes:

- `DocumentCreated`: New document initialization
- `TextOperationApplied`: Text insertions, deletions, or formatting
- `UserJoinedDocument`: User starts editing session
- `UserLeftDocument`: User ends editing session
- `CursorPositionUpdated`: Cursor movement and text selection
- `UserStartedTyping`/`UserStoppedTyping`: Typing indicators

## Key Patterns Demonstrated

### 1. Multi-Stream Operations

Each command reads from multiple streams to ensure consistency:

```rust
// JoinDocumentCommand reads from:
// - Session stream (to check if session exists)
// - User stream (to verify user exists)
// - Document stream (to check permissions)
```

### 2. Version-Based Conflict Resolution

Text operations include version numbers to detect concurrent edits:

```rust
pub struct TextOperation {
    pub operation_type: OperationType,
    pub position: CursorPosition,
    pub content: Option<String>,
    pub version: DocumentVersion,  // Ensures operations are applied in order
}
```

### 3. Real-Time State Synchronization

The `DocumentStateProjection` maintains the current state of all documents:

```rust
pub struct DocumentState {
    pub content: String,
    pub version: DocumentVersion,
    pub active_sessions: HashMap<SessionId, SessionInfo>,
    // ... other fields
}
```

### 4. Subscription-Based Updates

The example shows how to use EventCore's subscription system for real-time updates:

```rust
let mut subscription = event_store.subscribe(None)?;

while let Ok(event) = subscription.next_event().await {
    // Process event and broadcast to WebSocket clients
}
```

## Running the Example

```bash
# Run the example
cargo run --example realtime_collaboration

# Run the tests
cargo test -p eventcore-examples realtime_collaboration
```

## Integration with WebSocket/SSE

While this example demonstrates the event sourcing patterns, a production system would integrate with:

- **WebSocket**: For bidirectional real-time communication
- **Server-Sent Events (SSE)**: For server-to-client updates
- **Message Queue**: For broadcasting updates to multiple server instances

Example WebSocket integration pattern:

```rust
// Pseudo-code for WebSocket integration
async fn handle_websocket_connection(ws: WebSocket, executor: Executor) {
    // Subscribe to document events
    let mut subscription = event_store.subscribe(Some(document_id))?;
    
    // Handle incoming operations from client
    while let Some(msg) = ws.recv().await {
        let operation: TextOperation = deserialize(msg)?;
        
        // Execute command
        let command = ApplyTextOperationCommand { /* ... */ };
        executor.execute(&command).await?;
    }
    
    // Send updates to client
    while let Ok(event) = subscription.next_event().await {
        ws.send(serialize(event)?).await?;
    }
}
```

## Advanced Features

### Operational Transformation (OT)

For handling truly concurrent edits without version conflicts, you could extend this example with OT algorithms:

```rust
// Transform operation1 against operation2
fn transform(op1: TextOperation, op2: TextOperation) -> (TextOperation, TextOperation) {
    // Implement OT algorithm
}
```

### Cursor Presence

The example tracks cursor positions but could be extended with:
- Cursor colors for different users
- Selection highlighting
- Remote cursor animations

### Offline Support

EventCore's event log naturally supports offline editing:
1. Queue operations locally when offline
2. Sync with server when reconnected
3. Resolve conflicts using version numbers or OT

## Testing Strategies

The example includes tests for:

1. **Happy path workflows**: Creating documents, joining sessions, editing
2. **Conflict scenarios**: Concurrent edits with version conflicts
3. **Access control**: Permission validation
4. **Edge cases**: Invalid operations, out-of-bounds edits

## Performance Considerations

For production use, consider:

1. **Batching operations**: Group multiple character insertions
2. **Debouncing cursor updates**: Reduce event frequency
3. **Projection snapshots**: Periodically snapshot document state
4. **Stream partitioning**: Distribute documents across multiple streams

## Security Considerations

1. **Authentication**: Verify user identity before creating sessions
2. **Authorization**: Check permissions for each operation
3. **Rate limiting**: Prevent spam and abuse
4. **Input validation**: Sanitize text content and operation bounds

## Conclusion

This example demonstrates how EventCore's multi-stream event sourcing enables building sophisticated collaborative applications. The combination of:

- Strong consistency guarantees
- Real-time event subscriptions
- Type-safe command handling
- Comprehensive event history

Makes it ideal for collaborative editing scenarios where data integrity and real-time updates are critical.

## Further Reading

- [EventCore Documentation](https://github.com/your-org/eventcore)
- [Operational Transformation](https://en.wikipedia.org/wiki/Operational_transformation)
- [CRDTs for Collaboration](https://crdt.tech/)
- [Real-time Collaborative Editing](https://www.inkandswitch.com/local-first.html)