#!/bin/bash

# -------------------------------------------------------------------------------------------------
# This script checks a crate for critical dependency version conflicts before publishing.
#
# It packages the crate and inspects the Cargo.lock to detect version mismatches that only
# appear after publishing (path deps resolve differently than version deps).
#
# Usage:
#   ./check-crate-deps.sh pubky-testnet
# -------------------------------------------------------------------------------------------------

set -e
set -u
set -o pipefail

# Check arguments
if [ $# -ne 1 ]; then
  echo "Error: Crate name required."
  echo "Usage: $0 <crate>"
  exit 1
fi

CRATE=$1

# Critical crates where version mismatches cause runtime failures:
# - pkarr: DHT network communication (incompatible protocols)
# - mainline: underlying DHT implementation
# - pubky-common: shared types (PublicKey, Keypair) cause type mismatches at API boundaries
CRITICAL_CRATES="pkarr mainline pubky-common"

echo "Packaging $CRATE to check for dependency conflicts..."
cargo package -p "$CRATE" --no-verify --allow-dirty 2>/dev/null

VERSION=$(cargo metadata --format-version 1 --no-deps | jq -r ".packages[] | select(.name == \"$CRATE\") | .version")
CRATE_FILE="target/package/${CRATE}-${VERSION}.crate"

if [ ! -f "$CRATE_FILE" ]; then
  echo "Error: Could not find packaged crate file at $CRATE_FILE"
  exit 1
fi

echo "Checking packaged Cargo.lock for critical dependency conflicts..."
CARGO_LOCK=$(tar -xzf "$CRATE_FILE" -O "${CRATE}-${VERSION}/Cargo.lock" 2>/dev/null)

HAS_CONFLICTS=false
CONFLICTING_DEPS=""
for DEP in $CRITICAL_CRATES; do
  VERSION_COUNT=$(echo "$CARGO_LOCK" | grep -A1 "name = \"$DEP\"" | grep "version" | sort -u | wc -l || true)
  if [ "$VERSION_COUNT" -gt 1 ]; then
    echo "Error: Multiple $DEP versions detected in packaged crate!"
    echo "$CARGO_LOCK" | grep -A2 "name = \"$DEP\""
    echo ""
    HAS_CONFLICTS=true
    CONFLICTING_DEPS="$CONFLICTING_DEPS $DEP"
  fi
done

if [ "$HAS_CONFLICTS" = true ]; then
  echo "This will cause runtime failures (DHT communication, type mismatches)."
  echo ""
  echo "The conflict comes from published crates.io versions (not local path deps)."
  echo "Check which dependency versions are specified in Cargo.toml files, then verify"
  echo "what versions those published crates use on crates.io."
  echo ""
  echo "To fix: publish updated versions of the dependent crates with consistent"
  echo "dependency versions, then update this crate's Cargo.toml to use those versions."
  exit 1
fi

echo "No critical dependency conflicts found."
