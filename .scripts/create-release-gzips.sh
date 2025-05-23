#!/bin/bash

# -------------------------------------------------------------------------------------------------
# This script prepares the release binaries for the current project.
# It builds all the binaries and prepares them for upload as a Github Release.
# The end result will be a target/github-release directory with the following structure:
#
# target/github-release/
# ├── pubky-core-linux-amd64-v0.5.0-rc.0.tar.gz
# ├── pubky-core-osx-arm64-v0.5.0-rc.0.tar.gz
# ├── pubky-core-windows-amd64-v0.5.0-rc.0.tar.gz
# └── ...
#
# Make sure you installed https://github.com/cross-rs/cross for cross-compilation.
# -------------------------------------------------------------------------------------------------


set -e # fail the script if any command fails
set -u # fail the script if any variable is not set
set -o pipefail # fail the script if any pipe command fails


# Check if cross is installed
if ! command -v cross &> /dev/null
then
    echo "cross executable not be found. It is required to cross-compile the binaries. Please install it from https://github.com/cross-rs/cross"
    exit 1
fi

# Read the version from the homeserver
VERSION=$(cargo pkgid -p pubky-homeserver | awk -F# '{print $NF}')
echo "Preparing release executables for version $VERSION..."

builds=(
# target, nickname
# "aarch64-apple-darwin,osx-arm64" 
# "x86_64-apple-darwin,osx-amd64"
"x86_64-unknown-linux-musl,linux-amd64"
"aarch64-unknown-linux-musl,linux-arm64"
"x86_64-pc-windows-gnu,windows-amd64"

# LMDB mapsize is a usize and is too big for armv7hf and armhf
# So we can't build these yet with LMDB.
# "armv7-unknown-linux-musleabihf,linux-armv7hf"
# "arm-unknown-linux-musleabihf,linux-armhf"
)

# List of artifacts to build.
artifcats=("pubky-homeserver")

echo "Create the github-release directory..."
rm -rf target/github-release
mkdir -p target/github-release

# Build the binaries
echo "Build all the binaries for version $VERSION..."
for BUILD in "${builds[@]}"; do
    # Split tuple by comma
    IFS=',' read -r TARGET NICKNAME <<< "$BUILD"

    echo "Build $NICKNAME with $TARGET"
    FOLDER="pubky-core-$NICKNAME-v$VERSION"
    DICT="target/github-release/$FOLDER"
    for ARTIFACT in "${artifcats[@]}"; do
        cross build -p $ARTIFACT --release --target $TARGET
        if [[ $TARGET == *"windows"* ]]; then
            cp target/$TARGET/release/$ARTIFACT.exe $DICT
        else
            cp target/$TARGET/release/$ARTIFACT $DICT
        fi
    done;
    (cd target/github-release && tar -czf $FOLDER.tar.gz $FOLDER && rm -rf $FOLDER)
done


tree target/github-release
(cd target/github-release && pwd)
