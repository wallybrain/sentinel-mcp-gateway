#!/bin/bash
# Launch sentinel-gateway with env vars from .env
# Used by remote mcporter (Hetzner) over SSH tunnel.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# Export all vars from .env
set -a
source .env
set +a

# Construct DATABASE_URL from POSTGRES_PASSWORD (same as add-mcp.sh)
export DATABASE_URL="postgres://sentinel:${POSTGRES_PASSWORD}@127.0.0.1:5432/sentinel"

exec ./target/release/sentinel-gateway --config sentinel.toml
