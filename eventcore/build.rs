// Enforce absence of lint-suppression attributes ("#[" + "allow" + ... ) in eventcore sources.
// This runs on every build, so downstream crates also fail if we sneak suppressions in.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // Run the check unless explicitly disabled (e.g., CHECK_NO_ALLOW=0).
    if env::var("CHECK_NO_ALLOW")
        .map(|v| v == "0")
        .unwrap_or(false)
    {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let check_paths = ["src", "tests", "build.rs", "examples"];

    let mut violations = Vec::new();
    for relative in check_paths.iter() {
        let path = manifest_dir.join(relative);
        if !path.exists() {
            continue;
        }
        scan_path(&path, &mut violations);
    }

    if violations.is_empty() {
        return;
    }

    eprintln!("ERROR: disallowed allow-attributes detected in eventcore:");
    for (file, line, content) in violations {
        eprintln!("  {}:{}: {}", file.display(), line, content.trim());
    }
    panic!("lint suppression attributes are forbidden");
}

fn scan_path(path: &Path, violations: &mut Vec<(PathBuf, usize, String)>) {
    const IGNORED_DIRS: [&str; 4] = ["target", ".git", "scripts", "vendor"];

    if path.is_dir() {
        if IGNORED_DIRS.iter().any(|&d| path.ends_with(d)) {
            return;
        }
        for entry in fs::read_dir(path).expect("read dir") {
            let entry = entry.expect("dir entry");
            scan_path(&entry.path(), violations);
        }
        return;
    }

    // Only inspect Rust source files and build script.
    if let Some(ext) = path.extension() {
        if ext != "rs" {
            return;
        }
    } else {
        return;
    }

    if path
        .file_name()
        .map(|name| name == "build.rs")
        .unwrap_or(false)
    {
        return;
    }

    if let Ok(content) = fs::read_to_string(path) {
        for (idx, line) in content.lines().enumerate() {
            if line.contains("#[allow") || line.contains("#![allow") {
                violations.push((path.to_path_buf(), idx + 1, line.to_string()));
            }
        }
    }
}
