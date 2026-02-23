# Public-Readiness Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make sentinel-gateway clone-and-run ready for any developer on a naked VPS, with BSL 1.1 licensing and OpenClaw integration docs.

**Architecture:** In-place cleanup of existing repo. Parameterize all hardcoded paths, add deployment docs, add OpenClaw security positioning, add BSL 1.1 license. No separate fork.

**Tech Stack:** Rust (existing), Bash (setup script), Markdown (docs), TOML (config templates)

---

### Task 1: Add BSL 1.1 LICENSE file

**Files:**
- Create: `LICENSE`
- Modify: `Cargo.toml:4` (change license field)

**Step 1: Create LICENSE file**

Write `LICENSE` with the full BSL 1.1 text:
- Licensor: Wally Blanchard
- Licensed Work: Sentinel Gateway
- Additional Use Grant: Production use by any individual or organization is permitted. Use in a product or service that offers the Licensed Work as a managed or hosted MCP gateway to third parties is not permitted.
- Change License: Apache License, Version 2.0
- Change Date: 2030-02-23
- Include the full BSL 1.1 legal text from https://mariadb.com/bsl11/

**Step 2: Update Cargo.toml license field**

Change `license = "Proprietary"` to `license = "BSL-1.1"`

**Step 3: Commit**

```bash
git add LICENSE Cargo.toml
git commit -m "license: adopt BSL 1.1 with Apache 2.0 change license"
```

---

### Task 2: Update .gitignore and remove .planning/ from tracking

**Files:**
- Modify: `.gitignore`

**Step 1: Add entries to .gitignore**

Append these entries to `.gitignore`:

```gitignore
# Runtime config (user-specific paths)
sentinel.toml

# Internal project planning
.planning/
```

Note: `docker-compose.override.yml` is already gitignored.

**Step 2: Remove .planning/ from git tracking (keep files on disk)**

```bash
git rm -r --cached .planning/
```

This removes 94 files from tracking while preserving them locally.

**Step 3: Commit**

```bash
git add .gitignore
git commit -m "chore: gitignore sentinel.toml, .planning/; untrack planning files"
```

---

### Task 3: Create sentinel.toml.example

**Files:**
- Create: `sentinel.toml.example`

**Step 1: Write sentinel.toml.example**

This is the template users copy to `sentinel.toml` and customize. All backends are commented out as examples. Uses `npx -y` for stdio backends (portable, no absolute paths).

```toml
# Sentinel Gateway Configuration
# Copy this file to sentinel.toml and uncomment the backends you need.
# Secrets are referenced by env var name — never stored in this file.

[gateway]
# Address to listen on (use 127.0.0.1 for local, 0.0.0.0 inside Docker)
listen = "127.0.0.1:9200"
# Log level: trace, debug, info, warn, error
log_level = "info"
# Enable audit logging to Postgres (requires DATABASE_URL)
audit_enabled = true
# Health/metrics endpoint
health_listen = "127.0.0.1:9201"

[auth]
# Env var containing the JWT secret (NOT the secret itself)
jwt_secret_env = "JWT_SECRET_KEY"
jwt_issuer = "sentinel-gateway"
jwt_audience = "sentinel-api"

[postgres]
# Env var containing the Postgres connection URL
url_env = "DATABASE_URL"
max_connections = 10

# ============================================================================
# Backends — uncomment and configure the ones you need.
# Two types: "http" (URL-based) and "stdio" (managed child process).
# ============================================================================

# --- HTTP Backends ---
# These connect to MCP servers running as separate services (Docker, etc.)

# [[backends]]
# name = "n8n"
# type = "http"
# url = "http://127.0.0.1:3001"
# timeout_secs = 60
# retries = 3

# [[backends]]
# name = "sqlite"
# type = "http"
# url = "http://127.0.0.1:3002"
# timeout_secs = 60
# retries = 3

# --- stdio Backends ---
# These are spawned and managed by the gateway as child processes.
# Uses npx for portability — works on any machine with Node.js installed.

# [[backends]]
# name = "context7"
# type = "stdio"
# command = "npx"
# args = ["-y", "@upstash/context7-mcp"]
# restart_on_exit = true
# max_restarts = 5

# [[backends]]
# name = "firecrawl"
# type = "stdio"
# command = "npx"
# args = ["-y", "firecrawl-mcp"]

# [[backends]]
# name = "sequential-thinking"
# type = "stdio"
# command = "npx"
# args = ["-y", "@modelcontextprotocol/server-sequential-thinking"]

# [[backends]]
# name = "playwright"
# type = "stdio"
# command = "npx"
# args = ["-y", "@playwright/mcp"]

# ============================================================================
# RBAC — Role-Based Access Control
# Each role defines permissions and optionally denied tools.
# ============================================================================

[rbac.roles.admin]
permissions = ["*"]

[rbac.roles.developer]
permissions = ["tools.read", "tools.execute"]
denied_tools = []

[rbac.roles.viewer]
permissions = ["tools.read"]

# ============================================================================
# Rate Limits
# default_rpm applies to all tools unless overridden per_tool.
# ============================================================================

[rate_limits]
default_rpm = 1000

[rate_limits.per_tool]
# example: limit expensive workflow executions
# execute_workflow = 10

# ============================================================================
# Kill Switch — emergency disable for tools or backends without restart.
# ============================================================================

[kill_switch]
disabled_tools = []
disabled_backends = []
```

**Step 2: Commit**

```bash
git add sentinel.toml.example
git commit -m "config: add sentinel.toml.example with portable npx backends"
```

---

### Task 4: Create sentinel-docker.toml

**Files:**
- Create: `sentinel-docker.toml`

**Step 1: Write sentinel-docker.toml**

Docker-specific config. HTTP backends only — stdio processes can't run inside the minimal container. The Dockerfile already copies this to `/etc/sentinel/sentinel.toml`.

```toml
# Sentinel Gateway — Docker Configuration
# This file is copied into the Docker image at build time.
# Only HTTP backends are supported in Docker mode (no stdio).

[gateway]
listen = "0.0.0.0:9200"
log_level = "info"
audit_enabled = true
health_listen = "0.0.0.0:9201"

[auth]
jwt_secret_env = "JWT_SECRET_KEY"
jwt_issuer = "sentinel-gateway"
jwt_audience = "sentinel-api"

[postgres]
url_env = "DATABASE_URL"
max_connections = 10

[[backends]]
name = "n8n"
type = "http"
url = "http://mcp-n8n:3000"
timeout_secs = 60
retries = 3

[[backends]]
name = "sqlite"
type = "http"
url = "http://mcp-sqlite:3000"
timeout_secs = 60
retries = 3

[rbac.roles.admin]
permissions = ["*"]

[rbac.roles.developer]
permissions = ["tools.read", "tools.execute"]
denied_tools = []

[rbac.roles.viewer]
permissions = ["tools.read"]

[rate_limits]
default_rpm = 1000

[kill_switch]
disabled_tools = []
disabled_backends = []
```

**Step 2: Commit**

```bash
git add sentinel-docker.toml
git commit -m "config: add sentinel-docker.toml for containerized deployment"
```

---

### Task 5: Create docker-compose.override.yml.example

**Files:**
- Create: `docker-compose.override.yml.example`

**Step 1: Write the override example**

```yaml
# Docker Compose Override — Server-Specific Configuration
# Copy to docker-compose.override.yml and customize paths for your server.
# This file is gitignored.
#
# The base docker-compose.yml uses sensible defaults. This override is only
# needed if you have custom build contexts or volume paths.

services:
  mcp-n8n:
    build:
      # Point to your local clone of https://github.com/nicepkg/n8n-mcp-server
      context: /path/to/n8n-mcp-server
      dockerfile: /path/to/sentinel-gateway/sidecars/Dockerfile.n8n

  mcp-sqlite:
    build:
      # Point to your local clone of the sqlite MCP server
      context: /path/to/sqlite-mcp-server
      dockerfile: /path/to/sentinel-gateway/sidecars/Dockerfile.mcp-sqlite
    volumes:
      # Map your database directory into the container
      - /path/to/your/databases:/data
```

**Step 2: Commit**

```bash
git add docker-compose.override.yml.example
git commit -m "config: add docker-compose.override.yml.example"
```

---

### Task 6: Update docker-compose.yml with portable paths

**Files:**
- Modify: `docker-compose.yml`

**Step 1: Replace hardcoded paths with env var fallbacks**

Replace the `mcp-n8n` build context:
```yaml
  mcp-n8n:
    build:
      context: ${N8N_MCP_PATH:?Set N8N_MCP_PATH to your n8n-mcp-server directory}
      dockerfile: sidecars/Dockerfile.n8n
```

Replace the `mcp-sqlite` build context and volume:
```yaml
  mcp-sqlite:
    build:
      context: ${SQLITE_MCP_PATH:?Set SQLITE_MCP_PATH to your sqlite-mcp-server directory}
      dockerfile: sidecars/Dockerfile.mcp-sqlite
    # ...
    volumes:
      - ${SQLITE_DB_DIR:-./data}:/data
```

Note: Using `${VAR:?message}` makes Docker Compose fail with a clear error if the var is missing, instead of silently using a wrong path.

**Step 2: Add the new env vars to .env.example**

Append to `.env.example`:
```
# --- Sidecar Build Paths (for docker-compose) ---
# Absolute path to your clone of the n8n MCP server repo
N8N_MCP_PATH=/path/to/n8n-mcp-server
# Absolute path to your clone of the sqlite MCP server repo
SQLITE_MCP_PATH=/path/to/sqlite-mcp-server
# Directory containing SQLite databases to expose
SQLITE_DB_DIR=/path/to/databases
```

**Step 3: Also add FIRECRAWL_API_KEY and mark required vs optional**

Update `.env.example` to group vars as required vs optional:
```
# Sentinel Gateway - Environment Variables
# Copy to .env and replace with real values.

# === REQUIRED ===

# JWT secret key for token validation (generate: openssl rand -base64 32)
JWT_SECRET_KEY=change-me-in-production

# PostgreSQL password (used by Docker Compose and DATABASE_URL)
POSTGRES_PASSWORD=change-me-in-production

# Sentinel session token (JWT signed with JWT_SECRET_KEY, role=admin)
# Generate: see scripts/setup.sh or docs/DEPLOYMENT.md
SENTINEL_TOKEN=change-me-generate-with-jwt-secret

# === OPTIONAL (enable as needed) ===

# n8n API key (copy from n8n dashboard → Settings → API)
# N8N_API_KEY=

# Bearer token for /metrics endpoint (generate: python3 -c "import secrets; print(secrets.token_urlsafe(32))")
# HEALTH_TOKEN=

# Shared secret for HTTP sidecar backends (generate: openssl rand -base64 32)
# BACKEND_SHARED_SECRET=

# Firecrawl API key (get from https://firecrawl.dev)
# FIRECRAWL_API_KEY=

# === SIDECAR BUILD PATHS (for docker-compose) ===

# Absolute path to your clone of the n8n MCP server repo
# N8N_MCP_PATH=/path/to/n8n-mcp-server

# Absolute path to your clone of the sqlite MCP server repo
# SQLITE_MCP_PATH=/path/to/sqlite-mcp-server

# Directory containing SQLite databases to expose via mcp-sqlite
# SQLITE_DB_DIR=./data
```

**Step 4: Commit**

```bash
git add docker-compose.yml .env.example
git commit -m "config: parameterize docker-compose paths, reorganize .env.example"
```

---

### Task 7: Rewrite add-mcp.sh to be portable

**Files:**
- Modify: `add-mcp.sh`

**Step 1: Rewrite add-mcp.sh**

Replace entire file. The new version auto-detects the repo directory and reads from local `.env`.

```bash
#!/bin/bash
# Register sentinel-gateway as an MCP server in Claude Code.
# Run this OUTSIDE of Claude Code (in a regular terminal).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ENV_FILE="${SCRIPT_DIR}/.env"
BINARY="${SCRIPT_DIR}/target/release/sentinel-gateway"
CONFIG="${SCRIPT_DIR}/sentinel.toml"

# Verify prerequisites
if [[ ! -f "$ENV_FILE" ]]; then
  echo "Error: .env not found at $ENV_FILE"
  echo "Copy .env.example to .env and fill in your values."
  exit 1
fi

if [[ ! -f "$BINARY" ]]; then
  echo "Error: Binary not found at $BINARY"
  echo "Run: cargo build --release"
  exit 1
fi

if [[ ! -f "$CONFIG" ]]; then
  echo "Error: Config not found at $CONFIG"
  echo "Copy sentinel.toml.example to sentinel.toml and configure your backends."
  exit 1
fi

# Read env vars
read_env() { sed -n "s/^${1}=//p" "$ENV_FILE" | head -1; }

JWT_SECRET_KEY="$(read_env JWT_SECRET_KEY)"
POSTGRES_PASSWORD="$(read_env POSTGRES_PASSWORD)"
SENTINEL_TOKEN="$(read_env SENTINEL_TOKEN)"
FIRECRAWL_API_KEY="$(read_env FIRECRAWL_API_KEY)"
BACKEND_SHARED_SECRET="$(read_env BACKEND_SHARED_SECRET)"

# Build JSON with python to avoid shell quoting issues
JSON=$(python3 -c "
import json, sys, os
env = {
    'JWT_SECRET_KEY': sys.argv[1],
    'SENTINEL_TOKEN': sys.argv[2],
    'DATABASE_URL': 'postgres://sentinel:' + sys.argv[3] + '@127.0.0.1:5432/sentinel',
}
# Add optional env vars only if set
for key in ['FIRECRAWL_API_KEY', 'BACKEND_SHARED_SECRET']:
    val = sys.argv[4 if key == 'FIRECRAWL_API_KEY' else 5]
    if val:
        env[key] = val

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

claude mcp add-json sentinel-gateway "$JSON"

echo ""
echo "Done. Restart Claude Code and run /mcp to verify."
```

**Step 2: Commit**

```bash
git add add-mcp.sh
git commit -m "chore: rewrite add-mcp.sh for portable path detection"
```

---

### Task 8: Create scripts/setup.sh

**Files:**
- Create: `scripts/setup.sh`

**Step 1: Write interactive setup script**

```bash
#!/bin/bash
# Sentinel Gateway — Interactive Setup
# Generates .env and sentinel.toml from templates.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"

echo "=== Sentinel Gateway Setup ==="
echo ""

# --- Generate .env ---
if [[ -f "$ROOT/.env" ]]; then
  echo ".env already exists. Skipping. (Delete it to regenerate.)"
else
  echo "Generating .env with random secrets..."

  JWT_SECRET=$(openssl rand -base64 32)
  PG_PASS=$(openssl rand -base64 24)
  HEALTH_TOKEN=$(python3 -c "import secrets; print(secrets.token_urlsafe(32))")
  BACKEND_SECRET=$(openssl rand -base64 32)

  cat > "$ROOT/.env" <<EOF
# Sentinel Gateway — Generated $(date +%Y-%m-%d)
# KEEP THIS FILE SECRET. Never commit it.

# === REQUIRED ===
JWT_SECRET_KEY=${JWT_SECRET}
POSTGRES_PASSWORD=${PG_PASS}
SENTINEL_TOKEN=generate-after-setup-see-docs

# === OPTIONAL ===
# N8N_API_KEY=
HEALTH_TOKEN=${HEALTH_TOKEN}
BACKEND_SHARED_SECRET=${BACKEND_SECRET}
# FIRECRAWL_API_KEY=

# === SIDECAR BUILD PATHS ===
# N8N_MCP_PATH=/path/to/n8n-mcp-server
# SQLITE_MCP_PATH=/path/to/sqlite-mcp-server
# SQLITE_DB_DIR=./data
EOF

  echo "Created .env with generated secrets."
  echo ""
  echo "IMPORTANT: You still need to generate SENTINEL_TOKEN."
  echo "See docs/DEPLOYMENT.md for instructions."
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
echo "  1. Edit .env — set SENTINEL_TOKEN (see docs/DEPLOYMENT.md)"
echo "  2. Edit sentinel.toml — uncomment your backends"
echo "  3. Build: cargo build --release"
echo "  4. (Optional) Start sidecars: docker compose up -d"
echo "  5. Register with Claude Code: ./add-mcp.sh"
echo ""
echo "Full guide: docs/DEPLOYMENT.md"
```

**Step 2: Make executable**

```bash
chmod +x scripts/setup.sh
```

**Step 3: Commit**

```bash
git add scripts/setup.sh
git commit -m "feat: add interactive setup script"
```

---

### Task 9: Write docs/DEPLOYMENT.md

**Files:**
- Create: `docs/DEPLOYMENT.md`

**Step 1: Write the deployment guide**

Full step-by-step for a naked VPS. Contents:

1. **Prerequisites** — Rust (via rustup), Node.js 20+ (for stdio backends), Docker + Docker Compose (for sidecars), PostgreSQL (via Docker or system), Python 3 (for setup script)
2. **Quick Start** (5 steps) — clone, setup.sh, build, docker compose up, add-mcp.sh
3. **Detailed Setup** — each step explained with expected output
4. **Generating SENTINEL_TOKEN** — python3 one-liner using PyJWT or a curl to a JWT generator
5. **Configuring Backends** — how to add/remove backends in sentinel.toml
6. **Running Without Docker** — native postgres, manual sidecar management
7. **Security Hardening** — firewall rules, TLS termination, log rotation
8. **Troubleshooting** — common errors and fixes
9. **Going Public Checklist** — steps to run before flipping repo to public visibility

See Task 9 implementation for full content (too long to inline here — write the complete file during execution).

**Step 2: Commit**

```bash
git add docs/DEPLOYMENT.md
git commit -m "docs: add DEPLOYMENT.md for naked VPS setup"
```

---

### Task 10: Write docs/OPENCLAW.md

**Files:**
- Create: `docs/OPENCLAW.md`

**Step 1: Write OpenClaw integration guide**

Contents:

1. **The Problem** — OpenClaw's MCP security gaps (CVE-2026-25253, 1800+ exposed instances, 92% exploitation probability with 10 plugins, Microsoft's isolation advisory)
2. **How Sentinel Fills the Gap** — feature-by-feature mapping:
   - JWT auth → prevents unauthorized MCP access
   - RBAC → restrict which tools agents can call
   - Rate limiting → prevent runaway agents
   - Audit logging → full request/response trail
   - Circuit breakers → isolate failing backends
   - Kill switch → emergency disable without restart
3. **Architecture** — ASCII diagram showing OpenClaw → Sentinel → MCP servers
4. **Setup Guide** — step-by-step to point OpenClaw at Sentinel:
   - Configure OpenClaw's `mcpServers` to use Sentinel as the gateway
   - Example JSON config for OpenClaw's gateway settings
5. **Recommended RBAC Policies** — example policies for OpenClaw agents (restrict filesystem, limit API calls, deny dangerous tools)
6. **Comparison** — Sentinel vs Runlayer vs CrowdStrike AIDR vs no gateway

See Task 10 implementation for full content.

**Step 2: Commit**

```bash
git add docs/OPENCLAW.md
git commit -m "docs: add OpenClaw integration guide"
```

---

### Task 11: Rewrite README.md

**Files:**
- Modify: `README.md`

**Step 1: Rewrite for public audience**

New structure:
1. **Title + tagline** — "Sentinel Gateway — Secure MCP gateway for AI agents"
2. **Badges** — license (BSL 1.1), Rust version, build status placeholder
3. **One-paragraph description** — what it does, who it's for
4. **OpenClaw callout** — "Using OpenClaw? See our [security integration guide](docs/OPENCLAW.md)"
5. **Architecture diagram** (keep existing, clean up)
6. **Features list** (keep existing)
7. **Quick Start** — 5-step clone→run (link to DEPLOYMENT.md for details)
8. **Configuration** — link to sentinel.toml.example, explain the pattern
9. **Documentation** — table of docs with links
10. **Security** — brief note on the IBM/Anthropic whitepaper alignment
11. **License** — BSL 1.1 summary with link to LICENSE
12. **Contributing** — basic build/test instructions

**Step 2: Commit**

```bash
git add README.md
git commit -m "docs: rewrite README for public audience with OpenClaw positioning"
```

---

### Task 12: Final verification and push

**Step 1: Verify no secrets in staged files**

```bash
git diff HEAD --cached | grep -iE '(password|secret|token|key).*=' || echo "No secrets found"
```

**Step 2: Verify .planning/ is untracked**

```bash
git ls-files .planning/ | wc -l  # should be 0
```

**Step 3: Verify sentinel.toml is gitignored**

```bash
git status sentinel.toml  # should not appear
```

**Step 4: Run tests to confirm nothing broke**

```bash
cargo test
```

**Step 5: Push**

```bash
git push origin main
```
