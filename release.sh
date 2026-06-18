#!/bin/bash
set -e

# Read current version from Cargo.toml
CURRENT=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
MAJOR=$(echo "$CURRENT" | cut -d. -f1)
MINOR=$(echo "$CURRENT" | cut -d. -f2)
PATCH=$(echo "$CURRENT" | cut -d. -f3)

ARG=${1:-minor}

case "$ARG" in
  major)
    VERSION="$((MAJOR + 1)).0.0"
    ;;
  minor)
    VERSION="${MAJOR}.$((MINOR + 1)).0"
    ;;
  patch)
    VERSION="${MAJOR}.${MINOR}.$((PATCH + 1))"
    ;;
  [0-9]*)
    VERSION="$ARG"
    ;;
  *)
    echo "Usage: ./release.sh [major|minor|patch|x.y.z]"
    echo "  (default: minor)"
    exit 1
    ;;
esac

TAG="v$VERSION"

echo "Current: v$CURRENT  →  $TAG"
read -p "Proceed? [y/N] " confirm
[[ "$confirm" =~ ^[Yy]$ ]] || exit 0

# Update version in Cargo.toml
sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml

# Quick compile check before tagging (actual binaries built by CI)
cargo check 2>&1 | tail -1

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
