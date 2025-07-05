## Description

<!-- Brief description of changes and motivation -->
<!-- Note: GitHub Copilot will use the checklists below to guide its review -->

## Type of Change

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Performance improvement
- [ ] Documentation update
- [ ] Security enhancement

## Testing

- [ ] All tests pass locally (`cargo test --workspace`)
- [ ] Added/updated tests for new functionality
- [ ] Added/updated property-based tests for invariants

## Performance Impact

<!-- For performance-sensitive changes, include benchmark results -->

<details>
<summary>Benchmark Results</summary>

```bash
# Run benchmarks before and after changes:
git checkout main
cargo bench --bench event_store -- --save-baseline main
git checkout your-branch
cargo bench --bench event_store -- --baseline main

# For realistic workload benchmarks:
cargo bench --bench realistic_workloads -- --save-baseline main
# ... switch branches ...
cargo bench --bench realistic_workloads -- --baseline main
```

<!-- Paste benchmark comparison results here -->

</details>

## Security Checklist

### Input Validation
- [ ] All public API inputs use validated `nutype` types
- [ ] No raw strings/primitives for domain concepts
- [ ] Proper error messages without sensitive information

### Data Protection
- [ ] No sensitive data (passwords, keys, PII) stored unencrypted
- [ ] Proper use of `SecureString` or similar for sensitive fields
- [ ] Audit trail considerations for compliance

### Dependencies
- [ ] Ran `cargo audit` - no vulnerabilities
- [ ] New dependencies justified in PR description
- [ ] Dependencies from reputable sources with active maintenance

### Error Handling
- [ ] All errors use proper Result types
- [ ] No `unwrap()` in production code paths
- [ ] Error messages don't leak implementation details

## Code Quality

### Type Safety
- [ ] Illegal states made unrepresentable
- [ ] Parse, don't validate - smart constructors used
- [ ] Total functions - all cases handled

### Performance
- [ ] No unbounded allocations
- [ ] Appropriate use of `&str` vs `String`
- [ ] Batch operations where applicable
- [ ] Resource cleanup guaranteed (RAII)

### Documentation
- [ ] Public APIs have doc comments with examples
- [ ] Complex algorithms explained
- [ ] Breaking changes noted in comments

## Reviewer Checklist

- [ ] Code follows project style guidelines
- [ ] Changes are well-tested
- [ ] Documentation is clear and complete
- [ ] Security considerations addressed
- [ ] Performance impact acceptable
- [ ] Breaking changes justified

## Review Focus

<!-- Guide reviewers to specific areas that need attention -->
<!-- Examples:
- Complex algorithm in src/executor/optimization.rs needs performance review
- New error handling pattern in command.rs - looking for consistency feedback
- Security implications of the new stream access pattern
- API breaking changes in types.rs need careful consideration
-->