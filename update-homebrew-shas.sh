#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

if [ $# -ne 1 ]; then
    echo "Usage: $0 VERSION"
    echo "Example: $0 0.1.0"
    exit 1
fi

VERSION=$1
BASE_URL="https://github.com/tatimblin/agent-execution-harness/releases/download/v${VERSION}"

echo -e "${YELLOW}Calculating SHA256 hashes for version $VERSION...${NC}"

# Download and calculate SHA256 for each binary
echo "Downloading and calculating SHA256 for macOS ARM64..."
ARM64_SHA=$(curl -sL "${BASE_URL}/harness-macos-arm64" | shasum -a 256 | cut -d' ' -f1)

echo "Downloading and calculating SHA256 for macOS x86_64..."
X86_64_SHA=$(curl -sL "${BASE_URL}/harness-macos-x86_64" | shasum -a 256 | cut -d' ' -f1)

echo "Downloading and calculating SHA256 for Linux x86_64..."
LINUX_SHA=$(curl -sL "${BASE_URL}/harness-linux-x86_64" | shasum -a 256 | cut -d' ' -f1)

echo
echo -e "${GREEN}SHA256 hashes:${NC}"
echo "ARM64:  $ARM64_SHA"
echo "x86_64: $X86_64_SHA"
echo "Linux:  $LINUX_SHA"
echo

# Update the formula
echo -e "${YELLOW}Updating Homebrew formula...${NC}"
sed -i.bak "s/PLACEHOLDER_ARM64_SHA/$ARM64_SHA/g" agent-execution-harness.rb
sed -i.bak "s/PLACEHOLDER_X86_64_SHA/$X86_64_SHA/g" agent-execution-harness.rb
sed -i.bak "s/PLACEHOLDER_LINUX_SHA/$LINUX_SHA/g" agent-execution-harness.rb
rm agent-execution-harness.rb.bak

echo -e "${GREEN}✓ Updated agent-execution-harness.rb with correct SHA256 hashes${NC}"
echo
echo "You can now:"
echo "1. Test the formula: brew install --build-from-source ./agent-execution-harness.rb"
echo "2. Create a Homebrew tap repository and add this formula"
echo "3. Or submit a PR to homebrew-core (if the tool meets their criteria)"