#!/usr/bin/env bash
# Alaz Git Post-Commit Hook
#
# Install:
#   cp scripts/post-commit-hook.sh .git/hooks/post-commit
#   chmod +x .git/hooks/post-commit
#
# Or symlink for auto-updates:
#   ln -sf ../../scripts/post-commit-hook.sh .git/hooks/post-commit
#
# Required env vars (do NOT hardcode):
#   ALAZ_URL       Alaz server URL (default: http://localhost:3456)
#   ALAZ_API_KEY   API key issued by the Alaz server

ALAZ_URL="${ALAZ_URL:-http://localhost:3456}"

if [ -z "${ALAZ_API_KEY:-}" ]; then
    # Silently skip if no key is configured — never block a commit.
    exit 0
fi

PROJECT_NAME=$(basename "$(git rev-parse --show-toplevel)")
COMMIT_HASH=$(git rev-parse HEAD)
COMMIT_MESSAGE=$(git log -1 --pretty=%B)

# Build file changes JSON array from git diff
FILES=$(git diff-tree --no-commit-id -r --numstat --diff-filter=ACDMR "$COMMIT_HASH" | while read added removed path; do
    # Handle binary files (- means binary)
    [ "$added" = "-" ] && added=0
    [ "$removed" = "-" ] && removed=0

    # Determine change type
    change_type="modify"
    git diff-tree --no-commit-id -r --name-status "$COMMIT_HASH" | grep -q "^A.*$path" && change_type="add"
    git diff-tree --no-commit-id -r --name-status "$COMMIT_HASH" | grep -q "^D.*$path" && change_type="delete"

    echo "{\"path\":\"$path\",\"change_type\":\"$change_type\",\"lines_added\":$added,\"lines_removed\":$removed}"
done | paste -sd, -)

# Get diff (truncated to ~50KB for API)
DIFF=$(git diff HEAD~1 HEAD 2>/dev/null | head -c 50000)

# Send to Alaz (non-blocking, fire-and-forget)
curl -s -X POST "$ALAZ_URL/api/v1/git/ingest" \
    -H "X-API-Key: $ALAZ_API_KEY" \
    -H "Content-Type: application/json" \
    -d "$(jq -n \
        --arg project "$PROJECT_NAME" \
        --arg commit_hash "$COMMIT_HASH" \
        --arg commit_message "$COMMIT_MESSAGE" \
        --arg diff "$DIFF" \
        --argjson files "[$FILES]" \
        '{project: $project, commit_hash: $commit_hash, commit_message: $commit_message, files: $files, diff: $diff}'
    )" > /dev/null 2>&1 &
