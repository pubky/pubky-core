#!/bin/bash

# -------------------------------------------------------------------------------------------------
# This script publishes a crate to crates.io.
#
# Usage:
#   ./publish-libs.sh pubky-testnet   # Publish pubky-testnet
#   ./publish-libs.sh pubky           # Publish pubky + npm package
# -------------------------------------------------------------------------------------------------

set -e # fail the script if any command fails
set -u # fail the script if any variable is not set
set -o pipefail # fail the script if any pipe command fails

# Check arguments
if [ $# -ne 1 ]; then
  echo "Error: Crate name required."
  echo "Usage: $0 <crate>"
  echo ""
  echo "Examples:"
  echo "  $0 pubky-testnet   # Publish pubky-testnet"
  echo "  $0 pubky           # Publish pubky + npm package"
  exit 1
fi

CRATE=$1

# Publish the crate
echo "Publishing $CRATE..."
cargo publish -p "$CRATE"

# Publish npm package if pubky
if [ "$CRATE" = "pubky" ]; then
  echo "Publishing the npm package to npmjs.com..."
  (cd pubky-sdk/bindings/js/pkg && npm ci && npm run build && npm publish)
fi

echo "Done"
