mod error;
mod http;
mod retry;
mod sse;
pub mod stdio;

pub use error::BackendError;
pub use http::{build_http_client, discover_tools, HttpBackend};
pub use sse::parse_sse_data;
pub use stdio::{discover_stdio_tools, run_supervisor, StdioBackend};

/// Unified backend enum that dispatches to either HTTP or stdio transports.
#[derive(Clone)]
pub enum Backend {
    Http(HttpBackend),
    Stdio(StdioBackend),
}

impl Backend {
    pub async fn send(&self, json_rpc_body: &str) -> Result<String, BackendError> {
        match self {
            Backend::Http(h) => h.send(json_rpc_body).await,
            Backend::Stdio(s) => s.send(json_rpc_body).await,
        }
    }
}
