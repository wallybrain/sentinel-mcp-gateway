# Deployment Guide

Step-by-step instructions for deploying Sentinel Gateway on a fresh VPS or any Linux server.

---

## Prerequisites

| Dependency | Version | Purpose |
|------------|---------|---------|
| **Rust** | 1.75+ | Build the gateway binary |
| **Node.js** | 20+ | Required for stdio MCP backends (via `npx`) |
| **Docker** + **Docker Compose** | 24+ | PostgreSQL and optional HTTP sidecars |
| **Python 3** | 3.10+ | Setup script and JWT token generation |
| **OpenSSL** | any | Secret generation |

### Install Prerequisites (Ubuntu/Debian)

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Node.js (via nvm)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.0/install.sh | bash
source ~/.bashrc
nvm install 20

# Docker
curl -fsSL https://get.docker.com | sh

# Python 3 (usually pre-installed)
sudo apt install -y python3
```

---

## Quick Start

```bash
# 1. Clone
git clone https://github.com/wallybrain/sentinel-mcp-gateway.git
cd sentinel-gateway

# 2. Setup (generates .env and sentinel.toml)
./scripts/setup.sh

# 3. Generate SENTINEL_TOKEN (see "Generating Tokens" below)

# 4. Build
cargo build --release

# 5. Start PostgreSQL
docker compose up -d postgres

# 6. Register with Claude Code
./add-mcp.sh
```

---

## Detailed Setup

### 1. Clone the Repository

```bash
git clone https://github.com/wallybrain/sentinel-mcp-gateway.git
cd sentinel-gateway
```

### 2. Run the Setup Script

```bash
./scripts/setup.sh
```

This generates:
- `.env` with random secrets (`JWT_SECRET_KEY`, `POSTGRES_PASSWORD`, `HEALTH_TOKEN`, `BACKEND_SHARED_SECRET`)
- `sentinel.toml` copied from `sentinel.toml.example`

Output:

```
=== Sentinel Gateway Setup ===

Generating .env with random secrets...
Created .env with generated secrets.

Created sentinel.toml from template.
Edit it to uncomment the backends you want to use.

=== Setup complete ===
```

### 3. Generate SENTINEL_TOKEN

The gateway requires a JWT token for session authentication. Generate one using the `JWT_SECRET_KEY` from your `.env`:

```bash
# Read your JWT secret
JWT_SECRET=$(grep '^JWT_SECRET_KEY=' .env | cut -d= -f2-)

# Generate a 1-year admin token
python3 -c "
import json, base64, hmac, hashlib, time
secret = '$JWT_SECRET'
header = base64.urlsafe_b64encode(json.dumps({'alg':'HS256','typ':'JWT'}).encode()).rstrip(b'=').decode()
now = int(time.time())
payload = base64.urlsafe_b64encode(json.dumps({
  'sub':'admin','role':'admin','iss':'sentinel-gateway',
  'aud':'sentinel-api','iat':now,'exp':now+86400*365
}).encode()).rstrip(b'=').decode()
sig = base64.urlsafe_b64encode(hmac.new(secret.encode(),
  f'{header}.{payload}'.encode(), hashlib.sha256).digest()).rstrip(b'=').decode()
print(f'{header}.{payload}.{sig}')
"
```

Paste the output into `.env` as `SENTINEL_TOKEN=<token>`.

**Important**: The Rust gateway uses the JWT secret as raw bytes (`secret.as_bytes()`), not base64-decoded. The Python snippet above matches this behavior. Do not base64-encode or decode the secret.

### 4. Configure Backends

Edit `sentinel.toml` and uncomment the backends you want. See [sentinel.toml.example](../sentinel.toml.example) for all options.

**Minimal example** (just Context7 for documentation lookup):

```toml
[[backends]]
name = "context7"
type = "stdio"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
restart_on_exit = true
max_restarts = 5
```

### 5. Build the Binary

```bash
cargo build --release
```

The binary is at `target/release/sentinel-gateway` (~14 MB).

Expected output:

```
   Compiling sentinel-gateway v0.1.0
    Finished `release` profile [optimized] target(s)
```

### 6. Start PostgreSQL

PostgreSQL stores audit logs. The included Docker Compose file runs it locally:

```bash
docker compose up -d postgres
```

Verify it's healthy:

```bash
docker compose ps
# NAME               STATUS         PORTS
# sentinel-postgres  Up (healthy)   127.0.0.1:5432->5432/tcp
```

### 7. Register with Claude Code

```bash
./add-mcp.sh
```

This registers the gateway binary as an MCP server in Claude Code's config. Restart Claude Code and run `/mcp` to verify.

---

## Optional: HTTP Sidecar Backends

The Docker Compose file can also run HTTP-based MCP backends (n8n, SQLite). These require external repos:

```bash
# Clone the sidecar repos
git clone https://github.com/nicepkg/n8n-mcp-server ../n8n-mcp-server
git clone https://github.com/nicepkg/sqlite-mcp-server ../sqlite-mcp-server

# Set paths in .env
echo "N8N_MCP_PATH=$(pwd)/../n8n-mcp-server" >> .env
echo "SQLITE_MCP_PATH=$(pwd)/../sqlite-mcp-server" >> .env
echo "SQLITE_DB_DIR=$(pwd)/data" >> .env

# Create data directory
mkdir -p data

# Start all services
docker compose up -d
```

Or create a `docker-compose.override.yml` from the template:

```bash
cp docker-compose.override.yml.example docker-compose.override.yml
# Edit paths in the override file
```

---

## Running Without Docker

If you prefer not to use Docker, install PostgreSQL natively:

```bash
# Ubuntu/Debian
sudo apt install postgresql postgresql-client

# Create database and user
sudo -u postgres createuser sentinel
sudo -u postgres createdb sentinel -O sentinel
sudo -u postgres psql -c "ALTER USER sentinel PASSWORD 'your-password';"
```

Set `DATABASE_URL` in `.env`:

```
DATABASE_URL=postgres://sentinel:your-password@127.0.0.1:5432/sentinel
```

Run the gateway directly:

```bash
./target/release/sentinel-gateway --config sentinel.toml
```

---

## Security Hardening

### Bind to Localhost

The gateway binds to `127.0.0.1` by default. Never expose it on `0.0.0.0` without a reverse proxy and TLS.

### TLS Termination

Use a reverse proxy (Caddy, nginx) for HTTPS:

```
# Caddy example
sentinel.yourdomain.com {
    reverse_proxy 127.0.0.1:9200
}
```

### Firewall

Block external access to gateway and sidecar ports:

```bash
# Allow only loopback
iptables -A INPUT -p tcp --dport 9200 -i lo -j ACCEPT
iptables -A INPUT -p tcp --dport 9200 -j DROP
iptables -A INPUT -p tcp --dport 9201 -j DROP
iptables -A INPUT -p tcp --dport 5432 -j DROP
```

### RBAC

Define least-privilege roles in `sentinel.toml`. See [OPENCLAW.md](./OPENCLAW.md) for recommended policies.

### Rate Limits

Set conservative defaults and tighten per-tool:

```toml
[rate_limits]
default_rpm = 100

[rate_limits.per_tool]
expensive_operation = 10
```

### Hot Reload

Send SIGHUP to reload `kill_switch` and `rate_limits` without restarting:

```bash
kill -HUP $(pgrep sentinel-gateway)
```

Changes to backends, auth, or postgres config require a full restart.

### Audit Logging

Enabled by default (`audit_enabled = true`). Query audit logs:

```sql
SELECT timestamp, tool_name, backend_name, status, latency_ms
FROM audit_log
ORDER BY timestamp DESC
LIMIT 20;
```

---

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `Connection refused on port 5432` | PostgreSQL not running | `docker compose up -d postgres` |
| `JWT validation failed` | Token signed with wrong secret | Regenerate token with correct `JWT_SECRET_KEY` |
| `Backend unhealthy: n8n` | HTTP backend not reachable | Check `docker compose ps` and backend URL |
| `Backend unhealthy: context7` | npx can't download package | Check Node.js installation and network |
| `Permission denied` building Rust | Sandbox restrictions | Some environments need sandbox disabled for cargo |
| `SQLX_OFFLINE=true` error | Missing offline query data | Set `SQLX_OFFLINE=true` env var during build |
| `add-mcp.sh: .env not found` | Haven't run setup | Run `./scripts/setup.sh` first |
| `add-mcp.sh: Binary not found` | Haven't built | Run `cargo build --release` first |
