# OpenClaw Integration Guide

> How to secure OpenClaw's MCP connections using Sentinel Gateway.

> **Status**: Production-tested. This guide documents a working deployment where an OpenClaw agent on a remote ARM64 server accesses 50 MCP tools on an x86_64 host through Sentinel Gateway via an SSH stdio tunnel over WireGuard. All tool calls are JWT-authenticated, rate-limited, and audit-logged.

---

## The Problem: OpenClaw's MCP Security Gap

OpenClaw connects to MCP backends with minimal security controls. Several high-profile vulnerabilities have made this a pressing concern:

- **CVE-2026-25253** (CVSS 8.8): One-click remote code execution via stolen authentication tokens through the Control UI.
- **1,800+ exposed instances** discovered leaking API keys, chat histories, and credentials to the public internet (VentureBeat, Feb 2026).
- **92% exploitation probability** with just 10 MCP plugins deployed (Pynt security research).
- **Microsoft advisory** (Feb 2026): OpenClaw should be treated as untrusted code execution.

The core gaps in OpenClaw's MCP layer:

| Gap | Risk |
|-----|------|
| **No MCP-layer auth** | Any process on the same host can call MCP servers directly |
| **No access control** | All agents have full access to all tools on all backends |
| **No audit trail** | No record of which tools were called, by whom, or what data was accessed |
| **No rate limiting** | Runaway or compromised agents can make unlimited tool calls |
| **No circuit breakers** | One failing backend can cascade failures across all agents |
| **No emergency controls** | Disabling a compromised tool requires restarting the entire system |

---

## How Sentinel Gateway Fills the Gap

Sentinel Gateway is a single Rust binary (~14 MB) that implements the MCP Gateway Pattern defined in the IBM/Anthropic enterprise whitepaper. It federates multiple MCP backends behind a unified security layer.

| OpenClaw Gap | Sentinel Feature |
|---|---|
| No MCP auth | JWT authentication — every request validated before reaching any backend |
| No access control | RBAC — restrict which tools each role can call, with deny lists |
| No rate limiting | Per-tool configurable rate limits with sliding window |
| No audit trail | PostgreSQL-backed request/response audit logging |
| No failure isolation | Circuit breakers — automatic backend isolation on failure |
| No emergency controls | Kill switch — disable individual tools or entire backends without restart |
| No observability | Prometheus metrics — request counts, latencies, error rates |

---

## Architecture

### The SSH Stdio Tunnel Pattern

The recommended integration uses OpenClaw's **mcporter** skill to connect to Sentinel Gateway over an SSH stdio tunnel. This pattern keeps all backends and secrets on a single secured host while giving remote OpenClaw agents full tool access.

```
┌─────────────────────────────────────────────────────────────────────┐
│  REMOTE HOST (OpenClaw)                                             │
│                                                                     │
│  OpenClaw Agent                                                     │
│      │                                                              │
│      │ mcporter skill: "mcporter call sentinel.<tool> ..."          │
│      │                                                              │
│      v                                                              │
│  mcporter CLI ──── SSH stdio ──── WireGuard / LAN ────────────┐     │
│                                                                │     │
└────────────────────────────────────────────────────────────────│─────┘
                                                                 │
┌────────────────────────────────────────────────────────────────│─────┐
│  BACKEND HOST (Sentinel + backends)                            │     │
│                                                                v     │
│  run-sentinel.sh                                                     │
│      │  sources .env (secrets never leave this host)                 │
│      │  constructs DATABASE_URL                                      │
│      v                                                              │
│  Sentinel Gateway (Rust binary)                                      │
│      │  JWT auth, RBAC, rate limiting, audit logging                │
│      │                                                              │
│      ├──→ HTTP Backends                                             │
│      │     ├── mcp-n8n (127.0.0.1:3001)                            │
│      │     └── mcp-sqlite (127.0.0.1:3002)                         │
│      │                                                              │
│      ├──→ Stdio Backends (child processes)                          │
│      │     ├── context7 (library documentation)                     │
│      │     ├── firecrawl (web scraping)                             │
│      │     ├── playwright (browser automation)                      │
│      │     └── sequential-thinking (chain-of-thought)               │
│      │                                                              │
│      └──→ PostgreSQL (127.0.0.1:5432, audit logs)                   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Why This Pattern Works

**Secrets never leave the backend host.** The `.env` file containing JWT secrets, API keys, and database credentials is sourced by `run-sentinel.sh` on the backend host. The SSH tunnel carries only MCP JSON-RPC messages — no credentials traverse the network.

**Single trust boundary.** All tool calls from all remote OpenClaw agents funnel through one Sentinel instance. One config, one audit log, one place to apply rate limits and kill switches.

**Cross-architecture compatible.** Sentinel runs on the host where it was compiled. The remote OpenClaw host doesn't need Rust, the sentinel binary, or any MCP backend packages — it only needs SSH and mcporter.

**Network-layer encryption.** SSH encrypts the stdio tunnel. When combined with WireGuard (recommended), you get double encryption with mutual authentication at the network level.

---

## Setup Guide

### Prerequisites

**Backend host** (where Sentinel and backends run):
- Sentinel Gateway binary (`cargo build --release`)
- PostgreSQL for audit logging (`docker compose up -d postgres`)
- MCP backend packages (Node.js for stdio backends)
- SSH server

**Remote OpenClaw host:**
- OpenClaw (v2026.2+ recommended)
- Node.js 22+ (for OpenClaw and mcporter)
- mcporter (`npm install -g mcporter`)
- SSH client with key-based auth to the backend host

### Step 1: Deploy Sentinel Gateway on the Backend Host

See [DEPLOYMENT.md](./DEPLOYMENT.md) for the full guide. Quick version:

```bash
git clone https://github.com/wallybrain/sentinel-mcp-gateway.git
cd sentinel-gateway
./scripts/setup.sh          # generates .env and sentinel.toml
cargo build --release       # builds target/release/sentinel-gateway
docker compose up -d postgres
```

### Step 2: Create the Launch Wrapper

Create `run-sentinel.sh` in the sentinel-gateway directory. This script sources secrets and launches the binary — it's what SSH will execute remotely.

```bash
#!/bin/bash
# Launch sentinel-gateway with env vars from .env
# Used by remote mcporter over SSH tunnel.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# Export all vars from .env (dotenv format, no 'export' prefix)
set -a
source .env
set +a

# Construct DATABASE_URL from POSTGRES_PASSWORD
export DATABASE_URL="postgres://sentinel:${POSTGRES_PASSWORD}@127.0.0.1:5432/sentinel"

exec ./target/release/sentinel-gateway --config sentinel.toml
```

```bash
chmod +x run-sentinel.sh
```

### Step 3: Set Up SSH Key Authentication

On the **remote OpenClaw host**, generate an SSH key and authorize it on the backend host:

```bash
# On the OpenClaw host:
ssh-keygen -t ed25519 -f ~/.ssh/id_ed25519 -N "" -C "openclaw-agent"

# Copy the public key to the backend host:
ssh-copy-id -i ~/.ssh/id_ed25519.pub user@backend-host

# Verify passwordless SSH works:
ssh user@backend-host "echo SSH_OK"
```

If using WireGuard or a VPN, use the tunnel IP address (e.g., `10.0.0.1`) rather than the public IP.

### Step 4: Install and Configure mcporter

On the **remote OpenClaw host**:

```bash
# Install mcporter globally
npm install -g mcporter

# Register Sentinel as a stdio server over SSH
mcporter config add sentinel \
  --command ssh \
  --arg "-o" --arg "ConnectTimeout=5" \
  --arg "user@backend-host" \
  --arg "/path/to/sentinel-gateway/run-sentinel.sh" \
  --description "Sentinel MCP Gateway (SSH tunnel)" \
  --scope home
```

Verify the connection:

```bash
# List all tools available through Sentinel
mcporter list sentinel

# Test a tool call
mcporter call sentinel.echo message="Hello from OpenClaw"
```

You should see all federated tools from every backend configured in `sentinel.toml`.

### Step 5: Enable the mcporter Skill in OpenClaw

```bash
openclaw config set skills.entries.mcporter.enabled true
systemctl --user restart openclaw-gateway
```

The OpenClaw agent can now use mcporter as a skill to call any Sentinel-protected tool:

```
mcporter call sentinel.firecrawl_scrape url=https://example.com
mcporter call sentinel.resolve-library-id libraryName=react query="hooks"
mcporter call sentinel.browser_navigate url=https://example.com
mcporter list sentinel --schema
```

### Step 6: Verify End-to-End

Run an agent turn that exercises the full pipeline:

```bash
openclaw agent --local --agent main \
  -m "Use mcporter to call sentinel.echo with message='integration test'" \
  --timeout 60
```

The agent should invoke the mcporter skill, SSH into the backend host, run sentinel, execute the echo tool, and return the result — all authenticated and audit-logged.

---

## What This Enables

The Sentinel + OpenClaw integration unlocks capabilities that neither system provides alone:

### Governed Autonomous Agents

OpenClaw agents can operate autonomously (responding to Discord messages, running on heartbeat schedules, executing cron jobs) while every tool call passes through Sentinel's security pipeline. If an agent receives a prompt injection via a chat message, Sentinel's rate limiting and RBAC prevent it from causing damage beyond its permitted scope.

### Multi-Agent Access Control

Different OpenClaw agents can receive different JWT tokens with different roles. A research agent gets read-only access; a development agent gets write access but no destructive operations; an admin agent gets full access with aggressive rate limits.

```toml
# Research agent: read-only, rate-limited
[rbac.roles.research]
permissions = ["tools.read", "tools.execute"]
denied_tools = ["filesystem__write_file", "filesystem__delete_file"]

# Development agent: write access, no destructive ops
[rbac.roles.development]
permissions = ["tools.read", "tools.execute"]
denied_tools = ["filesystem__delete_file"]
```

### Cross-Platform Tool Federation

A single Sentinel instance can serve tools to multiple AI agent platforms simultaneously. Claude Code connects via stdio (local), OpenClaw connects via SSH tunnel (remote), and any future MCP-compatible agent can connect the same way. All share the same backends, the same audit log, the same rate limits.

### Auditable AI Operations

Every tool call from every agent is logged to PostgreSQL with timestamp, agent identity (from JWT `sub` claim), tool name, backend, request parameters, response status, and latency. This provides a complete, queryable audit trail for compliance, debugging, and incident response.

```sql
-- What did the OpenClaw agent do in the last hour?
SELECT timestamp, tool_name, status, latency_ms
FROM audit_log
WHERE client_subject = 'openclaw-main'
  AND timestamp > NOW() - INTERVAL '1 hour'
ORDER BY timestamp DESC;
```

### Emergency Response

If an agent is behaving unexpectedly, you can instantly disable specific tools or entire backends without restarting anything:

```toml
# Kill switch: disable a specific tool
[kill_switch]
disabled_tools = ["filesystem__delete_file"]

# Or disable an entire backend
disabled_backends = ["firecrawl"]
```

Sentinel supports TOML hot reload — changes take effect without restarting the gateway or dropping active connections.

---

## Performance

Measured with OpenClaw v2026.2.22 connecting to Sentinel over WireGuard (LAN latency ~1ms):

| Metric | Value |
|--------|-------|
| SSH connection setup | ~100-200ms (one-time per mcporter call) |
| Sentinel cold start + backend discovery | ~2-3s |
| Sentinel security pipeline (auth + RBAC + audit) | < 1ms per request |
| End-to-end tool call (echo) | ~5-6s |
| End-to-end with web scrape (firecrawl) | ~8-10s |
| Full agent turn including LLM reasoning | ~18-20s |

The LLM thinking time (10-12s) dominates every agent turn. The tunnel overhead is a small fraction of total latency. For most use cases, the security benefits far outweigh the few seconds of additional latency.

### Optimization: SSH Connection Multiplexing

To eliminate repeated SSH handshake overhead, enable SSH connection multiplexing on the OpenClaw host:

```
# ~/.ssh/config
Host backend-host
    HostName 10.0.0.1
    User your-user
    ControlMaster auto
    ControlPath ~/.ssh/sockets/%r@%h-%p
    ControlPersist 600
```

```bash
mkdir -p ~/.ssh/sockets
```

This keeps the SSH connection alive for 10 minutes between mcporter calls, reducing subsequent connection setup to near-zero.

---

## Important: OpenClaw Config Pitfalls

**Do NOT add `mcpServers` to `openclaw.json`.** OpenClaw has strict config validation — unknown keys cause the gateway to crash-loop. MCP servers are managed through mcporter, not through `openclaw.json`.

```json
// WRONG — causes crash loop
{
  "gateway": {
    "mcpServers": { ... }
  }
}

// CORRECT — mcporter manages MCP servers separately
// ~/.mcporter/mcporter.json
{
  "mcpServers": {
    "sentinel": {
      "command": "ssh",
      "args": ["user@host", "/path/to/run-sentinel.sh"]
    }
  }
}
```

If the gateway is crash-looping due to a bad config key, restore the backup:

```bash
cp ~/.openclaw/openclaw.json.bak ~/.openclaw/openclaw.json
systemctl --user restart openclaw-gateway
```

---

## Comparison with Alternatives

| Feature | Sentinel Gateway | Runlayer | CrowdStrike AIDR | No Gateway |
|---|---|---|---|---|
| Self-hosted | Yes | No (SaaS) | No (SaaS) | N/A |
| Open source | BSL 1.1 | Proprietary | Proprietary | N/A |
| MCP-native | Yes | Wrapper | SDK-based | N/A |
| JWT Auth | Yes | Yes | N/A | No |
| RBAC | Yes | Limited | Yes | No |
| Audit logging | Yes (PostgreSQL) | Yes | Yes | No |
| Rate limiting | Yes (per-tool) | Unknown | Unknown | No |
| Circuit breakers | Yes | No | No | No |
| Kill switch | Yes | No | No | No |
| Prometheus metrics | Yes | No | Yes | No |
| Single binary | Yes (~14 MB Rust) | No | No | N/A |
| Cross-architecture | Yes (SSH tunnel) | No | No | N/A |
| Offline capable | Yes | No | No | Yes |

**Why self-hosted matters for OpenClaw users:** OpenClaw deployments often handle sensitive data — API keys, source code, internal documents. Routing MCP traffic through a third-party SaaS gateway defeats the purpose of running agents locally. Sentinel runs on your infrastructure with zero external dependencies.

---

## FAQ

**Does Sentinel change the tools available to OpenClaw?**
No. Sentinel federates tool catalogs from all configured backends and presents them as a unified set, minus any tools blocked by RBAC or the kill switch.

**Can multiple OpenClaw agents share one Sentinel instance?**
Yes. Each agent can use a different JWT token with different role claims. Sentinel applies per-role RBAC and logs each agent's activity separately.

**What happens if the SSH tunnel drops?**
mcporter starts a fresh connection on the next tool call. There is no persistent state in the tunnel — each invocation is independent. Enable SSH `ControlMaster` multiplexing to reduce reconnection overhead.

**What happens if a backend goes down?**
The circuit breaker detects repeated failures and temporarily isolates the backend. Other backends continue normally. When the failed backend recovers, it's automatically re-enabled.

**Do I need Rust on the OpenClaw host?**
No. The sentinel binary runs on the backend host. The OpenClaw host only needs SSH and mcporter (Node.js).

**Can I restrict which tools OpenClaw can access?**
Yes. Use RBAC `denied_tools` in `sentinel.toml` to block specific tools per role, or use the kill switch to disable tools globally. You can also configure different JWT tokens for different agents.

**Where are audit logs stored?**
PostgreSQL on the backend host. Each record includes: timestamp, agent identity (from JWT), tool name, backend, request parameters, response status, and latency.

---

## Sources

- [Microsoft Security Blog: Running OpenClaw Safely](https://www.microsoft.com/en-us/security/blog/2026/02/19/running-openclaw-safely-identity-isolation-runtime-risk/) (Feb 2026)
- [CrowdStrike: What Security Teams Need to Know About OpenClaw](https://www.crowdstrike.com/en-us/blog/what-security-teams-need-to-know-about-openclaw-ai-super-agent/) (Feb 2026)
- [VentureBeat: OpenClaw Proves Agentic AI Works](https://venturebeat.com/security/openclaw-agentic-ai-security-risk-ciso-guide) (Feb 2026)
- [Adversa.ai: OpenClaw Security Hardening 2026](https://adversa.ai/blog/openclaw-security-101-vulnerabilities-hardening-2026/)
- [IBM/Anthropic: Architecting Secure Enterprise AI Agents with MCP](https://www.ibm.com/think/insights/architecting-secure-enterprise-ai-agents-mcp) (Oct 2025)
