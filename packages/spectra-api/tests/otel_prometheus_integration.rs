use spectra_api::server::{Handler, HttpServer, ServerConfig, ServerResponse};
use spectra_runtime::metrics::MetricsRegistry;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

fn get(addr: std::net::SocketAddr, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect real HTTP server");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let body = response
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    (status, body)
}

#[test]
fn real_server_shares_one_registry_with_prometheus_route() {
    let registry = MetricsRegistry::new();
    let custom = registry
        .register_counter("r2707_example_total", "R2707 example", &[])
        .unwrap();
    custom.inc(1.0).unwrap();
    let handler: Handler = Arc::new(|_| ServerResponse::text(200, "ok"));
    let mut server = HttpServer::start(ServerConfig::default(), handler)
        .unwrap()
        .with_metrics_registry(registry);
    let (status, body) = get(server.local_addr(), "/metrics");
    assert_eq!(status, 200);
    assert!(body.contains("r2707_example_total 1"));
    assert!(body.contains("spectra_http_requests_total"));
    server.shutdown().unwrap();
}
