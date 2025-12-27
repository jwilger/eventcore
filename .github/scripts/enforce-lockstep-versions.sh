#!/usr/bin/env bash
# Enforces lockstep major/minor versioning across workspace crates
# Per ADR-025: All crates must share identical major.minor versions
# while patch versions may differ independently.
#
# This script:
# 1. Finds the highest major.minor version among all workspace crates
# 2. Updates all crates to that major.minor (preserving patch differences)
# 3. Validates the result matches the lockstep policy
#
# Usage: ./enforce-lockstep-versions.sh
# Exit codes: 0 = success, 1 = validation failure

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "🔍 Checking workspace version lockstep policy..."

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

# Extract all versions and find the highest major.minor
declare -A versions
declare -A major_minor_versions
max_major=0
max_minor=0

echo ""
echo "📦 Current crate versions:"
while IFS= read -r toml; do
    # Extract crate name and version using basic grep/sed
    crate_name=$(grep "^name = " "$toml" | head -1 | sed 's/name = "\(.*\)"/\1/')
    version=$(grep "^version = " "$toml" | head -1 | sed 's/version = "\(.*\)"/\1/')

    if [ -n "$crate_name" ] && [ -n "$version" ]; then
        versions["$crate_name"]="$version"

        # Parse version components (strip pre-release/build metadata for comparison)
        version_base="${version%%-*}"  # Strip pre-release (e.g., -alpha.1)
        version_base="${version_base%%+*}"  # Strip build metadata (e.g., +build.123)
        IFS='.' read -r major minor patch <<< "$version_base"
        major_minor="${major}.${minor}"
        major_minor_versions["$crate_name"]="$major_minor"

        echo "  $crate_name: $version (major.minor: $major_minor)"

        # Track highest major.minor
        if [ "$major" -gt "$max_major" ]; then
            max_major="$major"
            max_minor="$minor"
        elif [ "$major" -eq "$max_major" ] && [ "$minor" -gt "$max_minor" ]; then
            max_minor="$minor"
        fi
    fi
done <<< "$CRATE_TOMLS"

target_major_minor="${max_major}.${max_minor}"
echo ""
echo -e "${YELLOW}🎯 Target major.minor version: ${target_major_minor}${NC}"

# Check if all crates already match
all_match=true
for crate in "${!major_minor_versions[@]}"; do
    if [ "${major_minor_versions[$crate]}" != "$target_major_minor" ]; then
        all_match=false
        break
    fi
done

if [ "$all_match" = true ]; then
    echo -e "${GREEN}✅ All crates already have matching major.minor versions!${NC}"
    echo ""
    echo "No changes needed. Lockstep policy satisfied."
    exit 0
fi

# Need to fix versions
echo ""
echo -e "${YELLOW}⚠️  Version skew detected! Updating crates to match ${target_major_minor}.x${NC}"
echo ""

# Update each crate's major.minor while preserving patch version
while IFS= read -r toml; do
    crate_name=$(grep "^name = " "$toml" | head -1 | sed 's/name = "\(.*\)"/\1/')
    version=$(grep "^version = " "$toml" | head -1 | sed 's/version = "\(.*\)"/\1/')

    if [ -n "$crate_name" ] && [ -n "$version" ]; then
        # Parse version components (strip pre-release/build metadata)
        version_base="${version%%-*}"  # Strip pre-release
        version_base="${version_base%%+*}"  # Strip build metadata
        IFS='.' read -r major minor patch <<< "$version_base"
        current_major_minor="${major}.${minor}"

        if [ "$current_major_minor" != "$target_major_minor" ]; then
            new_version="${target_major_minor}.${patch}"
            echo "  📝 $crate_name: $version → $new_version"

            # Update version in Cargo.toml (first occurrence only, which is the package version)
            # Handle macOS/Linux sed differences
            if [[ "$OSTYPE" == "darwin"* ]]; then
                sed -i "" "0,/^version = \".*\"/s/^version = \".*\"/version = \"${new_version}\"/" "$toml"
            else
                sed -i "0,/^version = \".*\"/s/^version = \".*\"/version = \"${new_version}\"/" "$toml"
            fi
        fi
    fi
done <<< "$CRATE_TOMLS"

echo ""
echo -e "${GREEN}✅ Version lockstep enforcement complete!${NC}"
echo ""
echo "Updated versions:"
while IFS= read -r toml; do
    crate_name=$(grep "^name = " "$toml" | head -1 | sed 's/name = "\(.*\)"/\1/')
    version=$(grep "^version = " "$toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
    if [ -n "$crate_name" ] && [ -n "$version" ]; then
        echo "  $crate_name: $version"
    fi
done <<< "$CRATE_TOMLS"

echo ""
echo "🔐 Per ADR-025: All crates now share major.minor version ${target_major_minor}"
echo "   (patch versions may still differ independently for targeted bug fixes)"
