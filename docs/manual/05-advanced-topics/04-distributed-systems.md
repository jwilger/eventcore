# Chapter 5.4: Distributed Systems

EventCore excels in distributed systems where multiple services need to coordinate while maintaining consistency. This chapter covers patterns for building resilient, scalable distributed event-sourced architectures.

## Distributed EventCore Architecture

### Service Boundaries

Each service owns its event streams and commands:

```rust
// User Service
#[derive(Command, Clone)]
struct CreateUser {
    #[stream]
    user_id: StreamId,

    email: Email,
    profile: UserProfile,
}

// Order Service
#[derive(Command, Clone)]
struct CreateOrder {
    #[stream]
    order_id: StreamId,

    #[stream]
    customer_id: StreamId, // References user from User Service

    items: Vec<OrderItem>,
}

// Payment Service
#[derive(Command, Clone)]
struct ProcessPayment {
    #[stream]
    payment_id: StreamId,

    #[stream]
    order_id: StreamId, // References order from Order Service

    amount: Money,
    method: PaymentMethod,
}
```

### Event Publishing

Services publish events for other services to consume:

```rust
use eventcore::distributed::{EventPublisher, EventSubscriber};

#[async_trait]
trait EventPublisher {
    async fn publish(&self, event: &StoredEvent) -> Result<(), PublishError>;
}

struct MessageBusPublisher {
    bus: MessageBus,
    topic_mapping: HashMap<String, String>,
}

impl MessageBusPublisher {
    async fn publish_event<E>(&self, event: &StoredEvent<E>) -> Result<(), PublishError>
    where
        E: Serialize,
    {
        let topic = self.topic_mapping
            .get(&E::event_type())
            .ok_or(PublishError::UnknownEventType)?;

        let message = DistributedEvent {
            event_id: event.id,
            event_type: E::event_type(),
            stream_id: event.stream_id.clone(),
            version: event.version,
            payload: serde_json::to_value(&event.payload)?,
            metadata: event.metadata.clone(),
            occurred_at: event.occurred_at,
            published_at: Utc::now(),
            service_id: self.service_id(),
        };

        self.bus.publish(topic, &message).await?;
        Ok(())
    }

    fn service_id(&self) -> String {
        std::env::var("SERVICE_ID").unwrap_or_else(|_| "unknown".to_string())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DistributedEvent {
    event_id: EventId,
    event_type: String,
    stream_id: StreamId,
    version: EventVersion,
    payload: serde_json::Value,
    metadata: EventMetadata,
    occurred_at: DateTime<Utc>,
    published_at: DateTime<Utc>,
    service_id: String,
}
```

### Event Subscription

Services subscribe to events from other services:

```rust
#[async_trait]
trait EventSubscriber {
    async fn subscribe<F>(&self, topic: &str, handler: F) -> Result<(), SubscribeError>
    where
        F: Fn(DistributedEvent) -> BoxFuture<'_, Result<(), HandleError>> + Send + Sync + 'static;
}

struct OrderEventHandler {
    executor: CommandExecutor,
}

impl OrderEventHandler {
    async fn handle_user_events(&self, event: DistributedEvent) -> Result<(), HandleError> {
        match event.event_type.as_str() {
            "UserRegistered" => {
                let user_registered: UserRegisteredEvent = serde_json::from_value(event.payload)?;

                // Create customer profile in order service
                let command = CreateCustomerProfile {
                    customer_id: StreamId::from(format!("customer-{}", user_registered.user_id)),
                    user_id: user_registered.user_id,
                    email: user_registered.email,
                    preferences: CustomerPreferences::default(),
                };

                self.executor.execute(&command).await?;
            }
            "UserUpdated" => {
                // Handle user updates
                let user_updated: UserUpdatedEvent = serde_json::from_value(event.payload)?;

                let command = UpdateCustomerProfile {
                    customer_id: StreamId::from(format!("customer-{}", user_updated.user_id)),
                    email: user_updated.email,
                    profile_updates: user_updated.profile_changes,
                };

                self.executor.execute(&command).await?;
            }
            _ => {
                // Unknown event type - log and ignore
                tracing::debug!("Ignoring unknown event type: {}", event.event_type);
            }
        }
        Ok(())
    }
}

// Setup subscription
async fn setup_event_subscriptions(
    subscriber: &impl EventSubscriber,
    handler: OrderEventHandler,
) -> Result<(), SubscribeError> {
    // Subscribe to user events
    subscriber.subscribe("user-events", move |event| {
        let handler = handler.clone();
        Box::pin(async move {
            handler.handle_user_events(event).await
        })
    }).await?;

    // Subscribe to payment events
    subscriber.subscribe("payment-events", move |event| {
        let handler = handler.clone();
        Box::pin(async move {
            handler.handle_payment_events(event).await
        })
    }).await?;

    Ok(())
}
```

## Distributed Transactions

Handle distributed transactions with the saga pattern:

```rust
#[derive(Command, Clone)]
struct DistributedOrderSaga {
    #[stream]
    saga_id: StreamId,

    order_details: OrderDetails,
    customer_id: UserId,
}

#[derive(Default)]
struct DistributedSagaState {
    order_created: bool,
    payment_reserved: bool,
    inventory_reserved: bool,
    shipping_scheduled: bool,
    completed: bool,
    compensation_needed: bool,
    failed_step: Option<String>,
}

impl CommandLogic for DistributedOrderSaga {
    type State = DistributedSagaState;
    type Event = SagaEvent;

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        if state.compensation_needed {
            self.handle_compensation(&state)
        } else {
            self.handle_forward_flow(&state)
        }
    }
}

impl DistributedOrderSaga {
    fn handle_forward_flow(&self, state: &DistributedSagaState) -> Result<NewEvents<SagaEvent>, CommandError> {
        match (state.order_created, state.payment_reserved, state.inventory_reserved, state.shipping_scheduled) {
            (false, _, _, _) => {
                // Step 1: Create order
                Ok(NewEvents::from(vec![SagaEvent::OrderCreationRequested { order_details: self.order_details.clone() }]))
            }
            (true, false, _, _) => {
                // Step 2: Reserve payment
                Ok(NewEvents::from(vec![SagaEvent::PaymentReservationRequested { customer_id: self.customer_id, amount: self.order_details.total_amount() }]))
            }
            (true, true, false, _) => {
                // Step 3: Reserve inventory
                Ok(NewEvents::from(vec![SagaEvent::InventoryReservationRequested { items: self.order_details.items.clone() }]))
            }
            (true, true, true, false) => {
                // Step 4: Schedule shipping
                Ok(NewEvents::from(vec![SagaEvent::ShippingScheduleRequested { order_id: self.order_details.order_id, shipping_address: self.order_details.shipping_address.clone() }]))
            }
            (true, true, true, true) => {
                // All steps completed
                Ok(NewEvents::from(vec![SagaEvent::SagaCompleted]))
            }
        }
    }

    fn handle_compensation(&self, state: &DistributedSagaState) -> Result<NewEvents<SagaEvent>, CommandError> {
        // Compensate in reverse order
        if state.shipping_scheduled {
            Ok(NewEvents::from(vec![SagaEvent::ShippingCancellationRequested]))
        } else if state.inventory_reserved {
            Ok(NewEvents::from(vec![SagaEvent::InventoryReleaseRequested]))
        } else if state.payment_reserved {
            Ok(NewEvents::from(vec![SagaEvent::PaymentReleaseRequested]))
        } else if state.order_created {
            Ok(NewEvents::from(vec![SagaEvent::OrderCancellationRequested]))
        } else {
            Ok(NewEvents::from(vec![SagaEvent::CompensationCompleted]))
        }
    }
}
```

// External service integration
struct ExternalServiceClient {
http_client: reqwest::Client,
service_url: String,
timeout: Duration,
}

impl ExternalServiceClient {
async fn create_order(&self, order: &OrderDetails) -> Result<OrderId, ServiceError> {
let response = self.http_client
.post(&format!("{}/orders", self.service_url))
.json(order)
.timeout(self.timeout)
.send()
.await?;

        if response.status().is_success() {
            let result: CreateOrderResponse = response.json().await?;
            Ok(result.order_id)
        } else {
            Err(ServiceError::RequestFailed {
                status: response.status(),
                body: response.text().await.unwrap_or_default(),
            })
        }
    }

    async fn cancel_order(&self, order_id: OrderId) -> Result<(), ServiceError> {
        let response = self.http_client
            .delete(&format!("{}/orders/{}", self.service_url, order_id))
            .timeout(self.timeout)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ServiceError::RequestFailed {
                status: response.status(),
                body: response.text().await.unwrap_or_default(),
            });
        }

        Ok(())
    }

}

## Event Sourcing Across Services

### Cross-Service Projections

Build projections that consume events from multiple services:

```rust
struct CrossServiceOrderProjection {
    orders: HashMap<OrderId, OrderView>,
    event_store: Arc<dyn EventStore>,
    user_service_client: UserServiceClient,
    payment_service_client: PaymentServiceClient,
}

#[derive(Debug, Clone)]
struct OrderView {
    order_id: OrderId,
    customer_info: CustomerInfo,
    items: Vec<OrderItem>,
    payment_status: PaymentStatus,
    shipping_status: ShippingStatus,
    total_amount: Money,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[async_trait]
impl Projection for CrossServiceOrderProjection {
    type Event = DistributedEvent;
    type Error = ProjectionError;

    async fn apply(&mut self, event: &StoredEvent<Self::Event>) -> Result<(), Self::Error> {
        match event.payload.event_type.as_str() {
            "OrderCreated" => {
                let order_created: OrderCreatedEvent =
                    serde_json::from_value(event.payload.payload.clone())?;

                // Get customer info from user service
                let customer_info = self.user_service_client
                    .get_customer_info(order_created.customer_id)
                    .await?;

                let order_view = OrderView {
                    order_id: order_created.order_id,
                    customer_info,
                    items: order_created.items,
                    payment_status: PaymentStatus::Pending,
                    shipping_status: ShippingStatus::NotStarted,
                    total_amount: order_created.total_amount,
                    created_at: event.occurred_at,
                    updated_at: event.occurred_at,
                };

                self.orders.insert(order_created.order_id, order_view);
            }
            "PaymentProcessed" => {
                let payment_processed: PaymentProcessedEvent =
                    serde_json::from_value(event.payload.payload.clone())?;

                if let Some(order) = self.orders.get_mut(&payment_processed.order_id) {
                    order.payment_status = PaymentStatus::Completed;
                    order.updated_at = event.occurred_at;
                }
            }
            "ShipmentDispatched" => {
                let shipment_dispatched: ShipmentDispatchedEvent =
                    serde_json::from_value(event.payload.payload.clone())?;

                if let Some(order) = self.orders.get_mut(&shipment_dispatched.order_id) {
                    order.shipping_status = ShippingStatus::Dispatched;
                    order.updated_at = event.occurred_at;
                }
            }
            _ => {} // Ignore other events
        }

        Ok(())
    }
}
```

### Event Federation

Federate events across service boundaries:

```rust
struct EventFederationHub {
    publishers: HashMap<String, Box<dyn EventPublisher>>,
    subscribers: HashMap<String, Vec<Box<dyn EventSubscriber>>>,
    routing_rules: RoutingRules,
}

#[derive(Debug, Clone)]
struct RoutingRules {
    routes: Vec<RoutingRule>,
}

#[derive(Debug, Clone)]
struct RoutingRule {
    source_service: String,
    event_pattern: String,
    target_services: Vec<String>,
    transformation: Option<String>,
}

impl EventFederationHub {
    async fn route_event(&self, event: &DistributedEvent) -> Result<(), FederationError> {
        let applicable_rules = self.routing_rules
            .routes
            .iter()
            .filter(|rule| {
                rule.source_service == event.service_id &&
                self.matches_pattern(&event.event_type, &rule.event_pattern)
            });

        for rule in applicable_rules {
            let transformed_event = if let Some(ref transformation) = rule.transformation {
                self.transform_event(event, transformation)?
            } else {
                event.clone()
            };

            for target_service in &rule.target_services {
                if let Some(publisher) = self.publishers.get(target_service) {
                    publisher.publish_federated_event(&transformed_event).await?;
                }
            }
        }

        Ok(())
    }

    fn matches_pattern(&self, event_type: &str, pattern: &str) -> bool {
        // Simple pattern matching - could be more sophisticated
        pattern == "*" ||
        pattern == event_type ||
        (pattern.ends_with("*") && event_type.starts_with(&pattern[..pattern.len()-1]))
    }

    fn transform_event(&self, event: &DistributedEvent, transformation: &str) -> Result<DistributedEvent, FederationError> {
        // Apply transformation rules
        match transformation {
            "user_to_customer" => {
                let mut transformed = event.clone();
                transformed.event_type = transformed.event_type.replace("User", "Customer");
                Ok(transformed)
            }
            "anonymize_pii" => {
                let mut transformed = event.clone();
                // Remove PII from payload
                if let Some(email) = transformed.payload.get_mut("email") {
                    *email = serde_json::Value::String("***@***.***".to_string());
                }
                Ok(transformed)
            }
            _ => Err(FederationError::UnknownTransformation(transformation.to_string())),
        }
    }
}
```

## Service Discovery and Health

### Service Registry

```rust
#[async_trait]
trait ServiceRegistry {
    async fn register_service(&self, service: ServiceInfo) -> Result<(), RegistryError>;
    async fn discover_services(&self, service_type: &str) -> Result<Vec<ServiceInfo>, RegistryError>;
    async fn health_check(&self, service_id: &str) -> Result<HealthStatus, RegistryError>;
}

#[derive(Debug, Clone)]
struct ServiceInfo {
    id: String,
    name: String,
    service_type: String,
    version: String,
    endpoints: HashMap<String, String>,
    health_check_url: String,
    capabilities: Vec<String>,
    metadata: HashMap<String, String>,
    registered_at: DateTime<Utc>,
}

struct ConsulServiceRegistry {
    consul_client: ConsulClient,
}

impl ConsulServiceRegistry {
    async fn register_eventcore_service(&self) -> Result<(), RegistryError> {
        let service = ServiceInfo {
            id: format!("eventcore-{}", uuid::Uuid::new_v4()),
            name: "order-service".to_string(),
            service_type: "eventcore".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            endpoints: hashmap! {
                "http".to_string() => "http://localhost:8080".to_string(),
                "grpc".to_string() => "grpc://localhost:8081".to_string(),
                "events".to_string() => "kafka://localhost:9092/order-events".to_string(),
            },
            health_check_url: "http://localhost:8080/health".to_string(),
            capabilities: vec![
                "event-sourcing".to_string(),
                "order-management".to_string(),
                "payment-processing".to_string(),
            ],
            metadata: hashmap! {
                "environment".to_string() => "production".to_string(),
                "region".to_string() => "us-east-1".to_string(),
            },
            registered_at: Utc::now(),
        };

        self.register_service(service).await
    }
}
```

### Circuit Breaker for Service Calls

```rust
struct ServiceCircuitBreaker {
    state: Arc<RwLock<CircuitBreakerState>>,
    config: CircuitBreakerConfig,
}

#[derive(Debug)]
struct CircuitBreakerConfig {
    failure_threshold: u32,
    timeout: Duration,
    retry_timeout: Duration,
}

#[derive(Debug)]
enum CircuitBreakerState {
    Closed { failure_count: u32 },
    Open { failed_at: DateTime<Utc> },
    HalfOpen,
}

impl ServiceCircuitBreaker {
    async fn call<F, T, E>(&self, operation: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: Future<Output = Result<T, E>>,
    {
        // Check circuit state
        {
            let state = self.state.read().await;
            match *state {
                CircuitBreakerState::Open { failed_at } => {
                    if Utc::now() - failed_at < self.config.retry_timeout {
                        return Err(CircuitBreakerError::CircuitOpen);
                    }
                    // Transition to half-open
                }
                _ => {}
            }
        }

        // Update to half-open if we were open
        {
            let mut state = self.state.write().await;
            if matches!(*state, CircuitBreakerState::Open { .. }) {
                *state = CircuitBreakerState::HalfOpen;
            }
        }

        // Execute operation with timeout
        match tokio::time::timeout(self.config.timeout, operation).await {
            Ok(Ok(result)) => {
                // Success - reset circuit
                let mut state = self.state.write().await;
                *state = CircuitBreakerState::Closed { failure_count: 0 };
                Ok(result)
            }
            Ok(Err(e)) => {
                // Operation failed
                self.record_failure().await;
                Err(CircuitBreakerError::OperationFailed(e))
            }
            Err(_) => {
                // Timeout
                self.record_failure().await;
                Err(CircuitBreakerError::Timeout)
            }
        }
    }

    async fn record_failure(&self) {
        let mut state = self.state.write().await;
        match *state {
            CircuitBreakerState::Closed { failure_count } => {
                let new_count = failure_count + 1;
                if new_count >= self.config.failure_threshold {
                    *state = CircuitBreakerState::Open { failed_at: Utc::now() };
                } else {
                    *state = CircuitBreakerState::Closed { failure_count: new_count };
                }
            }
            CircuitBreakerState::HalfOpen => {
                *state = CircuitBreakerState::Open { failed_at: Utc::now() };
            }
            _ => {}
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum CircuitBreakerError<E> {
    #[error("Circuit breaker is open")]
    CircuitOpen,

    #[error("Operation timed out")]
    Timeout,

    #[error("Operation failed: {0}")]
    OperationFailed(E),
}
```

## Distributed Monitoring

### Distributed Tracing

```rust
use opentelemetry::{global, trace::{TraceContextExt, Tracer}};
use tracing_opentelemetry::OpenTelemetrySpanExt;

#[derive(Clone)]
struct DistributedCommandExecutor {
    inner: CommandExecutor,
    tracer: Box<dyn Tracer + Send + Sync>,
}

impl DistributedCommandExecutor {
    async fn execute_with_tracing<C: Command>(
        &self,
        command: &C,
        parent_context: Option<SpanContext>,
    ) -> CommandResult<ExecutionResult> {
        let span = self.tracer
            .span_builder(format!("execute_command_{}", std::any::type_name::<C>()))
            .with_kind(SpanKind::Internal)
            .start(&self.tracer);

        if let Some(parent) = parent_context {
            span.set_parent(parent);
        }

        let _guard = span.enter();

        span.set_attribute("command.type", std::any::type_name::<C>());
        span.set_attribute("service.name", self.service_name());

        match self.inner.execute(command).await {
            Ok(result) => {
                span.set_attribute("command.success", true);
                span.set_attribute("events.written", result.events_written.len() as i64);
                Ok(result)
            }
            Err(e) => {
                span.set_attribute("command.success", false);
                span.set_attribute("error.message", e.to_string());
                Err(e)
            }
        }
    }
}

// Distributed event with trace context
#[derive(Debug, Serialize, Deserialize)]
struct TracedDistributedEvent {
    #[serde(flatten)]
    event: DistributedEvent,
    trace_id: String,
    span_id: String,
}

impl From<(&StoredEvent, &SpanContext)> for TracedDistributedEvent {
    fn from((event, context): (&StoredEvent, &SpanContext)) -> Self {
        Self {
            event: event.into(),
            trace_id: context.trace_id().to_string(),
            span_id: context.span_id().to_string(),
        }
    }
}
```

### Metrics Collection

```rust
use prometheus::{Counter, Histogram, Gauge, Registry};

#[derive(Clone)]
struct DistributedMetrics {
    registry: Registry,
    // Command metrics
    commands_total: Counter,
    command_duration: Histogram,
    command_errors: Counter,
    // Event metrics
    events_published: Counter,
    events_consumed: Counter,
    event_lag: Gauge,
    // Service metrics
    service_health: Gauge,
    active_connections: Gauge,
}

impl DistributedMetrics {
    fn new(service_name: &str) -> Self {
        let registry = Registry::new();

        let commands_total = Counter::new(
            "eventcore_commands_total",
            "Total commands executed"
        ).unwrap();

        let command_duration = Histogram::new(
            "eventcore_command_duration_seconds",
            "Command execution duration"
        ).unwrap();

        let command_errors = Counter::new(
            "eventcore_command_errors_total",
            "Total command errors"
        ).unwrap();

        let events_published = Counter::new(
            "eventcore_events_published_total",
            "Total events published"
        ).unwrap();

        let events_consumed = Counter::new(
            "eventcore_events_consumed_total",
            "Total events consumed"
        ).unwrap();

        let event_lag = Gauge::new(
            "eventcore_event_lag_seconds",
            "Event processing lag"
        ).unwrap();

        let service_health = Gauge::new(
            "eventcore_service_health",
            "Service health status (0=down, 1=up)"
        ).unwrap();

        let active_connections = Gauge::new(
            "eventcore_active_connections",
            "Number of active connections"
        ).unwrap();

        // Register all metrics
        registry.register(Box::new(commands_total.clone())).unwrap();
        registry.register(Box::new(command_duration.clone())).unwrap();
        registry.register(Box::new(command_errors.clone())).unwrap();
        registry.register(Box::new(events_published.clone())).unwrap();
        registry.register(Box::new(events_consumed.clone())).unwrap();
        registry.register(Box::new(event_lag.clone())).unwrap();
        registry.register(Box::new(service_health.clone())).unwrap();
        registry.register(Box::new(active_connections.clone())).unwrap();

        Self {
            registry,
            commands_total,
            command_duration,
            command_errors,
            events_published,
            events_consumed,
            event_lag,
            service_health,
            active_connections,
        }
    }

    fn record_command_executed(&self, command_type: &str, duration: Duration, success: bool) {
        self.commands_total
            .with_label_values(&[command_type])
            .inc();

        self.command_duration
            .with_label_values(&[command_type])
            .observe(duration.as_secs_f64());

        if !success {
            self.command_errors
                .with_label_values(&[command_type])
                .inc();
        }
    }

    fn record_event_published(&self, event_type: &str) {
        self.events_published
            .with_label_values(&[event_type])
            .inc();
    }

    fn record_event_consumed(&self, event_type: &str, lag: Duration) {
        self.events_consumed
            .with_label_values(&[event_type])
            .inc();

        self.event_lag
            .with_label_values(&[event_type])
            .set(lag.as_secs_f64());
    }

    async fn export_metrics(&self) -> String {
        use prometheus::Encoder;
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder.encode_to_string(&metric_families).unwrap()
    }
}
```

## Testing Distributed Systems

```rust
#[cfg(test)]
mod distributed_tests {
    use super::*;
    use testcontainers::*;

    #[tokio::test]
    async fn test_distributed_saga() {
        // Setup test environment with multiple services
        let docker = clients::Cli::default();
        let kafka_container = docker.run(images::kafka::Kafka::default());
        let postgres_container = docker.run(images::postgres::Postgres::default());

        // Start services
        let user_service = start_user_service(&postgres_container).await;
        let order_service = start_order_service(&postgres_container).await;
        let payment_service = start_payment_service(&postgres_container).await;

        // Setup event routing
        let event_hub = EventFederationHub::new(&kafka_container);

        // Execute distributed saga
        let saga = DistributedOrderSaga {
            saga_id: StreamId::new(),
            order_details: create_test_order(),
            customer_id: create_test_customer(&user_service).await,
        };

        let result = order_service.execute_saga(&saga).await;

        // Verify all services were coordinated correctly
        assert!(result.is_ok());

        // Verify final state across services
        let order = order_service.get_order(saga.order_details.order_id).await?;
        assert_eq!(order.status, OrderStatus::Completed);

        let payment = payment_service.get_payment(saga.order_details.order_id).await?;
        assert_eq!(payment.status, PaymentStatus::Completed);
    }

    #[tokio::test]
    async fn test_service_failure_compensation() {
        // Similar setup but simulate payment service failure
        // Verify compensation is triggered
        // Verify order is cancelled
        // Verify inventory is released
    }
}
```

## Best Practices

1. **Design for independence** - Services should be loosely coupled
2. **Use event-driven communication** - Prefer async events over sync calls
3. **Implement circuit breakers** - Protect against cascading failures
4. **Monitor everything** - Comprehensive observability is critical
5. **Plan for failure** - Design compensation strategies upfront
6. **Version everything** - Events, services, and APIs
7. **Test across services** - Include distributed testing
8. **Document service contracts** - Clear event schemas and APIs

## Summary

Distributed EventCore systems:

- ✅ **Service boundaries** - Clear ownership of streams and commands
- ✅ **Event-driven** - Async communication between services
- ✅ **Fault tolerant** - Circuit breakers and compensation
- ✅ **Observable** - Distributed tracing and metrics
- ✅ **Scalable** - Independent scaling of services

Key patterns:

1. Own your streams - each service owns its event streams
2. Publish events - share state changes via events
3. Use sagas - coordinate distributed transactions
4. Monitor health - track service health and performance
5. Plan for failure - implement circuit breakers and compensation

Next, let's explore [Performance Optimization](./05-performance-optimization.md) →
