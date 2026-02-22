use std::time::Duration;

use sentinel_gateway::health::circuit_breaker::{CircuitBreaker, CircuitState};

#[test]
fn starts_closed_and_allows_requests() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    assert_eq!(cb.state(), CircuitState::Closed);
    assert!(cb.allow_request());
}

#[test]
fn transitions_to_open_after_threshold_failures() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Closed);
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);
    assert!(!cb.allow_request());
}

#[test]
fn transitions_to_half_open_after_recovery_timeout() {
    let cb = CircuitBreaker::new(2, Duration::from_millis(10));
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);
    assert!(!cb.allow_request());
    std::thread::sleep(Duration::from_millis(15));
    assert!(cb.allow_request());
    assert_eq!(cb.state(), CircuitState::HalfOpen);
}

#[test]
fn success_in_half_open_transitions_to_closed() {
    let cb = CircuitBreaker::new(2, Duration::from_millis(10));
    cb.record_failure();
    cb.record_failure();
    std::thread::sleep(Duration::from_millis(15));
    assert!(cb.allow_request());
    assert_eq!(cb.state(), CircuitState::HalfOpen);
    cb.record_success();
    assert_eq!(cb.state(), CircuitState::Closed);
    assert!(cb.allow_request());
}

#[test]
fn failure_in_half_open_transitions_back_to_open() {
    let cb = CircuitBreaker::new(2, Duration::from_millis(10));
    cb.record_failure();
    cb.record_failure();
    std::thread::sleep(Duration::from_millis(15));
    assert!(cb.allow_request());
    assert_eq!(cb.state(), CircuitState::HalfOpen);
    // In half-open, failure_count is already at threshold, so one more failure reopens
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);
    assert!(!cb.allow_request());
}
