#!/usr/bin/env bash
# Check for #[allow(dead_code)] annotations in Rust source files
# These are TDD markers and must be removed before commit

if grep -rn "#\[allow(dead_code)\]" --include="*.rs" src/ tests/ 2>/dev/null; then
    echo ""
    echo "ERROR: Found #[allow(dead_code)] annotations"
    echo "These are TDD markers and must be removed before commit"
    exit 1
fi

exit 0
