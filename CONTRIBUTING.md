# Contributing to EventCore

Thank you for your interest in contributing to EventCore! This document provides guidelines and instructions for contributing to the project.

## Code of Conduct

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/eventcore.git`
3. Add upstream remote: `git remote add upstream https://github.com/eventsourcing/eventcore.git`
4. Create a feature branch: `git checkout -b my-feature-branch`

## Development Setup

### Prerequisites

- Rust 1.70+ (check with `rustc --version`)
- Docker and Docker Compose (for PostgreSQL databases)
- Nix (optional, for development environment)

### Initial Setup

```bash
# Enter development environment (if using Nix)
nix develop

# Start databases
docker-compose up -d

# Run tests to verify setup
cargo test --workspace
```

## Commit Guidelines

### Commit Message Format

We follow a specific commit message format:

```
Short summary (max 50 chars)

Detailed explanation of the change. Wrap lines at 72 characters.
Focus on WHY the change was made, not just what changed.

Include any breaking changes, performance implications, or other
important notes.
```

Example:

```
Add snapshot support for long-running streams

Event streams with millions of events cause performance issues during
state reconstruction. Snapshots allow faster recovery by storing
periodic state checkpoints.

Implements automatic snapshot creation based on configurable event
count thresholds. Snapshots are stored alongside events and loaded
transparently during reconstruction.
```

### GPG Commit Signing (Recommended)

While not required, we encourage contributors to sign their commits with GPG for added security and authenticity.

#### Setting up GPG Signing

1. **Generate a GPG key** (if you don't have one):

   ```bash
   gpg --full-generate-key
   ```

   - Choose RSA and RSA (default)
   - Key size: 4096 bits
   - Expiration: Your preference (1-2 years recommended)
   - Use your GitHub email address

2. **List your GPG keys**:

   ```bash
   gpg --list-secret-keys --keyid-format=long
   ```

   Look for a line like `sec rsa4096/3AA5C34371567BD2`

3. **Export your public key**:

   ```bash
   gpg --armor --export 3AA5C34371567BD2
   ```

   Copy the output including `-----BEGIN PGP PUBLIC KEY BLOCK-----` and `-----END PGP PUBLIC KEY BLOCK-----`

4. **Add the key to GitHub**:
   - Go to Settings → SSH and GPG keys
   - Click "New GPG key"
   - Paste your public key

5. **Configure Git to sign commits**:

   ```bash
   git config --global user.signingkey 3AA5C34371567BD2
   git config --global commit.gpgsign true
   ```

6. **Configure GPG agent** (for password caching):
   ```bash
   echo "default-cache-ttl 3600" >> ~/.gnupg/gpg-agent.conf
   echo "max-cache-ttl 86400" >> ~/.gnupg/gpg-agent.conf
   ```

#### Verifying Signed Commits

To verify signatures on existing commits:

```bash
git log --show-signature
```

To verify a specific commit:

```bash
git verify-commit <commit-hash>
```

## Development Workflow

1. **Create a feature branch** from `main`
2. **Make your changes** following our coding standards
3. **Write tests** for new functionality
4. **Run the test suite**: `cargo test --workspace`
5. **Run linting**: `cargo clippy --workspace --all-targets -- -D warnings`
6. **Format code**: `cargo fmt`
7. **Commit your changes** with descriptive messages
8. **Push to your fork** and create a pull request

## Testing

### Running Tests

```bash
# Run all tests
cargo test --workspace

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture

# Run integration tests only
cargo test --test '*' --workspace
```

### Writing Tests

- Write unit tests for pure functions
- Write integration tests for database operations
- Use property-based testing for invariants
- Follow the existing test patterns in the codebase

## Stacked Pull Requests (Optional)

For complex features, consider using stacked PRs to break work into reviewable chunks.

### Setup

If using Nix:

```bash
nix develop  # git-spice is included
gs repo init
gs auth      # Authenticate with GitHub (one-time)
```

Without Nix:

```bash
# Install git-spice (see: https://github.com/abhinav/git-spice)
# On macOS:
brew install git-spice

# On Linux (download binary):
# https://github.com/abhinav/git-spice/releases

gs repo init
gs auth
```

### Creating a Stack

```bash
# Start from main
git checkout main && git pull

# Create first branch
gs branch create add-user-model
# ... make changes, commit ...

# Stack another branch on top
gs branch create add-user-api
# ... make changes, commit ...

# Submit all PRs
gs stack submit
```

### Maintaining a Stack

```bash
# After making changes to any branch
gs stack submit    # Updates all PRs

# After a PR is merged (squash-merge)
gs repo sync       # Fetch and update main
gs stack restack   # Rebase remaining branches
gs stack submit    # Update remaining PRs
```

### Best Practices

1. **Small, focused PRs** - Each PR should be independently reviewable
2. **One beads issue per PR** - Use `discovered-from` for dependencies
3. **Descriptive branch names** - `gs branch create meaningful-name`
4. **Submit early** - Create draft PRs to show intent
5. **Restack promptly** - After any PR merges, restack the remaining stack

### Troubleshooting

**Conflicts during restack:**

```bash
# Resolve conflicts in your editor
git add <resolved-files>
git rebase --continue
# Then re-submit
gs stack submit
```

**Detached from stack:**

```bash
gs stack    # See current stack state
gs branch track --base <upstream-branch>  # Re-attach if needed
```

For more details, see [git-spice documentation](https://abhinav.github.io/git-spice/).

## Pull Request Process

1. **Update documentation** for any changed functionality
2. **Add tests** for new features
3. **Ensure CI passes** - all checks must be green
4. **Update CHANGELOG.md** with your changes (once we have one)
5. **Request review** from maintainers

### PR Title Format

Use clear, descriptive titles:

- ✅ "Add snapshot support for event streams"
- ✅ "Fix race condition in concurrent command execution"
- ❌ "Fix bug"
- ❌ "Update code"

## Security

Please review our [Security Policy](SECURITY.md) for guidelines on:

- Reporting vulnerabilities
- Security best practices for contributions
- Dependency management

### Security Checklist for Contributors

Before submitting your PR, ensure:

- [ ] No hardcoded secrets or credentials
- [ ] All user input is validated using `nutype` types
- [ ] SQL queries use parameterized statements (via `sqlx`)
- [ ] Error messages don't leak sensitive information
- [ ] New dependencies are justified and from reputable sources
- [ ] Tests don't contain real credentials or PII

## Code Style

### Rust Guidelines

1. **Follow Rust idioms** - use `clippy` to catch anti-patterns
2. **Use meaningful names** - prefer clarity over brevity
3. **Document public APIs** - all public items need doc comments
4. **Prefer composition** - small, focused functions that compose
5. **Handle errors explicitly** - use `Result` types, avoid `unwrap()`

### Type-Driven Development

EventCore follows strict type-driven development principles:

1. **Types first** - design types that make illegal states unrepresentable
2. **Parse, don't validate** - use smart constructors with `nutype`
3. **No primitive obsession** - wrap primitives in domain types
4. **Total functions** - handle all cases explicitly

Example:

```rust
// Good: Domain type with validation
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
    derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Deref, Serialize, Deserialize)
)]
pub struct StreamId(String);

// Bad: Using raw String
pub fn process_stream(stream_id: String) { ... }

// Good: Using domain type
pub fn process_stream(stream_id: StreamId) { ... }
```

## Documentation

### Code Documentation

- Document all public APIs with doc comments
- Include examples in doc comments where helpful
- Explain "why" not just "what"
- Document invariants and assumptions

### User Documentation

When adding new features:

1. Update relevant sections in `/docs`
2. Add examples to `/eventcore-examples` if applicable
3. Update API documentation

## Questions?

- Open a [Discussion](https://github.com/eventsourcing/eventcore/discussions) for questions
- Check existing issues before creating new ones
- Join our community chat (to be added)

## Recognition

Contributors will be recognized in:

- The project README
- Release notes
- Special thanks in documentation

Thank you for contributing to EventCore!
