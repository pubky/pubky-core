#!/bin/bash

NEW_VERSION=$1

if [ -z "$NEW_VERSION" ]; then
  echo "Error: New version not specified."
  echo "Usage: $0 <new_version>"
  exit 1
fi

# Update Cargo.toml
find . -name "Cargo.toml" -type f -exec sed -i "s/^version = .*/version = \"$NEW_VERSION\"/" {} +

# Update pubky-client/package.json
sed -i "s/\"version\": \".*\"/\"version\": \"$NEW_VERSION\"/" pubky-client/package.json

# Update pubky-client/release.toml

