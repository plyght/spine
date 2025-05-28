#!/usr/bin/env bash
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if version argument is provided
if [ -z "$1" ]; then
    print_error "Version argument required. Usage: ./scripts/release.sh v1.0.0"
    exit 1
fi

VERSION=$1

# Validate version format (should start with 'v')
if [[ ! $VERSION =~ ^v[0-9]+\.[0-9]+\.[0-9]+.*$ ]]; then
    print_error "Version should be in format 'v1.0.0' or 'v1.0.0-beta.1'"
    exit 1
fi

# Check for required dependencies
command -v gh >/dev/null 2>&1 || {
    print_error "gh (GitHub CLI) is not installed. Please install it first: https://cli.github.com/"
    exit 1
}

command -v jq >/dev/null 2>&1 || {
    print_error "jq is not installed. Please install it first."
    exit 1
}

# Check if user is authenticated with GitHub CLI
if ! gh auth status >/dev/null 2>&1; then
    print_error "GitHub CLI is not authenticated. Please run 'gh auth login' first."
    exit 1
fi

# Check if user has write access to this repository
REPO_INFO=$(gh repo view --json owner,name,permissions 2>/dev/null || echo "")
if [[ -z "$REPO_INFO" ]]; then
    print_error "Unable to determine repository information. Make sure you're in a git repository with GitHub remote."
    exit 1
fi

CAN_WRITE=$(echo "$REPO_INFO" | jq -r '.permissions.push // false')
if [[ "$CAN_WRITE" != "true" ]]; then
    print_error "You don't have write access to this repository. Only repository maintainers can create releases."
    exit 1
fi

print_status "Starting release process for version $VERSION"

# Check if we're on a clean git state
if ! git diff-index --quiet HEAD --; then
    print_error "Working directory is not clean. Please commit or stash your changes."
    exit 1
fi

# Check if we're on main/master branch
CURRENT_BRANCH=$(git branch --show-current)
if [[ "$CURRENT_BRANCH" != "main" && "$CURRENT_BRANCH" != "master" ]]; then
    print_warning "You're not on main/master branch. Current branch: $CURRENT_BRANCH"
    read -p "Continue? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        print_status "Release cancelled."
        exit 1
    fi
fi

# Build the project
print_status "Building release binary..."
cargo build --release

# Check if binary exists
BINARY_PATH="target/release/spn"
if [ ! -f "$BINARY_PATH" ]; then
    print_error "Binary not found at $BINARY_PATH"
    exit 1
fi

# Create git tag
print_status "Creating git tag $VERSION..."
git tag "$VERSION"

# Push tag to origin
print_status "Pushing tag to origin..."
git push origin "$VERSION"

# Create GitHub release with binary
print_status "Creating GitHub release..."
gh release create "$VERSION" \
    --title "Release $VERSION" \
    --generate-notes \
    "$BINARY_PATH#spine-binary-$(uname -s)-$(uname -m)"

print_status "Release $VERSION created successfully!"
print_status "You can view it at: https://github.com/$(gh repo view --json owner,name -q '.owner.login + "/" + .name')/releases/tag/$VERSION"