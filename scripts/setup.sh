#!/bin/bash
# Sentinel Gateway — Interactive Setup
# Generates .env and sentinel.toml from templates.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"

echo "=== Sentinel Gateway Setup ==="
echo ""

# --- Check prerequisites ---
for cmd in openssl python3 cargo; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "Warning: $cmd not found. Install it before building."
  fi
done

# --- Generate .env ---
if [[ -f "$ROOT/.env" ]]; then
  echo ".env already exists. Skipping. (Delete it to regenerate.)"
else
  echo "Generating .env with random secrets..."

  JWT_SECRET=$(openssl rand -base64 32)
  PG_PASS=$(openssl rand -base64 24 | tr -d '/+=' | head -c 24)
  HEALTH_TOKEN=$(python3 -c "import secrets; print(secrets.token_urlsafe(32))")
  BACKEND_SECRET=$(openssl rand -base64 32)

  cat > "$ROOT/.env" <<EOF
# Sentinel Gateway — Generated $(date +%Y-%m-%d)
# KEEP THIS FILE SECRET. Never commit it.

# === REQUIRED ===
JWT_SECRET_KEY=${JWT_SECRET}
POSTGRES_PASSWORD=${PG_PASS}
SENTINEL_TOKEN=generate-after-setup-see-below

# === OPTIONAL (uncomment as needed) ===
# N8N_API_KEY=
HEALTH_TOKEN=${HEALTH_TOKEN}
BACKEND_SHARED_SECRET=${BACKEND_SECRET}
# FIRECRAWL_API_KEY=

# === SIDECAR BUILD PATHS (for docker compose) ===
# N8N_MCP_PATH=/path/to/n8n-mcp-server
# SQLITE_MCP_PATH=/path/to/sqlite-mcp-server
# SQLITE_DB_DIR=./data
EOF

  echo "Created .env with generated secrets."
  echo ""
  echo "--- IMPORTANT: Generate SENTINEL_TOKEN ---"
  echo ""
  echo "Run this command (replacing the JWT_SECRET_KEY with yours from .env):"
  echo ""
  echo "  python3 -c \""
  echo "  import json, base64, hmac, hashlib, time"
  echo "  secret = '${JWT_SECRET}'"
  echo "  header = base64.urlsafe_b64encode(json.dumps({'alg':'HS256','typ':'JWT'}).encode()).rstrip(b'=').decode()"
  echo "  now = int(time.time())"
  echo "  payload = base64.urlsafe_b64encode(json.dumps({"
  echo "    'sub':'admin','role':'admin','iss':'sentinel-gateway',"
  echo "    'aud':'sentinel-api','iat':now,'exp':now+86400*365"
  echo "  }).encode()).rstrip(b'=').decode()"
  echo "  sig = base64.urlsafe_b64encode(hmac.new(secret.encode(),"
  echo "    f'{header}.{payload}'.encode(), hashlib.sha256).digest()).rstrip(b'=').decode()"
  echo "  print(f'{header}.{payload}.{sig}')"
  echo "  \""
  echo ""
  echo "Then paste the output as SENTINEL_TOKEN in .env"
fi

echo ""

# --- Generate sentinel.toml ---
if [[ -f "$ROOT/sentinel.toml" ]]; then
  echo "sentinel.toml already exists. Skipping. (Delete it to regenerate.)"
else
  cp "$ROOT/sentinel.toml.example" "$ROOT/sentinel.toml"
  echo "Created sentinel.toml from template."
  echo "Edit it to uncomment the backends you want to use."
fi

echo ""
echo "=== Setup complete ==="
echo ""
echo "Next steps:"
echo "  1. Generate SENTINEL_TOKEN (see instructions above)"
echo "  2. Edit sentinel.toml — uncomment your backends"
echo "  3. Build: cargo build --release"
echo "  4. (Optional) Start sidecars: docker compose up -d"
echo "  5. Register with Claude Code: ./add-mcp.sh"
echo ""
echo "Full guide: docs/DEPLOYMENT.md"
