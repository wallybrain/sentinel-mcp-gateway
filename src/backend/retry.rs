use std::future::Future;
use std::time::Duration;

use rand::Rng;

use super::error::BackendError;

/// Retry an async operation with exponential backoff and jitter.
///
/// Base delay: 100ms * 2^attempt. Jitter: random 0..base/2.
/// If `max_retries` is 0, the operation executes once with no retry.
pub async fn retry_with_backoff<F, Fut, T>(
    max_retries: u32,
    mut operation: F,
) -> Result<T, BackendError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, BackendError>>,
{
    let mut attempt = 0u32;

    loop {
        match operation().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if !e.is_retryable() || attempt >= max_retries {
                    return Err(e);
                }

                let base_ms = 100u64.saturating_mul(1u64 << attempt);
                let jitter_ms = if base_ms > 1 {
                    rand::rng().random_range(0..base_ms / 2)
                } else {
                    0
                };
                let delay = Duration::from_millis(base_ms + jitter_ms);

                tracing::warn!(
                    attempt = attempt + 1,
                    max_retries,
                    delay_ms = delay.as_millis() as u64,
                    error = %e,
                    "retrying after transient error"
                );

                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}
