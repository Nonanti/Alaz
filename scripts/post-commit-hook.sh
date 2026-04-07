#!/bin/bash
ALAZ_URL="${ALAZ_URL:-http://localhost:3456}"
ALAZ_API_KEY="${ALAZ_API_KEY:?Set ALAZ_API_KEY environment variable}"
PROJECT_NAME=$(basename "$(git rev-parse --show-toplevel)")
COMMIT_HASH=$(git rev-parse HEAD)
COMMIT_MESSAGE=$(git log -1 --pretty=%B)

FILES=$(git diff-tree --no-commit-id -r --numstat --diff-filter=ACDMR "$COMMIT_HASH" | while read added removed path; do
    [ "$added" = "-" ] && added=0
    [ "$removed" = "-" ] && removed=0
    change_type="modify"
    git diff-tree --no-commit-id -r --name-status "$COMMIT_HASH" | grep -q "^A.*$path" && change_type="add"
    git diff-tree --no-commit-id -r --name-status "$COMMIT_HASH" | grep -q "^D.*$path" && change_type="delete"
    echo "{\"path\":\"$path\",\"change_type\":\"$change_type\",\"lines_added\":$added,\"lines_removed\":$removed}"
done | paste -sd, -)

DIFF=$(git diff HEAD~1 HEAD 2>/dev/null | head -c 50000)

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
