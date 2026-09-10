//! API conformance suite for the Phase 22 HTTP/1.1 baseline.
//!
//! The suite is intentionally runnable outside `cargo test` so release tooling can
//! emit a machine-readable report from the same checks that guard the crate.

use crate::http::{
    parse_request, parse_response, serialize_request, serialize_response, Header, Http1Parser,
    HttpVersion, Method, ParseErrorKind, Status,
};
use crate::json::{
    encode_json, json_kind_of, parse_json, JsonNumber, JsonObject, JsonValue, JSON_KIND_ARRAY,
    JSON_KIND_BOOL, JSON_KIND_NULL, JSON_KIND_NUMBER, JSON_KIND_OBJECT, JSON_KIND_STRING,
};
use crate::routing::{RouteMethod, Router};
use serde_json::json;
use std::panic::{catch_unwind, AssertUnwindSafe};

pub const SUITE_ID: &str = "spectra.api.conformance.v0";
pub const SUITE_VERSION: &str = "0.1.0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceCaseResult {
    pub id: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceReport {
    pub suite: &'static str,
    pub version: &'static str,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub cases: Vec<ConformanceCaseResult>,
}

impl ConformanceReport {
    pub fn is_success(&self) -> bool {
        self.failed == 0 && self.total == self.passed
    }

    pub fn to_json_string(&self) -> String {
        let cases: Vec<_> = self
            .cases
            .iter()
            .map(|case| {
                json!({
                    "id": case.id,
                    "category": case.category,
                    "description": case.description,
                    "passed": case.passed,
                    "detail": case.detail,
                })
            })
            .collect();
        serde_json::to_string_pretty(&json!({
            "suite": self.suite,
            "version": self.version,
            "total": self.total,
            "passed": self.passed,
            "failed": self.failed,
            "cases": cases,
        }))
        .expect("conformance report is valid JSON")
    }
}

struct Case {
    id: &'static str,
    category: &'static str,
    description: &'static str,
    run: fn() -> Result<(), String>,
}

pub fn conformance_v0_cases() -> Vec<(&'static str, &'static str, &'static str)> {
    cases()
        .iter()
        .map(|case| (case.id, case.category, case.description))
        .collect()
}

pub fn run_v0_suite() -> ConformanceReport {
    let mut results = Vec::new();
    for case in cases() {
        let outcome = catch_unwind(AssertUnwindSafe(|| (case.run)()));
        let (passed, detail) = match outcome {
            Ok(Ok(())) => (true, "ok".to_string()),
            Ok(Err(message)) => (false, message),
            Err(_) => (false, "case panicked".to_string()),
        };
        results.push(ConformanceCaseResult {
            id: case.id,
            category: case.category,
            description: case.description,
            passed,
            detail,
        });
    }
    let total = results.len();
    let passed = results.iter().filter(|case| case.passed).count();
    ConformanceReport {
        suite: SUITE_ID,
        version: SUITE_VERSION,
        total,
        passed,
        failed: total - passed,
        cases: results,
    }
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            id: "http1.request.get_minimal",
            category: "http1",
            description: "parse a minimal HTTP/1.1 GET request with Host",
            run: http1_request_get_minimal,
        },
        Case {
            id: "http1.request.content_length_body",
            category: "http1",
            description: "parse POST Content-Length bodies exactly",
            run: http1_request_content_length_body,
        },
        Case {
            id: "http1.request.pipelined_streaming",
            category: "http1",
            description: "stream and retain pipelined request bytes",
            run: http1_request_pipelined_streaming,
        },
        Case {
            id: "http1.request.chunked_round_trip",
            category: "http1",
            description: "round-trip chunked requests with extensions and trailers",
            run: http1_request_chunked_round_trip,
        },
        Case {
            id: "http1.response.rfc7230_sample",
            category: "http1",
            description: "parse a representative RFC 7230 response sample",
            run: http1_response_rfc7230_sample,
        },
        Case {
            id: "http1.response.chunked_round_trip",
            category: "http1",
            description: "round-trip chunked HTTP/1.1 responses",
            run: http1_response_chunked_round_trip,
        },
        Case {
            id: "http1.connection.http10_keep_alive",
            category: "http1",
            description: "honor HTTP/1.0 keep-alive only when requested",
            run: http1_connection_http10_keep_alive,
        },
        Case {
            id: "http1.error.malformed_header_position",
            category: "http1",
            description: "report malformed headers with typed byte positions",
            run: http1_error_malformed_header_position,
        },
        Case {
            id: "http1.error.invalid_chunk_size",
            category: "http1",
            description: "report invalid chunk sizes as typed parser errors",
            run: http1_error_invalid_chunk_size,
        },
        Case {
            id: "http1.error.unsupported_transfer_encoding",
            category: "http1",
            description: "reject unsupported transfer encodings",
            run: http1_error_unsupported_transfer_encoding,
        },
        Case {
            id: "http1.error.conflicting_content_length",
            category: "http1",
            description: "reject conflicting Content-Length headers",
            run: http1_error_conflicting_content_length,
        },
        Case {
            id: "http1.types.method_status_matrix",
            category: "http1",
            description: "validate documented Method and Status semantics",
            run: http1_types_method_status_matrix,
        },
        Case {
            id: "http1.types.header_validation",
            category: "http1",
            description: "validate header name and value rules",
            run: http1_types_header_validation,
        },
        Case {
            id: "json.kind.matrix",
            category: "json",
            description: "classify all public JSON value kinds",
            run: json_kind_matrix,
        },
        Case {
            id: "json.round_trip.nested_object",
            category: "json",
            description: "round-trip nested objects, arrays, strings, numbers, bools, and null",
            run: json_round_trip_nested_object,
        },
        Case {
            id: "json.escape.unicode",
            category: "json",
            description: "decode common escapes and unicode sequences",
            run: json_escape_unicode,
        },
        Case {
            id: "json.error.invalid_syntax_offset",
            category: "json",
            description: "report invalid JSON syntax with line, column, and byte offset",
            run: json_error_invalid_syntax_offset,
        },
        Case {
            id: "json.encode.non_finite_rejected",
            category: "json",
            description: "reject NaN and infinity before JSON encoding",
            run: json_encode_non_finite_rejected,
        },
        Case {
            id: "json.number.exponent_round_trip",
            category: "json",
            description: "preserve supported exponent number representations",
            run: json_number_exponent_round_trip,
        },
        Case {
            id: "routing.literal.match",
            category: "routing",
            description: "match literal routes",
            run: routing_literal_match,
        },
        Case {
            id: "routing.param.extract",
            category: "routing",
            description: "extract named path parameters",
            run: routing_param_extract,
        },
        Case {
            id: "routing.wildcard.extract",
            category: "routing",
            description: "extract wildcard path tails",
            run: routing_wildcard_extract,
        },
        Case {
            id: "routing.regex.constraint",
            category: "routing",
            description: "apply numeric route constraints",
            run: routing_regex_constraint,
        },
        Case {
            id: "routing.method.separation",
            category: "routing",
            description: "keep routes with the same path but different methods separate",
            run: routing_method_separation,
        },
        Case {
            id: "routing.conflict.overlap",
            category: "routing",
            description: "reject overlapping literal and parameter routes",
            run: routing_conflict_overlap,
        },
        Case {
            id: "routing.invalid.path",
            category: "routing",
            description: "reject invalid route match paths",
            run: routing_invalid_path,
        },
    ]
}

fn ensure(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn http1_request_get_minimal() -> Result<(), String> {
    let request = parse_request(b"GET /hello HTTP/1.1\r\nHost: example.com\r\n\r\n")
        .map_err(|error| error.to_string())?;
    ensure(request.method == "GET", "method mismatch")?;
    ensure(request.target == "/hello", "target mismatch")?;
    ensure(request.version == HttpVersion::HTTP_11, "version mismatch")?;
    ensure(request.keep_alive, "HTTP/1.1 request should keep alive")?;
    ensure(
        request.headers.iter().any(|header| {
            header.name.eq_ignore_ascii_case("host") && header.value == "example.com"
        }),
        "Host header missing",
    )
}

fn http1_request_content_length_body() -> Result<(), String> {
    let request = parse_request(
        b"POST /submit HTTP/1.1\r\nHost: example.com\r\nContent-Length: 11\r\n\r\nhello world",
    )
    .map_err(|error| error.to_string())?;
    ensure(request.method == "POST", "method mismatch")?;
    ensure(request.body.bytes() == b"hello world", "body mismatch")
}

fn http1_request_pipelined_streaming() -> Result<(), String> {
    let mut parser = Http1Parser::request();
    parser.push(
        b"GET /one HTTP/1.1\r\nHost: example.com\r\n\r\nGET /two HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n",
    );
    let first = parser
        .parse_next_request()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "missing first request".to_string())?;
    let second = parser
        .parse_next_request()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "missing second request".to_string())?;
    ensure(first.target == "/one", "first target mismatch")?;
    ensure(second.target == "/two", "second target mismatch")?;
    ensure(!second.keep_alive, "Connection: close not honored")?;
    ensure(
        parser.buffered_len() == 0,
        "pipelined buffer not fully consumed",
    )
}

fn http1_request_chunked_round_trip() -> Result<(), String> {
    let raw = b"POST /upload HTTP/1.1\r\nHost: example.com\r\nTransfer-Encoding: chunked\r\n\r\n4;sig=a\r\nWiki\r\n5\r\npedia\r\n0\r\nDigest: sha-256=abc\r\n\r\n";
    let request = parse_request(raw).map_err(|error| error.to_string())?;
    ensure(request.body.chunked, "request body not marked chunked")?;
    ensure(
        request.body.bytes() == b"Wikipedia",
        "chunked bytes mismatch",
    )?;
    ensure(
        request.body.trailers
            == vec![Header {
                name: "Digest".to_string(),
                value: "sha-256=abc".to_string(),
            }],
        "trailer mismatch",
    )?;
    ensure(
        serialize_request(&request) == raw,
        "chunked request did not round-trip",
    )
}

fn http1_response_rfc7230_sample() -> Result<(), String> {
    let response = parse_response(
        b"HTTP/1.1 200 OK\r\nDate: Sun, 06 Nov 1994 08:49:37 GMT\r\nContent-Length: 5\r\nConnection: keep-alive\r\n\r\nhello",
    )
    .map_err(|error| error.to_string())?;
    ensure(response.status_code == 200, "status mismatch")?;
    ensure(response.reason == "OK", "reason mismatch")?;
    ensure(response.body.bytes() == b"hello", "response body mismatch")?;
    ensure(response.keep_alive, "response keep-alive not detected")
}

fn http1_response_chunked_round_trip() -> Result<(), String> {
    let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\nMozilla\r\n9\r\nDeveloper\r\n0\r\n\r\n";
    let response = parse_response(raw).map_err(|error| error.to_string())?;
    ensure(response.body.chunked, "response body not marked chunked")?;
    ensure(
        response.body.bytes() == b"MozillaDeveloper",
        "chunked response bytes mismatch",
    )?;
    ensure(
        serialize_response(&response) == raw,
        "chunked response did not round-trip",
    )
}

fn http1_connection_http10_keep_alive() -> Result<(), String> {
    let closed = parse_request(b"GET / HTTP/1.0\r\nHost: example.com\r\n\r\n")
        .map_err(|error| error.to_string())?;
    let kept =
        parse_request(b"GET / HTTP/1.0\r\nHost: example.com\r\nConnection: keep-alive\r\n\r\n")
            .map_err(|error| error.to_string())?;
    ensure(!closed.keep_alive, "HTTP/1.0 should close by default")?;
    ensure(kept.keep_alive, "HTTP/1.0 keep-alive header not honored")
}

fn http1_error_malformed_header_position() -> Result<(), String> {
    let err = parse_request(b"GET / HTTP/1.1\r\nBad Header: value\r\n\r\n")
        .expect_err("invalid header must fail");
    ensure(
        err.kind == ParseErrorKind::InvalidHeader,
        "wrong parse error kind",
    )?;
    ensure(
        err.position == "GET / HTTP/1.1\r\n".len(),
        "wrong error position",
    )
}

fn http1_error_invalid_chunk_size() -> Result<(), String> {
    let err = parse_request(
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nZ\r\nbad\r\n0\r\n\r\n",
    )
    .expect_err("invalid chunk size must fail");
    ensure(
        err.kind == ParseErrorKind::InvalidChunkSize,
        "wrong chunk error kind",
    )
}

fn http1_error_unsupported_transfer_encoding() -> Result<(), String> {
    let err = parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n")
        .expect_err("unsupported transfer encoding must fail");
    ensure(
        err.kind == ParseErrorKind::UnsupportedTransferEncoding,
        "wrong transfer-encoding error kind",
    )
}

fn http1_error_conflicting_content_length() -> Result<(), String> {
    let err = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nx")
        .expect_err("conflicting content length must fail");
    ensure(
        err.kind == ParseErrorKind::BodyLengthMismatch,
        "wrong content-length error kind",
    )
}

fn http1_types_method_status_matrix() -> Result<(), String> {
    let methods = [
        (Method::Get, "GET", true, false),
        (Method::Head, "HEAD", true, false),
        (Method::Post, "POST", false, true),
        (Method::Put, "PUT", false, true),
        (Method::Patch, "PATCH", false, true),
        (Method::Delete, "DELETE", false, false),
        (Method::Options, "OPTIONS", true, false),
    ];
    for (method, label, safe, body) in methods {
        ensure(
            method.as_str() == label,
            format!("method label mismatch for {label}"),
        )?;
        ensure(
            method.is_safe() == safe,
            format!("safe mismatch for {label}"),
        )?;
        ensure(
            method.allows_body() == body,
            format!("body allowance mismatch for {label}"),
        )?;
    }
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
        let status = Status::new(code).map_err(|error| error.to_string())?;
        ensure(
            status.reason() == reason,
            format!("reason mismatch for {code}"),
        )?;
        ensure(
            status.class() == class,
            format!("class mismatch for {code}"),
        )?;
        ensure(
            status.is_success() == success,
            format!("success mismatch for {code}"),
        )?;
    }
    ensure(Status::new(99).is_err(), "status 99 should be invalid")?;
    ensure(Status::new(600).is_err(), "status 600 should be invalid")
}

fn http1_types_header_validation() -> Result<(), String> {
    ensure(
        Header::new("Content-Type", "application/json").is_ok(),
        "valid header rejected",
    )?;
    ensure(
        Header::new("Bad Header", "value").is_err(),
        "bad header name accepted",
    )?;
    ensure(
        Header::new("X-Good", "bad\rvalue").is_err(),
        "bad header value accepted",
    )
}

fn json_kind_matrix() -> Result<(), String> {
    for (text, kind) in [
        ("null", JSON_KIND_NULL),
        ("true", JSON_KIND_BOOL),
        ("12.5", JSON_KIND_NUMBER),
        (r#""hello""#, JSON_KIND_STRING),
        ("[1,2,3]", JSON_KIND_ARRAY),
        (r#"{"ok":true}"#, JSON_KIND_OBJECT),
    ] {
        ensure(
            json_kind_of(text) == kind,
            format!("kind mismatch for {text}"),
        )?;
    }
    Ok(())
}

fn json_round_trip_nested_object() -> Result<(), String> {
    let mut child = JsonObject::new();
    child.insert(
        "items".to_string(),
        JsonValue::Array(vec![
            JsonValue::Number(JsonNumber::from_i64(1)),
            JsonValue::Bool(false),
            JsonValue::Null,
        ]),
    );
    let mut root = JsonObject::new();
    root.insert("name".to_string(), JsonValue::String("Ada".to_string()));
    root.insert("meta".to_string(), JsonValue::Object(child));
    let value = JsonValue::Object(root);
    let encoded = encode_json(&value).map_err(|error| error.to_string())?;
    let decoded = parse_json(&encoded).map_err(|error| error.to_string())?;
    ensure(decoded == value, "JSON round-trip mismatch")
}

fn json_escape_unicode() -> Result<(), String> {
    let decoded = parse_json(r#"{"text":"line\nquote\"slash\\tab\tunicode \u263A"}"#)
        .map_err(|error| error.to_string())?;
    let JsonValue::Object(map) = decoded else {
        return Err("expected object".to_string());
    };
    ensure(
        map.get("text")
            == Some(&JsonValue::String(
                "line\nquote\"slash\\tab\tunicode ☺".to_string(),
            )),
        "decoded escape text mismatch",
    )
}

fn json_error_invalid_syntax_offset() -> Result<(), String> {
    let err = parse_json("{\n  \"ok\": true,\n  bad\n}").expect_err("invalid JSON must fail");
    ensure(err.line == 3, "wrong JSON error line")?;
    ensure(
        err.offset >= "{\n  \"ok\": true,\n  ".len(),
        "wrong JSON error offset",
    )
}

fn json_encode_non_finite_rejected() -> Result<(), String> {
    ensure(
        JsonNumber::from_f64(f64::NAN).is_err(),
        "NaN should not be encodable as JSON",
    )?;
    ensure(
        JsonNumber::from_f64(f64::INFINITY).is_err(),
        "infinity should not be encodable as JSON",
    )
}

fn json_number_exponent_round_trip() -> Result<(), String> {
    let value = JsonValue::Number(JsonNumber::parse("1e-9").map_err(|error| error.to_string())?);
    let encoded = encode_json(&value).map_err(|error| error.to_string())?;
    let decoded = parse_json(&encoded).map_err(|error| error.to_string())?;
    ensure(decoded == value, "exponent number round-trip mismatch")
}

fn routing_literal_match() -> Result<(), String> {
    let mut router = Router::default();
    let route = router
        .add(RouteMethod::Get, "/users")
        .map_err(|error| error.to_string())?;
    let matched = router
        .match_path(RouteMethod::Get, "/users")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "literal route did not match".to_string())?;
    ensure(matched.route_id == route, "literal route id mismatch")
}

fn routing_param_extract() -> Result<(), String> {
    let mut router = Router::default();
    let route = router
        .add(RouteMethod::Get, "/users/{id}")
        .map_err(|error| error.to_string())?;
    let matched = router
        .match_path(RouteMethod::Get, "/users/42")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "param route did not match".to_string())?;
    ensure(matched.route_id == route, "param route id mismatch")?;
    ensure(
        matched.params.get("id").map(String::as_str) == Some("42"),
        "param value mismatch",
    )
}

fn routing_wildcard_extract() -> Result<(), String> {
    let mut router = Router::default();
    let route = router
        .add(RouteMethod::Get, "/files/*path")
        .map_err(|error| error.to_string())?;
    let matched = router
        .match_path(RouteMethod::Get, "/files/a/b/c.txt")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "wildcard route did not match".to_string())?;
    ensure(matched.route_id == route, "wildcard route id mismatch")?;
    ensure(
        matched.params.get("path").map(String::as_str) == Some("a/b/c.txt"),
        "wildcard value mismatch",
    )
}

fn routing_regex_constraint() -> Result<(), String> {
    let mut router = Router::default();
    router
        .add(RouteMethod::Get, r"/orders/{id:\d+}")
        .map_err(|error| error.to_string())?;
    ensure(
        router
            .match_path(RouteMethod::Get, "/orders/123")
            .map_err(|error| error.to_string())?
            .is_some(),
        "digits route did not match",
    )?;
    ensure(
        router
            .match_path(RouteMethod::Get, "/orders/nope")
            .map_err(|error| error.to_string())?
            .is_none(),
        "non-digits route matched numeric constraint",
    )
}

fn routing_method_separation() -> Result<(), String> {
    let mut router = Router::default();
    let get = router
        .add(RouteMethod::Get, "/todos")
        .map_err(|error| error.to_string())?;
    let post = router
        .add(RouteMethod::Post, "/todos")
        .map_err(|error| error.to_string())?;
    let matched_get = router
        .match_path(RouteMethod::Get, "/todos")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "GET route missing".to_string())?;
    let matched_post = router
        .match_path(RouteMethod::Post, "/todos")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "POST route missing".to_string())?;
    ensure(matched_get.route_id == get, "GET route id mismatch")?;
    ensure(matched_post.route_id == post, "POST route id mismatch")
}

fn routing_conflict_overlap() -> Result<(), String> {
    let mut router = Router::default();
    router
        .add(RouteMethod::Get, "/users/{id}")
        .map_err(|error| error.to_string())?;
    ensure(
        router.add(RouteMethod::Get, "/users/me").is_err(),
        "overlapping literal route accepted",
    )
}

fn routing_invalid_path() -> Result<(), String> {
    let router = Router::default();
    ensure(
        router
            .match_path(RouteMethod::Get, "no-leading-slash")
            .is_err(),
        "invalid path accepted",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_conformance_v0_suite_passes() {
        let report = run_v0_suite();
        assert!(
            report.is_success(),
            "conformance failures:\n{}",
            report.to_json_string()
        );
        assert!(report.total >= 25);
        assert!(report.cases.iter().any(|case| case.category == "http1"));
        assert!(report.cases.iter().any(|case| case.category == "json"));
        assert!(report.cases.iter().any(|case| case.category == "routing"));
    }
}
