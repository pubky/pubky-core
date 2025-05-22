#!/bin/bash


# -------------------------------------------------------------------------------------------------
# This script prepares the release binaries for the current project.
# It builds all the binaries and the npm package tarball.
# The end result will be a target/github-release directory with the following structure:
#
# target/github-release/$version/
# ├── pubky-homeserver
# └── ...
#
# Make sure you installed https://github.com/cross-rs/cross for cross-compilation.
# -------------------------------------------------------------------------------------------------

# Read the version from the homeserver
VERSION=$(cargo pkgid -p pubky-homeserver | awk -F# '{print $NF}')

builds=(
# target, nickname
"aarch64-apple-darwin,osx-arm64" 
"x86_64-apple-darwin,osx-amd64"
"x86_64-unknown-linux-musl,linux-amd64"
"aarch64-unknown-linux-musl,linux-arm64"
"x86_64-pc-windows-gnu,windows-amd64"
"armv7-unknown-linux-musleabihf,linux-armv7hf"
"arm-unknown-linux-musleabihf,linux-armhf"
)

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
    cross build -p pubky-homeserver --release --target $TARGET
    if [[ $TARGET == *"windows"* ]]; then
        cp target/$TARGET/release/$ARTIFACT.exe $DICT
    else
        cp target/$TARGET/release/$ARTIFACT $DICT
    fi
    (cd target/github-release && tar -czf $FOLDER.tar.gz $FOLDER && rm -rf $FOLDER)
done


tree target/github-release
(cd target/github-release && pwd)
