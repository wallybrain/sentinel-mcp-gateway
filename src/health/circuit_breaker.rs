use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl From<u8> for CircuitState {
    fn from(val: u8) -> Self {
        match val {
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }
}

pub struct CircuitBreaker {
    state: AtomicU8,
    failure_count: AtomicU32,
    failure_threshold: u32,
    recovery_timeout: Duration,
    last_failure: Mutex<Option<Instant>>,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, recovery_timeout: Duration) -> Self {
        Self {
            state: AtomicU8::new(0),
            failure_count: AtomicU32::new(0),
            failure_threshold: threshold,
            recovery_timeout,
            last_failure: Mutex::new(None),
        }
    }

    pub fn allow_request(&self) -> bool {
        match self.state() {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let last = self.last_failure.lock().unwrap();
                if let Some(t) = *last {
                    if t.elapsed() >= self.recovery_timeout {
                        drop(last);
                        self.state.store(2, Ordering::SeqCst);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
        self.state.store(0, Ordering::SeqCst);
    }

    pub fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
        *self.last_failure.lock().unwrap() = Some(Instant::now());
        if count >= self.failure_threshold {
            self.state.store(1, Ordering::SeqCst);
        }
    }

    pub fn state(&self) -> CircuitState {
        CircuitState::from(self.state.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_allows_requests() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn open_blocks_requests() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        for _ in 0..3 {
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn half_open_after_timeout() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(10));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        std::thread::sleep(Duration::from_millis(15));
        assert!(cb.allow_request());
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn success_resets_to_closed() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(10));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        std::thread::sleep(Duration::from_millis(15));
        assert!(cb.allow_request());
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn threshold_behavior() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }
}
