# EventCore GitHub Workflows

## Release Process

EventCore uses an automated release process powered by [release-plz](https://release-plz.ieni.dev/):

### 1. Release PR Creation (`release-plz.yml`)
- **Trigger**: Every push to `main`
- **Actions**:
  - Checks for unreleased changes
  - Creates/updates a "Release PR" with:
    - Version bumps for all changed crates
    - Updated changelogs
    - Dependency updates
  - The PR is labeled with `release` and `automated`

### 2. Publishing to crates.io
- **Trigger**: When the Release PR is merged
- **Actions**:
  - release-plz detects the version changes
  - Publishes crates to crates.io in dependency order:
    1. `eventcore-macros`
    2. `eventcore`
    3. `eventcore-memory`
    4. `eventcore-postgres`
  - Creates a GitHub release with tag (e.g., `v0.1.2`)

### 3. Documentation Publishing (`release.yml`)
- **Trigger**: When release-plz creates a GitHub release
- **Actions**:
  - Validates the release
  - Builds and publishes documentation to GitHub Pages
  - Adds version information to the documentation

## Other Workflows

### CI (`ci.yml`)
- Runs tests, linting, and formatting checks on all PRs and pushes to main

### PR Validation (`pr-validation.yml`)
- Ensures PR descriptions follow the template
- Requires human verification of checklists

### Security (`cargo-audit.yml`)
- Runs security audits on dependencies

## Configuration Files

- `release-plz.toml`: Configuration for the release-plz tool
- `dependabot.yml`: Automated dependency updates