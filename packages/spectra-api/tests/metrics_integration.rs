use spectra_api::server::{Handler, HttpServer, ServerConfig, ServerResponse};
use spectra_runtime::metrics::MetricsRegistry;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

fn request(addr: std::net::SocketAddr, path: &str) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        stream,
        "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        path
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response.split_whitespace().nth(1).unwrap().parse().unwrap();
    let headers = response.split("\r\n\r\n").next().unwrap_or("").to_string();
    let body = response.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, headers, body)
}

#[test]
fn real_http_server_exports_prometheus_metrics() {
    let registry = MetricsRegistry::new();
    let custom = registry
        .register_counter("spectra_test_events_total", "Test events", &["kind"])
        .unwrap();
    let latency = registry
        .register_histogram(
            "spectra_test_latency_seconds",
            "Test latency",
            &[0.1, 0.5, 1.0],
            &[],
        )
        .unwrap();
    custom.inc_labels(&[("kind", "request")], 3.0).unwrap();
    latency.observe(0.2).unwrap();
    latency.observe(0.8).unwrap();
    let handler: Handler = Arc::new(|request| {
        if request.target == "/error" {
            ServerResponse::text(500, "error")
        } else {
            ServerResponse::text(200, "ok")
        }
    });
    let mut server = HttpServer::start(ServerConfig::default(), handler)
        .unwrap()
        .with_metrics_registry(registry.clone());
    request(server.local_addr(), "/ok");
    request(server.local_addr(), "/error");
    let (status, headers, body) = request(server.local_addr(), "/metrics");
    assert_eq!(status, 200);
    assert!(headers.contains("text/plain; version=0.0.4"));
    for name in [
        "spectra_http_requests_total",
        "spectra_http_request_duration_seconds",
        "spectra_http_errors_total",
        "spectra_http_active_connections",
        "spectra_http_accepted_connections_total",
        "spectra_http_timeouts_total",
        "spectra_test_events_total",
        "spectra_test_latency_seconds_bucket",
    ] {
        assert!(body.contains(name), "missing {name}\n{body}");
    }
    assert!(body.contains("spectra_http_requests_total{method=\"GET\",status=\"200\"}"));
    assert!(body.contains("spectra_http_errors_total{class=\"5xx\"}"));
    assert!(body.contains("spectra_test_latency_seconds_count 2"));
    let path = std::env::var("SPECTRA_R2702_METRICS_PATH")
        .unwrap_or_else(|_| "target/r2702-metrics/metrics.txt".into());
    let path = std::path::Path::new(&path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, &body).unwrap();
    server.shutdown().unwrap();
}

#[test]
fn metrics_registry_rejects_sensitive_labels_and_preserves_concurrent_updates() {
    let registry = MetricsRegistry::new();
    assert!(registry.register_counter("bad", "bad", &["url"]).is_err());
    assert!(registry
        .register_histogram("bad_hist", "bad", &[f64::NAN], &[])
        .is_err());
    let counter = registry
        .register_counter("spectra_concurrent_total", "Concurrent", &[])
        .unwrap();
    let counter = Arc::new(counter);
    let threads = (0..8)
        .map(|_| {
            let counter = Arc::clone(&counter);
            std::thread::spawn(move || {
                for _ in 0..100 {
                    counter.inc(1.0).unwrap();
                }
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    assert!(registry
        .render_prometheus()
        .contains("spectra_concurrent_total 800"));
}
