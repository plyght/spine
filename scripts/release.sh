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

command -v curl >/dev/null 2>&1 || {
    print_error "curl is not installed. Please install it first."
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

# Function to generate AI-powered release notes
generate_ai_release_notes() {
    local version="$1"
    
    # Check if OpenAI API key is available
    if [[ -z "${OPENAI_API_KEY:-}" ]]; then
        print_warning "OPENAI_API_KEY not set. Falling back to GitHub's auto-generated notes."
        return 1
    fi
    
    # Get the latest tag (excluding the current one we're about to create)
    local last_tag
    last_tag=$(git tag --sort=-version:refname | grep -Fxv "${version}" | head -n1 2>/dev/null || echo "")
    
    # Get commit log since last tag or from beginning if no previous tag
    local git_log
    if [[ -n "$last_tag" ]]; then
        git_log=$(git log --oneline --no-merges "${last_tag}..HEAD" 2>/dev/null || echo "")
    else
        git_log=$(git log --oneline --no-merges 2>/dev/null || echo "")
    fi
    
    # If no commits, return early
    if [[ -z "$git_log" ]]; then
        print_warning "No commits found for release notes. Using GitHub's auto-generated notes."
        return 1
    fi
    
    # Get repository info for context
    local repo_name
    repo_name=$(gh repo view --json name -q '.name' 2>/dev/null || echo "Unknown")
    
    # Prepare the prompt for OpenAI
    local prompt
    read -r -d '' prompt <<EOF
Generate professional release notes for version ${version} of the ${repo_name} project.

Based on these git commits:
${git_log}

Please create release notes that:
1. Start with a brief summary of what's new in this release
2. Group changes into categories like: New Features, Bug Fixes, Improvements, etc.
3. Use clear, user-friendly language
4. Highlight the most important changes
5. Keep it concise but informative
6. Format in markdown

Do not include commit hashes or technical implementation details.
EOF
    
    # Call OpenAI API
    local response
    response=$(curl --fail --silent --show-error --retry 3 -w "\n%{http_code}" \
        -H "Authorization: Bearer ${OPENAI_API_KEY}" \
        -H "Content-Type: application/json" \
        -d "{
            \"model\": \"o4-mini-2025-04-16\",
            \"messages\": [
                {
                    \"role\": \"user\",
                    \"content\": $(printf '%s' "$prompt" | jq -R -s .)
                }
            ],
            \"max_completion_tokens\": 1000
        }" \
        "https://api.openai.com/v1/chat/completions")
    
    # Extract HTTP status code and response body
    local http_code
    http_code=$(echo "$response" | tail -n1)
    local response_body
    response_body=$(echo "$response" | head -n -1)
    
    # Check if API call was successful
    if [[ "$http_code" -ne 200 ]]; then
        print_warning "OpenAI API call failed (HTTP $http_code). Falling back to GitHub's auto-generated notes."
        return 1
    fi
    
    # Extract the content from the response
    local release_notes
    release_notes=$(echo "$response_body" | jq -r '.choices[0].message.content // empty' 2>/dev/null)
    
    if [[ -z "$release_notes" ]]; then
        print_warning "Failed to parse OpenAI response. Falling back to GitHub's auto-generated notes."
        return 1
    fi
    
    # Output the release notes to stdout
    printf '%s' "$release_notes"
    return 0
}

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

# Generate AI-powered release notes
print_status "Generating release notes..."
RELEASE_NOTES=$(generate_ai_release_notes "$VERSION")
if [[ $? -eq 0 && -n "$RELEASE_NOTES" ]]; then
    print_status "Using AI-generated release notes"
    if ! gh release create "$VERSION" \
        --title "Release $VERSION" \
        --notes "$RELEASE_NOTES" \
        "$BINARY_PATH#spine-binary-$(uname -s)-$(uname -m)"; then
        print_error "GitHub release creation failed."
        exit 1
    fi
else
    print_status "Using GitHub's auto-generated release notes"
    if ! gh release create "$VERSION" \
        --title "Release $VERSION" \
        --generate-notes \
        "$BINARY_PATH#spine-binary-$(uname -s)-$(uname -m)"; then
        print_error "GitHub release creation failed."
        exit 1
    fi
fi

print_status "Release $VERSION created successfully!"
print_status "You can view it at: https://github.com/$(gh repo view --json owner,name -q '.owner.login + "/" + .name')/releases/tag/$VERSION"