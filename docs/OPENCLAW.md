# OpenClaw Integration Guide

> How to secure OpenClaw's MCP connections using Sentinel Gateway.

OpenClaw is one of the most popular open-source AI agent frameworks, with over 180,000 GitHub stars. Its extensibility through MCP (Model Context Protocol) servers is a major strength, but that same extensibility introduces serious security risks when MCP connections are left unprotected. Sentinel Gateway sits between OpenClaw and its MCP backends, adding authentication, access control, rate limiting, audit logging, circuit breakers, and emergency kill switches.

---

## The Problem: OpenClaw's MCP Security Gap

OpenClaw connects directly to MCP servers with minimal security controls. Several high-profile vulnerabilities and research findings have made this a pressing concern:

- **CVE-2026-25253** (CVSS 8.8): One-click remote code execution via stolen authentication tokens through the Control UI. An attacker who gains access to the web interface can execute arbitrary commands on the host through any connected MCP server.

- **1,800+ exposed instances** discovered leaking API keys, chat histories, and credentials to the public internet (VentureBeat, Feb 2026). Many deployments run OpenClaw with default settings and no network isolation.

- **92% exploitation probability** with just 10 MCP plugins deployed, according to Pynt security research. Each additional MCP server exponentially increases the attack surface because there is no authentication or authorization between OpenClaw and its backends.

- **Microsoft advisory** (Feb 2026): OpenClaw should be treated as untrusted code execution and is not appropriate for standard workstations without additional security controls.

The core gaps in OpenClaw's MCP layer:

| Gap | Risk |
|-----|------|
| **No MCP-layer auth** | Any process on the same host can call MCP servers directly, bypassing OpenClaw entirely |
| **No access control** | All connected agents have full access to all tools on all backends |
| **No audit trail** | No record of which tools were called, by whom, or what data was accessed |
| **No rate limiting** | Runaway or compromised agents can make unlimited tool calls |
| **No circuit breakers** | One failing or slow backend can cascade failures across all agents |
| **No emergency controls** | Disabling a compromised tool requires restarting the entire system |

---

## How Sentinel Gateway Fills the Gap

Sentinel Gateway is a single Rust binary (~14 MB) that implements the MCP Gateway Pattern defined in the IBM/Anthropic enterprise whitepaper. It federates multiple MCP backends behind a unified security layer.

| OpenClaw Gap | Sentinel Feature |
|---|---|
| No MCP auth | JWT authentication -- every request is validated before reaching any backend |
| No access control | RBAC -- restrict which tools each role can call, with deny lists for dangerous operations |
| No rate limiting | Per-tool configurable rate limits with sliding window (requests per minute) |
| No audit trail | PostgreSQL-backed request/response audit logging with full tool call details |
| No failure isolation | Circuit breakers -- automatic backend isolation when error thresholds are exceeded |
| No emergency controls | Kill switch -- disable individual tools or entire backends without restart |
| No observability | Prometheus metrics endpoint -- request counts, latencies, error rates per tool and backend |

---

## Architecture

### Without Sentinel Gateway

```
OpenClaw Agent A --> MCP Server: filesystem    (unprotected)
OpenClaw Agent A --> MCP Server: brave-search  (unprotected)
OpenClaw Agent B --> MCP Server: github        (unprotected)
OpenClaw Agent B --> MCP Server: web-scraper   (unprotected)

  - No authentication between agents and servers
  - No visibility into what tools are being called
  - No way to restrict access per agent
  - No way to disable a tool without killing the process
```

### With Sentinel Gateway

```
OpenClaw Agent A --+
                   +---> Sentinel Gateway ---> MCP Server: filesystem
OpenClaw Agent B --+          |            ---> MCP Server: brave-search
                              |            ---> MCP Server: github
                              |            ---> MCP Server: web-scraper
                              |
                        +-----------+
                        | JWT Auth  |
                        | RBAC      |
                        | Rate Limit|
                        | Audit Log |
                        | Circuit   |
                        | Breakers  |
                        | Kill Sw.  |
                        | Metrics   |
                        +-----------+
                              |
                        PostgreSQL
                        (audit log)
```

Every tool call passes through Sentinel's security pipeline before reaching any backend. Agents authenticate once via JWT, and all subsequent requests are authorized, rate-limited, logged, and monitored.

---

## Setup Guide

### Prerequisites

- Sentinel Gateway binary (built from source with `cargo build --release`)
- PostgreSQL instance for audit logging (Docker Compose included)
- OpenClaw installation with MCP servers you want to protect

### Step 1: Deploy Sentinel Gateway

See [DEPLOYMENT.md](./DEPLOYMENT.md) for full deployment instructions.

```bash
git clone https://github.com/wallybrain/sentinel-gateway.git
cd sentinel-gateway
./scripts/setup.sh
cargo build --release
docker compose up -d postgres
```

### Step 2: Configure Your MCP Backends

Each MCP server that OpenClaw currently connects to directly becomes a backend entry in `sentinel.toml`:

```toml
# Example: common OpenClaw MCP servers

[[backends]]
name = "filesystem"
type = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/allowed/path"]
restart_on_exit = true
max_restarts = 5

[[backends]]
name = "brave-search"
type = "stdio"
command = "npx"
args = ["-y", "@anthropic/mcp-server-brave-search"]

[[backends]]
name = "github"
type = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]

[[backends]]
name = "web-scraper"
type = "stdio"
command = "npx"
args = ["-y", "@anthropic/mcp-server-fetch"]
```

Environment variables (API keys, tokens) are inherited from the gateway's environment. Set them in `.env`.

### Step 3: Point OpenClaw at Sentinel

Replace OpenClaw's direct MCP server connections with a single Sentinel Gateway entry.

**Before** (direct, unprotected connections):

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/allowed/path"]
    },
    "brave-search": {
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-brave-search"],
      "env": { "BRAVE_API_KEY": "your-brave-key" }
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "your-token" }
    }
  }
}
```

**After** (all traffic routed through Sentinel):

```json
{
  "mcpServers": {
    "sentinel-gateway": {
      "command": "/path/to/sentinel-gateway",
      "args": ["--config", "/path/to/sentinel.toml"],
      "env": {
        "JWT_SECRET_KEY": "your-jwt-secret",
        "SENTINEL_TOKEN": "your-sentinel-token",
        "DATABASE_URL": "postgres://sentinel:password@127.0.0.1:5432/sentinel",
        "BRAVE_API_KEY": "your-brave-key",
        "GITHUB_PERSONAL_ACCESS_TOKEN": "your-token"
      }
    }
  }
}
```

Sentinel federates tool catalogs from all backends and presents them as a unified set. OpenClaw sees the same tools it would see with direct connections.

### Step 4: Configure RBAC for OpenClaw Agents

Assign roles via JWT claims. The `role` claim must match a role in `sentinel.toml`:

```json
{
  "sub": "openclaw-agent-research",
  "role": "viewer",
  "iss": "sentinel-gateway",
  "aud": "sentinel-api"
}
```

---

## Recommended Security Policies

### 1. Research Assistant

Read-only tools. Can search the web and look up documentation but cannot modify files or interact with external services.

```toml
[rbac.roles.research]
permissions = ["tools.read", "tools.execute"]
denied_tools = [
  "filesystem__write_file",
  "filesystem__delete_file",
  "filesystem__move_file",
  "filesystem__create_directory",
  "github__create_issue",
  "github__create_pull_request",
  "github__push_files",
]

[rate_limits.per_tool]
brave_search = 30
web_scraper__fetch = 20
```

### 2. Development Agent

Code tools allowed, no destructive filesystem operations. Rate-limited on expensive operations.

```toml
[rbac.roles.development]
permissions = ["tools.read", "tools.execute"]
denied_tools = ["filesystem__delete_file"]

[rate_limits.per_tool]
github__push_files = 10
github__create_pull_request = 5
filesystem__write_file = 60
```

### 3. Full Autonomy

All tools available with rate limits and full audit logging. Use only for trusted, well-tested workflows.

```toml
[rbac.roles.autonomous]
permissions = ["tools.read", "tools.execute"]
denied_tools = []

[rate_limits]
default_rpm = 500

[rate_limits.per_tool]
filesystem__delete_file = 5
github__push_files = 10
```

Even in full autonomy mode, Sentinel provides complete audit trail, rate limiting, circuit breakers, and kill switch.

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
| Latency overhead | < 1 ms (stdio) | Network hop | Network hop | None |
| Offline capable | Yes | No | No | Yes |

**Why self-hosted matters for OpenClaw users:** OpenClaw deployments often handle sensitive data -- API keys, source code, internal documents. Routing MCP traffic through a third-party SaaS gateway defeats the purpose of running agents locally. Sentinel runs on your infrastructure with zero external dependencies.

---

## FAQ

**Does Sentinel change the tools available to OpenClaw?**
No. Sentinel federates tool catalogs from all configured backends and presents them as a unified set, minus any tools blocked by RBAC or the kill switch.

**What happens if a backend goes down?**
The circuit breaker detects repeated failures and temporarily isolates the backend. Other backends continue normally. When the failed backend recovers, it's automatically re-enabled.

**What is the performance overhead?**
Sub-millisecond per request for stdio backends (JWT validation, RBAC check, audit log write). Negligible compared to actual MCP tool call latency.

**Where are audit logs stored?**
PostgreSQL, specified by `DATABASE_URL`. Each record includes: timestamp, agent identity (from JWT), tool name, backend, request parameters, response status, and latency.

---

## Sources

- [Microsoft Security Blog: Running OpenClaw Safely](https://www.microsoft.com/en-us/security/blog/2026/02/19/running-openclaw-safely-identity-isolation-runtime-risk/) (Feb 2026)
- [CrowdStrike: What Security Teams Need to Know About OpenClaw](https://www.crowdstrike.com/en-us/blog/what-security-teams-need-to-know-about-openclaw-ai-super-agent/) (Feb 2026)
- [VentureBeat: OpenClaw Proves Agentic AI Works](https://venturebeat.com/security/openclaw-agentic-ai-security-risk-ciso-guide) (Feb 2026)
- [Adversa.ai: OpenClaw Security Hardening 2026](https://adversa.ai/blog/openclaw-security-101-vulnerabilities-hardening-2026/)
- [IBM/Anthropic: Architecting Secure Enterprise AI Agents with MCP](https://www.ibm.com/think/insights/architecting-secure-enterprise-ai-agents-mcp) (Oct 2025)
