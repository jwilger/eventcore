# GitHub Copilot Review Instructions

When reviewing pull requests for EventCore, please focus on the following areas based on the PR template checklists:

## Security Review Points

1. **Input Validation**
   - Verify all public API inputs use validated `nutype` types
   - Check for any raw strings or primitives used for domain concepts
   - Ensure error messages don't leak sensitive information

2. **Data Protection**
   - Confirm no passwords, keys, or PII are stored unencrypted
   - Check for proper use of secure types for sensitive fields
   - Verify audit trail implementation for compliance requirements

3. **Dependencies**
   - Flag any new dependencies that lack justification
   - Check dependencies are from reputable sources with active maintenance
   - Verify `cargo audit` has been run successfully

4. **Error Handling**
   - Ensure all errors use proper Result types
   - Flag any `unwrap()` or `expect()` in production code paths
   - Check error messages don't expose implementation details

## Code Quality Review Points

1. **Type Safety**
   - Verify illegal states are made unrepresentable through types
   - Check for "parse, don't validate" pattern usage
   - Ensure all functions are total (handle all cases)

2. **Performance**
   - Look for unbounded allocations or potential memory leaks
   - Check appropriate use of `&str` vs `String`
   - Verify batch operations are used where applicable
   - Ensure proper resource cleanup (RAII)

3. **Documentation**
   - Verify all public APIs have doc comments with examples
   - Check complex algorithms are properly explained
   - Ensure breaking changes are clearly noted

## Review Focus Areas

Pay special attention to the "Review Focus" section in each PR description. This section highlights specific areas where the PR author wants focused review, such as:

- Complex algorithms needing performance review
- New patterns requiring consistency feedback
- Security implications of changes
- API breaking changes needing careful consideration

## EventCore-Specific Patterns

1. **Event Sourcing**
   - Verify events are immutable
   - Check event ordering uses UUIDv7
   - Ensure proper stream version tracking

2. **Type-Driven Development**
   - Confirm nutype validation is only used at library boundaries
   - Verify smart constructors return Result types
   - Check domain types encode business rules

3. **Command Pattern**
   - Ensure commands declare all streams they access
   - Verify atomic multi-stream operations
   - Check proper concurrency control

## Compliance Considerations

For PRs touching security-sensitive areas, verify alignment with:
- OWASP Top 10 requirements
- NIST Cybersecurity Framework
- GDPR data protection principles
- PCI DSS for payment-related code
- HIPAA for healthcare-related code
- SOX for financial controls

Refer to the [COMPLIANCE_CHECKLIST.md](../COMPLIANCE_CHECKLIST.md) for detailed requirements.