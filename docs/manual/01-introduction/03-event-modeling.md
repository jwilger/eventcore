# Chapter 1.3: Event Modeling Fundamentals

Event modeling is a visual technique for designing event-driven systems. It helps you discover your domain events, commands, and read models before writing any code. This chapter teaches you how to model systems that naturally translate to EventCore implementations.

## What is Event Modeling?

Event modeling is a method of describing systems using three core elements:

1. **Events** (Orange) - Things that happened
2. **Commands** (Blue) - Things users want to do
3. **Read Models** (Green) - Views of current state

The genius is in its simplicity: model your system on a timeline showing what happens when.

## The Event Modeling Process

### Step 1: Brain Storming Events

Start by identifying what happens in your system. Use past-tense language:

```
Example: Task Management System

Events (what happened):
- Task Created
- Task Assigned
- Task Completed
- Comment Added
- Due Date Changed
- Task Archived
```

**Key principles:**

- Past tense ("Created" not "Create")
- Record facts ("Task Completed" not "Complete Task")
- Include relevant data in event names

### Step 2: Building the Timeline

Arrange events on a timeline to tell the story of your system:

```
Time →
|
├─ Task Created ──┬─ Task Assigned ──┬─ Comment Added ──┬─ Task Completed
|   (by: Alice)   |   (to: Bob)      |   (by: Bob)      |   (by: Bob)
|   title: "Fix"  |                  |   "Working on it" |
|                 |                  |                   |
└─────────────────┴──────────────────┴───────────────────┴─────────────────
```

This visual representation helps you:

- See the flow of your system
- Identify missing events
- Understand event relationships

### Step 3: Identifying Commands

Commands trigger events. Look at each event and ask "What user action caused this?"

```
Command (Blue)           →  Event (Orange)
─────────────────────────────────────────
Create Task              →  Task Created
Assign Task              →  Task Assigned
Complete Task            →  Task Completed
Add Comment              →  Comment Added
```

In EventCore, these become your command types:

```rust
#[derive(Command, Clone)]
struct CreateTask {
    #[stream]
    task_id: StreamId,
    title: TaskTitle,
    description: TaskDescription,
}

#[derive(Command, Clone)]
struct AssignTask {
    #[stream]
    task_id: StreamId,
    #[stream]
    user_id: StreamId,
}
```

### Step 4: Designing Read Models

Read models answer questions. Look at your UI/API needs:

```
Question                     →  Read Model (Green)
──────────────────────────────────────────────
"What tasks do I have?"      →  My Tasks List
"What's the project status?" →  Project Dashboard
"Who worked on what?"        →  Activity Timeline
```

In EventCore, these become projections:

```rust
// Read model for "My Tasks"
struct MyTasksProjection {
    tasks_by_user: HashMap<UserId, Vec<TaskSummary>>,
}

impl CqrsProjection for MyTasksProjection {
    fn apply(&mut self, event: &StoredEvent<TaskEvent>) {
        match &event.payload {
            TaskEvent::TaskAssigned { user_id, .. } => {
                // Update tasks_by_user
            }
            // ... handle other events
        }
    }
}
```

## Event Modeling Patterns

### Pattern 1: State Transitions

Many business processes are state machines:

```
Draft → Published → Archived
  ↓         ↓
Deleted  Unpublished

Events:
- ArticleDrafted
- ArticlePublished
- ArticleUnpublished
- ArticleArchived
- ArticleDeleted
```

In EventCore:

```rust
#[derive(Command, Clone)]
struct PublishArticle {
    #[stream]
    article_id: StreamId,
    #[stream]
    author_id: StreamId,    // Also track author actions
    scheduled_time: Option<Timestamp>,
}
```

### Pattern 2: Collaborative Operations

When multiple entities participate:

```
Money Transfer Timeline:

Source Account ──────┬──────────────┬─────────
                     ↓              ↑
                Money Withdrawn     │
                                    │
Target Account ──────────────┬──────┴─────────
                             ↓
                        Money Deposited
```

In EventCore, this is ONE atomic command:

```rust
#[derive(Command, Clone)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,
    #[stream]
    to_account: StreamId,
    amount: Money,
}
```

### Pattern 3: Process Flows

Complex business processes with multiple steps:

```
Order Flow:
Order Created → Payment Processed → Inventory Reserved → Order Shipped
      |                |                    |                  |
   Order Stream   Payment Stream    Inventory Stream    Shipping Stream
```

Each step might be a separate command or one complex command:

```rust
#[derive(Command, Clone)]
struct FulfillOrder {
    #[stream]
    order_id: StreamId,
    #[stream]
    payment_id: StreamId,
    #[stream]
    inventory_id: StreamId,
    #[stream]
    shipping_id: StreamId,
}
```

## From Model to Implementation

### 1. Events Become Rust Enums

Your discovered events:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
enum TaskEvent {
    Created { title: String, description: String },
    Assigned { user_id: UserId },
    Completed { completed_at: Timestamp },
    CommentAdded { author: UserId, text: String },
}
```

### 2. Commands Become EventCore Commands

Your identified commands:

```rust
#[derive(Command, Clone)]
struct CreateTask {
    #[stream]
    task_id: StreamId,
    title: TaskTitle,
}

impl CommandLogic for CreateTask {
    type Event = TaskEvent;
    type State = TaskState;

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        require!(!state.exists, "Task already exists");

        Ok(NewEvents::from(vec![TaskEvent::Created {
            title: self.title.as_ref().to_string(),
            description: String::new(),
        }]))
    }
}
```

### 3. Read Models Become Projections

Your view requirements:

```rust
#[derive(Default)]
struct TasksByUserProjection {
    index: HashMap<UserId, HashSet<TaskId>>,
}

impl CqrsProjection for TasksByUserProjection {
    fn apply(&mut self, event: &StoredEvent<TaskEvent>) {
        match &event.payload {
            TaskEvent::Assigned { user_id } => {
                self.index
                    .entry(user_id.clone())
                    .or_default()
                    .insert(TaskId::from(&event.stream_id));
            }
            _ => {}
        }
    }
}
```

## Workshop: Model a Coffee Shop

Let's practice with a simple domain:

### Step 1: Brainstorm Events

What happens in a coffee shop?

- Customer Entered
- Order Placed
- Payment Received
- Coffee Prepared
- Order Completed
- Customer Left

### Step 2: Build Timeline

```
Customer Entered → Order Placed → Payment Received → Coffee Prepared → Order Completed
     |                 |               |                  |                |
  Customer ID      Order Stream   Payment Stream    Barista Stream    Order Stream
```

### Step 3: Identify Commands

- Enter Shop → Customer Entered
- Place Order → Order Placed
- Process Payment → Payment Received
- Prepare Coffee → Coffee Prepared
- Complete Order → Order Completed

### Step 4: Design Read Models

- Queue Display: Shows pending orders for baristas
- Customer Receipt: Shows order details and status
- Daily Sales Report: Aggregates all payments

### Step 5: Implement in EventCore

```rust
// One command handling the full order flow
#[derive(Command, Clone)]
struct PlaceAndPayOrder {
    #[stream]
    order_id: StreamId,
    #[stream]
    customer_id: StreamId,
    #[stream]
    register_id: StreamId,
    items: Vec<MenuItem>,
    payment: PaymentMethod,
}
```

## Best Practices

1. **Start with Events, Not Structure**
   - Don't design database schemas
   - Focus on what happens in the business

2. **Use Domain Language**
   - "InvoiceSent" not "UpdateInvoiceStatus"
   - Match the language your users use

3. **Model Time Explicitly**
   - Show the flow of events
   - Understand concurrent vs sequential operations

4. **Keep Events Focused**
   - One event = one business fact
   - Don't combine unrelated changes

5. **Commands Match User Intent**
   - "TransferMoney" not "UpdateAccountBalance"
   - Commands are what users want to do

## Common Pitfalls

❌ **Modeling State Instead of Events**

```rust
// Bad: Thinking in state
AccountUpdated { balance: 100 }

// Good: Thinking in events
MoneyDeposited { amount: 50 }
```

❌ **Technical Events**

```rust
// Bad: Technical focus
DatabaseRecordInserted

// Good: Business focus
CustomerRegistered
```

❌ **Missing the Why**

```rust
// Bad: Just the what
PriceChanged { new_price: 100 }

// Good: Including why
PriceReducedForSale { original: 150, sale_price: 100, reason: "Black Friday" }
```

## Summary

Event modeling helps you:

1. Understand your domain before coding
2. Discover events, commands, and read models
3. Design systems that map naturally to EventCore
4. Communicate with stakeholders visually

The key insight: **Model what happens, not what is.**

Next, let's look at [EventCore's Architecture](./04-architecture.md) to understand how your models become working systems →
