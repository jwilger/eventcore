#!/usr/bin/env bash
# Validates that all workspace crates follow lockstep major/minor versioning
# Per ADR-025: All crates must share identical major.minor versions
#
# This is a validation-only script (used in CI checks).
# For enforcement/fixing, see enforce-lockstep-versions.sh
#
# Usage: ./validate-lockstep-versions.sh
# Exit codes: 0 = valid, 1 = version skew detected

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "🔍 Validating workspace version lockstep policy (ADR-025)..."

# Find all workspace crate Cargo.toml files (excluding workspace root)
CRATE_TOMLS=$(find . -name "Cargo.toml" \
    -not -path "./target/*" \
    -not -path "./.worktrees/*" \
    -not -path "./Cargo.toml" \
    -type f)

if [ -z "$CRATE_TOMLS" ]; then
    echo -e "${RED}❌ No crate Cargo.toml files found${NC}"
    exit 1
fi

# Extract all versions and check for major.minor consistency
declare -A major_minor_versions
reference_major_minor=""
reference_crate=""

echo ""
echo "📦 Crate versions:"
while IFS= read -r toml; do
    # Extract crate name and version
    crate_name=$(grep "^name = " "$toml" | head -1 | sed 's/name = "\(.*\)"/\1/')
    version=$(grep "^version = " "$toml" | head -1 | sed 's/version = "\(.*\)"/\1/')

    if [ -n "$crate_name" ] && [ -n "$version" ]; then
        # Parse version components
        IFS='.' read -r major minor patch <<< "$version"
        major_minor="${major}.${minor}"
        major_minor_versions["$crate_name"]="$major_minor"

        echo "  $crate_name: $version (major.minor: $major_minor)"

        # Set reference version from first crate
        if [ -z "$reference_major_minor" ]; then
            reference_major_minor="$major_minor"
            reference_crate="$crate_name"
        fi
    fi
done <<< "$CRATE_TOMLS"

echo ""
echo "🎯 Expected major.minor (from $reference_crate): $reference_major_minor"
echo ""

# Check all crates against reference
has_violations=false
for crate in "${!major_minor_versions[@]}"; do
    crate_major_minor="${major_minor_versions[$crate]}"
    if [ "$crate_major_minor" != "$reference_major_minor" ]; then
        echo -e "${RED}❌ VIOLATION: $crate has major.minor $crate_major_minor (expected $reference_major_minor)${NC}"
        has_violations=true
    fi
done

if [ "$has_violations" = true ]; then
    echo ""
    echo -e "${RED}💥 Version lockstep validation FAILED${NC}"
    echo ""
    echo "Per ADR-025, all workspace crates must share identical major.minor versions."
    echo "Patch versions may differ independently for targeted bug fixes."
    echo ""
    echo "To fix this:"
    echo "  1. Run: ./.github/scripts/enforce-lockstep-versions.sh"
    echo "  2. Review and commit the changes"
    echo ""
    exit 1
fi

echo -e "${GREEN}✅ Version lockstep validation PASSED${NC}"
echo ""
echo "All crates correctly share major.minor version $reference_major_minor"
echo "(patch versions may differ as per ADR-025 policy)"
exit 0
