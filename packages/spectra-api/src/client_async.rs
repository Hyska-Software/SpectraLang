use mio::net::TcpStream as MioTcpStream;
use mio::{Events, Interest, Poll, Token};
use rustls::pki_types::ServerName;
use rustls::ClientConnection;

const ASYNC_CLIENT_TOKEN: Token = Token(0);

impl HttpClient {
    /// Executes an HTTP/1.1 request with non-blocking socket readiness.
    ///
    /// DNS resolution is intentionally performed inside the bounded I/O worker
    /// that owns this operation; after resolution, connect, write, and read
    /// progress exclusively through the platform backend selected by `mio`.
    pub(crate) fn request_nonblocking(
        &self,
        request: ClientRequest,
        cancellation: spectra_runtime::stdlib::CancellationToken,
    ) -> Result<ClientResponse, ClientError> {
        let span = tracing::begin_external_span(SpanKind::Client, "http.client").ok();
        if let Some(id) = span {
            let _ = tracing::span_set_attribute(id, "http.request.method", &request.method);
            let _ = tracing::span_set_attribute(id, "url.full", &request.url);
        }

        let mut request = request;
        if let Some(traceparent) = tracing::current_traceparent() {
            upsert_header(&mut request.headers, "traceparent", &traceparent);
        }
        let result = self.request_nonblocking_inner(request, &cancellation);

        if let Some(id) = span {
            if let Ok(response) = &result {
                let _ = tracing::span_set_attribute_int(
                    id,
                    "http.response.status_code",
                    response.status_code as i64,
                );
                let _ = tracing::span_set_attribute_int(
                    id,
                    "http.response.body.size",
                    response.body.bytes().len() as i64,
                );
            }
            let _ = tracing::span_set_status(
                id,
                if result.is_ok() {
                    SpanStatus::Ok
                } else {
                    SpanStatus::Error
                },
            );
            let _ = tracing::span_end(id);
        }
        result
    }

    fn request_nonblocking_inner(
        &self,
        request: ClientRequest,
        cancellation: &spectra_runtime::stdlib::CancellationToken,
    ) -> Result<ClientResponse, ClientError> {
        let mut current = request;
        for redirect_count in 0..=self.config.max_redirects {
            check_async_client_cancelled(cancellation)?;
            let parsed_url = parse_url(&current.url, true)?;
            let addresses = self.resolve_destination(&parsed_url.authority)?;
            let response =
                self.send_once_nonblocking(&current, &parsed_url, &addresses, cancellation)?;
            if let Some(next_url) = redirect_target_with_options(&response, &current.url, true)? {
                if redirect_count == self.config.max_redirects {
                    return Err(ClientError::new(
                        ClientErrorKind::RedirectLimit,
                        "redirect limit exceeded",
                    ));
                }
                current = redirected_request_with_options(
                    current,
                    next_url,
                    response.status_code,
                    true,
                )?;
                self.stats
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .redirects_followed += 1;
                continue;
            }

            return Ok(ClientResponse {
                status_code: response.status_code,
                reason: response.reason,
                headers: response.headers,
                body: response.body,
                final_url: current.url,
                redirect_count,
                keep_alive: response.keep_alive,
            });
        }

        Err(ClientError::new(
            ClientErrorKind::RedirectLimit,
            "redirect loop exceeded configured limit",
        ))
    }

    fn send_once_nonblocking(
        &self,
        request: &ClientRequest,
        parsed_url: &ParsedUrl,
        addresses: &[std::net::SocketAddr],
        cancellation: &spectra_runtime::stdlib::CancellationToken,
    ) -> Result<ParsedResponse, ClientError> {
        if parsed_url.scheme.is_tls() {
            return self.send_once_nonblocking_tls(request, parsed_url, addresses, cancellation);
        }
        self.send_once_nonblocking_http(request, parsed_url, addresses, cancellation)
    }

    fn send_once_nonblocking_http(
        &self,
        request: &ClientRequest,
        parsed_url: &ParsedUrl,
        addresses: &[std::net::SocketAddr],
        cancellation: &spectra_runtime::stdlib::CancellationToken,
    ) -> Result<ParsedResponse, ClientError> {
        let address = *addresses.first().ok_or_else(|| {
            ClientError::new(
                ClientErrorKind::ConnectionFailed,
                "URL host did not resolve to an address",
            )
        })?;
        check_async_client_cancelled(cancellation)?;

        let mut stream = MioTcpStream::connect(address)
            .map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?;
        let mut poll = Poll::new().map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?;
        poll.registry()
            .register(&mut stream, ASYNC_CLIENT_TOKEN, Interest::WRITABLE)
            .map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?;

        let deadline = Instant::now() + self.config.timeout;
        let wire = build_request_wire(request, parsed_url, &self.config);
        let mut written = 0usize;
        let mut connected = false;
        let mut parser = Http1Parser::response_with_config(self.config.parser_config());
        let mut head_buffer = Vec::new();
        let mut events = Events::with_capacity(8);
        let mut buffer = [0_u8; 8192];

        self.stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .opened_connections += 1;

        loop {
            check_async_client_cancelled(cancellation)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ClientError::new(
                    ClientErrorKind::Timeout,
                    "HTTP client timed out waiting for readiness",
                ));
            }
            let wait = remaining.min(Duration::from_millis(10));
            poll.poll(&mut events, Some(wait))
                .map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?;
            if events.is_empty() {
                continue;
            }

            for event in events.iter() {
                if event.token() != ASYNC_CLIENT_TOKEN {
                    continue;
                }
                if !connected {
                    if let Some(error) = stream
                        .take_error()
                        .map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?
                    {
                        return Err(io_error(ClientErrorKind::ConnectionFailed, error));
                    }
                    if event.is_error() && !event.is_writable() && !event.is_readable() {
                        return Err(ClientError::new(
                            ClientErrorKind::ConnectionFailed,
                            "HTTP connection failed before becoming writable",
                        ));
                    }
                    connected = true;
                }

                if event.is_writable() && written < wire.len() {
                    loop {
                        check_async_client_cancelled(cancellation)?;
                        match std::io::Write::write(&mut stream, &wire[written..]) {
                            Ok(0) => {
                                return Err(ClientError::new(
                                    ClientErrorKind::ConnectionFailed,
                                    "HTTP connection closed while writing request",
                                ));
                            }
                            Ok(count) => written += count,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                            Err(error) => return Err(io_error(ClientErrorKind::ConnectionFailed, error)),
                        }
                        if written == wire.len() {
                            poll.registry()
                                .reregister(&mut stream, ASYNC_CLIENT_TOKEN, Interest::READABLE)
                                .map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?;
                            break;
                        }
                    }
                }

                if written == wire.len() && (event.is_readable() || event.is_read_closed()) {
                    loop {
                        check_async_client_cancelled(cancellation)?;
                        match std::io::Read::read(&mut stream, &mut buffer) {
                            Ok(0) => {
                                return Err(ClientError::new(
                                    ClientErrorKind::Protocol,
                                    "connection closed before a complete HTTP response was received",
                                ));
                            }
                            Ok(count) => {
                                if request.method.eq_ignore_ascii_case("HEAD") {
                                    head_buffer.extend_from_slice(&buffer[..count]);
                                    if let Some(header_end) = head_buffer
                                        .windows(4)
                                        .position(|window| window == b"\r\n\r\n")
                                    {
                                        let response =
                                            parse_head_response_headers(&head_buffer[..header_end])?;
                                        let _ = poll.registry().deregister(&mut stream);
                                        return Ok(response);
                                    }
                                } else {
                                    parser.push(&buffer[..count]);
                                    match parser.parse_next_response() {
                                        Ok(Some(response)) => {
                                            let _ = poll.registry().deregister(&mut stream);
                                            return Ok(response);
                                        }
                                        Ok(None) => {}
                                        Err(error) => {
                                            return Err(ClientError::new(
                                                ClientErrorKind::Protocol,
                                                format!("invalid HTTP response: {error}"),
                                            ));
                                        }
                                    }
                                }
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                            Err(error) => return Err(io_error(ClientErrorKind::ConnectionFailed, error)),
                        }
                    }
                }
            }
            events.clear();
        }
    }

    fn send_once_nonblocking_tls(
        &self,
        request: &ClientRequest,
        parsed_url: &ParsedUrl,
        addresses: &[std::net::SocketAddr],
        cancellation: &spectra_runtime::stdlib::CancellationToken,
    ) -> Result<ParsedResponse, ClientError> {
        let address = *addresses.first().ok_or_else(|| {
            ClientError::new(
                ClientErrorKind::ConnectionFailed,
                "URL host did not resolve to an address",
            )
        })?;
        check_async_client_cancelled(cancellation)?;

        let server_name = ServerName::try_from(parsed_url.authority.host.clone()).map_err(|error| {
            ClientError::new(
                ClientErrorKind::InvalidUrl,
                format!("invalid HTTPS server name: {error}"),
            )
        })?;
        let tls_config = match &self.config.tls_config {
            Some(config) => Arc::clone(config),
            None => crate::tls::TlsClientConfig::with_webpki_roots()
                .build()
                .map_err(|error| {
                    ClientError::new(
                        ClientErrorKind::Protocol,
                        format!("failed to build default HTTPS trust store: {error}"),
                    )
                })?,
        };
        let mut connection = ClientConnection::new(tls_config, server_name).map_err(|error| {
            ClientError::new(
                ClientErrorKind::Protocol,
                format!("failed to create HTTPS client connection: {error}"),
            )
        })?;

        let mut stream = MioTcpStream::connect(address)
            .map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?;
        let mut poll = Poll::new().map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?;
        poll.registry()
            .register(&mut stream, ASYNC_CLIENT_TOKEN, Interest::WRITABLE)
            .map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?;

        let deadline = Instant::now() + self.config.timeout;
        let wire = build_request_wire(request, parsed_url, &self.config);
        let mut written = 0usize;
        let mut connected = false;
        let mut request_sent = false;
        let mut parser = Http1Parser::response_with_config(self.config.parser_config());
        let mut head_buffer = Vec::new();
        let mut events = Events::with_capacity(8);
        let mut buffer = [0_u8; 8192];

        self.stats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .opened_connections += 1;

        loop {
            check_async_client_cancelled(cancellation)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ClientError::new(
                    ClientErrorKind::Timeout,
                    "HTTPS client timed out waiting for readiness",
                ));
            }

            let mut interest = Interest::READABLE;
            if !connected || connection.wants_write() {
                interest |= Interest::WRITABLE;
            }
            poll.registry()
                .reregister(&mut stream, ASYNC_CLIENT_TOKEN, interest)
                .map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?;
            let wait = remaining.min(Duration::from_millis(10));
            poll.poll(&mut events, Some(wait))
                .map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?;
            if events.is_empty() {
                continue;
            }

            for event in events.iter() {
                if event.token() != ASYNC_CLIENT_TOKEN {
                    continue;
                }
                if !connected {
                    if let Some(error) = stream
                        .take_error()
                        .map_err(|error| io_error(ClientErrorKind::ConnectionFailed, error))?
                    {
                        return Err(io_error(ClientErrorKind::ConnectionFailed, error));
                    }
                    if event.is_error() && !event.is_writable() && !event.is_readable() {
                        return Err(ClientError::new(
                            ClientErrorKind::ConnectionFailed,
                            "HTTPS connection failed before becoming writable",
                        ));
                    }
                    connected = true;
                }

                check_async_client_cancelled(cancellation)?;
                if event.is_writable() {
                    flush_tls_socket(&mut connection, &mut stream)?;
                }

                if event.is_readable() || event.is_read_closed() {
                    loop {
                        check_async_client_cancelled(cancellation)?;
                        match connection.read_tls(&mut stream) {
                            Ok(0) => {
                                return Err(ClientError::new(
                                    ClientErrorKind::Protocol,
                                    "HTTPS connection closed during TLS exchange",
                                ));
                            }
                            Ok(_) => {
                                connection.process_new_packets().map_err(|error| {
                                    ClientError::new(
                                        ClientErrorKind::Protocol,
                                        format!("HTTPS TLS handshake failed: {error}"),
                                    )
                                })?;
                                if event.is_readable() {
                                    break;
                                }
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                            Err(error) => {
                                return Err(io_error(ClientErrorKind::ConnectionFailed, error));
                            }
                        }
                    }
                }

                if !connection.is_handshaking() && !request_sent {
                    write_tls_application_data(&mut connection, &wire, &mut written)?;
                    request_sent = written == wire.len();
                }
                if connection.wants_write() {
                    flush_tls_socket(&mut connection, &mut stream)?;
                }

                if request_sent && (event.is_readable() || event.is_read_closed()) {
                    loop {
                        check_async_client_cancelled(cancellation)?;
                        match connection.reader().read(&mut buffer) {
                            Ok(0) => {
                                return Err(ClientError::new(
                                    ClientErrorKind::Protocol,
                                    "HTTPS connection closed before a complete response was received",
                                ));
                            }
                            Ok(count) => {
                                if request.method.eq_ignore_ascii_case("HEAD") {
                                    head_buffer.extend_from_slice(&buffer[..count]);
                                    if let Some(header_end) = head_buffer
                                        .windows(4)
                                        .position(|window| window == b"\r\n\r\n")
                                    {
                                        let response =
                                            parse_head_response_headers(&head_buffer[..header_end])?;
                                        let _ = poll.registry().deregister(&mut stream);
                                        return Ok(response);
                                    }
                                } else {
                                    parser.push(&buffer[..count]);
                                    match parser.parse_next_response() {
                                        Ok(Some(response)) => {
                                            let _ = poll.registry().deregister(&mut stream);
                                            return Ok(response);
                                        }
                                        Ok(None) => {}
                                        Err(error) => {
                                            return Err(ClientError::new(
                                                ClientErrorKind::Protocol,
                                                format!("invalid HTTPS response: {error}"),
                                            ));
                                        }
                                    }
                                }
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                            Err(error) => {
                                return Err(io_error(ClientErrorKind::ConnectionFailed, error));
                            }
                        }
                    }
                }
            }
            events.clear();
        }
    }
}

fn flush_tls_socket(
    connection: &mut ClientConnection,
    stream: &mut MioTcpStream,
) -> Result<(), ClientError> {
    while connection.wants_write() {
        match connection.write_tls(stream) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(io_error(ClientErrorKind::ConnectionFailed, error)),
        }
    }
    Ok(())
}

fn write_tls_application_data(
    connection: &mut ClientConnection,
    wire: &[u8],
    written: &mut usize,
) -> Result<(), ClientError> {
    while *written < wire.len() {
        match connection.writer().write(&wire[*written..]) {
            Ok(0) => {
                return Err(ClientError::new(
                    ClientErrorKind::ConnectionFailed,
                    "HTTPS TLS writer accepted no request bytes",
                ));
            }
            Ok(count) => *written += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(io_error(ClientErrorKind::ConnectionFailed, error)),
        }
    }
    Ok(())
}

fn check_async_client_cancelled(
    cancellation: &spectra_runtime::stdlib::CancellationToken,
) -> Result<(), ClientError> {
    if cancellation.load(std::sync::atomic::Ordering::Acquire) {
        Err(ClientError::new(
            ClientErrorKind::ConnectionFailed,
            "HTTP client request cancelled",
        ))
    } else {
        Ok(())
    }
}
