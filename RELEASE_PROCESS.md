# EventCore Release Process

This document describes the release process for the EventCore workspace.

## Automated Release Process

The project uses [release-plz](https://release-plz.dev/) for automated releases:

1. When changes are pushed to `main`, release-plz creates/updates a release PR
2. When the release PR is merged, release-plz publishes packages to crates.io

## Known Issues

### Package Publishing Order

release-plz has a known issue where it doesn't always respect the dependency graph when publishing workspace packages. This can cause publishing failures when packages are published out of order.

**Correct publishing order based on dependencies:**
1. `eventcore-macros` (no internal dependencies)
2. `eventcore` (depends on eventcore-macros)
3. `eventcore-memory` (depends on eventcore)
4. `eventcore-postgres` (depends on eventcore and eventcore-memory)

### Manual Publishing Process

If release-plz fails to publish packages in the correct order, you'll need to publish manually:

```bash
# 1. First, ensure you're on the latest main branch
git checkout main
git pull origin main

# 2. Publish packages in the correct order
cd eventcore-macros
cargo publish
cd ..
# Wait for crates.io to index the package (~1-2 minutes)

cd eventcore
cargo publish
cd ..
# Wait for crates.io to index the package

cd eventcore-memory
cargo publish
cd ..
# Wait for crates.io to index the package

cd eventcore-postgres
cargo publish
cd ..

# 3. Create git tags for the release
git tag -a eventcore-v0.1.X -m "Release eventcore v0.1.X"
git tag -a eventcore-macros-v0.1.X -m "Release eventcore-macros v0.1.X"
git tag -a eventcore-memory-v0.1.X -m "Release eventcore-memory v0.1.X"
git tag -a eventcore-postgres-v0.1.X -m "Release eventcore-postgres v0.1.X"

# 4. Push tags to GitHub
git push origin --tags
```

Replace `0.1.X` with the actual version number.

## Workflow Configuration

The release workflow is split into two parts:

1. **Release PR Creation**: Runs on push to main when it's NOT a release PR merge
2. **Package Publishing**: Runs on push to main when it IS a release PR merge (commit message starts with "chore: release")

This separation prevents manual version bumps from triggering immediate publishing.

## Troubleshooting

### "Package already published" errors

If you see errors about packages already being published, check:
1. Which packages have actually been published to crates.io
2. Continue manual publishing from the next package in the dependency order

### Version mismatch errors

If you see errors about version requirements not being met:
1. Ensure all workspace dependencies in `Cargo.toml` use the same version
2. Check that the versions match what's in the package's `Cargo.toml`

### Pre-commit hook failures

Always ensure pre-commit hooks pass before attempting to publish. The hooks run:
- Code formatting
- Linting
- Tests
- Type checking