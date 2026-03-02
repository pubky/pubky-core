#!/bin/bash

# -------------------------------------------------------------------------------------------------
# This script sets the version of a workspace crate.
#
# Usage:
#   ./set-version.sh 0.7.0 pubky-testnet   # Set pubky-testnet to 0.7.0
#   ./set-version.sh 0.7.0 pubky           # Set pubky + npm package
# -------------------------------------------------------------------------------------------------

set -e # fail the script if any command fails
set -u # fail the script if any variable is not set
set -o pipefail # fail the script if any pipe command fails

# Check if the version and crate are provided
if [ $# -ne 2 ]; then
  echo "Error: Version and crate name required."
  echo "Usage: $0 <version> <crate>"
  echo ""
  echo "Examples:"
  echo "  $0 0.7.0 pubky-testnet   # Set pubky-testnet to 0.7.0"
  echo "  $0 0.7.0 pubky           # Set pubky + npm package"
  exit 1
fi

NEW_VERSION=$1
CRATE=$2

# Rough semver format validation
SEMVER_REGEX="^([0-9]+)\.([0-9]+)\.([0-9]+)(-([0-9A-Za-z.-]+))?(\+([0-9A-Za-z.-]+))?$"
if [[ ! "$NEW_VERSION" =~ $SEMVER_REGEX ]]; then
  echo "Error: Version '$NEW_VERSION' is not in semver format (e.g., 1.2.3, 1.0.0-alpha, 2.0.1+build.123)."
  exit 1
fi

# Ask for confirmation to update the version
read -p "Are you sure you want to set the version to $NEW_VERSION for $CRATE? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
  echo "Version change cancelled."
  exit 1
fi

# Map crate name to directory (pubky crate lives in pubky-sdk dir)
CRATE_DIR="$CRATE"
if [ "$CRATE" = "pubky" ]; then
  CRATE_DIR="pubky-sdk"
fi

# Find the crate's Cargo.toml
MANIFEST_PATH="$CRATE_DIR/Cargo.toml"
if [ ! -f "$MANIFEST_PATH" ]; then
  echo "Error: Could not find $MANIFEST_PATH"
  exit 1
fi

# Set version for the crate (update first version = "x.x.x" line in [package] section)
echo "Setting $CRATE to $NEW_VERSION..."
# Use portable sed -i syntax (BSD/macOS requires '' argument, GNU/Linux does not)
case "$OSTYPE" in
    darwin*) sed -i '' 's/^version = ".*"$/version = "'"$NEW_VERSION"'"/' "$MANIFEST_PATH" ;;
    *)       sed -i 's/^version = ".*"$/version = "'"$NEW_VERSION"'"/' "$MANIFEST_PATH" ;;
esac

# Update npm package if pubky
if [ "$CRATE" = "pubky" ]; then
  echo "Updating npm package version to $NEW_VERSION..."
  (cd pubky-sdk/bindings/js/pkg && npm version --no-git-tag-version --allow-same-version "$NEW_VERSION")
fi

echo "Done"
