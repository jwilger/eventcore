# Part 3: Core Concepts

This part provides a deep dive into EventCore's core concepts and design principles. Understanding these concepts will help you build robust, scalable event-sourced systems.

## Chapters in This Part

1. **[Commands and the Macro System](./01-commands-and-macros.md)** - Deep dive into command implementation
2. **[Events and Event Stores](./02-events-and-stores.md)** - Understanding events and storage
3. **[State Reconstruction](./03-state-reconstruction.md)** - How EventCore rebuilds state from events
4. **[Multi-Stream Atomicity](./04-multi-stream-atomicity.md)** - The key innovation of EventCore
5. **[Error Handling](./05-error-handling.md)** - Comprehensive error handling strategies

## What You'll Learn

- How the `#[derive(Command)]` macro works internally
- Event design principles and best practices
- The state reconstruction algorithm
- How multi-stream atomicity is guaranteed
- Error handling patterns for production systems

## Prerequisites

- Completed Part 2: Getting Started
- Basic understanding of Rust macros helpful
- Familiarity with database transactions

## Time to Complete

- Reading: ~30 minutes
- With examples: ~1 hour

Ready to dive deep? Let's start with [Commands and the Macro System](./01-commands-and-macros.md) →