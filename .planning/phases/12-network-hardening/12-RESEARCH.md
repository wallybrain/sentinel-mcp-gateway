# Phase 12: Network Hardening - Research

**Researched:** 2026-02-22
**Domain:** Linux network security (iptables, port binding, Docker network cleanup)
**Confidence:** HIGH

## Summary

Phase 12 requires network hardening for Sentinel Gateway ports, iptables rules, and Docker network cleanup. The critical finding is that the current deployment state already satisfies most requirements: Sentinel runs as a native binary using stdio transport (port 9200 is not listening at all), port 9201 (health endpoint) is already bound to 127.0.0.1, and all sidecar ports (5432, 3001, 3002) are already bound to 127.0.0.1 in docker-compose.yml.

The remaining work is: (1) add defensive iptables DROP rules for ports 9200 and 9201 on eth0 (defense-in-depth even though they are loopback-only), (2) update fix-iptables.sh with those rules so they survive reboots, and (3) remove the stale `mcp-context-forge_mcpnet` Docker network.

**Primary recommendation:** This is a small, single-plan phase. Add iptables rules, update fix-iptables.sh, remove stale network, verify.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| NET-01 | Sentinel ports (9200, 9201) bound to 127.0.0.1 only | **Already satisfied.** Port 9200 is not listening (stdio mode). Port 9201 is bound to 127.0.0.1 (verified via `ss -tlnp`). Default in Rust source is `127.0.0.1`. sentinel.toml confirms `listen = "127.0.0.1:9200"`. Verification task only. |
| NET-02 | iptables DROP rules for ports 9200 and 9201 on eth0 | Defense-in-depth. Use existing fix-iptables.sh pattern (privileged Docker container). Add two DROP rules matching existing 8080/9999 pattern. |
| NET-03 | fix-iptables.sh updated with Sentinel port rules | Add rules to `/home/lwb3/v1be-code-server/fix-iptables.sh`. Follow existing idempotent pattern (`-C || -A`). |
| NET-04 | Stale ContextForge Docker networks cleaned up | `mcp-context-forge_mcpnet` network exists with no running containers. Remove with `docker network rm`. |
</phase_requirements>

## Current State (Verified)

### Port Binding Status

| Port | Service | Listening? | Bound to | Status |
|------|---------|------------|----------|--------|
| 9200 | Sentinel MCP (HTTP) | **NO** | N/A | Not used in stdio mode |
| 9201 | Sentinel Health | **YES** | 127.0.0.1 | Already correct |
| 5432 | Postgres | YES | 127.0.0.1 | Already correct (docker-compose) |
| 3001 | mcp-n8n | YES | 127.0.0.1 | Already correct (docker-compose) |
| 3002 | mcp-sqlite | YES | 127.0.0.1 | Already correct (docker-compose) |

### Rust Source Defaults (HIGH confidence)

```rust
// src/config/types.rs
fn default_listen() -> String {
    "127.0.0.1:9200".to_string()
}
fn default_health_listen() -> String {
    "127.0.0.1:9201".to_string()
}
```

Both defaults are 127.0.0.1. The sentinel.toml also sets `listen = "127.0.0.1:9200"`. There is no `health_listen` override in sentinel.toml, so the default (127.0.0.1:9201) is used.

### Docker Networks

| Network | Status | Action |
|---------|--------|--------|
| `mcp-context-forge_mcpnet` | Stale (all containers exited) | Remove |
| `sentinel-gateway_sentinelnet` | Active (postgres running) | Keep |
| `n8n-mcp_default` | Active (external, used by mcp-n8n) | Keep |
| `webproxy` | Active (Caddy/Authelia) | Keep |

### ContextForge Containers

All 5 ContextForge containers are in `Exited` state. The network `mcp-context-forge_mcpnet` can be removed after disconnecting any stopped containers, or by running `docker network rm` (Docker allows removal when no running containers use it -- stopped containers are auto-disconnected).

### Existing fix-iptables.sh Pattern

Location: `/home/lwb3/v1be-code-server/fix-iptables.sh`

The script uses a privileged Docker container to run iptables (since `sudo` is unavailable). The pattern is:
1. Spin up `ubuntu:24.04` with `--privileged --net=host`
2. Install iptables
3. Use `-C` (check) then `-A` or `-I` (idempotent append/insert)
4. Echo confirmation for each rule

Current rules:
- ACCEPT: bridge `br-03b170a2a124` -> port 8080 (code-server)
- DROP: eth0 -> port 8080 (code-server)
- DROP: eth0 -> port 9999 (cpu-monitor)

## Architecture Patterns

### Idempotent iptables Rule Pattern

```bash
# Check if rule exists, add only if missing
iptables -C INPUT -i eth0 -p tcp --dport PORT -j DROP 2>/dev/null || \
iptables -A INPUT -i eth0 -p tcp --dport PORT -j DROP && \
echo 'DROP eth0 -> port PORT (description)'
```

This is the exact pattern used in the existing fix-iptables.sh. New rules for 9200 and 9201 should follow this identical pattern.

### Docker Network Cleanup

```bash
# Remove network (works when no running containers are attached)
docker network rm mcp-context-forge_mcpnet
```

Stopped containers are automatically disconnected when the network is removed. No need to disconnect them first.

### External Verification

Since `sudo` is unavailable and `curl` from the public IP would route via loopback on the server itself, external verification requires either:
1. Using `nmap` from an external host (not available)
2. Using the VPS provider's console
3. Verifying via `ss -tlnp` that ports are bound to 127.0.0.1 (not 0.0.0.0)

The practical verification is: `ss -tlnp | grep -E '9200|9201'` shows 127.0.0.1 binding (or no listener for 9200). This is sufficient proof that external access is impossible at the bind level, with iptables as defense-in-depth.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| iptables management | Custom iptables wrapper | Extend existing fix-iptables.sh | Proven pattern, runs at boot via systemd |
| Port scanning | Custom port scanner | `ss -tlnp` for local verification | Standard Linux tool, accurate |
| Network cleanup | Manual container disconnects | `docker network rm` | Docker handles stopped container cleanup |

## Common Pitfalls

### Pitfall 1: Forgetting the Privileged Container Pattern
**What goes wrong:** Trying to run `iptables` directly or with `sudo` -- neither works on this VPS.
**How to avoid:** Always use the `docker run --rm --privileged --net=host` pattern from fix-iptables.sh.

### Pitfall 2: Testing Port Access from the Same Server
**What goes wrong:** `curl http://PUBLIC_IP:9201` from the server routes via loopback, bypassing iptables eth0 rules.
**How to avoid:** Verify binding with `ss -tlnp` (shows 127.0.0.1 vs 0.0.0.0). Trust the bind address + iptables combination. Note from MEMORY.md: "curl http://public-ip:port from server routes via lo".

### Pitfall 3: Docker Network Still Referenced
**What goes wrong:** `docker network rm` fails if a running container is attached.
**How to avoid:** Verify all ContextForge containers are stopped first (`docker ps --filter name=mcp-context-forge`). They are currently all exited.

### Pitfall 4: Bridge Interface Name Change
**What goes wrong:** The webproxy bridge name (`br-03b170a2a124`) is hardcoded in fix-iptables.sh and could change if the network is recreated.
**How to avoid:** Don't recreate the webproxy network. The Sentinel rules only need eth0 DROP (no bridge ACCEPT needed since Sentinel ports are loopback-only, not needed by Docker containers).

## Discrepancy Analysis: Requirements vs Reality

The requirements (NET-01, NET-02) reference ports 9200 and 9201 as if they might be publicly exposed. In reality:

- **Port 9200**: Not listening at all. Sentinel uses stdio transport, not HTTP. The `listen` config in sentinel.toml is unused in native binary mode. NET-01 is already satisfied by architecture.
- **Port 9201**: Listening on 127.0.0.1 only. The health server binds to the `health_listen` default. NET-01 is already satisfied by configuration.

**Recommendation:** Still add iptables DROP rules for both ports (defense-in-depth). If the deployment mode ever changes back to Docker/HTTP, the firewall rules will already be in place. This satisfies NET-02 without any risk.

## Open Questions

1. **Should NET-01 be marked as "already satisfied" or verified with a task?**
   - Recommendation: Include a verification task that confirms current state, then mark as satisfied. No changes needed.

2. **Should ContextForge containers be removed entirely (not just networks)?**
   - This is DECOM-01 (v1.2+, out of scope). Only remove the stale network per NET-04.

## Sources

### Primary (HIGH confidence)
- Live system state via `ss -tlnp` -- ports 9201, 5432, 3001, 3002 bound to 127.0.0.1; 9200 not listening
- Live system state via `docker network ls` -- `mcp-context-forge_mcpnet` exists, stale
- Live system state via `docker ps -a` -- all ContextForge containers exited
- Rust source `src/config/types.rs` -- default bindings are 127.0.0.1
- `sentinel.toml` -- listen = "127.0.0.1:9200"
- `docker-compose.yml` -- all ports prefixed with "127.0.0.1:"
- `/home/lwb3/v1be-code-server/fix-iptables.sh` -- existing iptables management pattern

### Secondary (MEDIUM confidence)
- MEMORY.md notes on iptables behavior, VPS networking quirks

## Metadata

**Confidence breakdown:**
- Port binding status: HIGH -- verified via live `ss -tlnp` output
- iptables pattern: HIGH -- existing script is proven, pattern is simple
- Docker cleanup: HIGH -- verified network exists and containers are stopped
- Requirement gap analysis: HIGH -- verified against source code defaults

**Research date:** 2026-02-22
**Valid until:** 2026-03-22 (stable infrastructure, no moving parts)
