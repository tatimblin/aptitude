#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

usage() {
    echo "Usage: $0 [major|minor|patch|VERSION]"
    echo "  major: 1.0.0 -> 2.0.0"
    echo "  minor: 1.0.0 -> 1.1.0"
    echo "  patch: 1.0.0 -> 1.0.1"
    echo "  VERSION: specific version like 1.2.3"
    exit 1
}

if [ $# -ne 1 ]; then
    usage
fi

# Get current version from Cargo.toml
current_version=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo -e "${YELLOW}Current version: $current_version${NC}"

# Calculate new version
case $1 in
    major)
        IFS='.' read -r major minor patch <<< "$current_version"
        new_version="$((major + 1)).0.0"
        ;;
    minor)
        IFS='.' read -r major minor patch <<< "$current_version"
        new_version="$major.$((minor + 1)).0"
        ;;
    patch)
        IFS='.' read -r major minor patch <<< "$current_version"
        new_version="$major.$minor.$((patch + 1))"
        ;;
    *)
        if [[ $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            new_version=$1
        else
            echo -e "${RED}Invalid version format${NC}"
            usage
        fi
        ;;
esac

echo -e "${YELLOW}New version: $new_version${NC}"
read -p "Continue? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    exit 1
fi

# Update Cargo.toml version
echo -e "${YELLOW}Updating Cargo.toml...${NC}"
sed -i.bak "s/^version = \".*\"/version = \"$new_version\"/" Cargo.toml
rm Cargo.toml.bak

# Update Homebrew formula version
echo -e "${YELLOW}Updating Homebrew formula...${NC}"
sed -i.bak "s/version \".*\"/version \"$new_version\"/" agent-execution-harness.rb
rm agent-execution-harness.rb.bak

# Commit changes
echo -e "${YELLOW}Committing version bump...${NC}"
git add Cargo.toml Cargo.lock agent-execution-harness.rb
git commit -m "Bump version to $new_version"

# Create and push tag
echo -e "${YELLOW}Creating and pushing tag...${NC}"
git tag "v$new_version"
git push && git push --tags

echo -e "${YELLOW}Waiting for GitHub Actions to complete...${NC}"
echo "This will build binaries, create the release, and publish to crates.io"
echo "Check progress: https://github.com/tatimblin/agent-execution-harness/actions"

echo -e "${GREEN}✓ Release $new_version started!${NC}"
echo
echo "Next steps for Homebrew:"
echo "1. Wait for GitHub release to complete"
echo "2. Download the binaries and calculate SHA256:"
echo "   curl -sL https://github.com/tatimblin/agent-execution-harness/releases/download/v$new_version/harness-macos-arm64 | shasum -a 256"
echo "   curl -sL https://github.com/tatimblin/agent-execution-harness/releases/download/v$new_version/harness-macos-x86_64 | shasum -a 256"
echo "   curl -sL https://github.com/tatimblin/agent-execution-harness/releases/download/v$new_version/harness-linux-x86_64 | shasum -a 256"
echo "3. Update the SHA256 values in agent-execution-harness.rb"
echo "4. Create/update your Homebrew tap: https://github.com/tatimblin/homebrew-tap"