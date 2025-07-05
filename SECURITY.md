# Security Policy

## Reporting Security Vulnerabilities

We take the security of EventCore seriously. If you believe you have found a security vulnerability in EventCore, please report it to us through GitHub Security Advisories.

**Please do not report security vulnerabilities through public GitHub issues.**

### How to Report

1. Go to the [Security tab](https://github.com/eventsourcing/eventcore/security) in our GitHub repository
2. Click "Report a vulnerability"
3. Provide a clear description of the vulnerability including:
   - Type of vulnerability (e.g., SQL injection, resource exhaustion)
   - Affected components or modules
   - Steps to reproduce
   - Potential impact
   - Any suggested fixes (if applicable)

### What to Expect

- **Initial Response**: We will acknowledge receipt of your report within 7 days
- **Assessment**: We will investigate and assess the severity within 30 days
- **Resolution**: We aim to resolve confirmed vulnerabilities within 30-90 days, depending on complexity
  - Critical vulnerabilities that are actively exploited will be prioritized
  - Expedited fixes may be available for sponsors or through paid support

### Disclosure Policy

- We follow responsible disclosure practices
- Security advisories will be published after a fix is available
- We will credit reporters who wish to be acknowledged
- We request that you do not publicly disclose the vulnerability until we have published a fix

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Security Considerations for Contributors

When contributing to EventCore, please follow these security guidelines:

### Code Security

1. **Input Validation**
   - Always use validated types (`nutype`) for public API inputs
   - Validate at system boundaries only - internal functions can trust validated types
   - Never trust user input without validation

2. **SQL Security**
   - Use parameterized queries exclusively - never concatenate SQL strings
   - Review the `sqlx` query macros to ensure compile-time SQL verification
   - Test for SQL injection attempts in integration tests

3. **Error Handling**
   - Never expose internal system details in error messages
   - Don't leak database connection strings or file paths
   - Use the `thiserror` crate for structured error types

4. **Dependencies**
   - Run `cargo audit` before submitting PRs
   - Justify any new dependencies in PR descriptions
   - Prefer well-maintained, widely-used crates
   - Check for security advisories on dependencies

5. **Memory Safety**
   - Avoid unbounded allocations (use limits on collections)
   - Be careful with recursive data structures
   - Use `Box` for large stack allocations
   - Leverage Rust's ownership system - avoid `unsafe` code

6. **Testing**
   - Never commit real credentials or sensitive data
   - Use mock data for all tests
   - Include security-focused test cases (e.g., malformed input)
   - Test error paths thoroughly

### Development Practices

- **Code Review**: All changes require review before merging
- **CI Security Checks**: All PRs must pass `cargo audit` and security lints
- **Commit Signing**: Contributors are encouraged to sign commits with GPG
- **Branch Protection**: Main branch requires PR reviews and passing CI

## Security Features in EventCore

EventCore includes several security-focused design decisions:

- **Type Safety**: Extensive use of validated newtypes prevents many common vulnerabilities
- **Concurrency Control**: Optimistic locking prevents lost updates
- **Resource Limits**: Configurable timeouts and batch sizes prevent resource exhaustion
- **Audit Trail**: Event sourcing provides complete audit history by design

## Compliance

EventCore aims to align with industry security standards:

- **OWASP** Secure Coding Practices
- **NIST** Software Development Framework
- General secure development lifecycle practices

Specific compliance documentation is in development.

## Contact

For non-security questions, please use:
- GitHub Issues for bug reports and feature requests
- GitHub Discussions for questions and community support

For security issues, use only the GitHub Security Advisory process described above.