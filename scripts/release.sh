#!/usr/bin/env bash
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    printf "${GREEN}[INFO]${NC} %s\n" "$1"
}

print_warning() {
    printf "${YELLOW}[WARN]${NC} %s\n" "$1"
}

print_error() {
    printf "${RED}[ERROR]${NC} %s\n" "$1"
}

# Check if version argument is provided
if [ "$#" -eq 0 ]; then
    print_error "Version argument required. Usage: ./scripts/release.sh v1.0.0"
    exit 1
fi

VERSION=$1

# Validate version format (should start with 'v')
if [[ ! $VERSION =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9]+(\.[0-9]+)?)?$ ]]; then
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
REPO_INFO=$(gh repo view --json owner,name,viewerPermission 2>/dev/null || echo "")
if [[ -z "$REPO_INFO" ]]; then
    print_error "Unable to determine repository information. Make sure you're in a git repository with GitHub remote."
    exit 1
fi

VIEWER_PERMISSION=$(echo "$REPO_INFO" | jq -r '.viewerPermission // "NONE"')
if [[ "$VIEWER_PERMISSION" != "ADMIN" && "$VIEWER_PERMISSION" != "WRITE" ]]; then
    print_error "You don't have write access to this repository. Only repository maintainers can create releases. Current permission: $VIEWER_PERMISSION"
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

# Build for current platform only to avoid cross-compilation issues
CURRENT_TARGET=$(rustc -vV | grep host | cut -d' ' -f2)
TARGETS=("$CURRENT_TARGET")

# Array to store built binaries
BUILT_BINARIES=()

# Build for multiple targets
print_status "Building release binaries for multiple platforms..."

for target in "${TARGETS[@]}"; do
    print_status "Building for target: $target"
    
    # Install target if not already installed
    if ! rustup target list --installed | grep -q "$target"; then
        print_status "Installing target $target..."
        rustup target add "$target"
    fi
    
    # Build for target
    if cargo build --release --target "$target"; then
        binary_path="target/$target/release/spn"
        if [ -f "$binary_path" ]; then
            # Create target-specific binary name
            case "$target" in
                "x86_64-unknown-linux-gnu")
                    binary_name="spn-linux-x64"
                    ;;
                "aarch64-unknown-linux-gnu")
                    binary_name="spn-linux-arm64"
                    ;;
                "x86_64-apple-darwin")
                    binary_name="spn-macos-x64"
                    ;;
                "aarch64-apple-darwin")
                    binary_name="spn-macos-arm64"
                    ;;
                *)
                    # Use current platform detection for unknown targets
                    if [[ "$OSTYPE" == "darwin"* ]]; then
                        if [[ "$(uname -m)" == "arm64" ]]; then
                            binary_name="spn-macos-arm64"
                        else
                            binary_name="spn-macos-x64"
                        fi
                    else
                        if [[ "$(uname -m)" == "aarch64" ]]; then
                            binary_name="spn-linux-arm64"
                        else
                            binary_name="spn-linux-x64"
                        fi
                    fi
                    ;;
            esac
            
            # Copy binary with target-specific name
            cp "$binary_path" "target/$binary_name"
            BUILT_BINARIES+=("target/$binary_name#$binary_name")
            print_status "Successfully built $binary_name"
        else
            print_warning "Binary not found for target $target, skipping..."
        fi
    else
        print_warning "Failed to build for target $target, skipping..."
    fi
done

# Check if at least one binary was built
if [ ${#BUILT_BINARIES[@]} -eq 0 ]; then
    print_error "No binaries were successfully built"
    exit 1
fi

print_status "Successfully built ${#BUILT_BINARIES[@]} binaries"

# Create git tag
print_status "Creating git tag $VERSION..."
if git rev-parse "$VERSION" >/dev/null 2>&1; then
    print_error "Tag $VERSION already exists."
    exit 1
fi
git tag -a "$VERSION" -m "Release $VERSION"

# Push tag to origin
print_status "Pushing tag to origin..."
git push origin "$VERSION"

# Create GitHub release with binary
print_status "Creating GitHub release..."
print_status "Using GitHub's auto-generated release notes"
if ! gh release create "$VERSION" \
    --title "Release $VERSION" \
    --generate-notes \
    "${BUILT_BINARIES[@]}"; then
    print_error "GitHub release creation failed."
    exit 1
fi

print_status "Release $VERSION created successfully!"
print_status "You can view it at: https://github.com/$(gh repo view --json owner,name -q '.owner.login + "/" + .name')/releases/tag/$VERSION"