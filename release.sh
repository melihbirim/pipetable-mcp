#!/bin/bash
set -e

VERSION=${1:-}
if [ -z "$VERSION" ]; then
  echo "Usage: ./release.sh <version>   (e.g. ./release.sh 0.1.0)"
  exit 1
fi

TAG="v$VERSION"

# Update version in Cargo.toml
sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml

# Verify it built before tagging
cargo build --release 2>&1 | tail -1

git add Cargo.toml Cargo.lock
git commit -m "$TAG"
git tag "$TAG"
git push origin main
git push origin "$TAG"

echo ""
echo "Tagged $TAG and pushed. GitHub Actions is now building:"
echo "  https://github.com/melihbirim/pipetable-mcp/actions"
echo ""
echo "Release will appear at:"
echo "  https://github.com/melihbirim/pipetable-mcp/releases/tag/$TAG"
