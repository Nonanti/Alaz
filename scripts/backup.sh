#!/bin/bash
set -euo pipefail
BACKUP_DIR="${BACKUP_DIR:-./backups}"
DB_HOST="${DB_HOST:-localhost}"
DB_PORT="${DB_PORT:-5432}"
DB_USER="${DB_USER:-alaz}"
DB_NAME="${DB_NAME:-alaz}"
KEEP_DAYS=7

mkdir -p "$BACKUP_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="$BACKUP_DIR/alaz_${TIMESTAMP}.sql.gz"

echo "[$(date)] Starting backup..."
PGPASSWORD="${PGPASSWORD:-alaz}" pg_dump \
    -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" "$DB_NAME" \
    --no-owner --no-acl | gzip > "$BACKUP_FILE"

SIZE=$(du -h "$BACKUP_FILE" | cut -f1)
echo "[$(date)] Backup complete: $BACKUP_FILE ($SIZE)"

find "$BACKUP_DIR" -name "alaz_*.sql.gz" -mtime +"$KEEP_DAYS" -delete
REMAINING=$(find "$BACKUP_DIR" -name "alaz_*.sql.gz" | wc -l)
echo "[$(date)] Kept $REMAINING backups (pruned older than ${KEEP_DAYS} days)"
