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
        stream_declarations: StreamDeclarations<Self::StreamSet>,
        state: Self::State,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        // Check for timeout
        if let Some(timeout) = state.timeout_at {
            if Utc::now() > timeout {
                return Ok(vec![
                    StreamWrite::new(
                        &stream_declarations,
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
                self.handle_payment_step(&stream_declarations, &state).await
            }
            FulfillmentStep::PaymentConfirmed => {
                self.handle_inventory_step(&stream_declarations, &state).await
            }
            FulfillmentStep::InventoryReserved => {
                self.handle_shipping_step(&stream_declarations, &state).await
            }
            FulfillmentStep::Shipped => {
                self.handle_delivery_step(&stream_declarations, &state).await
            }
            FulfillmentStep::Delivered => {
                self.handle_completion_step(&stream_declarations, &state).await
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
        stream_declarations: &StreamDeclarations<OrderFulfillmentProcessStreamSet>,
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
                    stream_declarations,
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
        stream_declarations: &StreamDeclarations<OrderFulfillmentProcessStreamSet>,
        state: &OrderFulfillmentState,
    ) -> CommandResult<Vec<StreamWrite<OrderFulfillmentProcessStreamSet, ProcessEvent>>> {
        if !state.inventory_reserved {
            // Reserve inventory
            Ok(vec![
                StreamWrite::new(
                    stream_declarations,
                    self.process_id.clone(),
                    ProcessEvent::InventoryReserved,
                )?
            ])
        } else {
            // Move to shipping
            Ok(vec![
                StreamWrite::new(
                    stream_declarations,
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
        stream_declarations: StreamDeclarations<Self::StreamSet>,
        state: Self::State,
        _stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        if state.compensation_mode {
            self.handle_compensation(&stream_declarations, &state).await
        } else {
            self.handle_forward_execution(&stream_declarations, &state).await
        }
    }
}

impl BookingSaga {
    async fn handle_forward_execution(
        &self,
        stream_declarations: &StreamDeclarations<BookingSagaStreamSet>,
        state: &SagaState,
    ) -> CommandResult<Vec<StreamWrite<BookingSagaStreamSet, SagaEvent>>> {
        if state.current_step >= state.steps.len() {
            // All steps completed
            return Ok(vec![
                StreamWrite::new(
                    stream_declarations,
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
                        stream_declarations,
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
                        stream_declarations,
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
                        stream_declarations,
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
        stream_declarations: &StreamDeclarations<BookingSagaStreamSet>,
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
                        stream_declarations,
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
                        stream_declarations,
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
        stream_declarations: StreamDeclarations<Self::StreamSet>,
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
                    &stream_declarations,
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
                    &stream_declarations,
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
```

... (rest unchanged)
