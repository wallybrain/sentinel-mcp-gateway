# Sentinel MCP Gateway

[![License: BSL 1.1](https://img.shields.io/badge/License-BSL_1.1-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)

A single-binary Rust MCP gateway that secures AI agent tool calls with centralized authentication, RBAC, rate limiting, and audit logging. Implements the [IBM/Anthropic MCP Gateway pattern](https://www.ibm.com/think/insights/architecting-secure-enterprise-ai-agents-mcp).

## Why

AI agents (Claude Code, OpenClaw, etc.) connect to MCP servers with no security layer — no auth, no access control, no audit trail. Any process on the same host can call any tool. Sentinel Gateway sits between agents and MCP servers, adding enterprise security controls without modifying either side.

> **Using OpenClaw?** See our [tested integration guide](docs/OPENCLAW.md) — production-verified SSH stdio tunnel pattern that secures OpenClaw's MCP connections with JWT auth, RBAC, rate limiting, and audit logging. Addresses the critical security gaps behind [CVE-2026-25253](https://nvd.nist.gov/) and [1,800+ exposed instances](https://venturebeat.com/security/openclaw-agentic-ai-security-risk-ciso-guide).

## Architecture

```
AI Agent (Claude Code, OpenClaw, etc.)
    |
    v
Sentinel Gateway (single Rust binary, ~14 MB, <50 MB RAM)
    |  JWT auth, RBAC, rate limiting, audit, circuit breakers
    |
    +---> MCP Server A (HTTP or stdio)
    +---> MCP Server B
    +---> MCP Server N
```

All backends are optional and configurable. Use any combination of HTTP and stdio MCP servers — the gateway doesn't care what's behind it, only that it's secured.

### Optional Sidecar Services (Docker Compose)

| Service | Purpose | Port |
|---------|---------|------|
| sentinel-postgres | Audit log storage | 127.0.0.1:5432 |
| mcp-n8n | n8n workflow API bridge | 127.0.0.1:3001 |
| mcp-sqlite | SQLite database operations | 127.0.0.1:3002 |

## Features

- **JWT Authentication** — session-level token validation on every request
- **RBAC** — role-based access control with per-tool deny lists
- **Rate Limiting** — per-tool configurable limits with sliding window
- **Audit Logging** — PostgreSQL-backed request/response trail
- **Circuit Breakers** — automatic backend isolation on failure
- **Health Checks** — periodic backend health with configurable intervals
- **stdio Process Management** — auto-restart with configurable max retries
- **Prometheus Metrics** — request counts, latencies, error rates, backend health
- **TOML Hot Reload** — update config without restarting the gateway
- **Kill Switch** — emergency disable for individual tools or entire backends

## Quick Start

```bash
# 1. Clone
git clone https://github.com/wallybrain/sentinel-mcp-gateway.git
cd sentinel-gateway

# 2. Setup (generates .env and sentinel.toml)
./scripts/setup.sh

# 3. Build
cargo build --release

# 4. Start PostgreSQL (for audit logging)
docker compose up -d postgres

# 5. Register with Claude Code
./add-mcp.sh
```

See [DEPLOYMENT.md](docs/DEPLOYMENT.md) for the full guide including JWT token generation, backend configuration, and security hardening.

## Configuration

Runtime config lives in `sentinel.toml` (native) or `sentinel-docker.toml` (Docker). Copy the example and uncomment the backends you need:

```bash
cp sentinel.toml.example sentinel.toml
# Edit sentinel.toml — uncomment your backends
```

Secrets are referenced by environment variable name, never stored in config files. See [.env.example](.env.example).

### Example: Adding a Backend

```toml
# HTTP backend (running separately)
[[backends]]
name = "my-service"
type = "http"
url = "http://127.0.0.1:3003"
timeout_secs = 30

# stdio backend (managed by gateway)
[[backends]]
name = "context7"
type = "stdio"
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
restart_on_exit = true
max_restarts = 5
```

## Documentation

| Document | Description |
|----------|-------------|
| [Deployment Guide](docs/DEPLOYMENT.md) | Step-by-step setup on a naked VPS |
| [OpenClaw Integration](docs/OPENCLAW.md) | Securing OpenClaw with Sentinel |
| [MCP Topology](docs/MCP-TOPOLOGY.md) | Connection flow and protocol details |
| [Whitepaper Requirements](docs/WHITEPAPER-REQUIREMENTS.md) | IBM/Anthropic gateway spec coverage |
| [Rust Wrapper Analysis](docs/RUST-WRAPPER-ANALYSIS.md) | Technical deep-dive on the stdio bridge |

## Status

**v1.0 shipped** — 47/47 requirements from the IBM/Anthropic whitepaper, 145 tests, 3,776 LOC Rust.

Production-tested with Claude Code (local stdio) and OpenClaw (remote SSH stdio tunnel via mcporter). See [docs/OPENCLAW.md](docs/OPENCLAW.md) for the full integration guide with cross-architecture deployment (x86_64 backend + ARM64 OpenClaw host).

## License

[Business Source License 1.1](LICENSE) — you can use, modify, and deploy Sentinel Gateway freely. The only restriction: you cannot offer it as a managed/hosted MCP gateway service to third parties.

Converts to [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0) on **2030-02-23**.

For alternative licensing, contact the author.
