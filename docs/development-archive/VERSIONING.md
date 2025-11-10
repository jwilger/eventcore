# EventCore Versioning Strategy

This document defines the semantic versioning strategy for the EventCore workspace and all its constituent crates.

## Overview

EventCore follows [Semantic Versioning 2.0.0](https://semver.org/) with workspace-synchronized versioning across all crates. This ensures compatibility and simplifies dependency management for users.

## Workspace Structure

The EventCore workspace consists of:

| Crate                  | Purpose                              | Type     |
| ---------------------- | ------------------------------------ | -------- |
| `eventcore`            | Core library with traits and types   | Library  |
| `eventcore-postgres`   | PostgreSQL adapter implementation    | Library  |
| `eventcore-memory`     | In-memory adapter for testing        | Library  |
| `eventcore-examples`   | Example implementations and patterns | Examples |
| `eventcore-benchmarks` | Performance benchmarks               | Tools    |
| `eventcore-macros`     | Procedural macros for ergonomics     | Macros   |

## Version Synchronization

**All crates share the same version number** to ensure compatibility and avoid dependency hell.

### Benefits

- **Simplified dependency management**: Users specify one version for the entire ecosystem
- **Guaranteed compatibility**: All crates at the same version work together
- **Clear upgrade path**: Single version bump upgrades entire ecosystem
- **Reduced confusion**: No matrix of compatible versions to manage

### Example

```toml
[dependencies]
eventcore = "0.2.0"
eventcore-postgres = "0.2.0"  # Same version
eventcore-memory = "0.2.0"    # Same version
```

## Semantic Versioning Rules

### Major Version (X.0.0) - Breaking Changes

Breaking changes that require user code modifications:

#### Core Library (`eventcore`)

- Changes to public trait signatures (Command, EventStore, Projection)
- Removal of public types or methods
- Changes to error types and their variants
- Modifications to core event/stream structures
- Changes to serialization format

#### Database Adapters

- Database schema changes requiring migration
- Changes to configuration structures
- Removal of adapter methods

#### Macros

- Changes to macro syntax or generated code
- New required attributes or parameters

#### Examples

```rust
// v1.0.0 -> v2.0.0: Breaking change
// Before
impl Command for MyCommand {
    async fn handle(&self, state: Self::State, input: Self::Input) -> CommandResult<Vec<Event>>
}

// After - CommandStreams provides declarations; handle focuses on domain logic
impl CommandLogic for MyCommand {
    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError>
}
```

### Minor Version (0.X.0) - New Features

Backward-compatible additions:

#### New Features

- New optional trait methods with default implementations
- New configuration options with sensible defaults
- New crates added to workspace
- New macros or macro features
- Performance improvements without API changes
- New event store backends

#### Examples

```rust
// v1.1.0: New optional method
trait EventStore {
    // Existing methods...

    // New method with default implementation
    async fn get_stream_metadata(&self, stream_id: &StreamId) -> Result<StreamMetadata, EventStoreError> {
        Ok(StreamMetadata::default())
    }
}
```

### Patch Version (0.0.X) - Bug Fixes

Non-breaking fixes and improvements:

#### Bug Fixes

- Fixes to incorrect behavior
- Performance optimizations
- Documentation improvements
- Dependency updates (patch/minor only)
- Internal refactoring
- Test improvements

#### Examples

- Fixing race conditions in concurrent operations
- Correcting error messages
- Improving documentation examples
- Updating dependencies to patch versions

## Pre-release Versions

### Alpha (X.Y.Z-alpha.N)

- Early development phase
- APIs may change significantly
- Not recommended for production
- Breaking changes possible between alpha versions

### Beta (X.Y.Z-beta.N)

- Feature complete for the release
- API stabilization phase
- Limited breaking changes
- Suitable for early adopters and testing

### Release Candidate (X.Y.Z-rc.N)

- Final testing phase
- API frozen
- Only critical bug fixes
- Production readiness validation

### Example Release Timeline

```
1.0.0-alpha.1  -> API design and early implementation
1.0.0-alpha.2  -> Core features complete
1.0.0-beta.1   -> Feature complete, API stabilization
1.0.0-beta.2   -> Bug fixes and polish
1.0.0-rc.1     -> Final testing
1.0.0          -> Stable release
```

## Migration Strategy

### Breaking Changes

- **Migration Guide**: Detailed guide in CHANGELOG.md
- **Deprecation Period**: Mark old APIs as deprecated for one minor version
- **Code Examples**: Before/after examples for all breaking changes
- **Tooling**: Migration scripts where possible

### Database Schema Changes

- **Schema Migrations**: SQL scripts for database changes
- **Backward Compatibility**: Support reading old and new formats during transition
- **Migration Tools**: Command-line tools for data migration

## Release Process

### 1. Version Planning

- Determine version type based on changes
- Update all Cargo.toml files with new version
- Update CHANGELOG.md with release notes

### 2. Pre-release Testing

- All tests must pass on multiple Rust versions
- Integration tests with real databases
- Performance benchmarks within targets
- Security audit clean

### 3. Documentation

- Update all documentation
- Verify examples work with new version
- Update migration guides if needed

### 4. Release Execution

```bash
# 1. Update versions in workspace
cargo workspaces version X.Y.Z

# 2. Tag release
git tag -a vX.Y.Z -m "Release vX.Y.Z"

# 3. Publish crates in dependency order
cargo publish -p eventcore-macros
cargo publish -p eventcore
cargo publish -p eventcore-memory
cargo publish -p eventcore-postgres
# Note: examples and benchmarks are not published
```

### 5. Post-release

- Update documentation websites
- Announce on relevant channels
- Monitor for issues

## Compatibility Matrix

### Minimum Supported Rust Version (MSRV)

- **Current**: 1.70.0
- **Policy**: Update MSRV only on minor versions
- **Rationale**: Balance between new features and compatibility

### Database Compatibility

| EventCore Version | PostgreSQL    |
| ----------------- | ------------- |
| 0.1.x             | 15+           |
| 1.x.x             | 15+           |
| 2.x.x             | 16+ (planned) |

### Feature Compatibility

- **Feature flags**: Maintain backward compatibility within major versions
- **Optional dependencies**: Clearly documented breaking changes

## Version Constraints for Users

### Recommended Constraints

```toml
[dependencies]
# Exact version for maximum stability
eventcore = "=1.2.3"

# Compatible updates (patches)
eventcore = "~1.2.3"

# Compatible updates (minor versions)
eventcore = "^1.2.3"
```

### Development Dependencies

```toml
[dev-dependencies]
# More relaxed for examples and testing
eventcore-memory = "1.0"
```

## Breaking Change Policy

### Communication

- **Advance Notice**: Major breaking changes announced in advance
- **RFC Process**: Significant changes go through RFC process
- **Community Input**: Breaking changes discussed with community

### Timing

- **Regular Schedule**: Major versions on predictable schedule
- **Emergency Only**: Unplanned breaking changes only for security

### Deprecation Process

1. **Mark as deprecated** with clear migration path
2. **One minor version warning period** minimum
3. **Remove in next major version**

## Version Metadata

### Cargo.toml Metadata

```toml
[package]
version = "1.2.3"
rust-version = "1.70.0"  # MSRV

[package.metadata.docs.rs]
features = ["postgres", "memory"]
rustdoc-args = ["--cfg", "docsrs"]
```

### Build Metadata

- **Build date**: Embedded in release builds
- **Git commit**: SHA embedded for traceability
- **Feature flags**: Document enabled features

## Monitoring and Metrics

### Version Adoption

- Track download statistics by version
- Monitor issue reports by version
- User feedback on migration experience

### Quality Metrics

- Test coverage per version
- Performance regression detection
- Documentation completeness

## Emergency Procedures

### Security Vulnerabilities

- **Immediate patch**: Security fixes bypass normal process
- **All supported versions**: Backport to all maintained versions
- **Clear communication**: Security advisories with details

### Critical Bugs

- **Hotfix process**: Fast-track for critical production issues
- **Minimal changes**: Only fix the critical issue
- **Follow-up release**: Proper fix in next regular release

---

This versioning strategy ensures EventCore remains a reliable, predictable, and easy-to-use event sourcing library while allowing for innovation and growth.
