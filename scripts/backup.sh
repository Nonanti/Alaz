#!/usr/bin/env bash
set -euo pipefail
# Alaz Database Backup Script
#
# Usage: bash scripts/backup.sh
# Cron:  0 3 * * * cd /path/to/alaz && bash scripts/backup.sh
#
# Env vars (with sensible defaults):
#   BACKUP_DIR   Where to write backups (default: ./backups)
#   DB_HOST      Postgres host (default: localhost)
#   DB_PORT      Postgres port (default: 5435)
#   DB_USER      Postgres user (default: alaz)
#   DB_NAME      Database name (default: alaz)
#   PGPASSWORD   Postgres password (required; never hardcode)
#   KEEP_DAYS    Prune backups older than N days (default: 7)

BACKUP_DIR="${BACKUP_DIR:-./backups}"
DB_HOST="${DB_HOST:-localhost}"
DB_PORT="${DB_PORT:-5435}"
DB_USER="${DB_USER:-alaz}"
DB_NAME="${DB_NAME:-alaz}"
KEEP_DAYS="${KEEP_DAYS:-7}"

: "${PGPASSWORD:?PGPASSWORD not set}"

mkdir -p "$BACKUP_DIR"

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="$BACKUP_DIR/alaz_${TIMESTAMP}.sql.gz"

echo "[$(date)] Starting backup..."

pg_dump \
    -h "$DB_HOST" \
    -p "$DB_PORT" \
    -U "$DB_USER" \
    "$DB_NAME" \
    --no-owner \
    --no-acl \
    | gzip > "$BACKUP_FILE"

SIZE=$(du -h "$BACKUP_FILE" | cut -f1)
echo "[$(date)] Backup complete: $BACKUP_FILE ($SIZE)"

# Prune old backups
find "$BACKUP_DIR" -name "alaz_*.sql.gz" -mtime +"$KEEP_DAYS" -delete
REMAINING=$(find "$BACKUP_DIR" -name "alaz_*.sql.gz" | wc -l)
echo "[$(date)] Kept $REMAINING backups (pruned older than ${KEEP_DAYS} days)"
