use std::fmt;

#[derive(Debug)]
pub enum BackendError {
    Request(reqwest::Error),
    HttpStatus(u16, String),
    Stream(reqwest::Error),
    NoDataInSse,
    InvalidResponse(String),
}

impl BackendError {
    pub fn is_retryable(&self) -> bool {
        match self {
            BackendError::Request(e) => e.is_timeout() || e.is_connect(),
            BackendError::HttpStatus(code, _) => *code >= 500,
            BackendError::Stream(_) => true,
            BackendError::NoDataInSse => false,
            BackendError::InvalidResponse(_) => false,
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Request(e) => write!(f, "request error: {e}"),
            BackendError::HttpStatus(code, body) => {
                write!(f, "HTTP {code}: {body}")
            }
            BackendError::Stream(e) => write!(f, "stream error: {e}"),
            BackendError::NoDataInSse => write!(f, "no data line found in SSE response"),
            BackendError::InvalidResponse(msg) => write!(f, "invalid response: {msg}"),
        }
    }
}

impl std::error::Error for BackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BackendError::Request(e) | BackendError::Stream(e) => Some(e),
            _ => None,
        }
    }
}
