---
status: testing
phase: 04-authentication-authorization
source: 04-01-SUMMARY.md, 04-02-SUMMARY.md
started: 2026-02-22T15:00:00Z
updated: 2026-02-22T15:00:00Z
---

## Current Test

number: 1
name: Full Test Suite Passes (85 tests)
expected: |
  `cargo test` runs all 85 tests with 0 failures across all test binaries.
awaiting: user response

## Tests

### 1. Full Test Suite Passes (85 tests)
expected: `cargo test` runs all 85 tests with 0 failures across all test binaries
result: [pending]

### 2. Valid JWT Accepted
expected: A JWT with correct HS256 signature, non-expired, matching iss/aud is accepted and returns a CallerIdentity with subject and role extracted from claims
result: [pending]

### 3. Invalid JWT Rejected
expected: An expired, malformed, or wrong-signature JWT is rejected with AuthError. Missing token also rejected. All map to JSON-RPC error code -32001
result: [pending]

### 4. RBAC: Viewer Can List But Not Call
expected: A viewer role (tools.read permission) can see tools in tools/list but gets JSON-RPC error -32003 on tools/call
result: [pending]

### 5. RBAC: Developer Denied Tool Hidden and Blocked
expected: A developer with denied_tools configured cannot see or call those tools. Allowed tools work normally
result: [pending]

### 6. RBAC: Admin Wildcard Access
expected: An admin with ["*"] permission sees all tools and can call all tools. However, denied_tools still overrides wildcard
result: [pending]

### 7. RBAC: Unknown Role Denied Everything
expected: An unrecognized role (not in config) sees no tools and cannot call any tools
result: [pending]

### 8. Same is_tool_allowed Function for List and Call
expected: The exact same `is_tool_allowed()` function is used in both tools/list filtering and tools/call enforcement in gateway.rs
result: [pending]

### 9. Binary Starts with JWT Auth Enabled
expected: Running the binary with JWT_SECRET_KEY and SENTINEL_TOKEN env vars set validates the token at startup and logs the authenticated identity before entering the dispatch loop
result: [pending]

### 10. Binary Starts in Dev Mode Without Auth
expected: Running the binary without JWT secret configured logs a warning about auth being disabled and starts with default admin identity
result: [pending]

## Summary

total: 10
passed: 0
issues: 0
pending: 10
skipped: 0

## Gaps

[none yet]
