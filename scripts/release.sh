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

# Function to execute function calls for AI
execute_function_call() {
    local function_name="$1"
    local arguments="$2"
    
    case "$function_name" in
        "search_repository_info")
            local repo_name
            repo_name=$(echo "$arguments" | jq -r '.repository_name // ""')
            if [[ -n "$repo_name" ]]; then
                # URL-encode the repository name
                local encoded_repo_name
                encoded_repo_name=$(printf '%s' "$repo_name" | jq -sRr @uri)
                # Search for repository information online
                local search_results
                search_results=$(curl -s "https://api.github.com/search/repositories?q=${encoded_repo_name}&sort=stars&order=desc" | jq -r '.items[0] | select(.name) | "Description: " + (.description // "No description") + "\nLanguage: " + (.language // "Unknown") + "\nStars: " + (.stargazers_count | tostring) + "\nTopics: " + (.topics | join(", "))' 2>/dev/null || echo "Repository information not found")
                echo "$search_results"
            else
                echo "Repository name not provided"
            fi
            ;;
        "get_readme_content")
            if [[ -f "README.md" ]]; then
                cat README.md 2>/dev/null || echo "README.md exists but cannot be read"
            else
                echo "No README.md found in repository"
            fi
            ;;
        "get_repository_description")
            gh repo view --json description,topics,language -q '"Description: " + (.description // "No description") + "\nLanguage: " + (.language // "Unknown") + "\nTopics: " + (.topics | join(", "))' 2>/dev/null || echo "Could not fetch repository description"
            ;;
        "get_previous_releases")
            local limit
            limit=$(echo "$arguments" | jq -r '.limit // 5')
            gh release list --limit "$limit" --json name,tagName,body,publishedAt -q '.[] | "Release: " + .name + " (" + .tagName + ")\nPublished: " + .publishedAt + "\nNotes:\n" + (.body // "No release notes") + "\n---"' 2>/dev/null || echo "No previous releases found"
            ;;
        "get_project_structure")
            local depth
            depth=$(echo "$arguments" | jq -r '.depth // 2')
            find . -maxdepth "$depth" -type f -name "*.rs" -o -name "*.toml" -o -name "*.md" -o -name "*.json" -o -name "*.yaml" -o -name "*.yml" | grep -v "target/" | grep -v "node_modules/" | head -n "$((depth * 15))" | sort 2>/dev/null || echo "Could not read project structure"
            ;;
        "get_git_changes")
            local last_tag
            last_tag=$(echo "$arguments" | jq -r '.since_tag // ""')
            if [[ -n "$last_tag" ]]; then
                git diff --stat "$last_tag..HEAD" 2>/dev/null | head -20 || echo "Could not get git changes since $last_tag"
            else
                git diff --stat HEAD~10..HEAD 2>/dev/null | head -20 || echo "Could not get recent git changes"
            fi
            ;;
        "get_recent_activity")
            local days
            days=$(echo "$arguments" | jq -r '.days // 30')
            local date_threshold
            if [[ "$OSTYPE" == "darwin"* ]]; then
                date_threshold=$(date -v-${days}d +%Y-%m-%d)
            else
                date_threshold=$(date -d "$days days ago" +%Y-%m-%d)
            fi
            {
                echo "Recent Issues (last $days days):"
                gh issue list --limit 5 --state all --search "created:>$date_threshold" --json title,state,createdAt -q '.[] | "- " + .title + " (" + .state + ", " + .createdAt + ")"' 2>/dev/null || echo "No recent issues"
                echo -e "\nRecent PRs (last $days days):"
                gh pr list --limit 5 --state all --search "created:>$date_threshold" --json title,state,createdAt -q '.[] | "- " + .title + " (" + .state + ", " + .createdAt + ")"' 2>/dev/null || echo "No recent PRs"
            }
            ;;
        "analyze_commit_types")
            local last_tag
            last_tag=$(echo "$arguments" | jq -r '.since_tag // ""')
            local commits
            if [[ -n "$last_tag" ]]; then
                commits=$(git log --oneline --no-merges "$last_tag..HEAD" 2>/dev/null || echo "")
            else
                commits=$(git log --oneline --no-merges HEAD~20..HEAD 2>/dev/null || echo "")
            fi
            
            if [[ -n "$commits" ]]; then
                echo "Commit Analysis:"
                echo "$commits" | awk '
                BEGIN { feat=0; fix=0; docs=0; refactor=0; other=0 }
                /^[a-f0-9]+ (feat|add|implement)/ { feat++ }
                /^[a-f0-9]+ (fix|resolve|correct)/ { fix++ }
                /^[a-f0-9]+ (docs|update.*readme|documentation)/ { docs++ }
                /^[a-f0-9]+ (refactor|cleanup|reorganize)/ { refactor++ }
                ! /^[a-f0-9]+ (feat|add|implement|fix|resolve|correct|docs|update.*readme|documentation|refactor|cleanup|reorganize)/ { other++ }
                END {
                    print "Features/Additions: " feat
                    print "Bug Fixes: " fix  
                    print "Documentation: " docs
                    print "Refactoring: " refactor
                    print "Other: " other
                }'
            else
                echo "No commits found for analysis"
            fi
            ;;
        "get_main_source_overview")
            {
                echo "Main source files overview:"
                if [[ -f "src/main.rs" ]]; then
                    echo "=== src/main.rs (first 30 lines) ==="
                    head -30 src/main.rs | sed 's/^/    /'
                fi
                if [[ -f "Cargo.toml" ]]; then
                    echo -e "\n=== Cargo.toml ==="
                    cat Cargo.toml | sed 's/^/    /'
                fi
                if [[ -f "package.json" ]]; then
                    echo -e "\n=== package.json (description and scripts) ==="
                    jq -r '{description, scripts}' package.json 2>/dev/null | sed 's/^/    /' || echo "    Could not parse package.json"
                fi
            } 2>/dev/null || echo "Could not read main source files"
            ;;
        *)
            echo "Unknown function: $function_name"
            ;;
    esac
}

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
    prompt=$(cat <<'EOF'
You are tasked with generating professional release notes for ${repo_name} ${version}.

Previous tag: ${last_tag:-"(no previous releases)"}

IMPORTANT: Before writing release notes, you MUST gather comprehensive context by calling these functions:
1. get_repository_description - understand the project
2. get_readme_content - learn what the project does
3. get_main_source_overview - see main source files to understand functionality
4. get_previous_releases - see past release notes to avoid repetition
5. get_project_structure - understand codebase organization
6. analyze_commit_types - categorize the changes
7. get_git_changes - see actual file modifications

Available git commits since last release:
${git_log}

After gathering context, write release notes following these requirements:
- Write in active voice focusing on user benefits
- Use specific action words (add, fix, improve, update)
- Group into logical sections (New Features, Improvements, Bug Fixes, etc.)
- Avoid repeating content from previous releases
- Be specific about what changed, not just that something changed
- Omit purely internal changes unless they impact users
- Format as clean markdown with bullet points
- Keep under 250 words total
- No commit hashes, PR numbers, or technical jargon

Example format:
## What's New
Brief summary highlighting the most important changes.

### New Features
- Add [specific capability] for [user benefit]
- Implement [feature] to [solve user problem]

### Improvements
- Enhance [specific area] performance
- Update [component] with [user-facing improvement]

### Bug Fixes
- Fix [specific issue] affecting [scenario]
- Resolve [problem] in [context]

Remember: Call the functions first to understand the project and avoid repetition!
EOF
)
    
    # Define tools for function calling
    local tools
    tools=$(cat /tmp/tools.json)

    # Initial API call with tools
    local response
    response=$(curl --fail --silent --show-error --retry 3 -w "\n%{http_code}" \
        -H "Authorization: Bearer ${OPENAI_API_KEY}" \
        -H "Content-Type: application/json" \
        -d "{
            \"model\": \"o4-mini-2025-04-16\",
            \"input\": [
                {
                    \"role\": \"user\",
                    \"content\": $(printf '%s' "$prompt" | jq -R -s .)
                }
            ],
            \"tools\": $tools,
            \"max_completion_tokens\": 1000
        }" \
        "https://api.openai.com/v1/responses")
    
    # Extract HTTP status code and response body
    local http_code
    http_code=$(echo "$response" | tail -n1)
    local response_body
    response_body=$(echo "$response" | sed '$d')
    
    # Check if API call was successful
    if [[ "$http_code" -ne 200 ]]; then
        print_warning "OpenAI API call failed (HTTP $http_code). Falling back to GitHub's auto-generated notes."
        return 1
    fi
    
    # Check if model wants to call functions
    local function_calls
    function_calls=$(echo "$response_body" | jq -r '.output // empty' 2>/dev/null)
    
    # Check if any output items are function calls
    local has_function_calls
    has_function_calls=$(echo "$function_calls" | jq -r 'map(select(.type == "function_call")) | length > 0' 2>/dev/null)
    
    if [[ "$has_function_calls" == "true" ]]; then
        # Build initial input messages array  
        local input_messages
        input_messages=$(jq -n --arg content "$prompt" '[{"role": "user", "content": $content}]')
        
        # Append the function calls from the model's response
        local function_call_items
        function_call_items=$(echo "$function_calls" | jq -c 'map(select(.type == "function_call"))')
        input_messages=$(echo "$input_messages" | jq --argjson calls "$function_call_items" '. + $calls')
        
        # Execute each function call and collect results
        while IFS= read -r tool_call; do
            local function_name arguments call_id
            
            function_name=$(echo "$tool_call" | jq -r '.name')
            arguments=$(echo "$tool_call" | jq -r '.arguments')
            call_id=$(echo "$tool_call" | jq -r '.call_id')
            
            # Auto-inject last_tag for functions that need it
            if [[ "$function_name" == "get_git_changes" || "$function_name" == "analyze_commit_types" ]]; then
                if [[ -n "$last_tag" ]]; then
                    arguments=$(echo "$arguments" | jq --arg tag "$last_tag" '. + {since_tag: $tag}')
                else
                    arguments=$(echo "$arguments" | jq '. + {since_tag: null}')
                fi
            fi
            
            local result
            result=$(execute_function_call "$function_name" "$arguments")
            
            # Append function result to input messages
            local function_result
            function_result=$(jq -n --arg id "$call_id" --arg output "$result" '{"type": "function_call_output", "call_id": $id, "output": $output}')
            input_messages=$(echo "$input_messages" | jq --argjson result "$function_result" '. + [$result]')
            
        done < <(echo "$function_call_items" | jq -c '.[]')
        
        # Make second API call with function results
        local final_response
        final_response=$(curl --fail --silent --show-error --retry 3 -w "\n%{http_code}" \
            -H "Authorization: Bearer ${OPENAI_API_KEY}" \
            -H "Content-Type: application/json" \
            -d "$(jq -n --argjson msgs "$input_messages" --argjson tools_def "$tools" '{
                "model": "o4-mini-2025-04-16",
                "input": $msgs,
                "tools": $tools_def,
                "max_completion_tokens": 1000
            }')" \
            "https://api.openai.com/v1/responses")
        
        local final_http_code
        final_http_code=$(echo "$final_response" | tail -n1)
        local final_response_body
        final_response_body=$(echo "$final_response" | sed '$d')
        
        if [[ "$final_http_code" -ne 200 ]]; then
            print_warning "OpenAI API second call failed (HTTP $final_http_code). Falling back to GitHub's auto-generated notes."
            return 1
        fi
        
        response_body="$final_response_body"
    fi
    
    # Extract the content from the response
    local release_notes
    release_notes=$(echo "$response_body" | jq -r '.output_text // empty' 2>/dev/null)
    
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

# Generate AI-powered release notes
print_status "Generating release notes..."
RELEASE_NOTES=$(generate_ai_release_notes "$VERSION")
if [[ $? -eq 0 && -n "$RELEASE_NOTES" ]]; then
    print_status "Using AI-generated release notes"
    if ! gh release create "$VERSION" \
        --title "Release $VERSION" \
        --notes "$RELEASE_NOTES" \
        "${BUILT_BINARIES[@]}"; then
        print_error "GitHub release creation failed."
        exit 1
    fi
else
    print_status "Using GitHub's auto-generated release notes"
    if ! gh release create "$VERSION" \
        --title "Release $VERSION" \
        --generate-notes \
        "${BUILT_BINARIES[@]}"; then
        print_error "GitHub release creation failed."
        exit 1
    fi
fi

print_status "Release $VERSION created successfully!"
print_status "You can view it at: https://github.com/$(gh repo view --json owner,name -q '.owner.login + "/" + .name')/releases/tag/$VERSION"