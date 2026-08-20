#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_and_status_enumerate_documented_http_values() {
        let methods = [
            (METHOD_GET, "GET", true, false),
            (METHOD_HEAD, "HEAD", true, false),
            (METHOD_POST, "POST", false, true),
            (METHOD_PUT, "PUT", false, true),
            (METHOD_PATCH, "PATCH", false, true),
            (METHOD_DELETE, "DELETE", false, false),
            (METHOD_OPTIONS, "OPTIONS", true, false),
        ];
        for (code, name, safe, body) in methods {
            let method = Method::from_code(code).expect("documented method");
            assert_eq!(method.code(), code);
            assert_eq!(method.as_str(), name);
            assert_eq!(method.is_safe(), safe);
            assert_eq!(method.allows_body(), body);
        }
        assert!(Method::from_code(99).is_none());

        for (code, reason, class, success) in [
            (100, "Continue", 1, false),
            (200, "OK", 2, true),
            (201, "Created", 2, true),
            (204, "No Content", 2, true),
            (301, "Moved Permanently", 3, false),
            (400, "Bad Request", 4, false),
            (404, "Not Found", 4, false),
            (409, "Conflict", 4, false),
            (422, "Unprocessable Content", 4, false),
            (429, "Too Many Requests", 4, false),
            (500, "Internal Server Error", 5, false),
            (503, "Service Unavailable", 5, false),
        ] {
            let status = Status::new(code).expect("documented status");
            assert_eq!(status.reason(), reason);
            assert_eq!(status.class(), class);
            assert_eq!(status.is_success(), success);
        }
        assert!(Status::new(99).is_err());
        assert!(Status::new(600).is_err());
    }

    #[test]
    fn headers_and_cookies_are_case_insensitive_and_validate_input() {
        let mut headers = Headers::new();
        headers
            .insert("Content-Type", "application/json")
            .expect("valid header");
        assert_eq!(headers.get("content-type"), Some("application/json"));
        assert!(headers.contains("CONTENT-TYPE"));
        headers
            .insert("content-type", "text/plain")
            .expect("case-insensitive upsert");
        assert_eq!(headers.len(), 1);
        assert_eq!(headers.get("CONTENT-TYPE"), Some("text/plain"));
        assert!(headers.insert("Bad Header", "value").is_err());
        assert!(headers.insert("X-Good", "bad\rvalue").is_err());

        let cookie = Cookie::new("Session", "abc123").expect("valid cookie");
        assert_eq!(cookie.name, "Session");
        assert!(Cookie::new("bad name", "abc").is_err());
        assert!(Cookie::new("session", "bad;value").is_err());
        assert_eq!(
            cookie_value_from_header("SESSION=abc123; theme=dark", "session"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn cookies_support_attributes_set_cookie_serialization_and_signatures() {
        let cookie = Cookie::with_options(
            "session",
            "abc123",
            Some("/account".to_string()),
            Some("example.com".to_string()),
            Some(3600),
            true,
            true,
            CookieSameSite::Lax,
        )
        .expect("valid cookie options");
        assert_eq!(cookie.path.as_deref(), Some("/account"));
        assert_eq!(cookie.domain.as_deref(), Some("example.com"));
        assert_eq!(cookie.max_age, Some(3600));
        assert!(cookie.secure);
        assert!(cookie.http_only);
        assert_eq!(cookie.same_site, CookieSameSite::Lax);
        let header = cookie.header_value();
        assert!(header.contains("session=abc123"));
        assert!(header.contains("Path=/account"));
        assert!(header.contains("Domain=example.com"));
        assert!(header.contains("Max-Age=3600"));
        assert!(header.contains("Secure"));
        assert!(header.contains("HttpOnly"));
        assert!(header.contains("SameSite=Lax"));

        let signed = cookie.sign("test-secret").expect("signed cookie");
        assert!(signed.signature().is_some());
        assert!(signed.verify("test-secret").is_ok());
        assert!(matches!(
            signed.verify("wrong-secret"),
            Err(CookieError::InvalidSignature)
        ));
        let mut tampered = signed.clone();
        tampered.value = "attacker".to_string();
        assert!(matches!(
            tampered.verify("test-secret"),
            Err(CookieError::InvalidSignature)
        ));

        let expired = Cookie::with_options(
            "session",
            "abc123",
            None,
            None,
            Some(0),
            true,
            true,
            CookieSameSite::Strict,
        )
        .expect("expired cookie options");
        let expired = expired.sign("test-secret").expect("expired signature");
        assert!(expired.is_expired());
        assert!(matches!(
            expired.verify("test-secret"),
            Err(CookieError::Expired)
        ));

        assert!(matches!(
            Cookie::with_options(
                "session",
                "abc123",
                None,
                None,
                None,
                false,
                false,
                CookieSameSite::None,
            ),
            Err(HttpTypeError::InvalidCookieSameSite)
        ));

        let mut headers = Headers::new();
        headers
            .insert("Set-Cookie", cookie.header_value())
            .expect("first set-cookie");
        headers
            .insert("set-cookie", signed.header_value())
            .expect("second set-cookie");
        assert_eq!(headers.len(), 2);
    }

    #[test]
    fn request_response_types_cover_crud_style_flow() {
        let create = Request::new(Method::Post, "/users")
            .expect("request")
            .with_header("Content-Type", "application/json")
            .expect("header")
            .with_header("Cookie", "SESSION=abc123; theme=dark")
            .expect("cookie header")
            .with_body(br#"{"name":"Ada"}"#.to_vec());
        assert_eq!(create.method, Method::Post);
        assert_eq!(create.path, "/users");
        assert_eq!(create.header("content-type"), Some("application/json"));
        assert_eq!(create.cookie("session").as_deref(), Some("abc123"));
        assert_eq!(create.body, br#"{"name":"Ada"}"#);

        let read = Request::new(Method::Get, "/users/7").expect("read request");
        let update = Request::new(Method::Put, "/users/7").expect("update request");
        let delete = Request::new(Method::Delete, "/users/7").expect("delete request");
        assert!(read.method.is_safe());
        assert!(update.method.allows_body());
        assert!(!delete.method.allows_body());

        let response = Response::new(Status::new(201).expect("created"))
            .with_header("Location", "/users/7")
            .expect("location")
            .with_body(br#"{"id":7}"#.to_vec());
        assert_eq!(response.status.reason(), "Created");
        assert_eq!(response.header("location"), Some("/users/7"));
        assert_eq!(response.body.len(), 8);
    }

    #[test]
    fn request_accepts_absolute_http_urls_for_outbound_client_calls() {
        let request = Request::new(Method::Get, "http://127.0.0.1:8080/health").expect("URL");
        assert_eq!(request.path, "http://127.0.0.1:8080/health");
        let https = Request::new(Method::Get, "https://example.test/path").expect("HTTPS URL");
        assert_eq!(https.path, "https://example.test/path");
        assert!(Request::new(Method::Get, "ftp://example.test/path").is_err());
        assert!(Request::new(Method::Get, "http://example.test/line\nbreak").is_err());
    }

    #[test]
    fn request_parser_streams_headers_then_body() {
        let mut parser = Http1Parser::request();
        parser.push(b"POST /submit HTTP/1.1\r\nHost: example.com\r\nContent-Length: 11\r\n");
        assert!(parser.parse_next_request().unwrap().is_none());
        parser.push(b"\r\nhello ");
        assert!(parser.parse_next_request().unwrap().is_none());
        parser.push(b"world");

        let request = parser
            .parse_next_request()
            .unwrap()
            .expect("complete request");
        assert_eq!(request.method, "POST");
        assert_eq!(request.target, "/submit");
        assert_eq!(request.version, HttpVersion::HTTP_11);
        assert_eq!(request.body.bytes(), b"hello world");
        assert!(request.keep_alive);
        assert_eq!(parser.buffered_len(), 0);
    }

    #[test]
    fn parser_keeps_pipelined_request_bytes_for_next_message() {
        let mut parser = Http1Parser::request();
        parser.push(
            b"GET /one HTTP/1.1\r\nHost: example.com\r\n\r\nGET /two HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n",
        );

        let first = parser.parse_next_request().unwrap().expect("first request");
        assert_eq!(first.target, "/one");
        assert!(first.keep_alive);

        let second = parser
            .parse_next_request()
            .unwrap()
            .expect("second request");
        assert_eq!(second.target, "/two");
        assert!(!second.keep_alive);
        assert_eq!(parser.buffered_len(), 0);
    }

    #[test]
    fn response_parser_accepts_rfc_7230_style_sample() {
        let response = parse_response(
            b"HTTP/1.1 200 OK\r\nDate: Sun, 06 Nov 1994 08:49:37 GMT\r\nContent-Length: 5\r\nConnection: keep-alive\r\n\r\nhello",
        )
        .expect("valid response");

        assert_eq!(response.version, HttpVersion::HTTP_11);
        assert_eq!(response.status_code, 200);
        assert_eq!(response.reason, "OK");
        assert_eq!(response.body.bytes(), b"hello");
        assert!(response.keep_alive);
    }

    #[test]
    fn chunked_request_round_trips_with_extensions_and_trailers() {
        let raw = b"POST /upload HTTP/1.1\r\nHost: example.com\r\nTransfer-Encoding: chunked\r\n\r\n4;sig=a\r\nWiki\r\n5\r\npedia\r\n0\r\nDigest: sha-256=abc\r\n\r\n";
        let request = parse_request(raw).expect("chunked request");

        assert!(request.body.chunked);
        assert_eq!(request.body.chunks.len(), 2);
        assert_eq!(request.body.chunks[0].data, b"Wiki");
        assert_eq!(request.body.chunks[0].extension.as_deref(), Some("sig=a"));
        assert_eq!(request.body.bytes(), b"Wikipedia");
        assert_eq!(
            request.body.trailers,
            vec![Header {
                name: "Digest".to_string(),
                value: "sha-256=abc".to_string()
            }]
        );
        assert_eq!(serialize_request(&request), raw);
    }

    #[test]
    fn chunked_response_round_trips_without_trailers() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\nMozilla\r\n9\r\nDeveloper\r\n0\r\n\r\n";
        let response = parse_response(raw).expect("chunked response");

        assert!(response.body.chunked);
        assert_eq!(response.body.bytes(), b"MozillaDeveloper");
        assert!(response.body.trailers.is_empty());
        assert_eq!(serialize_response(&response), raw);
    }

    #[test]
    fn http_10_keep_alive_requires_connection_header() {
        let closed = parse_request(b"GET / HTTP/1.0\r\nHost: example.com\r\n\r\n")
            .expect("HTTP/1.0 request");
        assert!(!closed.keep_alive);

        let kept =
            parse_request(b"GET / HTTP/1.0\r\nHost: example.com\r\nConnection: keep-alive\r\n\r\n")
                .expect("HTTP/1.0 keep-alive request");
        assert!(kept.keep_alive);
    }

    #[test]
    fn malformed_header_reports_typed_position() {
        let err = parse_request(b"GET / HTTP/1.1\r\nBad Header: value\r\n\r\n")
            .expect_err("invalid header name");
        assert_eq!(err.kind, ParseErrorKind::InvalidHeader);
        assert_eq!(err.position, "GET / HTTP/1.1\r\n".len());
    }

    #[test]
    fn malformed_chunk_size_reports_typed_position() {
        let err = parse_request(
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nZ\r\nbad\r\n0\r\n\r\n",
        )
        .expect_err("invalid chunk size");
        assert_eq!(err.kind, ParseErrorKind::InvalidChunkSize);
        assert_eq!(
            err.position,
            "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".len()
        );
    }

    #[test]
    fn rejects_conflicting_content_length() {
        let err =
            parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nx")
                .expect_err("conflicting content length");
        assert_eq!(err.kind, ParseErrorKind::BodyLengthMismatch);
    }

    #[test]
    fn rejects_unsupported_transfer_encoding() {
        let err = parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n")
            .expect_err("unsupported transfer encoding");
        assert_eq!(err.kind, ParseErrorKind::UnsupportedTransferEncoding);
    }
}
