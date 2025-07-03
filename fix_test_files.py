#!/usr/bin/env python3
"""
Script to fix test files for the new Command API migration.
"""

import os
import re
import sys
from pathlib import Path

def fix_test_file(file_path):
    """Fix a single test file for the new command API."""
    print(f"Fixing {file_path}")
    
    with open(file_path, 'r') as f:
        content = f.read()
    
    original_content = content
    
    # Add missing imports for CommandStreams and CommandLogic if not present
    if 'CommandStreams' not in content and 'impl Command for' in content:
        # Find the imports section and add the traits
        if 'use eventcore::{' in content:
            content = re.sub(
                r'use eventcore::\{([^}]+)\};',
                lambda m: f'use eventcore::{{{m.group(1)}, CommandLogic, CommandStreams}};',
                content
            )
        elif 'use eventcore::' in content:
            # Add after existing eventcore imports
            content = re.sub(
                r'(use eventcore::[^;]+;)',
                r'\1\nuse eventcore::{CommandLogic, CommandStreams};',
                content,
                count=1
            )
    
    # Fix execute() calls with 3 parameters to 2 parameters
    # Pattern: .execute(command, input, options) -> .execute(command, options)
    content = re.sub(
        r'\.execute\(\s*([^,]+),\s*[^,]+Input\s*\{[^}]+\},\s*([^)]+)\)',
        r'.execute(\1, \2)',
        content,
        flags=re.MULTILINE | re.DOTALL
    )
    
    # Fix execute() calls with &Command and separate input
    content = re.sub(
        r'\.execute\(\s*&([^,]+),\s*([^,]+Input\s*\{[^}]+\}),\s*([^)]+)\)',
        r'.execute(&\1 { /* fields from \2 */ }, \3)',
        content,
        flags=re.MULTILINE | re.DOTALL
    )
    
    # Remove Input type definitions
    content = re.sub(
        r'type Input = [^;]+;\s*',
        '',
        content
    )
    
    # Convert old Command trait to new pattern
    content = re.sub(
        r'impl Command for (\w+) \{([^}]*?)type StreamSet = ([^;]+);([^}]*?)fn read_streams\(&self, input: &Self::Input\) -> Vec<StreamId> \{([^}]*?)\}([^}]*?)fn apply\(&self, state: &mut Self::State, ([^}]*?)\}([^}]*?)async fn handle\(',
        r'impl CommandStreams for \1 {\n    type StreamSet = \3;\n\n    fn read_streams(&self) -> Vec<StreamId> {\5}\n}\n\n#[async_trait::async_trait]\nimpl CommandLogic for \1 {\8\n\n    async fn handle(',
        content,
        flags=re.MULTILINE | re.DOTALL
    )
    
    if content != original_content:
        with open(file_path, 'w') as f:
            f.write(content)
        print(f"  Modified {file_path}")
        return True
    else:
        print(f"  No changes needed in {file_path}")
        return False

def main():
    """Main function to fix all test files."""
    eventcore_root = Path(__file__).parent
    
    # Find all test files that might need fixing
    test_files = []
    
    # Look in eventcore/tests/
    eventcore_tests = eventcore_root / "eventcore" / "tests"
    if eventcore_tests.exists():
        test_files.extend(eventcore_tests.glob("*.rs"))
    
    # Look in eventcore-postgres/tests/
    postgres_tests = eventcore_root / "eventcore-postgres" / "tests"
    if postgres_tests.exists():
        test_files.extend(postgres_tests.glob("*.rs"))
    
    # Look in eventcore-examples/tests/
    examples_tests = eventcore_root / "eventcore-examples" / "tests"
    if examples_tests.exists():
        test_files.extend(examples_tests.glob("*.rs"))
    
    modified_count = 0
    for test_file in test_files:
        if fix_test_file(test_file):
            modified_count += 1
    
    print(f"\nFixed {modified_count} out of {len(test_files)} files.")

if __name__ == "__main__":
    main()