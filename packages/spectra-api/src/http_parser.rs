pub fn parse_request(bytes: &[u8]) -> Result<ParsedRequest, ParseError> {
    let mut parser = Http1Parser::request();
    parser.push(bytes);
    match parser.parse_next_request()? {
        Some(request) if parser.buffered_len() == 0 => Ok(request),
        Some(_) => Err(ParseError::new(
            ParseErrorKind::InvalidStartLine,
            bytes.len() - parser.buffered_len(),
            "input contains trailing bytes after one HTTP request",
        )),
        None => Err(ParseError::new(
            ParseErrorKind::Incomplete,
            bytes.len(),
            "incomplete HTTP request",
        )),
    }
}

pub fn parse_response(bytes: &[u8]) -> Result<ParsedResponse, ParseError> {
    let mut parser = Http1Parser::response();
    parser.push(bytes);
    match parser.parse_next_response()? {
        Some(response) if parser.buffered_len() == 0 => Ok(response),
        Some(_) => Err(ParseError::new(
            ParseErrorKind::InvalidStartLine,
            bytes.len() - parser.buffered_len(),
            "input contains trailing bytes after one HTTP response",
        )),
        None => Err(ParseError::new(
            ParseErrorKind::Incomplete,
            bytes.len(),
            "incomplete HTTP response",
        )),
    }
}

pub fn serialize_request(request: &ParsedRequest) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(request.method.as_bytes());
    out.push(b' ');
    out.extend_from_slice(request.target.as_bytes());
    out.push(b' ');
    out.extend_from_slice(request.version.to_string().as_bytes());
    out.extend_from_slice(b"\r\n");
    write_headers(&mut out, &request.headers);
    out.extend_from_slice(b"\r\n");
    write_body(&mut out, &request.body);
    out
}

pub fn serialize_response(response: &ParsedResponse) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(response.version.to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(response.status_code.to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(response.reason.as_bytes());
    out.extend_from_slice(b"\r\n");
    write_headers(&mut out, &response.headers);
    out.extend_from_slice(b"\r\n");
    write_body(&mut out, &response.body);
    out
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_head(
    bytes: &[u8],
    absolute_base: usize,
    mode: ParserMode,
) -> Result<(StartLine, Vec<Header>), ParseError> {
    let mut line_start = 0usize;
    let Some(first_line_end) = find_crlf(bytes, line_start) else {
        return Err(ParseError::new(
            ParseErrorKind::InvalidStartLine,
            absolute_base,
            "HTTP start line is missing CRLF",
        ));
    };
    let start_line = parse_start_line(&bytes[line_start..first_line_end], absolute_base, mode)?;
    line_start = first_line_end + 2;

    let mut headers = Vec::new();
    while line_start < bytes.len() {
        let line_end = find_crlf(bytes, line_start).unwrap_or(bytes.len());
        let line = &bytes[line_start..line_end];
        if line.is_empty() {
            break;
        }
        if line[0] == b' ' || line[0] == b'\t' {
            return Err(ParseError::new(
                ParseErrorKind::ObsoleteLineFolding,
                absolute_base + line_start,
                "obsolete folded HTTP headers are rejected",
            ));
        }
        headers.push(parse_header(line, absolute_base + line_start)?);
        line_start = if line_end == bytes.len() {
            bytes.len()
        } else {
            line_end + 2
        };
    }

    Ok((start_line, headers))
}

fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|offset| start + offset)
}

fn parse_start_line(
    line: &[u8],
    absolute_position: usize,
    mode: ParserMode,
) -> Result<StartLine, ParseError> {
    let line_text = std::str::from_utf8(line).map_err(|_| {
        ParseError::new(
            ParseErrorKind::InvalidStartLine,
            absolute_position,
            "HTTP start line must be valid ASCII/UTF-8",
        )
    })?;
    match mode {
        ParserMode::Request => parse_request_line(line_text, absolute_position),
        ParserMode::Response => parse_status_line(line_text, absolute_position),
    }
}

fn parse_request_line(line: &str, absolute_position: usize) -> Result<StartLine, ParseError> {
    let mut parts = line.split(' ');
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if parts.next().is_some() || method.is_empty() || target.is_empty() || version.is_empty() {
        return Err(ParseError::new(
            ParseErrorKind::InvalidStartLine,
            absolute_position,
            "HTTP request line must be METHOD target HTTP-version",
        ));
    }
    if !is_token(method) {
        return Err(ParseError::new(
            ParseErrorKind::InvalidMethod,
            absolute_position,
            "HTTP method contains invalid token characters",
        ));
    }
    if target.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err(ParseError::new(
            ParseErrorKind::InvalidTarget,
            absolute_position + method.len() + 1,
            "HTTP request target contains invalid whitespace or control bytes",
        ));
    }
    let version = parse_version(version, absolute_position + method.len() + target.len() + 2)?;
    Ok(StartLine {
        version,
        kind: StartLineKind::Request {
            method: method.to_string(),
            target: target.to_string(),
        },
    })
}

fn parse_status_line(line: &str, absolute_position: usize) -> Result<StartLine, ParseError> {
    let mut parts = line.splitn(3, ' ');
    let version_text = parts.next().unwrap_or_default();
    let status_text = parts.next().unwrap_or_default();
    let reason = parts.next().unwrap_or_default();
    if version_text.is_empty() || status_text.is_empty() {
        return Err(ParseError::new(
            ParseErrorKind::InvalidStartLine,
            absolute_position,
            "HTTP status line must be HTTP-version status-code reason",
        ));
    }
    let version = parse_version(version_text, absolute_position)?;
    if status_text.len() != 3 || !status_text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::new(
            ParseErrorKind::InvalidStatus,
            absolute_position + version_text.len() + 1,
            "HTTP status code must contain exactly three digits",
        ));
    }
    let status_code: u16 = status_text.parse().map_err(|_| {
        ParseError::new(
            ParseErrorKind::InvalidStatus,
            absolute_position + version_text.len() + 1,
            "HTTP status code is outside the supported range",
        )
    })?;
    if !(100..=999).contains(&status_code) {
        return Err(ParseError::new(
            ParseErrorKind::InvalidStatus,
            absolute_position + version_text.len() + 1,
            "HTTP status code is outside the supported range",
        ));
    }
    if !is_reason_phrase(reason) {
        return Err(ParseError::new(
            ParseErrorKind::InvalidStatus,
            absolute_position + version_text.len() + status_text.len() + 2,
            "HTTP reason phrase contains invalid control bytes",
        ));
    }
    Ok(StartLine {
        version,
        kind: StartLineKind::Response {
            status_code,
            reason: reason.to_string(),
        },
    })
}

fn parse_version(text: &str, position: usize) -> Result<HttpVersion, ParseError> {
    let Some(rest) = text.strip_prefix("HTTP/") else {
        return Err(ParseError::new(
            ParseErrorKind::InvalidVersion,
            position,
            "HTTP version must start with HTTP/",
        ));
    };
    let Some((major, minor)) = rest.split_once('.') else {
        return Err(ParseError::new(
            ParseErrorKind::InvalidVersion,
            position,
            "HTTP version must contain major and minor numbers",
        ));
    };
    if major.is_empty()
        || minor.is_empty()
        || !major.bytes().all(|b| b.is_ascii_digit())
        || !minor.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(ParseError::new(
            ParseErrorKind::InvalidVersion,
            position,
            "HTTP version numbers must be decimal digits",
        ));
    }
    let major: u8 = major.parse().map_err(|_| {
        ParseError::new(
            ParseErrorKind::InvalidVersion,
            position,
            "HTTP major version is too large",
        )
    })?;
    let minor: u8 = minor.parse().map_err(|_| {
        ParseError::new(
            ParseErrorKind::InvalidVersion,
            position,
            "HTTP minor version is too large",
        )
    })?;
    Ok(HttpVersion { major, minor })
}

fn parse_header(line: &[u8], absolute_position: usize) -> Result<Header, ParseError> {
    let Some(colon) = line.iter().position(|byte| *byte == b':') else {
        return Err(ParseError::new(
            ParseErrorKind::InvalidHeader,
            absolute_position,
            "HTTP header line is missing ':'",
        ));
    };
    let name = bytes_to_ascii(&line[..colon], absolute_position)?;
    if !is_valid_header_name(&name) {
        return Err(ParseError::new(
            ParseErrorKind::InvalidHeader,
            absolute_position,
            "HTTP header field-name is invalid",
        ));
    }
    let mut value_start = colon + 1;
    while value_start < line.len() && matches!(line[value_start], b' ' | b'\t') {
        value_start += 1;
    }
    let mut value_end = line.len();
    while value_end > value_start && matches!(line[value_end - 1], b' ' | b'\t') {
        value_end -= 1;
    }
    let value = bytes_to_http_value(
        &line[value_start..value_end],
        absolute_position + value_start,
    )?;
    if !is_valid_header_value(&value) {
        return Err(ParseError::new(
            ParseErrorKind::InvalidHeader,
            absolute_position + value_start,
            "HTTP header field-value contains invalid bytes",
        ));
    }
    Ok(Header { name, value })
}

impl BodyMeta {
    fn from_headers(headers: &[Header], absolute_base: usize) -> Result<Self, ParseError> {
        let transfer_encoding = header_values(headers, "transfer-encoding");
        if !transfer_encoding.is_empty() {
            if !header_values(headers, "content-length").is_empty() {
                return Err(ParseError::new(
                    ParseErrorKind::ConflictingFraming,
                    absolute_base,
                    "Transfer-Encoding and Content-Length cannot coexist",
                ));
            }
            let codings = transfer_encoding
                .iter()
                .flat_map(|value| value.split(','))
                .map(|part| part.trim().to_ascii_lowercase())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            if codings.last().map(|coding| coding.as_str()) != Some("chunked") {
                return Err(ParseError::new(
                    ParseErrorKind::UnsupportedTransferEncoding,
                    absolute_base,
                    "only chunked transfer-coding is supported for HTTP/1.1 messages",
                ));
            }
            if codings.iter().any(|coding| coding != "chunked")
                || codings[..codings.len().saturating_sub(1)]
                    .iter()
                    .any(|coding| coding == "chunked")
            {
                return Err(ParseError::new(
                    ParseErrorKind::UnsupportedTransferEncoding,
                    absolute_base,
                    "only a single final chunked transfer-coding is supported",
                ));
            }
            return Ok(Self::Chunked);
        }

        let lengths = header_values(headers, "content-length");
        if lengths.is_empty() {
            return Ok(Self::Empty);
        }
        let mut parsed = None;
        for value in lengths {
            let length = value.parse::<usize>().map_err(|_| {
                ParseError::new(
                    ParseErrorKind::BodyLengthMismatch,
                    absolute_base,
                    "Content-Length must be a non-negative decimal integer",
                )
            })?;
            if let Some(existing) = parsed {
                if existing != length {
                    return Err(ParseError::new(
                        ParseErrorKind::BodyLengthMismatch,
                        absolute_base,
                        "conflicting Content-Length values are rejected",
                    ));
                }
            }
            parsed = Some(length);
        }
        Ok(Self::ContentLength(parsed.unwrap_or(0)))
    }
}

fn header_values<'a>(headers: &'a [Header], name: &str) -> Vec<&'a str> {
    headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str())
        .collect()
}

fn parse_body(
    buffer: &[u8],
    body_start: usize,
    absolute_base: usize,
    config: &ParserConfig,
    meta: &BodyMeta,
) -> Result<Option<(HttpBody, usize)>, ParseError> {
    match meta {
        BodyMeta::Empty => Ok(Some((HttpBody::empty(), 0))),
        BodyMeta::ContentLength(length) => {
            if *length > config.max_body_bytes {
                return Err(ParseError::new(
                    ParseErrorKind::BodyTooLarge,
                    absolute_base + body_start,
                    "HTTP body exceeds configured limit",
                ));
            }
            let available = buffer.len().saturating_sub(body_start);
            if available < *length {
                return Ok(None);
            }
            let bytes = buffer[body_start..body_start + length].to_vec();
            Ok(Some((HttpBody::from_bytes(bytes), *length)))
        }
        BodyMeta::Chunked => parse_chunked_body(buffer, body_start, absolute_base, config),
    }
}

fn parse_chunked_body(
    buffer: &[u8],
    body_start: usize,
    absolute_base: usize,
    config: &ParserConfig,
) -> Result<Option<(HttpBody, usize)>, ParseError> {
    let mut cursor = body_start;
    let mut body = HttpBody {
        chunks: Vec::new(),
        trailers: Vec::new(),
        chunked: true,
    };
    let mut total_data = 0usize;

    loop {
        let Some(size_line_end) = find_crlf(buffer, cursor) else {
            return Ok(None);
        };
        let size_line = &buffer[cursor..size_line_end];
        let (size, extension) = parse_chunk_size_line(size_line, absolute_base + cursor)?;
        cursor = size_line_end + 2;
        if size > config.max_chunk_bytes {
            return Err(ParseError::new(
                ParseErrorKind::BodyTooLarge,
                absolute_base + cursor,
                "HTTP chunk exceeds configured per-chunk limit",
            ));
        }
        total_data = total_data.saturating_add(size);
        if total_data > config.max_body_bytes {
            return Err(ParseError::new(
                ParseErrorKind::BodyTooLarge,
                absolute_base + cursor,
                "HTTP chunked body exceeds configured body limit",
            ));
        }
        if size == 0 {
            let Some((trailers, consumed_trailers)) =
                parse_trailer_section(buffer, cursor, absolute_base)?
            else {
                return Ok(None);
            };
            body.trailers = trailers;
            let consumed = cursor + consumed_trailers - body_start;
            return Ok(Some((body, consumed)));
        }
        if buffer.len() < cursor + size + 2 {
            return Ok(None);
        }
        if &buffer[cursor + size..cursor + size + 2] != b"\r\n" {
            return Err(ParseError::new(
                ParseErrorKind::InvalidChunkTerminator,
                absolute_base + cursor + size,
                "HTTP chunk data must be followed by CRLF",
            ));
        }
        body.chunks.push(BodyChunk {
            data: buffer[cursor..cursor + size].to_vec(),
            extension,
        });
        cursor += size + 2;
    }
}

fn parse_trailer_section(
    buffer: &[u8],
    cursor: usize,
    absolute_base: usize,
) -> Result<Option<(Vec<Header>, usize)>, ParseError> {
    let mut headers = Vec::new();
    let mut line_start = cursor;
    loop {
        let Some(line_end) = find_crlf(buffer, line_start) else {
            return Ok(None);
        };
        let line = &buffer[line_start..line_end];
        if line.is_empty() {
            return Ok(Some((headers, line_end + 2 - cursor)));
        }
        if line[0] == b' ' || line[0] == b'\t' {
            return Err(ParseError::new(
                ParseErrorKind::ObsoleteLineFolding,
                absolute_base + line_start,
                "obsolete folded HTTP trailers are rejected",
            ));
        }
        headers.push(parse_header(line, absolute_base + line_start)?);
        line_start = line_end + 2;
    }
}

fn parse_chunk_size_line(
    line: &[u8],
    absolute_position: usize,
) -> Result<(usize, Option<String>), ParseError> {
    let text = std::str::from_utf8(line).map_err(|_| {
        ParseError::new(
            ParseErrorKind::InvalidChunkSize,
            absolute_position,
            "HTTP chunk-size line must be valid ASCII/UTF-8",
        )
    })?;
    let (size_text, extension) = match text.split_once(';') {
        Some((size, ext)) => (size.trim(), Some(ext.trim().to_string())),
        None => (text.trim(), None),
    };
    if size_text.is_empty() || !size_text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ParseError::new(
            ParseErrorKind::InvalidChunkSize,
            absolute_position,
            "HTTP chunk-size must be hexadecimal",
        ));
    }
    let size = usize::from_str_radix(size_text, 16).map_err(|_| {
        ParseError::new(
            ParseErrorKind::InvalidChunkSize,
            absolute_position,
            "HTTP chunk-size is too large",
        )
    })?;
    Ok((size, extension.filter(|value| !value.is_empty())))
}

fn determine_keep_alive(version: &HttpVersion, headers: &[Header]) -> bool {
    let connection_tokens = header_values(headers, "connection")
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(|token| token.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if connection_tokens.iter().any(|token| token == "close") {
        return false;
    }
    if connection_tokens.iter().any(|token| token == "keep-alive") {
        return true;
    }
    version.major > 1 || (version.major == 1 && version.minor >= 1)
}

fn bytes_to_ascii(bytes: &[u8], absolute_position: usize) -> Result<String, ParseError> {
    if bytes.iter().any(|byte| !byte.is_ascii()) {
        return Err(ParseError::new(
            ParseErrorKind::InvalidHeader,
            absolute_position,
            "HTTP header field-name must be ASCII",
        ));
    }
    std::str::from_utf8(bytes)
        .map(|value| value.to_string())
        .map_err(|_| {
            ParseError::new(
                ParseErrorKind::InvalidHeader,
                absolute_position,
                "HTTP header field-name is not valid text",
            )
        })
}

fn bytes_to_http_value(bytes: &[u8], absolute_position: usize) -> Result<String, ParseError> {
    let mut out = String::new();
    for (idx, byte) in bytes.iter().copied().enumerate() {
        if byte == b'\t' || byte == b' ' || (0x21..=0x7e).contains(&byte) || byte >= 0x80 {
            out.push(char::from(byte));
        } else {
            return Err(ParseError::new(
                ParseErrorKind::InvalidHeader,
                absolute_position + idx,
                "HTTP header field-value contains a disallowed control byte",
            ));
        }
    }
    Ok(out)
}

fn is_token(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(is_tchar)
}

fn is_tchar(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn is_reason_phrase(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte == b'\t' || byte == b' ' || (0x21..=0x7e).contains(&byte) || byte >= 0x80)
}

fn write_headers(out: &mut Vec<u8>, headers: &[Header]) {
    for header in headers {
        out.extend_from_slice(header.name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(header.value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
}

fn write_body(out: &mut Vec<u8>, body: &HttpBody) {
    if body.chunked {
        for chunk in &body.chunks {
            out.extend_from_slice(format!("{:X}", chunk.data.len()).as_bytes());
            if let Some(extension) = &chunk.extension {
                out.push(b';');
                out.extend_from_slice(extension.as_bytes());
            }
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(&chunk.data);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"0\r\n");
        write_headers(out, &body.trailers);
        out.extend_from_slice(b"\r\n");
    } else {
        for chunk in &body.chunks {
            out.extend_from_slice(&chunk.data);
        }
    }
}

