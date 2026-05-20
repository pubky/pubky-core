#!/bin/bash

# -------------------------------------------------------------------------------------------------
# This script sets the version of release-tracked members of the workspace.
# It also updates the inner member dependency versions.
# The pubky_test_utils helper crates stay pinned to the published 0.1.0 API.
# -------------------------------------------------------------------------------------------------

set -e # fail the script if any command fails
set -u # fail the script if any variable is not set
set -o pipefail # fail the script if any pipe command fails

PINNED_TEST_UTILS_VERSION="0.1.0"

# Check if cargo-set-version is installed
if ! cargo --list | grep -q "set-version"; then
  echo "Error: cargo-set-version is not installed but required."
  echo "Please install it first by running:"
  echo "  cargo install cargo-set-version"
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "Error: python3 is not installed but required."
  exit 1
fi


# TODO: Because cargo set-version updates all member dependency versions to the new version, we need to pin the pubky_test_utils helper crates back to 0.1.0 after running it.
# This is a bit hacky but allows us to keep the helper crates at their published versions so we don't have to publish new versions of them every time we want to update the version of the workspace members that depend on them.
pin_test_utils_versions() {
  echo "Pinning pubky_test_utils helper crates to $PINNED_TEST_UTILS_VERSION..."
  python3 - "$PINNED_TEST_UTILS_VERSION" <<'PY'
from pathlib import Path
import re
import sys

version = sys.argv[1]

# These helper crates are already published and consumed as 0.1.0.
# Keep their local path+version dependencies aligned with crates.io so packaging
# published crates resolves them from the registry without publishing new helper versions.
package_manifests = [
    "test_utils/drop_db_helper/Cargo.toml",
    "test_utils/test_macro/Cargo.toml",
    "test_utils/pubky_test/Cargo.toml",
]

dependency_manifests = {
    "pubky-homeserver/Cargo.toml": ["pubky_test_utils"],
    "pubky-testnet/Cargo.toml": ["pubky_test_utils"],
    "test_utils/test_macro/Cargo.toml": ["pubky_test_utils_drop_db_helper"],
    "test_utils/pubky_test/Cargo.toml": [
        "pubky_test_utils_macro",
        "pubky_test_utils_drop_db_helper",
    ],
}

def replace_once(text, pattern, replacement, path, description):
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise SystemExit(f"failed to update {description} in {path}")
    return updated

for manifest in package_manifests:
    path = Path(manifest)
    text = path.read_text()
    text = replace_once(
        text,
        r'^version = "[^"]+"',
        f'version = "{version}"',
        path,
        "package version",
    )
    path.write_text(text)

for manifest, dependency_names in dependency_manifests.items():
    path = Path(manifest)
    text = path.read_text()
    for dependency_name in dependency_names:
        pattern = rf'({re.escape(dependency_name)}\s*=\s*\{{[^}}]*\bversion\s*=\s*)"[^"]+"'
        text = replace_once(
            text,
            pattern,
            lambda match: f'{match.group(1)}"{version}"',
            path,
            f"{dependency_name} dependency version",
        )
    path.write_text(text)
PY
}


# Check if the version is provided
NEW_VERSION=${1:-}
if [ -z "$NEW_VERSION" ]; then
  echo "Error: New version not specified."
  echo "Usage: $0 <new_version>"
  exit 1
fi

# Rough semver format validation
SEMVER_REGEX="^([0-9]+)\.([0-9]+)\.([0-9]+)(-([0-9A-Za-z.-]+))?(\+([0-9A-Za-z.-]+))?$"
if [[ ! "$NEW_VERSION" =~ $SEMVER_REGEX ]]; then
  echo "Error: Version '$NEW_VERSION' is not in semver format (e.g., 1.2.3, 1.0.0-alpha, 2.0.1+build.123)."
  exit 1
fi

# Ask for confirmation to update the version
read -p "Are you sure you want to set the version to $NEW_VERSION? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]
then
    echo "Version change cancelled."
    exit 1
fi

# Update the pubky-sdk package.json
echo "Updating pubky-sdk package.json version to $NEW_VERSION..."
(cd pubky-sdk/bindings/js/pkg && npm version --no-git-tag-version --allow-same-version "$NEW_VERSION")

# Set the version of all rust members of the workspace
# cargo set-version also updates the inner member dependency versions.
echo "Setting the version of all rust members of the workspace to $NEW_VERSION..."
cargo set-version "$NEW_VERSION"
pin_test_utils_versions



echo Done
