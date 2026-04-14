#!/usr/bin/env bash
set -euo pipefail
# deploy.sh — build and deploy Alaz to a remote host
#
# Required env vars:
#   DEPLOY_HOST   user@host of the target server
#
# Optional:
#   DEPLOY_PATH   remote path to deploy into (default: ~/alaz)
#   RESTART_CMD   command run over SSH after upload (default: systemctl --user restart alaz)

: "${DEPLOY_HOST:?DEPLOY_HOST not set (e.g. user@example.com)}"
DEPLOY_PATH="${DEPLOY_PATH:-~/alaz}"
RESTART_CMD="${RESTART_CMD:-systemctl --user restart alaz}"

echo "==> Building release binary"
cargo build --release

echo "==> Uploading binary to ${DEPLOY_HOST}:${DEPLOY_PATH}/alaz"
rsync -avz target/release/alaz "${DEPLOY_HOST}:${DEPLOY_PATH}/alaz"

echo "==> Uploading systemd unit"
rsync -avz alaz.service "${DEPLOY_HOST}:${DEPLOY_PATH}/"

echo "==> Restarting service on remote"
ssh "${DEPLOY_HOST}" "${RESTART_CMD}"

echo "==> Done."
