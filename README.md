# Sentinel Gateway

A Rust-based enterprise MCP (Model Context Protocol) gateway, implementing the MCP Gateway pattern from the [IBM/Anthropic whitepaper](https://www.ibm.com/think/insights/architecting-secure-enterprise-ai-agents-mcp) (October 2025).

## Status

**v1.0 shipped** — 47/47 requirements, 138 tests, 3,776 LOC Rust.
**v1.1 in progress** — Deployed and running in production. Network hardening, monitoring, and ops phases remain.

## What It Does

A single-binary Rust MCP gateway that:

- Routes MCP tool calls from AI clients (Claude Code) to backend MCP servers
- Provides centralized auth (JWT), RBAC, rate limiting, and audit trails
- Manages both HTTP and stdio backends with health checks, circuit breakers, and auto-restart
- Replaces a 5-container Python/PostgreSQL/Redis stack (IBM ContextForge) with one process
- Speaks stdio (for Claude Code) with Streamable HTTP planned for future clients
- Exposes Prometheus metrics and supports TOML hot-reload

## Architecture

```
Claude Code (stdio)
    |
    v
Sentinel Gateway (single Rust binary, ~14 MB, <50 MB RAM)
    |  auth, RBAC, rate limit, audit, circuit breakers
    |
    +---> mcp-n8n (HTTP, port 3001)
    +---> mcp-sqlite (HTTP, port 3002)
    +---> context7 (stdio, managed)
    +---> firecrawl (stdio, managed)
    +---> playwright (stdio, managed)
    +---> sequential-thinking (stdio, managed)
    +---> ollama (stdio, managed)
```

### Sidecar Services (Docker Compose)

| Service | Purpose | Port |
|---------|---------|------|
| sentinel-postgres | Audit log storage | 127.0.0.1:5432 |
| mcp-n8n | n8n workflow API bridge | 127.0.0.1:3001 |
| mcp-sqlite | SQLite database operations | 127.0.0.1:3002 |

## Features

- **JWT Authentication** — Session-level token validation
- **Rate Limiting** — Per-tool configurable limits with sliding window
- **Circuit Breakers** — Automatic backend isolation on failure
- **Audit Logging** — PostgreSQL-backed request/response audit trail
- **Health Checks** — Periodic backend health with configurable intervals
- **stdio Process Management** — Auto-restart with configurable max retries
- **Prometheus Metrics** — Request counts, latencies, error rates, backend health
- **TOML Hot Reload** — Update config without restarting the gateway
- **Kill Switch** — Emergency disable for individual tools or entire backends

## Build

```bash
cargo build --release
# Binary: target/release/sentinel-gateway
```

## Configuration

Runtime config lives in `sentinel.toml` (native) or `sentinel-docker.toml` (Docker).
Secrets are referenced by environment variable name, never stored in config files.

## Documentation

| Document | Description |
|----------|-------------|
| [Current Infrastructure](docs/CURRENT-INFRASTRUCTURE.md) | VPS and container map |
| [Rust Wrapper Analysis](docs/RUST-WRAPPER-ANALYSIS.md) | Technical deep-dive on the original stdio bridge |
| [ContextForge Gateway](docs/CONTEXTFORGE-GATEWAY.md) | IBM ContextForge deployment details |
| [MCP Topology](docs/MCP-TOPOLOGY.md) | How Claude Code connects to all MCP servers |
| [Whitepaper Requirements](docs/WHITEPAPER-REQUIREMENTS.md) | Gateway requirements from IBM/Anthropic PDF |

## Motivation

1. **Learning & ownership** — Deep Rust systems programming; own every line
2. **Performance & footprint** — Single binary, minimal resources, production-grade
3. **Product opportunity** — No production Rust MCP gateway exists in the ecosystem
4. **Enterprise alignment** — Implements the IBM/Anthropic whitepaper pattern natively

## License

Copyright (c) 2026 Wally Blanchard. All rights reserved.

This source code and documentation are proprietary. No part of this repository may be reproduced, distributed, or transmitted in any form without prior written permission.
