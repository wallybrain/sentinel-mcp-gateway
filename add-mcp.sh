#!/bin/bash
# Register sentinel-gateway as an MCP server in Claude Code.
# Run this OUTSIDE of Claude Code (in a regular terminal).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ENV_FILE="${SCRIPT_DIR}/.env"
BINARY="${SCRIPT_DIR}/target/release/sentinel-gateway"
CONFIG="${SCRIPT_DIR}/sentinel.toml"

# --- Verify prerequisites ---

if [[ ! -f "$ENV_FILE" ]]; then
  echo "Error: .env not found at $ENV_FILE"
  echo "Run: cp .env.example .env  (then fill in values)"
  echo "  or: ./scripts/setup.sh"
  exit 1
fi

if [[ ! -f "$BINARY" ]]; then
  echo "Error: Binary not found at $BINARY"
  echo "Run: cargo build --release"
  exit 1
fi

if [[ ! -f "$CONFIG" ]]; then
  echo "Error: Config not found at $CONFIG"
  echo "Run: cp sentinel.toml.example sentinel.toml  (then configure backends)"
  exit 1
fi

# --- Read env vars ---
read_env() { sed -n "s/^${1}=//p" "$ENV_FILE" | head -1; }

JWT_SECRET_KEY="$(read_env JWT_SECRET_KEY)"
POSTGRES_PASSWORD="$(read_env POSTGRES_PASSWORD)"
SENTINEL_TOKEN="$(read_env SENTINEL_TOKEN)"
FIRECRAWL_API_KEY="$(read_env FIRECRAWL_API_KEY)"
BACKEND_SHARED_SECRET="$(read_env BACKEND_SHARED_SECRET)"

if [[ -z "$JWT_SECRET_KEY" || "$JWT_SECRET_KEY" == "change-me-in-production" ]]; then
  echo "Error: JWT_SECRET_KEY not set in .env"
  exit 1
fi

if [[ -z "$SENTINEL_TOKEN" || "$SENTINEL_TOKEN" == "change-me-generate-with-jwt-secret" ]]; then
  echo "Error: SENTINEL_TOKEN not set in .env"
  echo "See docs/DEPLOYMENT.md for generation instructions."
  exit 1
fi

# --- Build MCP registration JSON ---
JSON=$(python3 -c "
import json, sys

env = {
    'JWT_SECRET_KEY': sys.argv[1],
    'SENTINEL_TOKEN': sys.argv[2],
    'DATABASE_URL': 'postgres://sentinel:' + sys.argv[3] + '@127.0.0.1:5432/sentinel',
}

# Add optional env vars only if set
if sys.argv[4]:
    env['FIRECRAWL_API_KEY'] = sys.argv[4]
if sys.argv[5]:
    env['BACKEND_SHARED_SECRET'] = sys.argv[5]

print(json.dumps({
    'command': sys.argv[6],
    'args': ['--config', sys.argv[7]],
    'env': env
}))
" "$JWT_SECRET_KEY" "$SENTINEL_TOKEN" "$POSTGRES_PASSWORD" \
  "$FIRECRAWL_API_KEY" "$BACKEND_SHARED_SECRET" \
  "$BINARY" "$CONFIG")

echo "Generated MCP config:"
echo "$JSON" | python3 -m json.tool
echo ""

claude mcp add-json sentinel-gateway "$JSON"

echo ""
echo "Done. Restart Claude Code and run /mcp to verify."
