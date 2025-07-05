#!/bin/bash
set -e

# Sync manual documentation
echo "Syncing manual documentation..."
mkdir -p src/manual

# Save the API documentation file (it has custom content)
if [ -f "src/manual/07-reference/01-api-documentation.md" ]; then
    cp src/manual/07-reference/01-api-documentation.md /tmp/api-doc-backup.md
fi

cp -r ../docs/manual/* src/manual/

# Restore the API documentation file
if [ -f "/tmp/api-doc-backup.md" ]; then
    cp /tmp/api-doc-backup.md src/manual/07-reference/01-api-documentation.md
fi

# Copy other important files
echo "Copying additional documentation..."
cp ../CHANGELOG.md src/changelog.md
cp ../LICENSE src/license.md
cp ../CODE_OF_CONDUCT.md src/contributing.md

# Add front matter to contributing.md
cat > src/contributing.md.tmp << 'EOF'
# Contributing to EventCore

Thank you for your interest in contributing to EventCore! We welcome contributions from the community.

## Code of Conduct

EOF
cat ../CODE_OF_CONDUCT.md >> src/contributing.md.tmp
mv src/contributing.md.tmp src/contributing.md

# Create example pages
echo "Creating example documentation..."
mkdir -p src/examples
cat > src/examples/banking.md << 'EOF'
# Banking Example

The banking example demonstrates EventCore's multi-stream atomic operations by implementing a double-entry bookkeeping system.

## Key Features

- **Atomic Transfers**: Move money between accounts with ACID guarantees
- **Balance Validation**: Prevent overdrafts with compile-time safe types
- **Audit Trail**: Complete history of all transactions
- **Account Lifecycle**: Open, close, and freeze accounts

## Running the Example

```bash
cargo run --example banking
```

## Code Structure

The example includes:

- `types.rs` - Domain types with validation (AccountId, Money, etc.)
- `events.rs` - Account events (Opened, Deposited, Withdrawn, etc.)
- `commands.rs` - Business operations (OpenAccount, Transfer, etc.)
- `projections.rs` - Read models for account balances and history

[View Source Code](https://github.com/jwilger/eventcore/tree/main/eventcore-examples/src/banking)
EOF

cat > src/examples/ecommerce.md << 'EOF'
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
EOF

cat > src/examples/sagas.md << 'EOF'
# Sagas Example

The saga example implements distributed transaction patterns using EventCore's multi-stream capabilities.

## What are Sagas?

Sagas are a pattern for managing long-running business processes that span multiple bounded contexts or services. EventCore makes implementing sagas straightforward with its multi-stream atomicity.

## Example Scenario

This example implements a travel booking saga that coordinates:

- Flight reservation
- Hotel booking
- Car rental
- Payment processing

Each step can fail, triggering compensating actions to maintain consistency.

## Running the Example

```bash
cargo run --example sagas
```

## Implementation Details

- **Orchestration**: Central saga coordinator manages the workflow
- **Compensation**: Automatic rollback on failures
- **Idempotency**: Safe retries with exactly-once semantics
- **Monitoring**: Built-in observability for saga progress

[View Source Code](https://github.com/jwilger/eventcore/tree/main/eventcore-examples/src/sagas)
EOF

# Copy static files to src directory for mdBook
echo "Copying static files..."
cp static/logo.png src/
cp static/.nojekyll src/

echo "Documentation sync complete!"