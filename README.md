# Sentinel Gateway

A Rust-based enterprise MCP (Model Context Protocol) gateway, implementing the MCP Gateway pattern from the [IBM/Anthropic whitepaper](https://www.ibm.com/think/insights/architecting-secure-enterprise-ai-agents-mcp) (October 2025).

## Project Status

**Phase: Research & Documentation** — The Rust gateway does not exist yet. This repository documents the current infrastructure and serves as the foundation for building a custom Rust replacement for IBM ContextForge.

## What This Will Be

A single-binary Rust MCP gateway that:

- Routes MCP tool calls from AI clients (Claude Code) to backend MCP servers
- Provides centralized auth (JWT/OAuth), RBAC, rate limiting, and audit trails
- Replaces a 5-container Python/PostgreSQL/Redis stack with one process
- Speaks both stdio (for Claude Code) and Streamable HTTP (for future clients)
- Implements the full MCP Gateway pattern from the IBM/Anthropic enterprise whitepaper

## Documentation

| Document | Description |
|----------|-------------|
| [Current Infrastructure](docs/CURRENT-INFRASTRUCTURE.md) | Complete map of what's running today |
| [Rust Wrapper Analysis](docs/RUST-WRAPPER-ANALYSIS.md) | Technical deep-dive on the existing Rust stdio bridge |
| [ContextForge Gateway](docs/CONTEXTFORGE-GATEWAY.md) | IBM ContextForge deployment details |
| [MCP Topology](docs/MCP-TOPOLOGY.md) | How Claude Code connects to all MCP servers |
| [Whitepaper Requirements](docs/WHITEPAPER-REQUIREMENTS.md) | Gateway requirements extracted from the IBM/Anthropic PDF |

## Current Architecture (What We're Replacing)

```
Claude Code (stdio)
    |
    v
Docker wrapper (Python mcpgateway.wrapper)
    |  stdio -> HTTP
    v
ContextForge Gateway (Python/FastAPI) --- 127.0.0.1:9200
    |           |           |
    v           v           v
 Postgres    Redis     Backend routing
 (audit)    (cache)        |
                           +---> mcp-n8n (Node.js, port 3000)
                           |       -> n8n API (port 5678)
                           |
                           +---> mcp-sqlite (Node.js, port 3000)
                                   -> SQLite databases

+ 5 ungoverned stdio MCP servers (context7, firecrawl, exa, etc.)
  launched directly by Claude Code, no auth/audit
```

**5 containers, ~1 GB RAM, ~330 MB disk** for what amounts to: route requests to 2 backends with a token check.

## Target Architecture (What We're Building)

```
Claude Code (stdio)
    |
    v
Sentinel Gateway (single Rust binary, ~10 MB RAM)
    |  auth, RBAC, rate limit, audit
    |
    +---> mcp-n8n backend (HTTP)
    +---> mcp-sqlite backend (HTTP)
    +---> context7 (stdio, managed)
    +---> firecrawl (stdio, managed)
    +---> exa (stdio, managed)
    +---> any future backend
```

**1 process, <50 MB RAM, sub-ms routing latency.**

## Motivation

1. **Learning & ownership** — Deep Rust systems programming; own every line
2. **Performance & footprint** — Single binary, minimal resources, production-grade
3. **Product opportunity** — No production Rust MCP gateway exists in the ecosystem
4. **Enterprise alignment** — Implements the IBM/Anthropic whitepaper pattern natively

## License

Copyright (c) 2026 Wally Blanchard. All rights reserved.

This source code and documentation are proprietary. No part of this repository may be reproduced, distributed, or transmitted in any form without prior written permission.
