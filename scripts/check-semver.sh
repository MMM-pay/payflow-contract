#!/usr/bin/env bash
set -euo pipefail

latest_tag=$(git describe --tags --abbrev=0 2>/dev/null || echo "")
if [[ -z "$latest_tag" ]]; then
  echo "No previous tag found; skipping semver diff check."
  exit 0
fi

echo "Comparing current PR against tag: $latest_tag"

cargo_changed=$(git diff --name-only "$latest_tag" HEAD -- 'contracts/*/Cargo.toml')
lib_changed=$(git diff --name-only "$latest_tag" HEAD -- 'contracts/*/src/lib.rs')

if [[ -n "$lib_changed" ]]; then
  echo "Detected changes in contract public entry points (lib.rs)."
  if [[ -z "$cargo_changed" ]]; then
    echo "::error::Public contract entry points changed since $latest_tag, but no Cargo.toml version was bumped."
    exit 1
  else
    echo "Cargo.toml was updated alongside public API changes. Verifying version bump..."
    old_version=$(git show "$latest_tag:contracts/vault/Cargo.toml" | grep '^version' | head -n1 | cut -d'"' -f2 || echo "0.0.0")
    new_version=$(grep '^version' contracts/vault/Cargo.toml | head -n1 | cut -d'"' -f2 || echo "0.0.0")
    if [[ "$old_version" == "$new_version" ]]; then
      echo "::error::Cargo.toml version ($new_version) has not been bumped compared to $latest_tag ($old_version)."
      exit 1
    fi
  fi
fi

echo "SemVer check passed successfully."
