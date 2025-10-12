# Security Guide

This guide covers security best practices when building applications with EventCore.

## Overview

EventCore provides a solid foundation for secure applications through:
- Strong type safety that prevents many common vulnerabilities
- Immutable event storage providing natural audit trails
- Built-in concurrency control preventing data races
- Configurable resource limits preventing DoS attacks

However, EventCore is a library, not a complete application framework. Security responsibilities are shared between EventCore and your application code.

## What EventCore Provides

### Type Safety
- Validated domain types using `nutype` prevent injection attacks
- Exhaustive pattern matching eliminates undefined behavior
- Memory safety guaranteed by Rust

### Concurrency Control
- Optimistic locking prevents lost updates
- Version checking ensures consistency
- Atomic multi-stream operations maintain integrity

### Resource Protection
- Configurable timeouts prevent runaway operations
- Batch size limits prevent memory exhaustion
- Retry limits prevent infinite loops

## What You Must Implement

### Authentication & Authorization
EventCore does not provide:
- User authentication
- Stream-level access control
- Command authorization
- Read model security

You must implement these at the application layer.

### Data Protection
EventCore stores events as-is. You must:
- Encrypt sensitive data before storing
- Implement key management
- Handle data retention/deletion
- Ensure compliance with regulations

### Input Validation
While EventCore validates its own types, you must:
- Validate all user input
- Sanitize data before processing
- Implement rate limiting
- Prevent abuse patterns

## Security Layers

```
┌─────────────────────────────────────┐
│         Application Layer           │
│  • Authentication                   │
│  • Authorization                    │
│  • Input Validation                 │
│  • Rate Limiting                    │
├─────────────────────────────────────┤
│         EventCore Layer             │
│  • Type Safety                      │
│  • Concurrency Control              │
│  • Resource Limits                  │
│  • Audit Trail                      │
├─────────────────────────────────────┤
│         Storage Layer               │
│  • Encryption at Rest               │
│  • Access Control                   │
│  • Backup Security                  │
│  • Network Security                 │
└─────────────────────────────────────┘
```

## Next Steps

- [Authentication & Authorization](./02-authentication.md)
- [Data Encryption](./03-encryption.md)
- [Input Validation](./04-validation.md)
- [Compliance](./05-compliance.md)