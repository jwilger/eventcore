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

# Check compiled artifacts as well as Cargo's resolved graph. A fresh target
# directory prevents a checker-enabled artifact from satisfying a negative
# control. `ECM001` is a checker-only diagnostic sentinel, so it is a stable
# positive/negative control without requiring a separate test binary.
isolation_target=$(mktemp -d)
trap 'rm -rf "$isolation_target"' EXIT

assert_checker_sentinel() {
  local expected=$1
  local artifact
  artifact=$(find "$isolation_target/release/deps" -name 'libeventcore-*.rlib' -print -quit)

  if [ -z "$artifact" ]; then
    echo "eventcore release artifact was not produced" >&2
    exit 1
  fi

  if rg --text --quiet 'ECM001' "$artifact"; then
    actual=present
  else
    actual=absent
  fi

  if [ "$actual" != "$expected" ]; then
    echo "checker sentinel is $actual; expected $expected" >&2
    exit 1
  fi
}

CARGO_TARGET_DIR="$isolation_target" cargo build -p eventcore --lib --release --no-default-features --features experimental-modeling
assert_checker_sentinel absent

rm -rf "$isolation_target"
isolation_target=$(mktemp -d)
CARGO_TARGET_DIR="$isolation_target" cargo build -p eventcore --lib --release --no-default-features --features experimental-model-check
assert_checker_sentinel present

# A consumer cannot reach the model module without opting in, while a
# runtime-only consumer can. This checks the public feature boundary rather
# than relying on Cargo.lock, which legitimately retains test dependencies.
consumer_dir=$(mktemp -d)
trap 'rm -rf "$isolation_target" "$consumer_dir"' EXIT
workspace_root=$(pwd)
mkdir "$consumer_dir/src"
cat > "$consumer_dir/Cargo.toml" <<EOF
[package]
name = "eventcore-feature-boundary-check"
version = "0.0.0"
edition = "2024"

[dependencies]
eventcore = { path = "$workspace_root/eventcore", default-features = false }
EOF
cat > "$consumer_dir/src/main.rs" <<'EOF'
fn main() {
    let _ = core::marker::PhantomData::<eventcore::model::Modeled<()>>;
}
EOF

if CARGO_TARGET_DIR="$isolation_target" cargo check --manifest-path "$consumer_dir/Cargo.toml" --quiet; then
  echo "eventcore::model is available without experimental-modeling" >&2
  exit 1
fi

sed -i 's/default-features = false/default-features = false, features = ["experimental-modeling"]/' "$consumer_dir/Cargo.toml"
CARGO_TARGET_DIR="$isolation_target" cargo check --manifest-path "$consumer_dir/Cargo.toml" --quiet
