#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{HttpServer, ServerConfig, ServerResponse};
    use crate::tls::{serve_single_https_request, TlsClientConfig, TlsServerConfig};
    use rcgen::generate_simple_self_signed;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };
    use std::thread;

    // Client tests share process-global tracing state; serialize the whole
    // module so unrelated requests cannot publish spans to another test's
    // collector.
    static CLIENT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn local_client_config() -> ClientConfig {
        ClientConfig::default().allow_private_networks(true)
    }

    fn spawn_test_collector(
        listener: TcpListener,
    ) -> (thread::JoinHandle<Vec<u8>>, mpsc::Sender<()>) {
        let (stop_tx, stop_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            listener
                .set_nonblocking(true)
                .expect("collector nonblocking mode");
            let mut payload = Vec::new();
            loop {
                match stop_rx.try_recv() {
                    Ok(()) | Err(mpsc::TryRecvError::Disconnected) => return payload,
                    Err(mpsc::TryRecvError::Empty) => {}
                }

                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("collector request: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("collector read timeout");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                let body_start = loop {
                    let read = stream.read(&mut buffer).expect("collector read");
                    assert!(read > 0, "collector request truncated");
                    request.extend_from_slice(&buffer[..read]);
                    if let Some(index) = request
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                    {
                        break index + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&request[..body_start]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("Content-Length:")
                            .or_else(|| line.strip_prefix("content-length:"))
                    })
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .expect("content length");
                while request.len() < body_start + content_length {
                    let read = stream.read(&mut buffer).expect("collector body read");
                    assert!(read > 0, "collector body truncated");
                    request.extend_from_slice(&buffer[..read]);
                }
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .expect("collector response");
                payload.extend_from_slice(&request[body_start..body_start + content_length]);
            }
        });
        (join, stop_tx)
    }

    fn start_client_test_server() -> HttpServer {
        HttpServer::start(
            ServerConfig {
                idle_timeout: Duration::from_millis(250),
                ..ServerConfig::default()
            },
            Arc::new(|request| match request.target.as_str() {
                "/echo" => ServerResponse::text(
                    200,
                    format!(
                        "{}:{}",
                        request.method,
                        String::from_utf8_lossy(&request.body.bytes())
                    ),
                ),
                "/large" => ServerResponse::bytes(200, request.body.bytes()),
                "/trace" => ServerResponse::text(
                    200,
                    request
                        .headers
                        .iter()
                        .find(|header| header.name.eq_ignore_ascii_case("traceparent"))
                        .map(|header| header.value.clone())
                        .unwrap_or_default(),
                ),
                "/redirect-post" => redirect_response(302, "/landed"),
                "/redirect-preserve" => redirect_response(307, "/echo"),
                "/loop" => redirect_response(302, "/loop"),
                "/landed" => ServerResponse::text(
                    200,
                    format!("{}:{}", request.method, request.body.bytes().len()),
                ),
                "/slow" => {
                    thread::sleep(Duration::from_millis(120));
                    ServerResponse::text(200, "slow")
                }
                _ => ServerResponse::text(404, "missing"),
            }),
        )
        .expect("client test server starts")
    }

    fn redirect_response(status: u16, location: &str) -> ServerResponse {
        ServerResponse {
            status_code: status,
            reason: "Redirect".to_string(),
            headers: vec![Header {
                name: "Location".to_string(),
                value: location.to_string(),
            }],
            body: HttpBody::empty(),
            close: false,
        }
    }

    fn url(server: &HttpServer, path: &str) -> String {
        format!("http://{}{}", server.local_addr(), path)
    }

    #[test]
    fn client_supports_methods_and_arbitrary_bodies() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mut server = start_client_test_server();
        let client = HttpClient::new(local_client_config());

        let get = client.get(&url(&server, "/echo")).expect("GET");
        assert_eq!(get.body.bytes(), b"GET:");

        let post = client
            .post(&url(&server, "/echo"), b"post-body".to_vec())
            .expect("POST");
        assert_eq!(post.body.bytes(), b"POST:post-body");

        let put = client
            .put(&url(&server, "/echo"), b"put-body".to_vec())
            .expect("PUT");
        assert_eq!(put.body.bytes(), b"PUT:put-body");

        let patch = client
            .patch(&url(&server, "/echo"), b"patch-body".to_vec())
            .expect("PATCH");
        assert_eq!(patch.body.bytes(), b"PATCH:patch-body");

        let delete = client
            .request(
                ClientRequest::new("DELETE", url(&server, "/echo")).with_body(b"gone".to_vec()),
            )
            .expect("DELETE");
        assert_eq!(delete.body.bytes(), b"DELETE:gone");

        let head = client.head(&url(&server, "/echo")).expect("HEAD");
        assert_eq!(head.status_code, 200);
        assert!(head.body.bytes().is_empty());

        let stats = client.stats();
        assert!(stats.opened_connections >= 1);
        assert!(client.stats().pooled_connections >= 1);
        let _ = server.shutdown();
    }

    #[test]
    fn client_injects_w3c_trace_context_and_emits_client_span() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let collector = TcpListener::bind("127.0.0.1:0").expect("collector bind");
        let collector_addr = collector.local_addr().expect("collector address");
        let collector_thread = thread::spawn(move || {
            let (mut stream, _) = collector.accept().expect("collector request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            let body_start;
            loop {
                let read = stream.read(&mut buffer).expect("collector read");
                assert!(read > 0, "collector request truncated");
                request.extend_from_slice(&buffer[..read]);
                if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    body_start = index + 4;
                    break;
                }
            }
            let headers = String::from_utf8_lossy(&request[..body_start]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Content-Length:")
                        .or_else(|| line.strip_prefix("content-length:"))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            while request.len() < body_start + content_length {
                let read = stream.read(&mut buffer).expect("collector body read");
                assert!(read > 0, "collector body truncated");
                request.extend_from_slice(&buffer[..read]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("collector response");
            request[body_start..body_start + content_length].to_vec()
        });
        let config = tracing::config_new(
            &format!("http://{collector_addr}/v1/traces"),
            "spectra-api-client-test",
        )
        .expect("trace config");
        tracing::config_start(config).expect("trace worker");
        let root = tracing::span_start("client.test", SpanKind::Internal).expect("root span");
        let expected = tracing::inject(root).expect("root traceparent");
        let mut server = start_client_test_server();
        let client = HttpClient::new(local_client_config());
        let response = client.get(&url(&server, "/trace")).expect("traced request");
        let propagated = String::from_utf8(response.body.bytes()).unwrap();
        assert!(
            propagated.starts_with(&expected[..35]),
            "trace ID must propagate"
        );
        assert_ne!(propagated, expected, "client span must be a child span");
        tracing::span_end(root).expect("root end");
        tracing::flush().expect("client trace flush");
        tracing::config_shutdown(config).expect("client trace shutdown");
        let payload = collector_thread.join().expect("collector thread");
        assert!(payload
            .windows(b"http.client".len())
            .any(|window| window == b"http.client"));
        let _ = server.shutdown();
    }

    #[test]
    fn concurrent_requests_isolate_trace_context() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let collector = TcpListener::bind("127.0.0.1:0").expect("collector bind");
        let collector_addr = collector.local_addr().expect("collector address");
        let (collector_thread, collector_stop) = spawn_test_collector(collector);
        let config = tracing::config_new(
            &format!("http://{collector_addr}/v1/traces"),
            "spectra-api-concurrency-test",
        ).expect("trace config");
        tracing::config_start(config).expect("trace worker");
        let mut server = start_client_test_server();
        let address = server.local_addr();
        let client = Arc::new(HttpClient::new(local_client_config()));
        let handles = (0..8)
            .map(|_| {
                let client = Arc::clone(&client);
                thread::spawn(move || {
                    let root = tracing::span_start("request.root", SpanKind::Server).expect("root");
                    let expected = tracing::inject(root).expect("traceparent");
                    let response = client
                        .get(&format!("http://{address}/trace"))
                        .expect("request");
                    let propagated =
                        String::from_utf8(response.body.bytes()).expect("traceparent text");
                    tracing::span_end(root).expect("root end");
                    (expected, propagated)
                })
            })
            .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("request thread"))
            .collect::<Vec<_>>();
        let trace_ids = results
            .iter()
            .map(|(value, _)| &value[..35])
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            trace_ids.len(),
            results.len(),
            "concurrent requests shared a trace ID"
        );
        for (expected, propagated) in results {
            assert!(propagated.starts_with(&expected[..35]));
            assert_ne!(propagated, expected);
        }
        tracing::flush().expect("concurrent trace flush");
        tracing::config_shutdown(config).expect("concurrent trace shutdown");
        collector_stop.send(()).expect("stop collector");
        assert!(!collector_thread.join().expect("collector thread").is_empty());
        let _ = server.shutdown();
    }

    #[test]
    fn client_reuses_pooled_connection() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mut server = start_client_test_server();
        let client = HttpClient::new(local_client_config());
        client.get(&url(&server, "/echo")).expect("first GET");
        client.get(&url(&server, "/echo")).expect("second GET");
        let stats = client.stats();
        assert_eq!(stats.opened_connections, 1);
        assert!(stats.reused_connections >= 1);
        let _ = server.shutdown();
    }

    #[test]
    fn client_follows_redirects_with_method_semantics() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mut server = start_client_test_server();
        let client = HttpClient::new(local_client_config());

        let converted = client
            .post(&url(&server, "/redirect-post"), b"discarded".to_vec())
            .expect("302 POST redirect");
        assert_eq!(converted.status_code, 200);
        assert_eq!(converted.body.bytes(), b"GET:0");
        assert_eq!(converted.redirect_count, 1);

        let preserved = client
            .post(&url(&server, "/redirect-preserve"), b"kept".to_vec())
            .expect("307 POST redirect");
        assert_eq!(preserved.body.bytes(), b"POST:kept");
        assert_eq!(preserved.redirect_count, 1);

        let stats = client.stats();
        assert_eq!(stats.redirects_followed, 2);
        let _ = server.shutdown();
    }

    #[test]
    fn client_enforces_redirect_limit() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mut server = start_client_test_server();
        let client = HttpClient::new(ClientConfig {
            max_redirects: 2,
            ..local_client_config()
        });
        let err = client
            .get(&url(&server, "/loop"))
            .expect_err("redirect limit");
        assert_eq!(err.kind, ClientErrorKind::RedirectLimit);
        let _ = server.shutdown();
    }

    #[test]
    fn client_handles_large_bodies() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mut server = start_client_test_server();
        let client = HttpClient::new(ClientConfig {
            max_body_bytes: 512 * 1024,
            ..local_client_config()
        });
        let body = vec![b'x'; 256 * 1024];
        let response = client
            .post(&url(&server, "/large"), body.clone())
            .expect("large body");
        assert_eq!(response.body.bytes(), body);
        let _ = server.shutdown();
    }

    #[test]
    fn client_reports_explicit_timeout() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mut server = start_client_test_server();
        let client = HttpClient::new(ClientConfig {
            timeout: Duration::from_millis(20),
            ..local_client_config()
        });
        let err = client.get(&url(&server, "/slow")).expect_err("timeout");
        assert_eq!(err.kind, ClientErrorKind::Timeout);
        let _ = server.shutdown();
    }

    #[test]
    fn client_reports_connection_failure() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        // The dropped listener's port can be grabbed by another test server
        // between drop and connect; retry with fresh ports so a stolen port
        // (successful connect) never fails the test.
        for _ in 0..10 {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused port");
            let addr = listener.local_addr().expect("local addr");
            drop(listener);
            let client = HttpClient::new(ClientConfig {
                timeout: Duration::from_millis(50),
                ..local_client_config()
            });
            match client.get(&format!("http://{addr}/missing")) {
                Err(err) => {
                    assert_eq!(err.kind, ClientErrorKind::ConnectionFailed);
                    return;
                }
                Ok(_) => continue,
            }
        }
        panic!("could not observe a refused connection in 10 attempts");
    }

    #[test]
    fn client_reports_protocol_error() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind protocol server");
        let addr = listener.local_addr().expect("local addr");
        let join = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept protocol test");
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .expect("set read timeout");
            let mut request = [0_u8; 512];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"not-http\r\nContent-Length: 0\r\n\r\n")
                .expect("write invalid");
            stream.flush().expect("flush invalid response");
        });
        let client = HttpClient::new(local_client_config());
        let err = client
            .get(&format!("http://{addr}/bad"))
            .expect_err("protocol error");
        assert_eq!(err.kind, ClientErrorKind::Protocol);
        join.join().expect("protocol server joined");
    }

    #[test]
    fn client_nonblocking_request_observes_cancellation_during_readiness_wait() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mut server = start_client_test_server();
        let token = Arc::new(AtomicBool::new(false));
        let worker_token = Arc::clone(&token);
        let client = Arc::new(HttpClient::new(local_client_config()));
        let url = url(&server, "/slow");
        let worker = thread::spawn(move || {
            client.request_nonblocking(ClientRequest::new("GET", url), worker_token)
        });
        // Cancel promptly: /slow answers after 120ms, so a flag set here always
        // lands during the readiness wait. No fixed sleep: under load a sleep
        // can overshoot the response and let the request complete first.
        token.store(true, Ordering::Release);
        let result = worker.join().expect("nonblocking client worker");
        let error = result.expect_err("cancelled request must not complete");
        assert!(error.message.contains("cancelled"));
        let _ = server.shutdown();
    }

    #[test]
    fn client_nonblocking_request_supports_validated_https() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let certified = generate_simple_self_signed(vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ])
        .expect("self-signed HTTPS certificate");
        let cert_der = certified.cert.der().to_vec();
        let client_tls = TlsClientConfig::with_roots(vec![cert_der.clone()])
            .build()
            .expect("client TLS configuration");
        let server_tls = TlsServerConfig::new(
            vec![cert_der],
            certified.key_pair.serialize_der(),
        )
        .build()
        .expect("server TLS configuration");
        let listener = TcpListener::bind("127.0.0.1:0").expect("HTTPS listener");
        let address = listener.local_addr().expect("HTTPS address");
        let server = thread::spawn(move || {
            serve_single_https_request(
                listener,
                server_tls,
                b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecure"
                    .to_vec(),
                Duration::from_secs(5),
            )
        });

        let client = HttpClient::new(
            ClientConfig::default()
                .allow_private_networks(true)
                .with_tls_config(client_tls),
        );
        let token = Arc::new(AtomicBool::new(false));
        let response = client
            .request_nonblocking(
                ClientRequest::new("GET", format!("https://127.0.0.1:{}/secure", address.port())),
                token,
            )
            .expect("validated asynchronous HTTPS request");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.bytes(), b"secure");
        assert!(response.final_url.starts_with("https://127.0.0.1:"));

        let exchange = server
            .join()
            .expect("HTTPS server thread joins")
            .expect("HTTPS exchange");
        assert!(String::from_utf8_lossy(&exchange.request).starts_with("GET /secure HTTP/1.1"));
    }
    use spectra_runtime::ffi::{SpectraHostCallContext, SpectraHostValue, HOST_STATUS_INVALID_ARGUMENT, HOST_STATUS_SUCCESS};

    fn call_host(
        function: extern "C" fn(*mut SpectraHostCallContext) -> i32,
        args: &[SpectraHostValue],
    ) -> (i32, SpectraHostValue) {
        let mut result = [0_i64];
        let mut ctx = SpectraHostCallContext {
            args: args.as_ptr(),
            arg_len: args.len(),
            results: result.as_mut_ptr(),
            result_len: result.len(),
            invoke_fn: None,
        };
        let status = function(&mut ctx);
        (status, result[0])
    }

    fn spectra_string(text: &str) -> SpectraHostValue {
        crate::alloc_spectra_string(text)
    }

    fn tls_loopback_server(
        cert_der: Vec<u8>,
        key_der: Vec<u8>,
        body: &'static str,
    ) -> (
        std::net::SocketAddr,
        thread::JoinHandle<Result<crate::tls::HttpsServerExchange, crate::tls::TlsError>>,
    ) {
        use crate::tls::TlsServerConfig;
        let server_tls = TlsServerConfig::new(vec![cert_der], key_der)
            .build()
            .expect("server TLS configuration");
        let listener = TcpListener::bind("127.0.0.1:0").expect("HTTPS listener");
        let address = listener.local_addr().expect("HTTPS address");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let server = thread::spawn(move || {
            serve_single_https_request(
                listener,
                server_tls,
                response.into_bytes(),
                Duration::from_secs(5),
            )
        });
        (address, server)
    }

    fn fetch_status_via_hosts(client: SpectraHostValue, url: &str) -> Result<i64, i32> {
        let (_, request) = call_host(crate::http::request, &[1, spectra_string(url)]);
        let (status, task) = call_host(client_request, &[client, request]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let response = spectra_runtime::stdlib::block_on_task_value(task)?;
        let (status, code) = call_host(crate::http::response_status, &[response]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        Ok(code)
    }

    #[test]
    fn client_tls_config_pins_custom_roots_through_hosts() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let certified = generate_simple_self_signed(vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ])
        .expect("self-signed HTTPS certificate");
        let cert_der = certified.cert.der().to_vec();
        let key_der = certified.key_pair.serialize_der();

        // Wiring validation first: unknown handles and garbage roots fail fast.
        let (status, _) = call_host(client_set_tls_config, &[0, 0]);
        assert_eq!(status, HOST_STATUS_INVALID_ARGUMENT);
        let (status, tls) = call_host(crate::tls::tls_client_config, &[]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let (status, _) = call_host(
            crate::tls::tls_config_add_root,
            &[tls, spectra_string("!!!not-base64!!!")],
        );
        assert_eq!(status, HOST_STATUS_INVALID_ARGUMENT);
        let (status, server_tls) = call_host(crate::tls::tls_config_new, &[1]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let (status, _) = call_host(client_set_tls_config, &[0, server_tls]);
        assert_eq!(status, HOST_STATUS_INVALID_ARGUMENT);

        // Pinned fetch through the hosts succeeds.
        let (pinned_address, pinned_server) =
            tls_loopback_server(cert_der.clone(), key_der.clone(), "pinned");
        let (status, client) = call_host(client_new, &[]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let (status, policy) = call_host(crate::security::ssrf_policy, &[]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let (status, policy) =
            call_host(crate::security::ssrf_allow_private_networks, &[policy, 1]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let (status, _) = call_host(client_set_ssrf_policy, &[client, policy]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let cert_b64 = spectra_string(&crate::grpc_host::encode_base64(&cert_der));
        let (status, added) = call_host(crate::tls::tls_config_add_root, &[tls, cert_b64]);
        assert_eq!((status, added), (HOST_STATUS_SUCCESS, 1));
        let (status, pinned) = call_host(client_set_tls_config, &[client, tls]);
        assert_eq!((status, pinned), (HOST_STATUS_SUCCESS, 1));
        let code = match fetch_status_via_hosts(
            client,
            &format!("https://127.0.0.1:{}/pinned", pinned_address.port()),
        ) {
            Ok(code) => code,
            Err(status) => panic!("pinned fetch failed with host status {status}"),
        };
        assert_eq!(code, 200);
        let exchange = pinned_server.join().expect("pinned server joins");
        assert!(
            exchange.is_ok(),
            "pinned server must complete the HTTPS exchange"
        );

        // The webpki default honestly fails against the private CA.
        let (default_address, default_server) =
            tls_loopback_server(cert_der, key_der, "unpinned");
        let (status, plain) = call_host(client_new, &[]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let (status, policy) = call_host(crate::security::ssrf_policy, &[]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let (status, policy) =
            call_host(crate::security::ssrf_allow_private_networks, &[policy, 1]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let (status, _) = call_host(client_set_ssrf_policy, &[plain, policy]);
        assert_eq!(status, HOST_STATUS_SUCCESS);
        let failure = fetch_status_via_hosts(
            plain,
            &format!("https://127.0.0.1:{}/unpinned", default_address.port()),
        );
        assert!(failure.is_err(), "webpki default must not trust the private CA");
        let exchange = default_server.join().expect("default server joins");
        assert!(
            exchange.is_err(),
            "aborted handshake must surface server-side too"
        );
    }

    #[test]
    fn sync_client_parses_bracketed_ipv6_authorities() {
        assert_eq!(
            parse_authority("[::1]:8080", 80).expect("bracketed with port"),
            ("::1".to_string(), 8080)
        );
        assert_eq!(
            parse_authority("[::1]", 80).expect("bracketed default port"),
            ("::1".to_string(), 80)
        );
        assert_eq!(
            parse_authority("[2001:db8::1]:443", 80).expect("full address"),
            ("2001:db8::1".to_string(), 443)
        );
        assert!(parse_authority("[::1]x", 80).is_err());
        assert!(parse_authority("[]:80", 80).is_err());
        assert!(parse_authority("[::1]:", 80).is_err());
        assert!(parse_authority("[::1]:abc", 80).is_err());
        // Unbracketed hosts keep the established host:port split.
        assert_eq!(
            parse_authority("example.com:8080", 80).expect("hostname port"),
            ("example.com".to_string(), 8080)
        );
        assert_eq!(
            parse_authority("example.com", 80).expect("hostname default"),
            ("example.com".to_string(), 80)
        );
    }

    #[test]
    fn sync_client_fetches_bracketed_ipv6_loopback() {
        let _guard = CLIENT_TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let listener = match TcpListener::bind("[::1]:0") {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("skipping IPv6 loopback test: {error}");
                return;
            }
        };
        let address = listener.local_addr().expect("IPv6 loopback address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("IPv6 accept");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).expect("read request");
                assert!(read > 0, "request truncated");
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8(request).expect("request is UTF-8");
            assert!(text.starts_with("GET / HTTP/1.1"), "unexpected request: {text}");
            assert!(
                text.lines().any(|line| line.starts_with("Host: [::1]")),
                "Host header must bracket the literal: {text}"
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi")
                .expect("write response");
        });
        let client = HttpClient::new(local_client_config());
        let token = Arc::new(AtomicBool::new(false));
        let response = client
            .request_nonblocking(
                ClientRequest::new("GET", format!("http://[::1]:{}/", address.port())),
                token,
            )
            .expect("IPv6 loopback fetch");
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.bytes(), b"hi");
        server.join().expect("IPv6 server joins");
    }
}
