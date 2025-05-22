#!/bin/bash

# -------------------------------------------------------------------------------------------------
# This script sets the version of all members of the workspace.
# It also updates the inner member dependency versions.
# -------------------------------------------------------------------------------------------------

# Check if cargo-set-version is installed
if ! cargo --list | grep -q "set-version"; then
  echo "Error: cargo-set-version is not installed but required."
  echo "Please install it first by running:"
  echo "  cargo install cargo-set-version"
  exit 1
fi


# Check if the version is provided
NEW_VERSION=$1
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

# Set the version of all rust members of the workspace
#cargo set-version $NEW_VERSION

# Update the pubky-client package.json
echo "Updating pubky-client package.json version to $NEW_VERSION..."
(cd pubky-client/pkg && npm version --no-git-tag-version --allow-same-version "$NEW_VERSION")

echo Done
