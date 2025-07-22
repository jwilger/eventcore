# EventCore Release Process

This document describes the release process for the EventCore workspace.

## Automated Release Process

The project uses a hybrid approach for fully automated releases:

1. **[release-plz](https://release-plz.dev/)** - Creates/updates release PRs when changes are pushed to `main`
2. **[cargo-publish-workspace](https://github.com/foresterre/cargo-publish-workspace)** - Publishes packages to crates.io in topological (dependency) order when release PRs are merged

This combination ensures reliable, automated releases without manual intervention.

## How It Works

### Package Publishing Order

cargo-publish-workspace automatically determines and uses the correct publishing order based on the dependency graph. It handles the topological sorting internally, ensuring packages are always published in the correct order.

**The dependency graph (automatically handled by cargo-publish-workspace):**
1. `eventcore-macros` (no internal dependencies)
2. `eventcore` (depends on eventcore-macros)
3. `eventcore-memory` (depends on eventcore)
4. `eventcore-postgres` (depends on eventcore and eventcore-memory)

The workflow includes a 30-second delay between publishing each crate to ensure crates.io has time to index each package before dependent crates are published.

### Fallback: Manual Publishing Process

In the unlikely event that the automated process fails, you can publish manually:

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

# 3. Create git tags for the release (if not already created)
# Note: release-plz usually creates these tags automatically
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