/// Parse SSE-formatted text and extract the first non-empty `data:` line content.
pub fn parse_sse_data(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let stripped = if let Some(rest) = line.strip_prefix("data:") {
            rest.trim()
        } else {
            continue;
        };

        if !stripped.is_empty() {
            return Some(stripped.to_string());
        }
    }
    None
}
