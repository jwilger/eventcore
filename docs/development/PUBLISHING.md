# EventCore Publishing Strategy

This document outlines the comprehensive publishing strategy for the EventCore workspace, including release processes, dependency management, and crate coordination.

## Table of Contents

- [Overview](#overview)
- [Crate Hierarchy](#crate-hierarchy)
- [Publishing Order](#publishing-order)
- [Version Alignment](#version-alignment)
- [Release Process](#release-process)
- [Quality Gates](#quality-gates)
- [Distribution Channels](#distribution-channels)
- [Documentation Publishing](#documentation-publishing)

## Overview

EventCore uses a **synchronized workspace publishing strategy** where all crates share the same version number and are released together. This approach ensures compatibility and simplifies dependency management for users.

### Key Principles

1. **Synchronized Versions**: All crates use the same version number
2. **Dependency Order**: Publish in dependency order to avoid failures
3. **Quality First**: Comprehensive testing before any release
4. **Rollback Ready**: Ability to yank and rollback releases
5. **Documentation Sync**: Keep documentation in sync with releases

## Crate Hierarchy

### Dependency Graph

```
eventcore-macros (no dependencies)
       ↑
   eventcore (depends on eventcore-macros)
       ↑
   ├── eventcore-memory (depends on eventcore)
   ├── eventcore-postgres (depends on eventcore)
   └── eventcore-benchmarks (dev tool, not published)
       ↑
   eventcore-examples (depends on eventcore + adapters, not published)
```

### Crate Categories

#### Published Libraries
- **`eventcore`**: Core library (foundational)
- **`eventcore-macros`**: Procedural macros (foundational)
- **`eventcore-memory`**: In-memory adapter (optional)
- **`eventcore-postgres`**: PostgreSQL adapter (optional)

#### Unpublished Crates
- **`eventcore-examples`**: Example code and tutorials
- **`eventcore-benchmarks`**: Performance benchmarks and profiling

### Publishing Matrix

| Crate | Published | Purpose | Dependencies |
|-------|-----------|---------|--------------|
| `eventcore-macros` | ✅ | Procedural macros | None |
| `eventcore` | ✅ | Core library | `eventcore-macros` |
| `eventcore-memory` | ✅ | Testing adapter | `eventcore` |
| `eventcore-postgres` | ✅ | Production adapter | `eventcore` |
| `eventcore-examples` | ❌ | Documentation | All above |
| `eventcore-benchmarks` | ❌ | Performance tools | All above |

## Publishing Order

### Critical Publishing Sequence

Publishing **must** follow this exact order to avoid dependency resolution failures:

```bash
1. eventcore-macros     # No dependencies
2. eventcore           # Depends on eventcore-macros
3. eventcore-memory    # Depends on eventcore
4. eventcore-postgres  # Depends on eventcore
```

### Automated Publishing Script

```bash
#!/bin/bash
# publish.sh - Automated publishing script

set -e

VERSION=${1:?Version required}
DRY_RUN=${2:-false}

echo "Publishing EventCore v$VERSION"

# Validate workspace state
echo "Validating workspace..."
cargo workspaces list --json | jq -r '.[].name' | while read crate; do
    echo "Checking $crate..."
    cargo check -p "$crate"
done

# Quality gates
echo "Running quality gates..."
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check --all

# Update versions
echo "Updating versions to $VERSION..."
if [ "$DRY_RUN" = "false" ]; then
    cargo workspaces version --exact "$VERSION" --no-git-commit
fi

# Publish in dependency order
CRATES=("eventcore-macros" "eventcore" "eventcore-memory" "eventcore-postgres")

for crate in "${CRATES[@]}"; do
    echo "Publishing $crate..."
    if [ "$DRY_RUN" = "true" ]; then
        cargo publish -p "$crate" --dry-run
    else
        cargo publish -p "$crate"
        
        # Wait for crate to be available
        echo "Waiting for $crate to be available on crates.io..."
        while ! cargo search "$crate" --limit 1 | grep -q "$crate"; do
            echo "Waiting..."
            sleep 10
        done
        echo "$crate is now available"
    fi
done

echo "Publishing complete!"
```

### Manual Publishing Process

For manual releases:

```bash
# 1. Prepare release
git checkout main
git pull origin main

# 2. Update versions
cargo workspaces version --exact 1.0.0 --no-git-commit

# 3. Publish macros first
cargo publish -p eventcore-macros

# 4. Wait for availability and publish core
sleep 60  # Allow crates.io indexing
cargo publish -p eventcore

# 5. Publish adapters
sleep 60
cargo publish -p eventcore-memory
cargo publish -p eventcore-postgres

# 6. Create git tag
git add .
git commit -m "Release v1.0.0"
git tag -a v1.0.0 -m "Release v1.0.0"
git push origin main --tags
```

## Version Alignment

### Workspace Configuration

```toml
# Cargo.toml (workspace root)
[workspace.package]
version = "1.0.0"  # Single source of truth

# Individual crates inherit this version
[package]
version.workspace = true
```

### Dependency Specifications

#### Internal Dependencies

```toml
# eventcore/Cargo.toml
[dependencies]
eventcore-macros = { version = "=1.0.0", path = "../eventcore-macros" }

# eventcore-postgres/Cargo.toml  
[dependencies]
eventcore = { version = "=1.0.0", path = "../eventcore" }
```

#### External Dependencies

```toml
# Use flexible versioning for external deps
[dependencies]
tokio = "1.45"      # Compatible minor versions
serde = "^1.0.219"  # Compatible updates
sqlx = "~0.8.6"     # Patch updates only
```

### Version Constraints Strategy

| Dependency Type | Constraint | Reasoning |
|----------------|------------|-----------|
| Workspace internal | `=1.0.0` | Exact version for perfect compatibility |
| Major external | `^1.45` | Compatible minor/patch updates |
| Stable external | `~1.0.219` | Patch updates only |
| Unstable external | `=0.8.6` | Exact version for pre-1.0 crates |

## Release Process

### Pre-Release Phase

#### 1. Planning and Preparation

```bash
# Update dependencies
cargo update --workspace

# Security audit
cargo audit

# Performance validation
cargo bench

# Documentation generation
cargo doc --workspace --no-deps
```

#### 2. Version Determination

Follow [semantic versioning rules](VERSIONING.md):

- **Patch (x.y.Z)**: Bug fixes, doc updates
- **Minor (x.Y.0)**: New features, backward compatible
- **Major (X.0.0)**: Breaking changes

#### 3. CHANGELOG Update

```markdown
## [1.0.0] - 2024-01-15

### Added
- New feature descriptions

### Changed  
- Breaking change descriptions

### Fixed
- Bug fix descriptions
```

### Release Execution

#### 1. Quality Gates

All quality gates must pass:

```bash
# Run comprehensive test suite
cargo nextest run --workspace --all-features

# Check formatting
cargo fmt --check --all

# Lint checks
cargo clippy --workspace --all-targets -- -D warnings

# Security audit
cargo audit

# Documentation build
cargo doc --workspace --no-deps --document-private-items

# Benchmark validation (optional)
cargo bench --workspace
```

#### 2. Version Updates

```bash
# Update all workspace versions
cargo workspaces version --exact 1.0.0 --no-git-commit

# Verify version consistency
grep -r "version.*=" Cargo.toml */Cargo.toml
```

#### 3. Publication

Execute the publishing script or manual process described above.

#### 4. Post-Release

```bash
# Create release tag
git add .
git commit -m "Release v1.0.0"
git tag -a v1.0.0 -m "Release v1.0.0"

# Push to repository
git push origin main --tags

# Update documentation sites
cargo doc --workspace --no-deps
# Deploy to GitHub Pages or docs.rs
```

### Emergency Releases

For critical security fixes or major bugs:

#### Hotfix Process

```bash
# Create hotfix branch
git checkout -b hotfix/v1.0.1

# Make minimal fix
# ... implement fix ...

# Fast-track testing
cargo test --workspace
cargo clippy --workspace

# Emergency publish
./publish.sh 1.0.1

# Merge back to main
git checkout main
git merge hotfix/v1.0.1
```

## Quality Gates

### Automated Gates (CI/CD)

```yaml
# .github/workflows/release.yml
name: Release
on:
  push:
    tags: ['v*']

jobs:
  quality-gates:
    runs-on: ubuntu-latest
    steps:
      - name: Test
        run: cargo nextest run --workspace
      
      - name: Clippy
        run: cargo clippy --workspace -- -D warnings
        
      - name: Format
        run: cargo fmt --check
        
      - name: Security Audit
        run: cargo audit
        
      - name: Documentation
        run: cargo doc --workspace --no-deps

  publish:
    needs: quality-gates
    runs-on: ubuntu-latest
    steps:
      - name: Publish to crates.io
        run: ./publish.sh ${{ github.ref_name }}
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
```

### Manual Quality Verification

#### Performance Validation

```bash
# Benchmark critical paths
cargo bench --workspace

# Memory usage analysis
cargo valgrind --bin example --

# Load testing
./scripts/load-test.sh
```

#### Integration Testing

```bash
# Real database testing
docker-compose up -d postgres
cargo test --package eventcore-postgres --features integration

# End-to-end scenarios
cargo run --package eventcore-examples --bin banking
cargo run --package eventcore-examples --bin ecommerce
```

## Distribution Channels

### Primary Distribution: crates.io

- **Official registry**: Main distribution channel
- **Automatic**: Published through `cargo publish`
- **Discovery**: Searchable and browsable
- **Documentation**: Auto-generated docs.rs integration

### Alternative Distribution

#### GitHub Releases

```bash
# Create GitHub release with assets
gh release create v1.0.0 \
  --title "EventCore v1.0.0" \
  --notes-file CHANGELOG.md \
  target/release/eventcore-migrate
```

#### Docker Images (Future)

```dockerfile
# Dockerfile for tools
FROM rust:alpine
COPY target/release/eventcore-migrate /usr/local/bin/
ENTRYPOINT ["eventcore-migrate"]
```

#### Package Managers (Future)

- **Homebrew**: For CLI tools
- **APT/YUM**: For Linux distributions  
- **Chocolatey**: For Windows

## Documentation Publishing

### docs.rs Integration

EventCore leverages docs.rs for automatic documentation hosting:

```toml
# Cargo.toml metadata for docs.rs
[package.metadata.docs.rs]
features = ["postgres", "memory"]
rustdoc-args = ["--cfg", "docsrs"]
```

### Custom Documentation

#### GitHub Pages

```yaml
# .github/workflows/docs.yml
name: Documentation
on:
  push:
    branches: [main]
    tags: ['v*']

jobs:
  docs:
    runs-on: ubuntu-latest
    steps:
      - name: Generate docs
        run: |
          cargo doc --workspace --no-deps
          echo '<meta http-equiv="refresh" content="0; url=eventcore">' > target/doc/index.html
          
      - name: Deploy to GitHub Pages
        uses: peaceiris/actions-gh-pages@v3
        with:
          github_token: ${{ secrets.GITHUB_TOKEN }}
          publish_dir: ./target/doc
```

#### Documentation Versioning

```bash
# docs/
├── latest/          # Current development
├── stable/          # Latest stable release
├── v1.0/           # Version-specific docs
└── v0.1/           # Legacy versions
```

### Tutorial and Example Publishing

```bash
# Publish interactive tutorials
mdbook build docs/tutorial
mdbook serve --open

# Publish example code
cargo run --package eventcore-examples --example banking
cargo run --package eventcore-examples --example ecommerce
```

## Rollback and Recovery

### Yanking Releases

```bash
# Yank a problematic release
cargo yank --vers 1.0.0 eventcore
cargo yank --vers 1.0.0 eventcore-postgres
cargo yank --vers 1.0.0 eventcore-memory
cargo yank --vers 1.0.0 eventcore-macros

# Un-yank if issue is resolved
cargo yank --vers 1.0.0 --undo eventcore
```

### Emergency Recovery

```bash
# Prepare emergency patch
git checkout v1.0.0
git checkout -b emergency/v1.0.1

# Implement critical fix
# ... make minimal changes ...

# Fast-track release
./publish.sh 1.0.1 --emergency

# Communicate to users
gh issue create --title "Security Advisory: v1.0.0" \
  --body "Please upgrade to v1.0.1 immediately"
```

## Monitoring and Analytics

### Release Metrics

- **Download statistics**: Track adoption rates
- **Issue reports**: Monitor post-release issues  
- **Performance**: Monitor regression reports
- **Documentation**: Track doc.rs page views

### Success Criteria

| Metric | Target | Measurement |
|--------|--------|-------------|
| Publish success rate | 100% | Automated publishing |
| Time to publish | < 30 minutes | End-to-end automation |
| Documentation coverage | 95% | Missing docs warnings |
| Zero critical post-release issues | 0 | Issue tracking |

---

This publishing strategy ensures reliable, predictable releases while maintaining high quality and user confidence in the EventCore ecosystem.