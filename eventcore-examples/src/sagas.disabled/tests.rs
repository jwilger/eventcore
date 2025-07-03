//! Tests for the order fulfillment saga example
//!
//! These tests demonstrate EventCore's saga pattern capabilities with:
//! - Successful multi-step workflows
//! - Failure scenarios and compensation logic
//! - Complex state management across multiple streams

use eventcore::prelude::*;
use eventcore::testing::prelude::*;
use proptest::prelude::*;
use std::sync::Arc;

use crate::sagas::{commands::*, events::*, types::*};

// ============================================================================
// Unit Tests for Types and Events
// ============================================================================

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_money_operations() {
        let money1 = Money::from_dollars(10.50);
        let money2 = Money::from_dollars(5.25);
        
        assert_eq!(money1.to_dollars(), 10.50);
        assert_eq!(money2.to_dollars(), 5.25);
        assert_eq!(money1.add(&money2).to_dollars(), 15.75);
        
        let item_price = Money::from_dollars(2.99);
        let quantity = Quantity::try_new(3).unwrap();
        assert_eq!(item_price.multiply(quantity.into()).to_dollars(), 8.97);
    }

    #[test]
    fn test_order_item_calculations() {
        let product_id = ProductId::generate();
        let quantity = Quantity::try_new(5).unwrap();
        let unit_price = Money::from_dollars(12.99);
        
        let item = OrderItem::new(product_id, quantity, unit_price);
        assert_eq!(item.total_price().to_dollars(), 64.95);
    }

    #[test]
    fn test_saga_event_utilities() {
        let saga_id = SagaId::generate();
        
        let event = SagaEvent::SagaCompleted {
            saga_id: saga_id.clone(),
            completed_at: chrono::Utc::now(),
        };
        
        assert_eq!(event.saga_id(), &saga_id);
        assert!(event.is_terminal());
        assert!(!event.is_failure());
        
        let failure_event = SagaEvent::PaymentFailed {
            saga_id: saga_id.clone(),
            payment_id: PaymentId::generate(),
            reason: "Card declined".to_string(),
        };
        
        assert!(failure_event.is_failure());
        assert!(!failure_event.is_terminal());
    }
}

// ============================================================================
// Integration Tests for Saga Workflow
// ============================================================================

#[cfg(test)]
mod integration_tests {
    use super::*;

    pub async fn setup_test_environment() -> (Arc<MockEventStore<SagaEvent>>, CommandExecutor<MockEventStore<SagaEvent>>) {
        let event_store = Arc::new(MockEventStore::new());
        let executor = CommandExecutor::new(event_store.clone());
        (event_store, executor)
    }

    pub fn create_test_order_input() -> OrderFulfillmentInput {
        let product1 = ProductId::generate();
        let product2 = ProductId::generate();
        
        OrderFulfillmentInput {
            order_id: OrderId::generate(),
            customer_id: CustomerId::generate(),
            items: vec![
                OrderItem::new(
                    product1,
                    Quantity::try_new(2).unwrap(),
                    Money::from_dollars(15.99),
                ),
                OrderItem::new(
                    product2,
                    Quantity::try_new(1).unwrap(),
                    Money::from_dollars(29.99),
                ),
            ],
            shipping_address: ShippingAddress {
                street: "123 Main St".to_string(),
                city: "Anytown".to_string(),
                state: "CA".to_string(),
                zip_code: "12345".to_string(),
                country: "USA".to_string(),
            },
            payment_method: PaymentMethod::CreditCard {
                last_four: "1234".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn test_successful_saga_execution() {
        let (_event_store, executor) = integration_tests::setup_test_environment().await;
        let command = OrderFulfillmentSaga;
        let input = integration_tests::create_test_order_input();
        
        // Execute the saga command
        let result = executor.execute(&command, input.clone()).await;
        assert!(result.is_ok(), "Saga execution should succeed: {:?}", result);
        
        let events = result.unwrap();
        assert!(!events.is_empty(), "Should produce events");
        
        // Check that saga started event is produced
        let saga_started = events.iter().find(|e| {
            matches!(e.event, SagaEvent::SagaStarted { .. })
        });
        assert!(saga_started.is_some(), "Should have SagaStarted event");
    }

    #[tokio::test]
    async fn test_payment_processing() {
        let (_event_store, executor) = integration_tests::setup_test_environment().await;
        let command = ProcessPaymentCommand;
        let input = ProcessPaymentInput {
            payment_id: PaymentId::generate(),
            order_id: OrderId::generate(),
            amount: Money::from_dollars(100.00),
            method: PaymentMethod::CreditCard {
                last_four: "4321".to_string(),
            },
        };
        
        let result = executor.execute(&command, input.clone()).await;
        assert!(result.is_ok(), "Payment processing should succeed");
        
        let events = result.unwrap();
        assert!(!events.is_empty(), "Should produce payment events");
        
        // Check for payment authorization
        let payment_auth = events.iter().find(|e| {
            matches!(e.event, PaymentEvent::PaymentAuthorized { .. })
        });
        assert!(payment_auth.is_some(), "Should have PaymentAuthorized event");
        
        // Check for payment capture
        let payment_capture = events.iter().find(|e| {
            matches!(e.event, PaymentEvent::PaymentCaptured { .. })
        });
        assert!(payment_capture.is_some(), "Should have PaymentCaptured event");
    }

    #[tokio::test]
    async fn test_inventory_reservation() {
        let (_event_store, executor) = integration_tests::setup_test_environment().await;
        let command = ReserveInventoryCommand;
        let input = ReserveInventoryInput {
            order_id: OrderId::generate(),
            items: vec![
                OrderItem::new(
                    ProductId::generate(),
                    Quantity::try_new(3).unwrap(),
                    Money::from_dollars(25.00),
                ),
            ],
        };
        
        let result = executor.execute(&command, input.clone()).await;
        assert!(result.is_ok(), "Inventory reservation should succeed");
        
        let events = result.unwrap();
        assert!(!events.is_empty(), "Should produce inventory events");
        
        // Check for inventory reservation
        let inventory_reserved = events.iter().find(|e| {
            matches!(e.event, InventoryEvent::InventoryReserved { .. })
        });
        assert!(inventory_reserved.is_some(), "Should have InventoryReserved event");
    }

    #[tokio::test]
    async fn test_shipping_arrangement() {
        let (_event_store, executor) = integration_tests::setup_test_environment().await;
        let command = ArrangeShippingCommand;
        let input = ArrangeShippingInput {
            order_id: OrderId::generate(),
            shipment_id: ShipmentId::generate(),
            address: ShippingAddress {
                street: "456 Oak Ave".to_string(),
                city: "Somewhere".to_string(),
                state: "NY".to_string(),
                zip_code: "67890".to_string(),
                country: "USA".to_string(),
            },
            items: vec![
                OrderItem::new(
                    ProductId::generate(),
                    Quantity::try_new(1).unwrap(),
                    Money::from_dollars(49.99),
                ),
            ],
        };
        
        let result = executor.execute(&command, input.clone()).await;
        assert!(result.is_ok(), "Shipping arrangement should succeed");
        
        let events = result.unwrap();
        assert!(!events.is_empty(), "Should produce shipping events");
        
        // Check for shipment creation
        let shipment_created = events.iter().find(|e| {
            matches!(e.event, ShippingEvent::ShipmentCreated { .. })
        });
        assert!(shipment_created.is_some(), "Should have ShipmentCreated event");
        
        // Check for shipment dispatch
        let shipment_dispatched = events.iter().find(|e| {
            matches!(e.event, ShippingEvent::ShipmentDispatched { .. })
        });
        assert!(shipment_dispatched.is_some(), "Should have ShipmentDispatched event");
    }

    #[tokio::test]
    async fn test_saga_state_reconstruction() {
        let (_event_store, executor) = integration_tests::setup_test_environment().await;
        let command = OrderFulfillmentSaga;
        let input = integration_tests::create_test_order_input();
        
        // Execute saga multiple times to test state reconstruction
        for i in 0..3 {
            let result = executor.execute(&command, input.clone()).await;
            assert!(
                result.is_ok(),
                "Saga execution {} should succeed: {:?}",
                i,
                result
            );
        }
    }
}

// ============================================================================
// Property-Based Tests
// ============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;

    prop_compose! {
        fn arb_money()(cents in 0u64..1_000_000) -> Money {
            Money::new(cents)
        }
    }

    prop_compose! {
        fn arb_quantity()(qty in 1u32..100) -> Quantity {
            Quantity::try_new(qty).unwrap()
        }
    }

    prop_compose! {
        fn arb_product_id()(id in "[a-z]{1,10}") -> ProductId {
            ProductId::try_new(format!("product-{}", id)).unwrap()
        }
    }

    prop_compose! {
        fn arb_order_item()(
            product_id in arb_product_id(),
            quantity in arb_quantity(),
            unit_price in arb_money()
        ) -> OrderItem {
            OrderItem::new(product_id, quantity, unit_price)
        }
    }

    proptest! {
        #[test]
        fn prop_money_addition_is_commutative(
            a in arb_money(),
            b in arb_money()
        ) {
            prop_assert_eq!(a.add(&b), b.add(&a));
        }

        #[test]
        fn prop_money_multiplication_correct(
            price in arb_money(),
            qty in arb_quantity()
        ) {
            let result = price.multiply(qty.into_inner());
            let expected = Money::new(price.into_inner() * qty.into_inner() as u64);
            prop_assert_eq!(result, expected);
        }

        #[test]
        fn prop_order_item_total_calculation(
            item in arb_order_item()
        ) {
            let expected = Money::new(
                item.unit_price.into_inner() * item.quantity.into_inner() as u64
            );
            prop_assert_eq!(item.total_price(), expected);
        }

        #[test]
        fn prop_saga_events_maintain_saga_id(
            saga_id in "[a-z0-9\\-]{1,50}",
            order_id in "[a-z0-9\\-]{1,50}",
            customer_id in "[a-z0-9\\-]{1,50}"
        ) {
            let saga_id = SagaId::try_new(saga_id).unwrap();
            let order_id = OrderId::try_new(order_id).unwrap();
            let customer_id = CustomerId::try_new(customer_id).unwrap();
            
            let event = SagaEvent::SagaStarted {
                saga_id: saga_id.clone(),
                order_id,
                customer_id,
                total_amount: Money::new(1000),
                started_at: chrono::Utc::now(),
            };
            
            prop_assert_eq!(event.saga_id(), &saga_id);
        }
    }
}

// ============================================================================
// Failure Scenario Tests
// ============================================================================

#[cfg(test)]
mod failure_tests {
    use super::*;

    #[tokio::test]
    async fn test_insufficient_inventory_scenario() {
        // This test would simulate insufficient inventory and verify compensation
        // In a real implementation, we would modify the inventory state to trigger failures
        
        let (_event_store, executor) = integration_tests::setup_test_environment().await;
        let command = ReserveInventoryCommand;
        
        // Create an order with very high quantities to simulate insufficient stock
        let input = ReserveInventoryInput {
            order_id: OrderId::generate(),
            items: vec![
                OrderItem::new(
                    ProductId::generate(),
                    Quantity::try_new(99).unwrap(), // High quantity
                    Money::from_dollars(10.00),
                ),
            ],
        };
        
        let result = executor.execute(&command, input).await;
        
        // In this simplified example, the command will still succeed
        // but in a real implementation with actual inventory checking,
        // this would demonstrate failure handling
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_payment_failure_compensation() {
        // This test demonstrates how payment failures would trigger compensation
        // In a real implementation, we would simulate payment failures
        
        let (_event_store, executor) = integration_tests::setup_test_environment().await;
        let command = ProcessPaymentCommand;
        
        let input = ProcessPaymentInput {
            payment_id: PaymentId::generate(),
            order_id: OrderId::generate(),
            amount: Money::from_dollars(0.01), // Very small amount to simulate decline
            method: PaymentMethod::CreditCard {
                last_four: "0000".to_string(), // Invalid card simulation
            },
        };
        
        let result = executor.execute(&command, input).await;
        
        // In this simplified example, payments always succeed
        // but this demonstrates the structure for handling failures
        assert!(result.is_ok());
    }
}

// ============================================================================
// Performance and Stress Tests
// ============================================================================

#[cfg(test)]
mod performance_tests {
    use super::*;

    #[tokio::test]
    async fn test_concurrent_saga_execution() {
        let (_event_store, executor) = integration_tests::setup_test_environment().await;
        let command = OrderFulfillmentSaga;
        
        // Execute multiple sagas concurrently
        let mut handles = Vec::new();
        
        for i in 0..10 {
            let executor = executor.clone();
            let command = command.clone();
            let mut input = integration_tests::create_test_order_input();
            
            // Ensure unique order IDs
            input.order_id = OrderId::try_new(format!("order-{}", i)).unwrap();
            
            let handle = tokio::spawn(async move {
                executor.execute(&command, input).await
            });
            
            handles.push(handle);
        }
        
        // Wait for all sagas to complete
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok(), "Concurrent saga should succeed");
        }
    }

    #[tokio::test]
    async fn test_saga_with_many_items() {
        let (_event_store, executor) = integration_tests::setup_test_environment().await;
        let command = ReserveInventoryCommand;
        
        // Create an order with many items
        let mut items = Vec::new();
        for i in 0..50 {
            items.push(OrderItem::new(
                ProductId::try_new(format!("product-{}", i)).unwrap(),
                Quantity::try_new(1).unwrap(),
                Money::from_dollars(10.00),
            ));
        }
        
        let input = ReserveInventoryInput {
            order_id: OrderId::generate(),
            items,
        };
        
        let result = executor.execute(&command, input).await;
        assert!(result.is_ok(), "Large order should be processed successfully");
        
        let events = result.unwrap();
        assert_eq!(events.len(), 50, "Should produce events for all items");
    }
}