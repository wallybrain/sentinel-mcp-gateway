#!/bin/bash
# Add sentinel-gateway MCP server to Claude Code
# Run this OUTSIDE of Claude Code (in a regular terminal)

# Use sed to extract values (handles = in base64 values)
JWT_SECRET_KEY="$(sed -n 's/^JWT_SECRET_KEY=//p' /home/lwb3/sentinel-gateway/.env)"
POSTGRES_PASSWORD="$(sed -n 's/^POSTGRES_PASSWORD=//p' /home/lwb3/sentinel-gateway/.env)"
SENTINEL_TOKEN="$(sed -n 's/^SENTINEL_TOKEN=//p' /home/lwb3/sentinel-gateway/.env)"
FIRECRAWL_API_KEY="$(sed -n 's/^FIRECRAWL_API_KEY=//p' /home/lwb3/sentinel-gateway/.env)"
BACKEND_SHARED_SECRET="$(sed -n 's/^BACKEND_SHARED_SECRET=//p' /home/lwb3/sentinel-gateway/.env)"

# Build JSON with python to avoid shell quoting issues
JSON=$(python3 -c "
import json, sys
print(json.dumps({
    'command': '/home/lwb3/sentinel-gateway/target/release/sentinel-gateway',
    'args': ['--config', '/home/lwb3/sentinel-gateway/sentinel.toml'],
    'env': {
        'JWT_SECRET_KEY': sys.argv[1],
        'SENTINEL_TOKEN': sys.argv[2],
        'DATABASE_URL': 'postgres://sentinel:' + sys.argv[3] + '@127.0.0.1:5432/sentinel',
        'FIRECRAWL_API_KEY': sys.argv[4],
        'BACKEND_SHARED_SECRET': sys.argv[5],
    }
}))
" "$JWT_SECRET_KEY" "$SENTINEL_TOKEN" "$POSTGRES_PASSWORD" "$FIRECRAWL_API_KEY" "$BACKEND_SHARED_SECRET")

echo "Generated JSON:"
echo "$JSON" | python3 -m json.tool

claude mcp add-json sentinel-gateway "$JSON"

echo "Done. Restart Claude Code and run /mcp to verify."
