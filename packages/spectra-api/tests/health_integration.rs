use spectra_api::health::register_sqlite_check;
use spectra_api::server::{Handler, HttpServer, ServerConfig, ServerResponse};
use spectra_runtime::health::{HealthCategory, HealthRegistry, HealthState};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

fn request(addr: std::net::SocketAddr, method: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        stream,
        "{} {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        method, path
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = response.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

fn server(registry: HealthRegistry) -> HttpServer {
    let handler: Handler = Arc::new(|_| ServerResponse::text(200, "application"));
    HttpServer::start(ServerConfig::default(), handler)
        .unwrap()
        .with_health_registry(registry)
}

#[test]
fn health_routes_report_liveness_readiness_startup_and_recovery() {
    let registry = HealthRegistry::new();
    let available = Arc::new(AtomicBool::new(false));
    let check_state = Arc::clone(&available);
    registry
        .register_check(
            "database",
            HealthCategory::Database,
            Duration::from_millis(100),
            true,
            move || {
                if check_state.load(Ordering::SeqCst) {
                    Ok(())
                } else {
                    Err("database unavailable".into())
                }
            },
        )
        .unwrap();
    registry.refresh().unwrap();
    let mut http = server(registry.clone());
    let (status, body) = request(http.local_addr(), "GET", "/healthz");
    assert_eq!(status, 200);
    assert!(body.contains("healthy"));
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json["checks"].is_object());
    let (status, body) = request(http.local_addr(), "GET", "/readyz");
    assert_eq!(status, 503);
    assert!(body.contains("database"));
    let (status, _) = request(http.local_addr(), "GET", "/startupz");
    assert_eq!(status, 503);
    available.store(true, Ordering::SeqCst);
    registry.refresh().unwrap();
    registry.set_startup_complete();
    let (status, _) = request(http.local_addr(), "GET", "/readyz");
    assert_eq!(status, 200);
    let (status, _) = request(http.local_addr(), "GET", "/startupz");
    assert_eq!(status, 200);
    let (status, _) = request(http.local_addr(), "POST", "/healthz");
    assert_eq!(status, 405);
    http.shutdown().unwrap();
    assert!(registry.shutdown(Duration::from_secs(2)));
}

#[test]
fn slow_check_times_out_without_blocking_health_route() {
    let registry = HealthRegistry::new();
    registry
        .register_check(
            "slow",
            HealthCategory::ExternalService,
            Duration::from_millis(20),
            true,
            || {
                std::thread::sleep(Duration::from_millis(200));
                Ok(())
            },
        )
        .unwrap();
    registry.refresh().unwrap();
    assert_eq!(registry.readiness(), HealthState::Unavailable);
    assert_eq!(registry.snapshot().checks["slow"].status, "timeout");
    let mut http = server(registry.clone());
    let started = std::time::Instant::now();
    let (status, _) = request(http.local_addr(), "GET", "/healthz");
    assert_eq!(status, 200);
    assert!(started.elapsed() < Duration::from_millis(150));
    http.shutdown().unwrap();
    assert!(registry.shutdown(Duration::from_secs(2)));
}

#[test]
fn sqlite_file_backed_probe_executes_select_one() {
    let path = std::env::temp_dir().join(format!("spectra-health-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let registry = HealthRegistry::new();
    register_sqlite_check(&registry, "database", &path, Duration::from_secs(1), true).unwrap();
    registry.refresh().unwrap();
    assert_eq!(registry.snapshot().checks["database"].status, "pass");
    assert_eq!(registry.readiness(), HealthState::Healthy);
    assert!(registry.shutdown(Duration::from_secs(1)));
    let _ = std::fs::remove_file(path);
}
