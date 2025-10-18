# Part 2: Getting Started

This comprehensive tutorial walks you through building a complete task management system with EventCore. You'll learn event modeling, domain design, command implementation, projections, and testing.

## What We'll Build

A task management system with:

- Creating and managing tasks
- Assigning tasks to users
- Comments and activity tracking
- Real-time task lists and dashboards
- Complete audit trail

## Chapters in This Part

1. **[Setting Up Your Project](./01-setup.md)** - Create a new Rust project with EventCore
2. **[Modeling the Domain](./02-domain-modeling.md)** - Design events and commands using event modeling
3. **[Implementing Commands](./03-commands.md)** - Build commands with the macro system
4. **[Working with Projections](./04-projections.md)** - Create read models for queries
5. **[Testing Your Application](./05-testing.md)** - Write comprehensive tests

## Prerequisites

- Rust 1.70+ installed
- Basic Rust knowledge (ownership, traits, async)
- PostgreSQL 12+ (or use in-memory store for learning)
- 30-60 minutes to complete

## Learning Outcomes

By the end of this tutorial, you'll understand:

- How to model domains with events
- Using EventCore's macro system
- Building multi-stream commands
- Creating and updating projections
- Testing event-sourced systems

## Code Repository

The complete code for this tutorial is available at:

```bash
git clone https://github.com/your-org/eventcore-task-tutorial
cd eventcore-task-tutorial
```

Ready? Let's [set up your project](./01-setup.md) →
