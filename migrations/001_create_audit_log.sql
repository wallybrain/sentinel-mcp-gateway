CREATE TABLE IF NOT EXISTS audit_log (
    id BIGSERIAL PRIMARY KEY,
    request_id UUID NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    client_subject TEXT NOT NULL,
    client_role TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    backend_name TEXT NOT NULL,
    request_args JSONB,
    response_status TEXT NOT NULL,
    error_message TEXT,
    latency_ms BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_log_timestamp ON audit_log (timestamp);
CREATE INDEX idx_audit_log_request_id ON audit_log (request_id);
CREATE INDEX idx_audit_log_client_subject ON audit_log (client_subject);
CREATE INDEX idx_audit_log_tool_name ON audit_log (tool_name);
