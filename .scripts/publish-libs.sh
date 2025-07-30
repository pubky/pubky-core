#!/bin/bash

# -------------------------------------------------------------------------------------------------
# This script publishes all the crates of the workspace to crates.io.
# -------------------------------------------------------------------------------------------------

set -e # fail the script if any command fails
set -u # fail the script if any variable is not set
set -o pipefail # fail the script if any pipe command fails

# Check if cargo-set-version is installed
if ! cargo --list | grep -q "workspaces"; then
  echo "Error: cargo-workspaces is not installed but required."
  echo "Please install it first by running:"
  echo "  cargo install cargo-workspaces"
  exit 1
fi


# Publish all the crates of the workspace to crates.io.
# ws does this in the correct order on how crates are depended on each other.
echo "Publishing all the crates of the workspace to crates.io..."
cargo ws publish --no-git-commit --publish-as-is

# Publish the npm package to npmjs.com.
echo "Publishing the npm package to npmjs.com..."
(cd pubky-client/bindings/js/pkg && npm ci && npm run build && npm publish)

echo "Done"