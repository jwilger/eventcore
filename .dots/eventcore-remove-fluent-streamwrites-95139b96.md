---
title: Remove fluent StreamWrites API, add GWT testing helpers
status: open
priority: 2
issue-type: task
created-at: "2026-02-17T07:49:26.948759-08:00"
---

[migrated from bead eventcore-945, type: task]

**Background:** The StreamWrites fluent builder API (with_stream, with_event, build) was added to make test setup more pleasant. However, it creates a dual-API pattern that is confusing.

**Changes:**
1. **Remove fluent API** from StreamWrites since it is internal code anyway
2. **Add GWT testing helpers** to eventcore-testing crate

**Desired testing ergonomics:**
```rust
// Given the following events on stream A and B
let scenario = TestScenario::new()
    .given_events("account-123", vec![
        MoneyDeposited { amount: 100 },
        MoneyWithdrawn { amount: 50 },
    ])
    .given_events("account-456", vec![
        MoneyDeposited { amount: 200 },
    ]);

// When command is executed
scenario.when(TransferMoney { from: "account-123", to: "account-456", amount: 25 });

// Then I see the following new events
scenario.then_events("account-123", vec![
    MoneyWithdrawn { amount: 25 },
]);
scenario.then_events("account-456", vec![
    MoneyDeposited { amount: 25 },
]);
```

**Similar pattern for projection testing:**
```rust
let scenario = ProjectionTestScenario::new()
    .given_events(vec![...])
    .when_projected::<BalanceProjection>()
    .then_state_equals(expected_balance);
```

**Note:** This is about test ergonomics, not production API. The Result-based builder pattern remains for production use.
