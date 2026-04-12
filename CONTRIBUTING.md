# Contributing to EventCore

Thank you for your interest in contributing to EventCore! This document provides guidelines and instructions for contributing to the project.

## Code of Conduct

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/eventcore.git`
3. Add upstream remote: `git remote add upstream https://github.com/jwilger/eventcore.git`
4. Create a feature branch: `git checkout -b my-feature-branch`

## Development Setup

### Prerequisites

- Rust stable toolchain (2024 edition) -- the exact version is pinned via `rust-toolchain.toml`
- Docker and Docker Compose (for PostgreSQL backend tests only)
- Nix (recommended) -- run `nix develop` for the correct toolchain and all dev dependencies

### Workspace Crates

| Crate                | Purpose                                                            |
| -------------------- | ------------------------------------------------------------------ |
| `eventcore`          | Main library: `execute()`, `run_projection()`, re-exports          |
| `eventcore-types`    | Shared vocabulary: traits (`EventStore`, `CommandLogic`) and types |
| `eventcore-macros`   | `#[derive(Command)]`, `require!`, `emit!` macro implementations    |
| `eventcore-postgres` | PostgreSQL backend with ACID transactions and advisory locks       |
| `eventcore-sqlite`   | SQLite backend with optional SQLCipher encryption                  |
| `eventcore-memory`   | Zero-dependency in-memory store for tests and development          |
| `eventcore-testing`  | Contract tests, chaos harness, `EventCollector`, `TestScenario`    |
| `eventcore-examples` | Integration tests demonstrating EventCore patterns                 |

### Initial Setup

```bash
# Enter development environment (recommended)
nix develop

# Start PostgreSQL (only needed for postgres backend tests)
docker-compose up -d

# Run tests to verify setup
cargo nextest run --workspace
```

## Commit Guidelines

### Commit Message Format

This project uses [Conventional Commits](https://www.conventionalcommits.org/).
All commit messages and PR titles must follow this format:

```
type(scope): short description
```

**Types:**

| Type       | When to use                                  |
| ---------- | -------------------------------------------- |
| `feat`     | A new feature or capability                  |
| `fix`      | A bug fix                                    |
| `docs`     | Documentation-only changes                   |
| `style`    | Formatting, whitespace (no logic changes)    |
| `refactor` | Code restructuring without behavior change   |
| `test`     | Adding or updating tests                     |
| `chore`    | Maintenance, tooling, CI, dependency updates |

**Scopes** (optional but encouraged): `core`, `postgres`, `sqlite`, `memory`,
`testing`, `macros`, `types`, `examples`

**Examples:**

```
feat(core): add retry policy configuration to execute()

fix(postgres): prevent advisory lock leak on transaction rollback

refactor(types): extract StreamVersion validation into nutype

test(sqlite): add contract tests for SQLCipher encryption mode

chore: update workspace dependencies to latest compatible versions
```

Add a blank line and a body paragraph when the "why" needs explanation.
Never add `Co-Authored-By` trailers to commit messages.

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
4. **Run the test suite**: `cargo nextest run --workspace` (fallback: `cargo test --workspace`)
5. **Run linting**: `cargo clippy --all-targets --all-features -- -D warnings`
6. **Format code**: `cargo fmt --all`
7. **Commit your changes** with descriptive messages
8. **Push to your fork** and create a pull request

## Testing

### Running Tests

```bash
# Run all tests (preferred)
cargo nextest run --workspace

# Run a specific test by name
cargo nextest run --workspace -E 'test(test_name)'

# Run a specific integration test file
cargo nextest run --test feature_name_test

# Fallback: standard cargo test
cargo test --workspace
```

> **Note:** `cargo nextest` is the primary test runner. Use `cargo test` only
> as a fallback when nextest is unavailable.

### Writing Tests

- Write unit tests for pure functions
- Write integration tests for database operations
- Use property-based testing for invariants
- Follow the existing test patterns in the codebase

### Mutation Testing

Run `cargo mutants` to verify test suite quality. Zero surviving mutants is
required -- every behavioral mutation the tool introduces must be caught by at
least one test.

### Pre-Commit Hooks

Pre-commit hooks are enforced via `.pre-commit-config.yaml` and lefthook. They
run formatting, linting, and dependency checks automatically on each commit.
Keep hooks green before pushing.

## Branching Workflow

1. **Create a feature branch** from `main`: `git checkout -b type/description`
2. **Make commits** using Conventional Commits
3. **Push and create a PR**: `git push -u origin <branch>` then `gh pr create`
4. **PRs are squash-merged** into `main`

### Best Practices

1. **Small, focused PRs** - Each PR should be independently reviewable
2. **One issue per PR** - Link the GitHub issue in the PR description
3. **Descriptive branch names** - e.g. `feat/add-user-model`
4. **Submit early** - Create draft PRs to show intent

## Task Tracking

This project uses **GitHub Issues** for all task tracking. See AGENTS.md for
label conventions and CLI commands.

## Pull Request Process

1. **Update documentation** for any changed functionality
2. **Add tests** for new features
3. **Ensure CI passes** - all checks must be green
4. Changelogs are auto-generated by release-plz from Conventional Commit messages. Do not manually edit per-crate `CHANGELOG.md` files
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
    derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Into, Serialize, Deserialize)
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

- Open a [Discussion](https://github.com/jwilger/eventcore/discussions) for questions
- Check existing issues before creating new ones
- Join our community chat (to be added)

## Recognition

Contributors will be recognized in:

- The project README
- Release notes
- Special thanks in documentation

Thank you for contributing to EventCore!
