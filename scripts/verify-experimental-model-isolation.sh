#!/usr/bin/env bash
set -euo pipefail

# The checker dependency is deliberately absent from normal and runtime-only
# builds. Cargo.lock may still mention it; this verifies the resolved normal
# dependency graph instead of treating the lockfile as a deployed artifact.
if cargo tree -p eventcore --no-default-features -e normal | rg --quiet '^.*inventory v'; then
  echo "inventory leaked into the feature-disabled eventcore dependency graph" >&2
  exit 1
fi

if cargo tree -p eventcore --features experimental-modeling -e normal | rg --quiet '^.*inventory v'; then
  echo "inventory leaked into the runtime-only modeled dependency graph" >&2
  exit 1
fi

if ! cargo tree -p eventcore --features experimental-model-check -e normal | rg --quiet '^.*inventory v'; then
  echo "inventory is missing from the checker-enabled dependency graph" >&2
  exit 1
fi
