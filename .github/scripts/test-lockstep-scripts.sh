#!/usr/bin/env bash
# Integration test for lockstep version enforcement and validation scripts
# This test simulates version skew and verifies the enforcement script fixes it

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "🧪 Testing lockstep version scripts..."
echo ""

# Store original versions for restoration
ORIGINAL_EVENTCORE_VERSION=$(grep "^version = " eventcore/Cargo.toml | head -1)
ORIGINAL_TYPES_VERSION=$(grep "^version = " eventcore-types/Cargo.toml | head -1)

cleanup() {
    echo ""
    echo "🔄 Restoring original versions..."
    sed -i "0,/^version = \".*\"/s|^version = \".*\"|$ORIGINAL_EVENTCORE_VERSION|" eventcore/Cargo.toml
    sed -i "0,/^version = \".*\"/s|^version = \".*\"|$ORIGINAL_TYPES_VERSION|" eventcore-types/Cargo.toml
}

trap cleanup EXIT

# Test 1: Validation should pass on current state
echo "Test 1: Validation on consistent versions"
if ./.github/scripts/validate-lockstep-versions.sh > /dev/null 2>&1; then
    echo -e "${GREEN}✅ PASS: Validation passes on consistent versions${NC}"
else
    echo -e "${RED}❌ FAIL: Validation should pass on consistent versions${NC}"
    exit 1
fi
echo ""

# Test 2: Create version skew
echo "Test 2: Simulating version skew..."
echo "  Original eventcore version: $ORIGINAL_EVENTCORE_VERSION"
echo "  Modifying eventcore to 0.3.0..."
sed -i "0,/^version = \".*\"/s|^version = \".*\"|version = \"0.3.0\"|" eventcore/Cargo.toml

# Verify validation detects the skew
echo "  Testing validation detects skew..."
if ./.github/scripts/validate-lockstep-versions.sh > /dev/null 2>&1; then
    echo -e "${RED}❌ FAIL: Validation should detect version skew${NC}"
    exit 1
else
    echo -e "${GREEN}✅ PASS: Validation correctly detects version skew${NC}"
fi
echo ""

# Test 3: Enforcement script should fix the skew
echo "Test 3: Enforcing lockstep versions..."
./.github/scripts/enforce-lockstep-versions.sh

# Verify all crates are now at 0.3.x
EVENTCORE_VERSION=$(grep "^version = " eventcore/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
TYPES_VERSION=$(grep "^version = " eventcore-types/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')

IFS='.' read -r eventcore_major eventcore_minor eventcore_patch <<< "$EVENTCORE_VERSION"
IFS='.' read -r types_major types_minor types_patch <<< "$TYPES_VERSION"

if [ "$eventcore_major" = "0" ] && [ "$eventcore_minor" = "3" ] && \
   [ "$types_major" = "0" ] && [ "$types_minor" = "3" ]; then
    echo -e "${GREEN}✅ PASS: Enforcement script correctly updated all crates to 0.3.x${NC}"
else
    echo -e "${RED}❌ FAIL: Enforcement script did not update versions correctly${NC}"
    echo "  eventcore: $EVENTCORE_VERSION"
    echo "  eventcore-types: $TYPES_VERSION"
    exit 1
fi
echo ""

# Test 4: Validation should now pass
echo "Test 4: Validation after enforcement"
if ./.github/scripts/validate-lockstep-versions.sh > /dev/null 2>&1; then
    echo -e "${GREEN}✅ PASS: Validation passes after enforcement${NC}"
else
    echo -e "${RED}❌ FAIL: Validation should pass after enforcement${NC}"
    exit 1
fi
echo ""

echo -e "${GREEN}🎉 All tests passed!${NC}"
echo ""
echo "The lockstep version scripts are working correctly:"
echo "  ✅ Validation detects version skew"
echo "  ✅ Enforcement fixes version skew"
echo "  ✅ Validation passes after enforcement"
