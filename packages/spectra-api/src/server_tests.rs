#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::parse_response;
    use crate::http::Status;
    use crate::sse;
    use std::io::{Read, Write};

    fn start_test_server(config: ServerConfig) -> HttpServer {
        HttpServer::start(
            config,
            Arc::new(
                |request| match (request.method.as_str(), request.target.as_str()) {
                    ("GET", "/hello") => ServerResponse::text(200, "hello"),
                    ("POST", "/echo") => ServerResponse::bytes(200, request.body.bytes()),
                    ("GET", "/chunked") => {
                        ServerResponse::chunked(200, vec![b"alpha".to_vec(), b"beta".to_vec()])
                    }
                    ("HEAD", "/hello") => ServerResponse::text(200, "hello"),
                    _ => ServerResponse::text(404, "missing"),
                },
            ),
        )
        .expect("server starts")
    }

    fn request(addr: SocketAddr, raw: &[u8]) -> Vec<u8> {
        let mut stream = TcpStream::connect(addr).expect("connect test server");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        stream.write_all(raw).expect("write request");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read response");
        response
    }

    #[test]
    fn end_to_end_get_post_chunked_and_head() {
        let mut server = start_test_server(ServerConfig {
            idle_timeout: Duration::from_millis(50),
            ..ServerConfig::default()
        });
        let addr = server.local_addr();

        let get = request(
            addr,
            b"GET /hello HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        let get = parse_response(&get).expect("GET response parses");
        assert_eq!(get.status_code, 200);
        assert_eq!(get.body.bytes(), b"hello");

        let post = request(
            addr,
            b"POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 7\r\nConnection: close\r\n\r\npayload",
        );
        let post = parse_response(&post).expect("POST response parses");
        assert_eq!(post.status_code, 200);
        assert_eq!(post.body.bytes(), b"payload");

        let chunked = request(
            addr,
            b"GET /chunked HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        let chunked = parse_response(&chunked).expect("chunked response parses");
        assert!(chunked.body.chunked);
        assert_eq!(chunked.body.bytes(), b"alphabeta");

        let head = request(
            addr,
            b"HEAD /hello HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        let head_text = String::from_utf8(head).expect("HEAD response is utf-8");
        assert!(head_text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(head_text.contains("Content-Length: 5\r\n"));
        assert!(head_text.ends_with("\r\n\r\n"));

        let stats = server.shutdown().expect("shutdown");
        assert_eq!(stats.completed_requests, 4);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn body_limit_violation_returns_413_and_cleans_up() {
        let mut server = start_test_server(ServerConfig {
            max_body_bytes: 4,
            idle_timeout: Duration::from_millis(50),
            ..ServerConfig::default()
        });
        let raw = request(
            server.local_addr(),
            b"POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 7\r\nConnection: close\r\n\r\npayload",
        );
        let response = parse_response(&raw).expect("413 response parses");
        assert_eq!(response.status_code, 413);

        let stats = server.shutdown().expect("shutdown");
        assert_eq!(stats.body_limit_violations, 1);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn slowloris_timeout_closes_connection() {
        let mut server = start_test_server(ServerConfig {
            read_timeout: Duration::from_millis(40),
            idle_timeout: Duration::from_millis(500),
            poll_interval: Duration::from_millis(1),
            ..ServerConfig::default()
        });
        let mut stream = TcpStream::connect(server.local_addr()).expect("connect");
        stream
            .write_all(b"GET /hello HTTP/1.1\r\nHost")
            .expect("partial write");
        thread::sleep(Duration::from_millis(120));
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
        assert!(
            response.is_empty() || String::from_utf8_lossy(&response).contains("408"),
            "unexpected timeout response: {:?}",
            String::from_utf8_lossy(&response)
        );

        let stats = server.shutdown().expect("shutdown");
        assert!(stats.timeouts >= 1);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn parse_error_returns_400_and_cleans_up() {
        let mut server = start_test_server(ServerConfig {
            idle_timeout: Duration::from_millis(50),
            ..ServerConfig::default()
        });
        let raw = request(
            server.local_addr(),
            b"GET /bad HTTP/1.1\r\nBad Header: value\r\nConnection: close\r\n\r\n",
        );
        let response = parse_response(&raw).expect("400 response parses");
        assert_eq!(response.status_code, 400);

        let stats = server.shutdown().expect("shutdown");
        assert_eq!(stats.parse_errors, 1);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn r2216_serve_routes_to_registered_handler_and_shutdowns_cleanly() {
        let mut router = routing::Router::default();
        let route = router
            .add(routing::RouteMethod::Get, "/hello")
            .expect("route");
        let response = Response::new(Status::new(200).expect("status"))
            .with_header("Content-Type", "text/plain")
            .expect("header")
            .with_body(b"hello lifecycle".to_vec());
        handler::register_sync_response_for_route(route, response);

        let mut server = HttpServer::start_with_dispatcher(
            ServerConfig {
                idle_timeout: Duration::from_millis(50),
                shutdown_grace_period: Duration::from_millis(200),
                ..ServerConfig::default()
            },
            routed_handler(router),
        )
        .expect("server starts");
        let raw = request(
            server.local_addr(),
            b"GET /hello HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        let response = parse_response(&raw).expect("response parses");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.bytes(), b"hello lifecycle");

        let stats = server.shutdown().expect("shutdown");
        assert_eq!(stats.completed_requests, 1);
        assert_eq!(stats.cancelled_connections, 0);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn r2403_routed_sse_response_streams_through_http_server_loop() {
        let sse_server = Arc::new(Mutex::new(sse::SseServer::new()));
        sse_server
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set_heartbeat_interval(Duration::from_millis(40))
            .expect("configure routed heartbeat");
        let mut router = routing::Router::default();
        let route = router
            .add(routing::RouteMethod::Get, "/events")
            .expect("routed SSE route");
        let response_handle = sse::store_routed_response(Arc::clone(&sse_server))
            .expect("store routed SSE response");
        handler::register_sync_response_handle_for_route(route, response_handle);
        let mut server = HttpServer::start_with_dispatcher(
            ServerConfig {
                shutdown_grace_period: Duration::from_millis(200),
                poll_interval: Duration::from_millis(1),
                ..ServerConfig::default()
            },
            routed_handler(router),
        )
        .expect("start routed SSE server");

        let mut client = TcpStream::connect(server.local_addr()).expect("connect routed SSE");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set routed SSE read timeout");
        client
            .write_all(
                b"GET /events HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n",
            )
            .expect("write routed SSE request");
        let mut response = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !response.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = client.read(&mut buffer).expect("read routed SSE headers");
            assert!(count > 0, "routed SSE closed before headers");
            response.extend_from_slice(&buffer[..count]);
        }
        assert!(String::from_utf8_lossy(&response).contains("text/event-stream"));

        let event = sse::SseEvent::new(
            Some("7".to_string()),
            Some("update".to_string()),
            "routed".to_string(),
            Some(1_000),
        )
        .expect("create routed event");
        assert_eq!(
            sse_server
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .publish_event(&event)
                .expect("publish routed event"),
            1
        );
        while !response
            .windows(b"id: 7\nevent: update\ndata: routed\nretry: 1000\n\n".len())
            .any(|window| window == b"id: 7\nevent: update\ndata: routed\nretry: 1000\n\n")
        {
            let count = client.read(&mut buffer).expect("read routed SSE event");
            assert!(count > 0, "routed SSE closed before event");
            response.extend_from_slice(&buffer[..count]);
        }
        assert!(String::from_utf8_lossy(&response).contains("retry: 1000"));

        let stats = server.shutdown().expect("shutdown routed SSE server");
        assert_eq!(stats.completed_requests, 1);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn r2216_shutdown_drains_in_flight_keep_alive_request() {
        let handler_entered = Arc::new(AtomicBool::new(false));
        let handler_entered_for_handler = Arc::clone(&handler_entered);
        let mut server = HttpServer::start(
            ServerConfig {
                idle_timeout: Duration::from_secs(5),
                shutdown_grace_period: Duration::from_millis(500),
                poll_interval: Duration::from_millis(1),
                ..ServerConfig::default()
            },
            Arc::new(move |_| {
                handler_entered_for_handler.store(true, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(80));
                ServerResponse::text(200, "drained")
            }),
        )
        .expect("server starts");
        let mut stream = TcpStream::connect(server.local_addr()).expect("connect");
        stream
            .write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n")
            .expect("write request");
        let client = thread::spawn(move || {
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut response = Vec::new();
            let mut buf = [0_u8; 1024];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => response.extend_from_slice(&buf[..n]),
                    Err(error)
                        if error.kind() == std::io::ErrorKind::ConnectionReset
                            && !response.is_empty() =>
                    {
                        break
                    }
                    Err(error) => panic!("read response: {error}"),
                }
            }
            response
        });

        wait_until(Duration::from_secs(1), || {
            handler_entered.load(Ordering::SeqCst)
        });
        let stats = server.shutdown().expect("shutdown");
        let raw = client.join().expect("client thread");
        let response = parse_response(&raw).expect("response parses");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.bytes(), b"drained");
        assert_eq!(stats.completed_requests, 1);
        assert!(stats.drained_connections >= 1, "{stats:?}");
        assert_eq!(stats.cancelled_connections, 0);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn r2216_shutdown_cancels_unfinished_connections_after_grace_period() {
        let mut server = HttpServer::start(
            ServerConfig {
                read_timeout: Duration::from_secs(5),
                idle_timeout: Duration::from_secs(5),
                shutdown_grace_period: Duration::ZERO,
                poll_interval: Duration::from_millis(1),
                ..ServerConfig::default()
            },
            Arc::new(|_| ServerResponse::text(200, "unused")),
        )
        .expect("server starts");
        let mut stream = TcpStream::connect(server.local_addr()).expect("connect");
        stream
            .write_all(b"GET /unfinished HTTP/1.1\r\nHost")
            .expect("partial write");
        wait_until(Duration::from_secs(1), || {
            server.stats().active_connections == 1
        });

        let stats = server.shutdown().expect("shutdown");
        assert_eq!(stats.completed_requests, 0);
        assert!(stats.cancelled_connections >= 1, "{stats:?}");
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn connection_limiter_survives_10k_concurrent_slots_without_threads() {
        let mut limiter = ConnectionLimiter::new(10_000);
        for _ in 0..10_000 {
            assert!(limiter.try_open());
        }
        assert!(!limiter.try_open());
        assert_eq!(limiter.peak, 10_000);
        assert_eq!(limiter.rejected, 1);
        for _ in 0..10_000 {
            limiter.close();
        }
        assert_eq!(limiter.active, 0);
    }

    fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if condition() {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
}
