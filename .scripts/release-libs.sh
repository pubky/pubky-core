#!/bin/bash

# -------------------------------------------------------------------------------------------------
# This script publishes all the crates of the workspace to crates.io.
# -------------------------------------------------------------------------------------------------

set -e # fail the script if any command fails
set -u # fail the script if any variable is not set
set -o pipefail # fail the script if any pipe command fails


# Publish all the crates of the workspace to crates.io.
# ws does this in the correct order on how crates are depended on each other.
echo "Publishing all the crates of the workspace to crates.io..."
cargo ws publish --no-git-commit --publish-as-is

# Publish the npm package to npmjs.com.
echo "Publishing the npm package to npmjs.com..."
(cd pubky-client/pkg && npm ci && npm run build && npm publish)

echo "Done"