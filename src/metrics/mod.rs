use prometheus::{
    CounterVec, GaugeVec, HistogramVec, Opts, Registry, TextEncoder,
};

#[derive(Clone)]
pub struct Metrics {
    pub requests_total: CounterVec,
    pub request_duration_seconds: HistogramVec,
    pub errors_total: CounterVec,
    pub backend_healthy: GaugeVec,
    pub rate_limit_hits_total: CounterVec,
    registry: Registry,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let requests_total = CounterVec::new(
            Opts::new("sentinel_requests_total", "Total MCP requests"),
            &["tool", "status"],
        )
        .expect("requests_total metric");

        let request_duration_seconds = HistogramVec::new(
            prometheus::histogram_opts!(
                "sentinel_request_duration_seconds",
                "Request latency in seconds"
            ),
            &["tool"],
        )
        .expect("request_duration_seconds metric");

        let errors_total = CounterVec::new(
            Opts::new("sentinel_errors_total", "Total errors by type"),
            &["tool", "error_type"],
        )
        .expect("errors_total metric");

        let backend_healthy = GaugeVec::new(
            Opts::new("sentinel_backend_healthy", "Backend health status"),
            &["backend"],
        )
        .expect("backend_healthy metric");

        let rate_limit_hits_total = CounterVec::new(
            Opts::new("sentinel_rate_limit_hits_total", "Total rate limit hits"),
            &["tool"],
        )
        .expect("rate_limit_hits_total metric");

        registry
            .register(Box::new(requests_total.clone()))
            .expect("register requests_total");
        registry
            .register(Box::new(request_duration_seconds.clone()))
            .expect("register request_duration_seconds");
        registry
            .register(Box::new(errors_total.clone()))
            .expect("register errors_total");
        registry
            .register(Box::new(backend_healthy.clone()))
            .expect("register backend_healthy");
        registry
            .register(Box::new(rate_limit_hits_total.clone()))
            .expect("register rate_limit_hits_total");

        Self {
            requests_total,
            request_duration_seconds,
            errors_total,
            backend_healthy,
            rate_limit_hits_total,
            registry,
        }
    }

    pub fn gather_text(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder
            .encode_to_string(&metric_families)
            .unwrap_or_default()
    }

    pub fn record_request(&self, tool: &str, status: &str, duration_secs: f64) {
        self.requests_total
            .with_label_values(&[tool, status])
            .inc();
        self.request_duration_seconds
            .with_label_values(&[tool])
            .observe(duration_secs);
        if status != "success" {
            self.errors_total
                .with_label_values(&[tool, status])
                .inc();
        }
    }

    pub fn record_rate_limit_hit(&self, tool: &str) {
        self.rate_limit_hits_total
            .with_label_values(&[tool])
            .inc();
    }

    pub fn set_backend_health(&self, backend: &str, healthy: bool) {
        self.backend_healthy
            .with_label_values(&[backend])
            .set(if healthy { 1.0 } else { 0.0 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_new_creates_all_metrics() {
        let m = Metrics::new();
        // Touch each metric so it appears in gather output
        m.record_request("_init", "success", 0.0);
        m.record_request("_init", "error", 0.0);
        m.record_rate_limit_hit("_init");
        m.set_backend_health("_init", true);
        let text = m.gather_text();
        // All 5 metric families should be present (as TYPE lines)
        assert!(text.contains("sentinel_requests_total"), "missing requests_total");
        assert!(
            text.contains("sentinel_request_duration_seconds"),
            "missing request_duration_seconds"
        );
        assert!(text.contains("sentinel_errors_total"), "missing errors_total");
        assert!(
            text.contains("sentinel_backend_healthy"),
            "missing backend_healthy"
        );
        assert!(
            text.contains("sentinel_rate_limit_hits_total"),
            "missing rate_limit_hits_total"
        );
    }

    #[test]
    fn test_record_request_increments_counter() {
        let m = Metrics::new();
        m.record_request("echo", "success", 0.05);
        let text = m.gather_text();
        assert!(
            text.contains("sentinel_requests_total{"),
            "requests_total not recorded"
        );
        assert!(
            text.contains("sentinel_request_duration_seconds"),
            "duration not recorded"
        );
    }

    #[test]
    fn test_record_rate_limit_hit() {
        let m = Metrics::new();
        m.record_rate_limit_hit("echo");
        let text = m.gather_text();
        assert!(
            text.contains("sentinel_rate_limit_hits_total{tool=\"echo\"}"),
            "rate limit hit not recorded"
        );
    }

    #[test]
    fn test_set_backend_health() {
        let m = Metrics::new();
        m.set_backend_health("n8n", true);
        m.set_backend_health("sqlite", false);
        let text = m.gather_text();
        assert!(
            text.contains("sentinel_backend_healthy{backend=\"n8n\"} 1"),
            "n8n should be healthy (1)"
        );
        assert!(
            text.contains("sentinel_backend_healthy{backend=\"sqlite\"} 0"),
            "sqlite should be unhealthy (0)"
        );
    }
}
