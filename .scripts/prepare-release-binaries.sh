#!/bin/bash


# -------------------------------------------------------------------------------------------------
# This script prepares the release binaries for the current project.
# It builds all the binaries and the npm package tarball.
# The end result will be a target/github-release directory with the following structure:
#
# target/github-release/$version/
# ├── pubky-homeserver
# └── ...
# -------------------------------------------------------------------------------------------------

# Read the version from the homeserver
version=$(cargo pkgid -p pubky-homeserver | awk -F# '{print $NF}')

# Build the binaries
echo "Build all the binaries for version $version..."
#cargo build -p pubky-homeserver --release
# Build the npm package
echo "Build the npm package..."
#(cd pubky-client/pkg && npm ci && npm run build && npm pack)

# Create the release directory
echo "Create the release directory..."
FOLDER=target/github-release/pubky-core-$version
rm -rf $FOLDER
mkdir -p $FOLDER

# Copy the executables to the release directory
cp target/release/pubky-homeserver $FOLDER/

# gzip the executables
tar -czf $FOLDER.tar.gz $FOLDER


echo Done
