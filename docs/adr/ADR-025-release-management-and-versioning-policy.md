# ADR-025: Release Management and Versioning Policy

## Status

accepted

## Context

EventCore is a Rust workspace with six published crates distributed via crates.io:

```
eventcore/                    (main entry point - facade with re-exports)
eventcore-types/              (shared vocabulary - traits and types)
eventcore-macros/             (proc-macros for derive(Command))
eventcore-postgres/           (PostgreSQL adapter implementation)
eventcore-testing/            (test utilities and contract test suite)
eventcore-memory/             (in-memory storage for testing)
```

These crates have complex interdependencies defined by ADR-022's feature flag architecture:

```
eventcore → eventcore-types (always)
eventcore → eventcore-postgres (via feature flag)
eventcore → eventcore-macros (via feature flag)
eventcore-postgres → eventcore-types
eventcore-testing → eventcore-types
eventcore-memory → eventcore-types
```

**Key Forces:**

1. **User Mental Model**: Consumers expect `eventcore = "0.2"` and `eventcore-postgres = "0.2"` to be compatible - mismatched versions create confusion and support burden
2. **crates.io Immutability**: Once published, crate versions are permanent and cannot be unpublished (only yanked, which breaks existing builds)
3. **Dependency Order Constraints**: Must publish eventcore-types before eventcore-postgres, and eventcore-postgres before eventcore, or publication fails with missing dependency errors
4. **Partial Failure Risk**: If publication fails mid-release (network errors, crates.io downtime, credential issues), some crates may be published while others remain at old versions, creating version skew
5. **Maintainer Cognitive Load**: Manual versioning across six Cargo.toml files is error-prone - easy to forget a crate or introduce version mismatches
6. **Patch Independence Need**: A bug fix in eventcore-postgres shouldn't require releasing eventcore 0.2.1 when eventcore code hasn't changed
7. **Quality Gate Integration**: Release workflow must respect existing CI gates (nextest, clippy, format, mutation@80%, security-audit) to prevent publishing broken code
8. **Two-Phase Review Need**: Maintainers want to preview version bumps and changelog before committing to publication (crates.io releases are irreversible)
9. **GitHub Release Expectations**: Rust ecosystem conventions include GitHub releases with change summaries linking to crates.io
10. **Nix Development Environment**: Pinned toolchains in `nix develop` mean release tooling must work within Nix shell constraints

**Current State:**

- All workspace crates manually synchronized to version 0.2.0 (as of 2025-12-23)
- No automation exists for version bumping or publication
- Releases performed manually via `cargo publish` with dependency-order awareness
- No changelog generation or release note automation
- Version skew previously occurred (eventcore 0.1.8 while dependencies at 0.1.0)

**Why This Decision Now:**

The workspace has reached six published crates with active development. Manual versioning has already caused version skew issues. Before the next release, we must establish automation to prevent human error and reduce maintainer burden.

## Decision

Implement automated release management using release-plz with the following policy:

### 1. Full Lockstep Versioning (Updated 2025-12-27)

All workspace crates SHALL maintain identical major, minor, AND patch version numbers using Cargo workspace version inheritance.

**Implementation:**
- Workspace `Cargo.toml` defines `[workspace.package]` with shared `version`
- All crates use `version.workspace = true` to inherit from workspace
- release-plz bumps workspace version, all crates get identical version automatically

**Examples:**
- ✅ Allowed: All crates at 0.3.0 (identical versions)
- ✅ Allowed: All crates at 0.2.1 after patch in any crate
- ❌ Forbidden: eventcore 0.3.0, eventcore-types 0.2.5 (version skew)
- ❌ Forbidden: Mixed patch versions (0.2.1 and 0.2.0)

**Rationale for Change:**
Initial ADR proposed lockstep major/minor with independent patches. Implementation revealed this required 234 lines of custom bash scripts and complex workflow orchestration to enforce, fighting against release-plz's natural behavior. Full lockstep via workspace inheritance is native Cargo functionality requiring zero custom code. The complexity cost outweighs the benefit of independent patches.

### 2. Two-Phase Publication Workflow

**Phase 1: Release PR Preview (on push to main)**
- release-plz analyzes commits since last release (conventional commits)
- Creates/updates release PR with:
  - Version bumps in all affected Cargo.toml files
  - Generated changelog entries per crate
  - Preview of what will be published
- PR must pass ALL CI quality gates before merge eligibility

**Phase 2: Publish to crates.io (on release PR merge)**
- release-plz publishes crates in dependency order:
  1. eventcore-types (no EventCore dependencies)
  2. eventcore-macros, eventcore-postgres, eventcore-testing, eventcore-memory (depend on eventcore-types)
  3. eventcore (depends on all via feature flags)
- Creates GitHub release with combined changelog
- Tags repository with release version

### 3. Tooling: release-plz

Use release-plz for automation because:
- Native Rust workspace support with dependency-order publication
- Conventional commit analysis for semver bump detection
- PR-based preview workflow (review before publish)
- Built-in retry logic for transient failures
- Active maintenance and Rust ecosystem adoption

### 4. Dependency Order Publication

Publication SHALL follow dependency graph order to prevent "missing dependency version" errors:

```
Step 1: eventcore-types
  ↓
Step 2: eventcore-macros, eventcore-postgres, eventcore-testing, eventcore-memory (parallel)
  ↓
Step 3: eventcore
```

### 5. Failure Handling

**Partial Publish Failures:**
- release-plz retries individual crate publication failures with exponential backoff (3 attempts)
- If retry exhausted, workflow fails but already-published crates remain
- Maintainer resolves issue (credential refresh, crates.io outage wait) and re-runs workflow
- Re-run skips already-published versions (idempotent)

**Rollback Policy:**
- crates.io does NOT support unpublish (only yank)
- If broken version published, publish patched version immediately
- Yank broken version to prevent new users from installing it
- Document incident in CHANGELOG.md

### 6. Quality Gates Integration

Release PR merge is BLOCKED unless ALL CI checks pass:
- `cargo nextest run --workspace` (all tests green)
- `cargo clippy --all-targets --all-features -- -D warnings` (no lint violations)
- `cargo fmt --all -- --check` (formatting consistent)
- Mutation testing ≥80% (cargo-mutants)
- `cargo audit` (no security advisories in dependencies)

## Rationale

### Why Lockstep Major/Minor Versioning?

**Problem Prevented:**
Without lockstep versioning, users face compatibility confusion:
- `eventcore = "0.3.0"` with `eventcore-postgres = "0.2.5"` - will this work?
- Breaking change in eventcore-types affects all crates, but only eventcore-types bumps to 0.3.0
- Support burden: "Which combinations are compatible?"

**Trade-offs Accepted:**
- eventcore 0.3.0 may be published even if only eventcore-types changed (user sees "eventcore updated" but no eventcore code changed)
- All crates bump major/minor together even if some unchanged

**Why Worth It:**
- Clear compatibility guarantee: matching major/minor = compatible
- Reduces cognitive load for users (don't need dependency matrix)
- Aligns with user expectation from existing documentation (VERSIONING.md assumes lockstep)
- Industry precedent: tokio crates use lockstep versioning for same reasons

### Why Full Lockstep? (Updated 2025-12-27)

**Problem Solved:**
Workspace version inheritance eliminates all custom enforcement complexity while providing identical compatibility guarantees.

**Implementation Reality:**
- Independent patches required 234 lines of custom bash scripts
- Complex workflow orchestration (modify PR branch post-creation)
- Fighting against release-plz's natural behavior
- Maintenance burden outweighed benefit

**Benefits of Full Lockstep:**
- Zero custom code (native Cargo workspace feature)
- Single source of truth (workspace Cargo.toml)
- release-plz works naturally without intervention
- Simpler release workflow (62 lines vs 121 lines)
- No post-processing of PRs needed
- PR descriptions accurate immediately

**Trade-off Accepted:**
- All crates get new version even if only one changed
- "Unnecessary" releases (e.g., eventcore 0.2.1 when only eventcore-postgres changed)
- Changelog may show "no changes" for some crates

**Why Trade-off is Worth It:**
- Reduced complexity = fewer bugs in release automation
- Maintainer time saved >> minor version number inflation
- Users already expect lockstep (mental model unchanged)
- Industry precedent: many Rust workspaces use full lockstep for simplicity

### Why Two-Phase Workflow (PR Preview + Publish)?

**Problem Prevented:**
Direct publish on every main commit would:
- Publish broken code if CI intermittently fails
- No human review of version bumps before irreversible crates.io publication
- No opportunity to fix changelog wording before release

**Trade-offs Accepted:**
- Extra step (merge release PR) adds latency between code merge and crates.io availability
- Release PR creates extra noise in PR list

**Why Worth It:**
- Preview changes before irreversible publication (crates.io doesn't allow unpublish)
- Quality gates enforced (release PR must pass CI)
- Matches existing culture (project has pre-commit hooks, emphasizes quality gates)
- Maintainer control: can edit changelog, adjust version bumps if semver detection wrong

### Why release-plz Over Alternatives?

**Alternatives Considered:**

1. **Manual cargo publish** - Current state, error-prone, already caused version skew
2. **cargo-workspaces** - Requires manual version bumping, no conventional commit analysis
3. **release-please** (Google) - Node.js tooling, less Rust ecosystem integration
4. **semantic-release** - Complex plugin system, no native workspace support

**Why release-plz:**
- Rust-native (compiles in Nix environment)
- Native workspace dependency-order publication
- Conventional commit semver detection (automates version decision)
- PR-based preview workflow (matches maintainer preference)
- Active maintenance and ecosystem adoption

### Why Dependency Order Publication?

**Technical Requirement:**
crates.io validates dependencies at publish time. If we publish:
1. eventcore first (depends on eventcore-postgres 0.2.1)
2. eventcore-postgres not yet published at 0.2.1
3. Publish fails: "dependency eventcore-postgres 0.2.1 not found"

**Solution:**
Publish bottom-up (dependencies before dependents):
- eventcore-types first (depends on nothing in workspace)
- Adapters second (depend only on eventcore-types)
- eventcore last (depends on adapters via feature flags)

### Why Retry Logic with Exponential Backoff?

**Problem Solved:**
Transient failures (network hiccups, crates.io temporary overload) shouldn't require manual maintainer intervention.

**Implementation:**
- retry up to 3 times per crate
- exponential backoff: 5s, 10s, 20s delays
- If still fails, workflow fails but published crates remain

**Why Worth It:**
- Reduces false-failure rate (transient network issues common)
- Avoids partial publish state from temporary issues
- Maintainer only intervenes for real issues (credentials expired, crates.io outage)

## Consequences

### Positive

- **Reduced Human Error**: Automation prevents version skew and forgotten Cargo.toml updates
- **Clear Compatibility Guarantee**: Matching major/minor versions = compatible crates
- **Faster Patch Releases**: Independent patch versions avoid unnecessary releases
- **Quality Assurance**: Release PR must pass CI before merge (prevents broken releases)
- **Maintainer Control**: Preview version bumps and changelog before irreversible publication
- **Idempotent Recovery**: Re-running publish workflow after partial failure is safe
- **Ecosystem Alignment**: GitHub releases + crates.io publication matches Rust conventions
- **Audit Trail**: Release PRs document what changed and why versions bumped

### Negative

- **Version Bump Noise**: eventcore may bump to 0.3.0 even if only eventcore-types changed (users see "update" but no eventcore code changes)
- **Extra Complexity**: CI must run release-plz, manage CARGO_REGISTRY_TOKEN secret
- **Release Latency**: Two-phase workflow adds delay between code merge and crates.io availability
- **Conventional Commit Discipline**: Requires maintainers to write correct commit message format (feat:, fix:, breaking:) or semver detection fails
- **Tooling Dependency**: Reliance on release-plz maintenance and compatibility
- **Partial Publish Recovery**: Maintainer must understand how to re-run workflow after partial failure

### Enabled Future Decisions

- Changelog automation can generate consolidated release notes from conventional commits
- GitHub Actions can auto-publish on release PR merge without manual cargo publish
- Version skew detection can fail CI if manual Cargo.toml edits violate lockstep policy
- Dependency update automation (dependabot) can integrate with release workflow
- Release cadence can be regularized (weekly, biweekly) with predictable automation

### Constrained Future Decisions

- Workspace must continue using conventional commits or release-plz won't detect semver bumps
- Breaking changes in eventcore-types force major/minor bump across ALL workspace crates
- Cannot remove lockstep versioning without breaking user mental model and documentation
- Must maintain dependency-order publication constraint (can't publish eventcore before eventcore-types)
- Nix environment must continue supporting release-plz (cargo install compatibility)

## Alternatives Considered

### Alternative 1: Fully Independent Versioning

Allow each crate to version independently (eventcore 0.5.0, eventcore-types 0.3.2, eventcore-postgres 0.4.1).

**Why Rejected:**
- **User Confusion**: Which versions are compatible? Requires dependency matrix documentation
- **Support Burden**: "Does eventcore 0.5.0 work with eventcore-postgres 0.4.1?" - maintainer must track
- **Mental Model Violation**: Existing VERSIONING.md and user expectations assume lockstep
- **Breaking Change Communication**: Hard to signal "this eventcore-types breaking change affects all crates" when only eventcore-types bumps major version

### Alternative 2: Full Lockstep (Including Patch)

All crates always maintain identical major.minor.patch versions.

**Why Rejected:**
- **Unnecessary Releases**: Bug fix in eventcore-postgres forces eventcore 0.2.1 even if eventcore unchanged
- **Changelog Noise**: eventcore 0.2.1 changelog says "no changes" or repeats eventcore-postgres changes
- **Slower Patch Turnaround**: Must coordinate workspace-wide release for single crate bug fix
- **Industry Anti-Pattern**: Most workspace projects (tokio, serde) allow independent patches

### Alternative 3: Immediate Publish on Main Commit

Publish to crates.io automatically on every commit to main, no release PR preview.

**Why Rejected:**
- **No Review Gate**: Irreversible publication before human review of version bumps/changelog
- **CI False Positives**: Intermittent CI failures could publish broken code
- **No Changelog Editing**: Automatically generated changelogs may need maintainer refinement
- **Crates.io Irreversibility**: Cannot unpublish, only yank (breaks existing builds)

### Alternative 4: Manual Versioning with Automated Publish

Maintainer manually edits Cargo.toml versions, automation only handles publication.

**Why Rejected:**
- **Human Error**: Manual version bumping already caused version skew (eventcore 0.1.8, others 0.1.0)
- **Maintainer Burden**: Must remember to update six Cargo.toml files correctly
- **Semver Mistakes**: Easy to forget breaking change requires major bump, not minor
- **No Changelog Automation**: Manually written changelogs often incomplete or inconsistent

### Alternative 5: Monorepo with Single Published Crate

Collapse all workspace crates into single eventcore crate with feature flags.

**Why Rejected:**
- **Violates ADR-022**: Feature flag architecture requires separate crates to avoid circular dependencies
- **Violates ADR-011**: Heavy dependencies (PostgreSQL) must remain isolated from main crate
- **Compilation Bloat**: All users pay compile cost for all adapters even if unused
- **Ecosystem Anti-Pattern**: Rust convention separates heavy dependencies into optional crates

### Alternative 6: Git Tags for Versioning, No crates.io

Use Git tags for releases, distribute via GitHub, skip crates.io publication.

**Why Rejected:**
- **Ecosystem Friction**: Rust users expect `cargo add eventcore`, not Git dependency syntax
- **No Semver Resolution**: Cargo's semver resolver doesn't work with Git dependencies
- **Update Discovery**: Users can't find new versions via `cargo update` or dependabot
- **Project Goal Violation**: EventCore explicitly targets crates.io distribution for ecosystem reach

## References

- ADR-022: Crate Reorganization for Feature Flag-Based Adapter Re-exports (defines workspace structure and dependency graph)
- ADR-015: eventcore-testing Crate Scope and Publication (establishes version synchronization expectation)
- ADR-011: In-Memory Event Store Crate Location (principle: separate crates for heavy dependencies)
- GitHub Actions Rust CI/CD Best Practices (stored in memento knowledge graph)
- EventCore Automated Release Planning 2025-12-23 (memento: planning session identifying this ADR as blocker)
- Conventional Commits Specification: https://www.conventionalcommits.org/
- release-plz Documentation: https://release-plz.dev/
- crates.io Publishing Policy: https://doc.rust-lang.org/cargo/reference/publishing.html
