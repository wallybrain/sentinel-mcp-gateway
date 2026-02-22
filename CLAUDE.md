# Sentinel Gateway

## Overview

Rust-based enterprise MCP gateway — replacing IBM ContextForge (Python/FastAPI) with a single-binary solution.

## Project Status

**Phase: Research & Documentation** — No Rust code yet. Documenting the current infrastructure and requirements.

## Key Paths

| Path | Purpose |
|------|---------|
| `docs/` | Architecture and requirements documentation |
| `docs/WHITEPAPER-REQUIREMENTS.md` | v1/v2 requirements from IBM/Anthropic whitepaper |
| `docs/CURRENT-INFRASTRUCTURE.md` | Current VPS and container map |
| `docs/MCP-TOPOLOGY.md` | MCP connection topology (governed + ungoverned) |
| `docs/CONTEXTFORGE-GATEWAY.md` | Existing ContextForge deployment details |
| `docs/RUST-WRAPPER-ANALYSIS.md` | Existing Rust wrapper code analysis |

## References

- **Whitepaper PDF**: `/home/lwb3/Architecting-secure-enterprise-AI-agents-with-MCP.pdf`
- **ContextForge source**: `/home/lwb3/mcp-context-forge/`
- **Rust wrapper source**: `/home/lwb3/mcp-context-forge/tools_rust/wrapper/`

## Build Notes

- Rust builds require `dangerouslyDisableSandbox: true` (bwrap loopback permission error)
- Docker commands also need sandbox disabled
