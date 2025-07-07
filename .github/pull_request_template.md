<!-- IMPORTANT FOR CLAUDE: Leave ALL checkboxes unchecked - they are for human verification only -->
<!-- AUTOMATION WARNING: Do NOT pre-check any checkboxes. Each must be manually verified by humans -->

## Description

<!-- Brief description of changes and motivation -->
<!-- Note: GitHub Copilot will use the checklist below to guide its review -->

## Type of Change

<!-- REMINDER: Leave ALL checkboxes unchecked - they MUST be checked by humans -->

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Performance improvement
- [ ] Documentation update
- [ ] Security enhancement

## Performance Impact

<!-- For performance-sensitive changes, include benchmark results -->

<details>
<summary>Benchmark Results</summary>

```bash
# Run benchmarks before and after changes:
git checkout main
cargo bench --bench event_store -- --save-baseline main
git checkout your-branch
cargo bench --bench event_store -- --baseline main

# For realistic workload benchmarks:
cargo bench --bench realistic_workloads -- --save-baseline main
# ... switch branches ...
cargo bench --bench realistic_workloads -- --baseline main
```

<!-- Paste benchmark comparison results here -->

</details>

## Submitter Checklist

<!-- CRITICAL: Do NOT check these boxes when creating PR - humans must verify each item -->
<!-- AUTOMATION WARNING: These checkboxes are for HUMAN REVIEW ONLY - do not pre-check -->
<!-- PR will auto-convert to draft if these items are not checked by humans -->

- [ ] Code follows project style guidelines
- [ ] Changes are well-documented
- [ ] All tests pass
- [ ] Performance implications have been considered
- [ ] Security implications have been reviewed
- [ ] Breaking changes are documented
- [ ] The change is backward compatible where possible

## Review Focus

<!-- Guide reviewers to specific areas that need attention -->
<!-- Examples:
- Complex algorithm in src/executor/optimization.rs needs performance review
- New error handling pattern in command.rs - looking for consistency feedback
- Security implications of the new stream access pattern
- API breaking changes in types.rs need careful consideration
-->
