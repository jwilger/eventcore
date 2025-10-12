# Chapter 5.3: Long-Running Processes

Long-running processes, also known as sagas or process managers, coordinate complex business workflows that span multiple commands and may take significant time to complete. EventCore provides patterns for implementing these reliably.

## What Are Long-Running Processes?

Long-running processes are stateful workflows that:
- React to events
- Execute commands
- Maintain state across time
- Handle failures and compensations
- May run for days, weeks, or months

Examples include:
- Order fulfillment workflows
- User onboarding sequences
- Financial transaction processing
- Document approval chains

## Process Manager Pattern

EventCore implements the process manager pattern:

```rust
use eventcore::process::{ProcessManager, ProcessState, ProcessResult};

#[derive(Command, Clone)]
struct OrderFulfillmentProcess {
    #[stream]
    process_id: StreamId,
    
    #[stream]
    order_id: StreamId,
    
    current_step: FulfillmentStep,
    timeout_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
enum FulfillmentStep {
    PaymentPending,
    PaymentConfirmed,
    InventoryReserved,
    Shipped,
    Delivered,
    Completed,
    Failed(String),
}

#[derive(Default)]
struct OrderFulfillmentState {
    order_id: Option<OrderId>,
    current_step: FulfillmentStep,
    payment_confirmed: bool,
    inventory_reserved: bool,
    shipping_info: Option<ShippingInfo>,
    timeout_at: Option<DateTime<Utc>>,
    retry_count: u32,
    created_at: DateTime<Utc>,
}

impl CommandLogic for OrderFulfillmentProcess {
    type State = OrderFulfillmentState;
    type Event = ProcessEvent;
    
    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            ProcessEvent::Started { order_id, timeout_at } => {
                state.order_id = Some(*order_id);
                state.current_step = FulfillmentStep::PaymentPending;
                state.timeout_at = *timeout_at;
                state.created_at = event.occurred_at;
            }
            ProcessEvent::StepCompleted { step } => {
                state.current_step = step.clone();
            }
            ProcessEvent::PaymentConfirmed => {
                state.payment_confirmed = true;
            }
            ProcessEvent::InventoryReserved => {
                state.inventory_reserved = true;
            }
            ProcessEvent::ShippingInfoUpdated { info } => {
                state.shipping_info = Some(info.clone());
            }
            ProcessEvent::Failed { reason } => {
                state.current_step = FulfillmentStep::Failed(reason.clone());
            }
            ProcessEvent::RetryAttempted => {
                state.retry_count += 1;
            }
        }
    }
    
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check for timeout
        if let Some(timeout) = state.timeout_at {
            if Utc::now() > timeout {
                return Ok(vec![
                    StreamWrite::new(
                        &read_streams,
                        self.process_id.clone(),
                        ProcessEvent::Failed {
                            reason: "Process timed out".to_string(),
                        }
                    )?
                ]);
            }
        }
        
        // Execute current step
        match state.current_step {
            FulfillmentStep::PaymentPending => {
                self.handle_payment_step(&read_streams, &state).await
            }
            FulfillmentStep::PaymentConfirmed => {
                self.handle_inventory_step(&read_streams, &state).await
            }
            FulfillmentStep::InventoryReserved => {
                self.handle_shipping_step(&read_streams, &state).await
            }
            FulfillmentStep::Shipped => {
                self.handle_delivery_step(&read_streams, &state).await
            }
            FulfillmentStep::Delivered => {
                self.handle_completion_step(&read_streams, &state).await
            }
            FulfillmentStep::Completed | FulfillmentStep::Failed(_) => {
                // Process finished - no more events
                Ok(vec![])
            }
        }
    }
}

impl OrderFulfillmentProcess {
    async fn handle_payment_step(
        &self,
        read_streams: &ReadStreams<OrderFulfillmentProcessStreamSet>,
        state: &OrderFulfillmentState,
    ) -> CommandResult<Vec<StreamWrite<OrderFulfillmentProcessStreamSet, ProcessEvent>>> {
        if !state.payment_confirmed {
            // Check if payment was confirmed by external event
            // This would typically listen to payment events
            Ok(vec![])
        } else {
            // Move to next step
            Ok(vec![
                StreamWrite::new(
                    read_streams,
                    self.process_id.clone(),
                    ProcessEvent::StepCompleted {
                        step: FulfillmentStep::PaymentConfirmed,
                    }
                )?
            ])
        }
    }
    
    async fn handle_inventory_step(
        &self,
        read_streams: &ReadStreams<OrderFulfillmentProcessStreamSet>,
        state: &OrderFulfillmentState,
    ) -> CommandResult<Vec<StreamWrite<OrderFulfillmentProcessStreamSet, ProcessEvent>>> {
        if !state.inventory_reserved {
            // Reserve inventory
            Ok(vec![
                StreamWrite::new(
                    read_streams,
                    self.process_id.clone(),
                    ProcessEvent::InventoryReserved,
                )?
            ])
        } else {
            // Move to shipping
            Ok(vec![
                StreamWrite::new(
                    read_streams,
                    self.process_id.clone(),
                    ProcessEvent::StepCompleted {
                        step: FulfillmentStep::InventoryReserved,
                    }
                )?
            ])
        }
    }
    
    // Similar implementations for other steps...
}
```

## Event-Driven Process Coordination

Processes react to events from other parts of the system:

```rust
#[async_trait]
impl EventHandler<SystemEvent> for OrderFulfillmentProcess {
    async fn handle_event(
        &self,
        event: &StoredEvent<SystemEvent>,
        executor: &CommandExecutor,
    ) -> Result<(), ProcessError> {
        match &event.payload {
            SystemEvent::Payment(PaymentEvent::Confirmed { order_id, .. }) => {
                // Payment confirmed - advance process
                let process_command = AdvanceOrderProcess {
                    process_id: derive_process_id(order_id),
                    trigger: ProcessTrigger::PaymentConfirmed,
                };
                executor.execute(&process_command).await?;
            }
            SystemEvent::Inventory(InventoryEvent::Reserved { order_id, .. }) => {
                let process_command = AdvanceOrderProcess {
                    process_id: derive_process_id(order_id),
                    trigger: ProcessTrigger::InventoryReserved,
                };
                executor.execute(&process_command).await?;
            }
            SystemEvent::Shipping(ShippingEvent::Dispatched { order_id, tracking, .. }) => {
                let process_command = AdvanceOrderProcess {
                    process_id: derive_process_id(order_id),
                    trigger: ProcessTrigger::Shipped { tracking_number: tracking.clone() },
                };
                executor.execute(&process_command).await?;
            }
            _ => {} // Ignore other events
        }
        Ok(())
    }
}

#[derive(Command, Clone)]
struct AdvanceOrderProcess {
    #[stream]
    process_id: StreamId,
    
    trigger: ProcessTrigger,
}

#[derive(Debug, Clone)]
enum ProcessTrigger {
    PaymentConfirmed,
    InventoryReserved,
    Shipped { tracking_number: String },
    Delivered,
    Failed { reason: String },
}
```

## Saga Pattern Implementation

For distributed transactions, implement the saga pattern:

```rust
#[derive(Command, Clone)]
struct Bookingsaga {
    #[stream]
    saga_id: StreamId,
    
    #[stream]
    reservation_id: StreamId,
    
    steps: Vec<SagaStep>,
    current_step: usize,
    compensation_mode: bool,
}

#[derive(Debug, Clone)]
struct SagaStep {
    name: String,
    command: Box<dyn SerializableCommand>,
    compensation: Box<dyn SerializableCommand>,
    status: StepStatus,
}

#[derive(Debug, Clone, PartialEq)]
enum StepStatus {
    Pending,
    Completed,
    Failed,
    Compensated,
}

impl CommandLogic for BookingSaga {
    type State = SagaState;
    type Event = SagaEvent;
    
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if state.compensation_mode {
            self.handle_compensation(&read_streams, &state).await
        } else {
            self.handle_forward_execution(&read_streams, &state).await
        }
    }
}

impl BookingSaga {
    async fn handle_forward_execution(
        &self,
        read_streams: &ReadStreams<BookingSagaStreamSet>,
        state: &SagaState,
    ) -> CommandResult<Vec<StreamWrite<BookingSagaStreamSet, SagaEvent>>> {
        if state.current_step >= state.steps.len() {
            // All steps completed
            return Ok(vec![
                StreamWrite::new(
                    read_streams,
                    self.saga_id.clone(),
                    SagaEvent::Completed,
                )?
            ]);
        }
        
        let current_step = &state.steps[state.current_step];
        
        match current_step.status {
            StepStatus::Pending => {
                // Execute current step
                Ok(vec![
                    StreamWrite::new(
                        read_streams,
                        self.saga_id.clone(),
                        SagaEvent::StepStarted {
                            step_index: state.current_step,
                            step_name: current_step.name.clone(),
                        }
                    )?
                ])
            }
            StepStatus::Completed => {
                // Move to next step
                Ok(vec![
                    StreamWrite::new(
                        read_streams,
                        self.saga_id.clone(),
                        SagaEvent::StepAdvanced {
                            next_step: state.current_step + 1,
                        }
                    )?
                ])
            }
            StepStatus::Failed => {
                // Start compensation
                Ok(vec![
                    StreamWrite::new(
                        read_streams,
                        self.saga_id.clone(),
                        SagaEvent::CompensationStarted {
                            failed_step: state.current_step,
                        }
                    )?
                ])
            }
            StepStatus::Compensated => unreachable!("Cannot be compensated in forward mode"),
        }
    }
    
    async fn handle_compensation(
        &self,
        read_streams: &ReadStreams<BookingSagaStreamSet>,
        state: &SagaState,
    ) -> CommandResult<Vec<StreamWrite<BookingSagaStreamSet, SagaEvent>>> {
        // Compensate completed steps in reverse order
        let compensation_step = state.steps
            .iter()
            .rposition(|step| step.status == StepStatus::Completed);
        
        match compensation_step {
            Some(index) => {
                Ok(vec![
                    StreamWrite::new(
                        read_streams,
                        self.saga_id.clone(),
                        SagaEvent::CompensationStepStarted {
                            step_index: index,
                            step_name: state.steps[index].name.clone(),
                        }
                    )?
                ])
            }
            None => {
                // All compensations completed
                Ok(vec![
                    StreamWrite::new(
                        read_streams,
                        self.saga_id.clone(),
                        SagaEvent::CompensationCompleted,
                    )?
                ])
            }
        }
    }
}

// Example saga for hotel + flight + car booking
fn create_travel_booking_saga(
    hotel_booking: BookHotelCommand,
    flight_booking: BookFlightCommand,
    car_booking: BookCarCommand,
) -> BookingSaga {
    let steps = vec![
        SagaStep {
            name: "book_hotel".to_string(),
            command: Box::new(hotel_booking.clone()),
            compensation: Box::new(CancelHotelCommand {
                booking_id: hotel_booking.booking_id,
            }),
            status: StepStatus::Pending,
        },
        SagaStep {
            name: "book_flight".to_string(),
            command: Box::new(flight_booking.clone()),
            compensation: Box::new(CancelFlightCommand {
                booking_id: flight_booking.booking_id,
            }),
            status: StepStatus::Pending,
        },
        SagaStep {
            name: "book_car".to_string(),
            command: Box::new(car_booking.clone()),
            compensation: Box::new(CancelCarCommand {
                booking_id: car_booking.booking_id,
            }),
            status: StepStatus::Pending,
        },
    ];
    
    BookingSaga {
        saga_id: StreamId::from(format!("booking-saga-{}", SagaId::new())),
        reservation_id: StreamId::from(format!("reservation-{}", ReservationId::new())),
        steps,
        current_step: 0,
        compensation_mode: false,
    }
}
```

## Timeout and Retry Handling

Long-running processes need robust timeout and retry logic:

```rust
#[derive(Debug, Clone)]
struct ProcessTimeout {
    timeout_at: DateTime<Utc>,
    retry_policy: RetryPolicy,
    max_retries: u32,
    current_retries: u32,
}

#[derive(Debug, Clone)]
enum RetryPolicy {
    FixedDelay { delay: Duration },
    ExponentialBackoff { base_delay: Duration, max_delay: Duration },
    LinearBackoff { initial_delay: Duration, increment: Duration },
}

impl ProcessTimeout {
    fn should_retry(&self) -> bool {
        self.current_retries < self.max_retries
    }
    
    fn next_retry_delay(&self) -> Duration {
        match &self.retry_policy {
            RetryPolicy::FixedDelay { delay } => *delay,
            RetryPolicy::ExponentialBackoff { base_delay, max_delay } => {
                let delay = *base_delay * 2_u32.pow(self.current_retries);
                std::cmp::min(delay, *max_delay)
            }
            RetryPolicy::LinearBackoff { initial_delay, increment } => {
                *initial_delay + (*increment * self.current_retries)
            }
        }
    }
    
    fn next_timeout(&self) -> DateTime<Utc> {
        Utc::now() + self.next_retry_delay()
    }
}

// Timeout scheduler for processes
#[async_trait]
trait ProcessTimeoutScheduler {
    async fn schedule_timeout(
        &self,
        process_id: StreamId,
        timeout_at: DateTime<Utc>,
    ) -> Result<(), TimeoutError>;
    
    async fn cancel_timeout(
        &self,
        process_id: StreamId,
    ) -> Result<(), TimeoutError>;
}

struct InMemoryTimeoutScheduler {
    timeouts: Arc<RwLock<BTreeMap<DateTime<Utc>, Vec<StreamId>>>>,
    executor: CommandExecutor,
}

impl InMemoryTimeoutScheduler {
    async fn run_timeout_checker(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        
        loop {
            interval.tick().await;
            self.check_timeouts().await;
        }
    }
    
    async fn check_timeouts(&self) {
        let now = Utc::now();
        let mut timeouts = self.timeouts.write().await;
        
        // Find expired timeouts
        let expired: Vec<_> = timeouts
            .range(..=now)
            .flat_map(|(_, process_ids)| process_ids.clone())
            .collect();
        
        // Remove expired timeouts
        timeouts.retain(|&timeout_time, _| timeout_time > now);
        
        // Trigger timeout commands
        for process_id in expired {
            let timeout_command = ProcessTimeoutCommand {
                process_id,
                timed_out_at: now,
            };
            
            if let Err(e) = self.executor.execute(&timeout_command).await {
                tracing::error!("Failed to execute timeout command: {}", e);
            }
        }
    }
}

#[derive(Command, Clone)]
struct ProcessTimeoutCommand {
    #[stream]
    process_id: StreamId,
    
    timed_out_at: DateTime<Utc>,
}

impl CommandLogic for ProcessTimeoutCommand {
    type State = ProcessState;
    type Event = ProcessEvent;
    
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check if process should retry or fail
        let should_retry = state.timeout.as_ref()
            .map(|t| t.should_retry())
            .unwrap_or(false);
        
        if should_retry {
            let next_timeout = state.timeout.as_ref().unwrap().next_timeout();
            
            Ok(vec![
                StreamWrite::new(
                    &read_streams,
                    self.process_id.clone(),
                    ProcessEvent::RetryScheduled {
                        retry_at: next_timeout,
                        attempt: state.timeout.as_ref().unwrap().current_retries + 1,
                    }
                )?
            ])
        } else {
            Ok(vec![
                StreamWrite::new(
                    &read_streams,
                    self.process_id.clone(),
                    ProcessEvent::Failed {
                        reason: "Process timed out after maximum retries".to_string(),
                    }
                )?
            ])
        }
    }
}
```

## Process Monitoring and Observability

Monitor long-running processes in production:

```rust
use prometheus::{Counter, Histogram, Gauge};

lazy_static! {
    static ref PROCESS_STARTED: Counter = register_counter!(
        "eventcore_processes_started_total",
        "Total number of processes started"
    ).unwrap();
    
    static ref PROCESS_COMPLETED: Counter = register_counter!(
        "eventcore_processes_completed_total",
        "Total number of processes completed"
    ).unwrap();
    
    static ref PROCESS_FAILED: Counter = register_counter!(
        "eventcore_processes_failed_total",
        "Total number of processes failed"
    ).unwrap();
    
    static ref PROCESS_DURATION: Histogram = register_histogram!(
        "eventcore_process_duration_seconds",
        "Process execution duration"
    ).unwrap();
    
    static ref ACTIVE_PROCESSES: Gauge = register_gauge!(
        "eventcore_active_processes",
        "Number of currently active processes"
    ).unwrap();
}

#[derive(Clone)]
struct ProcessMetrics {
    process_counts: HashMap<String, ProcessCounts>,
    active_processes: HashSet<StreamId>,
}

#[derive(Debug, Default)]
struct ProcessCounts {
    started: u64,
    completed: u64,
    failed: u64,
    average_duration: Duration,
}

impl ProcessMetrics {
    fn record_process_started(&mut self, process_type: &str, process_id: StreamId) {
        PROCESS_STARTED.with_label_values(&[process_type]).inc();
        
        self.process_counts
            .entry(process_type.to_string())
            .or_default()
            .started += 1;
        
        self.active_processes.insert(process_id);
        ACTIVE_PROCESSES.set(self.active_processes.len() as f64);
    }
    
    fn record_process_completed(
        &mut self, 
        process_type: &str, 
        process_id: StreamId, 
        duration: Duration
    ) {
        PROCESS_COMPLETED.with_label_values(&[process_type]).inc();
        PROCESS_DURATION.observe(duration.as_secs_f64());
        
        let counts = self.process_counts
            .entry(process_type.to_string())
            .or_default();
        counts.completed += 1;
        
        // Update average duration
        let total_completed = counts.completed;
        counts.average_duration = (counts.average_duration * (total_completed - 1) as u32 + duration) 
            / total_completed as u32;
        
        self.active_processes.remove(&process_id);
        ACTIVE_PROCESSES.set(self.active_processes.len() as f64);
    }
    
    fn record_process_failed(&mut self, process_type: &str, process_id: StreamId) {
        PROCESS_FAILED.with_label_values(&[process_type]).inc();
        
        self.process_counts
            .entry(process_type.to_string())
            .or_default()
            .failed += 1;
        
        self.active_processes.remove(&process_id);
        ACTIVE_PROCESSES.set(self.active_processes.len() as f64);
    }
}

// Process health monitoring
#[derive(Debug)]
struct ProcessHealthCheck {
    max_process_age: Duration,
    max_retry_count: u32,
    warning_thresholds: HealthThresholds,
}

#[derive(Debug)]
struct HealthThresholds {
    failure_rate: f64,        // 0.0-1.0
    average_duration: Duration,
    stuck_process_age: Duration,
}

impl ProcessHealthCheck {
    async fn check_process_health(&self, metrics: &ProcessMetrics) -> HealthStatus {
        let mut issues = Vec::new();
        
        for (process_type, counts) in &metrics.process_counts {
            // Check failure rate
            let total = counts.started;
            if total > 0 {
                let failure_rate = counts.failed as f64 / total as f64;
                if failure_rate > self.warning_thresholds.failure_rate {
                    issues.push(format!(
                        "High failure rate for {}: {:.1}%", 
                        process_type, 
                        failure_rate * 100.0
                    ));
                }
            }
            
            // Check average duration
            if counts.average_duration > self.warning_thresholds.average_duration {
                issues.push(format!(
                    "Slow processes for {}: {:?}", 
                    process_type, 
                    counts.average_duration
                ));
            }
        }
        
        // Check for stuck processes
        let stuck_count = self.count_stuck_processes(&metrics.active_processes).await;
        if stuck_count > 0 {
            issues.push(format!("{} processes appear stuck", stuck_count));
        }
        
        if issues.is_empty() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Warning { issues }
        }
    }
    
    async fn count_stuck_processes(&self, active_processes: &HashSet<StreamId>) -> usize {
        // This would query the event store to check process ages
        // Implementation depends on your monitoring setup
        0
    }
}

#[derive(Debug)]
enum HealthStatus {
    Healthy,
    Warning { issues: Vec<String> },
    Critical { issues: Vec<String> },
}
```

## Testing Long-Running Processes

Test processes thoroughly:

```rust
#[cfg(test)]
mod process_tests {
    use super::*;
    use eventcore::testing::prelude::*;
    
    #[tokio::test]
    async fn test_order_fulfillment_happy_path() {
        let store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(store);
        
        let order_id = OrderId::new();
        let process = OrderFulfillmentProcess::start(order_id).unwrap();
        
        // Start process
        executor.execute(&process).await.unwrap();
        
        // Simulate payment confirmation
        let payment_event = PaymentConfirmed {
            order_id,
            amount: Money::from_cents(1000),
        };
        
        // Process should advance
        let advance_command = AdvanceOrderProcess {
            process_id: process.process_id,
            trigger: ProcessTrigger::PaymentConfirmed,
        };
        executor.execute(&advance_command).await.unwrap();
        
        // Continue with inventory, shipping, etc.
        // Verify process reaches completion
    }
    
    #[tokio::test]
    async fn test_process_timeout_and_retry() {
        let store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(store);
        let scheduler = InMemoryTimeoutScheduler::new(executor.clone());
        
        let order_id = OrderId::new();
        let mut process = OrderFulfillmentProcess::start(order_id).unwrap();
        process.timeout_at = Some(Utc::now() + Duration::from_secs(1));
        
        // Start process
        executor.execute(&process).await.unwrap();
        
        // Wait for timeout
        tokio::time::sleep(Duration::from_secs(2)).await;
        
        // Verify timeout was triggered
        // Check retry logic works
    }
    
    #[tokio::test]
    async fn test_saga_compensation() {
        let store = InMemoryEventStore::new();
        let executor = CommandExecutor::new(store);
        
        // Create booking saga
        let saga = create_travel_booking_saga(
            create_hotel_booking(),
            create_flight_booking(),
            create_car_booking(),
        );
        
        // Start saga
        executor.execute(&saga).await.unwrap();
        
        // Simulate hotel booking success
        simulate_step_success(&executor, &saga.saga_id, 0).await;
        
        // Simulate flight booking failure
        simulate_step_failure(&executor, &saga.saga_id, 1, "No availability").await;
        
        // Verify compensation started
        // Check hotel booking was cancelled
    }
}
```

## Best Practices

1. **Design for failure** - Always plan compensation strategies
2. **Use timeouts** - Prevent processes from hanging forever
3. **Implement retries** - Handle transient failures gracefully
4. **Monitor actively** - Track process health in production
5. **Keep state minimal** - Only store what's needed for decisions
6. **Test thoroughly** - Include failure scenarios and edge cases
7. **Document workflows** - Make process logic clear
8. **Version processes** - Handle schema evolution like events

## Summary

Long-running processes in EventCore:

- ✅ **Stateful workflows** - Coordinate complex business processes
- ✅ **Event-driven** - React to events from other parts of the system
- ✅ **Fault tolerant** - Handle failures and compensations
- ✅ **Monitorable** - Track health and performance
- ✅ **Testable** - Comprehensive testing support

Key patterns:
1. Use process managers for complex workflows
2. Implement saga pattern for distributed transactions
3. Handle timeouts and retries robustly
4. Monitor process health actively
5. Test all failure scenarios

Next, let's explore [Distributed Systems](./04-distributed-systems.md) →