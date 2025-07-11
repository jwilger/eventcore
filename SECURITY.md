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

## Security Considerations for Application Developers

When building applications with EventCore, follow these security best practices:

### 1. Event Payload Security

- **Never store sensitive data unencrypted** in events (passwords, API keys, SSNs, etc.)
- **Use encryption** for PII and sensitive business data before storing in events
- **Consider data retention** - events are immutable and permanent by design

```rust
// Bad: Storing sensitive data directly
#[derive(Serialize, Deserialize)]
struct UserRegistered {
    email: String,
    password: String,  // Never do this!
}

// Good: Store only necessary data
#[derive(Serialize, Deserialize)]
struct UserRegistered {
    user_id: UserId,
    email_hash: String,  // Store hash for lookups
    registered_at: Timestamp,
}
```

### 2. Stream Access Control

EventCore doesn't provide built-in authorization. Implement access control at the application layer:

```rust
// Implement authorization before command execution
async fn handle_command(cmd: Command, user: AuthenticatedUser) -> Result<()> {
    // Check user permissions for the affected streams
    if !user.can_access_stream(&cmd.stream_id()) {
        return Err(CommandError::Unauthorized);
    }
    
    executor.execute(cmd).await
}
```

### 3. Input Validation

Always validate and sanitize input at application boundaries:

```rust
// Use nutype for domain validation
#[nutype(
    sanitize(trim),
    validate(regex = r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$"),
    derive(Debug, Clone, Serialize, Deserialize)
)]
struct Email(String);

// Validate before creating commands
let email = Email::try_new(untrusted_input)
    .map_err(|_| "Invalid email format")?;
```

### 4. Rate Limiting and Resource Protection

Protect against resource exhaustion:

```rust
// Configure executor with appropriate limits
let executor = CommandExecutor::builder(event_store)
    .with_timeout(Duration::from_secs(30))
    .with_max_retries(3)
    .build();

// Implement rate limiting at API layer
rate_limiter.check_rate_limit(user_id)?;
```

### 5. Projection Security

- **Sanitize data** before displaying in read models
- **Implement row-level security** in projections
- **Validate projection state** before exposing to users

### 6. Monitoring and Alerting

- **Log security events** (failed auth, suspicious patterns)
- **Monitor for anomalies** in event patterns
- **Alert on security violations** promptly

### 7. Compliance Considerations

- **GDPR**: Implement event encryption and consider pseudonymization
- **PCI DSS**: Never store credit card details in events
- **HIPAA**: Encrypt all health-related data
- **Audit Requirements**: Leverage event sourcing's natural audit trail

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