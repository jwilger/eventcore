# GWT Scenarios

This directory contains Given-When-Then scenarios organized by feature area.

## Structure

```
scenarios/
├── commands/
│   ├── single-stream-execution.md
│   └── multi-stream-atomic.md
├── projections/
│   └── projection-runner.md
└── ...
```

## Creating Scenarios

Use the `/design gwt` command:
```
/design gwt <feature-name>
```

## Scenario Format

```gherkin
Feature: [Feature Name]
  As a [role]
  I want [capability]
  So that [benefit]

  Scenario: [Name]
    Given [precondition]
    When [action]
    Then [outcome]
```

## Integration Tests

Scenarios in this directory often map to integration tests in:
- `eventcore/tests/I-*.rs`
- `eventcore-postgres/tests/i*.rs`
