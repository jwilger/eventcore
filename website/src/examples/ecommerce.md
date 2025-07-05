# E-Commerce Example

The e-commerce example shows how to build a complete order processing system with inventory management using EventCore.

## Key Features

- **Order Processing**: Multi-step order workflow with validation
- **Inventory Management**: Real-time stock tracking across warehouses
- **Dynamic Pricing**: Apply discounts and calculate totals
- **Multi-Stream Operations**: Coordinate between orders, inventory, and customers

## Running the Example

```bash
cargo run --example ecommerce
```

## Code Walkthrough

The example demonstrates:

- Complex state machines for order lifecycle
- Compensation patterns for failed operations
- Projection-based inventory queries
- Integration with external payment systems

[View Source Code](https://github.com/jwilger/eventcore/tree/main/eventcore-examples/src/ecommerce)
