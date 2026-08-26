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
    fn r2401_routed_websocket_upgrade_round_trips_through_http_server() {
        let websocket_server = Arc::new(Mutex::new(crate::websocket::WebSocketServer::new()));
        let mut router = routing::Router::default();
        let route = router
            .add(routing::RouteMethod::Get, "/socket")
            .expect("routed WebSocket route");
        crate::websocket::register_server_route(Arc::clone(&websocket_server), route)
            .expect("attach WebSocket route");
        let mut server = HttpServer::start_with_dispatcher(
            ServerConfig {
                shutdown_grace_period: Duration::from_millis(200),
                poll_interval: Duration::from_millis(1),
                ..ServerConfig::default()
            },
            routed_handler(router),
        )
        .expect("start routed WebSocket HTTP server");

        let accept_server = Arc::clone(&websocket_server);
        let server_thread = thread::spawn(move || {
            let cancellation = Arc::new(AtomicBool::new(false));
            let mut connection = accept_server
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .accept(&cancellation)
                .expect("accept routed WebSocket connection");
            assert_eq!(
                connection
                    .receive_message()
                    .expect("receive routed WebSocket message"),
                Some(crate::websocket::WebSocketMessage::Text("routed".to_string()))
            );
            connection
                .send_text("routed-echo")
                .expect("send routed WebSocket echo");
            connection.close(1000, "done").expect("close routed WebSocket");
        });

        let mut client = crate::websocket::WebSocketClient::new();
        client.allow_private_networks(true);
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut connection = client
            .connect(
                &format!("ws://127.0.0.1:{}/socket", server.local_addr().port()),
                &cancellation,
            )
            .expect("connect routed WebSocket client");
        connection.send_text("routed").expect("send routed message");
        assert_eq!(
            connection
                .receive_message()
                .expect("receive routed echo"),
            Some(crate::websocket::WebSocketMessage::Text(
                "routed-echo".to_string()
            ))
        );
        connection.close(1000, "done").expect("close routed client");
        server_thread.join().expect("routed WebSocket thread");

        let stats = server.shutdown().expect("shutdown routed WebSocket server");
        assert_eq!(stats.completed_requests, 1);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    #[ignore = "release soak: opens 10,000 routed WebSocket connections"]
    fn r2401_routed_websocket_10k_concurrent_connections_soak() {
        const CONNECTIONS: usize = 10_000;
        let websocket_server = Arc::new(Mutex::new(crate::websocket::WebSocketServer::new()));
        let mut router = routing::Router::default();
        let route = router
            .add(routing::RouteMethod::Get, "/socket")
            .expect("routed WebSocket soak route");
        crate::websocket::register_server_route(Arc::clone(&websocket_server), route)
            .expect("attach WebSocket soak route");
        let mut server = HttpServer::start_with_dispatcher(
            ServerConfig {
                max_connections: CONNECTIONS,
                shutdown_grace_period: Duration::from_millis(500),
                poll_interval: Duration::from_millis(1),
                ..ServerConfig::default()
            },
            routed_handler(router),
        )
        .expect("start routed WebSocket soak server");

        let mut clients = Vec::with_capacity(CONNECTIONS);
        for _ in 0..CONNECTIONS {
            let mut client = TcpStream::connect(server.local_addr()).expect("connect soak client");
            client
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("set soak read timeout");
            client
                .set_write_timeout(Some(Duration::from_secs(10)))
                .expect("set soak write timeout");
            client
                .write_all(
                    b"GET /socket HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
                )
                .expect("write soak handshake");
            let mut response = Vec::new();
            let mut buffer = [0_u8; 512];
            while !response.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = client.read(&mut buffer).expect("read soak handshake");
                assert!(count > 0, "soak peer closed before upgrade");
                response.extend_from_slice(&buffer[..count]);
            }
            assert!(
                response.starts_with(b"HTTP/1.1 101 Switching Protocols\r\n"),
                "unexpected soak upgrade response: {}",
                String::from_utf8_lossy(&response)
            );
            clients.push(client);
        }

        let cancellation = Arc::new(AtomicBool::new(false));
        for _ in 0..CONNECTIONS {
            let connection = websocket_server
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .accept(&cancellation)
                .expect("accept every routed soak connection");
            drop(connection);
        }
        assert_eq!(server.stats().accepted_connections, CONNECTIONS);
        drop(clients);
        let stats = server.shutdown().expect("shutdown routed WebSocket soak server");
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
    fn r2216_slow_handler_offloads_and_fast_connection_answers_first() {
        let mut server = HttpServer::start(
            ServerConfig {
                read_timeout: Duration::from_secs(5),
                idle_timeout: Duration::from_secs(5),
                poll_interval: Duration::from_millis(1),
                worker_threads: 2,
                ..ServerConfig::default()
            },
            Arc::new(|request| match request.target.as_str() {
                "/slow" => {
                    thread::sleep(Duration::from_millis(500));
                    ServerResponse::text(200, "slow")
                }
                _ => ServerResponse::text(200, "fast"),
            }),
        )
        .expect("server starts");

        // Client A fires the slow request first.
        let mut slow = TcpStream::connect(server.local_addr()).expect("connect slow client");
        slow.write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .expect("write slow request");
        // Give the event loop time to parse and dispatch A to a worker.
        thread::sleep(Duration::from_millis(100));

        // Client B must receive its fast response while A is still in flight.
        let started = Instant::now();
        let fast = request(
            server.local_addr(),
            b"GET /fast HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        let elapsed = started.elapsed();
        let fast = parse_response(&fast).expect("fast response parses");
        assert_eq!(fast.status_code, 200);
        assert_eq!(fast.body.bytes(), b"fast");
        assert!(
            elapsed < Duration::from_millis(200),
            "fast connection blocked behind the slow handler for {elapsed:?}"
        );

        slow.set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set slow read timeout");
        let mut raw = Vec::new();
        let mut buf = [0_u8; 1024];
        loop {
            match slow.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => raw.extend_from_slice(&buf[..n]),
                Err(error)
                    if error.kind() == std::io::ErrorKind::ConnectionReset && !raw.is_empty() =>
                {
                    break
                }
                Err(error) => panic!("read slow response: {error}"),
            }
        }
        let slow_response = parse_response(&raw).expect("slow response parses");
        assert_eq!(slow_response.status_code, 200);
        assert_eq!(slow_response.body.bytes(), b"slow");

        server.shutdown().expect("shutdown");
    }

    #[test]
    fn r2216_panicking_handler_returns_500_and_server_survives() {
        let mut server = HttpServer::start(
            ServerConfig {
                idle_timeout: Duration::from_millis(200),
                poll_interval: Duration::from_millis(1),
                ..ServerConfig::default()
            },
            Arc::new(|request| {
                if request.target == "/panic" {
                    panic!("handler exploded");
                }
                ServerResponse::text(200, "ok")
            }),
        )
        .expect("server starts");

        let raw = request(
            server.local_addr(),
            b"GET /panic HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        let response = parse_response(&raw).expect("panicked response parses");
        assert_eq!(response.status_code, 500);

        // The worker pool and event loop survive the panic.
        let raw = request(
            server.local_addr(),
            b"GET /alive HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        let response = parse_response(&raw).expect("post-panic response parses");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.bytes(), b"ok");

        server.shutdown().expect("shutdown");
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

    #[test]
    fn tls_gateway_serves_h2_and_http11_on_the_same_tls_port() {
        use crate::tls::{TlsCertificateStore, TlsClientConfig, TlsServerConfig};
        use rcgen::generate_simple_self_signed;
        use rustls::pki_types::ServerName;
        use tokio::net::TcpStream;
        use tokio::runtime::Builder;
        use tokio_rustls::TlsConnector;

        let certified = generate_simple_self_signed(vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ])
        .expect("self-signed gateway certificate");
        let cert_der = certified.cert.der().to_vec();
        let certificates = TlsCertificateStore::new(TlsServerConfig::new(
            vec![cert_der.clone()],
            certified.key_pair.serialize_der(),
        ))
        .expect("TLS certificate store");

        let mut server = HttpServer::start(
            ServerConfig {
                tls_certificates: Some(std::sync::Arc::new(certificates)),
                ..ServerConfig::default()
            },
            Arc::new(|request| {
                ServerResponse::text(200, format!("{} {}", request.method, request.target))
            }),
        )
        .expect("TLS-enabled server starts");
        let cleartext_addr = server.local_addr();
        let tls_addr = server.tls_local_addr().expect("TLS gateway address");
        assert_ne!(cleartext_addr.port(), tls_addr.port());

        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("gateway test runtime");
        runtime.block_on(async move {
            // HTTP/2 leg: ALPN h2 on the TLS port.
            let client_config = TlsClientConfig::with_roots(vec![cert_der.clone()])
                .with_alpn_protocols(vec![b"h2".to_vec()])
                .build()
                .expect("h2 client config");
            let stream = TcpStream::connect(tls_addr).await.expect("connect h2");
            let connector = TlsConnector::from(client_config);
            let name = ServerName::try_from("localhost").expect("server name");
            let tls_stream = connector
                .connect(name, stream)
                .await
                .expect("h2 TLS handshake");
            assert_eq!(tls_stream.get_ref().1.alpn_protocol(), Some(&b"h2"[..]));
            let (mut h2_client, h2_connection) =
                h2::client::handshake(tls_stream).await.expect("h2 handshake");
            tokio::spawn(async move {
                let _ = h2_connection.await;
            });
            let request = ::http::Request::builder()
                .method("GET")
                .uri("/secure-h2")
                .body(())
                .expect("h2 request");
            let (response, _) = h2_client
                .send_request(request, true)
                .expect("h2 stream");
            let response = response.await.expect("h2 response");
            assert_eq!(response.status(), 200);
            assert!(response.headers().get("content-type").is_some());
            let mut body_stream = response.into_body();
            let mut body = Vec::new();
            while let Some(chunk) = body_stream.data().await {
                let chunk = chunk.expect("h2 body chunk");
                body.extend_from_slice(&chunk);
                let _ = body_stream.flow_control().release_capacity(chunk.len());
            }
            assert_eq!(body, b"GET /secure-h2");

            // HTTP/1.1 leg: ALPN http/1.1 on the SAME TLS port.
            let client_config = TlsClientConfig::with_roots(vec![cert_der])
                .with_alpn_protocols(vec![b"http/1.1".to_vec()])
                .build()
                .expect("http/1.1 client config");
            let stream = TcpStream::connect(tls_addr)
                .await
                .expect("connect http/1.1 over TLS");
            let connector = TlsConnector::from(client_config);
            let name = ServerName::try_from("localhost").expect("server name");
            let mut tls_stream = connector
                .connect(name, stream)
                .await
                .expect("http/1.1 TLS handshake");
            assert_eq!(
                tls_stream.get_ref().1.alpn_protocol(),
                Some(&b"http/1.1"[..])
            );
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            tls_stream
                .write_all(
                    b"GET /secure-http11 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("write http/1.1 request");
            let mut raw = Vec::new();
            let mut buffer = [0_u8; 8_192];
            loop {
                match tls_stream.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => raw.extend_from_slice(&buffer[..read]),
                }
            }
            let response = parse_response(&raw).expect("http/1.1 response parses");
            assert_eq!(response.status_code, 200);
            assert_eq!(response.body.bytes(), b"GET /secure-http11");
        });

        // Cleartext HTTP/1.1 keeps working on the original mio port.
        let raw = request(
            cleartext_addr,
            b"GET /hello HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        let response = parse_response(&raw).expect("cleartext response parses");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.bytes(), b"GET /hello");

        server.shutdown().expect("shutdown TLS server");
    }

    #[test]
    fn tls_gateway_streams_routed_sse_events_and_heartbeats_on_the_tls_port() {
        use crate::tls::{TlsCertificateStore, TlsClientConfig, TlsServerConfig};
        use rcgen::generate_simple_self_signed;
        use rustls::pki_types::ServerName;
        use tokio::net::TcpStream;
        use tokio::runtime::Builder;
        use tokio_rustls::TlsConnector;

        let sse_server = Arc::new(Mutex::new(sse::SseServer::new()));
        sse_server
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set_heartbeat_interval(Duration::from_millis(40))
            .expect("configure gateway SSE heartbeat");
        let mut router = routing::Router::default();
        let route = router
            .add(routing::RouteMethod::Get, "/events")
            .expect("gateway SSE route");
        let response_handle = sse::store_routed_response(Arc::clone(&sse_server))
            .expect("store gateway SSE response");
        handler::register_sync_response_handle_for_route(route, response_handle);

        let certified = generate_simple_self_signed(vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ])
        .expect("self-signed SSE gateway certificate");
        let cert_der = certified.cert.der().to_vec();
        let certificates = TlsCertificateStore::new(TlsServerConfig::new(
            vec![cert_der.clone()],
            certified.key_pair.serialize_der(),
        ))
        .expect("SSE gateway certificate store");

        let mut server = HttpServer::start_with_dispatcher(
            ServerConfig {
                tls_certificates: Some(std::sync::Arc::new(certificates)),
                shutdown_grace_period: Duration::from_millis(200),
                ..ServerConfig::default()
            },
            routed_handler(router),
        )
        .expect("start SSE TLS gateway");
        let tls_addr = server.tls_local_addr().expect("TLS gateway address");

        let publisher = Arc::clone(&sse_server);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            for index in 0..3_u32 {
                let event = sse::SseEvent::new(
                    Some(index.to_string()),
                    Some("update".to_string()),
                    format!("tls-{index}"),
                    None,
                )
                .expect("create gateway SSE event");
                assert_eq!(
                    publisher
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .publish_event(&event)
                        .expect("publish gateway SSE event"),
                    1
                );
            }
        });

        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("SSE gateway test runtime");
        runtime.block_on(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let client_config = TlsClientConfig::with_roots(vec![cert_der])
                .with_alpn_protocols(vec![b"http/1.1".to_vec()])
                .build()
                .expect("SSE gateway client config");
            let stream = TcpStream::connect(tls_addr)
                .await
                .expect("connect SSE gateway");
            let connector = TlsConnector::from(client_config);
            let name = ServerName::try_from("localhost").expect("server name");
            let mut tls_stream = connector
                .connect(name, stream)
                .await
                .expect("SSE gateway TLS handshake");
            assert_eq!(
                tls_stream.get_ref().1.alpn_protocol(),
                Some(&b"http/1.1"[..])
            );
            tls_stream
                .write_all(
                    b"GET /events HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n",
                )
                .await
                .expect("write gateway SSE request");

            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut raw = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                let read = tokio::time::timeout(
                    Duration::from_millis(500),
                    tls_stream.read(&mut buffer),
                )
                .await;
                match read {
                    Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                    Ok(Ok(read)) => raw.extend_from_slice(&buffer[..read]),
                }
                let text = String::from_utf8_lossy(&raw);
                if text.matches("data: tls-").count() >= 3 && text.contains(": heartbeat") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&raw);
            assert!(
                text.contains("text/event-stream"),
                "gateway SSE response missing headers: {text}"
            );
            for index in 0..3_u32 {
                assert!(
                    text.contains(&format!("id: {index}\nevent: update\ndata: tls-{index}\n")),
                    "gateway SSE event {index} missing: {text}"
                );
            }
            assert!(
                text.contains(": heartbeat"),
                "gateway SSE stream missing heartbeat: {text}"
            );
        });

        server.shutdown().expect("shutdown SSE TLS gateway");
    }

    #[test]
    fn tls_gateway_upgrades_routed_websocket_and_echoes_masked_frames_on_the_tls_port() {
        use crate::tls::{TlsCertificateStore, TlsClientConfig, TlsServerConfig};
        use crate::websocket::{encode_frame, FrameRole, WebSocketFrame};
        use rcgen::generate_simple_self_signed;
        use rustls::pki_types::ServerName;
        use tokio::net::TcpStream;
        use tokio::runtime::Builder;
        use tokio_rustls::TlsConnector;

        type TlsClientLeg = tokio_rustls::client::TlsStream<TcpStream>;

        async fn write_client_frame(
            stream: &mut TlsClientLeg,
            opcode: u8,
            payload: Vec<u8>,
        ) -> bool {
            use tokio::io::AsyncWriteExt;

            let frame = WebSocketFrame {
                fin: true,
                rsv1: false,
                opcode,
                masked: true,
                payload,
            };
            match encode_frame(&frame, FrameRole::Client) {
                Ok(encoded) => stream.write_all(&encoded).await.is_ok(),
                Err(_) => false,
            }
        }

        /// Reads one server frame (unmasked per RFC 6455) into
        /// `(fin, opcode, payload)`.
        async fn read_server_frame(
            stream: &mut TlsClientLeg,
            pending: &mut Vec<u8>,
        ) -> Option<(bool, u8, Vec<u8>)> {
            use tokio::io::AsyncReadExt;

            let mut chunk = [0_u8; 4096];
            loop {
                if pending.len() >= 2 {
                    let fin = pending[0] & 0x80 != 0;
                    assert_eq!(
                        pending[1] & 0x80,
                        0,
                        "server frames must not be masked"
                    );
                    let opcode = pending[0] & 0x0f;
                    let length_marker = (pending[1] & 0x7f) as usize;
                    let header_len = 2 + if length_marker == 126 { 2 } else { 0 };
                    if length_marker != 127 && pending.len() >= header_len {
                        let length = if length_marker == 126 {
                            u16::from_be_bytes([pending[2], pending[3]]) as usize
                        } else {
                            length_marker
                        };
                        if pending.len() >= header_len + length {
                            let payload =
                                pending[header_len..header_len + length].to_vec();
                            pending.drain(..header_len + length);
                            return Some((fin, opcode, payload));
                        }
                    }
                }
                let read = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut chunk))
                    .await
                    .ok()?
                    .ok()?;
                if read == 0 {
                    return None;
                }
                pending.extend_from_slice(&chunk[..read]);
            }
        }

        let websocket_server = Arc::new(Mutex::new(crate::websocket::WebSocketServer::new()));
        let mut router = routing::Router::default();
        let route = router
            .add(routing::RouteMethod::Get, "/socket")
            .expect("gateway WebSocket route");
        crate::websocket::register_server_route(Arc::clone(&websocket_server), route)
            .expect("attach gateway WebSocket route");

        let certified = generate_simple_self_signed(vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ])
        .expect("self-signed WebSocket gateway certificate");
        let cert_der = certified.cert.der().to_vec();
        let certificates = TlsCertificateStore::new(TlsServerConfig::new(
            vec![cert_der.clone()],
            certified.key_pair.serialize_der(),
        ))
        .expect("WebSocket gateway certificate store");

        let mut server = HttpServer::start_with_dispatcher(
            ServerConfig {
                tls_certificates: Some(std::sync::Arc::new(certificates)),
                shutdown_grace_period: Duration::from_millis(200),
                ..ServerConfig::default()
            },
            routed_handler(router),
        )
        .expect("start WebSocket TLS gateway");
        let tls_addr = server.tls_local_addr().expect("TLS gateway address");

        let runtime = Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("WebSocket gateway test runtime");
        runtime.block_on(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let client_config = TlsClientConfig::with_roots(vec![cert_der])
                .with_alpn_protocols(vec![b"http/1.1".to_vec()])
                .build()
                .expect("WebSocket gateway client config");
            let stream = TcpStream::connect(tls_addr)
                .await
                .expect("connect WebSocket gateway");
            let connector = TlsConnector::from(client_config);
            let name = ServerName::try_from("localhost").expect("server name");
            let mut tls_stream = connector
                .connect(name, stream)
                .await
                .expect("WebSocket gateway TLS handshake");
            assert_eq!(
                tls_stream.get_ref().1.alpn_protocol(),
                Some(&b"http/1.1"[..])
            );
            tls_stream
                .write_all(
                    b"GET /socket HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
                )
                .await
                .expect("write gateway upgrade request");

            let mut head = Vec::new();
            let mut chunk = [0_u8; 512];
            while !head.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = tokio::time::timeout(Duration::from_secs(3), tls_stream.read(&mut chunk))
                    .await
                    .expect("upgrade head read did not time out")
                    .expect("upgrade head read succeeds");
                assert!(read > 0, "peer closed before the 101 handshake");
                head.extend_from_slice(&chunk[..read]);
            }
            let head_end = head
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("upgrade head terminator");
            let head_text = String::from_utf8_lossy(&head[..head_end]).to_string();
            assert!(
                head_text.starts_with("HTTP/1.1 101 Switching Protocols"),
                "unexpected upgrade response: {head_text}"
            );
            assert!(
                head_text.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
                "unexpected Sec-WebSocket-Accept: {head_text}"
            );

            // Frames may share the final TLS record with the 101 head.
            let mut pending = head.split_off(head_end + 4);

            // Masked text frame echoes back with identical payload.
            assert!(write_client_frame(&mut tls_stream, 0x1, b"tls-mask".to_vec()).await);
            let (_, opcode, payload) =
                read_server_frame(&mut tls_stream, &mut pending).await.expect("text echo");
            assert_eq!((opcode, payload), (0x1, b"tls-mask".to_vec()));

            // Masked binary frame echoes back as binary.
            assert!(write_client_frame(&mut tls_stream, 0x2, vec![9, 8, 7]).await);
            let (_, opcode, payload) =
                read_server_frame(&mut tls_stream, &mut pending).await.expect("binary echo");
            assert_eq!((opcode, payload), (0x2, vec![9, 8, 7]));

            // Ping is answered by a pong carrying the application payload.
            assert!(write_client_frame(&mut tls_stream, 0x9, b"hb".to_vec()).await);
            let (_, opcode, payload) =
                read_server_frame(&mut tls_stream, &mut pending).await.expect("pong");
            assert_eq!((opcode, payload), (0xA, b"hb".to_vec()));

            // Close handshake echoes the close code and ends the pump.
            let mut close_payload = 1000_u16.to_be_bytes().to_vec();
            close_payload.extend_from_slice(b"done");
            assert!(write_client_frame(&mut tls_stream, 0x8, close_payload.clone()).await);
            let (_, opcode, payload) =
                read_server_frame(&mut tls_stream, &mut pending).await.expect("close echo");
            assert_eq!((opcode, payload), (0x8, close_payload));
        });

        server.shutdown().expect("shutdown WebSocket TLS gateway");
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
