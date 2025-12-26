# EventCore Release Process

This document describes the release process for the EventCore workspace.

## Automated Release Process

The project uses **[release-plz](https://release-plz.dev/)** for fully automated releases with a two-phase workflow (see ADR-025):

**Phase 1: Release PR Creation** (`.github/workflows/release-plz.yml`)
- Triggered on push to `main` (except release PR merges)
- Analyzes commits using conventional commits for semver detection
- Creates/updates release PR with version bumps and changelog
- PR must pass all CI quality gates before merge

**Phase 2: Publication** (`.github/workflows/publish.yml`)
- Triggered when release PR is merged (commit message starts with "chore: release")
- Publishes crates to crates.io in dependency order
- Creates GitHub release with aggregated changelog
- Creates git tags for the release

This ensures reliable, automated releases without manual intervention while maintaining:

- Correct dependency ordering during publishing (release-plz)
- Retry logic with exponential backoff for transient failures (release-plz)
- Consistent changelog generation and GitHub releases (release-plz)
- Automated version management and PR creation (release-plz)
- Quality gates enforcement (CI must pass before release PR can merge)

## How It Works

### Package Publishing Order

cargo-workspaces automatically determines and uses the correct publishing order based on the dependency graph. It handles the topological sorting internally, ensuring packages are always published in the correct order.

**The dependency graph (automatically handled by cargo-workspaces):**

1. `eventcore-types` (no internal dependencies - shared types)
2. `eventcore-macros` (no internal dependencies - proc macros)
3. `eventcore` (depends on eventcore-types and eventcore-macros)
4. `eventcore-testing` (depends on eventcore-types - testing utilities)
5. `eventcore-memory` (depends on eventcore)
6. `eventcore-postgres` (depends on eventcore and eventcore-memory)

**Note:** `eventcore-types` and `eventcore-testing` are not yet published to crates.io. They will be published for the first time when the automated release workflow runs.

The workflow includes a 30-second delay between publishing each crate to ensure crates.io has time to index each package before dependent crates are published.

### Fallback: Manual Publishing Process

In the unlikely event that the automated process fails, you can publish manually:

```bash
# 1. First, ensure you're on the latest main branch
git checkout main
git pull origin main

# 2. Publish packages in dependency order
cd eventcore-types
cargo publish
cd ..
# Wait for crates.io to index the package (~1-2 minutes)

cd eventcore-macros
cargo publish
cd ..
# Wait for crates.io to index the package

cd eventcore
cargo publish
cd ..
# Wait for crates.io to index the package

cd eventcore-testing
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
git tag -a eventcore-v0.X.Y -m "Release eventcore v0.X.Y"
git tag -a eventcore-types-v0.X.Y -m "Release eventcore-types v0.X.Y"
git tag -a eventcore-macros-v0.X.Y -m "Release eventcore-macros v0.X.Y"
git tag -a eventcore-testing-v0.X.Y -m "Release eventcore-testing v0.X.Y"
git tag -a eventcore-memory-v0.X.Y -m "Release eventcore-memory v0.X.Y"
git tag -a eventcore-postgres-v0.X.Y -m "Release eventcore-postgres v0.X.Y"

# 4. Push tags to GitHub
git push origin --tags
```

Replace `0.X.Y` with the actual version number.

## Workflow Configuration

The release workflow is split into two parts:

1. **Release PR Creation**: Runs on push to main when it's NOT a release PR merge
2. **Package Publishing**: Runs on push to main when it IS a release PR merge (commit message starts with "chore: release")

This separation prevents manual version bumps from triggering immediate publishing.

## Credentials and Permissions

### crates.io Publishing Token

The automated workflow uses a `CARGO_REGISTRY_TOKEN` stored as a GitHub Actions secret to authenticate with crates.io during publishing.

**Required permissions:**
- Publish access to all EventCore workspace crates
- Owner or team member with publish rights

**Crate ownership verification:**

```bash
# Check ownership for each crate (requires authentication)
cargo owner --list eventcore
cargo owner --list eventcore-macros
cargo owner --list eventcore-memory
cargo owner --list eventcore-postgres
cargo owner --list eventcore-types
cargo owner --list eventcore-testing
```

### Credential Rotation

To rotate the crates.io publishing token:

1. **Generate new token on crates.io:**
   - Log in to [crates.io](https://crates.io/)
   - Navigate to Account Settings → API Tokens
   - Click "New Token"
   - Name: `EventCore GitHub Actions` (or similar)
   - Scopes: `publish-update` (allows publishing new versions)
   - Click "Generate"
   - Copy the token immediately (it won't be shown again)

2. **Update GitHub Actions secret:**
   - Navigate to repository Settings → Secrets and variables → Actions
   - Find `CARGO_REGISTRY_TOKEN` in repository secrets
   - Click "Update" and paste the new token
   - Save changes

3. **Revoke old token:**
   - Return to crates.io Account Settings → API Tokens
   - Find the old token
   - Click "Revoke"
   - Confirm revocation

4. **Verify new token works:**
   - Trigger a test release or wait for the next automated release
   - Monitor the GitHub Actions workflow logs
   - Verify successful authentication and publishing

**Recommended rotation schedule:**
- Rotate tokens annually
- Rotate immediately if token is compromised or exposed
- Rotate when team members with token access leave the project

## Troubleshooting

### "Package already published" errors

If you see errors about packages already being published, check:

1. Which packages have actually been published to crates.io
2. Continue manual publishing from the next package in the dependency order

### Version mismatch errors

If you see errors about version requirements not being met:

1. Ensure all workspace dependencies in `Cargo.toml` use the same version
2. Check that the versions match what's in the package's `Cargo.toml`

### Authentication errors during publishing

If you see "no token found" or "authentication failed" errors:

1. Verify `CARGO_REGISTRY_TOKEN` is set in GitHub Actions secrets
2. Check that the token hasn't expired or been revoked on crates.io
3. Verify the token has `publish-update` permissions
4. Rotate the token following the credential rotation procedure above

### Pre-commit hook failures

Always ensure pre-commit hooks pass before attempting to publish. The hooks run:

- Code formatting
- Linting
- Tests
- Type checking
