---
title: Error Message Consistency Audit
status: open
priority: 2
issue-type: task
created-at: "2026-02-17T07:49:26.944336-08:00"
---

[migrated from bead eventcore-016, type: chore]

Audit and ensure consistency, clarity, and actionability of error messages written incrementally throughout eventcore-001 to eventcore-014. NOT 'add error messages at the end' - error quality is foundational from eventcore-001. This ensures error messages are consistent in format, provide appropriate context, and are actionable across all increments.

## Acceptance Criteria
Feature: Error messages are consistent and actionable

Scenario: Audit reveals error message consistency
  Given auditor reviews all error types across I-001 to I-014
  When checking error message format
  Then all errors include relevant context (stream IDs, versions)
  And all errors explain what failed and why
  And error format is consistent across all increments

Scenario: Developer receives actionable error messages
  Given developer encounters various error scenarios
  When error is returned
  Then error message explains what failed
  And error suggests next steps (retry, increase capacity, fix code)
  And error includes context for debugging (actual vs expected values)

Scenario: Version conflict error provides full context
  Given concurrent modification causes conflict
  When developer receives ConcurrencyError
  Then error includes stream IDs and current/expected versions
  And error explains "Automatic retry will reattempt with fresh state"
  And error links to concurrency documentation

Scenario: Business rule violation includes context
  Given account has balance 50
  When developer executes Withdraw with amount 100
  And business rule "sufficient funds" fails in handle()
  Then CommandError::BusinessRuleViolation is returned
  And error shows "Insufficient funds: balance 50, required 100"
  And error is actionable for debugging

Scenario: Audit identifies and fixes inconsistencies
  Given auditor reviews all error messages
  When inconsistencies are found (missing context, unclear wording)
  Then inconsistencies are documented and prioritized
  And critical issues are fixed before release
  And minor issues are tracked for future improvement
